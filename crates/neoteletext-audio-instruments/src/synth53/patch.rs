//! A program: the panel of the instrument as a fixed record.
//!
//! # The layout, and why this one
//!
//! The record is the instrument's own: the 24 bytes of a program in its
//! memory, as the operation manual sets them out for whoever reads the
//! memory through the serial interface. Each byte holds one knob in its
//! low seven bits, the number the instrument's converter made of the
//! knob when the program was stored, and one switch in its top bit. The
//! last two bytes have a knob and no switch.
//!
//! The 2020 reissue's program dump (Sequential LLC, "Prophet-5 MIDI
//! Implementation" 1.4, page 12: 128 bytes, one for each parameter) is a
//! documented layout of the same controls too. It was not chosen: it
//! carries a dozen parameters the original has not (velocity, pressure,
//! Vintage, the count and detuning of voices in unison), and its knobs
//! run from 0 to 120, where the instrument emulated here stores what
//! its own converter measures, 0 to 127.
//!
//! Every value of every byte is a program: a knob has all 128 values of
//! its seven bits, a switch both of its one, and the two top bits that
//! mean nothing are not kept.
//!
//! Source: Sequential Circuits, "Prophet-5 Synthesizer Operation
//! Manual" (CM1000D, February 1982, for revision 3.3), section 6-7,
//! "Status 3: Save to tape" (pages 42 and 43 of the scan): "a file is
//! organized as 40 24-byte programs"; "The least-significant seven bits
//! of each byte represent a programmable potentiometer setting of 0-127
//! steps, while the MSB represents a switch setting (1=on, 0=off)"; and
//! the table of the 24 bytes, which [`Knob`] and [`Switch`] follow.

/// Bytes of a program.
///
/// Source: the operation manual, section 6-7: "40 24-byte programs".
pub const PATCH_BYTES: usize = 24;

/// The largest value of a knob.
///
/// Source: the operation manual, section 6-7: "a programmable
/// potentiometer setting of 0-127 steps".
pub const KNOB_MAX: u8 = 127;

/// The number the converter makes of a knob at a position of its dial,
/// given in tenths of the dial's 0 to 10: twelve steps to each mark, as
/// revision 3 is trimmed, on which the dial's 10 is 120 and the numbers
/// above it are beyond the knob's travel.
///
/// Source: TM3 adjustment 4-14 (page 67): the converter's gain is set so
/// that the cutoff knob at 10 gives "as close to 10.000V as the 83-mV
/// quantized voltage steps will allow", 120 steps.
#[must_use]
pub const fn dial(tenths: u8) -> u8 {
    let steps = (tenths as u16).saturating_mul(12).saturating_add(5) / 10;
    if steps > KNOB_MAX as u16 { KNOB_MAX } else { steps as u8 }
}

/// The bit of a byte that holds its switch.
///
/// Source: the same sentence: "the MSB represents a switch setting".
const SWITCH_BIT: u8 = 0x80;

/// The knobs, each named by the byte whose low seven bits hold it.
///
/// Source: the operation manual, section 6-7, the table of data bytes
/// (page 43 of the scan).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Knob {
    /// The filter envelope's attack.
    FilterAttack = 0,
    /// The filter envelope's decay.
    FilterDecay = 1,
    /// The filter envelope's sustain level.
    FilterSustain = 2,
    /// The filter envelope's release.
    FilterRelease = 3,
    /// The amplifier envelope's attack.
    AmplifierAttack = 4,
    /// The amplifier envelope's decay.
    AmplifierDecay = 5,
    /// The amplifier envelope's sustain level.
    AmplifierSustain = 6,
    /// The amplifier envelope's release.
    AmplifierRelease = 7,
    /// The filter's cutoff.
    Cutoff = 8,
    /// How far the filter envelope moves the cutoff.
    FilterEnvelopeAmount = 9,
    /// The mixer's level of oscillator B.
    MixerOscB = 10,
    /// Oscillator B's pulse width.
    OscBPulseWidth = 11,
    /// The mixer's level of oscillator A.
    MixerOscA = 12,
    /// Oscillator A's pulse width.
    OscAPulseWidth = 13,
    /// The mixer's level of the noise.
    MixerNoise = 14,
    /// The filter's resonance.
    Resonance = 15,
    /// The glide's rate.
    Glide = 16,
    /// The low-frequency oscillator's frequency.
    LfoFrequency = 17,
    /// Wheel-Mod's source mix: the low-frequency oscillator at 0, noise
    /// at the top.
    SourceMix = 18,
    /// Poly-Mod's amount of oscillator B.
    PolyModOscB = 19,
    /// Poly-Mod's amount of the filter envelope.
    PolyModFilterEnvelope = 20,
    /// Oscillator A's frequency.
    OscAFrequency = 21,
    /// Oscillator B's frequency.
    OscBFrequency = 22,
    /// Oscillator B's fine tuning, upwards from the frequency knob's
    /// pitch.
    OscBFine = 23,
}

/// The switches, each named by the byte whose top bit holds it.
///
/// Source: the operation manual, section 6-7, the same table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Switch {
    /// Oscillator A's pulse.
    OscAPulse = 0,
    /// Oscillator A's sawtooth.
    OscASawtooth = 1,
    /// Sync: oscillator A follows oscillator B.
    Sync = 2,
    /// Oscillator B's sawtooth.
    OscBSawtooth = 3,
    /// Oscillator B's triangle.
    OscBTriangle = 4,
    /// Oscillator B's pulse.
    OscBPulse = 5,
    /// Oscillator B's keyboard switch: off, it ignores the keys.
    OscBKeyboard = 6,
    /// Unison.
    Unison = 7,
    /// Poly-Mod to oscillator A's frequency.
    PolyModFrequencyA = 8,
    /// Poly-Mod to oscillator A's pulse width.
    PolyModPulseWidthA = 9,
    /// Poly-Mod to the filter's cutoff.
    PolyModFilter = 10,
    /// The low-frequency oscillator's sawtooth.
    LfoSawtooth = 11,
    /// The low-frequency oscillator's triangle.
    LfoTriangle = 12,
    /// The low-frequency oscillator's square.
    LfoSquare = 13,
    /// The filter's keyboard switch: on, the cutoff follows the keys.
    FilterKeyboard = 14,
    /// Release: off, both envelopes' releases are as short as they go,
    /// whatever their knobs say, unless the footswitch is down.
    Release = 15,
    /// Wheel-Mod to oscillator A's frequency.
    WheelFrequencyA = 16,
    /// Wheel-Mod to oscillator B's frequency.
    WheelFrequencyB = 17,
    /// Wheel-Mod to oscillator A's pulse width.
    WheelPulseWidthA = 18,
    /// Wheel-Mod to oscillator B's pulse width.
    WheelPulseWidthB = 19,
    /// Wheel-Mod to the filter's cutoff.
    WheelFilter = 20,
    /// Oscillator B's low-frequency switch.
    OscBLowFrequency = 21,
}

/// The first byte whose top bit holds no switch.
///
/// Source: the operation manual's table marks the switch of bytes 22
/// and 23 "X".
const SWITCH_BYTES: usize = 22;
const _: () = assert!(Switch::OscBLowFrequency as usize == SWITCH_BYTES - 1);
const _: () = assert!(Knob::OscBFine as usize == PATCH_BYTES - 1);

/// One program. Every value of every field is a program the instrument
/// can play.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    bytes: [u8; PATCH_BYTES],
}

impl Patch {
    /// A plain starting sound: oscillator A's sawtooth, two octaves up
    /// its knob, through an open filter that follows the keys; both
    /// envelopes at full sustain; the release switch on.
    ///
    /// Source: own choice.
    #[allow(
        clippy::indexing_slicing,
        reason = "worked out by the compiler: a fault here stops the build, and no program"
    )]
    pub const DEFAULT: Self = {
        let mut bytes = [0u8; PATCH_BYTES];
        let middle = KNOB_MAX / 2 + 1;
        bytes[Knob::OscAFrequency as usize] = middle;
        bytes[Knob::OscBFrequency as usize] = middle;
        bytes[Knob::OscAPulseWidth as usize] = middle;
        bytes[Knob::OscBPulseWidth as usize] = middle;
        bytes[Knob::MixerOscA as usize] = KNOB_MAX;
        bytes[Knob::Cutoff as usize] = KNOB_MAX;
        bytes[Knob::FilterSustain as usize] = KNOB_MAX;
        bytes[Knob::AmplifierSustain as usize] = KNOB_MAX;
        bytes[Switch::OscASawtooth as usize] |= SWITCH_BIT;
        bytes[Switch::OscBKeyboard as usize] |= SWITCH_BIT;
        bytes[Switch::FilterKeyboard as usize] |= SWITCH_BIT;
        bytes[Switch::LfoTriangle as usize] |= SWITCH_BIT;
        bytes[Switch::Release as usize] |= SWITCH_BIT;
        Self { bytes }
    };

    /// Reads a program from its 24 bytes. Nothing is refused, for every
    /// value of every byte is a setting of a knob and a switch; the top
    /// bits of the last two bytes, which hold no switch, are not kept.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Self {
        let mut kept = *bytes;
        for byte in kept.iter_mut().skip(SWITCH_BYTES) {
            *byte &= KNOB_MAX;
        }
        Self { bytes: kept }
    }

    /// The program as its 24 bytes.
    #[must_use]
    pub const fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        self.bytes
    }

    /// A knob's value, 0 to [`KNOB_MAX`].
    #[must_use]
    pub fn get(&self, knob: Knob) -> u8 {
        self.bytes.get(usize::from(knob as u8)).map_or(0, |byte| byte & KNOB_MAX)
    }

    /// Whether a switch is on.
    #[must_use]
    pub fn on(&self, switch: Switch) -> bool {
        self.bytes.get(usize::from(switch as u8)).is_some_and(|byte| byte & SWITCH_BIT != 0)
    }

    /// Sets a knob; a value above [`KNOB_MAX`] is taken as it.
    pub fn set(&mut self, knob: Knob, value: u8) {
        if let Some(byte) = self.bytes.get_mut(usize::from(knob as u8)) {
            *byte = (*byte & SWITCH_BIT) | value.min(KNOB_MAX);
        }
    }

    /// Sets a switch.
    pub fn switch(&mut self, switch: Switch, on: bool) {
        if let Some(byte) = self.bytes.get_mut(usize::from(switch as u8)) {
            *byte = (*byte & KNOB_MAX) | if on { SWITCH_BIT } else { 0 };
        }
    }

    /// The program with a knob set: for building one in a chain.
    #[must_use]
    pub fn with(mut self, knob: Knob, value: u8) -> Self {
        self.set(knob, value);
        self
    }

    /// The program with a switch set: for building one in a chain.
    #[must_use]
    pub fn with_switch(mut self, switch: Switch, on: bool) -> Self {
        self.switch(switch, on);
        self
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self::DEFAULT
    }
}
