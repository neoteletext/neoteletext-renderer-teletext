//! Object invocation (clauses 13.3 to 13.5, 13.8): default objects, the
//! limits of Annex A.8, and accepting an object for the walk.

use super::super::{Frame, SPRITE_BIAS, Walker};

use super::super::Attrs;
use super::super::planes::{Plane, scan_index, type_index};
use crate::consts::{
    OBJECT_BODY_TRIPLETS, OBJECT_INVOCATIONS_PER_TYPE, OBJECTS_OPEN_AT_ONCE, OBJECTS_OPENED_IN_SUCCESSION,
};
use crate::objects::{self, ObjectRef};
use crate::packet::{ObjectKind, ObjectNumber, ObjectSource};
use crate::sprites::Placement;
use crate::types::CellRect;

impl<'s> Walker<'_, 's> {
    /// A default object of REQ-OBJ-040, found and accepted, with origin and
    /// active position row 0, column 0 (clause 13.8).
    pub(super) fn default_object(&mut self, default: objects::DefaultInvocation) -> Option<Frame<'s>> {
        // Annex A.8 item 1: default objects count toward the limits.
        if !self.within_limits(default.kind, (0, 0)) {
            return None;
        }
        let inputs = self.inputs;
        let found = objects::find_public(
            inputs.lookup,
            Some(default.page),
            inputs.page,
            default.kind,
            default.number,
            inputs.level,
        );
        let Ok(object) = found else {
            self.report.faults.absent_objects = self.report.faults.absent_objects.saturating_add(1);
            return None;
        };
        self.accept(object, (0, 0), (0, 0), None)
    }

    /// An Object Invocation (REQ-OBJ-001 to REQ-OBJ-008). The origin is the
    /// active position plus the offsets of an Origin Modifier immediately
    /// before it, and the active position returns after the object
    /// (EN 300 706 Table 28, mode 10000; clause 13.7; REQ-OBJ-002).
    pub(super) fn invoke(
        &mut self,
        frame: &Frame<'s>,
        source: ObjectSource,
        kind: ObjectKind,
        number: ObjectNumber,
        (row_offset, column_offset): (u8, u8),
    ) -> Option<Frame<'s>> {
        let inputs = self.inputs;
        // REQ-OBJ-008; a local object only from the page's own data.
        let allowed = frame.plane.may_invoke(kind)
            && match source {
                ObjectSource::Illegal => false,
                ObjectSource::Local => frame.body.is_local(),
                ObjectSource::Public | ObjectSource::Global => true,
            };
        if !allowed {
            self.report.faults.forbidden_invocations = self.report.faults.forbidden_invocations.saturating_add(1);
            return None;
        }
        let invocation = frame.position();
        if !self.within_limits(kind, invocation) {
            return None;
        }
        let found = if source == ObjectSource::Local {
            objects::find_local(inputs.page, kind, number, inputs.level)
        } else {
            objects::find_public(
                inputs.lookup,
                inputs.links.object_page(source),
                inputs.page,
                kind,
                number,
                inputs.level,
            )
        };
        let Ok(object) = found else {
            self.report.faults.absent_objects = self.report.faults.absent_objects.saturating_add(1);
            return None;
        };
        let origin = (
            invocation.0.saturating_add(usize::from(row_offset)),
            invocation.1.saturating_add(usize::from(column_offset)),
        );
        self.accept(object, origin, invocation, frame.sprite)
    }

    /// A shown sprite (ANIM-8), found and accepted: the object of its
    /// frame in force, invoked as an Object Invocation triplet whose
    /// active position is the sprite's origin would invoke it, from the
    /// page's own data, so any type and any source is permitted
    /// (REQ-OBJ-008). `None`, drawing nothing, when no cell of the sprite
    /// is on the page, the object is absent, or the limits of Annex A.8
    /// are reached: sprites count toward the 127 invocations of each type
    /// with the page's own objects, so the work of a grid stays bounded as
    /// it is without them (REQ-OBJ-020).
    pub(super) fn sprite(&mut self, placement: &Placement) -> Option<Frame<'s>> {
        let window = placement.window()?;
        let biased = |position: i16, bias: usize| {
            usize::try_from(i32::from(position).saturating_add(i32::try_from(bias).ok()?)).ok()
        };
        let origin = (biased(placement.row, SPRITE_BIAS.0)?, biased(placement.column, SPRITE_BIAS.1)?);
        // Each sprite is an invocation of its own from the page: no object
        // is open at it, and what earlier sprites wrote or did not write
        // does not count against it (Annex A.8 items 2 and 3).
        self.open.clear();
        self.successive = 0;
        let frame = placement.frame;
        if !self.within_limits(frame.kind, origin) {
            return None;
        }
        let inputs = self.inputs;
        let found = match frame.source {
            ObjectSource::Illegal => Err(objects::Absent::BadNumber),
            ObjectSource::Local => objects::find_local(inputs.page, frame.kind, frame.number, inputs.level),
            ObjectSource::Public | ObjectSource::Global => objects::find_public(
                inputs.lookup,
                inputs.links.object_page(frame.source),
                inputs.page,
                frame.kind,
                frame.number,
                inputs.level,
            ),
        };
        let Ok(object) = found else {
            self.report.faults.absent_objects = self.report.faults.absent_objects.saturating_add(1);
            return None;
        };
        self.accept(object, origin, origin, Some(window))
    }

    /// The limits of Annex A.8 (REQ-OBJ-020), checked before an object is
    /// looked for or scanned, so that the work stays bounded whatever the
    /// data: at most 127 invocations of each type, every permitted
    /// invocation counted whether or not its object is found; at most 15
    /// objects open at the invocation's position; at most 15 objects opened
    /// in succession without anything displayable written.
    fn within_limits(&mut self, kind: ObjectKind, invocation: (usize, usize)) -> bool {
        let start = scan_index(invocation.0, invocation.1);
        let open = self.open.as_slice().iter().filter(|&&(first, last)| first <= start && start <= last).count();
        let Some(count) = self.invoked.get_mut(type_index(kind)) else { return false };
        if *count >= OBJECT_INVOCATIONS_PER_TYPE
            || open >= OBJECTS_OPEN_AT_ONCE
            || self.successive >= OBJECTS_OPENED_IN_SUCCESSION
        {
            self.report.faults.limited_invocations = self.report.faults.limited_invocations.saturating_add(1);
            return false;
        }
        *count = count.saturating_add(1);
        true
    }

    /// Scans an object and makes its walk record (REQ-OBJ-006): it is open
    /// from the invocation to the last position it addresses (Annex A.8).
    fn accept(
        &mut self,
        object: ObjectRef<'s>,
        origin: (usize, usize),
        invocation: (usize, usize),
        sprite: Option<CellRect>,
    ) -> Option<Frame<'s>> {
        let scanned = objects::prescan(&object, self.inputs.page);
        let Ok(extent) = scanned else {
            self.report.steps = self.report.steps.saturating_add(u32::from(OBJECT_BODY_TRIPLETS));
            self.report.faults.absent_objects = self.report.faults.absent_objects.saturating_add(1);
            return None;
        };
        self.report.steps = self.report.steps.saturating_add(u32::from(extent.end.saturating_sub(object.first)));
        let start = scan_index(invocation.0, invocation.1);
        let end = extent.last.map_or(start, |(row, column)| {
            scan_index(origin.0.saturating_add(usize::from(row)), origin.1.saturating_add(usize::from(column)))
        });
        let _ = self.open.push((start, end.max(start)));
        self.successive = self.successive.saturating_add(1);
        self.report.objects = self.report.objects.saturating_add(1);
        let slot = self.next_slot;
        self.next_slot = self.next_slot.saturating_add(1);
        Some(Frame {
            plane: Plane::of(object.kind),
            body: object.body,
            cursor: object.first,
            end: extent.end,
            local_data: false,
            origin,
            row: 0,
            column: 0,
            slot,
            designation: None,
            passive: Attrs::PASSIVE,
            last_write: None,
            sprite,
        })
    }
}
