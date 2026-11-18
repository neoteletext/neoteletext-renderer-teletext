//! Each voice's peak since it was last asked for (page-rules.md, RULE-32):
//! what a port reports as a channel's `level` to a page whose rules watch
//! it. The peak is of the voice after its gain, ramp and ducking and before
//! the master volume and the mute, so that a page's meter shows what the
//! page plays and learns nothing of the viewer's volume.

use super::{Mixer, VOICES};

/// The peak of a full-scale voice: the magnitude of the most negative
/// 16-bit sample (RULE-32: "peak is on the 16-bit scale 0 to 32,768").
pub const PEAK_FULL_SCALE: u16 = 32_768;
/// The peak that is one step of a level (RULE-32: `level = min(255, peak ÷
/// 128)`), so that level 128 is half of full scale, −6 decibels.
pub const PEAK_PER_LEVEL: u16 = 128;

/// The level, 0 to 255, a peak is reported as: `min(255, peak ÷ 128)`
/// (RULE-32). It is linear: 255 is full scale, 128 is −6 decibels.
#[must_use]
pub const fn meter_level(peak: u16) -> u8 {
    let level = peak / PEAK_PER_LEVEL;
    if level > u8::MAX as u16 { u8::MAX } else { level as u8 }
}

impl Mixer {
    /// Keeps, or stops keeping, voice `voice`'s peak (RULE-32: "nothing is
    /// metered for a channel no rule watches"). Turning it on or off clears
    /// the peak held. A voice past the last is ignored.
    pub fn set_metered(&mut self, voice: usize, on: bool) {
        let Some(bit) = u32::try_from(voice).ok().filter(|_| voice < VOICES).and_then(|voice| 1_u8.checked_shl(voice))
        else {
            return;
        };
        if on {
            self.metered |= bit;
        } else {
            self.metered &= !bit;
        }
        if let Some(peak) = self.peaks.get_mut(voice) {
            *peak = 0;
        }
    }

    /// Whether voice `voice`'s peak is kept.
    #[must_use]
    pub fn is_metered(&self, voice: usize) -> bool {
        voice < VOICES
            && u32::try_from(voice).ok().and_then(|voice| self.metered.checked_shr(voice)).unwrap_or(0) & 1 == 1
    }

    /// Voice `voice`'s peak since this was last called, 0 to
    /// [`PEAK_FULL_SCALE`]: the largest magnitude of the voice's samples
    /// after its gain, ramp and ducking, before the master volume and the
    /// mute, over the output samples rendered since (RULE-32). The peak
    /// held starts again from 0. It is 0 for a voice that is not metered.
    pub fn take_peak(&mut self, voice: usize) -> u16 {
        self.peaks.get_mut(voice).map_or(0, core::mem::take)
    }

    /// Notes one output sample of voice `index` in its peak, when it is
    /// metered.
    pub(super) fn note_peak(&mut self, index: usize, value: i32) {
        if !self.is_metered(index) {
            return;
        }
        let magnitude = u16::try_from(value.unsigned_abs().min(u32::from(PEAK_FULL_SCALE))).unwrap_or(PEAK_FULL_SCALE);
        if let Some(peak) = self.peaks.get_mut(index) {
            *peak = (*peak).max(magnitude);
        }
    }
}
