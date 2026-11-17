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
//! words beneath. A pointer is read to about one mark of the dial;
//! where the sheet's own text gives a setting, that is used; a setting
//! marked `?` below could not be read with confidence and is a choice
//! among what the drawing allows.

// Without the feature there is nothing here to test.
#![cfg(feature = "prophet5")]
#![allow(
    dead_code,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    reason = "fixtures, of which each user takes a part"
)]

use neoteletext_audio_instruments::prophet5::patch::dial;
use neoteletext_audio_instruments::prophet5::{Knob, Model, Patch, Switch};

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
        let mut patch = Patch::from_bytes(&[0; 24]);
        for &(knob, tenths) in self.knobs {
            let steps = match (knob, model) {
                (Knob::LfoFrequency, Model::Rev3) => {
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
    PolyModOscB as PolyOscB, Resonance,
};

/// 1-1, Brass. Oscillator A "up 1 octave (basic pitch)"; "OSC B is
/// programmed off (no waveform selected)". `?`: the cutoff and the
/// resonance at about 1, the amplifier's sustain (6 taken).
pub const BRASS: Sheet = Sheet {
    name: "1-1 Brass",
    semitones: (12, 24),
    knobs: &[
        (Lfo, 60),
        (WidthA, 20),
        (WidthB, 20),
        (MixerOscA, 95),
        (MixerOscB, 90),
        (Cutoff, 10),
        (Resonance, 10),
        (Amount, 70),
        (FA, 35),
        (FD, 40),
        (FS, 50),
        (FR, 40),
        (AA, 30),
        (AD, 40),
        (AS, 60),
        (AR, 50),
        (Glide, 60),
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
/// wheel "must be moved up 1/4 to 1/3 for proper effect". `?`: the
/// mixer's levels at about 4, the filter envelope's knobs at about 0.
pub const LOW_STRINGS: Sheet = Sheet {
    name: "1-2 Low Strings",
    semitones: (12, 12),
    knobs: &[
        (Lfo, 60),
        (WidthA, 30),
        (WidthB, 30),
        (Fine, 20),
        (MixerOscA, 40),
        (MixerOscB, 40),
        (Cutoff, 60),
        (Resonance, 5),
        (AA, 60),
        (AD, 35),
        (AS, 95),
        (AR, 55),
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
/// programmed off". `?`: the cutoff and the envelope amount at about 4,
/// the filter's sustain; the amplifier's decay and sustain could not be
/// read at all, and are taken as the filter's decay and none.
pub const MUTED_CLAV: Sheet = Sheet {
    name: "1-3 Muted Clav.",
    semitones: (12, 12),
    knobs: &[
        (Lfo, 70),
        (WidthA, 15),
        (WidthB, 15),
        (MixerOscA, 95),
        (MixerOscB, 95),
        (Cutoff, 40),
        (Resonance, 30),
        (Amount, 40),
        (FD, 60),
        (FS, 60),
        (FR, 65),
        (AD, 60),
        (AR, 60),
    ],
    switches: &[Switch::LfoTriangle, Switch::WheelFilter, Switch::OscAPulse, Switch::OscBPulse, Switch::OscBKeyboard],
};

/// 1-5, Flutes. Oscillator A "up 3 octaves", oscillator B "LF mode":
/// "POLY-MOD is being used to create a quasi-tremolo effect". `?`:
/// oscillator B's frequency at about 6.
pub const FLUTES: Sheet = Sheet {
    name: "1-5 Flutes",
    semitones: (36, 0),
    knobs: &[
        (PolyOscB, 20),
        (Lfo, 70),
        (WidthA, 50),
        (WidthB, 50),
        (FrequencyB, 60),
        (MixerOscA, 95),
        (Cutoff, 30),
        (Resonance, 5),
        (Amount, 35),
        (FA, 50),
        (FD, 70),
        (FS, 75),
        (FR, 50),
        (AA, 50),
        (AD, 60),
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
/// use as a modulation source for the POLY-MOD section". `?`: the
/// mixer's level of oscillator A, the filter's sustain, the amplifier's
/// decay and sustain.
pub const SYNC: Sheet = Sheet {
    name: "1-7 Sync I",
    semitones: (15, 12),
    knobs: &[
        (PolyEnvelope, 60),
        (Lfo, 65),
        (WidthA, 50),
        (WidthB, 50),
        (MixerOscA, 50),
        (MixerOscB, 50),
        (Cutoff, 60),
        (Resonance, 20),
        (FA, 35),
        (FD, 60),
        (FR, 70),
        (AS, 95),
        (AR, 70),
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
/// `?`: the mixer's levels, the filter's decay, sustain and release,
/// the amplifier's decay and sustain.
pub const ORGAN: Sheet = Sheet {
    name: "1-8 Percussive Organ",
    semitones: (43, 12),
    knobs: &[
        (Lfo, 65),
        (WidthA, 50),
        (WidthB, 50),
        (MixerOscA, 35),
        (MixerOscB, 65),
        (Cutoff, 30),
        (Resonance, 5),
        (Amount, 35),
        (FD, 60),
        (FS, 30),
        (FR, 40),
        (AS, 95),
        (AR, 40),
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
/// at 3". `?`: the envelope amount at about 9, the filter's attack and
/// release; its decay and sustain could not be read, and 5 and 3 are
/// taken.
pub const UNISON_GLIDE: Sheet = Sheet {
    name: "2-1 Unison Glide With Resonance",
    semitones: (24, 12),
    knobs: &[
        (Lfo, 65),
        (WidthA, 30),
        (WidthB, 30),
        (MixerOscA, 50),
        (MixerOscB, 50),
        (Cutoff, 20),
        (Resonance, 65),
        (Amount, 90),
        (FD, 50),
        (FS, 30),
        (FR, 75),
        (AD, 50),
        (AS, 95),
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
/// oscillator B "up 2 octaves - less 1/2 step". `?`: both Poly-Mod
/// amounts at about 3.5, the mixer's level of oscillator A, the
/// filter's sustain, the amplifier's sustain.
pub const BELLS: Sheet = Sheet {
    name: "4-8 Clangorous Bells",
    semitones: (30, 23),
    knobs: &[
        (PolyEnvelope, 35),
        (PolyOscB, 35),
        (Lfo, 40),
        (WidthA, 50),
        (WidthB, 50),
        (MixerOscA, 95),
        (MixerOscB, 50),
        (Cutoff, 30),
        (Resonance, 50),
        (Amount, 30),
        (FD, 70),
        (FS, 30),
        (FR, 70),
        (AD, 70),
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
