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
