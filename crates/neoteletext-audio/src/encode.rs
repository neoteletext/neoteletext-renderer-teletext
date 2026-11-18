//! Writing sounds, for publishing tools (REQ-AUD-020): IMA ADPCM encoding,
//! records, and records cut into broadcast packets. Opus is not encoded
//! here: a publisher's encoder writes the packets, and they are carried as
//! they are (decision E.124). The synthesizers' sounds are written from
//! their event lists ([`Sound::events`], [`Sound::live_events`],
//! [`event_blocks`]) and their banks from bank images ([`Sound::bank`]),
//! each checked with the receiver's own reader (midi-architecture.md,
//! MIDI-21). This module allocates.

use alloc::vec::Vec;

use neoteletext_core::packet::Packet;

use crate::idl::{FRAGMENT_DATA, packet};
use crate::ima::{State, encode as encode_code};
use neoteletext_audio_instruments::midi::block::{BlockFault, BlockWriter, WriteError, check};
use neoteletext_audio_instruments::midi::{Message, TICKS_PER_BLOCK};

use crate::record::{
    DATA_FIXED, EVENT_BLOCK_LENGTH_BYTES, EVENT_DATA_FIXED, TAG_LABEL, TYPE_CONTROL, TYPE_DATA, TYPE_OBJECT,
    record_length, write,
};
use crate::{
    Codec, MAX_BODY, MOST_BANK_BYTES, MOST_BANKS, MOST_EVENT_BLOCK_BYTES, MOST_EVENT_SOUND_BLOCKS,
    MOST_EVENT_SOUND_BYTES, SERVICE_SCOPE, rate_code,
};

/// A sound ready to publish.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sound {
    /// The handle commands name.
    pub handle: u8,
    /// Its generation.
    pub generation: u16,
    /// Stable across generations.
    pub id: u32,
    /// The codec.
    pub codec: Codec,
    /// Samples a second: one of [`crate::SAMPLE_RATES`].
    pub rate_hz: u32,
    /// Bytes in each whole block.
    pub block_bytes: u16,
    /// Samples in the whole sound.
    pub samples: u32,
    /// The encoded blocks, back to back; for an event codec, each block
    /// with its 2-byte length before it, as a DATA record carries them
    /// (MIDI-42).
    pub data: Vec<u8>,
    /// A label, at most [`crate::record::MAX_LABEL`] bytes.
    pub label: Option<Vec<u8>>,
    /// A sound of the synth6581 family's chip and clock byte
    /// ([`crate::record::TAG_SYNTH6581_CHIP`]).
    pub chip: Option<u8>,
    /// The handles of the banks a sound names, bank index 0 first
    /// ([`crate::record::TAG_BANKS`]): 1 to [`MOST_BANKS`] for a family with
    /// banks (MIDI-57). The server sends every one before the sound.
    pub banks: Vec<u8>,
    /// The variant a sound plays ([`crate::record::TAG_VARIANT`]), set from
    /// the sound's kind (MIDI-48).
    pub variant: Option<u8>,
    /// The family a bank is for ([`crate::record::TAG_BANK_FAMILY`]);
    /// [`Sound::bank`] sets it.
    pub family: Option<Codec>,
    /// A synth8 sound's key mode and split point
    /// ([`crate::record::TAG_SYNTH8_KEYS`]).
    pub keys: Option<[u8; 2]>,
    /// A local sound ([`crate::record::TAG_LOCAL`], RULE-60);
    /// [`Sound::local`] sets it.
    pub local: bool,
}

/// Why a sound could not be written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncodeError {
    /// A rate the profile does not carry.
    Rate(u32),
    /// A block size the codec does not allow.
    BlockSize(u16),
    /// No samples, or more blocks than a sound may have.
    Length,
    /// A label longer than the bound.
    Label,
    /// A codec this module does not encode: Opus comes as packets
    /// ([`Sound::opus`], [`Sound::packet_records`]).
    Encoder,
    /// Opus packets that are not all one length: the profile carries
    /// constant-rate packets, one a block (decision E.124).
    PacketLength,
    /// A synthesizer's block, counted from 0, that the receiver would not
    /// wholly act on (MIDI-21, MIDI-30): the fault its reader finds.
    Block {
        /// The block.
        index: usize,
        /// What is wrong with it.
        fault: BlockFault,
    },
    /// More blocks or more bytes than a stored event sound may have
    /// (MIDI-41, MIDI-104).
    Total,
    /// A sound of a family with banks that names none or more than
    /// [`MOST_BANKS`], or a sound without banks that names one (MIDI-57).
    Banks,
    /// A local sound's setup with an event after tick 0 (RULE-60: "every
    /// event at tick 0").
    Setup,
}

impl Sound {
    /// Encodes mono 16-bit samples.
    ///
    /// # Errors
    /// A rate or block size the profile does not allow, or a length it
    /// cannot carry.
    pub fn new(
        handle: u8,
        generation: u16,
        codec: Codec,
        rate_hz: u32,
        block_bytes: u16,
        pcm: &[i16],
    ) -> Result<Self, EncodeError> {
        rate_code(rate_hz).ok_or(EncodeError::Rate(rate_hz))?;
        if !codec.allows_block(block_bytes) {
            return Err(EncodeError::BlockSize(block_bytes));
        }
        let samples = u32::try_from(pcm.len()).map_err(|_| EncodeError::Length)?;
        let per_block = codec.samples_per_block(block_bytes);
        if samples == 0 || u16::try_from(samples.div_ceil(per_block)).is_err() {
            return Err(EncodeError::Length);
        }
        let data = match codec {
            Codec::Pcm16 => pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect(),
            Codec::ImaAdpcm => {
                let per_block = usize::try_from(per_block).map_err(|_| EncodeError::Length)?;
                let mut out = Vec::new();
                ima_blocks(&mut State { predictor: 0, index: 0 }, pcm, per_block, &mut out);
                out
            }
            // Neither is encoded from samples: Opus comes as packets and an
            // instrument's sound as its own blocks.
            _ => return Err(EncodeError::Encoder),
        };
        Ok(Self {
            handle,
            generation,
            id: u32::from(handle),
            codec,
            rate_hz,
            block_bytes,
            samples,
            data,
            label: None,
            chip: None,
            banks: Vec::new(),
            variant: None,
            family: None,
            keys: None,
            local: false,
        })
    }

    /// A live sound (REQ-AUD-014): announced with no samples, its blocks
    /// written as they come by a [`LiveEncoder`].
    ///
    /// # Errors
    /// A rate or block size the profile does not allow.
    pub fn live(
        handle: u8,
        generation: u16,
        codec: Codec,
        rate_hz: u32,
        block_bytes: u16,
    ) -> Result<Self, EncodeError> {
        rate_code(rate_hz).ok_or(EncodeError::Rate(rate_hz))?;
        if !codec.allows_block(block_bytes) {
            return Err(EncodeError::BlockSize(block_bytes));
        }
        Ok(Self {
            handle,
            generation,
            id: u32::from(handle),
            codec,
            rate_hz,
            block_bytes,
            samples: 0,
            data: Vec::new(),
            label: None,
            chip: None,
            banks: Vec::new(),
            variant: None,
            family: None,
            keys: None,
            local: false,
        })
    }

    /// An Opus sound from an encoder's packets, `packet_bytes` each, back to
    /// back in `packets`, of which the sound keeps `samples` samples: the
    /// last packet's frame may run past the sound's end (decision E.124).
    ///
    /// # Errors
    /// Packets of a length the profile does not carry or not all one length,
    /// none, more than a sound may have, or `samples` the packets do not hold.
    pub fn opus(
        handle: u8,
        generation: u16,
        packet_bytes: u16,
        packets: &[u8],
        samples: u32,
    ) -> Result<Self, EncodeError> {
        if !Codec::Opus.allows_block(packet_bytes) {
            return Err(EncodeError::BlockSize(packet_bytes));
        }
        let count = packets.len().checked_div(usize::from(packet_bytes)).unwrap_or(0);
        if !packets.len().is_multiple_of(usize::from(packet_bytes)) {
            return Err(EncodeError::PacketLength);
        }
        let blocks = u32::try_from(count).map_err(|_| EncodeError::Length)?;
        let held = blocks.saturating_mul(crate::OPUS_FRAME_SAMPLES);
        if count == 0
            || u16::try_from(count).is_err()
            || samples == 0
            || samples > held
            || samples.div_ceil(crate::OPUS_FRAME_SAMPLES) != blocks
        {
            return Err(EncodeError::Length);
        }
        Ok(Self {
            handle,
            generation,
            id: u32::from(handle),
            codec: Codec::Opus,
            rate_hz: crate::OPUS_RATE,
            block_bytes: packet_bytes,
            samples,
            data: packets.to_vec(),
            label: None,
            chip: None,
            banks: Vec::new(),
            variant: None,
            family: None,
            keys: None,
            local: false,
        })
    }

    /// A stored sound of an event codec from its blocks' event lists
    /// (MIDI-39 to MIDI-41): 1 to [`MOST_EVENT_SOUND_BLOCKS`] blocks of at
    /// most [`MOST_EVENT_BLOCK_BYTES`] bytes, at most
    /// [`MOST_EVENT_SOUND_BYTES`] in all, each read whole by the receiver's
    /// reader (MIDI-21). Its banks, chip, variant and keyboard are set
    /// afterwards, in the fields `banks`, `chip`, `variant` and `keys`.
    ///
    /// # Errors
    /// A codec that is not an event codec ([`EncodeError::Encoder`]), a
    /// block the reader would not wholly act on ([`EncodeError::Block`]),
    /// or no block, too many or too many bytes ([`EncodeError::Total`]).
    pub fn events<B: AsRef<[u8]>>(
        handle: u8,
        generation: u16,
        codec: Codec,
        blocks: &[B],
    ) -> Result<Self, EncodeError> {
        if !codec.is_event() {
            return Err(EncodeError::Encoder);
        }
        let count = u16::try_from(blocks.len()).map_err(|_| EncodeError::Total)?;
        if count == 0 || count > MOST_EVENT_SOUND_BLOCKS {
            return Err(EncodeError::Total);
        }
        let mut data = Vec::new();
        let mut longest: usize = 0;
        for (index, block) in blocks.iter().enumerate() {
            let block = block.as_ref();
            check(block).map_err(|fault| EncodeError::Block { index, fault })?;
            longest = longest.max(block.len());
            push_event_block(&mut data, block);
        }
        let total = data.len().saturating_sub(blocks.len().saturating_mul(EVENT_BLOCK_LENGTH_BYTES));
        if u32::try_from(total).map_or(true, |total| total > MOST_EVENT_SOUND_BYTES) {
            return Err(EncodeError::Total);
        }
        Ok(Self {
            handle,
            generation,
            id: u32::from(handle),
            codec,
            rate_hz: crate::INSTRUMENT_RATE,
            block_bytes: u16::try_from(longest).map_err(|_| EncodeError::Total)?,
            samples: u32::from(count).saturating_mul(crate::INSTRUMENT_BLOCK_SAMPLES),
            data,
            label: None,
            chip: None,
            banks: Vec::new(),
            variant: None,
            family: None,
            keys: None,
            local: false,
        })
    }

    /// A local sound (page-rules.md, RULE-60) of `codec`, a family that
    /// plays notes (codes 13 to 18): a stored sound of one block, its
    /// `setup`, an event list of at most [`MOST_EVENT_BLOCK_BYTES`] bytes
    /// with every event at tick 0, announced with
    /// [`crate::record::TAG_LOCAL`]. Its banks, variant and keyboard are set
    /// afterwards, as [`Sound::events`]' are.
    ///
    /// # Errors
    /// A codec that has no local sound ([`EncodeError::Encoder`]), a setup
    /// the receiver's reader would not wholly act on
    /// ([`EncodeError::Block`]), or an event of it after tick 0
    /// ([`EncodeError::Setup`]).
    pub fn local(handle: u8, generation: u16, codec: Codec, setup: &[u8]) -> Result<Self, EncodeError> {
        if !codec.has_banks() {
            return Err(EncodeError::Encoder);
        }
        let mut sound = Self::events(handle, generation, codec, &[setup])?;
        let late = crate::midi::block::Events::new(setup)
            .any(|read| matches!(read, crate::midi::block::Read::Event(event) if event.time != 0));
        if late {
            return Err(EncodeError::Setup);
        }
        sound.local = true;
        Ok(sound)
    }

    /// A live sound of an event codec (MIDI-20), announced with no blocks
    /// and with the largest block as its block bytes, so that any block it
    /// later carries fits the receiver's ring (MIDI-103). Its blocks go by
    /// [`Sound::live_event_records`].
    ///
    /// # Errors
    /// A codec that is not an event codec.
    pub fn live_events(handle: u8, generation: u16, codec: Codec) -> Result<Self, EncodeError> {
        if !codec.is_event() {
            return Err(EncodeError::Encoder);
        }
        let block_bytes = u16::try_from(MOST_EVENT_BLOCK_BYTES).map_err(|_| EncodeError::BlockSize(u16::MAX))?;
        Self::live(handle, generation, codec, crate::INSTRUMENT_RATE, block_bytes)
    }

    /// A bank (MIDI-55) for `family`, a codec of a family with banks, from
    /// its image of 1 to [`MOST_BANK_BYTES`] bytes in the family's form
    /// (MIDI-56), cut into pieces of [`crate::BANK_PIECE_BYTES`].
    ///
    /// # Errors
    /// A `family` without banks ([`EncodeError::Encoder`]), or an image
    /// empty or too long ([`EncodeError::Length`]).
    pub fn bank(handle: u8, generation: u16, family: Codec, image: &[u8]) -> Result<Self, EncodeError> {
        if !family.has_banks() {
            return Err(EncodeError::Encoder);
        }
        let samples = u32::try_from(image.len()).map_err(|_| EncodeError::Length)?;
        if samples == 0 || samples > MOST_BANK_BYTES {
            return Err(EncodeError::Length);
        }
        let piece = u16::try_from(crate::BANK_PIECE_BYTES.min(image.len())).map_err(|_| EncodeError::Length)?;
        Ok(Self {
            handle,
            generation,
            id: u32::from(handle),
            codec: Codec::Bank,
            rate_hz: crate::INSTRUMENT_RATE,
            block_bytes: piece,
            samples,
            data: image.to_vec(),
            label: None,
            chip: None,
            banks: Vec::new(),
            variant: None,
            family: Some(family),
            keys: None,
            local: false,
        })
    }

    /// The DATA records of a live event sound's next blocks, numbered from
    /// block `first` (counting modulo 2¹⁶): each block checked by the
    /// receiver's reader, as many whole blocks to a record as fit, with an
    /// offset of 0 (MIDI-42, MIDI-47).
    ///
    /// # Errors
    /// Not a live event sound ([`EncodeError::Encoder`]), or a block the
    /// receiver would not wholly act on ([`EncodeError::Block`]).
    pub fn live_event_records<B: AsRef<[u8]>>(&self, first: u16, blocks: &[B]) -> Result<Vec<Vec<u8>>, EncodeError> {
        if !self.codec.is_event() || self.samples != 0 {
            return Err(EncodeError::Encoder);
        }
        let mut data = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            check(block.as_ref()).map_err(|fault| EncodeError::Block { index, fault })?;
            push_event_block(&mut data, block.as_ref());
        }
        Ok(self.event_records(&data, first, false))
    }

    /// DATA records of an event codec's length-prefixed blocks, numbered
    /// from block `first`: as many whole blocks in each as fit (MIDI-47),
    /// each record carrying the byte offset of its first block within the
    /// sound, or 0 for a live sound (MIDI-42).
    fn event_records(&self, data: &[u8], mut first: u16, stored: bool) -> Vec<Vec<u8>> {
        let mut records = Vec::new();
        let mut rest = data;
        let mut offset: u32 = 0;
        while !rest.is_empty() {
            let mut body = Vec::with_capacity(MAX_BODY);
            body.push(self.handle);
            body.extend_from_slice(&self.generation.to_be_bytes());
            body.extend_from_slice(&first.to_be_bytes());
            body.extend_from_slice(&(if stored { offset } else { 0 }).to_be_bytes());
            while let Some((length, after)) = rest.split_first_chunk::<EVENT_BLOCK_LENGTH_BYTES>() {
                let length = usize::from(u16::from_be_bytes(*length));
                let whole = EVENT_BLOCK_LENGTH_BYTES.saturating_add(length);
                // MIDI-30 makes every block fit an empty record, so a record
                // always takes at least one.
                if body.len().saturating_add(whole) > MAX_BODY && body.len() > EVENT_DATA_FIXED {
                    break;
                }
                let (block, after) = after.split_at_checked(length).unwrap_or((after, &[]));
                push_event_block(&mut body, block);
                offset = offset.saturating_add(u32::try_from(length).unwrap_or(u32::MAX));
                first = first.wrapping_add(1);
                rest = after;
            }
            records.push(frame(TYPE_DATA, &body));
        }
        records
    }

    /// The DATA records of a live Opus sound's next packets, `block_bytes`
    /// each, back to back in `packets`, numbered from block `first` (counting
    /// modulo 2¹⁶): as many to a record as fit, since a packet is small.
    ///
    /// # Errors
    /// Not an Opus sound, or packets that are not all its length.
    pub fn packet_records(&self, first: u16, packets: &[u8]) -> Result<Vec<Vec<u8>>, EncodeError> {
        // A synthesizer's live blocks go by `Sound::live_event_records`.
        if self.codec != Codec::Opus {
            return Err(EncodeError::Encoder);
        }
        if !packets.len().is_multiple_of(usize::from(self.block_bytes)) {
            return Err(EncodeError::PacketLength);
        }
        let per_record =
            MAX_BODY.saturating_sub(DATA_FIXED).checked_div(usize::from(self.block_bytes)).unwrap_or(1).max(1);
        Ok(self.records_of(packets, first, per_record))
    }

    /// Samples in each whole block.
    #[must_use]
    pub fn samples_per_block(&self) -> u32 {
        self.codec.samples_per_block(self.block_bytes)
    }

    /// Blocks in the whole sound.
    #[must_use]
    pub fn blocks(&self) -> u16 {
        if self.codec.is_event() {
            return u16::try_from(self.samples / crate::INSTRUMENT_BLOCK_SAMPLES).unwrap_or(u16::MAX);
        }
        u16::try_from(self.samples.div_ceil(self.codec.samples_per_block(self.block_bytes))).unwrap_or(u16::MAX)
    }

    /// The OBJECT record that announces the sound.
    ///
    /// # Errors
    /// A label longer than the bound.
    pub fn object_record(&self) -> Result<Vec<u8>, EncodeError> {
        let mut body = Vec::new();
        body.push(self.handle);
        body.extend_from_slice(&self.generation.to_be_bytes());
        body.extend_from_slice(&self.id.to_be_bytes());
        body.push(self.codec.code());
        body.push(rate_code(self.rate_hz).ok_or(EncodeError::Rate(self.rate_hz))?);
        body.extend_from_slice(&self.block_bytes.to_be_bytes());
        body.extend_from_slice(&self.samples.to_be_bytes());
        body.extend_from_slice(&self.blocks().to_be_bytes());
        if let Some(label) = &self.label {
            if label.len() > crate::record::MAX_LABEL {
                return Err(EncodeError::Label);
            }
            body.push(TAG_LABEL);
            body.push(u8::try_from(label.len()).map_err(|_| EncodeError::Label)?);
            body.extend_from_slice(label);
        }
        for (tag, value) in [
            (crate::record::TAG_SYNTH6581_CHIP, self.chip),
            (crate::record::TAG_VARIANT, self.variant),
            (crate::record::TAG_BANK_FAMILY, self.family.map(Codec::code)),
        ] {
            if let Some(value) = value {
                body.extend_from_slice(&[tag, 1, value]);
            }
        }
        if self.codec.has_banks() == self.banks.is_empty() || self.banks.len() > MOST_BANKS {
            return Err(EncodeError::Banks);
        }
        if !self.banks.is_empty() {
            body.push(crate::record::TAG_BANKS);
            body.push(u8::try_from(self.banks.len()).map_err(|_| EncodeError::Banks)?);
            body.extend_from_slice(&self.banks);
        }
        if let Some([mode, split]) = self.keys {
            body.extend_from_slice(&[crate::record::TAG_SYNTH8_KEYS, 2, mode, split]);
        }
        if self.codec.is_event() && self.samples != 0 {
            let total =
                self.data.len().saturating_sub(usize::from(self.blocks()).saturating_mul(EVENT_BLOCK_LENGTH_BYTES));
            let total = u32::try_from(total).map_err(|_| EncodeError::Total)?;
            body.extend_from_slice(&[crate::record::TAG_TOTAL_BYTES, 4]);
            body.extend_from_slice(&total.to_be_bytes());
        }
        if self.local {
            body.extend_from_slice(&[crate::record::TAG_LOCAL, 1, crate::record::LOCAL_SOUND]);
        }
        Ok(frame(TYPE_OBJECT, &body))
    }

    /// The DATA records that carry the sound: as many whole blocks in each
    /// as fit a record (MIDI-47).
    #[must_use]
    pub fn data_records(&self) -> Vec<Vec<u8>> {
        if self.codec.is_event() {
            return self.event_records(&self.data, 0, true);
        }
        let per_record =
            MAX_BODY.saturating_sub(DATA_FIXED).checked_div(usize::from(self.block_bytes)).unwrap_or(1).max(1);
        self.records_of(&self.data, 0, per_record)
    }

    /// DATA records of encoded blocks, `per_record` to a record, numbered
    /// from block `first`.
    fn records_of(&self, data: &[u8], mut first: u16, per_record: usize) -> Vec<Vec<u8>> {
        let block = usize::from(self.block_bytes);
        let mut records = Vec::new();
        for chunk in data.chunks(block.saturating_mul(per_record)) {
            let mut body = Vec::with_capacity(DATA_FIXED.saturating_add(chunk.len()));
            body.push(self.handle);
            body.extend_from_slice(&self.generation.to_be_bytes());
            body.extend_from_slice(&first.to_be_bytes());
            body.extend_from_slice(chunk);
            records.push(frame(TYPE_DATA, &body));
            first = first.wrapping_add(u16::try_from(per_record).unwrap_or(u16::MAX));
        }
        records
    }
}

/// The encoder of a live sound's blocks (REQ-AUD-014), which carries what
/// one call learns into the next: the IMA ADPCM step index the last block
/// reached, the samples of a block not yet whole, and the next block's
/// number. A live sound encoded in many calls is so byte for byte the sound
/// encoded in one. Each call starting afresh, from the smallest step, cost
/// the start of every batch several samples of adaptation (the server
/// measured 2 to 7 decibels of signal-to-noise ratio lost at each restart),
/// and a batch that ended part way through a block put silence into the
/// middle of the sound.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LiveEncoder {
    /// The live sound, for its handle, generation, codec and block size.
    sound: Sound,
    /// Samples in each whole block, at least one.
    per_block: usize,
    /// The number of the next block written, counting modulo 2¹⁶.
    next: u16,
    /// The IMA ADPCM encoder's state where the last block ended.
    state: State,
    /// Samples of the next block, fewer than [`LiveEncoder::per_block`].
    pending: Vec<i16>,
}

impl LiveEncoder {
    /// An encoder for `sound`, made by [`Sound::live`], whose next block is
    /// numbered `first`.
    ///
    /// # Errors
    /// An Opus sound, whose packets come from a publisher's encoder and go
    /// by [`Sound::packet_records`] (decision E.124).
    pub fn new(sound: &Sound, first: u16) -> Result<Self, EncodeError> {
        if sound.codec == Codec::Opus {
            return Err(EncodeError::Encoder);
        }
        Ok(Self {
            sound: sound.clone(),
            per_block: usize::try_from(sound.samples_per_block()).unwrap_or(1).max(1),
            next: first,
            state: State { predictor: 0, index: 0 },
            pending: Vec::new(),
        })
    }

    /// The live sound this encoder writes.
    #[must_use]
    pub fn sound(&self) -> &Sound {
        &self.sound
    }

    /// The number the next block written will carry.
    #[must_use]
    pub fn next_block(&self) -> u16 {
        self.next
    }

    /// The DATA records of every block `pcm` completes, one block a record
    /// so each is sent as soon as it exists. Samples left over, too few for
    /// a block, wait for the next call or for [`LiveEncoder::finish`].
    pub fn records(&mut self, pcm: &[i16]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(pcm);
        let whole = self.pending.len().saturating_sub(self.pending.len().checked_rem(self.per_block).unwrap_or(0));
        let records = self.encode(whole);
        self.pending.drain(..whole);
        records
    }

    /// The DATA record of the samples still waiting, their block padded with
    /// silence, at the end of the sound; none if no sample waits.
    pub fn finish(&mut self) -> Vec<Vec<u8>> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        self.pending.resize(self.per_block, 0);
        let records = self.encode(self.per_block);
        self.pending.clear();
        records
    }

    /// The records of the first `count` waiting samples, a whole number of
    /// blocks, numbered on from the last.
    fn encode(&mut self, count: usize) -> Vec<Vec<u8>> {
        let samples = self.pending.get(..count).unwrap_or(&[]);
        let mut data = Vec::new();
        match self.sound.codec {
            Codec::Pcm16 => data.extend(samples.iter().flat_map(|sample| sample.to_le_bytes())),
            Codec::ImaAdpcm => ima_blocks(&mut self.state, samples, self.per_block, &mut data),
            // Neither is encoded from samples (`Sound::new` refuses both).
            _ => {}
        }
        let records = self.sound.records_of(&data, self.next, 1);
        let blocks = u16::try_from(records.len()).unwrap_or(u16::MAX);
        self.next = self.next.wrapping_add(blocks);
        records
    }
}

/// IMA ADPCM blocks of `per_block` samples, appended to `out`, starting from
/// `state` and leaving it where the last block ended: each block's header
/// holds its first sample and the step index the encoder had reached, so it
/// decodes on its own (the block layout of Microsoft's "DVI ADPCM Wave Type",
/// cited in [`crate::ima`]).
fn ima_blocks(state: &mut State, pcm: &[i16], per_block: usize, out: &mut Vec<u8>) {
    for block in pcm.chunks(per_block) {
        let Some((&first, rest)) = block.split_first() else { continue };
        state.predictor = first;
        out.extend_from_slice(&first.to_le_bytes());
        out.push(state.index);
        out.push(0);
        // Two codes a byte, low nibble first; an odd count is padded.
        for pair in rest.chunks(2) {
            let low = encode_code(state, pair.first().copied().unwrap_or(0));
            let high = match pair.get(1) {
                Some(&sample) => encode_code(state, sample),
                None => 0,
            };
            out.push(low | (high << 4));
        }
    }
}

/// Appends one event block with its 2-byte length (MIDI-42).
fn push_event_block(data: &mut Vec<u8>, block: &[u8]) {
    data.extend_from_slice(&u16::try_from(block.len()).unwrap_or(u16::MAX).to_be_bytes());
    data.extend_from_slice(block);
}

/// One timed event of a synthesizer's sound, as the server places it: its
/// absolute tick from the sound's start
/// ([`neoteletext_audio_instruments::midi::tick_of_microseconds`],
/// MIDI-17) and its message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimedEvent<'a> {
    /// The tick, 0.1 ms from the sound's start.
    pub tick: u64,
    /// The message.
    pub message: Message<'a>,
}

/// Why timed events could not be written into blocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlocksError {
    /// An event, counted from 0, earlier than the one before it.
    Order(usize),
    /// An event whose tick lies past the sound's last block.
    PastEnd(usize),
    /// An event whose message a block does not carry (MIDI-7, MIDI-8,
    /// MIDI-11, MIDI-27, MIDI-28).
    Message(usize),
    /// A block, counted from 0, that would pass its bytes (MIDI-30).
    Full(usize),
    /// A block that would hold more than 256 events (MIDI-31).
    TooManyEvents(usize),
}

/// Writes timed events into `blocks` blocks of event lists (MIDI-16,
/// MIDI-23): each event in block `floor(tick ÷ 200)` at time `tick mod
/// 200` (MIDI-17), in the order given, which must not go back in time
/// (MIDI-35). The server keeps its groups that must not be split together
/// (MIDI-37) and adds its restatements (MIDI-93) before calling this.
///
/// # Errors
/// The first event or block at fault, as [`BlocksError`] says.
pub fn event_blocks(events: &[TimedEvent<'_>], blocks: u16) -> Result<Vec<Vec<u8>>, BlocksError> {
    let per_block = u64::from(TICKS_PER_BLOCK);
    let mut out = Vec::with_capacity(usize::from(blocks));
    let mut buffer = [0_u8; MOST_EVENT_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    let mut current: u64 = 0;
    let mut last: u64 = 0;
    for (index, event) in events.iter().enumerate() {
        if event.tick < last {
            return Err(BlocksError::Order(index));
        }
        last = event.tick;
        let block = event.tick.checked_div(per_block).unwrap_or(0);
        if block >= u64::from(blocks) {
            return Err(BlocksError::PastEnd(index));
        }
        while current < block {
            out.push(writer.bytes().to_vec());
            writer.clear();
            current = current.saturating_add(1);
        }
        let time =
            u8::try_from(event.tick.checked_rem(per_block).unwrap_or(0)).map_err(|_| BlocksError::PastEnd(index))?;
        let at = usize::try_from(block).unwrap_or(usize::MAX);
        writer.push(time, &event.message).map_err(|error| match error {
            WriteError::Full => BlocksError::Full(at),
            WriteError::TooManyEvents => BlocksError::TooManyEvents(at),
            WriteError::Time | WriteError::Message => BlocksError::Message(index),
        })?;
    }
    while u64::try_from(out.len()).unwrap_or(u64::MAX) < u64::from(blocks) {
        out.push(writer.bytes().to_vec());
        writer.clear();
    }
    Ok(out)
}

/// A command, written as a CONTROL record (REQ-AUD-006).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Command {
    /// 0 to 3.
    pub channel: u8,
    /// Newer than the channel's last command.
    pub sequence: u16,
    /// 1 play, 2 stop, 3 gain.
    pub operation: u8,
    /// The sound's handle, for a play.
    pub handle: u8,
    /// The sound's generation, for a play.
    pub generation: u16,
    /// 0 to 100.
    pub gain: u8,
    /// Milliseconds.
    pub fade_ms: u16,
    /// The flags of [`crate::record`].
    pub flags: u8,
    /// 0 to 6.
    pub duck: u8,
    /// A page, or `None` for the whole service.
    pub page: Option<u16>,
}

impl Command {
    /// The CONTROL record.
    #[must_use]
    pub fn record(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.push(self.channel);
        body.extend_from_slice(&self.sequence.to_be_bytes());
        body.push(self.operation);
        body.push(self.handle);
        body.extend_from_slice(&self.generation.to_be_bytes());
        body.push(self.gain);
        body.extend_from_slice(&self.fade_ms.to_be_bytes());
        body.push(self.flags);
        body.push(self.duck);
        body.extend_from_slice(&self.page.unwrap_or(SERVICE_SCOPE).to_be_bytes());
        frame(TYPE_CONTROL, &body)
    }
}

/// A record: header, body and CRC-32 (REQ-AUD-003). A body longer than
/// [`MAX_BODY`] gives an empty record, which no receiver accepts.
#[must_use]
pub fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = alloc::vec![0_u8; record_length(body.len())];
    let written = write(kind, body, &mut bytes).unwrap_or(0);
    bytes.truncate(written);
    bytes
}

/// Cuts records into the broadcast packets of audio service `service`
/// (REQ-AUD-007), numbering them from `record_id`. Returns the packets and
/// the next record id.
#[must_use]
pub fn packets(service: u8, mut record_id: u8, records: &[Vec<u8>]) -> (Vec<Packet>, u8) {
    let mut out = Vec::new();
    for record in records {
        let count = record.len().div_ceil(FRAGMENT_DATA);
        for (index, chunk) in record.chunks(FRAGMENT_DATA).enumerate() {
            let last = index.saturating_add(1) == count;
            out.push(packet(service, record_id, u8::try_from(index).unwrap_or(u8::MAX), last, chunk));
        }
        record_id = record_id.wrapping_add(1);
    }
    (out, record_id)
}
