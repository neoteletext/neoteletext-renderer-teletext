//! The audio profile's constants (REQ-AUD-003 to REQ-AUD-015): the record
//! frame's sizes, the channels, the gain and fade bounds, and the live
//! sound's timing.

/// The profile version every record carries (REQ-AUD-003).
pub const PROFILE_VERSION: u8 = 2;
/// The largest record of the audio profile, header and CRC included
/// (REQ-AUD-003): 1 KB keeps the reassembly buffer of a microcontroller
/// small.
pub const MAX_RECORD: usize = 1024;
/// The largest record of any profile in this frame, header and CRC
/// included: a channel's records carry an address of 8,000 octets with the
/// fields around it (REQ-1661, DOC-1952), and each of its
/// 241 fragments keeps the one-byte index of REQ-AUD-007.
pub const MAX_FRAMED_RECORD: usize = 8192;
/// Record header: version, type and a two-byte body length.
pub const RECORD_HEADER: usize = 4;
/// The CRC-32 after the body.
pub const RECORD_CRC: usize = 4;
/// The largest body of an audio record.
pub const MAX_BODY: usize = MAX_RECORD - RECORD_HEADER - RECORD_CRC;
/// The largest body of any record in this frame.
pub const MAX_FRAMED_BODY: usize = MAX_FRAMED_RECORD - RECORD_HEADER - RECORD_CRC;
/// Channels, each with one voice (REQ-AUD-010).
pub const CHANNELS: u8 = 4;
/// No level changes faster than this (REQ-AUD-015): tellyboy measured a
/// level step as a click and glides every change over 5 ms
/// (`TB_AUDIO_DECLICK_MS`).
pub const MIN_FADE_MS: u16 = 5;
/// The largest fade a command may ask for, ten seconds.
pub const MAX_FADE_MS: u16 = 10_000;
/// Gain is a whole percentage (REQ-AUD-015).
pub const MAX_GAIN: u8 = 100;
/// The page field's value for a command that belongs to the whole service.
pub const SERVICE_SCOPE: u16 = 0xFFFF;
/// Blocks a live sound keeps (REQ-AUD-014): a ring, the newest replacing the
/// oldest. 32 blocks of 512 bytes are 16 KB, about 2 seconds of 8 kHz IMA
/// ADPCM.
pub const LIVE_RING_BLOCKS: u16 = 32;
/// A live sound fades out after this long with no new block (REQ-AUD-014),
/// the mute of EN 300 708 clause 6.7.1.1 at the time this profile sets.
pub const LIVE_STALL_MS: u64 = 2_000;
/// How a stalled live sound fades out.
pub const LIVE_STALL_FADE_MS: u16 = 100;
/// A live block this many blocks behind the newest that has still not
/// arrived is given up as silence, so a lost block delays nothing
/// (REQ-AUD-014).
pub const LIVE_LOOKAHEAD_BLOCKS: u16 = 3;
/// A progressive start waits for this many milliseconds of contiguous
/// sound (REQ-AUD-013).
pub const PROGRESSIVE_BUFFER_MS: u32 = 40;
/// The Opus decoders a receiver keeps (decision E.124), about 28 KB each:
/// one for a live sound and one for a stored sound arriving at once. A third
/// Opus sound waits for one to free; a stored sound frees its decoder once
/// every block is decoded.
pub const OPUS_DECODERS: usize = 2;

/// The instruments a receiver keeps (REQ-6411): one for each channel, since
/// each channel may play an instrument's sound and a live one keeps its
/// instrument for as long as it plays. Each holds a SID chip and a DX7 rack
/// where they are built in, 13,552 bytes together, and room for the largest
/// analogue synthesizer built in with its bank: 20,456 bytes more with the
/// CS-80, 13,744 with the Jupiter-8, 13,288 with the Juno, 6,008 with the
/// Prophet-5 and 3,784 with the Minimoog (measured on x86-64, 17 November
/// 2026). With every instrument the four are 136,032 bytes.
pub const INSTRUMENT_LANES: usize = CHANNELS as usize;

/// A kibibyte: the audio design's "KB" and "MB" of receiver memory are
/// binary multiples (DEC-2372), as a port's buffers are.
const KIB: usize = 1024;

/// What a receiver holds (REQ-AUD-017): the bytes of its object store, the
/// objects it keeps, and the records it reassembles at once. A port sizes
/// its receiver, publishes these, and holds at least its level's minimum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReceiverLimits {
    /// Bytes of the object store's arena.
    pub store_bytes: usize,
    /// Objects held at once.
    pub objects: usize,
    /// Records reassembled at once (REQ-AUD-007).
    pub records_in_flight: usize,
}

impl ReceiverLimits {
    /// True when every figure is at least `minimum`'s.
    #[must_use]
    pub const fn meets(&self, minimum: &Self) -> bool {
        self.store_bytes >= minimum.store_bytes
            && self.objects >= minimum.objects
            && self.records_in_flight >= minimum.records_in_flight
    }
}

/// The least a microcontroller receiver, Level A, holds (REQ-AUD-017;
/// `docs/spec/design/audio.md` section 12): 16 KB of sounds, about four
/// seconds of IMA ADPCM at 8 kHz, 8 objects and 2 records in flight.
pub const LEVEL_A_MINIMUM: ReceiverLimits = ReceiverLimits { store_bytes: 16 * KIB, objects: 8, records_in_flight: 2 };

/// The least a desktop receiver, Level B, holds (REQ-AUD-017;
/// `docs/spec/design/audio.md` section 12): 4 MB of sounds, 256 objects and
/// 8 records in flight.
pub const LEVEL_B_MINIMUM: ReceiverLimits =
    ReceiverLimits { store_bytes: 4 * KIB * KIB, objects: 256, records_in_flight: 8 };
