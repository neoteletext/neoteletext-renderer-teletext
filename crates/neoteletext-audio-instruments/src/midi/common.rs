//! The behaviours every synthesizer that plays notes shares, one rule each
//! where the adopted interfaces differ (MIDI-9, MIDI-84 to MIDI-92,
//! DEC-6464 version 2), and the per-channel state they need: the sustain
//! and the other pedals, pitch bend and its range by registered parameter
//! 0, the registered and non-registered parameter selection (MIDI-76),
//! bank select (MIDI-64), volume, modulation, expression and the
//! pressures.
//!
//! A family keeps one [`Common`] for its sound and gives it every channel
//! message first ([`Common::take`]). The common layer settles what is the
//! same in every family and hands on, as an [`Action`], what the family's
//! own voices must do. A family never reads controllers 64, 120, 121, 123
//! to 127, 98 to 101, 6, 38, 96 or 97 itself: the action says what they
//! mean. What the family's own document gives a controller, a parameter
//! number or a program change comes to it as [`Action::Control`],
//! [`Action::NonRegistered`], [`Action::Registered`] or
//! [`Action::ProgramChange`].
//!
//! The synth6581 family takes no channel messages (S6581-26) and keeps no
//! [`Common`].

use super::{
    CHANNELS, ChannelMessage, DATA_MAX, NULL_PARAMETER, PITCH_BEND_CENTRE, RPN_PITCH_BEND_SENSITIVITY, controller,
    status,
};

/// The sustain and the other switch pedals are down at this value and
/// above, up below it (MIDI-87; MIDI 1.0 4.2, printed page 12, PDF 44).
pub const PEDAL_DOWN: u8 = 64;
/// The release velocity of a note on of velocity 0, taken as a note off
/// (MIDI-88; MIDI 1.0 4.2, printed page 10, PDF 42).
pub const NOTE_ON_OFF_VELOCITY: u8 = 64;
/// Controller 7's value at a sound's start: full (MIDI-82).
pub const VOLUME_FULL: u8 = DATA_MAX;
/// Expression's value at a sound's start and after reset all controllers
/// (RP-015, page 2: "Set Expression (#11) to 127").
pub const EXPRESSION_FULL: u8 = DATA_MAX;
/// The longest an all sound off may take to bring every voice to silence,
/// in samples: 21.3 ms (MIDI-86).
pub const ALL_SOUND_OFF_SAMPLES: usize = 1_024;
/// Cents in a semitone, for registered parameter 0's data (MIDI-90).
pub const CENTS_PER_SEMITONE: u16 = 100;

/// One in the volume law's fixed point, Q28.
pub const VOLUME_ONE_Q28: i32 = 1 << 28;
/// The most the volume's gain moves in one sample, Q28: from nothing to
/// full in 256 samples, 5.3 ms (MIDI-82).
pub const VOLUME_SLEW_Q28: i32 = VOLUME_ONE_Q28 >> 8;
/// Controller 7's largest value, squared: the law's denominator.
const VOLUME_FULL_SQUARED: i64 = (VOLUME_FULL as i64) * (VOLUME_FULL as i64);

/// The gain controller 7 asks for, Q28: `(value × value) ÷ (127 × 127)`
/// of full (MIDI-82, MIDI-92), the one law of every family.
#[must_use]
pub fn volume_gain_q28(value: u8) -> i32 {
    let square = i64::from(value).saturating_mul(i64::from(value));
    i32::try_from(square.saturating_mul(i64::from(VOLUME_ONE_Q28)) / VOLUME_FULL_SQUARED).unwrap_or(VOLUME_ONE_Q28)
}

/// How a family is set up for the common layer: the figures its family
/// document states.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    /// The pitch bend range before any message, in cents (MIDI-90).
    pub bend_range_cents: u16,
    /// The largest pitch bend range, in cents: a larger value is set to
    /// this (MIDI-90).
    pub most_bend_range_cents: u16,
    /// Whether controllers 0 and 32 select banks (MIDI-64). Where they do
    /// not, they come to the family as [`Action::Control`].
    pub bank_select: bool,
    /// Whether controllers 99 and 98 select non-registered parameters
    /// (MIDI-74, MIDI-76). Where they do not, they come to the family as
    /// [`Action::Control`].
    pub non_registered: bool,
}

/// A data entry, increment or decrement to the parameter selected
/// (MIDI-76).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataEntry {
    /// Controller 6: the value's seven high bits, the low cleared
    /// (MIDI-75).
    Coarse(u8),
    /// Controller 38: the value's seven low bits, the high kept.
    Fine(u8),
    /// Controller 96, with its value.
    Increment(u8),
    /// Controller 97, with its value.
    Decrement(u8),
}

/// Which parameter number a channel has selected (MIDI-76).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selected {
    /// None: at a sound's start, after the null number, reset all
    /// controllers or a lost block.
    None,
    /// A registered parameter number, `MSB × 128 + LSB`.
    Registered(u16),
    /// A non-registered parameter number.
    NonRegistered(u16),
}

/// What a family's voices must do for a channel message (MIDI-9).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Nothing: the common layer took it whole (a parameter number
    /// selected, a bank select kept, a data entry to no parameter).
    None,
    /// A message no family acts on: passed over and counted (MIDI-8:
    /// local control).
    PassedOver,
    /// A key down, velocity 1 to 127.
    NoteOn {
        /// The key.
        key: u8,
        /// The velocity.
        velocity: u8,
    },
    /// A key up; a note on of velocity 0 comes as this with velocity 64
    /// (MIDI-88). Whether the pedal holds it is the family's to apply from
    /// [`ChannelState::sustain`].
    NoteOff {
        /// The key.
        key: u8,
        /// The release velocity.
        velocity: u8,
    },
    /// The sustain pedal moved, or was set again (MIDI-87): when it is up,
    /// every note it held is released.
    Sustain {
        /// Down at 64 to 127.
        down: bool,
    },
    /// Every key held on the part is released as a note off would; notes
    /// the pedal holds stay until it goes up (MIDI-85). Controllers 123 to
    /// 127 all come as this (MIDI-89).
    AllNotesOff,
    /// Every voice of the part to silence within
    /// [`ALL_SOUND_OFF_SAMPLES`], then emptied; nothing else changes
    /// (MIDI-86).
    AllSoundOff,
    /// Reset all controllers (MIDI-84, RP-015). The common layer has set
    /// its own state as RP-015 says (modulation 0, expression 127, pedals
    /// 64 to 67 up, no parameter selected, bend centred, pressures 0); the
    /// family sets its voices to match, releases the notes only the
    /// sustain pedal held, and sets its own controllers as its document's
    /// list under MIDI-84 item 4 says.
    ResetAllControllers,
    /// A pitch bend, 14 bits (MIDI-91); [`Common::bend_cents`] gives the
    /// bend in cents at the channel's range.
    PitchBend {
        /// 0 to 16,383, centre 8,192.
        value: u16,
    },
    /// Registered parameter 0 set the channel's pitch bend range
    /// (MIDI-90).
    BendRange {
        /// The range of a full bend, in cents.
        cents: u16,
    },
    /// Controller 7 (MIDI-82, MIDI-92); [`volume_gain_q28`] is the law.
    Volume {
        /// 0 to 127.
        value: u8,
    },
    /// Channel pressure.
    ChannelPressure {
        /// 0 to 127.
        pressure: u8,
    },
    /// Polyphonic key pressure.
    KeyPressure {
        /// The key.
        key: u8,
        /// 0 to 127.
        pressure: u8,
    },
    /// A program change, with the bank select values in force (MIDI-59,
    /// MIDI-64).
    ProgramChange {
        /// 0 to 127.
        program: u8,
        /// Controller 0's last value, 0 at the start.
        bank_coarse: u8,
        /// Controller 32's last value, 0 at the start.
        bank_fine: u8,
    },
    /// A data entry, increment or decrement to a registered parameter
    /// other than 0, which the common layer does not know.
    Registered {
        /// The parameter number.
        parameter: u16,
        /// What changes it.
        entry: DataEntry,
    },
    /// A data entry, increment or decrement to a non-registered parameter
    /// (MIDI-76).
    NonRegistered {
        /// The parameter number.
        parameter: u16,
        /// What changes it.
        entry: DataEntry,
    },
    /// Any other control change: the family's own map (MIDI-80). The
    /// common layer also keeps modulation (1), expression (11) and the
    /// pedals 65 to 67, which come here as well.
    Control {
        /// 0 to 119.
        controller: u8,
        /// 0 to 127.
        value: u8,
    },
}

/// What [`Common::take`] answers: the channel and what to do on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Taken {
    /// 0 to 15.
    pub channel: u8,
    /// What the family does.
    pub action: Action,
}

/// One channel's common state, as the common layer keeps it.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one of MIDI 1.0's four switch pedals, 64 to 67, which RP-015 names one by one"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChannelState {
    /// The sustain pedal, controller 64.
    pub sustain: bool,
    /// The portamento switch, controller 65, where the family has it.
    pub portamento: bool,
    /// Sostenuto, controller 66.
    pub sostenuto: bool,
    /// The soft pedal, controller 67.
    pub soft: bool,
    /// Pitch bend, 14 bits, centre 8,192.
    pub bend: u16,
    /// The range of a full bend, in cents (MIDI-90).
    pub bend_range_cents: u16,
    /// Controller 7.
    pub volume: u8,
    /// Controller 1.
    pub modulation: u8,
    /// Controller 11.
    pub expression: u8,
    /// Channel pressure.
    pub channel_pressure: u8,
    /// Controller 0's last value.
    pub bank_coarse: u8,
    /// Controller 32's last value.
    pub bank_fine: u8,
    /// The parameter number selected.
    pub selected: Selected,
    /// The registered parameter number's two bytes as last received.
    registered: [u8; 2],
    /// The non-registered parameter number's two bytes.
    non_registered: [u8; 2],
}

/// Both bytes of the null parameter number.
const NULL_BYTES: [u8; 2] = [DATA_MAX, DATA_MAX];

impl ChannelState {
    /// A channel as a sound starts it (MIDI-61, MIDI-82, MIDI-90).
    #[must_use]
    pub const fn new(bend_range_cents: u16) -> Self {
        Self {
            sustain: false,
            portamento: false,
            sostenuto: false,
            soft: false,
            bend: PITCH_BEND_CENTRE,
            bend_range_cents,
            volume: VOLUME_FULL,
            modulation: 0,
            expression: EXPRESSION_FULL,
            channel_pressure: 0,
            bank_coarse: 0,
            bank_fine: 0,
            selected: Selected::None,
            registered: NULL_BYTES,
            non_registered: NULL_BYTES,
        }
    }

    /// RP-015's reset, page 2 (MIDI-84 items 2 and 3): what it sets, and
    /// nothing it leaves (volume, bank select, the bend range).
    fn reset(&mut self) {
        self.modulation = 0;
        self.expression = EXPRESSION_FULL;
        self.sustain = false;
        self.portamento = false;
        self.sostenuto = false;
        self.soft = false;
        self.registered = NULL_BYTES;
        self.non_registered = NULL_BYTES;
        self.selected = Selected::None;
        self.bend = PITCH_BEND_CENTRE;
        self.channel_pressure = 0;
    }
}

/// What [`Common::channel`] gives for a channel past 15, which a four-bit
/// index never is.
static UNREACHED: ChannelState = ChannelState::new(0);

/// The parameter number of two bytes, or `None` for the null number.
fn parameter(bytes: [u8; 2]) -> Option<u16> {
    let [coarse, fine] = bytes;
    let number = u16::from(coarse & DATA_MAX) << 7 | u16::from(fine & DATA_MAX);
    (number != NULL_PARAMETER).then_some(number)
}

/// The common layer of one sound: the sixteen channels' common state.
#[derive(Clone, Debug)]
pub struct Common {
    config: Config,
    channels: [ChannelState; CHANNELS],
}

impl Common {
    /// Every channel as a sound starts it, at `config`'s bend range.
    #[must_use]
    pub fn new(config: Config) -> Self {
        let range = config.bend_range_cents.min(config.most_bend_range_cents);
        Self { config, channels: [ChannelState::new(range); CHANNELS] }
    }

    /// The family's setup.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// Channel `channel`'s state (0 to 15; a larger one is taken modulo 16).
    #[must_use]
    pub fn channel(&self, channel: u8) -> &ChannelState {
        let index = usize::from(channel & super::CHANNEL_MASK);
        // Sixteen channels, and the index is four bits: the fallback is
        // never reached.
        self.channels.get(index).unwrap_or(&UNREACHED)
    }

    /// The bend of `channel` in cents, `(bend − 8,192) × range ÷ 8,192`
    /// (MIDI-91).
    #[must_use]
    pub fn bend_cents(&self, channel: u8) -> i32 {
        let state = self.channel(channel);
        let offset = i32::from(state.bend).saturating_sub(i32::from(PITCH_BEND_CENTRE));
        offset.saturating_mul(i32::from(state.bend_range_cents)).checked_div(i32::from(PITCH_BEND_CENTRE)).unwrap_or(0)
    }

    /// Sets `channel`'s pitch bend range, in cents, held to the family's
    /// most: for a family whose adopted interface has a message of its own
    /// for the range, which sets the same range, the last received of
    /// either holding (MIDI-90).
    pub fn set_bend_range_cents(&mut self, channel: u8, cents: u16) {
        let most = self.config.most_bend_range_cents;
        if let Some(state) = self.channels.get_mut(usize::from(channel & super::CHANNEL_MASK)) {
            state.bend_range_cents = cents.min(most);
        }
    }

    /// Input was lost (MIDI-102): every channel's parameter selection
    /// becomes none and its sustain pedal goes up. The family releases
    /// every note itself.
    pub fn lost(&mut self) {
        for state in &mut self.channels {
            state.registered = NULL_BYTES;
            state.non_registered = NULL_BYTES;
            state.selected = Selected::None;
            state.sustain = false;
        }
    }

    /// Takes one channel message: keeps the common state, and answers what
    /// the family's voices must do. A data byte above 127, which a block
    /// never holds (MIDI-11), is passed over.
    pub fn take(&mut self, message: ChannelMessage) -> Taken {
        let channel = message.channel();
        let action = if message.data1 > DATA_MAX || message.data2 > DATA_MAX {
            Action::PassedOver
        } else {
            self.action(channel, message)
        };
        Taken { channel, action }
    }

    fn action(&mut self, channel: u8, message: ChannelMessage) -> Action {
        let ChannelMessage { data1, data2, .. } = message;
        let config = self.config;
        let Some(state) = self.channels.get_mut(usize::from(channel)) else { return Action::PassedOver };
        match message.kind() {
            status::NOTE_OFF => Action::NoteOff { key: data1, velocity: data2 },
            status::NOTE_ON if data2 == 0 => Action::NoteOff { key: data1, velocity: NOTE_ON_OFF_VELOCITY },
            status::NOTE_ON => Action::NoteOn { key: data1, velocity: data2 },
            status::KEY_PRESSURE => Action::KeyPressure { key: data1, pressure: data2 },
            status::PROGRAM_CHANGE => {
                Action::ProgramChange { program: data1, bank_coarse: state.bank_coarse, bank_fine: state.bank_fine }
            }
            status::CHANNEL_PRESSURE => {
                state.channel_pressure = data1;
                Action::ChannelPressure { pressure: data1 }
            }
            status::PITCH_BEND => {
                state.bend = message.bend();
                Action::PitchBend { value: state.bend }
            }
            status::CONTROL_CHANGE => control(state, config, data1, data2),
            _ => Action::PassedOver,
        }
    }
}

/// A control change on one channel.
fn control(state: &mut ChannelState, config: Config, number: u8, value: u8) -> Action {
    let down = value >= PEDAL_DOWN;
    match number {
        controller::SUSTAIN => {
            state.sustain = down;
            Action::Sustain { down }
        }
        controller::VOLUME => {
            state.volume = value;
            Action::Volume { value }
        }
        controller::ALL_SOUND_OFF => Action::AllSoundOff,
        controller::RESET_ALL_CONTROLLERS => {
            state.reset();
            Action::ResetAllControllers
        }
        controller::LOCAL_CONTROL => Action::PassedOver,
        // Omni off and on, mono on and poly on are all notes off and
        // nothing more (MIDI-89).
        controller::ALL_NOTES_OFF..=controller::POLY_ON => Action::AllNotesOff,
        controller::RPN_COARSE | controller::RPN_FINE => {
            let byte = usize::from(number == controller::RPN_FINE);
            if let Some(place) = state.registered.get_mut(byte) {
                *place = value;
            }
            state.selected = parameter(state.registered).map_or(Selected::None, Selected::Registered);
            Action::None
        }
        controller::NRPN_COARSE | controller::NRPN_FINE if config.non_registered => {
            let byte = usize::from(number == controller::NRPN_FINE);
            if let Some(place) = state.non_registered.get_mut(byte) {
                *place = value;
            }
            state.selected = parameter(state.non_registered).map_or(Selected::None, Selected::NonRegistered);
            Action::None
        }
        controller::DATA_ENTRY
        | controller::DATA_ENTRY_FINE
        | controller::DATA_INCREMENT
        | controller::DATA_DECREMENT => {
            let entry = match number {
                controller::DATA_ENTRY => DataEntry::Coarse(value),
                controller::DATA_ENTRY_FINE => DataEntry::Fine(value),
                controller::DATA_INCREMENT => DataEntry::Increment(value),
                _ => DataEntry::Decrement(value),
            };
            data_entry(state, config, entry)
        }
        controller::BANK_SELECT if config.bank_select => {
            state.bank_coarse = value;
            Action::None
        }
        controller::BANK_SELECT_FINE if config.bank_select => {
            state.bank_fine = value;
            Action::None
        }
        _ => {
            match number {
                controller::MODULATION => state.modulation = value,
                controller::EXPRESSION => state.expression = value,
                controller::PORTAMENTO => state.portamento = down,
                controller::SOSTENUTO => state.sostenuto = down,
                controller::SOFT => state.soft = down,
                _ => {}
            }
            Action::Control { controller: number, value }
        }
    }
}

/// A data entry, increment or decrement to the selected parameter: to no
/// parameter it changes nothing (MIDI-76); to registered parameter 0 it
/// sets the bend range (MIDI-90); to any other it goes to the family.
fn data_entry(state: &mut ChannelState, config: Config, entry: DataEntry) -> Action {
    match state.selected {
        Selected::None => Action::None,
        Selected::Registered(RPN_PITCH_BEND_SENSITIVITY) => {
            let range = state.bend_range_cents;
            let semitones = range / CENTS_PER_SEMITONE;
            // Coarse is the semitones, clearing the cents as a coarse
            // value clears its fine (MIDI-75); fine is the cents; each step
            // of increment or decrement is one cent, not below 0 (MIDI-90,
            // NeoTeletext's own reading, section 19 item 15).
            let wanted = match entry {
                DataEntry::Coarse(value) => u16::from(value).saturating_mul(CENTS_PER_SEMITONE),
                DataEntry::Fine(value) => semitones.saturating_mul(CENTS_PER_SEMITONE).saturating_add(u16::from(value)),
                DataEntry::Increment(_) => range.saturating_add(1),
                DataEntry::Decrement(_) => range.saturating_sub(1),
            };
            state.bend_range_cents = wanted.min(config.most_bend_range_cents);
            Action::BendRange { cents: state.bend_range_cents }
        }
        Selected::Registered(parameter) => Action::Registered { parameter, entry },
        Selected::NonRegistered(parameter) => Action::NonRegistered { parameter, entry },
    }
}

/// The 14-bit value of a pitch bend that bends `cents` on an instrument
/// whose full bend is a fixed `range_cents`: how a family whose voices
/// bend by a fixed range is given a bend at the channel's range set by
/// registered parameter 0 (MIDI-90). A bend past the fixed range is held
/// at its end.
#[must_use]
pub fn bend_for_fixed_range(cents: i32, range_cents: u16) -> u16 {
    if range_cents == 0 {
        return PITCH_BEND_CENTRE;
    }
    let offset = cents.saturating_mul(i32::from(PITCH_BEND_CENTRE)).checked_div(i32::from(range_cents)).unwrap_or(0);
    let value = i32::from(PITCH_BEND_CENTRE).saturating_add(offset).clamp(0, i32::from(super::PITCH_BEND_MAX));
    u16::try_from(value).unwrap_or(PITCH_BEND_CENTRE)
}

#[cfg(test)]
mod tests;
