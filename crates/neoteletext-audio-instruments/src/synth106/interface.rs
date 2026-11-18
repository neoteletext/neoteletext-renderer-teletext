//! The programming interface of synth106 and synth60, without the sound:
//! the bytes of the Juno-106's three system exclusive messages, the
//! JU-06A's controllers and their bands, the bend range, the patch's two
//! forms and its packer and check, the bank image, the messages the family
//! takes, and the state a sender tracks and restates
//! (`neoteletext-common/docs/audio/synth106-synth60.md`, cited as "S106-n";
//! `midi-architecture.md`, "MIDI-n"; `server.md`, "SRV-n").
//!
//! The receiver's instrument ([`super::midi::Midi`]) reads every message
//! through the same [`Tracker`] the server keeps (MIDI-21, MIDI-94), so one
//! reading of each message exists. This module is built by the feature
//! `interface` alone, with no synthesis code, for the server (SRV-88).
//!
//! Every reader here takes untrusted bytes: each length is checked against
//! a named constant before it is used, and nothing panics.

use super::patch::{FIRST_SWITCHES, PATCH_BYTES, SECOND_SWITCHES, SLIDER_MAX};
// The patch's value type and its fields, under the names the server calls
// (`server.md`, SRV-88).
pub use super::patch::{ChorusMode, Model, Patch, PatchError, PwmSource, Range, Slider};
use crate::midi::common::{Action, CENTS_PER_SEMITONE, Common, Config, Selected};
use crate::midi::{
    Banks, CHANNEL_MASK, ChannelMessage, DATA_MAX, KIND_MASK, MOST_BANK_BYTES, MOST_BANK_PATCHES, Message, Outcome,
    PITCH_BEND_CENTRE, RPN_PITCH_BEND_SENSITIVITY, STATUS_BIT, controller, status,
};

/// Roland's manufacturer's identifier, byte b (Juno-106 manual, page 35,
/// sections 3.1 to 3.3: "0100 0001 Roland ID #").
pub const ROLAND: u8 = 0x41;
/// Function type `30`, the patch change: sent "When Group, Bank or Patch
/// number is changed" (S106-13; Juno-106 manual, page 35, section 3.1).
pub const FUNCTION_PATCH: u8 = 0x30;
/// Function type `31`, manual mode: sent "When Manual Button is pressed"
/// (S106-14; section 3.2).
pub const FUNCTION_MANUAL: u8 = 0x31;
/// Function type `32`, the control change of one parameter (S106-17;
/// section 3.3).
pub const FUNCTION_PARAMETER: u8 = 0x32;
/// Byte e of a function type `31` message: "Number indicates "Manual""
/// (S106-14).
pub const MANUAL_NUMBER: u8 = 0;
/// The bytes of a function type `30` or `31` message, `F0` to `F7`:
/// `F0`, b, c, d, e, the 18 values, `F7` (S106-13: "exactly 24 bytes").
pub const PATCH_MESSAGE_BYTES: usize = 24;
/// The bytes of a function type `32` message, `F0` to `F7` (S106-17:
/// "exactly 7 bytes").
pub const PARAMETER_MESSAGE_BYTES: usize = 7;
/// The three control changes that follow a synth60 patch's message, three
/// bytes each (S106-31: "as 9 bytes").
pub const SYNTH60_CONTROL_BYTES: usize = 9;
/// A synth60 patch as messages (S106-31: "The whole patch as messages is
/// 33 bytes").
pub const SYNTH60_PATCH_BYTES: usize = PATCH_MESSAGE_BYTES + SYNTH60_CONTROL_BYTES;
/// The most bytes of a patch of either model as messages.
pub const MOST_PATCH_BYTES: usize = SYNTH60_PATCH_BYTES;
/// The 18 values of a patch (S106-13: "(18 bytes total for values)").
pub const VALUES: usize = PATCH_BYTES;
/// The values of a synth60 patch: the 18 and three controller values
/// (S106-31).
pub const SYNTH60_VALUES: usize = VALUES + 3;

/// Where each byte stands in a message counted from its `F0`, as the
/// manual's letters name them (S106-13, S106-17).
const AT_ID: usize = 1;
/// Byte c.
const AT_FUNCTION: usize = 2;
/// Byte d, the channel.
const AT_CHANNEL: usize = 3;
/// Byte e: the program number, or the parameter number.
const AT_NUMBER: usize = 4;
/// Byte f, the first value.
const AT_VALUES: usize = 5;
/// `F7` of a function type `30` or `31` message.
const AT_PATCH_END: usize = AT_VALUES + VALUES;
const _: () = assert!(AT_PATCH_END + 1 == PATCH_MESSAGE_BYTES);
/// `F7` of a function type `32` message.
const AT_PARAMETER_END: usize = AT_VALUES + 1;
const _: () = assert!(AT_PARAMETER_END + 1 == PARAMETER_MESSAGE_BYTES);
/// Byte d's bits above the channel, which are 0 (S106-19).
const CHANNEL_HIGH_BITS: u8 = !CHANNEL_MASK;

// A bank of 128 patches of either model fits the bank codec (S106-39,
// MIDI-55): 3,072 or 4,224 bytes of 16,384.
const _: () = assert!(MOST_BANK_PATCHES * SYNTH60_PATCH_BYTES <= MOST_BANK_BYTES);

/// The default bend range, in cents: two semitones (S106-26).
pub const DEFAULT_BEND_RANGE_CENTS: u16 = 200;
/// The largest bend range, in cents: twelve semitones, the Juno-106's
/// "±1 octave" (S106-26; Juno-106 manual, page 23).
pub const MOST_BEND_RANGE_CENTS: u16 = 1_200;
/// The most semitones controller 87 sets: its 13 bands give 0 to 12
/// (S106-26).
pub const MOST_BEND_SEMITONES: u8 = 12;
const _: () = assert!(MOST_BEND_SEMITONES as u16 * CENTS_PER_SEMITONE == MOST_BEND_RANGE_CENTS);

/// The most cents registered parameter 0's data entry fine sets: a value
/// of 100 to 127 is taken as 99 (S106-26).
const MOST_CENTS: u8 = 99;
const _: () = assert!(MOST_CENTS as u16 + 1 == CENTS_PER_SEMITONE);

/// What the family is to the common layer (MIDI-64, MIDI-74, MIDI-90): no
/// bank select, no non-registered parameters, a bend range of two
/// semitones up to twelve.
pub const CONFIG: Config = Config {
    bend_range_cents: DEFAULT_BEND_RANGE_CENTS,
    most_bend_range_cents: MOST_BEND_RANGE_CENTS,
    bank_select: false,
    non_registered: false,
};

/// The JU-06A's controllers in its sound-module mode (JU-06A chart, page
/// 1; S106-25), by the chart's names.
pub mod ju06a {
    /// LFO RATE.
    pub const LFO_RATE: u8 = 3;
    /// PORTAMENTO TIME: counted as ignored (S106-25).
    pub const PORTAMENTO_TIME: u8 = 5;
    /// LFO DELAY.
    pub const LFO_DELAY: u8 = 9;
    /// DCO RANGE: 16′, 8′, 4′.
    pub const DCO_RANGE: u8 = 12;
    /// DCO LFO.
    pub const DCO_LFO: u8 = 13;
    /// DCO PWM.
    pub const DCO_PWM: u8 = 14;
    /// DCO LFO/MAN/ENV.
    pub const DCO_PWM_SOURCE: u8 = 15;
    /// DCO SQR SW: the pulse.
    pub const DCO_SQUARE: u8 = 16;
    /// DCO SAW SW.
    pub const DCO_SAW: u8 = 17;
    /// DCO SUB: the sub-oscillator's level.
    pub const DCO_SUB: u8 = 18;
    /// DCO NOISE.
    pub const DCO_NOISE: u8 = 19;
    /// HPF FREQ: positions 0 to 3.
    pub const HPF: u8 = 20;
    /// VCF ENV Polarity: +, −.
    pub const VCF_POLARITY: u8 = 21;
    /// VCF ENV.
    pub const VCF_ENV: u8 = 22;
    /// VCF LFO.
    pub const VCF_LFO: u8 = 23;
    /// VCF KYBD.
    pub const VCF_KEYBOARD: u8 = 24;
    /// VCA ENV / GATE.
    pub const VCA_MODE: u8 = 25;
    /// VCA LEVEL.
    pub const VCA_LEVEL: u8 = 26;
    /// ENV S.
    pub const ENV_SUSTAIN: u8 = 27;
    /// DCO SUB SW: synth60 only (S106-27).
    pub const DCO_SUB_SWITCH: u8 = 28;
    /// VCF RES.
    pub const VCF_RESONANCE: u8 = 71;
    /// ENV R.
    pub const ENV_RELEASE: u8 = 72;
    /// ENV A.
    pub const ENV_ATTACK: u8 = 73;
    /// VCF FREQ.
    pub const VCF_FREQUENCY: u8 = 74;
    /// ENV D.
    pub const ENV_DECAY: u8 = 75;
    /// POLY / (POLY) / SOLO / UNISON: counted as ignored (S106-35).
    pub const ASSIGN_MODE: u8 = 86;
    /// BEND RANGE (S106-26).
    pub const BEND_RANGE: u8 = 87;
    /// CHORUS OFF / I / II / I+II.
    pub const CHORUS: u8 = 93;
}

/// A switch of the panel that a JU-06A controller moves, with the number
/// of its positions (S106-24, S106-25).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Switch {
    /// Controller 12: 16′, 8′, 4′.
    Range,
    /// Controller 15: LFO, MAN, ENV.
    PwmSource,
    /// Controller 16: off, on.
    Pulse,
    /// Controller 17: off, on.
    Sawtooth,
    /// Controller 20: 0 to 3.
    Hpf,
    /// Controller 21: +, −.
    Polarity,
    /// Controller 25: ENV, GATE.
    Vca,
    /// Controller 28: off, on; synth60 only.
    Sub,
    /// Controller 93: OFF, I, II, I+II.
    Chorus,
}

/// Positions of a switch of two (S106-24).
const TWO: u8 = 2;
/// Positions of a switch of three.
const THREE: u8 = 3;
/// Positions of a switch of four.
const FOUR: u8 = 4;
/// Values of a data byte, 0 to 127: what a switch's bands divide.
const DATA_VALUES: u16 = DATA_MAX as u16 + 1;

impl Switch {
    /// How many positions it has.
    #[must_use]
    pub const fn positions(self) -> u8 {
        match self {
            Self::Range | Self::PwmSource => THREE,
            Self::Hpf | Self::Chorus => FOUR,
            Self::Pulse | Self::Sawtooth | Self::Polarity | Self::Vca | Self::Sub => TWO,
        }
    }

    /// Its controller.
    #[must_use]
    pub const fn controller(self) -> u8 {
        match self {
            Self::Range => ju06a::DCO_RANGE,
            Self::PwmSource => ju06a::DCO_PWM_SOURCE,
            Self::Pulse => ju06a::DCO_SQUARE,
            Self::Sawtooth => ju06a::DCO_SAW,
            Self::Hpf => ju06a::HPF,
            Self::Polarity => ju06a::VCF_POLARITY,
            Self::Vca => ju06a::VCA_MODE,
            Self::Sub => ju06a::DCO_SUB_SWITCH,
            Self::Chorus => ju06a::CHORUS,
        }
    }
}

/// The position, from 0, that a switch controller's value selects:
/// `floor(value × n ÷ 128)` (S106-24).
#[must_use]
pub const fn position(value: u8, positions: u8) -> u8 {
    let value = if value > DATA_MAX { DATA_MAX } else { value };
    let position = (value as u16).saturating_mul(positions as u16) / DATA_VALUES;
    // Below `positions`, which is at most 4.
    position as u8
}

/// The canonical value of position `position` of a switch of `positions`:
/// the lowest of its band, `ceil(p × 128 ÷ n)` (S106-24).
#[must_use]
pub const fn canonical(position: u8, positions: u8) -> u8 {
    if positions == 0 {
        return 0;
    }
    let value = (position as u16).saturating_mul(DATA_VALUES).div_ceil(positions as u16);
    if value > DATA_MAX as u16 { DATA_MAX } else { value as u8 }
}

/// The semitones controller 87's value sets: `floor(value × 13 ÷ 128)`
/// (S106-26).
#[must_use]
pub const fn bend_semitones(value: u8) -> u8 {
    position(value, MOST_BEND_SEMITONES + 1)
}

/// The canonical value of controller 87 for `semitones`, 0 to 12:
/// `ceil(n × 128 ÷ 13)` (S106-26). More than 12 is taken as 12.
#[must_use]
pub const fn bend_range_value(semitones: u8) -> u8 {
    let semitones = if semitones > MOST_BEND_SEMITONES { MOST_BEND_SEMITONES } else { semitones };
    canonical(semitones, MOST_BEND_SEMITONES + 1)
}

/// What a JU-06A controller moves on the panel (S106-25).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    /// A slider, set to the value unchanged (S106-23).
    Slider(Slider),
    /// A switch, by its bands (S106-24).
    Switch(Switch),
}

/// The panel control a controller moves, or `None` for one that moves no
/// field of the patch (S106-25).
#[must_use]
pub const fn panel_control(number: u8) -> Option<Control> {
    use ju06a::{
        DCO_LFO, DCO_NOISE, DCO_PWM, DCO_PWM_SOURCE, DCO_RANGE, DCO_SAW, DCO_SQUARE, DCO_SUB, DCO_SUB_SWITCH,
        ENV_ATTACK, ENV_DECAY, ENV_RELEASE, ENV_SUSTAIN, HPF, LFO_DELAY, LFO_RATE, VCA_LEVEL, VCA_MODE, VCF_ENV,
        VCF_FREQUENCY, VCF_KEYBOARD, VCF_LFO, VCF_POLARITY, VCF_RESONANCE,
    };
    Some(match number {
        LFO_RATE => Control::Slider(Slider::LfoRate),
        LFO_DELAY => Control::Slider(Slider::LfoDelay),
        DCO_LFO => Control::Slider(Slider::DcoLfo),
        DCO_PWM => Control::Slider(Slider::DcoPwm),
        DCO_NOISE => Control::Slider(Slider::Noise),
        VCF_FREQUENCY => Control::Slider(Slider::VcfFrequency),
        VCF_RESONANCE => Control::Slider(Slider::VcfResonance),
        VCF_ENV => Control::Slider(Slider::VcfEnvelope),
        VCF_LFO => Control::Slider(Slider::VcfLfo),
        VCF_KEYBOARD => Control::Slider(Slider::VcfKeyboard),
        VCA_LEVEL => Control::Slider(Slider::VcaLevel),
        ENV_ATTACK => Control::Slider(Slider::Attack),
        ENV_DECAY => Control::Slider(Slider::Decay),
        ENV_SUSTAIN => Control::Slider(Slider::Sustain),
        ENV_RELEASE => Control::Slider(Slider::Release),
        DCO_SUB => Control::Slider(Slider::Sub),
        DCO_RANGE => Control::Switch(Switch::Range),
        DCO_PWM_SOURCE => Control::Switch(Switch::PwmSource),
        DCO_SQUARE => Control::Switch(Switch::Pulse),
        DCO_SAW => Control::Switch(Switch::Sawtooth),
        HPF => Control::Switch(Switch::Hpf),
        VCF_POLARITY => Control::Switch(Switch::Polarity),
        VCA_MODE => Control::Switch(Switch::Vca),
        DCO_SUB_SWITCH => Control::Switch(Switch::Sub),
        ju06a::CHORUS => Control::Switch(Switch::Chorus),
        _ => return None,
    })
}

/// Sets the field controller `number` moves to `value`, on `model`
/// (S106-23 to S106-27). Answers whether the model takes the controller:
/// controller 28 on synth106, and one that moves no field, do not, and
/// change nothing. On synth106, ENV of controller 15 is taken as MAN and
/// I+II of controller 93 as II (S106-27).
pub fn set_control(patch: &mut Patch, model: Model, number: u8, value: u8) -> bool {
    let Some(control) = panel_control(number) else { return false };
    let value = value.min(DATA_MAX);
    match control {
        Control::Slider(slider) => patch.set(slider, value),
        Control::Switch(switch) => {
            let at = position(value, switch.positions());
            let on = at != 0;
            match switch {
                Switch::Range => {
                    patch.range = match at {
                        0 => Range::Sixteen,
                        1 => Range::Eight,
                        _ => Range::Four,
                    };
                }
                Switch::PwmSource => {
                    patch.pwm = match (at, model) {
                        (0, _) => PwmSource::Lfo,
                        (1, _) | (_, Model::Synth106) => PwmSource::Manual,
                        _ => PwmSource::Envelope,
                    };
                }
                Switch::Pulse => patch.pulse = on,
                Switch::Sawtooth => patch.sawtooth = on,
                Switch::Hpf => patch.hpf = at,
                Switch::Polarity => patch.envelope_inverted = on,
                Switch::Vca => patch.vca_gate = on,
                Switch::Sub if model == Model::Synth106 => return false,
                Switch::Sub => patch.sub = on,
                Switch::Chorus => {
                    patch.chorus = match (at, model) {
                        (0, _) => ChorusMode::Off,
                        (1, _) => ChorusMode::One,
                        (2, _) | (_, Model::Synth106) => ChorusMode::Two,
                        _ => ChorusMode::Both,
                    };
                }
            }
        }
    }
    true
}

/// The canonical value of the position a patch's switch stands at
/// (S106-24).
#[must_use]
pub fn switch_value(patch: &Patch, switch: Switch) -> u8 {
    let at = match switch {
        Switch::Range => match patch.range {
            Range::Sixteen => 0,
            Range::Eight => 1,
            Range::Four => 2,
        },
        Switch::PwmSource => match patch.pwm {
            PwmSource::Lfo => 0,
            PwmSource::Manual => 1,
            PwmSource::Envelope => 2,
        },
        Switch::Pulse => u8::from(patch.pulse),
        Switch::Sawtooth => u8::from(patch.sawtooth),
        Switch::Hpf => patch.hpf.min(super::patch::HPF_MAX),
        Switch::Polarity => u8::from(patch.envelope_inverted),
        Switch::Vca => u8::from(patch.vca_gate),
        Switch::Sub => u8::from(patch.sub),
        Switch::Chorus => match patch.chorus {
            ChorusMode::Off => 0,
            ChorusMode::One => 1,
            ChorusMode::Two => 2,
            ChorusMode::Both => 3,
        },
    };
    canonical(at, switch.positions())
}

/// The three settings a synth60 patch carries after its 18 values, as
/// control changes in this order (S106-31).
pub const SYNTH60_SETTINGS: [Switch; 3] = [Switch::PwmSource, Switch::Sub, Switch::Chorus];

/// A patch's name as the Juno-106 gives it (S106-11): group `'A'` or
/// `'B'`, bank 1 to 8, patch 1 to 8; program 0 is A-11 and 127 is B-88.
#[must_use]
pub const fn program_name(program: u8) -> (char, u8, u8) {
    /// Programs in a group, and patches in a bank (Juno-106 manual, page
    /// 27: groups A and B of eight banks of eight).
    const GROUP: u8 = 64;
    /// Patches in a bank.
    const BANK: u8 = 8;
    let program = program & DATA_MAX;
    let group = if program < GROUP { 'A' } else { 'B' };
    (group, ((program % GROUP) / BANK).saturating_add(1), (program % BANK).saturating_add(1))
}

/// Why a message, a patch or a bank is refused: the rule, and the byte of
/// the whole message from its `F0` (or of the bank image) that breaks it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// The byte, from 0.
    pub byte: usize,
    /// The rule it breaks.
    pub rule: Rule,
}

/// The rules of S106-19, S106-31, S106-37 and S106-38.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    /// Not a system exclusive message: no `F0`.
    NotExclusive,
    /// Byte b is not Roland's `41`.
    Manufacturer,
    /// Byte c is not `30`, `31` or `32`, or (in a bank) not `30`.
    Function,
    /// Not exactly 24 or 7 bytes, or 33 for a synth60 patch, or no `F7`
    /// where it ends.
    Length,
    /// A byte other than the last is not a data byte.
    NotData,
    /// Byte d has a bit set above bit 3.
    Channel,
    /// A function type `31` message's byte e is not `00`.
    Manual,
    /// A function type `32` message's byte e is above 17.
    ParameterNumber,
    /// A value breaks the patch's rules (S106-18).
    Patch(PatchError),
    /// A synth60 patch's control changes are not controllers 15, 28 and 93
    /// in that order, on the message's channel.
    Controls,
    /// A synth60 patch's controller value is not canonical (S106-37).
    NotCanonical,
    /// A synth60 patch's 18 values disagree with its three settings
    /// (S106-31).
    Disagrees,
    /// A bank's patch k does not carry k in byte e (S106-38).
    Program,
    /// A bank holds no patch, or more than 128, or not a whole number of
    /// patches (S106-12, S106-38).
    BankSize,
    /// The buffer given to write into is too short.
    NoRoom,
}

const fn refuse<T>(byte: usize, rule: Rule) -> Result<T, Refusal> {
    Err(Refusal { byte, rule })
}

/// One of the three system exclusive messages, read (S106-13 to S106-18).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exclusive {
    /// Function type `30` or `31`: the panel, as its 18 values say.
    Patch {
        /// `FUNCTION_PATCH` or `FUNCTION_MANUAL`.
        function: u8,
        /// The channel, byte d, 0 to 15: not used by the instrument
        /// (S106-2).
        channel: u8,
        /// Byte e: the program number, which the instrument does not use
        /// (S106-16), or 0 for manual mode.
        program: u8,
        /// The 18 values, read ([`Patch::from_bytes`]).
        patch: Patch,
    },
    /// Function type `32`: one parameter's new value.
    Parameter {
        /// The channel.
        channel: u8,
        /// 0 to 17.
        number: u8,
        /// The value, which the parameter's rules accept.
        value: u8,
    },
}

/// Reads a whole message from its `F0` to its `F7` as one of the three, or
/// says which rule of S106-19 it breaks, refused whole. Its work is bounded
/// by 24 bytes, whatever `message` holds.
///
/// # Errors
///
/// A [`Refusal`] naming the first byte that breaks a rule.
pub fn read_message(message: &[u8]) -> Result<Exclusive, Refusal> {
    let byte = |at: usize| message.get(at).copied();
    if byte(0) != Some(status::SYSTEM_EXCLUSIVE) {
        return refuse(0, Rule::NotExclusive);
    }
    if byte(AT_ID) != Some(ROLAND) {
        return refuse(AT_ID, Rule::Manufacturer);
    }
    let Some(function) = byte(AT_FUNCTION) else { return refuse(AT_FUNCTION, Rule::Length) };
    let length = match function {
        FUNCTION_PATCH | FUNCTION_MANUAL => PATCH_MESSAGE_BYTES,
        FUNCTION_PARAMETER => PARAMETER_MESSAGE_BYTES,
        _ => return refuse(AT_FUNCTION, Rule::Function),
    };
    if message.len() != length {
        return refuse(message.len().min(length).saturating_sub(1), Rule::Length);
    }
    let Some((&end, data)) = message.split_last() else { return refuse(0, Rule::Length) };
    if end != status::END_OF_EXCLUSIVE {
        return refuse(length.saturating_sub(1), Rule::Length);
    }
    if let Some(at) = data.iter().skip(1).position(|&value| value & STATUS_BIT != 0) {
        return refuse(at.saturating_add(1), Rule::NotData);
    }
    let channel = byte(AT_CHANNEL).unwrap_or(0);
    if channel & CHANNEL_HIGH_BITS != 0 {
        return refuse(AT_CHANNEL, Rule::Channel);
    }
    let number = byte(AT_NUMBER).unwrap_or(0);
    if function == FUNCTION_PARAMETER {
        let value = byte(AT_VALUES).unwrap_or(0);
        if number > SECOND_SWITCHES {
            return refuse(AT_NUMBER, Rule::ParameterNumber);
        }
        let mut check = Patch::DEFAULT;
        if let Err(error) = check.set_parameter(number, value) {
            return refuse(AT_VALUES, Rule::Patch(error));
        }
        return Ok(Exclusive::Parameter { channel, number, value });
    }
    if function == FUNCTION_MANUAL && number != MANUAL_NUMBER {
        return refuse(AT_NUMBER, Rule::Manual);
    }
    let mut values = [0u8; VALUES];
    for (value, from) in values.iter_mut().zip(data.iter().skip(AT_VALUES)) {
        *value = *from;
    }
    match Patch::from_bytes(&values) {
        Ok(patch) => Ok(Exclusive::Patch { function, channel, program: number, patch }),
        Err(error) => {
            let parameter = match error {
                PatchError::Slider(number) | PatchError::Parameter(number) => number,
                PatchError::Range => FIRST_SWITCHES,
                PatchError::Switches => SECOND_SWITCHES,
            };
            refuse(AT_VALUES.saturating_add(usize::from(parameter)), Rule::Patch(error))
        }
    }
}

/// Reads a system exclusive message as a block carries it, its bytes after
/// `F0` (MIDI-27): as [`read_message`].
///
/// # Errors
///
/// As [`read_message`], the byte counted from the `F0`.
pub fn read_exclusive(after_f0: &[u8]) -> Result<Exclusive, Refusal> {
    let mut whole = [0u8; PATCH_MESSAGE_BYTES];
    let Some(place) = whole.get_mut(1..=after_f0.len()) else {
        return refuse(PATCH_MESSAGE_BYTES, Rule::Length);
    };
    place.copy_from_slice(after_f0);
    if let Some(first) = whole.first_mut() {
        *first = status::SYSTEM_EXCLUSIVE;
    }
    read_message(whole.get(..=after_f0.len()).unwrap_or(&[]))
}

/// Writes a function type `30` message of `patch`'s 18 values into `out`:
/// byte d `channel`, byte e `program` (S106-13, S106-16).
pub fn write_patch_message(patch: &Patch, program: u8, channel: u8, out: &mut [u8; PATCH_MESSAGE_BYTES]) {
    let head = [status::SYSTEM_EXCLUSIVE, ROLAND, FUNCTION_PATCH, channel & CHANNEL_MASK, program & DATA_MAX];
    let values = patch.to_bytes();
    for (to, from) in out.iter_mut().zip(head.iter().chain(&values).chain(&[status::END_OF_EXCLUSIVE])) {
        *to = *from;
    }
}

/// Writes a function type `32` message setting parameter `number` to
/// `value` (S106-17), for the server's named parameters (SRV-33).
#[must_use]
pub const fn parameter_message(channel: u8, number: u8, value: u8) -> [u8; PARAMETER_MESSAGE_BYTES] {
    [
        status::SYSTEM_EXCLUSIVE,
        ROLAND,
        FUNCTION_PARAMETER,
        channel & CHANNEL_MASK,
        number & DATA_MAX,
        value & DATA_MAX,
        status::END_OF_EXCLUSIVE,
    ]
}

/// The bytes of a patch of `model` as messages: 24, or 33 for synth60
/// (S106-31, S106-39).
#[must_use]
pub const fn patch_bytes(model: Model) -> usize {
    match model {
        Model::Synth106 => PATCH_MESSAGE_BYTES,
        Model::Synth60 => SYNTH60_PATCH_BYTES,
    }
}

/// Packs a patch of `model` as its messages into `out` (S106-31, S106-37;
/// SRV-55): a function type `30` message with `program` in byte e and
/// `channel` in byte d, and for synth60 the three control changes on that
/// channel at canonical values, the 18 values agreeing with them. Answers
/// the bytes written, [`patch_bytes`].
pub fn pack_patch(model: Model, patch: &Patch, program: u8, channel: u8, out: &mut [u8; MOST_PATCH_BYTES]) -> usize {
    let mut message = [0u8; PATCH_MESSAGE_BYTES];
    write_patch_message(patch, program, channel, &mut message);
    for (to, from) in out.iter_mut().zip(&message) {
        *to = *from;
    }
    if model == Model::Synth60 {
        let status = status::CONTROL_CHANGE | (channel & CHANNEL_MASK);
        let controls = SYNTH60_SETTINGS.map(|switch| [status, switch.controller(), switch_value(patch, switch)]);
        for (to, from) in out.iter_mut().skip(PATCH_MESSAGE_BYTES).zip(controls.iter().flatten()) {
            *to = *from;
        }
    }
    patch_bytes(model)
}

/// Checks a patch of `model` given as its messages, as the server and the
/// receiver's bank reader both do (S106-37; MIDI-21, MIDI-63): for
/// synth106 exactly a function type `30` or `31` message; for synth60 that
/// message and the three control changes of S106-31, on the message's
/// channel, at canonical values, the 18 values agreeing with them. Answers
/// the patch.
///
/// # Errors
///
/// A [`Refusal`] naming the byte and the rule.
pub fn check_patch(model: Model, bytes: &[u8]) -> Result<Patch, Refusal> {
    let length = patch_bytes(model);
    if bytes.len() != length {
        return refuse(bytes.len().min(length), Rule::Length);
    }
    let message = bytes.get(..PATCH_MESSAGE_BYTES).unwrap_or(&[]);
    let Exclusive::Patch { channel, mut patch, .. } = read_message(message)? else {
        return refuse(AT_FUNCTION, Rule::Function);
    };
    if model == Model::Synth106 {
        return Ok(patch);
    }
    let read = patch;
    let status = status::CONTROL_CHANGE | channel;
    for (index, switch) in SYNTH60_SETTINGS.iter().enumerate() {
        let at = PATCH_MESSAGE_BYTES.saturating_add(index.saturating_mul(3));
        let control = bytes.get(at..at.saturating_add(3)).unwrap_or(&[]);
        let [kind, number, value] = *control else { return refuse(at, Rule::Length) };
        if kind != status || number != switch.controller() {
            return refuse(at, Rule::Controls);
        }
        if value > DATA_MAX || canonical(position(value, switch.positions()), switch.positions()) != value {
            return refuse(at.saturating_add(2), Rule::NotCanonical);
        }
        set_control(&mut patch, model, number, value);
    }
    // The 18 values are the nearest Juno-106 patch to the three settings
    // (S106-31): the PWM bit and the chorus bits.
    let pwm_agrees = (read.pwm == PwmSource::Lfo) == (patch.pwm == PwmSource::Lfo);
    let chorus_agrees = match patch.chorus {
        ChorusMode::Both => read.chorus == ChorusMode::Two,
        chorus => read.chorus == chorus,
    };
    if !pwm_agrees {
        return refuse(AT_VALUES.saturating_add(usize::from(SECOND_SWITCHES)), Rule::Disagrees);
    }
    if !chorus_agrees {
        return refuse(AT_VALUES.saturating_add(usize::from(FIRST_SWITCHES)), Rule::Disagrees);
    }
    Ok(patch)
}

/// Why a typed patch is refused for a model (S106-36): the field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldError {
    /// The high-pass filter's position is above 3.
    Hpf,
    /// The envelope as the pulse width's source, on synth106.
    Pwm,
    /// Chorus I+II, on synth106.
    Chorus,
    /// The sub-oscillator switched off, on synth106, which has no switch.
    Sub,
}

/// Checks a patch built from typed fields for `model` (S106-36): a value
/// only synth60 has in a synth106 patch, and a high-pass position above 3,
/// are refused. A slider is held to 0 to 127 by [`Patch::set`] itself.
///
/// # Errors
///
/// The first field refused.
pub fn check_fields(model: Model, patch: &Patch) -> Result<(), FieldError> {
    if patch.hpf > super::patch::HPF_MAX {
        return Err(FieldError::Hpf);
    }
    if model == Model::Synth106 {
        if patch.pwm == PwmSource::Envelope {
            return Err(FieldError::Pwm);
        }
        if patch.chorus == ChorusMode::Both {
            return Err(FieldError::Chorus);
        }
        if !patch.sub {
            return Err(FieldError::Sub);
        }
    }
    Ok(())
}

/// Builds a bank image of `patches`, 1 to 128 of `model`, into `out`
/// (S106-38, MIDI-56): each patch's messages back to back, patch k a
/// function type `30` message with k in byte e, on `channel`. Answers the
/// bytes written.
///
/// # Errors
///
/// [`Rule::BankSize`] for no patch or more than 128; [`Rule::NoRoom`]
/// where `out` is too short.
pub fn build_bank(model: Model, patches: &[Patch], channel: u8, out: &mut [u8]) -> Result<usize, Refusal> {
    if patches.is_empty() || patches.len() > MOST_BANK_PATCHES {
        return refuse(0, Rule::BankSize);
    }
    let size = patch_bytes(model);
    let mut written = 0usize;
    for (patch, program) in patches.iter().zip(0u8..) {
        let mut bytes = [0u8; MOST_PATCH_BYTES];
        let length = pack_patch(model, patch, program, channel, &mut bytes);
        let Some(place) = out.get_mut(written..written.saturating_add(size)) else {
            return refuse(written, Rule::NoRoom);
        };
        place.copy_from_slice(bytes.get(..length).unwrap_or(&[]));
        written = written.saturating_add(size);
    }
    Ok(written)
}

/// The patch of `program` in a bank image of `model`, as the receiver reads
/// it on a program change (S106-10, MIDI-56, MIDI-59): patch k is at byte
/// `k × size` and is a function type `30` message carrying k in byte e.
/// `None` where the image holds no whole patch there; the refusal where it
/// holds a faulty one (MIDI-63). Its work is bounded by one patch.
///
/// # Errors
///
/// Inside the `Some`: a [`Refusal`] whose byte is counted in the image.
#[must_use]
pub fn bank_patch(model: Model, image: &[u8], program: u8) -> Option<Result<Patch, Refusal>> {
    let size = patch_bytes(model);
    let start = usize::from(program).checked_mul(size)?;
    let bytes = image.get(start..start.checked_add(size)?)?;
    let counted = |refusal: Refusal| Refusal { byte: refusal.byte.saturating_add(start), rule: refusal.rule };
    if bytes.get(AT_FUNCTION) != Some(&FUNCTION_PATCH) && bytes.get(AT_ID) == Some(&ROLAND) {
        return Some(Err(counted(Refusal { byte: AT_FUNCTION, rule: Rule::Function })));
    }
    Some(check_patch(model, bytes).map_err(counted).and_then(|patch| {
        if bytes.get(AT_NUMBER) == Some(&program) {
            Ok(patch)
        } else {
            Err(counted(Refusal { byte: AT_NUMBER, rule: Rule::Program }))
        }
    }))
}

/// Checks a whole bank image of `model` (S106-38, MIDI-53): 1 to 128 whole
/// patches, each one [`bank_patch`] takes. Answers how many it holds.
///
/// # Errors
///
/// The first refusal, its byte counted in the image.
pub fn check_bank(model: Model, image: &[u8]) -> Result<usize, Refusal> {
    let size = patch_bytes(model);
    let count = image.len().checked_div(size).unwrap_or(0);
    if image.is_empty() || !image.len().is_multiple_of(size) || count > MOST_BANK_PATCHES {
        return refuse(0, Rule::BankSize);
    }
    for program in (0..=DATA_MAX).take(count) {
        match bank_patch(model, image, program) {
            Some(Ok(_)) => {}
            Some(Err(refusal)) => return Err(refusal),
            None => return refuse(usize::from(program).saturating_mul(size), Rule::BankSize),
        }
    }
    Ok(count)
}

/// The patch an instrument of `model` stands at with no patch, or one
/// missing or refused (S106-21).
#[must_use]
pub const fn default_patch(model: Model) -> Patch {
    Patch::default_for(model)
}

/// Whether the family acts on `message` at all, on `model` (SRV-24): the
/// messages of S106-1 to S106-30 the instrument does not count as ignored.
/// A data entry, increment or decrement is taken here, though it acts only
/// while registered parameter 0 is selected (S106-26).
#[must_use]
pub fn accepts(model: Model, message: &Message<'_>) -> bool {
    match message {
        Message::SystemExclusive(bytes) => read_exclusive(bytes).is_ok(),
        Message::Channel(message) => match message.kind() {
            status::NOTE_OFF | status::NOTE_ON | status::PROGRAM_CHANGE | status::PITCH_BEND => true,
            status::CONTROL_CHANGE => takes_controller(model, message.data1),
            _ => false,
        },
    }
}

/// Whether `model` takes controller `number` (S106-25, S106-26, S106-28,
/// S106-29).
#[must_use]
pub const fn takes_controller(model: Model, number: u8) -> bool {
    match number {
        controller::MODULATION
        | controller::DATA_ENTRY
        | controller::VOLUME
        | controller::DATA_ENTRY_FINE
        | controller::SUSTAIN
        | ju06a::BEND_RANGE
        | controller::DATA_INCREMENT
        | controller::DATA_DECREMENT
        | controller::RPN_FINE
        | controller::RPN_COARSE
        | controller::ALL_SOUND_OFF
        | controller::RESET_ALL_CONTROLLERS
        | controller::ALL_NOTES_OFF..=controller::POLY_ON => true,
        ju06a::DCO_SUB_SWITCH => matches!(model, Model::Synth60),
        _ => panel_control(number).is_some(),
    }
}

/// What a message does to the sound, for the instrument's engine: the
/// [`Tracker`] has already set the panel and the performance state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    /// Nothing for the engine.
    None,
    /// A key down (S106-3).
    KeyDown(u8),
    /// A key up (S106-4).
    KeyUp(u8),
    /// The pedal (S106-9).
    Sustain(bool),
    /// All notes off and the mode messages (S106-29 item 3).
    AllNotesOff,
    /// All sound off (S106-29 item 1).
    AllSoundOff,
    /// Reset all controllers (S106-29 item 2): the wheel, the bender and
    /// the pedal to rest.
    ResetControllers,
    /// The bender, 14 bits (S106-7).
    Bend(u16),
    /// The bend range, in cents (S106-26).
    BendRange(u16),
    /// Controller 7 (S106-28).
    Volume(u8),
    /// Controller 1 (S106-8).
    Wheel(u8),
    /// The panel moved: the engine is to play [`Tracker::panel`].
    Panel,
}

/// The last message that set the bend range (S106-26, S106-40 item 3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RangeBy {
    /// Neither: two semitones.
    Default,
    /// Controller 87, with its semitones.
    Controller(u8),
    /// Registered parameter 0.
    Registered,
}

/// The state of a synth106 or synth60 part as a sender keeps it and as the
/// receiver's instrument plays it (MIDI-93, MIDI-94; S106-10, S106-40):
/// the program last chosen, the panel, whether the panel has moved since
/// or came inline, the bend range and the message that set it, and the
/// common layer's controllers. Its size is fixed: one panel and one
/// common layer.
#[derive(Clone, Debug)]
pub struct Tracker {
    model: Model,
    common: Common,
    panel: Patch,
    /// The last program chosen, or `None` before any.
    program: Option<u8>,
    /// Whether the panel has moved since that program change, or its last
    /// patch came inline (S106-40 item 2).
    moved: bool,
    range_by: RangeBy,
}

impl Tracker {
    /// A part as a fresh instrument holds it, at its default patch until
    /// [`Tracker::start`] reads its bank.
    #[must_use]
    pub fn new(model: Model) -> Self {
        Self {
            model,
            common: Common::new(CONFIG),
            panel: default_patch(model),
            program: None,
            moved: false,
            range_by: RangeBy::Default,
        }
    }

    /// The model.
    #[must_use]
    pub const fn model(&self) -> Model {
        self.model
    }

    /// The panel as it stands.
    #[must_use]
    pub const fn panel(&self) -> &Patch {
        &self.panel
    }

    /// The program last chosen, or `None` before any.
    #[must_use]
    pub const fn program(&self) -> Option<u8> {
        self.program
    }

    /// The common layer's state, on channel 0, which every channel's
    /// messages are taken on (S106-2).
    #[must_use]
    pub const fn common(&self) -> &Common {
        &self.common
    }

    /// The panel of program `program` of bank index 0, and the outcome:
    /// [`Outcome::PassedOver`] where the bank holds none (S106-10),
    /// [`Outcome::PatchRefused`] where it is faulty (MIDI-63).
    fn bank_panel(&self, banks: &Banks<'_>, program: u8) -> (Patch, Outcome) {
        let image = banks.get(0).unwrap_or(&[]);
        match bank_patch(self.model, image, program) {
            Some(Ok(patch)) => (patch, Outcome::Applied),
            Some(Err(_)) => (default_patch(self.model), Outcome::PatchRefused),
            None => (default_patch(self.model), Outcome::PassedOver),
        }
    }

    /// The panel at the sound's start: program 0 of bank index 0, or the
    /// default where it is missing or faulty (MIDI-61, S106-10).
    pub fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let (panel, outcome) = self.bank_panel(banks, 0);
        self.panel = panel;
        self.moved = false;
        // A bank without program 0 is no fault of a message.
        if outcome == Outcome::PassedOver { Outcome::Applied } else { outcome }
    }

    /// Input was lost (MIDI-102, S106-40): the pedal up and no parameter
    /// selected; the panel stays. The engine releases every note.
    pub fn lost(&mut self) {
        self.common.lost();
    }

    /// Takes one message, in a block's order (S106-33, MIDI-35), and says
    /// what it does to the sound and whether it was taken.
    pub fn take(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> (Outcome, Effect) {
        match message {
            Message::SystemExclusive(bytes) => self.exclusive(bytes),
            Message::Channel(message) => self.channel(*message, banks),
        }
    }

    fn exclusive(&mut self, bytes: &[u8]) -> (Outcome, Effect) {
        // A message that breaks a rule is refused whole and changes
        // nothing (S106-19).
        match read_exclusive(bytes) {
            Ok(Exclusive::Patch { patch, .. }) => self.panel = patch,
            Ok(Exclusive::Parameter { number, value, .. }) => {
                if self.panel.set_parameter(number, value).is_err() {
                    return (Outcome::PassedOver, Effect::None);
                }
            }
            Err(_) => return (Outcome::PassedOver, Effect::None),
        }
        self.moved = true;
        (Outcome::Applied, Effect::Panel)
    }

    fn channel(&mut self, message: ChannelMessage, banks: &Banks<'_>) -> (Outcome, Effect) {
        // Every channel is the one part (S106-2): each message is taken on
        // channel 0, so that a score spread over all sixteen is one.
        let mut message = ChannelMessage { status: message.status & KIND_MASK, ..message };
        // Registered parameter 0's cents are 0 to 99: 100 to 127 is taken
        // as 99 (S106-26).
        if message.kind() == status::CONTROL_CHANGE
            && message.data1 == controller::DATA_ENTRY_FINE
            && self.common.channel(0).selected == Selected::Registered(RPN_PITCH_BEND_SENSITIVITY)
        {
            message.data2 = message.data2.min(MOST_CENTS);
        }
        let taken = self.common.take(message);
        let applied = |effect| (Outcome::Applied, effect);
        match taken.action {
            Action::None => {
                // A parameter number selected, or a data entry with no
                // parameter: the latter is counted (S106-26).
                let entry = matches!(
                    message.data1,
                    controller::DATA_ENTRY
                        | controller::DATA_ENTRY_FINE
                        | controller::DATA_INCREMENT
                        | controller::DATA_DECREMENT
                );
                if message.kind() == status::CONTROL_CHANGE && entry {
                    (Outcome::PassedOver, Effect::None)
                } else {
                    applied(Effect::None)
                }
            }
            Action::NoteOn { key, .. } => applied(Effect::KeyDown(key)),
            Action::NoteOff { key, .. } => applied(Effect::KeyUp(key)),
            Action::Sustain { down } => applied(Effect::Sustain(down)),
            Action::AllNotesOff => applied(Effect::AllNotesOff),
            Action::AllSoundOff => applied(Effect::AllSoundOff),
            Action::ResetAllControllers => applied(Effect::ResetControllers),
            Action::PitchBend { value } => applied(Effect::Bend(value)),
            Action::BendRange { cents } => {
                self.range_by = RangeBy::Registered;
                applied(Effect::BendRange(cents))
            }
            Action::Volume { value } => applied(Effect::Volume(value)),
            Action::ProgramChange { program, .. } => {
                let (panel, outcome) = self.bank_panel(banks, program);
                self.panel = panel;
                self.program = Some(program);
                self.moved = false;
                (outcome, Effect::Panel)
            }
            Action::Control { controller: controller::MODULATION, value } => applied(Effect::Wheel(value)),
            Action::Control { controller: ju06a::BEND_RANGE, value } => {
                let semitones = bend_semitones(value);
                let cents = u16::from(semitones).saturating_mul(CENTS_PER_SEMITONE);
                self.common.set_bend_range_cents(0, cents);
                self.range_by = RangeBy::Controller(semitones);
                applied(Effect::BendRange(cents))
            }
            Action::Control { controller, value } => {
                if set_control(&mut self.panel, self.model, controller, value) {
                    self.moved = true;
                    applied(Effect::Panel)
                } else {
                    (Outcome::PassedOver, Effect::None)
                }
            }
            Action::PassedOver
            | Action::ChannelPressure { .. }
            | Action::KeyPressure { .. }
            | Action::Registered { .. }
            | Action::NonRegistered { .. } => (Outcome::PassedOver, Effect::None),
        }
    }

    /// Gives the part's restatement to `out`, on `channel` (S106-40,
    /// MIDI-93, MIDI-95), in this order: the program change of the last
    /// program chosen (0 if none has been); where the panel has moved since
    /// or came inline, the whole panel as one function type `30` message
    /// and on synth60 its three control changes; the bend range where it is
    /// not two semitones, by the message that last set it; then pitch bend,
    /// controller 7, controller 1 and hold where each is not at rest. Held
    /// notes are not restated.
    pub fn restate(&self, channel: u8, out: &mut dyn FnMut(Message<'_>)) {
        let channel = channel & CHANNEL_MASK;
        let cc = |number: u8, value: u8| {
            Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value))
        };
        let program = self.program.unwrap_or(0);
        out(Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, channel, program, 0)));
        if self.moved {
            let mut bytes = [0u8; MOST_PATCH_BYTES];
            let length = pack_patch(self.model, &self.panel, program, channel, &mut bytes);
            out(Message::SystemExclusive(bytes.get(1..PATCH_MESSAGE_BYTES).unwrap_or(&[])));
            for &[_, number, value] in bytes.get(PATCH_MESSAGE_BYTES..length).unwrap_or(&[]).as_chunks::<3>().0 {
                out(cc(number, value));
            }
        }
        let state = self.common.channel(0);
        if state.bend_range_cents != DEFAULT_BEND_RANGE_CENTS {
            match self.range_by {
                RangeBy::Controller(semitones) => out(cc(ju06a::BEND_RANGE, bend_range_value(semitones))),
                RangeBy::Registered | RangeBy::Default => {
                    let range = state.bend_range_cents;
                    let byte = |value: u16| u8::try_from(value).unwrap_or(DATA_MAX).min(DATA_MAX);
                    out(cc(controller::RPN_COARSE, 0));
                    out(cc(controller::RPN_FINE, 0));
                    out(cc(controller::DATA_ENTRY, byte(range / CENTS_PER_SEMITONE)));
                    out(cc(controller::DATA_ENTRY_FINE, byte(range % CENTS_PER_SEMITONE)));
                    // The sequence leaves registered parameter 0 selected;
                    // where the part has none selected, the null number
                    // puts that back (MIDI-76, MIDI-93).
                    if state.selected != Selected::Registered(RPN_PITCH_BEND_SENSITIVITY) {
                        out(cc(controller::RPN_COARSE, DATA_MAX));
                        out(cc(controller::RPN_FINE, DATA_MAX));
                    }
                }
            }
        }
        if state.bend != PITCH_BEND_CENTRE {
            let seven = u16::from(DATA_MAX);
            let low = u8::try_from(state.bend & seven).unwrap_or(0);
            let high = u8::try_from((state.bend >> 7) & seven).unwrap_or(0);
            out(Message::Channel(ChannelMessage::new(status::PITCH_BEND, channel, low, high)));
        }
        if state.volume != DATA_MAX {
            out(cc(controller::VOLUME, state.volume));
        }
        if state.modulation != 0 {
            out(cc(controller::MODULATION, state.modulation));
        }
        if state.sustain {
            out(cc(controller::SUSTAIN, DATA_MAX));
        }
    }
}

// The slider's values are a data byte's: a controller sets a slider to its
// value unchanged (S106-23).
const _: () = assert!(SLIDER_MAX == DATA_MAX);

// Named by its path so that it is found both where this module is a file
// of its own and where the feature `interface` alone reads it through a
// path from the crate root (lib.rs).
#[path = "interface/parameters.rs"]
mod parameters;
pub use parameters::{PARAMETERS, SYNTH60_PARAMETERS, parameters, read_parameter};
