//! The image sequence of `neottx render --frames` (animation.md ANIM-18,
//! REQ-6529): a page played for a number of ticks of the presentation
//! clock, with a script of timed operations and records, an image written
//! every so many ticks. Nothing here reads a clock: tick `n` is the
//! `n`th turn of the loop, so the images of a run are the same on every
//! machine and every run.

use super::grid_of;
use crate::records::{self, SUBCODE_SHOWN};
use crate::script::{Action, RecordKind, Step};
use neoteletext_app::session::Session;
use neoteletext_app::{Failure, exit};
use neoteletext_core::compose::compose_sprites;
use neoteletext_core::controls::{self, Controls};
use neoteletext_core::fsm::PageKey;
use neoteletext_core::level1::{DisplayOptions, Grid};
use neoteletext_core::sprites::{Animation, Applied, Operation, Placements, Sprites, Target};

/// The most ticks a sequence plays (ANIM-18): ten minutes.
pub const MAX_SEQUENCE_TICKS: u32 = 36_000;
/// The most bytes an atomic frame holds before what it holds is applied
/// (ANIM-2, ANIM-16).
const MAX_FRAME_BYTES: usize = 262_144;
/// The most ticks an atomic frame stays open with no end before what it
/// holds is applied: two seconds (ANIM-2, ANIM-16).
const MAX_FRAME_TICKS: u32 = 120;

/// An atomic frame that has begun and not ended (ANIM-2): the records held
/// for the page, unapplied and in arrival order.
struct OpenFrame {
    identifier: u16,
    began: u32,
    held: Vec<Step>,
    bytes: usize,
}

/// The page being played: its sprites and animation state, and the frame
/// that is open.
struct Player {
    /// The page, as a record names it.
    page: u16,
    subcode: u16,
    controls: Controls,
    sprites: Sprites,
    animation: Animation,
    open: Option<OpenFrame>,
    /// Operations that were refused as out of range or as not acting on
    /// their target, which a record's reader refuses first.
    refused: u32,
}

/// The failure of a script's line.
fn refused(step: &Step, why: &str) -> Failure {
    (exit::INPUT, format!("the script's line {}: {why}", step.line))
}

impl Player {
    /// Whether a record for `page` and `subcode` is for the page played.
    /// An ANIMATE record may name whichever sub-page is shown.
    fn ours(&self, page: u16, subcode: u16, any_shown: bool) -> bool {
        page == self.page && (subcode == self.subcode || (any_shown && subcode == SUBCODE_SHOWN))
    }

    fn operate(&mut self, target: Target, operation: Operation, tick: u32) {
        let controls = self.controls.list.len();
        if self.animation.apply(target, operation, tick, &self.sprites, controls) == Applied::Refused {
            self.refused = self.refused.saturating_add(1);
        }
    }

    /// Applies one line of the script at `tick`: an operation at once; a
    /// record at once too, unless a frame is open for the page, which
    /// holds it (ANIM-2).
    fn step(&mut self, step: &Step, tick: u32) -> Result<(), Failure> {
        match &step.action {
            Action::Operation { target, operation } => self.operate(*target, *operation, tick),
            Action::Record { kind: RecordKind::Frame, body } => {
                let mark = records::frame(body).map_err(|why| refused(step, &why))?;
                if !self.ours(mark.page, mark.subcode, false) {
                    return Ok(());
                }
                // A second begin ends the frame that was open, as an end
                // does; an end with no begin, or another's, does nothing.
                if mark.begin || self.open.as_ref().is_some_and(|open| open.identifier == mark.identifier) {
                    self.end_frame(tick)?;
                }
                if mark.begin {
                    self.open =
                        Some(OpenFrame { identifier: mark.identifier, began: tick, held: Vec::new(), bytes: 0 });
                }
            }
            Action::Record { kind, body } => {
                let Some(open) = self.open.as_mut() else { return self.record(step, *kind, body, tick) };
                open.bytes = open.bytes.saturating_add(body.len());
                open.held.push(step.clone());
                if open.bytes >= MAX_FRAME_BYTES {
                    self.end_frame(tick)?;
                }
            }
        }
        Ok(())
    }

    /// Applies a record that is not a FRAME.
    fn record(&mut self, step: &Step, kind: RecordKind, body: &[u8], tick: u32) -> Result<(), Failure> {
        match kind {
            RecordKind::Animate => {
                let record = records::animate(body).map_err(|why| refused(step, &why))?;
                if self.ours(record.page, record.subcode, true) {
                    for (target, operation) in record.operations {
                        self.operate(target, operation, tick);
                    }
                }
            }
            RecordKind::Sprites => {
                let record = records::sprites(body).map_err(|why| refused(step, &why))?;
                if self.ours(record.page, record.subcode, false) {
                    if !cfg!(feature = "objects") {
                        return Err(neoteletext_app::not_in_build("teletext sprites", "objects"));
                    }
                    // ANIM-6: a SPRITES record replaces all of a sub-page's
                    // sprites and forgets the state of what it replaces.
                    self.sprites = record.sprites;
                    self.animation.clear_objects();
                }
            }
            RecordKind::Frame => {}
        }
        Ok(())
    }

    /// Ends the open frame, if any: everything it held is applied, in
    /// order, on this tick (ANIM-2).
    fn end_frame(&mut self, tick: u32) -> Result<(), Failure> {
        let Some(open) = self.open.take() else { return Ok(()) };
        for step in &open.held {
            if let Action::Record { kind, body } = &step.action {
                self.record(step, *kind, body, tick)?;
            }
        }
        Ok(())
    }

    /// The start of a tick: a frame open for [`MAX_FRAME_TICKS`] with no
    /// end is applied (ANIM-2).
    fn begin_tick(&mut self, tick: u32) -> Result<(), Failure> {
        if self.open.as_ref().is_some_and(|open| tick.saturating_sub(open.began) >= MAX_FRAME_TICKS) {
            self.end_frame(tick)?;
        }
        Ok(())
    }
}

/// Plays the page `key` for `ticks` ticks from tick 0 with `script`, and
/// calls `write` with the tick and the page as presented at every tick
/// that is a multiple of `every`, and at the last. The page's own grid is
/// built once; each image composes the shown sprites over it and draws
/// the controls as the animation state has them.
///
/// # Errors
/// A page that is not stored, a record of the script that its reader
/// refuses, and whatever `write` fails with.
pub(super) fn play(
    session: &mut Session,
    key: PageKey,
    script: &[Step],
    (ticks, every): (u32, u32),
    mut write: impl FnMut(u32, &Grid) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let page_grid = grid_of(session, key)?;
    let decoder = session.decoder();
    let store = decoder.store();
    let not_stored = || (exit::NOT_FOUND, "the page is no longer stored".to_owned());
    let page = store.find(key).and_then(|index| store.slot(index)).ok_or_else(not_stored)?;
    let magazine = decoder.magazine_data(key.address.magazine);
    let lookup = decoder.enhancement_lookup();
    let config = *decoder.config();
    // The options a page's grid is built with differ from these only in
    // row 0, which is the receiver's and which a sprite never writes.
    let options = DisplayOptions::default();
    let mut player = Player {
        page: key.address.mpp(),
        subcode: page.key.subcode.value(),
        controls: controls::controls(page),
        sprites: Sprites::NONE,
        animation: Animation::NONE,
        open: None,
        refused: 0,
    };
    let mut steps = script.iter().peekable();
    let mut grid = Grid::EMPTY;
    let last = ticks.saturating_sub(1);
    for tick in 0..ticks {
        player.begin_tick(tick)?;
        while let Some(step) = steps.next_if(|step| step.tick <= tick) {
            player.step(step, tick)?;
        }
        let _ = player.animation.advance(tick);
        if tick.checked_rem(every) != Some(0) && tick != last {
            continue;
        }
        let placements = Placements::of(&player.sprites, &player.animation);
        let drawn = compose_sprites(&mut grid, &page_grid, page, &magazine, &lookup, &config, &options, &placements);
        controls::apply_animation(&mut grid, &player.controls, &player.animation, &drawn.cells);
        write(tick, &grid)?;
    }
    let skipped = player.animation.skipped;
    if skipped > 0 || player.refused > 0 {
        eprintln!(
            "neottx: {skipped} operations named a target that does not exist or had no room for its state, and {} were refused as out of range",
            player.refused
        );
    }
    Ok(())
}
