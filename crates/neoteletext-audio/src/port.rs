//! The voice sink: what a port supplies to play sound (REQ-AUD-018). The
//! receiver tells it which sounds exist, gives it their samples as 16-bit
//! PCM, and starts, stops and ramps voices on numbered channels. The sink
//! mixes and plays; it decides nothing.

/// A sound as the sink knows it: its handle and generation. A newer
/// generation of a handle is a different sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub struct SoundId {
    /// The handle of the OBJECT record.
    pub handle: u8,
    /// Its generation.
    pub generation: u16,
}

/// A sound's shape: what the sink needs to hold its samples.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoundFormat {
    /// Samples a second: one of [`crate::SAMPLE_RATES`].
    pub rate_hz: u32,
    /// Samples in the whole sound.
    pub samples: u32,
}

/// Starting a sound on a channel (REQ-AUD-011, REQ-AUD-015).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Start {
    /// The sound.
    pub sound: SoundId,
    /// 0 to 100 per cent.
    pub gain: u8,
    /// The ramp in and, for the sound it replaces, out; at least
    /// [`crate::MIN_FADE_MS`].
    pub fade_ms: u16,
    /// Repeat until stopped.
    pub looped: bool,
    /// While this sound plays, every other channel plays at this per cent of
    /// its gain: one of [`crate::DUCK_PERCENT`].
    pub duck_percent: u8,
    /// Wait for the channel's current sound to end instead of replacing it.
    /// A channel holds one such waiting sound; a newer one replaces it.
    pub after_current: bool,
}

/// One IMA ADPCM block given undecoded to a sink that decodes IMA ADPCM
/// itself, such as tellyboy's driver (REQ-AUD-018, DEC-2449). Its header
/// has been checked; its codes are as received.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImaBlock<'a> {
    /// The block: the WAVE IMA ADPCM header, then two codes a byte.
    pub bytes: &'a [u8],
    /// The samples of it the sound keeps: the final block's last code may
    /// be padding.
    pub samples: u32,
    /// Fade the first 5 ms in: the block before it has not arrived
    /// (REQ-AUD-013).
    pub fade_in: bool,
    /// Fade the last 5 ms out: the block after it has not arrived.
    pub fade_out: bool,
}

/// Starting a local sound on a channel (page-rules.md, RULE-60): what the
/// sink needs to make the family's instrument in its sound output stage.
/// The sound's banks came just before it, through
/// [`VoiceSink::local_bank`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocalStart<'a> {
    /// The sound.
    pub sound: SoundId,
    /// The family: the code of its codec, 13 to 18
    /// ([`crate::Codec::code`]).
    pub family: u8,
    /// The variant of the family the sound plays
    /// ([`crate::record::TAG_VARIANT`]): 0, or 1 for synth60 and synth52,
    /// 2 for synth51.
    pub variant: u8,
    /// The family's option byte: synth8's key mode
    /// ([`crate::record::SYNTH8_KEYS_WHOLE`],
    /// [`crate::record::SYNTH8_KEYS_DUAL`] or
    /// [`crate::record::SYNTH8_KEYS_SPLIT`]); 0 for every other family.
    pub option: u8,
    /// synth8's split point, a MIDI key ([`crate::record::TAG_SYNTH8_KEYS`]'
    /// second byte); 0 for every other family.
    pub split: u8,
    /// 0 to 100 per cent.
    pub gain: u8,
    /// The ramp in and, for the sound it replaces, out; at least
    /// [`crate::MIN_FADE_MS`].
    pub fade_ms: u16,
    /// While this sound plays, every other channel plays at this per cent
    /// of its gain: one of [`crate::DUCK_PERCENT`].
    pub duck_percent: u8,
    /// The setup block: an event list (MIDI-23) with every event at tick 0,
    /// at most [`crate::MOST_EVENT_BLOCK_BYTES`] bytes, which the sink
    /// applies in order to the new instrument before any other message.
    pub setup: &'a [u8],
}

/// What a port supplies to play sound.
pub trait VoiceSink {
    /// How many channels the sink plays, at most [`crate::CHANNELS`]. The
    /// receiver plays the lowest-numbered channels and ignores the rest
    /// (REQ-AUD-016).
    fn channels(&self) -> u8 {
        crate::CHANNELS
    }

    /// A sound exists: the sink makes room for its samples, silent until
    /// they arrive.
    fn define(&mut self, sound: SoundId, format: SoundFormat);

    /// Samples of a sound, starting at sample `first`.
    fn samples(&mut self, sound: SoundId, first: u32, pcm: &[i16]);

    /// The sound is gone. A voice already playing it plays to its end; every
    /// stream of it, on any audio channel, ends as [`VoiceSink::end_stream`]
    /// ends one.
    fn forget(&mut self, sound: SoundId);

    /// A stream begins on audio channel `channel` (REQ-AUD-014; INT-33,
    /// INT-37): samples of `sound` that come in order through
    /// [`VoiceSink::stream_samples`], with no length known ahead, until
    /// [`VoiceSink::end_stream`] or [`VoiceSink::forget`]. A stream is named
    /// by its audio channel and its sound together, so one sound may stream
    /// on two audio channels at once, each its own playing; a stream begun
    /// under a name the sink still holds replaces the one before it. A live
    /// sound streams on each audio channel that plays it; each playing of a
    /// synthesizer's sound, stored or live, is a stream of its own, rendered
    /// as it plays. The sink buffers a stream's samples, at most two seconds
    /// of them ([`crate::STREAM_HELD_MS`]), starts its voice once enough
    /// are held, and follows the sender's clock.
    fn stream(&mut self, channel: u8, sound: SoundId, rate_hz: u32);

    /// The next samples of the stream of `sound` on audio channel `channel`,
    /// in order.
    fn stream_samples(&mut self, channel: u8, sound: SoundId, pcm: &[i16]);

    /// The stream of `sound` on audio channel `channel` ends (INT-36): no
    /// more samples come. A voice playing it plays what it holds, then ends,
    /// and a sound waiting after it on the channel starts.
    fn end_stream(&mut self, channel: u8, sound: SoundId);

    /// Starts a sound on a channel, ramping out what the channel played.
    fn start(&mut self, channel: u8, start: Start);

    /// Ramps the channel to silence and stops it, dropping any sound waiting
    /// to follow.
    fn stop(&mut self, channel: u8, fade_ms: u16);

    /// Ramps the channel to a new gain.
    fn gain(&mut self, channel: u8, gain: u8, fade_ms: u16);

    /// The `concealed` report (REQ-AUD-013): block `block` of a sound, of
    /// `samples` samples, was given as silence, or concealed by its decoder,
    /// because it had not arrived when it was due. It is for diagnostics; a
    /// sink plays nothing differently for it, and may ignore it.
    fn concealed(&mut self, _sound: SoundId, _block: u16, _samples: u32) {}

    /// Where the voice on `channel` has got to in the stored sound it plays:
    /// the sample of that sound it plays next, as near as the sink knows to
    /// what is being heard (`design/audio.md` section 10.1; tellyboy's
    /// `tb_audio_voice_position`; DEC-2470). `None` when the sink does not
    /// know, the voice plays a live sound or nothing, or the sound has not
    /// started; the provided answer is `None`.
    ///
    /// When it answers, the receiver judges a stored sound's blocks late by
    /// it rather than by its own clock, so a sink's output latency, or a port
    /// that ticks late, does not report a block concealed that arrives
    /// before the voice reaches it (REQ-AUD-013); and an Opus sound's
    /// samples decoded after a gap the voice has already entered are faded
    /// in from where the voice is.
    fn position(&self, _channel: u8) -> Option<u32> {
        None
    }

    /// Whether the sink decodes IMA ADPCM itself (REQ-AUD-018, DEC-2449).
    /// When it does, an IMA ADPCM sound's blocks come to
    /// [`VoiceSink::ima_samples`] and [`VoiceSink::ima_stream`] undecoded,
    /// and never as samples. The provided answer is no.
    fn decodes_ima(&self) -> bool {
        false
    }

    /// A stored IMA ADPCM sound's block, starting at sample `first`, for a
    /// sink that decodes it itself, fading either edge as the block says.
    fn ima_samples(&mut self, _sound: SoundId, _first: u32, _block: ImaBlock<'_>) {}

    /// A live IMA ADPCM sound's next block, in order, for the stream of it
    /// on audio channel `channel` (INT-37), for a sink that decodes it
    /// itself. Nothing of it is held back: before a block given as silence,
    /// which comes through [`VoiceSink::stream_samples`] after a
    /// [`VoiceSink::concealed`] report, the sink fades out the last 5 ms it
    /// holds (REQ-AUD-013).
    fn ima_stream(&mut self, _channel: u8, _sound: SoundId, _block: ImaBlock<'_>) {}

    /// Whether the sink plays local sounds (page-rules.md, RULE-60): it has
    /// a sound output stage that holds a family's instrument and renders it
    /// as it mixes. Where it does not, the receiver ignores a local sound's
    /// OBJECT, as it ignores a codec it does not have. The provided answer
    /// is no.
    fn plays_local(&self) -> bool {
        false
    }

    /// One bank of the local sound about to start on audio channel
    /// `channel` (RULE-60): bank index `index`, 0 to
    /// [`crate::MOST_BANKS`] − 1, and its whole image, at most
    /// [`crate::MOST_BANK_BYTES`] bytes, which the sink copies. The banks
    /// of a start come in index order, each before
    /// [`VoiceSink::local`], which starts the instrument with the banks
    /// given for that channel and sound since the last start.
    fn local_bank(&mut self, _channel: u8, _sound: SoundId, _index: u8, _image: &[u8]) {}

    /// Starts a local sound on audio channel `channel`, ramping out what
    /// the channel played (RULE-60): the sink makes the family's
    /// instrument, starts it with the banks given, applies the setup, and
    /// from then on gives it the MIDI messages the page's rules send to the
    /// channel. No stream begins and no samples come from the receiver.
    /// [`VoiceSink::stop`] and [`VoiceSink::gain`] act on the channel as on
    /// any sound; a stop releases every note and ends the instrument, and
    /// the receiver stops the channel when the sound's page is no longer
    /// open.
    fn local(&mut self, _channel: u8, _start: LocalStart<'_>) {}
}
