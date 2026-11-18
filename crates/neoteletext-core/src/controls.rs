//! Controls (decision E.127, REQ-INT-117 to REQ-INT-119): the cells a page
//! answers itself, such as its buttons and the rows of its lists, declared
//! by NeoTeletext's control triplet. That is column mode 00101, which
//! Table 29 of EN 300 706 reserves and every decoder skips (REQ-L25-052),
//! so a page that declares controls looks the same everywhere. A receiver
//! that takes a pointer shows each control as it shows a link, and gives a
//! control or a link under the pointer one of two looks, made only of
//! what Level 2.5 already does to colours.
//!
//! A control triplet's address is its first column and its data its
//! width, 1 to 40 (D5 to D0); its row is the active row the triplet falls
//! on (REQ-L25-003). D6 set joins the run to the control on the row above
//! at the same column and width, so a control may be a rectangle.
//!
//! Controls use column mode 00101 only. Column mode 01010 was a pointer
//! area until 28 October 2026 (decision E.128, withdrawn by decision E.133,
//! REQ-RCH-037; REQ-INT-120 withdrawn); it is now NeoTeletext's row set
//! triplet (DEC-2179, REQ-2180, `consts::MODE_ROW_SET`), which the
//! enhancement processor reads, and it declares no control.

use crate::acq::StoredPage;
use crate::consts::{
    ADJACENT_CLUT_FLIP, COLUMNS, CONTROL_JOINED, CONTROL_WIDTH_MASK, LOCAL_TRIPLETS, MODE_CONTROL, PACKET_FIRST_ROW,
    PACKET_LAST_TEXT_ROW, ROW_ADDRESS_LAST,
};
use crate::level1::{Cell, Grid};
use crate::objects::{Body, Fetch, row_of};
use crate::packet::TripletFunction;
use crate::sprites::{Animation, ForcedLook, SpriteCells, Target};
use crate::types::{CellRect, DotRect, FixedVec};

/// The most controls a page has: a control triplet beyond them is dropped
/// (REQ-INT-118). A page's 208 local triplets could declare more, one a
/// triplet, but a page of 23 rows holds no more distinct things to press
/// than a list down it and a row of buttons.
pub const MAX_CONTROLS: usize = 32;
/// The first row a control may lie on: row 0 is the header, as a page's
/// `LT` links begin at row 1 (TTI Link Extension section E.1.2).
pub const FIRST_CONTROL_ROW: u8 = PACKET_FIRST_ROW;
/// The last: row 24 is the Fastext row, whose prompts the receiver draws.
pub const LAST_CONTROL_ROW: u8 = PACKET_LAST_TEXT_ROW;

/// One control: a rectangle of cells, as wide as its triplets say and as
/// tall as the rows they join.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Control {
    /// Its top row, 1 to 23.
    pub row: u8,
    /// Its first column.
    pub column: u8,
    /// Cells across, 1 to 40.
    pub width: u8,
    /// Rows down, at least 1.
    pub height: u8,
}

impl Control {
    const NONE: Self = Self { row: 0, column: 0, width: 0, height: 0 };

    /// Its bottom row.
    #[must_use]
    pub const fn last_row(self) -> u8 {
        self.row.saturating_add(self.height.saturating_sub(1))
    }

    /// Whether the cell at `row`, `column` is one of its cells.
    #[must_use]
    pub const fn contains(self, row: u8, column: u8) -> bool {
        row >= self.row
            && row <= self.last_row()
            && column >= self.column
            && column < self.column.saturating_add(self.width)
    }

    /// Its cells.
    #[must_use]
    pub const fn cells(self) -> CellRect {
        CellRect { row: self.row, column: self.column, rows: self.height, columns: self.width }
    }

    /// Its cells in dots, 12 by 20 a cell: a control's rectangle for the
    /// rule table's collision and region tests (page-rules.md RULE-19;
    /// animation.md ANIM-13).
    #[must_use]
    pub const fn bounds(self) -> DotRect {
        self.cells().dots()
    }

    /// Whether any of its cells is on `row` between `column` and `end`,
    /// exclusive.
    const fn overlaps(self, row: u8, column: u8, end: u8) -> bool {
        row >= self.row
            && row <= self.last_row()
            && column < self.column.saturating_add(self.width)
            && self.column < end
    }
}

/// A page's controls, and how many of their triplets were dropped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Controls {
    /// The controls, in the order their first triplets come.
    pub list: FixedVec<Control, MAX_CONTROLS>,
    /// Triplets dropped (REQ-INT-118): outside rows 1 to 23 or past column
    /// 39, of no width, a control over another control, joined to nothing
    /// above, or beyond [`MAX_CONTROLS`].
    pub dropped: u16,
}

impl Controls {
    /// No controls.
    pub const NONE: Self = Self { list: FixedVec::new(Control::NONE), dropped: 0 };

    /// The control holding the cell at `row`, `column`, by its place in
    /// the list.
    #[must_use]
    pub fn at(&self, row: u8, column: u8) -> Option<usize> {
        self.list.as_slice().iter().position(|control| control.contains(row, column))
    }

    /// The control numbered `number`: controls are numbered from 1 in the
    /// order the page declares them (page-rules.md RULE-14).
    #[must_use]
    pub fn numbered(&self, number: u16) -> Option<Control> {
        self.list.as_slice().get(usize::from(number).checked_sub(1)?).copied()
    }

    /// The number of the shown control holding the cell at `row`,
    /// `column`: a control the animation state hides takes no pointer
    /// event (animation.md ANIM-6).
    #[must_use]
    pub fn shown_at(&self, row: u8, column: u8, animation: &Animation) -> Option<u16> {
        let number = u16::try_from(self.at(row, column)?.saturating_add(1)).ok()?;
        animation.control_shown(number).then_some(number)
    }

    /// The rectangle, in dots, of the control numbered `number` as the
    /// animation state has it (ANIM-13): `None` while it does not exist or
    /// is hidden.
    #[must_use]
    pub fn bounds(&self, number: u16, animation: &Animation) -> Option<DotRect> {
        self.numbered(number).filter(|_| animation.control_shown(number)).map(Control::bounds)
    }

    /// Takes one control triplet of `data` on `row`: a new run, or the run
    /// joined to the one above at the same column and width; false when it
    /// is dropped, as a run over another control is.
    fn take(&mut self, row: u8, column: u8, data: u8) -> bool {
        let width = data & CONTROL_WIDTH_MASK;
        let end = column.saturating_add(width);
        if width == 0 || usize::from(end) > COLUMNS || !(FIRST_CONTROL_ROW..=LAST_CONTROL_ROW).contains(&row) {
            return false;
        }
        if self.list.as_slice().iter().any(|control| control.overlaps(row, column, end)) {
            return false;
        }
        if data & CONTROL_JOINED == 0 {
            return self.list.push(Control { row, column, width, height: 1 });
        }
        let above = row.saturating_sub(1);
        let joined = self
            .list
            .as_mut_slice()
            .iter_mut()
            .find(|control| control.last_row() == above && control.column == column && control.width == width);
        match joined {
            Some(control) => {
                control.height = control.height.saturating_add(1);
                true
            }
            None => false,
        }
    }
}

/// The controls `page` declares in its local enhancement data: its packets
/// X/26 in designation order, up to the first Termination Marker or Object
/// Definition (REQ-L25-002), with the active row moved as REQ-L25-003 says.
/// Objects declare none: a control is the page's own (REQ-INT-117).
#[must_use]
pub fn controls(page: &StoredPage) -> Controls {
    use TripletFunction as F;
    let mut found = Controls::NONE;
    let mut row = 0;
    for index in 0..LOCAL_TRIPLETS {
        let Fetch::Triplet(triplet) = Body::Local.fetch(page, index) else { continue };
        match triplet.function() {
            F::Termination { .. } | F::ObjectDefinition { .. } => break,
            F::SetActivePosition { address, .. } | F::FullRowColour { address, .. } => row = row_of(address),
            F::AddressDisplayRow0 { address: ROW_ADDRESS_LAST, .. } => row = 0,
            F::ReservedColumn { column, mode: MODE_CONTROL, data } if !found.take(row, column, data) => {
                found.dropped = found.dropped.saturating_add(1);
            }
            _ => {}
        }
    }
    found
}

/// How a receiver shows a control or a link under the pointer
/// (REQ-INT-119).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// The pointer is over it: each colour of its cells becomes the same
    /// entry of the adjacent CLUT, the pairing of Table 29's flash mode 11
    /// (CLUT 0 with 1, 2 with 3), so a page chooses its hover colours by
    /// defining CLUTs 2 and 3.
    Hover,
    /// A button is held down on it: the hover colours, then foreground and
    /// background exchanged as Invert Colour exchanges them (Display
    /// Attributes D4); a cell already inverted is turned back.
    Pressed,
}

/// Gives the cells of `area` on `grid` the look, before the grid is
/// rendered.
pub fn apply_look(grid: &mut Grid, area: Control, look: Look) {
    apply_look_under(grid, area, look, &SpriteCells::NONE);
}

/// As [`apply_look`], leaving alone the cells `sprites` names: a sprite
/// lies over the page's controls and links (animation.md ANIM-12), so a
/// look given to what is beneath does not recolour it.
pub fn apply_look_under(grid: &mut Grid, area: Control, look: Look, sprites: &SpriteCells) {
    let rows = grid.cells.iter_mut().enumerate().skip(usize::from(area.row)).take(usize::from(area.height));
    for (row, cells) in rows {
        let cells = cells.iter_mut().enumerate().skip(usize::from(area.column)).take(usize::from(area.width));
        for (column, cell) in cells {
            if sprites.contains(row, column) {
                continue;
            }
            cell.foreground ^= ADJACENT_CLUT_FLIP;
            cell.background ^= ADJACENT_CLUT_FLIP;
            if look == Look::Pressed {
                cell.attributes = cell.attributes.with_invert(!cell.attributes.invert());
            }
        }
    }
}

/// Draws the controls as the animation state has them (animation.md
/// ANIM-6), on a grid already composed with its sprites, whose changed
/// cells `sprites` names and which are left alone:
///
/// - a hidden control is drawn as spaces on its row's background, the
///   row's full row colour;
/// - a control with a forced look is drawn with the hover look for look 1
///   and the pressed look for looks 2 and 3, a teletext control having two
///   looks (decision E.127).
///
/// The caller gives the pointer's own look ([`apply_look_under`]) only to
/// a control or a link with no forced look (`Animation::forced_look`).
pub fn apply_animation(grid: &mut Grid, controls: &Controls, animation: &Animation, sprites: &SpriteCells) {
    for (index, &control) in controls.list.as_slice().iter().enumerate() {
        let Ok(number) = u16::try_from(index.saturating_add(1)) else { continue };
        if !animation.control_shown(number) {
            hide(grid, control, sprites);
            continue;
        }
        match animation.forced_look(Target::Control(number)) {
            ForcedLook::None => {}
            ForcedLook::Hover => apply_look_under(grid, control, Look::Hover, sprites),
            ForcedLook::Pressed | ForcedLook::Alternative => apply_look_under(grid, control, Look::Pressed, sprites),
        }
    }
}

/// Draws a hidden control: spaces on its row's background (ANIM-6).
fn hide(grid: &mut Grid, area: Control, sprites: &SpriteCells) {
    let row_colours = grid.row_colours;
    let rows = grid.cells.iter_mut().enumerate().skip(usize::from(area.row)).take(usize::from(area.height));
    for (row, cells) in rows {
        let background = row_colours.get(row).copied().unwrap_or(Cell::BLANK.background);
        let cells = cells.iter_mut().enumerate().skip(usize::from(area.column)).take(usize::from(area.width));
        for (column, cell) in cells {
            if !sprites.contains(row, column) {
                *cell = Cell { background, boxed: cell.boxed, ..Cell::BLANK };
            }
        }
    }
}

#[cfg(test)]
mod tests;
