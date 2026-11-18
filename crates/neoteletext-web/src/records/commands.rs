//! The command records, types 28 to 33 (neoteletext-common
//! `docs/page-rules.md`, DEC-6587; RULE-21, RULE-99, RULE-100, RULE-22,
//! RULE-102, RULE-103), and the one body check of a rule table's groups
//! (RULE-56).
//!
//! ```text
//! LOCAL_MIDI  28  page, sub-code, audio channel, status, data 1, data 2
//! VARIABLE    29  page, sub-code, operation, variable, operand kind, operand
//! TIMER       30  page, sub-code, operation, timer
//! REPORT      31  page, sub-code, what, value
//! RUN         32  page, sub-code, group, mode, timer, ticks
//! SEQUENCE    33  page, sub-code, operation, sequence
//! ```
//!
//! Each is a record the server may send directly and a command a group
//! of a page's rule table may hold, the same bytes either way (RULE-118).
//! Their bodies are read and written by `neoteletext-rules`
//! ([`neoteletext_rules::command`]), which the evaluator applies them
//! with; this module frames them as records. [`body_check`] is the
//! function `neoteletext-rules` reaches the readers of `CONTROL`,
//! `SPRITES` and `NAPLPS_OBJECTS` through: the server and every receiver
//! pass this one to [`neoteletext_rules::decode`] and
//! [`neoteletext_rules::check`].

use neoteletext_audio::record::{FLAG_AFTER_CURRENT, Record, TYPE_CONTROL};
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_rules::BodyFacts;
use neoteletext_rules::command::{self, Command, MAX_BODY_BYTES};

use super::{
    RecordFault, TYPE_DRAWING_LINKS, TYPE_NAPLPS_OBJECTS, TYPE_REGIS_CURSOR, TYPE_SPRITES, drawing_links,
    naplps_objects, regis_cursor, sprites, take_place, write_packets,
};

pub use neoteletext_audio::record::{TYPE_LOCAL_MIDI, TYPE_REPORT, TYPE_RUN, TYPE_SEQUENCE, TYPE_TIMER, TYPE_VARIABLE};

// RULE-56: the rule table's crate states the six command types itself,
// having no dependency; they are the audio profile's, where the record
// types are defined, which the audio receiver leaves alone.
const _: () = assert!(
    command::TYPE_LOCAL_MIDI == TYPE_LOCAL_MIDI
        && command::TYPE_VARIABLE == TYPE_VARIABLE
        && command::TYPE_TIMER == TYPE_TIMER
        && command::TYPE_REPORT == TYPE_REPORT
        && command::TYPE_RUN == TYPE_RUN
        && command::TYPE_SEQUENCE == TYPE_SEQUENCE
);

// RULE-109: a cue's number is the one data byte of a Cue Point event in a
// block, so the highest a `cue` trigger names is the highest MIDI data
// byte the block reader takes.
const _: () = assert!(neoteletext_rules::limits::MAX_CUE == neoteletext_audio::record::MIDI_DATA_MAX);

// RULE-56: the rule table's crate has no dependency, so it states the
// `CONTROL` record's type and length itself; here both are linked, and
// they are the audio profile's. The places of the gain and the fade,
// which a binding fills (RULE-106), are held by a test.
const _: () = assert!(
    command::TYPE_CONTROL == TYPE_CONTROL
        && command::CONTROL_BYTES == neoteletext_audio::record::CONTROL_BYTES
        && command::TYPE_SPRITES == TYPE_SPRITES
        && command::TYPE_NAPLPS_OBJECTS == TYPE_NAPLPS_OBJECTS
        && command::TYPE_ANIMATE == super::TYPE_ANIMATE
        && command::TYPE_DRAWING_LINKS == TYPE_DRAWING_LINKS
        && command::TYPE_REGIS_CURSOR == TYPE_REGIS_CURSOR
        && neoteletext_rules::limits::MAX_GAIN == neoteletext_audio::MAX_GAIN
        && neoteletext_rules::limits::MAX_FADE_MS == neoteletext_audio::MAX_FADE_MS
        && neoteletext_rules::limits::AUDIO_CHANNELS == neoteletext_audio::CHANNELS as usize
);

/// One record of types 28 to 33, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CommandRecord<'a> {
    /// The record's type, 28 to 33.
    pub kind: u8,
    /// The page the command is for.
    pub page: PageAddress,
    /// Its sub-code (RULE-112).
    pub subcode: SubCode,
    /// The command.
    pub command: Command,
    /// The record's body, which
    /// [`neoteletext_rules::evaluator::Event::Command`] takes as it is
    /// for types 29 to 33.
    pub body: &'a [u8],
}

/// Reads the body of a record of type `kind`, 28 to 33.
///
/// # Errors
/// Another type, a page or a sub-code outside its range, a body that is
/// not exactly its record's length, or any field
/// [`neoteletext_rules::command::decode`] refuses.
pub fn read(kind: u8, body: &[u8]) -> Result<CommandRecord<'_>, RecordFault> {
    let (page, subcode, _) = take_place(body)?;
    let (_, _, command) = command::decode(kind, body).map_err(|_| RecordFault::BadValue("command"))?;
    Ok(CommandRecord { kind, page, subcode, command, body })
}

/// Writes the body of `command` for the page `page` and `subcode` at the
/// front of `out`, and how many bytes it took: the bytes the record
/// carries and the bytes a group holds (RULE-59: one function for a
/// command whether it is sent directly or put in a group).
///
/// # Errors
/// What [`neoteletext_rules::command::encode`] refuses.
pub fn body(command: &Command, page: PageAddress, subcode: SubCode, out: &mut [u8]) -> Result<usize, RecordFault> {
    command::encode(command, page.mpp(), subcode.value(), out).map_err(|_| RecordFault::BadValue("command"))
}

/// Writes `command` for the page `page` and `subcode` as packets 4/31 of
/// audio service `service`, record id `record_id`. Returns the packets
/// written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode(
    command: &Command,
    page: PageAddress,
    subcode: SubCode,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut bytes = [0_u8; MAX_BODY_BYTES];
    let length = body(command, page, subcode, &mut bytes)?;
    let bytes = bytes.get(..length).ok_or(RecordFault::Length)?;
    // The writer checks with the reader, so the two cannot disagree.
    let back = read(command.kind(), bytes)?;
    if (back.page, back.subcode, back.command) != (page, subcode, *command) {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(command.kind(), bytes, service, record_id, out).ok_or(RecordFault::Length)
}

/// The body check of a rule table's groups (RULE-56, RULE-45): reads the
/// body of a `CONTROL`, a `SPRITES`, a `NAPLPS_OBJECTS`, a `DRAWING_LINKS`
/// or a `REGIS_CURSOR` record (PAR-20) with the
/// reader that reads that record when the server sends it, and gives what
/// `neoteletext-rules` needs of it, or `None` when that reader refuses
/// the body or the type is another.
#[must_use]
pub fn body_check(kind: u8, body: &[u8]) -> Option<BodyFacts> {
    match kind {
        TYPE_CONTROL => {
            // The audio profile reads a CONTROL as a whole record, so the
            // body is framed as the server frames it and read back.
            let mut record = [0_u8; neoteletext_audio::record::CONTROL_BYTES + FRAME_BYTES];
            if body.len() != neoteletext_audio::record::CONTROL_BYTES {
                return None;
            }
            let length = neoteletext_audio::record::write(kind, body, &mut record)?;
            let Ok(Record::Control(control)) = neoteletext_audio::record::parse(record.get(..length)?) else {
                return None;
            };
            Some(BodyFacts {
                page: control.page,
                sub: None,
                sequence: control.sequence,
                after_current: control.flags & FLAG_AFTER_CURRENT != 0,
            })
        }
        TYPE_SPRITES => {
            let record = sprites::read(body).ok()?;
            Some(placed(record.page, record.subcode))
        }
        TYPE_NAPLPS_OBJECTS => {
            let record = naplps_objects::read(body).ok()?;
            Some(placed(record.page, record.subcode))
        }
        // Renderer parity PAR-20: a group's links are one whole sending,
        // part 0 of 1, which replaces the links the receiver held; a
        // sending of several parts is the server's to send directly.
        TYPE_DRAWING_LINKS => {
            let record = drawing_links::read(body).ok()?;
            (record.part == 0 && record.parts == 1).then(|| placed(record.page, record.subcode))
        }
        TYPE_REGIS_CURSOR => {
            let record = regis_cursor::read(body).ok()?;
            Some(placed(record.page, record.subcode))
        }
        _ => None,
    }
}

/// The bytes the record frame puts around a body: its header and its
/// CRC-32.
const FRAME_BYTES: usize = neoteletext_audio::RECORD_HEADER + neoteletext_audio::RECORD_CRC;

/// What a record of a page and a sub-code names.
const fn placed(page: PageAddress, subcode: SubCode) -> BodyFacts {
    BodyFacts { page: Some(page.mpp()), sub: Some(subcode.value()), sequence: 0, after_current: false }
}

#[cfg(test)]
mod tests;
