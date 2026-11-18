//! Encoding the input stream: one item, read back before it is sent.

use super::decode::read;
use super::{
    Action, COMPOSE, DEVICE_SHIFT, DRAWING_HOVER, DRAWING_HOVER_LEAVE, Device, EVENT, Event, FIRST_EXTENSION, FOCUS_IN,
    FOCUS_OUT, FUNCTIONAL, HAS_BASE, HAS_MODS, HAS_PIXELS, HAS_TEXT, HELLO, HOVER, HOVER_LEAVE, InputError, Item, KEY,
    Key, KeyCode, LAST_EXTENSION, LOST, MAX_PICK_OFFSET, MAX_PICK_POSITION, MOUSE, MouseAction, PAGE_STATE, PASTE,
    PICK, Point, RESIZE, RESUME, RULE_EVENT, RULES_FAULT, SELECT, STAMP_BYTES, STATE_NONE, STATE_OFFER, STATE_PROOF,
    Select, Stamp, TOUCH, Touch, WHEEL, check_text, is_page_parameters,
};
use super::{PAGE_DIGITS, QUERY_MARK, SELECT_PLACE, SUB_DIGITS, SUB_MARK, SUBCODE_ANY};
use crate::caps::AGENT;
use crate::wire::{MAX_VARINT, ONE_BYTE_MAX, Writer};
use crate::{MAX_AGENT, MAX_COLUMNS, MAX_COMPOSE, MAX_KEY_TEXT, MAX_PASTE, MAX_ROWS, TOKEN_BYTES};

/// HOVER's payload at its longest: the stamp, the flags and two varints.
const HOVER_PAYLOAD: usize = STAMP_BYTES + 1 + 2 * MAX_VARINT;
/// PICK's payload at its longest: the stamp, five varints and the commit
/// count's four bytes.
const PICK_PAYLOAD: usize = STAMP_BYTES + 5 * MAX_VARINT + 4;
/// `DRAWING_HOVER`'s payload at its longest: the stamp, the flags, a varint
/// and the commit count's four bytes.
const DRAWING_HOVER_PAYLOAD: usize = STAMP_BYTES + 1 + MAX_VARINT + 4;
/// TOUCH's payload at its longest: the stamp, the flags, the id and four
/// varints.
pub(super) const TOUCH_PAYLOAD: usize = STAMP_BYTES + 2 + 4 * MAX_VARINT;
/// RESUME's payload: the token and the acknowledgement, big-endian.
pub(super) const RESUME_PAYLOAD: usize = TOKEN_BYTES + 2;
/// The bytes of a cell's column or row: every one is under 128, one
/// varint byte.
const CELL_BYTES: usize = 1;
const _: () = assert!(MAX_COLUMNS <= ONE_BYTE_MAX + 1 && MAX_ROWS <= ONE_BYTE_MAX + 1);
/// The most bytes a pointer's move takes as an event (REQ-RCH-035), what a
/// holder of one move needs: a MOUSE move, its event byte, type, flags and
/// button, the stamp, the cell, the dot's two varints and the modifiers;
/// or a TOUCH, its event byte, type and one-byte length, the stamp, its
/// flags and id, the cell and the dot; whichever is longer.
pub const MAX_MOVE_BYTES: usize = {
    let mouse = 4 + STAMP_BYTES + 2 * CELL_BYTES + 2 * MAX_VARINT + 1;
    let touch = 3 + STAMP_BYTES + 2 + 2 * CELL_BYTES + 2 * MAX_VARINT;
    if mouse > touch { mouse } else { touch }
};
// TOUCH's length is one varint byte.
const _: () = assert!(TOUCH_PAYLOAD <= ONE_BYTE_MAX as usize);

/// The code of a pointer's action (DOC-1938).
const fn action_code(action: MouseAction) -> u8 {
    match action {
        MouseAction::Press => 0,
        MouseAction::Release => 1,
        MouseAction::Move => 2,
        MouseAction::Gone => 3,
    }
}

/// The code of MOUSE's device, bits 4 and 5 (REQ-RCH-042).
const fn device_code(device: Device) -> u8 {
    match device {
        Device::Mouse => 0,
        Device::Touch => 1,
        Device::Pen => 2,
    }
}

/// Writes an extension: its type, its payload's length, then the payload
/// `fill` writes, so the length can go before it.
fn write_extension<const N: usize>(
    kind: u8,
    writer: &mut Writer<'_>,
    fill: impl FnOnce(&mut Writer<'_>) -> Result<(), InputError>,
) -> Result<(), InputError> {
    let mut payload = [0_u8; N];
    let mut inner = Writer::new(&mut payload);
    fill(&mut inner)?;
    let length = inner.len();
    writer.byte(kind)?;
    writer.uv(u32::try_from(length).map_err(|_| InputError::Message)?)?;
    writer.bytes(payload.get(..length).unwrap_or(&[]))?;
    Ok(())
}

/// The stamp every input event begins with (DOC-1934, REQ-1555).
fn write_stamp(on: Stamp, writer: &mut Writer<'_>) -> Result<(), InputError> {
    writer.uv(u32::from(on.page))?;
    writer.uv(u32::from(on.sub))?;
    Ok(())
}

/// SELECT (DOC-1753, DEC-1730): the address's place as text,
/// the same text the URL map reads: `302`, `302/0002` or `302?a=1`, and
/// nothing when the tab shows no page.
fn write_select(select: Select, parameters: &str, writer: &mut Writer<'_>) -> Result<(), InputError> {
    if !is_page_parameters(select, parameters) {
        return Err(InputError::BadValue("select"));
    }
    let names_sub = select.sub != SUBCODE_ANY;
    let mut place = [0_u8; SELECT_PLACE];
    let used = if select.page == 0 {
        0
    } else {
        write_hex::<PAGE_DIGITS>(select.page, &mut place)?;
        if names_sub {
            let (_, rest) = place.split_at_mut(PAGE_DIGITS);
            let (mark, digits) = rest.split_first_mut().ok_or(InputError::Message)?;
            *mark = SUB_MARK;
            write_hex::<SUB_DIGITS>(select.sub, digits)?;
            SELECT_PLACE
        } else {
            PAGE_DIGITS
        }
    };
    let query = if parameters.is_empty() { 0 } else { parameters.len().saturating_add(1) };
    writer.byte(SELECT)?;
    writer.uv(u32::try_from(used.saturating_add(query)).map_err(|_| InputError::Message)?)?;
    writer.bytes(place.get(..used).unwrap_or(&[]))?;
    if !parameters.is_empty() {
        writer.byte(QUERY_MARK)?;
        writer.bytes(parameters.as_bytes())?;
    }
    Ok(())
}

/// `value`'s last `N` hexadecimal digits, as capitals, into `out`.
fn write_hex<const N: usize>(value: u16, out: &mut [u8]) -> Result<(), InputError> {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut rest = usize::from(value);
    for at in (0..N).rev() {
        let digit = rest.checked_rem(DIGITS.len()).and_then(|digit| DIGITS.get(digit));
        *out.get_mut(at).ok_or(InputError::Message)? = *digit.ok_or(InputError::Message)?;
        rest = rest.checked_div(DIGITS.len()).ok_or(InputError::Message)?;
    }
    Ok(())
}

fn write_touch(touch: &Touch, writer: &mut Writer<'_>) -> Result<(), InputError> {
    write_extension::<TOUCH_PAYLOAD>(TOUCH, writer, |inner| {
        write_stamp(touch.on, inner)?;
        let pixels = if touch.pixels.is_some() { HAS_PIXELS } else { 0 };
        inner.bytes(&[action_code(touch.action) | pixels, touch.id])?;
        inner.uv(touch.column)?;
        inner.uv(touch.row)?;
        if let Some((x, y)) = touch.pixels {
            inner.uv(x)?;
            inner.uv(y)?;
        }
        Ok(())
    })
}

/// Writes one item, then reads it back: what does not read back as it was
/// written is not sent.
///
/// # Errors
/// When the item breaks a bound, or `out` is too small.
pub fn write(item: &Item<'_>, out: &mut [u8]) -> Result<usize, InputError> {
    let mut writer = Writer::new(out);
    match item {
        Item::Text(text) => {
            check_text(text)?;
            writer.bytes(text.as_bytes())?;
        }
        Item::Event(event) => {
            writer.byte(EVENT)?;
            write_event(event, &mut writer)?;
        }
    }
    let length = writer.len();
    let written = out.get(..length).unwrap_or(&[]);
    match read(written) {
        Ok(Some((back, used))) if used == length && back == *item => Ok(length),
        Ok(_) => Err(InputError::BadValue("does not read back")),
        Err(error) => Err(error),
    }
}

fn write_point(point: &Point, writer: &mut Writer<'_>) -> Result<(), InputError> {
    writer.uv(point.column)?;
    writer.uv(point.row)?;
    if let Some((x, y)) = point.pixels {
        writer.uv(x)?;
        writer.uv(y)?;
    }
    if point.mods != 0 {
        writer.byte(point.mods)?;
    }
    Ok(())
}

fn point_flags(point: &Point) -> u8 {
    (if point.pixels.is_some() { HAS_PIXELS } else { 0 }) | (if point.mods == 0 { 0 } else { HAS_MODS })
}

/// A key event: its flags, its code, then the modifiers, text and base
/// key it carries.
fn write_key(key: &Key<'_>, writer: &mut Writer<'_>) -> Result<(), InputError> {
    let action = match key.action {
        Action::Press => 0,
        Action::Repeat => 1,
        Action::Release => 2,
    };
    let (functional, value) = match key.key {
        KeyCode::Character(character) => (0, u32::from(character)),
        KeyCode::Functional(code) => (FUNCTIONAL, code),
    };
    let flags = action
        | functional
        | (if key.mods == 0 { 0 } else { HAS_MODS })
        | (if key.text.is_some() { HAS_TEXT } else { 0 })
        | (if key.base.is_some() { HAS_BASE } else { 0 })
        | (if key.lost { LOST } else { 0 });
    writer.byte(KEY)?;
    write_stamp(key.on, writer)?;
    writer.byte(flags)?;
    writer.uv(value)?;
    if key.mods != 0 {
        writer.byte(key.mods)?;
    }
    if let Some(text) = key.text {
        writer.str(text, MAX_KEY_TEXT)?;
    }
    if let Some(base) = key.base {
        writer.uv(u32::from(base))?;
    }
    Ok(())
}

/// `DRAWING_HOVER` (REQ-6369): the stamp, the flags, the drawing and the
/// commit count.
fn write_drawing_hover(
    on: Stamp,
    leave: bool,
    drawing: u16,
    commits: u32,
    writer: &mut Writer<'_>,
) -> Result<(), InputError> {
    // There is no event for being over no drawing.
    if drawing == 0 {
        return Err(InputError::BadValue("drawing hover"));
    }
    write_extension::<DRAWING_HOVER_PAYLOAD>(DRAWING_HOVER, writer, |inner| {
        write_stamp(on, inner)?;
        inner.byte(if leave { DRAWING_HOVER_LEAVE } else { 0 })?;
        inner.uv(u32::from(drawing))?;
        inner.bytes(&commits.to_be_bytes())?;
        Ok(())
    })
}

/// Writes an extension whose payload is the stamp and then `parts`, in
/// order, with no copy of them: a `RULE_EVENT` or a `PAGE_STATE` may be
/// most of a message.
fn write_stamped(kind: u8, on: Stamp, parts: &[&[u8]], writer: &mut Writer<'_>) -> Result<(), InputError> {
    let mut stamp = [0_u8; STAMP_BYTES];
    let mut inner = Writer::new(&mut stamp);
    write_stamp(on, &mut inner)?;
    let stamp_length = inner.len();
    let length = parts.iter().fold(stamp_length, |length, part| length.saturating_add(part.len()));
    writer.byte(kind)?;
    writer.uv(u32::try_from(length).map_err(|_| InputError::Message)?)?;
    writer.bytes(stamp.get(..stamp_length).unwrap_or(&[]))?;
    for part in parts {
        writer.bytes(part)?;
    }
    Ok(())
}

/// The rule table's messages (RULE-46 to RULE-48).
fn write_rules(event: &Event<'_>, writer: &mut Writer<'_>) -> Result<(), InputError> {
    match event {
        Event::RuleEvent(event) => {
            let (head, length) = crate::rules::rule_event_head(event);
            let head = head.get(..length).unwrap_or(&[]);
            let input = match event.body {
                crate::rules::RuleBody::Input(bytes) => bytes,
                _ => &[],
            };
            write_stamped(RULE_EVENT, event.on, &[head, input], writer)
        }
        Event::PageState(state) => write_stamped(PAGE_STATE, state.on, &[state.body], writer),
        Event::RulesFault(fault) => {
            write_stamped(RULES_FAULT, fault.on, &[&crate::rules::rules_fault_bytes(fault)], writer)
        }
        _ => Err(InputError::BadValue("not a rule table message")),
    }
}

/// Viewer state's messages (VS-204): each a fixed payload.
fn write_state(event: &Event<'_>, writer: &mut Writer<'_>) -> Result<(), InputError> {
    match event {
        Event::StateProof(proof) => {
            write_extension::<{ crate::state::STATE_PROOF_BYTES }>(STATE_PROOF, writer, |inner| {
                Ok(inner.bytes(&crate::state::proof_payload(proof))?)
            })
        }
        Event::StateNone(reason) => {
            write_extension::<{ crate::state::STATE_NONE_BYTES }>(STATE_NONE, writer, |inner| {
                Ok(inner.byte(reason.code())?)
            })
        }
        Event::StateOffer => write_extension::<{ crate::state::STATE_OFFER_BYTES }>(STATE_OFFER, writer, |_| Ok(())),
        _ => Err(InputError::BadValue("not a state message")),
    }
}

fn write_event(event: &Event<'_>, writer: &mut Writer<'_>) -> Result<(), InputError> {
    match event {
        Event::Key(key) => write_key(key, writer)?,
        Event::Mouse(mouse) => {
            let flags =
                action_code(mouse.action) | point_flags(&mouse.at) | (device_code(mouse.device) << DEVICE_SHIFT);
            writer.byte(MOUSE)?;
            write_stamp(mouse.on, writer)?;
            writer.bytes(&[flags, mouse.button])?;
            write_point(&mouse.at, writer)?;
        }
        Event::Wheel(wheel) => {
            writer.byte(WHEEL)?;
            write_stamp(wheel.on, writer)?;
            writer.byte(point_flags(&wheel.at))?;
            writer.sv(wheel.dx)?;
            writer.sv(wheel.dy)?;
            write_point(&wheel.at, writer)?;
        }
        Event::Paste { on, text } => {
            writer.byte(PASTE)?;
            write_stamp(*on, writer)?;
            writer.str(text, MAX_PASTE)?;
        }
        Event::FocusIn => writer.byte(FOCUS_IN)?,
        Event::FocusOut => writer.byte(FOCUS_OUT)?,
        Event::Resize { columns, rows, width, height } => {
            writer.byte(RESIZE)?;
            for value in [*columns, *rows, *width, *height] {
                writer.uv(value)?;
            }
        }
        Event::Compose { on, cursor, text } => {
            writer.byte(COMPOSE)?;
            write_stamp(*on, writer)?;
            writer.uv(*cursor)?;
            writer.str(text, MAX_COMPOSE)?;
        }
        Event::Hello { version, caps, agent } => {
            if agent.is_some() != (caps & AGENT != 0) {
                return Err(InputError::BadValue("agent without its capability"));
            }
            writer.byte(HELLO)?;
            writer.uv(*version)?;
            writer.uv(*caps)?;
            if let Some(agent) = agent {
                writer.str(agent, MAX_AGENT)?;
            }
        }
        Event::Hover { on, leave, column, row } => write_extension::<HOVER_PAYLOAD>(HOVER, writer, |inner| {
            write_stamp(*on, inner)?;
            inner.byte(if *leave { HOVER_LEAVE } else { 0 })?;
            inner.uv(*column)?;
            inner.uv(*row)?;
            Ok(())
        })?,
        Event::Pick(pick) => {
            if pick.x.abs() > MAX_PICK_OFFSET
                || pick.y.abs() > MAX_PICK_OFFSET
                || pick.picture_x >= MAX_PICK_POSITION
                || pick.picture_y >= MAX_PICK_POSITION
            {
                return Err(InputError::BadValue("pick"));
            }
            write_extension::<PICK_PAYLOAD>(PICK, writer, |inner| {
                write_stamp(pick.on, inner)?;
                inner.uv(u32::from(pick.drawing))?;
                inner.sv(pick.x)?;
                inner.sv(pick.y)?;
                inner.uv(pick.picture_x)?;
                inner.uv(pick.picture_y)?;
                inner.bytes(&pick.commits.to_be_bytes())?;
                Ok(())
            })?;
        }
        Event::DrawingHover { on, leave, drawing, commits } => {
            write_drawing_hover(*on, *leave, *drawing, *commits, writer)?;
        }
        Event::Touch(touch) => write_touch(touch, writer)?,
        Event::Resume(resume) => write_extension::<RESUME_PAYLOAD>(RESUME, writer, |inner| {
            inner.bytes(&resume.token)?;
            inner.bytes(&resume.acked.to_be_bytes())?;
            Ok(())
        })?,
        Event::Select(select, parameters) => write_select(*select, parameters, writer)?,
        Event::StateProof(_) | Event::StateNone(_) | Event::StateOffer => write_state(event, writer)?,
        Event::RuleEvent(_) | Event::PageState(_) | Event::RulesFault(_) => write_rules(event, writer)?,
        Event::Extension { kind, payload } => {
            if !(FIRST_EXTENSION..=LAST_EXTENSION).contains(kind) {
                return Err(InputError::Type(*kind));
            }
            writer.byte(*kind)?;
            writer.uv(u32::try_from(payload.len()).map_err(|_| InputError::Message)?)?;
            writer.bytes(payload)?;
        }
    }
    Ok(())
}
