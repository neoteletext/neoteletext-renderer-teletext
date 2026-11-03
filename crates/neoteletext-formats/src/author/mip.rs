//! The Magazine Inventory Page (clause 11.3, Figures 25 and 26, Table 24;
//! REQ-NUM-026): page mFD, sub-page S1 = 0, with a page code for each page
//! number in packets X/1 to X/14 at the position the MOT uses for it, and
//! the thirteen-bit sub-page counts of the codes that need them in packets
//! X/15 to X/24 (clause 11.3.4). Its sub-code is written as every
//! enhancement data page's, nnXs (Annex A.1; REQ-NUM-022), by
//! [`enhancement_subcode`].

use std::boxed::Box;
use std::collections::BTreeMap;

use neoteletext_core::consts::{
    DATA_BYTES, MIP_CODE_LAST_PACKET, MIP_COUNT_FIRST_INDEX, MIP_COUNT_FIRST_PACKET, MIP_COUNT_GROUP_BITS,
    MIP_COUNT_GROUP_BYTES, MIP_COUNT_GROUPS_PER_PACKET, MIP_COUNT_LAST_PACKET, MIP_NIBBLE_BITS, NIBBLE_MASK,
    PAGE_FUNCTION_MIP,
};
use neoteletext_core::limits::PAGE_MIP;
use neoteletext_core::packet::{PageCode, mip_position};
use neoteletext_core::types::{Magazine, PageAddress, PageNumber, SubCode};

use super::{AuthorError, Result, enhancement_subcode, new_page};
use crate::consts::PAGE_CODING_NIBBLES;
use crate::encode::nibble_row;
use crate::page::{PageFile, PageFunction};

/// The bits of a count below D12 (clause 11.3.4).
const COUNT_LOW_MASK: u16 = (1 << MIP_COUNT_GROUP_BITS) - 1;

/// A Magazine Inventory Page being written.
#[derive(Clone, Debug)]
pub struct Mip {
    magazine: Magazine,
    update: u8,
    codes: BTreeMap<PageNumber, (PageCode, Option<u16>)>,
}

impl Mip {
    /// An empty inventory for a magazine: every page not named reads 00,
    /// "page not in transmission", where its packet is sent.
    #[must_use]
    pub const fn new(magazine: Magazine) -> Self {
        Self { magazine, update: 0, codes: BTreeMap::new() }
    }

    /// Sets the update indicator S2 (Annex A.1).
    pub fn update_indicator(&mut self, update: u8) -> &mut Self {
        self.update = update;
        self
    }

    /// Gives page `page` the code `code`, with its number of sub-pages
    /// where the code is one counted in X/15 to X/24 (Table 24). A count
    /// is refused for any other code, and a counted code needs one that
    /// fits it: below 2^12 for a code whose D12 is 0, from 2^12 for one
    /// whose D12 is 1 (codes 51, D1 and E1).
    ///
    /// # Errors
    /// [`AuthorError::BadPosition`] for a count given to a code that is not
    /// counted, a counted code without a count, or a count outside the range
    /// its code allows.
    pub fn page(&mut self, page: PageNumber, code: PageCode, count: Option<u16>) -> Result<&mut Self> {
        match (code.is_counted(), count) {
            (false, None) => {}
            (true, Some(count)) => {
                let d12 = count >> MIP_COUNT_GROUP_BITS;
                let fits = code.count_d12().map_or(d12 == 0, |bit| bit == d12);
                if !fits {
                    return Err(AuthorError::BadPosition);
                }
            }
            _ => return Err(AuthorError::BadPosition),
        }
        self.codes.insert(page, (code, count));
        Ok(self)
    }

    /// The page file: every packet of X/1 to X/14 that holds a code other
    /// than 00, and every packet of counts that holds a count.
    ///
    /// # Errors
    /// [`AuthorError::BadPosition`] when a page's code or count falls outside
    /// the table's packets, and [`AuthorError::PageFull`] when the packets need
    /// more lines than a page file keeps.
    pub fn build(&self) -> Result<Box<PageFile>> {
        let mut rows: BTreeMap<u8, [u8; DATA_BYTES]> = BTreeMap::new();
        for (&page, &(code, _)) in &self.codes {
            let position = mip_position(page);
            if position.packet > MIP_CODE_LAST_PACKET {
                return Err(AuthorError::BadPosition);
            }
            let row = rows.entry(position.packet).or_insert([0; DATA_BYTES]);
            // Figure 26: the low nibble, then the high one.
            let nibbles = [code.0 & NIBBLE_MASK, code.0 >> MIP_NIBBLE_BITS];
            for (offset, nibble) in nibbles.into_iter().enumerate() {
                *row.get_mut(position.index.saturating_add(offset)).ok_or(AuthorError::BadPosition)? = nibble;
            }
        }
        // Clause 11.3.4: the counted codes take groups in the order of
        // Figure 26, by packet and then by byte, as the reader counts them.
        let mut counted: std::vec::Vec<(u8, usize, u16)> = self
            .codes
            .iter()
            .filter_map(|(&page, &(_, count))| count.map(|count| (page, count)))
            .map(|(page, count)| {
                let position = mip_position(page);
                (position.packet, position.index, count)
            })
            .collect();
        counted.sort_unstable();
        for (group, &(_, _, count)) in counted.iter().enumerate() {
            let packet = usize::from(MIP_COUNT_FIRST_PACKET)
                .saturating_add(group.checked_div(MIP_COUNT_GROUPS_PER_PACKET).unwrap_or(0));
            let packet = u8::try_from(packet).ok().filter(|&packet| packet <= MIP_COUNT_LAST_PACKET);
            let packet = packet.ok_or(AuthorError::PageFull)?;
            let in_packet = group.checked_rem(MIP_COUNT_GROUPS_PER_PACKET).unwrap_or(0);
            let first = MIP_COUNT_FIRST_INDEX.saturating_add(in_packet.saturating_mul(MIP_COUNT_GROUP_BYTES));
            let row = rows.entry(packet).or_insert([0; DATA_BYTES]);
            let low = count & COUNT_LOW_MASK;
            for nibble in 0..MIP_COUNT_GROUP_BYTES {
                let shift = MIP_NIBBLE_BITS.saturating_mul(nibble as u32);
                let value = (low.checked_shr(shift).unwrap_or(0) as u8) & NIBBLE_MASK;
                *row.get_mut(first.saturating_add(nibble)).ok_or(AuthorError::BadPosition)? = value;
            }
        }
        let address = PageAddress { magazine: self.magazine, page: PageNumber(PAGE_MIP) };
        let mut page = Box::new(new_page(address, SubCode::new(0)));
        page.function = Some(PageFunction { function: PAGE_FUNCTION_MIP, coding: PAGE_CODING_NIBBLES });
        let mut last_packet = 0u8;
        for (&packet, values) in &rows {
            page.set_row(usize::from(packet), &nibble_row(values));
            last_packet = packet;
        }
        // Clause 11.3.2: a MIP has one sub-page, S1 = 0.
        page.subcode = enhancement_subcode(last_packet, self.update, 0);
        Ok(page)
    }
}
