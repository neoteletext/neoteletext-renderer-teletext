//! synth6581 and synth8580 played through their adopted interface, ASID,
//! as the receiver plays them (`neoteletext-common/docs/audio/
//! synth6581-synth8580.md`, cited as "S6581-n"): blocks of timed events
//! given to the family's front end by the one block player every family
//! shares.

// Without synth6581's feature there is nothing here to test.
#![cfg(feature = "synth6581")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

use neoteletext_audio_instruments::BLOCK_SAMPLES;
use neoteletext_audio_instruments::midi::block::{BlockWriter, check};
use neoteletext_audio_instruments::midi::{
    Banks, ChannelMessage, Instrument, MOST_BLOCK_BYTES, Message, Played, play_block, sample_of_tick, status,
};
use neoteletext_audio_instruments::synth6581::interface::{
    self, Asid, FULL_UPDATE_IDS, MOST_MESSAGE_BYTES, MOST_REGISTER_DATA_A_BLOCK, RegisterData, Tracker,
};
use neoteletext_audio_instruments::synth6581::midi::Midi;
use neoteletext_audio_instruments::synth6581::{ChipClock, ENVELOPE_3, Synth6581, Variant};

/// The ASID ID of the mode and volume register (S6581-11).
const MODE_VOLUME_ID: u8 = 21;
/// The address of the mode and volume register (S6581-27).
const MODE_VOLUME: u8 = 0x18;
/// The ASID IDs of voice 3's control register, its first and second write.
const VOICE_3_CONTROL: u8 = 24;
const VOICE_3_CONTROL_SECOND: u8 = 27;
/// Control bits (S6581-30).
const GATE: u8 = 0x01;
const SAWTOOTH: u8 = 0x20;

/// A sounding chip's opening update: voice 3 a gated sawtooth near 440 Hz
/// on the PAL clock (F = 7,493, S6581-28), attack and decay at their
/// fastest, sustain at 8 of 15, release at its fastest; voice 1 a pulse
/// through the low-pass filter; full volume.
fn opening() -> RegisterData {
    let mut values = [0_u8; FULL_UPDATE_IDS];
    let set = |values: &mut [u8; FULL_UPDATE_IDS], id: u8, value: u8| values[usize::from(id)] = value;
    set(&mut values, 0, 0x45); // voice 1 frequency
    set(&mut values, 1, 0x1D);
    set(&mut values, 3, 0x08); // voice 1 pulse width, a square
    set(&mut values, 5, 0xF0); // voice 1 sustain at the peak
    set(&mut values, 12, 0x45); // voice 3 frequency
    set(&mut values, 13, 0x1D);
    set(&mut values, 17, 0x80); // voice 3 sustain 8
    set(&mut values, 19, 0x40); // cutoff
    set(&mut values, 20, 0xF1); // resonance, voice 1 filtered
    set(&mut values, MODE_VOLUME_ID, 0x1F); // low-pass, full volume
    set(&mut values, 22, 0x41); // voice 1 pulse, gated
    set(&mut values, VOICE_3_CONTROL, SAWTOOTH | GATE);
    RegisterData::full(&values)
}

/// The bytes after `0xF0` of `data`, as a block's event carries them.
fn sysex(data: &RegisterData) -> Vec<u8> {
    let mut out = [0; MOST_MESSAGE_BYTES];
    let length = data.write_midi(&mut out);
    out[1..length].to_vec()
}

/// One block of `events`, each a tick and the bytes after `0xF0` of a
/// system exclusive message or a channel message.
fn block(events: &[(u8, Message<'_>)]) -> Vec<u8> {
    let mut buffer = [0_u8; MOST_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    for (tick, message) in events {
        writer.push(*tick, message).unwrap();
    }
    writer.bytes().to_vec()
}

/// Plays `blocks` into a new chip of `model` on `clock`, giving every
/// sample and what each block's playing found.
fn play(model: Variant, clock: ChipClock, blocks: &[Vec<u8>]) -> (Vec<i16>, Vec<Played>) {
    let mut midi = Midi::new(model, clock);
    let banks = Banks::none();
    assert_eq!(midi.start(&banks), neoteletext_audio_instruments::midi::Outcome::Applied);
    let mut samples = Vec::new();
    let mut found = Vec::new();
    for bytes in blocks {
        let mut out = [0_i16; BLOCK_SAMPLES];
        found.push(play_block(&mut midi, bytes, &banks, &mut out));
        samples.extend_from_slice(&out);
    }
    (samples, found)
}

/// [`play`] on synth6581 and the PAL clock.
fn play_pal(blocks: &[Vec<u8>]) -> (Vec<i16>, Vec<Played>) {
    play(Variant::Synth6581, ChipClock::Pal, blocks)
}

/// The opening block, and `rest` silent blocks after it.
fn opened(rest: usize) -> Vec<Vec<u8>> {
    let opening = sysex(&opening());
    let mut blocks = vec![block(&[(0, Message::SystemExclusive(&opening))])];
    blocks.extend(core::iter::repeat_n(Vec::new(), rest));
    blocks
}

/// Whether `samples` are not one constant level: the chip is sounding.
fn sounding(samples: &[i16]) -> bool {
    samples.iter().any(|&sample| sample != samples[0])
}

/// xorshift64 (Marsaglia, "Xorshift RNGs", 2003, the triple 13, 7, 17).
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 32) as u8
    }
}

// Verifies S6581-2, S6581-7 and S6581-26: no event but a register data
// message writes a register. A block of every other event (channel
// messages, meta and escape events, real-time messages, a system exclusive
// message that is not ASID and every ASID command not taken) plays exactly
// as an empty block, and each is passed over and counted.
#[test]
fn s6581_2_only_register_data_writes_a_register() {
    let mut others: Vec<Vec<u8>> = Vec::new();
    for command in (0..=0x7F_u8).filter(|&command| !matches!(command, 0x4C..=0x4E)) {
        others.push(vec![0x2D, command, 0xF7]);
    }
    // A further chip's register update in 0x4E's own form (S6581-20).
    let mut further = sysex(&RegisterData::full(&[0xFF; FULL_UPDATE_IDS]));
    further[1] = 0x50;
    others.push(further);
    // Not ASID: a manufacturer's message (S6581-7).
    others.push(vec![0x43, 0x10, 0x01, 0xF7]);
    let mut events: Vec<(u8, Message<'_>)> = others.iter().map(|bytes| (0, Message::SystemExclusive(bytes))).collect();
    for kind in [
        status::NOTE_OFF,
        status::NOTE_ON,
        status::KEY_PRESSURE,
        status::CONTROL_CHANGE,
        status::PROGRAM_CHANGE,
        status::CHANNEL_PRESSURE,
        status::PITCH_BEND,
    ] {
        let one_byte = matches!(kind, status::PROGRAM_CHANGE | status::CHANNEL_PRESSURE);
        events.push((5, Message::Channel(ChannelMessage::new(kind, 0, 0x7F, if one_byte { 0 } else { 0x7F }))));
    }
    let mut busy = block(&events);
    // A meta event, an escape and a timing clock, which the writer never
    // writes (MIDI-8), appended by hand at a delta of 0.
    busy.extend_from_slice(&[0, 0xFF, 0x01, 0x02, b'h', b'i', 0, 0xF7, 0x01, 0x4E, 0, 0xF8]);
    let mut with = opened(0);
    with.push(busy);
    let (heard, found) = play_pal(&with);
    let (plain, _) = play_pal(&opened(1));
    assert!(sounding(&plain[BLOCK_SAMPLES..]));
    assert_eq!(heard, plain);
    assert_eq!(found[1].passed_over as usize, events.len() + 3);
    assert!(!found[1].malformed);
}

// Verifies S6581-9: of a message of every command 0x00 to 0x7F, only 0x4E
// changes a register; 0x4C and 0x4D are not counted; every other is counted
// once.
#[test]
fn s6581_9_every_command_value() {
    let mut volume = RegisterData::empty();
    volume.set(MODE_VOLUME_ID, 0x0F);
    let register_data = sysex(&volume);
    let others: Vec<Vec<u8>> = (0..=0x7F_u8).map(|command| vec![0x2D, command, 0xF7]).collect();
    let events: Vec<(u8, Message<'_>)> = (0..=0x7F_u8)
        .map(|command| {
            let bytes: &[u8] = if command == interface::command::REGISTER_DATA {
                &register_data
            } else {
                &others[usize::from(command)]
            };
            (0, Message::SystemExclusive(bytes))
        })
        .collect();
    let (heard, found) = play_pal(&[block(&events)]);
    assert_eq!(found[0].passed_over, 125);
    // The one register data message set the volume: a silent synth6581
    // steps when its volume moves (S6581-34, S6581-38).
    let (silent, _) = play_pal(&[Vec::new()]);
    assert_ne!(heard, silent);
}

// Verifies S6581-8 and S6581-14: a malformed register data message is
// dropped whole and counted, writing none of its registers; a block of
// random bytes never panics and renders 960 samples.
#[test]
fn s6581_8_malformed_messages_write_nothing() {
    let good = sysex(&opening());
    // One data byte short: the count no longer matches the mask.
    let mut short = good.clone();
    short.remove(short.len() - 2);
    let mut high = good.clone();
    high[12] = 0x80;
    let mut blocks = vec![block(&[(0, Message::SystemExclusive(&short))])];
    blocks[0].extend_from_slice(&[0, 0xF0, u8::try_from(high.len()).unwrap()]);
    blocks[0].extend_from_slice(&high);
    let (heard, found) = play_pal(&blocks);
    assert_eq!(found[0].passed_over, 2);
    assert!(!sounding(&heard));
    // The same message whole sounds: what was dropped would have.
    assert!(sounding(&play_pal(&opened(0)).0));
    let mut random = Random(0x6581_8580);
    for _ in 0..500 {
        let length = (random.next() % 1_005) as usize;
        let mut bytes: Vec<u8> = (0..length).map(|_| random.byte()).collect();
        // Half the blocks begin with a well-formed ASID head, so the
        // decoder itself is reached.
        if random.next().is_multiple_of(2) && bytes.len() > 4 {
            let length = bytes[2] & 0x3F;
            bytes[..4].copy_from_slice(&[0, 0xF0, length, 0x2D]);
        }
        let (samples, _) = play_pal(&[bytes]);
        assert_eq!(samples.len(), BLOCK_SAMPLES);
    }
}

// Verifies S6581-15: one message clearing voice 3's gate by ID 24 and
// setting it by ID 27 makes both writes, so the sustaining envelope begins
// a new attack; the two exchanged release it.
#[test]
fn s6581_15_both_control_writes_are_made() {
    let sustain = 136; // sustain 8 × 17 (S6581-31)
    let mut retrigger = RegisterData::empty();
    retrigger.set(VOICE_3_CONTROL, SAWTOOTH);
    retrigger.set(VOICE_3_CONTROL_SECOND, SAWTOOTH | GATE);
    let mut release = RegisterData::empty();
    release.set(VOICE_3_CONTROL, SAWTOOTH | GATE);
    release.set(VOICE_3_CONTROL_SECOND, SAWTOOTH);
    for (message, rises) in [(retrigger, true), (release, false)] {
        let mut midi = Midi::new(Variant::Synth6581, ChipClock::Pal);
        let banks = Banks::none();
        let mut out = [0_i16; BLOCK_SAMPLES];
        let _ = play_block(&mut midi, &opened(0)[0], &banks, &mut out);
        assert_eq!(midi.chip().read(ENVELOPE_3), sustain);
        let bytes = sysex(&message);
        let _ = midi.midi(&Message::SystemExclusive(&bytes), &banks);
        midi.render(&mut out[..24]);
        let level = midi.chip().read(ENVELOPE_3);
        assert_eq!(level > sustain, rises, "level {level}");
        assert_eq!(level < sustain, !rises, "level {level}");
    }
}

// Verifies S6581-18: a sounding chip plays sample for sample the same with
// and without start and stop messages in its blocks, and neither is
// counted.
#[test]
fn s6581_18_start_and_stop_change_nothing() {
    let start = [0x2D, 0x4C, 0xF7];
    let stop = [0x2D, 0x4D, 0xF7];
    let mut with = opened(0);
    with.push(block(&[(0, Message::SystemExclusive(&start)), (100, Message::SystemExclusive(&stop))]));
    let (heard, found) = play_pal(&with);
    let (plain, _) = play_pal(&opened(1));
    assert_eq!(heard, plain);
    assert_eq!(found[1].passed_over, 0);
    // A start or stop with a payload is malformed (S6581-18).
    let long = [0x2D, 0x4C, 0x00, 0xF7];
    let (_, found) = play_pal(&[block(&[(0, Message::SystemExclusive(&long))])]);
    assert_eq!(found[0].passed_over, 1);
}

// Verifies S6581-38: the same block played as each kind differs where it
// uses the filter.
#[test]
fn s6581_38_the_two_kinds_differ() {
    let (synth6581, _) = play(Variant::Synth6581, ChipClock::Pal, &opened(2));
    let (synth8580, _) = play(Variant::Synth8580, ChipClock::Pal, &opened(2));
    assert_ne!(synth6581, synth8580);
}

// Verifies S6581-39: the same blocks play otherwise on the NTSC clock.
#[test]
fn s6581_39_the_clock_changes_the_sound() {
    let (pal, _) = play(Variant::Synth6581, ChipClock::Pal, &opened(2));
    let (ntsc, _) = play(Variant::Synth6581, ChipClock::Ntsc, &opened(2));
    assert_ne!(pal, ntsc);
}

// Verifies S6581-40: a register data message at each tick 0 to 199 takes
// effect immediately before sample floor(time × 24 ÷ 5): the samples equal
// those of the chip written by hand there.
#[test]
fn s6581_40_a_message_lands_on_its_sample() {
    let mut volume = RegisterData::empty();
    volume.set(MODE_VOLUME_ID, 0x0F);
    let bytes = sysex(&volume);
    for tick in 0..=199_u8 {
        let (heard, _) = play_pal(&[block(&[(tick, Message::SystemExclusive(&bytes))])]);
        let at = sample_of_tick(tick);
        assert_eq!(at, usize::from(tick) * 24 / 5);
        let mut chip = Synth6581::new(Variant::Synth6581, ChipClock::Pal);
        let mut expected = [0_i16; BLOCK_SAMPLES];
        chip.render(&mut expected[..at]);
        chip.write(MODE_VOLUME, 0x0F);
        chip.render(&mut expected[at..]);
        assert_eq!(heard, expected, "tick {tick}");
    }
    // Two messages at one tick writing one register: the later wins.
    let mut quiet = RegisterData::empty();
    quiet.set(MODE_VOLUME_ID, 0x03);
    let first = sysex(&quiet);
    let (both, _) =
        play_pal(&[block(&[(50, Message::SystemExclusive(&first)), (50, Message::SystemExclusive(&bytes))])]);
    let (later, _) = play_pal(&[block(&[(50, Message::SystemExclusive(&bytes))])]);
    assert_eq!(both, later);
}

// Verifies S6581-42: a block's 21st register data message changes nothing
// and is counted, as is a second start message; the next block's budget is
// whole again.
#[test]
fn s6581_42_twenty_register_updates_a_block() {
    let mut off = RegisterData::empty();
    off.set(MODE_VOLUME_ID, 0);
    let mut on = RegisterData::empty();
    on.set(MODE_VOLUME_ID, 0x0F);
    let (off, on) = (sysex(&off), sysex(&on));
    let start = [0x2D, 0x4C, 0xF7];
    let mut events: Vec<(u8, Message<'_>)> =
        (0..MOST_REGISTER_DATA_A_BLOCK).map(|_| (0, Message::SystemExclusive(&off))).collect();
    events.push((0, Message::SystemExclusive(&on)));
    events.push((0, Message::SystemExclusive(&start)));
    events.push((0, Message::SystemExclusive(&start)));
    let full = block(&events);
    let next = block(&[(0, Message::SystemExclusive(&on))]);
    let (heard, found) = play_pal(&[full.clone(), next.clone()]);
    assert_eq!(found[0].passed_over, 2);
    assert_eq!(found[1].passed_over, 0);
    let (silent, _) = play_pal(&[Vec::new(), next]);
    assert_eq!(heard, silent);
    assert_ne!(heard[BLOCK_SAMPLES..], silent[..BLOCK_SAMPLES]);
}

// Verifies S6581-43: twenty register data messages of all 28 IDs and one
// start and one stop message are a block every check takes whole, within
// the family's 874 bytes. Only one delta time can take two bytes within a
// block's 200 ticks, so this block, at tick 128, is 853.
#[test]
fn s6581_43_the_largest_block() {
    let all = sysex(&RegisterData::from_values(&[Some(0xA5); 28]));
    let start = [0x2D, 0x4C, 0xF7];
    let stop = [0x2D, 0x4D, 0xF7];
    let mut events: Vec<(u8, Message<'_>)> =
        (0..MOST_REGISTER_DATA_A_BLOCK).map(|_| (128, Message::SystemExclusive(&all))).collect();
    events.push((128, Message::SystemExclusive(&start)));
    events.push((128, Message::SystemExclusive(&stop)));
    let largest = block(&events);
    assert_eq!(largest.len(), interface::MOST_BLOCK_BYTES - 21);
    assert_eq!(check(&largest), Ok(22));
    let (_, found) = play_pal(&[largest]);
    assert_eq!(found[0].passed_over, 0);
}

// Verifies S6581-47: a tune played with a full register update of the
// registers' present values inserted at a seeded tick of every block plays
// sample for sample as without.
#[test]
fn s6581_47_a_restatement_is_inaudible() {
    let mut random = Random(0x0047_6581);
    let first = sysex(&opening());
    let mut tracker = Tracker::new();
    tracker.take(&Asid::RegisterData(opening()));
    let mut plain = vec![block(&[(0, Message::SystemExclusive(&first))])];
    let mut restated = plain.clone();
    for _ in 0..40 {
        // Up to four partial updates a block, of random IDs and values,
        // the gates among them.
        let mut updates: Vec<(u8, Vec<u8>)> = Vec::new();
        let mut tick = 0_u8;
        for _ in 0..random.next() % 5 {
            tick = tick.saturating_add((random.next() % 50) as u8).min(199);
            let mut data = RegisterData::empty();
            for _ in 0..=random.next() % 4 {
                data.set((random.next() % 28) as u8, random.byte());
            }
            updates.push((tick, sysex(&data)));
        }
        let at = (random.next() % 200) as u8;
        let mut with: Vec<(u8, Vec<u8>)> = Vec::new();
        let mut inserted = false;
        for (time, bytes) in &updates {
            if !inserted && *time >= at {
                with.push((at, sysex(&tracker.restatement())));
                inserted = true;
            }
            tracker.take(&interface::read(bytes).unwrap());
            with.push((*time, bytes.clone()));
        }
        if !inserted {
            with.push((at, sysex(&tracker.restatement())));
        }
        let messages = |list: &[(u8, Vec<u8>)]| -> Vec<u8> {
            let events: Vec<(u8, Message<'_>)> =
                list.iter().map(|(time, bytes)| (*time, Message::SystemExclusive(bytes))).collect();
            block(&events)
        };
        plain.push(messages(&updates));
        restated.push(messages(&with));
    }
    for model in [Variant::Synth6581, Variant::Synth8580] {
        let heard = play(model, ChipClock::Pal, &plain).0;
        assert!(sounding(&heard));
        assert_eq!(heard, play(model, ChipClock::Pal, &restated).0);
    }
}

// Verifies S6581-48: a lost block leaves the registers sounding: the chip
// told input was lost renders as one given an empty block.
#[test]
fn s6581_48_a_lost_block_sounds_on() {
    let banks = Banks::none();
    let mut lost = Midi::new(Variant::Synth6581, ChipClock::Pal);
    let mut kept = Midi::new(Variant::Synth6581, ChipClock::Pal);
    let mut out = [0_i16; BLOCK_SAMPLES];
    let _ = play_block(&mut lost, &opened(0)[0], &banks, &mut out);
    let _ = play_block(&mut kept, &opened(0)[0], &banks, &mut out);
    lost.lost();
    let mut concealed = [0_i16; BLOCK_SAMPLES];
    lost.render(&mut concealed);
    let _ = play_block(&mut kept, &[], &banks, &mut out);
    assert_eq!(concealed, out);
    assert!(concealed.iter().any(|&sample| sample != concealed[0]));
}

// Verifies S6581-50 through the front end: a patch, packed, plays as the
// full register update it is.
#[test]
fn s6581_50_a_patch_plays_as_its_update() {
    let mut patch = interface::Patch::default();
    for (id, value) in (0_u8..).zip(&mut patch.values) {
        *value = opening().get(id).unwrap();
    }
    let packed = interface::pack_patch(&patch);
    let (heard, _) = play_pal(&[block(&[(0, Message::SystemExclusive(&packed[1..]))])]);
    assert!(sounding(&heard));
    assert_eq!(heard, play_pal(&opened(0)).0);
}
