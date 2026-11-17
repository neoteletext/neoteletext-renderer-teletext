//! The DX7 rack: MIDI messages, sixteen notes among sixteen parts, and its
//! bounds (REQ-6412 items 6 and 7).
#![cfg(feature = "dx7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "a test"
)]

use neoteletext_audio_instruments::dx7::msfa::env::Env;
use neoteletext_audio_instruments::dx7::patch::PACKED_VOICE_BYTES;
use neoteletext_audio_instruments::dx7::{Bank, Patch, Rack, VoiceParams};

/// The default sine, changed by `change`.
fn patch(change: impl FnOnce(&mut VoiceParams)) -> Patch {
    let mut params = *Patch::DEFAULT.params();
    change(&mut params);
    Patch::new(params).unwrap()
}

/// A sine that takes about 20 seconds to die away after its key goes up
/// (rate 20 is a qrate of 12, 2.3 decibels a second).
fn lingering() -> Patch {
    patch(|voice| voice.operators[0].eg_rates[3] = 20)
}

fn rack_with(patches: &[Patch]) -> Rack {
    let mut bank = Bank::new();
    for (index, patch) in patches.iter().enumerate() {
        bank.set(index, *patch);
    }
    let mut rack = Rack::new();
    rack.set_bank(&bank);
    rack
}

fn render(rack: &mut Rack, samples: usize) -> Vec<i16> {
    let mut out = vec![0_i16; samples];
    rack.render(&mut out);
    out
}

fn peak(samples: &[i16]) -> i32 {
    samples.iter().map(|&sample| i32::from(sample).abs()).max().unwrap_or(0)
}

fn sounding(rack: &Rack, part: u8) -> Vec<u8> {
    (0..128).filter(|&key| rack.is_sounding(part, key)).collect()
}

/// How long all sound off takes to silence a note: the rack's fade of 32
/// blocks of 32 samples (`src/dx7/note.rs`, `CUT_FADE_STEP`), 21.3
/// milliseconds, and a block more for the block in which it is heard.
const CUT_SAMPLES: usize = 33 * 32;
#[test]
fn a_note_sounds_until_its_key_goes_up() {
    let mut rack = Rack::new();
    assert_eq!(peak(&render(&mut rack, 960)), 0);
    rack.event(0x90, 60, 100);
    assert_eq!(rack.sounding_notes(), 1);
    assert!(peak(&render(&mut rack, 960)) > 2000);
    rack.event(0x80, 60, 64);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0);
    assert_eq!(peak(&render(&mut rack, 960)), 0);
    // A note on with a velocity of 0 is a note off.
    rack.event(0x93, 60, 100);
    assert!(rack.is_sounding(3, 60));
    rack.event(0x93, 60, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0);
    assert_eq!(rack.ignored_events(), 0);
}

/// The seventeenth note takes the place of the note released longest ago.
#[test]
fn a_seventeenth_note_takes_the_note_released_longest_ago() {
    let mut rack = rack_with(&[lingering()]);
    for key in 40..56 {
        rack.event(0x90, key, 100);
    }
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 16);
    // Released in this order: 47, then 42, then 50.
    for key in [47, 42, 50] {
        rack.event(0x80, key, 0);
        render(&mut rack, 320);
    }
    assert_eq!(rack.sounding_notes(), 16, "the released notes linger");

    rack.event(0x90, 100, 100);
    assert_eq!(rack.sounding_notes(), 16);
    assert!(rack.is_sounding(0, 100));
    assert!(!rack.is_sounding(0, 47), "47 was released first");
    assert!(rack.is_sounding(0, 42) && rack.is_sounding(0, 50));

    rack.event(0x90, 101, 100);
    assert!(!rack.is_sounding(0, 42), "42 was released next");
    assert!(rack.is_sounding(0, 50));
    rack.event(0x90, 102, 100);
    assert!(!rack.is_sounding(0, 50));
    assert_eq!(rack.sounding_notes(), 16);
    assert_eq!(sounding(&rack, 0), [40, 41, 43, 44, 45, 46, 48, 49, 51, 52, 53, 54, 55, 100, 101, 102]);
}

/// With none released, the seventeenth takes the oldest held; a note held
/// by the pedal counts as held.
#[test]
fn a_seventeenth_note_takes_the_oldest_held_if_none_is_released() {
    let mut rack = rack_with(&[lingering()]);
    // Across parts: the sixteen notes are shared.
    for key in 40..56_u8 {
        rack.event(0x90 | (key & 3), key, 100);
    }
    render(&mut rack, 960);
    rack.event(0x95, 100, 100);
    assert_eq!(rack.sounding_notes(), 16);
    assert!(!rack.is_sounding(0, 40), "40 was the oldest");
    assert!(rack.is_sounding(1, 41) && rack.is_sounding(5, 100));

    // 41 is now the oldest, but its key is retriggered, which makes it the
    // newest; 42 is the oldest then.
    rack.event(0x91, 41, 100);
    assert_eq!(rack.sounding_notes(), 16);
    rack.event(0x95, 101, 100);
    assert!(rack.is_sounding(1, 41));
    assert!(!rack.is_sounding(2, 42));

    // Pedal down on part 3; key 43 goes up and is held by the pedal: it is
    // not "released", so a released note, if any, goes first.
    rack.event(0xb3, 64, 127);
    rack.event(0x83, 43, 0);
    rack.event(0x80, 44, 0);
    rack.event(0x95, 102, 100);
    assert!(rack.is_sounding(3, 43), "the pedal holds 43");
    assert!(!rack.is_sounding(0, 44), "44 was released");
    // With no released note left, 43 is the oldest held.
    rack.event(0x95, 103, 100);
    assert!(!rack.is_sounding(3, 43));
    assert_eq!(rack.sounding_notes(), 16);
}

#[test]
fn a_key_already_sounding_is_retriggered_in_place() {
    let mut rack = rack_with(&[lingering()]);
    rack.event(0x90, 60, 100);
    rack.event(0x90, 60, 100);
    assert_eq!(rack.sounding_notes(), 1);
    render(&mut rack, 960);
    // Also while it dies away.
    rack.event(0x80, 60, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 1);
    rack.event(0x90, 60, 100);
    assert_eq!(rack.sounding_notes(), 1);
    // The same key on another part is another note.
    rack.event(0x91, 60, 100);
    assert_eq!(rack.sounding_notes(), 2);
    // One note off ends it: the retriggered note is held by one key.
    rack.event(0x80, 60, 0);
    rack.event(0xb0, 120, 0);
    rack.event(0xb1, 120, 0);
    render(&mut rack, CUT_SAMPLES);
    assert_eq!(rack.sounding_notes(), 0);
}

#[test]
fn the_sustain_pedal_holds_notes() {
    let mut rack = Rack::new();
    rack.event(0xb0, 64, 127);
    rack.event(0x90, 60, 100);
    rack.event(0x90, 64, 100);
    rack.event(0x80, 60, 0);
    let held = render(&mut rack, 4800);
    assert_eq!(sounding(&rack, 0), [60, 64]);
    assert!(peak(&held[2400..]) > 2000);

    // The pedal of another part does nothing to this one.
    rack.event(0xb1, 64, 0);
    render(&mut rack, 960);
    assert_eq!(sounding(&rack, 0), [60, 64]);

    // Pedal up: 60, whose key is up, ends; 64, whose key is down, stays.
    rack.event(0xb0, 64, 0);
    render(&mut rack, 960);
    assert_eq!(sounding(&rack, 0), [64]);
    rack.event(0x80, 64, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0);

    // 63 is up and 64 is down.
    rack.event(0xb0, 64, 63);
    rack.event(0x90, 60, 100);
    rack.event(0x80, 60, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0);
    rack.event(0xb0, 64, 64);
    rack.event(0x90, 60, 100);
    rack.event(0x80, 60, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 1);
    assert_eq!(rack.ignored_events(), 0);
}

#[test]
fn all_notes_off_releases_a_parts_keys() {
    let mut rack = Rack::new();
    for key in [60, 64, 67] {
        rack.event(0x90, key, 100);
        rack.event(0x91, key, 100);
    }
    rack.event(0xb0, 123, 0);
    render(&mut rack, 960);
    assert_eq!(sounding(&rack, 0), [] as [u8; 0]);
    assert_eq!(sounding(&rack, 1), [60, 64, 67]);

    // The pedal still holds, as it holds after a note off.
    rack.event(0xb1, 64, 127);
    rack.event(0xb1, 123, 0);
    render(&mut rack, 960);
    assert_eq!(sounding(&rack, 1), [60, 64, 67]);
    rack.event(0xb1, 64, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0);
    assert_eq!(rack.ignored_events(), 0);
}

#[test]
fn all_sound_off_silences_a_part_within_twenty_two_milliseconds() {
    let mut rack = rack_with(&[lingering()]);
    rack.event(0xb0, 64, 127);
    for key in [60, 64, 67] {
        rack.event(0x90, key, 100);
        rack.event(0x92, key, 100);
    }
    render(&mut rack, 960);
    let before = render(&mut rack, 960);
    let step = |samples: &[i16]| samples.windows(2).map(|pair| (i32::from(pair[0]) - i32::from(pair[1])).abs()).max();
    rack.event(0xb0, 120, 0);
    let fading = render(&mut rack, CUT_SAMPLES);
    assert_eq!(sounding(&rack, 0), [] as [u8; 0], "the pedal does not hold against all sound off");
    assert_eq!(sounding(&rack, 2), [60, 64, 67]);
    // No click: no step between two samples larger than the chords' own.
    assert!(step(&fading) <= step(&before), "{:?} then {:?}", step(&before), step(&fading));
}

#[test]
fn release_all_releases_every_part_and_lifts_every_pedal() {
    let mut rack = Rack::new();
    rack.event(0xb0, 64, 127);
    rack.event(0x90, 60, 100);
    rack.event(0x80, 60, 0);
    rack.event(0x90, 62, 100);
    rack.event(0x97, 64, 100);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 3);
    rack.release_all();
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0);
    // The pedal is up again.
    rack.event(0x90, 60, 100);
    rack.event(0x80, 60, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0);
}

#[test]
fn a_program_change_chooses_the_patch_of_later_notes() {
    let octave_up = patch(|voice| voice.operators[0].frequency_coarse = 2);
    let patches = [Patch::DEFAULT, octave_up];
    let crossings = |samples: &[i16]| samples.windows(2).filter(|pair| pair[0] < 0 && pair[1] >= 0).count();

    let mut rack = rack_with(&patches);
    rack.event(0x90, 69, 100);
    assert!((439..=441).contains(&crossings(&render(&mut rack, 48_000))));

    // The note that sounds keeps its patch.
    rack.event(0xc0, 1, 0);
    assert!((439..=441).contains(&crossings(&render(&mut rack, 48_000))));
    rack.event(0x80, 69, 0);
    render(&mut rack, 960);
    rack.event(0x90, 69, 100);
    assert!((879..=881).contains(&crossings(&render(&mut rack, 48_000))));

    // Each part has its own program; part 1 is still at program 0.
    rack.event(0x80, 69, 0);
    render(&mut rack, 960);
    rack.event(0x91, 69, 100);
    assert!((439..=441).contains(&crossings(&render(&mut rack, 48_000))));

    // An empty place of the bank is the default patch; a program above 31
    // is ignored and counted, and the program stays.
    rack.event(0x81, 69, 0);
    rack.event(0xc0, 31, 0);
    render(&mut rack, 960);
    rack.event(0x90, 69, 100);
    assert!((439..=441).contains(&crossings(&render(&mut rack, 48_000))));
    assert_eq!(rack.ignored_events(), 0);
    rack.event(0xc0, 1, 0x80);
    rack.event(0xc0, 32, 0);
    rack.event(0xc0, 127, 0);
    assert_eq!(rack.ignored_events(), 2);
    rack.event(0x80, 69, 0);
    render(&mut rack, 960);
    rack.event(0x90, 69, 100);
    assert!((879..=881).contains(&crossings(&render(&mut rack, 48_000))));
}

#[test]
fn other_messages_are_ignored_and_counted() {
    let mut rack = Rack::new();
    let ignored = [
        (0xa0, 60, 100),   // polyphonic key pressure
        (0xd0, 100, 0),    // channel pressure
        (0xf0, 0x43, 0),   // system exclusive
        (0xf8, 0, 0),      // timing clock
        (0xff, 0, 0),      // reset
        (0x00, 60, 100),   // not a status byte
        (0x7f, 60, 100),   // not a status byte
        (0xb0, 0, 0),      // bank select
        (0xb0, 2, 64),     // breath controller
        (0xb0, 65, 127),   // portamento switch
        (0xb0, 122, 0),    // local control
        (0xb0, 127, 0),    // poly mode
        (0x90, 0x80, 100), // a data byte above 127
        (0x90, 60, 0x80),
        (0x80, 60, 0xff),
        (0xb0, 7, 0x80),
        (0xe0, 0x80, 0),
        (0xe0, 0, 0x80),
        (0xc0, 0x80, 0),
    ];
    for (count, (status, data1, data2)) in ignored.into_iter().enumerate() {
        rack.event(status, data1, data2);
        assert_eq!(rack.ignored_events(), count as u64 + 1, "{status:#x} {data1} {data2}");
    }
    assert_eq!(rack.sounding_notes(), 0);
    assert_eq!(peak(&render(&mut rack, 960)), 0);

    let handled =
        [(0x90, 60, 1), (0x80, 60, 0), (0x80, 61, 0), (0xb0, 1, 5), (0xb0, 7, 90), (0xb0, 64, 0), (0xb0, 120, 0)];
    let before = rack.ignored_events();
    for (status, data1, data2) in
        handled.into_iter().chain([(0xb0, 121, 0), (0xb0, 123, 0), (0xc0, 0, 0), (0xe0, 0, 64)])
    {
        rack.event(status, data1, data2);
    }
    assert_eq!(rack.ignored_events(), before);
    rack.reset();
    assert_eq!(rack.ignored_events(), 0);
}

/// Volume is a square law in the log domain: 127 is no attenuation, 90 is
/// half the amplitude (`(90/127)^2`), 0 is silence.
#[test]
fn volume_attenuates_a_part() {
    let level = |volume: Option<u8>| {
        let mut rack = Rack::new();
        if let Some(volume) = volume {
            rack.event(0xb0, 7, volume);
        }
        rack.event(0x90, 69, 100);
        peak(&render(&mut rack, 9600)[4800..])
    };
    let full = level(None);
    assert_eq!(level(Some(127)), full);
    assert_eq!(level(Some(0)), 0);
    let half = f64::from(level(Some(90))) / f64::from(full);
    assert!((half - (90.0_f64 / 127.0).powi(2)).abs() < 0.005, "{half}");
    // Another part's volume does nothing to this one, and volume glides
    // over a block of 32 samples: on a sine of 3,520 Hz, 14 samples long,
    // each period is quieter than the one before until the new level.
    let mut rack = Rack::new();
    rack.event(0xb1, 7, 0);
    rack.event(0x90, 105, 100);
    assert_eq!(peak(&render(&mut rack, 4800)[2400..]), full);
    rack.event(0xb0, 7, 20);
    let samples = render(&mut rack, 4800);
    let (first, second, settled) = (peak(&samples[..14]), peak(&samples[14..28]), peak(&samples[2400..]));
    let expected = f64::from(full) * (20.0_f64 / 127.0).powi(2);
    assert!((f64::from(settled) - expected).abs() < 2.5, "volume 20 is (20/127)^2 of {full}: {settled}");
    assert!(first > 4 * settled && first > second && second > settled, "{first}, {second}, {settled}");
}

/// Reset all controllers puts the wheel, the bend and the pedal at rest
/// and leaves the volume.
#[test]
fn reset_all_controllers_leaves_the_volume() {
    let mut plain = Rack::new();
    plain.event(0xb0, 7, 64);
    plain.event(0x90, 69, 100);
    let expected = render(&mut plain, 9600);

    let mut rack = Rack::new();
    rack.event(0xb0, 7, 64);
    rack.event(0xb0, 1, 127);
    rack.event(0xe0, 0, 127);
    rack.event(0xb0, 64, 127);
    rack.event(0xb0, 121, 0);
    rack.event(0x90, 69, 100);
    assert_eq!(render(&mut rack, 9600), expected);
    rack.event(0x80, 69, 0);
    render(&mut rack, 960);
    assert_eq!(rack.sounding_notes(), 0, "the pedal is up");
}

/// Sixteen notes at the patch's full level, exactly in phase, reach the
/// output's limiter, which bends them under its ceiling; sixteen keys
/// that are not in phase stay clear of the ceiling.
#[test]
fn sixteen_full_level_notes_meet_the_limiter_and_not_the_rail() {
    let mut rack = Rack::new();
    for part in 0..16 {
        rack.event(0x90 | part, 69, 127);
    }
    let unison = render(&mut rack, 9600);
    let top = peak(&unison);
    assert!((KNEE + 1..=CEILING).contains(&top), "sixteen in unison peak at {top}");
    assert!(unwrapped(&unison));

    // Sixteen different keys: far from full scale most of the time.
    let mut rack = Rack::new();
    for key in 0..16 {
        rack.event(0x90, 48 + 3 * key, 127);
    }
    let chord = render(&mut rack, 48_000);
    assert!(peak(&chord) < CEILING, "{}", peak(&chord));
    let mean_square = chord.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / chord.len() as f64;
    println!("sixteen keys: peak {}, RMS {:.0}", peak(&chord), mean_square.sqrt());
    assert!(mean_square.sqrt() < 13_000.0);
}

/// What is far beyond full scale rests at the limiter's ceiling and is
/// not wrapped: sixteen notes in unison of a voice whose velocity
/// sensitivity raises it above full level.
#[test]
fn what_is_beyond_full_scale_rests_at_the_ceiling() {
    let mut rack = rack_with(&[patch(|voice| voice.operators[0].key_velocity_sensitivity = 7)]);
    for part in 0..16 {
        rack.event(0xc0 | part, 0, 0);
        rack.event(0x90 | part, 69, 127);
    }
    let samples = render(&mut rack, 9600);
    assert!((CEILING - 40..=CEILING).contains(&peak(&samples)), "{}", peak(&samples));
    assert!(unwrapped(&samples));
}

/// The output limiter's knee and ceiling.
const KNEE: i32 = neoteletext_audio_instruments::output::LIMIT_KNEE as i32;
const CEILING: i32 = neoteletext_audio_instruments::output::LIMIT_CEILING as i32;

/// Whether no sample jumps from one rail towards the other, as a wrapped
/// one would: a 440 Hz sine of full scale moves 1,900 a sample.
fn unwrapped(samples: &[i16]) -> bool {
    samples.windows(2).all(|pair| (i32::from(pair[0]) - i32::from(pair[1])).abs() < 8000)
}

/// A small generator of random numbers: xorshift.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: u64) -> u64 {
        self.next() % limit
    }
}

/// A random voice that the instrument could hold: each byte within its
/// field.
fn random_patch(random: &mut Random) -> Patch {
    loop {
        let mut packed = [0_u8; PACKED_VOICE_BYTES];
        for (index, byte) in packed.iter_mut().enumerate() {
            let limit = match index {
                0..102 if matches!(index % 17, 11..=13 | 15) => 120,
                110 | 111 | 118.. => 128,
                116 => 96,
                117 => 49,
                _ => 100,
            };
            *byte = random.below(limit) as u8;
        }
        if let Ok(patch) = Patch::from_packed(&packed) {
            return patch;
        }
    }
}

fn random_bank(random: &mut Random) -> Bank {
    let mut bank = Bank::new();
    for index in 0..32 {
        bank.set(index, random_patch(random));
    }
    bank
}

/// A stream of messages a player might send, mostly: notes, the handled
/// controllers, program changes and bends on a few parts.
fn musical_event(random: &mut Random) -> (u8, u8, u8) {
    let part = random.below(4) as u8;
    let key = 36 + random.below(48) as u8;
    match random.below(16) {
        0..6 => (0x90 | part, key, random.below(128) as u8),
        6..11 => (0x80 | part, key, 0),
        11 => (0xb0 | part, [1, 7, 64, 120, 121, 123][random.below(6) as usize], random.below(128) as u8),
        12 => (0xc0 | part, random.below(32) as u8, 0),
        13 => (0xe0 | part, random.below(128) as u8, random.below(128) as u8),
        _ => (random.below(256) as u8, random.below(256) as u8, random.below(256) as u8),
    }
}

/// Two racks given the same messages render the same samples, and a third
/// rack doing something else in between changes nothing.
#[test]
fn a_rack_is_deterministic_and_alone() {
    let mut random = Random(0x1dea_5eed_0bad_cafe);
    let bank = random_bank(&mut random);
    let mut first = Rack::new();
    let mut second = Rack::new();
    let mut other = Rack::new();
    first.set_bank(&bank);
    second.set_bank(&bank);
    other.set_bank(&random_bank(&mut random));
    let mut heard = 0_u64;
    for _ in 0..300 {
        let events: Vec<(u8, u8, u8)> = (0..random.below(6)).map(|_| musical_event(&mut random)).collect();
        for &(status, data1, data2) in &events {
            first.event(status, data1, data2);
        }
        let a = render(&mut first, 960);

        // The other rack plays and renders between the two.
        let (status, data1, data2) = musical_event(&mut random);
        other.event(status, data1, data2);
        render(&mut other, 1 + random.below(2000) as usize);

        for &(status, data1, data2) in &events {
            second.event(status, data1, data2);
        }
        let b = render(&mut second, 960);
        assert_eq!(a, b);
        heard += a.iter().map(|&sample| u64::from(sample.unsigned_abs())).sum::<u64>();
    }
    assert!(heard > 1_000_000, "the racks were nearly silent: {heard}");
    assert_eq!(first.ignored_events(), second.ignored_events());

    // A rack after reset is a new rack with the same bank.
    first.reset();
    let mut fresh = Rack::new();
    fresh.set_bank(&bank);
    for _ in 0..50 {
        let (status, data1, data2) = musical_event(&mut random);
        first.event(status, data1, data2);
        fresh.event(status, data1, data2);
        assert_eq!(render(&mut first, 960), render(&mut fresh, 960));
    }
}

/// A key struck again while its note sounds goes on sounding to the end of
/// the block it is in, wherever in the block the message comes: the note
/// is not left out of the rest of that block, which would cut a notch of
/// silence into it.
#[test]
fn a_retriggered_note_sounds_through_the_block_it_is_struck_in() {
    // Without oscillator key sync the sine runs on through the second key,
    // so that nothing but a gap can make a step.
    let unsynced = patch(|voice| voice.oscillator_key_sync = 0);
    let largest_step = |samples: &[i16]| {
        samples.windows(2).map(|pair| (i32::from(pair[1]) - i32::from(pair[0])).abs()).max().unwrap_or(0)
    };
    let mut steady = rack_with(&[unsynced]);
    steady.event(0x90, 69, 100);
    let slope = largest_step(&render(&mut steady, 9600)[4800..]);
    assert!((150..250).contains(&slope), "a sine of 440 Hz at 3,270 steps by {slope}");
    for offset in 0..32 {
        let mut rack = rack_with(&[unsynced]);
        rack.event(0x90, 69, 100);
        let mut samples = render(&mut rack, 4800 + offset);
        rack.event(0x90, 69, 100);
        samples.extend(render(&mut rack, 960));
        let step = largest_step(&samples[2400..]);
        assert!(step <= slope + 2, "struck again {offset} samples into a block: a step of {step}");
    }
}

/// Blocks are counted across calls: rendering in calls of any lengths gives
/// the samples one call gives.
#[test]
fn the_length_of_a_call_does_not_matter() {
    let mut random = Random(0x0c0f_fee0_1234_abcd);
    let bank = random_bank(&mut random);
    let play = |rack: &mut Rack| {
        rack.set_bank(&bank);
        for part in 0..4 {
            rack.event(0xc0 | part, 3 + part, 0);
            rack.event(0x90 | part, 50 + 5 * part, 110);
        }
    };
    let mut whole = Rack::new();
    play(&mut whole);
    let expected = render(&mut whole, 20_000);

    let mut pieces = Rack::new();
    play(&mut pieces);
    let mut got = Vec::new();
    while got.len() < expected.len() {
        let length = (random.below(100) as usize).min(expected.len() - got.len());
        got.extend(render(&mut pieces, length));
    }
    assert_eq!(got, expected);
}

/// Every note ends, and the rack is silent, within a time computed from
/// the patch after all keys are released: the envelope falls from at most
/// 16 doublings at its fourth rate, and a voice whose fourth level is not
/// zero is then faded over 750 blocks.
#[test]
fn every_note_ends_within_its_bound() {
    for (rate, level, key_scaling) in
        [(60_u8, 0_u8, 0_u8), (45, 0, 0), (45, 0, 7), (70, 60, 0), (99, 99, 0), (35, 0, 0)]
    {
        let voice = patch(|voice| {
            voice.algorithm = 31;
            for operator in &mut voice.operators {
                operator.output_level = 99;
                operator.eg_rates[3] = rate;
                operator.eg_levels[3] = level;
                operator.rate_scaling = key_scaling;
            }
        });
        let mut rack = rack_with(&[voice]);
        for key in 30..46 {
            rack.event(0x90, key, 127);
        }
        render(&mut rack, 9600);
        assert_eq!(rack.sounding_notes(), 16);
        rack.release_all();

        // Rate scaling only shortens it, so the bound is the unscaled rate's.
        let qrate = (i32::from(rate) * 41) >> 6;
        let fall_blocks = (16_i64 << 24) / i64::from(Env::increment(qrate)) + 1;
        let fade_blocks = if level == 0 { 0 } else { 750 };
        let bound_blocks = (fall_blocks + fade_blocks + 3) as usize;
        let tail = render(&mut rack, bound_blocks * 32);
        assert_eq!(rack.sounding_notes(), 0, "rate {rate} level {level}: notes sound after {bound_blocks} blocks");
        assert_eq!(peak(&render(&mut rack, 4800)), 0);
        // And it was a release, not a cut: there is sound a twentieth of the
        // way through the bound.
        assert!(peak(&tail[tail.len() / 20..]) > 0 || bound_blocks < 100, "rate {rate} level {level}");
        println!(
            "rate {rate}, level {level}, scaling {key_scaling}: silent within {:.2} s",
            bound_blocks as f64 * 32.0 / 48_000.0
        );
    }
}

/// Random bytes as messages, random banks, calls of random lengths: no
/// panic (this build checks every overflow), never more than sixteen notes.
#[test]
fn random_messages_never_panic() {
    let mut random = Random(0x0f02_2b0a_7dea_d5ed);
    for round in 0..40 {
        let mut rack = Rack::new();
        rack.set_bank(&random_bank(&mut random));
        let mut out = [0_i16; 700];
        for step in 0..400 {
            let (status, data1, data2) = if round % 2 == 0 {
                musical_event(&mut random)
            } else {
                (random.next() as u8, random.next() as u8, random.next() as u8)
            };
            rack.event(status, data1, data2);
            if step % 3 == 0 {
                let length = random.below(700) as usize;
                rack.render(&mut out[..length]);
            }
            if random.below(200) == 0 {
                rack.release_all();
            }
            if random.below(300) == 0 {
                rack.set_bank(&random_bank(&mut random));
            }
            if random.below(500) == 0 {
                rack.reset();
            }
            assert!(rack.sounding_notes() <= 16);
        }
    }
}

/// How fast a rack renders with sixteen notes held, each of six operators
/// with feedback. Run it optimised:
/// `cargo test --release ... --test dx7_rack -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, which prints; meaningful only in a release build"]
fn speed_with_sixteen_notes_held() {
    let voice = patch(|voice| {
        voice.algorithm = 4;
        voice.feedback = 6;
        voice.lfo_pitch_mod_depth = 30;
        voice.lfo_amp_mod_depth = 30;
        for (operator, coarse) in voice.operators.iter_mut().zip([1, 2, 1, 3, 1, 7]) {
            operator.output_level = 90;
            operator.frequency_coarse = coarse;
            operator.amp_mod_sensitivity = 2;
        }
    });
    let mut rack = rack_with(&[voice]);
    for key in 0..16 {
        rack.event(0x90 | key, 40 + 3 * key, 100);
    }
    let seconds = 60;
    let mut block = [0_i16; 960];
    let mut heard = 0_u64;
    let start = std::time::Instant::now();
    for _ in 0..seconds * 50 {
        rack.render(&mut block);
        heard += block.iter().map(|&sample| u64::from(sample.unsigned_abs())).sum::<u64>();
    }
    let elapsed = start.elapsed().as_secs_f64();
    assert_eq!(rack.sounding_notes(), 16);
    assert!(heard > 0);
    println!(
        "{seconds} s of audio with 16 notes of 6 operators in {elapsed:.3} s: {:.1} times real time, {:.1} ns for each \
         operator sample",
        f64::from(seconds) / elapsed,
        elapsed * 1e9 / (f64::from(seconds) * 48_000.0 * 96.0)
    );
}
