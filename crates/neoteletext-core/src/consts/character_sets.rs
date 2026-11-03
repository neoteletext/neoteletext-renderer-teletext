//! Character sets (clause 15): the national option positions of Table 36,
//! the right guillemet of Table 37, the G3 set of Table 48 and the undefined
//! positions of the G3 and Latin G2 sets.

use super::spacing_attributes::SPACE;

/// The 13 national option positions of the Latin G0 set (Table 36).
pub const NATIONAL_POSITIONS: [u8; 13] = [0x23, 0x24, 0x40, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x60, 0x7B, 0x7C, 0x7D, 0x7E];
/// The right guillemet "»" in the Latin G2 set (Table 37, position 3/B),
/// the receiver's mark for a link leaving the service (REQ-NAV-011).
pub const G2_RIGHT_GUILLEMET: u8 = 0x3B;

/// The G3 set occupies codes 2/0 to 7/F (Table 48).
pub const G3_FIRST_CODE: u8 = SPACE;
/// Positions 2/0 to 7/F: six columns of sixteen (Table 48).
pub const G3_CODES: usize = 96;

/// The undefined positions of the G3 set: 6/E, 6/F, 7/E and 7/F (Table 25,
/// its "7/E and 7/E" read as 7/E and 7/F, decision E.54).
pub const G3_UNDEFINED: [u8; 4] = [0x6E, 0x6F, 0x7E, 0x7F];
/// The Latin G2 positions that should not be transmitted: 5/9, 5/A, 5/B and
/// 6/5 (Table 37 note 5, decision E.50).
pub const LATIN_G2_UNDEFINED: [u8; 4] = [0x59, 0x5A, 0x5B, 0x65];

/// A Table 32 designation value from its group (bits 3 to 6) and national
/// option index (bits 0 to 2, C12 C13 C14 in that order, as Table 32 and
/// the SAA5246A datasheet both write them).
const fn table_32(group: u8, national: u8) -> u8 {
    (group << super::DESIGNATION_GROUP_SHIFT) | national
}

/// One national option ROM variant of the SAA5246A: the Table 32 value its
/// ROM gives for each of the eight values of C12 C13 C14, `None` where the
/// variant has no set for that value, and the value of the language every
/// unsupported combination defaults to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RomVariantSets {
    /// By national option index (C12 C13 C14).
    pub by_national: [Option<u8>; 8],
    /// The variant's default language.
    pub default: u8,
}

/// The eight national option ROM variants of the SAA5246A, in the order
/// /E, /H, /T, /J, /K, /L, /I, /S (Philips SAA5246A product specification,
/// January 1993, Tables 15 to 23, each with its note "Other combinations
/// default to ..."). Each language is written as the EN 300 706 Table 32
/// entry holding the same set. Choices where the datasheet is unclear or
/// the engine has no set are DEC-2430's: Table 18 (/J) prints C12 C13 C14
/// as 011 for five languages at once, so only Serbo-Croat Latin (010),
/// Serbo-Croatian Cyrillic (100) and Rumanian (111, as Table 16 gives it)
/// are taken; and /S's Thai (010) has no set in the engine, so it defaults
/// to English.
pub const SAA5246A_ROM_VARIANTS: [RomVariantSets; 8] = {
    const fn sets(by_national: [Option<u8>; 8], default: u8) -> RomVariantSets {
        RomVariantSets { by_national, default }
    }
    const WEST: u8 = 0b0000;
    const CENTRAL: u8 = 0b0001;
    const SOUTH: u8 = 0b0010;
    const SOUTH_EAST: u8 = 0b0011;
    const CYRILLIC: u8 = 0b0100;
    const TURKISH_GREEK: u8 = 0b0110;
    const LATIN_ARABIC: u8 = 0b1000;
    const HEBREW_ARABIC: u8 = 0b1010;
    [
        // /E, Table 15: English, German, Swedish, Italian, French, Spanish.
        sets(
            [
                Some(table_32(WEST, 0b000)),
                Some(table_32(WEST, 0b001)),
                Some(table_32(WEST, 0b010)),
                Some(table_32(WEST, 0b011)),
                Some(table_32(WEST, 0b100)),
                Some(table_32(WEST, 0b101)),
                None,
                None,
            ],
            table_32(WEST, 0b000),
        ),
        // /H, Table 16: Polish, German, Swedish, Serbo-Croat, Czech,
        // Rumanian; default German.
        sets(
            [
                Some(table_32(CENTRAL, 0b000)),
                Some(table_32(CENTRAL, 0b001)),
                Some(table_32(CENTRAL, 0b010)),
                None,
                None,
                Some(table_32(SOUTH_EAST, 0b101)),
                Some(table_32(CENTRAL, 0b110)),
                Some(table_32(SOUTH_EAST, 0b111)),
            ],
            table_32(CENTRAL, 0b001),
        ),
        // /T, Table 17: English, German, Italian, French, Spanish, Turkish.
        sets(
            [
                Some(table_32(SOUTH, 0b000)),
                Some(table_32(SOUTH, 0b001)),
                None,
                Some(table_32(SOUTH, 0b011)),
                Some(table_32(SOUTH, 0b100)),
                Some(table_32(SOUTH, 0b101)),
                Some(table_32(SOUTH, 0b110)),
                None,
            ],
            table_32(SOUTH, 0b000),
        ),
        // /J, Table 18: Serbo-Croat Latin, Serbo-Croatian Cyrillic,
        // Rumanian; default Serbo-Croat Latin.
        sets(
            [
                None,
                None,
                Some(table_32(SOUTH_EAST, 0b101)),
                None,
                Some(table_32(CYRILLIC, 0b000)),
                None,
                None,
                Some(table_32(SOUTH_EAST, 0b111)),
            ],
            table_32(SOUTH_EAST, 0b101),
        ),
        // /K, Table 19: French, Arabic; default French.
        sets(
            [
                None,
                None,
                None,
                None,
                Some(table_32(LATIN_ARABIC, 0b100)),
                None,
                None,
                Some(table_32(LATIN_ARABIC, 0b111)),
            ],
            table_32(LATIN_ARABIC, 0b100),
        ),
        // /L, Table 20: Hebrew with English, Arabic; default Hebrew.
        sets(
            [
                None,
                None,
                None,
                None,
                None,
                Some(table_32(HEBREW_ARABIC, 0b101)),
                None,
                Some(table_32(HEBREW_ARABIC, 0b111)),
            ],
            table_32(HEBREW_ARABIC, 0b101),
        ),
        // /I, Table 21: Turkish, Greek; default Turkish.
        sets(
            [
                None,
                None,
                None,
                None,
                None,
                None,
                Some(table_32(TURKISH_GREEK, 0b110)),
                Some(table_32(TURKISH_GREEK, 0b111)),
            ],
            table_32(TURKISH_GREEK, 0b110),
        ),
        // /S, Tables 22 and 23: English; Thai (010) has no set here.
        sets([Some(table_32(WEST, 0b000)), None, None, None, None, None, None, None], table_32(WEST, 0b000)),
    ]
};
