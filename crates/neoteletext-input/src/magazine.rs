//! Magazine music on a push connection (REQ-6927 item 3, DEC-6953 item 5):
//! the test a host applies to each record of the audio profile's carriage,
//! so that a record of magazine music on a connection whose WELCOME did not
//! agree [`MAGAZINE_SOUND_CAP`] is a protocol error, as any record of a
//! capability not agreed is. Nothing here allocates.

use neoteletext_audio::record::{Control, Record, parse};

use crate::caps::MAGAZINE_SOUND_CAP;

/// A record of magazine music came on a connection that did not agree it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotAgreed;

/// Whether `record`, one whole record of the audio profile's frame, is
/// magazine music's: a CONTROL record naming a magazine (REQ-6925 item 1)
/// or a `MAGAZINE_MUSIC` record of a page's setting (REQ-6926 item 1). A
/// record that does not parse is not: its own reader refuses it.
#[must_use]
pub fn is_magazine_record(record: &[u8]) -> bool {
    matches!(parse(record), Ok(Record::Control(Control { magazine: Some(_), .. }) | Record::PageMusic(_)))
}

/// Checks `record` against the capabilities `caps` the connection's WELCOME
/// agreed (REQ-6927 item 3).
///
/// # Errors
/// [`NotAgreed`] when the record is magazine music's and `caps` has not
/// [`MAGAZINE_SOUND_CAP`]: a protocol error, on which a host closes the
/// connection.
pub fn check_record(record: &[u8], caps: u32) -> Result<(), NotAgreed> {
    if caps & MAGAZINE_SOUND_CAP == 0 && is_magazine_record(record) { Err(NotAgreed) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test reports a failure by panicking, so it unwraps results it expects to be good"
    )]

    use neoteletext_audio::record::{TYPE_CONTROL, TYPE_MAGAZINE_MUSIC, write, write_version};
    use neoteletext_audio::{MAX_RECORD, PROFILE_VERSION};

    use super::*;
    use crate::caps::{ALL_CAPS, SELECT_CAP};

    fn record(version: u8, kind: u8, body: &[u8]) -> std::vec::Vec<u8> {
        let mut out = [0_u8; MAX_RECORD];
        let length = write_version(version, kind, body, &mut out).unwrap();
        out.get(..length).unwrap().to_vec()
    }

    // REQ-6927 item 3: a magazine CONTROL record, and a page's setting, on a
    // connection without bit 26 is a protocol error; with it, and for every
    // other record, it is not.
    #[test]
    fn a_magazine_record_without_bit_26_is_a_protocol_error() {
        // A play of handle 4 for magazine 1 (0xFF01), and for page 150.
        let magazine = record(PROFILE_VERSION, TYPE_CONTROL, &[0, 0, 1, 1, 4, 0, 1, 80, 0, 0, 0, 0, 0xFF, 0x01]);
        let setting = record(PROFILE_VERSION, TYPE_MAGAZINE_MUSIC, &[0x01, 0x50, 0, 0, 0xFF]);
        let mut page = [0_u8; MAX_RECORD];
        let length = write(TYPE_CONTROL, &[0, 0, 1, 1, 4, 0, 1, 80, 0, 0, 0, 0, 0x01, 0x50], &mut page).unwrap();
        let page = page.get(..length).unwrap();
        for caps in [0, SELECT_CAP, ALL_CAPS & !MAGAZINE_SOUND_CAP] {
            assert_eq!(check_record(&magazine, caps), Err(NotAgreed), "{caps:#x}");
            assert_eq!(check_record(&setting, caps), Err(NotAgreed), "{caps:#x}");
            assert_eq!(check_record(page, caps), Ok(()), "a page's play needs no capability");
        }
        for caps in [MAGAZINE_SOUND_CAP, MAGAZINE_SOUND_CAP | SELECT_CAP] {
            assert_eq!(check_record(&magazine, caps), Ok(()));
            assert_eq!(check_record(&setting, caps), Ok(()));
        }
        // What is no record is its reader's to refuse.
        assert_eq!(check_record(&[1, 2, 3], 0), Ok(()));
    }
}
