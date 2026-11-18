//! synthD's programming interface without its synthesis: the controller
//! map, the 60-byte patch, the bank image, which messages synthD takes, and
//! the tracker that writes its restatement (`neoteletext-common/docs/audio/
//! synthd.md`, cited here as "SD-n"; `server.md`, SRV-34 to SRV-36,
//! SRV-55, SRV-60, SRV-68, SRV-69 and SRV-88).
//!
//! Everything here is what the server needs to write and check synthD's
//! bytes; it holds no oscillator, filter or contour and builds behind the
//! crate's `interface` feature alone. The receiver's instrument
//! (`synthd::midi`) uses the same tables, so that the server's check is the
//! receiver's own (MIDI-21).
//!
//! # The patch
//!
//! A patch is the values of synthD's 60 panel controllers, one byte each,
//! 0 to 127, in the order of [`LAYOUT`] (SD-22): the Voyager's controllers
//! in ascending order of number, each fine controller after its coarse
//! controller, then NeoTeletext's own. Every 60 bytes of 0 to 127 is a
//! patch; nothing else is.

use crate::midi::common::{Action, CENTS_PER_SEMITONE, Common, Config, VOLUME_FULL};
use crate::midi::{
    Banks, ChannelMessage, DATA_MAX, MOST_BANK_PATCHES, Message, NULL_PARAMETER, Outcome, PITCH_BEND_CENTRE,
    RPN_PITCH_BEND_SENSITIVITY, controller, status,
};

/// Bytes of a patch: the 60 controllers of SD-22's table, 46 the
/// Voyager's and 14 NeoTeletext's own.
pub const PATCH_BYTES: usize = 60;
/// The Voyager's controllers among them, which come first (SD-22).
pub const VOYAGER_BYTES: usize = 46;
/// The most patches a bank holds: program changes 0 to 127 (SD-12, SD-27).
pub const MOST_PATCHES: usize = MOST_BANK_PATCHES;
/// The most bytes of a bank image: 128 patches of 60 bytes (SD-27).
pub const MOST_BANK_BYTES: usize = MOST_PATCHES * PATCH_BYTES;
const _: () = assert!(MOST_BANK_BYTES <= crate::midi::MOST_BANK_BYTES);

/// The pitch bend range before any registered parameter 0 message, in
/// cents: 200 ÷ 301 of an octave, the range of the instrument modelled
/// (SD-6 item 4).
pub const DEFAULT_BEND_RANGE_CENTS: u16 = 797;
/// The largest pitch bend range, in cents: 31 semitones, the largest of
/// the Voyager's ten choices (SD-6 item 2, section 13 item 11).
pub const MOST_BEND_RANGE_CENTS: u16 = 3_100;

/// synthD's figures for the common layer (SD-6, SD-12): no bank select
/// (controllers 0 and 32 counted as ignored, MIDI-64), and controllers 99
/// and 98 select a non-registered parameter, to which data entry then
/// changes nothing (SD-6 items 1 and 3).
pub const COMMON: Config = Config {
    bend_range_cents: DEFAULT_BEND_RANGE_CENTS,
    most_bend_range_cents: MOST_BEND_RANGE_CENTS,
    bank_select: false,
    non_registered: true,
};

/// What one byte of the patch is to its control (SD-18).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The most significant seven bits of a pair: a control change of it
    /// also sets the fine part to 0 (MIDI-75).
    Coarse,
    /// The least significant seven bits of a pair.
    Fine,
    /// A control of one controller.
    Single,
}

/// The patch's 60 bytes, in order: each byte's controller and its role
/// (SD-22's table; SD-17 tables A to E).
pub const LAYOUT: [(u8, Role); PATCH_BYTES] = {
    use Role::{Coarse, Fine, Single};
    [
        // Table A, the Voyager's pairs, in ascending order of number.
        (5, Coarse),  // GLIDE (2): GLIDE RATE
        (37, Fine),   //
        (10, Coarse), // Oscillator 2 FREQUENCY (7): OSC 2 FREQ
        (42, Fine),   //
        (12, Coarse), // Oscillator 3 FREQUENCY (10): OSC 3 FREQ, read 12 and 44 (section 6.2 item 5)
        (44, Fine),   //
        (14, Coarse), // EXTERNAL INPUT VOLUME (15): EXTERNAL
        (46, Fine),   //
        (15, Coarse), // Mixer VOLUME of oscillator 1 (12)
        (47, Fine),   //
        (16, Coarse), // Mixer VOLUME of oscillator 2 (13)
        (48, Fine),   //
        (17, Coarse), // Mixer VOLUME of oscillator 3 (14)
        (49, Fine),   //
        (18, Coarse), // NOISE VOLUME (16)
        (50, Fine),   //
        (19, Coarse), // CUTOFF FREQUENCY (17): CUTOFF
        (51, Fine),   //
        (21, Coarse), // EMPHASIS (18): RESONANCE
        (53, Fine),   //
        (23, Coarse), // Filter contour ATTACK TIME (20)
        (55, Fine),   //
        (24, Coarse), // Filter contour DECAY TIME (21)
        (56, Fine),   //
        (25, Coarse), // Filter contour SUSTAIN LEVEL (22)
        (57, Fine),   //
        (27, Coarse), // AMOUNT OF CONTOUR (19): AMOUNT TO FILTER, its upper half
        (59, Fine),   //
        (28, Coarse), // Loudness contour ATTACK TIME (23)
        (60, Fine),   //
        (29, Coarse), // Loudness contour DECAY TIME (24)
        (61, Fine),   //
        (30, Coarse), // Loudness contour SUSTAIN LEVEL (25)
        (62, Fine),   //
        // The Voyager's single controllers, tables C and E.
        (65, Single), // GLIDE switch (38): GLIDE SWITCH
        (74, Single), // Oscillator 1 RANGE (4), octave: OSC 1 OCTAVE
        (75, Single), // Oscillator 2 RANGE (6), octave: OSC 2 OCTAVE
        (76, Single), // Oscillator 3 RANGE (9), octave: OSC 3 OCTAVE
        (79, Single), // OSC. 3 CONTROL (28): 3 KB CONT
        (80, Single), // Oscillator 3 RANGE (9), LO/HI: 3 FREQ
        (81, Single), // Mixer switch of the external input (32)
        (82, Single), // Mixer switch of oscillator 1 (29)
        (83, Single), // Mixer switch of oscillator 2 (30)
        (84, Single), // Mixer switch of oscillator 3 (31)
        (85, Single), // Mixer switch of the noise (33)
        (88, Single), // DECAY switch (39): RELEASE SWITCH (section 6.2 item 7)
        // NeoTeletext's own, SD-17 rule (c).
        (89, Coarse),  // TUNE (1)
        (90, Fine),    //
        (102, Single), // MODULATION MIX (3)
        (103, Single), // Oscillator 1 RANGE (4), LO/HI
        (104, Single), // Oscillator 1 WAVEFORM (5)
        (105, Single), // Oscillator 2 RANGE (6), LO/HI
        (106, Single), // Oscillator 2 WAVEFORM (8)
        (107, Single), // Oscillator 3 WAVEFORM (11)
        (108, Single), // Main output VOLUME (26)
        (109, Single), // OSCILLATOR MODULATION (27)
        (110, Single), // Noise WHITE / PINK (34)
        (111, Single), // FILTER MODULATION (35)
        (112, Single), // KEYBOARD CONTROL 1 (36)
        (113, Single), // KEYBOARD CONTROL 2 (37)
    ]
};

/// Where each control's byte, or its pair's coarse byte, lies in the patch
/// (SD-22's table), named as SRV-34 names its field.
pub mod byte {
    /// GLIDE, a pair.
    pub const GLIDE: usize = 0;
    /// Oscillator 2 FREQUENCY, a pair.
    pub const OSCILLATOR_2_FREQUENCY: usize = 2;
    /// Oscillator 3 FREQUENCY, a pair.
    pub const OSCILLATOR_3_FREQUENCY: usize = 4;
    /// EXTERNAL INPUT VOLUME, a pair.
    pub const EXTERNAL_INPUT_VOLUME: usize = 6;
    /// Mixer VOLUME of oscillator 1, a pair.
    pub const OSCILLATOR_1_VOLUME: usize = 8;
    /// Mixer VOLUME of oscillator 2, a pair.
    pub const OSCILLATOR_2_VOLUME: usize = 10;
    /// Mixer VOLUME of oscillator 3, a pair.
    pub const OSCILLATOR_3_VOLUME: usize = 12;
    /// NOISE VOLUME, a pair.
    pub const NOISE_VOLUME: usize = 14;
    /// CUTOFF FREQUENCY, a pair.
    pub const CUTOFF_FREQUENCY: usize = 16;
    /// EMPHASIS, a pair.
    pub const EMPHASIS: usize = 18;
    /// Filter contour ATTACK TIME, a pair.
    pub const FILTER_ATTACK_TIME: usize = 20;
    /// Filter contour DECAY TIME, a pair.
    pub const FILTER_DECAY_TIME: usize = 22;
    /// Filter contour SUSTAIN LEVEL, a pair.
    pub const FILTER_SUSTAIN_LEVEL: usize = 24;
    /// AMOUNT OF CONTOUR, a pair.
    pub const AMOUNT_OF_CONTOUR: usize = 26;
    /// Loudness contour ATTACK TIME, a pair.
    pub const LOUDNESS_ATTACK_TIME: usize = 28;
    /// Loudness contour DECAY TIME, a pair.
    pub const LOUDNESS_DECAY_TIME: usize = 30;
    /// Loudness contour SUSTAIN LEVEL, a pair.
    pub const LOUDNESS_SUSTAIN_LEVEL: usize = 32;
    /// The GLIDE switch, controller 65.
    pub const GLIDE_SWITCH: usize = 34;
    /// Oscillator 1 RANGE's octave.
    pub const OSCILLATOR_1_RANGE_OCTAVE: usize = 35;
    /// Oscillator 2 RANGE's octave.
    pub const OSCILLATOR_2_RANGE_OCTAVE: usize = 36;
    /// Oscillator 3 RANGE's octave.
    pub const OSCILLATOR_3_RANGE_OCTAVE: usize = 37;
    /// OSC. 3 CONTROL.
    pub const OSCILLATOR_3_CONTROL: usize = 38;
    /// Oscillator 3 RANGE's LO or HI.
    pub const OSCILLATOR_3_RANGE_LO_HI: usize = 39;
    /// The mixer's switch of the external input.
    pub const EXTERNAL_INPUT_SWITCH: usize = 40;
    /// The mixer's switch of oscillator 1.
    pub const OSCILLATOR_1_SWITCH: usize = 41;
    /// The mixer's switch of oscillator 2.
    pub const OSCILLATOR_2_SWITCH: usize = 42;
    /// The mixer's switch of oscillator 3.
    pub const OSCILLATOR_3_SWITCH: usize = 43;
    /// The mixer's switch of the noise.
    pub const NOISE_SWITCH: usize = 44;
    /// The DECAY switch.
    pub const DECAY_SWITCH: usize = 45;
    /// TUNE, a pair.
    pub const TUNE: usize = 46;
    /// MODULATION MIX.
    pub const MODULATION_MIX: usize = 48;
    /// Oscillator 1 RANGE's LO or HI.
    pub const OSCILLATOR_1_RANGE_LO_HI: usize = 49;
    /// Oscillator 1 WAVEFORM.
    pub const OSCILLATOR_1_WAVEFORM: usize = 50;
    /// Oscillator 2 RANGE's LO or HI.
    pub const OSCILLATOR_2_RANGE_LO_HI: usize = 51;
    /// Oscillator 2 WAVEFORM.
    pub const OSCILLATOR_2_WAVEFORM: usize = 52;
    /// Oscillator 3 WAVEFORM.
    pub const OSCILLATOR_3_WAVEFORM: usize = 53;
    /// The main output VOLUME.
    pub const MAIN_OUTPUT_VOLUME: usize = 54;
    /// OSCILLATOR MODULATION.
    pub const OSCILLATOR_MODULATION: usize = 55;
    /// Noise WHITE / PINK.
    pub const NOISE_COLOUR: usize = 56;
    /// FILTER MODULATION.
    pub const FILTER_MODULATION: usize = 57;
    /// KEYBOARD CONTROL 1.
    pub const KEYBOARD_CONTROL_1: usize = 58;
    /// KEYBOARD CONTROL 2.
    pub const KEYBOARD_CONTROL_2: usize = 59;
}

/// The modulation wheel's fine controller (SD-9; MIDI 1.0 4.2, Table III,
/// PDF 102: controller 1 plus 32).
pub const MODULATION_FINE: u8 = 33;
/// The fine part of the Voyager's MASTER VOLUME, counted as ignored
/// (SD-10, SD-17 table F).
pub const VOLUME_FINE: u8 = 39;

/// The Voyager's controllers for controls synthD lacks, each counted as
/// ignored and refused by the server (SD-1, SD-17 table F).
pub const IGNORED_VOYAGER: [u8; 34] = [
    2,
    34, // MOD 1
    3,
    35, // LFO RATE
    4,
    36, // MOD 2
    8,
    40, // PEDAL AMOUNT
    9,
    41, // OSC 1 WAVE
    11,
    43, // OSC 2 WAVE
    13,
    45, // OSC 3 WAVE
    20,
    52, // Filter SPACING
    22,
    54, // Filter KB AMOUNT
    26,
    58, // FILTER ENV RELEASE
    31,
    63,          // VOL ENV RELEASE
    VOLUME_FINE, // MASTER VOLUME's fine part (SD-10)
    67,          // LFO SYNC
    68,
    69,
    70, // MW SOURCE, DESTINATION and SHAPING
    71,
    72,
    73, // PEDAL SOURCE, DEST. and SHAPING
    77, // 1-2 SYNC
    78, // 3-1 FM
    86, // Filter MODE
    87, // ENV GATE SWITCH
];

/// The controllers of NeoTeletext's own, SD-17 rule (c): taken from those
/// MIDI 1.0 4.2's Table III leaves undefined (85 to 90 and 102 to 119
/// among those above 31) less those the Voyager's map uses (85 to 88).
const OWN_CONTROLLERS: [u8; 14] = [89, 90, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113];

/// Whether `number` is undefined in MIDI 1.0 4.2's Table III (PDF 102)
/// above 63: 85 to 90 and 102 to 119.
const fn undefined_in_table_iii(number: u8) -> bool {
    matches!(number, 85..=90 | 102..=119)
}

/// The Voyager's controllers synthD takes (SD-17 rule a) or counts as
/// ignored (table F), and 6, 38 (its MW AMOUNT): every number its map uses.
const fn on_voyager_map(number: u8) -> bool {
    matches!(number, 1..=31 | 33..=63 | 65 | 67..=88)
}

// SD-17's verification: no controller appears twice among the patch's,
// the common layer's and table F's; every number of rule (c) is one Table
// III leaves undefined and the Voyager's map does not use; the Voyager's
// come first and NeoTeletext's after, each in ascending order; a coarse
// byte is followed by its fine byte.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const _: () = {
    let common = [
        controller::MODULATION,
        MODULATION_FINE,
        controller::DATA_ENTRY,
        controller::DATA_ENTRY_FINE,
        controller::VOLUME,
        controller::SUSTAIN,
        controller::DATA_INCREMENT,
        controller::DATA_DECREMENT,
        controller::NRPN_FINE,
        controller::NRPN_COARSE,
        controller::RPN_FINE,
        controller::RPN_COARSE,
    ];
    let mut seen = [false; 128];
    let mut last = 0;
    let mut index = 0;
    while index < PATCH_BYTES {
        let (number, role) = LAYOUT[index];
        assert!(!seen[number as usize]);
        seen[number as usize] = true;
        // Ascending by the coarse or single controller, each fine after
        // its coarse (SD-22).
        if !matches!(role, Role::Fine) {
            assert!(index == 0 || last < number);
            last = number;
        }
        if index < VOYAGER_BYTES {
            assert!(on_voyager_map(number));
        } else {
            assert!(undefined_in_table_iii(number) && !on_voyager_map(number));
            assert!(OWN_CONTROLLERS[index - VOYAGER_BYTES] == number);
        }
        match role {
            Role::Coarse => {
                assert!(index + 1 < PATCH_BYTES);
                assert!(matches!(LAYOUT[index + 1].1, Role::Fine));
                // MIDI 1.0's pairing for the Voyager's, two consecutive
                // numbers for NeoTeletext's TUNE (SD-17 rule c).
                let fine = LAYOUT[index + 1].0;
                assert!(fine == number + 32 || (index >= VOYAGER_BYTES && fine == number + 1));
            }
            Role::Fine => assert!(index > 0 && matches!(LAYOUT[index - 1].1, Role::Coarse)),
            Role::Single => {}
        }
        index += 1;
    }
    let mut index = 0;
    while index < common.len() {
        assert!(!seen[common[index] as usize]);
        seen[common[index] as usize] = true;
        index += 1;
    }
    let mut index = 0;
    while index < IGNORED_VOYAGER.len() {
        assert!(!seen[IGNORED_VOYAGER[index] as usize] && on_voyager_map(IGNORED_VOYAGER[index]));
        seen[IGNORED_VOYAGER[index] as usize] = true;
        index += 1;
    }
    // Every controller of the Voyager's map is then placed: taken, the
    // common layer's, or ignored.
    let mut number = 0;
    while number < 128 {
        assert!(!on_voyager_map(number) || seen[number as usize]);
        number += 1;
    }
};

/// The default patch's 60 values (SD-21; SD-22's last column).
pub const DEFAULT_PATCH: [u8; PATCH_BYTES] = [
    0, 0, // GLIDE 0
    64, 0, // oscillator 2 FREQUENCY 0
    64, 0, // oscillator 3 FREQUENCY 0
    0, 0, // EXTERNAL INPUT VOLUME 0
    127, 127, // oscillator 1 VOLUME 100
    127, 127, // oscillator 2 VOLUME 100
    127, 127, // oscillator 3 VOLUME 100
    0, 0, // NOISE VOLUME 0
    127, 127, // CUTOFF FREQUENCY +500
    0, 0, // EMPHASIS 0
    0, 0, // filter ATTACK TIME 0
    0, 0, // filter DECAY TIME 0
    127, 127, // filter SUSTAIN LEVEL 100
    64, 0, // AMOUNT OF CONTOUR 0
    0, 0, // loudness ATTACK TIME 0
    0, 0, // loudness DECAY TIME 0
    127, 127, // loudness SUSTAIN LEVEL 100
    0,   // GLIDE switch off
    32,  // oscillator 1 at 8'
    32,  // oscillator 2 at 8'
    32,  // oscillator 3's octave 8', not heard while LO
    127, // OSC. 3 CONTROL on
    0,   // oscillator 3 LO
    0,   // external input off
    127, // oscillator 1 on
    0,   // oscillator 2 off
    0,   // oscillator 3 off
    0,   // noise off
    0,   // DECAY off
    64, 0,   // TUNE 0
    0,   // MODULATION MIX 0
    127, // oscillator 1 HI
    43,  // oscillator 1 sawtooth
    127, // oscillator 2 HI
    43,  // oscillator 2 sawtooth
    0,   // oscillator 3 triangular
    127, // main output VOLUME 100
    0,   // OSCILLATOR MODULATION off
    0,   // white
    0,   // FILTER MODULATION off
    0,   // KEYBOARD CONTROL 1 off
    0,   // KEYBOARD CONTROL 2 off
];

/// The byte of the patch that controller `number` sets, with its role, or
/// `None` for a controller that is no panel control's.
#[must_use]
pub fn place(number: u8) -> Option<(usize, Role)> {
    LAYOUT.iter().position(|&(found, _)| found == number).and_then(|at| LAYOUT.get(at).map(|&(_, role)| (at, role)))
}

/// The bytes of the pair or single control byte `at` belongs to: a pair's
/// coarse and fine bytes, or the one byte twice.
fn control_bytes(at: usize) -> (usize, usize) {
    match LAYOUT.get(at).map(|&(_, role)| role) {
        Some(Role::Coarse) => (at, at.saturating_add(1)),
        Some(Role::Fine) => (at.saturating_sub(1), at),
        _ => (at, at),
    }
}

/// Sets one panel controller's value in `values`, as its control change
/// does (SD-17, SD-18): a coarse controller sets its byte and clears its
/// fine byte (MIDI-75), a fine or single controller sets its byte alone.
/// Answers `None` for a controller that is no panel control's, and
/// otherwise whether a byte changed.
pub fn set_value(values: &mut [u8; PATCH_BYTES], number: u8, value: u8) -> Option<bool> {
    let (at, role) = place(number)?;
    let value = value & DATA_MAX;
    let mut changed = false;
    if let Some(byte) = values.get_mut(at) {
        changed |= *byte != value;
        *byte = value;
    }
    if role == Role::Coarse
        && let Some(fine) = values.get_mut(at.saturating_add(1))
    {
        changed |= *fine != 0;
        *fine = 0;
    }
    Some(changed)
}

/// Why bytes are not a synthD patch (SD-22, MIDI-63).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchFault {
    /// The patch is not 60 bytes long: its length.
    Length(usize),
    /// A byte is above 127.
    Byte {
        /// Its place, 0 to 59.
        at: usize,
        /// Its value.
        value: u8,
    },
}

/// Checks a patch's bytes and gives its 60 values (SD-22).
///
/// # Errors
///
/// [`PatchFault::Length`] for a patch not of 60 bytes;
/// [`PatchFault::Byte`] for the first byte above 127.
pub fn check_values(bytes: &[u8]) -> Result<[u8; PATCH_BYTES], PatchFault> {
    let values: [u8; PATCH_BYTES] = bytes.try_into().map_err(|_| PatchFault::Length(bytes.len()))?;
    match values.iter().position(|&value| value > DATA_MAX) {
        Some(at) => Err(PatchFault::Byte { at, value: values.get(at).copied().unwrap_or(0) }),
        None => Ok(values),
    }
}

/// A synthD patch as named fields, each the value of its controller or
/// pair as the interface carries it (SRV-34, `SynthDPanel`): a pair is 0
/// to 16,383 (`coarse × 128 + fine`, SD-18), every other field 0 to 127.
/// What a value means is SD-18, SD-19 and SD-20's law; [`super::Patch`]
/// holds the same panel in its own units.
#[expect(missing_docs, reason = "each field is named as the control of SD-22's table it holds, in its order")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    pub glide: u16,
    pub oscillator_2_frequency: u16,
    pub oscillator_3_frequency: u16,
    pub external_input_volume: u16,
    pub oscillator_1_volume: u16,
    pub oscillator_2_volume: u16,
    pub oscillator_3_volume: u16,
    pub noise_volume: u16,
    pub cutoff_frequency: u16,
    pub emphasis: u16,
    pub filter_attack_time: u16,
    pub filter_decay_time: u16,
    pub filter_sustain_level: u16,
    pub amount_of_contour: u16,
    pub loudness_attack_time: u16,
    pub loudness_decay_time: u16,
    pub loudness_sustain_level: u16,
    pub glide_switch: u8,
    pub oscillator_1_range_octave: u8,
    pub oscillator_2_range_octave: u8,
    pub oscillator_3_range_octave: u8,
    pub oscillator_3_control: u8,
    pub oscillator_3_range_lo_hi: u8,
    pub external_input_switch: u8,
    pub oscillator_1_switch: u8,
    pub oscillator_2_switch: u8,
    pub oscillator_3_switch: u8,
    pub noise_switch: u8,
    pub decay_switch: u8,
    pub tune: u16,
    pub modulation_mix: u8,
    pub oscillator_1_range_lo_hi: u8,
    pub oscillator_1_waveform: u8,
    pub oscillator_2_range_lo_hi: u8,
    pub oscillator_2_waveform: u8,
    pub oscillator_3_waveform: u8,
    pub main_output_volume: u8,
    pub oscillator_modulation: u8,
    pub noise_colour: u8,
    pub filter_modulation: u8,
    pub keyboard_control_1: u8,
    pub keyboard_control_2: u8,
}

/// The largest value of a pair: 14 bits.
pub const PAIR_MAX: u16 = 0x3FFF;

/// A pair's 14-bit number from its two bytes at `at`.
#[must_use]
pub fn pair(values: &[u8; PATCH_BYTES], at: usize) -> u16 {
    let byte = |at: usize| u16::from(values.get(at).copied().unwrap_or(0) & DATA_MAX);
    byte(at) << 7 | byte(at.saturating_add(1))
}

/// A pair's two bytes, coarse then fine: `floor(value ÷ 128)` and
/// `value mod 128` (SRV-34), a value above 16,383 held to it.
#[must_use]
pub fn pair_bytes(value: u16) -> [u8; 2] {
    let value = value.min(PAIR_MAX);
    let seven = u16::from(DATA_MAX);
    [u8::try_from(value >> 7).unwrap_or(DATA_MAX), u8::try_from(value & seven).unwrap_or(0)]
}

impl Patch {
    /// The default patch of SD-21.
    #[must_use]
    pub fn default_patch() -> Self {
        Self::from_values(&DEFAULT_PATCH)
    }

    /// The named fields of 60 values (SRV-34).
    #[must_use]
    pub fn from_values(values: &[u8; PATCH_BYTES]) -> Self {
        let word = |at: usize| pair(values, at);
        let single = |at: usize| values.get(at).copied().unwrap_or(0) & DATA_MAX;
        Self {
            glide: word(byte::GLIDE),
            oscillator_2_frequency: word(byte::OSCILLATOR_2_FREQUENCY),
            oscillator_3_frequency: word(byte::OSCILLATOR_3_FREQUENCY),
            external_input_volume: word(byte::EXTERNAL_INPUT_VOLUME),
            oscillator_1_volume: word(byte::OSCILLATOR_1_VOLUME),
            oscillator_2_volume: word(byte::OSCILLATOR_2_VOLUME),
            oscillator_3_volume: word(byte::OSCILLATOR_3_VOLUME),
            noise_volume: word(byte::NOISE_VOLUME),
            cutoff_frequency: word(byte::CUTOFF_FREQUENCY),
            emphasis: word(byte::EMPHASIS),
            filter_attack_time: word(byte::FILTER_ATTACK_TIME),
            filter_decay_time: word(byte::FILTER_DECAY_TIME),
            filter_sustain_level: word(byte::FILTER_SUSTAIN_LEVEL),
            amount_of_contour: word(byte::AMOUNT_OF_CONTOUR),
            loudness_attack_time: word(byte::LOUDNESS_ATTACK_TIME),
            loudness_decay_time: word(byte::LOUDNESS_DECAY_TIME),
            loudness_sustain_level: word(byte::LOUDNESS_SUSTAIN_LEVEL),
            glide_switch: single(byte::GLIDE_SWITCH),
            oscillator_1_range_octave: single(byte::OSCILLATOR_1_RANGE_OCTAVE),
            oscillator_2_range_octave: single(byte::OSCILLATOR_2_RANGE_OCTAVE),
            oscillator_3_range_octave: single(byte::OSCILLATOR_3_RANGE_OCTAVE),
            oscillator_3_control: single(byte::OSCILLATOR_3_CONTROL),
            oscillator_3_range_lo_hi: single(byte::OSCILLATOR_3_RANGE_LO_HI),
            external_input_switch: single(byte::EXTERNAL_INPUT_SWITCH),
            oscillator_1_switch: single(byte::OSCILLATOR_1_SWITCH),
            oscillator_2_switch: single(byte::OSCILLATOR_2_SWITCH),
            oscillator_3_switch: single(byte::OSCILLATOR_3_SWITCH),
            noise_switch: single(byte::NOISE_SWITCH),
            decay_switch: single(byte::DECAY_SWITCH),
            tune: word(byte::TUNE),
            modulation_mix: single(byte::MODULATION_MIX),
            oscillator_1_range_lo_hi: single(byte::OSCILLATOR_1_RANGE_LO_HI),
            oscillator_1_waveform: single(byte::OSCILLATOR_1_WAVEFORM),
            oscillator_2_range_lo_hi: single(byte::OSCILLATOR_2_RANGE_LO_HI),
            oscillator_2_waveform: single(byte::OSCILLATOR_2_WAVEFORM),
            oscillator_3_waveform: single(byte::OSCILLATOR_3_WAVEFORM),
            main_output_volume: single(byte::MAIN_OUTPUT_VOLUME),
            oscillator_modulation: single(byte::OSCILLATOR_MODULATION),
            noise_colour: single(byte::NOISE_COLOUR),
            filter_modulation: single(byte::FILTER_MODULATION),
            keyboard_control_1: single(byte::KEYBOARD_CONTROL_1),
            keyboard_control_2: single(byte::KEYBOARD_CONTROL_2),
        }
    }
}

/// Packs a patch's named fields into its 60 bytes, in SD-22's order
/// (SRV-34, SRV-55): each pair as `floor(value ÷ 128)` then `value mod
/// 128`, each single as its value. A field above its top (16,383 for a
/// pair, 127 for a single) is held to it, so that the bytes are always a
/// patch.
#[must_use]
pub fn pack_patch(patch: &Patch) -> [u8; PATCH_BYTES] {
    let mut values = [0u8; PATCH_BYTES];
    let mut word = |at: usize, value: u16| {
        let [coarse, fine] = pair_bytes(value);
        if let Some(place) = values.get_mut(at) {
            *place = coarse;
        }
        if let Some(place) = values.get_mut(at.saturating_add(1)) {
            *place = fine;
        }
    };
    word(byte::GLIDE, patch.glide);
    word(byte::OSCILLATOR_2_FREQUENCY, patch.oscillator_2_frequency);
    word(byte::OSCILLATOR_3_FREQUENCY, patch.oscillator_3_frequency);
    word(byte::EXTERNAL_INPUT_VOLUME, patch.external_input_volume);
    word(byte::OSCILLATOR_1_VOLUME, patch.oscillator_1_volume);
    word(byte::OSCILLATOR_2_VOLUME, patch.oscillator_2_volume);
    word(byte::OSCILLATOR_3_VOLUME, patch.oscillator_3_volume);
    word(byte::NOISE_VOLUME, patch.noise_volume);
    word(byte::CUTOFF_FREQUENCY, patch.cutoff_frequency);
    word(byte::EMPHASIS, patch.emphasis);
    word(byte::FILTER_ATTACK_TIME, patch.filter_attack_time);
    word(byte::FILTER_DECAY_TIME, patch.filter_decay_time);
    word(byte::FILTER_SUSTAIN_LEVEL, patch.filter_sustain_level);
    word(byte::AMOUNT_OF_CONTOUR, patch.amount_of_contour);
    word(byte::LOUDNESS_ATTACK_TIME, patch.loudness_attack_time);
    word(byte::LOUDNESS_DECAY_TIME, patch.loudness_decay_time);
    word(byte::LOUDNESS_SUSTAIN_LEVEL, patch.loudness_sustain_level);
    word(byte::TUNE, patch.tune);
    let singles = [
        (byte::GLIDE_SWITCH, patch.glide_switch),
        (byte::OSCILLATOR_1_RANGE_OCTAVE, patch.oscillator_1_range_octave),
        (byte::OSCILLATOR_2_RANGE_OCTAVE, patch.oscillator_2_range_octave),
        (byte::OSCILLATOR_3_RANGE_OCTAVE, patch.oscillator_3_range_octave),
        (byte::OSCILLATOR_3_CONTROL, patch.oscillator_3_control),
        (byte::OSCILLATOR_3_RANGE_LO_HI, patch.oscillator_3_range_lo_hi),
        (byte::EXTERNAL_INPUT_SWITCH, patch.external_input_switch),
        (byte::OSCILLATOR_1_SWITCH, patch.oscillator_1_switch),
        (byte::OSCILLATOR_2_SWITCH, patch.oscillator_2_switch),
        (byte::OSCILLATOR_3_SWITCH, patch.oscillator_3_switch),
        (byte::NOISE_SWITCH, patch.noise_switch),
        (byte::DECAY_SWITCH, patch.decay_switch),
        (byte::MODULATION_MIX, patch.modulation_mix),
        (byte::OSCILLATOR_1_RANGE_LO_HI, patch.oscillator_1_range_lo_hi),
        (byte::OSCILLATOR_1_WAVEFORM, patch.oscillator_1_waveform),
        (byte::OSCILLATOR_2_RANGE_LO_HI, patch.oscillator_2_range_lo_hi),
        (byte::OSCILLATOR_2_WAVEFORM, patch.oscillator_2_waveform),
        (byte::OSCILLATOR_3_WAVEFORM, patch.oscillator_3_waveform),
        (byte::MAIN_OUTPUT_VOLUME, patch.main_output_volume),
        (byte::OSCILLATOR_MODULATION, patch.oscillator_modulation),
        (byte::NOISE_COLOUR, patch.noise_colour),
        (byte::FILTER_MODULATION, patch.filter_modulation),
        (byte::KEYBOARD_CONTROL_1, patch.keyboard_control_1),
        (byte::KEYBOARD_CONTROL_2, patch.keyboard_control_2),
    ];
    for (at, value) in singles {
        if let Some(place) = values.get_mut(at) {
            *place = value.min(DATA_MAX);
        }
    }
    values
}

/// Checks a patch's bytes, as the receiver does (SD-22, SRV-35), and gives
/// its named fields.
///
/// # Errors
///
/// As [`check_values`].
pub fn check_patch(bytes: &[u8]) -> Result<Patch, PatchFault> {
    check_values(bytes).map(|values| Patch::from_values(&values))
}

/// Why bytes or patches are not a synthD bank (SD-27, SRV-35, SRV-60).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankFault {
    /// No patch: a bank holds 1 to 128.
    Empty,
    /// More patches than 128: how many.
    TooManyPatches(usize),
    /// An image whose length is not a whole number of patches, or is
    /// longer than 128 of them: its length.
    Length(usize),
    /// A patch's byte is above 127.
    Byte {
        /// The patch, its program number.
        program: usize,
        /// The byte within it, 0 to 59.
        at: usize,
        /// Its value.
        value: u8,
    },
    /// The buffer given is shorter than the image: the bytes it needs.
    Room(usize),
}

/// Builds a bank image from 1 to 128 patches: patch k at byte `k × 60`
/// (SD-27, MIDI-56 item 2). Answers the image's length in `out`.
///
/// # Errors
///
/// [`BankFault::Empty`] or [`BankFault::TooManyPatches`] for a count
/// outside 1 to 128; [`BankFault::Room`] where `out` is shorter than the
/// image.
pub fn build_bank(patches: &[Patch], out: &mut [u8]) -> Result<usize, BankFault> {
    if patches.is_empty() {
        return Err(BankFault::Empty);
    }
    if patches.len() > MOST_PATCHES {
        return Err(BankFault::TooManyPatches(patches.len()));
    }
    let length = patches.len().saturating_mul(PATCH_BYTES);
    let image = out.get_mut(..length).ok_or(BankFault::Room(length))?;
    let (places, _) = image.as_chunks_mut::<PATCH_BYTES>();
    for (place, patch) in places.iter_mut().zip(patches) {
        *place = pack_patch(patch);
    }
    Ok(length)
}

/// Checks a bank image as the receiver reads it (SD-27, MIDI-53) and
/// answers how many patches it holds.
///
/// # Errors
///
/// [`BankFault::Empty`] for no bytes; [`BankFault::Length`] for a length
/// that is not a whole number of patches or is above 7,680;
/// [`BankFault::Byte`] for the first byte above 127.
pub fn check_bank(image: &[u8]) -> Result<usize, BankFault> {
    if image.is_empty() {
        return Err(BankFault::Empty);
    }
    if !image.len().is_multiple_of(PATCH_BYTES) || image.len() > MOST_BANK_BYTES {
        return Err(BankFault::Length(image.len()));
    }
    let (patches, _) = image.as_chunks::<PATCH_BYTES>();
    for (program, patch) in patches.iter().enumerate() {
        if let Err(PatchFault::Byte { at, value }) = check_values(patch) {
            return Err(BankFault::Byte { program, at, value });
        }
    }
    Ok(image.len() / PATCH_BYTES)
}

/// The values of patch `program` of bank index 0 (SD-12): the default
/// patch where the bank holds none there ([`Outcome::PassedOver`],
/// MIDI-59) or it is faulty ([`Outcome::PatchRefused`], MIDI-63).
#[must_use]
pub fn bank_patch(banks: &Banks<'_>, program: u8) -> ([u8; PATCH_BYTES], Outcome) {
    match banks.fixed_patch(0, usize::from(program), PATCH_BYTES) {
        None => (DEFAULT_PATCH, Outcome::PassedOver),
        Some(bytes) => match check_values(bytes) {
            Ok(values) => (values, Outcome::Applied),
            Err(_) => (DEFAULT_PATCH, Outcome::PatchRefused),
        },
    }
}

/// Whether synthD acts on a control change of `number` itself, as a panel
/// control (SD-17) or the modulation wheel (SD-9), beside the common
/// layer's controllers.
#[must_use]
pub fn takes_controller(number: u8) -> bool {
    number == controller::MODULATION || number == MODULATION_FINE || place(number).is_some()
}

/// Whether synthD takes `message`, which the server checks every event
/// against (SD-1, SRV-24): note off and on, program change, pitch bend,
/// and control changes of the panel controllers of SD-17, the modulation
/// wheel's 1 and 33, volume 7, the pedal 64, the registered parameter
/// controllers 6, 38 and 96 to 101, and 120, 121 and 123 to 127. Every
/// other message, among them every system exclusive message, key and
/// channel pressure, and table F's controllers, is not taken.
#[must_use]
pub fn accepts(message: &Message<'_>) -> bool {
    let Message::Channel(message) = message else { return false };
    if message.data1 > DATA_MAX || message.data2 > DATA_MAX {
        return false;
    }
    match message.kind() {
        status::NOTE_OFF | status::NOTE_ON | status::PROGRAM_CHANGE | status::PITCH_BEND => true,
        status::CONTROL_CHANGE => {
            let number = message.data1;
            takes_controller(number)
                || matches!(
                    number,
                    controller::DATA_ENTRY
                        | controller::VOLUME
                        | controller::DATA_ENTRY_FINE
                        | controller::SUSTAIN
                        | controller::DATA_INCREMENT..=controller::RPN_COARSE
                        | controller::ALL_SOUND_OFF
                        | controller::RESET_ALL_CONTROLLERS
                        | controller::ALL_NOTES_OFF..=controller::POLY_ON
                )
        }
        _ => false,
    }
}

/// Gives a patch's 60 values as the 60 control changes it stands for, in
/// the patch's order, on `channel` (SD-26).
pub fn control_changes(values: &[u8; PATCH_BYTES], channel: u8, out: &mut dyn FnMut(Message<'_>)) {
    for (&(number, _), &value) in LAYOUT.iter().zip(values) {
        out(Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value)));
    }
}

/// A message on channel 0: synthD's sixteen channels are one keyboard and
/// one panel (SD-2), so its state is kept once.
#[must_use]
pub const fn on_one_channel(message: ChannelMessage) -> ChannelMessage {
    ChannelMessage::new(message.kind(), 0, message.data1, message.data2)
}

/// All 60 bytes moved.
const ALL_MOVED: u64 = (1 << PATCH_BYTES) - 1;

/// The sender's copy of a synthD sound's state, kept from every message
/// the server writes (SRV-68, MIDI-94), and the restatement SD-28 states
/// (SRV-69). It is bounded: one panel, one common layer of 16 channels.
#[derive(Clone, Debug)]
pub struct Tracker {
    common: Common,
    values: [u8; PATCH_BYTES],
    program: Option<u8>,
    /// The bytes of the panel moved by a control change since the last
    /// program change, one bit each; both bytes of a pair together.
    moved: u64,
    /// The modulation wheel's 14-bit number (SD-9).
    wheel: u16,
}

impl Tracker {
    /// A sound at its start, at patch 0 of bank index 0 (SD-12, MIDI-61).
    #[must_use]
    pub fn new(banks: &Banks<'_>) -> Self {
        let (values, _) = bank_patch(banks, 0);
        Self { common: Common::new(COMMON), values, program: None, moved: 0, wheel: 0 }
    }

    /// The panel's values as the receiver now holds them.
    #[must_use]
    pub const fn values(&self) -> &[u8; PATCH_BYTES] {
        &self.values
    }

    /// The pitch bend range, in cents (SD-6).
    #[must_use]
    pub fn bend_range_cents(&self) -> u16 {
        self.common.channel(0).bend_range_cents
    }

    /// The modulation wheel's 14-bit number.
    #[must_use]
    pub const fn wheel(&self) -> u16 {
        self.wheel
    }

    /// Takes one message the server wrote, with the sound's banks.
    pub fn take(&mut self, message: &Message<'_>, banks: &Banks<'_>) {
        let Message::Channel(message) = message else { return };
        let message = on_one_channel(*message);
        let taken = self.common.take(message);
        match taken.action {
            Action::ProgramChange { program, .. } => {
                self.values = bank_patch(banks, program).0;
                self.program = Some(program);
                self.moved = 0;
            }
            Action::ResetAllControllers => {
                self.wheel = 0;
                self.set(controller::PORTAMENTO, 0);
            }
            Action::Control { controller: number, value } => match number {
                controller::MODULATION => self.wheel = u16::from(value) << 7,
                MODULATION_FINE => self.wheel = (self.wheel & !u16::from(DATA_MAX)) | u16::from(value),
                _ => self.set(number, value),
            },
            _ => {}
        }
    }

    fn set(&mut self, number: u8, value: u8) {
        if set_value(&mut self.values, number, value).is_some()
            && let Some((at, _)) = place(number)
        {
            let (first, second) = control_bytes(at);
            self.moved |= 1 << first | 1 << second;
        }
    }

    /// Gives synthD's restatement, in SD-28's order, on channel 0: (1) the
    /// program change of the patch last chosen, unless every panel control
    /// was moved since, when the 60 control changes stand for it; (2) every
    /// panel controller moved by a control change since, in ascending order
    /// of number, both of a pair; (3) a bend range not the default, by
    /// registered parameter 0, then the null parameter number; (4) pitch
    /// bend, controller 7, the modulation wheel as 1 then 33, and the
    /// pedal, each only where it is not as a fresh synthD holds it.
    pub fn restate(&self, out: &mut dyn FnMut(Message<'_>)) {
        let mut send =
            |kind: u8, data1: u8, data2: u8| out(Message::Channel(ChannelMessage::new(kind, 0, data1, data2)));
        if let Some(program) = self.program
            && self.moved != ALL_MOVED
        {
            send(status::PROGRAM_CHANGE, program, 0);
        }
        for (at, (&(number, _), &value)) in LAYOUT.iter().zip(&self.values).enumerate() {
            if self.moved & 1 << at != 0 {
                send(status::CONTROL_CHANGE, number, value);
            }
        }
        let state = self.common.channel(0);
        let range = state.bend_range_cents;
        if range != DEFAULT_BEND_RANGE_CENTS {
            let [coarse, fine] = pair_bytes(RPN_PITCH_BEND_SENSITIVITY);
            let semitones = range / CENTS_PER_SEMITONE;
            let cents = range % CENTS_PER_SEMITONE;
            let [null_coarse, null_fine] = pair_bytes(NULL_PARAMETER);
            send(status::CONTROL_CHANGE, controller::RPN_COARSE, coarse);
            send(status::CONTROL_CHANGE, controller::RPN_FINE, fine);
            send(status::CONTROL_CHANGE, controller::DATA_ENTRY, u8::try_from(semitones).unwrap_or(DATA_MAX));
            send(status::CONTROL_CHANGE, controller::DATA_ENTRY_FINE, u8::try_from(cents).unwrap_or(0));
            send(status::CONTROL_CHANGE, controller::RPN_COARSE, null_coarse);
            send(status::CONTROL_CHANGE, controller::RPN_FINE, null_fine);
        }
        if state.bend != PITCH_BEND_CENTRE {
            let [high, low] = pair_bytes(state.bend);
            send(status::PITCH_BEND, low, high);
        }
        if state.volume != VOLUME_FULL {
            send(status::CONTROL_CHANGE, controller::VOLUME, state.volume);
        }
        if self.wheel != 0 {
            let [coarse, fine] = pair_bytes(self.wheel);
            send(status::CONTROL_CHANGE, controller::MODULATION, coarse);
            send(status::CONTROL_CHANGE, MODULATION_FINE, fine);
        }
        if state.sustain {
            send(status::CONTROL_CHANGE, controller::SUSTAIN, DATA_MAX);
        }
    }
}
