//! Composing the screen the viewer shows: the page at its level, or what
//! stands in for it.

use super::{ComposeContext, SecurityIndicator, Viewer, ViewerState};
use crate::acq::EnhancementReport;
use crate::acq::StoredPage;
use crate::compose::{compose_level, compose_level_traced};
use crate::level1::{DisplayOptions, Grid, build_grid, build_grid_traced};
use crate::trace::TraceSink;

impl Viewer {
    /// Builds the grid the viewer shows (REQ-ARCH-006): Level 1, then the
    /// pass of the presentation level, with the receiver's status area,
    /// rolling header and row 24. While holding it leaves the grid alone.
    ///
    /// At Levels 2.5 and 3.5 the result carries the enhancement processor's
    /// report. The caller records it with `Decoder::record_enhancement`
    /// once `context` no longer borrows the decoder (REQ-L25-051); the
    /// decoder counts each build once however often the page is redrawn.
    #[must_use]
    pub fn compose(&self, grid: &mut Grid, page: Option<&StoredPage>, context: &ComposeContext<'_>) -> Composed {
        self.compose_with(grid, page, context, None)
    }

    /// [`Viewer::compose`], writing the transitions of the presentation
    /// level's walk to `trace` (REQ-FSM-010), as `compose_level_traced`
    /// does for the other commands.
    #[must_use]
    pub fn compose_traced(
        &self,
        grid: &mut Grid,
        page: Option<&StoredPage>,
        context: &ComposeContext<'_>,
        trace: &mut TraceSink<'_>,
    ) -> Composed {
        self.compose_with(grid, page, context, Some(trace))
    }

    fn compose_with(
        &self,
        grid: &mut Grid,
        page: Option<&StoredPage>,
        context: &ComposeContext<'_>,
        trace: Option<&mut TraceSink<'_>>,
    ) -> Composed {
        let ComposeContext { magazine, config, rolling_header, store, row24, lookup } = *context;
        // REQ-NAV-006: while held, new receptions do not reach the screen.
        if self.machine.state == ViewerState::Holding {
            return Composed::Held;
        }
        let searching = page.is_none() || self.machine.state == ViewerState::Searching;
        let options = DisplayOptions {
            status: self.status(store),
            searching,
            rolling_header: if page.is_none() { Some(*rolling_header) } else { None },
            clock: clock_of(rolling_header, config),
            row24,
            security_indicator: self.indicator != SecurityIndicator::None || self.confirming(),
            mix: self.mix,
            boxed: self.boxed,
        };
        Composed::Built(if let Some(trace) = trace {
            build_grid_traced(grid, page, magazine, config, &options, trace);
            page.and_then(|page| compose_level_traced(grid, page, magazine, &lookup, config, &options, trace))
        } else {
            build_grid(grid, page, magazine, config, &options);
            page.and_then(|page| compose_level(grid, page, magazine, &lookup, config, &options))
        })
    }
}

/// What [`Viewer::compose`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Composed {
    /// The viewer is holding, and the grid was left alone (REQ-NAV-006).
    Held,
    /// The grid was built; at Levels 2.5 and 3.5, with the report of the
    /// faults the enhancement processor met, for the decoder's statistics.
    Built(Option<EnhancementReport>),
}

/// The clock a found page shows: the latest header's, once one has come
/// and while rolling is on (decision E.11); none, and the page keeps its
/// own, otherwise.
fn clock_of(
    header: &[u8; crate::consts::HEADER_DISPLAY_BYTES],
    config: &crate::config::Config,
) -> Option<[u8; crate::consts::CLOCK_BYTES]> {
    let clock: [u8; crate::consts::CLOCK_BYTES] =
        header.get(crate::consts::CLOCK_COLUMN - crate::consts::HEADER_DISPLAY_COLUMN..)?.try_into().ok()?;
    (config.rolling_header && clock.iter().any(|&byte| byte != crate::consts::SPACE)).then_some(clock)
}
