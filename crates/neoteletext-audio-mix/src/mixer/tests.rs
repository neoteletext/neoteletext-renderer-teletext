#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses indexing into data it built to a known size and `unwrap` on results it expects to be good"
)]

// Tests of REQ-AUD-011, REQ-AUD-014, REQ-AUD-015 and REQ-AUD-019.
use super::*;
use crate::sinc::TAPS_BEFORE;
use std::vec;
use std::vec::Vec;

const RATE: u32 = 8_000;

fn start(key: SoundKey, gain: u8) -> Start {
    Start { key, gain, fade_ms: 5, looped: false, duck_percent: 100, after_current: false }
}

fn constant(mixer: &mut Mixer, key: SoundKey, value: i16, samples: usize) {
    mixer.define(key, RATE, samples);
    mixer.samples(key, 0, &vec![value; samples]);
}

fn render(mixer: &mut Mixer, samples: usize) -> Vec<i16> {
    let mut out = vec![0; samples];
    mixer.render(&mut out);
    out
}

// Verifies REQ-AUD-015.
#[test]
fn a_sound_ramps_in_over_its_fade_and_plays_at_its_gain() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 10_000, 4000);
    mixer.start(0, start((1, 1), 50));
    let out = render(&mut mixer, 400);
    // 5 ms at 8 kHz is 40 samples of ramp: no step at the start.
    assert!(out[0].abs() < 1000, "starts near silence: {}", out[0]);
    assert!(out[20] > out[5] && out[20] < 5000, "rising");
    // Settled: half of 10 000, give or take the filter's ripple.
    assert!((4_900..=5_100).contains(&out[300]), "{}", out[300]);
}

// Verifies REQ-AUD-011 and REQ-AUD-015.
#[test]
fn a_stop_fades_out_and_ends_the_voice() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 10_000, 8000);
    mixer.start(0, start((1, 1), 100));
    render(&mut mixer, 200);
    mixer.stop(0, 10);
    let out = render(&mut mixer, 200);
    assert!(out[0] > 9_000, "still sounding as the fade begins");
    assert_eq!(out[100], 0, "silent once 10 ms have passed");
    assert!(!mixer.active());
}

// Verifies REQ-AUD-019.
#[test]
fn voices_sum_and_the_sum_is_clamped() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 20_000, 2000);
    constant(&mut mixer, (2, 1), 20_000, 2000);
    mixer.start(0, start((1, 1), 100));
    mixer.start(1, start((2, 1), 100));
    assert_eq!(render(&mut mixer, 500)[400], i16::MAX);
}

// Verifies DEC-6950: the unclamped render hands over the voices' sum as
// it is, where the clamped render of the same voices stops at full scale.
#[test]
fn the_unclamped_render_hands_over_the_sum_beyond_the_16_bit_range() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 20_000, 2000);
    constant(&mut mixer, (2, 1), 20_000, 2000);
    mixer.start(0, start((1, 1), 100));
    mixer.start(1, start((2, 1), 100));
    let mut out = vec![0_i32; 500];
    mixer.render_pulling_unclamped(&mut out, &mut pulled::Silence);
    // Two voices of 20 000, give or take the filter's ripple.
    assert!((39_800..=40_200).contains(&out[400]), "{}", out[400]);
    // Within the range, it is the clamped render's sample exactly.
    let mut quiet = Mixer::new(RATE);
    let mut same = Mixer::new(RATE);
    for mixer in [&mut quiet, &mut same] {
        constant(mixer, (1, 1), 10_000, 2000);
        mixer.start(0, start((1, 1), 50));
    }
    let mut wide = vec![0_i32; 500];
    quiet.render_pulling_unclamped(&mut wide, &mut pulled::Silence);
    assert_eq!(wide, render(&mut same, 500).into_iter().map(i32::from).collect::<Vec<_>>());
}

// Verifies REQ-AUD-015 and REQ-AUD-019.
#[test]
fn ducking_lowers_the_other_voices_by_table_19s_step() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 10_000, 8000);
    constant(&mut mixer, (2, 1), 0, 8000);
    mixer.start(0, start((1, 1), 100));
    render(&mut mixer, 200);
    // Voice 1 plays silence but asks the others to duck by 12 dB (25 %).
    mixer.start(1, Start { duck_percent: 25, ..start((2, 1), 100) });
    let out = render(&mut mixer, 400);
    assert!((2_400..=2_600).contains(&out[399]), "{}", out[399]);
}

// Verifies REQ-6925 item 3 and DEC-6924 item 2: a voice started again on
// a live sound whose samples kept coming joins at what is written now, not
// where it stopped, which is how music of an outer level resumes at its
// live point.
#[test]
fn a_voice_started_again_on_a_live_sound_joins_at_its_present_samples() {
    let mut mixer = Mixer::new(RATE);
    mixer.stream((1, 1), RATE);
    mixer.stream_samples((1, 1), &[1_000; 4000]);
    mixer.start(0, start((1, 1), 100));
    let out = render(&mut mixer, 400);
    assert!((990..=1_000).contains(&out[399]), "{}", out[399]);
    mixer.stop(0, 5);
    render(&mut mixer, 100);
    // While it is silent the sound goes on: half a second at another level.
    mixer.stream_samples((1, 1), &[5_000; 4000]);
    mixer.start(0, start((1, 1), 100));
    let out = render(&mut mixer, 200);
    // The first sample heard after the ramp is the present level, with none
    // of the 1,000s left unplayed when it stopped.
    assert!(out[100..].iter().all(|&sample| (4_900..=5_000).contains(&sample)), "{:?}", &out[100..110]);
}

// Verifies REQ-AUD-011.
#[test]
fn a_queued_sound_follows_the_current_one() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 1_000, 100);
    constant(&mut mixer, (2, 1), 8_000, 2000);
    mixer.start(0, start((1, 1), 100));
    mixer.start(0, Start { after_current: true, ..start((2, 1), 100) });
    let out = render(&mut mixer, 600);
    assert!((900..=1_100).contains(&out[60]), "the first sound");
    assert!((7_800..=8_200).contains(&out[500]), "then the queued one: {}", out[500]);
}

// Verifies REQ-AUD-019.
#[test]
fn a_sound_is_resampled_to_the_output_rate() {
    // A 1 kHz tone at 8 kHz played at 48 kHz keeps its pitch: zero
    // crossings every 24 output samples.
    let mut mixer = Mixer::new(48_000);
    let tone: Vec<i16> = (0..8000).map(|n| [0, 7071, 10_000, 7071, 0, -7071, -10_000, -7071][n % 8]).collect();
    mixer.define((1, 1), 8_000, tone.len());
    mixer.samples((1, 1), 0, &tone);
    mixer.start(0, start((1, 1), 100));
    let out = render(&mut mixer, 4800);
    let crossings: Vec<usize> = (2000..4700).filter(|&n| out[n] < 0 && out[n + 1] >= 0).collect();
    let spacings: Vec<usize> = crossings.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(!spacings.is_empty() && spacings.iter().all(|&gap| gap == 48), "{spacings:?}");
    let peak = out[2000..4700].iter().map(|value| value.unsigned_abs()).max().unwrap();
    assert!((9_500..=10_500).contains(&peak), "amplitude kept: {peak}");
}

#[test]
fn mute_silences_and_a_forgotten_sound_waits_for_its_voice() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 10_000, 8000);
    mixer.start(0, start((1, 1), 100));
    mixer.forget((1, 1));
    assert!(render(&mut mixer, 100)[99] > 9_000, "still playing after being forgotten");
    mixer.set_master(100, true);
    assert_eq!(render(&mut mixer, 100)[99], 0);
}

/// Samples voice 0's live sound has buffered ahead of it.
fn buffered(mixer: &Mixer) -> i64 {
    let playing = mixer.voices[0].playing.unwrap();
    let sound = mixer.sounds.iter().find(|sound| sound.key() == playing.key).unwrap();
    sound.ahead(playing.whole)
}

// Verifies REQ-AUD-014.
#[test]
fn a_live_sound_waits_for_its_target_then_plays() {
    let mut mixer = Mixer::new(RATE);
    mixer.stream((1, 1), RATE);
    mixer.start(0, start((1, 1), 100));
    // 400 ms at 8 kHz is 3 200 samples: 3 100 are not enough.
    mixer.stream_samples((1, 1), &vec![10_000; 3_100]);
    assert!(render(&mut mixer, 200).iter().all(|&value| value == 0), "silent until the target is buffered");
    mixer.stream_samples((1, 1), &vec![10_000; 200]);
    let out = render(&mut mixer, 200);
    assert!((9_800..=10_200).contains(&out[150]), "{}", out[150]);
}

#[test]
fn a_live_sound_follows_a_sender_whose_clock_runs_one_per_cent_fast() {
    let mut mixer = Mixer::new(RATE);
    mixer.stream((1, 1), RATE);
    mixer.start(0, start((1, 1), 100));
    // 101 samples arrive for every 100 played; 200 000 samples (25 s)
    // would put 2 000 extra in the buffer were the rate not trimmed.
    for _ in 0..2_000 {
        mixer.stream_samples((1, 1), &[5_000; 101]);
        render(&mut mixer, 100);
    }
    let ahead = buffered(&mixer);
    // Steady state: 1 % of trim is a fifth of the target, 640 samples, over.
    assert!((3_200..=4_000).contains(&ahead), "buffer held near the target: {ahead}");
    // And one running slow drains less than the target's worth.
    for _ in 0..4_000 {
        mixer.stream_samples((1, 1), &[5_000; 99]);
        render(&mut mixer, 100);
    }
    let ahead = buffered(&mixer);
    assert!((2_400..=3_200).contains(&ahead), "buffer held near the target: {ahead}");
}

#[test]
fn a_forgotten_live_sound_plays_what_it_holds_then_ends() {
    let mut mixer = Mixer::new(RATE);
    mixer.stream((1, 1), RATE);
    mixer.start(0, start((1, 1), 100));
    mixer.stream_samples((1, 1), &vec![10_000; 3_600]);
    render(&mut mixer, 1_000);
    mixer.forget((1, 1));
    let out = render(&mut mixer, 3_000);
    assert!(out[2_000] > 9_000, "still playing what it holds");
    assert!(!mixer.active() && out[2_999] == 0, "then ended");
    // Rendering never frees: the next call that may allocate collects it.
    mixer.stream((2, 1), RATE);
    assert_eq!(mixer.sounds.iter().map(Sound::key).collect::<Vec<_>>(), [(2, 1)], "and gone");
}

#[test]
fn a_voice_that_falls_a_ring_behind_skips_to_the_target() {
    let mut mixer = Mixer::new(RATE);
    mixer.stream((1, 1), RATE);
    mixer.start(0, start((1, 1), 100));
    // Two seconds of samples at once: more than the ring keeps ahead.
    mixer.stream_samples((1, 1), &vec![10_000; 16_000]);
    assert_eq!(buffered(&mixer), 3_200);
}

/// A tone of eight samples a period.
fn tone(samples: usize) -> Vec<i16> {
    (0..samples).map(|n| [0, 7071, 10_000, 7071, 0, -7071, -10_000, -7071][n % 8]).collect()
}

#[test]
fn a_sound_defined_with_its_buffer_mixes_as_one_defined_by_length() {
    // Not the output rate, so the voice resamples it.
    const SOURCE_RATE: u32 = 11_025;
    let pcm = tone(4_000);
    let mut by_length = Mixer::new(RATE);
    by_length.define((1, 1), SOURCE_RATE, pcm.len());
    by_length.samples((1, 1), 0, &pcm);
    // A silent buffer filled afterwards, and one given filled.
    let mut silent = Mixer::new(RATE);
    silent.define_with((1, 1), SOURCE_RATE, vec![0; pcm.len()]);
    silent.samples((1, 1), 0, &pcm);
    let mut filled = Mixer::new(RATE);
    filled.define_with((1, 1), SOURCE_RATE, pcm.clone());
    by_length.start(0, start((1, 1), 80));
    let expected = render(&mut by_length, 3_000);
    assert!(expected.iter().any(|&value| value != 0), "the tone sounds");
    for mut mixer in [silent, filled] {
        mixer.start(0, start((1, 1), 80));
        assert_eq!(render(&mut mixer, 3_000), expected);
    }
}

#[test]
fn a_defined_buffer_is_held_as_given() {
    let buffer = vec![0; 1_000];
    let place = buffer.as_ptr();
    let mut mixer = Mixer::new(RATE);
    mixer.define_with((1, 1), RATE, buffer);
    assert!(mixer.sounds.iter().any(|sound| sound.holds(place)), "not copied");
}

/// Two seconds of a tone given a live sound in pieces of 160 samples, each
/// followed by 20 ms of rendering.
fn stream_tone(mixer: &mut Mixer) -> Vec<i16> {
    let pcm = tone(16_000);
    mixer.start(0, start((1, 1), 90));
    pcm.chunks(160)
        .flat_map(|piece| {
            mixer.stream_samples((1, 1), piece);
            render(mixer, 160)
        })
        .collect()
}

#[test]
fn a_live_sound_given_its_ring_mixes_as_one_made_by_rate() {
    let mut by_rate = Mixer::new(RATE);
    by_rate.stream((1, 1), RATE);
    let expected = stream_tone(&mut by_rate);
    assert!(expected.iter().any(|&value| value != 0), "the tone sounds");
    let mut given = Mixer::new(RATE);
    assert_eq!(given.stream_with((1, 1), RATE, vec![0; Mixer::stream_ring(RATE)]), Ok(()));
    assert_eq!(stream_tone(&mut given), expected);
}

#[test]
fn a_ring_of_the_wrong_length_is_handed_back_and_changes_nothing() {
    let mut mixer = Mixer::new(RATE);
    mixer.stream((1, 1), RATE);
    for length in [0, Mixer::stream_ring(RATE) - 1, Mixer::stream_ring(RATE) + 1] {
        let ring: Vec<i16> = Vec::with_capacity(length + 7);
        let (place, capacity) = (ring.as_ptr(), ring.capacity());
        let mut ring = ring;
        ring.resize(length, 0);
        let refused = mixer.stream_with((1, 1), RATE, ring).unwrap_err();
        assert_eq!((refused.as_ptr(), refused.len(), refused.capacity()), (place, length, capacity), "the same buffer");
        assert!(!mixer.sounds.iter().any(|sound| sound.holds(place)), "not kept");
    }
    // The live sound it would have replaced is still there and plays.
    let out = stream_tone(&mut mixer);
    assert!(out.iter().any(|&value| value != 0));
    // A refused ring for a new key makes no sound.
    assert!(mixer.stream_with((2, 1), RATE, vec![0; 3]).is_err());
    assert_eq!(mixer.sounds.iter().map(Sound::key).collect::<Vec<_>>(), [(1, 1)]);
}

#[test]
fn a_ring_is_four_times_the_target() {
    assert_eq!(Mixer::stream_ring(RATE), 12_800);
    assert_eq!(Mixer::stream_ring(48_000), 76_800);
    // A rate of zero is taken as one, as `stream` takes it.
    assert_eq!(Mixer::stream_ring(0), 1);
}

// Verifies REQ-AUD-013 (DEC-2470): the mixer reports where a voice is in its
// stored sound, for a port's `VoiceSink::position`, and nothing for a voice
// that is silent or playing a live sound.
#[test]
fn a_voice_reports_where_it_is_in_its_stored_sound() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 1_000, 4000);
    assert_eq!(mixer.position(0), None, "nothing plays");
    mixer.start(0, start((1, 1), 100));
    assert_eq!(mixer.position(0), Some(0));
    render(&mut mixer, 250);
    // At the sound's own rate a voice moves one sample an output sample.
    assert_eq!(mixer.position(0), Some(250));
    mixer.stream((2, 1), RATE);
    mixer.start(1, start((2, 1), 100));
    assert_eq!(mixer.position(1), None, "a live sound's place is in its ring");
    mixer.stop(0, 5);
    assert_eq!(mixer.position(0), None, "a stopping voice");
}

/// The output rate of the tests of the pulled voice and the peak: the rate
/// a pulled voice is asked at, so that the resampler's filter stands on its
/// centre tap.
const DEVICE_RATE: u32 = PULLED_RATE_HZ;

/// A sine of `amplitude` and 480 samples a period at [`DEVICE_RATE`], 100 Hz.
fn sine(amplitude: f64, samples: u32) -> Vec<i16> {
    (0..samples)
        .map(|n| {
            #[allow(clippy::cast_possible_truncation, reason = "the amplitude is within a sample's range")]
            let value = (amplitude * (std::f64::consts::TAU * f64::from(n) / 480.0).sin()).round() as i16;
            value
        })
        .collect()
}

// Verifies RULE-32 (REQ-6527): the level is linear, 255 at full scale and
// halving with each 6 decibels.
#[test]
fn a_sine_at_each_listed_level_is_metered_at_it() {
    for (amplitude, level) in
        [(32_767.0, 255), (16_384.0, 128), (8_192.0, 64), (4_096.0, 32), (2_048.0, 16), (1_024.0, 8), (512.0, 4)]
    {
        let mut mixer = Mixer::new(DEVICE_RATE);
        mixer.define((1, 1), DEVICE_RATE, 4_800);
        mixer.samples((1, 1), 0, &sine(amplitude, 4_800));
        mixer.set_metered(0, true);
        mixer.start(0, start((1, 1), 100));
        // Past the ramp in, then one period and more.
        render(&mut mixer, 1_000);
        mixer.take_peak(0);
        render(&mut mixer, 800);
        let peak = mixer.take_peak(0);
        // The filter's centre tap is 14,734 of 16,384 with its neighbours
        // making up the rest, so a slow sine passes within a part in 500.
        let metered = meter_level(peak);
        assert!(metered.abs_diff(level) <= 1, "amplitude {amplitude}: peak {peak}, level {metered}, expected {level}");
    }
}

// Verifies RULE-32 (REQ-6527): the peak is since last asked, after the
// voice's gain and before the master volume and the mute, and only of a
// watched voice.
#[test]
fn the_peak_is_since_last_asked_after_the_gain_and_before_the_master() {
    let mut mixer = Mixer::new(RATE);
    constant(&mut mixer, (1, 1), 20_000, 8_000);
    constant(&mut mixer, (2, 1), 20_000, 8_000);
    mixer.set_metered(0, true);
    mixer.set_master(10, true);
    mixer.start(0, start((1, 1), 50));
    mixer.start(1, start((2, 1), 100));
    let out = render(&mut mixer, 400);
    assert_eq!(out[399], 0, "muted");
    let peak = mixer.take_peak(0);
    assert!((9_900..=10_100).contains(&peak), "half of 20,000, the master and the mute apart: {peak}");
    assert_eq!(mixer.take_peak(0), 0, "taken: it starts again");
    assert_eq!(mixer.take_peak(1), 0, "voice 1 is not watched, so nothing was metered for it");
    assert!(!mixer.is_metered(1));
    mixer.stop(0, 5);
    render(&mut mixer, 400);
    mixer.take_peak(0);
    render(&mut mixer, 100);
    assert_eq!(mixer.take_peak(0), 0, "a silent voice has no peak");
    assert_eq!(meter_level(PEAK_FULL_SCALE), 255);
    assert_eq!(meter_level(127), 0);
    assert_eq!(meter_level(128), 1);
}

/// A scripted source of a pulled voice: every sample is `value` as it
/// stands when its block is asked for, and it counts the samples and the
/// blocks asked for.
struct Scripted {
    value: i16,
    samples: usize,
    blocks: usize,
    longest: usize,
}

impl Scripted {
    fn of(value: i16) -> Self {
        Self { value, samples: 0, blocks: 0, longest: 0 }
    }
}

impl Pull for Scripted {
    fn pull(&mut self, voice: usize, key: SoundKey, block: &mut [i16]) {
        assert_eq!((voice, key), (0, (9, 1)), "the voice and the sound it plays");
        assert!((1..=LOCAL_BLOCK_SAMPLES).contains(&block.len()), "a block of 1 to 128 samples: {}", block.len());
        block.fill(self.value);
        self.samples = self.samples.saturating_add(block.len());
        self.blocks = self.blocks.saturating_add(1);
        self.longest = self.longest.max(block.len());
    }
}

// Verifies RULE-62 (REQ-6527): what a source renders from a message queued
// before a device buffer sounds from the first samples of that buffer,
// wherever the buffer before it ended; and nothing is pulled ahead of the
// buffer in hand.
#[test]
fn a_pulled_voice_sounds_from_the_start_of_the_device_buffer_after_a_message() {
    // A device buffer of 10 ms, and others that end elsewhere in a block.
    for buffer in [480_usize, 441, 128, 100, 37, 1] {
        let mut mixer = Mixer::new(DEVICE_RATE);
        mixer.pulled((9, 1));
        mixer.start(0, start((9, 1), 100));
        let mut source = Scripted::of(0);
        let mut rendered = 0_usize;
        // Buffers of silence past the ramp in of 5 ms.
        for _ in 0..480_usize.div_ceil(buffer) + 7 {
            let mut out = vec![0; buffer];
            mixer.render_pulling(&mut out, &mut source);
            assert!(out.iter().all(|&value| value == 0));
            rendered += buffer;
            // Exactly what the buffer's samples read was pulled: the last
            // of them and the taps the filter reads past it.
            assert_eq!(source.samples, rendered + PULLED_LOOKAHEAD_SAMPLES, "buffer {buffer}");
        }
        assert!(source.blocks > 0 && source.longest <= LOCAL_BLOCK_SAMPLES);
        // The message arrives: every block rendered from now holds the note.
        source.value = 10_000;
        let mut out = vec![0; 480];
        mixer.render_pulling(&mut out, &mut source);
        // The buffer's first sample reads the new block through the
        // filter's last tap, which weighs nothing where the rates agree:
        // the note is in the second, and at its level once the filter's
        // taps are all upon it.
        let first = out.iter().position(|&value| value != 0).unwrap();
        assert_eq!(first, 1, "buffer {buffer}: the first sounding sample");
        let settled = PULLED_LOOKAHEAD_SAMPLES + TAPS_BEFORE as usize + 1;
        assert!((9_900..=10_100).contains(&out[settled]), "buffer {buffer}: {}", out[settled]);
        assert!(first < LOCAL_BLOCK_SAMPLES, "within the first 128 samples of the buffer (RULE-62)");
    }
}

// Verifies RULE-62 (REQ-6527): a device buffer is pulled in blocks of 128
// samples, the last cut to what the buffer still needs, and a message that
// arrives while a buffer is rendered waits for the next block and no
// longer.
#[test]
fn a_device_buffer_is_pulled_in_blocks_of_128_samples() {
    /// A source whose value changes after its first block of each call.
    struct Stepping {
        blocks: Vec<usize>,
    }
    impl Pull for Stepping {
        fn pull(&mut self, _voice: usize, _key: SoundKey, block: &mut [i16]) {
            // The second block of the run holds the note: a message that
            // came while the first was being rendered.
            block.fill(if self.blocks.is_empty() { 0 } else { 10_000 });
            self.blocks.push(block.len());
        }
    }
    let mut mixer = Mixer::new(DEVICE_RATE);
    mixer.pulled((9, 1));
    mixer.start(0, Start { fade_ms: 0, ..start((9, 1), 100) });
    let mut source = Stepping { blocks: Vec::new() };
    let mut out = vec![0; 480];
    mixer.render_pulling(&mut out, &mut source);
    // 480 samples and the filter's 8 ahead: three blocks of 128 and one of
    // what is left.
    assert_eq!(source.blocks, [128, 128, 128, 104]);
    let first = out.iter().position(|&value| value != 0).unwrap();
    assert!((LOCAL_BLOCK_SAMPLES - PULLED_LOOKAHEAD_SAMPLES..=LOCAL_BLOCK_SAMPLES).contains(&first), "{first}");
}

// Verifies RULE-62 (REQ-6527): a pulled voice is resampled to the device's
// rate, stops and gains as any voice does, and ends when it is forgotten.
#[test]
fn a_pulled_voice_is_resampled_stopped_and_forgotten_as_any_voice() {
    let mut mixer = Mixer::new(44_100);
    mixer.pulled((9, 1));
    mixer.start(0, start((9, 1), 50));
    let mut source = Scripted::of(10_000);
    let mut out = vec![0; 4_410];
    mixer.render_pulling(&mut out, &mut source);
    assert!((4_900..=5_100).contains(&out[4_000]), "at its gain: {}", out[4_000]);
    // A tenth of a second at 44.1 kHz is 4,800 samples at 48 kHz, less the
    // one step the last output sample has yet to take.
    assert_eq!(source.samples, 4_799 + PULLED_LOOKAHEAD_SAMPLES, "pulled at 48 kHz");
    assert_eq!(mixer.position(0), None, "a pulled voice is at no place in a stored sound");
    mixer.gain(0, 100, 5);
    mixer.render_pulling(&mut out, &mut source);
    assert!((9_900..=10_100).contains(&out[4_000]), "{}", out[4_000]);
    mixer.stop(0, 5);
    mixer.render_pulling(&mut out, &mut source);
    assert!(out[4_000] == 0 && !mixer.active(), "stopped");
    let samples = source.samples;
    mixer.render_pulling(&mut out, &mut source);
    assert_eq!(source.samples, samples, "a stopped voice pulls nothing");
    // Forgotten while it plays, it ends at once and is collected.
    mixer.start(0, start((9, 1), 100));
    mixer.render_pulling(&mut out, &mut source);
    mixer.forget((9, 1));
    mixer.render_pulling(&mut out, &mut source);
    assert!(out[4_000] == 0 && !mixer.active());
    // Without a source it is silence.
    mixer.pulled((9, 1));
    mixer.start(0, start((9, 1), 100));
    assert!(render(&mut mixer, 1_000).iter().all(|&value| value == 0));
}

// Verifies RULE-62 (REQ-6527): the ring is what the mixer says, and another
// is handed back.
#[test]
fn a_pulled_ring_of_the_wrong_length_is_handed_back() {
    let mut mixer = Mixer::new(DEVICE_RATE);
    assert_eq!(Mixer::pulled_ring(), 4 * LOCAL_BLOCK_SAMPLES);
    assert_eq!(LOCAL_BLOCK_SAMPLES, 128);
    assert_eq!(mixer.pulled_with((9, 1), vec![0; 100]).unwrap_err().len(), 100);
    assert!(mixer.sounds.is_empty());
    assert!(mixer.pulled_with((9, 1), vec![0; Mixer::pulled_ring()]).is_ok());
}
