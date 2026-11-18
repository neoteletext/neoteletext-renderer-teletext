//! Eight of Sequential's forty original factory programs, entered by
//! hand from the patch sheets, for the tests and the demonstration. No
//! program data of Sequential's is here: only what a player would set
//! on the panel from the printed sheets. Each test file and the
//! demonstration include it as a module; by itself it tests nothing.
//!
//! Source: Sequential LLC, "Original Prophet-5 Factory Program Patch
//! Sheets" 1.1 (2020), scanned from the original manual: one drawing of
//! the panel for each program, each knob a pointer on its dial of 0 to
//! 10 and each switch filled or empty, with the oscillators' pitches in
//! words beneath. Each pointer's angle was measured on the scan (the
//! dial's 0 is 150 degrees left of upright and its 10 150 degrees
//! right, as its printed numbers stand) and is given to a tenth of a
//! mark; where the sheet's own text gives a setting, that is used. The
//! frequency knobs are taken from the sheets' words, since their
//! drawings stand up to half a mark from them. A setting marked `?`
//! below was read by eye from the scan.

// Without the feature there is nothing here to test.
#![cfg(feature = "synth53")]
#![allow(
    dead_code,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    reason = "fixtures, of which each user takes a part"
)]

use neoteletext_audio_instruments::synth53::interface::{PROGRAM_BYTES, Program};
use neoteletext_audio_instruments::synth53::patch::dial;
use neoteletext_audio_instruments::synth53::{Knob, Model, Patch, Switch};

/// One patch sheet: every knob in tenths of its dial but the two
/// frequency knobs, which are in semitones as the sheet's words give
/// them, and every switch that is filled in.
pub struct Sheet {
    /// The program's number and name.
    pub name: &'static str,
    /// Oscillator A's and oscillator B's pitch above the knob's 0, in
    /// semitones; and oscillator B's knob in tenths where it is in its
    /// low-frequency mode, which the sheet gives no words for.
    pub semitones: (u8, u8),
    /// The knobs, in tenths of the dial.
    pub knobs: &'static [(Knob, u8)],
    /// The switches that are on.
    pub switches: &'static [Switch],
}

impl Sheet {
    /// The sheet as a program of a model. The low-frequency oscillator's
    /// knob is read as revisions 1 and 2, for which the sheets were
    /// drawn, take it (11 thousandths of a hertz at 0 and a ninth of an
    /// octave a step, which is sheet 1-1's "approximately 5 cycles per
    /// second" where it draws the knob), and set to give the same rate
    /// on revision 3, whose knob runs from 0.04 Hz at 0.909 octave to
    /// the volt.
    #[must_use]
    pub fn program(&self, model: Model) -> Patch {
        let mut patch = Patch::of_program(&Program::from_bytes([0; PROGRAM_BYTES]), model);
        for &(knob, tenths) in self.knobs {
            let steps = match (knob, model) {
                (Knob::LfoFrequency, Model::Synth53) => {
                    let hertz = 0.011 * (f64::from(dial(tenths)) / 9.0).exp2();
                    (12.0 * (hertz / 0.04).log2() * 1.1).round().clamp(0.0, 120.0) as u8
                }
                _ => dial(tenths),
            };
            patch.set(knob, steps);
        }
        for &switch in self.switches {
            patch.switch(switch, true);
        }
        let low = patch.on(Switch::OscBLowFrequency);
        patch.set(Knob::OscAFrequency, model.frequency_knob(self.semitones.0));
        if !low {
            patch.set(Knob::OscBFrequency, model.frequency_knob(self.semitones.1));
        }
        patch
    }
}

use Knob::{
    AmplifierAttack as AA, AmplifierDecay as AD, AmplifierRelease as AR, AmplifierSustain as AS, Cutoff,
    FilterAttack as FA, FilterDecay as FD, FilterEnvelopeAmount as Amount, FilterRelease as FR, FilterSustain as FS,
    Glide, LfoFrequency as Lfo, MixerOscA, MixerOscB, OscAPulseWidth as WidthA, OscBFine as Fine,
    OscBFrequency as FrequencyB, OscBPulseWidth as WidthB, PolyModFilterEnvelope as PolyEnvelope,
    PolyModOscB as PolyOscB, Resonance, SourceMix as Mix,
};

/// 1-1, Brass. Oscillator A "up 1 octave (basic pitch)"; "OSC B is
/// programmed off (no waveform selected)"; "PULSE-WIDTH on both
/// oscillators is set at 2". The cutoff and the resonance are drawn at
/// 0: the filter envelope opens the filter. `?`: the filter's decay.
pub const BRASS: Sheet = Sheet {
    name: "1-1 Brass",
    semitones: (12, 24),
    knobs: &[
        (Lfo, 66),
        (WidthA, 20),
        (WidthB, 20),
        (MixerOscA, 99),
        (MixerOscB, 98),
        (Amount, 70),
        (FA, 45),
        (FD, 56),
        (FS, 50),
        (FR, 46),
        (AA, 41),
        (AD, 39),
        (AS, 59),
        (AR, 50),
        (Glide, 65),
    ],
    switches: &[
        Switch::LfoTriangle,
        Switch::WheelFrequencyA,
        Switch::WheelFrequencyB,
        Switch::OscASawtooth,
        Switch::OscBKeyboard,
        Switch::FilterKeyboard,
        Switch::Release,
    ],
};

/// 1-2, Low Strings. Both oscillators "up 1 octave"; the modulation
/// wheel "must be moved up 1/4 to 1/3 for proper effect". The filter
/// envelope's knobs and its amount are drawn at 0. `?`: the amplifier's
/// attack.
pub const LOW_STRINGS: Sheet = Sheet {
    name: "1-2 Low Strings",
    semitones: (12, 12),
    knobs: &[
        (Lfo, 65),
        (WidthA, 30),
        (WidthB, 29),
        (Fine, 16),
        (MixerOscA, 41),
        (MixerOscB, 39),
        (Cutoff, 56),
        (Resonance, 1),
        (AA, 58),
        (AD, 40),
        (AS, 100),
        (AR, 53),
    ],
    switches: &[
        Switch::LfoTriangle,
        Switch::WheelPulseWidthA,
        Switch::WheelPulseWidthB,
        Switch::OscAPulse,
        Switch::OscBPulse,
        Switch::OscBKeyboard,
        Switch::FilterKeyboard,
        Switch::Release,
    ],
};

/// 1-3, Muted Clav. Both oscillators "up 1 octave"; "RELEASE is
/// programmed off". The sheet draws the amplifier's attack, decay and
/// sustain all at 0, which on this instrument is a click and no note:
/// own choice, the amplifier's decay is taken as the filter's, which
/// the sheet draws at 6. `?`: the amplifier's release.
pub const MUTED_CLAV: Sheet = Sheet {
    name: "1-3 Muted Clav.",
    semitones: (12, 12),
    knobs: &[
        (Lfo, 68),
        (WidthA, 13),
        (WidthB, 14),
        (MixerOscA, 99),
        (MixerOscB, 99),
        (Cutoff, 54),
        (Resonance, 31),
        (Amount, 44),
        (FD, 60),
        (FS, 60),
        (FR, 64),
        (AD, 60),
        (AR, 65),
    ],
    switches: &[Switch::LfoTriangle, Switch::WheelFilter, Switch::OscAPulse, Switch::OscBPulse, Switch::OscBKeyboard],
};

/// 1-5, Flutes. Oscillator A "up 3 octaves", oscillator B "LF mode":
/// "POLY-MOD is being used to create a quasi-tremolo effect". `?`: the
/// amplifier's attack.
pub const FLUTES: Sheet = Sheet {
    name: "1-5 Flutes",
    semitones: (36, 0),
    knobs: &[
        (PolyOscB, 20),
        (Lfo, 72),
        (WidthA, 50),
        (WidthB, 50),
        (FrequencyB, 55),
        (MixerOscA, 100),
        (Cutoff, 27),
        (Resonance, 1),
        (Amount, 35),
        (FA, 50),
        (FD, 70),
        (FS, 70),
        (FR, 50),
        (AA, 50),
        (AD, 59),
        (AS, 80),
        (AR, 50),
    ],
    switches: &[
        Switch::PolyModFilter,
        Switch::LfoSquare,
        Switch::WheelFrequencyA,
        Switch::OscASawtooth,
        Switch::OscBTriangle,
        Switch::OscBLowFrequency,
        Switch::FilterKeyboard,
        Switch::Release,
    ],
};

/// 1-7, Sync I. Oscillator A "up 1 octave + a minor 3rd", oscillator B
/// "up 1 octave (basic pitch)"; "Although the filter ENVELOPE AMOUNT is
/// set at 0, the filter envelope generator settings are programmed for
/// use as a modulation source for the POLY-MOD section". The source mix,
/// whose dial is marked from 5 at the low-frequency oscillator through 0
/// to 5 at the noise, is drawn at the oscillator's 5: no noise. `?`: the
/// low-frequency oscillator's frequency.
pub const SYNC: Sheet = Sheet {
    name: "1-7 Sync I",
    semitones: (15, 12),
    knobs: &[
        (PolyEnvelope, 61),
        (Lfo, 66),
        (Mix, 0),
        (WidthA, 50),
        (WidthB, 49),
        (MixerOscA, 50),
        (MixerOscB, 50),
        (Cutoff, 55),
        (Resonance, 20),
        (FA, 40),
        (FD, 60),
        (FR, 69),
        (AA, 1),
        (AS, 100),
        (AR, 69),
        (Glide, 60),
    ],
    switches: &[
        Switch::PolyModFrequencyA,
        Switch::LfoTriangle,
        Switch::WheelFrequencyB,
        Switch::OscASawtooth,
        Switch::Sync,
        Switch::OscBKeyboard,
        Switch::FilterKeyboard,
    ],
};

/// 1-8, Percussive Organ. Oscillator A "up 3 octaves + a perfect 5th",
/// "must be a square wave"; oscillator B "up 1 octave (basic pitch)".
/// `?`: the low-frequency oscillator's frequency, the resonance (at 0),
/// the filter's attack (at 0), the amplifier's attack and decay (at 0)
/// and its release.
pub const ORGAN: Sheet = Sheet {
    name: "1-8 Percussive Organ",
    semitones: (43, 12),
    knobs: &[
        (Lfo, 72),
        (WidthA, 50),
        (WidthB, 49),
        (MixerOscA, 40),
        (MixerOscB, 68),
        (Cutoff, 34),
        (Amount, 39),
        (FD, 59),
        (FS, 25),
        (FR, 39),
        (AS, 99),
        (AR, 39),
    ],
    switches: &[
        Switch::LfoTriangle,
        Switch::WheelFrequencyB,
        Switch::OscAPulse,
        Switch::OscBTriangle,
        Switch::OscBKeyboard,
        Switch::FilterKeyboard,
        Switch::Release,
    ],
};

/// 2-1, Unison Glide With Resonance. Oscillator A "up 2 octaves",
/// oscillator B "up 1 octave"; "PULSE-WIDTH on both oscillators is set
/// at 3". The envelope amount and the filter's decay and sustain are
/// drawn at 10.
pub const UNISON_GLIDE: Sheet = Sheet {
    name: "2-1 Unison Glide With Resonance",
    semitones: (24, 12),
    knobs: &[
        (Lfo, 69),
        (WidthA, 30),
        (WidthB, 29),
        (MixerOscA, 51),
        (MixerOscB, 50),
        (Cutoff, 20),
        (Resonance, 64),
        (Amount, 99),
        (FD, 99),
        (FS, 100),
        (FR, 74),
        (AD, 51),
        (AS, 100),
        (AR, 80),
        (Glide, 60),
    ],
    switches: &[
        Switch::LfoTriangle,
        Switch::WheelFrequencyA,
        Switch::WheelFrequencyB,
        Switch::OscASawtooth,
        Switch::OscBSawtooth,
        Switch::OscBKeyboard,
        Switch::FilterKeyboard,
        Switch::Unison,
        Switch::Release,
    ],
};

/// 4-8, Clangorous Bells. Oscillator A "up 2 octaves + a tritone",
/// oscillator B "up 2 octaves - less 1/2 step". Poly-Mod's amount of
/// the filter envelope is drawn at 0 and its amount of oscillator B at
/// its top: the clang is oscillator B on oscillator A's frequency.
pub const BELLS: Sheet = Sheet {
    name: "4-8 Clangorous Bells",
    semitones: (30, 23),
    knobs: &[
        (PolyOscB, 98),
        (Lfo, 42),
        (WidthA, 50),
        (WidthB, 50),
        (MixerOscA, 98),
        (MixerOscB, 50),
        (Cutoff, 30),
        (Resonance, 50),
        (Amount, 29),
        (FD, 70),
        (FS, 30),
        (FR, 69),
        (AD, 75),
        (AR, 80),
    ],
    switches: &[
        Switch::PolyModFrequencyA,
        Switch::LfoTriangle,
        Switch::WheelPulseWidthA,
        Switch::OscAPulse,
        Switch::OscBTriangle,
        Switch::OscBPulse,
        Switch::OscBKeyboard,
        Switch::FilterKeyboard,
        Switch::Release,
    ],
};

/// The eight sheets.
pub const SHEETS: [&Sheet; 8] = [&BRASS, &LOW_STRINGS, &MUTED_CLAV, &FLUTES, &SYNC, &ORGAN, &UNISON_GLIDE, &BELLS];
