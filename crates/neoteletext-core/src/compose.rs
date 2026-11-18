//! Composing a page at the configured presentation level (REQ-PRF-006,
//! layer L3): the pass of the level applied to the page's Level 1 grid.
//! It is separate from the viewer because a port without the viewer still
//! composes pages (decision E.61). A level that is not compiled in falls
//! back to the highest compiled level below it.

use crate::acq::{EnhancementLookup, EnhancementReport, MagazineData, StoredPage};
use crate::config::{Config, PresentationLevel};
#[cfg(feature = "level15")]
use crate::level1::PageRows;
#[cfg(feature = "level25")]
use crate::level1::Size;
use crate::level1::{DisplayOptions, Grid};
use crate::sprites::{Placements, SpriteCells};
use crate::trace::TraceSink;
use crate::types::CellRect;

/// Applies the pass of the presentation level to a page's Level 1 grid
/// (REQ-PRF-006): nothing at Level 1, the Level 1.5 pass at Level 1.5, and
/// the enhancement processor at Levels 2.5 and 3.5 (REQ-L25-001), as
/// EN 300 706 clauses 10.1 and 10.2 divide the levels (DEC-564); a level
/// not compiled in falls back first (decision E.61). The
/// level is read on every build, so changing it never re-acquires a page.
/// At Levels 2.5 and 3.5 it returns the processor's report of the faults it
/// met, for the caller's statistics and trace (REQ-L25-051).
pub fn compose_level(
    grid: &mut Grid,
    page: &StoredPage,
    magazine: &MagazineData,
    lookup: &EnhancementLookup<'_>,
    config: &Config,
    options: &DisplayOptions,
) -> Option<EnhancementReport> {
    let _ = (&grid, page, magazine, lookup, options);
    let report = match config.level.available() {
        PresentationLevel::Level1 => None,
        #[cfg(feature = "level15")]
        PresentationLevel::Level15 => {
            let rows = PageRows::of(page, config, options);
            crate::level15::apply(grid, &page.enhancements, page.enhancements_present, config, rows);
            None
        }
        #[cfg(feature = "level25")]
        PresentationLevel::Level25 | PresentationLevel::Level35 => {
            Some(crate::enhance::apply(grid, page, magazine, lookup, config, options))
        }
        #[allow(
            unreachable_patterns,
            reason = "`available` returns only compiled levels, so with every level compiled in this arm cannot be reached; it serves builds that leave a level out"
        )]
        _ => None,
    };
    // REQ-CHR-015: letters are joined from what the cells finally hold.
    #[cfg(feature = "extended-sets")]
    crate::joining::join(grid);
    report
}

/// As [`compose_level`], and when `Config::trace` is on, reports the
/// transitions of the walk of the level to `trace` (REQ-FSM-010): the
/// Level 1.5 triplet walk (REQ-FSM-007), or the page-level run of the
/// enhancement walk (REQ-FSM-011).
pub fn compose_level_traced(
    grid: &mut Grid,
    page: &StoredPage,
    magazine: &MagazineData,
    lookup: &EnhancementLookup<'_>,
    config: &Config,
    options: &DisplayOptions,
    trace: &mut TraceSink<'_>,
) -> Option<EnhancementReport> {
    let _ = (&grid, page, magazine, lookup, options, &trace);
    let report = match config.level.available() {
        PresentationLevel::Level1 => None,
        #[cfg(feature = "level15")]
        PresentationLevel::Level15 => {
            let rows = PageRows::of(page, config, options);
            crate::level15::apply_traced(grid, &page.enhancements, page.enhancements_present, config, rows, trace);
            None
        }
        #[cfg(feature = "level25")]
        PresentationLevel::Level25 | PresentationLevel::Level35 => {
            Some(crate::enhance::apply_traced(grid, page, magazine, lookup, config, options, trace))
        }
        #[allow(
            unreachable_patterns,
            reason = "`available` returns only compiled levels, so with every level compiled in this arm cannot be reached; it serves builds that leave a level out"
        )]
        _ => None,
    };
    #[cfg(feature = "extended-sets")]
    crate::joining::join(grid);
    report
}

/// What [`compose_sprites`] drew (animation.md ANIM-8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpritesDrawn {
    /// The cells that differ from the page's own rendering: what the
    /// sprites changed. A caller that gives looks to controls and links
    /// afterwards leaves these cells alone, since sprites lie over the
    /// page (ANIM-12).
    pub cells: SpriteCells,
    /// The smallest rectangle of cells holding every cell a sprite may
    /// write now, for the damage of ANIM-4
    /// (`render::DamageTracker::damage`); `None` when no sprite is on the
    /// page.
    pub covered: Option<CellRect>,
    /// The report of the build with the sprites' objects, when sprites
    /// were drawn: its faults count a sprite's absent object too.
    pub report: Option<EnhancementReport>,
}

impl SpritesDrawn {
    /// Nothing drawn.
    pub const NONE: Self = Self { cells: SpriteCells::NONE, covered: None, report: None };
}

/// Composes a page's sprites over its own rendering (ANIM-8, ANIM-12;
/// REQ-6529). `page_grid` is the page's own grid, layer 0, as
/// [`compose_level`] left it, which the caller keeps apart and builds again
/// only when the page's own content changes; `grid` becomes that grid with
/// each shown sprite of `sprites` drawn over it, in order of layer, then
/// order, then number, by invoking the object of its frame in force as an
/// Object Invocation triplet at its origin would (EN 300 706 clause 13).
///
/// - A sprite writes only the cells of its width and height from its
///   origin, inside columns 0 to 39 and rows 1 to 23: every cell outside
///   the sprites is the page's own, cell for cell.
/// - An object that is absent draws nothing; a DRCS character is never
///   drawn (REQ-2196).
/// - Below Presentation Level 2.5, or without objects compiled in
///   (decision E.71), no sprite is drawn and `grid` is the page's own.
///
/// `page`, `magazine`, `lookup`, `config` and `options` are those the page
/// grid was built with. The objects are read from the page's own packets
/// and from the object pages it links, in the store (ANIM-17).
#[allow(
    clippy::too_many_arguments,
    reason = "the arguments of `compose_level`, with the page's own grid and its sprites"
)]
pub fn compose_sprites(
    grid: &mut Grid,
    page_grid: &Grid,
    page: &StoredPage,
    magazine: &MagazineData,
    lookup: &EnhancementLookup<'_>,
    config: &Config,
    options: &DisplayOptions,
    sprites: &Placements,
) -> SpritesDrawn {
    *grid = *page_grid;
    let _ = (page, magazine, lookup, options);
    let covered = sprites.covered();
    if covered.is_none() || !config.level.available().is_enhanced() || !cfg!(feature = "objects") {
        return SpritesDrawn::NONE;
    }
    #[cfg(feature = "level25")]
    {
        let report = crate::enhance::apply_sprites(grid, page, magazine, lookup, config, options, sprites.as_slice());
        #[cfg(feature = "extended-sets")]
        crate::joining::join(grid);
        let mut cells = SpriteCells::NONE;
        let rows = grid.cells.iter_mut().zip(&page_grid.cells).enumerate();
        for (row, (drawn, own)) in rows {
            for (column, (drawn, own)) in drawn.iter_mut().zip(own).enumerate() {
                if !sprites.covers(row, column) {
                    *drawn = *own;
                } else if drawn != own {
                    cells.insert(row, column);
                }
            }
        }
        // The page-wide state is the page's own: a sprite sets no screen or
        // row colour. Only the rows holding double height are found again.
        grid.designation = page_grid.designation;
        grid.designation_fallback = page_grid.designation_fallback;
        grid.colour_map = page_grid.colour_map;
        grid.screen_colour = page_grid.screen_colour;
        grid.row_colours = page_grid.row_colours;
        grid.double_height_rows = 0;
        for (row, row_cells) in grid.cells.iter().enumerate() {
            if row_cells.iter().any(|cell| cell.size == Size::DoubleTop) {
                grid.double_height_rows |= 1u32.checked_shl(row as u32).unwrap_or(0);
            }
        }
        SpritesDrawn { cells, covered, report: Some(report) }
    }
    #[cfg(not(feature = "level25"))]
    SpritesDrawn::NONE
}
