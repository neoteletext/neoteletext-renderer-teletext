//! The envelope generators of the analogue building blocks: the length
//! and the shape of each stage against the times set (TODO-6430).

// Without the feature there is nothing here to test.
#![cfg(feature = "analog")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

use neoteletext_audio_instruments::analog::LEVEL;
use neoteletext_audio_instruments::analog::envelope::{
    ATTACK_TARGET, ATTACK_TARGET_FLOOR, ENVELOPE_ONE, Fall, IMMEDIATE, REST_LEVEL, RcEnvelope, Stage, SteppedEnvelope,
    apply, linear_step, time_constant,
};

const RATE: u32 = 48_000;
const ONE: f64 = ENVELOPE_ONE as f64;

#[test]
fn the_coefficient_is_one_less_the_exponential() {
    let mut worst = 0.0f64;
    for rate in [48_000u32, 96_000, 192_000, 750] {
        for microseconds in [1u32, 10, 21, 50, 500, 1_500, 20_000, 1_000_000, 12_000_000, u32::MAX] {
            let exact = 1.0 - (-1.0e6 / (f64::from(microseconds) * f64::from(rate))).exp();
            let found = f64::from(time_constant(microseconds, rate)) / 2_147_483_648.0;
            worst = worst.max((found - exact).abs());
        }
    }
    assert!(worst < 1.0e-9, "{worst}");
    // No time at all, and no rate, are immediate.
    assert_eq!((time_constant(0, RATE), time_constant(1_000, 0)), (IMMEDIATE, IMMEDIATE));
}

#[test]
fn a_time_constant_of_a_whole_power_of_two_of_samples_is_no_exception() {
    // A million over a power of two of microsecond-hertz makes x, or x
    // halved, exactly one in Q32: the series' first product is then 2^64,
    // and must not wrap to nothing.
    for (microseconds, rate) in
        [(1_000u32, 1_000u32), (20, 50_000), (500, 1_000), (250, 1_000), (125, 1_000), (125, 500), (125, 250)]
    {
        let exact = 1.0 - (-1.0e6 / (f64::from(microseconds) * f64::from(rate))).exp();
        let found = f64::from(time_constant(microseconds, rate)) / 2_147_483_648.0;
        assert!((found - exact).abs() < 1.0e-9, "{microseconds} us at {rate} Hz: {found} for {exact}");
    }
}

/// Samples until `done`, ticking the envelope.
fn samples_until(envelope: &mut RcEnvelope, mut done: impl FnMut(&RcEnvelope, i32) -> bool) -> usize {
    for sample in 1..=20 * RATE as usize {
        let level = envelope.tick();
        if done(envelope, level) {
            return sample;
        }
    }
    panic!("the envelope never got there");
}

#[test]
fn the_analogue_attack_overshoots_its_peak_and_takes_1_466_time_constants() {
    assert!((f64::from(ATTACK_TARGET) / ONE - 1.3).abs() < 1e-9);
    assert_eq!(RcEnvelope::ATTACK_TIME_CONSTANTS_THOUSANDTHS, ((13.0f64 / 3.0).ln() * 1000.0) as u32);
    for milliseconds in [2u32, 50, 1_000] {
        let mut envelope = RcEnvelope::new();
        envelope.set_attack(time_constant(milliseconds * 1000, RATE));
        envelope.gate(true);
        let mut levels = Vec::new();
        let samples = samples_until(&mut envelope, |envelope, level| {
            levels.push(level);
            envelope.stage() == Stage::Decay
        });
        let expected = (13.0f64 / 3.0).ln() * f64::from(milliseconds) / 1000.0 * f64::from(RATE);
        assert!(
            (samples as f64 - expected).abs() <= 1.0 + expected * 1e-4,
            "{milliseconds} ms: {samples} for {expected}"
        );
        // The curve on the way: 1.3 (1 - exp(-t / tau)), still steep at
        // the peak, where a curve to the peak itself would be flat.
        let time_constant_samples = f64::from(milliseconds) / 1000.0 * f64::from(RATE);
        for (index, &level) in levels.iter().enumerate().step_by(7) {
            let exact = 1.3 * (1.0 - (-((index + 1) as f64) / time_constant_samples).exp());
            assert!((f64::from(level) / ONE - exact.min(1.0)).abs() < 1e-5, "{milliseconds} ms at {index}");
        }
        assert_eq!(levels[levels.len() - 1], ENVELOPE_ONE);
    }
}

#[test]
fn an_instruments_own_attack_target_sets_how_long_the_attack_takes() {
    // Towards twice the peak the attack takes ln 2 time constants; the
    // target is held between its floor and the largest level.
    let time = |target: i32| {
        let mut envelope = RcEnvelope::new();
        envelope.set_attack_target(target);
        envelope.set_attack(time_constant(100_000, RATE));
        envelope.gate(true);
        samples_until(&mut envelope, |envelope, _| envelope.stage() == Stage::Decay) as f64 / f64::from(RATE)
    };
    let twice = time(i32::MAX);
    assert!((twice - 0.1 * f64::ln(2.0)).abs() < 0.0002, "{twice}");
    let floor = 0.1 * (f64::from(ATTACK_TARGET_FLOOR) / f64::from(ATTACK_TARGET_FLOOR - ENVELOPE_ONE)).ln();
    assert!((time(0) - floor).abs() < 0.0002 && (time(ENVELOPE_ONE) - floor).abs() < 0.0002);
    // Unset, it is the block's own.
    let unset = 0.1 * (f64::from(ATTACK_TARGET) / f64::from(ATTACK_TARGET - ENVELOPE_ONE)).ln();
    assert!((time(ATTACK_TARGET) - unset).abs() < 0.0002);
}

#[test]
fn the_analogue_decay_and_release_fall_by_e_in_a_time_constant() {
    for milliseconds in [5u32, 200, 800] {
        let mut envelope = RcEnvelope::new();
        envelope.set_decay(time_constant(milliseconds * 1000, RATE));
        envelope.set_release(time_constant(milliseconds * 2000, RATE));
        envelope.set_sustain(ENVELOPE_ONE / 4);
        envelope.gate(true);
        // The attack is immediate: one sample to the peak.
        assert_eq!((envelope.tick(), envelope.stage()), (ENVELOPE_ONE, Stage::Decay));
        // From 1 towards 0.25: 0.25 + 0.75 / e after a time constant.
        let after = 0.25 + 0.75 / std::f64::consts::E;
        let samples = samples_until(&mut envelope, |_, level| f64::from(level) / ONE <= after);
        let expected = f64::from(milliseconds) / 1000.0 * f64::from(RATE);
        assert!((samples as f64 - expected).abs() <= 1.0 + expected * 1e-4, "decay {milliseconds}: {samples}");
        // It settles on the sustain level and stays, to the last unit.
        for _ in 0..40 * expected as usize {
            envelope.tick();
        }
        assert!((envelope.level() - ENVELOPE_ONE / 4).abs() <= 1, "{}", envelope.level());
        assert_eq!(envelope.stage(), Stage::Decay);

        envelope.gate(false);
        let samples = samples_until(&mut envelope, |_, level| f64::from(level) / ONE <= 0.25 / std::f64::consts::E);
        assert!((samples as f64 - 2.0 * expected).abs() <= 1.0 + expected * 2e-4, "release {milliseconds}: {samples}");
        // And comes to rest: ln(0.25 * 32768) time constants in all.
        let rest = samples + samples_until(&mut envelope, |envelope, _| envelope.stage() == Stage::Rest);
        let expected_rest = (0.25 * ONE / f64::from(REST_LEVEL)).ln() * 2.0 * expected;
        assert!((rest as f64 - expected_rest).abs() <= 2.0 + expected_rest * 1e-3, "rest {milliseconds}: {rest}");
        assert_eq!((envelope.tick(), envelope.level()), (0, 0));
    }
}

#[test]
fn the_slowest_analogue_release_still_reaches_rest() {
    // A time constant of 20 s at four times the output rate moves by a
    // four-millionth of the distance a sample; the fraction carried keeps
    // it moving where a product cut to a unit would stop four million
    // units up, 49 dB below the peak.
    let rate = 192_000;
    let mut envelope = RcEnvelope::new();
    envelope.set_release(time_constant(20_000_000, rate));
    envelope.gate(true);
    envelope.tick();
    envelope.gate(false);
    let mut level = ENVELOPE_ONE;
    let mut samples = 0u64;
    while envelope.stage() != Stage::Rest {
        let next = envelope.tick();
        assert!(next <= level);
        level = next;
        samples += 1;
        assert!(samples < 250 * u64::from(rate), "stuck at {level}");
    }
    let expected = (ONE / f64::from(REST_LEVEL)).ln() * 20.0 * f64::from(rate);
    assert!((samples as f64 / expected - 1.0).abs() < 1e-3, "{samples} for {expected}");
}

#[test]
fn a_gate_in_any_stage_does_what_a_key_does() {
    let mut envelope = RcEnvelope::new();
    envelope.set_attack(time_constant(10_000, RATE));
    envelope.set_decay(time_constant(10_000, RATE));
    envelope.set_release(time_constant(10_000, RATE));
    envelope.set_sustain(ENVELOPE_ONE / 2);
    // Off at rest stays at rest.
    envelope.gate(false);
    assert_eq!((envelope.stage(), envelope.tick()), (Stage::Rest, 0));
    // Released in the attack, it falls from where it was; taken again in
    // the release, it rises from where it is, with no jump either way.
    envelope.gate(true);
    let mut last = 0;
    for _ in 0..200 {
        last = envelope.tick();
    }
    envelope.gate(false);
    let fallen = envelope.tick();
    assert!(envelope.stage() == Stage::Release && fallen < last && last - fallen < ENVELOPE_ONE / 100);
    envelope.gate(true);
    let risen = envelope.tick();
    assert!(envelope.stage() == Stage::Attack && risen > fallen && risen - fallen < ENVELOPE_ONE / 100);
    // A second gate while it is on changes nothing; a retrigger starts
    // the attack again from the sustain level.
    for _ in 0..20_000 {
        envelope.tick();
    }
    envelope.gate(true);
    assert_eq!(envelope.stage(), Stage::Decay);
    envelope.retrigger();
    assert!(envelope.stage() == Stage::Attack && envelope.tick() > ENVELOPE_ONE / 2);
    // Values beyond their ranges are held to them.
    envelope.set_sustain(i32::MAX);
    envelope.set_attack(-5);
    envelope.set_decay(i32::MIN);
    envelope.set_release(i32::MAX);
    for _ in 0..10 {
        assert!((0..=ENVELOPE_ONE).contains(&envelope.tick()));
    }
}

#[test]
fn the_stepped_envelope_is_a_staircase_of_straight_lines() {
    // Updated 750 times a second, every 64 samples: an attack of 100 ms,
    // a decay of 200 ms for the whole range, a release of 400 ms.
    let (interval, updates) = (64u32, RATE / 64);
    let mut envelope = SteppedEnvelope::new();
    envelope.set_interval(interval);
    envelope.set_attack(linear_step(100_000, updates));
    envelope.set_decay(Fall::Linear(linear_step(200_000, updates)));
    envelope.set_release(Fall::Linear(linear_step(400_000, updates)));
    envelope.set_sustain(ENVELOPE_ONE / 2);
    envelope.gate(true);
    let levels: Vec<i32> = (0..RATE).map(|_| envelope.tick()).collect();
    // A staircase: it changes only where an update falls.
    for (index, pair) in levels.windows(2).enumerate() {
        assert!(pair[0] == pair[1] || (index + 1) % interval as usize == 0, "a step at {index}");
    }
    // The peak 100 ms in, give or take an update; the sustain level half
    // of 200 ms later; straight on the way.
    let peak = levels.iter().position(|&level| level == ENVELOPE_ONE).unwrap_or(0);
    assert!(peak.abs_diff(4_800) <= interval as usize, "{peak}");
    let sustained = levels.iter().rposition(|&level| level > ENVELOPE_ONE / 2).unwrap_or(0) + 1;
    assert!(sustained.abs_diff(peak + 4_800) <= interval as usize, "{sustained}");
    assert!(
        (f64::from(levels[2_400]) / ONE - 0.5).abs() < 0.02
            && (f64::from(levels[peak + 2_400]) / ONE - 0.75).abs() < 0.02
    );
    assert_eq!((levels[RATE as usize - 1], envelope.stage()), (ENVELOPE_ONE / 2, Stage::Decay));

    envelope.gate(false);
    let released: Vec<i32> = (0..RATE).map(|_| envelope.tick()).collect();
    let rest = released.iter().position(|&level| level == 0).unwrap_or(0);
    // Half the range at 400 ms for the whole.
    assert!(rest.abs_diff(9_600) <= 2 * interval as usize, "{rest}");
    assert_eq!(envelope.stage(), Stage::Rest);
}

#[test]
fn the_stepped_envelope_falls_by_shares_as_a_processor_multiplies() {
    let (interval, updates) = (48u32, RATE / 48);
    let mut envelope = SteppedEnvelope::new();
    envelope.set_interval(interval);
    envelope.set_decay(Fall::Proportional(time_constant(50_000, updates)));
    envelope.set_release(Fall::Proportional(time_constant(100_000, updates)));
    envelope.set_sustain(ENVELOPE_ONE / 5);
    envelope.gate(true);
    assert_eq!(envelope.tick(), ENVELOPE_ONE);
    // After a time constant of decay, 0.2 + 0.8 / e, read at an update.
    let levels: Vec<i32> = (0..RATE).map(|_| envelope.tick()).collect();
    let at = 2_400 - 1;
    assert!((f64::from(levels[at]) / ONE - (0.2 + 0.8 / std::f64::consts::E)).abs() < 2e-3, "{}", levels[at]);
    assert!((levels[RATE as usize - 1] - ENVELOPE_ONE / 5).abs() <= 1024);
    // A sustain level raised while it is held is climbed to, not jumped to.
    envelope.set_sustain(ENVELOPE_ONE / 2);
    let climbed = (0..interval).map(|_| envelope.tick()).last().unwrap_or(0);
    assert!(climbed > ENVELOPE_ONE / 5 && climbed < ENVELOPE_ONE / 2, "{climbed}");
    envelope.set_sustain(ENVELOPE_ONE / 5);
    for _ in 0..RATE {
        envelope.tick();
    }
    envelope.gate(false);
    let released: Vec<i32> = (0..2 * RATE).map(|_| envelope.tick()).collect();
    assert!((f64::from(released[4_800 - 1]) / ONE - 0.2 / std::f64::consts::E).abs() < 1e-3);
    assert_eq!((released[2 * RATE as usize - 1], envelope.stage()), (0, Stage::Rest));
    // The shortest interval is one sample, and no time is one step.
    envelope.set_interval(0);
    assert_eq!((linear_step(0, 750), linear_step(1_000_000, 0)), (ENVELOPE_ONE, ENVELOPE_ONE));
    assert!((linear_step(1_000_000, 1000) - ENVELOPE_ONE / 1000).abs() <= 1);
}

#[test]
fn an_envelope_scales_a_signal() {
    assert_eq!(apply(LEVEL, ENVELOPE_ONE), LEVEL);
    assert_eq!(apply(-LEVEL, ENVELOPE_ONE / 2), -LEVEL / 2);
    assert_eq!(apply(LEVEL, 0), 0);
}
