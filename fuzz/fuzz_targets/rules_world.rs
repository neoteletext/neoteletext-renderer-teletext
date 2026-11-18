//! A world and an event script for the rule evaluator's fuzz targets, both
//! made from the fuzzer's bytes (RULE-83): the same bytes give the same
//! answers, so two runs can be compared.

use neoteletext_rules::animate::Operation;
use neoteletext_rules::evaluator::{
    Bounds, Input, InputEvent, KeyAction, KeyIdentity, PointerAction, Report, SoundOperation, SoundState, TargetState,
    World,
};
use neoteletext_rules::limits::MAX_POINTER_OBJECTS;
use neoteletext_rules::state::PageState;
use neoteletext_rules::table::Target;

/// What a world was given, in order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Given {
    Sound(SoundOperation),
    Animate(Operation, u32),
    Report(Report),
    Cycles(u32),
    State(Box<PageState>),
}

/// A world whose every answer is a function of the fuzzer's bytes and the
/// question.
pub struct FuzzWorld<'a> {
    seed: &'a [u8],
    pub given: Vec<Given>,
}

impl<'a> FuzzWorld<'a> {
    pub fn new(seed: &'a [u8]) -> Self {
        Self { seed, given: Vec::new() }
    }

    /// A byte of the seed for a question.
    fn byte(&self, question: usize) -> u8 {
        if self.seed.is_empty() { 0 } else { self.seed[question % self.seed.len()] }
    }
}

impl World for FuzzWorld<'_> {
    fn picture(&self) -> (u16, u16) {
        (800, 480)
    }

    fn target(&self, target: Target) -> Option<TargetState> {
        let at = usize::from(target.number) * 7 + usize::from(target.kind.code());
        let flags = self.byte(at);
        if flags & 1 == 0 {
            return None;
        }
        let left = i32::from(self.byte(at + 1)) * 4 - 100;
        let top = i32::from(self.byte(at + 2)) * 2 - 20;
        let bounds = Bounds { left, top, right: left + i32::from(self.byte(at + 3)), bottom: top + i32::from(flags) };
        Some(TargetState {
            shown: flags & 2 != 0,
            cycling: flags & 4 != 0,
            frame: self.byte(at + 4),
            look: self.byte(at + 5),
            layer: self.byte(at + 6),
            order: self.byte(at + 7).cast_signed(),
            x: i16::from(self.byte(at + 8)) - 100,
            y: i16::from(self.byte(at + 9)) * 100,
            bounds: (flags & 2 != 0).then_some(bounds),
        })
    }

    fn objects_at(&self, x: u16, y: u16, objects: &mut [u16; MAX_POINTER_OBJECTS]) -> usize {
        let at = usize::from(x) + usize::from(y) * 3;
        // More than the array holds may be claimed: the evaluator must not
        // believe it.
        let count = usize::from(self.byte(at) % 20);
        for (place, object) in objects.iter_mut().enumerate() {
            *object = u16::from(self.byte(at + place + 1) % 8);
        }
        count
    }

    fn sound(&self, channel: u8) -> SoundState {
        let at = 1000 + usize::from(channel) * 5;
        let flags = self.byte(at);
        SoundState {
            playing: flags & 1 != 0,
            local: flags & 2 != 0,
            handle: self.byte(at + 1),
            gain: self.byte(at + 2),
            family: 12 + self.byte(at + 3) % 8,
            variant: self.byte(at + 4) % 3,
        }
    }

    fn synth(&self, channel: u8, part: u8, class: u8, index: u16) -> i32 {
        let at = 2000 + usize::from(channel) + usize::from(part) * 3 + usize::from(class) * 5 + usize::from(index);
        i32::from_be_bytes([self.byte(at), self.byte(at + 1), self.byte(at + 2), self.byte(at + 3)])
    }

    fn sound_operation(&mut self, operation: SoundOperation) -> bool {
        self.given.push(Given::Sound(operation));
        self.byte(self.given.len()) & 1 != 0
    }

    fn animate(&mut self, operation: Operation, tick: u32) -> bool {
        self.given.push(Given::Animate(operation, tick));
        self.byte(self.given.len()) & 2 != 0
    }

    fn report(&mut self, report: Report) {
        self.given.push(Given::Report(report));
    }

    fn advance_cycles(&mut self, tick: u32) {
        self.given.push(Given::Cycles(tick));
    }

    fn page_state(&mut self, state: &PageState) {
        self.given.push(Given::State(Box::new(*state)));
    }
}

/// One step of a script.
pub enum Step {
    /// An input event to give the evaluator.
    Event(InputEvent),
    /// A metered report.
    Metered([u8; 4]),
    /// The sub-page shown changes.
    Subpage(u16),
    /// A tick to evaluate.
    Tick,
}

/// Reads a script from the fuzzer's bytes: each step a byte that says
/// which, and the bytes it needs. Events are numbered in order from 1.
pub fn script(bytes: &[u8]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut rest = bytes;
    let mut sequence = 0_u32;
    while let Some((&kind, after)) = rest.split_first() {
        let (body, after) = after.split_at(after.len().min(6));
        let field = |at: usize| body.get(at).copied().unwrap_or(0);
        let word = |at: usize| u16::from_be_bytes([field(at), field(at + 1)]);
        let code = u32::from(field(1) % 4) + u32::from(b'a');
        let key =
            if field(0) & 1 == 0 { KeyIdentity::Character(code) } else { KeyIdentity::Named(u32::from(field(1))) };
        let base = (field(0) & 2 != 0).then_some(u32::from(field(2) % 4) + u32::from(b'a'));
        let pointer = |action| Input::Pointer {
            action,
            button: field(0) % 5,
            x: word(1) % 900,
            y: word(3) % 500,
            control: u16::from(field(5) % 40),
        };
        let input = match kind % 16 {
            0 => Some(Input::Key { action: KeyAction::Press, key, base, mods: field(3) }),
            1 => Some(Input::Key { action: KeyAction::Repeat, key, base, mods: field(3) }),
            2 => Some(Input::Key { action: KeyAction::Release, key, base, mods: field(3) }),
            3 => Some(Input::Text),
            4 => Some(pointer(PointerAction::Press)),
            5 => Some(pointer(PointerAction::Release)),
            6 => Some(pointer(PointerAction::Move)),
            7 => Some(pointer(PointerAction::Gone)),
            8 => Some(Input::Wheel { x: word(1) % 900, y: word(3) % 500, control: u16::from(field(5) % 40) }),
            9 => Some(Input::Focus),
            10 => Some(Input::Blur),
            11 => {
                steps.push(Step::Metered([field(0), field(1), field(2), field(3)]));
                None
            }
            12 => {
                steps.push(Step::Subpage(word(0) % 4));
                None
            }
            _ => {
                steps.push(Step::Tick);
                None
            }
        };
        if let Some(input) = input {
            sequence += 1;
            steps.push(Step::Event(InputEvent { sequence, input }));
        }
        rest = after;
    }
    steps
}

/// Splits the fuzzer's bytes into a table's body and the rest: two bytes
/// of length, big-endian, then the body.
pub fn split(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let (length, rest) = data.split_first_chunk::<2>()?;
    rest.split_at_checked(usize::from(u16::from_be_bytes(*length)))
}
