//! A patch: the panel of the instrument as a fixed record.
//!
//! The Juno-106 sends and takes a patch as 18 bytes, sixteen sliders and
//! two bytes of switches. The Juno-60's panel has the same sliders and a
//! few more switch positions (a switch for the sub-oscillator, a third
//! source for the pulse width, a third chorus setting); the Juno-60 itself
//! has no MIDI, so it has no byte form of its own. The audio profile's
//! patch is the Juno-106's 18 bytes followed by two of NeoTeletext's own
//! that carry those three settings ([`PATCH_BYTES`], the owner's decision
//! of 18 November 2026), and [`Patch::from_bytes`] reads and checks all
//! twenty.
//!
//! Source: Roland JUNO-106 owner's manual, "MIDI Implementation",
//! sections 3.1 to 3.4 (the last printed page): the patch messages of 18
//! value bytes and the parameter number table. Roland's JUNO-106/SYSTEM-8
//! Parameter Correspondence Table (2016) lists the same controls. Roland
//! JUNO-60 service notes, page 16, tables 3 and 4: the Juno-60's
//! programmable switches.

use super::Model;

/// Bytes of a Juno-106 patch as the instrument sends it over MIDI.
///
/// Source: JUNO-106 owner's manual, MIDI Implementation 3.1: "(18 bytes
/// total for values)".
pub const MIDI_PATCH_BYTES: usize = 18;

/// Bytes of a patch in the audio profile: the Juno-106's
/// [`MIDI_PATCH_BYTES`], then byte 18 holding the Juno-60's three settings
/// the Juno-106 has not ([`SUB_OFF_BIT`], [`PWM_ENVELOPE_BIT`],
/// [`CHORUS_BOTH_BIT`]) and byte 19, reserved and zero.
///
/// Source: own choice, the owner's of 18 November 2026: the patch grows by
/// two bytes so that both models use it.
pub const PATCH_BYTES: usize = MIDI_PATCH_BYTES + 2;

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
    /// Both buttons down, which the Juno-60 has and the Juno-106 has not;
    /// the Juno-106 takes it as II.
    Both,
}

/// What moves the pulse's width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwmSource {
    /// The low-frequency oscillator, as far as the slider says.
    Lfo,
    /// Nothing: the slider is the width.
    Manual,
    /// The envelope, which the Juno-60 has and the Juno-106 has not; the
    /// Juno-106 takes it as manual.
    Envelope,
}

/// Why eighteen bytes are no patch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchError {
    /// The slider of this parameter number is above [`SLIDER_MAX`].
    Slider(u8),
    /// The first switch byte has its top bit set, or its three range bits
    /// are not exactly one of 16', 8' and 4'.
    Range,
    /// The second switch byte has one of its three top bits set.
    Switches,
    /// Byte 18 has a bit set above the three that are defined, or asks for
    /// chorus I+II while byte 16 says the chorus is off; or byte 19, which
    /// is reserved, is not zero.
    Juno60,
}

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
/// Byte 18, bit 0: the Juno-60's sub-oscillator switch is off. Clear on
/// every Juno-106 patch, whose sub-oscillator has a level and no switch.
pub const SUB_OFF_BIT: u8 = 1 << 0;
/// Byte 18, bit 1: the envelope moves the pulse's width, the Juno-60's
/// third source. Byte 17 then says manual, the nearest the Juno-106 has.
pub const PWM_ENVELOPE_BIT: u8 = 1 << 1;
/// Byte 18, bit 2: both chorus buttons are down, the Juno-60's I+II. Byte
/// 16 then says II, the nearest the Juno-106 has.
pub const CHORUS_BOTH_BIT: u8 = 1 << 2;
/// Byte 18: the bits that mean something.
const JUNO_60_MASK: u8 = SUB_OFF_BIT | PWM_ENVELOPE_BIT | CHORUS_BOTH_BIT;

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
    /// Whether the sub-oscillator sounds. The Juno-106 has no such
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
    /// What the Juno-106 plays when its Manual button is pressed with
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
    /// the Juno-106's flat one, the Juno-60 has a high-pass filter at 235
    /// Hz, which would leave its plain sound without its bass.
    ///
    /// Source: own choice of a plain starting sound; the positions are
    /// the schematics' ([`law::JUNO_106`](super::law::JUNO_106) and
    /// [`law::JUNO_60`](super::law::JUNO_60), `hpf`).
    #[must_use]
    pub const fn default_for(model: Model) -> Self {
        let mut patch = Self::DEFAULT;
        patch.hpf = match model {
            Model::Juno106 => Self::DEFAULT.hpf,
            Model::Juno60 => 0,
        };
        patch
    }

    /// Reads a patch from its [`PATCH_BYTES`]: the Juno-106's 18, then the
    /// Juno-60's settings.
    ///
    /// # Errors
    ///
    /// [`PatchError::Slider`] if one of the first sixteen bytes is above
    /// 127; [`PatchError::Range`] if byte 16 is above 127 or does not
    /// name exactly one range; [`PatchError::Switches`] if byte 17 has a
    /// bit set above the five that are defined; [`PatchError::Juno60`] if
    /// byte 18 has a bit set above its three, asks for chorus I+II with
    /// the chorus off, or byte 19 is not zero.
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Result<Self, PatchError> {
        let [values @ .., first, second, juno_60, reserved] = bytes;
        let mut sliders = [0u8; SLIDERS];
        for ((slider, &value), number) in sliders.iter_mut().zip(values).zip(0u8..) {
            if value > SLIDER_MAX {
                return Err(PatchError::Slider(number));
            }
            *slider = value;
        }
        let range = RANGE_BITS.iter().find(|(bits, _)| first & RANGE_MASK == *bits).map(|&(_, range)| range);
        let Some(range) = range.filter(|_| first & !DATA_MASK == 0) else {
            return Err(PatchError::Range);
        };
        if second & !SWITCHES_MASK != 0 {
            return Err(PatchError::Switches);
        }
        let both = juno_60 & CHORUS_BOTH_BIT != 0;
        if juno_60 & !JUNO_60_MASK != 0 || *reserved != 0 || (both && first & CHORUS_OFF_BIT != 0) {
            return Err(PatchError::Juno60);
        }
        Ok(Self {
            sliders,
            range,
            pulse: first & PULSE_BIT != 0,
            sawtooth: first & SAWTOOTH_BIT != 0,
            sub: juno_60 & SUB_OFF_BIT == 0,
            chorus: if first & CHORUS_OFF_BIT != 0 {
                ChorusMode::Off
            } else if both {
                ChorusMode::Both
            } else if first & CHORUS_ONE_BIT != 0 {
                ChorusMode::One
            } else {
                ChorusMode::Two
            },
            pwm: if juno_60 & PWM_ENVELOPE_BIT != 0 {
                PwmSource::Envelope
            } else if second & PWM_MANUAL_BIT != 0 {
                PwmSource::Manual
            } else {
                PwmSource::Lfo
            },
            envelope_inverted: second & ENVELOPE_INVERTED_BIT != 0,
            vca_gate: second & VCA_GATE_BIT != 0,
            hpf: HPF_MAX.saturating_sub((second >> HPF_SHIFT) & HPF_MAX),
        })
    }

    /// The patch as its [`PATCH_BYTES`]. The Juno-106's 18 hold the
    /// nearest that instrument has (I+II as II, the envelope as the pulse
    /// width's source as manual) and byte 18 the Juno-60's own settings, so
    /// that reading the bytes gives this patch back.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        let mut bytes = [0u8; PATCH_BYTES];
        let [values @ .., first, second, juno_60, _reserved] = &mut bytes;
        *values = self.sliders;
        let range = RANGE_BITS.iter().find(|(_, range)| *range == self.range).map_or(0, |&(bits, _)| bits);
        let chorus = match self.chorus {
            // As the owner's manual's own example of a patch writes it.
            ChorusMode::Off => CHORUS_OFF_BIT,
            ChorusMode::One => CHORUS_ONE_BIT,
            ChorusMode::Two | ChorusMode::Both => 0,
        };
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        *first = range | flag(self.pulse, PULSE_BIT) | flag(self.sawtooth, SAWTOOTH_BIT) | chorus;
        *second = flag(self.pwm != PwmSource::Lfo, PWM_MANUAL_BIT)
            | flag(self.envelope_inverted, ENVELOPE_INVERTED_BIT)
            | flag(self.vca_gate, VCA_GATE_BIT)
            | (HPF_MAX.saturating_sub(self.hpf.min(HPF_MAX)) << HPF_SHIFT);
        *juno_60 = flag(!self.sub, SUB_OFF_BIT)
            | flag(self.pwm == PwmSource::Envelope, PWM_ENVELOPE_BIT)
            | flag(self.chorus == ChorusMode::Both, CHORUS_BOTH_BIT);
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
