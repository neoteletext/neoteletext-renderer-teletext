//! `neottx render`, `check` and `write-text --picture` of ReGIS and NAPLPS
//! pictures (renderer parity PAR-7, PAR-10, REQ-6763), run as a user runs
//! them.

mod common;

use common::neottx;
use neoteletext_app::exit::CHECK_ERRORS;
use std::fs;

/// The PNG signature and the width and height of its IHDR chunk.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(bytes.get(..8), Some(&b"\x89PNG\r\n\x1a\n"[..]));
    let field = |at: usize| u32::from_be_bytes(bytes.get(at..at + 4).unwrap().try_into().unwrap());
    (field(16), field(20))
}

// PAR-7: one file of each language renders, at its own size.
#[test]
fn a_picture_file_of_each_language_renders_at_its_size() {
    let folder = common::folder("pictures-render");
    for (name, bytes, size) in [
        ("p.regis", &b"S(E)W(I2)P[10,10]F(V[100,10][100,100][10,100])"[..], (800, 480)),
        ("p.naplps", &[0x0E, 0x31, 0x50, 0x40, 0x40][..], (480, 500)),
    ] {
        let (input, output) = (folder.join(name), folder.join(format!("{name}.png")));
        fs::write(&input, bytes).unwrap();
        let ran = neottx(&["render", input.to_str().unwrap(), "100", "-o", output.to_str().unwrap()]);
        assert!(ran.status.success(), "{name}: {}", String::from_utf8_lossy(&ran.stderr));
        assert_eq!(png_size(&fs::read(&output).unwrap()), size, "{name}");
    }
}

// PAR-7: `check` exits non-zero on a NAPLPS stream past
// MAX_PROCESSED_BYTES and zero on a clean one. A file that long is past
// MAX_PICTURE_STREAM_BYTES too, so it is refused as it is read; the
// renderer's own refusal of the processed bytes is the picture module's
// unit test.
#[test]
fn check_refuses_a_naplps_stream_past_the_processing_limit() {
    let folder = common::folder("pictures-check");
    let (clean, long) = (folder.join("clean.nap"), folder.join("long.nap"));
    fs::write(&clean, [0x0E, 0x31, 0x50, 0x40, 0x40]).unwrap();
    fs::write(&long, vec![0_u8; naplps::limits::MAX_PROCESSED_BYTES + 1]).unwrap();
    assert!(neottx(&["check", clean.to_str().unwrap()]).status.success());
    let refused = neottx(&["check", long.to_str().unwrap()]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("larger than"));
    // A stream the renderer refuses within the file bound: a PDI longer
    // than the renderer holds (MAX_PDI_BYTES), an operand byte after
    // operand byte.
    let pdi = folder.join("pdi.nap");
    let mut bytes = vec![0x0E, 0x34];
    bytes.extend(std::iter::repeat_n(0x40_u8, naplps::limits::MAX_PDI_BYTES + 1));
    fs::write(&pdi, bytes).unwrap();
    let refused = neottx(&["check", pdi.to_str().unwrap()]);
    let printed = String::from_utf8_lossy(&refused.stdout);
    assert_eq!(refused.status.code(), Some(i32::from(CHECK_ERRORS)), "{printed}");
    assert!(printed.contains("refused"), "{printed}");
}

/// A sub-page's picture as read back: its stream, its links and its cursor.
type ReadBack = (Vec<u8>, Vec<(u16, Vec<u8>)>, Option<u16>);

/// Every REGIS or NAPLPS record's bytes in a file of packets, joined, and
/// the links and cursor records, read by the engine's record reader.
fn records_of(file: &[u8]) -> ReadBack {
    use neoteletext_web::records::{ChannelRecord, Read, RecordReader};
    let mut reader = RecordReader::new();
    let (mut stream, mut links, mut cursor) = (Vec::new(), Vec::new(), None);
    for bytes in file.as_chunks::<{ neoteletext_core::consts::PACKET_BYTES }>().0 {
        match reader.push(&neoteletext_core::packet::Packet { bytes: *bytes }) {
            Read::Record(ChannelRecord::Regis(record) | ChannelRecord::Naplps(record)) => {
                assert_eq!(usize::try_from(record.offset).unwrap(), stream.len());
                stream.extend_from_slice(record.data);
            }
            Read::Record(ChannelRecord::DrawingLinks(record)) => {
                links.extend(record.links().map(|link| (link.drawing, link.target.to_vec())));
            }
            Read::Record(ChannelRecord::RegisCursor(record)) => cursor = Some(record.drawing),
            _ => {}
        }
    }
    (stream, links, cursor)
}

// PAR-8: a ReGIS and a NAPLPS picture converted into a channel's sub-page
// read back as the same bytes, links and cursor, and the sub-page renders
// as the picture file does (PAR-7).
#[test]
fn a_picture_carried_in_a_channel_reads_back_and_renders_as_its_file() {
    let folder = common::folder("pictures-channel");
    let page = folder.join("page.tti");
    fs::write(&page, "PN,20000\nSC,0001\nOL,1,Hello\n").unwrap();
    let links = folder.join("links.json");
    fs::write(&links, r#"[{"drawing": 3, "target": "300"}]"#).unwrap();
    let pictures: [(&str, &str, Vec<u8>); 2] = [
        ("regis", "p.regis", b"S(E)W(I2)P[10,10]F(V[300,10][300,200][10,200])".to_vec()),
        ("naplps", "p.nap", vec![0x0E, 0x31, 0x50, 0x40, 0x40]),
    ];
    for (flag, name, bytes) in pictures {
        let picture = folder.join(name);
        fs::write(&picture, &bytes).unwrap();
        let channel = folder.join(format!("channel-{flag}"));
        let ran = neottx(&[
            "convert",
            page.to_str().unwrap(),
            "--to",
            "channel",
            "-o",
            channel.to_str().unwrap(),
            "--page",
            "200/0001",
            &format!("--{flag}"),
            picture.to_str().unwrap(),
            "--links",
            links.to_str().unwrap(),
            "--cursor",
            "3",
        ]);
        assert!(ran.status.success(), "{flag}: {}", String::from_utf8_lossy(&ran.stderr));
        let file = channel.join("200.t42");
        let (stream, read_links, cursor) = records_of(&fs::read(&file).unwrap());
        assert_eq!(stream, bytes, "{flag}");
        assert_eq!(read_links, [(3, b"300".to_vec())]);
        assert_eq!(cursor, Some(3));
        let (from_channel, from_file) =
            (folder.join(format!("{flag}-channel.png")), folder.join(format!("{flag}-file.png")));
        let rendered = neottx(&["render", file.to_str().unwrap(), "200/0001", "-o", from_channel.to_str().unwrap()]);
        assert!(rendered.status.success(), "{flag}: {}", String::from_utf8_lossy(&rendered.stderr));
        assert!(
            neottx(&["render", picture.to_str().unwrap(), "100", "-o", from_file.to_str().unwrap()]).status.success()
        );
        assert_eq!(fs::read(&from_channel).unwrap(), fs::read(&from_file).unwrap(), "{flag}");
    }
    // A picture larger than MAX_PICTURE_STREAM_BYTES is refused.
    let large = folder.join("large.regis");
    fs::write(&large, vec![b' '; neoteletext_formats::picture_text::MAX_PICTURE_STREAM_BYTES + 1]).unwrap();
    let out = folder.join("channel-large");
    let refused = neottx(&[
        "convert",
        page.to_str().unwrap(),
        "--to",
        "channel",
        "-o",
        out.to_str().unwrap(),
        "--page",
        "200/0001",
        "--regis",
        large.to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("MAX_PICTURE_STREAM_BYTES"));
}

// PAR-10: write-text --picture writes a picture that draws.
#[test]
fn write_text_writes_a_picture_in_each_language() {
    let folder = common::folder("pictures-write-text");
    let text = folder.join("text.txt");
    fs::write(&text, "Καλημέρα κόσμε\nδεύτερη γραμμή\n").unwrap();
    for language in ["regis", "naplps"] {
        let out = folder.join(format!("text.{language}"));
        let ran = neottx(&[
            "write-text",
            "--language",
            "el",
            "--page",
            "100",
            "--picture",
            language,
            text.to_str().unwrap(),
            out.to_str().unwrap(),
        ]);
        assert!(ran.status.success(), "{language}: {}", String::from_utf8_lossy(&ran.stderr));
        let checked = neottx(&["check", out.to_str().unwrap()]);
        assert!(checked.status.success(), "{language}: {}", String::from_utf8_lossy(&checked.stdout));
    }
}

/// Writes a channel's entry file into `folder`, so that `neottx html` and
/// the readers take it as a static channel.
fn entry_file(folder: &std::path::Path) {
    fs::write(folder.join("channel.ttx"), r#"{"name":"Pictures","initial_page":"200"}"#).unwrap();
}

/// Converts `page` with a picture into a channel folder `channel`, with the
/// extra arguments given, and gives the sub-page's file.
fn convert_picture(page: &std::path::Path, channel: &std::path::Path, extra: &[&str]) -> std::path::PathBuf {
    let mut arguments = vec![
        "convert",
        page.to_str().unwrap(),
        "--to",
        "channel",
        "-o",
        channel.to_str().unwrap(),
        "--page",
        "200/0001",
    ];
    arguments.extend_from_slice(extra);
    let ran = neottx(&arguments);
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    entry_file(channel);
    channel.join("200.t42")
}

/// The objects file of PAR-8: one region, hidden, so its cover fills it.
const OBJECTS: &str = r#"{"objects": [{"number": 7, "kind": "region", "shown": false, "detectable": true,
    "layer": 1, "order": 0, "x": 0, "y": 0, "width": 200, "height": 200, "origin_x": 0, "origin_y": 0,
    "frames": [], "cover": 3, "label": "A door"}], "title": "The harbour"}"#;

// PAR-8: `--objects` writes the NAPLPS_OBJECTS record after the stream's
// and before the links, read back by the engine's reader; the sub-page
// renders with the objects composed (ANIM-11), unlike the bare file.
#[test]
fn naplps_objects_are_carried_and_composed() {
    use neoteletext_web::records::{ChannelRecord, Read, RecordReader};
    let folder = common::folder("pictures-objects");
    let page = folder.join("page.tti");
    fs::write(&page, "PN,20000\nSC,0001\nOL,1,Hello\n").unwrap();
    let (picture, objects, links) = (folder.join("p.nap"), folder.join("objects.json"), folder.join("links.json"));
    fs::write(&picture, [0x0E, 0x31, 0x50, 0x40, 0x40]).unwrap();
    fs::write(&objects, OBJECTS).unwrap();
    fs::write(&links, r#"[{"object": 7, "target": "300"}]"#).unwrap();
    let channel = folder.join("channel");
    let file = convert_picture(
        &page,
        &channel,
        &[
            "--naplps",
            picture.to_str().unwrap(),
            "--objects",
            objects.to_str().unwrap(),
            "--links",
            links.to_str().unwrap(),
        ],
    );
    let bytes = fs::read(&file).unwrap();
    let mut reader = RecordReader::new();
    let mut kinds = Vec::new();
    for packet in bytes.as_chunks::<{ neoteletext_core::consts::PACKET_BYTES }>().0 {
        match reader.push(&neoteletext_core::packet::Packet { bytes: *packet }) {
            Read::Record(ChannelRecord::Naplps(_)) => kinds.push("naplps"),
            Read::Record(ChannelRecord::NaplpsObjects(record)) => {
                assert_eq!((record.generation, record.title), (1, Some("The harbour")));
                let objects: Vec<_> =
                    record.labelled_objects().map(|object| (object.object.number, object.label)).collect();
                assert_eq!(objects, [(7, "A door")]);
                kinds.push("objects");
            }
            Read::Record(ChannelRecord::DrawingLinks(_)) => kinds.push("links"),
            _ => {}
        }
    }
    assert_eq!(kinds, ["naplps", "objects", "links"]);
    let (composed, bare) = (folder.join("composed.png"), folder.join("bare.png"));
    assert!(neottx(&["render", file.to_str().unwrap(), "200/0001", "-o", composed.to_str().unwrap()]).status.success());
    assert!(neottx(&["render", picture.to_str().unwrap(), "100", "-o", bare.to_str().unwrap()]).status.success());
    assert_ne!(fs::read(&composed).unwrap(), fs::read(&bare).unwrap(), "the hidden region's cover is drawn");
    // An objects file the record's reader would refuse is refused.
    fs::write(&objects, OBJECTS.replace("\"layer\": 1", "\"layer\": 9")).unwrap();
    let refused = neottx(&[
        "convert",
        page.to_str().unwrap(),
        "--to",
        "channel",
        "-o",
        folder.join("refused").to_str().unwrap(),
        "--page",
        "200/0001",
        "--naplps",
        picture.to_str().unwrap(),
        "--objects",
        objects.to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
}

// PAR-8: `neottx html` on a channel whose sub-page carries a picture
// writes the picture's PNG beside the page's HTML, equal to `neottx
// render`'s, with the picture's title as the `alt` text.
#[test]
fn html_draws_a_picture_sub_page_as_render_does() {
    let folder = common::folder("pictures-html");
    let page = folder.join("page.tti");
    fs::write(&page, "PN,20000\nSC,0001\nDE,A picture\nOL,1,Hello\n").unwrap();
    let (regis, naplps, objects) = (folder.join("p.regis"), folder.join("p.nap"), folder.join("objects.json"));
    fs::write(&regis, b"S(E)W(I2)P[10,10]F(V[300,10][300,200][10,200])D(T'The harbour',C)").unwrap();
    fs::write(&naplps, [0x0E, 0x31, 0x50, 0x40, 0x40]).unwrap();
    fs::write(&objects, OBJECTS).unwrap();
    for (name, extra) in [
        ("regis", vec!["--regis", regis.to_str().unwrap()]),
        ("naplps", vec!["--naplps", naplps.to_str().unwrap(), "--objects", objects.to_str().unwrap()]),
    ] {
        let channel = folder.join(format!("channel-{name}"));
        let file = convert_picture(&page, &channel, &extra);
        let ran = neottx(&["html", channel.to_str().unwrap(), "--address", "https://h.example/p/", "--public", "200"]);
        assert!(ran.status.success(), "{name}: {}", String::from_utf8_lossy(&ran.stderr));
        let rendered = folder.join(format!("{name}.png"));
        assert!(
            neottx(&["render", file.to_str().unwrap(), "200/0001", "-o", rendered.to_str().unwrap()]).status.success()
        );
        assert_eq!(fs::read(channel.join("200/picture.png")).unwrap(), fs::read(&rendered).unwrap(), "{name}");
        let html = fs::read_to_string(channel.join("200/index.html")).unwrap();
        assert!(html.contains("alt=\"The harbour\""), "{name}: {html}");
    }
    // A page that is not public shows nothing of itself: no picture file.
    let channel = folder.join("channel-private");
    convert_picture(&page, &channel, &["--regis", regis.to_str().unwrap()]);
    assert!(neottx(&["html", channel.to_str().unwrap(), "--address", "https://h.example/p/"]).status.success());
    assert!(!channel.join("200/picture.png").exists());
}

// PAR-7: `check` reports a ReGIS commit that fails, with its code and
// drawing, and exits non-zero (PAR-23): drawing 5 names a parent, 99,
// that does not exist, code 5 of `docs/regis-v2.md`'s failed commits.
#[test]
fn check_reports_a_failed_regis_commit() {
    let folder = common::folder("pictures-check-commit");
    let (failing, clean) = (folder.join("failing.regis"), folder.join("clean.regis"));
    fs::write(&failing, b"D(B5)P[0,0]V[20,20]D(E)D(S5(P99),C)").unwrap();
    fs::write(&clean, b"D(B5)P[0,0]V[20,20]D(E)D(C)").unwrap();
    let refused = neottx(&["check", failing.to_str().unwrap()]);
    let printed = String::from_utf8_lossy(&refused.stdout);
    assert_eq!(refused.status.code(), Some(i32::from(CHECK_ERRORS)), "{printed}");
    assert!(printed.contains("a commit failed: code 5, drawing 5"), "{printed}");
    let passed = neottx(&["check", clean.to_str().unwrap()]);
    assert!(passed.status.success(), "{}", String::from_utf8_lossy(&passed.stdout));
}

/// Raise when what a picture draws changes on purpose, and regenerate the
/// golden images with `NEOTELETEXT_UPDATE_GOLDEN=1` (DEVELOPMENT.md
/// section 8).
const PICTURE_FORMAT_VERSION: u32 = 4;

// PAR-7: one file of each language, and a `.t42` carrying each record
// kind, render to the golden images.
#[test]
fn pictures_render_to_the_golden_images() {
    let folder = common::folder("pictures-golden");
    let page = folder.join("page.tti");
    fs::write(&page, "PN,20000\nSC,0001\nOL,1,Hello\n").unwrap();
    let (regis, naplps) = (folder.join("p.regis"), folder.join("p.nap"));
    // Two colours, a line, a filled polygon and text in a provided set.
    fs::write(
        &regis,
        "S(E)W(I2)P[10,10]F(V[300,10][300,200][10,200])W(I5)P[400,300]V[700,450]P[20,400]T(A101)\"Hello\"T(A0)",
    )
    .unwrap();
    // A filled rectangle.
    fs::write(&naplps, [0x0E, 0x31, 0x50, 0x40, 0x40]).unwrap();
    let (objects, links) = (folder.join("objects.json"), folder.join("links.json"));
    fs::write(&objects, OBJECTS).unwrap();
    fs::write(&links, r#"[{"drawing": 7, "target": "300"}]"#).unwrap();
    let regis_t42 = convert_picture(
        &page,
        &folder.join("channel-regis"),
        &["--regis", regis.to_str().unwrap(), "--links", links.to_str().unwrap(), "--cursor", "7"],
    );
    let naplps_t42 = convert_picture(
        &page,
        &folder.join("channel-naplps"),
        &[
            "--naplps",
            naplps.to_str().unwrap(),
            "--objects",
            objects.to_str().unwrap(),
            "--links",
            links.to_str().unwrap(),
            "--cursor",
            "7",
        ],
    );
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/pictures");
    let update = std::env::var_os("NEOTELETEXT_UPDATE_GOLDEN").is_some();
    let mut names = format!("format {PICTURE_FORMAT_VERSION}\n");
    for (name, input, page) in [
        ("regis.png", &regis, "100"),
        ("naplps.png", &naplps, "100"),
        ("regis-t42.png", &regis_t42, "200/0001"),
        ("naplps-t42.png", &naplps_t42, "200/0001"),
    ] {
        let output = folder.join(name);
        let ran = neottx(&["render", input.to_str().unwrap(), page, "-o", output.to_str().unwrap()]);
        assert!(ran.status.success(), "{name}: {}", String::from_utf8_lossy(&ran.stderr));
        let drawn = fs::read(&output).unwrap();
        names.push_str(name);
        names.push('\n');
        if update {
            fs::create_dir_all(&golden).unwrap();
            fs::write(golden.join(name), &drawn).unwrap();
        } else {
            let expected =
                fs::read(golden.join(name)).expect("tests/golden/pictures; generate with NEOTELETEXT_UPDATE_GOLDEN=1");
            assert!(drawn == expected, "{name} changed: if deliberate, raise PICTURE_FORMAT_VERSION and regenerate");
        }
    }
    let index = golden.join("format.txt");
    if update {
        fs::write(&index, &names).unwrap();
    } else {
        assert_eq!(fs::read_to_string(&index).unwrap(), names, "raise PICTURE_FORMAT_VERSION and regenerate");
    }
}

// PAR-10, PAR-8: write-text --picture --to channel writes a static
// channel's sub-page that carries the picture, which renders as the
// picture file of the same text does.
#[test]
fn write_text_writes_a_picture_into_a_channel() {
    let folder = common::folder("pictures-write-channel");
    let text = folder.join("text.txt");
    fs::write(&text, "Καλημέρα κόσμε\n").unwrap();
    for language in ["regis", "naplps"] {
        let (file, channel) = (folder.join(format!("text.{language}")), folder.join(format!("channel-{language}")));
        let write = |extra: &[&str], output: &std::path::Path| {
            let mut arguments = vec!["write-text", "--language", "el", "--page", "200/0001", "--picture", language];
            arguments.extend_from_slice(extra);
            arguments.extend([text.to_str().unwrap(), output.to_str().unwrap()]);
            let ran = neottx(&arguments);
            assert!(ran.status.success(), "{language}: {}", String::from_utf8_lossy(&ran.stderr));
        };
        write(&[], &file);
        write(&["--to", "channel"], &channel);
        let sub_page = channel.join("200.t42");
        let (from_channel, from_file) =
            (folder.join(format!("{language}-channel.png")), folder.join(format!("{language}-file.png")));
        let ran = neottx(&["render", sub_page.to_str().unwrap(), "200/0001", "-o", from_channel.to_str().unwrap()]);
        assert!(ran.status.success(), "{language}: {}", String::from_utf8_lossy(&ran.stderr));
        assert!(neottx(&["render", file.to_str().unwrap(), "100", "-o", from_file.to_str().unwrap()]).status.success());
        assert_eq!(fs::read(&from_channel).unwrap(), fs::read(&from_file).unwrap(), "{language}");
    }
}

// PAR-8, TODO-6808: a picture converted into a sub-page of a page of two
// sub-pages goes into that sub-page's own file in the page's folder, the
// page kept whole (before, the page was cut down to the one sub-page), and
// into the page's file, which holds the first sub-page too; its records
// take more than MAX_SUBPAGE_FILE, and the channel reader of `neottx
// contents` reads the sub-page's file (before, it skipped it) and it
// renders from its `NNN/SSSS.t42` as its picture file does.
#[test]
fn a_picture_in_a_sub_page_folder_is_read_past_the_pages_bound() {
    let folder = common::folder("pictures-sub-page-folder");
    let page = folder.join("page.tti");
    fs::write(&page, "PN,20000\nSC,0001\nOL,1,First\nPN,20000\nSC,0002\nOL,1,Second\n").unwrap();
    // A long ReGIS stream: the same short line drawn many times.
    let regis = folder.join("p.regis");
    let bytes = b"S(E)W(I2)".iter().copied().chain(b"P[10,10]V[300,200]".repeat(400)).collect::<Vec<u8>>();
    fs::write(&regis, &bytes).unwrap();
    let channel = folder.join("channel");
    convert_picture(&page, &channel, &["--regis", regis.to_str().unwrap()]);
    // The page keeps both its sub-pages, so its folder, and the picture is
    // in its first sub-page's own file there.
    let file = channel.join("200").join("0001.t42");
    assert!(channel.join("200").join("0002.t42").is_file(), "the page keeps its other sub-page");
    assert_eq!(records_of(&fs::read(channel.join("200").join("0002.t42")).unwrap()).0, b"");
    let written = fs::read(&file).unwrap();
    assert!(written.len() > neoteletext_web::subpages::MAX_SUBPAGE_FILE, "{}", written.len());
    assert_eq!(records_of(&written).0, bytes);
    // The page's file holds its first sub-page too, so it carries the
    // picture as well.
    assert_eq!(records_of(&fs::read(channel.join("200.t42")).unwrap()).0, bytes);
    // Without the page's file the reader reads the sub-page's own.
    fs::remove_file(channel.join("200.t42")).unwrap();
    let contents = neottx(&["contents", channel.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&contents.stderr);
    assert!(contents.status.success(), "{stderr}");
    assert!(!stderr.contains("skipped"), "{stderr}");
    let (from_channel, from_file) = (folder.join("channel.png"), folder.join("file.png"));
    let rendered = neottx(&["render", file.to_str().unwrap(), "200/0001", "-o", from_channel.to_str().unwrap()]);
    assert!(rendered.status.success(), "{}", String::from_utf8_lossy(&rendered.stderr));
    assert!(neottx(&["render", regis.to_str().unwrap(), "100", "-o", from_file.to_str().unwrap()]).status.success());
    assert_eq!(fs::read(&from_channel).unwrap(), fs::read(&from_file).unwrap());
}
