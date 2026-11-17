//! A handful of patches from Moog's 1974 sound charts ("Sound Charts
//! minimoog", Tom Rhea, Moog Music Inc.), as fixtures for the tests and
//! the demonstration. Each test file and example that uses them includes
//! this as a module; by itself it tests nothing.
//!
//! The knobs' figures are those printed on each chart: dial positions,
//! and for the contours' times the milliseconds and seconds the dials are
//! marked in. The scan read for this is small, and what it does not show
//! clearly is an own reading, as follows. The rocker switches are taken
//! as: OSCILLATOR MODULATION on wherever oscillator 3 is set up as the
//! slow oscillator (the charts' notes say "apply modulation"); both
//! KEYBOARD CONTROL switches on; DECAY on for the plucked and struck
//! sounds; GLIDE on where the GLIDE knob is set. The charts' "0*" for
//! oscillator 2's FREQUENCY means "in unison", which here is 5 cents, so
//! that the two oscillators beat as two tuned by ear do.

// Without the feature there is nothing here to test.
#![cfg(feature = "minimoog")]
#![allow(dead_code, reason = "fixtures, of which each user takes a part")]

use neoteletext_audio_instruments::minimoog::{ContourPatch, OscillatorPatch, Patch, Range, Waveform};

/// "In unison", by ear: cents.
const UNISON: i16 = 5;

/// An oscillator in the mixer; the volume is the dial's figure times
/// ten.
const fn oscillator(range: Range, waveform: Waveform, frequency: i16, volume: u8) -> OscillatorPatch {
    OscillatorPatch { range, waveform, frequency, volume, on: true }
}

/// Oscillator 3 as the slow oscillator: at LO, a triangle, out of the
/// mixer, with its FREQUENCY at a dial figure in tenths of the dial's
/// -7 to 7.
const fn slow(dial_tenths: i16) -> OscillatorPatch {
    OscillatorPatch {
        range: Range::Lo,
        waveform: Waveform::Triangle,
        frequency: dial_tenths * 10,
        volume: 0,
        on: false,
    }
}

/// An oscillator out of the mixer.
const SILENT: OscillatorPatch =
    OscillatorPatch { range: Range::Eight, waveform: Waveform::Sawtooth, frequency: 0, volume: 0, on: false };

/// A contour as a chart prints it: milliseconds, milliseconds, and the
/// sustain dial's figure times ten.
const fn contour(attack: u16, decay: u16, sustain: u8) -> ContourPatch {
    ContourPatch { attack, decay, sustain }
}

/// What every chart here shares.
const CHART: Patch = Patch {
    oscillator_modulation: true,
    oscillator_3_control: false,
    keyboard_control: [true, true],
    volume: 50,
    ..Patch::DEFAULT
};

/// Page 3, "SHAMISEN (Chinese plucked instrument)".
pub const SHAMISEN: Patch = Patch {
    oscillators: [
        oscillator(Range::Four, Waveform::Sawtooth, 0, 50),
        oscillator(Range::Four, Waveform::Sawtooth, UNISON, 50),
        slow(40),
    ],
    cutoff: -200,
    emphasis: 40,
    contour_amount: 60,
    filter_contour: contour(0, 400, 40),
    loudness_contour: contour(0, 600, 0),
    decay_on: true,
    ..CHART
};

/// Page 3, "HARPSICORD".
pub const HARPSICORD: Patch = Patch {
    oscillators: [
        oscillator(Range::Eight, Waveform::Sawtooth, 0, 30),
        oscillator(Range::Eight, Waveform::Narrow, UNISON, 50),
        SILENT,
    ],
    cutoff: 500,
    emphasis: 70,
    contour_amount: 0,
    loudness_contour: contour(0, 300, 0),
    decay_on: true,
    oscillator_modulation: false,
    ..CHART
};

/// Page 4, "ELECTRIC PIANO".
pub const ELECTRIC_PIANO: Patch = Patch {
    oscillators: [oscillator(Range::Eight, Waveform::Triangle, 0, 60), SILENT, slow(35)],
    cutoff: -350,
    emphasis: 50,
    contour_amount: 40,
    filter_contour: contour(0, 600, 0),
    loudness_contour: contour(0, 600, 0),
    decay_on: true,
    ..CHART
};

/// Page 4, "VIOLIN".
pub const VIOLIN: Patch = Patch {
    glide: 40,
    glide_on: true,
    oscillators: [
        oscillator(Range::Four, Waveform::Sawtooth, 0, 60),
        oscillator(Range::Four, Waveform::Narrow, UNISON, 20),
        slow(30),
    ],
    cutoff: 0,
    emphasis: 30,
    contour_amount: 30,
    filter_contour: contour(400, 1_000, 60),
    loudness_contour: contour(500, 1_000, 60),
    ..CHART
};

/// Page 5, "CELLO".
pub const CELLO: Patch = Patch {
    glide: 50,
    glide_on: true,
    oscillators: [
        oscillator(Range::Sixteen, Waveform::Triangle, 0, 30),
        oscillator(Range::Sixteen, Waveform::Sawtooth, UNISON, 60),
        slow(30),
    ],
    cutoff: 0,
    emphasis: 0,
    contour_amount: 30,
    filter_contour: contour(200, 800, 70),
    loudness_contour: contour(600, 1_000, 70),
    ..CHART
};

/// Page 6, "BASS VIOL, PLUCKED".
pub const BASS_VIOL_PLUCKED: Patch = Patch {
    oscillators: [oscillator(Range::ThirtyTwo, Waveform::Sawtooth, 0, 80), SILENT, slow(-70)],
    cutoff: -300,
    emphasis: 40,
    contour_amount: 40,
    filter_contour: contour(0, 300, 40),
    loudness_contour: contour(0, 700, 0),
    decay_on: true,
    ..CHART
};

/// Page 7, "FLUTE".
pub const FLUTE: Patch = Patch {
    oscillators: [oscillator(Range::Four, Waveform::Triangle, 0, 50), SILENT, slow(40)],
    noise_volume: 3,
    noise_on: true,
    cutoff: 100,
    emphasis: 40,
    contour_amount: 20,
    filter_contour: contour(200, 250, 20),
    loudness_contour: contour(400, 250, 65),
    ..CHART
};

/// Page 8, "CLARINET (BASS CLARINET)".
pub const CLARINET: Patch = Patch {
    oscillators: [oscillator(Range::Eight, Waveform::Square, 0, 50), SILENT, SILENT],
    noise_volume: 3,
    noise_on: true,
    cutoff: 0,
    emphasis: 50,
    contour_amount: 20,
    filter_contour: contour(200, 400, 80),
    loudness_contour: contour(200, 300, 80),
    oscillator_modulation: false,
    ..CHART
};

/// Every chart here, with its name.
pub const CHARTS: [(&str, Patch); 8] = [
    ("Shamisen", SHAMISEN),
    ("Harpsicord", HARPSICORD),
    ("Electric piano", ELECTRIC_PIANO),
    ("Violin", VIOLIN),
    ("Cello", CELLO),
    ("Bass viol, plucked", BASS_VIOL_PLUCKED),
    ("Flute", FLUTE),
    ("Clarinet", CLARINET),
];
