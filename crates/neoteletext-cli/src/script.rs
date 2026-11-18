//! The script of an image sequence (animation.md ANIM-18, REQ-6529): a text
//! file of timed operations, one a line, `<tick> <operation>`, in
//! ascending tick. The operations are those on the animation state
//! (ANIM-6), written
//!
//! ```text
//! move <n> <x> <y> [relative]
//! show <n>
//! hide <n>
//! look <n> <0-3>
//! order <n> <layer|keep> <order>
//! frame <n> <f>
//! cycle <n> <first> <last> <period> [once]
//! stop_cycle <n>
//! reset <n>
//! record <type> <hexadecimal body>
//! ```
//!
//! `<n>` is an object's number. A teletext page has controls too, which
//! page-rules.md RULE-14 numbers apart from objects, so `neottx` also takes
//! `control:<n>` where an operation acts on controls. A `record` is applied
//! at its tick; the types are `ANIMATE`, `SPRITES` and `FRAME`. An empty
//! line, and a line that begins with `#`, is skipped: `neottx rules-run`
//! writes its reports as such lines.
//!
//! The script is input from outside: every count and length is bounded
//! before it is used, and a line that is not understood is refused with
//! its number, never guessed at.

use neoteletext_core::limits::KEEP_LAYER;
use neoteletext_core::sprites::{ForcedLook, Operation, Target};
use std::fmt::Write as _;

/// The longest script read, in bytes: every operation of the longest
/// sequence, 36,000 ticks, at a hundred bytes a tick.
pub const MAX_SCRIPT_BYTES: usize = 4 * 1024 * 1024;
/// The longest body of a record, in bytes: the audio profile's record
/// frame holds no more (page-rules.md section 8).
pub const MAX_RECORD_BODY: usize = neoteletext_web::records::MAX_FRAMED_BODY;
/// The word that names a control in place of an object.
const CONTROL_PREFIX: &str = "control:";
/// The word of `order` that keeps the layer.
const KEEP: &str = "keep";

/// The records a script may apply (ANIM-18), by their type in the audio
/// profile's record frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordKind {
    /// Type 23: operations on a sub-page's objects (ANIM-6).
    Animate,
    /// Type 24: a teletext sub-page's sprites (ANIM-7).
    Sprites,
    /// Type 27: the beginning or the end of an atomic frame (ANIM-2).
    Frame,
}

impl RecordKind {
    /// The record's type number.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Self::Animate => neoteletext_web::records::TYPE_ANIMATE,
            Self::Sprites => neoteletext_web::records::TYPE_SPRITES,
            Self::Frame => neoteletext_web::records::TYPE_FRAME,
        }
    }

    /// The record's name, as a script writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Animate => "ANIMATE",
            Self::Sprites => "SPRITES",
            Self::Frame => "FRAME",
        }
    }

    fn named(word: &str) -> Option<Self> {
        [Self::Animate, Self::Sprites, Self::Frame]
            .into_iter()
            .find(|kind| word.eq_ignore_ascii_case(kind.name()) || word.parse() == Ok(kind.number()))
    }
}

/// What one line of a script does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// An operation on the animation state of a target.
    Operation {
        /// The control or object.
        target: Target,
        /// The operation.
        operation: Operation,
    },
    /// A record applied at the line's tick.
    Record {
        /// Its type.
        kind: RecordKind,
        /// Its body.
        body: Vec<u8>,
    },
}

/// One line of a script.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Step {
    /// The tick it is applied at.
    pub tick: u32,
    /// The line it was read from, counted from 1, for messages.
    pub line: usize,
    /// What it does.
    pub action: Action,
}

/// The words of one line after its tick and its operation's name.
struct Words<'a> {
    words: core::str::SplitAsciiWhitespace<'a>,
}

impl Words<'_> {
    fn word(&mut self, what: &str) -> Result<&str, String> {
        self.words.next().ok_or_else(|| format!("{what} is missing"))
    }

    fn number<T: core::str::FromStr>(&mut self, what: &str) -> Result<T, String> {
        let word = self.word(what)?;
        word.parse().map_err(|_| format!("{what} is not a number in its range: {word:?}"))
    }

    fn target(&mut self) -> Result<Target, String> {
        let word = self.word("the target")?;
        let (number, control) = match word.strip_prefix(CONTROL_PREFIX) {
            Some(number) => (number, true),
            None => (word, false),
        };
        match number.parse::<u16>() {
            Ok(number) if number != 0 => Ok(if control { Target::Control(number) } else { Target::Object(number) }),
            _ => Err(format!("the target is not a number from 1 to 65,535: {word:?}")),
        }
    }

    /// An optional last word that must be `flag` when present.
    fn flag(&mut self, flag: &str) -> Result<bool, String> {
        match self.words.next() {
            None => Ok(false),
            Some(word) if word == flag => Ok(true),
            Some(word) => Err(format!("{word:?} where only {flag:?} may stand")),
        }
    }

    fn end(&mut self) -> Result<(), String> {
        self.words.next().map_or(Ok(()), |word| Err(format!("{word:?} follows the operation's last word")))
    }
}

fn hexadecimal(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || text.len() > MAX_RECORD_BODY.saturating_mul(2) {
        return Err(format!("a record's body is whole bytes in hexadecimal, at most {MAX_RECORD_BODY}"));
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| core::str::from_utf8(pair).ok().and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(|| "a record's body is not hexadecimal".to_owned())
}

fn action(name: &str, words: &mut Words<'_>) -> Result<Action, String> {
    if name == "record" {
        let word = words.word("the record's type")?;
        let kind = RecordKind::named(word)
            .ok_or_else(|| format!("{word:?} is not a record a script applies (ANIMATE, SPRITES or FRAME)"))?;
        let body = hexadecimal(words.word("the record's body")?)?;
        words.end()?;
        return Ok(Action::Record { kind, body });
    }
    let target = words.target()?;
    let operation = match name {
        "show" => Operation::Show,
        "hide" => Operation::Hide,
        "look" => {
            let look = ForcedLook::from_code(words.number("the look")?).ok_or("the look is 0 to 3")?;
            Operation::Look(look)
        }
        "move" => {
            let (x, y) = (words.number("x")?, words.number("y")?);
            return Ok(Action::Operation {
                target,
                operation: Operation::Move { relative: words.flag("relative")?, x, y },
            });
        }
        "order" => {
            let layer = match words.word("the layer")? {
                KEEP => KEEP_LAYER,
                word => word.parse().map_err(|_| format!("the layer is 0 to 3 or {KEEP:?}: {word:?}"))?,
            };
            Operation::order(layer, words.number("the order")?).ok_or(format!("the layer is 0 to 3 or {KEEP:?}"))?
        }
        "frame" => Operation::Frame(words.number("the frame")?),
        "cycle" => {
            let (first, last, period) = (words.number("first")?, words.number("last")?, words.number("the period")?);
            let operation = Operation::Cycle { first, last, period, once: words.flag("once")? };
            return Ok(Action::Operation { target, operation });
        }
        "stop_cycle" => Operation::StopCycle,
        "reset" => Operation::Reset,
        _ => return Err(format!("{name:?} is not an operation")),
    };
    words.end()?;
    Ok(Action::Operation { target, operation })
}

/// Reads a script.
///
/// # Errors
/// A message naming the first line that is not `<tick> <operation>` as the
/// module describes, or whose tick is below the line before it.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    let mut last_tick = 0;
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let refused = |why: String| format!("line {number}: {why}");
        let mut words = Words { words: line.split_ascii_whitespace() };
        let tick: u32 = words.number("the tick").map_err(refused)?;
        if tick < last_tick {
            return Err(refused(format!("tick {tick} comes after tick {last_tick}: a script is in ascending tick")));
        }
        last_tick = tick;
        let name = words.word("the operation").map_err(refused)?.to_owned();
        let action = action(&name, &mut words).map_err(refused)?;
        steps.push(Step { tick, line: number, action });
    }
    Ok(steps)
}

/// One operation as a line of a script, without its line end: what
/// [`parse`] reads back as the same operation.
#[must_use]
pub fn write_operation(tick: u32, target: Target, operation: Operation) -> String {
    let target = match target {
        Target::Control(number) => format!("{CONTROL_PREFIX}{number}"),
        Target::Object(number) => number.to_string(),
    };
    let mut line = format!("{tick} ");
    // Writing to a string does not fail.
    let _ = match operation {
        Operation::Show => write!(line, "show {target}"),
        Operation::Hide => write!(line, "hide {target}"),
        Operation::Look(look) => write!(line, "look {target} {}", look.code()),
        Operation::Move { relative, x, y } => {
            write!(line, "move {target} {x} {y}{}", if relative { " relative" } else { "" })
        }
        Operation::Order { layer, order } => match layer {
            Some(layer) => write!(line, "order {target} {layer} {order}"),
            None => write!(line, "order {target} {KEEP} {order}"),
        },
        Operation::Frame(frame) => write!(line, "frame {target} {frame}"),
        Operation::Cycle { first, last, period, once } => {
            write!(line, "cycle {target} {first} {last} {period}{}", if once { " once" } else { "" })
        }
        Operation::StopCycle => write!(line, "stop_cycle {target}"),
        Operation::Reset => write!(line, "reset {target}"),
    };
    line
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::unreachable,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and `unreachable!` on outcomes it rules out"
)]
mod tests {
    use super::*;

    fn object(tick: u32, line: usize, number: u16, operation: Operation) -> Step {
        Step { tick, line, action: Action::Operation { target: Target::Object(number), operation } }
    }

    /// ANIM-18 (REQ-6529): each operation of a script, as the document
    /// writes it, and written back as the same line.
    #[test]
    fn each_operation_is_read_and_written_back() {
        let text = "\
# a comment, and an empty line

0 move 10 120 -80
0 move 10 8 0 relative
1 show 10
1 hide 20
2 look 10 3
3 order 10 2 -5
3 order 10 keep 127
4 frame 10 15
5 cycle 10 0 3 6
5 cycle 10 1 2 3600 once
6 stop_cycle 10
7 reset 65535
7 look control:3 2
";
        let steps = parse(text).unwrap();
        let expected = [
            object(0, 3, 10, Operation::Move { relative: false, x: 120, y: -80 }),
            object(0, 4, 10, Operation::Move { relative: true, x: 8, y: 0 }),
            object(1, 5, 10, Operation::Show),
            object(1, 6, 20, Operation::Hide),
            object(2, 7, 10, Operation::Look(ForcedLook::Alternative)),
            object(3, 8, 10, Operation::Order { layer: Some(2), order: -5 }),
            object(3, 9, 10, Operation::Order { layer: None, order: 127 }),
            object(4, 10, 10, Operation::Frame(15)),
            object(5, 11, 10, Operation::Cycle { first: 0, last: 3, period: 6, once: false }),
            object(5, 12, 10, Operation::Cycle { first: 1, last: 2, period: 3_600, once: true }),
            object(6, 13, 10, Operation::StopCycle),
            object(7, 14, 65_535, Operation::Reset),
            Step {
                tick: 7,
                line: 15,
                action: Action::Operation {
                    target: Target::Control(3),
                    operation: Operation::Look(ForcedLook::Pressed),
                },
            },
        ];
        assert_eq!(steps, expected);
        let written: Vec<String> = steps
            .iter()
            .map(|step| match &step.action {
                Action::Operation { target, operation } => write_operation(step.tick, *target, *operation),
                Action::Record { .. } => String::new(),
            })
            .collect();
        let lines: Vec<&str> = text.lines().skip(2).collect();
        assert_eq!(written, lines);
    }

    /// ANIM-18: a record's line, by name or by type number.
    #[test]
    fn a_record_is_read_with_its_body() {
        let steps = parse("9 record FRAME 0100000101000007\n9 record 24 01000001\n9 record animate 00\n").unwrap();
        let bodies: Vec<(RecordKind, &[u8])> = steps
            .iter()
            .map(|step| match &step.action {
                Action::Record { kind, body } => (*kind, body.as_slice()),
                Action::Operation { .. } => unreachable!("every line is a record"),
            })
            .collect();
        let frame: &[u8] = &[1, 0, 0, 1, 1, 0, 0, 7];
        assert_eq!(
            bodies,
            [(RecordKind::Frame, frame), (RecordKind::Sprites, &[1, 0, 0, 1]), (RecordKind::Animate, &[0])]
        );
    }

    /// A line that is not understood is refused with its number.
    #[test]
    fn a_line_that_is_not_understood_is_refused() {
        for (text, part) in [
            ("x show 1", "line 1: the tick is not a number"),
            ("5", "line 1: the operation is missing"),
            ("5 jump 1", "is not an operation"),
            ("5 show", "the target is missing"),
            ("5 show 0", "the target is not a number from 1"),
            ("5 show 65536", "the target is not a number from 1"),
            ("5 show 1 2", "follows the operation's last word"),
            ("5 look 1 4", "the look is 0 to 3"),
            ("5 move 1 2", "y is missing"),
            ("5 move 1 2 40000", "y is not a number in its range"),
            ("5 move 1 2 3 absolute", "where only \"relative\" may stand"),
            ("5 order 1 4 0", "the layer is 0 to 3"),
            ("5 order 1 keep 128", "the order is not a number in its range"),
            ("5 frame 1 256", "the frame is not a number in its range"),
            ("5 cycle 1 0 3", "the period is missing"),
            ("5 cycle 1 0 3 6 twice", "where only \"once\" may stand"),
            ("5 record RULES 00", "is not a record a script applies"),
            ("5 record FRAME 0", "whole bytes in hexadecimal"),
            ("5 record FRAME 0g", "is not hexadecimal"),
            ("5 record FRAME", "the record's body is missing"),
            ("5 show 1\n4 show 1", "line 2: tick 4 comes after tick 5"),
        ] {
            let refusal = parse(text).unwrap_err();
            assert!(refusal.contains(part), "{text:?}: {refusal}");
        }
        let long = format!("0 record FRAME {}", "00".repeat(MAX_RECORD_BODY + 1));
        assert!(parse(&long).unwrap_err().contains("at most 8184"));
        assert_eq!(parse(&format!("0 record FRAME {}", "00".repeat(MAX_RECORD_BODY))).unwrap().len(), 1);
    }
}
