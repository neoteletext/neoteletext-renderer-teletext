//! The channel's records on a push connection (DOC-1952, DOC-1955 to
//! DOC-1957, DOC-1961; REQ-480, REQ-TST-040): the reader never panics, on
//! packets or on a record's body, and every index, LOG, `SET_COOKIE` and
//! DENIED record it accepts, written again by its encoder, reads back the
//! same. The LINKS, SUBPAGES and REDIRECT records have targets of their own.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_core::consts::PACKET_BYTES;
use neoteletext_core::packet::Packet;
use neoteletext_web::records::index::{IndexMagazine, IndexPage, Progress};
use neoteletext_web::records::{
    ChannelRecord, Read, RecordReader, TYPE_SET_COOKIE, cookie, denied, index, links, log, subpages, write_packets,
};

/// A service any record may be written on.
const SERVICE: u8 = 3;

/// The one record of `kind` that `packets` hold, read back.
fn read_back(packets: &[Packet], each: &mut dyn FnMut(ChannelRecord<'_>)) {
    let mut reader = RecordReader::new();
    let mut seen = 0;
    for packet in packets {
        match reader.push(packet) {
            Read::Record(record) => {
                seen += 1;
                each(record);
            }
            Read::Invalid(count) => panic!("{count} packets of a record its encoder wrote were refused"),
            Read::Nothing => {}
        }
    }
    assert!(seen > 0, "the encoder's record was read");
}

fn index_again(body: &[u8]) {
    let Ok(record) = index::read(body) else { return };
    let pages: Vec<IndexPage<'_>> = record.pages().collect();
    let magazine = [IndexMagazine { magazine: record.magazine, name: record.name, pages: &pages }];
    let mut packets = Vec::new();
    index::encode(record.generation, &magazine, SERVICE, 0, &mut |packet| packets.push(packet))
        .expect("a record the reader took is an index the encoder takes");
    let mut back = Vec::new();
    read_back(&packets, &mut |again| {
        if let ChannelRecord::Index(again) = again {
            assert_eq!((again.generation, again.magazine), (record.generation, record.magazine));
            back.extend(
                again.pages().map(|page| (page.page, page.subpages, page.title.to_owned(), page.section.to_owned())),
            );
        }
    });
    let pages: Vec<_> =
        pages.iter().map(|page| (page.page, page.subpages, page.title.to_owned(), page.section.to_owned())).collect();
    assert_eq!(back, pages);
}

fn log_again(body: &[u8]) {
    let Ok(record) = log::read(body) else { return };
    let mut packets = Vec::new();
    log::encode(record.line.level, record.page, record.line.text, SERVICE, 0, &mut |packet| packets.push(packet))
        .expect("a line the reader took is one the encoder takes");
    read_back(&packets, &mut |again| assert_eq!(again, ChannelRecord::Log(record)));
}

fn cookie_again(body: &[u8]) {
    let Ok(line) = cookie::read(body) else { return };
    let mut packets = Vec::new();
    write_packets(TYPE_SET_COOKIE, line, SERVICE, 0, &mut |packet| packets.push(packet)).expect("fits the frame");
    read_back(&packets, &mut |again| assert_eq!(again, ChannelRecord::SetCookie(line)));
}

fn denied_again(body: &[u8]) {
    let Ok(record) = denied::read(body) else { return };
    let mut packets = Vec::new();
    denied::encode(record, SERVICE, 0, &mut |packet| packets.push(packet)).expect("the encoder takes what was read");
    read_back(&packets, &mut |again| assert_eq!(again, ChannelRecord::Denied(record)));
}

fuzz_target!(|data: &[u8]| {
    let Some((&choice, body)) = data.split_first() else { return };
    match choice % 5 {
        0 => index_again(body),
        1 => log_again(body),
        2 => cookie_again(body),
        3 => denied_again(body),
        _ => {
            // Packets: the reader and every receiver's tracker of a
            // sending never panic, whatever arrives.
            let mut reader = RecordReader::new();
            let (mut progress, mut links, mut lists) =
                (Progress::new(), links::Sending::new(), subpages::Sending::new());
            for &bytes in body.as_chunks::<PACKET_BYTES>().0 {
                let packet = Packet { bytes };
                if let (Read::Record(record), packets) = reader.push_counted(&packet) {
                    match record {
                        ChannelRecord::Index(record) => {
                            progress.note(&record);
                        }
                        ChannelRecord::Links(record) => {
                            links.note(&record, packets);
                        }
                        ChannelRecord::SubPages(record) => {
                            lists.note(&record, packets);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
});
