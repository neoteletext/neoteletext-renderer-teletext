//! A handful of the Jupiter-8's factory patches, set by hand from
//! Roland's patch sheets, for the tests and the demonstration. Each
//! test file and example that wants them includes this as a module.
//!
//! Source: "Roland Jupiter-8 factory patch sheets" (65 scanned pages):
//! each sheet draws the panel with every slider and switch where the
//! patch has it, and names under it the assign mode and what the bender
//! reaches. The scans are coarse: a slider's position is read to
//! about half a division of its ten, a knob's to its nearest marking,
//! and what could not be told apart is said below. They are fixtures,
//! not a transcription to rely on; the data cassette is not used.

// Without the feature there is nothing here.
#![cfg(feature = "jupiter8")]
#![allow(dead_code, reason = "each file that includes this uses a part")]

use neoteletext_audio_instruments::jupiter8::{Param, Patch};

/// A panel's sliders, each at a position of its scale of 0 to 10 in
/// tenths.
fn sliders(patch: Patch, positions: &[(Param, u8)]) -> Patch {
    positions.iter().fold(patch, |patch, &(param, tenths)| patch.at(param, tenths))
}

/// A panel's switches and knobs, each at a value of its byte.
fn switches(patch: Patch, values: &[(Param, u8)]) -> Patch {
    values.iter().fold(patch, |patch, &(param, value)| patch.with(param, value))
}

/// A patch with the source mixer's knob at its middle, as every sheet
/// here has it or nearly.
fn mixed() -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.set_source_mix(128);
    patch
}

/// VCO-2's fine tune a little sharp of the middle: the sheets show the
/// knob just right of 12 o'clock. Taken as 8 cents.
const A_LITTLE_SHARP: u8 = 148;

/// Patch 31, "LO STRINGS" (sheet 18).
#[must_use]
pub fn lo_strings() -> Patch {
    let patch = sliders(
        mixed(),
        &[
            (Param::LfoRate, 66),
            (Param::VcoLfoMod, 18),
            (Param::PwmDepth, 60),
            (Param::VcfCutoff, 70),
            (Param::VcfEnvMod, 30),
            (Param::VcfLfoMod, 5),
            (Param::VcfKeyFollow, 10),
            (Param::VcaLevel, 45),
            (Param::Env1Attack, 45),
            (Param::Env1Decay, 60),
            (Param::Env1Sustain, 60),
            (Param::Env1Release, 43),
            (Param::Env2Attack, 45),
            (Param::Env2Decay, 55),
            (Param::Env2Sustain, 55),
            (Param::Env2Release, 45),
        ],
    );
    switches(
        patch,
        &[
            // The modulator's switch is at its top, read as VCO-1.
            (Param::Vco2Mod, 0),
            (Param::PwmSource, 0),
            (Param::Vco1Range, 0),
            (Param::Vco1Wave, 1),
            (Param::Vco2Range, 0),
            (Param::FineTune, A_LITTLE_SHARP),
            (Param::Vco2Wave, 2),
            (Param::VcfSlope, 1),
            (Param::Env1KeyFollow, 1),
            (Param::Env2KeyFollow, 1),
        ],
    )
}

/// Patch 32, "MID STRINGS" (sheet 19).
#[must_use]
pub fn mid_strings() -> Patch {
    let patch = sliders(
        mixed(),
        &[
            (Param::LfoRate, 70),
            (Param::VcoLfoMod, 20),
            (Param::PwmDepth, 36),
            (Param::VcfCutoff, 78),
            (Param::VcfEnvMod, 26),
            (Param::VcaLevel, 30),
            (Param::Env1Attack, 50),
            (Param::Env1Decay, 72),
            (Param::Env1Sustain, 52),
            (Param::Env1Release, 35),
            (Param::Env2Attack, 53),
            (Param::Env2Decay, 58),
            (Param::Env2Sustain, 52),
            (Param::Env2Release, 50),
        ],
    );
    switches(
        patch,
        &[
            (Param::Vco2Mod, 0),
            (Param::PwmSource, 0),
            (Param::Vco1Range, 1),
            (Param::Vco1Wave, 1),
            (Param::Vco2Range, 12),
            (Param::FineTune, A_LITTLE_SHARP),
            (Param::Vco2Wave, 2),
            (Param::VcfSlope, 1),
            (Param::Env1KeyFollow, 1),
            (Param::Env2KeyFollow, 1),
        ],
    )
}

/// Patch 35, "LO BRASS" (sheet 22).
#[must_use]
pub fn lo_brass() -> Patch {
    let patch = sliders(
        mixed(),
        &[
            (Param::LfoRate, 67),
            (Param::VcfCutoff, 43),
            (Param::VcfEnvMod, 100),
            (Param::VcaLevel, 48),
            (Param::Env1Attack, 25),
            (Param::Env1Decay, 50),
            (Param::Env1Sustain, 27),
            (Param::Env1Release, 18),
            (Param::Env2Attack, 0),
            (Param::Env2Decay, 0),
            (Param::Env2Sustain, 65),
            (Param::Env2Release, 35),
        ],
    );
    switches(
        patch,
        &[
            (Param::Vco1Range, 0),
            (Param::Vco1Wave, 1),
            (Param::Vco2Range, 0),
            (Param::FineTune, A_LITTLE_SHARP),
            (Param::Vco2Wave, 1),
        ],
    )
}

/// Patch 14, "SYNC SWEEP" (sheet 5).
#[must_use]
pub fn sync_sweep() -> Patch {
    let patch = sliders(
        mixed(),
        &[
            (Param::LfoRate, 29),
            (Param::LfoDelay, 54),
            (Param::VcoLfoMod, 100),
            (Param::VcoEnvMod, 40),
            (Param::PwmDepth, 78),
            (Param::VcfCutoff, 62),
            (Param::VcfResonance, 62),
            (Param::VcfEnvMod, 50),
            (Param::VcfLfoMod, 33),
            (Param::VcfKeyFollow, 73),
            (Param::VcaLevel, 48),
            (Param::Env1Decay, 100),
            (Param::Env1Sustain, 100),
            (Param::Env2Decay, 100),
            (Param::Env2Sustain, 100),
        ],
    );
    switches(
        patch,
        &[
            // The modulator's switch is at its foot: VCO-2 alone.
            (Param::Vco1Mod, 0),
            (Param::PwmSource, 0),
            (Param::Vco1Range, 0),
            (Param::Vco1Wave, 1),
            (Param::Sync, 1),
            // The range knob at 12 o'clock, between 8' and 4'.
            (Param::Vco2Range, 18),
            (Param::Vco2Wave, 2),
            // The sheet: "UNISON", and "BENDER: VCO-2 ONLY (MANUAL SYNC
            // SWEEP)".
            (Param::AssignMode, 2),
            (Param::Vco1Bend, 0),
        ],
    )
}

/// Patch 58, "GONG" (sheet 41).
#[must_use]
pub fn gong() -> Patch {
    let patch = sliders(
        mixed(),
        &[
            (Param::LfoRate, 50),
            (Param::PwmDepth, 100),
            (Param::CrossMod, 55),
            (Param::VcfCutoff, 75),
            (Param::VcfResonance, 60),
            (Param::VcfEnvMod, 45),
            (Param::VcaLevel, 56),
            (Param::Env1Decay, 53),
            (Param::Env1Sustain, 37),
            (Param::Env1Release, 47),
            (Param::Env2Decay, 0),
            (Param::Env2Sustain, 100),
            (Param::Env2Release, 75),
        ],
    );
    switches(
        patch,
        &[
            (Param::PwmSource, 2),
            (Param::Vco1Range, 1),
            (Param::Vco1Wave, 2),
            // The range knob is drawn near 12 o'clock, between 8' and
            // 4', with an asterisk, and the sheet's head says by hand
            // what the drawing cannot: "TUNE UP A FIFTH", seven
            // semitones above VCO-1's 8'.
            (Param::Vco2Range, 19),
            (Param::Vco2Wave, 0),
            (Param::Env1Polarity, 1),
            // The sheet: "UNISON MODE".
            (Param::AssignMode, 2),
        ],
    )
}

/// The patches, each with its number and name on the sheets.
#[must_use]
pub fn all() -> [(u8, &'static str, Patch); 5] {
    [
        (14, "SYNC SWEEP", sync_sweep()),
        (31, "LO STRINGS", lo_strings()),
        (32, "MID STRINGS", mid_strings()),
        (35, "LO BRASS", lo_brass()),
        (58, "GONG", gong()),
    ]
}
