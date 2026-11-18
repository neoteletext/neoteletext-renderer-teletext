//! A patch: the panel of the instrument as a fixed record, and its 18
//! values (S106-17, S106-18, S106-22).
//!
//! A synth106 patch is the Juno-106's 18 values: sixteen sliders and two
//! bytes of switches, which [`Patch::from_bytes`] reads exactly, checking
//! each. synth60's panel has the same sliders and three settings more
//! (the sub-oscillator switch, the envelope as the pulse width's source,
//! and chorus I+II), which the same record holds and which a synth60
//! patch carries as three JU-06A controller values after the 18 (S106-31,
//! [`super::interface`]).
//!
//! Source: Roland JUNO-106 owner's manual, page 35, "MIDI
//! Implementation", sections 3.1 to 3.4: the patch messages of 18 value
//! bytes and the parameter number table. Roland's JUNO-106/SYSTEM-8
//! Parameter Correspondence Table (2016) lists the same controls. Roland
//! JUNO-60 service notes, page 16, tables 3 and 4: synth60's
//! programmable switches.

/// Which instrument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    /// synth106.
    Synth106,
    /// synth60.
    Synth60,
}

/// Bytes of a synth106 patch.
///
/// Source: JUNO-106 owner's manual, MIDI Implementation 3.1: "(18 bytes
/// total for values)".
pub const PATCH_BYTES: usize = 18;

/// Sliders of a patch: the first sixteen bytes.
///
/// Source: JUNO-106 owner's manual, MIDI Implementation 3.4: parameter
/// numbers 0 to 15 are potentiometers.
pub const SLIDERS: usize = 16;

/// The largest value of a slider.
///
/// Source: JUNO-106 owner's manual, MIDI Implementation 3.1: "value 0 -
/// 127".
pub const SLIDER_MAX: u8 = 127;

/// The largest position of the high-pass filter's switch.
///
/// Source: JUNO-106 owner's manual, printed page 18: positions 0 to 3.
pub const HPF_MAX: u8 = 3;

/// The sliders, each named by its parameter number.
///
/// Source: JUNO-106 owner's manual, MIDI Implementation 3.4, "for
/// potentiometers".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Slider {
    /// The low-frequency oscillator's rate.
    LfoRate = 0,
    /// The low-frequency oscillator's delay time.
    LfoDelay = 1,
    /// How far the low-frequency oscillator moves the pitch.
    DcoLfo = 2,
    /// The pulse's width, or how far its source moves it.
    DcoPwm = 3,
    /// The noise's level.
    Noise = 4,
    /// The low-pass filter's cutoff frequency.
    VcfFrequency = 5,
    /// The low-pass filter's resonance.
    VcfResonance = 6,
    /// How far the envelope moves the cutoff.
    VcfEnvelope = 7,
    /// How far the low-frequency oscillator moves the cutoff.
    VcfLfo = 8,
    /// How far the cutoff follows the keyboard.
    VcfKeyboard = 9,
    /// The amplifier's level.
    VcaLevel = 10,
    /// The envelope's attack time.
    Attack = 11,
    /// The envelope's decay time.
    Decay = 12,
    /// The envelope's sustain level.
    Sustain = 13,
    /// The envelope's release time.
    Release = 14,
    /// The sub-oscillator's level.
    Sub = 15,
}

/// The oscillator's range, by the organ stop it is named after.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Range {
    /// 16': an octave below.
    Sixteen,
    /// 8': middle C is the third C of the keyboard.
    Eight,
    /// 4': an octave above.
    Four,
}

/// The chorus switches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChorusMode {
    /// No chorus.
    Off,
    /// Chorus I.
    One,
    /// Chorus II.
    Two,
    /// Both buttons down, which synth60 has and synth106 has not;
    /// synth106 takes it as II.
    Both,
}

/// What moves the pulse's width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwmSource {
    /// The low-frequency oscillator, as far as the slider says.
    Lfo,
    /// Nothing: the slider is the width.
    Manual,
    /// The envelope, which synth60 has and synth106 has not; the
    /// synth106 takes it as manual.
    Envelope,
}

/// Why eighteen bytes, or one parameter's value, are no patch (S106-19).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchError {
    /// The slider of this parameter number is above [`SLIDER_MAX`].
    Slider(u8),
    /// The first switch byte has its top bit set, or its three range bits
    /// are not exactly one of 16', 8' and 4'.
    Range,
    /// The second switch byte has one of its three top bits set.
    Switches,
    /// No parameter has this number: the Juno-106's are 0 to 17.
    Parameter(u8),
}

/// The parameter number of the first switch byte (S106-17).
pub const FIRST_SWITCHES: u8 = 16;
/// The parameter number of the second switch byte (S106-17).
pub const SECOND_SWITCHES: u8 = 17;
const _: () = assert!(FIRST_SWITCHES as usize == SLIDERS && SECOND_SWITCHES as usize + 1 == PATCH_BYTES);

/// Byte 16, bits 0 to 2: the range, one bit for each.
///
/// Source: JUNO-106 owner's manual, MIDI Implementation 3.4, "for
/// switches": "100 : 4', 010 : 8', 001 : 16'".
const RANGE_BITS: [(u8, Range); 3] = [(0b001, Range::Sixteen), (0b010, Range::Eight), (0b100, Range::Four)];
/// Byte 16: the mask of the range bits.
const RANGE_MASK: u8 = 0b111;
/// Byte 16, bit 3: the pulse is on. Source: the same table, "pulse 1:on".
const PULSE_BIT: u8 = 1 << 3;
/// Byte 16, bit 4: the sawtooth is on. Source: the same table, "saw 1:on".
const SAWTOOTH_BIT: u8 = 1 << 4;
/// Byte 16, bit 5: the chorus is off. Source: the same table, "chorus
/// 1:off 0:on".
const CHORUS_OFF_BIT: u8 = 1 << 5;
/// Byte 16, bit 6: the chorus, if on, is I and not II. Source: the same
/// table, "chorus 1: 1, 0: 2".
const CHORUS_ONE_BIT: u8 = 1 << 6;
/// Byte 17, bit 0: the pulse width is manual. Source: the same table,
/// "PWM 1:MAN 0:LFO".
const PWM_MANUAL_BIT: u8 = 1 << 0;
/// Byte 17, bit 1: the envelope moves the cutoff downwards. Source: the
/// same table, "ENV 1: - 0: +".
const ENVELOPE_INVERTED_BIT: u8 = 1 << 1;
/// Byte 17, bit 2: the gate, not the envelope, opens the amplifier.
/// Source: the same table, "VCA 1:gate 0:ENV".
const VCA_GATE_BIT: u8 = 1 << 2;
/// Byte 17, bits 3 and 4: three less the high-pass filter's position.
///
/// Source: the same table, "HP filter 11 : off, 10 : 1, 01 : 2, 00 : 3";
/// what it calls off is the switch's position 0 (JUNO-106 service notes,
/// module board schematic, the table of HPF A and B against modes 0 to
/// 3).
const HPF_SHIFT: u32 = 3;
/// Bytes 16 and 17: the bits of a data byte.
const DATA_MASK: u8 = 0x7f;
/// Byte 17: the bits that mean something.
const SWITCHES_MASK: u8 = 0b1_1111;

/// One patch. Every value of every field is a patch the instrument can
/// play: [`Patch::set`] holds a slider to its range, and the high-pass
/// position is held to [`HPF_MAX`] where it is used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::struct_excessive_bools, reason = "each is a switch of the panel, on or off")]
pub struct Patch {
    sliders: [u8; SLIDERS],
    /// The oscillator's range.
    pub range: Range,
    /// Whether the pulse sounds.
    pub pulse: bool,
    /// Whether the sawtooth sounds.
    pub sawtooth: bool,
    /// Whether the sub-oscillator sounds. synth106 has no such
    /// switch, only the level; its patches have this on.
    pub sub: bool,
    /// The chorus.
    pub chorus: ChorusMode,
    /// What moves the pulse's width.
    pub pwm: PwmSource,
    /// Whether the envelope moves the cutoff downwards.
    pub envelope_inverted: bool,
    /// Whether the gate, and not the envelope, opens the amplifier.
    pub vca_gate: bool,
    /// The high-pass filter's position, 0 to [`HPF_MAX`]; anything above
    /// is taken as [`HPF_MAX`].
    pub hpf: u8,
}

impl Patch {
    /// What synth106 plays when its Manual button is pressed with
    /// every slider down but the cutoff, the sustain and the amplifier's
    /// level: a sawtooth at 8' through an open filter, no chorus.
    ///
    /// Source: own choice of a plain starting sound.
    pub const DEFAULT: Self = Self {
        sliders: [0, 0, 0, 0, 0, SLIDER_MAX, 0, 0, 0, 0, SLIDER_MAX / 2 + 1, 0, 0, SLIDER_MAX, 0, 0],
        range: Range::Eight,
        pulse: false,
        sawtooth: true,
        sub: true,
        chorus: ChorusMode::Off,
        pwm: PwmSource::Manual,
        envelope_inverted: false,
        vca_gate: false,
        hpf: 1,
    };

    /// The plain starting sound of a model: [`Patch::DEFAULT`] with the
    /// high-pass switch where that model's filter is flat. At position 1,
    /// synth106's flat one, synth60 has a high-pass filter at 235
    /// Hz, which would leave its plain sound without its bass.
    ///
    /// Source: S106-21, own choice of a plain starting sound; the
    /// positions are the schematics' (`law::SYNTH106` and `law::SYNTH60`,
    /// `hpf`).
    #[must_use]
    pub const fn default_for(model: Model) -> Self {
        let mut patch = Self::DEFAULT;
        patch.hpf = match model {
            Model::Synth106 => Self::DEFAULT.hpf,
            Model::Synth60 => 0,
        };
        patch
    }

    /// Reads a patch from its 18 values (S106-13, S106-18), with the
    /// sub-oscillator switch on, the pulse width's source LFO or manual and
    /// the chorus off, I or II: what the 18 values say on synth60 too
    /// (S106-32).
    ///
    /// # Errors
    ///
    /// [`PatchError::Slider`] if one of the first sixteen bytes is above
    /// 127; [`PatchError::Range`] if byte 16 is above 127 or does not
    /// name exactly one range; [`PatchError::Switches`] if byte 17 has a
    /// bit set above the five that are defined.
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Result<Self, PatchError> {
        let mut patch = Self::DEFAULT;
        for (&value, number) in bytes.iter().zip(0u8..) {
            patch.set_parameter(number, value)?;
        }
        patch.sub = true;
        Ok(patch)
    }

    /// Sets parameter `number`, 0 to 17, to `value`, as the Juno-106's
    /// function type `32` message does (S106-17): a slider, or one of the
    /// two switch bytes whole. The sub-oscillator switch is left as it is;
    /// a switch byte sets the pulse width's source to LFO or manual and the
    /// chorus to off, I or II (S106-32). A value that is refused changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// As [`Patch::from_bytes`], for the one value, and
    /// [`PatchError::Parameter`] for a number above 17.
    pub fn set_parameter(&mut self, number: u8, value: u8) -> Result<(), PatchError> {
        match number {
            FIRST_SWITCHES => self.set_first_switches(value),
            SECOND_SWITCHES => self.set_second_switches(value),
            _ => {
                let place = self.sliders.get_mut(usize::from(number)).ok_or(PatchError::Parameter(number))?;
                if value > SLIDER_MAX {
                    return Err(PatchError::Slider(number));
                }
                *place = value;
                Ok(())
            }
        }
    }

    /// Parameter `number`'s value, 0 to 17, as the 18 values hold it.
    #[must_use]
    pub fn parameter(&self, number: u8) -> Option<u8> {
        match number {
            FIRST_SWITCHES => Some(self.first_switches()),
            SECOND_SWITCHES => Some(self.second_switches()),
            _ => self.sliders.get(usize::from(number)).copied(),
        }
    }

    /// Reads the first switch byte (S106-18): range, pulse, sawtooth and
    /// chorus.
    fn set_first_switches(&mut self, first: u8) -> Result<(), PatchError> {
        let range = RANGE_BITS.iter().find(|(bits, _)| first & RANGE_MASK == *bits).map(|&(_, range)| range);
        let Some(range) = range.filter(|_| first & !DATA_MASK == 0) else {
            return Err(PatchError::Range);
        };
        self.range = range;
        self.pulse = first & PULSE_BIT != 0;
        self.sawtooth = first & SAWTOOTH_BIT != 0;
        self.chorus = if first & CHORUS_OFF_BIT != 0 {
            ChorusMode::Off
        } else if first & CHORUS_ONE_BIT != 0 {
            ChorusMode::One
        } else {
            ChorusMode::Two
        };
        Ok(())
    }

    /// Reads the second switch byte (S106-18): the pulse width's source,
    /// the envelope's polarity, the amplifier's control and the high-pass
    /// filter.
    fn set_second_switches(&mut self, second: u8) -> Result<(), PatchError> {
        if second & !SWITCHES_MASK != 0 {
            return Err(PatchError::Switches);
        }
        self.pwm = if second & PWM_MANUAL_BIT != 0 { PwmSource::Manual } else { PwmSource::Lfo };
        self.envelope_inverted = second & ENVELOPE_INVERTED_BIT != 0;
        self.vca_gate = second & VCA_GATE_BIT != 0;
        self.hpf = HPF_MAX.saturating_sub((second >> HPF_SHIFT) & HPF_MAX);
        Ok(())
    }

    /// The first switch byte, as a sender writes it: chorus off as bits 6
    /// and 5 at `01`, I+II as II (S106-31; `server.md`, SRV-30).
    fn first_switches(&self) -> u8 {
        let range = RANGE_BITS.iter().find(|(_, range)| *range == self.range).map_or(0, |&(bits, _)| bits);
        let chorus = match self.chorus {
            // As the owner's manual's own example of a patch writes it.
            ChorusMode::Off => CHORUS_OFF_BIT,
            ChorusMode::One => CHORUS_ONE_BIT,
            ChorusMode::Two | ChorusMode::Both => 0,
        };
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        range | flag(self.pulse, PULSE_BIT) | flag(self.sawtooth, SAWTOOTH_BIT) | chorus
    }

    /// The second switch byte, as a sender writes it: the envelope as the
    /// pulse width's source as manual (S106-31).
    fn second_switches(&self) -> u8 {
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        flag(self.pwm != PwmSource::Lfo, PWM_MANUAL_BIT)
            | flag(self.envelope_inverted, ENVELOPE_INVERTED_BIT)
            | flag(self.vca_gate, VCA_GATE_BIT)
            | (HPF_MAX.saturating_sub(self.hpf.min(HPF_MAX)) << HPF_SHIFT)
    }

    /// The patch as its 18 values, as a sender writes them (S106-31):
    /// I+II is written as II and the envelope as the pulse width's source
    /// as manual, which a synth60 patch's three controller values then
    /// refine; the sub-oscillator's switch is not among them.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        let mut bytes = [0u8; PATCH_BYTES];
        for (byte, number) in bytes.iter_mut().zip(0u8..) {
            *byte = self.parameter(number).unwrap_or(0);
        }
        bytes
    }

    /// A slider's value, 0 to [`SLIDER_MAX`].
    #[must_use]
    pub fn get(&self, slider: Slider) -> u8 {
        self.sliders.get(usize::from(slider as u8)).copied().unwrap_or(0)
    }

    /// Sets a slider; a value above [`SLIDER_MAX`] is taken as it.
    pub fn set(&mut self, slider: Slider, value: u8) {
        if let Some(kept) = self.sliders.get_mut(usize::from(slider as u8)) {
            *kept = value.min(SLIDER_MAX);
        }
    }

    /// The patch with a slider set: for building one in a chain.
    #[must_use]
    pub fn with(mut self, slider: Slider, value: u8) -> Self {
        self.set(slider, value);
        self
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self::DEFAULT
    }
}
