//! Records: the one form every carriage delivers (REQ-AUD-003 to
//! REQ-AUD-006). Integers are big-endian; a record is at most
//! [`MAX_RECORD`](crate::MAX_RECORD) bytes and is checked against every bound before anything
//! in it is used.

use crate::{
    CHANNELS, Codec, DUCK_PERCENT, FIRST_MAGAZINE, LAST_MAGAZINE, MAGAZINE_SCOPE, MAX_BODY, MAX_FADE_MS, MAX_GAIN,
    MOST_BANK_BYTES, MOST_BANKS, MOST_EVENT_SOUND_BLOCKS, MOST_EVENT_SOUND_BYTES, MUSIC_PLAY, MUSIC_SILENT,
    PROFILE_VERSION, SERVICE_SCOPE, sample_rate,
};
use frame::Fields;

mod frame;

pub use frame::{RecordError, framed_length, record_length, split, split_versioned, write, write_version};

/// Record type: announce a sound.
pub const TYPE_OBJECT: u8 = 1;
/// Record type: blocks of a sound.
pub const TYPE_DATA: u8 = 2;
/// Record type: a command.
pub const TYPE_CONTROL: u8 = 3;
/// Record type: the input system's cursor record (area INT,
/// DOC-1954), carried with this profile's records on
/// the same data channel because EN 300 708 Table 16 gives data channel 13
/// to Datavideo. It is always one fragment; this receiver leaves it alone.
pub const TYPE_CURSOR: u8 = 4;
/// Record type: the input profile's control record (WELCOME and the input
/// modes, DOC-1953), carried as the cursor
/// record is, one fragment; this receiver leaves it alone.
pub const TYPE_INPUT_CONTROL: u8 = 5;

/// Record type: the channel's index, its magazines and pages
/// (DOC-1955), read by the web agent; one
/// or more fragments; this receiver leaves it alone.
pub const TYPE_INDEX: u8 = 6;
/// Record type: a log line for the channel's author (DOC-1956,
/// REQ-INP-026), read by the web agent; this receiver leaves it alone.
pub const TYPE_LOG: u8 = 7;
// Record type 8, `SET_COOKIE`, is retired by viewer state (DEC-6471,
// VS-204) and never reused: no longer a channel record, so the audio
// receiver counts one as refused.

/// Record type: a sub-page's TTI Link Extension links (DOC-1958,
/// REQ-1494), read by the web agent; this receiver leaves it alone.
pub const TYPE_LINKS: u8 = 9;

/// Record type: a page's list of sub-pages (DOC-1959,
/// REQ-1503), read by the web agent; this receiver leaves it alone.
pub const TYPE_SUBPAGES: u8 = 10;

/// Record type: a channel's redirect, to another page or another channel
/// (DOC-1960, REQ-1688), read by the web agent; this receiver
/// leaves it alone.
pub const TYPE_REDIRECT: u8 = 11;

/// Record type: a selection refused, or a program not answering (DOC-1961,
/// REQ-1481, DEC-1730), read by the web agent; this receiver
/// leaves it alone.
pub const TYPE_DENIED: u8 = 12;

/// Record type: bytes of a sub-page's ReGIS (DEC-6335), read by the web
/// agent's record reader for the ReGIS renderer; this receiver leaves it
/// alone. Type 13 is kept for the CHANNEL record (REQ-3581).
pub const TYPE_REGIS: u8 = 14;

/// Record type: the numbered drawings of a sub-page's ReGIS picture, or the
/// objects of its NAPLPS picture (ANIM-11), that are links, and where each
/// leads (REQ-6370; widened to NAPLPS objects by renderer parity PAR-2,
/// REQ-6761, its bytes unchanged), read by the web agent's record reader;
/// this receiver leaves it alone.
pub const TYPE_DRAWING_LINKS: u8 = 15;

/// Record type: the numbered drawing of a sub-page's ReGIS picture, or the
/// object of its NAPLPS picture, that is its cursor (REQ-6371; widened to
/// NAPLPS objects by renderer parity PAR-2, REQ-6761, its bytes unchanged),
/// read by the web agent's record reader; this receiver leaves it alone.
pub const TYPE_REGIS_CURSOR: u8 = 16;

/// Record type: bytes of a sub-page's NAPLPS (REQ-6408 item 2), read by
/// the web agent's record reader for the NAPLPS renderer; this receiver
/// leaves it alone.
pub const TYPE_NAPLPS: u8 = 17;

/// Record type: a viewer-state challenge (DEC-6471, VS-204), read by the
/// web agent's record reader (`neoteletext_web::records::state`); this
/// receiver leaves it alone.
pub const TYPE_STATE_CHALLENGE: u8 = 18;

/// Record type: the end of a remembered viewer state (DEC-6471, VS-204),
/// read by the web agent; this receiver leaves it alone.
pub const TYPE_STATE_END: u8 = 19;

/// Record type: an offer to keep viewer state (DEC-6471, VS-204), read by
/// the web agent; this receiver leaves it alone.
pub const TYPE_STATE_KEEP_OFFER: u8 = 20;
/// Record type: a page's rule table (neoteletext-common page-rules.md
/// RULE-41), read by neoteletext-web's record reader, as the six that
/// follow are; this receiver leaves them alone.
pub const TYPE_RULES: u8 = 21;
/// Record type: a query of a page's rule state (RULE-47).
pub const TYPE_RULES_QUERY: u8 = 22;
/// Record type: animation operations (neoteletext-common animation.md
/// ANIM-6).
pub const TYPE_ANIMATE: u8 = 23;
/// Record type: a sub-page's teletext sprites (ANIM-7).
pub const TYPE_SPRITES: u8 = 24;
/// Record type: a sub-page's NAPLPS objects (ANIM-11).
pub const TYPE_NAPLPS_OBJECTS: u8 = 25;
/// Record type: bytes of a page's sprite sheet (ANIM-17).
pub const TYPE_SHEET: u8 = 26;
/// Record type: the begin or the end of an atomic frame (ANIM-2).
pub const TYPE_FRAME: u8 = 27;
/// Record type: one MIDI message to a local sound (neoteletext-common
/// page-rules.md RULE-21), the first of the six commands a page's rule
/// table runs and the server may send directly (RULE-114), read by
/// neoteletext-web's record reader; this receiver leaves them alone.
pub const TYPE_LOCAL_MIDI: u8 = 28;
/// Record type: a change of one of a page's variables (RULE-99).
pub const TYPE_VARIABLE: u8 = 29;
/// Record type: a start or a stop of one of a page's timers (RULE-100).
pub const TYPE_TIMER: u8 = 30;
/// Record type: a report asked of a page (RULE-22).
pub const TYPE_REPORT: u8 = 31;
/// Record type: a run of a group of a page's rule table (RULE-102).
pub const TYPE_RUN: u8 = 32;
/// Record type: a start or a stop of a sequence (RULE-103).
pub const TYPE_SEQUENCE: u8 = 33;

/// Record type: a page's magazine-music setting (REQ-6926 item 1,
/// DEC-6952), read by this receiver. Only profile version 3 defines it, so
/// it is written with [`write_version`] and refused in a record of version
/// 2.
pub const TYPE_MAGAZINE_MUSIC: u8 = 34;

/// Whether a record type belongs to the input profile, which shares this
/// profile's carriage and is left alone by the audio receiver.
#[must_use]
pub const fn is_input_type(kind: u8) -> bool {
    kind == TYPE_CURSOR || kind == TYPE_INPUT_CONTROL
}

/// Whether a record type is one of the channel's records beside sound
/// (DOC-1951): the input profile's, the index, LOG, links, sub-pages,
/// redirect, denied, ReGIS, NAPLPS, the three of viewer state, the seven
/// of the page rule table and animation, and the six commands. The audio
/// receiver leaves every one alone, and counts none as refused.
#[must_use]
pub const fn is_channel_type(kind: u8) -> bool {
    is_input_type(kind)
        || kind == TYPE_INDEX
        || kind == TYPE_LOG
        || kind == TYPE_LINKS
        || kind == TYPE_SUBPAGES
        || kind == TYPE_REDIRECT
        || kind == TYPE_DENIED
        || kind == TYPE_REGIS
        || kind == TYPE_DRAWING_LINKS
        || kind == TYPE_REGIS_CURSOR
        || kind == TYPE_NAPLPS
        || kind == TYPE_STATE_CHALLENGE
        || kind == TYPE_STATE_END
        || kind == TYPE_STATE_KEEP_OFFER
        // The page rule table's and animation's seven (RULE-41, ANIM-2).
        || kind == TYPE_RULES
        || kind == TYPE_RULES_QUERY
        || kind == TYPE_ANIMATE
        || kind == TYPE_SPRITES
        || kind == TYPE_NAPLPS_OBJECTS
        || kind == TYPE_SHEET
        || kind == TYPE_FRAME
        // The six commands of the second form (RULE-114).
        || (kind >= TYPE_LOCAL_MIDI && kind <= TYPE_SEQUENCE)
}

/// OBJECT's fixed fields: handle, generation, id, codec, rate, block bytes,
/// sample count, block count.
pub const OBJECT_FIXED: usize = 1 + 2 + 4 + 1 + 1 + 2 + 4 + 2;
/// DATA's fixed fields: handle, generation, first block.
pub const DATA_FIXED: usize = 1 + 2 + 2;
/// The fixed fields of a DATA record of an event codec (MIDI-42): handle,
/// generation, first block, and the byte offset of the first block within
/// the sound, 4 bytes, 0 for a live sound.
pub const EVENT_DATA_FIXED: usize = DATA_FIXED + EVENT_OFFSET_BYTES;
/// The bytes of a DATA record's byte offset, for an event codec (MIDI-42).
pub const EVENT_OFFSET_BYTES: usize = 4;
/// The bytes of the length before each block of an event codec in a DATA
/// record, big-endian (MIDI-42).
pub const EVENT_BLOCK_LENGTH_BYTES: usize = 2;
/// CONTROL's fields: channel, sequence, operation, handle, generation, gain,
/// fade, flags, duck, page.
pub const CONTROL_BYTES: usize = 1 + 2 + 1 + 1 + 2 + 1 + 2 + 1 + 1 + 2;
/// `MAGAZINE_MUSIC`'s fields (DEC-6952 item 1): page, sub-code, setting.
pub const MAGAZINE_MUSIC_BYTES: usize = 2 + 2 + 1;
/// The longest label, in bytes of UTF-8 (REQ-AUD-004).
pub const MAX_LABEL: usize = 64;
/// The longest language tag.
pub const MAX_LANGUAGE: usize = 16;
/// Extension tag of a label.
pub const TAG_LABEL: u8 = 1;
/// Extension tag of a language.
pub const TAG_LANGUAGE: u8 = 2;
/// Extension tag of a sound of the synth6581 family's chip and clock, one
/// byte, on codec 12 (MIDI-48; S6581-39): bit 0 is 0 for synth6581 and 1 for
/// synth8580; bit 1 is 0 for the PAL clock and 1 for the NTSC clock; the
/// rest are 0. Without it: synth6581 on the PAL clock.
pub const TAG_SYNTH6581_CHIP: u8 = 3;
/// Extension tag of the banks a sound names, 1 to [`MOST_BANKS`] bytes, on
/// codecs 13 to 18 (MIDI-48, MIDI-57): the handles of its banks, bank index
/// 0 first. Required on those codecs.
pub const TAG_BANKS: u8 = 4;
/// Extension tag of the variant a sound plays, one byte, on codecs 14 and
/// 16 (MIDI-48), below [`Codec::variants`]: for codec 14, 0 synth106 and 1
/// synth60; for codec 16, 0 synth53, 1 synth52 and 2 synth51. Without the
/// tag: 0.
pub const TAG_VARIANT: u8 = 5;
/// Extension tag of the family a bank is for, one byte, on codec 19
/// (MIDI-48): the code of that family's codec, 13 to 18
/// ([`Codec::is_family_code`]). Required on that codec.
pub const TAG_BANK_FAMILY: u8 = 6;
/// Extension tag of a synth8 sound's keyboard, two bytes, on codec 17: the
/// key mode ([`SYNTH8_KEYS_WHOLE`], [`SYNTH8_KEYS_DUAL`] or
/// [`SYNTH8_KEYS_SPLIT`]) and the split point, a MIDI key of 0 to
/// [`MIDI_DATA_MAX`], from which up a split keyboard plays the upper patch.
/// Without the tag: the whole keyboard plays the upper patch. The adopted
/// interface has no message for either (`synth8.md`, S8-18), so a receiver
/// that joins a live sound has them from the OBJECT.
pub const TAG_SYNTH8_KEYS: u8 = 7;
/// Extension tag of the total bytes of a stored sound's blocks, 4 bytes,
/// big-endian, 0 to [`MOST_EVENT_SOUND_BYTES`], on codecs 12 to 18
/// (MIDI-41). Required on a stored sound of those codecs; a live sound's
/// OBJECT must not carry it.
pub const TAG_TOTAL_BYTES: u8 = 8;
/// Extension tag of a local sound (page-rules.md, RULE-60), one byte,
/// [`LOCAL_SOUND`], on codecs 13 to 18, the families that play notes: the
/// sound is an instrument the page keeps open and its rules play, announced
/// as a stored sound of one block, its setup ([`LOCAL_BLOCKS`]). A receiver
/// never renders it from blocks and never streams it: a play gives the sink
/// the local start ([`crate::port::VoiceSink::local`]). On the synth6581
/// family, which has no note messages, and on every other codec the tag
/// means nothing and is skipped, as any unknown tag is.
pub const TAG_LOCAL: u8 = 9;
/// [`TAG_LOCAL`]'s one value (RULE-60: "with one value byte, 1").
pub const LOCAL_SOUND: u8 = 1;
/// The blocks of a local sound: one, its setup, an event list with every
/// event at tick 0, applied in order when the instrument is made (RULE-60).
pub const LOCAL_BLOCKS: u16 = 1;
/// [`TAG_SYNTH8_KEYS`]: eight voices of the upper patch.
pub const SYNTH8_KEYS_WHOLE: u8 = 0;
/// [`TAG_SYNTH8_KEYS`]: every key plays both patches, four voices each.
pub const SYNTH8_KEYS_DUAL: u8 = 1;
/// [`TAG_SYNTH8_KEYS`]: keys below the split point play the lower patch,
/// the others the upper, four voices each.
pub const SYNTH8_KEYS_SPLIT: u8 = 2;
/// The largest value of a MIDI data byte (MIDI 1.0: seven bits).
pub const MIDI_DATA_MAX: u8 = 0x7F;
// The record module states the MIDI layer's highest data byte for builds
// without that layer (a cue number's bound, page-rules.md RULE-109); where
// the layer is built the two are one figure.
#[cfg(feature = "midi")]
const _: () = assert!(MIDI_DATA_MAX == crate::midi::DATA_MAX);
/// [`TAG_SYNTH6581_CHIP`]'s bit for synth8580.
pub const SYNTH6581_CHIP_8580: u8 = 1;
/// [`TAG_SYNTH6581_CHIP`]'s bit for the NTSC clock.
pub const SYNTH6581_CLOCK_NTSC: u8 = 2;
/// The bits of [`TAG_SYNTH6581_CHIP`] that mean something.
const SYNTH6581_CHIP_BITS: u8 = SYNTH6581_CHIP_8580 | SYNTH6581_CLOCK_NTSC;

/// Command flag: start before the whole sound has arrived (REQ-AUD-013).
pub const FLAG_PROGRESSIVE: u8 = 1;
/// Command flag: repeat until stopped.
pub const FLAG_LOOP: u8 = 2;
/// Command flag: wait for the channel's current sound to end.
pub const FLAG_AFTER_CURRENT: u8 = 4;
const ALL_FLAGS: u8 = FLAG_PROGRESSIVE | FLAG_LOOP | FLAG_AFTER_CURRENT;

/// The lowest and highest page numbers a command may name (EN 300 706
/// clause 7.1.2).
const FIRST_PAGE: u16 = 0x100;
const LAST_PAGE: u16 = 0x8FF;

/// An OBJECT record: a sound's definition (REQ-AUD-004).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Object<'a> {
    /// The short name DATA and CONTROL use.
    pub handle: u8,
    /// Newer when the sound or its definition changes.
    pub generation: u16,
    /// Stable across generations within a service.
    pub id: u32,
    /// The codec, or `None` for a reserved code: the object is ignored.
    pub codec: Option<Codec>,
    /// Samples a second.
    pub rate_hz: u32,
    /// Bytes in each whole block.
    pub block_bytes: u16,
    /// Samples in the whole sound; 0 for a live sound.
    pub samples: u32,
    /// DATA blocks in the whole sound; 0 for a live sound.
    pub blocks: u16,
    /// The label, as sent, not yet cleaned for display.
    pub label: Option<&'a [u8]>,
    /// The BCP 47 language tag, as sent.
    pub language: Option<&'a [u8]>,
    /// A sound of the synth6581 family's chip and clock
    /// ([`TAG_SYNTH6581_CHIP`]); `None` is a synth6581 on the PAL clock.
    pub chip: Option<u8>,
    /// The handles of the banks a sound names ([`TAG_BANKS`]), bank index 0
    /// first; [`Object::banks`] gives them as a slice.
    pub banks: [u8; MOST_BANKS],
    /// How many of [`Object::banks`] are named: 0 where the tag is absent.
    pub bank_count: u8,
    /// The variant a sound plays ([`TAG_VARIANT`]); `None` is variant 0.
    pub variant: Option<u8>,
    /// The family a bank is for ([`TAG_BANK_FAMILY`]): the code of its
    /// codec.
    pub family: Option<u8>,
    /// A synth8 sound's key mode and split point
    /// ([`TAG_SYNTH8_KEYS`]); `None` is the whole keyboard.
    pub keys: Option<[u8; 2]>,
    /// The total bytes of a stored event sound's blocks
    /// ([`TAG_TOTAL_BYTES`]).
    pub total: Option<u32>,
    /// A local sound ([`TAG_LOCAL`], RULE-60): its one block is its setup,
    /// and it is played by rules, not rendered from blocks.
    pub local: bool,
}

impl Object<'_> {
    /// The handles of the banks the sound names, bank index 0 first.
    #[must_use]
    pub fn banks(&self) -> &[u8] {
        self.banks.get(..usize::from(self.bank_count)).unwrap_or(&[])
    }
}

/// A DATA record: whole blocks of a sound (REQ-AUD-005). Its layout after
/// the first block's number is its codec's, which the record does not say:
/// for an event codec it is the offset and length-prefixed blocks of
/// MIDI-42, which [`event_blocks`] reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Data<'a> {
    /// The sound's handle.
    pub handle: u8,
    /// Its generation.
    pub generation: u16,
    /// The index of the first block.
    pub first_block: u16,
    /// The blocks, back to back; for an event codec, the byte offset and
    /// then the blocks, each with its length.
    pub blocks: &'a [u8],
}

/// The byte offset of an event codec's DATA record's first block, and its
/// blocks, each a 2-byte length and that many bytes (MIDI-42): the offset
/// and an iterator of the blocks, which yields `None` and ends at a length
/// that runs past the record. `None` for a record too short for the offset.
#[must_use]
pub fn event_blocks(blocks: &[u8]) -> Option<(u32, EventBlocks<'_>)> {
    let (offset, rest) = blocks.split_first_chunk::<EVENT_OFFSET_BYTES>()?;
    Some((u32::from_be_bytes(*offset), EventBlocks { rest }))
}

/// The length-prefixed blocks of an event codec's DATA record (MIDI-42).
#[derive(Clone, Debug)]
pub struct EventBlocks<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for EventBlocks<'a> {
    type Item = Option<&'a [u8]>;

    fn next(&mut self) -> Option<Option<&'a [u8]>> {
        if self.rest.is_empty() {
            return None;
        }
        let block = self
            .rest
            .split_first_chunk::<EVENT_BLOCK_LENGTH_BYTES>()
            .and_then(|(length, rest)| rest.split_at_checked(usize::from(u16::from_be_bytes(*length))));
        let Some((block, rest)) = block else {
            self.rest = &[];
            return Some(None);
        };
        self.rest = rest;
        Some(Some(block))
    }
}

/// What a command does (REQ-AUD-011).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Operation {
    /// Start a sound.
    Play,
    /// Fade the channel out.
    Stop,
    /// Ramp the channel to a new gain.
    Gain,
}

/// A CONTROL record (REQ-AUD-006).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Control {
    /// 0 to 3.
    pub channel: u8,
    /// The channel's sequence number (REQ-AUD-010).
    pub sequence: u16,
    /// What to do.
    pub operation: Operation,
    /// The sound, for a play.
    pub handle: u8,
    /// The sound's generation, for a play.
    pub generation: u16,
    /// 0 to 100 per cent.
    pub gain: u8,
    /// The ramp, at least [`crate::MIN_FADE_MS`] once applied.
    pub fade_ms: u16,
    /// [`FLAG_PROGRESSIVE`], [`FLAG_LOOP`] and [`FLAG_AFTER_CURRENT`].
    pub flags: u8,
    /// 0 to 6: the attenuation of EN 300 708 Table 19 applied to the other
    /// channels while this sound plays.
    pub duck: u8,
    /// The page the command belongs to, or `None` for the whole service or
    /// a magazine.
    pub page: Option<u16>,
    /// The magazine a play belongs to, [`FIRST_MAGAZINE`] to
    /// [`LAST_MAGAZINE`] (REQ-6925 item 1, DEC-6951): the page field's
    /// [`MAGAZINE_SCOPE`] plus the magazine. `None` for a page's command
    /// or the whole service's; when it is given, `page` is `None`. Only a
    /// play in a record of profile version 3 names one.
    pub magazine: Option<u8>,
}

/// What a page does to its magazine's music while it is open (REQ-6926
/// item 1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MusicSetting {
    /// The music plays at its own gain: the setting of a page the sender
    /// has said nothing of.
    #[default]
    Play,
    /// The music is lowered by this duck code, 1 to 6, of EN 300 708
    /// Table 19 ([`DUCK_PERCENT`]).
    Duck(u8),
    /// The music fades out.
    Silent,
}

impl MusicSetting {
    /// The setting a `MAGAZINE_MUSIC` record's setting byte names (DEC-6952
    /// item 1): [`MUSIC_PLAY`], a duck code 1 to 6, or [`MUSIC_SILENT`];
    /// `None` for any other byte.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            MUSIC_PLAY => Some(Self::Play),
            MUSIC_SILENT => Some(Self::Silent),
            // Duck code 0 is no ducking, which is play and is written so.
            code if code > 0 && (code as usize) < DUCK_PERCENT.len() => Some(Self::Duck(code)),
            _ => None,
        }
    }

    /// The setting byte that names this setting; `None` for a duck code
    /// outside 1 to 6.
    #[must_use]
    pub const fn code(self) -> Option<u8> {
        match self {
            Self::Play => Some(MUSIC_PLAY),
            Self::Silent => Some(MUSIC_SILENT),
            Self::Duck(code) if code > 0 && (code as usize) < DUCK_PERCENT.len() => Some(code),
            Self::Duck(_) => None,
        }
    }
}

/// A `MAGAZINE_MUSIC` record (REQ-6926 item 1, DEC-6952): the setting of one
/// sub-page, keyed on its page number and sub-code together (DEC-6599).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageMusic {
    /// The page number, 0x100 to 0x8FF.
    pub page: u16,
    /// The sub-code: no bit outside the thirteen of a sub-code, and not the
    /// value that means any sub-page.
    pub sub: u16,
    /// What the page does to its magazine's music.
    pub setting: MusicSetting,
}

/// One record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Record<'a> {
    /// A sound's definition.
    Object(Object<'a>),
    /// Blocks of a sound.
    Data(Data<'a>),
    /// A command.
    Control(Control),
    /// A page's magazine-music setting.
    PageMusic(PageMusic),
}

/// Reads one whole record, exactly `bytes` long.
///
/// # Errors
/// Any failure of REQ-AUD-003 to REQ-AUD-006: the frame, the CRC, the type
/// or any field.
pub fn parse(bytes: &[u8]) -> Result<Record<'_>, RecordError> {
    let (version, kind, body) = split_versioned(bytes)?;
    // What only profile version 3 defines is refused in a record of
    // version 2, as a receiver of version 2 refuses it (DEC-6951 item 3).
    let magazines = version >= PROFILE_VERSION;
    if body.len() > MAX_BODY {
        return Err(RecordError::TooLong);
    }
    let mut fields = Fields::new(body);
    match kind {
        TYPE_OBJECT => object(&mut fields).map(Record::Object),
        TYPE_DATA => {
            let data = Data {
                handle: fields.u8()?,
                generation: fields.u16()?,
                first_block: fields.u16()?,
                blocks: fields.rest(),
            };
            if data.blocks.is_empty() {
                return Err(RecordError::BadValue("no blocks"));
            }
            Ok(Record::Data(data))
        }
        TYPE_CONTROL => {
            let control = control(&mut fields, magazines)?;
            fields.end()?;
            Ok(Record::Control(control))
        }
        TYPE_MAGAZINE_MUSIC if magazines => {
            let music = page_music(&mut fields)?;
            fields.end()?;
            Ok(Record::PageMusic(music))
        }
        other => Err(RecordError::UnknownType(other)),
    }
}

fn object<'a>(fields: &mut Fields<'a>) -> Result<Object<'a>, RecordError> {
    let handle = fields.u8()?;
    let generation = fields.u16()?;
    let id = fields.u32()?;
    let codec = Codec::from_code(fields.u8()?);
    let rate_hz = sample_rate(fields.u8()?).ok_or(RecordError::BadValue("sample rate"))?;
    let block_bytes = fields.u16()?;
    let samples = fields.u32()?;
    let blocks = fields.u16()?;
    let mut object = Object {
        handle,
        generation,
        id,
        codec,
        rate_hz,
        block_bytes,
        samples,
        blocks,
        label: None,
        language: None,
        chip: None,
        banks: [0; MOST_BANKS],
        bank_count: 0,
        variant: None,
        family: None,
        keys: None,
        total: None,
        local: false,
    };
    if let Some(codec) = codec {
        if !codec.allows_block(block_bytes) {
            return Err(RecordError::BadValue("block size"));
        }
        if !codec.allows_rate(rate_hz) {
            return Err(RecordError::BadValue("sample rate"));
        }
        let per_block = codec.samples_per_block(block_bytes);
        // A block the codec allows holds a sample; the check also leaves
        // the division no zero to panic on in the linked image
        // (REQ-GEN-019).
        if per_block == 0 {
            return Err(RecordError::BadValue("block size"));
        }
        let expected = samples.div_ceil(per_block);
        if u32::from(blocks) != expected {
            return Err(RecordError::BadValue("block count"));
        }
    }
    // Extensions: tag, length, value. Unknown tags are skipped.
    while !fields.is_empty() {
        let tag = fields.u8()?;
        let length = usize::from(fields.u8()?);
        let value = fields.take(length)?;
        match tag {
            TAG_LABEL if length <= MAX_LABEL => object.label = Some(value),
            TAG_LANGUAGE if length <= MAX_LANGUAGE => object.language = Some(value),
            TAG_LABEL | TAG_LANGUAGE => return Err(RecordError::BadValue("extension too long")),
            // The synthesizers' tags mean something only on their own
            // codecs (MIDI-48); on another they are skipped, as any unknown
            // tag is.
            TAG_SYNTH6581_CHIP if codec == Some(Codec::Synth6581) => match value {
                [chip] if chip & !SYNTH6581_CHIP_BITS == 0 => object.chip = Some(*chip),
                _ => return Err(RecordError::BadValue("chip")),
            },
            TAG_BANKS if codec.is_some_and(Codec::has_banks) => {
                if value.is_empty() || value.len() > MOST_BANKS {
                    return Err(RecordError::BadValue("banks"));
                }
                for (place, &handle) in object.banks.iter_mut().zip(value) {
                    *place = handle;
                }
                object.bank_count = u8::try_from(value.len()).map_err(|_| RecordError::BadValue("banks"))?;
            }
            TAG_VARIANT if codec.is_some_and(|codec| codec.variants() > 1) => match value {
                [variant] if codec.is_some_and(|codec| *variant < codec.variants()) => object.variant = Some(*variant),
                _ => return Err(RecordError::BadValue("variant")),
            },
            TAG_BANK_FAMILY if codec == Some(Codec::Bank) => match value {
                [family] if Codec::is_family_code(*family) => object.family = Some(*family),
                _ => return Err(RecordError::BadValue("family")),
            },
            TAG_SYNTH8_KEYS if codec == Some(Codec::Synth8) => match value {
                [mode, split] if *mode <= SYNTH8_KEYS_SPLIT && *split <= MIDI_DATA_MAX => {
                    object.keys = Some([*mode, *split]);
                }
                _ => return Err(RecordError::BadValue("keys")),
            },
            TAG_TOTAL_BYTES if codec.is_some_and(Codec::is_event) => match value {
                [a, b, c, d] => object.total = Some(u32::from_be_bytes([*a, *b, *c, *d])),
                _ => return Err(RecordError::BadValue("total bytes")),
            },
            // RULE-60: only a family that plays notes has a local sound.
            TAG_LOCAL if codec.is_some_and(Codec::has_banks) => match value {
                [LOCAL_SOUND] => object.local = true,
                _ => return Err(RecordError::BadValue("local")),
            },
            _ => {}
        }
    }
    if let Some(codec) = codec {
        check_synthesizer(&mut object, codec)?;
    }
    Ok(object)
}

/// The rules of an OBJECT of an event codec or a bank (MIDI-40, MIDI-41,
/// MIDI-55, MIDI-57).
fn check_synthesizer(object: &mut Object<'_>, codec: Codec) -> Result<(), RecordError> {
    if codec.is_event() {
        let live = object.samples == 0;
        let whole = u32::from(object.blocks).saturating_mul(crate::INSTRUMENT_BLOCK_SAMPLES);
        if !live && (object.blocks > MOST_EVENT_SOUND_BLOCKS || object.samples != whole) {
            return Err(RecordError::BadValue("block count"));
        }
        // A stored sound states its total, which bounds its store; a live
        // sound has none (MIDI-41).
        let most = u32::from(object.blocks).saturating_mul(u32::from(object.block_bytes)).min(MOST_EVENT_SOUND_BYTES);
        match (live, object.total) {
            (true, None) => {}
            (false, Some(total)) if total <= most => {}
            _ => return Err(RecordError::BadValue("total bytes")),
        }
    }
    // A local sound is a stored sound of one block, its setup (RULE-60).
    if object.local && (object.samples == 0 || object.blocks != LOCAL_BLOCKS) {
        return Err(RecordError::BadValue("local"));
    }
    // A sound that plays patches names its banks (MIDI-57).
    if codec.has_banks() && object.bank_count == 0 {
        return Err(RecordError::BadValue("banks"));
    }
    if codec == Codec::Bank {
        // A bank image of 1 to 16,384 bytes, never live (MIDI-55).
        if object.samples == 0 || object.samples > MOST_BANK_BYTES {
            return Err(RecordError::BadValue("bank bytes"));
        }
        // A bank says whose patches it holds; one for a family this build
        // leaves out is ignored, as a sound of that family is.
        let family = object.family.ok_or(RecordError::BadValue("family"))?;
        if Codec::from_code(family).is_none() {
            object.codec = None;
        }
    }
    Ok(())
}

/// Reads a `MAGAZINE_MUSIC` record's body (DEC-6952 item 1).
fn page_music(fields: &mut Fields<'_>) -> Result<PageMusic, RecordError> {
    let page = fields.u16()?;
    if !(FIRST_PAGE..=LAST_PAGE).contains(&page) {
        return Err(RecordError::BadValue("page"));
    }
    let sub = fields.u16()?;
    if !neoteletext_core::limits::is_sub_code(sub) || sub == neoteletext_core::limits::SUBCODE_ANY {
        return Err(RecordError::BadValue("sub-code"));
    }
    let setting = MusicSetting::from_code(fields.u8()?).ok_or(RecordError::BadValue("setting"))?;
    Ok(PageMusic { page, sub, setting })
}

fn control(fields: &mut Fields<'_>, magazines: bool) -> Result<Control, RecordError> {
    let channel = fields.u8()?;
    if channel >= CHANNELS {
        return Err(RecordError::BadValue("channel"));
    }
    let sequence = fields.u16()?;
    let operation = match fields.u8()? {
        1 => Operation::Play,
        2 => Operation::Stop,
        3 => Operation::Gain,
        _ => return Err(RecordError::BadValue("operation")),
    };
    let handle = fields.u8()?;
    let generation = fields.u16()?;
    let gain = fields.u8()?;
    if gain > MAX_GAIN {
        return Err(RecordError::BadValue("gain"));
    }
    let fade_ms = fields.u16()?;
    if fade_ms > MAX_FADE_MS {
        return Err(RecordError::BadValue("fade"));
    }
    let flags = fields.u8()?;
    if flags & !ALL_FLAGS != 0 {
        return Err(RecordError::BadValue("flags"));
    }
    let duck = fields.u8()?;
    if usize::from(duck) >= DUCK_PERCENT.len() {
        return Err(RecordError::BadValue("duck"));
    }
    let scope = fields.u16()?;
    let (page, magazine) = match scope {
        SERVICE_SCOPE => (None, None),
        page if (FIRST_PAGE..=LAST_PAGE).contains(&page) && page & 0xFF <= 0x99 && page & 0x0F <= 9 => {
            (Some(page), None)
        }
        // A play of profile version 3 may belong to a magazine (REQ-6925
        // item 1, DEC-6951); a stop or a set gain belongs to its channel.
        _ if magazines && operation == Operation::Play => match magazine_of_scope(scope) {
            Some(magazine) => (None, Some(magazine)),
            None => return Err(RecordError::BadValue("page")),
        },
        _ => return Err(RecordError::BadValue("page")),
    };
    Ok(Control { channel, sequence, operation, handle, generation, gain, fade_ms, flags, duck, page, magazine })
}

/// The magazine a CONTROL record's page field names, [`FIRST_MAGAZINE`] to
/// [`LAST_MAGAZINE`], when it is [`MAGAZINE_SCOPE`] plus one of them
/// (DEC-6951 item 1).
fn magazine_of_scope(scope: u16) -> Option<u8> {
    let magazine = u8::try_from(scope.checked_sub(MAGAZINE_SCOPE)?).ok()?;
    (FIRST_MAGAZINE..=LAST_MAGAZINE).contains(&magazine).then_some(magazine)
}

/// The magazine of page `page`, a page number 0x100 to 0x8FF: its hundreds
/// digit, [`FIRST_MAGAZINE`] to [`LAST_MAGAZINE`] (EN 300 706 clause 3.1);
/// `None` for a value that is no page number.
#[must_use]
pub fn magazine_of_page(page: u16) -> Option<u8> {
    if !(FIRST_PAGE..=LAST_PAGE).contains(&page) {
        return None;
    }
    u8::try_from(page >> u8::BITS).ok()
}

#[cfg(test)]
mod tests;
