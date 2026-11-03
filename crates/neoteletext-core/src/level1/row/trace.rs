//! The trace of the row attribute machine (REQ-FSM-006, REQ-FSM-010): one
//! entry for each cell of each row, which is much output, so it is made
//! only when `Config::trace_cells` asks for it as well as `Config::trace`.
//! The untraced decoding is a separate instance of the same code, given
//! [`NoTrace`], and pays nothing for it.

use super::RowState;
use crate::consts::ATTRIBUTE_LAST;
use crate::trace::{Machine, TraceSink};

/// What the row attribute machine reports after each cell.
pub(in crate::level1) trait CellTrace {
    /// The cell at `column` held `code`; the machine went from `from` to
    /// `to`, and the cell shows `shown`.
    fn cell(&mut self, code: u8, from: &RowState, to: &RowState, shown: Shown);
}

/// What step 2 of REQ-FSM-006 chose for the cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::level1) enum Shown {
    /// A character of G0, or a G0 blast-through character in mosaic mode.
    Character,
    /// A G1 mosaic.
    Mosaic,
    /// An attribute's space.
    Space,
    /// An attribute's held mosaic.
    HeldMosaic,
}

/// No trace: the decoding every port uses.
pub(in crate::level1) struct NoTrace;

impl CellTrace for NoTrace {
    #[inline(always)]
    fn cell(&mut self, _code: u8, _from: &RowState, _to: &RowState, _shown: Shown) {}
}

/// Reports each cell of display row `row` to a trace sink.
pub(in crate::level1) struct RowTracer<'s, 't> {
    /// Where the entries go.
    pub sink: &'s mut TraceSink<'t>,
    /// The display row.
    pub row: u8,
}

impl CellTrace for RowTracer<'_, '_> {
    fn cell(&mut self, code: u8, from: &RowState, to: &RowState, shown: Shown) {
        let event = if code <= ATTRIBUTE_LAST {
            ATTRIBUTE_NAMES.get(usize::from(code)).copied().unwrap_or(CHARACTER)
        } else {
            CHARACTER
        };
        let action = match shown {
            Shown::Character => "show character",
            Shown::Mosaic => "show mosaic",
            Shown::Space => "show space",
            Shown::HeldMosaic => "show held mosaic",
        };
        self.sink.transition(Machine::RowAttributes(self.row), state_name(from), event, action, state_name(to));
    }
}

/// The event name of a code that is not an attribute.
const CHARACTER: &str = "character";

/// The Table 26 names of the 32 spacing attributes, 0/0 to 1/F.
const ATTRIBUTE_NAMES: [&str; ATTRIBUTE_LAST as usize + 1] = [
    "Alpha Black",
    "Alpha Red",
    "Alpha Green",
    "Alpha Yellow",
    "Alpha Blue",
    "Alpha Magenta",
    "Alpha Cyan",
    "Alpha White",
    "Flash",
    "Steady",
    "End Box",
    "Start Box",
    "Normal Size",
    "Double Height",
    "Double Width",
    "Double Size",
    "Mosaic Black",
    "Mosaic Red",
    "Mosaic Green",
    "Mosaic Yellow",
    "Mosaic Blue",
    "Mosaic Magenta",
    "Mosaic Cyan",
    "Mosaic White",
    "Conceal",
    "Contiguous Mosaics",
    "Separated Mosaics",
    "ESC",
    "Black Background",
    "New Background",
    "Hold Mosaics",
    "Release Mosaics",
];

/// The state's name in the trace: the mode, and whether mosaics are held,
/// which together decide what an attribute's cell shows (REQ-L1-004).
const fn state_name(state: &RowState) -> &'static str {
    match (state.mosaic, state.hold) {
        (false, false) => "alphanumeric",
        (false, true) => "alphanumeric, hold",
        (true, false) => "mosaic",
        (true, true) => "mosaic, hold",
    }
}
