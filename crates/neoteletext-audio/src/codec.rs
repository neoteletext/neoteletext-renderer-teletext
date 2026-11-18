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
    /// The synth6581 family, synth6581 and synth8580, which
    /// [`crate::record::TAG_SYNTH6581_CHIP`] chooses (code 12, MIDI-107): a
    /// block is an event list (MIDI-23).
    Synth6581,
    /// synth7 (code 13): a block is an event list.
    Synth7,
    /// The synth106 family, synth106 and synth60, which
    /// [`crate::record::TAG_VARIANT`] chooses (code 14): a block is an event
    /// list.
    Synth106,
    /// synthD (code 15): a block is an event list.
    SynthD,
    /// The synth53 family, synth53, synth52 and synth51, which
    /// [`crate::record::TAG_VARIANT`] chooses (code 16): a block is an event
    /// list.
    Synth53,
    /// synth8 (code 17): a block is an event list.
    Synth8,
    /// synth80 (code 18): a block is an event list.
    Synth80,
    /// A bank of any family with banks, which its OBJECT names
    /// ([`crate::record::TAG_BANK_FAMILY`]) (code 19, MIDI-55): its blocks
    /// are the pieces of one bank image, stored as a sound and never played.
    Bank,
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
            // A receiver built without a family ignores its objects, as it
            // would any reserved code (MIDI-107). Codes 3 to 11 are retired
            // and reserved for ever (MIDI-108).
            12 if cfg!(feature = "synth6581") => Some(Self::Synth6581),
            13 if cfg!(feature = "synth7") => Some(Self::Synth7),
            14 if cfg!(feature = "synth106") => Some(Self::Synth106),
            15 if cfg!(feature = "synthd") => Some(Self::SynthD),
            16 if cfg!(feature = "synth53") => Some(Self::Synth53),
            17 if cfg!(feature = "synth8") => Some(Self::Synth8),
            18 if cfg!(feature = "synth80") => Some(Self::Synth80),
            // A bank is read by a build with any family that has banks;
            // one for a family the build leaves out is ignored when its
            // OBJECT is read (`record::parse`).
            19 if cfg!(any(
                feature = "synth7",
                feature = "synth106",
                feature = "synthd",
                feature = "synth53",
                feature = "synth8",
                feature = "synth80"
            )) =>
            {
                Some(Self::Bank)
            }
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
            Self::Synth6581 => 12,
            Self::Synth7 => 13,
            Self::Synth106 => 14,
            Self::SynthD => 15,
            Self::Synth53 => 16,
            Self::Synth8 => 17,
            Self::Synth80 => 18,
            Self::Bank => 19,
        }
    }

    /// Whether `code` names a family that has banks, built into this
    /// receiver or not: the codes a bank may be for (MIDI-48, tag 6).
    #[must_use]
    pub const fn is_family_code(code: u8) -> bool {
        code >= Self::Synth7.code() && code <= Self::Synth80.code()
    }

    /// How many variants of its family a codec plays, which
    /// [`crate::record::TAG_VARIANT`] chooses among, counted from 0
    /// (MIDI-48): synth106 and synth60; synth53, synth52 and synth51; one
    /// for every other.
    #[must_use]
    pub const fn variants(self) -> u8 {
        match self {
            Self::Synth106 => 2,
            Self::Synth53 => 3,
            _ => 1,
        }
    }

    /// Whether a sound of the codec names 1 to [`MOST_BANKS`] banks
    /// (MIDI-57): every family but the synth6581 family's.
    #[must_use]
    pub const fn has_banks(self) -> bool {
        matches!(self, Self::Synth7 | Self::Synth106 | Self::SynthD | Self::Synth53 | Self::Synth8 | Self::Synth80)
    }

    /// Whether the codec is a bank: a bank image, stored as a sound and
    /// never played.
    #[must_use]
    pub const fn is_bank(self) -> bool {
        matches!(self, Self::Bank)
    }

    /// Whether a block of the codec is an event list (MIDI-23): the seven
    /// synthesizer codecs, 12 to 18.
    #[must_use]
    pub const fn is_event(self) -> bool {
        self.has_banks() || matches!(self, Self::Synth6581)
    }

    /// Whether the codec carries sound at `hz`: Opus only at 48 kHz, the
    /// rate its decoder writes; the others at every rate of the profile.
    #[must_use]
    pub const fn allows_rate(self, hz: u32) -> bool {
        match self {
            Self::Opus => hz == OPUS_RATE,
            Self::Pcm16 | Self::ImaAdpcm => true,
            // An instrument is rendered at one rate (REQ-6411 item 1), and
            // a bank is carried as its instrument's sounds are.
            _ => hz == INSTRUMENT_RATE,
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
            // The longest block of the sound (MIDI-40).
            Self::Synth6581
            | Self::Synth7
            | Self::Synth106
            | Self::SynthD
            | Self::Synth53
            | Self::Synth8
            | Self::Synth80 => bytes as usize <= MOST_EVENT_BLOCK_BYTES,
            // A piece of a bank image: at most what a DATA record carries
            // (MIDI-55).
            Self::Bank => bytes >= 1 && bytes as usize <= BANK_PIECE_BYTES,
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
            // A bank counts one sample a byte of its image, so that it is
            // a stored sound of the profile (MIDI-55).
            Self::Bank => bytes as u32,
            // An instrument's block is 20 ms (REQ-6411 item 1).
            _ => INSTRUMENT_BLOCK_SAMPLES,
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
            // A bank's last piece is what is left of its image (MIDI-55).
            Self::Bank => samples,
            // A block of an instrument is whole however few of its samples
            // the sound keeps, as an Opus packet is.
            _ => block_bytes as u32,
        }
    }

    /// Whether the codec is an instrument's input, rendered by the
    /// instrument and not decoded: an event codec (MIDI-23).
    #[must_use]
    pub const fn is_instrument(self) -> bool {
        self.is_event()
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
/// The most bytes of an event codec's block (MIDI-30): a record's body,
/// less the DATA fields of an event codec and the block's length, so that a
/// block always fits one DATA record.
pub const MOST_EVENT_BLOCK_BYTES: usize =
    crate::MAX_BODY - crate::record::EVENT_DATA_FIXED - crate::record::EVENT_BLOCK_LENGTH_BYTES;
// MIDI-30: 1,024 − 4 − 4 − 9 − 2 = 1,005, the figure the instruments' event
// lists are bounded by.
const _: () = assert!(MOST_EVENT_BLOCK_BYTES == 1_005);
#[cfg(feature = "midi")]
const _: () = assert!(MOST_EVENT_BLOCK_BYTES == neoteletext_audio_instruments::midi::MOST_BLOCK_BYTES);
/// The most blocks of a stored sound of an event codec: two minutes
/// (MIDI-40, MIDI-104).
pub const MOST_EVENT_SOUND_BLOCKS: u16 = 6_000;
/// The most bytes of all the blocks of a stored sound of an event codec
/// (MIDI-41): 6,000 Opus blocks of the longest packet, the most store any
/// stored sound took before the event codecs.
pub const MOST_EVENT_SOUND_BYTES: u32 = MOST_EVENT_SOUND_BLOCKS as u32 * OPUS_MAX_PACKET as u32;
const _: () = assert!(MOST_EVENT_SOUND_BYTES == 960_000);
/// The bytes of a piece of a bank image: the most a DATA record of the
/// fixed layout carries (MIDI-55).
pub const BANK_PIECE_BYTES: usize = crate::MAX_BODY - crate::record::DATA_FIXED;
const _: () = assert!(BANK_PIECE_BYTES == 1_011);
/// The most bytes of a bank image (MIDI-55): 16 KiB. Named here rather than
/// taken from the instruments crate, so that a build without that crate
/// (DEC-2355) reads banks to the same bound; the assertion below holds the
/// two equal.
pub const MOST_BANK_BYTES: u32 = 16_384;
/// The most banks a sound names (MIDI-57).
pub const MOST_BANKS: usize = 10;
// The instruments render what the profile says a block is, and bound banks
// as the profile does.
#[cfg(feature = "midi")]
const _: () = {
    assert!(MOST_BANK_BYTES as usize == neoteletext_audio_instruments::midi::MOST_BANK_BYTES);
    assert!(MOST_BANKS == neoteletext_audio_instruments::midi::MOST_BANKS);
    assert!(INSTRUMENT_RATE == neoteletext_audio_instruments::SAMPLE_RATE);
    assert!(INSTRUMENT_BLOCK_SAMPLES as usize == neoteletext_audio_instruments::BLOCK_SAMPLES);
};
// A page's rules name a family by its codec's code (page-rules.md, RULE-28),
// which the instruments crate's tables and local sounds are found by.
#[cfg(feature = "midi")]
const _: () = {
    use neoteletext_audio_instruments::midi::family;
    assert!(Codec::Synth6581.code() == family::SYNTH6581);
    assert!(Codec::Synth7.code() == family::SYNTH7);
    assert!(Codec::Synth106.code() == family::SYNTH106);
    assert!(Codec::SynthD.code() == family::SYNTHD);
    assert!(Codec::Synth53.code() == family::SYNTH53);
    assert!(Codec::Synth8.code() == family::SYNTH8);
    assert!(Codec::Synth80.code() == family::SYNTH80);
};
// A local sound's option byte is synth8's key mode as tag 7 carries it
// (RULE-60), which the instruments crate reads.
#[cfg(feature = "synth8")]
const _: () = {
    use neoteletext_audio_instruments::synth8::interface::{KEY_MODE_DUAL, KEY_MODE_SPLIT, KEY_MODE_WHOLE};
    assert!(crate::record::SYNTH8_KEYS_WHOLE == KEY_MODE_WHOLE);
    assert!(crate::record::SYNTH8_KEYS_DUAL == KEY_MODE_DUAL);
    assert!(crate::record::SYNTH8_KEYS_SPLIT == KEY_MODE_SPLIT);
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
