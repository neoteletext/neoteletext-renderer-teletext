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

    /// The sound is gone. A voice already playing it plays to its end; a
    /// live sound's voice plays what it holds, then ends.
    fn forget(&mut self, sound: SoundId);

    /// A live sound exists (REQ-AUD-014): its samples come without end, in
    /// order, through [`VoiceSink::stream_samples`]. The sink buffers them,
    /// starts a voice once enough are held, and follows the sender's clock.
    fn stream(&mut self, sound: SoundId, rate_hz: u32);

    /// The next samples of a live sound, in order.
    fn stream_samples(&mut self, sound: SoundId, pcm: &[i16]);

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

    /// A live IMA ADPCM sound's next block, in order, for a sink that
    /// decodes it itself. Nothing of it is held back: before a block given
    /// as silence, which comes through [`VoiceSink::stream_samples`] after
    /// a [`VoiceSink::concealed`] report, the sink fades out the last 5 ms
    /// it holds (REQ-AUD-013).
    fn ima_stream(&mut self, _sound: SoundId, _block: ImaBlock<'_>) {}
}
