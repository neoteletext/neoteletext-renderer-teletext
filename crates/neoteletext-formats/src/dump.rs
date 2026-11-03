//! Raw screen dumps (REQ-INP-017): 1000 bytes (25 rows of 40) or 960 bytes
//! (24 rows), each byte masked to 7 bits, which also reads the BBC
//! Microcomputer form with every top bit set.
//!
//! The BBC Microcomputer's mode 7 draws a double-height row only if the row
//! below repeats it, so its dumps hold the row twice, where teletext ignores
//! the row under a double-height row (decision E.15). With the
//! repeated-row option (REQ-181, Phase 2) a row that repeats, byte for byte,
//! a row above it holding Double Height is left out of the page, as that
//! ignored lower row.

use neoteletext_core::consts::{
    CODE_MASK_7BIT, COLUMNS, DOUBLE_HEIGHT, DOUBLE_HEIGHT_FIRST_ROW, DOUBLE_HEIGHT_LAST_ROW,
};

use crate::consts::{DUMP_BYTES_24, DUMP_BYTES_25};
use crate::item::{Diagnostic, DiagnosticKind, Item, Reader, offset_of};
use crate::page::PageFile;
use crate::text::copy_prefix;

/// Reads a whole dump into `page`; with `repeated_rows`, a row repeating
/// the double-height row above it is left out. Returns false when `data`
/// is neither size.
pub fn parse(data: &[u8], page: &mut PageFile, repeated_rows: bool) -> bool {
    if data.len() != DUMP_BYTES_25 && data.len() != DUMP_BYTES_24 {
        return false;
    }
    page.clear();
    // The double-height row above, whose repeat is the ignored lower row.
    let mut above: Option<[u8; COLUMNS]> = None;
    for (row, codes) in data.chunks(COLUMNS).enumerate() {
        let mut masked = [0; COLUMNS];
        for (slot, &code) in masked.iter_mut().zip(codes) {
            *slot = code & CODE_MASK_7BIT;
        }
        if repeated_rows && above.is_some_and(|above| above == masked) {
            above = None;
            continue;
        }
        let double =
            (DOUBLE_HEIGHT_FIRST_ROW..=DOUBLE_HEIGHT_LAST_ROW).contains(&row) && masked.contains(&DOUBLE_HEIGHT);
        above = double.then_some(masked);
        page.set_row(row, &masked);
    }
    true
}

/// A reader for one screen dump, held whole and read when the input ends.
#[derive(Clone, Debug)]
pub struct DumpReader {
    buffer: [u8; DUMP_BYTES_25],
    len: usize,
    total: u64,
    page: PageFile,
    repeated_rows: bool,
}

impl Default for DumpReader {
    fn default() -> Self {
        Self::new()
    }
}

impl DumpReader {
    /// A reader.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_repeated_rows(false)
    }

    /// A reader that, when `repeated_rows`, leaves out a row repeating the
    /// double-height row above it, as the BBC Microcomputer's dumps hold
    /// it (REQ-181).
    #[must_use]
    pub const fn with_repeated_rows(repeated_rows: bool) -> Self {
        Self { buffer: [0; DUMP_BYTES_25], len: 0, total: 0, page: PageFile::new(), repeated_rows }
    }
}

impl Reader for DumpReader {
    fn feed(&mut self, bytes: &[u8], _out: &mut dyn FnMut(Item<'_>)) {
        self.total = offset_of(self.total, bytes.len());
        let take = copy_prefix(self.buffer.get_mut(self.len..).unwrap_or(&mut []), bytes);
        self.len = self.len.saturating_add(take);
    }

    fn finish(&mut self, out: &mut dyn FnMut(Item<'_>)) {
        let data = self.buffer;
        let whole = u64::try_from(self.len).is_ok_and(|len| len == self.total);
        if whole && parse(data.get(..self.len).unwrap_or(&[]), &mut self.page, self.repeated_rows) {
            out(Item::Page(&self.page));
        } else {
            let size = u32::try_from(self.total).unwrap_or(u32::MAX);
            out(Item::Diagnostic(Diagnostic::at(0, DiagnosticKind::BadSize { size })));
        }
        self.len = 0;
        self.total = 0;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn read(data: &[u8]) -> (Option<PageFile>, Vec<Diagnostic>) {
        read_with(data, false)
    }

    fn read_with(data: &[u8], repeated_rows: bool) -> (Option<PageFile>, Vec<Diagnostic>) {
        let mut reader = DumpReader::with_repeated_rows(repeated_rows);
        let mut page = None;
        let mut diagnostics = Vec::new();
        let mut out = |item: Item<'_>| match item {
            Item::Page(found) => page = Some(found.clone()),
            Item::Diagnostic(diagnostic) => diagnostics.push(diagnostic),
            _ => {}
        };
        for byte in data {
            reader.feed(core::slice::from_ref(byte), &mut out);
        }
        reader.finish(&mut out);
        (page, diagnostics)
    }

    #[test]
    fn reads_both_sizes_and_masks_the_top_bit() {
        let mut data = std::vec![0xC1u8; 1000];
        data[40] = 0x8D;
        let (page, diagnostics) = read(&data);
        let page = page.unwrap();
        assert_eq!(diagnostics, []);
        assert_eq!(page.row(0).unwrap()[0], b'A');
        assert_eq!(page.row(1).unwrap()[0], 0x0D);
        assert!(page.has_row(24));
        let (page, _) = read(&data[..960]);
        assert!(!page.unwrap().has_row(24));
        let (page, diagnostics) = read(&data[..999]);
        assert!(page.is_none());
        assert_eq!(diagnostics[0].kind, DiagnosticKind::BadSize { size: 999 });
        let mut long = data.clone();
        long.push(0);
        assert!(read(&long).0.is_none());
    }

    /// REQ-181: with the option, a row repeating the double-height row above
    /// it is the ignored lower row, left out; without it, it is read as any
    /// row. A repeat of a row without Double Height, a row that differs, and
    /// a repeat under row 23 (where Double Height has no effect) are kept.
    #[test]
    fn a_repeated_row_under_a_double_height_row_is_left_out_with_the_option() {
        let mut data = std::vec![0xA0u8; 1000];
        let row = |data: &mut Vec<u8>, number: usize, text: &[u8]| {
            data[number * 40..number * 40 + text.len()].copy_from_slice(text);
        };
        // The BBC form: every top bit set; 0x8D is Double Height.
        row(&mut data, 3, b"\x8dBIG");
        row(&mut data, 4, b"\x8dBIG");
        row(&mut data, 6, b"PLAIN");
        row(&mut data, 7, b"PLAIN");
        row(&mut data, 9, b"\x8dTOP");
        row(&mut data, 10, b"\x8dTOQ");
        row(&mut data, 23, b"\x8dLOW");
        row(&mut data, 24, b"\x8dLOW");
        let (page, _) = read_with(&data, true);
        let page = page.unwrap();
        assert!(page.has_row(3) && !page.has_row(4), "the repeat is the ignored lower row");
        assert!(page.has_row(6) && page.has_row(7), "no Double Height, no lower row");
        assert!(page.has_row(9) && page.has_row(10), "a row that differs is kept");
        assert!(page.has_row(23) && page.has_row(24), "Double Height has no effect in row 23");
        let (page, _) = read(&data);
        assert!(page.unwrap().has_row(4), "without the option the repeat is read");
    }
}
