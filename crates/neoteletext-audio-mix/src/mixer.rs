//! The mixer: four voices mixed to mono at one output rate, with ducking
//! and a clamp after the sum (REQ-AUD-015, REQ-AUD-019). A voice and the
//! sound on it: the source position in 32.32 fixed point, its step, and the
//! windowed-sinc interpolation between samples ([`sinc`](crate::sinc)).

use crate::ramp::{MIN_RAMP_MS, Ramp, UNITY, level, scale};
use crate::sinc::{FILTER, ONE, PHASES, TAPS_BEFORE};
use crate::sound::{STREAM_TARGET_MS, Sound, SoundKey, Source};
use alloc::vec::Vec;

mod cues;
mod meter;
mod pulled;
mod stream;

pub(crate) use cues::Marks;
pub use cues::{HeardCue, MAX_PENDING_CUE_MARKS};

pub use meter::{PEAK_FULL_SCALE, PEAK_PER_LEVEL, meter_level};
pub use pulled::{LOCAL_BLOCK_SAMPLES, PULLED_LOOKAHEAD_SAMPLES, PULLED_RATE_HZ, Pull};

/// Voices, one for each channel of the audio profile.
pub const VOICES: usize = 4;

/// Fraction bits of a source position.
const FRACTION_BITS: u32 = 32;
/// Shift from a position's fraction to a filter phase.
const PHASE_SHIFT: u32 = FRACTION_BITS - PHASES.trailing_zeros();

/// How a voice should start a sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Start {
    /// The sound.
    pub key: SoundKey,
    /// 0 to 100 per cent.
    pub gain: u8,
    /// The ramp in, and out for what it replaces.
    pub fade_ms: u16,
    /// Repeat until stopped.
    pub looped: bool,
    /// The other voices' level while this plays, in per cent.
    pub duck_percent: u8,
    /// Wait for the voice's current sound to end.
    pub after_current: bool,
}

/// Four voices mixed to mono at one output rate.
pub struct Mixer {
    rate_hz: u32,
    voices: [Voice; VOICES],
    sounds: Vec<Sound>,
    master: Ramp,
    master_percent: u8,
    muted: bool,
    /// The voices whose peak is kept, a bit each (RULE-32).
    metered: u8,
    /// Each metered voice's peak since it was last asked for.
    peaks: [u16; VOICES],
    /// Cue marks dropped (RULE-109).
    cue_marks_dropped: u32,
}

/// A sound on a voice.
#[derive(Clone, Copy, Debug)]
struct Playing {
    key: SoundKey,
    /// The source position: whole samples and a 32-bit fraction.
    whole: i64,
    fraction: u32,
    /// The source step per output sample, in the same form.
    step_whole: i64,
    step_fraction: u32,
    /// The nominal step, whole and fraction in one: a live sound's step is
    /// trimmed from it to follow the sender's clock.
    nominal: u64,
    /// A live sound's voice waits, silent, until the target is buffered.
    primed: bool,
    looped: bool,
    gain: Ramp,
    /// Other voices' level while this plays, in per cent.
    duck_percent: u8,
    fade_ms: u16,
    /// Stopping: it ends when its gain reaches zero.
    stopping: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    playing: Option<Playing>,
    /// The sound it replaced, fading out.
    outgoing: Option<Playing>,
    /// One sound waiting for the current one to end.
    pending: Option<Start>,
    duck: Option<Ramp>,
}

impl Mixer {
    /// A silent mixer rendering at `rate_hz`, at full master volume.
    #[must_use]
    pub fn new(rate_hz: u32) -> Self {
        Self {
            rate_hz: rate_hz.max(1),
            voices: [Voice::default(); VOICES],
            sounds: Vec::new(),
            master: Ramp::at(UNITY),
            master_percent: 100,
            muted: false,
            metered: 0,
            peaks: [0; VOICES],
            cue_marks_dropped: 0,
        }
    }

    /// The output rate.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate_hz
    }

    /// Makes room for a sound's samples, silent until they arrive. A sound
    /// of the same key is replaced. It allocates the samples' buffer; a
    /// caller that holds a lock the audio callback takes allocates it
    /// beforehand and calls [`Mixer::define_with`].
    pub fn define(&mut self, key: SoundKey, rate_hz: u32, samples: usize) {
        self.define_with(key, rate_hz, alloc::vec![0; samples]);
    }

    /// Defines a sound holding `samples`, the buffer as given and never
    /// copied: its length is the sound's, and what it holds plays until
    /// [`Mixer::samples`] overwrites it (zeros for the silence
    /// [`Mixer::define`] makes). A sound of the same key is replaced.
    ///
    /// It exists so that a port whose audio callback takes a lock around the
    /// mixer can allocate a sound, up to minutes of samples, before taking
    /// that lock: allocating under it would hold the callback up for as long
    /// as the allocator takes. What it may still allocate is a slot in the
    /// mixer's small table of sounds, and only when the table grows.
    pub fn define_with(&mut self, key: SoundKey, rate_hz: u32, samples: Vec<i16>) {
        self.collect();
        self.sounds.retain(|sound| sound.key() != key);
        self.sounds.push(Sound::new(key, rate_hz.max(1), samples));
    }

    /// Copies samples into a sound from sample `first`; samples beyond its
    /// end are dropped.
    pub fn samples(&mut self, key: SoundKey, first: usize, pcm: &[i16]) {
        let Some(sound) = self.sounds.iter_mut().find(|sound| sound.key() == key) else { return };
        sound.copy_in(first, pcm);
    }

    /// Forgets a sound once no voice plays it.
    pub fn forget(&mut self, key: SoundKey) {
        for sound in self.sounds.iter_mut().filter(|sound| sound.key() == key) {
            sound.forget();
        }
        self.collect();
    }

    /// Forgets every sound and silences every voice at once.
    pub fn clear(&mut self) {
        self.voices = [Voice::default(); VOICES];
        self.sounds.clear();
    }

    /// Starts a sound on a voice, ramping out what it played; or, with
    /// `after_current`, queues it for when the current sound ends.
    pub fn start(&mut self, voice: usize, start: Start) {
        let rate = self.rate_hz;
        let Some(sound) = self.sounds.iter().find(|sound| sound.key() == start.key) else { return };
        let source = sound.source();
        let Some(slot) = self.voices.get_mut(voice) else { return };
        if start.after_current && slot.playing.is_some_and(|playing| !playing.stopping) {
            slot.pending = Some(start);
            return;
        }
        slot.pending = None;
        let replaced = slot.playing.take().map(|mut old| {
            old.gain.to(0, u32::from(start.fade_ms), rate);
            old.stopping = true;
            slot.outgoing = Some(old);
            old.key
        });
        slot.playing = Some(begin(&start, source, rate));
        if let Some(key) = replaced {
            self.forget_stopped_cues(key);
        }
    }

    /// Ramps a voice to silence and stops it, dropping its waiting sound.
    pub fn stop(&mut self, voice: usize, fade_ms: u16) {
        let rate = self.rate_hz;
        let Some(slot) = self.voices.get_mut(voice) else { return };
        slot.pending = None;
        let stopped = slot.playing.as_mut().map(|playing| {
            playing.gain.to(0, u32::from(fade_ms), rate);
            playing.stopping = true;
            playing.key
        });
        if let Some(key) = stopped {
            self.forget_stopped_cues(key);
        }
    }

    /// Ramps a voice to a new gain.
    pub fn gain(&mut self, voice: usize, percent: u8, fade_ms: u16) {
        let rate = self.rate_hz;
        if let Some(playing) = self.voices.get_mut(voice).and_then(|slot| slot.playing.as_mut())
            && !playing.stopping
        {
            playing.gain.to(level(percent), u32::from(fade_ms), rate);
        }
    }

    /// The user's master volume, 0 to 100 per cent, and mute.
    pub fn set_master(&mut self, percent: u8, muted: bool) {
        self.master_percent = percent.min(100);
        self.muted = muted;
        let target = if muted { 0 } else { level(self.master_percent) };
        self.master.to(target, MIN_RAMP_MS, self.rate_hz);
    }

    /// Where the voice plays its stored sound: the sample it plays next, for
    /// a port's `VoiceSink::position` (`neoteletext-audio`, DEC-2470). `None`
    /// for a voice that is silent, stopping or playing a live sound, whose
    /// position counts its ring and not the sound. It is the mixer's
    /// position: a port whose output buffer lies between the mixer and the
    /// listener subtracts what that buffer holds.
    #[must_use]
    pub fn position(&self, voice: usize) -> Option<u32> {
        let playing = self.voices.get(voice)?.playing.filter(|playing| !playing.stopping)?;
        let sound = self.sounds.iter().find(|sound| sound.key() == playing.key)?;
        if sound.is_live() || sound.is_pulled() {
            return None;
        }
        u32::try_from(playing.whole.max(0)).ok()
    }

    /// Whether any voice is sounding or about to.
    #[must_use]
    pub fn active(&self) -> bool {
        self.voices.iter().any(|voice| voice.playing.is_some() || voice.outgoing.is_some() || voice.pending.is_some())
    }

    /// Renders mono samples at the output rate. It never allocates or frees.
    /// A pulled voice (RULE-62) is given silence: a port that plays one
    /// calls [`Mixer::render_pulling`].
    pub fn render(&mut self, out: &mut [i16]) {
        self.render_pulling(out, &mut pulled::Silence);
    }

    /// Renders mono samples at the output rate, asking `source` for each
    /// pulled voice's samples as they are needed (RULE-62). It never
    /// allocates or frees. The sum is clamped to the 16-bit range
    /// (REQ-AUD-019).
    pub fn render_pulling(&mut self, out: &mut [i16], source: &mut dyn Pull) {
        self.trim_streams();
        let pulling = self.sounds.iter().any(Sound::is_pulled);
        let length = out.len();
        for (done, sample) in out.iter_mut().enumerate() {
            let mixed = self.mixed_sample(pulling, length.saturating_sub(done), source);
            *sample = i16::try_from(mixed.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(0);
        }
    }

    /// Renders as [`Mixer::render_pulling`] does, but hands over the sum
    /// of the voices after the master volume as it is, not clamped: a
    /// sample may lie outside the 16-bit range, by as much as the voices
    /// sum to. For a port that sums several mixers and limits the whole
    /// itself, which a clamp here would hide the peaks from (DEC-6950).
    /// A port that plays the samples as they are calls
    /// [`Mixer::render_pulling`], whose clamp REQ-AUD-019 requires. It
    /// never allocates or frees.
    pub fn render_pulling_unclamped(&mut self, out: &mut [i32], source: &mut dyn Pull) {
        self.trim_streams();
        let pulling = self.sounds.iter().any(Sound::is_pulled);
        let length = out.len();
        for (done, sample) in out.iter_mut().enumerate() {
            *sample = self.mixed_sample(pulling, length.saturating_sub(done), source);
        }
    }

    /// One output sample: the voices' sum after the master volume, not
    /// clamped. `wanted` is how many samples of this buffer are still to
    /// come, this one counted, for what a pulled voice is fed.
    fn mixed_sample(&mut self, pulling: bool, wanted: usize, source: &mut dyn Pull) -> i32 {
        self.update_ducks();
        let mut sum: i32 = 0;
        for index in 0..VOICES {
            if pulling {
                self.feed(index, wanted, source);
            }
            let value = self.voice_sample(index);
            self.note_peak(index, value);
            sum = sum.saturating_add(value);
        }
        let master = self.master.advance();
        scale(sum, master)
    }

    /// One output sample of one voice, advancing it; a finished sound
    /// gives way to the waiting one.
    fn voice_sample(&mut self, index: usize) -> i32 {
        let rate = self.rate_hz;
        let Some(voice) = self.voices.get(index).copied() else { return 0 };
        let duck = voice.duck.map_or(UNITY, |ramp| ramp.level());
        let mut total: i32 = 0;
        let mut voice = voice;
        for slot in [&mut voice.playing, &mut voice.outgoing] {
            let Some(mut playing) = *slot else { continue };
            let gain = playing.gain.advance();
            let Some(sound) = self.sounds.iter().find(|sound| sound.key() == playing.key) else {
                *slot = None;
                continue;
            };
            if sound.is_pulled() {
                // Its samples were pulled as far as the filter reads
                // ([`Mixer::feed`]): nothing is buffered ahead, so it never
                // waits (RULE-62). Forgotten, it has no source and ends.
                if sound.is_forgotten() {
                    *slot = None;
                    continue;
                }
                let value = interpolate(sound, playing.whole, playing.fraction);
                total = total.saturating_add(scale(scale(value, gain), duck));
                advance(&mut playing);
                *slot = if playing.stopping && playing.gain.settled_at_zero() { None } else { Some(playing) };
                continue;
            }
            if sound.is_live() {
                if !playing.primed {
                    playing.primed = sound.ahead(playing.whole) >= sound.target();
                    if sound.is_forgotten() {
                        // It will get no more samples: what it has plays out.
                        playing.primed = true;
                    }
                }
                if playing.primed {
                    let value = interpolate(sound, playing.whole, playing.fraction);
                    total = total.saturating_add(scale(scale(value, gain), duck));
                    advance(&mut playing);
                    if sound.ahead(playing.whole) <= 0 {
                        if sound.is_forgotten() {
                            playing.stopping = true;
                            playing.gain = Ramp::at(0);
                        } else {
                            // Starved: wait, silent, for the target again.
                            playing.primed = false;
                        }
                    }
                }
                *slot = if playing.stopping && playing.gain.settled_at_zero() { None } else { Some(playing) };
                continue;
            }
            let value = interpolate(sound, playing.whole, playing.fraction);
            total = total.saturating_add(scale(scale(value, gain), duck));
            let length = sound.length();
            advance(&mut playing);
            if playing.whole >= length {
                if playing.looped && length > 0 {
                    playing.whole = playing.whole.checked_rem(length).unwrap_or(0);
                } else {
                    playing.stopping = true;
                    playing.gain = Ramp::at(0);
                }
            }
            *slot = if playing.stopping && playing.gain.settled_at_zero() { None } else { Some(playing) };
        }
        if voice.playing.is_none()
            && let Some(next) = voice.pending.take()
            && let Some(source) = self.sounds.iter().find(|sound| sound.key() == next.key).map(Sound::source)
        {
            voice.playing = Some(begin(&next, source, rate));
        }
        if let Some(place) = self.voices.get_mut(index) {
            *place = voice;
        }
        total
    }

    /// Sets each voice's duck ramp toward the lowest level any other
    /// sounding voice asks for.
    fn update_ducks(&mut self) {
        let rate = self.rate_hz;
        let asks: [Option<(u8, u16)>; VOICES] = core::array::from_fn(|index| {
            self.voices
                .get(index)
                .and_then(|voice| voice.playing)
                .filter(|playing| !playing.stopping && playing.duck_percent < 100)
                .map(|playing| (playing.duck_percent, playing.fade_ms))
        });
        for (index, voice) in self.voices.iter_mut().enumerate() {
            let strongest = asks
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .filter_map(|(_, ask)| *ask)
                .min_by_key(|&(percent, _)| percent);
            let (target, fade) =
                strongest.map_or((UNITY, MIN_RAMP_MS), |(percent, fade)| (level(percent), u32::from(fade)));
            let mut ramp = voice.duck.unwrap_or(Ramp::at(UNITY));
            if ramp.target() != target {
                ramp.to(target, fade, rate);
            }
            ramp.advance();
            voice.duck = Some(ramp);
        }
    }

    /// Drops forgotten sounds no voice plays.
    fn collect(&mut self) {
        let voices = self.voices;
        self.sounds.retain(|sound| {
            !sound.is_forgotten()
                || voices.iter().any(|voice| {
                    [voice.playing, voice.outgoing].iter().flatten().any(|playing| playing.key == sound.key())
                        || voice.pending.is_some_and(|start| start.key == sound.key())
                })
        });
    }
}

/// A sound starting on a voice, its gain ramping in. A live sound's voice
/// joins [`STREAM_TARGET_MS`] behind the newest sample written; a pulled
/// sound's voice starts at the next sample pulled, with nothing held ahead
/// (RULE-62).
fn begin(start: &Start, source: Source, output_rate: u32) -> Playing {
    let (source_rate, written, joined) = match source {
        Source::Stored(rate) => (rate, None, None),
        Source::Live(rate, written) => {
            let target = i64::from(rate.saturating_mul(STREAM_TARGET_MS) / 1000);
            (rate, Some(written), Some(written.saturating_sub(target).max(0)))
        }
        Source::Pulled(written) => (PULLED_RATE_HZ, None, Some(written)),
    };
    // The step is source rate / output rate, in 32.32 fixed point.
    let nominal = (u64::from(source_rate) << FRACTION_BITS).checked_div(u64::from(output_rate)).unwrap_or(0);
    let mut gain = Ramp::at(0);
    gain.to(level(start.gain), u32::from(start.fade_ms), output_rate);
    let mut playing = Playing {
        key: start.key,
        whole: joined.unwrap_or(0),
        fraction: 0,
        step_whole: 0,
        step_fraction: 0,
        nominal,
        primed: written.is_none(),
        looped: start.looped && joined.is_none(),
        gain,
        duck_percent: start.duck_percent,
        fade_ms: start.fade_ms,
        stopping: false,
    };
    set_step(&mut playing, nominal);
    playing
}

/// Sets a voice's step from 32.32 fixed point.
fn set_step(playing: &mut Playing, step: u64) {
    playing.step_whole = i64::try_from(step >> FRACTION_BITS).unwrap_or(0);
    playing.step_fraction = u32::try_from(step & u64::from(u32::MAX)).unwrap_or(0);
}

fn advance(playing: &mut Playing) {
    let (fraction, carry) = playing.fraction.overflowing_add(playing.step_fraction);
    playing.fraction = fraction;
    playing.whole = playing.whole.saturating_add(playing.step_whole).saturating_add(i64::from(carry));
}

/// The sound's value between samples `whole` and `whole + 1`, filtered by
/// the windowed sinc; samples outside the sound are silence.
fn interpolate(sound: &Sound, whole: i64, fraction: u32) -> i32 {
    let phase = usize::try_from(fraction >> PHASE_SHIFT).unwrap_or(0);
    let Some(taps) = FILTER.get(phase) else { return 0 };
    let first = whole.saturating_sub(TAPS_BEFORE);
    let mut sum: i32 = 0;
    for (offset, &tap) in (0_i64..).zip(taps.iter()) {
        let at = first.saturating_add(offset);
        let value = sound.at(at);
        sum = sum.saturating_add(i32::from(value).saturating_mul(i32::from(tap)));
    }
    sum / ONE
}

#[cfg(test)]
mod tests;
