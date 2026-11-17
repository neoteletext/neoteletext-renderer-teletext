//! The gain of an instrument's volume control on its way to where the
//! control was last put.
//!
//! A gain that changed between one sample and the next would step the
//! signal under a sounding note by the whole of its change, which is a
//! click; so the gain an instrument multiplies by is not what its volume
//! controller asks but [`VolumeGain`], which goes there a little each
//! sample. It is counted in samples and in nothing else, so the same
//! messages give the same samples however the calls to render are cut.

use super::filter::ONE_Q28;

/// The most the gain moves in one sample, in Q28: from nothing to all of
/// it in 256 samples, 5.3 ms at the output rate.
///
/// Source: own choice: long against a sample and short against a hand
/// on a volume control.
pub const VOLUME_SLEW: i32 = ONE_Q28 >> 8;

/// The largest value of a volume controller.
///
/// Source: MIDI 1.0: a data byte is seven bits.
const CONTROLLER_FULL: i64 = 0x7f;
/// Its square, which the square of a controller is a share of.
const CONTROLLER_FULL_SQUARED: i64 = CONTROLLER_FULL * CONTROLLER_FULL;

/// The gain a volume controller asks for, in Q28: the square of its
/// share of its largest value, 127, so half the controller is a quarter of
/// the level. A value above the controller's largest, which no message
/// carries, asks for as much more.
///
/// Source: own choice of the law, as a volume control's taper.
#[must_use]
pub fn wanted(controller: u8) -> i32 {
    let square = i64::from(controller).saturating_mul(i64::from(controller));
    i32::try_from(square.saturating_mul(i64::from(ONE_Q28)) / CONTROLLER_FULL_SQUARED).unwrap_or(ONE_Q28)
}

/// A volume's gain as it stands, in Q28.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VolumeGain {
    gain: i32,
}

impl VolumeGain {
    /// The gain of a volume control at its top, one in Q28: where an
    /// instrument's is before any message.
    #[must_use]
    pub const fn full() -> Self {
        Self { gain: ONE_Q28 }
    }

    /// The gain of the next sample: [`VOLUME_SLEW`] nearer to `wanted`
    /// than the last, or `wanted` itself once it is that near, which it
    /// then stays at and never passes. A `wanted` that changes on the
    /// way is gone towards from where the gain stands.
    #[inline]
    #[must_use]
    pub fn towards(&mut self, wanted: i32) -> i32 {
        let step = wanted.saturating_sub(self.gain).clamp(-VOLUME_SLEW, VOLUME_SLEW);
        self.gain = self.gain.saturating_add(step);
        self.gain
    }
}

impl Default for VolumeGain {
    fn default() -> Self {
        Self::full()
    }
}

#[cfg(test)]
mod tests {
    use super::{ONE_Q28, VOLUME_SLEW, VolumeGain, wanted};

    #[test]
    fn a_controller_asks_for_the_square_of_its_share() {
        assert_eq!(wanted(0), 0);
        assert_eq!(wanted(127), ONE_Q28);
        // 64 squared of 127 squared, cut to a whole number of Q28.
        assert_eq!(wanted(64), 68_169_857);
        assert_eq!(i64::from(wanted(64)), 4_096 * i64::from(ONE_Q28) / 16_129);
        // Each step of the controller is more gain than the last.
        assert!((1..=127u8).all(|value| wanted(value) > wanted(value - 1)));
    }

    #[test]
    fn it_stays_where_it_is_wanted() {
        let mut gain = VolumeGain::full();
        for _ in 0..1_000 {
            assert_eq!(gain.towards(ONE_Q28), ONE_Q28);
        }
    }

    #[test]
    fn it_arrives_exactly_and_never_passes() {
        // A target that is no whole number of steps away, either way.
        let wanted = ONE_Q28 / 3 + 5;
        for start in [ONE_Q28, 0] {
            let mut gain = VolumeGain::full();
            for _ in 0..256 {
                let _ = gain.towards(start);
            }
            let mut last = start;
            let mut samples = 0;
            while last != wanted {
                let now = gain.towards(wanted);
                assert!((now - last).abs() <= VOLUME_SLEW, "{last} to {now}");
                assert!((now - wanted).abs() < (last - wanted).abs(), "{last} to {now}");
                assert!(now == wanted || (now > wanted) == (start > wanted), "{now} is past {wanted}");
                last = now;
                samples += 1;
                assert!(samples <= 256, "it takes more than 256 samples");
            }
            assert_eq!(gain.towards(wanted), wanted);
        }
    }

    #[test]
    fn the_whole_way_is_256_samples() {
        let mut gain = VolumeGain::full();
        let samples = (1..=1_000).find(|_| gain.towards(0) == 0);
        assert_eq!(samples, Some(256));
    }

    #[test]
    fn a_target_that_changes_on_the_way_is_gone_towards_from_there() {
        let mut gain = VolumeGain::full();
        let mut last = ONE_Q28;
        for _ in 0..100 {
            last = gain.towards(0);
        }
        assert_eq!(last, ONE_Q28 - 100 * VOLUME_SLEW);
        assert_eq!(gain.towards(ONE_Q28), last + VOLUME_SLEW);
        // Any target at all, the extremes of an i32 with them, is gone
        // towards without a wrap.
        for wanted in [i32::MAX, i32::MIN, 0] {
            let before = gain.towards(wanted);
            let after = gain.towards(wanted);
            assert!((after - before).abs() <= VOLUME_SLEW);
        }
    }
}
