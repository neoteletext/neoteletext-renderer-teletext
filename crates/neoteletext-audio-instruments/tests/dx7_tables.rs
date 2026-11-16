//! The DX7's constant tables: each generated table equals its formula,
//! recomputed here in floating point, and each table copied from
//! music-synthesizer-for-android equals the values of its source, written
//! out again here as fetched from commit
//! f67d41d313b7dc85f6fb99e79e515cc9d208cfff (REQ-6412 item 3).
#![cfg(feature = "dx7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    clippy::float_cmp,
    reason = "a test"
)]
#![allow(clippy::unreadable_literal, reason = "the original's tables are written here as the original writes them")]

use core::f64::consts::PI;

use neoteletext_audio_instruments::dx7::kernel::{self, CARRIER_COMPENSATION_EIGHTHS};
use neoteletext_audio_instruments::dx7::msfa::dx7note::{COARSEMUL, EXP_SCALE_DATA, PITCHMODSENSTAB, VELOCITY_DATA};
use neoteletext_audio_instruments::dx7::msfa::env::{self, LEVEL_LUT};
use neoteletext_audio_instruments::dx7::msfa::pitchenv::{self, PITCHTAB, RATETAB};
use neoteletext_audio_instruments::dx7::msfa::tables::{FINE_LOGFREQ, FREQUENCY};
use neoteletext_audio_instruments::dx7::msfa::{self, lfo};
use neoteletext_audio_instruments::dx7::tables::{EXPONENT, LOG_SINE, VOLUME_ATTENUATION};

/// Shirriff, part III: `-log2(sin((n + .5)/1024 × π/2))`, "rounded to an
/// integer by computing int(y×1024 + .5002)".
#[test]
fn log_sine_equals_its_formula() {
    for (n, &value) in LOG_SINE.iter().enumerate() {
        let angle = (n as f64 + 0.5) / 1024.0 * PI / 2.0;
        let expected = (-angle.sin().log2() * 1024.0 + 0.5002) as u16;
        assert_eq!(value, expected, "entry {n}");
    }
    // The rounding constant matters for one entry alone, which plain
    // rounding would make one less.
    let plain: Vec<usize> = (0..1024)
        .filter(|&n| {
            let angle = (n as f64 + 0.5) / 1024.0 * PI / 2.0;
            (-angle.sin().log2() * 1024.0).round() as u16 != LOG_SINE[n]
        })
        .collect();
    assert_eq!(plain, [233]);
}

/// Shirriff, part II, note 2: `round(2^frac × 2048)`.
#[test]
fn exponent_equals_its_formula() {
    for (n, &value) in EXPONENT.iter().enumerate() {
        let expected = (2.0_f64.powf(n as f64 / 1024.0) * 2048.0).round() as u16;
        assert_eq!(value, expected, "entry {n}");
    }
    assert_eq!(EXPONENT[0], 2048);
    assert_eq!(EXPONENT[1023], 4093);
}

#[test]
fn volume_equals_its_formula() {
    assert_eq!(VOLUME_ATTENUATION[0], 16 * 1024);
    assert_eq!(VOLUME_ATTENUATION[127], 0);
    for (value, &attenuation) in VOLUME_ATTENUATION.iter().enumerate().skip(1) {
        let expected = (2.0 * (127.0 / value as f64).log2() * 1024.0).round().min(16.0 * 1024.0) as u16;
        assert_eq!(attenuation, expected, "volume {value}");
    }
}

/// The sine the two tables make together is a sine: within 8 of 16,372
/// times the true sine of the middle of each of its 4,096 steps (the
/// rounding of the two tables, the larger being the log-sine's half of
/// 1/1024 of a doubling, which is 5.5 at the peak). Its peak
/// is 16,372 and not 16,384 because the exponent takes the complement of
/// the attenuation, so no attenuation is `2^(1023/1024)` of full scale.
#[test]
fn the_tables_make_a_sine() {
    let mut worst = 0.0_f64;
    for step in 0..4096_u32 {
        let value = kernel::sine(step << 20);
        let angle = (f64::from(step) + 0.5) / 4096.0 * 2.0 * PI;
        worst = worst.max((f64::from(value) - 16372.0 * angle.sin()).abs());
    }
    assert!(worst < 8.0, "worst error {worst}");
    assert_eq!(kernel::sine(0x4000_0000 - 1), 16372);
    assert_eq!(kernel::sine(0xc000_0000), -16372);
}

/// An attenuation of one doubling halves a sample, and 14 doublings
/// silence it.
#[test]
fn attenuation_is_a_logarithm() {
    let peak = 0x4000_0000 - 1;
    assert_eq!(kernel::operator_sample(peak, kernel::ATTENUATION_ONE), 8186);
    assert_eq!(kernel::operator_sample(peak, 2 * kernel::ATTENUATION_ONE), 4093);
    assert_eq!(kernel::operator_sample(peak, kernel::ATTENUATION_SILENT - 1), 1);
    for phase in (0..=u32::MAX).step_by(1 << 20) {
        assert_eq!(kernel::operator_sample(phase, kernel::ATTENUATION_SILENT), 0);
        assert_eq!(kernel::operator_sample(phase, kernel::ATTENUATION_MAX), 0);
        assert_eq!(kernel::operator_sample(phase, u32::MAX), 0);
    }
}

/// Shirriff, part IV, note 8: the logarithms of 2 to 6 in the chip's small
/// ROM are binary 01.000, 01.101, 10.000, 10.011 and 10.101.
#[test]
fn carrier_compensation_is_the_chips() {
    assert_eq!(CARRIER_COMPENSATION_EIGHTHS, [0, 0, 8, 13, 16, 19, 21]);
    for (count, &eighths) in CARRIER_COMPENSATION_EIGHTHS.iter().enumerate().skip(1) {
        let exact = (count as f64).log2() * 8.0;
        assert!((f64::from(eighths) - exact).abs() <= 0.5, "{count} carriers");
    }
}

/// `freqlut.cc`, `Freqlut::init`: `y = (1LL << (24 + MAX_LOGFREQ_INT)) /
/// sample_rate`, each entry `floor(y + 0.5)`, `y *= pow(2, 1.0 / N_SAMPLES)`.
/// The original's running product drifts from the closed form by rounding
/// alone; an entry may differ by one.
#[test]
fn frequency_table_equals_freqlut_init_at_48000() {
    let mut y = (1_i64 << 44) as f64 / 48000.0;
    let inc = 2.0_f64.powf(1.0 / 1024.0);
    assert_eq!(FREQUENCY.len(), 1025);
    for (i, &value) in FREQUENCY.iter().enumerate() {
        let closed = ((1_i64 << 44) as f64 / 48000.0 * 2.0_f64.powf(i as f64 / 1024.0) + 0.5).floor() as u32;
        assert_eq!(value, closed, "entry {i}");
        let original = (y + 0.5).floor() as i64;
        assert!((i64::from(value) - original).abs() <= 1, "entry {i}: {value} against {original}");
        y *= inc;
    }
}

/// `dx7note.cc`, `osc_freq`: `floor(24204406.323123 * log(1 + 0.01 * fine) +
/// 0.5)`.
#[test]
fn fine_table_equals_osc_freq() {
    for (fine, &value) in FINE_LOGFREQ.iter().enumerate() {
        let expected = (24_204_406.323_123 * (1.0 + 0.01 * fine as f64).ln() + 0.5).floor() as i32;
        assert_eq!(value, expected, "fine {fine}");
    }
    assert_eq!(FINE_LOGFREQ[0], 0);
}

/// `dx7note.cc`, `coarsemul`.
#[test]
fn coarsemul_is_the_originals() {
    #[rustfmt::skip]
    let expected: [i32; 32] = [
        -16777216, 0, 16777216, 26591258, 33554432, 38955489, 43368474, 47099600,
        50331648, 53182516, 55732705, 58039632, 60145690, 62083076, 63876816,
        65546747, 67108864, 68576247, 69959732, 71268397, 72509921, 73690858,
        74816848, 75892776, 76922906, 77910978, 78860292, 79773775, 80654032,
        81503396, 82323963, 83117622
    ];
    assert_eq!(COARSEMUL, expected);
    // And each is the logarithm it stands for: of a half, then of 1 to 31.
    for (coarse, &value) in COARSEMUL.iter().enumerate() {
        let ratio = if coarse == 0 { 0.5 } else { coarse as f64 };
        assert!((f64::from(value) - ratio.log2() * 16_777_216.0).abs() <= 1.0, "coarse {coarse}");
    }
}

/// `dx7note.cc`, `velocity_data`.
#[test]
fn velocity_data_is_the_originals() {
    #[rustfmt::skip]
    let expected: [u8; 64] = [
        0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163,
        166, 170, 173, 174, 178, 181, 184, 186, 189, 190, 194, 196, 198, 200, 202,
        205, 206, 209, 211, 214, 216, 218, 220, 222, 224, 225, 227, 229, 230, 232,
        233, 235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248, 249, 250, 251,
        252, 253, 254
    ];
    assert_eq!(VELOCITY_DATA, expected);
}

/// `dx7note.cc`, `exp_scale_data` and `pitchmodsenstab`.
#[test]
fn scaling_tables_are_the_originals() {
    #[rustfmt::skip]
    let expected: [u8; 33] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66,
        80, 94, 110, 126, 142, 158, 174, 190, 206, 222, 238, 250
    ];
    assert_eq!(EXP_SCALE_DATA, expected);
    assert_eq!(PITCHMODSENSTAB, [0, 10, 20, 33, 55, 92, 153, 255]);
}

/// `env.cc`, `levellut`.
#[test]
fn levellut_is_the_originals() {
    assert_eq!(LEVEL_LUT, [0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46]);
    assert_eq!(env::Env::scaleoutlevel(19), 46);
    assert_eq!(env::Env::scaleoutlevel(20), 48);
    assert_eq!(env::Env::scaleoutlevel(99), 127);
}

/// `pitchenv.cc`, `ratetab` and `pitchtab`.
#[test]
fn pitch_envelope_tables_are_the_originals() {
    #[rustfmt::skip]
    let ratetab: [u8; 100] = [
        1, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
        12, 13, 13, 14, 14, 15, 16, 16, 17, 18, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 30, 31, 33, 34, 36, 37, 38, 39, 41, 42, 44, 46, 47,
        49, 51, 53, 54, 56, 58, 60, 62, 64, 66, 68, 70, 72, 74, 76, 79, 82,
        85, 88, 91, 94, 98, 102, 106, 110, 115, 120, 125, 130, 135, 141, 147,
        153, 159, 165, 171, 178, 185, 193, 202, 211, 232, 243, 254, 255
    ];
    #[rustfmt::skip]
    let pitchtab: [i8; 100] = [
        -128, -116, -104, -95, -85, -76, -68, -61, -56, -52, -49, -46, -43,
        -41, -39, -37, -35, -33, -32, -31, -30, -29, -28, -27, -26, -25, -24,
        -23, -22, -21, -20, -19, -18, -17, -16, -15, -14, -13, -12, -11, -10,
        -9, -8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
        28, 29, 30, 31, 32, 33, 34, 35, 38, 40, 43, 46, 49, 53, 58, 65, 73,
        82, 92, 103, 115, 127
    ];
    assert_eq!(RATETAB, ratetab);
    assert_eq!(PITCHTAB, pitchtab);
}

/// The units the original computes in floating point when it starts, for
/// this crate's block and sample rate: `lfo.cc`, `Lfo::init`, `N * 25190424
/// / sample_rate + 0.5`; `pitchenv.cc`, `PitchEnv::init`, `N * (1 << 24) /
/// (21.3 * sample_rate) + 0.5`.
#[test]
fn units_equal_the_originals_formulas() {
    let n = f64::from(msfa::N);
    let rate = f64::from(msfa::SAMPLE_RATE);
    assert_eq!(msfa::N, 32);
    assert_eq!(msfa::SAMPLE_RATE, 48_000);
    assert_eq!(lfo::UNIT, (n * 25_190_424.0 / rate + 0.5) as u32);
    assert_eq!(pitchenv::UNIT, (n * f64::from(1 << 24) / (21.3 * rate) + 0.5) as i32);
}
