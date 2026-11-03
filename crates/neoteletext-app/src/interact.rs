//! Pieces of interaction that every viewer shares: the messages the
//! receiver draws in row 24.

use neoteletext_core::consts::{ALPHA_YELLOW, COLUMNS, SPACE};

/// A receiver-drawn row 24 message, in yellow, as 7-bit codes.
#[must_use]
pub fn message_row(text: &str) -> [u8; COLUMNS] {
    let mut row = [SPACE; COLUMNS];
    row[0] = ALPHA_YELLOW;
    for (slot, byte) in row.iter_mut().skip(1).zip(text.bytes()) {
        *slot = if byte.is_ascii() && byte >= SPACE { byte } else { SPACE };
    }
    row
}

/// The instruction that opens the question of a link to another service.
pub const LEAVE_INSTRUCTION: &str = "Same key: ";
/// What joins the service's name to its origin.
const LEAVE_JOIN: &str = " at ";
/// What stands for the start of an origin too long for the row.
const CUT_MARK: &str = "..";
/// The origin's scheme that goes unsaid: a secure origin is the expected
/// one, and plain HTTP is what a viewer must see (REQ-SEC-008).
const SECURE_SCHEME: &str = "https://";

/// Row 24 while a link to another service waits for its key again
/// (REQ-288, DEC-2454): the instruction, then the service's name as its
/// descriptor gives it, then its origin, `Same key: Weather at
/// weather.example`, in the 39 columns after the colour. The origin keeps
/// its scheme when it is not `https`. When the row cannot hold them all the
/// name is cut from its end, and is left out with its join when even the
/// origin alone does not fit; the origin is never cut at its end, which
/// says whose site it is, and is cut at its start, marked `..`, only when it
/// alone is longer than the row's room.
#[must_use]
pub fn leave_prompt(name: &str, origin: &str) -> [u8; COLUMNS] {
    let room = COLUMNS.saturating_sub(1).saturating_sub(LEAVE_INSTRUCTION.len());
    let origin = origin.strip_prefix(SECURE_SCHEME).unwrap_or(origin);
    // Characters outside printable ASCII are drawn as spaces, one each.
    let printable = |text: &str| -> String {
        text.chars()
            .map(|character| if character.is_ascii_graphic() || character == ' ' { character } else { ' ' })
            .collect()
    };
    let (name, origin) = (printable(name.trim()), printable(origin));
    let text = if origin.len() > room {
        let keep = room.saturating_sub(CUT_MARK.len());
        format!("{CUT_MARK}{}", origin.get(origin.len().saturating_sub(keep)..).unwrap_or_default())
    } else {
        let for_name = room.saturating_sub(origin.len()).saturating_sub(LEAVE_JOIN.len());
        let name = name.get(..name.len().min(for_name)).unwrap_or_default().trim_end();
        if name.is_empty() { origin } else { format!("{name}{LEAVE_JOIN}{origin}") }
    };
    message_row(&format!("{LEAVE_INSTRUCTION}{text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_yellow_and_printable() {
        let row = message_row("Ready\u{7}");
        assert_eq!(row[0], ALPHA_YELLOW);
        assert_eq!(&row[1..6], b"Ready");
        assert_eq!(row[6], SPACE, "control characters are not drawn");
    }

    fn text(row: [u8; COLUMNS]) -> String {
        String::from_utf8_lossy(&row[1..]).trim_end().to_owned()
    }

    // Verifies REQ-288: the row names the service and its origin with the
    // whole instruction, keeping a scheme other than https.
    #[test]
    fn the_question_names_the_service_and_its_origin() {
        assert_eq!(text(leave_prompt("Weather", "https://weather.example")), "Same key: Weather at weather.example");
        assert_eq!(text(leave_prompt("News", "http://news.example")), "Same key: News at http://news.example");
    }

    // Verifies REQ-288: at the longest name and a long origin the name is
    // cut, then left out, and the origin's end is never cut.
    #[test]
    fn a_long_name_gives_way_to_the_origin() {
        let long_name = "A".repeat(COLUMNS);
        let row = text(leave_prompt(&long_name, "https://weather.example"));
        assert_eq!(row.len(), COLUMNS - 1, "{row}");
        assert!(row.starts_with(LEAVE_INSTRUCTION) && row.ends_with(" at weather.example"), "{row}");
        let origin = "https://a-very-long-host-name.example";
        assert_eq!(text(leave_prompt("Weather", origin)), "Same key: a-very-long-host-name.example");
        let longer = "https://the-very-longest-of-host-names.example.org";
        let row = text(leave_prompt("Weather", longer));
        assert!(row.starts_with("Same key: ..") && row.ends_with("names.example.org"), "{row}");
        assert_eq!(row.len(), COLUMNS - 1);
    }
}
