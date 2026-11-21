//! A world and a script for the rule evaluator's fuzz targets, both made
//! from the fuzzer's bytes (RULE-83): the same bytes give the same
//! answers, so two runs can be compared.

use neoteletext_rules::command::{self, Command, Operand, StartMode, TYPE_CONTROL, TYPE_LOCAL_MIDI, VariableOperation};
use neoteletext_rules::evaluator::{
    Bounds, Counters, Event, Input, InputEvent, KeyAction, KeyIdentity, PointerAction, Report, SoundState, TargetState,
    World,
};
use neoteletext_rules::limits::{MAX_COMMAND_BYTES, MAX_POINTER_OBJECTS, VARIABLES};
use neoteletext_rules::state::PageState;
use neoteletext_rules::table::{Table, Target, Value};
use neoteletext_rules::{Evaluator, decode};
use neoteletext_web::records::body_check;
use neoteletext_web::records::naplps_objects::{RASTER_HEIGHT, RASTER_WIDTH};

/// A ReGIS picture's width and height in pixels (RULE-13; the ReGIS
/// renderer's screen).
const REGIS_PICTURE: (u16, u16) = (800, 480);

/// The page every fuzzed table is for.
pub const PAGE: u16 = 0x101;
/// Its sub-code.
pub const SUB: u16 = 0x0001;
/// The generation the evaluator is given.
pub const GENERATION: u16 = 1;
/// The most ticks one input runs, so that a run stays short.
pub const MAX_TICKS: usize = 200;

/// What a world was given, in order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Given {
    /// A command: its record type, its body and the page tick.
    Command(u8, Vec<u8>, u32),
    Report(Report),
    Cycles(u32),
    State(Box<PageState>),
}

/// A world whose every answer is a function of the fuzzer's bytes and the
/// question.
pub struct FuzzWorld<'a> {
    seed: &'a [u8],
    pub given: Vec<Given>,
    /// The commands given so far of each of the two sorts.
    sounds: usize,
    others: usize,
}

impl<'a> FuzzWorld<'a> {
    pub fn new(seed: &'a [u8]) -> Self {
        Self { seed, given: Vec::new(), sounds: 0, others: 0 }
    }

    /// A byte of the seed for a question.
    fn byte(&self, question: usize) -> u8 {
        if self.seed.is_empty() { 0 } else { self.seed[question % self.seed.len()] }
    }
}

impl World for FuzzWorld<'_> {
    /// RULE-13: a ReGIS picture of 800 by 480 pixels, or the teletext
    /// raster of 480 by 500 dots that a teletext page and a NAPLPS picture
    /// share (RAS-5, DEC-6853), chosen by the seed's first byte.
    fn picture(&self) -> (u16, u16) {
        if self.byte(0) & 1 == 0 { REGIS_PICTURE } else { (RASTER_WIDTH, RASTER_HEIGHT) }
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

    // An answer is a function of how many commands of its own sort came
    // before, so that two runs of one script are answered alike.
    fn command(&mut self, kind: u8, body: &[u8], tick: u32) -> bool {
        assert!(body.len() <= MAX_COMMAND_BYTES && !body.is_empty());
        assert!(command::is_allowed(kind) && !(command::TYPE_VARIABLE..=command::TYPE_SEQUENCE).contains(&kind));
        self.given.push(Given::Command(kind, body.to_vec(), tick));
        if matches!(kind, TYPE_CONTROL | TYPE_LOCAL_MIDI) {
            self.sounds += 1;
            self.byte(self.sounds) & 1 != 0
        } else {
            self.others += 1;
            self.byte(self.others) & 2 != 0
        }
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
#[derive(Clone, Copy)]
pub enum Step {
    /// An input event to give the evaluator.
    Event(InputEvent),
    /// A metered report.
    Metered([u8; 4]),
    /// A cue heard: its audio channel and its number.
    Cue(u8, u8),
    /// A painting finished (DEC-7031 item 4): what was painted, which may
    /// be a target no `painted` trigger can name.
    Painted(Target),
    /// A command the server sent directly.
    Command(Command),
    /// A state query (RULE-47).
    Query(u32),
    /// A tick to evaluate.
    Tick,
}

/// A command the server sends directly, from six of the fuzzer's bytes.
fn server_command(field: &dyn Fn(usize) -> u8) -> Command {
    let variable = field(1) % 16;
    let constant = i32::from_be_bytes([field(2), field(3), field(4), field(5)]);
    match field(0) % 12 {
        0 => Command::Variable { operation: VariableOperation::Set, variable, operand: Operand::Constant(constant) },
        1 => Command::Variable { operation: VariableOperation::Add, variable, operand: Operand::Constant(constant) },
        2 => Command::Variable {
            operation: VariableOperation::Sub,
            variable,
            operand: Operand::Variable { constant, variable: field(2) % 16 },
        },
        3 => Command::Variable { operation: VariableOperation::Inc, variable, operand: Operand::None },
        4 => Command::Variable { operation: VariableOperation::Clear, variable: 255, operand: Operand::None },
        5 => Command::Timer { start: field(1) & 1 == 0, timer: field(2) % 8 },
        6 => Command::Report { value: Some(Value::variable(variable)) },
        7 => Command::Report { value: None },
        8 => Command::Run { group: field(1) % 64 },
        9 => Command::Sequence { start: true, sequence: field(1) % 8, mode: StartMode::At(u32::from(field(2))) },
        10 => Command::Sequence {
            start: true,
            sequence: field(1) % 8,
            mode: StartMode::Beat { timer: if field(2) & 8 == 0 { field(2) % 8 } else { 255 }, ticks: 4 },
        },
        _ if field(1) & 1 == 0 => Command::Sequence { start: true, sequence: field(2) % 8, mode: StartMode::Next },
        _ => Command::Sequence { start: false, sequence: field(2) % 8, mode: StartMode::Next },
    }
}

/// Reads a script from the fuzzer's bytes: each step a byte that says
/// which, and the bytes it needs. Events are numbered in order from 1.
/// Step 14 is a painting finished (DEC-7031 item 4); 15 to 17 are ticks.
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
        let input = match kind % 18 {
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
                steps.push(Step::Cue(field(0) % 4, field(1) % 128));
                None
            }
            13 => {
                steps.push(Step::Command(server_command(&field)));
                None
            }
            14 => {
                // The page, a drawing of the few the worlds' pointers meet
                // (0 among them, which names none), any drawing, or a
                // control, which no painting is of.
                let painted = match field(0) % 4 {
                    0 => Target::PAGE,
                    1 => Target::object(u16::from(field(1) % 8)),
                    2 => Target::object(word(1)),
                    _ => Target::control(u16::from(field(1) % 8)),
                };
                steps.push(Step::Painted(painted));
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

/// Splits the fuzzer's bytes into a table's image and the rest: two bytes
/// of length, big-endian, then the image.
pub fn split(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let (length, rest) = data.split_first_chunk::<2>()?;
    rest.split_at_checked(usize::from(u16::from_be_bytes(*length)))
}

/// The table the fuzzer's image decodes to for the targets' page, if it
/// decodes.
pub fn table_of(image: &[u8]) -> Option<Box<Table>> {
    let mut table = Box::new(Table::new(PAGE, SUB));
    decode(image, PAGE, SUB, body_check, &mut table).ok()?;
    Some(table)
}

/// An evaluator for a decoded table.
pub fn evaluator_of(table: &Table) -> Box<Evaluator> {
    Box::new(Evaluator::new(table, GENERATION, body_check).expect("a decoded table is evaluated"))
}

/// What one tick and the interval before it gave the world, in order,
/// and the counters and the variables after.
pub type Interval = (Vec<Given>, Counters, [i32; VARIABLES]);

/// Gives the evaluator one step that is not a tick: each event is
/// handled at once or waits (DEC-6635 items 1 and 4); a metered report is
/// a change to the world between handlings.
pub fn give(evaluator: &mut Evaluator, world: &mut FuzzWorld<'_>, step: &Step) {
    match step {
        Step::Event(event) => {
            evaluator.handle(Event::Input(*event), world);
        }
        Step::Metered(values) => evaluator.set_metered(values),
        Step::Cue(channel, number) => {
            evaluator.handle(Event::Cue { channel: *channel, number: *number }, world);
        }
        Step::Painted(target) => {
            evaluator.handle(Event::Painted { target: *target }, world);
        }
        Step::Command(command) => {
            let mut body = [0_u8; command::MAX_BODY_BYTES];
            if let Ok(length) = command::encode(command, PAGE, SUB, &mut body) {
                evaluator.handle(Event::Command { kind: command.kind(), body: &body[..length] }, world);
            }
        }
        Step::Query(identifier) => {
            evaluator.handle(Event::Query(*identifier), world);
        }
        Step::Tick => {}
    }
}

/// Runs the script once and gives what the world was given, interval by
/// interval, each ending with its tick. `each` is given the evaluator and
/// what the interval and the tick just run gave the world. `record` is
/// given each step that is not a tick with how many tick steps had run
/// when it was given (DEC-6635 item 5).
pub fn run(
    table: &Table,
    seed: &[u8],
    steps: &[Step],
    each: &mut dyn FnMut(&Evaluator, &[Given]),
    record: &mut dyn FnMut(u32, Step),
) -> Vec<Interval> {
    let mut evaluator = evaluator_of(table);
    let mut world = FuzzWorld::new(seed);
    let (mut ticks, mut before) = (0_usize, 0_usize);
    let mut intervals = Vec::new();
    for step in steps {
        if !matches!(step, Step::Tick) {
            record(evaluator.counters().ticks, *step);
            give(&mut evaluator, &mut world, step);
            continue;
        }
        if ticks >= MAX_TICKS {
            continue;
        }
        ticks += 1;
        if ticks.is_multiple_of(5) {
            let query = Step::Query(u32::try_from(ticks).unwrap_or(0));
            record(evaluator.counters().ticks, query);
            give(&mut evaluator, &mut world, &query);
        }
        evaluator.tick(&mut world);
        each(&evaluator, &world.given[before..]);
        intervals.push((world.given[before..].to_vec(), *evaluator.counters(), *evaluator.variables()));
        before = world.given.len();
    }
    evaluator.close(&mut world);
    evaluator.tick(&mut world);
    each(&evaluator, &world.given[before..]);
    intervals.push((world.given[before..].to_vec(), *evaluator.counters(), *evaluator.variables()));
    intervals
}
