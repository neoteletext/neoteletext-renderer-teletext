//! The text of an `OL` line (REQ-INP-013): the 8-bit and escape forms of
//! control codes, and the Double Height alias of row text.

use neoteletext_core::consts::{CODE_MASK_7BIT, DOUBLE_HEIGHT, ESC, TTI_DOUBLE_HEIGHT_ALIAS, TTI_ESCAPE_OFFSET};

use crate::consts::TTI_PACKED_CHARACTERS;

/// Characters of a packed enhancement line: the designation code and 39.
pub(super) const PACKED_LINE: usize = 1 + TTI_PACKED_CHARACTERS;

/// Codes decoded from the text of an `OL` line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecodedText {
    /// The codes, 7-bit, padded with spaces for rows.
    pub codes: [u8; PACKED_LINE],
    /// Codes found.
    pub count: usize,
    /// More codes than fit.
    pub overflow: bool,
    /// An escape byte ended the line.
    pub bad_escape: bool,
}

/// Decodes the text of an `OL` line (REQ-INP-013). Control codes may be
/// written as the byte plus 0x80, or as ESC followed by the code plus 0x40.
/// With `row_text`, a raw 0x10 is Double Height, as the MRG format
/// specifies, and 0x90 is still code 0x10; packed enhancement lines take no
/// such alias. `capacity` is how many codes the line may hold, at most
/// a packed line's; a larger value is taken as that, and codes past it
/// set the overflow flag.
#[must_use]
pub fn decode_text(text: &[u8], row_text: bool, capacity: usize) -> DecodedText {
    let mut result = DecodedText { codes: [0; PACKED_LINE], count: 0, overflow: false, bad_escape: false };
    let capacity = capacity.min(PACKED_LINE);
    let mut bytes = text.iter();
    while let Some(&byte) = bytes.next() {
        let code = if byte == ESC {
            if let Some(&next) = bytes.next() {
                next.wrapping_sub(TTI_ESCAPE_OFFSET) & CODE_MASK_7BIT
            } else {
                result.bad_escape = true;
                break;
            }
        } else {
            // The alias is the raw byte alone: 0x90 is code 0x10 written
            // plus 0x80, so it is tested before the top bit goes (DEC-2362).
            if row_text && byte == TTI_DOUBLE_HEIGHT_ALIAS { DOUBLE_HEIGHT } else { byte & CODE_MASK_7BIT }
        };
        if result.count >= capacity {
            result.overflow = true;
            break;
        }
        if let Some(slot) = result.codes.get_mut(result.count) {
            *slot = code;
        }
        result.count = result.count.saturating_add(1);
    }
    result
}
