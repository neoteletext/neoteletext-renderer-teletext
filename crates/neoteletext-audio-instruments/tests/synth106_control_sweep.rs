//! Every control of synth106 moved under every kind of sound, one at a
//! time, and the largest step of the output that follows measured against
//! the sound's own (TODO-6438).
//!
//! A click is a step from one sample to the next that the sound would not
//! have made by itself. Four rounds of review each found one in another
//! control; this file measures them all at once. For both models and for
//! each of [`SOUNDS`] it moves every slider of the patch (by one step and
//! by a quarter of its travel from where the sound has it, and from
//! either end to the other), every switch from each of its positions to
//! each other, and sends every message the instrument takes, each at
//! [`PHASES`] instants of the note, and prints for each the largest step
//! of the 50 ms that follow against the sound's own.
//!
//! **The sound's own step** is the largest of three: the largest step of
//! the sound left alone, over the same 50 ms and the 200 ms before; the
//! same of the sound that has had the new setting from its start, where
//! a change leaves a setting (a slider opened makes a brighter sound,
//! whose edges are its own); and a sixty-fourth of the level standing in
//! the output, which is what the fastest of the instrument's own slews
//! moves it by in a sample ([`SLEW_SHARE`]).
//!
//! Two measures are printed for each row:
//!
//! - **the step ratio**: the largest step of the output after the change
//!   over the sound's own;
//! - **the change's own step**: the largest step of the difference
//!   between the output after the change and the output left alone, over
//!   the sound's own. A sawtooth's edge is so large that a click under it
//!   does not raise the first measure; for a change of a level or of a
//!   path after the voices (the amplifier's level and mode, the high-pass
//!   switch, the chorus, the volume) the difference has no edge until the
//!   change makes one, and it is this measure that is held to the bound.
//!   For a change that moves the pitch or a waveform the difference is
//!   one sound less another, and says nothing.
//!
//! A row above [`BOUND`] is a fault unless [`THE_INSTRUMENTS_OWN`] names
//! the document or the circuit that makes the real instrument step there
//! too. Three rows must not change the output at all: the same patch set
//! again, and the messages the instrument ignores.

// Without the feature there is nothing here to test.
#![cfg(feature = "synth106")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    reason = "a test, in f64"
)]

#[path = "synth106_support.rs"]
mod support;

use neoteletext_audio_instruments::synth106::{ChorusMode, Model, Patch, PwmSource, Range, Slider, Synth106};
use support::{CONTROL, MODELS, NOTE_OFF, NOTE_ON, PITCH_BEND, flat, instrument, samples};

/// The sliders, with the names the table prints.
const SLIDERS: [(Slider, &str); 16] = [
    (Slider::LfoRate, "LFO RATE"),
    (Slider::LfoDelay, "LFO DELAY"),
    (Slider::DcoLfo, "DCO LFO"),
    (Slider::DcoPwm, "DCO PWM"),
    (Slider::Noise, "NOISE"),
    (Slider::VcfFrequency, "VCF FREQ"),
    (Slider::VcfResonance, "VCF RES"),
    (Slider::VcfEnvelope, "VCF ENV"),
    (Slider::VcfLfo, "VCF LFO"),
    (Slider::VcfKeyboard, "VCF KYBD"),
    (Slider::VcaLevel, "VCA LEVEL"),
    (Slider::Attack, "ATTACK"),
    (Slider::Decay, "DECAY"),
    (Slider::Sustain, "SUSTAIN"),
    (Slider::Release, "RELEASE"),
    (Slider::Sub, "SUB"),
];

/// The sounds a control is moved under.
const SOUNDS: [&str; 8] =
    ["low sawtooth", "middle-C pulse", "high sub+noise", "chord", "resonant", "attack", "release", "silence"];

/// Instants of a note at which each control is moved, in samples after
/// the sound is set up: none a multiple of synth106's frame of 200
/// samples or of a cycle of any of the notes.
const PHASES: [usize; 4] = [0, 67, 131, 1_009];

/// Seconds measured after a change.
const AFTER: f64 = 0.05;

/// Seconds of a sound before the first of its instants, whose largest
/// step is the sound's own with the largest of the seconds after: six
/// voices' edges fall together now and then, and 50 ms alone would miss
/// the worst of them.
const BEFORE: f64 = 0.2;

/// The largest step ratio, and the largest step of a change's own, that
/// is no click: the sound's own largest step and three tenths. The
/// largest step of 50 ms of six voices is a matter of which of their
/// edges fall together in them, and differs by a fifth from one 50 ms to
/// the next.
const BOUND: f64 = 1.3;

/// One unit of a 16-bit sample: no sound's own step is counted smaller.
const UNIT: f64 = 1.0 / 32_768.0;

/// The share of the level standing in the output that the fastest of the
/// instrument's own slews moves it by in a sample: the amplifier's level,
/// whose time constant is 64 samples, taken from its middle to its top,
/// which doubles it. After a wide pulse's note the output stands away
/// from zero for seconds with no step of its own at all, and a level
/// moved under it is no click for moving what stands there this fast.
const SLEW_SHARE: f64 = 1.0 / 64.0;

/// What is done to the instrument before the sound starts, for the
/// changes that need a controller away from its rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prelude {
    Nothing,
    /// The bender at its top and the wheel at its top.
    BendAndWheel,
    /// The volume at 32.
    VolumeLow,
    /// The pedal down.
    Pedal,
}

/// A change: its control's name, what is done, what must come before,
/// and the doing.
struct Change {
    control: String,
    what: String,
    prelude: Prelude,
    /// The patch the sound plays before the change.
    from: Patch,
    /// Whether the change leaves a setting that a sound can have had from
    /// its start: a patch, or a controller's place. A message that ends
    /// notes does not.
    lasting: bool,
    /// Whether the change moves only a level or a path after the voices:
    /// the difference of the two outputs then has no edge that the sound
    /// has not, and its largest step is held to the bound too.
    level: bool,
    apply: Box<dyn Fn(&mut Synth106)>,
}

impl Change {
    fn patch(control: &str, change: String, from: Patch, patch: Patch) -> Self {
        Self {
            control: control.to_owned(),
            what: change,
            prelude: Prelude::Nothing,
            from,
            lasting: true,
            level: matches!(control, "VCA LEVEL" | "VCA MODE" | "HPF" | "CHORUS"),
            apply: Box::new(move |synth106| synth106.set_patch(&patch)),
        }
    }

    fn message(control: &str, change: &str, prelude: Prelude, from: Patch, messages: &'static [(u8, u8, u8)]) -> Self {
        Self {
            control: control.to_owned(),
            what: change.to_owned(),
            prelude,
            from,
            // The wheel, the volume and the bender stay where they are
            // put; every other message is an event.
            lasting: control.starts_with("CC 1 ") || control.starts_with("CC 7 ") || control == "pitch bend",
            level: control.starts_with("CC 7 "),
            apply: Box::new(move |synth106| {
                for &(status, first, second) in messages {
                    synth106.event(status, first, second);
                }
            }),
        }
    }
}

/// The cutoff slider's place for much the same cutoff on either model,
/// whose sliders cover different ranges: synth106's place, and the
/// synth60's.
const fn cutoff(model: Model, on_106: u8, on_60: u8) -> u8 {
    match model {
        Model::Synth106 => on_106,
        Model::Synth60 => on_60,
    }
}

/// The patch of a sound, and the keys it holds.
fn sound(model: Model, name: &str) -> (Patch, &'static [u8]) {
    let base = flat(model, Patch::DEFAULT);
    match name {
        // A sawtooth two octaves below middle C through a filter at
        // 1.6 kHz, with a little of everything that modulates, so that a
        // slider's step has something to move.
        "low sawtooth" => (
            base.with(Slider::VcfFrequency, cutoff(model, 80, 58))
                .with(Slider::LfoRate, 80)
                .with(Slider::DcoLfo, 10)
                .with(Slider::VcfLfo, 10)
                .with(Slider::VcfEnvelope, 20)
                .with(Slider::Decay, 60)
                .with(Slider::Sustain, 90),
            &[36],
        ),
        // A pulse of three quarters at middle C through 3.4 kHz.
        "middle-C pulse" => {
            let mut patch = base
                .with(Slider::VcfFrequency, cutoff(model, 90, 66))
                .with(Slider::DcoPwm, 64)
                .with(Slider::LfoRate, 70);
            patch.sawtooth = false;
            patch.pulse = true;
            (patch, &[60])
        }
        // Two octaves above middle C with the square below and noise,
        // through 14 kHz.
        "high sub+noise" => (
            base.with(Slider::VcfFrequency, cutoff(model, 110, 82))
                .with(Slider::Sub, 100)
                .with(Slider::Noise, 60)
                .with(Slider::VcfKeyboard, 64)
                .with(Slider::Release, 40),
            &[84],
        ),
        // Six keys of sawtooth, pulse and sub through 1.6 kHz and the
        // chorus.
        "chord" => {
            let mut patch = base
                .with(Slider::VcfFrequency, cutoff(model, 80, 58))
                .with(Slider::DcoPwm, 40)
                .with(Slider::Sub, 50)
                .with(Slider::Attack, 20)
                .with(Slider::Release, 50);
            patch.pulse = true;
            patch.chorus = ChorusMode::One;
            (patch, &[48, 52, 55, 60, 64, 67])
        }
        // A sawtooth an octave below middle C under a filter at 800 Hz
        // that rings just short of oscillating.
        "resonant" => (
            base.with(Slider::VcfFrequency, cutoff(model, 70, 51))
                .with(Slider::VcfResonance, 105)
                .with(Slider::VcfKeyboard, 100)
                .with(Slider::VcfEnvelope, 30)
                .with(Slider::Decay, 80)
                .with(Slider::Sustain, 64),
            &[48],
        ),
        // The G below middle C a seventh of the way up an attack of a
        // second and a half, the envelope opening the filter from 300 Hz.
        "attack" => {
            let mut patch = base
                .with(Slider::VcfFrequency, cutoff(model, 57, 40))
                .with(Slider::VcfEnvelope, 40)
                .with(Slider::Attack, 100)
                .with(Slider::Sub, 80);
            patch.pulse = true;
            (patch, &[55])
        }
        // Middle C dying away over seconds through 1.2 kHz.
        "release" => (
            base.with(Slider::VcfFrequency, cutoff(model, 75, 54))
                .with(Slider::Release, 100)
                .with(Slider::VcfEnvelope, 30),
            &[60],
        ),
        // A pulse at its widest on the gate: once the key is up no voice
        // sounds, and the capacitor before the high-pass switch gives up
        // the pulse's steady part for seconds.
        "silence" => {
            let mut patch = base.with(Slider::DcoPwm, 127).with(Slider::VcfFrequency, cutoff(model, 100, 74));
            patch.sawtooth = false;
            patch.pulse = true;
            patch.vca_gate = true;
            (patch, &[60])
        }
        _ => unreachable!("no sound of that name"),
    }
}

/// A sound to move a control under.
struct Sounding {
    /// The instrument at each of [`PHASES`], with the last sample before
    /// each.
    instants: Vec<(Synth106, i16)>,
    /// The largest step and the largest magnitude of the [`BEFORE`]
    /// seconds before the first of them, full scale 1.
    step: f64,
    peak: f64,
}

/// A sound at each of [`PHASES`]. `early` is done before the first key
/// goes down: with a change there, the sound has had the change's setting
/// from its start.
fn sounding(
    model: Model,
    name: &str,
    patch: &Patch,
    prelude: Prelude,
    early: Option<&dyn Fn(&mut Synth106)>,
) -> Sounding {
    let (_, keys) = sound(model, name);
    let mut synth106 = instrument(model, patch);
    match prelude {
        Prelude::Nothing => {}
        Prelude::BendAndWheel => {
            synth106.event(PITCH_BEND, 0x7f, 0x7f);
            synth106.event(CONTROL, 1, 127);
        }
        Prelude::VolumeLow => synth106.event(CONTROL, 7, 32),
        Prelude::Pedal => synth106.event(CONTROL, 64, 127),
    }
    if let Some(early) = early {
        early(&mut synth106);
    }
    let run = |synth106: &mut Synth106, seconds: f64| synth106.render(&mut vec![0i16; samples(seconds)]);
    run(&mut synth106, 0.05);
    for &key in keys {
        synth106.event(NOTE_ON, key, 100);
    }
    run(&mut synth106, if name == "attack" { 0.02 } else { 0.5 });
    if matches!(name, "release" | "silence") {
        for &key in keys {
            synth106.event(NOTE_OFF, key, 0);
        }
        run(&mut synth106, 0.3);
    }
    let mut before = vec![0i16; samples(BEFORE)];
    synth106.render(&mut before);
    let before: Vec<i32> = before.iter().map(|&sample| i32::from(sample)).collect();
    let (step, peak) = (largest_step(&before), largest(&before));
    let mut instants = Vec::new();
    let mut rendered = 0usize;
    for phase in PHASES {
        // One more sample at least, for the last before the change.
        let mut out = vec![0i16; phase - rendered + 1];
        synth106.render(&mut out);
        rendered = phase + 1;
        instants.push((synth106.clone(), out[out.len() - 1]));
    }
    Sounding { instants, step, peak }
}

/// A position of a switch: its name, and what puts a patch there.
type Position = (String, Box<dyn Fn(&mut Patch)>);
/// A switch of the panel: its name, and each of its positions.
type Switch = (&'static str, Vec<Position>);

/// The switches, each with every position it has.
fn switches() -> Vec<Switch> {
    fn position(name: impl core::fmt::Debug, set: impl Fn(&mut Patch) + 'static) -> Position {
        (format!("{name:?}"), Box::new(set))
    }
    vec![
        (
            "RANGE",
            [Range::Sixteen, Range::Eight, Range::Four]
                .into_iter()
                .map(|range| position(range, move |patch: &mut Patch| patch.range = range))
                .collect(),
        ),
        (
            "PULSE",
            [false, true].into_iter().map(|on| position(on, move |patch: &mut Patch| patch.pulse = on)).collect(),
        ),
        (
            "SAWTOOTH",
            [false, true].into_iter().map(|on| position(on, move |patch: &mut Patch| patch.sawtooth = on)).collect(),
        ),
        (
            "SUB SWITCH",
            [false, true].into_iter().map(|on| position(on, move |patch: &mut Patch| patch.sub = on)).collect(),
        ),
        (
            "PWM MODE",
            [PwmSource::Lfo, PwmSource::Manual, PwmSource::Envelope]
                .into_iter()
                .map(|pwm| position(pwm, move |patch: &mut Patch| patch.pwm = pwm))
                .collect(),
        ),
        (
            "VCF POLARITY",
            [false, true]
                .into_iter()
                .map(|inverted| {
                    position(if inverted { "inverted" } else { "normal" }, move |patch: &mut Patch| {
                        patch.envelope_inverted = inverted;
                    })
                })
                .collect(),
        ),
        (
            "VCA MODE",
            [false, true]
                .into_iter()
                .map(|gate| position(if gate { "gate" } else { "env" }, move |patch: &mut Patch| patch.vca_gate = gate))
                .collect(),
        ),
        ("HPF", (0..=3u8).map(|hpf| position(hpf, move |patch: &mut Patch| patch.hpf = hpf)).collect()),
        (
            "CHORUS",
            [ChorusMode::Off, ChorusMode::One, ChorusMode::Two, ChorusMode::Both]
                .into_iter()
                .map(|chorus| position(chorus, move |patch: &mut Patch| patch.chorus = chorus))
                .collect(),
        ),
    ]
}

/// Every change there is to make to an instrument playing `patch`: each
/// slider by a step and by a quarter of its travel from where the sound
/// has it, and from either end to the other; each switch from each of its
/// positions to each other; and every message.
fn changes(patch: &Patch) -> Vec<Change> {
    let mut all = Vec::new();
    // The same patch again: nothing may change at all.
    all.push(Change::patch("PATCH", "the same again".to_owned(), *patch, *patch));
    for (slider, name) in SLIDERS {
        let now = patch.get(slider);
        let one = if now < 127 { now + 1 } else { now - 1 };
        let quarter = if now <= 127 - 32 { now + 32 } else { now - 32 };
        all.push(Change::patch(name, format!("{now} to {one} (one step)"), *patch, patch.with(slider, one)));
        all.push(Change::patch(name, format!("{now} to {quarter} (a quarter)"), *patch, patch.with(slider, quarter)));
        for (from, to) in [(0u8, 127u8), (127, 0)] {
            all.push(Change::patch(
                name,
                format!("{from} to {to} (end to end)"),
                patch.with(slider, from),
                patch.with(slider, to),
            ));
        }
    }
    for (name, positions) in switches() {
        for (from_name, from) in &positions {
            for (to_name, to) in &positions {
                if from_name != to_name {
                    let (mut before, mut after) = (*patch, *patch);
                    from(&mut before);
                    from(&mut after);
                    to(&mut after);
                    all.push(Change::patch(name, format!("{from_name} to {to_name}"), before, after));
                }
            }
        }
    }
    // A whole other patch at once: the instrument's plain one.
    all.push(Change::patch("PATCH", "to the plain patch".to_owned(), *patch, Patch::DEFAULT));

    let from = *patch;
    let message = |control: &str, change: &str, prelude: Prelude, messages: &'static [(u8, u8, u8)]| {
        Change::message(control, change, prelude, from, messages)
    };
    let nothing = Prelude::Nothing;
    all.extend([
        message("CC 1 wheel", "0 to 1", nothing, &[(CONTROL, 1, 1)]),
        message("CC 1 wheel", "0 to 32", nothing, &[(CONTROL, 1, 32)]),
        message("CC 1 wheel", "0 to 127", nothing, &[(CONTROL, 1, 127)]),
        message("CC 1 wheel", "127 to 0", Prelude::BendAndWheel, &[(CONTROL, 1, 0)]),
        message("CC 7 volume", "127 to 126", nothing, &[(CONTROL, 7, 126)]),
        message("CC 7 volume", "127 to 95", nothing, &[(CONTROL, 7, 95)]),
        message("CC 7 volume", "127 to 0", nothing, &[(CONTROL, 7, 0)]),
        message("CC 7 volume", "32 to 127", Prelude::VolumeLow, &[(CONTROL, 7, 127)]),
        message(
            "CC 7 volume",
            "32 to 0 and back in one call",
            Prelude::VolumeLow,
            &[(CONTROL, 7, 0), (CONTROL, 7, 32)],
        ),
        message("CC 64 pedal", "up to down", nothing, &[(CONTROL, 64, 127)]),
        message("CC 64 pedal", "down to up", Prelude::Pedal, &[(CONTROL, 64, 0)]),
        message(
            "CC 64 pedal",
            "down, keys up, pedal up",
            nothing,
            &[(CONTROL, 64, 127), (CONTROL, 123, 0), (CONTROL, 64, 0)],
        ),
        message("CC 120 all sound off", "sent", nothing, &[(CONTROL, 120, 0)]),
        message("CC 121 reset controllers", "at rest", nothing, &[(CONTROL, 121, 0)]),
        message("CC 121 reset controllers", "bender and wheel at the top", Prelude::BendAndWheel, &[(CONTROL, 121, 0)]),
        message("CC 121 reset controllers", "pedal down", Prelude::Pedal, &[(CONTROL, 121, 0)]),
        message("CC 122 local control", "sent (ignored)", nothing, &[(CONTROL, 122, 0)]),
        message("CC 123 all notes off", "sent", nothing, &[(CONTROL, 123, 0)]),
        message("CC 124 omni off", "sent", nothing, &[(CONTROL, 124, 0)]),
        message("CC 125 omni on", "sent", nothing, &[(CONTROL, 125, 0)]),
        message("CC 126 mono on", "sent", nothing, &[(CONTROL, 126, 1)]),
        message("CC 127 poly on", "sent", nothing, &[(CONTROL, 127, 0)]),
        message("CC 123 all notes off", "pedal down", Prelude::Pedal, &[(CONTROL, 123, 0)]),
        message("CC 10 pan", "sent (ignored)", nothing, &[(CONTROL, 10, 0)]),
        message("pitch bend", "centre to one unit up", nothing, &[(PITCH_BEND, 1, 0x40)]),
        message("pitch bend", "centre to a quarter up", nothing, &[(PITCH_BEND, 0, 0x50)]),
        message("pitch bend", "centre to the top", nothing, &[(PITCH_BEND, 0x7f, 0x7f)]),
        message("pitch bend", "centre to the bottom", nothing, &[(PITCH_BEND, 0, 0)]),
        message("pitch bend", "top to centre", Prelude::BendAndWheel, &[(PITCH_BEND, 0, 0x40)]),
        message("program change", "sent (ignored)", nothing, &[(0xc0, 5, 0)]),
        message("channel pressure", "sent (ignored)", nothing, &[(0xd0, 127, 0)]),
        message("key pressure", "sent (ignored)", nothing, &[(0xa0, 60, 127)]),
        message("system", "0xf0 to 0xff sent (ignored)", nothing, &[(0xf0, 0, 0), (0xf8, 0, 0), (0xff, 0, 0)]),
        message(
            "note off",
            "every key of the sound",
            nothing,
            &[
                (NOTE_OFF, 36, 0),
                (NOTE_OFF, 48, 0),
                (NOTE_OFF, 52, 0),
                (NOTE_OFF, 55, 0),
                (NOTE_OFF, 60, 0),
                (NOTE_OFF, 64, 0),
                (NOTE_OFF, 67, 0),
                (NOTE_OFF, 84, 0),
            ],
        ),
    ]);
    all.push(Change {
        control: "reset()".to_owned(),
        what: "called".to_owned(),
        prelude: nothing,
        from,
        lasting: false,
        level: false,
        apply: Box::new(Synth106::reset),
    });
    all.push(Change {
        control: "release_all()".to_owned(),
        what: "called".to_owned(),
        prelude: nothing,
        from,
        lasting: false,
        level: false,
        apply: Box::new(Synth106::release_all),
    });
    all
}

/// The largest step from one sample to the next, full scale 1.
fn largest_step(signal: &[i32]) -> f64 {
    signal.windows(2).map(|pair| (pair[1] - pair[0]).abs()).max().map_or(0.0, |step| f64::from(step) / 32_768.0)
}

/// The largest magnitude, full scale 1.
fn largest(signal: &[i32]) -> f64 {
    signal.iter().map(|sample| sample.abs()).max().map_or(0.0, |peak| f64::from(peak) / 32_768.0)
}

/// The next [`AFTER`] seconds of an instrument, with the last sample
/// before them at their head.
fn following(synth106: &Synth106, last: i16) -> Vec<i32> {
    let mut synth106 = synth106.clone();
    let mut out = vec![0i16; samples(AFTER)];
    synth106.render(&mut out);
    core::iter::once(i32::from(last)).chain(out.iter().map(|&sample| i32::from(sample))).collect()
}

/// One row of the table: a change under a sound on a model, at the worst
/// of its instants.
struct Row {
    control: String,
    what: String,
    model: Model,
    sound: &'static str,
    /// Whether the difference is what is held to the bound.
    level: bool,
    /// The largest step after the change.
    step: f64,
    /// The largest step of the difference between the output after the
    /// change and the output left alone.
    difference: f64,
    /// What the step is measured against: the sound's own largest step.
    own: f64,
}

impl Row {
    /// What is held to the bound: the step ratio, or for a change of a
    /// level or of a path after the voices the difference's.
    fn measure(&self) -> f64 {
        if self.level { self.difference_ratio() } else { self.ratio() }
    }

    /// The step ratio.
    fn ratio(&self) -> f64 {
        self.step / self.own
    }

    /// The change's own step against the sound's.
    fn difference_ratio(&self) -> f64 {
        self.difference / self.own
    }
}

/// The rows of one sound on one model: each change, at the worst of its
/// instants.
fn rows_of(model: Model, name: &'static str) -> Vec<Row> {
    let (patch, _) = sound(model, name);
    let mut rows = Vec::new();
    // The sound left alone, for each patch it starts from and each
    // prelude, made once.
    let mut made: Vec<(Patch, Prelude, Sounding)> = Vec::new();
    for change in changes(&patch) {
        let same = |(from, prelude, _): &(Patch, Prelude, Sounding)| *from == change.from && *prelude == change.prelude;
        if !made.iter().any(same) {
            made.push((change.from, change.prelude, sounding(model, name, &change.from, change.prelude, None)));
        }
        let alone = &made.iter().find(|one| same(one)).expect("made above").2;
        // The sound that has had the change's setting from its start, at
        // the same instants.
        let always = change.lasting.then(|| sounding(model, name, &change.from, change.prelude, Some(&*change.apply)));
        let (mut step, mut own, mut difference, mut worst) = (0.0f64, 1.0f64, 0.0f64, -1.0f64);
        for (index, (synth106, last)) in alone.instants.iter().enumerate().take(instants()) {
            let mut moved = synth106.clone();
            (change.apply)(&mut moved);
            let after = following(&moved, *last);
            let same = following(synth106, *last);
            let apart: Vec<i32> = after.iter().zip(&same).map(|(a, b)| a - b).collect();
            // The sound's own step: of the sound left alone, of the sound
            // that was always so, and what the fastest of the
            // instrument's own slews makes of the level that stands in
            // the output; a unit at the least.
            let mut reference = largest_step(&same).max(alone.step).max(UNIT);
            let mut standing = largest(&same).max(alone.peak);
            if let Some(always) = &always {
                let (synth106, last) = &always.instants[index];
                let so = following(synth106, *last);
                reference = reference.max(largest_step(&so)).max(always.step);
                standing = standing.max(largest(&so)).max(always.peak);
            }
            reference = reference.max(standing * SLEW_SHARE);
            let (after, apart) = (largest_step(&after), largest_step(&apart));
            let ratio = if change.level { apart } else { after } / reference;
            if ratio > worst {
                (worst, step, own, difference) = (ratio, after, reference, apart);
            }
        }
        rows.push(Row {
            control: change.control,
            what: change.what,
            model,
            sound: name,
            level: change.level,
            step,
            difference,
            own,
        });
    }
    rows
}

/// How many of [`PHASES`] are measured: all of them, and in a build
/// without optimisation, which is some twenty times slower, the first.
const fn instants() -> usize {
    if cfg!(debug_assertions) { 1 } else { PHASES.len() }
}

/// The sounds that are measured: all of them, and in a build without
/// optimisation one that sounds and the silence.
fn sounds() -> &'static [&'static str] {
    if cfg!(debug_assertions) { &["middle-C pulse", "silence"] } else { &SOUNDS }
}

/// Every row: each change under each sound on each model, the sounds
/// measured side by side.
fn sweep() -> Vec<Row> {
    std::thread::scope(|scope| {
        let threads: Vec<_> = MODELS
            .into_iter()
            .flat_map(|model| sounds().iter().map(move |&name| (model, name)))
            .map(|(model, name)| scope.spawn(move || rows_of(model, name)))
            .collect();
        threads.into_iter().flat_map(|thread| thread.join().expect("a sound's rows")).collect()
    })
}

/// The rows that are above the bound and no fault, each with why: what
/// the control and the change begin with, the model if it is true of one
/// only, the sound, and the reason.
const THE_INSTRUMENTS_OWN: [(&str, &str, Option<Model>, &str, &str); 7] = [
    // Not a control of the instrument at all.
    (
        "reset()",
        "",
        None,
        "",
        "reset() is the host's, and puts the instrument back as it was made, silent, by its contract: what sounds \
         is cut off, as when the instrument is switched off",
    ),
    // The envelope, asked for its shortest attack while it climbs a slow
    // one, is at its peak a frame later: the note's own attack, to a
    // level above the sustain that either sound holds. Seen for the
    // sounds whose sustain is below the top.
    (
        "ATTACK",
        "127 to 0",
        None,
        "low sawtooth",
        "the attack ends in the 1.5 ms (synth106) or 1 ms (synth60) the slider now asks (service notes, page 1, \
         Specifications), at the envelope's peak, above the sustain level of either sound",
    ),
    (
        "ATTACK",
        "127 to 0",
        None,
        "resonant",
        "the attack ends in the 1.5 ms (synth106) or 1 ms (synth60) the slider now asks (service notes, page 1, \
         Specifications), at the envelope's peak, above the sustain level of either sound",
    ),
    // Looked at sample by sample: the steps are the sawtooth's edges, on
    // which the filter, opened over 13 octaves in a few thousandths of a
    // second with its resonance near the top, still rings.
    (
        "VCF FREQ",
        "0 to 127",
        Some(Model::Synth106),
        "resonant",
        "the cutoff's voltage goes to its place over the 1 ms of R113 and C61 (JUNO-106 service notes, module \
         board schematic) and the resonant filter rings on the sawtooth's edges as it opens",
    ),
    // Looked at sample by sample: the steps are the sawtooth's edges
    // through the filter the plain patch opens, in the 2 ms that its
    // release leaves of the note.
    (
        "PATCH",
        "to the plain patch",
        Some(Model::Synth60),
        "release",
        "synth60's cutoff voltage is held on C14, 4,700 pF, straight from the switch IC47 with no resistor \
         before it (service notes, CPU board schematic, printed page 9), so the filter opens at once, on a note \
         that the plain patch's release of 2 ms then ends",
    ),
    // Six pulses whose width is moved 45% of a cycle at one instant all
    // change their state at that instant; left alone, their edges fall
    // at six different times.
    (
        "DCO PWM",
        "0 to 127",
        Some(Model::Synth60),
        "chord",
        "the width is the voltage at a comparator (JUNO-60 service notes, page 14, \"Variable pulse width\"), \
         held on C13 with no resistor before it (CPU board schematic): moved at once, six comparators change \
         over together",
    ),
    (
        "DCO PWM",
        "127 to 0",
        Some(Model::Synth60),
        "chord",
        "the width is the voltage at a comparator (JUNO-60 service notes, page 14, \"Variable pulse width\"), \
         held on C13 with no resistor before it (CPU board schematic): moved at once, six comparators change \
         over together",
    ),
];

/// Why the real instrument steps too where a row is above the bound, or
/// nothing if nothing says it would.
fn the_instruments_own(row: &Row) -> Option<&'static str> {
    THE_INSTRUMENTS_OWN
        .iter()
        .find(|(control, change, model, sound, _)| {
            row.control == *control
                && row.what.starts_with(change)
                && model.is_none_or(|model| model == row.model)
                && (sound.is_empty() || row.sound == *sound)
        })
        .map(|&(.., reason)| reason)
}

#[test]
fn every_control_moved_under_every_sound_steps_no_more_than_the_instrument_does() {
    let rows = sweep();
    println!(
        "{:<26} {:<34} {:<8} {:<15} {:>8} {:>8} {:>7} {:>7}",
        "control", "change", "model", "sound", "step", "own", "ratio", "change"
    );
    let mut faults = Vec::new();
    let mut explained = 0usize;
    for row in &rows {
        let above = row.measure() > BOUND;
        let reason = the_instruments_own(row);
        let mark = match (above, reason) {
            (false, _) => "",
            (true, Some(reason)) => &format!("  the instrument's own: {reason}"),
            (true, None) => "  ABOVE",
        };
        println!(
            "{:<26} {:<34} {:<8} {:<15} {:>8.5} {:>8.5} {:>7.2} {:>7.2}{mark}",
            row.control,
            row.what,
            format!("{:?}", row.model),
            row.sound,
            row.step,
            row.own,
            row.ratio(),
            row.difference_ratio(),
        );
        if (row.what == "the same again" || row.what.ends_with("(ignored)")) && row.difference != 0.0 {
            faults.push(format!(
                "{} {} on {:?} under {} changes the output",
                row.control, row.what, row.model, row.sound
            ));
        }
        if above {
            match reason {
                Some(_) => explained += 1,
                None => faults.push(format!(
                    "{} {} on {:?} under {}: {:.2} and {:.2}",
                    row.control,
                    row.what,
                    row.model,
                    row.sound,
                    row.ratio(),
                    row.difference_ratio()
                )),
            }
        }
    }
    println!("{} rows, {} above the bound as the instrument is, {} faults", rows.len(), explained, faults.len());
    assert!(faults.is_empty(), "{} rows click:\n{}", faults.len(), faults.join("\n"));
}

#[test]
fn all_sound_off_closes_the_voices_in_a_fiftieth_of_a_second_and_a_key_struck_meanwhile_sounds() {
    for model in MODELS {
        // Six keys whose release would last twelve seconds.
        let (chord, keys) = sound(model, "chord");
        let mut synth106 = instrument(model, &chord.with(Slider::Release, 127));
        for &key in keys {
            synth106.event(NOTE_ON, key, 100);
        }
        let mut before = vec![0i16; samples(0.5)];
        synth106.render(&mut before);
        let before: Vec<i32> = before.iter().map(|&sample| i32::from(sample)).collect();
        let own = largest_step(&before[before.len() - samples(BEFORE)..]);
        synth106.event(CONTROL, 120, 0);
        // A key in the same instant takes a voice that is still closing.
        synth106.event(NOTE_ON, 72, 100);
        let after = following(&synth106, *before.last().expect("half a second") as i16);
        let step = largest_step(&after);
        println!("{model:?}: all sound off and a key at once: a step of {step:.5} at most, {own:.5} before");
        assert!(step < BOUND * own, "{model:?}: {step} against {own}");
        synth106.render(&mut vec![0i16; samples(0.02)]);
        assert_eq!(synth106.sounding_voices(), 1, "{model:?}: the six have closed and the new key sounds");
        let mut note = vec![0i16; samples(0.1)];
        synth106.render(&mut note);
        let peak = note.iter().map(|sample| sample.unsigned_abs()).max().unwrap_or(0);
        assert!(peak > 1_000, "{model:?}: the key struck with all sound off is not heard: {peak}");
        // And silenced in its turn, with nothing left sounding.
        synth106.event(CONTROL, 120, 0);
        synth106.render(&mut vec![0i16; samples(0.02)]);
        assert_eq!(synth106.sounding_voices(), 0, "{model:?}");
    }
}

/// A run of everything that slews or fades, each begun before the last
/// has ended, rendered in calls of `chunk` samples: the high-pass switch,
/// the VCA switch, the chorus, the volume, all sound off with a key
/// struck while the voices close, and `reset()` in the middle of it all.
/// Gives the samples, and how many of them follow the reset.
fn everything_at_once(model: Model, chunk: usize) -> (Vec<i16>, usize) {
    let mut patch = Patch::DEFAULT.with(Slider::DcoPwm, 100).with(Slider::Release, 60);
    patch.pulse = true;
    patch.chorus = ChorusMode::One;
    patch.hpf = 0;
    let mut synth106 = instrument(model, &patch);
    let mut out = Vec::new();
    let run = |synth106: &mut Synth106, count: usize, out: &mut Vec<i16>| {
        let mut piece = vec![0i16; count];
        for part in piece.chunks_mut(chunk) {
            synth106.render(part);
        }
        out.extend(piece);
    };
    synth106.event(NOTE_ON, 40, 100);
    synth106.event(NOTE_ON, 52, 100);
    run(&mut synth106, 5_000, &mut out);
    for (index, hpf) in [3u8, 0, 2, 1, 3, 2, 0].into_iter().enumerate() {
        patch.hpf = hpf;
        patch.vca_gate = index % 2 == 0;
        if index == 3 {
            patch.chorus = ChorusMode::Two;
        }
        synth106.set_patch(&patch);
        synth106.event(CONTROL, 7, (index * 37 % 128) as u8);
        run(&mut synth106, 97 + index * 131, &mut out);
    }
    synth106.event(CONTROL, 120, 0);
    run(&mut synth106, 33, &mut out);
    synth106.event(NOTE_ON, 64, 100);
    run(&mut synth106, 3_000, &mut out);
    patch.hpf = 3;
    synth106.set_patch(&patch);
    run(&mut synth106, 100, &mut out);
    synth106.reset();
    synth106.event(NOTE_ON, 60, 100);
    let after = 3_000;
    run(&mut synth106, after, &mut out);
    (out, after)
}

#[test]
fn what_slews_and_fades_gives_the_same_samples_in_calls_of_any_length_and_nothing_of_it_outlasts_reset() {
    for model in MODELS {
        let (whole, after) = everything_at_once(model, usize::MAX);
        for chunk in [1usize, 2, 7, 64, 199, 200, 4_800] {
            assert!(
                whole == everything_at_once(model, chunk).0,
                "{model:?}: calls of {chunk} samples give other samples"
            );
        }
        // After the reset: a new instrument given the patch, to the
        // sample, though a filter's corner, a gain and a fade were on
        // their way.
        let mut patch = Patch::DEFAULT.with(Slider::DcoPwm, 100).with(Slider::Release, 60);
        patch.pulse = true;
        patch.chorus = ChorusMode::Two;
        patch.hpf = 3;
        patch.vca_gate = true;
        let mut fresh = instrument(model, &patch);
        fresh.event(NOTE_ON, 60, 100);
        let mut tail = vec![0i16; after];
        fresh.render(&mut tail);
        assert!(whole[whole.len() - after..] == tail[..], "{model:?}: reset() leaves something of what was moving");
    }
}
