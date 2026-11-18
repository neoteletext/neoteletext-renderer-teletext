//! The animation operations of `neoteletext-rules` (animation.md ANIM-6)
//! as the engine core's own: the core has no dependency (REQ-GEN-017), so
//! it holds the operations it folds as plain values, and a program that
//! links both converts between them here.

use neoteletext_core::sprites::{ForcedLook, Operation, Target};
use neoteletext_rules::animate::{self, OperationKind};
use neoteletext_rules::table::TargetKind;

// ANIM-1: the two crates' ticks are one clock's.
const _: () =
    assert!(neoteletext_core::consts::PRESENTATION_TICKS_PER_SECOND == neoteletext_rules::limits::TICKS_PER_SECOND);
// RULE-13: a rule names the controls a teletext page can have, 1 to 32.
// The web crate may not use the core's `controls` (the layer table), so
// the two figures are held equal here, where both are linked.
const _: () = assert!(neoteletext_core::controls::MAX_CONTROLS == neoteletext_rules::limits::MAX_CONTROL as usize);
// ANIM-6, ANIM-14: and their ranges are the same ranges.
const _: () = assert!(neoteletext_core::limits::LAST_FRAME == neoteletext_rules::limits::MAX_FRAME);
const _: () = assert!(neoteletext_core::limits::MAX_CYCLE_PERIOD == neoteletext_rules::limits::MAX_CYCLE_PERIOD);
const _: () = assert!(neoteletext_core::limits::KEEP_LAYER == neoteletext_rules::limits::KEEP_LAYER);
const _: () = assert!(neoteletext_core::limits::LAST_LAYER == neoteletext_rules::limits::MAX_LAYER);

/// An operation as the core applies it: `None` when its target is neither
/// a control nor an object, or a field is outside the range the rules
/// crate's own check holds it to.
#[must_use]
pub fn to_core(operation: &animate::Operation) -> Option<(Target, Operation)> {
    let target = match operation.target.kind {
        TargetKind::Control => Target::Control(operation.target.number),
        TargetKind::Object => Target::Object(operation.target.number),
        TargetKind::Page | TargetKind::Rect => return None,
    };
    let applied = match operation.kind {
        OperationKind::Show => Operation::Show,
        OperationKind::Hide => Operation::Hide,
        OperationKind::Look(look) => Operation::Look(ForcedLook::from_code(look)?),
        OperationKind::Move { relative, x, y } => Operation::Move { relative, x, y },
        OperationKind::Order { layer, order } => Operation::order(layer, order)?,
        OperationKind::Frame(frame) => Operation::Frame(frame),
        OperationKind::Cycle { first, last, period, once } => Operation::Cycle { first, last, period, once },
        OperationKind::StopCycle => Operation::StopCycle,
        OperationKind::Reset => Operation::Reset,
    };
    Some((target, applied))
}
