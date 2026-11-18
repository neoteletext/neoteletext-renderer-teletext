//! What the tests of the page rule table's readings share
//! (`neoteletext-common/docs/page-rules.md`, RULE-28 to RULE-31, RULE-60):
//! the families, and the control message of each that sets a readable patch
//! field, as its family document gives it. Each test file includes it as a
//! module; by itself it tests nothing.

// Without every family there is nothing here to test.
#![cfg(all(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
#![allow(
    dead_code,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::panic,
    reason = "test support, of which each test file uses a part: a test reports a failure by panicking"
)]

use neoteletext_audio_instruments::midi::{ChannelMessage, Message, controller, family, status};
use neoteletext_audio_instruments::{synth7, synth8, synth53, synth80, synth106, synth6581, synthd};

/// A family and variant a rule can name (RULE-28), with its name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Kind {
    /// The name in the documents.
    pub name: &'static str,
    /// The family's codec code.
    pub code: u8,
    /// The variant.
    pub variant: u8,
}

/// Every family and variant with readable fields, in appendix A's order.
pub const KINDS: [Kind; 11] = [
    Kind { name: "synth6581", code: family::SYNTH6581, variant: 0 },
    Kind { name: "synth8580", code: family::SYNTH6581, variant: 1 },
    Kind { name: "synth7", code: family::SYNTH7, variant: 0 },
    Kind { name: "synth106", code: family::SYNTH106, variant: 0 },
    Kind { name: "synth60", code: family::SYNTH106, variant: 1 },
    Kind { name: "synthd", code: family::SYNTHD, variant: 0 },
    Kind { name: "synth53", code: family::SYNTH53, variant: 0 },
    Kind { name: "synth52", code: family::SYNTH53, variant: 1 },
    Kind { name: "synth51", code: family::SYNTH53, variant: 2 },
    Kind { name: "synth8", code: family::SYNTH8, variant: 0 },
    Kind { name: "synth80", code: family::SYNTH80, variant: 0 },
];

/// The families that play notes, which alone have a local sound (RULE-60).
pub fn note_kinds() -> impl Iterator<Item = Kind> {
    KINDS.into_iter().filter(|kind| kind.code != family::SYNTH6581)
}

/// One message as a test holds it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Sent {
    /// A channel message.
    Channel(ChannelMessage),
    /// A system exclusive message, every byte after its `0xF0`.
    Exclusive(Vec<u8>),
}

impl Sent {
    /// The message, to give an instrument or a tracker.
    #[must_use]
    pub fn message(&self) -> Message<'_> {
        match self {
            Self::Channel(message) => Message::Channel(*message),
            Self::Exclusive(bytes) => Message::SystemExclusive(bytes),
        }
    }

    /// The message as a block's event at tick 0 (MIDI-23).
    #[must_use]
    pub fn event(&self) -> Vec<u8> {
        match self {
            Self::Channel(message) if message.kind() == status::PROGRAM_CHANGE => {
                vec![0, message.status, message.data1]
            }
            Self::Channel(message) => vec![0, message.status, message.data1, message.data2],
            Self::Exclusive(bytes) => {
                let mut event = vec![0, status::SYSTEM_EXCLUSIVE];
                // A variable-length quantity of one or two bytes (MIDI-24).
                if bytes.len() > 0x7F {
                    event.push(0x80 | (bytes.len() >> 7) as u8);
                }
                event.push((bytes.len() & 0x7F) as u8);
                event.extend_from_slice(bytes);
                event
            }
        }
    }
}

/// A control change on `channel`.
#[must_use]
pub fn control(channel: u8, number: u8, value: u8) -> Sent {
    Sent::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value))
}

/// A note on of `key` on `channel`.
#[must_use]
pub fn note_on(channel: u8, key: u8, velocity: u8) -> Sent {
    Sent::Channel(ChannelMessage::new(status::NOTE_ON, channel, key, velocity))
}

/// A note off of `key` on `channel`.
#[must_use]
pub fn note_off(channel: u8, key: u8) -> Sent {
    Sent::Channel(ChannelMessage::new(status::NOTE_OFF, channel, key, 0))
}

/// The synth106 family's switches, in the order of their fields after the
/// sliders (appendix A.3, A.4).
const SYNTH106_SWITCHES: [synth106::interface::Switch; 9] = {
    use synth106::interface::Switch;
    [
        Switch::Range,
        Switch::Pulse,
        Switch::Sawtooth,
        Switch::Chorus,
        Switch::PwmSource,
        Switch::Polarity,
        Switch::Vca,
        Switch::Hpf,
        Switch::Sub,
    ]
};

/// The messages that set field `number` of `kind` to `value` on part
/// `part`, as the family's document gives its control: the family's own
/// control message, never a patch.
#[must_use]
pub fn set_field(kind: Kind, part: u8, number: u16, value: i32) -> Vec<Sent> {
    // Every field's value fits a byte but synthD's pairs, which are split
    // into two below.
    let byte = u8::try_from(value).unwrap_or(u8::MAX);
    match kind.code {
        // S6581-10: a register data message carrying the one identifier.
        family::SYNTH6581 => {
            let mut data = synth6581::interface::RegisterData::empty();
            data.set(u8::try_from(number).unwrap(), byte);
            let mut bytes = [0; synth6581::interface::MOST_MESSAGE_BYTES];
            let length = data.write_midi(&mut bytes);
            vec![Sent::Exclusive(bytes[1..length].to_vec())]
        }
        // S7-43: a parameter change.
        family::SYNTH7 => {
            let parameter = synth7::interface::parameter_of(number).unwrap();
            let change = synth7::sysex::ParameterChange { channel: part, parameter, value: byte };
            let bytes = synth7::sysex::parameter_change(change).expect("a value within the parameter's range");
            vec![Sent::Exclusive(bytes[1..].to_vec())]
        }
        // S106-23 to S106-25: a JU-06A controller, a slider's value
        // unchanged and a switch's the lowest of its position's band.
        family::SYNTH106 => {
            use synth106::interface::{Control, canonical, panel_control};
            let switch = usize::from(number).checked_sub(synth106::patch::SLIDERS).map(|at| SYNTH106_SWITCHES[at]);
            let moves = |control: Control| match (control, switch) {
                (Control::Slider(slider), None) => u16::from(slider as u8) == number,
                (Control::Switch(moved), Some(switch)) => moved == switch,
                _ => false,
            };
            let number = (0..=127).find(|&number| panel_control(number).is_some_and(moves)).unwrap();
            let value = switch.map_or(byte, |switch| canonical(byte, switch.positions()));
            vec![control(part, number, value)]
        }
        // SD-18: a single controller, or a pair, coarse then fine.
        family::SYNTHD => {
            use synthd::interface::{LAYOUT, PLACES, Role, pair_bytes};
            let place = PLACES[usize::from(number)];
            match LAYOUT[place] {
                (coarse, Role::Coarse) => {
                    let [high, low] = pair_bytes(u16::try_from(value).unwrap());
                    vec![control(part, coarse, high), control(part, LAYOUT[place + 1].0, low)]
                }
                (single, _) => vec![control(part, single, u8::try_from(value).unwrap())],
            }
        }
        // S53-23: the program parameter's non-registered parameter number,
        // then its value by data entry, coarse and fine.
        family::SYNTH53 => {
            let parameter = synth53::interface::program_parameter(number).unwrap();
            vec![
                control(part, controller::NRPN_COARSE, parameter >> 7),
                control(part, controller::NRPN_FINE, parameter & 0x7F),
                control(part, controller::DATA_ENTRY, byte >> 7),
                control(part, controller::DATA_ENTRY_FINE, byte & 0x7F),
            ]
        }
        // S8-16: the control's controller, on the half's channel.
        family::SYNTH8 => {
            let param = synth8::interface::param_of(number).unwrap();
            vec![control(part, param.controller(), byte)]
        }
        // S80-33: the control's controller.
        family::SYNTH80 => {
            let place = synth80::interface::place_of_parameter(number).unwrap();
            vec![control(part, synth80::interface::controller_of(place).unwrap(), byte)]
        }
        other => panic!("family {other} has no fields"),
    }
}
