//! A patch: the panel of the instrument, in the units its dials are
//! marked in, and its 60 bytes on the wire (`synthd.md`, SD-17 to SD-22).
//!
//! synthD has no memory and no patch format of its own: a sound is the
//! positions of its knobs and switches, written down on a sound chart.
//! [`Patch`] is those positions, in the dials' units; on the wire a patch
//! is the values of the 60 controllers that move them, in the order of
//! [`interface::LAYOUT`] (SD-22). [`Patch::from_values`] reads the values
//! by SD-18, SD-19 and SD-20's laws, and [`Patch::to_values`] writes each
//! field's canonical value, which reads back exactly.
//!
//! A range is 0 for LO, then 32', 16', 8', 4' and 2'; a waveform is the
//! switch's position from the left ([`Waveform`]). The external input is
//! fed from the instrument's own output, as a cord from the main output
//! to the external input feeds it, so its volume is how much of the
//! output returns.
//!
//! Source: the controls and their dials are those of the owner's manual
//! ("A guide to the operation of the Mini Moog Model D", sections E to J)
//! and of the service manual's figure 7-1, the front panel control
//! diagram; the dials' markings are those the 1974 sound charts write
//! their settings in.

use super::interface::{self, PATCH_BYTES, PatchFault, byte, pair, pair_bytes};
use super::law::{CONTOUR_TIMES, FREQUENCY_CENTS, KNOB_MAX, TUNE_CENTS};

/// The largest time of a contour's dial, in milliseconds.
pub const TIME_MAX: u16 = (CONTOUR_TIMES.1 / 1_000) as u16;
/// The ends of the CUTOFF FREQUENCY dial, in hundredths of a division.
///
/// Source: the sound charts' settings, from -5 to 5; Folkman's
/// adjustment procedure sets it "to -1".
pub const CUTOFF_MAX: i16 = 500;

/// The largest 14-bit number of a pair, and its middle (SD-18).
const PAIR_TOP: i32 = interface::PAIR_MAX as i32;
const PAIR_MIDDLE: i32 = 8_192;
const _: () = assert!(PAIR_MIDDLE == (PAIR_TOP + 1) / 2);
/// The upper half of a pair, which carries AMOUNT OF CONTOUR (SD-18 item 2).
const CONTOUR_SPAN: i32 = PAIR_TOP - PAIR_MIDDLE;
/// A knob's top, 100 tenths of a division.
const KNOB_TOP: i32 = KNOB_MAX as i32;
/// The largest value of one controller.
const VALUE_TOP: i32 = crate::midi::DATA_MAX as i32;
/// A switch is on, and a LO/HI controller HI, from this value (SD-20
/// items 2 and 4; MIDI 1.0 4.2, printed page 12, PDF 44).
const SWITCH_ON: u8 = 64;
/// The canonical values of a switch, off and on (SD-20 item 4).
const SWITCH_OFF_VALUE: u8 = 0;
const SWITCH_ON_VALUE: u8 = 127;
/// The values in each band of an octave controller: 0 to 15 32', 16 to
/// 31 16', and so on (SD-20 item 1; Voyager 3.4 manual, printed page 77).
const OCTAVE_BAND: u8 = 16;
/// The waveform selectors' six positions in 128 values: position
/// `floor(value × 6 ÷ 128)` (SD-20 item 5).
const WAVEFORM_POSITIONS: u16 = 6;
const VALUES: u16 = 128;
/// The canonical value of each waveform position, the lowest of its band
/// (SD-20 item 5).
const WAVEFORM_VALUES: [u8; 6] = [0, 22, 43, 64, 86, 107];

/// A field of a pair from its number, linear over its range (SD-18 item
/// 1): `lowest + floor((number × span + 8,191) ÷ 16,383)`.
fn from_pair(number: u16, lowest: i32, span: i32) -> i32 {
    let scaled = i32::from(number).saturating_mul(span).saturating_add(PAIR_TOP / 2) / PAIR_TOP;
    lowest.saturating_add(scaled)
}

/// The canonical number of a field (SD-18 item 1):
/// `floor(((field − lowest) × 16,383 + floor(span ÷ 2)) ÷ span)`, the field
/// held to its range.
fn to_pair(field: i32, lowest: i32, span: i32) -> u16 {
    let offset = field.saturating_sub(lowest).clamp(0, span);
    let number = offset.saturating_mul(PAIR_TOP).saturating_add(span / 2).checked_div(span).unwrap_or(0);
    u16::try_from(number).unwrap_or(0)
}

/// AMOUNT OF CONTOUR from its number: the pair's upper half (SD-18 item 2).
fn contour_amount_from_pair(number: u16) -> u8 {
    let above = i32::from(number).saturating_sub(PAIR_MIDDLE);
    if above < 0 {
        return 0;
    }
    let amount = above.saturating_mul(KNOB_TOP).saturating_add(CONTOUR_SPAN / 2) / CONTOUR_SPAN;
    u8::try_from(amount).unwrap_or(KNOB_MAX)
}

/// AMOUNT OF CONTOUR's canonical number: `8,192 + floor((field × 8,191 +
/// 50) ÷ 100)` (SD-18 item 2).
fn contour_amount_to_pair(amount: u8) -> u16 {
    let number = PAIR_MIDDLE.saturating_add(
        i32::from(amount.min(KNOB_MAX)).saturating_mul(CONTOUR_SPAN).saturating_add(KNOB_TOP / 2) / KNOB_TOP,
    );
    u16::try_from(number).unwrap_or(0)
}

/// A knob of one controller from its value (SD-19):
/// `floor((value × 100 + 63) ÷ 127)`.
fn knob_from_value(value: u8) -> u8 {
    let knob = i32::from(value).saturating_mul(KNOB_TOP).saturating_add(VALUE_TOP / 2) / VALUE_TOP;
    u8::try_from(knob).unwrap_or(KNOB_MAX)
}

/// A knob's canonical value (SD-19): `floor((field × 127 + 50) ÷ 100)`.
fn knob_to_value(knob: u8) -> u8 {
    let value = i32::from(knob.min(KNOB_MAX)).saturating_mul(VALUE_TOP).saturating_add(KNOB_TOP / 2) / KNOB_TOP;
    u8::try_from(value).unwrap_or(0)
}

/// A switch from its value (SD-20 item 4).
const fn switch_from_value(value: u8) -> bool {
    value >= SWITCH_ON
}

/// A switch's canonical value.
const fn switch_to_value(on: bool) -> u8 {
    if on { SWITCH_ON_VALUE } else { SWITCH_OFF_VALUE }
}

/// The spans of SD-18 item 1: each field's lowest value and its range.
mod span {
    use super::{CUTOFF_MAX, FREQUENCY_CENTS, KNOB_MAX, TIME_MAX, TUNE_CENTS};
    /// TUNE: −242 to +242 cents.
    pub const TUNE: (i32, i32) = (-(TUNE_CENTS as i32), 2 * TUNE_CENTS as i32);
    /// A FREQUENCY knob: −700 to +700 cents.
    pub const FREQUENCY: (i32, i32) = (-(FREQUENCY_CENTS as i32), 2 * FREQUENCY_CENTS as i32);
    /// CUTOFF FREQUENCY: −500 to +500 hundredths of a division.
    pub const CUTOFF: (i32, i32) = (-(CUTOFF_MAX as i32), 2 * CUTOFF_MAX as i32);
    /// An ATTACK or DECAY TIME: 0 to 10,000 milliseconds.
    pub const TIME: (i32, i32) = (0, TIME_MAX as i32);
    /// A knob of 0 to 100.
    pub const KNOB: (i32, i32) = (0, KNOB_MAX as i32);
}
const _: () = assert!(span::TUNE.1 == 484 && span::FREQUENCY.1 == 1_400 && span::CUTOFF.1 == 1_000);
const _: () = assert!(span::TIME.1 == 10_000 && span::KNOB.1 == 100);

/// A range switch's positions.
///
/// Source: owner's manual, page 4: "its 6 positions", LO to 2'.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Range {
    /// LO: below hearing, for clicks and for modulation.
    Lo = 0,
    /// 32'.
    ThirtyTwo = 1,
    /// 16'.
    Sixteen = 2,
    /// 8'.
    Eight = 3,
    /// 4'.
    Four = 4,
    /// 2'.
    Two = 5,
}

impl Range {
    const ALL: [Self; 6] = [Self::Lo, Self::ThirtyTwo, Self::Sixteen, Self::Eight, Self::Four, Self::Two];

    /// The footage of an octave controller's band, 0 for 32' to 4 for 2';
    /// the 1' band and above select 2' (SD-20 item 1).
    fn from_band(band: u8) -> Self {
        Self::ALL.get(usize::from(band).saturating_add(1)).copied().unwrap_or(Self::Two)
    }

    /// The canonical octave value and whether the LO/HI controller is HI
    /// (SD-20 items 1 and 3): LO is LO/HI 0 with the octave at 8'.
    fn values(self) -> (u8, bool) {
        match self {
            Self::Lo => (Self::Eight.octave_value(), false),
            footage => (footage.octave_value(), true),
        }
    }

    /// The lowest value of a footage's band: 0 (32'), 16, 32, 48, 64 (2').
    const fn octave_value(self) -> u8 {
        (self as u8).saturating_sub(1).saturating_mul(OCTAVE_BAND)
    }
}

/// A waveform switch's positions, from the left.
///
/// Source: owner's manual, page 4: "triangular, sawtooth-triangular,
/// sawtooth, square, wide rectangular, and narrow rectangular.
/// (Oscillator 3 substitutes a reverse sawtooth for the
/// sawtooth-triangular.)"
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Waveform {
    /// The triangle.
    Triangle = 0,
    /// The triangle with a little of the sawtooth; on oscillator 3, the
    /// sawtooth reversed.
    Shark = 1,
    /// The sawtooth.
    Sawtooth = 2,
    /// The square.
    Square = 3,
    /// The wide rectangular wave.
    Wide = 4,
    /// The narrow rectangular wave.
    Narrow = 5,
}

impl Waveform {
    const ALL: [Self; 6] = [Self::Triangle, Self::Shark, Self::Sawtooth, Self::Square, Self::Wide, Self::Narrow];
}

/// One oscillator's controls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OscillatorPatch {
    /// RANGE.
    pub range: Range,
    /// WAVEFORM.
    pub waveform: Waveform,
    /// FREQUENCY, in cents from -700 to 700; oscillator 1 has no such
    /// knob, and its is not used. With oscillator 3's keyboard control
    /// off, the same travel is 2.75 octaves either way.
    pub frequency: i16,
    /// The mixer's VOLUME, 0 to 100.
    pub volume: u8,
    /// The mixer's switch.
    pub on: bool,
}

/// One contour generator's controls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ContourPatch {
    /// ATTACK TIME, as the dial is marked: milliseconds, 0 to 10,000.
    pub attack: u16,
    /// DECAY TIME, likewise.
    pub decay: u16,
    /// SUSTAIN LEVEL, 0 to 100.
    pub sustain: u8,
}

/// A patch.
#[allow(clippy::struct_excessive_bools, reason = "each is a switch of the panel, on or off")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    /// TUNE, in cents from -242 to 242.
    pub tune: i16,
    /// GLIDE, 0 to 100.
    pub glide: u8,
    /// MODULATION MIX, 0 to 100: oscillator 3 at 0, noise at 100.
    pub modulation_mix: u8,
    /// The three oscillators.
    pub oscillators: [OscillatorPatch; 3],
    /// EXTERNAL INPUT VOLUME, 0 to 100: how much of the instrument's own
    /// output returns to the mixer.
    pub external_volume: u8,
    /// The mixer's switch of the external input.
    pub external_on: bool,
    /// NOISE VOLUME, 0 to 100.
    pub noise_volume: u8,
    /// The mixer's switch of the noise.
    pub noise_on: bool,
    /// The noise is pink; otherwise white.
    pub noise_pink: bool,
    /// CUTOFF FREQUENCY, in hundredths of the dial's -5 to 5.
    pub cutoff: i16,
    /// EMPHASIS, 0 to 100.
    pub emphasis: u8,
    /// AMOUNT OF CONTOUR, 0 to 100.
    pub contour_amount: u8,
    /// The filter's contour.
    pub filter_contour: ContourPatch,
    /// The loudness contour.
    pub loudness_contour: ContourPatch,
    /// The main output's VOLUME, 0 to 100.
    pub volume: u8,
    /// OSCILLATOR MODULATION.
    pub oscillator_modulation: bool,
    /// OSC. 3 CONTROL: the keyboard moves oscillator 3.
    pub oscillator_3_control: bool,
    /// FILTER MODULATION.
    pub filter_modulation: bool,
    /// KEYBOARD CONTROL 1 and 2.
    pub keyboard_control: [bool; 2],
    /// The GLIDE switch.
    pub glide_on: bool,
    /// The DECAY switch.
    pub decay_on: bool,
}

impl Patch {
    /// The patch an instrument plays until it is given another:
    /// oscillator 1 alone, a sawtooth at 8', through the filter wide
    /// open, the loudness contour as an organ's.
    ///
    /// Source: own choice.
    pub const DEFAULT: Self = {
        let oscillator = OscillatorPatch {
            range: Range::Eight,
            waveform: Waveform::Sawtooth,
            frequency: 0,
            volume: KNOB_MAX,
            on: false,
        };
        let contour = ContourPatch { attack: 0, decay: 0, sustain: KNOB_MAX };
        Self {
            tune: 0,
            glide: 0,
            modulation_mix: 0,
            oscillators: [
                OscillatorPatch { on: true, ..oscillator },
                oscillator,
                OscillatorPatch { range: Range::Lo, waveform: Waveform::Triangle, ..oscillator },
            ],
            external_volume: 0,
            external_on: false,
            noise_volume: 0,
            noise_on: false,
            noise_pink: false,
            cutoff: CUTOFF_MAX,
            emphasis: 0,
            contour_amount: 0,
            filter_contour: contour,
            loudness_contour: contour,
            volume: KNOB_MAX,
            oscillator_modulation: false,
            oscillator_3_control: true,
            filter_modulation: false,
            keyboard_control: [false, false],
            glide_on: false,
            decay_on: false,
        }
    };

    /// The panel the 60 values of a patch set, by SD-18, SD-19 and SD-20's
    /// laws. Every value from 0 to 127 is a position; a value above 127,
    /// which a checked patch never holds, is read by its low seven bits.
    #[must_use]
    pub fn from_values(values: &[u8; PATCH_BYTES]) -> Self {
        let single = |at: usize| values.get(at).copied().unwrap_or(0) & crate::midi::DATA_MAX;
        let field = |at: usize, (lowest, span): (i32, i32)| from_pair(pair(values, at), lowest, span);
        let signed = |at: usize, span: (i32, i32)| i16::try_from(field(at, span)).unwrap_or(0);
        let time = |at: usize| u16::try_from(field(at, span::TIME)).unwrap_or(TIME_MAX);
        let knob = |at: usize| u8::try_from(field(at, span::KNOB)).unwrap_or(KNOB_MAX);
        let switch = |at: usize| switch_from_value(single(at));
        let range = |octave: usize, lo_hi: usize| {
            if switch(lo_hi) {
                // The 1' band, 80 to 127, selects 2' (SD-20 item 1).
                Range::from_band(single(octave) / OCTAVE_BAND)
            } else {
                Range::Lo
            }
        };
        let waveform = |at: usize| {
            let position = u16::from(single(at)).saturating_mul(WAVEFORM_POSITIONS) / VALUES;
            Waveform::ALL.get(usize::from(position)).copied().unwrap_or(Waveform::Narrow)
        };
        let oscillator = |range: Range, waveform: Waveform, frequency: i16, volume: usize, on: usize| OscillatorPatch {
            range,
            waveform,
            frequency,
            volume: knob(volume),
            on: switch(on),
        };
        let contour = |attack: usize, decay: usize, sustain: usize| ContourPatch {
            attack: time(attack),
            decay: time(decay),
            sustain: knob(sustain),
        };
        Self {
            tune: signed(byte::TUNE, span::TUNE),
            glide: knob(byte::GLIDE),
            modulation_mix: knob_from_value(single(byte::MODULATION_MIX)),
            oscillators: [
                oscillator(
                    range(byte::OSCILLATOR_1_RANGE_OCTAVE, byte::OSCILLATOR_1_RANGE_LO_HI),
                    waveform(byte::OSCILLATOR_1_WAVEFORM),
                    0,
                    byte::OSCILLATOR_1_VOLUME,
                    byte::OSCILLATOR_1_SWITCH,
                ),
                oscillator(
                    range(byte::OSCILLATOR_2_RANGE_OCTAVE, byte::OSCILLATOR_2_RANGE_LO_HI),
                    waveform(byte::OSCILLATOR_2_WAVEFORM),
                    signed(byte::OSCILLATOR_2_FREQUENCY, span::FREQUENCY),
                    byte::OSCILLATOR_2_VOLUME,
                    byte::OSCILLATOR_2_SWITCH,
                ),
                oscillator(
                    range(byte::OSCILLATOR_3_RANGE_OCTAVE, byte::OSCILLATOR_3_RANGE_LO_HI),
                    waveform(byte::OSCILLATOR_3_WAVEFORM),
                    signed(byte::OSCILLATOR_3_FREQUENCY, span::FREQUENCY),
                    byte::OSCILLATOR_3_VOLUME,
                    byte::OSCILLATOR_3_SWITCH,
                ),
            ],
            external_volume: knob(byte::EXTERNAL_INPUT_VOLUME),
            external_on: switch(byte::EXTERNAL_INPUT_SWITCH),
            noise_volume: knob(byte::NOISE_VOLUME),
            noise_on: switch(byte::NOISE_SWITCH),
            noise_pink: switch(byte::NOISE_COLOUR),
            cutoff: signed(byte::CUTOFF_FREQUENCY, span::CUTOFF),
            emphasis: knob(byte::EMPHASIS),
            contour_amount: contour_amount_from_pair(pair(values, byte::AMOUNT_OF_CONTOUR)),
            filter_contour: contour(byte::FILTER_ATTACK_TIME, byte::FILTER_DECAY_TIME, byte::FILTER_SUSTAIN_LEVEL),
            loudness_contour: contour(
                byte::LOUDNESS_ATTACK_TIME,
                byte::LOUDNESS_DECAY_TIME,
                byte::LOUDNESS_SUSTAIN_LEVEL,
            ),
            volume: knob_from_value(single(byte::MAIN_OUTPUT_VOLUME)),
            oscillator_modulation: switch(byte::OSCILLATOR_MODULATION),
            oscillator_3_control: switch(byte::OSCILLATOR_3_CONTROL),
            filter_modulation: switch(byte::FILTER_MODULATION),
            keyboard_control: [switch(byte::KEYBOARD_CONTROL_1), switch(byte::KEYBOARD_CONTROL_2)],
            glide_on: switch(byte::GLIDE_SWITCH),
            decay_on: switch(byte::DECAY_SWITCH),
        }
    }

    /// The patch's 60 values, each field's canonical value (SD-18 to
    /// SD-20, SD-22), which [`Patch::from_values`] reads back exactly. A
    /// field beyond its range is held to it; oscillator 1's frequency,
    /// which has no control, is not written.
    #[must_use]
    pub fn to_values(&self) -> [u8; PATCH_BYTES] {
        let mut values = [0u8; PATCH_BYTES];
        let mut put = |at: usize, value: u8| {
            if let Some(place) = values.get_mut(at) {
                *place = value;
            }
        };
        let word = |put: &mut dyn FnMut(usize, u8), at: usize, number: u16| {
            let [coarse, fine] = pair_bytes(number);
            put(at, coarse);
            put(at.saturating_add(1), fine);
        };
        let field = |value: i32, (lowest, span): (i32, i32)| to_pair(value, lowest, span);
        let [first, second, third] = self.oscillators;
        let pairs = [
            (byte::GLIDE, field(i32::from(self.glide), span::KNOB)),
            (byte::OSCILLATOR_2_FREQUENCY, field(i32::from(second.frequency), span::FREQUENCY)),
            (byte::OSCILLATOR_3_FREQUENCY, field(i32::from(third.frequency), span::FREQUENCY)),
            (byte::EXTERNAL_INPUT_VOLUME, field(i32::from(self.external_volume), span::KNOB)),
            (byte::OSCILLATOR_1_VOLUME, field(i32::from(first.volume), span::KNOB)),
            (byte::OSCILLATOR_2_VOLUME, field(i32::from(second.volume), span::KNOB)),
            (byte::OSCILLATOR_3_VOLUME, field(i32::from(third.volume), span::KNOB)),
            (byte::NOISE_VOLUME, field(i32::from(self.noise_volume), span::KNOB)),
            (byte::CUTOFF_FREQUENCY, field(i32::from(self.cutoff), span::CUTOFF)),
            (byte::EMPHASIS, field(i32::from(self.emphasis), span::KNOB)),
            (byte::FILTER_ATTACK_TIME, field(i32::from(self.filter_contour.attack), span::TIME)),
            (byte::FILTER_DECAY_TIME, field(i32::from(self.filter_contour.decay), span::TIME)),
            (byte::FILTER_SUSTAIN_LEVEL, field(i32::from(self.filter_contour.sustain), span::KNOB)),
            (byte::AMOUNT_OF_CONTOUR, contour_amount_to_pair(self.contour_amount)),
            (byte::LOUDNESS_ATTACK_TIME, field(i32::from(self.loudness_contour.attack), span::TIME)),
            (byte::LOUDNESS_DECAY_TIME, field(i32::from(self.loudness_contour.decay), span::TIME)),
            (byte::LOUDNESS_SUSTAIN_LEVEL, field(i32::from(self.loudness_contour.sustain), span::KNOB)),
            (byte::TUNE, field(i32::from(self.tune), span::TUNE)),
        ];
        for (at, number) in pairs {
            word(&mut put, at, number);
        }
        let ranges = [
            (byte::OSCILLATOR_1_RANGE_OCTAVE, byte::OSCILLATOR_1_RANGE_LO_HI, first.range),
            (byte::OSCILLATOR_2_RANGE_OCTAVE, byte::OSCILLATOR_2_RANGE_LO_HI, second.range),
            (byte::OSCILLATOR_3_RANGE_OCTAVE, byte::OSCILLATOR_3_RANGE_LO_HI, third.range),
        ];
        for (octave, lo_hi, range) in ranges {
            let (octave_value, hi) = range.values();
            put(octave, octave_value);
            put(lo_hi, switch_to_value(hi));
        }
        let waveforms = [
            (byte::OSCILLATOR_1_WAVEFORM, first.waveform),
            (byte::OSCILLATOR_2_WAVEFORM, second.waveform),
            (byte::OSCILLATOR_3_WAVEFORM, third.waveform),
        ];
        for (at, waveform) in waveforms {
            put(at, WAVEFORM_VALUES.get(waveform as usize).copied().unwrap_or(0));
        }
        let switches = [
            (byte::GLIDE_SWITCH, self.glide_on),
            (byte::OSCILLATOR_3_CONTROL, self.oscillator_3_control),
            (byte::EXTERNAL_INPUT_SWITCH, self.external_on),
            (byte::OSCILLATOR_1_SWITCH, first.on),
            (byte::OSCILLATOR_2_SWITCH, second.on),
            (byte::OSCILLATOR_3_SWITCH, third.on),
            (byte::NOISE_SWITCH, self.noise_on),
            (byte::DECAY_SWITCH, self.decay_on),
            (byte::OSCILLATOR_MODULATION, self.oscillator_modulation),
            (byte::NOISE_COLOUR, self.noise_pink),
            (byte::FILTER_MODULATION, self.filter_modulation),
            (byte::KEYBOARD_CONTROL_1, self.keyboard_control[0]),
            (byte::KEYBOARD_CONTROL_2, self.keyboard_control[1]),
        ];
        for (at, on) in switches {
            put(at, switch_to_value(on));
        }
        put(byte::MODULATION_MIX, knob_to_value(self.modulation_mix));
        put(byte::MAIN_OUTPUT_VOLUME, knob_to_value(self.volume));
        values
    }

    /// Reads a patch from its bytes, checking them (SD-22).
    ///
    /// # Errors
    ///
    /// The [`PatchFault`] of a patch not of 60 bytes, or of its first byte
    /// above 127.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PatchFault> {
        interface::check_values(bytes).map(|values| Self::from_values(&values))
    }

    /// The patch as its 60 bytes: [`Patch::to_values`].
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        self.to_values()
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self::DEFAULT
    }
}
