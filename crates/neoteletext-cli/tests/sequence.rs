//! `neottx render --frames` (animation.md ANIM-18, REQ-6529): a page
//! played on the presentation clock into a sequence of images, with a
//! script of timed operations and records; the same images on every run.
//!
//! `tests/golden/sequence.txt` holds a checksum of each image of the
//! golden sequences and carries a format version: a deliberate change to
//! what is drawn raises it and regenerates the file in the same change,
//! with `NEOTELETEXT_UPDATE_GOLDEN=1` (DEVELOPMENT.md section 8).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

mod common;

use common::{folder, neottx};
use neoteletext_core::consts::{CONTROL_JOINED, MODE_CONTROL, PACKET_ENHANCEMENT, PAGE_TIME_FILLING};
use neoteletext_core::ecc::with_odd_parity;
use neoteletext_core::packet::{
    ColourData, ObjectKind, ObjectNumber, ObjectUsage, Packet, Triplet, TripletFunction, encode,
};
use neoteletext_core::types::{ControlBits, Magazine, PageNumber, SubCode};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Raise when what a sequence draws changes on purpose.
#[cfg(feature = "objects")]
const FORMAT_VERSION: u32 = 1;

const FLASH: u8 = 0x08;
const STEADY: u8 = 0x09;
const YELLOW: u8 = 3;
const RED: u8 = 1;
/// The designation codes of the packets X/26 that hold the local objects:
/// the frames of the sprites.
const LETTERS: [(u8, u8); 3] = [(1, b'A'), (2, b'B'), (3, b'C')];

fn magazine() -> Magazine {
    Magazine::new(1).unwrap()
}

fn header(page: u8) -> Packet {
    encode::header(magazine(), PageNumber(page), SubCode::new(0), ControlBits(0), &[with_odd_parity(b' '); 32]).unwrap()
}

fn row(number: u8, codes: &[u8]) -> Packet {
    let mut bytes = [with_odd_parity(b' '); 40];
    for (slot, &code) in bytes.iter_mut().zip(codes) {
        *slot = with_odd_parity(code);
    }
    encode::row(magazine(), number, &bytes)
}

fn x26(designation: u8, functions: &[TripletFunction]) -> Packet {
    // A reserved row mode does nothing (EN 300 706 Table 28, mode 00010).
    let filler = TripletFunction::ReservedRow { mode: 0b00010, address: 41, data: 0 }.to_triplet();
    let mut triplets: [Triplet; 13] = [filler; 13];
    for (slot, function) in triplets.iter_mut().zip(functions) {
        *slot = function.to_triplet();
    }
    encode::triplet_packet(magazine(), PACKET_ENHANCEMENT, designation, &triplets)
}

/// Page 100 as a T42 file: text on rows 1 to 23, a flashing word on row
/// 12, a control on row 20, and three local Passive objects of two
/// letters each.
fn page() -> Vec<u8> {
    let mut packets = vec![header(0x00)];
    let control = [
        TripletFunction::SetActivePosition { address: 40 + 20, column: 0 },
        TripletFunction::ReservedColumn { column: 2, mode: MODE_CONTROL, data: 6 },
        TripletFunction::SetActivePosition { address: 40 + 21, column: 0 },
        TripletFunction::ReservedColumn { column: 2, mode: MODE_CONTROL, data: 6 | CONTROL_JOINED },
        TripletFunction::Termination { kind: 0b111 },
    ];
    packets.push(x26(0, &control));
    for (designation, letter) in LETTERS {
        let object = [
            TripletFunction::ObjectDefinition {
                usage: ObjectUsage(0b11),
                kind: ObjectKind::Passive,
                number: ObjectNumber(u16::from(designation) << 4),
            },
            TripletFunction::ForegroundColour { column: 0, colour: ColourData { index: YELLOW, high: 0 } },
            TripletFunction::BackgroundColour { column: 0, colour: ColourData { index: RED, high: 0 } },
            TripletFunction::G0Character { column: 0, code: letter },
            TripletFunction::G0Character { column: 1, code: letter },
            TripletFunction::Termination { kind: 0b101 },
        ];
        packets.push(x26(designation, &object));
    }
    for number in 1..=23u8 {
        let mut codes = if number == 12 { vec![FLASH, b'N', b'O', b'W', STEADY] } else { vec![b' '] };
        codes.extend(format!("ROW {number:02} the quick brown fox").bytes());
        packets.push(row(number, &codes));
    }
    packets.push(header(PAGE_TIME_FILLING));
    packets.iter().flat_map(|packet| packet.bytes).collect()
}

fn hexadecimal(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        write!(text, "{byte:02x}").unwrap();
        text
    })
}

/// A sprite of a SPRITES record: its number, its layer, its origin as row
/// and column, its size as rows and columns, and the letters of its frames.
type SpriteFields<'a> = (u16, u8, (i8, i8), (u8, u8), &'a [u8]);

/// The body of a SPRITES record for page 100, sub-code 0 (ANIM-7).
fn sprites_record(sprites: &[SpriteFields<'_>]) -> String {
    let mut body = vec![0x01, 0x00, 0x00, 0x00, u8::try_from(sprites.len()).unwrap(), 0];
    for &(number, layer, (row, column), (height, width), frames) in sprites {
        body.extend_from_slice(&number.to_be_bytes());
        body.extend_from_slice(&[0b01, layer, 0, column.to_be_bytes()[0], row.to_be_bytes()[0], width, height]);
        body.push(u8::try_from(frames.len()).unwrap());
        for letter in frames {
            let designation = LETTERS.iter().find(|(_, held)| held == letter).unwrap().0;
            // A local Passive object: source 01, type 11, then its number.
            let reference: u16 = 0b01 << 11 | 0b11 << 9 | u16::from(designation) << 4;
            body.extend_from_slice(&reference.to_be_bytes());
        }
    }
    hexadecimal(&body)
}

/// The images of a run, by file name, in order.
fn images(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found: Vec<(String, Vec<u8>)> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ppm" || extension == "png"))
        .map(|path| (path.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&path).unwrap()))
        .collect();
    found.sort();
    found
}

/// Plays page 100 of the fixture at Level 2.5 with `script` into PPM
/// images in a folder of the test's own.
fn play(name: &str, script: &str, more: &[&str]) -> (PathBuf, std::process::Output) {
    let dir = folder(name);
    let input = dir.join("page.t42");
    std::fs::write(&input, page()).unwrap();
    let script_file = dir.join("script.txt");
    std::fs::write(&script_file, script).unwrap();
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let output = out.join("seq.ppm");
    let mut arguments = vec!["render", input.to_str().unwrap(), "100", "--level", "2.5", "--scale", "1"];
    arguments.extend(["--image", "ppm", "-o", output.to_str().unwrap(), "--script", script_file.to_str().unwrap()]);
    arguments.extend(more);
    (out, neottx(&arguments))
}

#[cfg(feature = "objects")]
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3))
}

/// ANIM-18 (REQ-6529): an image is written every `--frame-step` ticks and
/// always the last, each named with its tick in five digits before the
/// extension; ANIM-1: each shows the flash phase of its tick, 57 ticks
/// shown and 19 hidden.
#[test]
fn images_are_named_by_tick_and_flash_by_tick() {
    let (out, run) = play("sequence_flash", "", &["--frames", "78", "--frame-step", "19"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let found = images(&out);
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    let expected =
        ["seq-00000.ppm", "seq-00019.ppm", "seq-00038.ppm", "seq-00057.ppm", "seq-00076.ppm", "seq-00077.ppm"];
    assert_eq!(names, expected);
    let image = |index: usize| &found[index].1;
    assert!(image(0) == image(1) && image(1) == image(2), "shown in ticks 0 to 56");
    assert!(image(3) != image(0), "hidden from tick 57");
    assert!(image(4) == image(0) && image(5) == image(0), "shown again from tick 76");
}

/// ANIM-18 (REQ-6529): the options and the script are checked, and a line
/// that is not understood is refused with its number.
#[test]
fn the_options_and_the_script_are_checked() {
    for (more, part) in [
        (&["--frames", "0"][..], "--frames"),
        (&["--frames", "36001"], "--frames"),
        (&["--frames", "2", "--frame-step", "0"], "--frame-step"),
        (&[], "--frames"),
        (&["--frame-step", "2"], "--frames"),
    ] {
        let (_, run) = play("sequence_usage", "", more);
        assert_eq!(run.status.code(), Some(2), "{more:?}");
        assert!(String::from_utf8_lossy(&run.stderr).contains(part), "{more:?}");
    }
    let (out, run) = play("sequence_longest", "", &["--frames", "36000", "--frame-step", "36000"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let names: Vec<String> = images(&out).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["seq-00000.ppm", "seq-35999.ppm"], "the longest sequence");

    let (_, run) = play("sequence_bad_line", "0 show 1\n3 jump 1\n", &["--frames", "4"]);
    assert_eq!(run.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&run.stderr).contains("line 2: \"jump\" is not an operation"));
    let (_, run) = play("sequence_bad_record", "0 show 1\n\n2 record FRAME 0100000003000001\n", &["--frames", "4"]);
    assert_eq!(run.status.code(), Some(3));
    let message = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(message.contains("the script's line 3: the record's frame mark is outside its range"), "{message}");
    // An operation on a target that does not exist does nothing and is counted.
    let (_, run) = play("sequence_missing", "0 show 9\n0 hide control:5\n", &["--frames", "1"]);
    assert!(run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("2 operations named a target that does not exist"));
}

/// ANIM-6, ANIM-18 (REQ-6529): a control hidden and given looks by the
/// script, with no sprite, in any build.
#[test]
fn a_script_hides_a_control_and_forces_its_looks() {
    let script = "1 look control:1 1\n2 look control:1 2\n3 look control:1 3\n4 look control:1 0\n5 hide control:1\n6 show control:1\n";
    let (out, run) = play("sequence_controls", script, &["--frames", "7"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let found = images(&out);
    let image = |tick: usize| &found[tick].1;
    assert_eq!(found.len(), 7);
    assert!(image(1) != image(0), "the hover look");
    assert!(image(2) != image(1) && image(2) != image(0), "the pressed look");
    assert!(image(3) == image(2), "look 3 is the pressed look on a teletext control");
    assert!(image(4) == image(0), "look 0: as the page has it");
    assert!(image(5) != image(0) && image(5) != image(1) && image(5) != image(2), "hidden: spaces");
    assert!(image(6) == image(0), "shown again");
}

/// ANIM-18: without objects compiled in, a SPRITES record says so.
#[cfg(not(feature = "objects"))]
#[test]
fn sprites_need_the_objects_feature() {
    let script = format!("0 record SPRITES {}\n", sprites_record(&[(1, 2, (5, 10), (1, 2), b"A")]));
    let (_, run) = play("sequence_no_objects", &script, &["--frames", "1"]);
    assert_eq!(run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run.stderr).contains("teletext sprites: not in this build"));
}

/// The script of the golden sequence: two sprites, one cycling three
/// frames, moved, reordered, hidden and shown, and an atomic frame.
#[cfg(feature = "objects")]
fn golden_script() -> String {
    let sprites = sprites_record(&[(1, 2, (5, 10), (1, 2), b"ABC"), (2, 2, (5, 11), (1, 2), b"B")]);
    // ANIMATE inside a frame: move sprite 2 down two rows and hide sprite 1,
    // both seen at the frame's end and neither before it.
    let animate =
        hexadecimal(&[&[0x01, 0x00, 0x3F, 0x7F, 2, 0][..], &[15, 2, 0, 2, 1, 0, 0, 0, 2], &[6, 2, 0, 1]].concat());
    let begin = hexadecimal(&[0x01, 0x00, 0x00, 0x00, 1, 0, 0, 7]);
    let end = hexadecimal(&[0x01, 0x00, 0x00, 0x00, 2, 0, 0, 7]);
    format!(
        "# two sprites, the first of three frames\n\
         0 record SPRITES {sprites}\n\
         0 cycle 1 0 2 2\n\
         4 order 1 keep 5\n\
         6 move 1 3 1 relative\n\
         8 stop_cycle 1\n\
         8 frame 1 9\n\
         9 look control:1 2\n\
         10 record FRAME {begin}\n\
         10 record ANIMATE {animate}\n\
         12 record FRAME {end}\n\
         14 show 1\n\
         14 move 1 -1 22\n\
         15 reset 2\n"
    )
}

/// ANIM-8, ANIM-14, ANIM-18 (REQ-6529): the golden sequence, played twice
/// into identical images; what each tick shows; and a script read from
/// standard input.
#[cfg(feature = "objects")]
#[test]
fn a_sequence_of_sprites_is_the_same_on_every_run() {
    let script = golden_script();
    let (out, run) = play("sequence_golden", &script, &["--frames", "16"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(run.stderr.is_empty(), "{}", String::from_utf8_lossy(&run.stderr));
    let found = images(&out);
    assert_eq!(found.len(), 16);
    let (again_out, again) = play("sequence_golden_again", &script, &["--frames", "16"]);
    assert!(again.status.success());
    assert!(found == images(&again_out), "a second run gives the same images");

    let image = |tick: usize| &found[tick].1;
    // A cycle of three frames, one every two ticks from tick 1.
    assert!(image(1) == image(2) && image(2) != image(3) && image(3) == image(4), "frames A, A, B, B");
    assert!(image(0) == image(1), "the cycle shows its first frame until it starts");
    assert!(image(4) != image(3) || image(5) != image(4), "tick 4 puts sprite 1 on top, tick 5 is frame C");
    assert!(image(6) != image(5), "moved at tick 6");
    assert!(image(8) != image(7), "the cycle stopped on a frame past the last, which shows the last");
    assert!(image(9) != image(8), "the control pressed");
    assert!(image(10) == image(9) && image(11) == image(9), "what the frame holds is not applied before its end");
    assert!(image(12) != image(11) && image(13) == image(12), "applied at its end, on one tick");
    assert!(image(14) != image(13) && image(15) != image(14));

    let mut actual = format!("format {FORMAT_VERSION}\n");
    for (name, bytes) in &found {
        writeln!(actual, "{name} {:016x}", fnv(bytes)).unwrap();
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/sequence.txt");
    if std::env::var_os("NEOTELETEXT_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual).unwrap();
    } else {
        let expected = std::fs::read_to_string(&path)
            .expect("tests/golden/sequence.txt; generate with NEOTELETEXT_UPDATE_GOLDEN=1");
        assert_eq!(actual, expected, "the sequence changed: if deliberate, raise FORMAT_VERSION and regenerate");
    }

    // The same script from standard input, and as PNG files.
    let dir = folder("sequence_stdin");
    let input = dir.join("page.t42");
    std::fs::write(&input, page()).unwrap();
    let output = dir.join("seq.png");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_neottx"))
        .args(["render", input.to_str().unwrap(), "100", "--level", "2.5", "--scale", "1", "--frames", "16"])
        .args(["--frame-step", "5", "--script", "-", "-o", output.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), script.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    let names: Vec<String> = images(&dir).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["seq-00000.png", "seq-00005.png", "seq-00010.png", "seq-00015.png"]);
}

/// ANIM-2 (REQ-6529): a frame with no end is applied after 120 ticks, and
/// a second begin applies what the first held.
#[cfg(feature = "objects")]
#[test]
fn a_frame_that_is_not_ended_is_applied() {
    let sprites = sprites_record(&[(1, 2, (5, 10), (1, 2), b"A")]);
    let begin = |identifier: u8| hexadecimal(&[0x01, 0x00, 0x00, 0x00, 1, 0, 0, identifier]);
    let script = format!("0 record FRAME {}\n0 record SPRITES {sprites}\n", begin(1));
    let (out, run) = play("sequence_frame_limit", &script, &["--frames", "122", "--frame-step", "119"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let found = images(&out);
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["seq-00000.ppm", "seq-00119.ppm", "seq-00121.ppm"]);
    assert!(found[0].1 == found[1].1, "held for 120 ticks");
    assert!(found[2].1 != found[1].1, "then applied");

    let script = format!("0 record FRAME {}\n0 record SPRITES {sprites}\n2 record FRAME {}\n", begin(1), begin(2));
    let (out, run) = play("sequence_second_begin", &script, &["--frames", "3"]);
    assert!(run.status.success());
    let found = images(&out);
    assert!(found[0].1 == found[1].1 && found[2].1 != found[1].1, "a second begin applies what the first held");
}
