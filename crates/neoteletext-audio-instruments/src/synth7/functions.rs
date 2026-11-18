//! The fourteen function parameters of a part (`synth7.md`, S7-33 to
//! S7-36): how its wheels, pedals and controllers act and whether it is
//! polyphonic. They are not part of a voice: "function parameters cannot
//! be saved in internal or external memory" (DX7 manual, page 6).

use super::patch::MAX_99;

/// The first function parameter number, MONO/POLY MODE CHANGE.
///
/// Source: DX7 manual, page 31, group 2: "64 ~ 77".
pub const FIRST_FUNCTION_PARAMETER: u16 = 64;
/// The last function parameter number, AFTER TOUCH ASSIGN.
pub const LAST_FUNCTION_PARAMETER: u16 = 77;
/// How many function parameters there are.
pub const FUNCTION_PARAMETERS: usize = (LAST_FUNCTION_PARAMETER - FIRST_FUNCTION_PARAMETER + 1) as usize;

/// The function parameters by number, 64 to 77 (S7-33).
pub mod number {
    /// 64: 0 poly, 1 mono (S7-25, S7-26).
    pub const MONO_POLY: u16 = 64;
    /// 65: the pitch bend range in semitones, 0 to 12 (S7-9).
    pub const PITCH_BEND_RANGE: u16 = 65;
    /// 66: the pitch bend step in semitones, 0 smooth (S7-9).
    pub const PITCH_BEND_STEP: u16 = 66;
    /// 67: the portamento mode (S7-27).
    pub const PORTAMENTO_MODE: u16 = 67;
    /// 68: 0 portamento, 1 glissando (S7-27).
    pub const GLISSANDO: u16 = 68;
    /// 69: the portamento time, 0 none to 99 the slowest (S7-27).
    pub const PORTAMENTO_TIME: u16 = 69;
    /// 70: the modulation wheel's range (S7-36).
    pub const MODULATION_WHEEL_RANGE: u16 = 70;
    /// 71: what the modulation wheel acts on (S7-34).
    pub const MODULATION_WHEEL_ASSIGN: u16 = 71;
    /// 72: the foot controller's range.
    pub const FOOT_CONTROLLER_RANGE: u16 = 72;
    /// 73: what the foot controller acts on.
    pub const FOOT_CONTROLLER_ASSIGN: u16 = 73;
    /// 74: the breath controller's range.
    pub const BREATH_CONTROLLER_RANGE: u16 = 74;
    /// 75: what the breath controller acts on.
    pub const BREATH_CONTROLLER_ASSIGN: u16 = 75;
    /// 76: after touch's range.
    pub const AFTER_TOUCH_RANGE: u16 = 76;
    /// 77: what after touch acts on.
    pub const AFTER_TOUCH_ASSIGN: u16 = 77;
}

/// The largest pitch bend range and step: 12 semitones, ±1,200 cents.
///
/// Source: DX7 manual, page 6: "can be set from 0 to 12 … A setting of 12
/// permits pitch bend over a ±1200 cent (2 octave) range"; page 31.
pub const MAX_BEND_SEMITONES: u8 = 12;
/// The largest assign: three bits (S7-34).
///
/// Source: DX7 manual, page 31: "0 ~ 7".
pub const MAX_ASSIGN: u8 = 7;
/// A switch of two settings: mono/poly, portamento mode, glissando.
///
/// Source: DX7 manual, page 31: "0 ~ 1".
pub const MAX_FUNCTION_SWITCH: u8 = 1;

/// The assign bit of pitch (S7-34).
///
/// Source: TX7 manual, printed page 40, section 4-2, note *2: "b0 PITCH".
pub const ASSIGN_PITCH: u8 = 0b001;
/// The assign bit of amplitude: "b1 AMPLITUDE".
pub const ASSIGN_AMPLITUDE: u8 = 0b010;
/// The assign bit of EG bias: "b2 EG BIAS".
pub const ASSIGN_EG_BIAS: u8 = 0b100;

/// The largest value of each function parameter, 64 to 77 (S7-33).
///
/// Source: DX7 manual, page 31.
pub const FUNCTION_MAX: [u8; FUNCTION_PARAMETERS] = [
    MAX_FUNCTION_SWITCH,
    MAX_BEND_SEMITONES,
    MAX_BEND_SEMITONES,
    MAX_FUNCTION_SWITCH,
    MAX_FUNCTION_SWITCH,
    MAX_99,
    MAX_99,
    MAX_ASSIGN,
    MAX_99,
    MAX_ASSIGN,
    MAX_99,
    MAX_ASSIGN,
    MAX_99,
    MAX_ASSIGN,
];

/// The pitch bend range a part starts with, in semitones (S7-35): what
/// synth7 bent before it had function parameters.
pub const DEFAULT_BEND_SEMITONES: u8 = 2;

/// The function parameters a part starts with, 64 to 77 (S7-35): poly, a
/// bend of 2 semitones, smooth, no portamento, the wheel at full range on
/// pitch alone, and no other controller.
///
/// Source: NeoTeletext's own (`synth7.md` section 15 item 14), chosen to
/// keep the sound synth7 made before it had function parameters.
pub const FUNCTION_DEFAULTS: [u8; FUNCTION_PARAMETERS] =
    [0, DEFAULT_BEND_SEMITONES, 0, 0, 0, 0, MAX_99, ASSIGN_PITCH, 0, 0, 0, 0, 0, 0];

/// One controller's range and assign, as two function parameters hold them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerSetting {
    /// 0 to 99.
    pub range: u8,
    /// Bits [`ASSIGN_PITCH`], [`ASSIGN_AMPLITUDE`], [`ASSIGN_EG_BIAS`].
    pub assign: u8,
}

/// A part's fourteen function parameters, each within its range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Functions {
    values: [u8; FUNCTION_PARAMETERS],
}

impl Default for Functions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a function parameter change was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionError {
    /// The number is not 64 to 77.
    Number,
    /// The value is above the parameter's range.
    OutOfRange {
        /// The largest value allowed.
        max: u8,
    },
}

impl Functions {
    /// A part's function parameters before any parameter change (S7-35).
    pub const DEFAULT: Self = Self { values: FUNCTION_DEFAULTS };

    /// Function parameter `number`, 64 to 77, or `None` for another number.
    #[must_use]
    pub fn get(&self, number: u16) -> Option<u8> {
        let index = number.checked_sub(FIRST_FUNCTION_PARAMETER)?;
        self.values.get(usize::from(index)).copied()
    }

    /// Sets function parameter `number` to `value`, and says whether it
    /// changed.
    ///
    /// # Errors
    ///
    /// [`FunctionError::Number`] for a number that is not 64 to 77, and
    /// [`FunctionError::OutOfRange`] for a value above the parameter's
    /// range (S7-29, S7-33); either changes nothing.
    pub fn set(&mut self, number: u16, value: u8) -> Result<bool, FunctionError> {
        let index = number.checked_sub(FIRST_FUNCTION_PARAMETER).map(usize::from).ok_or(FunctionError::Number)?;
        let max = FUNCTION_MAX.get(index).copied().ok_or(FunctionError::Number)?;
        if value > max {
            return Err(FunctionError::OutOfRange { max });
        }
        let place = self.values.get_mut(index).ok_or(FunctionError::Number)?;
        let changed = *place != value;
        *place = value;
        Ok(changed)
    }

    /// The fourteen values, 64 first.
    #[must_use]
    pub const fn values(&self) -> &[u8; FUNCTION_PARAMETERS] {
        &self.values
    }

    fn value(&self, number: u16) -> u8 {
        self.get(number).unwrap_or(0)
    }

    /// Whether the part is in mono mode (S7-26).
    #[must_use]
    pub fn mono(&self) -> bool {
        self.value(number::MONO_POLY) != 0
    }

    /// The pitch bend range, 0 to 12 semitones.
    #[must_use]
    pub fn bend_range(&self) -> u8 {
        self.value(number::PITCH_BEND_RANGE)
    }

    /// The pitch bend step, 0 to 12 semitones; 0 is smooth.
    #[must_use]
    pub fn bend_step(&self) -> u8 {
        self.value(number::PITCH_BEND_STEP)
    }

    /// The portamento mode: in poly 0 retain and 1 follow, in mono 0
    /// fingered and 1 full time (S7-27).
    #[must_use]
    pub fn portamento_mode(&self) -> u8 {
        self.value(number::PORTAMENTO_MODE)
    }

    /// Whether the glide moves in semitones.
    #[must_use]
    pub fn glissando(&self) -> bool {
        self.value(number::GLISSANDO) != 0
    }

    /// The portamento time, 0 to 99.
    #[must_use]
    pub fn portamento_time(&self) -> u8 {
        self.value(number::PORTAMENTO_TIME)
    }

    /// The modulation wheel's range and assign (70, 71).
    #[must_use]
    pub fn wheel(&self) -> ControllerSetting {
        self.setting(number::MODULATION_WHEEL_RANGE)
    }

    /// The foot controller's (72, 73).
    #[must_use]
    pub fn foot(&self) -> ControllerSetting {
        self.setting(number::FOOT_CONTROLLER_RANGE)
    }

    /// The breath controller's (74, 75).
    #[must_use]
    pub fn breath(&self) -> ControllerSetting {
        self.setting(number::BREATH_CONTROLLER_RANGE)
    }

    /// After touch's (76, 77).
    #[must_use]
    pub fn after_touch(&self) -> ControllerSetting {
        self.setting(number::AFTER_TOUCH_RANGE)
    }

    fn setting(&self, range: u16) -> ControllerSetting {
        ControllerSetting { range: self.value(range), assign: self.value(range.saturating_add(1)) }
    }
}
