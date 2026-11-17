//! A patch: the panel of the instrument as a fixed record.
//!
//! The Minimoog has no memory and no patch format: a sound is the
//! positions of its knobs and switches, written down on a sound chart.
//! The record here is those positions, in the units the dials are marked
//! in, and its byte form is this crate's own.
//!
//! # The byte layout
//!
//! [`PATCH_BYTES`] bytes; numbers of two bytes have the low byte first.
//!
//! | Bytes | Control | Values |
//! |---|---|---|
//! | 0, 1 | TUNE | cents, -242 to 242, signed |
//! | 2 | GLIDE | 0 to 100: tenths of the dial's 0 to 10 |
//! | 3 | MODULATION MIX | 0 to 100 (0 is oscillator 3, 100 noise) |
//! | 4, 5 | Oscillator 1 RANGE, WAVEFORM | 0 to 5 each |
//! | 6, 7 | Oscillator 2 RANGE, WAVEFORM | 0 to 5 each |
//! | 8, 9 | Oscillator 2 FREQUENCY | cents, -700 to 700, signed |
//! | 10, 11 | Oscillator 3 RANGE, WAVEFORM | 0 to 5 each |
//! | 12, 13 | Oscillator 3 FREQUENCY | cents, -700 to 700, signed |
//! | 14, 15, 16 | Mixer VOLUME of oscillators 1, 2, 3 | 0 to 100 each |
//! | 17 | EXTERNAL INPUT VOLUME | 0 to 100 |
//! | 18 | NOISE VOLUME | 0 to 100 |
//! | 19, 20 | CUTOFF FREQUENCY | hundredths of the dial's -5 to 5, signed |
//! | 21 | EMPHASIS | 0 to 100 |
//! | 22 | AMOUNT OF CONTOUR | 0 to 100 |
//! | 23, 24 | Filter ATTACK TIME | milliseconds, 0 to 10,000 |
//! | 25, 26 | Filter DECAY TIME | milliseconds, 0 to 10,000 |
//! | 27 | Filter SUSTAIN LEVEL | 0 to 100 |
//! | 28, 29 | Loudness ATTACK TIME | milliseconds, 0 to 10,000 |
//! | 30, 31 | Loudness DECAY TIME | milliseconds, 0 to 10,000 |
//! | 32 | Loudness SUSTAIN LEVEL | 0 to 100 |
//! | 33 | Main output VOLUME | 0 to 100 |
//! | 34 | Switches | bits 0 to 6, see [`switch`] |
//! | 35 | Switches | bits 0 to 5, see [`switch`] |
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
//! their settings in. The layout is an own choice.

use super::law::{CONTOUR_TIMES, FREQUENCY_CENTS, KNOB_MAX, TUNE_CENTS};

/// Bytes of a patch.
pub const PATCH_BYTES: usize = 36;

/// The largest time of a contour's dial, in milliseconds.
pub const TIME_MAX: u16 = (CONTOUR_TIMES.1 / 1_000) as u16;
/// The ends of the CUTOFF FREQUENCY dial, in hundredths of a division.
///
/// Source: the sound charts' settings, from -5 to 5; Folkman's
/// adjustment procedure sets it "to -1".
pub const CUTOFF_MAX: i16 = 500;

/// Where the fields that are not an oscillator's lie: each one's first
/// byte, as the table of the module's documentation has them.
mod layout {
    pub const TUNE: u8 = 0;
    pub const GLIDE: u8 = 2;
    pub const MODULATION_MIX: u8 = 3;
    pub const VOLUMES: u8 = 14;
    pub const EXTERNAL_VOLUME: u8 = 17;
    pub const NOISE_VOLUME: u8 = 18;
    pub const CUTOFF: u8 = 19;
    pub const EMPHASIS: u8 = 21;
    pub const CONTOUR_AMOUNT: u8 = 22;
    pub const FILTER_CONTOUR: u8 = 23;
    pub const LOUDNESS_CONTOUR: u8 = 28;
    pub const VOLUME: u8 = 33;
    pub const SWITCHES: u8 = 34;
}

/// The bits of the two switch bytes.
pub mod switch {
    /// Byte 34: OSCILLATOR MODULATION.
    pub const OSCILLATOR_MODULATION: u8 = 1;
    /// Byte 34: OSC. 3 CONTROL, the keyboard's control of oscillator 3.
    pub const OSCILLATOR_3_CONTROL: u8 = 1 << 1;
    /// Byte 34: the mixer's switch of oscillator 1.
    pub const MIX_1: u8 = 1 << 2;
    /// Byte 34: the mixer's switch of oscillator 2.
    pub const MIX_2: u8 = 1 << 3;
    /// Byte 34: the mixer's switch of oscillator 3.
    pub const MIX_3: u8 = 1 << 4;
    /// Byte 34: the mixer's switch of the external input.
    pub const MIX_EXTERNAL: u8 = 1 << 5;
    /// Byte 34: the mixer's switch of the noise.
    pub const MIX_NOISE: u8 = 1 << 6;
    /// The bits of byte 34 that mean something.
    pub const FIRST_MASK: u8 = (1 << 7) - 1;
    /// Byte 35: the noise is pink, not white.
    pub const NOISE_PINK: u8 = 1;
    /// Byte 35: FILTER MODULATION.
    pub const FILTER_MODULATION: u8 = 1 << 1;
    /// Byte 35: KEYBOARD CONTROL 1, a third.
    pub const KEYBOARD_CONTROL_1: u8 = 1 << 2;
    /// Byte 35: KEYBOARD CONTROL 2, two thirds.
    pub const KEYBOARD_CONTROL_2: u8 = 1 << 3;
    /// Byte 35: GLIDE, beside the wheels.
    pub const GLIDE: u8 = 1 << 4;
    /// Byte 35: DECAY, beside the wheels.
    pub const DECAY: u8 = 1 << 5;
    /// The bits of byte 35 that mean something.
    pub const SECOND_MASK: u8 = (1 << 6) - 1;
}

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

    fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.get(usize::from(byte)).copied()
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

    fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.get(usize::from(byte)).copied()
    }
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

/// Why bytes are not a patch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchError {
    /// TUNE is beyond 242 cents either way.
    Tune,
    /// The knob at this byte is above 100.
    Knob(u8),
    /// A range or a waveform, at this byte, is above 5.
    Position(u8),
    /// An oscillator's FREQUENCY, whose low byte is this one, is beyond
    /// 700 cents either way.
    Frequency(u8),
    /// CUTOFF FREQUENCY is beyond the dial's -5 to 5.
    Cutoff,
    /// A contour's time, whose low byte is this one, is above 10,000
    /// milliseconds.
    Time(u8),
    /// A switch byte has a bit set that means nothing.
    Switches,
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

    /// Reads a patch from its bytes, checking every one.
    ///
    /// # Errors
    ///
    /// The [`PatchError`] of the first byte, in order, that is outside
    /// its range.
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Result<Self, PatchError> {
        let byte = |at: u8| bytes.get(usize::from(at)).copied().unwrap_or(0);
        let word = |at: u8| [byte(at), byte(at.saturating_add(1))];
        let knob = |at: u8| if byte(at) > KNOB_MAX { Err(PatchError::Knob(at)) } else { Ok(byte(at)) };
        let range = |at: u8| Range::from_byte(byte(at)).ok_or(PatchError::Position(at));
        let waveform = |at: u8| Waveform::from_byte(byte(at)).ok_or(PatchError::Position(at));
        let frequency = |at: u8| {
            let value = i16::from_le_bytes(word(at));
            if (-FREQUENCY_CENTS..=FREQUENCY_CENTS).contains(&value) {
                Ok(value)
            } else {
                Err(PatchError::Frequency(at))
            }
        };
        let time = |at: u8| {
            let value = u16::from_le_bytes(word(at));
            if value > TIME_MAX { Err(PatchError::Time(at)) } else { Ok(value) }
        };
        let contour = |at: u8| -> Result<ContourPatch, PatchError> {
            Ok(ContourPatch {
                attack: time(at)?,
                decay: time(at.saturating_add(2))?,
                sustain: knob(at.saturating_add(4))?,
            })
        };

        let tune = i16::from_le_bytes(word(layout::TUNE));
        if !(-TUNE_CENTS..=TUNE_CENTS).contains(&tune) {
            return Err(PatchError::Tune);
        }
        let glide = knob(layout::GLIDE)?;
        let modulation_mix = knob(layout::MODULATION_MIX)?;
        let off =
            OscillatorPatch { range: Range::Eight, waveform: Waveform::Triangle, frequency: 0, volume: 0, on: false };
        let mut oscillators = [
            OscillatorPatch { range: range(4)?, waveform: waveform(5)?, ..off },
            OscillatorPatch { range: range(6)?, waveform: waveform(7)?, frequency: frequency(8)?, ..off },
            OscillatorPatch { range: range(10)?, waveform: waveform(11)?, frequency: frequency(12)?, ..off },
        ];
        let (first, second) = (byte(layout::SWITCHES), byte(layout::SWITCHES.saturating_add(1)));
        let mixed = [switch::MIX_1, switch::MIX_2, switch::MIX_3];
        for ((oscillator, at), bit) in oscillators.iter_mut().zip(layout::VOLUMES..).zip(mixed) {
            oscillator.volume = knob(at)?;
            oscillator.on = first & bit != 0;
        }
        let external_volume = knob(layout::EXTERNAL_VOLUME)?;
        let noise_volume = knob(layout::NOISE_VOLUME)?;
        let cutoff = i16::from_le_bytes(word(layout::CUTOFF));
        if !(-CUTOFF_MAX..=CUTOFF_MAX).contains(&cutoff) {
            return Err(PatchError::Cutoff);
        }
        let emphasis = knob(layout::EMPHASIS)?;
        let contour_amount = knob(layout::CONTOUR_AMOUNT)?;
        let filter_contour = contour(layout::FILTER_CONTOUR)?;
        let loudness_contour = contour(layout::LOUDNESS_CONTOUR)?;
        let volume = knob(layout::VOLUME)?;
        if first & !switch::FIRST_MASK != 0 || second & !switch::SECOND_MASK != 0 {
            return Err(PatchError::Switches);
        }
        Ok(Self {
            tune,
            glide,
            modulation_mix,
            oscillators,
            external_volume,
            external_on: first & switch::MIX_EXTERNAL != 0,
            noise_volume,
            noise_on: first & switch::MIX_NOISE != 0,
            noise_pink: second & switch::NOISE_PINK != 0,
            cutoff,
            emphasis,
            contour_amount,
            filter_contour,
            loudness_contour,
            volume,
            oscillator_modulation: first & switch::OSCILLATOR_MODULATION != 0,
            oscillator_3_control: first & switch::OSCILLATOR_3_CONTROL != 0,
            filter_modulation: second & switch::FILTER_MODULATION != 0,
            keyboard_control: [second & switch::KEYBOARD_CONTROL_1 != 0, second & switch::KEYBOARD_CONTROL_2 != 0],
            glide_on: second & switch::GLIDE != 0,
            decay_on: second & switch::DECAY != 0,
        })
    }

    /// The patch as its bytes. A field beyond its range is held to it, so
    /// that the bytes are always a patch; oscillator 1's frequency, which
    /// has no byte, is not written.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        let knob = |value: u8| value.min(KNOB_MAX);
        let time = |value: u16| value.min(TIME_MAX).to_le_bytes();
        let bit = |on: bool, bit: u8| if on { bit } else { 0 };
        let [first_oscillator, second_oscillator, third_oscillator] = self.oscillators;
        let [tune_low, tune_high] = self.tune.clamp(-TUNE_CENTS, TUNE_CENTS).to_le_bytes();
        let frequency = |value: i16| value.clamp(-FREQUENCY_CENTS, FREQUENCY_CENTS).to_le_bytes();
        let [frequency_2_low, frequency_2_high] = frequency(second_oscillator.frequency);
        let [frequency_3_low, frequency_3_high] = frequency(third_oscillator.frequency);
        let [cutoff_low, cutoff_high] = self.cutoff.clamp(-CUTOFF_MAX, CUTOFF_MAX).to_le_bytes();
        let [filter_attack_low, filter_attack_high] = time(self.filter_contour.attack);
        let [filter_decay_low, filter_decay_high] = time(self.filter_contour.decay);
        let [loudness_attack_low, loudness_attack_high] = time(self.loudness_contour.attack);
        let [loudness_decay_low, loudness_decay_high] = time(self.loudness_contour.decay);
        let [first_control, second_control] = self.keyboard_control;
        let first = bit(self.oscillator_modulation, switch::OSCILLATOR_MODULATION)
            | bit(self.oscillator_3_control, switch::OSCILLATOR_3_CONTROL)
            | bit(first_oscillator.on, switch::MIX_1)
            | bit(second_oscillator.on, switch::MIX_2)
            | bit(third_oscillator.on, switch::MIX_3)
            | bit(self.external_on, switch::MIX_EXTERNAL)
            | bit(self.noise_on, switch::MIX_NOISE);
        let second = bit(self.noise_pink, switch::NOISE_PINK)
            | bit(self.filter_modulation, switch::FILTER_MODULATION)
            | bit(first_control, switch::KEYBOARD_CONTROL_1)
            | bit(second_control, switch::KEYBOARD_CONTROL_2)
            | bit(self.glide_on, switch::GLIDE)
            | bit(self.decay_on, switch::DECAY);
        [
            tune_low,
            tune_high,
            knob(self.glide),
            knob(self.modulation_mix),
            first_oscillator.range as u8,
            first_oscillator.waveform as u8,
            second_oscillator.range as u8,
            second_oscillator.waveform as u8,
            frequency_2_low,
            frequency_2_high,
            third_oscillator.range as u8,
            third_oscillator.waveform as u8,
            frequency_3_low,
            frequency_3_high,
            knob(first_oscillator.volume),
            knob(second_oscillator.volume),
            knob(third_oscillator.volume),
            knob(self.external_volume),
            knob(self.noise_volume),
            cutoff_low,
            cutoff_high,
            knob(self.emphasis),
            knob(self.contour_amount),
            filter_attack_low,
            filter_attack_high,
            filter_decay_low,
            filter_decay_high,
            knob(self.filter_contour.sustain),
            loudness_attack_low,
            loudness_attack_high,
            loudness_decay_low,
            loudness_decay_high,
            knob(self.loudness_contour.sustain),
            knob(self.volume),
            first,
            second,
        ]
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self::DEFAULT
    }
}
