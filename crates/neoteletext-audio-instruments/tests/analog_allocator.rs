//! The voice allocator of the analogue building blocks: which voice a
//! key takes, which it steals, the sustain pedal, and the monophonic
//! modes' priorities and triggers (TODO-6430).

// Without the feature there is nothing here to test.
#![cfg(feature = "analog")]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "a test")]

use neoteletext_audio_instruments::analog::allocator::{Allocator, Assign, HELD_KEYS, Mode, Priority, Stage, Trigger};

/// The key of each voice, or nothing for a voice at rest.
fn keys<const N: usize>(allocator: &Allocator<N>) -> [Option<u8>; N] {
    allocator.voices().map(|voice| (voice.stage != Stage::Rest).then_some(voice.key))
}

/// The stage of each voice.
fn stages<const N: usize>(allocator: &Allocator<N>) -> [Stage; N] {
    allocator.voices().map(|voice| voice.stage)
}

/// The trigger count of each voice.
fn triggers<const N: usize>(allocator: &Allocator<N>) -> [u32; N] {
    allocator.voices().map(|voice| voice.triggers)
}

#[test]
fn keys_take_voices_at_rest_and_let_them_go() {
    let mut allocator = Allocator::<4>::new(Mode::Poly(Assign::First));
    assert_eq!((allocator.sounding(), keys(&allocator)), (0, [None; 4]));
    allocator.key_down(60, 100);
    allocator.key_down(64, 90);
    assert_eq!(keys(&allocator), [Some(60), Some(64), None, None]);
    assert_eq!((allocator.voices()[0].velocity, allocator.voices()[1].velocity), (100, 90));
    assert!(allocator.voices()[0].gate() && allocator.voices()[1].gate() && !allocator.voices()[2].gate());
    assert_eq!(triggers(&allocator), [1, 1, 0, 0]);
    // Up: the voice is released, and sounds on until the instrument
    // says it has finished.
    allocator.key_up(60);
    assert_eq!(stages(&allocator), [Stage::Released, Stage::Held, Stage::Rest, Stage::Rest]);
    assert!(!allocator.voices()[0].gate());
    assert_eq!(allocator.sounding(), 2);
    allocator.finished(0);
    assert_eq!((allocator.sounding(), keys(&allocator)), (1, [None, Some(64), None, None]));
    // A voice that is held does not finish, and a voice that is not
    // there is nothing.
    allocator.finished(1);
    allocator.finished(99);
    assert_eq!(stages(&allocator)[1], Stage::Held);
    // A key that is not down goes up without effect.
    allocator.key_up(1);
    assert_eq!(keys(&allocator), [None, Some(64), None, None]);
}

#[test]
fn the_first_mode_takes_the_first_voice_at_rest_and_the_cyclic_the_next() {
    let mut first = Allocator::<4>::new(Mode::Poly(Assign::First));
    let mut cyclic = Allocator::<4>::new(Mode::Poly(Assign::Cyclic));
    // One key, played and let go three times over, each time finished.
    for round in 0..3 {
        for allocator in [&mut first, &mut cyclic] {
            allocator.key_down(60 + round, 64);
            allocator.key_up(60 + round);
        }
        let taken = |allocator: &Allocator<4>| stages(allocator).iter().position(|&stage| stage == Stage::Released);
        assert_eq!((taken(&first), taken(&cyclic)), (Some(0), Some(usize::from(round))));
        for allocator in [&mut first, &mut cyclic] {
            for voice in 0..4 {
                allocator.finished(voice);
            }
        }
    }
    // The cyclic mode leaves a released note ringing while there is a
    // voice at rest: four keys in turn land on four voices though each
    // is let go before the next.
    let mut cyclic = Allocator::<4>::new(Mode::Poly(Assign::Cyclic));
    for key in 0..4 {
        cyclic.key_down(key, 64);
        cyclic.key_up(key);
    }
    assert_eq!(keys(&cyclic), [Some(0), Some(1), Some(2), Some(3)]);
    assert_eq!(triggers(&cyclic), [1, 1, 1, 1]);
    // And it goes round: the next key after the fourth voice is the
    // first, which is the one released longest ago besides.
    cyclic.key_down(9, 64);
    assert_eq!(keys(&cyclic), [Some(9), Some(1), Some(2), Some(3)]);
}

#[test]
fn a_key_already_sounding_takes_its_own_voice_again() {
    for assign in [Assign::First, Assign::Cyclic] {
        let mut allocator = Allocator::<4>::new(Mode::Poly(assign));
        allocator.key_down(60, 64);
        allocator.key_down(62, 64);
        allocator.key_up(60);
        // Still dying away, struck again: the same voice, a new trigger.
        allocator.key_down(60, 127);
        assert_eq!(keys(&allocator), [Some(60), Some(62), None, None]);
        assert_eq!((triggers(&allocator), allocator.voices()[0].velocity), ([2, 1, 0, 0], 127));
        assert_eq!(stages(&allocator)[0], Stage::Held);
    }
}

#[test]
fn with_no_voice_at_rest_the_longest_released_goes_then_the_longest_held() {
    let mut allocator = Allocator::<4>::new(Mode::Poly(Assign::First));
    for key in [60, 61, 62, 63] {
        allocator.key_down(key, 64);
    }
    // Released in the order 62, 60: a fifth key takes 62's voice, a
    // sixth 60's.
    allocator.key_up(62);
    allocator.key_up(60);
    allocator.key_down(70, 64);
    assert_eq!(keys(&allocator), [Some(60), Some(61), Some(70), Some(63)]);
    allocator.key_down(71, 64);
    assert_eq!(keys(&allocator), [Some(71), Some(61), Some(70), Some(63)]);
    // All four held: the next takes the one held longest, 61, then 63,
    // then 70.
    for (key, expected) in [
        (72, [Some(71), Some(72), Some(70), Some(63)]),
        (73, [Some(71), Some(72), Some(70), Some(73)]),
        (74, [Some(71), Some(72), Some(74), Some(73)]),
    ] {
        allocator.key_down(key, 64);
        assert_eq!(keys(&allocator), expected);
    }
    assert_eq!(triggers(&allocator), [2, 2, 3, 2]);
    assert_eq!(allocator.sounding(), 4);
}

#[test]
fn the_pedal_holds_what_is_let_go_until_it_comes_up() {
    let mut allocator = Allocator::<3>::new(Mode::Poly(Assign::First));
    allocator.key_down(60, 64);
    allocator.sustain(true);
    allocator.key_down(64, 64);
    allocator.key_up(60);
    allocator.key_up(64);
    assert_eq!(stages(&allocator), [Stage::Sustained, Stage::Sustained, Stage::Rest]);
    assert!(allocator.voices()[0].gate() && allocator.voices()[1].gate());
    // A sustained key struck again is held again, on its own voice.
    allocator.key_down(64, 64);
    assert_eq!((stages(&allocator)[1], triggers(&allocator)), (Stage::Held, [1, 2, 0]));
    // Up: what is sustained is released; what is held stays.
    allocator.sustain(false);
    assert_eq!(stages(&allocator), [Stage::Released, Stage::Held, Stage::Rest]);
    // A sustained voice is stolen after the released and with the held,
    // by age.
    allocator.sustain(true);
    allocator.key_down(67, 64);
    allocator.key_up(67);
    allocator.key_down(70, 64);
    assert_eq!(keys(&allocator), [Some(70), Some(64), Some(67)]);
    // Everything up: every voice is released and the pedal is up too.
    allocator.all_keys_up();
    assert_eq!(stages(&allocator), [Stage::Released; 3]);
    allocator.key_down(72, 64);
    allocator.key_up(72);
    assert_eq!(stages(&allocator), [Stage::Released; 3]);
}

#[test]
fn a_monophonic_mode_sounds_the_key_its_priority_picks() {
    // Keys down in the order 60, 67, 55, 64; then up in the order 64,
    // 55, 67.
    let downs = [60, 67, 55, 64];
    let ups = [64, 55, 67];
    for (priority, after_downs, after_ups) in [
        (Priority::Last, [60, 67, 55, 64], [55, 67, 60]),
        (Priority::Low, [60, 60, 55, 55], [55, 60, 60]),
        (Priority::High, [60, 67, 67, 67], [67, 67, 60]),
    ] {
        let mut allocator = Allocator::<4>::new(Mode::Mono(priority, Trigger::Single));
        for (key, expected) in downs.into_iter().zip(after_downs) {
            allocator.key_down(key, key);
            assert_eq!(keys(&allocator), [Some(expected), None, None, None], "{priority:?} after {key} down");
            // The velocity is the sounding key's own.
            assert_eq!(allocator.voices()[0].velocity, expected);
        }
        for (key, expected) in ups.into_iter().zip(after_ups) {
            allocator.key_up(key);
            assert_eq!(keys(&allocator), [Some(expected), None, None, None], "{priority:?} after {key} up");
            assert_eq!((stages(&allocator)[0], allocator.voices()[0].velocity), (Stage::Held, expected));
        }
        // One trigger for the whole phrase: the first key's.
        assert_eq!(triggers(&allocator), [1, 0, 0, 0]);
        allocator.key_up(60);
        assert_eq!(stages(&allocator)[0], Stage::Released);
        // After a gap, the next key triggers.
        allocator.key_down(62, 64);
        assert_eq!(triggers(&allocator), [2, 0, 0, 0]);
    }
}

#[test]
fn multiple_trigger_starts_again_for_each_key_that_takes_the_note() {
    let mut allocator = Allocator::<2>::new(Mode::Mono(Priority::Last, Trigger::Multiple));
    allocator.key_down(60, 64);
    allocator.key_down(62, 64);
    allocator.key_down(64, 64);
    assert_eq!((keys(&allocator), triggers(&allocator)), ([Some(64), None], [3, 0]));
    // Handed back to a key still held, the note slides: no new trigger.
    allocator.key_up(64);
    assert_eq!((keys(&allocator), triggers(&allocator)), ([Some(62), None], [3, 0]));
    // With low-note priority a higher key does not take the note, and so
    // does not trigger; a lower one does both.
    let mut low = Allocator::<2>::new(Mode::Mono(Priority::Low, Trigger::Multiple));
    low.key_down(60, 64);
    low.key_down(65, 64);
    assert_eq!((keys(&low), triggers(&low)), ([Some(60), None], [1, 0]));
    low.key_down(55, 64);
    assert_eq!((keys(&low), triggers(&low)), ([Some(55), None], [2, 0]));
}

#[test]
fn unison_plays_the_one_note_on_every_voice() {
    let mut allocator = Allocator::<4>::new(Mode::Unison(Priority::Last, Trigger::Single));
    allocator.key_down(60, 80);
    assert_eq!((keys(&allocator), triggers(&allocator)), ([Some(60); 4], [1; 4]));
    allocator.key_down(72, 90);
    assert_eq!((keys(&allocator), triggers(&allocator)), ([Some(72); 4], [1; 4]));
    assert!(allocator.voices().iter().all(|voice| voice.velocity == 90 && voice.gate()));
    allocator.key_up(72);
    assert_eq!(keys(&allocator), [Some(60); 4]);
    allocator.key_up(60);
    assert_eq!(stages(&allocator), [Stage::Released; 4]);
    for voice in 0..4 {
        allocator.finished(voice);
    }
    assert_eq!(allocator.sounding(), 0);
}

#[test]
fn the_pedal_holds_a_monophonic_note_and_a_new_key_takes_it_over() {
    let mut allocator = Allocator::<2>::new(Mode::Mono(Priority::Last, Trigger::Single));
    allocator.sustain(true);
    allocator.key_down(60, 64);
    allocator.key_up(60);
    assert_eq!((stages(&allocator)[0], keys(&allocator)[0]), (Stage::Sustained, Some(60)));
    // The gate never fell: a single-trigger mode slides to the new key.
    allocator.key_down(64, 64);
    assert_eq!((stages(&allocator)[0], keys(&allocator)[0], triggers(&allocator)[0]), (Stage::Held, Some(64), 1));
    allocator.key_up(64);
    allocator.sustain(false);
    assert_eq!(stages(&allocator)[0], Stage::Released);
}

#[test]
fn more_keys_than_a_monophonic_mode_remembers_push_out_the_oldest() {
    let mut allocator = Allocator::<1>::new(Mode::Mono(Priority::Low, Trigger::Single));
    // The lowest key first, then one more than is remembered: the lowest
    // is forgotten, and the note is the lowest of the rest.
    let keys_down = HELD_KEYS as u8 + 1;
    for key in 0..keys_down {
        allocator.key_down(10 + key, 64);
    }
    assert_eq!(keys(&allocator), [Some(11)]);
    // Let go in turn, each hands the note to the next above.
    for key in 1..keys_down - 1 {
        allocator.key_up(10 + key);
        assert_eq!(keys(&allocator), [Some(11 + key)]);
    }
    // The forgotten key going up changes nothing; the last one releases.
    allocator.key_up(10);
    assert_eq!(stages(&allocator), [Stage::Held]);
    allocator.key_up(10 + keys_down - 1);
    assert_eq!(stages(&allocator), [Stage::Released]);
}

#[test]
fn changing_the_mode_releases_everything_and_starts_afresh() {
    let mut allocator = Allocator::<3>::new(Mode::Poly(Assign::Cyclic));
    allocator.key_down(60, 64);
    allocator.key_down(64, 64);
    allocator.sustain(true);
    allocator.set_mode(Mode::Mono(Priority::High, Trigger::Multiple));
    assert_eq!(allocator.mode(), Mode::Mono(Priority::High, Trigger::Multiple));
    assert_eq!(stages(&allocator), [Stage::Released, Stage::Released, Stage::Rest]);
    allocator.key_down(50, 64);
    assert_eq!((keys(&allocator)[0], stages(&allocator)[0]), (Some(50), Stage::Held));
    // And an instrument of no voices takes every call and does nothing.
    let mut none = Allocator::<0>::new(Mode::Poly(Assign::Cyclic));
    none.key_down(60, 64);
    none.key_up(60);
    none.set_mode(Mode::Unison(Priority::Low, Trigger::Single));
    none.key_down(60, 64);
    none.sustain(false);
    none.finished(0);
    assert_eq!(none.sounding(), 0);
}
