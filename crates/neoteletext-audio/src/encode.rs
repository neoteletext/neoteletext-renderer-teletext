//! Writing sounds, for publishing tools (REQ-AUD-020): IMA ADPCM encoding,
//! records, and records cut into broadcast packets. Opus is not encoded
//! here: a publisher's encoder writes the packets, and they are carried as
//! they are (decision E.124). This module allocates.

use alloc::vec::Vec;

use neoteletext_core::packet::Packet;

use crate::idl::{FRAGMENT_DATA, packet};
use crate::ima::{State, encode as encode_code};
use crate::record::{DATA_FIXED, TAG_LABEL, TYPE_CONTROL, TYPE_DATA, TYPE_OBJECT, record_length, write};
use crate::{Codec, MAX_BODY, SERVICE_SCOPE, rate_code};

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
    /// The encoded blocks, back to back.
    pub data: Vec<u8>,
    /// A label, at most [`crate::record::MAX_LABEL`] bytes.
    pub label: Option<Vec<u8>>,
    /// A SID sound's chip and clock byte ([`crate::record::TAG_SID_CHIP`]).
    pub chip: Option<u8>,
    /// The handle of a DX7 sound's bank ([`crate::record::TAG_DX7_BANK`]).
    pub bank: Option<u8>,
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
            Codec::Opus | Codec::Sid | Codec::Dx7 | Codec::Dx7Bank => return Err(EncodeError::Encoder),
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
            bank: None,
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
            bank: None,
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
            bank: None,
        })
    }

    /// A stored sound of an instrument codec from its blocks, `block_bytes`
    /// each, back to back in `blocks` (REQ-6411): a SID sound's register
    /// images, a DX7 sound's slots, or a bank's patches. A SID sound's chip
    /// and a DX7 sound's bank are set afterwards, in the fields `chip` and
    /// `bank`.
    ///
    /// # Errors
    /// A codec that is not an instrument's, a block size it does not allow,
    /// blocks that are not all that length, none, or more than a sound (or
    /// a bank) may have.
    pub fn instrument(
        handle: u8,
        generation: u16,
        codec: Codec,
        block_bytes: u16,
        blocks: &[u8],
    ) -> Result<Self, EncodeError> {
        if !matches!(codec, Codec::Sid | Codec::Dx7 | Codec::Dx7Bank) {
            return Err(EncodeError::Encoder);
        }
        if !codec.allows_block(block_bytes) {
            return Err(EncodeError::BlockSize(block_bytes));
        }
        if !blocks.len().is_multiple_of(usize::from(block_bytes)) {
            return Err(EncodeError::PacketLength);
        }
        let count = blocks.len().checked_div(usize::from(block_bytes)).unwrap_or(0);
        let count = u16::try_from(count).map_err(|_| EncodeError::Length)?;
        let samples = u32::from(count).saturating_mul(codec.samples_per_block(block_bytes));
        if count == 0 || (codec == Codec::Dx7Bank && samples > crate::DX7_BANK_PATCHES) {
            return Err(EncodeError::Length);
        }
        Ok(Self {
            handle,
            generation,
            id: u32::from(handle),
            codec,
            rate_hz: crate::INSTRUMENT_RATE,
            block_bytes,
            samples,
            data: blocks.to_vec(),
            label: None,
            chip: None,
            bank: None,
        })
    }

    /// The DATA records of a live Opus sound's next packets, `block_bytes`
    /// each, back to back in `packets`, numbered from block `first` (counting
    /// modulo 2¹⁶): as many to a record as fit, since a packet is small.
    ///
    /// # Errors
    /// Not an Opus sound, or packets that are not all its length.
    pub fn packet_records(&self, first: u16, packets: &[u8]) -> Result<Vec<Vec<u8>>, EncodeError> {
        // An instrument's blocks are carried as Opus packets are: whole,
        // of one length, as they come (REQ-6411).
        if !matches!(self.codec, Codec::Opus | Codec::Sid | Codec::Dx7) {
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
        for (tag, value) in [(crate::record::TAG_SID_CHIP, self.chip), (crate::record::TAG_DX7_BANK, self.bank)] {
            if let Some(value) = value {
                body.extend_from_slice(&[tag, 1, value]);
            }
        }
        Ok(frame(TYPE_OBJECT, &body))
    }

    /// The DATA records that carry the sound: as many whole blocks in each
    /// as fit a record.
    #[must_use]
    pub fn data_records(&self) -> Vec<Vec<u8>> {
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
            Codec::Opus | Codec::Sid | Codec::Dx7 | Codec::Dx7Bank => {}
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
