//! A handful of synth8's factory patches, set by hand from
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
#![cfg(feature = "synth8")]
#![allow(dead_code, reason = "each file that includes this uses a part")]

use neoteletext_audio_instruments::synth8::patch::{switch_value, vco2_range_value};
use neoteletext_audio_instruments::synth8::{
    AssignMode, Param, Patch, PwmSource, Vco1Range, Vco1Wave, Vco2Wave, VcoModulator,
};

/// A panel's sliders, each at a position of its scale of 0 to 10 in
/// tenths.
fn sliders(patch: Patch, positions: &[(Param, u8)]) -> Patch {
    positions.iter().fold(patch, |patch, &(param, tenths)| patch.at(param, tenths))
}

/// A panel's switches and knobs, each at its control change's value
/// (`synth8.md`, S8-21).
fn switches(patch: Patch, values: &[(Param, u8)]) -> Patch {
    values.iter().fold(patch, |patch, &(param, value)| patch.with(param, value))
}

/// A patch with the source mixer's knob at its middle, as every sheet
/// here has it or nearly.
fn mixed() -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.set_source_mix(64);
    patch
}

/// VCO-2's fine tune a little sharp of the middle: the sheets show the
/// knob just right of 12 o'clock. Taken as 8 cents: value 74, the signed
/// byte's +20 of 127 (S8-21 item 3).
const A_LITTLE_SHARP: u8 = 74;
/// A switch that is on.
const ON: u8 = switch_value(true);
/// VCF SLOPE at −12 dB an octave, the first position the reference manual
/// lists (S8-21 item 5).
const TWELVE_DB: u8 = 0;

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
            // The eight envelope sliders as measured on the scan at 300
            // dots an inch, the middle of each drawn knob between the
            // scale's end marks, which are 82 dots apart.
            (Param::Env1Attack, 50),
            (Param::Env1Decay, 67),
            (Param::Env1Sustain, 67),
            (Param::Env1Release, 46),
            (Param::Env2Attack, 54),
            (Param::Env2Decay, 62),
            (Param::Env2Sustain, 61),
            (Param::Env2Release, 51),
        ],
    );
    switches(
        patch,
        &[
            // The modulator's switch is at its top, read as VCO-1.
            (Param::VcoModulator, VcoModulator::Vco1.value()),
            (Param::PwmSource, PwmSource::Lfo.value()),
            (Param::Vco1Range, Vco1Range::Sixteen.value()),
            (Param::Vco1Wave, Vco1Wave::Sawtooth.value()),
            (Param::Vco2Range, vco2_range_value(0)),
            (Param::FineTune, A_LITTLE_SHARP),
            (Param::Vco2Wave, Vco2Wave::Pulse.value()),
            (Param::VcfSlope, TWELVE_DB),
            (Param::Env1KeyFollow, ON),
            (Param::Env2KeyFollow, ON),
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
            (Param::VcoModulator, VcoModulator::Vco1.value()),
            (Param::PwmSource, PwmSource::Lfo.value()),
            (Param::Vco1Range, Vco1Range::Eight.value()),
            (Param::Vco1Wave, Vco1Wave::Sawtooth.value()),
            (Param::Vco2Range, vco2_range_value(12)),
            (Param::FineTune, A_LITTLE_SHARP),
            (Param::Vco2Wave, Vco2Wave::Pulse.value()),
            (Param::VcfSlope, TWELVE_DB),
            (Param::Env1KeyFollow, ON),
            (Param::Env2KeyFollow, ON),
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
            (Param::Vco1Range, Vco1Range::Sixteen.value()),
            (Param::Vco1Wave, Vco1Wave::Sawtooth.value()),
            (Param::Vco2Range, vco2_range_value(0)),
            (Param::FineTune, A_LITTLE_SHARP),
            (Param::Vco2Wave, Vco2Wave::Sawtooth.value()),
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
            (Param::VcoModulator, VcoModulator::Vco2.value()),
            (Param::PwmSource, PwmSource::Lfo.value()),
            (Param::Vco1Range, Vco1Range::Sixteen.value()),
            (Param::Vco1Wave, Vco1Wave::Sawtooth.value()),
            (Param::Sync, ON),
            // The range knob at 12 o'clock, between 8' and 4'.
            (Param::Vco2Range, vco2_range_value(18)),
            (Param::Vco2Wave, Vco2Wave::Pulse.value()),
            // The sheet: "UNISON", and "BENDER: VCO-2 ONLY (MANUAL SYNC
            // SWEEP)".
            (Param::AssignMode, AssignMode::Unison.value()),
            (Param::Vco1Bend, switch_value(false)),
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
            (Param::PwmSource, PwmSource::Envelope.value()),
            (Param::Vco1Range, Vco1Range::Eight.value()),
            (Param::Vco1Wave, Vco1Wave::Pulse.value()),
            // The range knob is drawn near 12 o'clock, between 8' and
            // 4', with an asterisk, and the sheet's head says by hand
            // what the drawing cannot: "TUNE UP A FIFTH", seven
            // semitones above VCO-1's 8'.
            (Param::Vco2Range, vco2_range_value(19)),
            (Param::Vco2Wave, Vco2Wave::Sine.value()),
            (Param::Env1Polarity, ON),
            // The sheet: "UNISON MODE".
            (Param::AssignMode, AssignMode::Unison.value()),
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
