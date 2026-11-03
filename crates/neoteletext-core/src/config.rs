//! Configuration: every behaviour choice of REQUIREMENTS.md Part E as a
//! field, with documented defaults and named profiles (REQ-ARCH-008, area
//! PRF). Changing the configuration never requires re-acquiring pages.

use crate::consts::{
    DESIGNATION_LATIN_ENGLISH, FIELD_PERIOD_US, FLASH_FIELDS_HIDDEN, FLASH_FIELDS_SHOWN, MOMENTARY_REVEAL_US,
    NATIONAL_INDEX_MASK, PAGE_COMPLETION_TIMEOUT_US, RomVariantSets, SAA5246A_ROM_VARIANTS,
};

/// The presentation level (decision E.40, REQ-PRF-006). EN 300 706 clause 12
/// defines the levels as cumulative, so the variants are declared in level
/// order and compare that way.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum PresentationLevel {
    /// Level 1.0: the page is decoded from its Level 1 bytes alone, and
    /// packets X/26 are stored but not applied (REQ-L15-001).
    Level1,
    /// Level 1.5: the Level 1.5 pass of area L15 runs.
    Level15,
    /// Level 2.5: the enhancement processor of area L25 runs, without the
    /// features Tables 1, 25, 28 and 29 mark for Level 3.5 only.
    Level25,
    /// Level 3.5: the enhancement processor runs with every feature.
    Level35,
}

impl PresentationLevel {
    /// True at Levels 2.5 and 3.5, where the enhancement processor runs in
    /// place of the Level 1.5 pass (REQ-L25-001).
    #[must_use]
    pub const fn is_enhanced(self) -> bool {
        matches!(self, Self::Level25 | Self::Level35)
    }

    /// True at Level 3.5 only, for the packets, triplets and fields that
    /// Tables 1, 25, 28 and 29 tick for Level 3.5 alone (decision E.40).
    #[must_use]
    pub const fn is_level35(self) -> bool {
        matches!(self, Self::Level35)
    }

    /// The level pages are composed at in this build: this level, or the
    /// highest level below it whose feature is compiled in (decision E.61).
    #[must_use]
    pub const fn available(self) -> Self {
        match self {
            Self::Level25 | Self::Level35 if cfg!(feature = "level25") => self,
            Self::Level15 | Self::Level25 | Self::Level35 if cfg!(feature = "level15") => Self::Level15,
            _ => Self::Level1,
        }
    }
}

/// How characters that fail their parity check are shown (decision E.8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParityPolicy {
    /// Keep the last character that passed; never-good cells show a space.
    KeepLastGood,
    /// Show a space for every character that failed.
    AlwaysSpace,
    /// Show a visible error marker, for diagnostics.
    ErrorMarker,
}

/// When row 24 is displayed (decision E.23).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row24Policy {
    /// Only when X/27/0 carries the "display row 24" flag (Annex H).
    AnnexH,
    /// Whenever packet X/24 is present (the SAA5246A behaviour).
    WhenPresent,
    /// Never: the SAA5050 receivers showed 24 rows (REQ-PRF-002).
    Never,
}

/// Reveal style (REQ-NAV-007).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevealStyle {
    /// Toggled on and off.
    Latched,
    /// On for 160 milliseconds after each request, like the SAA5050.
    Momentary,
}

/// Next and previous page order (REQ-NAV-002).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepOrder {
    /// Numerical order.
    Numerical,
    /// Only pages present in the store.
    Stored,
}

/// The raster cell size (REQ-RND-002 and REQ-RND-003).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellMode {
    /// 12 by 20, the SAA5050 style.
    Saa5050,
    /// 12 by 10, the SAA5246A style.
    Saa5246a,
}

/// The national option ROM variants of the SAA5246A (REQ-PRF-003; the
/// design's Part D.5; the SAA5246A product specification, Tables 15 to 23).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum RomVariant {
    /// /E: West European (English, German, Swedish, Italian, French,
    /// Spanish).
    WestEuropean,
    /// /H: East European (Polish, German, Swedish, Serbo-Croat, Czech,
    /// Rumanian).
    EastEuropean,
    /// /T: West European and Turkish.
    WestEuropeanTurkish,
    /// /J: Yugoslav (Serbo-Croat Latin and Cyrillic, Rumanian).
    Yugoslav,
    /// /K: French and Arabic.
    FrenchArabic,
    /// /L: Arabic and Hebrew.
    ArabicHebrew,
    /// /I: Greek and Turkish.
    GreekTurkish,
    /// /S: English and Thai.
    EnglishThai,
}

impl RomVariant {
    /// Every variant, in the datasheet's order.
    pub const ALL: [Self; 8] = [
        Self::WestEuropean,
        Self::EastEuropean,
        Self::WestEuropeanTurkish,
        Self::Yugoslav,
        Self::FrenchArabic,
        Self::ArabicHebrew,
        Self::GreekTurkish,
        Self::EnglishThai,
    ];

    /// The letter the datasheet names the variant by: `E`, `H`, `T`, `J`,
    /// `K`, `L`, `I` or `S`.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Self::WestEuropean => 'E',
            Self::EastEuropean => 'H',
            Self::WestEuropeanTurkish => 'T',
            Self::Yugoslav => 'J',
            Self::FrenchArabic => 'K',
            Self::ArabicHebrew => 'L',
            Self::GreekTurkish => 'I',
            Self::EnglishThai => 'S',
        }
    }

    /// The variant a letter names, in either case.
    #[must_use]
    pub fn from_letter(letter: char) -> Option<Self> {
        Self::ALL.into_iter().find(|variant| variant.letter() == letter.to_ascii_uppercase())
    }

    /// The sets the variant's ROM holds.
    #[must_use]
    pub const fn sets(self) -> RomVariantSets {
        let [west, east, west_turkish, yugoslav, french_arabic, arabic_hebrew, greek_turkish, english_thai] =
            SAA5246A_ROM_VARIANTS;
        match self {
            Self::WestEuropean => west,
            Self::EastEuropean => east,
            Self::WestEuropeanTurkish => west_turkish,
            Self::Yugoslav => yugoslav,
            Self::FrenchArabic => french_arabic,
            Self::ArabicHebrew => arabic_hebrew,
            Self::GreekTurkish => greek_turkish,
            Self::EnglishThai => english_thai,
        }
    }

    /// The Table 32 value the variant gives a national option index, and
    /// whether the variant lacked it and gave its default language.
    #[must_use]
    pub fn designation(self, national: u8) -> (u8, bool) {
        let sets = self.sets();
        match sets.by_national.get(usize::from(national & NATIONAL_INDEX_MASK)).copied().flatten() {
            Some(value) => (value, false),
            None => (sets.default, true),
        }
    }
}

/// The decoder configuration: a plain struct with a `Default`
/// implementation and named profile constructors (REQ-GEN-011).
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate Part E decision that REQ-ARCH-008 requires to be its own setting"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    /// The presentation level (REQ-PRF-006, decision E.40). It replaces the
    /// former on and off switch for Level 1.5 (REQ-L15-001).
    pub level: PresentationLevel,
    /// Process X/26/15 like the other designation codes (decision E.2).
    pub process_x26_15: bool,
    /// Use the clause 12.3.2 row-setting rule instead of Annex A.3 (E.3).
    /// This and the single Start Box, exact framing code and out-of-order
    /// triplet settings are the alternatives of REQ-PRF-004, each
    /// independent of the profile.
    pub strict_row_setting: bool,
    /// Ignore triplets addressing positions earlier than the previous one
    /// (REQ-L15-009).
    pub ignore_out_of_order_triplets: bool,
    /// Default character set group when neither X/28/0 nor M/29/0 gives one
    /// (decision E.4), 0 to 15.
    pub default_region: u8,
    /// One fixed Table 32 designation value (group and national option)
    /// that every page uses, ignoring C12 to C14, X/28 and M/29, as a
    /// receiver with one character set in its ROM did (the SAA5050
    /// profile, REQ-PRF-002); `None` follows the page.
    pub fixed_designation: Option<u8>,
    /// The SAA5246A's national option ROM variant (REQ-PRF-003): C12 to
    /// C14 choose among that variant's sets alone, X/28 and M/29 are not
    /// read, and a combination it lacks gives its default language.
    /// `None` follows the page. [`Config::fixed_designation`] comes first.
    pub rom_variant: Option<RomVariant>,
    /// Parity failure display policy (decision E.8).
    pub parity_policy: ParityPolicy,
    /// Roll incoming headers while searching (decision E.11).
    pub rolling_header: bool,
    /// Microseconds a flashing character is shown (decision E.12).
    pub flash_shown_us: u64,
    /// Microseconds a flashing character is hidden (decision E.12).
    pub flash_hidden_us: u64,
    /// Round alphanumeric glyphs in the 12-by-20 mode (REQ-RND-012).
    pub rounding: bool,
    /// Require two consecutive Start Box codes (decision E.17).
    pub double_start_box: bool,
    /// Treat 0/0 and 1/0 as black colour attributes (decision E.18).
    pub black_colour_codes: bool,
    /// Page completion timeout in microseconds (decision E.19).
    pub page_completion_timeout_us: u64,
    /// Row 24 display rule (decision E.23).
    pub row24: Row24Policy,
    /// Accept a framing code with one wrong bit (decision E.24).
    pub framing_tolerance: bool,
    /// Store pages with hexadecimal numbers, for diagnostics (REQ-ACQ-013).
    /// The reserved numbers (mFD, mFE, mFF, 1BE, 1F0) are never stored as
    /// pages, whatever this says (REQ-NUM-010, DEC-2341): the MOT, the MIP
    /// and the Basic TOP Table are kept as received in the enhancement
    /// store, where a tool reads their bytes.
    pub store_hexadecimal_pages: bool,
    /// Microseconds after which a page not received again is stale
    /// (REQ-ACQ-012, REQ-FSM-004).
    pub stale_after_us: u64,
    /// Reveal style (REQ-NAV-007).
    pub reveal: RevealStyle,
    /// Momentary reveal timeout in microseconds.
    pub momentary_reveal_us: u64,
    /// Next and previous page order (REQ-NAV-002).
    pub step_order: StepOrder,
    /// Raster cell size.
    pub cell_mode: CellMode,
    /// Report state machine transitions (REQ-FSM-010).
    pub trace: bool,
    /// With `trace`, also report each cell's step of the row attribute
    /// machine (REQ-FSM-006): a thousand entries a page, so asked for
    /// apart (`neottx -vvv`).
    pub trace_cells: bool,
    /// Show row 0's status area even on pages with C7 set (REQ-L1-014).
    pub status_on_suppressed_header: bool,
    /// Apply NeoTeletext's row set triplet, column mode 01010, which reads
    /// a row's own codes in another set from its column (DEC-2179,
    /// REQ-2180). Off, the mode is reserved, as EN 300 706 Table 29 has it.
    pub row_set_triplets: bool,
}

/// Flash shown and hidden durations derived from decision E.12's field
/// counts at 50 fields per second.
const FLASH_SHOWN_US: u64 = FLASH_FIELDS_SHOWN * FIELD_PERIOD_US;
const FLASH_HIDDEN_US: u64 = FLASH_FIELDS_HIDDEN * FIELD_PERIOD_US;

/// Microseconds a page may go unreceived before it is stale: 60 seconds, a
/// generous multiple of the longest normal magazine cycle.
const DEFAULT_STALE_AFTER_US: u64 = 60_000_000;

impl Config {
    /// The default profile, "EN 300 706" (REQ-PRF-001).
    #[must_use]
    pub const fn en_300_706() -> Self {
        Self {
            // Decision E.40: the default profile presents at Level 3.5.
            level: PresentationLevel::Level35,
            process_x26_15: true,
            strict_row_setting: false,
            ignore_out_of_order_triplets: false,
            default_region: 0,
            fixed_designation: None,
            rom_variant: None,
            parity_policy: ParityPolicy::KeepLastGood,
            rolling_header: true,
            flash_shown_us: FLASH_SHOWN_US,
            flash_hidden_us: FLASH_HIDDEN_US,
            rounding: true,
            double_start_box: true,
            black_colour_codes: true,
            page_completion_timeout_us: PAGE_COMPLETION_TIMEOUT_US,
            row24: Row24Policy::AnnexH,
            framing_tolerance: true,
            store_hexadecimal_pages: false,
            stale_after_us: DEFAULT_STALE_AFTER_US,
            reveal: RevealStyle::Latched,
            momentary_reveal_us: MOMENTARY_REVEAL_US,
            step_order: StepOrder::Numerical,
            cell_mode: CellMode::Saa5050,
            trace: false,
            trace_cells: false,
            status_on_suppressed_header: false,
            row_set_triplets: true,
        }
    }

    /// The "SAA5050" profile (REQ-PRF-002): Level 1.0, a fixed English
    /// character set, row 24 never displayed, momentary reveal.
    #[must_use]
    pub const fn saa5050() -> Self {
        let mut config = Self::en_300_706();
        config.level = PresentationLevel::Level1;
        // The chip's ROM held Latin English alone: Table 32 group 0000,
        // national option 000 (REQ-PRF-002).
        config.fixed_designation = Some(DESIGNATION_LATIN_ENGLISH);
        // Decision E.18: the SAA5050 has no black colour codes.
        config.black_colour_codes = false;
        config.row24 = Row24Policy::Never;
        config.reveal = RevealStyle::Momentary;
        config
    }

    /// The "SAA5246A" profile (REQ-PRF-003): Level 1.5, as the chip
    /// presented (decision E.40).
    #[must_use]
    pub const fn saa5246a() -> Self {
        let mut config = Self::en_300_706();
        config.level = PresentationLevel::Level15;
        config.process_x26_15 = false;
        config.row24 = Row24Policy::WhenPresent;
        config.cell_mode = CellMode::Saa5246a;
        config.rounding = false;
        // The West European chip unless another variant is chosen
        // (DEC-2430).
        config.rom_variant = Some(RomVariant::WestEuropean);
        config
    }

    /// The profile with the given name: `en300706`, `saa5050` or
    /// `saa5246a`.
    #[must_use]
    pub fn profile(name: &str) -> Option<Self> {
        match name {
            "en300706" | "default" => Some(Self::en_300_706()),
            "saa5050" => Some(Self::saa5050()),
            "saa5246a" => Some(Self::saa5246a()),
            _ => None,
        }
    }
}

// REQ-GEN-011: the default is the "EN 300 706" profile.
impl Default for Config {
    fn default() -> Self {
        Self::en_300_706()
    }
}

#[cfg(test)]
mod tests;
