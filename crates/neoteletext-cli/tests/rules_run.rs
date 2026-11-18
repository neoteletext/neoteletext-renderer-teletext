//! `neottx rules-run` (animation.md ANIM-18, REQ-6529): a page rule table
//! evaluated against timed inputs, the animation operations its groups'
//! commands make written as the script of an image sequence, and its
//! reports and its other commands as comment lines.
#![allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]

mod common;

use std::fmt::Write as _;

use common::{folder, neottx};
use neoteletext_rules::animate::{self, Operation, OperationKind};
use neoteletext_rules::command::{self, Command, Operand, TYPE_ANIMATE, VariableOperation};
use neoteletext_rules::table::{Framed, Key, KeyKind, KeyMatch, Node, Rule, Table, Target, Trigger, TriggerTarget};
use neoteletext_web::records::body_check;

/// The page the tables are for: 150, sub-code 0002.
const PAGE: u16 = 0x150;
const SUB: u16 = 0x0002;
/// How `rules-run` is told that page.
const PAGE_ARGUMENT: [&str; 2] = ["--page", "150/0002"];

/// One command of a group: its record type and its body.
type Built = (u8, Vec<u8>);

fn animated(target: Target, kind: OperationKind) -> Built {
    let mut body = [0_u8; animate::MAX_RECORD_BYTES];
    let length = animate::write_record(PAGE, SUB, &[Operation { target, kind }], &mut body).unwrap();
    (TYPE_ANIMATE, body.get(..length).unwrap().to_vec())
}

fn commanded(command: Command) -> Built {
    let mut body = [0_u8; command::MAX_BODY_BYTES];
    let length = command::encode(&command, PAGE, SUB, &mut body).unwrap();
    (command.kind(), body.get(..length).unwrap().to_vec())
}

fn group(table: &mut Table, commands: &[Built]) -> u8 {
    let framed: Vec<Framed<'_>> =
        commands.iter().map(|(kind, body)| Framed { kind: *kind, body, bindings: &[] }).collect();
    table.push_group(&framed).unwrap()
}

fn image(table: &Table) -> Vec<u8> {
    let mut out = vec![0u8; neoteletext_rules::limits::MAX_TABLE_BYTES];
    let used = neoteletext_rules::encode(table, body_check, &mut out).unwrap();
    out.truncate(used);
    out
}

fn on_key_a() -> Trigger {
    Trigger::Key { keys: KeyMatch::of(&[Key::of(KeyKind::Character, u32::from('a'))]), repeat: false }
}

/// A table of four rules, each with its group: at opening, object 10
/// cycles its frames and control 1 is pressed; the key `a` moves object 10
/// one unit right, counts it and reports it; a press on object 20 shows
/// its frame 2; and when object 10 comes to overlap object 20, object 20
/// is hidden and a note sounds.
fn table() -> Vec<u8> {
    let object = Target::object;
    let mut table = Box::new(Table::new(PAGE, SUB));
    let cycle = OperationKind::Cycle { first: 0, last: 3, period: 6, once: false };
    let opening =
        group(&mut table, &[animated(object(10), cycle), animated(Target::control(1), OperationKind::Look(2))]);
    table.rules.push(Rule::new(Trigger::Open, opening)).unwrap();

    let count = Command::Variable { operation: VariableOperation::Inc, variable: 0, operand: Operand::None };
    let moving = group(
        &mut table,
        &[
            animated(object(10), OperationKind::Move { relative: true, x: 1, y: 0 }),
            commanded(count),
            commanded(Command::Report { value: None }),
        ],
    );
    table.rules.push(Rule::new(on_key_a(), moving)).unwrap();

    let framing = group(&mut table, &[animated(object(20), OperationKind::Frame(2))]);
    let press = Trigger::Press { target: TriggerTarget::of(object(20)), button: 0 };
    table.rules.push(Rule::new(press, framing)).unwrap();

    let note = Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 };
    let colliding = group(&mut table, &[animated(object(20), OperationKind::Hide), commanded(note)]);
    let mut collide = Rule::new(Trigger::Becomes, colliding);
    collide.condition.push(Node::Overlaps(object(10), object(20))).unwrap();
    table.rules.push(collide).unwrap();
    image(&table)
}

/// Objects 10 and 20 are one teletext cell each, a cell apart; the pointer
/// presses object 20; then `a` is pressed twice, which brings object 10
/// onto object 20.
const INPUTS: &str = "\
# two objects of one cell, with a cell between them
0 rectangle 10 0 0 12 20
0 rectangle 20 24 0 12 20
0 level 0 200
2 pointer 30 5 1
3 pointer 30 5 0
4 key down char a
5 key up char a
6 key down physical a
7 key up name Enter
";

fn run(name: &str, table_file: &[u8], inputs: &str, more: &[&str]) -> std::process::Output {
    let dir = folder(name);
    let (table_path, inputs_path) = (dir.join("table.bin"), dir.join("inputs.txt"));
    std::fs::write(&table_path, table_file).unwrap();
    std::fs::write(&inputs_path, inputs).unwrap();
    let mut arguments = vec!["rules-run", table_path.to_str().unwrap(), inputs_path.to_str().unwrap()];
    arguments.extend(PAGE_ARGUMENT);
    arguments.extend(more);
    neottx(&arguments)
}

/// ANIM-18 (REQ-6529): the operations each tick's commands make are
/// written in the script's form at their tick, and the reports and MIDI
/// messages as comment lines; the same on every run; and the output is a
/// script `neottx render --frames` reads.
#[test]
fn a_table_is_run_into_a_script() {
    let output = run("rules_run", &table(), INPUTS, &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    let expected = "\
# page 150/0002, 4 rules, 4 groups, 0 sequences
0 cycle 10 0 3 6
0 look control:1 2
2 frame 20 2
4 move 10 1 0 relative
# 4 report rule 1 row none tick 4 target page 0 input 4
6 move 10 1 0 relative
# 6 report rule 1 row none tick 6 target page 0 input 6
7 hide 20
# 7 midi 0 0x90 60 100
# 9 ticks, 5 rules fired, 5 groups run, 11 commands applied, 0 skipped, 0 events dropped
";
    assert_eq!(text, expected);

    // The table as hexadecimal text, and a second run, give the same.
    let hexadecimal: String = table().iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join("\n");
    let again = run("rules_run_again", hexadecimal.as_bytes(), INPUTS, &[]);
    assert_eq!(String::from_utf8(again.stdout).unwrap(), text);

    // RULE-96 (REQ-6526): with every event given on arrival the output
    // is the same: this table's one sound command is a `becomes` rule's,
    // which stays on the tick.
    let arrival = run("rules_run_arrival", &table(), INPUTS, &["--on-arrival"]);
    assert!(arrival.status.success(), "{}", String::from_utf8_lossy(&arrival.stderr));
    assert_eq!(String::from_utf8(arrival.stdout).unwrap(), text);

    // On a ReGIS picture a unit is a pixel, so two moves of one pixel do
    // not bring object 10 onto object 20, and nothing is hidden.
    let regis = run("rules_run_regis", &table(), INPUTS, &["--picture", "regis", "--frames", "12"]);
    let regis = String::from_utf8(regis.stdout).unwrap();
    assert!(regis.contains("6 move 10 1 0 relative") && !regis.contains("hide 20"), "{regis}");
    assert!(regis.contains("# 12 ticks, "), "{regis}");

    // The output is the script of an image sequence.
    let dir = folder("rules_run_script");
    let page = dir.join("page.tti");
    std::fs::write(&page, "PN,10000\nOL,1,HELLO\n").unwrap();
    let script = dir.join("script.txt");
    std::fs::write(&script, &text).unwrap();
    let image = dir.join("seq.png");
    let played = neottx(&[
        "render",
        page.to_str().unwrap(),
        "100",
        "--scale",
        "1",
        "--frames",
        "9",
        "--frame-step",
        "8",
        "--script",
        script.to_str().unwrap(),
        "-o",
        image.to_str().unwrap(),
    ]);
    assert!(played.status.success(), "{}", String::from_utf8_lossy(&played.stderr));
    assert!(dir.join("seq-00008.png").exists());
}

/// RULE-96 (REQ-6526, DEC-6580 item 1): with `--on-arrival` a key
/// rule's sound commands are written first among its tick's lines, as
/// the world is given them on the key's arrival; its report and its
/// animation operation stay in their order, and the counts are the same.
#[test]
fn a_key_rules_sound_is_written_on_arrival() {
    let mut table = Box::new(Table::new(PAGE, SUB));
    let moved = OperationKind::Move { relative: true, x: 1, y: 0 };
    let note = Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 };
    let all = group(
        &mut table,
        &[commanded(Command::Report { value: None }), animated(Target::object(10), moved), commanded(note)],
    );
    table.rules.push(Rule::new(on_key_a(), all)).unwrap();
    let body = image(&table);
    let inputs = "0 rectangle 10 0 0 12 20\n1 key down char a\n";

    let at_the_tick = run("rules_run_key_tick", &body, inputs, &[]);
    let on_arrival = run("rules_run_key_arrival", &body, inputs, &["--on-arrival"]);
    let head = "# page 150/0002, 1 rules, 1 groups, 0 sequences\n";
    let report = "# 1 report rule 0 row none tick 1 target page 0 input 1\n";
    let (moved, midi) = ("1 move 10 1 0 relative\n", "# 1 midi 0 0x90 60 100\n");
    let counts = "# 3 ticks, 1 rules fired, 1 groups run, 3 commands applied, 0 skipped, 0 events dropped\n";
    assert_eq!(String::from_utf8(at_the_tick.stdout).unwrap(), [head, report, moved, midi, counts].concat());
    assert_eq!(String::from_utf8(on_arrival.stdout).unwrap(), [head, midi, report, moved, counts].concat());
}

/// RULE-109, RULE-114 (REQ-6613, REQ-6612): a `cue` line fires the
/// table's `cue` rule at the next tick, and a `command` line is a record
/// the server sends directly, applied at step 5 of its tick, before the
/// cue of the same tick.
#[test]
fn a_cue_and_a_servers_command_are_inputs() {
    let mut table = Box::new(Table::new(PAGE, SUB));
    let reported = group(&mut table, &[commanded(Command::Report { value: None })]);
    table.rules.push(Rule::new(Trigger::Cue { channel: 1, number: 255 }, reported)).unwrap();
    let set =
        commanded(Command::Variable { operation: VariableOperation::Set, variable: 2, operand: Operand::Constant(7) });
    let read = commanded(Command::Report { value: Some(neoteletext_rules::table::Value::variable(2)) });
    let hexadecimal = |body: &[u8]| {
        body.iter().fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
    };
    let inputs = format!(
        "2 cue 1 9\n2 cue 0 9\n2 command {} {}\n2 command {} {}\n",
        set.0,
        hexadecimal(&set.1),
        read.0,
        hexadecimal(&read.1)
    );
    let output = run("rules_run_cue", &image(&table), &inputs, &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let expected = "\
# page 150/0002, 1 rules, 1 groups, 0 sequences
# 2 report rule none row none tick 2 target page 0 value 7
# 2 report rule 0 row none tick 2 target page 0 fired
# 4 ticks, 1 rules fired, 1 groups run, 3 commands applied, 0 skipped, 0 events dropped
";
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

/// ANIM-18: a table its decoder refuses, and a line of the inputs that is
/// not understood, are refused with what is wrong.
#[test]
fn a_refused_table_and_refused_inputs_say_why() {
    let mut cut = table();
    cut.pop();
    let output = run("rules_run_cut", &cut, INPUTS, &[]);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("the table is refused: Short"));
    for (inputs, part) in [
        ("0 key down char ab", "line 1: a char key is one character"),
        ("0 key sideways char a", "a key is down or up"),
        ("0 key down name NoSuchKey", "names no key"),
        ("0 pointer 1 2", "the buttons is missing"),
        ("0 level 4 10", "an audio channel is 0 to 3"),
        ("0 level 0 256", "the level is not a number in its range"),
        ("0 rectangle 0 1 2 3 4", "an object's number is 1 to 65,535"),
        ("0 rectangle 1 1 2 3 4 5", "follows the input's last word"),
        ("0 wheel 1", "is not an input"),
        ("0 cue 4 1", "a cue is an audio channel below 4"),
        ("0 cue 0 128", "a cue is an audio channel below 4"),
        ("0 command 29 0g", "a command's body is hexadecimal digits"),
        ("0 command 29", "the command's body is missing"),
        ("5 key down char a\n4 key up char a", "line 2: tick 4 comes after tick 5"),
    ] {
        let output = run("rules_run_inputs", &table(), inputs, &[]);
        assert_eq!(output.status.code(), Some(3), "{inputs:?}");
        let message = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(message.contains(part), "{inputs:?}: {message}");
    }
}

/// RULE-35's example, the worked examples of page-rules.md section 7.5,
/// and the default table of RULE-53 as a `rules` message, one a line.
const EXAMPLES: [&str; 9] = [
    r#"{"type":"rules","connections":[7],"page":"101","sub":"0001","table":{"rects":[{"name":"pad","x":40,"y":300,"width":200,"height":100}],"rows":{"keys":[{"p0":{"physical":"a"},"p1":60,"p2":101},{"p0":{"physical":"s"},"p1":62,"p2":103}]},"groups":[{"name":"note_on","commands":[{"type":"local_midi","channel":0,"message":{"event":"note_on","channel":1,"key":{"plus":0,"row":"p1"},"velocity":100}},{"type":"animate","ops":[{"action":"look","target":{"object":{"row":"p2"}},"look":"pressed"}]}]},{"name":"note_off","commands":[{"type":"local_midi","channel":0,"message":{"event":"note_off","channel":1,"key":{"plus":0,"row":"p1"},"velocity":64}},{"type":"animate","ops":[{"action":"look","target":{"object":{"row":"p2"}},"look":"normal"}]}]}],"sequences":[{"name":"pulse","length":30,"repeat":0,"steps":[{"tick":0,"run":"note_on"},{"tick":15,"run":"note_off"}]}],"rules":[{"name":"down","on":[{"event":"key","keys":[{"physical":{"row":"p0"}}]},{"event":"press","target":{"object":{"row":"p2"}},"button":0}],"when":null,"rows":"keys","run":"note_on"},{"name":"up","on":[{"event":"key_up","keys":[{"physical":{"row":"p0"}}]},{"event":"release","target":{"object":{"row":"p2"}},"button":0}],"rows":"keys","run":"note_off"},{"name":"typed","on":{"event":"key","keys":"any","repeat":true},"do":[{"type":"report"}]}]}}"#,
    r#"{"type":"rules","connections":"all","page":"120","sub":"0000","table":{"rules":[{"name":"typed","on":[{"event":"key","keys":"any","repeat":true},{"event":"text"}],"do":[{"type":"report"}]}]}}"#,
    r#"{"type":"rules","connections":"all","page":"130","sub":"0000","table":{"rules":[{"name":"vote_down","on":{"event":"press","target":{"control":3},"button":0},"do":[{"type":"animate","ops":[{"action":"look","target":{"control":3},"look":"pressed"}]}]},{"name":"vote","on":{"event":"release","target":{"control":3},"button":0},"echo":true,"do":[{"type":"animate","ops":[{"action":"look","target":{"control":3},"look":"normal"}]}]}]}}"#,
    r#"{"type":"rules","connections":"all","page":"140","sub":"0000","table":{"groups":[{"name":"up","commands":[{"type":"variable","op":"inc","variable":"v0"},{"type":"variable","op":"min","variable":"v0","value":10},{"type":"report","value":{"of":"variable","variable":"v0"}}]},{"name":"down","commands":[{"type":"variable","op":"dec","variable":"v0"},{"type":"variable","op":"max","variable":"v0","value":0},{"type":"report","value":{"of":"variable","variable":"v0"}}]}],"rules":[{"name":"start","on":{"event":"open"},"do":[{"type":"variable","op":"set","variable":"v0","value":5}]},{"name":"up","on":{"event":"key","keys":[{"name":"ArrowUp"}],"repeat":true},"run":"up"},{"name":"down","on":{"event":"key","keys":[{"name":"ArrowDown"}],"repeat":true},"run":"down"}]}}"#,
    r#"{"type":"rules","connections":"all","page":"150","sub":"0000","table":{"rules":[{"name":"left","on":{"event":"key","keys":[{"name":"ArrowLeft"}],"repeat":true},"do":[{"type":"animate","ops":[{"action":"move","target":{"object":10},"x":-8,"y":0,"relative":true}]}]},{"name":"right","on":{"event":"key","keys":[{"name":"ArrowRight"}],"repeat":true},"do":[{"type":"animate","ops":[{"action":"move","target":{"object":10},"x":8,"y":0,"relative":true}]}]},{"name":"hit","on":{"event":"becomes"},"when":{"overlaps":[{"object":10},{"object":20}]},"echo":true,"do":[{"type":"sound","kind":"play","channel":1,"handle":5,"generation":1,"gain":100,"fade_ms":5,"duck":0,"loop":false,"after":false},{"type":"animate","ops":[{"action":"hide","target":{"object":20}}]}]}]}}"#,
    r#"{"type":"rules","connections":"all","page":"160","sub":"0000","table":{"rules":[{"name":"start","on":{"event":"open"},"do":[{"type":"animate","ops":[{"action":"cycle","target":{"object":7},"first":0,"last":1,"period":30,"once":false}]},{"type":"timer","op":"start","timer":"t0"}]},{"name":"awake","on":{"event":"key","keys":"any"},"do":[{"type":"timer","op":"start","timer":"t0"}]},{"name":"asleep","on":{"event":"after","ticks":600,"timer":"t0"},"do":[{"type":"report"}]}]}}"#,
    r#"{"type":"rules","connections":"all","page":"170","sub":"0000","table":{"rules":[{"name":"gate_on","on":{"event":"becomes"},"when":{"and":[{"read":{"of":"variable","variable":"v0"},"op":"==","value":0},{"read":{"of":"synth","channel":0,"family":"synth106","part":1,"property":"notes"},"op":">","value":0}]},"do":[{"type":"animate","ops":[{"action":"show","target":{"object":50}}]}]},{"name":"gate_off","on":{"event":"becomes"},"when":{"or":[{"read":{"of":"variable","variable":"v0"},"op":"!=","value":0},{"read":{"of":"synth","channel":0,"family":"synth106","part":1,"property":"notes"},"op":"==","value":0}]},"do":[{"type":"animate","ops":[{"action":"hide","target":{"object":50}}]}]},{"name":"help","on":{"event":"key","keys":[{"char":"h"}]},"do":[{"type":"variable","op":"set","variable":"v0","value":1}]},{"name":"back","on":{"event":"key","keys":[{"name":"Escape"}]},"do":[{"type":"variable","op":"set","variable":"v0","value":0}]},{"name":"open_filter","on":{"event":"wheel","target":{"object":51}},"do":[{"type":"report"}]}]}}"#,
    r#"{"type":"rules","connections":"all","page":"180","sub":"0000","table":{"groups":[{"name":"go","commands":[{"type":"sequence","op":"start","sequence":"fanfare"}]},{"name":"n1","commands":[{"type":"local_midi","channel":0,"message":{"event":"note_on","channel":1,"key":60,"velocity":100}}]},{"name":"n2","commands":[{"type":"local_midi","channel":0,"message":{"event":"note_off","channel":1,"key":60,"velocity":64}},{"type":"local_midi","channel":0,"message":{"event":"note_on","channel":1,"key":64,"velocity":100}}]},{"name":"n3","commands":[{"type":"local_midi","channel":0,"message":{"event":"note_off","channel":1,"key":64,"velocity":64}},{"type":"local_midi","channel":0,"message":{"event":"note_on","channel":1,"key":67,"velocity":100}}]},{"name":"n4","commands":[{"type":"local_midi","channel":0,"message":{"event":"note_off","channel":1,"key":67,"velocity":64}}]},{"name":"lit","commands":[{"type":"animate","ops":[{"action":"show","target":{"object":9}}]}]},{"name":"unlit","commands":[{"type":"animate","ops":[{"action":"hide","target":{"object":9}}]}]}],"sequences":[{"name":"fanfare","length":32,"repeat":2,"steps":[{"tick":0,"run":"n1"},{"tick":8,"run":"n2"},{"tick":16,"run":"n3"},{"tick":24,"run":"n4"}]},{"name":"flash","length":6,"repeat":1,"steps":[{"tick":0,"run":"lit"},{"tick":5,"run":"unlit"}]}],"rules":[{"name":"start","on":{"event":"open"},"do":[{"type":"timer","op":"start","timer":"t0"}]},{"name":"space","on":{"event":"key","keys":[{"char":" "}]},"do":[{"type":"run","group":"go","at":{"beat":{"timer":"t0","ticks":30}}}]},{"name":"bar","on":{"event":"cue","channel":1,"number":1},"do":[{"type":"sequence","op":"start","sequence":"flash"}]}]}}"#,
    r#"{"type":"rules","connections":"all","page":"100","sub":"0000","table":{"groups":[{"name":"send","commands":[{"type":"report"}]}],"rules":[{"name":"keys","on":{"event":"key","keys":"any","repeat":true},"run":"send"},{"name":"keys_up","on":{"event":"key_up","keys":"any"},"run":"send"},{"name":"text","on":{"event":"text"},"run":"send"},{"name":"press","on":{"event":"press","target":"page","button":"any"},"run":"send"},{"name":"release","on":{"event":"release","target":"page","button":"any"},"run":"send"},{"name":"move","on":{"event":"move","target":"page","held":false},"run":"send"},{"name":"hover_in","on":{"event":"enter","target":{"control":"any"}},"run":"send"},{"name":"hover_out","on":{"event":"leave","target":{"control":"any"}},"run":"send"},{"name":"drawing_in","on":{"event":"enter","target":{"object":"any"}},"run":"send"},{"name":"drawing_out","on":{"event":"leave","target":{"object":"any"}},"run":"send"},{"name":"focus","on":{"event":"focus"},"run":"send"},{"name":"blur","on":{"event":"blur"},"run":"send"},{"name":"wheel","on":{"event":"wheel","target":"page"},"run":"send"}]}}"#,
];

fn run_json(name: &str, message: &str, inputs: &str) -> std::process::Output {
    let dir = folder(name);
    let (table_path, inputs_path) = (dir.join("table.json"), dir.join("inputs.txt"));
    std::fs::write(&table_path, message).unwrap();
    std::fs::write(&inputs_path, inputs).unwrap();
    neottx(&["rules-run", table_path.to_str().unwrap(), inputs_path.to_str().unwrap()])
}

/// RULE-35, RULE-39 (REQ-6612, REQ-6613): each of the document's own
/// `rules` messages is read, made into a table that a receiver's decoder
/// accepts, and evaluated; the page is the message's own.
#[test]
fn the_documents_rules_messages_are_read() {
    for (index, message) in EXAMPLES.iter().enumerate() {
        let output = run_json(&format!("rules_run_json_{index}"), message, "");
        assert!(output.status.success(), "example {index}: {}", String::from_utf8_lossy(&output.stderr));
        let text = String::from_utf8(output.stdout).unwrap();
        let first = text.lines().next().unwrap();
        let page: serde_json::Value = serde_json::from_str(message).unwrap();
        let named = format!("# page {}/{},", page["page"].as_str().unwrap(), page["sub"].as_str().unwrap());
        assert!(first.starts_with(&named), "example {index}: {first}");
    }
}

/// RULE-35 example 3 (RULE-7, RULE-22, RULE-99): the arrow keys change v0
/// between 0 and 10 from 5, and the same group reports the new value.
#[test]
fn a_counter_written_as_json_reports_its_new_values() {
    let output = run_json(
        "rules_run_json_counter",
        EXAMPLES[3],
        "1 key down name ArrowUp\n2 key down name ArrowUp\n3 key down name ArrowDown\n",
    );
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    let values: Vec<&str> =
        text.lines().filter(|line| line.contains(" report ")).filter_map(|line| line.rsplit(' ').next()).collect();
    assert_eq!(values, ["6", "7", "6"], "{text}");
}

/// RULE-35 and RULE-39: a rule with both `run` and `do` is refused, and so
/// is a `do` holding a record a group may not hold.
#[test]
fn a_faulty_rules_message_is_refused() {
    let both = r#"{"type":"rules","connections":"all","page":"120","sub":"0000","table":{
        "groups":[{"name":"g","commands":[{"type":"report"}]}],
        "rules":[{"name":"r","on":{"event":"open"},"run":"g","do":[{"type":"report"}]}]}}"#;
    let forbidden = r#"{"type":"rules","connections":"all","page":"120","sub":"0000","table":{
        "rules":[{"name":"r","on":{"event":"open"},"do":[{"type":"redirect"}]}]}}"#;
    for (name, message, why) in [("both", both, "invalid_rules"), ("forbidden", forbidden, "invalid_group")] {
        let output = run_json(&format!("rules_run_json_{name}"), message, "");
        assert!(!output.status.success(), "{name}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(why), "{name}: {stderr}");
    }
}
