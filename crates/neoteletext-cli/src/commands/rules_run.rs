//! `neottx rules-run` (animation.md ANIM-18, REQ-6529): a page rule table
//! evaluated against a file of timed inputs, with `neoteletext-rules`, and
//! the animation operations each tick's actions produce written in the
//! form of an image sequence's script (`crate::script`), so that a
//! rule-driven page of any renderer can be played into images without the
//! browser. Reports and sound operations are written as comment lines,
//! which a script's reader skips.
//!
//! The table is the RULES record's body (page-rules.md RULE-41), as bytes
//! or as hexadecimal text. The inputs are one a line, in ascending tick:
//!
//! ```text
//! <tick> key down|up char|name|physical <key>
//! <tick> pointer <x> <y> <buttons>
//! <tick> level <audio channel> <0-255>
//! <tick> rectangle <n> <x> <y> <width> <height>
//! ```
//!
//! A `rectangle` gives object `n` its bounding rectangle in the picture's
//! pixels (ANIM-13), which no renderer is here to report; a `move` the
//! table then makes moves it with the object. Nothing here reads a clock:
//! page tick `n` is the `n`th turn of the loop.
//!
//! With `--on-arrival` each key and pointer input is given to the
//! evaluator as a receiver gives one the moment it arrives (page-rules.md
//! RULE-96): the sound operations of the rules it fires are written
//! first among its tick's lines, and every line is otherwise the same.

use super::{io_failure, sandbox_closed};
use crate::operations::to_core;
use crate::script::{MAX_SCRIPT_BYTES, write_operation};
use neoteletext_app::{Failure, exit};
use neoteletext_core::consts::{CELL_HEIGHT, CELL_WIDTH, PAGE_HEIGHT, PAGE_WIDTH};
use neoteletext_core::limits::{MAX_ANIMATED_OBJECTS, MAX_CYCLING_OBJECTS};
use neoteletext_rules::animate::{AnimationStore, Applied, Operation, OperationKind};
use neoteletext_rules::evaluator::{
    Bounds, Input, InputEvent, KeyAction, KeyIdentity, PointerAction, Report, Reported, SoundOperation, SoundState,
    TargetState,
};
use neoteletext_rules::limits::{
    AUDIO_CHANNELS, MAX_BUTTON, MAX_GAIN, MAX_POINTER_OBJECTS, MAX_TABLE_BYTES, MAX_WATCHED, SUBCODE_ANY,
};
use neoteletext_rules::state::PageState;
use neoteletext_rules::table::{Target, TargetKind, WATCH_LEVEL};
use neoteletext_rules::{Evaluator, Table, World};
use std::io::{Read as _, Write as _};
use std::path::Path;

/// The most ticks a run evaluates, as an image sequence plays (ANIM-18).
use super::MAX_SEQUENCE_TICKS;

/// The most objects given a rectangle in one run: more than any page's
/// table names (`MAX_TABLE_TARGETS`, 64), so that a file of inputs cannot
/// make the run hold an unbounded list.
const MAX_RECTANGLES: usize = 256;
/// The handle the stand-in local sound of each audio channel answers
/// with: the first a channel gives a local sound (page-rules.md RULE-90).
const LOCAL_HANDLE: u8 = 1;
/// The layer and order of an object no operation has ordered: the scene's
/// layer, as a ReGIS drawing's default is (ANIM-10).
const DEFAULT_LAYER: u8 = 1;

/// Whose picture the table's page is: its size in pixels, and how many of
/// them one unit of an object's position is (ANIM-6: cells for a teletext
/// sprite, pixels for a ReGIS drawing and a NAPLPS object).
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Picture {
    /// A teletext page: 480 by 500 dots, positions in cells of 12 by 20.
    Teletext,
    /// A ReGIS picture: 800 by 480 pixels.
    Regis,
    /// A NAPLPS picture: 640 by 480 pixels.
    Naplps,
}

impl Picture {
    /// The picture's width and height (page-rules.md RULE-13).
    const fn size(self) -> (u16, u16) {
        match self {
            Self::Teletext => (PAGE_WIDTH as u16, PAGE_HEIGHT as u16),
            Self::Regis => (800, 480),
            Self::Naplps => (640, 480),
        }
    }

    /// The pixels across and down of one unit of an object's position.
    fn unit(self) -> (i32, i32) {
        match self {
            Self::Teletext => (i32::try_from(CELL_WIDTH).unwrap_or(1), i32::try_from(CELL_HEIGHT).unwrap_or(1)),
            Self::Regis | Self::Naplps => (1, 1),
        }
    }
}

/// One line of the inputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Given {
    Key { down: bool, key: KeyIdentity, base: Option<u32> },
    Pointer { x: u16, y: u16, buttons: u8 },
    Level { channel: u8, level: u8 },
    Rectangle { object: u16, bounds: Bounds },
}

fn number<T: core::str::FromStr>(words: &mut core::str::SplitAsciiWhitespace<'_>, what: &str) -> Result<T, String> {
    let word = words.next().ok_or_else(|| format!("{what} is missing"))?;
    word.parse().map_err(|_| format!("{what} is not a number in its range: {word:?}"))
}

/// The one character of a word, or the scalar value a `U+XXXX` word names.
fn character(word: &str) -> Option<u32> {
    if let Some(hexadecimal) = word.strip_prefix("U+") {
        return u32::from_str_radix(hexadecimal, 16).ok().filter(|&value| char::from_u32(value).is_some());
    }
    let mut characters = word.chars();
    let only = characters.next()?;
    characters.next().is_none().then_some(u32::from(only))
}

fn given(name: &str, words: &mut core::str::SplitAsciiWhitespace<'_>) -> Result<Given, String> {
    let given = match name {
        "key" => {
            let down = match words.next() {
                Some("down") => true,
                Some("up") => false,
                _ => return Err("a key is down or up".to_owned()),
            };
            let kind = words.next().ok_or("a key is a char, a name or physical")?;
            let word = words.next().ok_or("the key is missing")?;
            let (key, base) = match kind {
                "char" => {
                    (KeyIdentity::Character(character(word).ok_or("a char key is one character, or U+XXXX")?), None)
                }
                "name" => {
                    let code = neoteletext_input::keys::code(word).ok_or_else(|| format!("{word:?} names no key"))?;
                    (KeyIdentity::Named(code), None)
                }
                // A physical key is named by the character it has on a
                // United States layout, which the KEY event carries as its
                // base (page-rules.md RULE-10).
                "physical" => {
                    let base = character(word).ok_or("a physical key is one character, or U+XXXX")?;
                    (KeyIdentity::Character(base), Some(base))
                }
                _ => return Err("a key is a char, a name or physical".to_owned()),
            };
            Given::Key { down, key, base }
        }
        "pointer" => {
            Given::Pointer { x: number(words, "x")?, y: number(words, "y")?, buttons: number(words, "the buttons")? }
        }
        "level" => {
            let channel: u8 = number(words, "the audio channel")?;
            if usize::from(channel) >= AUDIO_CHANNELS {
                return Err(format!("an audio channel is 0 to {}", AUDIO_CHANNELS.saturating_sub(1)));
            }
            Given::Level { channel, level: number(words, "the level")? }
        }
        "rectangle" => {
            let object: u16 = number(words, "the object")?;
            let (x, y): (i16, i16) = (number(words, "x")?, number(words, "y")?);
            let (width, height): (u16, u16) = (number(words, "the width")?, number(words, "the height")?);
            if object == 0 {
                return Err("an object's number is 1 to 65,535".to_owned());
            }
            let (left, top) = (i32::from(x), i32::from(y));
            let bounds = Bounds {
                left,
                top,
                right: left.saturating_add(i32::from(width)),
                bottom: top.saturating_add(i32::from(height)),
            };
            Given::Rectangle { object, bounds }
        }
        _ => return Err(format!("{name:?} is not an input")),
    };
    match words.next() {
        None => Ok(given),
        Some(word) => Err(format!("{word:?} follows the input's last word")),
    }
}

/// Reads the inputs: `(tick, input)` in ascending tick. An empty line, and
/// a line that begins with `#`, is skipped.
fn parse_inputs(text: &str) -> Result<Vec<(u32, Given)>, String> {
    let mut inputs = Vec::new();
    let mut last_tick = 0;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let refused = |why: String| format!("line {}: {why}", index.saturating_add(1));
        let mut words = line.split_ascii_whitespace();
        let tick: u32 = number(&mut words, "the tick").map_err(refused)?;
        if tick < last_tick {
            return Err(refused(format!("tick {tick} comes after tick {last_tick}: the inputs are in ascending tick")));
        }
        last_tick = tick;
        let name = words.next().ok_or_else(|| refused("the input is missing".to_owned()))?;
        inputs.push((tick, given(name, &mut words).map_err(refused)?));
    }
    Ok(inputs)
}

/// The page as the evaluator reads it, and where its outputs go: the
/// objects' rectangles the inputs gave, the animation state the table's
/// own operations made, and the lines written.
struct Run {
    picture: Picture,
    rectangles: Vec<(u16, Bounds)>,
    animation: AnimationStore<MAX_ANIMATED_OBJECTS>,
    out: Vec<String>,
}

impl Run {
    fn rectangle(&self, object: u16) -> Option<Bounds> {
        self.rectangles.iter().find(|(number, _)| *number == object).map(|(_, bounds)| *bounds)
    }
}

impl World for Run {
    fn picture(&self) -> (u16, u16) {
        self.picture.size()
    }

    /// Every control and object the table names exists here: no renderer
    /// is present to say otherwise. One has a rectangle when the inputs
    /// gave it one and it is shown.
    fn target(&self, target: Target) -> Option<TargetState> {
        let state = self.animation.get(target).copied().unwrap_or_default();
        let shown = state.shown.unwrap_or(true);
        let (x, y) = state.position.unwrap_or((0, 0));
        let bounds = match target.kind {
            TargetKind::Object if shown => self.rectangle(target.number),
            _ => None,
        };
        Some(TargetState {
            shown,
            cycling: state.cycle.is_some(),
            frame: state.frame,
            look: state.look,
            layer: state.layer.unwrap_or(DEFAULT_LAYER),
            order: state.order.unwrap_or(0),
            x,
            y,
            bounds,
        })
    }

    fn objects_at(&self, x: u16, y: u16, objects: &mut [u16; MAX_POINTER_OBJECTS]) -> usize {
        let (x, y) = (i32::from(x), i32::from(y));
        let inside = self.rectangles.iter().filter(|(number, bounds)| {
            let shown = self.animation.get(Target::object(*number)).and_then(|state| state.shown).unwrap_or(true);
            shown && x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom
        });
        let mut count = 0;
        for ((number, _), slot) in inside.zip(objects.iter_mut()) {
            *slot = *number;
            count = usize::saturating_add(count, 1);
        }
        count
    }

    /// No sound plays here. So that a table's MIDI messages are written
    /// and not skipped for want of an instrument (page-rules.md RULE-20),
    /// every audio channel answers as holding a local sound at full gain;
    /// a sound's level is what a `level` line gives.
    fn sound(&self, _channel: u8) -> SoundState {
        SoundState { playing: true, local: true, handle: LOCAL_HANDLE, gain: MAX_GAIN, family: 0, variant: 0 }
    }

    fn synth(&self, _channel: u8, _part: u8, _class: u8, _index: u16) -> i32 {
        0
    }

    fn sound_operation(&mut self, operation: SoundOperation) -> bool {
        self.out.push(match operation {
            SoundOperation::Midi { channel, status, data1, data2 } => {
                format!("# midi {channel} {status:#04x} {data1} {data2}")
            }
            SoundOperation::Play { channel, handle, generation, gain, fade_ms, looped } => {
                format!("# play {channel} {handle} {generation} {gain} {fade_ms}{}", if looped { " loop" } else { "" })
            }
            SoundOperation::Stop { channel, fade_ms } => format!("# stop {channel} {fade_ms}"),
            SoundOperation::Gain { channel, gain, fade_ms } => format!("# gain {channel} {gain} {fade_ms}"),
        });
        true
    }

    fn animate(&mut self, operation: Operation, tick: u32) -> bool {
        // The object's rectangle moves with it, by the picture's units.
        if let (TargetKind::Object, OperationKind::Move { relative, x, y }) = (operation.target.kind, operation.kind) {
            let (from_x, from_y) =
                self.animation.get(operation.target).and_then(|state| state.position).unwrap_or((0, 0));
            let (by_x, by_y) = if relative {
                (i32::from(x), i32::from(y))
            } else {
                (i32::from(x).saturating_sub(i32::from(from_x)), i32::from(y).saturating_sub(i32::from(from_y)))
            };
            let (unit_x, unit_y) = self.picture.unit();
            let (by_x, by_y) = (by_x.saturating_mul(unit_x), by_y.saturating_mul(unit_y));
            if let Some((_, bounds)) = self.rectangles.iter_mut().find(|(number, _)| *number == operation.target.number)
            {
                *bounds = Bounds {
                    left: bounds.left.saturating_add(by_x),
                    top: bounds.top.saturating_add(by_y),
                    right: bounds.right.saturating_add(by_x),
                    bottom: bounds.bottom.saturating_add(by_y),
                };
            }
        }
        if self.animation.apply(&operation, (0, 0), tick, MAX_CYCLING_OBJECTS) != Applied::Done {
            return false;
        }
        match to_core(&operation) {
            Some((target, applied)) => {
                self.out.push(write_operation(tick, target, applied));
                true
            }
            None => false,
        }
    }

    fn report(&mut self, report: Report) {
        let kind = match report.target.kind {
            TargetKind::Page => "page",
            TargetKind::Control => "control",
            TargetKind::Object => "object",
            TargetKind::Rect => "rectangle",
        };
        let what = match report.what {
            Reported::Fired => "fired".to_owned(),
            Reported::Input { sequence } => format!("input {sequence}"),
            Reported::Value { reading, .. } => format!("value {reading}"),
        };
        self.out.push(format!(
            "# report rule {} tick {} target {kind} {} {what}",
            report.rule, report.tick, report.target.number
        ));
    }

    fn advance_cycles(&mut self, tick: u32) {
        self.animation.advance(tick, &mut |_| {});
    }

    fn page_state(&mut self, _state: &PageState) {}
}

/// The table's bytes: the file's own, or the bytes its hexadecimal text
/// names when the file is all hexadecimal digits and white space.
fn table_bytes(file: &[u8]) -> Vec<u8> {
    let text: Vec<u8> = file.iter().copied().filter(|byte| !byte.is_ascii_whitespace()).collect();
    let hexadecimal = !text.is_empty() && text.len().is_multiple_of(2) && text.iter().all(u8::is_ascii_hexdigit);
    if !hexadecimal {
        return file.to_vec();
    }
    text.chunks(2)
        .filter_map(|pair| core::str::from_utf8(pair).ok().and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect()
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    let most = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    std::fs::File::open(path)
        .and_then(|file| file.take(most).read_to_end(&mut bytes))
        .map_err(|error| io_failure(path, error))?;
    if bytes.len() > limit {
        return Err(io_failure(path, format!("at most {limit} bytes are read")));
    }
    Ok(bytes)
}

/// Evaluates `ticks` ticks of the table in `table_file`'s bytes against
/// `inputs`, and gives the lines to write. With `on_arrival`, each event
/// is given by `Evaluator::push_now` (RULE-96) and not left for the tick.
fn evaluate(
    table_file: &[u8],
    inputs: &[(u32, Given)],
    ticks: u32,
    picture: Picture,
    on_arrival: bool,
) -> Result<Vec<String>, String> {
    // A decoded table and its evaluator are tens of kilobytes each.
    let mut table = Box::new(Table::new());
    let header = neoteletext_rules::decode(&table_bytes(table_file), &mut table)
        .map_err(|fault| format!("the table is refused: {fault:?}"))?;
    let mut evaluator =
        Box::new(Evaluator::new(&table, header.generation, SUBCODE_ANY).map_err(|fault| format!("{fault:?}"))?);
    let watch: Vec<(u8, u8)> = evaluator.index().watch.iter().map(|slot| (slot.channel, slot.kind)).collect();
    let mut metered = [0u8; MAX_WATCHED];
    let mut run = Run { picture, rectangles: Vec::new(), animation: AnimationStore::new(), out: Vec::new() };
    let mut lines =
        vec![format!("# page {:03X}, generation {}, {} rules", header.page, header.generation, table.rules.len())];
    let mut pending = inputs.iter().peekable();
    let (mut sequence, mut held, mut at) = (0u32, 0u8, None::<(u16, u16)>);
    // A report or a sound operation is a comment of its tick.
    let of_tick = |tick: u32, line: String| match line.strip_prefix("# ") {
        Some(comment) => format!("# {tick} {comment}"),
        None => line,
    };
    let mut events: Vec<InputEvent> = Vec::new();
    for tick in 0..ticks {
        while let Some((_, given)) = pending.next_if(|(due, _)| *due <= tick) {
            let mut push = |input: Input| {
                sequence = sequence.saturating_add(1);
                events.push(InputEvent { sequence, input });
            };
            match *given {
                Given::Key { down, key, base } => {
                    let action = if down { KeyAction::Press } else { KeyAction::Release };
                    push(Input::Key { action, key, base, mods: 0 });
                }
                Given::Pointer { x, y, buttons } => {
                    if at != Some((x, y)) || buttons == held {
                        push(Input::Pointer { action: PointerAction::Move, button: buttons, x, y, control: 0 });
                    }
                    for button in 0..=MAX_BUTTON {
                        let bit = 1u8.checked_shl(u32::from(button)).unwrap_or(0);
                        if (buttons ^ held) & bit != 0 {
                            let action = if buttons & bit != 0 { PointerAction::Press } else { PointerAction::Release };
                            push(Input::Pointer { action, button, x, y, control: 0 });
                        }
                    }
                    (held, at) = (buttons, Some((x, y)));
                }
                Given::Level { channel, level } => {
                    for (slot, _) in
                        metered.iter_mut().zip(&watch).filter(|(_, watched)| **watched == (channel, WATCH_LEVEL))
                    {
                        *slot = level;
                    }
                    evaluator.set_metered(&metered);
                }
                Given::Rectangle { object, bounds } => {
                    if let Some((_, held)) = run.rectangles.iter_mut().find(|(number, _)| *number == object) {
                        *held = bounds;
                    } else if run.rectangles.len() < MAX_RECTANGLES {
                        run.rectangles.push((object, bounds));
                    } else {
                        return Err(format!("the inputs give more than {MAX_RECTANGLES} objects a rectangle"));
                    }
                }
            }
        }
        // The tick's levels and rectangles are in place before its
        // first event either way, so both ways read the same snapshot
        // (RULE-7).
        for event in events.drain(..) {
            let _ = if on_arrival { evaluator.push_now(event, &mut run) } else { evaluator.push(event) };
        }
        evaluator.tick(&mut run);
        lines.extend(run.out.drain(..).map(|line| of_tick(tick, line)));
    }
    let counters = evaluator.counters();
    lines.push(format!(
        "# {} ticks, {} rules fired, {} actions run, {} skipped, {} events dropped, {} overruns",
        counters.ticks, counters.fired, counters.actions, counters.skipped, counters.events_dropped, counters.overruns
    ));
    Ok(lines)
}

/// `neottx rules-run`.
///
/// # Errors
/// A file that cannot be read or is over its bound, a table its decoder
/// refuses, and a line of the inputs that is not understood.
pub fn rules_run(
    table: &Path,
    inputs: &Path,
    frames: Option<u32>,
    picture: Picture,
    on_arrival: bool,
    no_sandbox: bool,
) -> Result<(), Failure> {
    // Both files are read, within their bounds, before the sandbox closes,
    // and parsed after it (REQ-SEC-017).
    let table_file = read_bounded(table, MAX_TABLE_BYTES.saturating_mul(3))?;
    let inputs_file = read_bounded(inputs, MAX_SCRIPT_BYTES)?;
    sandbox_closed(no_sandbox)?;
    let text = String::from_utf8(inputs_file).map_err(|_| io_failure(inputs, "the inputs are text in UTF-8"))?;
    let given = parse_inputs(&text).map_err(|why| io_failure(inputs, why))?;
    // Without --frames, one tick past the last input, so that its effect
    // is applied.
    let last = given.last().map_or(0, |(tick, _)| *tick);
    let ticks = frames.unwrap_or_else(|| last.saturating_add(2)).min(MAX_SEQUENCE_TICKS);
    let lines = evaluate(&table_file, &given, ticks, picture, on_arrival)
        .map_err(|why| (exit::INPUT, format!("{}: {why}", table.display())))?;
    let mut out = std::io::stdout().lock();
    for line in lines {
        writeln!(out, "{line}").map_err(|error| (exit::INPUT, error.to_string()))?;
    }
    Ok(())
}
