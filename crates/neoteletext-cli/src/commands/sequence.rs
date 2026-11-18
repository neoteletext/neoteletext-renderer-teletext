//! The image sequence of `neottx render --frames` (animation.md ANIM-18,
//! REQ-6529): a page played for a number of ticks of the presentation
//! clock, with a script of timed operations and records, an image written
//! every so many ticks. Nothing here reads a clock: tick `n` is the
//! `n`th turn of the loop, so the images of a run are the same on every
//! machine and every run.

use super::grid_of;
use crate::records;
use crate::script::{Action, RecordKind, Step};
use neoteletext_app::session::Session;
use neoteletext_app::{Failure, exit};
use neoteletext_core::compose::compose_sprites;
use neoteletext_core::controls::{self, Controls};
use neoteletext_core::fsm::PageKey;
use neoteletext_core::level1::{DisplayOptions, Grid};
use neoteletext_core::sprites::{Animation, Applied, Operation, Placements, Sprites, Target};
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_web::records::frame::{FrameChange, FrameEnd, OpenFrames};
use neoteletext_web::records::{animate, frame, sprites};

/// The most ticks a sequence plays (ANIM-18): ten minutes.
pub const MAX_SEQUENCE_TICKS: u32 = 36_000;
/// The page being played: its sprites and animation state, and the frame
/// that is open.
struct Player {
    /// The page, as a record names it.
    page: PageAddress,
    subcode: SubCode,
    controls: Controls,
    sprites: Sprites,
    animation: Animation,
    /// Whether the page has an atomic frame open, how much it holds and
    /// for how long (ANIM-2): the follower a receiver of a channel uses,
    /// with its bounds on a frame's bytes and ticks (ANIM-16).
    frames: OpenFrames,
    /// The records the open frame holds, unapplied and in arrival order.
    held: Vec<Step>,
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
    /// An ANIMATE record names no sub-page when it is for whichever is
    /// shown (ANIM-6).
    fn ours(&self, page: PageAddress, subcode: Option<SubCode>) -> bool {
        page == self.page && subcode.is_none_or(|subcode| subcode == self.subcode)
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
                let mark = frame::read(body).map_err(|fault| refused(step, &records::why(fault)))?;
                if !self.ours(mark.page, Some(mark.subcode)) {
                    return Ok(());
                }
                // A second begin ends the frame that was open, as an end
                // does; an end with no begin, or another's, does nothing.
                match self.frames.record(&mark) {
                    FrameChange::Ended(_) | FrameChange::Reopened => self.end_frame(tick)?,
                    FrameChange::Opened | FrameChange::Nothing => {}
                }
            }
            Action::Record { kind, body } => match self.frames.hold(self.page, body.len()) {
                None => self.record(step, *kind, body, tick)?,
                Some(None) => self.held.push(step.clone()),
                // The frame is full: what it held is applied, then this.
                Some(Some(_)) => {
                    self.end_frame(tick)?;
                    self.record(step, *kind, body, tick)?;
                }
            },
        }
        Ok(())
    }

    /// Applies a record that is not a FRAME.
    fn record(&mut self, step: &Step, kind: RecordKind, body: &[u8], tick: u32) -> Result<(), Failure> {
        match kind {
            RecordKind::Animate => {
                let record = animate::read(body).map_err(|fault| refused(step, &records::why(fault)))?;
                if self.ours(record.page, record.subcode) {
                    for (target, operation) in records::operations(&record).map_err(|why| refused(step, &why))? {
                        self.operate(target, operation, tick);
                    }
                }
            }
            RecordKind::Sprites => {
                let record = sprites::read(body).map_err(|fault| refused(step, &records::why(fault)))?;
                if self.ours(record.page, Some(record.subcode)) {
                    if !cfg!(feature = "objects") {
                        return Err(neoteletext_app::not_in_build("teletext sprites", "objects"));
                    }
                    // ANIM-6: a SPRITES record replaces all of a sub-page's
                    // sprites and forgets the state of what it replaces.
                    self.sprites = records::sprites_of(&record).map_err(|why| refused(step, &why))?;
                    self.animation.clear_objects();
                }
            }
            RecordKind::Frame => {}
        }
        Ok(())
    }

    /// The frame ended: everything it held is applied, in order, on this
    /// tick (ANIM-2).
    fn end_frame(&mut self, tick: u32) -> Result<(), Failure> {
        for step in &core::mem::take(&mut self.held) {
            if let Action::Record { kind, body } = &step.action {
                self.record(step, *kind, body, tick)?;
            }
        }
        Ok(())
    }

    /// The start of a tick: a frame open for `MAX_FRAME_TICKS` with no
    /// end is applied (ANIM-2).
    fn begin_tick(&mut self, tick: u32) -> Result<(), Failure> {
        let mut late = false;
        self.frames.tick(&mut |_, end| late |= end == FrameEnd::Late);
        if late {
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
        page: key.address,
        subcode: page.key.subcode,
        controls: controls::controls(page),
        sprites: Sprites::NONE,
        animation: Animation::NONE,
        frames: OpenFrames::new(),
        held: Vec::new(),
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
