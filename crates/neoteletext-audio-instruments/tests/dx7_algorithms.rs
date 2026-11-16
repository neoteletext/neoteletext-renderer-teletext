//! The DX7's 32 algorithms: the routing table against two other accounts
//! of it, and what each algorithm does to rendered sound (REQ-6412 item 6).
#![cfg(feature = "dx7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "a test"
)]

use neoteletext_audio_instruments::dx7::kernel::{
    ALGORITHMS, ATTENUATION_MAX, Algorithm, MODULATORS_MAX, Voice, carrier_compensation,
};
use neoteletext_audio_instruments::dx7::{Bank, Patch, Rack, VoiceParams};

/// music-synthesizer-for-android `fm_core.cc`, `algorithms`, as fetched
/// from commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff. Each row is
/// operators 6 to 1; in each byte bits 0 and 1 are the bus written (0 the
/// output), bit 2 adds to the bus, bits 4 and 5 are the bus read, bit 6
/// reads the feedback and bit 7 writes it.
#[rustfmt::skip]
const ORIGINAL: [[u8; 6]; 32] = [
    [0xc1, 0x11, 0x11, 0x14, 0x01, 0x14], // 1
    [0x01, 0x11, 0x11, 0x14, 0xc1, 0x14], // 2
    [0xc1, 0x11, 0x14, 0x01, 0x11, 0x14], // 3
    [0x41, 0x11, 0x94, 0x01, 0x11, 0x14], // 4
    [0xc1, 0x14, 0x01, 0x14, 0x01, 0x14], // 5
    [0x41, 0x94, 0x01, 0x14, 0x01, 0x14], // 6
    [0xc1, 0x11, 0x05, 0x14, 0x01, 0x14], // 7
    [0x01, 0x11, 0xc5, 0x14, 0x01, 0x14], // 8
    [0x01, 0x11, 0x05, 0x14, 0xc1, 0x14], // 9
    [0x01, 0x05, 0x14, 0xc1, 0x11, 0x14], // 10
    [0xc1, 0x05, 0x14, 0x01, 0x11, 0x14], // 11
    [0x01, 0x05, 0x05, 0x14, 0xc1, 0x14], // 12
    [0xc1, 0x05, 0x05, 0x14, 0x01, 0x14], // 13
    [0xc1, 0x05, 0x11, 0x14, 0x01, 0x14], // 14
    [0x01, 0x05, 0x11, 0x14, 0xc1, 0x14], // 15
    [0xc1, 0x11, 0x02, 0x25, 0x05, 0x14], // 16
    [0x01, 0x11, 0x02, 0x25, 0xc5, 0x14], // 17
    [0x01, 0x11, 0x11, 0xc5, 0x05, 0x14], // 18
    [0xc1, 0x14, 0x14, 0x01, 0x11, 0x14], // 19
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x14], // 20
    [0x01, 0x14, 0x14, 0xc1, 0x14, 0x14], // 21
    [0xc1, 0x14, 0x14, 0x14, 0x01, 0x14], // 22
    [0xc1, 0x14, 0x14, 0x01, 0x14, 0x04], // 23
    [0xc1, 0x14, 0x14, 0x14, 0x04, 0x04], // 24
    [0xc1, 0x14, 0x14, 0x04, 0x04, 0x04], // 25
    [0xc1, 0x05, 0x14, 0x01, 0x14, 0x04], // 26
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x04], // 27
    [0x04, 0xc1, 0x11, 0x14, 0x01, 0x14], // 28
    [0xc1, 0x14, 0x01, 0x14, 0x04, 0x04], // 29
    [0x04, 0xc1, 0x11, 0x14, 0x04, 0x04], // 30
    [0xc1, 0x14, 0x04, 0x04, 0x04, 0x04], // 31
    [0xc4, 0x04, 0x04, 0x04, 0x04, 0x04], // 32
];

/// The number of outputs of each algorithm in the sound chip's algorithm
/// ROM: the largest COM value of its six entries, plus one (Shirriff,
/// "Yamaha DX7 chip reverse-engineering, part IV", note 5).
const CHIP_OUTPUTS: [u32; 32] =
    [2, 2, 2, 2, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 3, 3, 4, 4, 4, 5, 5, 3, 3, 3, 4, 4, 5, 6];

/// What a row of [`ORIGINAL`] says, in this crate's form: the original
/// computes operators 6 to 1, each reading a bus and writing one.
fn from_original(row: [u8; 6]) -> Algorithm {
    let mut buses = [0_u8; 3];
    let mut algorithm = Algorithm { modulators: [0; 6], carriers: 0, feedback_from: 0, feedback_to: 0 };
    for (position, flags) in row.into_iter().enumerate() {
        let index = 5 - position;
        let bit = 1_u8 << index;
        let (input, output, add) = (usize::from((flags >> 4) & 3), usize::from(flags & 3), flags & 4 != 0);
        if input != 0 {
            algorithm.modulators[index] = buses[input];
        }
        if flags & 0x40 != 0 {
            algorithm.feedback_to = index as u8;
        }
        if flags & 0x80 != 0 {
            algorithm.feedback_from = index as u8;
        }
        if output == 0 {
            algorithm.carriers |= bit;
        } else if add {
            buses[output] |= bit;
        } else {
            buses[output] = bit;
        }
    }
    algorithm
}

#[test]
fn the_table_is_the_originals_routing() {
    for (number, (algorithm, row)) in ALGORITHMS.iter().zip(ORIGINAL).enumerate() {
        assert_eq!(*algorithm, from_original(row), "algorithm {}", number + 1);
    }
}

#[test]
fn carrier_counts_are_the_chips() {
    for (number, (algorithm, outputs)) in ALGORITHMS.iter().zip(CHIP_OUTPUTS).enumerate() {
        assert_eq!(algorithm.carrier_count(), outputs, "algorithm {}", number + 1);
    }
}

/// What the algorithm chart shows of every algorithm: operator 1 is a
/// carrier; an operator is modulated only by operators above it, three at
/// most; every
/// operator reaches a carrier; one operator feeds back, to itself in all
/// but algorithms 4 and 6, where the loop is three and two operators long.
#[test]
fn the_table_is_well_formed() {
    for (number, algorithm) in ALGORITHMS.iter().enumerate() {
        let number = number + 1;
        assert!(algorithm.is_carrier(0), "algorithm {number}");
        assert!(algorithm.modulators.iter().all(|mask| mask.count_ones() as usize <= MODULATORS_MAX));
        let mut reaches = algorithm.carriers;
        for index in 0..6 {
            assert_eq!(algorithm.modulators[index] & ((2 << index) - 1), 0, "algorithm {number}");
            // Lower operators first: what they hear reaches a carrier too.
            if reaches & (1 << index) != 0 {
                reaches |= algorithm.modulators[index];
            }
        }
        assert_eq!(reaches, 0b11_1111, "algorithm {number}");
        if matches!(number, 12 | 13 | 16 | 17 | 18) {
            let most = algorithm.modulators.iter().map(|mask| mask.count_ones() as usize).max();
            assert_eq!(most, Some(MODULATORS_MAX), "algorithm {number}");
        }
        match number {
            4 => assert_eq!((algorithm.feedback_from, algorithm.feedback_to), (3, 5)),
            6 => assert_eq!((algorithm.feedback_from, algorithm.feedback_to), (4, 5)),
            _ => assert_eq!(algorithm.feedback_from, algorithm.feedback_to, "algorithm {number}"),
        }
    }
}

/// A voice of six operators at full level and a ratio of 1.00, in
/// `algorithm` (0 to 31), changed by `change`.
fn rack_with(algorithm: u8, change: impl FnOnce(&mut VoiceParams)) -> Rack {
    let mut params = *Patch::DEFAULT.params();
    params.algorithm = algorithm;
    for operator in &mut params.operators {
        operator.output_level = 99;
    }
    change(&mut params);
    let mut bank = Bank::new();
    bank.set(0, Patch::new(params).unwrap());
    let mut rack = Rack::new();
    rack.set_bank(&bank);
    rack
}

/// A tenth of a second of middle A.
fn render(rack: &mut Rack) -> Vec<i16> {
    rack.event(0x90, 69, 100);
    let mut samples = vec![0_i16; 4800];
    rack.render(&mut samples);
    samples
}

fn peak(samples: &[i16]) -> i32 {
    samples.iter().map(|&sample| i32::from(sample).abs()).max().unwrap()
}

/// With one operator switched on at a time, the sound is silent exactly
/// when that operator is not a carrier.
#[test]
fn one_operator_alone_sounds_only_if_it_is_a_carrier() {
    for (number, algorithm) in ALGORITHMS.iter().enumerate() {
        let mut carriers = 0;
        for index in 0..6 {
            let samples = render(&mut rack_with(number as u8, |voice| voice.operators_on = 1 << index));
            let sounds = peak(&samples) > 0;
            assert_eq!(sounds, algorithm.is_carrier(index), "algorithm {}, operator {}", number + 1, index + 1);
            carriers += u32::from(sounds);
        }
        assert_eq!(carriers, CHIP_OUTPUTS[number], "algorithm {}", number + 1);
    }
}

/// Switching any one operator off changes the sound of every algorithm:
/// each operator is a carrier or modulates, however far up, one.
#[test]
fn every_operator_is_heard() {
    for number in 0..32_u8 {
        let all = render(&mut rack_with(number, |_| ()));
        for index in 0..6 {
            let without = render(&mut rack_with(number, |voice| voice.operators_on = 0b11_1111 ^ (1 << index)));
            assert_ne!(all, without, "algorithm {}, operator {}", number + 1, index + 1);
        }
    }
}

/// An operator changes what a carrier sounds exactly when the table says
/// it modulates it, directly or through others: with one carrier and one
/// other operator switched on, the carrier is a plain sine unless the
/// other reaches it.
#[test]
fn modulation_follows_the_table() {
    for (number, algorithm) in ALGORITHMS.iter().enumerate() {
        for carrier in (0..6).filter(|&index| algorithm.is_carrier(index)) {
            let alone = render(&mut rack_with(number as u8, |voice| voice.operators_on = 1 << carrier));
            for other in (0..6).filter(|&index| !algorithm.is_carrier(index)) {
                // Whether `other` reaches `carrier` with every operator
                // between them switched off: only if it modulates it itself.
                let direct = algorithm.modulators[carrier] & (1 << other) != 0;
                let both =
                    render(&mut rack_with(number as u8, |voice| voice.operators_on = (1 << carrier) | (1 << other)));
                assert_eq!(
                    both != alone,
                    direct,
                    "algorithm {}: operator {} on operator {}",
                    number + 1,
                    other + 1,
                    carrier + 1
                );
            }
        }
    }
}

/// Feedback changes the sound, more at each setting, in every algorithm,
/// and in algorithms 4 and 6 only while every operator of the loop is on.
#[test]
fn feedback_follows_the_table() {
    for number in 0..32_u8 {
        let none = render(&mut rack_with(number, |_| ()));
        let some = render(&mut rack_with(number, |voice| voice.feedback = 1));
        let full = render(&mut rack_with(number, |voice| voice.feedback = 7));
        assert_ne!(none, some, "algorithm {}", number + 1);
        assert_ne!(some, full, "algorithm {}", number + 1);
    }
    // Algorithm 4: operator 6 hears operator 4, which hears 5, which hears
    // 6. Algorithm 6: operator 6 hears operator 5, which hears 6.
    for (number, in_loop) in [(3_u8, [3_u8, 4, 5].as_slice()), (5, [4, 5].as_slice())] {
        let on: u8 = in_loop.iter().map(|&index| 1 << index).sum();
        let closed = |feedback: u8, on: u8| {
            render(&mut rack_with(number, |voice| {
                voice.feedback = feedback;
                voice.operators_on = on;
            }))
        };
        assert_ne!(closed(7, on), closed(0, on), "algorithm {}", number + 1);
        // The loop is cut by switching off the operator the feedback goes
        // to: what is left sounds the same at any feedback.
        let cut = on & !(1 << 5);
        assert_eq!(closed(7, cut), closed(0, cut), "algorithm {}", number + 1);
    }
}

/// A feedback loop through more than one operator is computed sample by
/// sample: in algorithm 6, operator 6 is modulated by the mean of operator
/// 5's last two samples, so the first sample operator 5 gives after its own
/// first two is already changed by them.
#[test]
fn a_long_feedback_loop_is_closed_within_two_samples() {
    let algorithm = &ALGORITHMS[5];
    let run = |feedback: u8| {
        let mut voice = Voice::new();
        // Operators 5 and 6 at no attenuation, about a fourteenth of a turn a
        // sample.
        for index in 0..6 {
            voice.set_operator(index, 0x1234_5678, if index >= 4 { 0 } else { ATTENUATION_MAX });
        }
        // Twice, so that the glide from silence is over.
        voice.begin_block();
        voice.set_operator(4, 0x1234_5678, 0);
        voice.set_operator(5, 0x1234_5678, 0);
        voice.begin_block();
        let mut samples = [0_i32; 8];
        voice.render(algorithm, feedback, &mut samples);
        samples
    };
    let (open, closed) = (run(0), run(7));
    // Sample 0: nothing has been fed back yet.
    assert_eq!(open[0], closed[0]);
    // Sample 1: operator 6 hears operator 5's sample 0, and operator 5 hears
    // operator 6 at once.
    assert_ne!(open[1], closed[1]);
}

/// An algorithm of several carriers is about as loud as one of one: with
/// every modulator at level 0 and the carriers in phase, the peak of each
/// of the 32 is within 5% of a single carrier's, and never above it by more
/// than rounding.
#[test]
fn carriers_are_compensated_for_their_number() {
    let single = peak(&render(&mut rack_with(15, |voice| {
        for operator in &mut voice.operators[1..] {
            operator.output_level = 0;
        }
    })));
    assert!((2040..=2047).contains(&single), "single carrier peaks at {single}");
    for (number, algorithm) in ALGORITHMS.iter().enumerate() {
        let samples = render(&mut rack_with(number as u8, |voice| {
            for (index, operator) in voice.operators.iter_mut().enumerate() {
                if !algorithm.is_carrier(index) {
                    operator.output_level = 0;
                }
            }
        }));
        let ratio = f64::from(peak(&samples)) / f64::from(single);
        println!(
            "algorithm {:2}: {} carriers, peak {:.3} of one carrier",
            number + 1,
            algorithm.carrier_count(),
            ratio
        );
        assert!((0.95..=1.005).contains(&ratio), "algorithm {}: {ratio}", number + 1);
    }
    // Six carriers without the compensation would be six times one.
    let six = &ALGORITHMS[31];
    assert_eq!(carrier_compensation(six), 21 << 21);
    assert_eq!(carrier_compensation(&ALGORITHMS[15]), 0);
}
