//! synth53, synth52 and synth51's programming interface, without their
//! synthesis: the adopted interface of the family document
//! (`neoteletext-common/docs/audio/synth53-synth52-synth51.md`, cited as
//! "S53-n"; the architecture's requirements as "MIDI-n"), which is
//! Sequential's "Prophet-5 MIDI Implementation" 1.4 (cited as "MI, page
//! n").
//!
//! What is here is what the receiver and the server share (MIDI-21,
//! DEC-6462), and none of it makes a sound:
//!
//! - the 67 parameters of a program and their maxima ([`maximum`],
//!   [`parameter`]; S53-27), and the control changes that set them
//!   ([`parameter_of_controller`]; S53-21);
//! - the 128-byte program ([`Program`]; S53-33), its default ([`Program::default_for`];
//!   S53-30), and the packing of eight bits into seven ([`pack`],
//!   [`unpack`]; S53-31, S53-32);
//! - the program dump and the edit buffer dump ([`program_dump`],
//!   [`edit_buffer_dump`], [`check_patch`]; S53-28, S53-29), and the bank
//!   image of one group's program dumps ([`build_bank`], [`check_bank`],
//!   [`bank_group`]; S53-25, S53-26);
//! - the program as named fields ([`Fields`]; S53-47, the server
//!   document's SRV-37);
//! - which messages the family takes ([`accepts`]; S53-2);
//! - the one part's state as the interface moves it, for the receiver's
//!   instrument and the server's tracker alike, with its restatement
//!   ([`Part`]; S53-8, S53-13 to S53-23, S53-46).
//!
//! It is compiled under the feature `interface` without the family's
//! synthesis (the server document's SRV-88), and with it under `synth53`.

use crate::midi::common::{Action, CENTS_PER_SEMITONE, Common, Config, DataEntry, PEDAL_DOWN, Selected, VOLUME_FULL};
use crate::midi::{
    Banks, ChannelMessage, DATA_MAX, MOST_BANK_BYTES, Message, Outcome, PITCH_BEND_CENTRE, RPN_PITCH_BEND_SENSITIVITY,
    STATUS_BIT, controller, status,
};

/// Bytes of a program (S53-33): "The 128 'packed' parameter bytes in the
/// program dump follow the order of the NRPN list, one byte per parameter,
/// and padded with zeros from the final parameter to the 128th byte" (MI,
/// page 13).
pub const PROGRAM_BYTES: usize = 128;
/// The parameters of a program (S53-27): numbers 0 to 64, 86 and 87.
pub const PROGRAM_PARAMETERS: usize = 67;
/// One more than the highest parameter number a program carries, 87
/// (S53-33): bytes from here to 127 are padding.
pub const PARAMETER_SPAN: usize = 88;

/// Data bytes of one packet of the packed format: "with the MS bit stripped
/// from 7 parameter bytes" (MI, page 13).
pub const PACKET_DATA: usize = 7;
/// Bytes of one packed packet: "Data is packed in 8 byte 'packets'" (MI,
/// page 13).
pub const PACKET_BYTES: usize = PACKET_DATA + 1;
/// Packed bytes of a program: "128 bytes expanded to 152 MIDI bytes in
/// 'packed MS bit' format" (MI, page 12).
pub const PACKED_BYTES: usize = 152;
/// Packets of a packed program: 19.
pub const PACKETS: usize = PACKED_BYTES / PACKET_BYTES;
/// Data bytes the 19 packets hold: the program's 128 and five positions
/// after it, which a writer fills with 0 and a reader ignores (S53-31,
/// S53-32).
pub const UNPACKED_BYTES: usize = PACKETS * PACKET_DATA;
const _: () = assert!(PACKETS * PACKET_BYTES == PACKED_BYTES);
const _: () = assert!(UNPACKED_BYTES >= PROGRAM_BYTES && UNPACKED_BYTES - PROGRAM_BYTES < PACKET_DATA);

/// The maker's identifier of every message here, "DSI ID" (MI, page 12).
pub const SEQUENTIAL_ID: u8 = 0x01;
/// The instrument's identifier, "Prophet-5 ID" (MI, page 12; contradiction
/// C5 of the family document for why it is 0x31).
pub const PROPHET_5_ID: u8 = 0x31;
/// The command byte of a program dump, "Program Data" (MI, page 12).
pub const PROGRAM_DATA: u8 = 0x02;
/// The command byte of an edit buffer dump, "Edit Buffer Data" (MI, page
/// 12).
pub const EDIT_BUFFER_DATA: u8 = 0x03;
/// System exclusive's first byte.
const START: u8 = status::SYSTEM_EXCLUSIVE;
/// System exclusive's last byte.
const END: u8 = status::END_OF_EXCLUSIVE;
/// Bytes before a program dump's packed data: `F0 01 31 02`, the group and
/// the program number (MI, page 12).
const PROGRAM_DUMP_HEAD: usize = 6;
/// Bytes before an edit buffer dump's packed data: `F0 01 31 03` (MI, page
/// 12).
const EDIT_BUFFER_HEAD: usize = 4;
/// Bytes of a program dump after its command byte that are not packed
/// data: the group and the program number.
const PROGRAM_NUMBERS: usize = 2;
/// Where a program dump's packed data ends, counted from after its command
/// byte.
const PROGRAM_PACKED_END: usize = PROGRAM_NUMBERS + PACKED_BYTES;
/// Bytes of a program dump (S53-28): 159.
pub const PROGRAM_DUMP_BYTES: usize = PROGRAM_DUMP_HEAD + PACKED_BYTES + 1;
/// Bytes of an edit buffer dump (S53-29): 157.
pub const EDIT_BUFFER_DUMP_BYTES: usize = EDIT_BUFFER_HEAD + PACKED_BYTES + 1;
const _: () = assert!(PROGRAM_DUMP_BYTES == 159 && EDIT_BUFFER_DUMP_BYTES == 157);
// An edit buffer dump is one system exclusive event of a block (S53-26
// (b), MIDI-28).
const _: () = assert!(EDIT_BUFFER_DUMP_BYTES - 1 <= crate::midi::MOST_SYSTEM_EXCLUSIVE_BYTES);

/// Groups of programs a program dump numbers: "Group Number: 0 - 9" (MI,
/// page 12; S53-25).
pub const GROUPS: u8 = 10;
/// Programs of a group: "Program Number: 0 - 39" (MI, page 12), and
/// program change "0-39" (MI, page 2; S53-18).
pub const GROUP_PROGRAMS: u8 = 40;
/// The largest value of controller 32 that chooses a group: 1 to 10
/// choose groups 0 to 9 (S53-13).
pub const BANK_SELECT_MOST: u8 = GROUPS;
// A bank of one group is within the architecture's bank image (S53-26
// (a), MIDI-55).
const _: () = assert!(GROUP_PROGRAMS as usize * PROGRAM_DUMP_BYTES <= MOST_BANK_BYTES);
const _: () = assert!(GROUPS as usize == crate::midi::MOST_BANKS);

/// The pitch wheel range before any message, in cents: seven semitones,
/// "about a 5th", the instrument modelled's (S53-8; `mod.rs`,
/// `PITCH_BEND_SEMITONES`).
pub const DEFAULT_BEND_RANGE_CENTS: u16 = 700;
/// The largest pitch wheel range, in cents: an octave, "up to an octave"
/// (User's Guide, page 37; S53-8).
pub const MOST_BEND_RANGE_CENTS: u16 = 1_200;

/// The common layer's setup for this family: controllers 0 and 32 come to
/// the family as control changes, so that controller 0 is counted
/// (S53-13), and the non-registered parameter numbers are selected
/// (S53-22).
pub const CONFIG: Config = Config {
    bend_range_cents: DEFAULT_BEND_RANGE_CENTS,
    most_bend_range_cents: MOST_BEND_RANGE_CENTS,
    bank_select: false,
    non_registered: true,
};

/// The parameter numbers of a program (MI, pages 8 and 9; S53-27), named
/// as the server document's fields are (SRV-37).
pub mod parameter {
    /// OSC A FREQUENCY, 0 to 120.
    pub const OSC_A_FREQUENCY: u8 = 0;
    /// OSC B FREQUENCY, 0 to 120.
    pub const OSC_B_FREQUENCY: u8 = 1;
    /// OSC B FINE TUNE, 0 to 127.
    pub const OSC_B_FINE_TUNE: u8 = 2;
    /// OSC A SAW ON/OFF (printed "ON/FF").
    pub const OSC_A_SAW: u8 = 3;
    /// OSC A SQUARE ON/OFF.
    pub const OSC_A_SQUARE: u8 = 4;
    /// OSC B SAW ON/OFF.
    pub const OSC_B_SAW: u8 = 5;
    /// OSC B TRI ON/OFF.
    pub const OSC_B_TRI: u8 = 6;
    /// OSC B SQUARE ON/OFF.
    pub const OSC_B_SQUARE: u8 = 7;
    /// OSC A PULSE WIDTH, 0 to 120.
    pub const OSC_A_PULSE_WIDTH: u8 = 8;
    /// OSC B PULSE WIDTH, 0 to 120.
    pub const OSC_B_PULSE_WIDTH: u8 = 9;
    /// OSC SYNC ON/OFF.
    pub const OSC_SYNC: u8 = 10;
    /// OSC B LOW FREQ ON/OFF.
    pub const OSC_B_LOW_FREQ: u8 = 11;
    /// OSC B KEYBOARD ON/OFF.
    pub const OSC_B_KEYBOARD: u8 = 12;
    /// GLIDE RATE, 0 to 120.
    pub const GLIDE_RATE: u8 = 13;
    /// OSC A LEVEL, 0 to 120.
    pub const OSC_A_LEVEL: u8 = 14;
    /// OSC B LEVEL, 0 to 120.
    pub const OSC_B_LEVEL: u8 = 15;
    /// NOISE LEVEL, 0 to 120.
    pub const NOISE_LEVEL: u8 = 16;
    /// CUTOFF, 0 to 120.
    pub const CUTOFF: u8 = 17;
    /// RESONANCE, 0 to 120.
    pub const RESONANCE: u8 = 18;
    /// FILTER KEYBOARD TRACK OFF/HALF/FULL, 0 to 2.
    pub const FILTER_KEYBOARD_TRACK: u8 = 19;
    /// FILTER REV SELECT: 0 "1/2", 1 "3" (S53-38), kept.
    pub const FILTER_REV_SELECT: u8 = 20;
    /// LFO FREQUENCY, 0 to 120.
    pub const LFO_FREQUENCY: u8 = 21;
    /// LFO INITIAL AMOUNT, 0 to 120.
    pub const LFO_INITIAL_AMOUNT: u8 = 22;
    /// LFO SAW ON/OFF.
    pub const LFO_SAW: u8 = 23;
    /// LFO TRI ON/OFF.
    pub const LFO_TRI: u8 = 24;
    /// LFO SQUARE ON/OFF.
    pub const LFO_SQUARE: u8 = 25;
    /// LFO SOURCE MIX, 0 to 120.
    pub const LFO_SOURCE_MIX: u8 = 26;
    /// LFO FREQ A ON/OFF: Wheel-Mod to oscillator A's frequency.
    pub const LFO_FREQ_A: u8 = 27;
    /// LFO FREQ B ON/OFF: to oscillator B's frequency.
    pub const LFO_FREQ_B: u8 = 28;
    /// LFO FREQ PW A ON/OFF: to oscillator A's pulse width.
    pub const LFO_FREQ_PW_A: u8 = 29;
    /// LFO FREQ PW B ON/OFF: to oscillator B's pulse width.
    pub const LFO_FREQ_PW_B: u8 = 30;
    /// LFO FILTER ON/OFF: to the cutoff.
    pub const LFO_FILTER: u8 = 31;
    /// POLY MOD FILT ENV AMOUNT, 0 to 127.
    pub const POLY_MOD_FILT_ENV_AMOUNT: u8 = 32;
    /// POLY MOD OSC B AMOUNT, 0 to 120.
    pub const POLY_MOD_OSC_B_AMOUNT: u8 = 33;
    /// POLY MOD FREQ A ON/OFF.
    pub const POLY_MOD_FREQ_A: u8 = 34;
    /// POLY MOD PW ON/OFF.
    pub const POLY_MOD_PW: u8 = 35;
    /// POLY MOD FILTER ON/OFF.
    pub const POLY_MOD_FILTER: u8 = 36;
    /// VINTAGE, 0 to 127, kept (S53-39).
    pub const VINTAGE: u8 = 37;
    /// AFTERTOUCH > FILTER, kept.
    pub const AFTERTOUCH_TO_FILTER: u8 = 38;
    /// AFTERTOUCH > LFO (printed "AMP", contradiction C1), kept.
    pub const AFTERTOUCH_TO_LFO: u8 = 39;
    /// ENV FILTER AMOUNT, 0 to 120.
    pub const ENV_FILTER_AMOUNT: u8 = 40;
    /// VELOCITY > FILTER (printed "VELOCTIY"), kept.
    pub const VELOCITY_TO_FILTER: u8 = 41;
    /// VELOCITY > AMP, kept.
    pub const VELOCITY_TO_AMP: u8 = 42;
    /// ATTACK FILTER, 0 to 120.
    pub const ATTACK_FILTER: u8 = 43;
    /// ATTACK VCA, 0 to 120.
    pub const ATTACK_VCA: u8 = 44;
    /// DECAY FILTER, 0 to 120.
    pub const DECAY_FILTER: u8 = 45;
    /// DECAY VCA, 0 to 120.
    pub const DECAY_VCA: u8 = 46;
    /// SUSTAIN FILTER, 0 to 120.
    pub const SUSTAIN_FILTER: u8 = 47;
    /// SUSTAIN VCA, 0 to 120.
    pub const SUSTAIN_VCA: u8 = 48;
    /// RELEASE FILTER, 0 to 120.
    pub const RELEASE_FILTER: u8 = 49;
    /// RELEASE VCA, 0 to 120.
    pub const RELEASE_VCA: u8 = 50;
    /// RELEASE SWITCH.
    pub const RELEASE_SWITCH: u8 = 51;
    /// UNISON ON/OFF.
    pub const UNISON: u8 = 52;
    /// UNISON VOICE COUNT, 0 to 10 (S53-42).
    pub const UNISON_VOICE_COUNT: u8 = 53;
    /// UNISON DETUNE, 0 to 7 (S53-42).
    pub const UNISON_DETUNE: u8 = 54;
    /// The first of the ten UNISON NOTE parameters, 55 to 64, kept.
    pub const UNISON_NOTE_FIRST: u8 = 55;
    /// The UNISON NOTE parameters.
    pub const UNISON_NOTES: usize = 10;
    /// PITCH WHEEL RANGE, 0 to 11: 1 to 12 semitones (S53-8).
    pub const PITCH_WHEEL_RANGE: u8 = 86;
    /// RETRIGGER AND UNISON, 0 to 3: the key priority of unison (S53-42).
    pub const RETRIGGER_AND_UNISON: u8 = 87;
}

/// The top of a knob of the adopted interface: its dial's 10 (MI, pages 8
/// and 9; User's Guide, pages 18 to 35).
pub const KNOB_TOP: u8 = 120;
/// The top of a knob that runs 0 to 127.
const FULL: u8 = DATA_MAX;
/// The top of a switch.
const SWITCH: u8 = 1;

/// The 67 parameters of a program and their stated maxima, every row of
/// MI pages 8 and 9 numbered 0 to 64, 86 and 87 (S53-27).
pub const PARAMETERS: [(u8, u8); PROGRAM_PARAMETERS] = {
    use parameter as p;
    [
        (p::OSC_A_FREQUENCY, KNOB_TOP),
        (p::OSC_B_FREQUENCY, KNOB_TOP),
        (p::OSC_B_FINE_TUNE, FULL),
        (p::OSC_A_SAW, SWITCH),
        (p::OSC_A_SQUARE, SWITCH),
        (p::OSC_B_SAW, SWITCH),
        (p::OSC_B_TRI, SWITCH),
        (p::OSC_B_SQUARE, SWITCH),
        (p::OSC_A_PULSE_WIDTH, KNOB_TOP),
        (p::OSC_B_PULSE_WIDTH, KNOB_TOP),
        (p::OSC_SYNC, SWITCH),
        (p::OSC_B_LOW_FREQ, SWITCH),
        (p::OSC_B_KEYBOARD, SWITCH),
        (p::GLIDE_RATE, KNOB_TOP),
        (p::OSC_A_LEVEL, KNOB_TOP),
        (p::OSC_B_LEVEL, KNOB_TOP),
        (p::NOISE_LEVEL, KNOB_TOP),
        (p::CUTOFF, KNOB_TOP),
        (p::RESONANCE, KNOB_TOP),
        // "OFF/HALF/FULL", 0-2.
        (p::FILTER_KEYBOARD_TRACK, 2),
        (p::FILTER_REV_SELECT, SWITCH),
        (p::LFO_FREQUENCY, KNOB_TOP),
        (p::LFO_INITIAL_AMOUNT, KNOB_TOP),
        (p::LFO_SAW, SWITCH),
        (p::LFO_TRI, SWITCH),
        (p::LFO_SQUARE, SWITCH),
        (p::LFO_SOURCE_MIX, KNOB_TOP),
        (p::LFO_FREQ_A, SWITCH),
        (p::LFO_FREQ_B, SWITCH),
        (p::LFO_FREQ_PW_A, SWITCH),
        (p::LFO_FREQ_PW_B, SWITCH),
        (p::LFO_FILTER, SWITCH),
        (p::POLY_MOD_FILT_ENV_AMOUNT, FULL),
        (p::POLY_MOD_OSC_B_AMOUNT, KNOB_TOP),
        (p::POLY_MOD_FREQ_A, SWITCH),
        (p::POLY_MOD_PW, SWITCH),
        (p::POLY_MOD_FILTER, SWITCH),
        (p::VINTAGE, FULL),
        (p::AFTERTOUCH_TO_FILTER, SWITCH),
        (p::AFTERTOUCH_TO_LFO, SWITCH),
        (p::ENV_FILTER_AMOUNT, KNOB_TOP),
        (p::VELOCITY_TO_FILTER, SWITCH),
        (p::VELOCITY_TO_AMP, SWITCH),
        (p::ATTACK_FILTER, KNOB_TOP),
        (p::ATTACK_VCA, KNOB_TOP),
        (p::DECAY_FILTER, KNOB_TOP),
        (p::DECAY_VCA, KNOB_TOP),
        (p::SUSTAIN_FILTER, KNOB_TOP),
        (p::SUSTAIN_VCA, KNOB_TOP),
        (p::RELEASE_FILTER, KNOB_TOP),
        (p::RELEASE_VCA, KNOB_TOP),
        (p::RELEASE_SWITCH, SWITCH),
        (p::UNISON, SWITCH),
        // "UNISON VOICE COUNT", 0-10.
        (p::UNISON_VOICE_COUNT, 10),
        // "UNISON DETUNE", 0-7.
        (p::UNISON_DETUNE, 7),
        // "UNISON NOTE", 55-64, "1-10": only the maximum is checked
        // (S53-33).
        (55, 10),
        (56, 10),
        (57, 10),
        (58, 10),
        (59, 10),
        (60, 10),
        (61, 10),
        (62, 10),
        (63, 10),
        (64, 10),
        // "PITCH WHEEL RANGE", 0-11.
        (p::PITCH_WHEEL_RANGE, 11),
        // "RETRIGGER AND UNISON", 0-3.
        (p::RETRIGGER_AND_UNISON, 3),
    ]
};

/// Each number's maximum, or `None` for a reserved number.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const MAXIMA: [Option<u8>; PARAMETER_SPAN] = {
    let mut maxima = [None; PARAMETER_SPAN];
    let mut index = 0;
    while index < PARAMETERS.len() {
        let (number, most) = PARAMETERS[index];
        // S53-27: no number twice.
        assert!(maxima[number as usize].is_none());
        maxima[number as usize] = Some(most);
        index += 1;
    }
    maxima
};

/// The stated maximum of parameter `number`, or `None` for a reserved
/// number (65 to 85, 88 to 94), a global parameter (4096 to 4109) or any
/// other number, none of which a program carries (S53-23, S53-27).
#[must_use]
pub fn maximum(number: u16) -> Option<u8> {
    MAXIMA.get(usize::from(number)).copied().flatten()
}

/// The control changes of the program's parameters, every row of MI pages
/// 4 and 5 that has a parameter number, as (controller, parameter)
/// (section 5.1 of the family document; S53-21). Controllers 7 and 74 have
/// none.
pub const CONTROLLERS: [(u8, u8); 57] = {
    use parameter as p;
    [
        (3, p::OSC_A_FREQUENCY),
        (9, p::OSC_B_FREQUENCY),
        (14, p::OSC_B_FINE_TUNE),
        (15, p::OSC_A_SAW),
        (20, p::OSC_A_SQUARE),
        (21, p::OSC_A_PULSE_WIDTH),
        (22, p::OSC_B_PULSE_WIDTH),
        (23, p::OSC_SYNC),
        (24, p::OSC_B_LOW_FREQ),
        (25, p::OSC_B_KEYBOARD),
        (26, p::GLIDE_RATE),
        (27, p::OSC_A_LEVEL),
        (28, p::OSC_B_LEVEL),
        (29, p::NOISE_LEVEL),
        (30, p::OSC_B_SAW),
        (31, p::RESONANCE),
        (35, p::FILTER_KEYBOARD_TRACK),
        (41, p::FILTER_REV_SELECT),
        (46, p::LFO_FREQUENCY),
        (47, p::LFO_INITIAL_AMOUNT),
        (52, p::OSC_B_TRI),
        (53, p::LFO_SOURCE_MIX),
        (54, p::LFO_FREQ_A),
        (55, p::LFO_FREQ_B),
        (56, p::LFO_FREQ_PW_A),
        (57, p::LFO_FREQ_PW_B),
        (58, p::LFO_FILTER),
        (59, p::POLY_MOD_FILT_ENV_AMOUNT),
        (60, p::POLY_MOD_OSC_B_AMOUNT),
        (61, p::POLY_MOD_FREQ_A),
        (62, p::POLY_MOD_PW),
        (63, p::POLY_MOD_FILTER),
        (70, p::PITCH_WHEEL_RANGE),
        (71, p::RETRIGGER_AND_UNISON),
        (73, p::CUTOFF),
        (85, p::VINTAGE),
        (86, p::AFTERTOUCH_TO_FILTER),
        (87, p::AFTERTOUCH_TO_LFO),
        (89, p::ENV_FILTER_AMOUNT),
        (90, p::VELOCITY_TO_FILTER),
        (102, p::VELOCITY_TO_AMP),
        (103, p::ATTACK_FILTER),
        (104, p::ATTACK_VCA),
        (105, p::DECAY_FILTER),
        (106, p::DECAY_VCA),
        (107, p::SUSTAIN_FILTER),
        (108, p::SUSTAIN_VCA),
        (109, p::RELEASE_FILTER),
        (110, p::RELEASE_VCA),
        (111, p::RELEASE_SWITCH),
        (112, p::UNISON),
        (113, p::UNISON_VOICE_COUNT),
        (114, p::UNISON_DETUNE),
        (116, p::OSC_B_SQUARE),
        (117, p::LFO_SAW),
        (118, p::LFO_TRI),
        (119, p::LFO_SQUARE),
    ]
};

/// Controller 74, brightness (MI, pages 2 and 5; S53-12).
pub const BRIGHTNESS: u8 = 74;

/// The parameter each controller sets, by controller number, built from
/// [`CONTROLLERS`] with the assertions S53-21 asks for: no controller and
/// no parameter twice, every parameter a program parameter, and none of the
/// parameter number controllers, volume or brightness among them.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const PARAMETER_OF_CONTROLLER: [Option<u8>; 128] = {
    let mut map = [None; 128];
    let mut seen = [false; PARAMETER_SPAN];
    let mut index = 0;
    while index < CONTROLLERS.len() {
        let (number, parameter) = CONTROLLERS[index];
        assert!(map[number as usize].is_none());
        assert!(!seen[parameter as usize]);
        assert!(MAXIMA[parameter as usize].is_some());
        assert!(
            number != controller::DATA_ENTRY
                && number != controller::DATA_ENTRY_FINE
                && number != controller::VOLUME
                && number != BRIGHTNESS
                && !(number >= controller::DATA_INCREMENT && number <= controller::RPN_COARSE)
                && number < controller::ALL_SOUND_OFF
        );
        seen[parameter as usize] = true;
        map[number as usize] = Some(parameter);
        index += 1;
    }
    map
};

/// The parameter controller `number` sets (S53-21), or `None`.
#[must_use]
pub fn parameter_of_controller(number: u8) -> Option<u8> {
    PARAMETER_OF_CONTROLLER.get(usize::from(number)).copied().flatten()
}

/// Which synthesizer of the family: the kind, which alone chooses the
/// model (S53-1), and the OBJECT's tag 5 (MIDI-48).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    /// synth53: revision 3.
    Synth53,
    /// synth52: revisions 1 and 2.
    Synth52,
    /// synth51: one model with synth52 today (DEC-6461 item 6).
    Synth51,
}

impl Variant {
    /// The value of the OBJECT's tag 5 (S53-1): 0, 1 or 2.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Synth53 => 0,
            Self::Synth52 => 1,
            Self::Synth51 => 2,
        }
    }

    /// The variant tag 5's value names, or `None`.
    #[must_use]
    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Synth53),
            1 => Some(Self::Synth52),
            2 => Some(Self::Synth51),
            _ => None,
        }
    }

    /// What a writer puts in FILTER REV SELECT for the kind: 1 ("3") for
    /// synth53, 0 ("1/2") for the others (S53-38).
    #[must_use]
    pub const fn filter_revision(self) -> u8 {
        match self {
            Self::Synth53 => 1,
            Self::Synth52 | Self::Synth51 => 0,
        }
    }
}

/// A parameter byte above its parameter's stated maximum: the server's
/// refusal names the byte, the parameter and the maximum (S53-33).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParameterError {
    /// The parameter number, which is the byte's place in the program.
    pub parameter: u8,
    /// Its stated maximum.
    pub maximum: u8,
    /// The value found.
    pub value: u8,
}

/// One program: 128 bytes, byte n parameter n (S53-33). It holds whatever
/// bytes it was given, reserved and padding bytes included, so that a dump
/// is carried unchanged; [`Program::get`] reads a parameter as the
/// receiver takes it, held to its maximum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Program {
    bytes: [u8; PROGRAM_BYTES],
}

impl Program {
    /// A program of these bytes, as they are.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; PROGRAM_BYTES]) -> Self {
        Self { bytes }
    }

    /// The program's bytes, as they are.
    #[must_use]
    pub const fn bytes(&self) -> &[u8; PROGRAM_BYTES] {
        &self.bytes
    }

    /// The default program (S53-30), with FILTER REV SELECT written for
    /// the kind (S53-38): oscillator A's sawtooth alone, both frequency
    /// knobs and both pulse widths at 64, oscillator B on the keyboard,
    /// oscillator A full in the mixer, the cutoff open with full keyboard
    /// tracking, the low-frequency oscillator's triangle, both sustains
    /// full, the release switch on, unison off at five voices, the pitch
    /// wheel range seven semitones and the key assignment of value 0.
    #[must_use]
    #[allow(clippy::indexing_slicing, reason = "every index is a parameter number below 128, checked by the compiler")]
    pub const fn default_for(variant: Variant) -> Self {
        use parameter as p;
        let mut bytes = [0; PROGRAM_BYTES];
        let middle = 64;
        bytes[p::OSC_A_FREQUENCY as usize] = middle;
        bytes[p::OSC_B_FREQUENCY as usize] = middle;
        bytes[p::OSC_A_SAW as usize] = 1;
        bytes[p::OSC_A_PULSE_WIDTH as usize] = middle;
        bytes[p::OSC_B_PULSE_WIDTH as usize] = middle;
        bytes[p::OSC_B_KEYBOARD as usize] = 1;
        bytes[p::OSC_A_LEVEL as usize] = KNOB_TOP;
        bytes[p::CUTOFF as usize] = KNOB_TOP;
        // Full.
        bytes[p::FILTER_KEYBOARD_TRACK as usize] = 2;
        bytes[p::FILTER_REV_SELECT as usize] = variant.filter_revision();
        bytes[p::LFO_TRI as usize] = 1;
        bytes[p::SUSTAIN_FILTER as usize] = KNOB_TOP;
        bytes[p::SUSTAIN_VCA as usize] = KNOB_TOP;
        bytes[p::RELEASE_SWITCH as usize] = 1;
        // Five voices (S53-42 item 1).
        bytes[p::UNISON_VOICE_COUNT as usize] = 4;
        // Seven semitones (S53-8).
        bytes[p::PITCH_WHEEL_RANGE as usize] = 6;
        Self { bytes }
    }

    /// Parameter `number`'s value as a receiver takes it: a byte above the
    /// parameter's maximum is the maximum (S53-33); 0 for a number that is
    /// no parameter.
    #[must_use]
    pub fn get(&self, number: u8) -> u8 {
        let most = maximum(u16::from(number)).unwrap_or(0);
        self.bytes.get(usize::from(number)).map_or(0, |&byte| byte.min(most))
    }

    /// Sets parameter `number` to `value` held to its maximum (S53-21,
    /// S53-23). Answers whether `number` is a parameter of a program;
    /// where it is not, nothing changes.
    pub fn set(&mut self, number: u8, value: u8) -> bool {
        let Some(most) = maximum(u16::from(number)) else { return false };
        self.bytes.get_mut(usize::from(number)).is_some_and(|byte| {
            *byte = value.min(most);
            true
        })
    }

    /// The server's check (S53-33): every parameter byte within its stated
    /// maximum. Reserved and padding bytes are not checked, and stay as
    /// they are.
    ///
    /// # Errors
    ///
    /// The first parameter above its maximum.
    pub fn check(&self) -> Result<(), ParameterError> {
        for &(parameter, maximum) in &PARAMETERS {
            let value = self.bytes.get(usize::from(parameter)).copied().unwrap_or(0);
            if value > maximum {
                return Err(ParameterError { parameter, maximum, value });
            }
        }
        Ok(())
    }
}

/// Packs a program's 128 bytes into 152 (S53-32): in each packet of eight,
/// the first byte holds in bit i the top bit of the packet's data byte i,
/// and the next seven are the data bytes with their top bits cleared (MI,
/// page 13); the five positions after byte 127 are written 0.
#[must_use]
pub fn pack(program: &[u8; PROGRAM_BYTES]) -> [u8; PACKED_BYTES] {
    let mut data = [0; UNPACKED_BYTES];
    for (to, from) in data.iter_mut().zip(program) {
        *to = *from;
    }
    let mut packed = [0; PACKED_BYTES];
    let (packets, _) = packed.as_chunks_mut::<PACKET_BYTES>();
    let (sevens, _) = data.as_chunks::<PACKET_DATA>();
    for (packet, seven) in packets.iter_mut().zip(sevens) {
        let Some((head, body)) = packet.split_first_mut() else { continue };
        let mut bit = 1u8;
        for (to, &from) in body.iter_mut().zip(seven) {
            *to = from & DATA_MAX;
            if from & STATUS_BIT != 0 {
                *head |= bit;
            }
            bit = bit.wrapping_shl(1);
        }
    }
    packed
}

/// Unpacks 152 packed bytes into 133 (S53-31): data byte `7k + i` is
/// `P[8k + 1 + i]` with the top bit taken from bit i of `P[8k]`. Bytes 0
/// to 127 are the program; the last five are ignored by every reader here.
#[must_use]
pub fn unpack(packed: &[u8; PACKED_BYTES]) -> [u8; UNPACKED_BYTES] {
    let mut data = [0; UNPACKED_BYTES];
    let (packets, _) = packed.as_chunks::<PACKET_BYTES>();
    let (sevens, _) = data.as_chunks_mut::<PACKET_DATA>();
    for (packet, seven) in packets.iter().zip(sevens) {
        let Some((&head, body)) = packet.split_first() else { continue };
        let mut bit = 1u8;
        for (to, &from) in seven.iter_mut().zip(body) {
            *to = (from & DATA_MAX) | if head & bit != 0 { STATUS_BIT } else { 0 };
            bit = bit.wrapping_shl(1);
        }
    }
    data
}

/// The program of 152 packed bytes, its five extra positions ignored.
fn unpack_program(packed: &[u8; PACKED_BYTES]) -> Program {
    let data = unpack(packed);
    let mut bytes = [0; PROGRAM_BYTES];
    for (to, from) in bytes.iter_mut().zip(&data) {
        *to = *from;
    }
    Program { bytes }
}

/// The edit buffer dump of a program, all 157 bytes from `F0` to `F7`
/// (S53-29; MI, page 12).
#[must_use]
pub fn edit_buffer_dump(program: &Program) -> [u8; EDIT_BUFFER_DUMP_BYTES] {
    let mut dump = [0; EDIT_BUFFER_DUMP_BYTES];
    let head = [START, SEQUENTIAL_ID, PROPHET_5_ID, EDIT_BUFFER_DATA];
    let packed = pack(&program.bytes);
    for (to, from) in dump.iter_mut().zip(head.iter().chain(&packed).chain(&[END])) {
        *to = *from;
    }
    dump
}

/// The program dump of a program at `group` (0 to 9) and `number` (0 to
/// 39), all 159 bytes (S53-28; MI, page 12); `None` for a place outside
/// those.
#[must_use]
pub fn program_dump(group: u8, number: u8, program: &Program) -> Option<[u8; PROGRAM_DUMP_BYTES]> {
    if group >= GROUPS || number >= GROUP_PROGRAMS {
        return None;
    }
    let mut dump = [0; PROGRAM_DUMP_BYTES];
    let head = [START, SEQUENTIAL_ID, PROPHET_5_ID, PROGRAM_DATA, group, number];
    let packed = pack(&program.bytes);
    for (to, from) in dump.iter_mut().zip(head.iter().chain(&packed).chain(&[END])) {
        *to = *from;
    }
    Some(dump)
}

/// What a system exclusive message is to this family, read as the receiver
/// reads it: leniently, every byte's top bit ignored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sysex {
    /// An edit buffer dump of the right length (S53-29).
    EditBuffer(Program),
    /// The edit buffer dump's first four bytes and another length: a faulty
    /// patch (S53-29, MIDI-63).
    FaultyEditBuffer,
    /// A program dump of the right length, group and program number
    /// (S53-28).
    ProgramDump {
        /// 0 to 9.
        group: u8,
        /// 0 to 39.
        number: u8,
        /// The program.
        program: Program,
    },
    /// The program dump's first four bytes and another length, group or
    /// program: a faulty patch, at the group and program its bytes 4 and 5
    /// give where it has them (S53-28).
    FaultyProgramDump {
        /// Byte 4, if there is one.
        group: Option<u8>,
        /// Byte 5, if there is one.
        number: Option<u8>,
    },
    /// Anything else.
    Other,
}

/// Reads a system exclusive message given as every byte after its `F0`,
/// the last `F7` (the form of [`Message::SystemExclusive`]).
#[must_use]
pub fn read_sysex(after_start: &[u8]) -> Sysex {
    let [SEQUENTIAL_ID, PROPHET_5_ID, command, rest @ ..] = after_start else { return Sysex::Other };
    let ended = after_start.last() == Some(&END);
    match *command {
        EDIT_BUFFER_DATA => match <&[u8; PACKED_BYTES]>::try_from(rest.get(..PACKED_BYTES).unwrap_or(&[])) {
            Ok(packed) if ended && after_start.len() == EDIT_BUFFER_DUMP_BYTES - 1 => {
                Sysex::EditBuffer(unpack_program(packed))
            }
            _ => Sysex::FaultyEditBuffer,
        },
        PROGRAM_DATA => {
            let (group, number) = (rest.first().copied(), rest.get(1).copied());
            let packed = rest
                .get(PROGRAM_NUMBERS..PROGRAM_PACKED_END)
                .and_then(|packed| <&[u8; PACKED_BYTES]>::try_from(packed).ok());
            match (group, number, packed) {
                (Some(group), Some(number), Some(packed))
                    if ended
                        && after_start.len() == PROGRAM_DUMP_BYTES - 1
                        && group < GROUPS
                        && number < GROUP_PROGRAMS =>
                {
                    Sysex::ProgramDump { group, number, program: unpack_program(packed) }
                }
                _ => Sysex::FaultyProgramDump { group, number },
            }
        }
        _ => Sysex::Other,
    }
}

/// Reads one message of a bank image, from its `F0`.
fn read_message(message: &[u8]) -> Sysex {
    match message.split_first() {
        Some((&START, rest)) => read_sysex(rest),
        _ => Sysex::Other,
    }
}

/// The messages of a bank image, each from an `F0` up to and including the
/// next `F7` (or to the image's end), and any run of bytes between them
/// as an item of its own: the dumps back to back of S53-26 (a).
#[derive(Clone, Debug)]
pub struct Dumps<'a> {
    rest: &'a [u8],
}

impl<'a> Dumps<'a> {
    /// The messages of `image`.
    #[must_use]
    pub const fn new(image: &'a [u8]) -> Self {
        Self { rest: image }
    }
}

impl<'a> Iterator for Dumps<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let first = *self.rest.first()?;
        let end = if first == START {
            self.rest.iter().position(|&byte| byte == END).map_or(self.rest.len(), |at| at.saturating_add(1))
        } else {
            self.rest.iter().position(|&byte| byte == START).unwrap_or(self.rest.len())
        };
        let (item, rest) = self.rest.split_at(end.min(self.rest.len()));
        self.rest = rest;
        Some(item)
    }
}

/// The group a bank image holds (S53-26 (a)): that of its first valid
/// program dump, or else of its first faulty one that has a group byte;
/// `None` for an image with neither.
#[must_use]
pub fn bank_group(image: &[u8]) -> Option<u8> {
    let mut faulty = None;
    for message in Dumps::new(image) {
        match read_message(message) {
            Sysex::ProgramDump { group, .. } => return Some(group),
            Sysex::FaultyProgramDump { group: Some(group), .. } if faulty.is_none() => faulty = Some(group),
            _ => {}
        }
    }
    faulty
}

/// What a bank holds at a program number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lookup {
    /// A program.
    Found(Program),
    /// A faulty program dump stands there (S53-28, MIDI-63).
    Faulty,
    /// Nothing does (MIDI-59).
    Missing,
}

/// The program numbered `number` of one bank image: its first dump that
/// carries that program number (S53-28).
#[must_use]
pub fn lookup_image(image: &[u8], number: u8) -> Lookup {
    for message in Dumps::new(image) {
        match read_message(message) {
            Sysex::ProgramDump { number: found, program, .. } if found == number => return Lookup::Found(program),
            Sysex::FaultyProgramDump { number: Some(found), .. } if found == number => return Lookup::Faulty,
            _ => {}
        }
    }
    Lookup::Missing
}

/// The program at `group` and `number` among a sound's banks: in the bank
/// whose dumps carry that group, wherever it stands among them (S53-18,
/// S53-26 (a)).
#[must_use]
pub fn lookup(banks: &Banks<'_>, group: u8, number: u8) -> Lookup {
    (0..banks.len())
        .filter_map(|index| banks.get(index))
        .find(|image| bank_group(image) == Some(group))
        .map_or(Lookup::Missing, |image| lookup_image(image, number))
}

/// Why the server refuses an edit buffer dump or a program dump.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DumpError {
    /// Not `F0 01 31` and the dump's command byte.
    Header,
    /// Not the dump's length: the length found.
    Length(usize),
    /// A byte between `F0` and `F7` with its top bit set, at this offset.
    NotData(usize),
    /// Not ended by `F7`.
    End,
    /// A program dump's group above 9.
    Group(u8),
    /// A program dump's program number above 39.
    Number(u8),
    /// A parameter above its maximum (S53-33).
    Parameter(ParameterError),
}

/// Checks the framing every dump shares: `F0`, data bytes, `F7`, and the
/// length and command byte.
fn check_frame(bytes: &[u8], command: u8, length: usize) -> Result<(), DumpError> {
    if !matches!(bytes, [START, SEQUENTIAL_ID, PROPHET_5_ID, found, ..] if *found == command) {
        return Err(DumpError::Header);
    }
    if bytes.len() != length {
        return Err(DumpError::Length(bytes.len()));
    }
    if bytes.last() != Some(&END) {
        return Err(DumpError::End);
    }
    let inner = bytes.get(1..length.saturating_sub(1)).unwrap_or(&[]);
    match inner.iter().position(|&byte| byte & STATUS_BIT != 0) {
        Some(at) => Err(DumpError::NotData(at.saturating_add(1))),
        None => Ok(()),
    }
}

/// The server's check of an edit buffer dump, all 157 bytes from `F0`
/// (S53-29, S53-31, S53-33; the server document's SRV-38): its framing and
/// every parameter within its maximum. Reserved and padding bytes, and the
/// five positions after byte 127, are not checked (S53-33).
///
/// # Errors
///
/// What is wrong, the first fault found.
pub fn check_patch(bytes: &[u8]) -> Result<Program, DumpError> {
    check_frame(bytes, EDIT_BUFFER_DATA, EDIT_BUFFER_DUMP_BYTES)?;
    match read_message(bytes) {
        Sysex::EditBuffer(program) => program.check().map(|()| program).map_err(DumpError::Parameter),
        _ => Err(DumpError::Header),
    }
}

/// The server's check of one program dump, all 159 bytes from `F0`
/// (S53-28, S53-33): its group, program number and program.
///
/// # Errors
///
/// What is wrong, the first fault found.
pub fn check_program_dump(bytes: &[u8]) -> Result<(u8, u8, Program), DumpError> {
    check_frame(bytes, PROGRAM_DATA, PROGRAM_DUMP_BYTES)?;
    if let Sysex::ProgramDump { group, number, program } = read_message(bytes) {
        program.check().map_err(DumpError::Parameter)?;
        return Ok((group, number, program));
    }
    let group = bytes.get(PROGRAM_DUMP_HEAD.saturating_sub(PROGRAM_NUMBERS)).copied().unwrap_or(0);
    let number = bytes.get(PROGRAM_DUMP_HEAD.saturating_sub(1)).copied().unwrap_or(0);
    Err(if group >= GROUPS { DumpError::Group(group) } else { DumpError::Number(number) })
}

/// Why the server refuses a bank (S53-26 (a)).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankError {
    /// No program, or more than 40.
    Count(usize),
    /// More bytes than a bank image holds (MIDI-55).
    TooLong(usize),
    /// The dump at this index, counted from 0, is faulty.
    Dump(usize, DumpError),
    /// The dump at this index carries another group than the first.
    TwoGroups(usize),
    /// The dump at this index carries a program number already given.
    Repeated(usize),
    /// A group above 9 asked of [`build_bank`].
    Group(u8),
    /// The buffer given to [`build_bank`] is too short.
    Room,
}

/// What a checked bank holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BankSummary {
    /// Its group, 0 to 9.
    pub group: u8,
    /// Its programs, 1 to 40.
    pub programs: usize,
}

/// The server's check of a bank image (S53-26 (a), S53-28; the server
/// document's SRV-38 and SRV-60): 1 to 40 program dumps of 159 bytes back
/// to back and nothing else, every one of one group and each of a
/// different program number, each passing [`check_program_dump`].
///
/// # Errors
///
/// What is wrong, naming the dump.
pub fn check_bank(image: &[u8]) -> Result<BankSummary, BankError> {
    if image.len() > MOST_BANK_BYTES {
        return Err(BankError::TooLong(image.len()));
    }
    let mut group = None;
    let mut seen = [false; GROUP_PROGRAMS as usize];
    let mut count = 0usize;
    for (index, message) in Dumps::new(image).enumerate() {
        let (found, number, _) = check_program_dump(message).map_err(|error| BankError::Dump(index, error))?;
        if *group.get_or_insert(found) != found {
            return Err(BankError::TwoGroups(index));
        }
        match seen.get_mut(usize::from(number)) {
            Some(place) if !*place => *place = true,
            _ => return Err(BankError::Repeated(index)),
        }
        count = count.saturating_add(1);
    }
    match group {
        Some(group) if count <= usize::from(GROUP_PROGRAMS) => Ok(BankSummary { group, programs: count }),
        _ => Err(BankError::Count(count)),
    }
}

/// Writes the bank image of `programs` at `group` into `out`: program k as
/// the program dump of `group` and k (S53-26 (a); the server document's
/// SRV-38). Answers the image's length.
///
/// # Errors
///
/// No program or more than 40, a group above 9, a program that fails
/// [`Program::check`], or too little room in `out`.
pub fn build_bank(group: u8, programs: &[Program], out: &mut [u8]) -> Result<usize, BankError> {
    if programs.is_empty() || programs.len() > usize::from(GROUP_PROGRAMS) {
        return Err(BankError::Count(programs.len()));
    }
    if group >= GROUPS {
        return Err(BankError::Group(group));
    }
    let mut places = out.as_chunks_mut::<PROGRAM_DUMP_BYTES>().0.iter_mut();
    for ((index, program), number) in programs.iter().enumerate().zip(0..GROUP_PROGRAMS) {
        program.check().map_err(|error| BankError::Dump(index, DumpError::Parameter(error)))?;
        let place = places.next().ok_or(BankError::Room)?;
        let dump = program_dump(group, number, program).ok_or(BankError::Group(group))?;
        *place = dump;
    }
    Ok(programs.len().saturating_mul(PROGRAM_DUMP_BYTES))
}

/// A program as named fields (S53-47; the server document's SRV-37): the
/// 66 program parameters other than FILTER REV SELECT, which a writer
/// writes from the kind (S53-38), each an integer in its parameter's range.
#[expect(missing_docs, reason = "each field is the parameter of the same name in `parameter`, documented there")]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fields {
    pub osc_a_frequency: u8,
    pub osc_b_frequency: u8,
    pub osc_b_fine_tune: u8,
    pub osc_a_saw: u8,
    pub osc_a_square: u8,
    pub osc_b_saw: u8,
    pub osc_b_tri: u8,
    pub osc_b_square: u8,
    pub osc_a_pulse_width: u8,
    pub osc_b_pulse_width: u8,
    pub osc_sync: u8,
    pub osc_b_low_freq: u8,
    pub osc_b_keyboard: u8,
    pub glide_rate: u8,
    pub osc_a_level: u8,
    pub osc_b_level: u8,
    pub noise_level: u8,
    pub cutoff: u8,
    pub resonance: u8,
    pub filter_keyboard_track: u8,
    pub lfo_frequency: u8,
    pub lfo_initial_amount: u8,
    pub lfo_saw: u8,
    pub lfo_tri: u8,
    pub lfo_square: u8,
    pub lfo_source_mix: u8,
    pub lfo_freq_a: u8,
    pub lfo_freq_b: u8,
    pub lfo_freq_pw_a: u8,
    pub lfo_freq_pw_b: u8,
    pub lfo_filter: u8,
    pub poly_mod_filt_env_amount: u8,
    pub poly_mod_osc_b_amount: u8,
    pub poly_mod_freq_a: u8,
    pub poly_mod_pw: u8,
    pub poly_mod_filter: u8,
    pub vintage: u8,
    pub aftertouch_to_filter: u8,
    pub aftertouch_to_lfo: u8,
    pub env_filter_amount: u8,
    pub velocity_to_filter: u8,
    pub velocity_to_amp: u8,
    pub attack_filter: u8,
    pub attack_vca: u8,
    pub decay_filter: u8,
    pub decay_vca: u8,
    pub sustain_filter: u8,
    pub sustain_vca: u8,
    pub release_filter: u8,
    pub release_vca: u8,
    pub release_switch: u8,
    pub unison: u8,
    pub unison_voice_count: u8,
    pub unison_detune: u8,
    pub unison_notes: [u8; parameter::UNISON_NOTES],
    pub pitch_wheel_range: u8,
    pub retrigger_and_unison: u8,
}

impl Fields {
    /// Every field with its parameter number, in the program's order.
    fn pairs(&self) -> [(u8, u8); PROGRAM_PARAMETERS - 1] {
        use parameter as p;
        let [n0, n1, n2, n3, n4, n5, n6, n7, n8, n9] = self.unison_notes;
        [
            (p::OSC_A_FREQUENCY, self.osc_a_frequency),
            (p::OSC_B_FREQUENCY, self.osc_b_frequency),
            (p::OSC_B_FINE_TUNE, self.osc_b_fine_tune),
            (p::OSC_A_SAW, self.osc_a_saw),
            (p::OSC_A_SQUARE, self.osc_a_square),
            (p::OSC_B_SAW, self.osc_b_saw),
            (p::OSC_B_TRI, self.osc_b_tri),
            (p::OSC_B_SQUARE, self.osc_b_square),
            (p::OSC_A_PULSE_WIDTH, self.osc_a_pulse_width),
            (p::OSC_B_PULSE_WIDTH, self.osc_b_pulse_width),
            (p::OSC_SYNC, self.osc_sync),
            (p::OSC_B_LOW_FREQ, self.osc_b_low_freq),
            (p::OSC_B_KEYBOARD, self.osc_b_keyboard),
            (p::GLIDE_RATE, self.glide_rate),
            (p::OSC_A_LEVEL, self.osc_a_level),
            (p::OSC_B_LEVEL, self.osc_b_level),
            (p::NOISE_LEVEL, self.noise_level),
            (p::CUTOFF, self.cutoff),
            (p::RESONANCE, self.resonance),
            (p::FILTER_KEYBOARD_TRACK, self.filter_keyboard_track),
            (p::LFO_FREQUENCY, self.lfo_frequency),
            (p::LFO_INITIAL_AMOUNT, self.lfo_initial_amount),
            (p::LFO_SAW, self.lfo_saw),
            (p::LFO_TRI, self.lfo_tri),
            (p::LFO_SQUARE, self.lfo_square),
            (p::LFO_SOURCE_MIX, self.lfo_source_mix),
            (p::LFO_FREQ_A, self.lfo_freq_a),
            (p::LFO_FREQ_B, self.lfo_freq_b),
            (p::LFO_FREQ_PW_A, self.lfo_freq_pw_a),
            (p::LFO_FREQ_PW_B, self.lfo_freq_pw_b),
            (p::LFO_FILTER, self.lfo_filter),
            (p::POLY_MOD_FILT_ENV_AMOUNT, self.poly_mod_filt_env_amount),
            (p::POLY_MOD_OSC_B_AMOUNT, self.poly_mod_osc_b_amount),
            (p::POLY_MOD_FREQ_A, self.poly_mod_freq_a),
            (p::POLY_MOD_PW, self.poly_mod_pw),
            (p::POLY_MOD_FILTER, self.poly_mod_filter),
            (p::VINTAGE, self.vintage),
            (p::AFTERTOUCH_TO_FILTER, self.aftertouch_to_filter),
            (p::AFTERTOUCH_TO_LFO, self.aftertouch_to_lfo),
            (p::ENV_FILTER_AMOUNT, self.env_filter_amount),
            (p::VELOCITY_TO_FILTER, self.velocity_to_filter),
            (p::VELOCITY_TO_AMP, self.velocity_to_amp),
            (p::ATTACK_FILTER, self.attack_filter),
            (p::ATTACK_VCA, self.attack_vca),
            (p::DECAY_FILTER, self.decay_filter),
            (p::DECAY_VCA, self.decay_vca),
            (p::SUSTAIN_FILTER, self.sustain_filter),
            (p::SUSTAIN_VCA, self.sustain_vca),
            (p::RELEASE_FILTER, self.release_filter),
            (p::RELEASE_VCA, self.release_vca),
            (p::RELEASE_SWITCH, self.release_switch),
            (p::UNISON, self.unison),
            (p::UNISON_VOICE_COUNT, self.unison_voice_count),
            (p::UNISON_DETUNE, self.unison_detune),
            (55, n0),
            (56, n1),
            (57, n2),
            (58, n3),
            (59, n4),
            (60, n5),
            (61, n6),
            (62, n7),
            (63, n8),
            (64, n9),
            (p::PITCH_WHEEL_RANGE, self.pitch_wheel_range),
            (p::RETRIGGER_AND_UNISON, self.retrigger_and_unison),
        ]
    }

    /// The fields of a program, parameter 20 left out, each held to its
    /// maximum as a receiver reads it.
    #[must_use]
    pub fn of(program: &Program) -> Self {
        use parameter as p;
        let get = |number: u8| program.get(number);
        let notes =
            core::array::from_fn(|index| get(p::UNISON_NOTE_FIRST.saturating_add(u8::try_from(index).unwrap_or(0))));
        Self {
            osc_a_frequency: get(p::OSC_A_FREQUENCY),
            osc_b_frequency: get(p::OSC_B_FREQUENCY),
            osc_b_fine_tune: get(p::OSC_B_FINE_TUNE),
            osc_a_saw: get(p::OSC_A_SAW),
            osc_a_square: get(p::OSC_A_SQUARE),
            osc_b_saw: get(p::OSC_B_SAW),
            osc_b_tri: get(p::OSC_B_TRI),
            osc_b_square: get(p::OSC_B_SQUARE),
            osc_a_pulse_width: get(p::OSC_A_PULSE_WIDTH),
            osc_b_pulse_width: get(p::OSC_B_PULSE_WIDTH),
            osc_sync: get(p::OSC_SYNC),
            osc_b_low_freq: get(p::OSC_B_LOW_FREQ),
            osc_b_keyboard: get(p::OSC_B_KEYBOARD),
            glide_rate: get(p::GLIDE_RATE),
            osc_a_level: get(p::OSC_A_LEVEL),
            osc_b_level: get(p::OSC_B_LEVEL),
            noise_level: get(p::NOISE_LEVEL),
            cutoff: get(p::CUTOFF),
            resonance: get(p::RESONANCE),
            filter_keyboard_track: get(p::FILTER_KEYBOARD_TRACK),
            lfo_frequency: get(p::LFO_FREQUENCY),
            lfo_initial_amount: get(p::LFO_INITIAL_AMOUNT),
            lfo_saw: get(p::LFO_SAW),
            lfo_tri: get(p::LFO_TRI),
            lfo_square: get(p::LFO_SQUARE),
            lfo_source_mix: get(p::LFO_SOURCE_MIX),
            lfo_freq_a: get(p::LFO_FREQ_A),
            lfo_freq_b: get(p::LFO_FREQ_B),
            lfo_freq_pw_a: get(p::LFO_FREQ_PW_A),
            lfo_freq_pw_b: get(p::LFO_FREQ_PW_B),
            lfo_filter: get(p::LFO_FILTER),
            poly_mod_filt_env_amount: get(p::POLY_MOD_FILT_ENV_AMOUNT),
            poly_mod_osc_b_amount: get(p::POLY_MOD_OSC_B_AMOUNT),
            poly_mod_freq_a: get(p::POLY_MOD_FREQ_A),
            poly_mod_pw: get(p::POLY_MOD_PW),
            poly_mod_filter: get(p::POLY_MOD_FILTER),
            vintage: get(p::VINTAGE),
            aftertouch_to_filter: get(p::AFTERTOUCH_TO_FILTER),
            aftertouch_to_lfo: get(p::AFTERTOUCH_TO_LFO),
            env_filter_amount: get(p::ENV_FILTER_AMOUNT),
            velocity_to_filter: get(p::VELOCITY_TO_FILTER),
            velocity_to_amp: get(p::VELOCITY_TO_AMP),
            attack_filter: get(p::ATTACK_FILTER),
            attack_vca: get(p::ATTACK_VCA),
            decay_filter: get(p::DECAY_FILTER),
            decay_vca: get(p::DECAY_VCA),
            sustain_filter: get(p::SUSTAIN_FILTER),
            sustain_vca: get(p::SUSTAIN_VCA),
            release_filter: get(p::RELEASE_FILTER),
            release_vca: get(p::RELEASE_VCA),
            release_switch: get(p::RELEASE_SWITCH),
            unison: get(p::UNISON),
            unison_voice_count: get(p::UNISON_VOICE_COUNT),
            unison_detune: get(p::UNISON_DETUNE),
            unison_notes: notes,
            pitch_wheel_range: get(p::PITCH_WHEEL_RANGE),
            retrigger_and_unison: get(p::RETRIGGER_AND_UNISON),
        }
    }

    /// The 128-byte program of these fields for `variant` (S53-33, S53-38;
    /// the server's packer, SRV-55): each field in its byte, FILTER REV
    /// SELECT from the kind, every reserved and padding byte 0.
    ///
    /// # Errors
    ///
    /// The first field above its parameter's maximum.
    pub fn program(&self, variant: Variant) -> Result<Program, ParameterError> {
        let mut bytes = [0; PROGRAM_BYTES];
        for (number, value) in
            self.pairs().into_iter().chain([(parameter::FILTER_REV_SELECT, variant.filter_revision())])
        {
            if let Some(byte) = bytes.get_mut(usize::from(number)) {
                *byte = value;
            }
        }
        let program = Program { bytes };
        program.check().map(|()| program)
    }
}

/// The server's packer of named fields into an edit buffer dump (S53-47,
/// SRV-55).
///
/// # Errors
///
/// The first field above its parameter's maximum.
pub fn pack_patch(fields: &Fields, variant: Variant) -> Result<[u8; EDIT_BUFFER_DUMP_BYTES], ParameterError> {
    fields.program(variant).map(|program| edit_buffer_dump(&program))
}

/// Whether a message is one this family acts on (S53-2; the server
/// document's SRV-24): note off and on, program change, pitch bend, the
/// control changes the family document names, and an edit buffer dump of
/// the right length. A program dump travels only in a bank (S53-28).
#[must_use]
pub fn accepts(message: &Message<'_>) -> bool {
    match message {
        Message::Channel(message) => match message.kind() {
            status::NOTE_OFF | status::NOTE_ON | status::PROGRAM_CHANGE | status::PITCH_BEND => true,
            status::CONTROL_CHANGE => accepts_controller(message.data1),
            _ => false,
        },
        Message::SystemExclusive(bytes) => matches!(read_sysex(bytes), Sysex::EditBuffer(_)),
    }
}

/// Whether the family acts on controller `number` (S53-2).
fn accepts_controller(number: u8) -> bool {
    matches!(
        number,
        controller::MODULATION
            | controller::DATA_ENTRY
            | controller::VOLUME
            | controller::BANK_SELECT_FINE
            | controller::DATA_ENTRY_FINE
            | controller::SUSTAIN
            | BRIGHTNESS
            | controller::DATA_INCREMENT..=controller::RPN_COARSE
            | controller::ALL_SOUND_OFF
            | controller::RESET_ALL_CONTROLLERS
            | controller::ALL_NOTES_OFF..=controller::POLY_ON
    ) || parameter_of_controller(number).is_some()
}

/// What a message asks of the instrument, once the interface has moved its
/// state ([`Part::take`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// Taken whole, with nothing for the instrument to do: a parameter
    /// number selected, a bank select.
    None,
    /// Counted as ignored; nothing changed (S53-2).
    PassedOver,
    /// A key down, velocity 1 to 127, which does not change the sound
    /// (S53-4).
    NoteOn {
        /// The key.
        key: u8,
    },
    /// A key up; a note on of velocity 0 is one (S53-4, S53-5).
    NoteOff {
        /// The key.
        key: u8,
    },
    /// The damper pedal, down at 64 to 127 (S53-14).
    Sustain {
        /// Down.
        down: bool,
    },
    /// Controller 123, or one of 124 to 127 (S53-15).
    AllNotesOff,
    /// Controller 120 (S53-20).
    AllSoundOff,
    /// Controller 121 (S53-16): the wheel 0, the pedal up, the pitch wheel
    /// centred.
    ResetAllControllers,
    /// A pitch bend, 14 bits (S53-8).
    PitchBend {
        /// 0 to 16,383.
        value: u16,
    },
    /// The pitch wheel range changed, by registered parameter 0 or by
    /// parameter 86 (S53-8): [`Part::bend_range_cents`].
    BendRange,
    /// Controller 7 (S53-10).
    Volume {
        /// 0 to 127.
        value: u8,
    },
    /// Controller 1 (S53-9).
    Modulation {
        /// 0 to 127.
        value: u8,
    },
    /// Controller 74 (S53-12).
    Brightness {
        /// 0 to 127.
        value: u8,
    },
    /// The edit buffer was replaced whole, by a program change or an edit
    /// buffer dump (S53-18, S53-29): [`Part::program`].
    Program {
        /// Applied, or passed over for a program the sound lacks
        /// (MIDI-59), or refused for a faulty one (MIDI-63): in either of
        /// those the default program stands.
        outcome: Outcome,
    },
    /// One parameter of the edit buffer changed (S53-21, S53-23).
    Parameter {
        /// Its number.
        number: u8,
    },
}

impl Change {
    /// What the receiver counts of it (MIDI-123).
    #[must_use]
    pub const fn outcome(self) -> Outcome {
        match self {
            Self::PassedOver => Outcome::PassedOver,
            Self::Program { outcome } => outcome,
            _ => Outcome::Applied,
        }
    }
}

/// The program a program change last chose, kept so that a restatement
/// can tell whether the edit buffer still is it (S53-46).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Chosen {
    group: u8,
    number: u8,
    program: Program,
}

/// The value of controller 32 that chooses a group: 1 to 10 for groups 0
/// to 9 (S53-13).
const fn bank_select_of(group: u8) -> u8 {
    group.saturating_add(1)
}

/// The value of controller 64 that a restatement writes for a pedal down.
const PEDAL_DOWN_VALUE: u8 = DATA_MAX;
const _: () = assert!(PEDAL_DOWN_VALUE >= PEDAL_DOWN);

/// The family's one part, as the interface moves it: the common layer's
/// state, the group, the edit buffer and the program last chosen, the
/// pitch wheel and its range, and the controllers a restatement carries
/// (S53-3: every channel addresses it). The receiver's instrument plays
/// through it, and the server keeps one for each sound it writes, as its
/// tracker (MIDI-94), and writes the part's restatement from it
/// ([`Part::restate`]).
#[derive(Clone, Debug)]
pub struct Part {
    variant: Variant,
    common: Common,
    group: u8,
    edit: Program,
    chosen: Option<Chosen>,
    /// The most significant seven bits of the last data entry, which
    /// controller 38 keeps (S53-23).
    data_coarse: u8,
    bend_range_cents: u16,
    bend: u16,
    volume: u8,
    modulation: u8,
    sustain: bool,
    brightness: u8,
}

impl Part {
    /// A part as a sound starts it, before [`Part::start`]: the default
    /// program, group 0, the range of seven semitones, the controllers at
    /// rest and the volume full.
    #[must_use]
    pub fn new(variant: Variant) -> Self {
        Self {
            variant,
            common: Common::new(CONFIG),
            group: 0,
            edit: Program::default_for(variant),
            chosen: None,
            data_coarse: 0,
            bend_range_cents: DEFAULT_BEND_RANGE_CENTS,
            bend: PITCH_BEND_CENTRE,
            volume: VOLUME_FULL,
            modulation: 0,
            sustain: false,
            brightness: 0,
        }
    }

    /// The sound's start (S53-18, MIDI-61): the edit buffer holds program
    /// 0 of bank index 0, or the default program where that bank lacks it
    /// or holds a faulty one ([`Outcome::PatchRefused`]).
    pub fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let (program, outcome) = match banks.get(0).map_or(Lookup::Missing, |image| lookup_image(image, 0)) {
            Lookup::Found(program) => (program, Outcome::Applied),
            Lookup::Faulty => (self.default_program(), Outcome::PatchRefused),
            Lookup::Missing => (self.default_program(), Outcome::Applied),
        };
        self.edit = program;
        self.chosen = None;
        outcome
    }

    /// The variant.
    #[must_use]
    pub const fn variant(&self) -> Variant {
        self.variant
    }

    /// The edit buffer: the program sounding (S53-18).
    #[must_use]
    pub const fn program(&self) -> &Program {
        &self.edit
    }

    /// The group the next program change chooses within (S53-13).
    #[must_use]
    pub const fn group(&self) -> u8 {
        self.group
    }

    /// The pitch wheel range, in cents (S53-8).
    #[must_use]
    pub const fn bend_range_cents(&self) -> u16 {
        self.bend_range_cents
    }

    /// The pitch wheel, 14 bits.
    #[must_use]
    pub const fn bend(&self) -> u16 {
        self.bend
    }

    /// Controller 7.
    #[must_use]
    pub const fn volume(&self) -> u8 {
        self.volume
    }

    /// Controller 1.
    #[must_use]
    pub const fn modulation(&self) -> u8 {
        self.modulation
    }

    /// The damper pedal.
    #[must_use]
    pub const fn sustain(&self) -> bool {
        self.sustain
    }

    /// Controller 74.
    #[must_use]
    pub const fn brightness(&self) -> u8 {
        self.brightness
    }

    /// The common layer's state.
    #[must_use]
    pub const fn common(&self) -> &Common {
        &self.common
    }

    const fn default_program(&self) -> Program {
        Program::default_for(self.variant)
    }

    /// Input was lost (S53-46, MIDI-102): the pedal up and no parameter
    /// selected; the edit buffer, the group, the wheels, brightness, the
    /// volume and the range stay. The instrument releases its notes.
    pub fn lost(&mut self) {
        self.common.lost();
        self.sustain = false;
        self.data_coarse = 0;
    }

    /// Takes one message, with the sound's banks for a program change, and
    /// answers what the instrument must do (S53-2).
    pub fn take(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Change {
        match message {
            Message::Channel(message) => self.channel_message(*message, banks),
            Message::SystemExclusive(bytes) => match read_sysex(bytes) {
                Sysex::EditBuffer(program) => {
                    self.edit = program;
                    self.chosen = None;
                    Change::Program { outcome: Outcome::Applied }
                }
                Sysex::FaultyEditBuffer => {
                    self.edit = self.default_program();
                    self.chosen = None;
                    Change::Program { outcome: Outcome::PatchRefused }
                }
                // A program dump travels only in a bank (S53-28).
                Sysex::ProgramDump { .. } | Sysex::FaultyProgramDump { .. } | Sysex::Other => Change::PassedOver,
            },
        }
    }

    fn channel_message(&mut self, message: ChannelMessage, banks: &Banks<'_>) -> Change {
        let channel = message.channel();
        let control = message.kind() == status::CONTROL_CHANGE;
        let number = message.data1;
        let selected = self.common.channel(channel).selected;
        // S53-23: registered parameter 0 takes no increment or decrement.
        if control
            && matches!(number, controller::DATA_INCREMENT | controller::DATA_DECREMENT)
            && selected == Selected::Registered(RPN_PITCH_BEND_SENSITIVITY)
        {
            return Change::PassedOver;
        }
        let selecting = control && (controller::NRPN_FINE..=controller::RPN_COARSE).contains(&number);
        if selecting {
            self.data_coarse = 0;
        }
        let entry = control
            && matches!(
                number,
                controller::DATA_ENTRY
                    | controller::DATA_ENTRY_FINE
                    | controller::DATA_INCREMENT
                    | controller::DATA_DECREMENT
            );
        match self.common.take(message).action {
            // A data entry with nothing selected (S53-23).
            Action::None if entry => Change::PassedOver,
            // S53-22: a registered parameter number other than 0 and the
            // null one selects nothing synth5x has.
            Action::None
                if selecting
                    && number >= controller::RPN_FINE
                    && matches!(
                        self.common.channel(channel).selected,
                        Selected::Registered(number) if number != RPN_PITCH_BEND_SENSITIVITY
                    ) =>
            {
                Change::PassedOver
            }
            Action::None => Change::None,
            Action::NoteOn { key, .. } => Change::NoteOn { key },
            Action::NoteOff { key, .. } => Change::NoteOff { key },
            Action::Sustain { down } => {
                self.sustain = down;
                Change::Sustain { down }
            }
            Action::AllNotesOff => Change::AllNotesOff,
            Action::AllSoundOff => Change::AllSoundOff,
            Action::ResetAllControllers => {
                self.modulation = 0;
                self.sustain = false;
                self.bend = PITCH_BEND_CENTRE;
                self.data_coarse = 0;
                Change::ResetAllControllers
            }
            Action::PitchBend { value } => {
                self.bend = value;
                Change::PitchBend { value }
            }
            Action::BendRange { cents } => {
                self.bend_range_cents = cents.min(MOST_BEND_RANGE_CENTS);
                Change::BendRange
            }
            Action::Volume { value } => {
                self.volume = value;
                Change::Volume { value }
            }
            Action::ProgramChange { program, .. } => self.program_change(program, banks),
            Action::NonRegistered { parameter, entry } => self.data_entry(parameter, entry),
            Action::Control { controller, value } => self.control(controller, value),
            // S53-7: no pressure; S53-22: no registered parameter but 0.
            Action::PassedOver
            | Action::ChannelPressure { .. }
            | Action::KeyPressure { .. }
            | Action::Registered { .. } => Change::PassedOver,
        }
    }

    fn control(&mut self, number: u8, value: u8) -> Change {
        match number {
            controller::MODULATION => {
                self.modulation = value;
                Change::Modulation { value }
            }
            controller::BANK_SELECT_FINE if value <= BANK_SELECT_MOST => {
                self.group = value.saturating_sub(1);
                Change::None
            }
            BRIGHTNESS => {
                self.brightness = value;
                Change::Brightness { value }
            }
            _ => match parameter_of_controller(number) {
                Some(parameter) => self.set_parameter(parameter, value),
                // Controllers 0 and 4, bank select above 10, and every
                // controller not named (S53-2, S53-11, S53-13).
                None => Change::PassedOver,
            },
        }
    }

    /// Sets one parameter of the edit buffer, held to its maximum; the
    /// pitch wheel range follows parameter 86 (S53-8).
    fn set_parameter(&mut self, number: u8, value: u8) -> Change {
        if !self.edit.set(number, value) {
            return Change::PassedOver;
        }
        if number == parameter::PITCH_WHEEL_RANGE {
            let semitones = u16::from(self.edit.get(number)).saturating_add(1);
            self.bend_range_cents = semitones.saturating_mul(CENTS_PER_SEMITONE).min(MOST_BEND_RANGE_CENTS);
            return Change::BendRange;
        }
        Change::Parameter { number }
    }

    /// A data entry, increment or decrement to non-registered parameter
    /// `number` (S53-23).
    fn data_entry(&mut self, number: u16, entry: DataEntry) -> Change {
        let (Ok(parameter), Some(most)) = (u8::try_from(number), maximum(number)) else { return Change::PassedOver };
        let current = self.edit.get(parameter);
        let seven = u16::from(DATA_MAX);
        let wanted = match entry {
            DataEntry::Coarse(value) => {
                self.data_coarse = value;
                u16::from(value) << 7
            }
            DataEntry::Fine(value) => u16::from(self.data_coarse) << 7 | (u16::from(value) & seven),
            DataEntry::Increment(_) => u16::from(current).saturating_add(1),
            DataEntry::Decrement(_) => u16::from(current).saturating_sub(1),
        };
        let value = u8::try_from(wanted.min(u16::from(most))).unwrap_or(most);
        self.set_parameter(parameter, value)
    }

    /// A program change (S53-18): program `number` of the bank of the
    /// group last chosen, or the default program.
    fn program_change(&mut self, number: u8, banks: &Banks<'_>) -> Change {
        let found = if number < GROUP_PROGRAMS { lookup(banks, self.group, number) } else { Lookup::Missing };
        let (program, outcome) = match found {
            Lookup::Found(program) => (program, Outcome::Applied),
            Lookup::Faulty => (self.default_program(), Outcome::PatchRefused),
            Lookup::Missing => (self.default_program(), Outcome::PassedOver),
        };
        self.edit = program;
        self.chosen = Some(Chosen { group: self.group, number, program });
        Change::Program { outcome }
    }

    /// Gives the part's restatement to `out` on `channel`, in S53-46's
    /// order, to be written at one tick of one block (MIDI-93 to MIDI-98):
    /// controller 32 for the present group; the program change that chose
    /// the edit buffer, where it still is that program and the group is the
    /// one it was chosen in, and otherwise an edit buffer dump of it; the
    /// range by registered parameter 0 where it is not the default; then
    /// the pitch bend, controllers 7, 1, 64 and 74 where each is not at its
    /// value at a sound's start.
    pub fn restate(&self, channel: u8, out: &mut dyn FnMut(Message<'_>)) {
        let channel = channel & crate::midi::CHANNEL_MASK;
        let cc = |out: &mut dyn FnMut(Message<'_>), number: u8, value: u8| {
            out(Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value)));
        };
        cc(out, controller::BANK_SELECT_FINE, bank_select_of(self.group));
        match self.chosen {
            Some(chosen) if chosen.group == self.group && chosen.program == self.edit => {
                out(Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, channel, chosen.number, 0)));
            }
            _ => {
                let dump = edit_buffer_dump(&self.edit);
                out(Message::SystemExclusive(dump.get(1..).unwrap_or(&[])));
            }
        }
        if self.bend_range_cents != DEFAULT_BEND_RANGE_CENTS {
            let semitones = self.bend_range_cents / CENTS_PER_SEMITONE;
            let cents = self.bend_range_cents % CENTS_PER_SEMITONE;
            let byte = |value: u16| u8::try_from(value).unwrap_or(DATA_MAX);
            cc(out, controller::RPN_COARSE, 0);
            cc(out, controller::RPN_FINE, 0);
            cc(out, controller::DATA_ENTRY, byte(semitones));
            cc(out, controller::DATA_ENTRY_FINE, byte(cents));
        }
        if self.bend != PITCH_BEND_CENTRE {
            let seven = u16::from(DATA_MAX);
            let low = u8::try_from(self.bend & seven).unwrap_or(0);
            let high = u8::try_from((self.bend >> 7) & seven).unwrap_or(0);
            out(Message::Channel(ChannelMessage::new(status::PITCH_BEND, channel, low, high)));
        }
        if self.volume != VOLUME_FULL {
            cc(out, controller::VOLUME, self.volume);
        }
        if self.modulation != 0 {
            cc(out, controller::MODULATION, self.modulation);
        }
        if self.sustain {
            cc(out, controller::SUSTAIN, PEDAL_DOWN_VALUE);
        }
        if self.brightness != 0 {
            cc(out, BRIGHTNESS, self.brightness);
        }
    }
}

#[cfg(test)]
mod tests;
