//! `neottx rules-run` (animation.md ANIM-18, REQ-6529): a page rule table
//! evaluated against a file of timed inputs, with `neoteletext-rules`, and
//! the animation operations its groups' `ANIMATE` commands make written
//! in the form of an image sequence's script (`crate::script`), so that a
//! rule-driven page of any renderer can be played into images without the
//! browser. Reports and every other command a run gives the page are
//! written as comment lines, which a script's reader skips.
//!
//! The table is one of three things (`crate::rules_json`): the program
//! protocol's `rules` message as JSON (page-rules.md RULE-35), which names
//! its own page; or the table's image (RULE-41), as bytes or as
//! hexadecimal text, for the page `--page` and `--sub` name. The inputs
//! are one a line, in ascending tick:
//!
//! ```text
//! <tick> key down|up char|name|physical <key>
//! <tick> pointer <x> <y> <buttons>
//! <tick> level <audio channel> <0-255>
//! <tick> rectangle <n> <x> <y> <width> <height>
//! <tick> cue <audio channel> <n>
//! <tick> command <record type> <hexadecimal body>
//! ```
//!
//! A `rectangle` gives object `n` its bounding rectangle in the picture's
//! pixels (ANIM-13), which no renderer is here to report; a `move` the
//! table then makes moves it with the object. A `cue` is one the sound
//! helper reports heard (RULE-109), and a `command` a `VARIABLE`, `TIMER`,
//! `REPORT`, `RUN` or `SEQUENCE` record the server sends directly
//! (RULE-114). Nothing here reads a clock: page tick `n` is the `n`th
//! turn of the loop.
//!
//! Each input is given to the evaluator in the order of the lines, after
//! the tick step of the tick it names, and acts at once as it does in a
//! receiver (DEC-6635 item 1): its lines come after that tick step's own.

use super::{io_failure, sandbox_closed};
use crate::operations::to_core;
use crate::script::{MAX_SCRIPT_BYTES, write_operation};
use neoteletext_app::{Failure, exit};
use neoteletext_audio::record::{FLAG_LOOP, Operation as SoundOperation, Record};
use neoteletext_core::consts::{CELL_HEIGHT, CELL_WIDTH, PAGE_HEIGHT, PAGE_WIDTH};
use neoteletext_core::limits::{MAX_ANIMATED_OBJECTS, MAX_CYCLING_OBJECTS};
use neoteletext_core::types::SubCode;
use neoteletext_rules::animate::{self, AnimationStore, Applied, Operation, OperationKind};
use neoteletext_rules::command::{
    self, Command, MAX_BODY_BYTES, TYPE_ANIMATE, TYPE_CONTROL, TYPE_LOCAL_MIDI, TYPE_NAPLPS_OBJECTS, TYPE_SPRITES,
};
use neoteletext_rules::evaluator::{
    Bounds, Event, Input, InputEvent, KeyAction, KeyIdentity, PointerAction, Report, Reported, SoundState, TargetState,
};
use neoteletext_rules::limits::{
    AUDIO_CHANNELS, MAX_BUTTON, MAX_CUE, MAX_GAIN, MAX_POINTER_OBJECTS, MAX_WATCHED, NO_RULE,
};
use neoteletext_rules::state::PageState;
use neoteletext_rules::table::{Target, TargetKind, WATCH_LEVEL};
use neoteletext_rules::{Evaluator, Table, World};
use neoteletext_web::records::body_check;
use std::io::{Read as _, Write as _};
use std::path::Path;

/// The most ticks a run evaluates, as an image sequence plays (ANIM-18).
use super::MAX_SEQUENCE_TICKS;

/// The most objects given a rectangle in one run: four times what any
/// page's table names, so that a file of inputs cannot make the run hold
/// an unbounded list.
const MAX_RECTANGLES: usize = 4 * neoteletext_rules::limits::MAX_TABLE_TARGETS;
/// The handle the stand-in local sound of each audio channel answers
/// with: the first a channel gives a local sound (page-rules.md RULE-90).
const LOCAL_HANDLE: u8 = 1;
/// The layer and order of an object no operation has ordered: the scene's
/// layer, as a ReGIS drawing's default is (ANIM-10).
const DEFAULT_LAYER: u8 = 1;

/// Whose picture the table's page is: its size in pixels, and how many of
/// them one unit of an object's position is (ANIM-6: cells for a teletext
/// sprite, pixels for a ReGIS drawing, raster dots for a NAPLPS object).
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Picture {
    /// A teletext page: 480 by 500 dots, positions in cells of 12 by 20.
    Teletext,
    /// A ReGIS picture: 800 by 480 pixels.
    Regis,
    /// A NAPLPS picture: the teletext raster's 480 by 500 dots, positions in
    /// dots (RAS-5, DEC-6853).
    Naplps,
}

impl Picture {
    /// The picture's width and height (page-rules.md RULE-13).
    const fn size(self) -> (u16, u16) {
        match self {
            Self::Teletext | Self::Naplps => (PAGE_WIDTH as u16, PAGE_HEIGHT as u16),
            Self::Regis => (800, 480),
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
    Key {
        down: bool,
        key: KeyIdentity,
        base: Option<u32>,
    },
    Pointer {
        x: u16,
        y: u16,
        buttons: u8,
    },
    Level {
        channel: u8,
        level: u8,
    },
    Rectangle {
        object: u16,
        bounds: Bounds,
    },
    Cue {
        channel: u8,
        number: u8,
    },
    /// A record the server sends directly: its type, its body and the
    /// body's length.
    Command {
        kind: u8,
        body: [u8; MAX_BODY_BYTES],
        length: usize,
    },
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
        "cue" => {
            let (channel, number): (u8, u8) = (number(words, "the audio channel")?, number(words, "the cue")?);
            if usize::from(channel) >= AUDIO_CHANNELS || number > MAX_CUE {
                return Err(format!("a cue is an audio channel below {AUDIO_CHANNELS} and a number to {MAX_CUE}"));
            }
            Given::Cue { channel, number }
        }
        "command" => {
            let kind: u8 = number(words, "the record type")?;
            let bytes = hexadecimal(words.next().ok_or("the command's body is missing")?.as_bytes())
                .ok_or("a command's body is hexadecimal digits, two a byte")?;
            let mut body = [0_u8; MAX_BODY_BYTES];
            let room = body.get_mut(..bytes.len()).ok_or("the body is longer than any command's")?;
            room.copy_from_slice(&bytes);
            Given::Command { kind, body, length: bytes.len() }
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

    /// No sound plays here. So that a table's sound commands are written
    /// and not skipped for want of an instrument (page-rules.md RULE-118),
    /// every audio channel answers as holding a local sound at full gain;
    /// a sound's level is what a `level` line gives.
    fn sound(&self, _channel: u8) -> SoundState {
        SoundState { playing: true, local: true, handle: LOCAL_HANDLE, gain: MAX_GAIN, family: 0, variant: 0 }
    }

    fn synth(&self, _channel: u8, _part: u8, _class: u8, _index: u16) -> i32 {
        0
    }

    fn command(&mut self, kind: u8, body: &[u8], tick: u32) -> bool {
        match kind {
            TYPE_CONTROL => self.control(body),
            TYPE_LOCAL_MIDI => {
                let Ok((_, _, Command::LocalMidi { channel, status, data1, data2 })) = command::decode(kind, body)
                else {
                    return false;
                };
                self.out.push(format!("# midi {channel} {status:#04x} {data1} {data2}"));
                true
            }
            TYPE_ANIMATE => {
                let Ok((_, bytes)) = animate::read_record(body) else { return false };
                let mut any = false;
                for operation in animate::operations(bytes) {
                    any |= self.animate(operation, tick);
                }
                any
            }
            TYPE_SPRITES | TYPE_NAPLPS_OBJECTS => {
                let name = if kind == TYPE_SPRITES { "sprites" } else { "naplps_objects" };
                self.out.push(format!("# {name} {}", hexadecimal_text(body)));
                true
            }
            _ => false,
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
        let numbered = |number: u8| if number == NO_RULE { "none".to_owned() } else { number.to_string() };
        self.out.push(format!(
            "# report rule {} row {} tick {} target {kind} {} {what}",
            numbered(report.rule),
            numbered(report.row),
            report.tick,
            report.target.number
        ));
    }

    fn advance_cycles(&mut self, tick: u32) {
        self.animation.advance(tick, &mut |_| {});
    }

    fn page_state(&mut self, _state: &PageState) {}
}

impl Run {
    /// Writes a `CONTROL` command (REQ-AUD-006) as a comment line. The
    /// audio profile reads one as a whole record, so the body is framed
    /// as the server frames it and read back.
    fn control(&mut self, body: &[u8]) -> bool {
        let mut record = [0_u8; neoteletext_audio::MAX_RECORD];
        let Some(length) = neoteletext_audio::record::write(TYPE_CONTROL, body, &mut record) else { return false };
        let Ok(Record::Control(control)) = neoteletext_audio::record::parse(record.get(..length).unwrap_or(&[])) else {
            return false;
        };
        let (channel, fade_ms) = (control.channel, control.fade_ms);
        self.out.push(match control.operation {
            SoundOperation::Play => {
                let looped = if control.flags & FLAG_LOOP != 0 { " loop" } else { "" };
                format!("# play {channel} {} {} {} {fade_ms}{looped}", control.handle, control.generation, control.gain)
            }
            SoundOperation::Stop => format!("# stop {channel} {fade_ms}"),
            SoundOperation::Gain => format!("# gain {channel} {} {fade_ms}", control.gain),
        });
        true
    }

    /// Applies one operation of an `ANIMATE` command and writes it as a
    /// line of the script; false when it did nothing.
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
}

/// The bytes hexadecimal text names, two digits a byte, or `None` when it
/// is not that.
fn hexadecimal(text: &[u8]) -> Option<Vec<u8>> {
    if text.is_empty() || !text.len().is_multiple_of(2) || !text.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    text.chunks(2)
        .map(|pair| core::str::from_utf8(pair).ok().and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect()
}

/// Bytes as hexadecimal text.
fn hexadecimal_text(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        // Writing to a `String` cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The table's image: the file's own bytes, or the bytes its hexadecimal
/// text names when the file is all hexadecimal digits and white space.
fn image_bytes(file: &[u8]) -> Vec<u8> {
    let text: Vec<u8> = file.iter().copied().filter(|byte| !byte.is_ascii_whitespace()).collect();
    hexadecimal(&text).unwrap_or_else(|| file.to_vec())
}

/// The table of `file` (page-rules.md RULE-35, RULE-41): the program
/// protocol's `rules` message as JSON, which names its own page; or a
/// table's image, as bytes or as hexadecimal text, for the page `page` and
/// `sub`.
fn load_table(file: &[u8], page: u16, sub: u16) -> Result<Box<Table>, String> {
    // A JSON object begins with a brace, which no image's format byte and
    // no hexadecimal digit is.
    if file.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'{') {
        let text = core::str::from_utf8(file).map_err(|_| "a `rules` message is text in UTF-8".to_owned())?;
        let (_, _, table) = crate::rules_json::read_rules_message(text)?;
        return Ok(table);
    }
    // A decoded table is about a hundred kilobytes.
    let mut table = Box::new(Table::new(page, sub));
    neoteletext_rules::decode(&image_bytes(file), page, sub, body_check, &mut table)
        .map_err(|fault| format!("the table is refused: {fault:?}"))?;
    Ok(table)
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

/// The generation the run's table is given: its first (RULE-40).
const GENERATION: u16 = neoteletext_rules::limits::FIRST_GENERATION;

/// Evaluates `ticks` ticks of `table` against `inputs`, and gives the
/// lines to write.
fn evaluate(table: &Table, inputs: &[(u32, Given)], ticks: u32, picture: Picture) -> Result<Vec<String>, String> {
    let mut evaluator = Box::new(Evaluator::new(table, GENERATION, body_check).map_err(|fault| format!("{fault:?}"))?);
    let watch: Vec<(u8, u8)> = evaluator.index().watch.iter().map(|slot| (slot.channel, slot.kind)).collect();
    let mut metered = [0u8; MAX_WATCHED];
    let mut run = Run { picture, rectangles: Vec::new(), animation: AnimationStore::new(), out: Vec::new() };
    let mut lines = vec![format!(
        "# page {:03X}/{:04X}, {} rules, {} groups, {} sequences",
        table.page,
        table.sub,
        table.rules.len(),
        table.groups.len(),
        table.sequences.len()
    )];
    let mut pending = inputs.iter().peekable();
    let (mut sequence, mut held, mut at) = (0u32, 0u8, None::<(u16, u16)>);
    // A report or a command written as a comment is a comment of its
    // tick.
    let of_tick = |tick: u32, line: String| match line.strip_prefix("# ") {
        Some(comment) => format!("# {tick} {comment}"),
        None => line,
    };
    let mut events: Vec<Input> = Vec::new();
    for tick in 0..ticks {
        // DEC-6635: the lines of tick `t` are given after tick step `t`,
        // and before tick step `t + 1`, each acting at once.
        evaluator.tick(&mut run);
        while let Some((_, given)) = pending.next_if(|(due, _)| *due <= tick) {
            let mut push = |input: Input| events.push(input);
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
                Given::Cue { channel, number } => {
                    // A cue over a full queue is dropped and counted.
                    let _ = evaluator.handle(Event::Cue { channel, number }, &mut run);
                }
                Given::Command { kind, body, length } => {
                    // What the evaluator refuses or drops it counts.
                    let body = body.get(..length).unwrap_or(&[]);
                    let _ = evaluator.handle(Event::Command { kind, body }, &mut run);
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
            // A key's or the pointer's events act at once, in order
            // (DEC-6635 item 1); one over a full queue is dropped and
            // counted.
            for input in events.drain(..) {
                sequence = sequence.saturating_add(1);
                let _ = evaluator.handle(Event::Input(InputEvent { sequence, input }), &mut run);
            }
        }
        lines.extend(run.out.drain(..).map(|line| of_tick(tick, line)));
    }
    let counters = evaluator.counters();
    lines.push(format!(
        "# {} ticks, {} rules fired, {} groups run, {} commands applied, {} skipped, {} events dropped",
        counters.ticks, counters.fired, counters.groups, counters.commands, counters.skipped, counters.events_dropped
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
    page: &str,
    frames: Option<u32>,
    picture: Picture,
    no_sandbox: bool,
) -> Result<(), Failure> {
    // The page an image is for: a page alone is its sub-code 0000.
    let (page, sub) = match super::parse_selection(page)? {
        super::Selection::Page(address, subcode) => {
            (address.mpp(), if subcode == SubCode::ANY { 0 } else { subcode.value() })
        }
        super::Selection::All => return Err((exit::USAGE, "a table is one page's".to_owned())),
    };
    // Both files are read, within their bounds, before the sandbox closes,
    // and parsed after it (REQ-SEC-017).
    // A `rules` message's JSON is held to the script's bound, the most this
    // program reads of any text file; an image as hexadecimal text is
    // within it too.
    let table_file = read_bounded(table, MAX_SCRIPT_BYTES)?;
    let inputs_file = read_bounded(inputs, MAX_SCRIPT_BYTES)?;
    sandbox_closed(no_sandbox)?;
    let text = String::from_utf8(inputs_file).map_err(|_| io_failure(inputs, "the inputs are text in UTF-8"))?;
    let given = parse_inputs(&text).map_err(|why| io_failure(inputs, why))?;
    // Without --frames, one tick past the last input, so that its effect
    // is applied.
    let last = given.last().map_or(0, |(tick, _)| *tick);
    let ticks = frames.unwrap_or_else(|| last.saturating_add(2)).min(MAX_SEQUENCE_TICKS);
    let named = |why| (exit::INPUT, format!("{}: {why}", table.display()));
    let loaded = load_table(&table_file, page, sub).map_err(named)?;
    let lines = evaluate(&loaded, &given, ticks, picture).map_err(named)?;
    let mut out = std::io::stdout().lock();
    for line in lines {
        writeln!(out, "{line}").map_err(|error| (exit::INPUT, error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod picture_tests {
    use super::Picture;

    /// RAS-5 (DEC-6853): a NAPLPS page's world is the teletext raster, 480
    /// by 500, an object's position counted in dots.
    #[test]
    fn a_naplps_picture_is_the_raster_in_dots() {
        assert_eq!(Picture::Naplps.size(), Picture::Teletext.size());
        assert_eq!(Picture::Naplps.size(), (480, 500));
        assert_eq!(Picture::Naplps.unit(), (1, 1));
    }
}
