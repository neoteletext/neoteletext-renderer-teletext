//! The languages NeoTeletext writes and the set that writes each, as an
//! example of the generated protocol document (REQ-2181 item 6, DOC-1849):
//! read from the core's one table of languages, so that the list in the
//! document cannot drift from the engine's.

use neoteletext_core::charset::language::{LANGUAGES, LanguageSet};
use neoteletext_core::consts::DESIGNATION_GROUP_SHIFT;

use super::{Example, Field};

/// The bits of a Table 32 value below its group: the national option.
const OPTION_MASK: u8 = (1 << DESIGNATION_GROUP_SHIFT) - 1;

/// A Table 32 value as its group and option bits, as EN 300 706 Table 32
/// and the protocol document write them: `0101 110`.
fn designation(value: u8) -> String {
    format!("{:04b} {:03b}", value >> DESIGNATION_GROUP_SHIFT, value & OPTION_MASK)
}

/// The tag that names an entry exactly, such as `sr-Latn`.
fn tag(entry: &LanguageSet) -> String {
    match entry.tag_parts() {
        (language, Some(script)) => format!("{language}-{script}"),
        (language, None) => language.to_owned(),
    }
}

/// What the document says of one language: its set's designation, its
/// script, its direction, and the standard's own set where there is one.
fn described(entry: &LanguageSet) -> String {
    let direction = if entry.right_to_left() { "right to left" } else { "left to right" };
    let standard = entry
        .standard
        .filter(|&standard| standard != entry.value)
        .map_or_else(String::new, |standard| format!("; the standard's own set {}", designation(standard)));
    format!("{}: set {}, script {}, {direction}{standard}", tag(entry), designation(entry.value), entry.script)
}

/// The list: each language's tag in UTF-8, one after another, with a field
/// naming the language and saying what writes it.
pub fn example() -> Example {
    let mut bytes = Vec::new();
    let mut fields = Vec::new();
    for entry in &LANGUAGES {
        let offset = bytes.len();
        bytes.extend_from_slice(tag(entry).as_bytes());
        fields.push(Field {
            offset,
            length: bytes.len() - offset,
            bits: None,
            name: entry.name,
            value: described(entry),
        });
    }
    Example {
        name: "languages",
        caption: "Every language NeoTeletext writes, by its BCP 47 tag, with the Table 32 designation of the set that writes it",
        bytes,
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-2181 item 6: the example lists every language of the core's
    /// table, once, in the table's order, with its own set.
    #[test]
    fn the_list_is_the_core_table() {
        let example = example();
        assert_eq!(example.fields.len(), LANGUAGES.len());
        for (field, entry) in example.fields.iter().zip(&LANGUAGES) {
            assert_eq!(field.name, entry.name);
            let tag_bytes = &example.bytes[field.offset..field.offset + field.length];
            assert_eq!(tag_bytes, tag(entry).as_bytes());
            assert!(
                field.value.starts_with(&format!("{}: set {}", tag(entry), designation(entry.value))),
                "{}",
                field.value
            );
        }
    }

    /// The designation is written as Table 32's group and option bits.
    #[test]
    fn a_designation_is_its_group_and_option() {
        assert_eq!(designation((0b0101 << DESIGNATION_GROUP_SHIFT) | 0b110), "0101 110");
        assert_eq!(designation(0), "0000 000");
    }
}
