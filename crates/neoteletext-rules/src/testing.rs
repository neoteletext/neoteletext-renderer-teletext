//! What the crate's tests share: a page, a body check standing for
//! `neoteletext-web`'s, builders of command bodies, and a scripted world.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and plain arithmetic on small numbers"
)]

use std::vec::Vec;

use crate::animate::{self, Operation};
use crate::command::{self, Binding, BodyFacts, CONTROL_BYTES, Command, TYPE_ANIMATE, TYPE_CONTROL};
use crate::evaluator::{Report, SoundState, TargetState, World};
use crate::limits::MAX_POINTER_OBJECTS;
use crate::state::PageState;
use crate::table::{Framed, Table, Target};

/// The page the tests' tables are for.
pub(crate) const PAGE: u16 = 0x101;
/// Its sub-code.
pub(crate) const SUB: u16 = 0x0001;

/// The `CONTROL` flag "after the current sound" (REQ-AUD-006).
pub(crate) const AFTER_CURRENT: u8 = 4;
/// The `CONTROL` page field of the whole service.
const SERVICE: u16 = 0xFFFF;

/// A body check that stands for `neoteletext-web`'s (RULE-56): a
/// `CONTROL` by its length and its fields' places, a `SPRITES` or a
/// `NAPLPS_OBJECTS` by its page and sub-code and a count that is not 0.
pub(crate) fn body_check(kind: u8, body: &[u8]) -> Option<BodyFacts> {
    if kind == TYPE_CONTROL {
        if body.len() != CONTROL_BYTES || body[0] > 3 || !(1..=3).contains(&body[3]) || body[7] > 100 {
            return None;
        }
        let page = u16::from_be_bytes([body[12], body[13]]);
        return Some(BodyFacts {
            page: (page != SERVICE).then_some(page),
            sub: None,
            sequence: u16::from_be_bytes([body[1], body[2]]),
            after_current: body[10] & AFTER_CURRENT != 0,
        });
    }
    if body.len() < 5 || body[4] == 0 {
        return None;
    }
    Some(BodyFacts {
        page: Some(u16::from_be_bytes([body[0], body[1]])),
        sub: Some(u16::from_be_bytes([body[2], body[3]])),
        sequence: 0,
        after_current: false,
    })
}

/// A command with its record type, its body and its bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Built {
    pub(crate) kind: u8,
    pub(crate) body: Vec<u8>,
    pub(crate) bindings: Vec<Binding>,
}

impl Built {
    /// The command of types 28 to 33 the body holds.
    pub(crate) fn command(&self) -> Command {
        command::decode(self.kind, &self.body).unwrap().2
    }

    /// The same command with one binding more.
    pub(crate) fn bound(mut self, binding: Binding) -> Self {
        self.bindings.push(binding);
        self
    }
}

/// A command of types 28 to 33 for the tests' page.
pub(crate) fn built(command: Command) -> Built {
    let mut body = [0_u8; command::MAX_BODY_BYTES];
    let length = command::encode(&command, PAGE, SUB, &mut body).unwrap();
    Built { kind: command.kind(), body: body[..length].to_vec(), bindings: Vec::new() }
}

/// A `CONTROL` for the tests' page: operation 1 play, 2 stop, 3 gain.
pub(crate) fn control(channel: u8, operation: u8, handle: u8, gain: u8, fade_ms: u16) -> Built {
    let mut body = Vec::from([channel, 0, 0, operation, handle, 0, 1, gain]);
    body.extend(fade_ms.to_be_bytes());
    body.extend([0, 0]);
    body.extend(PAGE.to_be_bytes());
    Built { kind: TYPE_CONTROL, body, bindings: Vec::new() }
}

/// An `ANIMATE` of `operations` for the tests' page.
pub(crate) fn animated(operations: &[Operation]) -> Built {
    let mut body = [0_u8; animate::MAX_RECORD_BYTES];
    let length = animate::write_record(PAGE, SUB, operations, &mut body).unwrap();
    Built { kind: TYPE_ANIMATE, body: body[..length].to_vec(), bindings: Vec::new() }
}

/// Adds a group of `commands` to `table`, and gives its number.
pub(crate) fn group(table: &mut Table, commands: &[Built]) -> u8 {
    let framed: Vec<Framed<'_>> = commands
        .iter()
        .map(|command| Framed { kind: command.kind, body: &command.body, bindings: &command.bindings })
        .collect();
    table.push_group(&framed).unwrap()
}

/// What a scripted world was given.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "a test's log holds a state answer whole, as it was given, beside the small things: it is compared, never stored in number"
)]
pub(crate) enum Out {
    /// A command: its record type, its body and the tick.
    Command(u8, Vec<u8>, u32),
    /// A report.
    Report(Report),
    /// The cycles were advanced at a tick.
    Cycles(u32),
    /// A state answer.
    State(PageState),
}

/// A world a test scripts: what it answers, and a log of what it is
/// given.
#[derive(Clone, Debug, Default)]
pub(crate) struct Script {
    pub(crate) targets: Vec<(Target, TargetState)>,
    pub(crate) sounds: [SoundState; 4],
    /// The objects under every pixel.
    pub(crate) under: Vec<u16>,
    /// What `synth` answers.
    pub(crate) synth: i32,
    /// The record types whose commands do nothing.
    pub(crate) refuses: Vec<u8>,
    pub(crate) log: Vec<Out>,
}

impl Script {
    /// The reports logged, in order.
    pub(crate) fn reports(&self) -> Vec<Report> {
        self.log
            .iter()
            .filter_map(|out| match out {
                Out::Report(report) => Some(*report),
                _ => None,
            })
            .collect()
    }

    /// The commands logged, in order: each one's type and body.
    pub(crate) fn commands(&self) -> Vec<(u8, Vec<u8>)> {
        self.log
            .iter()
            .filter_map(|out| match out {
                Out::Command(kind, body, _) => Some((*kind, body.clone())),
                _ => None,
            })
            .collect()
    }
}

impl World for Script {
    fn picture(&self) -> (u16, u16) {
        (480, 500)
    }

    fn target(&self, target: Target) -> Option<TargetState> {
        self.targets.iter().find(|(held, _)| *held == target).map(|(_, state)| *state)
    }

    fn objects_at(&self, _x: u16, _y: u16, objects: &mut [u16; MAX_POINTER_OBJECTS]) -> usize {
        for (slot, object) in objects.iter_mut().zip(&self.under) {
            *slot = *object;
        }
        self.under.len().min(MAX_POINTER_OBJECTS)
    }

    fn sound(&self, channel: u8) -> SoundState {
        self.sounds[usize::from(channel)]
    }

    fn synth(&self, _channel: u8, _part: u8, _class: u8, _index: u16) -> i32 {
        self.synth
    }

    fn command(&mut self, kind: u8, body: &[u8], tick: u32) -> bool {
        self.log.push(Out::Command(kind, body.to_vec(), tick));
        !self.refuses.contains(&kind)
    }

    fn report(&mut self, report: Report) {
        self.log.push(Out::Report(report));
    }

    fn advance_cycles(&mut self, tick: u32) {
        self.log.push(Out::Cycles(tick));
    }

    fn page_state(&mut self, state: &PageState) {
        self.log.push(Out::State(*state));
    }
}
