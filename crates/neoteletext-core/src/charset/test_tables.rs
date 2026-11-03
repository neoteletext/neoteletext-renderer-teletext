//! The builders the character sets' tests share: designation values,
//! sets and the defined entries of Tables 32 and 33.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `panic!` on outcomes it rules out, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use crate::charset::*;
use crate::charset::code_table::base_unicode;
use crate::consts::DESIGNATION_GROUP_SHIFT as GROUP_SHIFT;

pub(super) const ALL_CODES: core::ops::RangeInclusive<u8> = 0x20..=0x7F;

pub(super) const fn latin_set(national: Option<NationalOption>) -> CharSet {
    CharSet::G0(G0 { set: G0Set::Latin, national })
}

pub(super) fn g0(set: G0Set) -> CharSet {
    CharSet::G0(G0 { set, national: None })
}

/// The text of a set's 96 characters, for whole-row comparisons.
pub(super) fn row(set: CharSet, first: u8) -> std::string::String {
    (first..first + 16).map(|code| base_unicode(set, code)).collect()
}

/// A Table 32 or 33 value from its group and national option index.
pub(super) fn value(group: u8, index: u8) -> u8 {
    group << GROUP_SHIFT | index
}

pub(super) fn latin_g0(national: NationalOption) -> G0 {
    G0 { set: G0Set::Latin, national: Some(national) }
}
