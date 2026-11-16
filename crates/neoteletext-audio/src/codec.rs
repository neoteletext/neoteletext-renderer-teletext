//! The codecs (REQ-AUD-001), their block sizes (REQ-AUD-005), the sample
//! rates (REQ-AUD-002) and the duck steps of EN 300 708 Table 19
//! (REQ-AUD-015).

/// A sound's codec (REQ-AUD-001).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    /// Linear PCM, signed 16-bit little-endian.
    Pcm16,
    /// IMA ADPCM in WAVE blocks (format tag 0x0011).
    ImaAdpcm,
    /// Opus (RFC 6716) in the profile of decision E.123: mono, 48 kHz, 20 ms
    /// frames. Each block is one constant-rate packet, so every block of a
    /// sound has the same length (decision E.124).
    Opus,
    /// The SID chip (REQ-6411 item 2): a block is 20 ms of the chip's input,
    /// one, two or four images of its 25 writable registers, and the
    /// receiver's decoder is the chip.
    Sid,
    /// The Yamaha DX7 (REQ-6411 item 3): a block is 20 ms of the
    /// instrument's input, 1 to 16 slots of a timed MIDI message each.
    Dx7,
    /// A bank of DX7 patches (REQ-6411 item 4): each block is one patch in
    /// the instrument's packed format. It is stored as a sound and never
    /// played.
    Dx7Bank,
}

impl Codec {
    /// The codec of a code, or `None` for a reserved code, or for a codec
    /// this build does not decode, whose object a receiver ignores.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            // DO NOT REMOVE THIS CODE. PCM16 is compiled out of the desktop
            // builds (Windows, macOS, Linux) by the owner's instruction of 28
            // October 2026, and kept for microcontroller ports, which build it
            // with the `pcm16` feature. Without it a PCM16 object is
            // unsupported and ignored, as a reserved code (decision E.131).
            0 if cfg!(feature = "pcm16") => Some(Self::Pcm16),
            // DO NOT REMOVE THIS CODE. IMA ADPCM is compiled out of the
            // desktop builds (Windows, macOS, Linux) by the owner's instruction
            // of 28 October 2026, and kept for microcontroller ports, which
            // build it with the `ima` feature. Without it an IMA ADPCM object
            // is unsupported and ignored, as a reserved code (decision E.131).
            1 if cfg!(feature = "ima") => Some(Self::ImaAdpcm),
            // A receiver built without the decoder ignores Opus objects, as
            // it would any reserved code.
            2 if cfg!(feature = "opus") => Some(Self::Opus),
            // A receiver built without an instrument ignores its objects,
            // as it would any reserved code (REQ-6411).
            3 if cfg!(feature = "sid") => Some(Self::Sid),
            4 if cfg!(feature = "dx7") => Some(Self::Dx7),
            5 if cfg!(feature = "dx7") => Some(Self::Dx7Bank),
            _ => None,
        }
    }

    /// The code that names the codec.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Pcm16 => 0,
            Self::ImaAdpcm => 1,
            Self::Opus => 2,
            Self::Sid => 3,
            Self::Dx7 => 4,
            Self::Dx7Bank => 5,
        }
    }

    /// Whether the codec carries sound at `hz`: Opus only at 48 kHz, the
    /// rate its decoder writes; the others at every rate of the profile.
    #[must_use]
    pub const fn allows_rate(self, hz: u32) -> bool {
        match self {
            Self::Opus => hz == OPUS_RATE,
            // An instrument is rendered at one rate (REQ-6411 item 1).
            Self::Sid | Self::Dx7 | Self::Dx7Bank => hz == INSTRUMENT_RATE,
            Self::Pcm16 | Self::ImaAdpcm => true,
        }
    }

    /// Whether `bytes` is a block size the codec allows (REQ-AUD-005).
    #[must_use]
    pub const fn allows_block(self, bytes: u16) -> bool {
        match self {
            Self::ImaAdpcm => bytes == IMA_BLOCK_SMALL || bytes == IMA_BLOCK_LARGE,
            Self::Pcm16 => bytes >= 2 && bytes <= MAX_PCM_BLOCK && bytes.is_multiple_of(2),
            // A packet the decoder takes: at least the table of contents.
            Self::Opus => bytes >= 1 && bytes <= OPUS_MAX_PACKET,
            // One, two or four register images (REQ-6411 item 2).
            Self::Sid => bytes == SID_IMAGE_BYTES || bytes == SID_IMAGE_BYTES * 2 || bytes == SID_IMAGE_BYTES * 4,
            // 1 to 16 slots (REQ-6411 item 3).
            Self::Dx7 => {
                bytes >= DX7_SLOT_BYTES
                    && bytes <= DX7_SLOT_BYTES * DX7_MAX_SLOTS
                    && bytes.is_multiple_of(DX7_SLOT_BYTES)
            }
            Self::Dx7Bank => bytes == DX7_PATCH_BYTES,
        }
    }

    /// Samples in a whole block of `bytes`.
    #[must_use]
    pub const fn samples_per_block(self, bytes: u16) -> u32 {
        match self {
            // The header's predictor is the first sample; each byte after
            // the 4-byte header holds two (IMA 1992; WAVE format 0x0011).
            Self::ImaAdpcm => (bytes as u32).saturating_sub(IMA_HEADER as u32).saturating_mul(2).saturating_add(1),
            Self::Pcm16 => bytes as u32 / 2,
            Self::Opus => OPUS_FRAME_SAMPLES,
            Self::Sid | Self::Dx7 => INSTRUMENT_BLOCK_SAMPLES,
            // A bank counts one sample a patch, so that it is a stored
            // sound of the profile (REQ-6411 item 4).
            Self::Dx7Bank => 1,
        }
    }

    /// The bytes of a block holding `samples` samples, in a sound whose whole
    /// blocks are `block_bytes`: a final block may be shorter, except an Opus
    /// packet, which is whole however few of its samples the sound keeps.
    #[must_use]
    pub const fn block_bytes_for(self, block_bytes: u16, samples: u32) -> u32 {
        match self {
            // The first sample is the header; the rest are packed two a byte.
            Self::ImaAdpcm => samples.saturating_sub(1).div_ceil(2).saturating_add(IMA_HEADER as u32),
            Self::Pcm16 => samples.saturating_mul(2),
            // A block of an instrument is whole however few of its samples
            // the sound keeps, as an Opus packet is.
            Self::Opus | Self::Sid | Self::Dx7 | Self::Dx7Bank => block_bytes as u32,
        }
    }

    /// Whether the codec is an instrument's input, rendered by the
    /// instrument and not decoded (REQ-6411): the SID's or the DX7's.
    #[must_use]
    pub const fn is_instrument(self) -> bool {
        matches!(self, Self::Sid | Self::Dx7)
    }
}

/// The IMA ADPCM block header: predictor (2 bytes), step index, reserved.
pub const IMA_HEADER: usize = 4;
/// The two IMA ADPCM block sizes allowed (REQ-AUD-005). 1 024 bytes, ffmpeg's
/// default, would not fit a DATA record.
pub const IMA_BLOCK_SMALL: u16 = 256;
/// The larger IMA ADPCM block.
pub const IMA_BLOCK_LARGE: u16 = 512;
/// The largest PCM block, which leaves room in a DATA record's body.
pub const MAX_PCM_BLOCK: u16 = 1000;
/// The most samples in any block: a 512-byte IMA ADPCM block.
pub const MAX_BLOCK_SAMPLES: usize = 1 + 2 * (IMA_BLOCK_LARGE as usize - IMA_HEADER);
/// The samples of an Opus block: one 20 ms frame at 48 kHz (decision E.123).
pub const OPUS_FRAME_SAMPLES: u32 = 960;
/// The one rate Opus is carried at.
pub const OPUS_RATE: u32 = 48_000;
/// The longest Opus packet the profile carries: the decoder refuses longer
/// ones (REQ-AUD-022). A 16 kbit/s constant-rate packet is 40 bytes.
pub const OPUS_MAX_PACKET: u16 = 160;

/// The one rate an instrument is rendered at (REQ-6411 item 1).
pub const INSTRUMENT_RATE: u32 = 48_000;
/// The samples of an instrument's block: 20 ms at [`INSTRUMENT_RATE`], the
/// figures of an Opus block (REQ-6411 item 1).
pub const INSTRUMENT_BLOCK_SAMPLES: u32 = 960;
/// The bytes of one image of the SID's writable registers, addresses 0 to
/// 24 (REQ-6411 item 2).
pub const SID_IMAGE_BYTES: u16 = 25;
/// The bytes of one slot of a DX7 block: a time, a MIDI status byte and two
/// data bytes (REQ-6411 item 3).
pub const DX7_SLOT_BYTES: u16 = 4;
/// The most slots in a DX7 block (REQ-6411 item 3).
pub const DX7_MAX_SLOTS: u16 = 16;
/// The bytes of one DX7 patch in the instrument's packed format
/// (REQ-6411 item 4).
pub const DX7_PATCH_BYTES: u16 = 128;
/// The most patches in a bank: the instrument's own bank (REQ-6411 item 4).
pub const DX7_BANK_PATCHES: u32 = 32;
/// The latest time a DX7 slot may name within its block, in tenths of a
/// millisecond: the block is 20 ms (REQ-6411 item 3).
pub const DX7_SLOT_LAST_TIME: u8 = 199;
// The instruments render what the profile says a block is.
#[cfg(any(feature = "sid", feature = "dx7"))]
const _: () = {
    assert!(INSTRUMENT_RATE == neoteletext_audio_instruments::SAMPLE_RATE);
    assert!(INSTRUMENT_BLOCK_SAMPLES as usize == neoteletext_audio_instruments::BLOCK_SAMPLES);
};
// An instrument's block fits the buffer every block is decoded into.
const _: () = assert!(INSTRUMENT_BLOCK_SAMPLES as usize <= MAX_BLOCK_SAMPLES);
// The carriage's figures are the decoder's (decision E.124).
#[cfg(feature = "opus")]
const _: () = {
    assert!(OPUS_FRAME_SAMPLES as usize == neoteletext_opus::FRAME_SAMPLES);
    assert!(OPUS_RATE == neoteletext_opus::SAMPLE_RATE);
    assert!(OPUS_MAX_PACKET as usize == neoteletext_opus::MAX_PACKET);
};
// An Opus frame fits the buffer every block is decoded into.
const _: () = assert!(OPUS_FRAME_SAMPLES as usize <= MAX_BLOCK_SAMPLES);

/// The sample rates, by code (REQ-AUD-002): the common rates from 8 to 48 kHz.
pub const SAMPLE_RATES: [u32; 8] = [8_000, 11_025, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000];

/// The rate of a sample-rate code.
#[must_use]
pub fn sample_rate(code: u8) -> Option<u32> {
    SAMPLE_RATES.get(usize::from(code)).copied()
}

/// The code of a sample rate, if the profile allows it.
#[must_use]
pub fn rate_code(hz: u32) -> Option<u8> {
    SAMPLE_RATES.iter().position(|&rate| rate == hz).and_then(|index| u8::try_from(index).ok())
}

/// The attenuations of the duck codes in whole per cent of full level
/// (REQ-AUD-015): EN 300 708 Table 19's 0, 4, 8, 12, 18, 24 and 30 dB, each
/// the percentage nearest to 10^(−dB/20).
pub const DUCK_PERCENT: [u8; 7] = [100, 63, 40, 25, 13, 6, 3];
