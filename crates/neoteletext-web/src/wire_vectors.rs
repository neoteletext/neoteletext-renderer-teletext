//! The wire's own bytes (`neoteletext-common/wire-vectors.tsv`, DOC-1963,
//! DEC-1759 lesson 3). Every program that speaks the protocol
//! runs this one table, as it runs the address map's. The master is in
//! neoteletext-common, and the commit gate compares this copy with it.

/// The table, as `neoteletext-common/wire-vectors.tsv` says how to read it.
pub const TABLE: &str = include_str!("wire_vectors.tsv");

/// One row of the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vector<'a> {
    /// What sort of fact it is: `version`, `close`, `input`, `refused` or
    /// `record`, and those of the page rule table and animation
    /// (page-rules.md RULE-85): `rules`, `rule_event`, `page_state`,
    /// `rules_fault`, `records` and `object`.
    pub kind: &'a str,
    /// Its name.
    pub name: &'a str,
    /// Its value: a number, a name, or bytes in hexadecimal; for `records`,
    /// several packets' bytes with a space between.
    pub value: &'a str,
    /// What it means.
    pub meaning: &'a str,
}

/// Every row, in order: a line that does not have the four columns is no row.
pub fn vectors() -> impl Iterator<Item = Vector<'static>> {
    TABLE.lines().filter(|line| !line.starts_with('#')).filter_map(|line| {
        let mut columns = line.split('\t');
        let vector =
            Vector { kind: columns.next()?, name: columns.next()?, value: columns.next()?, meaning: columns.next()? };
        columns.next().is_none().then_some(vector)
    })
}

/// A row's hexadecimal value as bytes, or `None` when it is not even digits.
#[must_use]
pub fn bytes_of(hexadecimal: &str) -> Option<([u8; MOST_VECTOR_BYTES], usize)> {
    let mut bytes = [0_u8; MOST_VECTOR_BYTES];
    if !hexadecimal.len().is_multiple_of(2) || hexadecimal.len() / 2 > MOST_VECTOR_BYTES {
        return None;
    }
    for (slot, pair) in bytes.iter_mut().zip(hexadecimal.as_bytes().chunks(2)) {
        let pair = core::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(pair, HEX_RADIX).ok()?;
    }
    Some((bytes, hexadecimal.len() / 2))
}

/// The room for the longest value: a `rules` row's table. The longest the
/// table holds is the default table of page-rules.md RULE-53 with its
/// wheel rule, 229 bytes; `every_value_fits` holds that each row fits.
pub const MOST_VECTOR_BYTES: usize = 256;
const _: () = assert!(MOST_VECTOR_BYTES >= neoteletext_core::consts::PACKET_BYTES);
/// Hexadecimal.
const HEX_RADIX: u32 = 16;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row's bytes fit [`MOST_VECTOR_BYTES`], a `records` row's
    /// packet by packet, so no reader of the table meets a row it cannot
    /// hold.
    #[test]
    fn every_value_fits() {
        const BYTES: [&str; 8] =
            ["input", "refused", "record", "rules", "rule_event", "page_state", "rules_fault", "object"];
        let mut read = 0;
        for vector in vectors() {
            if vector.kind == "records" {
                assert!(vector.value.split(' ').all(|packet| bytes_of(packet).is_some()), "{}", vector.name);
            } else if BYTES.contains(&vector.kind) {
                assert!(bytes_of(vector.value).is_some(), "{}", vector.name);
            } else {
                continue;
            }
            read += 1;
        }
        assert!(read > 0);
    }
}
