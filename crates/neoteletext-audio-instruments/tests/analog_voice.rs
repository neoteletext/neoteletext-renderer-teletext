//! The analogue building blocks together: voices of two oscillators, a
//! four-pole filter that distorts and an envelope, at twice the output
//! rate, through the decimator and the chorus; and what that costs
//! (TODO-6430).

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

#[path = "analog_support.rs"]
mod support;

use neoteletext_audio_instruments::analog::allocator::{Allocator, Assign, Mode, Stage as VoiceStage};
use neoteletext_audio_instruments::analog::chorus::Chorus;
use neoteletext_audio_instruments::analog::decimator::Decimator;
use neoteletext_audio_instruments::analog::drift::{CENTS_BITS, Variation, detune};
use neoteletext_audio_instruments::analog::envelope::{RcEnvelope, Stage, apply, time_constant};
use neoteletext_audio_instruments::analog::filter::{Ladder, ONE_Q28, OtaCascade};
use neoteletext_audio_instruments::analog::oscillator::{Oscillator, waveform};
use neoteletext_audio_instruments::analog::{Oversampling, frequency, to_sample};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, SAMPLE_RATE};
use support::{decibels, spectrum};

/// Voices of the instrument made here.
const VOICES: usize = 8;
/// The rate the voices run at.
const OVERSAMPLING: Oversampling = Oversampling::Twice;
/// Samples of the voices between two settings of their filters.
const CONTROL_INTERVAL: usize = 32;

/// Which four-pole filter a voice has.
#[derive(Clone, Copy)]
enum Kind {
    Ladder,
    Cascade,
}

/// One voice: a sawtooth and a pulse a few cents apart, into a four-pole
/// filter whose cutoff its envelope sweeps, into the same envelope as
/// its level.
struct Voice {
    oscillators: [Oscillator; 2],
    ladder: Ladder,
    cascade: OtaCascade,
    envelope: RcEnvelope,
    kind: Kind,
    triggers: u32,
}

impl Voice {
    fn new(kind: Kind) -> Self {
        let rate = OVERSAMPLING.rate();
        let mut envelope = RcEnvelope::new();
        envelope.set_attack(time_constant(5_000, rate));
        envelope.set_decay(time_constant(300_000, rate));
        envelope.set_release(time_constant(30_000, rate));
        envelope.set_sustain(1 << 29);
        let mut oscillators = [Oscillator::new(), Oscillator::new()];
        oscillators[0].enable(waveform::SAWTOOTH);
        oscillators[1].enable(waveform::PULSE);
        Self { oscillators, ladder: Ladder::new(), cascade: OtaCascade::new(), envelope, kind, triggers: 0 }
    }

    fn start(&mut self, index: u32, key: u8) {
        let rate = OVERSAMPLING.rate();
        let hz = 440.0 * ((f64::from(key) - 69.0) / 12.0).exp2();
        let step = frequency((hz * 1000.0) as u32, rate);
        // The second oscillator's tuning is off by up to five cents,
        // fixed for the voice.
        let offset = Variation::new(1, index).offset(5 << CENTS_BITS);
        self.oscillators[0].set_step(step);
        self.oscillators[1].set_step(detune(step, offset));
        self.envelope.retrigger();
    }

    /// The filter's cutoff for the envelope's level: 200 Hz to 8.2 kHz.
    fn control(&mut self) {
        let rate = OVERSAMPLING.rate();
        let level = self.envelope.level() >> 15;
        let cutoff = frequency(200_000, rate) + (frequency(8_000_000, rate) >> 15) * level.cast_unsigned();
        match self.kind {
            Kind::Ladder => self.ladder.set(cutoff, 2 * ONE_Q28),
            Kind::Cascade => self.cascade.set(cutoff, 2 * ONE_Q28),
        }
    }

    #[inline]
    fn tick(&mut self) -> i32 {
        let [first, second] = &mut self.oscillators;
        let mixed = first.tick(None).sawtooth / 2 + second.tick(None).pulse / 2;
        let filtered = match self.kind {
            Kind::Ladder => self.ladder.tick(mixed),
            Kind::Cascade => self.cascade.tick(mixed).four_pole,
        };
        apply(filtered, self.envelope.tick())
    }
}

/// An instrument of eight such voices, their sum brought down to the
/// output rate, through the chorus, to 16-bit samples.
struct Instrument {
    allocator: Allocator<VOICES>,
    voices: Vec<Voice>,
    decimator: Decimator,
    chorus: Option<Chorus>,
}

impl Instrument {
    fn new(kind: Kind, chorus: bool) -> Self {
        let chorus = chorus.then(|| {
            let mut chorus = Chorus::new(SAMPLE_RATE);
            chorus.set_delay(3_500, 1_850);
            chorus.set_rate(frequency(500, SAMPLE_RATE));
            chorus
        });
        Self {
            allocator: Allocator::new(Mode::Poly(Assign::Cyclic)),
            voices: (0..VOICES).map(|_| Voice::new(kind)).collect(),
            decimator: Decimator::sharp(),
            chorus,
        }
    }

    /// Carries the allocator's gates and triggers to the voices.
    fn follow(&mut self) {
        for (index, (voice, state)) in self.voices.iter_mut().zip(*self.allocator.voices()).enumerate() {
            if state.triggers != voice.triggers {
                voice.triggers = state.triggers;
                voice.start(index as u32, state.key);
            }
            voice.envelope.gate(state.gate());
            if state.stage == VoiceStage::Released && voice.envelope.stage() == Stage::Rest {
                self.allocator.finished(index);
            }
        }
    }

    fn render(&mut self, out: &mut [i16]) {
        self.follow();
        for (index, sample) in out.iter_mut().enumerate() {
            if (2 * index).is_multiple_of(CONTROL_INTERVAL) {
                for voice in &mut self.voices {
                    voice.control();
                }
            }
            let mut pair = [0i32; 2];
            for sum in &mut pair {
                for voice in &mut self.voices {
                    *sum += voice.tick() / 4;
                }
            }
            let mono = self.decimator.process(pair[0], pair[1]);
            let mixed = match &mut self.chorus {
                Some(chorus) => {
                    let [left, right] = chorus.process(mono);
                    mono / 2 + left / 4 + right / 4
                }
                None => mono,
            };
            *sample = to_sample(mixed);
        }
    }
}

#[test]
fn eight_voices_make_a_chord_that_is_bounded_clean_and_the_same_each_time() {
    let run = |kind: Kind| -> Vec<i16> {
        let mut instrument = Instrument::new(kind, true);
        // 0.8 s: the keys go up at 0.48 s, and a release of 30 ms is at
        // rest ten time constants later.
        let mut out = vec![0i16; 40 * BLOCK_SAMPLES];
        for (index, block) in out.chunks_mut(BLOCK_SAMPLES).enumerate() {
            match index {
                0 => {
                    for key in [45, 57, 60, 64, 67, 72, 76, 79] {
                        instrument.allocator.key_down(key, 100);
                    }
                }
                24 => instrument.allocator.all_keys_up(),
                _ => {}
            }
            instrument.render(block);
        }
        assert_eq!(instrument.allocator.sounding(), 0);
        out
    };
    for kind in [Kind::Ladder, Kind::Cascade] {
        let out = run(kind);
        if matches!(kind, Kind::Ladder) {
            assert_eq!(out, run(kind));
        }
        let peak = out.iter().map(|&sample| i32::from(sample).abs()).max().unwrap_or(0);
        assert!(peak > 4_000 && peak < 32_767, "{peak}");
        // Released, every voice comes to rest and the output with it.
        assert!(out[out.len() - 480..].iter().all(|&sample| sample.abs() <= 2), "{:?}", &out[out.len() - 4..]);
        // The chord is there: A2 at 110 Hz is the strongest thing below
        // 150 Hz, and the octave above it, in the chord too, is there.
        let held: Vec<f64> = out[2_400..2_400 + (1 << 14)].iter().map(|&sample| f64::from(sample)).collect();
        let magnitudes = spectrum(&held);
        let bin = |hz: f64| (hz * 16_384.0 / 48_000.0).round() as usize;
        let strongest = (0..bin(150.0)).max_by(|&a, &b| magnitudes[a].total_cmp(&magnitudes[b])).unwrap_or(0);
        assert!(strongest.abs_diff(bin(110.0)) <= 1, "{strongest}");
        assert!(decibels(magnitudes[bin(220.0)] / magnitudes[strongest]) > -20.0);
    }
}

/// How many seconds of audio eight voices render in a second, in a
/// release build: `cargo test --release -p neoteletext-audio-instruments
/// --features analog --test analog_voice -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, which needs a release build and an idle machine to mean anything"]
fn rendering_speed() {
    for (name, kind, chorus) in [
        ("transistor ladder", Kind::Ladder, false),
        ("amplifier cascade", Kind::Cascade, false),
        ("amplifier cascade and chorus", Kind::Cascade, true),
    ] {
        let mut instrument = Instrument::new(kind, chorus);
        for key in [45, 57, 60, 64, 67, 72, 76, 79] {
            instrument.allocator.key_down(key, 100);
        }
        let mut block = [0i16; BLOCK_SAMPLES];
        let seconds = 20usize;
        let blocks = seconds * SAMPLE_RATE as usize / BLOCK_SAMPLES;
        let started = std::time::Instant::now();
        let mut slowest = std::time::Duration::ZERO;
        let mut sum = 0i64;
        for _ in 0..blocks {
            let block_started = std::time::Instant::now();
            instrument.render(&mut block);
            slowest = slowest.max(block_started.elapsed());
            sum += block.iter().map(|&sample| i64::from(sample)).sum::<i64>();
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "{VOICES} voices, two oscillators and the {name} at {} Hz: {seconds} s of audio in {elapsed:.3} s: {:.1} times real time; slowest 20 ms block {:.3} ms (checksum {sum})",
            OVERSAMPLING.rate(),
            seconds as f64 / elapsed,
            slowest.as_secs_f64() * 1000.0
        );
    }
}
