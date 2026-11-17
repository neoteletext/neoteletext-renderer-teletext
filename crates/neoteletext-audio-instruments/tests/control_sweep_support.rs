//! What the control sweeps share: every thing that can change under a
//! sounding note is changed, one at a time, and the largest step from
//! one sample to the next in the 50 ms after it is measured against the
//! largest step the sound makes by itself. Each instrument's
//! `<instrument>_control_sweep.rs` includes this as a module and gives
//! it the sounds and the changes; by itself it tests nothing.
//!
//! # The measure
//!
//! A click is a step in the signal that the sound does not make by
//! itself. For one sound and one change, three instruments are played
//! the same way to the same instant:
//!
//! - one that is then left alone, whose largest step is the sound's
//!   **own**;
//! - one that is given the change at that instant, whose largest step in
//!   the 50 ms after it (the first of them from the last sample before
//!   the change) is the **step** in question;
//! - one that had the change from the first, before any key, whose
//!   largest step is the step of the sound that the change goes **to**.
//!
//! The ratio of a row is its step over the larger of the other two: a
//! change that goes from one sound to another with nothing sudden
//! between them stays about one, however different the two sounds are,
//! and a state emptied or a gain moved in one sample stands well above
//! it. Where both sounds are silent a floor stands in for them, so that
//! a ratio is of something. A change that starts a note is also held
//! against the start of that note on an instrument at rest.
//!
//! A sustained sound is played up to a crest of its wave, where a gain
//! that jumps steps the signal most; changed at a crossing of zero it
//! would step nothing and be missed.

#![allow(
    dead_code,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_panics_doc,
    clippy::print_stdout,
    reason = "test support, of which each test file uses a part, printing its table"
)]

use std::collections::BTreeMap;

use neoteletext_audio_instruments::SAMPLE_RATE;

/// Samples in the 50 ms after a change in which its step is looked for.
pub const AFTER: usize = SAMPLE_RATE as usize / 20;
/// Samples over which a sound's own largest step is taken: eight times
/// as long, so that it is the largest of many more of the sound's
/// cycles than the change is measured across, and of a whole cycle of a
/// slow modulation, and is not a lucky small one.
pub const ALONE: usize = 8 * AFTER;
/// The step, in units of a 16-bit sample, that stands in for a sound's
/// own where the sound is silent or nearly: 72 dB below a full-scale
/// sample, and a twelfth of the smallest own step of any sound here.
pub const FLOOR: u32 = 8;
/// The ratio at and below which a change is taken not to click: half as
/// much again as the largest step the sounds make by themselves. A
/// level that goes its whole way in the instruments' 5.3 ms adds a
/// fifth to the largest step of the mellowest sound here, and a third
/// where it rises under a note that is dying away; a level that jumps a
/// quarter of its way in one sample makes ten times that sound's step.
pub const BOUND: f64 = 1.5;

/// MIDI status: note on.
pub const NOTE_ON: u8 = 0x90;
/// Note off.
pub const NOTE_OFF: u8 = 0x80;
/// Polyphonic key pressure.
pub const KEY_PRESSURE: u8 = 0xa0;
/// Control change.
pub const CONTROL: u8 = 0xb0;
/// Program change.
pub const PROGRAM: u8 = 0xc0;
/// Channel pressure.
pub const CHANNEL_PRESSURE: u8 = 0xd0;
/// Pitch bend.
pub const PITCH_BEND: u8 = 0xe0;

/// What the sweep needs of an instrument.
pub trait Instrument: Clone + Send {
    /// Renders the next samples.
    fn play(&mut self, out: &mut [i16]);
}

/// Something done to an instrument.
pub type Act<I> = Box<dyn Fn(&mut I) + Send + Sync>;

/// How a sound is played to the instant of the change.
pub type Play<I> = Box<dyn Fn(&mut I, &mut Clock) -> i16 + Send + Sync>;

/// One sound: an instrument as it is made, and how it is played up to
/// the instant of the change.
pub struct Sound<I> {
    /// Its name in the table.
    pub name: &'static str,
    /// The instrument with the sound's patch, before any key.
    pub make: Box<dyn Fn() -> I + Send + Sync>,
    /// Plays it to the instant of the change, by the clock it is given;
    /// gives the last sample.
    pub play: Play<I>,
}

/// One change of one control.
pub struct Change<I> {
    /// The control, as the table and the allowances name it.
    pub control: String,
    /// What is done to it.
    pub what: String,
    /// What is done before any key, to put the control where the change
    /// starts from.
    pub before: Act<I>,
    /// The change.
    pub apply: Act<I>,
    /// What starts a note, where the change does: done to an instrument
    /// that no key has touched, its largest step is one the instrument
    /// makes by itself, as the two sounds' own are.
    pub onset: Option<Act<I>>,
}

impl<I> Change<I> {
    /// A change from where the sound's patch has the control.
    pub fn new(control: &str, change: &str, apply: impl Fn(&mut I) + Send + Sync + 'static) -> Self {
        Self {
            control: control.into(),
            what: change.into(),
            before: Box::new(|_| {}),
            apply: Box::new(apply),
            onset: None,
        }
    }

    /// A change from where `before` puts the control.
    pub fn from(
        control: &str,
        change: &str,
        before: impl Fn(&mut I) + Send + Sync + 'static,
        apply: impl Fn(&mut I) + Send + Sync + 'static,
    ) -> Self {
        Self {
            control: control.into(),
            what: change.into(),
            before: Box::new(before),
            apply: Box::new(apply),
            onset: None,
        }
    }

    /// A change that starts a note, as `onset` starts it on an
    /// instrument at rest.
    pub fn starting(control: &str, change: &str, apply: impl Fn(&mut I) + Send + Sync + Clone + 'static) -> Self {
        Self { onset: Some(Box::new(apply.clone())), ..Self::new(control, change, apply) }
    }
}

/// The changes of a control that turns or slides from `low` to `high`,
/// standing at `at` in the sound's patch: one step, a quarter of its
/// range, and from each end to the other. `set` puts it at a value.
pub fn continuous<I: 'static>(
    control: &str,
    (low, high): (i32, i32),
    at: i32,
    set: impl Fn(&mut I, i32) + Send + Sync + Clone + 'static,
) -> Vec<Change<I>> {
    let quarter = ((high - low) / 4).max(1);
    let one = if at < high { at + 1 } else { at - 1 };
    let far = if at + quarter <= high { at + quarter } else { at - quarter };
    let to = |value: i32| {
        let set = set.clone();
        move |instrument: &mut I| set(instrument, value)
    };
    vec![
        Change::new(control, &format!("{at} to {one}, a step"), to(one)),
        Change::new(control, &format!("{at} to {far}, a quarter"), to(far)),
        Change::from(control, &format!("{low} to {high}, end to end"), to(low), to(high)),
        Change::from(control, &format!("{high} to {low}, end to end"), to(high), to(low)),
    ]
}

/// The changes of a switch of `positions` positions standing at `at` in
/// the sound's patch: to each other position, and from each back.
/// `name` names a position, and `set` puts the switch at one.
pub fn switch<I: 'static>(
    control: &str,
    positions: i32,
    at: i32,
    name: impl Fn(i32) -> String,
    set: impl Fn(&mut I, i32) + Send + Sync + Clone + 'static,
) -> Vec<Change<I>> {
    let to = |value: i32| {
        let set = set.clone();
        move |instrument: &mut I| set(instrument, value)
    };
    let mut changes = Vec::new();
    for position in (0..positions).filter(|&position| position != at) {
        changes.push(Change::new(control, &format!("{} to {}", name(at), name(position)), to(position)));
        changes.push(Change::from(control, &format!("{} to {}", name(position), name(at)), to(position), to(at)));
    }
    changes
}

/// The names of an on-off switch's positions.
#[must_use]
pub fn on_off(position: i32) -> String {
    if position == 0 { "off".into() } else { "on".into() }
}

/// One row of the table.
#[derive(Clone, Debug)]
pub struct Row {
    /// The control.
    pub control: String,
    /// What was done to it.
    pub what: String,
    /// The sound it was done under.
    pub sound: &'static str,
    /// The sound's own largest step, the largest step of the sound the
    /// change goes to, and the largest step after the change.
    pub own: u32,
    /// See `own`.
    pub there: u32,
    /// See `own`.
    pub step: u32,
}

impl Row {
    /// The step over the larger of the two sounds' own, or of the floor.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        f64::from(self.step) / f64::from(self.own.max(self.there).max(FLOOR))
    }
}

/// The largest step from one sample to the next, the first from `last`.
#[must_use]
pub fn largest_step(last: i16, signal: &[i16]) -> u32 {
    let mut previous = i32::from(last);
    let mut largest = 0u32;
    for &sample in signal {
        largest = largest.max((i32::from(sample) - previous).unsigned_abs());
        previous = i32::from(sample);
    }
    largest
}

/// Renders so many samples.
pub fn run<I: Instrument>(instrument: &mut I, samples: usize) -> Vec<i16> {
    let mut out = vec![0i16; samples];
    instrument.play(&mut out);
    out
}

/// How long each wait for a crest was, so that a second instrument is
/// played for exactly as long as the first was, wherever its own crests
/// are: the two are then at the same instant of the same notes.
#[derive(Default)]
pub struct Clock {
    /// Samples each wait took, in order.
    waits: Vec<usize>,
    /// The wait that comes next, when the clock is being followed.
    next: Option<usize>,
}

impl Clock {
    /// A clock that follows this one's waits.
    fn again(&self) -> Self {
        Self { waits: self.waits.clone(), next: Some(0) }
    }
}

/// Renders `lead` samples and then on to a crest of the wave: a sample
/// within a tenth of the largest of the lead's last 50 ms, or 100 ms
/// more if none comes; or, by a clock that follows another, for as long
/// as that one waited. Gives the last sample.
pub fn to_crest<I: Instrument>(instrument: &mut I, lead: usize, clock: &mut Clock) -> i16 {
    let signal = run(instrument, lead);
    let mut last = signal.last().copied().unwrap_or(0);
    if let Some(next) = clock.next {
        let wait = clock.waits.get(next).copied().unwrap_or(0);
        clock.next = Some(next + 1);
        return run(instrument, wait).last().copied().unwrap_or(last);
    }
    let recent = &signal[signal.len().saturating_sub(AFTER)..];
    let peak = recent.iter().map(|&sample| i32::from(sample).abs()).max().unwrap_or(0);
    let mut wait = 0;
    while wait < 2 * AFTER && i32::from(last).abs() * 10 < peak * 9 {
        last = run(instrument, 1)[0];
        wait += 1;
    }
    clock.waits.push(wait);
    last
}

/// One row: a change under a sound.
#[must_use]
pub fn measure<I: Instrument>(sound: &Sound<I>, change: &Change<I>) -> Row {
    let mut changed = (sound.make)();
    (change.before)(&mut changed);
    let mut clock = Clock::default();
    let last = (sound.play)(&mut changed, &mut clock);
    let mut alone = changed.clone();
    let own = largest_step(last, &run(&mut alone, ALONE));
    (change.apply)(&mut changed);
    let step = largest_step(last, &run(&mut changed, AFTER));
    let mut twin = (sound.make)();
    (change.before)(&mut twin);
    (change.apply)(&mut twin);
    let last = (sound.play)(&mut twin, &mut clock.again());
    let mut there = largest_step(last, &run(&mut twin, ALONE));
    if let Some(onset) = &change.onset {
        let mut fresh = (sound.make)();
        (change.before)(&mut fresh);
        onset(&mut fresh);
        there = there.max(largest_step(0, &run(&mut fresh, ALONE)));
    }
    Row { control: change.control.clone(), what: change.what.clone(), sound: sound.name, own, there, step }
}

/// Every change under every sound, the sounds side by side.
#[must_use]
pub fn sweep<I: Instrument>(sounds: &[Sound<I>], changes: &(dyn Fn(&I) -> Vec<Change<I>> + Sync)) -> Vec<Row> {
    std::thread::scope(|scope| {
        let threads: Vec<_> = sounds
            .iter()
            .map(|sound| {
                scope.spawn(move || {
                    let changes = changes(&(sound.make)());
                    changes.iter().map(|change| measure(sound, change)).collect::<Vec<_>>()
                })
            })
            .collect();
        threads.into_iter().flat_map(|thread| thread.join().expect("a sound's sweep panicked")).collect()
    })
}

/// After all sound off under each sound, the instrument is silent
/// within `within` samples and stays so: what the message is for.
pub fn all_sound_off_silences<I: Instrument>(sounds: &[Sound<I>], off: &(dyn Fn(&mut I) + Sync), within: usize) {
    for sound in sounds {
        let mut instrument = (sound.make)();
        (sound.play)(&mut instrument, &mut Clock::default());
        off(&mut instrument);
        run(&mut instrument, within);
        let after = run(&mut instrument, 2 * AFTER);
        assert!(after.iter().all(|&sample| sample == 0), "{}: it still sounds after all sound off", sound.name);
    }
}

/// The messages that come while all sound off takes the sound out are
/// taken when it has gone, the same however the calls to render are
/// cut: a volume and a key sent straight after the message give the
/// same samples in calls of 7 samples as in one call.
pub fn messages_after_all_sound_off_are_taken<I: Instrument>(make: &dyn Fn() -> I, event: &dyn Fn(&mut I, u8, u8, u8)) {
    let played = |cut: usize| {
        let mut instrument = make();
        event(&mut instrument, NOTE_ON, 60, 100);
        run(&mut instrument, 2 * AFTER);
        event(&mut instrument, CONTROL, 120, 0);
        event(&mut instrument, CONTROL, 7, 64);
        event(&mut instrument, NOTE_ON, 64, 100);
        let mut out = vec![0i16; 8 * AFTER];
        for piece in out.chunks_mut(cut) {
            instrument.play(piece);
        }
        out
    };
    let whole = played(8 * AFTER);
    assert_eq!(whole, played(7), "the calls' lengths change the samples");
    // The key sounds, and at the volume that was asked: a quarter of
    // the level of the same key with the volume left alone.
    let mut plain = make();
    event(&mut plain, NOTE_ON, 64, 100);
    let full = run(&mut plain, 8 * AFTER);
    let peak = |signal: &[i16]| signal[4 * AFTER..].iter().map(|&sample| sample.unsigned_abs()).max().unwrap_or(0);
    let (quiet, loud) = (f64::from(peak(&whole)), f64::from(peak(&full)));
    assert!(loud > 100.0 && (quiet / loud - 0.254).abs() < 0.03, "the key after: {quiet} against {loud}");
}

/// All sound off leaves the pedal where it is: with the pedal down, a
/// key struck after the message and let go still sounds half a second
/// on, and stops when the pedal goes up. `make` gives an instrument
/// whose patch sustains and whose release is short.
pub fn all_sound_off_leaves_the_pedal<I: Instrument>(make: &dyn Fn() -> I, event: &dyn Fn(&mut I, u8, u8, u8)) {
    let mut instrument = make();
    event(&mut instrument, CONTROL, 64, 127);
    event(&mut instrument, NOTE_ON, 60, 100);
    run(&mut instrument, 2 * AFTER);
    event(&mut instrument, CONTROL, 120, 0);
    run(&mut instrument, AFTER);
    event(&mut instrument, NOTE_ON, 64, 100);
    run(&mut instrument, 2 * AFTER);
    event(&mut instrument, NOTE_OFF, 64, 0);
    let held = run(&mut instrument, 10 * AFTER);
    assert!(held[9 * AFTER..].iter().any(|&sample| sample.unsigned_abs() > 64), "the pedal no longer holds");
    event(&mut instrument, CONTROL, 64, 0);
    let after = run(&mut instrument, 20 * AFTER);
    assert!(after[19 * AFTER..].iter().all(|&sample| sample.unsigned_abs() < 8), "the note outlives the pedal");
}

/// Controls whose change steps the signal on the instrument itself.
pub struct Allowed {
    /// The controls, as the rows name them.
    pub controls: &'static [&'static str],
    /// The fact of the circuit that makes them step.
    pub why: &'static str,
}

impl Allowed {
    fn covers(&self, control: &str) -> bool {
        self.controls.contains(&control)
    }
}

/// Prints the table and holds every row to [`BOUND`] but those of the
/// controls that are `allowed`; an allowance that no row needs fails
/// too, so that the list says what the instrument does and no more.
pub fn check(title: &str, rows: &[Row], allowed: &[Allowed]) {
    println!("{title}: {} rows", rows.len());
    println!(
        "{:<34} {:<34} {:<12} {:>6} {:>6} {:>6} {:>7}",
        "control", "change", "sound", "own", "to", "step", "ratio"
    );
    for row in rows {
        let mark = if row.ratio() > BOUND { " *" } else { "" };
        println!(
            "{:<34} {:<34} {:<12} {:>6} {:>6} {:>6} {:>7.2}{mark}",
            row.control,
            row.what,
            row.sound,
            row.own,
            row.there,
            row.step,
            row.ratio()
        );
    }
    let above: Vec<&Row> = rows.iter().filter(|row| row.ratio() > BOUND).collect();
    let mut worst: BTreeMap<&str, (usize, f64)> = BTreeMap::new();
    for row in &above {
        let entry = worst.entry(row.control.as_str()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 = entry.1.max(row.ratio());
    }
    println!("{title}: {} of {} rows above {BOUND}, of {} controls:", above.len(), rows.len(), worst.len());
    for (control, (count, ratio)) in &worst {
        let why = allowed.iter().find(|allowance| allowance.covers(control)).map_or("NOT ALLOWED", |a| a.why);
        println!("  {control:<34} {count:>3} rows, worst {ratio:>7.2}: {why}");
    }
    let faults: Vec<String> = above
        .iter()
        .filter(|row| !allowed.iter().any(|allowance| allowance.covers(&row.control)))
        .map(|row| format!("{} ({}) under {}: {:.2}", row.control, row.what, row.sound, row.ratio()))
        .collect();
    assert!(faults.is_empty(), "{title}: {} changes click:\n{}", faults.len(), faults.join("\n"));
    let stale: Vec<&str> = allowed
        .iter()
        .flat_map(|allowance| allowance.controls)
        .copied()
        .filter(|control| !worst.contains_key(control))
        .collect();
    assert!(stale.is_empty(), "{title}: allowed to click, and do not: {stale:?}");
}
