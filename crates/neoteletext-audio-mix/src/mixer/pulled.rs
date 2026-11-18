//! The pulled voice (page-rules.md, RULE-62): a voice whose samples the
//! mixer asks for as it mixes, a block at a time, and never ahead. It is how
//! a local sound (RULE-60) answers a key at once: a stored sound's samples
//! are all there before it plays and a live sound's are held
//! [`STREAM_TARGET_MS`](crate::STREAM_TARGET_MS) ahead, but a pulled voice
//! holds less than one block, so a MIDI message given to its instrument
//! before a block is heard within that block.
//!
//! The mixer asks its [`Pull`] source for samples at [`PULLED_RATE_HZ`]
//! whenever the voice's resampler would read past what has been pulled, a
//! block of [`LOCAL_BLOCK_SAMPLES`] at a time, and resamples them to the
//! output's rate as it does every sound. It pulls no further than the
//! render call in hand reads: the last block of a call is cut to what the
//! call still needs, so the next call's first sample pulls afresh, and
//! what a source renders from a message queued before a device buffer is
//! heard from that buffer's first samples, not from wherever in a block the
//! buffer before it ended.

use super::{FRACTION_BITS, Mixer, Playing, VOICES};
use crate::sinc::{TAPS, TAPS_BEFORE};
use crate::sound::{Sound, SoundKey};
use alloc::vec::Vec;

/// The most samples of one block of a pulled voice (RULE-62: "blocks of
/// `LOCAL_BLOCK_SAMPLES` (128 samples, 2.67 milliseconds)"). A message to a
/// local sound's instrument waits at most this long for the next block. A
/// render call's last block is shorter where the call needs fewer.
pub const LOCAL_BLOCK_SAMPLES: usize = 128;
/// The rate a pulled voice's samples are asked for at (RULE-62: "at 48,000
/// samples a second"): the one rate an instrument is rendered at
/// (REQ-6411 item 1).
pub const PULLED_RATE_HZ: u32 = 48_000;
/// The samples past its position the resampler's filter reads, which a
/// pulled voice must therefore hold before a sample is mixed: the filter's
/// taps after its centre.
pub const PULLED_LOOKAHEAD_SAMPLES: usize = TAPS - 1 - TAPS_BEFORE as usize;
/// The blocks a pulled sound's ring holds: the block being played, the one
/// pulled for the filter's taps after it, and the samples before the
/// position the filter still reads, with one to spare.
const RING_BLOCKS: usize = 4;
/// The samples of a pulled sound's ring.
const RING_SAMPLES: usize = RING_BLOCKS * LOCAL_BLOCK_SAMPLES;
// The ring holds every sample the filter reads around a position, with a
// whole block pulled past it.
const _: () = assert!(RING_SAMPLES >= TAPS + 2 * LOCAL_BLOCK_SAMPLES);
// RULE-62: a block is 2.67 milliseconds, 128 samples of 48,000 a second.
const _: () = assert!(LOCAL_BLOCK_SAMPLES as u32 * 1_000_000 / PULLED_RATE_HZ == 2_666);

/// Where the mixer gets a pulled voice's samples: the port's instruments.
pub trait Pull {
    /// Fills `block`, of 1 to [`LOCAL_BLOCK_SAMPLES`] samples, with the
    /// next samples, at [`PULLED_RATE_HZ`], of the pulled sound `key` that
    /// voice `voice` plays. It is called from [`Mixer::render_pulling`], on
    /// the audio thread, and must not allocate or wait. A source applies
    /// what is queued for the sound's instrument, then renders the block
    /// (RULE-62).
    fn pull(&mut self, voice: usize, key: SoundKey, block: &mut [i16]);
}

/// The source of [`Mixer::render`], which has none: silence.
pub(super) struct Silence;

impl Pull for Silence {
    fn pull(&mut self, _voice: usize, _key: SoundKey, block: &mut [i16]) {
        block.fill(0);
    }
}

/// A voice's source position, whole samples and fraction in one.
fn position(playing: &Playing) -> i128 {
    i128::from(playing.whole) << FRACTION_BITS | i128::from(playing.fraction)
}

impl Mixer {
    /// Makes a pulled sound (RULE-62): one whose samples the mixer asks its
    /// [`Pull`] source for while a voice plays it. A sound of the same key
    /// is replaced. A pulled sound is played by one voice at a time. It
    /// allocates the sound's small ring; a caller that holds a lock the
    /// audio callback takes allocates it beforehand and calls
    /// [`Mixer::pulled_with`].
    pub fn pulled(&mut self, key: SoundKey) {
        let made = self.pulled_with(key, alloc::vec![0; Self::pulled_ring()]);
        debug_assert!(made.is_ok(), "a ring of pulled_ring's length is taken");
    }

    /// Makes a pulled sound, as [`Mixer::pulled`] does, whose ring is
    /// `ring`, the buffer as given and never copied.
    ///
    /// # Errors
    ///
    /// The ring, when its length is not [`Mixer::pulled_ring`]; the mixer
    /// is then unchanged.
    pub fn pulled_with(&mut self, key: SoundKey, ring: Vec<i16>) -> Result<(), Vec<i16>> {
        if ring.len() != Self::pulled_ring() {
            return Err(ring);
        }
        self.collect();
        self.sounds.retain(|sound| sound.key() != key);
        self.sounds.push(Sound::pulled(key, ring));
        Ok(())
    }

    /// The samples of a pulled sound's ring.
    #[must_use]
    pub const fn pulled_ring() -> usize {
        RING_SAMPLES
    }

    /// Pulls blocks of the pulled sound voice `index` plays until the
    /// resampler has every sample it reads for the voice's next output
    /// sample: just in time (RULE-62). A block is cut to what the voice
    /// reads in the `remaining` output samples of the render call, this one
    /// counted, so that nothing is pulled for a call yet to come.
    pub(super) fn feed(&mut self, index: usize, remaining: usize, source: &mut dyn Pull) {
        let Some(voice) = self.voices.get(index).copied() else { return };
        if index >= VOICES {
            return;
        }
        let lookahead = i64::try_from(PULLED_LOOKAHEAD_SAMPLES).unwrap_or(0);
        for playing in [voice.playing, voice.outgoing].into_iter().flatten() {
            let Some(sound) = self.sounds.iter_mut().find(|sound| sound.key() == playing.key && sound.is_pulled())
            else {
                continue;
            };
            if sound.is_forgotten() {
                continue;
            }
            let needed = playing.whole.saturating_add(lookahead).saturating_add(1);
            // Where the voice stands at the call's last output sample: its
            // position now and its step for each sample after this one.
            let steps = i128::try_from(remaining.saturating_sub(1)).unwrap_or(0);
            let last = position(&playing).saturating_add(steps.saturating_mul(i128::from(playing.nominal)));
            let last = i64::try_from(last >> FRACTION_BITS).unwrap_or(i64::MAX);
            let call_needs = last.saturating_add(lookahead).saturating_add(1);
            // An output rate far below the source's steps over several
            // blocks a sample; never more than the ring holds.
            for _ in 0..RING_BLOCKS {
                if sound.written() >= needed {
                    break;
                }
                let wanted = usize::try_from(call_needs.saturating_sub(sound.written())).unwrap_or(0);
                let mut block = [0_i16; LOCAL_BLOCK_SAMPLES];
                let Some(block) = block.get_mut(..wanted.clamp(1, LOCAL_BLOCK_SAMPLES)) else { break };
                source.pull(index, playing.key, block);
                sound.write_live(block);
            }
        }
    }
}
