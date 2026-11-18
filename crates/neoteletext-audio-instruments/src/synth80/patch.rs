//! A patch: the panel of the instrument as 85 control-change values in a
//! fixed order (`neoteletext-common/docs/audio/synth80.md`, cited here as
//! "S80-n": S80-33).
//!
//! Neither the instrument modelled nor the adopted interface (Deckard's
//! Dream's MIDI implementation, DD manual revision 1.3.0, PDF pages 71 to
//! 74) has a patch format, so a patch is the list of its control changes'
//! values (decision A4). A [`Tone`] is one row of the panel, the 26
//! controls that a tone selector button recalls and that a memory holds,
//! in the order the memory panel has them from left to right, which is
//! also the adopted table's order for each layer; a [`Patch`] is the tone
//! of channel I, the tone of channel II and the 33 master controls
//! ([`Global`]).
//!
//! | Bytes | What |
//! |---|---|
//! | 0 to 25 | Channel I's tone, one byte for each [`Control`] in order |
//! | 26 to 51 | Channel II's tone, the same |
//! | 52 to 84 | One byte for each [`Global`] in order |
//!
//! Byte k is always the value of the controller [`controller_of`] gives
//! for it, and every value 0 to 127 is valid in every byte (S80-16): a
//! lever is its position, value 0 its end of least effect (S80-13); a
//! switch is off at 0 to 64 and on at 65 to 127 (S80-14); a selector of
//! six takes the bands 0-21/22-42/43-63/64-84/85-105/106-127 (S80-15);
//! GLIDE MODE the bands 0-42/43-84/85-127 (S80-27). Only a byte above 127
//! is refused ([`PatchError`]).
//!
//! Source: S80-10, S80-11, S80-13 to S80-18, S80-20, S80-27 and S80-33;
//! owner's manual, the photograph inside the cover and its list of
//! controls, items 1 to 46, and page 36, the memory panel, whose legend
//! under the second memory gives the order of a tone's controls.

/// The largest value of a byte: a control change's seven bits (S80-33).
pub const LEVER_MAX: u8 = 127;
/// The middle of a lever that rests at its centre ("0-64-127", S80-13).
pub const LEVER_CENTRE: u8 = 64;
/// The lowest value at which a switch is on: the adopted table's band
/// "0-64/65-127" (S80-14).
pub const SWITCH_ON_FROM: u8 = 65;
/// A switch's canonical value off (S80-14).
pub const OFF: u8 = 0;
/// A switch's canonical value on (S80-14).
pub const ON: u8 = LEVER_MAX;
/// The lowest value of each band of a six-position selector, which is
/// also each position's canonical value: the adopted table's
/// "0-21/22-42/43-63/64-84/85-105/106-127" (S80-15).
pub const SELECTOR_BANDS: [u8; 6] = [0, 22, 43, 64, 85, 106];
/// The lowest value of each band of GLIDE MODE, and each position's
/// canonical value: the adopted table's "0-42/43-84/85-126", value 127
/// given to the last band (S80-27).
pub const GLIDE_BANDS: [u8; 3] = [0, 43, 85];

/// The position, 0 upwards, of the band of `bands` (each band's lowest
/// value, rising from 0) that holds `value`.
const fn band(value: u8, bands: &[u8]) -> usize {
    let mut position = 0;
    let mut rest = bands;
    let mut index: usize = 0;
    while let [lowest, others @ ..] = rest {
        if value >= *lowest {
            position = index;
        }
        rest = others;
        index = index.saturating_add(1);
    }
    position
}

/// What kind of control a byte is: how its value is read, and whether it
/// passes every place between two (S80-37).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A lever, 0 to 127 (S80-13).
    Lever,
    /// A switch of two positions (S80-14).
    Switch,
    /// A selector of six positions (S80-15).
    Selector,
    /// GLIDE MODE, of three positions (S80-27).
    Glide,
}

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
    /// What kind of control it is.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::Square | Self::Sawtooth => Kind::Switch,
            _ => Kind::Lever,
        }
    }
}

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
        values[Control::Sawtooth as usize] = ON;
        values[Control::Lpf as usize] = LEVER_MAX;
        values[Control::VcfLevel as usize] = LEVER_MAX;
        values[Control::Sustain as usize] = LEVER_MAX;
        values[Control::Level as usize] = LEVER_MAX;
        Self { values }
    };

    /// A tone from its 26 values, each held to 127.
    #[must_use]
    #[allow(clippy::indexing_slicing, reason = "the index is below the array's length, by the loop's bound")]
    pub const fn from_levers(levers: [u8; TONE_BYTES]) -> Self {
        let mut values = levers;
        let mut index = 0;
        while index < TONE_BYTES {
            if values[index] > LEVER_MAX {
                values[index] = LEVER_MAX;
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

    /// Sets a control to a value, held to 127: the value its controller
    /// carries (S80-10).
    pub fn set(&mut self, control: Control, value: u8) {
        if let Some(slot) = self.values.get_mut(control as usize) {
            *slot = value.min(LEVER_MAX);
        }
    }

    /// Whether a switch is on: at 65 to 127 (S80-14).
    #[must_use]
    pub fn on(&self, control: Control) -> bool {
        self.get(control) >= SWITCH_ON_FROM
    }

    /// Reads a tone from its 26 bytes.
    ///
    /// # Errors
    ///
    /// [`PatchError::Value`] with the first byte that is above 127.
    pub fn from_bytes(bytes: &[u8; TONE_BYTES]) -> Result<Self, PatchError> {
        check(bytes, 0)?;
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
    /// SUSTAIN I-II: a switch, on for SUSTAIN II.
    SustainMode = 24,
    /// SUSTAIN, short to long.
    Sustain = 25,
    /// The PORTAMENTO/GLISSANDO selector: a [`GlideMode`].
    GlideMode = 26,
    /// PORTAMENTO/GLISSANDO, short to long.
    GlideTime = 27,
    /// TREMOLO/CHORUS ON/OFF: a switch.
    Effect = 28,
    /// The CHORUS switch, on when down.
    Chorus = 29,
    /// The TREMOLO switch, on when down.
    Tremolo = 30,
    /// TREMOLO/CHORUS SPEED.
    EffectSpeed = 31,
    /// TREMOLO/CHORUS DEPTH.
    EffectDepth = 32,
}

impl Global {
    /// What kind of control it is.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::FeetOne | Self::FeetTwo | Self::SubFunction => Kind::Selector,
            Self::SustainMode | Self::Effect | Self::Chorus | Self::Tremolo => Kind::Switch,
            Self::GlideMode => Kind::Glide,
            _ => Kind::Lever,
        }
    }
}

/// The kind of each byte of a patch, in its order.
#[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build, and no program")]
pub const KINDS: [Kind; PATCH_BYTES] = {
    let mut kinds = [Kind::Lever; PATCH_BYTES];
    let mut channel = 0;
    while channel < 2 {
        kinds[channel * TONE_BYTES + Control::Square as usize] = Control::Square.kind();
        kinds[channel * TONE_BYTES + Control::Sawtooth as usize] = Control::Sawtooth.kind();
        channel += 1;
    }
    let globals = [
        Global::FeetOne,
        Global::FeetTwo,
        Global::SubFunction,
        Global::SustainMode,
        Global::GlideMode,
        Global::Effect,
        Global::Chorus,
        Global::Tremolo,
    ];
    let mut index = 0;
    while index < globals.len() {
        kinds[2 * TONE_BYTES + globals[index] as usize] = globals[index].kind();
        index += 1;
    }
    kinds
};

/// A channel's pitch against the keyboard's, as an organ's stops are
/// named. Each is named by its canonical value (S80-15).
///
/// Source: owner's manual, page 8, item 5: "If 8' is considered normal
/// pitch, then 16' is one octave below, 5-1/3' is a perfect fifth up, 4'
/// is one octave up, 2-2/3' is one octave plus a perfect fifth up, and 2'
/// is two octaves up."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Feet {
    /// 16'.
    Sixteen = SELECTOR_BANDS[0],
    /// 8'.
    Eight = SELECTOR_BANDS[1],
    /// 5 1/3'.
    FiveAndThird = SELECTOR_BANDS[2],
    /// 4'.
    Four = SELECTOR_BANDS[3],
    /// 2 2/3'.
    TwoAndTwoThirds = SELECTOR_BANDS[4],
    /// 2'.
    Two = SELECTOR_BANDS[5],
}

impl Feet {
    /// The stop a value selects, by the bands of S80-15, from the top of
    /// the panel: "At the top (16'), the pitch will be lowest. At the
    /// bottom (2'), the pitch will be highest" (DD manual, PDF page 35).
    #[must_use]
    pub const fn from_value(value: u8) -> Self {
        match band(value, &SELECTOR_BANDS) {
            0 => Self::Sixteen,
            1 => Self::Eight,
            2 => Self::FiveAndThird,
            3 => Self::Four,
            4 => Self::TwoAndTwoThirds,
            _ => Self::Two,
        }
    }
}

/// The sub oscillator's waveform, named by its canonical value (S80-15).
///
/// Source: owner's manual, page 9, item 11 A.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Function {
    /// A sine wave.
    Sine = SELECTOR_BANDS[0],
    /// "A sawtooth wave with rapid beginning and slow decay".
    Sawtooth = SELECTOR_BANDS[1],
    /// "An inverted sawtooth wave with slow beginning and rapid decay".
    InvertedSawtooth = SELECTOR_BANDS[2],
    /// A square wave.
    Square = SELECTOR_BANDS[3],
    /// White noise.
    Noise = SELECTOR_BANDS[4],
    /// The external input: with nothing connected, as here, "an 'off'
    /// mode for the whole sub oscillator section".
    External = SELECTOR_BANDS[5],
}

impl Function {
    /// The waveform a value selects, by the bands of S80-15, in the order
    /// the manuals give them from the top of the panel (DD manual, PDF
    /// page 36).
    #[must_use]
    pub const fn from_value(value: u8) -> Self {
        match band(value, &SELECTOR_BANDS) {
            0 => Self::Sine,
            1 => Self::Sawtooth,
            2 => Self::InvertedSawtooth,
            3 => Self::Square,
            4 => Self::Noise,
            _ => Self::External,
        }
    }
}

/// The PORTAMENTO/GLISSANDO selector, named by its canonical value
/// (S80-27).
///
/// Source: DD manual, PDF pages 41 and 42, the "3-setting switch", left
/// to right: "P or PORTAMENTO", centre with no glide, "G or GLISSANDO".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum GlideMode {
    /// A continuous glide.
    Portamento = GLIDE_BANDS[0],
    /// No glide: the pitch goes to the new key at once.
    Off = GLIDE_BANDS[1],
    /// A glide in semitone steps.
    Glissando = GLIDE_BANDS[2],
}

impl GlideMode {
    /// The position a value selects, by the bands of S80-27.
    #[must_use]
    pub const fn from_value(value: u8) -> Self {
        match band(value, &GLIDE_BANDS) {
            0 => Self::Portamento,
            1 => Self::Off,
            _ => Self::Glissando,
        }
    }
}

/// Why bytes are not a patch (S80-33).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchError {
    /// The byte at this place is above 127.
    Value(usize),
    /// The bytes are not 85: their number.
    Length(usize),
}

/// Whether every byte is a data byte; `offset` is the place of the first
/// in the whole record.
fn check(bytes: &[u8], offset: usize) -> Result<(), PatchError> {
    match bytes.iter().position(|&value| value > LEVER_MAX) {
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
        globals[Global::Detune as usize] = LEVER_CENTRE;
        globals[Global::Brilliance as usize] = LEVER_CENTRE;
        globals[Global::KeyboardBrillianceLow as usize] = LEVER_CENTRE;
        globals[Global::KeyboardBrillianceHigh as usize] = LEVER_CENTRE;
        globals[Global::KeyboardLevelLow as usize] = LEVER_CENTRE;
        globals[Global::KeyboardLevelHigh as usize] = LEVER_CENTRE;
        globals[Global::SubFunction as usize] = Function::Sine as u8;
        // S80-45: the glide at the selector's new off position.
        globals[Global::GlideMode as usize] = GlideMode::Off as u8;
        Self { tones: [Tone::DEFAULT; 2], globals }
    };

    /// The panel at rest with these two tones, and the mix at the centre
    /// if they differ.
    #[must_use]
    pub fn with_tones(one: Tone, two: Tone) -> Self {
        let mut patch = Self::DEFAULT;
        patch.tones = [one, two];
        if one != two {
            patch.set(Global::Mix, LEVER_CENTRE);
        }
        patch
    }

    /// A control's value.
    #[must_use]
    pub fn get(&self, global: Global) -> u8 {
        self.globals.get(global as usize).copied().unwrap_or(0)
    }

    /// Sets a control to a value, held to 127: the value its controller
    /// carries (S80-11).
    pub fn set(&mut self, global: Global, value: u8) {
        if let Some(slot) = self.globals.get_mut(global as usize) {
            *slot = value.min(LEVER_MAX);
        }
    }

    /// Whether a switch is on: at 65 to 127 (S80-14, S80-18).
    #[must_use]
    pub fn on(&self, global: Global) -> bool {
        self.get(global) >= SWITCH_ON_FROM
    }

    /// The value of byte `place`, 0 to 84; 0 past the end.
    #[must_use]
    pub fn byte(&self, place: usize) -> u8 {
        let [one, two] = &self.tones;
        match place.checked_sub(TONE_BYTES) {
            None => one.values.get(place),
            Some(second) => match second.checked_sub(TONE_BYTES) {
                None => two.values.get(second),
                Some(global) => self.globals.get(global),
            },
        }
        .copied()
        .unwrap_or(0)
    }

    /// Sets byte `place`, 0 to 84, to `value`, held to 127; past the end
    /// it does nothing.
    pub fn set_byte(&mut self, place: usize, value: u8) {
        let [one, two] = &mut self.tones;
        let slot = match place.checked_sub(TONE_BYTES) {
            None => one.values.get_mut(place),
            Some(second) => match second.checked_sub(TONE_BYTES) {
                None => two.values.get_mut(second),
                Some(global) => self.globals.get_mut(global),
            },
        };
        if let Some(slot) = slot {
            *slot = value.min(LEVER_MAX);
        }
    }

    /// Reads a patch from bytes of any length: it must be 85 (S80-33).
    ///
    /// # Errors
    ///
    /// [`PatchError::Length`] for another length, [`PatchError::Value`]
    /// for a byte above 127.
    pub fn read(bytes: &[u8]) -> Result<Self, PatchError> {
        let bytes: &[u8; PATCH_BYTES] = bytes.try_into().map_err(|_| PatchError::Length(bytes.len()))?;
        Self::from_bytes(bytes)
    }

    /// Reads a patch from its 85 bytes.
    ///
    /// # Errors
    ///
    /// [`PatchError::Value`] with the place of the first byte that is
    /// above 127.
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Result<Self, PatchError> {
        check(bytes, 0)?;
        let (one, rest) = bytes.split_at(TONE_BYTES);
        let (two, rest) = rest.split_at(TONE_BYTES);
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

/// The controller of each byte of a patch, in the patch's order: channel
/// I's tone (S80-10: 40 to 63, 65 and 66, leaving out 64, the sustain
/// pedal), channel II's (S80-10: 67 to 73, 119 and 75 to 92, leaving out
/// 74, which MPE controllers send), and the master controls (S80-11, the
/// adopted table's; S80-18, NeoTeletext's own 20 to 31; S80-20, GLIDE MODE
/// on 3 in place of the adopted table's 120).
///
/// Source: DD manual, PDF pages 71 to 74, read with the corrections of
/// S80-17; S80-33's table.
pub const CONTROLLERS: [u8; PATCH_BYTES] = [
    // Channel I.
    40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 65, 66,
    // Channel II.
    67, 68, 69, 70, 71, 72, 73, 119, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92,
    // FEET I and II, DETUNE CH II, MIX I-II, BRILLIANCE, RESONANCE.
    102, 103, 95, 8, 109, 110, // The four KEYBOARD CONTROL levers.
    115, 116, 117, 118, // The sub oscillator: FUNCTION, SPEED, VCO, VCF, VCA.
    104, 105, 106, 107, 108, // The touch response: PITCHBEND, SPEED, VCO, VCF.
    111, 112, 113, 114,
    // NeoTeletext's own: the ring modulator's five, the SUSTAIN I-II switch
    // and the SUSTAIN lever (S80-18).
    20, 21, 22, 23, 24, 25, 26, // GLIDE MODE on 3 (S80-20) and GLIDE TIME on 5 (S80-11).
    3, 5, // NeoTeletext's own: the tremolo/chorus's five (S80-18).
    27, 28, 29, 30, 31,
];

/// A controller that moves no byte of the patch, in [`PLACES`].
const NO_PLACE: u8 = u8::MAX;

/// The byte each controller moves, or [`NO_PLACE`]: [`CONTROLLERS`]
/// turned about.
#[allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "worked out by the compiler: a fault here stops the build, and no program; a place is below 85"
)]
const PLACES: [u8; 128] = {
    let mut places = [NO_PLACE; 128];
    let mut place = 0;
    while place < PATCH_BYTES {
        let controller = CONTROLLERS[place] as usize;
        // S80-10's verification: no controller appears twice.
        assert!(places[controller] == NO_PLACE, "a controller moves two bytes");
        places[controller] = place as u8;
        place += 1;
    }
    places
};

/// The byte of the patch a controller moves, or `None` for one that moves
/// none (S80-10, S80-11, S80-18, S80-20).
#[must_use]
pub fn place_of(controller: u8) -> Option<usize> {
    PLACES.get(usize::from(controller)).copied().filter(|&place| place != NO_PLACE).map(usize::from)
}

/// The controller that moves byte `place` of the patch (S80-33), or
/// `None` past its end.
#[must_use]
pub fn controller_of(place: usize) -> Option<u8> {
    CONTROLLERS.get(place).copied()
}

// MIDI 1.0's channel mode messages, its parameter numbers and data entry,
// bank select, the wheel, volume and the pedal move no byte (S80-20,
// S80-21, section 6.2 rule 2).
#[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build, and no program")]
const _: () = {
    let reserved = [0u8, 1, 6, 7, 32, 38, 64, 74, 93, 94, 96, 97, 98, 99, 100, 101];
    let mut index = 0;
    while index < reserved.len() {
        assert!(PLACES[reserved[index] as usize] == NO_PLACE);
        index += 1;
    }
    let mut mode = 120;
    while mode < 128 {
        assert!(PLACES[mode] == NO_PLACE);
        mode += 1;
    }
};
