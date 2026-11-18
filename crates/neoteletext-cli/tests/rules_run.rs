//! `neottx rules-run` (animation.md ANIM-18, REQ-6529): a page rule table
//! evaluated against timed inputs, its animation operations written as the
//! script of an image sequence, and its reports and sound operations as
//! comment lines.
#![allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]

mod common;

use common::{folder, neottx};
use neoteletext_rules::animate::{Operation, OperationKind};
use neoteletext_rules::table::{Action, Key, KeyKind, Node, Rule, Table, Target, Trigger, Variable, any_key};

/// A table of four rules: at opening, object 10 cycles its frames and
/// control 1 is pressed; the key `a` moves object 10 one unit right, counts
/// it and reports it; a press on object 20 shows its frame 2; and when
/// object 10 comes to overlap object 20, object 20 is hidden and a note
/// sounds.
fn table() -> Vec<u8> {
    let object = Target::object;
    let animate = |target, kind| Action::Animate(Operation { target, kind });
    let mut table = Box::new(Table::new());
    table.variables.push(Variable { min: 0, max: 10, initial: 0 }).unwrap();

    let mut open = Rule::new(Trigger::Open);
    let cycle = OperationKind::Cycle { first: 0, last: 3, period: 6, once: false };
    open.actions.push(animate(object(10), cycle)).unwrap();
    open.actions.push(animate(Target::control(1), OperationKind::Look(2))).unwrap();
    table.rules.push(open).unwrap();

    let mut keys = any_key();
    keys.push(Key { kind: KeyKind::Character, code: u32::from('a') }).unwrap();
    let mut key = Rule::new(Trigger::Key { keys, repeat: false, mods_mask: 0, mods_value: 0 });
    key.actions.push(animate(object(10), OperationKind::Move { relative: true, x: 1, y: 0 })).unwrap();
    key.actions.push(Action::Add { variable: 0, amount: 1 }).unwrap();
    key.actions.push(Action::Send(None)).unwrap();
    table.rules.push(key).unwrap();

    let mut press = Rule::new(Trigger::Press { target: object(20), button: 0 });
    press.actions.push(animate(object(20), OperationKind::Frame(2))).unwrap();
    table.rules.push(press).unwrap();

    let mut collide = Rule::new(Trigger::Becomes);
    collide.condition.push(Node::Overlaps(object(10), object(20))).unwrap();
    collide.actions.push(animate(object(20), OperationKind::Hide)).unwrap();
    collide.actions.push(Action::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 }).unwrap();
    table.rules.push(collide).unwrap();

    let mut body = vec![0u8; neoteletext_rules::limits::MAX_TABLE_BYTES];
    let used = neoteletext_rules::encode(0x150, 7, Some(&table), &mut body).unwrap();
    body.truncate(used);
    body
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
    arguments.extend(more);
    neottx(&arguments)
}

/// ANIM-18 (REQ-6529): the operations each tick's actions produce are
/// written in the script's form at their tick, and the reports and MIDI
/// messages as comment lines; the same on every run; and the output is a
/// script `neottx render --frames` reads.
#[test]
fn a_table_is_run_into_a_script() {
    let output = run("rules_run", &table(), INPUTS, &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    let expected = "\
# page 150, generation 7, 4 rules
0 cycle 10 0 3 6
0 look control:1 2
2 frame 20 2
4 move 10 1 0 relative
# 4 report rule 1 tick 4 target page 0 input 4
6 move 10 1 0 relative
# 6 report rule 1 tick 6 target page 0 input 6
7 hide 20
# 7 midi 0 0x90 60 100
# 9 ticks, 5 rules fired, 11 actions run, 0 skipped, 0 events dropped, 0 overruns
";
    assert_eq!(text, expected);

    // The table as hexadecimal text, and a second run, give the same.
    let hexadecimal: String = table().iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join("\n");
    let again = run("rules_run_again", hexadecimal.as_bytes(), INPUTS, &[]);
    assert_eq!(String::from_utf8(again.stdout).unwrap(), text);

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
        ("5 key down char a\n4 key up char a", "line 2: tick 4 comes after tick 5"),
    ] {
        let output = run("rules_run_inputs", &table(), inputs, &[]);
        assert_eq!(output.status.code(), Some(3), "{inputs:?}");
        let message = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(message.contains(part), "{inputs:?}: {message}");
    }
}
