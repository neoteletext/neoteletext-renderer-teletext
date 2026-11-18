//! Tests that the constants agree with one another where a value exists in
//! two forms (REQ-GEN-014).

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use super::*;

#[test]
fn framing_code_forms_agree() {
    assert_eq!(FRAMING_CODE.reverse_bits(), FRAMING_CODE_DVB);
}

#[test]
fn page_geometry_is_consistent() {
    assert_eq!(PAGE_WIDTH, 480);
    assert_eq!(PAGE_HEIGHT, 500);
    assert_eq!(CELL_WIDTH, 2 * CELL_DOTS);
    assert_eq!(CELL_HEIGHT, 2 * CELL_FIELD_LINES);
    assert_eq!(MOSAIC_ROW_TENTHS.iter().sum::<usize>(), 10);
    assert_eq!(PACKET_BITS, PACKET_BYTES * 8);
}

/// Decision E.48: the fast phases fill a half-second exactly, and slow
/// flash keeps the 3 to 1 ratio that decision E.12 measured.
#[test]
fn flash_units_agree_with_decisions_e12_and_e48() {
    assert_eq!(FLASH_FAST_OFF_UNITS * FLASH_PHASES, FLASH_FAST_HALF_UNITS);
    let slow_off = u32::from(FLASH_UNITS - FLASH_SLOW_ON_UNITS);
    assert_eq!(u32::from(FLASH_SLOW_ON_UNITS) * FLASH_HIDDEN_TICKS, slow_off * FLASH_SHOWN_TICKS);
}

/// ANIM-1 (REQ-6529): tick n is due n × 50,000 ÷ 3 microseconds after the
/// clock's start, and the tick of a time is the last one due by then.
#[test]
fn presentation_ticks_and_microseconds_agree() {
    assert_eq!(PRESENTATION_TICKS_PER_SECOND, 60);
    assert_eq!((FLASH_SHOWN_TICKS, FLASH_HIDDEN_TICKS, FLASH_UNIT_TICKS), (57, 19, 5));
    for (tick, due) in [(0, 0), (1, 16_666), (2, 33_333), (3, 50_000), (60, 1_000_000), (57, 950_000)] {
        assert_eq!(presentation_tick_due_us(tick), due, "tick {tick}");
        assert_eq!(presentation_tick_at_us(due), tick);
        assert_eq!(presentation_tick_at_us(due.saturating_sub(1)), tick.saturating_sub(1));
    }
    assert_eq!(presentation_tick_due_us(u32::MAX), u64::from(u32::MAX) * 50_000 / 3);
    assert_eq!(presentation_tick_at_us(u64::MAX), u32::MAX);
}

/// Decision E.43: the Colour Map's transparent entry is the renderer's
/// transparent palette index.
#[test]
fn transparent_indices_agree() {
    assert_eq!(COLOUR_TRANSPARENT, PALETTE_TRANSPARENT);
    assert_eq!(usize::from(COLOUR_TRANSPARENT), CLUT_ENTRIES);
    assert_eq!(COLOUR_MAP_ENTRIES, 1 << (CLUT_SHIFT + 2));
}

/// REQ-398: the index ranges of the palette are apart.
#[test]
fn palette_ranges_do_not_overlap() {
    let flash_end = usize::from(PALETTE_FLASH_BASE) + COLOURS * COLOURS;
    assert!(flash_end <= usize::from(PALETTE_CONCEAL_BASE));
    assert_eq!(usize::from(PALETTE_CONCEAL_BASE) + COLOURS * COLOURS, PALETTE_SIZE);
}

/// REQ-GEN-014: the fixed packet layouts fit together as the standard's
/// figures lay them out, so a mistyped position is caught here.
#[test]
fn packet_layouts_fit_together() {
    // A header's eight Hamming bytes follow the address (Figure 9).
    assert_eq!(HEADER_UNITS, DATA_START);
    assert_eq!(HEADER_C11_C14, HEADER_UNITS + 7);
    // X/27/0: six page links, then the link control byte, then the
    // two checksum bytes, which end the packet (Figure 12).
    assert_eq!(EDITORIAL_LINK_START, DATA_START + 1);
    assert_eq!(EDITORIAL_LINK_CONTROL, EDITORIAL_LINK_START + EDITORIAL_LINKS * PAGE_LINK_BYTES);
    assert_eq!(EDITORIAL_LINK_CHECKSUM, EDITORIAL_LINK_CONTROL + 1);
    assert_eq!(EDITORIAL_LINK_CHECKSUM + 2, PACKET_BYTES);
    const { assert!(EDITORIAL_INDEX_LINK < EDITORIAL_LINKS) };
    // 8/30: the initial page is a page link after the designation code,
    // and the status display ends the packet (Table 18).
    assert_eq!(SERVICE_INITIAL_PAGE, DATA_START + 1);
    assert_eq!(SERVICE_NETWORK_ID, SERVICE_INITIAL_PAGE + PAGE_LINK_BYTES);
    assert_eq!(SERVICE_STATUS + STATUS_MESSAGE_BYTES, PACKET_BYTES);
    // An object link's last field is a two-byte object number (Table 21).
    assert_eq!(MOT_LINK_DEFAULT_SECOND + 2, MOT_OBJECT_LINK_BYTES);
    assert_eq!(DATA_FIRST_BYTE_NUMBER, 6);
}

/// DEC-2464: the receiver's colour indices start on a CLUT boundary, so a
/// receiver index is its base with the CLUT 0 entry in the low bits.
#[test]
fn receiver_colours_follow_the_colour_map() {
    const { assert!(RECEIVER_COLOUR_BASE & CLUT_ENTRY_MASK == 0) };
    assert_eq!(usize::from(RECEIVER_COLOUR_BASE) + COLOURS, RESOLVED_COLOURS);
}

/// REQ-158: the Hamming 24/18 layout is clause 8.3's: the data bits are
/// every position that is not a protection bit, P1 to P5 at the powers of
/// two and P6 at B24, and there is one test for each of P1 to P5.
#[test]
fn hamming_2418_layout_is_clause_8_3() {
    let protection = |shift: u32| (shift + 1).is_power_of_two() || shift == TRIPLET_P6_SHIFT;
    let mut data = (0..TRIPLET_BITS).filter(|&shift| !protection(shift));
    for &shift in &TRIPLET_DATA_SHIFTS {
        assert_eq!(data.next(), Some(shift));
    }
    assert_eq!(data.next(), None);
    let mut tests =
        (0..TRIPLET_BITS).filter(|&shift| (shift + 1).is_power_of_two()).map(|shift| (shift + 1).trailing_zeros());
    for &test in &TRIPLET_TEST_BITS {
        assert_eq!(tests.next(), Some(test));
    }
    assert_eq!(tests.next(), None);
}
