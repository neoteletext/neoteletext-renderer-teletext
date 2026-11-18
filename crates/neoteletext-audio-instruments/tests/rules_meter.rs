//! The envelope position and the voice count a page's rules read
//! (`neoteletext-common/docs/page-rules.md`, RULE-31; rdm REQ-6527), and a
//! local sound's instrument (RULE-60, RULE-62): for every family, the
//! stages a note passes through and the level at rest.

// Without every family there is nothing here to test.
#![cfg(all(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

#[path = "rules_support.rs"]
mod support;

use neoteletext_audio_instruments::local::Local;
use neoteletext_audio_instruments::midi::{Banks, Instrument, Meter, MeterStage, Outcome, family};
use neoteletext_audio_instruments::watch::fields;
use neoteletext_audio_instruments::{SAMPLE_RATE, synth8, synth6581};
use support::{KINDS, Kind, Sent, note_kinds, note_off, note_on, set_field};

/// The samples rendered between two readings of the meter: a millisecond.
const STEP: usize = SAMPLE_RATE as usize / 1_000;
/// The longest a stage of the tests' envelopes is waited for: ten seconds.
const MOST_STEPS: usize = 10_000;

/// The number of the field of `kind` named `name` (appendix A).
fn field(kind: Kind, name: &str) -> u16 {
    let table = fields(kind.code, kind.variant).unwrap();
    u16::try_from(table.iter().position(|field| field.name == name).unwrap()).unwrap()
}

/// The messages that give `kind` an amplitude envelope with an attack, a
/// decay to a sustain level below the peak and a release, each long enough
/// to be met by a reading a millisecond: the family's own fields.
fn envelope(kind: Kind) -> Vec<Sent> {
    let set = |name: &str, value: i32| set_field(kind, 0, field(kind, name), value);
    let named: &[(&str, i32)] = match kind.code {
        // Operator 1's envelope generator: a first rate that rises, a
        // second and third that fall to a third level below the first, and
        // a fourth that falls to nothing.
        family::SYNTH7 => &[
            ("op1_eg_rate_1", 60),
            ("op1_eg_rate_2", 50),
            ("op1_eg_rate_3", 50),
            ("op1_eg_rate_4", 50),
            ("op1_eg_level_1", 99),
            ("op1_eg_level_2", 85),
            ("op1_eg_level_3", 70),
            ("op1_eg_level_4", 0),
        ],
        family::SYNTH106 => &[("attack", 40), ("decay", 40), ("sustain", 64), ("release", 40)],
        family::SYNTHD => &[
            ("loudness_attack_time", 1_000),
            ("loudness_decay_time", 600),
            ("loudness_sustain_level", 8_000),
            ("decay_switch", 127),
        ],
        family::SYNTH53 => &[("attack_vca", 40), ("decay_vca", 40), ("sustain_vca", 60), ("release_vca", 40)],
        family::SYNTH8 => &[("env2_attack", 40), ("env2_decay", 40), ("env2_sustain", 64), ("env2_release", 40)],
        family::SYNTH80 => &[
            ("channel_1_amplifier_attack", 40),
            ("channel_1_amplifier_decay", 40),
            ("channel_1_amplifier_sustain", 64),
            ("channel_1_amplifier_release", 40),
        ],
        _ => &[],
    };
    named.iter().flat_map(|&(name, value)| set(name, value)).collect()
}

/// Renders `instrument` a millisecond at a time until `until` says so of
/// its meter, or [`MOST_STEPS`] have passed, and answers the stages its
/// part 0 was read in, each once in the order met, with the meter of each
/// first reading.
fn stages(instrument: &mut dyn Instrument, until: impl Fn(Meter) -> bool) -> Vec<Meter> {
    let mut met: Vec<Meter> = Vec::new();
    let mut pcm = [0_i16; STEP];
    for _ in 0..MOST_STEPS {
        let meter = instrument.meter(0);
        if met.last().is_none_or(|last| last.stage != meter.stage) {
            met.push(meter);
        }
        if until(meter) {
            break;
        }
        instrument.render(&mut pcm);
    }
    met
}

fn codes(met: &[Meter]) -> Vec<u8> {
    met.iter().map(|meter| meter.stage.code()).collect()
}

// Verifies RULE-31 (REQ-6527): for each family that plays notes, after a
// note on the stage passes attack, decay and sustain in order, and release
// after the note off; the level is 0 when idle, and the voices are counted.
#[test]
fn a_note_passes_attack_decay_sustain_and_release_in_every_family() {
    let banks = Banks::none();
    for kind in note_kinds() {
        let mut local = Local::new(kind.code, kind.variant, 0, 0).unwrap();
        let setup: Vec<u8> = envelope(kind).iter().flat_map(Sent::event).collect();
        let played = local.start_with(&banks, &setup);
        assert_eq!((played.passed_over, played.malformed), (0, false), "{}: the setup is the family's own", kind.name);
        assert_eq!(local.meter(0), Meter::IDLE, "{}: at rest before any note", kind.name);
        assert_eq!(local.midi(&note_on(0, 60, 100).message(), &banks), Outcome::Applied);
        let held = stages(&mut local, |meter| meter.stage == MeterStage::Sustain);
        assert_eq!(codes(&held), [1, 2, 3], "{}: {held:?}", kind.name);
        assert!(held.iter().all(|meter| meter.voices == 1), "{}: one voice sounds", kind.name);
        let (decay, sustain) = (held[1].level, held[2].level);
        assert!(decay > sustain && sustain > 0, "{}: the decay falls from {decay} to {sustain}", kind.name);
        local.midi(&note_off(0, 60).message(), &banks);
        let let_go = stages(&mut local, |meter| meter.stage == MeterStage::Idle);
        assert_eq!(codes(&let_go), [4, 0], "{}: {let_go:?}", kind.name);
        assert!(let_go[0].level <= sustain, "{}: the release falls from the sustain", kind.name);
        let idle = let_go[1];
        assert_eq!((idle.level, idle.stage), (0, MeterStage::Idle), "{}: the level is 0 when idle", kind.name);
        // Every voice is at rest once the sound has died away.
        stages(&mut local, |meter| meter.voices == 0);
        assert_eq!(local.meter(0), Meter::IDLE, "{}", kind.name);
    }
}

// Verifies RULE-31 (REQ-6527): the newest voice that is not idle is the one
// read, and the count is of every voice not idle.
#[test]
fn the_newest_sounding_voice_is_read_and_every_voice_counted() {
    let banks = Banks::none();
    for kind in note_kinds() {
        // synthD is one voice.
        let most = if kind.code == family::SYNTHD { 1 } else { 2 };
        let mut local = Local::new(kind.code, kind.variant, 0, 0).unwrap();
        let setup: Vec<u8> = envelope(kind).iter().flat_map(Sent::event).collect();
        local.start_with(&banks, &setup);
        local.midi(&note_on(0, 60, 100).message(), &banks);
        stages(&mut local, |meter| meter.stage == MeterStage::Sustain);
        local.midi(&note_on(0, 67, 100).message(), &banks);
        let mut pcm = [0_i16; STEP];
        local.render(&mut pcm);
        let meter = local.meter(0);
        assert_eq!(meter.voices, most, "{}", kind.name);
        if most == 2 {
            assert_eq!(meter.stage, MeterStage::Attack, "{}: the newer note's envelope, not the held one's", kind.name);
        }
        // Released, every note dies away; the release-all of a local
        // sound's end does the same.
        local.release_all();
        stages(&mut local, |meter| meter.voices == 0 && meter.stage == MeterStage::Idle);
        assert_eq!(local.meter(0), Meter::IDLE, "{}", kind.name);
    }
}

// Verifies RULE-31 (REQ-6527): synth7's parts are metered apart, and
// synth8's part 0 is its upper patch and part 1 its lower.
#[test]
fn parts_are_metered_apart() {
    let banks = Banks::none();
    let mut synth7 = Local::new(family::SYNTH7, 0, 0, 0).unwrap();
    synth7.start_with(&banks, &[]);
    synth7.midi(&note_on(5, 60, 100).message(), &banks);
    assert_ne!(synth7.meter(5).stage, MeterStage::Idle);
    assert_eq!((synth7.meter(4).stage, synth7.meter(4).voices), (MeterStage::Idle, 1));
    // synth8 split at key 60: a key below plays the lower patch, part 1.
    let mut split = Local::new(family::SYNTH8, 0, synth8::interface::KEY_MODE_SPLIT, 60).unwrap();
    split.start_with(&banks, &[]);
    split.midi(&note_on(0, 40, 100).message(), &banks);
    let mut pcm = [0_i16; STEP];
    split.render(&mut pcm);
    assert_eq!(split.meter(0).stage, MeterStage::Idle, "the upper half is at rest");
    assert_ne!(split.meter(1).stage, MeterStage::Idle, "the lower half sounds");
    assert_eq!((split.meter(0).voices, split.meter(2)), (1, Meter { voices: 1, ..Meter::IDLE }));
    split.midi(&note_on(0, 72, 100).message(), &banks);
    split.render(&mut pcm);
    assert_ne!(split.meter(0).stage, MeterStage::Idle);
    assert_eq!(split.meter(0).voices, 2);
}

// Verifies RULE-31 (REQ-6527): the synth6581 family takes the voice as the
// part, and reads its envelope generator's 8-bit counter as it stands.
#[test]
fn the_synth6581_familys_voices_are_its_parts() {
    let banks = Banks::none();
    let kind = KINDS[0];
    let mut chip = synth6581::midi::Midi::new(synth6581::Variant::Synth6581, synth6581::ChipClock::Pal);
    chip.start(&banks);
    assert_eq!([chip.meter(0), chip.meter(1), chip.meter(2), chip.meter(3)], [Meter::IDLE; 4]);
    let write = |chip: &mut synth6581::midi::Midi, name: &str, value: i32| {
        for sent in set_field(kind, 0, field(kind, name), value) {
            assert_eq!(chip.midi(&sent.message(), &banks), Outcome::Applied);
        }
    };
    // Voice 2: attack 5, decay 5, sustain 8, release 5; then its gate with
    // the triangle (data sheet, registers 4 to 6).
    write(&mut chip, "voice2_attack_decay", 0x55);
    write(&mut chip, "voice2_sustain_release", 0x85);
    write(&mut chip, "voice2_control", 0x11);
    let mut pcm = [0_i16; STEP];
    let mut met: Vec<Meter> = Vec::new();
    for _ in 0..MOST_STEPS {
        let meter = chip.meter(1);
        if met.last().is_none_or(|last| last.stage != meter.stage) {
            met.push(meter);
        }
        if meter.stage == MeterStage::Sustain {
            break;
        }
        chip.render(&mut pcm);
    }
    assert_eq!(codes(&met), [1, 2, 3], "{met:?}");
    // Sustain 8 of 15 is the counter at 8 x 17.
    assert_eq!((met[2].level, met[2].voices), (0x88, 1));
    assert_eq!((chip.meter(0).stage, chip.meter(2).stage), (MeterStage::Idle, MeterStage::Idle));
    write(&mut chip, "voice2_control", 0x10);
    let mut after: Vec<u8> = Vec::new();
    for _ in 0..MOST_STEPS {
        let meter = chip.meter(1);
        if after.last() != Some(&meter.stage.code()) {
            after.push(meter.stage.code());
        }
        if meter.stage == MeterStage::Idle {
            break;
        }
        chip.render(&mut pcm);
    }
    assert_eq!(after, [4, 0]);
    assert_eq!(chip.meter(1), Meter::IDLE);
}

// Verifies RULE-60 and RULE-62 (REQ-6527): a local sound's instrument is
// made for every family that plays notes and for none other; its setup is
// applied before any sample; a note given before a block of 128 samples
// sounds within that block; and only the messages a rule may send are
// taken.
#[test]
fn a_local_sounds_instrument_is_made_set_up_and_played() {
    /// The samples of a block of a pulled voice (RULE-62,
    /// `neoteletext_audio_mix::LOCAL_BLOCK_SAMPLES`).
    const BLOCK: usize = 128;
    let banks = Banks::none();
    for kind in note_kinds() {
        let mut local = Local::new(kind.code, kind.variant, 0, 0).unwrap();
        local.start_with(&banks, &[]);
        let mut block = [0_i16; BLOCK];
        local.render(&mut block);
        assert!(block.iter().all(|&sample| sample == 0), "{}: silent before any note", kind.name);
        assert_eq!(local.channel_message(0x90, 60, 100, &banks), Outcome::Applied, "{}", kind.name);
        // The attack of the family's default patch is heard in the block
        // the message came before, or in the few after it: no buffer is
        // held ahead.
        let mut heard = None;
        for index in 0..8 {
            local.render(&mut block);
            if block.iter().any(|&sample| sample != 0) {
                heard = Some(index);
                break;
            }
        }
        assert_eq!(heard, Some(0), "{}: the note sounds in the first block after it", kind.name);
        assert_eq!(local.meter(0).voices, 1);
        // What a rule's midi action may not carry is passed over (RULE-21).
        for (status, data1, data2) in
            [(0xA0, 60, 1), (0xD0, 1, 0), (0xF0, 1, 2), (0xF8, 0, 0), (0x90, 128, 1), (0x90, 1, 128), (0x10, 1, 1)]
        {
            assert_eq!(
                local.channel_message(status, data1, data2, &banks),
                Outcome::PassedOver,
                "{}: {status:#X}",
                kind.name
            );
        }
        assert_eq!(local.channel_message(0x80, 60, 0, &banks), Outcome::Applied);
        // A program change's second byte is not given to the instrument.
        assert_ne!(local.channel_message(0xC0, 0, 99, &banks), Outcome::PatchRefused);
        // Rendering in other lengths gives the same samples.
        let mut whole = Local::new(kind.code, kind.variant, 0, 0).unwrap();
        let mut cut = whole.clone();
        for instrument in [&mut whole, &mut cut] {
            instrument.start_with(&banks, &[]);
            instrument.channel_message(0x90, 64, 90, &banks);
        }
        let (mut one, mut pieces) = ([0_i16; 4 * BLOCK], [0_i16; 4 * BLOCK]);
        whole.render(&mut one);
        for piece in pieces.chunks_mut(BLOCK) {
            cut.render(piece);
        }
        assert_eq!(one, pieces, "{}", kind.name);
    }
    // The synth6581 family has no notes and no local sound; nor has a code
    // that is no family's, a variant or a key mode there is none of.
    for (code, variant, option) in [
        (family::SYNTH6581, 0, 0),
        (12, 1, 0),
        (19, 0, 0),
        (0, 0, 0),
        (family::SYNTH7, 1, 0),
        (family::SYNTH106, 2, 0),
        (family::SYNTH53, 3, 0),
        (family::SYNTH8, 0, 3),
        (family::SYNTH80, 1, 0),
    ] {
        assert!(Local::new(code, variant, option, 0).is_none(), "{code} {variant} {option}");
    }
    // A setup's program change reads the sound's bank, as a block's does:
    // a faulty patch is counted.
    let image = [0xFF_u8; 2 * synth8::PATCH_BYTES];
    let images = [&image[..]];
    let faulty = Banks::new(&images);
    let mut local = Local::new(family::SYNTH8, 0, 0, 0).unwrap();
    let played = local.start_with(&faulty, &[0, 0xC0, 1]);
    assert_eq!((played.events, played.patches_refused), (1, 2), "program 0 at the start, and program 1");
}
