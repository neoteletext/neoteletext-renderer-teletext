//! Each part's state as the sender keeps it, and the restatement that
//! brings a part in any state to it (MIDI-93 to MIDI-99): the server
//! tracks every event it writes into a [`Parts`] (MIDI-94) and writes each
//! part's restatement ([`Part::restate`]) into a block at least once in
//! every 50 (MIDI-96) and at tick 0 of every stored sound (MIDI-99).
//!
//! What is generic to MIDI 1.0 is kept here: bank select and program
//! change, every control change, registered and non-registered parameter
//! values, pitch bend, channel pressure, volume, modulation and the sustain
//! pedal. What only a family's document can say, which system exclusive
//! message is its inline patch and which is a parameter change and of which
//! parameter, the family's code tells [`Part::take`] through a
//! [`SysexKind`]. Every table is of a fixed size, so the state is bounded
//! by the parameter tables and the 16 channels (MIDI-94).

use super::common::{EXPRESSION_FULL, PEDAL_DOWN, VOLUME_FULL};
use super::{
    CHANNEL_MASK, CHANNELS, ChannelMessage, DATA_MAX, MOST_SYSTEM_EXCLUSIVE_BYTES, Message, NULL_PARAMETER,
    PITCH_BEND_CENTRE, controller, status,
};

/// The registered and non-registered parameter values a part keeps: more
/// than any family document's parameter table has (the Prophet-5's NRPN 0
/// to 94, `synth53-synth52-synth51.md`).
pub const MOST_PARAMETERS: usize = 128;
/// The system exclusive parameter changes a part keeps, one a parameter:
/// the DX7's voice parameters 0 to 155 and function parameters 64 to 77
/// (DX7 manual, page 31).
pub const MOST_SYSEX_PARAMETERS: usize = 192;
/// The longest system exclusive parameter change kept, after its `0xF0`:
/// the DX7's and the Juno-106's are 6 (7 bytes in all, DX7 manual page 30;
/// Juno-106 manual, PDF 35).
pub const MOST_SYSEX_PARAMETER_BYTES: usize = 16;
/// Controllers 0 to 127.
const CONTROLLERS: usize = 128;

/// What a system exclusive message is to the family whose sound it is in,
/// as the family's code says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysexKind {
    /// A patch sent inline (MIDI-60): restated in place of the program
    /// change.
    Patch,
    /// A parameter change of this parameter, the family's own number:
    /// restated as its latest message.
    Parameter(u16),
    /// Anything else: not restated.
    Other,
}

/// A registered or non-registered parameter's value, kept for restatement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Parameter {
    registered: bool,
    number: u16,
    /// 14 bits: data entry's coarse value is its high seven bits.
    value: u16,
}

/// A system exclusive parameter change, kept as its bytes after `0xF0`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SysexParameter {
    key: u16,
    length: u8,
    bytes: [u8; MOST_SYSEX_PARAMETER_BYTES],
}

/// One part's state: what its restatement carries (MIDI-93).
#[derive(Clone, Debug)]
pub struct Part {
    bank_coarse: Option<u8>,
    bank_fine: Option<u8>,
    program: Option<u8>,
    /// The last inline patch, if it came after the last program change.
    patch: Option<(u16, [u8; MOST_SYSTEM_EXCLUSIVE_BYTES])>,
    /// Every controller moved, by number, but those restated otherwise.
    controllers: [Option<u8>; CONTROLLERS],
    parameters: [Option<Parameter>; MOST_PARAMETERS],
    sysex_parameters: [Option<SysexParameter>; MOST_SYSEX_PARAMETERS],
    /// The parameter selected by controllers 101 and 100, or 99 and 98.
    registered_select: [u8; 2],
    non_registered_select: [u8; 2],
    /// Which kind was selected last: true for registered.
    registered_last: bool,
    bend: u16,
    channel_pressure: u8,
    volume: u8,
    modulation: u8,
    sustain: u8,
}

/// Whether a controller is restated otherwise than by its own value: bank
/// select with the program, data entry and the parameter numbers as
/// parameter values, volume, modulation and the pedal at the end, and the
/// channel mode messages, which hold no state.
const fn restated_otherwise(number: u8) -> bool {
    matches!(
        number,
        controller::BANK_SELECT
            | controller::BANK_SELECT_FINE
            | controller::DATA_ENTRY
            | controller::DATA_ENTRY_FINE
            | controller::DATA_INCREMENT
            | controller::DATA_DECREMENT
            | controller::NRPN_FINE
            | controller::NRPN_COARSE
            | controller::RPN_FINE
            | controller::RPN_COARSE
            | controller::VOLUME
            | controller::MODULATION
            | controller::SUSTAIN
    ) || number >= controller::ALL_SOUND_OFF
}

impl Default for Part {
    fn default() -> Self {
        Self::new()
    }
}

impl Part {
    /// A part as a fresh synthesizer holds it (MIDI-61, MIDI-93).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bank_coarse: None,
            bank_fine: None,
            program: None,
            patch: None,
            controllers: [None; CONTROLLERS],
            parameters: [None; MOST_PARAMETERS],
            sysex_parameters: [None; MOST_SYSEX_PARAMETERS],
            registered_select: [DATA_MAX, DATA_MAX],
            non_registered_select: [DATA_MAX, DATA_MAX],
            registered_last: true,
            bend: PITCH_BEND_CENTRE,
            channel_pressure: 0,
            volume: VOLUME_FULL,
            modulation: 0,
            sustain: 0,
        }
    }

    /// Takes one channel message of this part.
    fn channel_message(&mut self, message: ChannelMessage) {
        let ChannelMessage { data1, data2, .. } = message;
        match message.kind() {
            status::PROGRAM_CHANGE => {
                self.program = Some(data1);
                self.patch = None;
            }
            status::CHANNEL_PRESSURE => self.channel_pressure = data1,
            status::PITCH_BEND => self.bend = message.bend(),
            status::CONTROL_CHANGE => self.control(data1, data2),
            _ => {}
        }
    }

    fn control(&mut self, number: u8, value: u8) {
        match number {
            controller::BANK_SELECT => self.bank_coarse = Some(value),
            controller::BANK_SELECT_FINE => self.bank_fine = Some(value),
            controller::VOLUME => self.volume = value,
            controller::MODULATION => self.modulation = value,
            controller::SUSTAIN => self.sustain = value,
            controller::RPN_COARSE | controller::RPN_FINE => {
                if let Some(byte) = self.registered_select.get_mut(usize::from(number == controller::RPN_FINE)) {
                    *byte = value;
                }
                self.registered_last = true;
            }
            controller::NRPN_COARSE | controller::NRPN_FINE => {
                if let Some(byte) = self.non_registered_select.get_mut(usize::from(number == controller::NRPN_FINE)) {
                    *byte = value;
                }
                self.registered_last = false;
            }
            controller::DATA_ENTRY
            | controller::DATA_ENTRY_FINE
            | controller::DATA_INCREMENT
            | controller::DATA_DECREMENT => self.data_entry(number, value),
            controller::RESET_ALL_CONTROLLERS => {
                // RP-015's reset (MIDI-84): what it sets goes back to what
                // a fresh synthesizer holds, so it is not restated.
                self.modulation = 0;
                self.sustain = 0;
                self.bend = PITCH_BEND_CENTRE;
                self.channel_pressure = 0;
                self.registered_select = [DATA_MAX, DATA_MAX];
                self.non_registered_select = [DATA_MAX, DATA_MAX];
                for pedal in [controller::EXPRESSION, controller::PORTAMENTO, controller::SOSTENUTO, controller::SOFT] {
                    if let Some(place) = self.controllers.get_mut(usize::from(pedal)) {
                        *place = None;
                    }
                }
            }
            _ if restated_otherwise(number) => {}
            _ => {
                if let Some(place) = self.controllers.get_mut(usize::from(number)) {
                    *place = Some(value);
                }
            }
        }
    }

    /// A data entry, increment or decrement to the selected parameter,
    /// kept as its value (MIDI-75, MIDI-76).
    fn data_entry(&mut self, number: u8, value: u8) {
        let (registered, bytes) =
            if self.registered_last { (true, self.registered_select) } else { (false, self.non_registered_select) };
        let [coarse, fine] = bytes;
        let parameter = u16::from(coarse & DATA_MAX) << 7 | u16::from(fine & DATA_MAX);
        if parameter == NULL_PARAMETER {
            return;
        }
        let held = self
            .parameters
            .iter()
            .position(|kept| kept.is_some_and(|kept| kept.registered == registered && kept.number == parameter));
        let Some(index) = held.or_else(|| self.parameters.iter().position(Option::is_none)) else { return };
        let Some(place) = self.parameters.get_mut(index) else { return };
        let old = place.map_or(0, |kept| kept.value);
        let seven = u16::from(DATA_MAX);
        let value = u16::from(value & DATA_MAX);
        let new = match number {
            controller::DATA_ENTRY => value << 7,
            controller::DATA_ENTRY_FINE => (old & !seven) | value,
            controller::DATA_INCREMENT => old.saturating_add(1).min(super::PITCH_BEND_MAX),
            _ => old.saturating_sub(1),
        };
        *place = Some(Parameter { registered, number: parameter, value: new });
    }

    /// Takes a system exclusive message the family has classed.
    fn system_exclusive(&mut self, bytes: &[u8], kind: SysexKind) {
        match kind {
            SysexKind::Patch => {
                let mut kept = [0; MOST_SYSTEM_EXCLUSIVE_BYTES];
                for (to, from) in kept.iter_mut().zip(bytes) {
                    *to = *from;
                }
                self.patch =
                    u16::try_from(bytes.len().min(MOST_SYSTEM_EXCLUSIVE_BYTES)).ok().map(|length| (length, kept));
            }
            SysexKind::Parameter(key) => {
                let Ok(length) = u8::try_from(bytes.len()) else { return };
                if bytes.len() > MOST_SYSEX_PARAMETER_BYTES {
                    return;
                }
                let held = self.sysex_parameters.iter().position(|kept| kept.is_some_and(|kept| kept.key == key));
                let Some(index) = held.or_else(|| self.sysex_parameters.iter().position(Option::is_none)) else {
                    return;
                };
                let mut kept = [0; MOST_SYSEX_PARAMETER_BYTES];
                for (to, from) in kept.iter_mut().zip(bytes) {
                    *to = *from;
                }
                if let Some(place) = self.sysex_parameters.get_mut(index) {
                    *place = Some(SysexParameter { key, length, bytes: kept });
                }
            }
            SysexKind::Other => {}
        }
    }

    /// Gives the part's restatement, in MIDI-93's and MIDI-95's order, to
    /// `out`, on `channel`: its bank select and program change, or its
    /// inline patch; every controller moved, by number; every registered,
    /// then non-registered parameter value, by number, each as its whole
    /// sequence (MIDI-37); every system exclusive parameter change, by its
    /// key; then pitch bend, channel pressure, volume, modulation and the
    /// pedal where they are not as a fresh synthesizer holds them. The
    /// server writes it whole in one block at one tick (MIDI-98).
    pub fn restate(&self, channel: u8, out: &mut dyn FnMut(Message<'_>)) {
        let channel = channel & CHANNEL_MASK;
        let mut cc = |number: u8, value: u8| {
            out(Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value)));
        };
        if let Some((length, bytes)) = &self.patch {
            out_patch(out, bytes, *length);
        } else if let Some(program) = self.program {
            if let Some(value) = self.bank_coarse {
                cc(controller::BANK_SELECT, value);
            }
            if let Some(value) = self.bank_fine {
                cc(controller::BANK_SELECT_FINE, value);
            }
            out(Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, channel, program, 0)));
        }
        let mut cc = |number: u8, value: u8| {
            out(Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value)));
        };
        for (number, value) in (0..=DATA_MAX).zip(&self.controllers) {
            if let Some(value) = value {
                cc(number, *value);
            }
        }
        for registered in [true, false] {
            let mut kept: [Option<Parameter>; MOST_PARAMETERS] = self.parameters;
            for place in &mut kept {
                *place = place.filter(|parameter| parameter.registered == registered);
            }
            // In ascending order of number: a selection sort over a table
            // of fixed size, with no panic path.
            while let Some(index) = (0..kept.len())
                .filter(|&index| kept.get(index).is_some_and(Option::is_some))
                .min_by_key(|&index| kept.get(index).copied().flatten().map_or(u16::MAX, |parameter| parameter.number))
            {
                let Some(parameter) = kept.get_mut(index).and_then(Option::take) else { break };
                let (coarse, fine) = if registered {
                    (controller::RPN_COARSE, controller::RPN_FINE)
                } else {
                    (controller::NRPN_COARSE, controller::NRPN_FINE)
                };
                let seven = u16::from(DATA_MAX);
                let byte = |value: u16| u8::try_from(value & seven).unwrap_or(0);
                cc(coarse, byte(parameter.number >> 7));
                cc(fine, byte(parameter.number));
                cc(controller::DATA_ENTRY, byte(parameter.value >> 7));
                cc(controller::DATA_ENTRY_FINE, byte(parameter.value));
            }
        }
        let mut sysex = self.sysex_parameters;
        while let Some(index) = (0..sysex.len())
            .filter(|&index| sysex.get(index).is_some_and(Option::is_some))
            .min_by_key(|&index| sysex.get(index).copied().flatten().map_or(u16::MAX, |parameter| parameter.key))
        {
            let Some(parameter) = sysex.get_mut(index).and_then(Option::take) else { break };
            if let Some(bytes) = parameter.bytes.get(..usize::from(parameter.length)) {
                out(Message::SystemExclusive(bytes));
            }
        }
        if self.bend != PITCH_BEND_CENTRE {
            let seven = u16::from(DATA_MAX);
            let low = u8::try_from(self.bend & seven).unwrap_or(0);
            let high = u8::try_from((self.bend >> 7) & seven).unwrap_or(0);
            out(Message::Channel(ChannelMessage::new(status::PITCH_BEND, channel, low, high)));
        }
        if self.channel_pressure != 0 {
            out(Message::Channel(ChannelMessage::new(status::CHANNEL_PRESSURE, channel, self.channel_pressure, 0)));
        }
        let mut cc = |number: u8, value: u8| {
            out(Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value)));
        };
        if self.volume != VOLUME_FULL {
            cc(controller::VOLUME, self.volume);
        }
        if self.modulation != 0 {
            cc(controller::MODULATION, self.modulation);
        }
        if self.sustain >= PEDAL_DOWN {
            cc(controller::SUSTAIN, self.sustain);
        }
    }
}

/// Gives an inline patch's bytes as one message.
fn out_patch(out: &mut dyn FnMut(Message<'_>), bytes: &[u8; MOST_SYSTEM_EXCLUSIVE_BYTES], length: u16) {
    if let Some(bytes) = bytes.get(..usize::from(length)) {
        out(Message::SystemExclusive(bytes));
    }
}

/// The sixteen parts of a sound as the sender keeps them (MIDI-94).
#[derive(Clone, Debug)]
pub struct Parts {
    parts: [Part; CHANNELS],
}

impl Default for Parts {
    fn default() -> Self {
        Self::new()
    }
}

impl Parts {
    /// Every part as a fresh synthesizer holds it.
    #[must_use]
    pub fn new() -> Self {
        Self { parts: core::array::from_fn(|_| Part::new()) }
    }

    /// Part `channel`'s state (0 to 15).
    #[must_use]
    pub fn part(&self, channel: u8) -> Option<&Part> {
        self.parts.get(usize::from(channel & CHANNEL_MASK))
    }

    /// Takes one message the sender wrote. A system exclusive message
    /// carries no channel of its own to MIDI 1.0: `sysex_channel` is the
    /// part the family's document says it addresses, with what it is.
    pub fn take(&mut self, message: &Message<'_>, sysex: Option<(u8, SysexKind)>) {
        match message {
            Message::Channel(channel) => {
                if let Some(part) = self.parts.get_mut(usize::from(channel.channel())) {
                    part.channel_message(*channel);
                }
            }
            Message::SystemExclusive(bytes) => {
                if let Some((channel, kind)) = sysex
                    && let Some(part) = self.parts.get_mut(usize::from(channel & CHANNEL_MASK))
                {
                    part.system_exclusive(bytes, kind);
                }
            }
        }
    }
}

// Expression is restated as an ordinary controller; reset all controllers
// takes it back to the value a fresh synthesizer holds.
const _: () = assert!(EXPRESSION_FULL == DATA_MAX);

#[cfg(test)]
mod tests;
