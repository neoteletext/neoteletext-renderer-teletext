//! The pulled voice (page-rules.md, RULE-62): a voice whose samples the
//! mixer asks for as it mixes, a block at a time, and never ahead. It is how
//! a local sound (RULE-60) answers a key at once: a stored sound's samples
//! are all there before it plays and a live sound's are held
//! [`STREAM_TARGET_MS`](crate::STREAM_TARGET_MS) ahead, but a pulled voice
//! holds less than one block, so a MIDI message given to its instrument
//! before a block is heard within that block.
//!
//! The mixer asks its [`Pull`] source for [`LOCAL_BLOCK_SAMPLES`] samples at
//! [`PULLED_RATE_HZ`] whenever the voice's resampler would read past what
//! has been pulled, and resamples them to the output's rate as it does
//! every sound.

use super::{Mixer, VOICES};
use crate::sinc::{TAPS, TAPS_BEFORE};
use crate::sound::{Sound, SoundKey};
use alloc::vec::Vec;

/// The samples of one block of a pulled voice (RULE-62: "blocks of
/// `LOCAL_BLOCK_SAMPLES` (128 samples, 2.67 milliseconds)"). A message to a
/// local sound's instrument waits at most this long for the next block.
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
    /// Fills `block` with the next [`LOCAL_BLOCK_SAMPLES`] samples, at
    /// [`PULLED_RATE_HZ`], of the pulled sound `key` that voice `voice`
    /// plays. It is called from [`Mixer::render_pulling`], on the audio
    /// thread, and must not allocate or wait. A source applies what is
    /// queued for the sound's instrument, then renders the block (RULE-62).
    fn pull(&mut self, voice: usize, key: SoundKey, block: &mut [i16; LOCAL_BLOCK_SAMPLES]);
}

/// The source of [`Mixer::render`], which has none: silence.
pub(super) struct Silence;

impl Pull for Silence {
    fn pull(&mut self, _voice: usize, _key: SoundKey, block: &mut [i16; LOCAL_BLOCK_SAMPLES]) {
        block.fill(0);
    }
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
    /// sample: just in time, and no further (RULE-62).
    pub(super) fn feed(&mut self, index: usize, source: &mut dyn Pull) {
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
            // An output rate far below the source's steps over several
            // blocks a sample; never more than the ring holds.
            for _ in 0..RING_BLOCKS {
                if sound.written() >= needed {
                    break;
                }
                let mut block = [0_i16; LOCAL_BLOCK_SAMPLES];
                source.pull(index, playing.key, &mut block);
                sound.write_live(&block);
            }
        }
    }
}
