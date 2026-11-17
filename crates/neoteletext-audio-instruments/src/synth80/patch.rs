//! A patch: the panel of the instrument as a fixed record of 85 bytes.
//!
//! synth80 has no digital patch: its four memories are small copies of
//! the panel's levers, and its presets are resistors. The byte layout
//! here is therefore this crate's own. It follows the instrument: a
//! [`Tone`] is one row of the panel, the 26 controls that a tone selector
//! button recalls and that a memory holds, in the order the memory panel
//! has them from left to right; a [`Patch`] is the tone of channel I, the
//! tone of channel II and the 33 controls that belong to neither and
//! that no memory or preset holds ([`Global`]).
//!
//! | Bytes | What |
//! |---|---|
//! | 0 to 25 | Channel I's tone, one byte for each [`Control`] in order |
//! | 26 to 51 | Channel II's tone, the same |
//! | 52 to 84 | One byte for each [`Global`] in order |
//!
//! A lever is 0 to 127 from the end the panel marks low, slow, short or 0
//! to the other; a switch is 0 or 1; a selector is its position from 0.
//! Any other value is refused ([`PatchError`]).
//!
//! Source: owner's manual, the photograph inside the cover and its list
//! of controls, items 1 to 46, and page 36, the memory panel, whose legend
//! under the second memory gives the order of a tone's controls; service
//! manual, page 1, Specifications, which lists the same controls.

use super::law::LEVER_MAX;

/// Bytes of a tone.
pub const TONE_BYTES: usize = 26;
/// Bytes of the controls outside the tones.
pub const GLOBAL_BYTES: usize = 33;
/// Bytes of a patch.
pub const PATCH_BYTES: usize = 2 * TONE_BYTES + GLOBAL_BYTES;

/// The controls of a tone, each named by its byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Control {
    /// SPEED of the pulse width's own oscillator, slow to fast.
    PwmSpeed = 0,
    /// PWM: how far that oscillator moves the pulse's width.
    Pwm = 1,
    /// PW: the pulse's width, from 50% to 90%.
    PulseWidth = 2,
    /// The square wave's switch.
    Square = 3,
    /// The sawtooth wave's switch.
    Sawtooth = 4,
    /// NOISE: the level of white noise into the filters.
    Noise = 5,
    /// HPF: the high-pass filter's cutoff, low to high.
    Hpf = 6,
    /// `RESh`: the high-pass filter's resonance.
    HpfResonance = 7,
    /// LPF: the low-pass filter's cutoff, low to high.
    Lpf = 8,
    /// `RESl`: the low-pass filter's resonance.
    LpfResonance = 9,
    /// IL: how far below the levers' setting the filters start, 0 to -5.
    InitialLevel = 10,
    /// AL: how far above it the attack takes them, 0 to +5.
    AttackLevel = 11,
    /// A of the filter's envelope, short to long.
    FilterAttack = 12,
    /// D of the filter's envelope.
    FilterDecay = 13,
    /// R of the filter's envelope.
    FilterRelease = 14,
    /// VCF LEVEL: how much of the filters' output reaches the amplifier.
    VcfLevel = 15,
    /// The sine wave's level into the amplifier, past the filters.
    Sine = 16,
    /// A of the amplifier's envelope, short to long.
    Attack = 17,
    /// D of the amplifier's envelope.
    Decay = 18,
    /// S of the amplifier's envelope.
    Sustain = 19,
    /// R of the amplifier's envelope.
    Release = 20,
    /// LEVEL: the tone's level.
    Level = 21,
    /// INITIAL BRILLIANCE: how far a harder stroke opens the filters.
    TouchBrilliance = 22,
    /// INITIAL LEVEL: how far a softer stroke lowers the level.
    TouchLevel = 23,
    /// AFTER BRILLIANCE: how far pressure on the key opens the filters.
    PressureBrilliance = 24,
    /// AFTER LEVEL: how far pressure on the key raises the level.
    PressureLevel = 25,
}

impl Control {
    /// The largest value of the control's byte.
    #[must_use]
    pub const fn max(self) -> u8 {
        match self {
            Self::Square | Self::Sawtooth => 1,
            _ => LEVER_MAX,
        }
    }
}

/// The largest value of each byte of a tone.
#[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build, and no program")]
pub(super) const TONE_MAXIMA: [u8; TONE_BYTES] = {
    let mut maxima = [LEVER_MAX; TONE_BYTES];
    maxima[Control::Square as usize] = Control::Square.max();
    maxima[Control::Sawtooth as usize] = Control::Sawtooth.max();
    maxima
};

/// One row of the panel: what a tone selector button recalls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tone {
    values: [u8; TONE_BYTES],
}

impl Tone {
    /// The panel's row as the owner's manual's cover shows it at rest: a
    /// sawtooth through open filters, the amplifier's envelope a gate, no
    /// touch response.
    ///
    /// Source: owner's manual, page 31, the diagram of the nominal
    /// settings, read as: sawtooth on, LPF at HIGH, VCF LEVEL at 10,
    /// sustain at 10, LEVEL at 10, everything else at its foot. Own
    /// choice where the diagram is too small to tell a lever's place.
    #[allow(
        clippy::indexing_slicing,
        reason = "worked out by the compiler: a fault here stops the build, and no program"
    )]
    pub const DEFAULT: Self = {
        let mut values = [0; TONE_BYTES];
        values[Control::Sawtooth as usize] = 1;
        values[Control::Lpf as usize] = LEVER_MAX;
        values[Control::VcfLevel as usize] = LEVER_MAX;
        values[Control::Sustain as usize] = LEVER_MAX;
        values[Control::Level as usize] = LEVER_MAX;
        Self { values }
    };

    /// A tone from its 26 values, each held to its control's largest.
    #[must_use]
    #[allow(clippy::indexing_slicing, reason = "the index is below the length of both arrays, by the loop's bound")]
    pub const fn from_levers(levers: [u8; TONE_BYTES]) -> Self {
        let mut values = levers;
        let mut index = 0;
        while index < TONE_BYTES {
            if values[index] > TONE_MAXIMA[index] {
                values[index] = TONE_MAXIMA[index];
            }
            index = index.saturating_add(1);
        }
        Self { values }
    }

    /// A control's value.
    #[must_use]
    pub fn get(&self, control: Control) -> u8 {
        self.values.get(control as usize).copied().unwrap_or(0)
    }

    /// Sets a control, held to its largest value.
    pub fn set(&mut self, control: Control, value: u8) {
        if let Some(slot) = self.values.get_mut(control as usize) {
            *slot = value.min(control.max());
        }
    }

    /// Whether a switch is on.
    #[must_use]
    pub fn on(&self, control: Control) -> bool {
        self.get(control) != 0
    }

    /// Reads a tone from its 26 bytes.
    ///
    /// # Errors
    ///
    /// [`PatchError::Value`] with the first byte that is above its
    /// control's largest value.
    pub fn from_bytes(bytes: &[u8; TONE_BYTES]) -> Result<Self, PatchError> {
        check(bytes, &TONE_MAXIMA, 0)?;
        Ok(Self { values: *bytes })
    }

    /// The tone as its 26 bytes.
    #[must_use]
    pub const fn to_bytes(&self) -> [u8; TONE_BYTES] {
        self.values
    }
}

/// The controls outside the tones, each named by its byte after the two
/// tones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Global {
    /// FEET of channel I: a [`Feet`].
    FeetOne = 0,
    /// FEET of channel II: a [`Feet`].
    FeetTwo = 1,
    /// DETUNE CH II, flat to sharp about the centre.
    Detune = 2,
    /// MIX: channel I alone at 0, channel II alone at 127.
    Mix = 3,
    /// Overall BRILLIANCE, duller to brighter about the centre.
    Brilliance = 4,
    /// Overall RESONANCE, added to both filters of both channels; it
    /// rests at 0, the instrument's lever up.
    Resonance = 5,
    /// KEYBOARD CONTROL, BRILLIANCE LOW: the lower keys duller or
    /// brighter about the centre.
    KeyboardBrillianceLow = 6,
    /// KEYBOARD CONTROL, BRILLIANCE HIGH: the upper keys.
    KeyboardBrillianceHigh = 7,
    /// KEYBOARD CONTROL, LEVEL LOW: the lower keys quieter or louder
    /// about the centre.
    KeyboardLevelLow = 8,
    /// KEYBOARD CONTROL, LEVEL HIGH: the upper keys.
    KeyboardLevelHigh = 9,
    /// SUB OSCILLATOR FUNCTION: a [`Function`].
    SubFunction = 10,
    /// SUB OSCILLATOR SPEED.
    SubSpeed = 11,
    /// SUB OSCILLATOR VCO: how far it moves the pitch.
    SubVco = 12,
    /// SUB OSCILLATOR VCF: how far it moves the filters.
    SubVcf = 13,
    /// SUB OSCILLATOR VCA: how far it moves the level.
    SubVca = 14,
    /// TOUCH RESPONSE PITCHBEND: how far below its note a hard stroke
    /// starts.
    TouchPitchBend = 15,
    /// TOUCH RESPONSE SPEED: how far pressure speeds the sub oscillator.
    TouchSpeed = 16,
    /// TOUCH RESPONSE VCO: how far pressure lets the sub oscillator move
    /// the pitch.
    TouchVco = 17,
    /// TOUCH RESPONSE VCF: how far pressure lets it move the filters.
    TouchVcf = 18,
    /// RING MODULATOR ATTACK TIME.
    RingAttack = 19,
    /// RING MODULATOR DECAY TIME.
    RingDecay = 20,
    /// RING MODULATOR DEPTH: how far its envelope raises its speed.
    RingDepth = 21,
    /// RING MODULATOR SPEED.
    RingSpeed = 22,
    /// RING MODULATOR MODULATION: none to all of the sound.
    RingModulation = 23,
    /// SUSTAIN I or II: 0 or 1.
    SustainMode = 24,
    /// SUSTAIN, short to long.
    Sustain = 25,
    /// PORTAMENTO or GLISSANDO: 0 or 1.
    Glissando = 26,
    /// PORTAMENTO/GLISSANDO, short to long.
    GlideTime = 27,
    /// TREMOLO/CHORUS ON: 0 or 1.
    Effect = 28,
    /// The CHORUS switch: 0 or 1.
    Chorus = 29,
    /// The TREMOLO switch: 0 or 1.
    Tremolo = 30,
    /// TREMOLO/CHORUS SPEED.
    EffectSpeed = 31,
    /// TREMOLO/CHORUS DEPTH.
    EffectDepth = 32,
}

/// The largest position of a selector of six.
const SELECTOR_MAX: u8 = 5;

impl Global {
    /// The largest value of the control's byte.
    #[must_use]
    pub const fn max(self) -> u8 {
        match self {
            Self::FeetOne | Self::FeetTwo | Self::SubFunction => SELECTOR_MAX,
            Self::SustainMode | Self::Glissando | Self::Effect | Self::Chorus | Self::Tremolo => 1,
            _ => LEVER_MAX,
        }
    }
}

/// The largest value of each byte outside the tones.
#[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build, and no program")]
pub(super) const GLOBAL_MAXIMA: [u8; GLOBAL_BYTES] = {
    let mut maxima = [LEVER_MAX; GLOBAL_BYTES];
    maxima[Global::FeetOne as usize] = SELECTOR_MAX;
    maxima[Global::FeetTwo as usize] = SELECTOR_MAX;
    maxima[Global::SubFunction as usize] = SELECTOR_MAX;
    maxima[Global::SustainMode as usize] = 1;
    maxima[Global::Glissando as usize] = 1;
    maxima[Global::Effect as usize] = 1;
    maxima[Global::Chorus as usize] = 1;
    maxima[Global::Tremolo as usize] = 1;
    maxima
};

/// A channel's pitch against the keyboard's, as an organ's stops are
/// named.
///
/// Source: owner's manual, page 8, item 5: "If 8' is considered normal
/// pitch, then 16' is one octave below, 5-1/3' is a perfect fifth up, 4'
/// is one octave up, 2-2/3' is one octave plus a perfect fifth up, and 2'
/// is two octaves up."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Feet {
    /// 16'.
    Sixteen = 0,
    /// 8'.
    Eight = 1,
    /// 5 1/3'.
    FiveAndThird = 2,
    /// 4'.
    Four = 3,
    /// 2 2/3'.
    TwoAndTwoThirds = 4,
    /// 2'.
    Two = 5,
}

impl Feet {
    /// The stop of a selector's position; 8' for one there is not.
    #[must_use]
    pub const fn from_position(position: u8) -> Self {
        match position {
            0 => Self::Sixteen,
            2 => Self::FiveAndThird,
            3 => Self::Four,
            4 => Self::TwoAndTwoThirds,
            5 => Self::Two,
            _ => Self::Eight,
        }
    }
}

/// The sub oscillator's waveform.
///
/// Source: owner's manual, page 9, item 11 A.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Function {
    /// A sine wave.
    Sine = 0,
    /// "A sawtooth wave with rapid beginning and slow decay".
    Sawtooth = 1,
    /// "An inverted sawtooth wave with slow beginning and rapid decay".
    InvertedSawtooth = 2,
    /// A square wave.
    Square = 3,
    /// White noise.
    Noise = 4,
    /// The external input: with nothing connected, as here, "an 'off'
    /// mode for the whole sub oscillator section".
    External = 5,
}

impl Function {
    /// The waveform of a selector's position; the external input for one
    /// there is not.
    #[must_use]
    pub const fn from_position(position: u8) -> Self {
        match position {
            0 => Self::Sine,
            1 => Self::Sawtooth,
            2 => Self::InvertedSawtooth,
            3 => Self::Square,
            4 => Self::Noise,
            _ => Self::External,
        }
    }
}

/// Why bytes are not a patch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchError {
    /// The byte at this place is above the largest value of its control.
    Value(usize),
}

/// Whether every byte is within its largest value; `offset` is the place
/// of the first in the whole record.
fn check(bytes: &[u8], maxima: &[u8], offset: usize) -> Result<(), PatchError> {
    match bytes.iter().zip(maxima).position(|(value, max)| value > max) {
        Some(place) => Err(PatchError::Value(offset.saturating_add(place))),
        None => Ok(()),
    }
}

/// The whole panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    /// The tones of channel I and channel II.
    pub tones: [Tone; 2],
    globals: [u8; GLOBAL_BYTES],
}

impl Patch {
    /// The panel at rest: [`Tone::DEFAULT`] on both channels, channel I
    /// alone, both at 8', every lever that rests at its centre there, and
    /// everything else off or at its foot, the overall RESONANCE lever
    /// among them: its foot here is the instrument's lever up, where it
    /// adds least.
    ///
    /// Source: owner's manual, page 31, the diagram of the nominal
    /// settings, which draws MIX and BRILLIANCE at their centres and
    /// RESONANCE at its top; page 8, item 7: BRILLIANCE's "nominal lever
    /// position for all patches is centered"; page 38, item 8:
    /// RESONANCE's "Nominal setting is always with the lever up for
    /// minimum additional resonance". Own choice: the mix at channel I alone, where
    /// the diagram has it at the centre, so that the two channels' equal
    /// tones do not merely add.
    #[allow(
        clippy::indexing_slicing,
        reason = "worked out by the compiler: a fault here stops the build, and no program"
    )]
    pub const DEFAULT: Self = {
        let mut globals = [0; GLOBAL_BYTES];
        globals[Global::FeetOne as usize] = Feet::Eight as u8;
        globals[Global::FeetTwo as usize] = Feet::Eight as u8;
        globals[Global::Detune as usize] = super::law::LEVER_CENTRE;
        globals[Global::Brilliance as usize] = super::law::LEVER_CENTRE;
        globals[Global::KeyboardBrillianceLow as usize] = super::law::LEVER_CENTRE;
        globals[Global::KeyboardBrillianceHigh as usize] = super::law::LEVER_CENTRE;
        globals[Global::KeyboardLevelLow as usize] = super::law::LEVER_CENTRE;
        globals[Global::KeyboardLevelHigh as usize] = super::law::LEVER_CENTRE;
        globals[Global::SubFunction as usize] = Function::Sine as u8;
        Self { tones: [Tone::DEFAULT; 2], globals }
    };

    /// The panel at rest with these two tones, and the mix at the centre
    /// if they differ.
    #[must_use]
    pub fn with_tones(one: Tone, two: Tone) -> Self {
        let mut patch = Self::DEFAULT;
        patch.tones = [one, two];
        if one != two {
            patch.set(Global::Mix, super::law::LEVER_CENTRE);
        }
        patch
    }

    /// A control's value.
    #[must_use]
    pub fn get(&self, global: Global) -> u8 {
        self.globals.get(global as usize).copied().unwrap_or(0)
    }

    /// Sets a control, held to its largest value.
    pub fn set(&mut self, global: Global, value: u8) {
        if let Some(slot) = self.globals.get_mut(global as usize) {
            *slot = value.min(global.max());
        }
    }

    /// Whether a switch is on.
    #[must_use]
    pub fn on(&self, global: Global) -> bool {
        self.get(global) != 0
    }

    /// Reads a patch from its 85 bytes.
    ///
    /// # Errors
    ///
    /// [`PatchError::Value`] with the place of the first byte that is
    /// above its control's largest value.
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Result<Self, PatchError> {
        let (one, rest) = bytes.split_at(TONE_BYTES);
        let (two, rest) = rest.split_at(TONE_BYTES);
        check(one, &TONE_MAXIMA, 0)?;
        check(two, &TONE_MAXIMA, TONE_BYTES)?;
        check(rest, &GLOBAL_MAXIMA, TONE_BYTES.saturating_mul(2))?;
        let mut patch = Self::DEFAULT;
        for (tone, source) in patch.tones.iter_mut().zip([one, two]) {
            tone.values.iter_mut().zip(source).for_each(|(value, &byte)| *value = byte);
        }
        patch.globals.iter_mut().zip(rest).for_each(|(value, &byte)| *value = byte);
        Ok(patch)
    }

    /// The patch as its 85 bytes.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        let mut bytes = [0; PATCH_BYTES];
        let [one, two] = &self.tones;
        let values = one.values.iter().chain(&two.values).chain(&self.globals);
        bytes.iter_mut().zip(values).for_each(|(byte, &value)| *byte = value);
        bytes
    }
}
