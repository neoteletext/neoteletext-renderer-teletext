//! The rack: sixteen parts, one for each MIDI channel, sharing sixteen
//! notes, driven by MIDI messages and rendered to samples.
//!
//! # Gain
//!
//! A note's samples are the sum of its carriers, each attenuated for the
//! number of carriers, so a note at the patch's full output level (99) with
//! no velocity sensitivity peaks at about ±8,186, whatever its algorithm;
//! velocity sensitivity at the hardest velocity can raise that to about
//! ±15,000. The notes are added and the sum is shifted right by
//! [`MIX_SHIFT`]: a full-level note is 24 decibels below full scale, and
//! sixteen of them exactly in phase just reach it. The sum goes through the
//! output's limiter ([`crate::output::limit`]), which bends what is near
//! full scale under it and never wraps.
//!
//! # Time
//!
//! Envelopes, the low frequency oscillators and pitch advance once for
//! each [`BLOCK_SAMPLES`] samples, and levels glide between. Blocks are
//! counted across calls to [`Rack::render`], so the length of a call does
//! not matter. A message takes effect at the next block: at most 0.67
//! milliseconds later, and at once when each call renders a multiple of 32
//! samples, as the 960 of an instrument block are.

use super::kernel::BLOCK_SAMPLES;
use super::msfa::Q24_ONE;
use super::msfa::lfo::{Lfo, Waveform};
use super::note::{Controls, Note};
use super::patch::{BANK_VOICES, Bank, Patch};
use super::tables::VOLUME_ATTENUATION;

/// Parts of the rack: one for each MIDI channel.
///
/// Source: MIDI 1.0: a channel is the low four bits of a status byte.
pub const PARTS: usize = 16;

/// Notes that sound at once, across all parts: the instrument's polyphony.
///
/// Source: Shirriff, "Reverse-engineering the Yamaha DX7 synthesizer's
/// sound chip": "The DX7 supports 16 simultaneous notes".
pub const NOTES: usize = 16;

/// The right shift of the sum of all notes that makes a sample.
///
/// Source: own choice, explained under "Gain" above.
pub const MIX_SHIFT: u32 = 2;

/// How far the pitch bend wheel bends at either end, in semitones.
///
/// Source: own choice: the usual default of a MIDI instrument. On the
/// instrument it is a function parameter of 0 to 12, kept outside the
/// voice.
pub const PITCH_BEND_SEMITONES: i32 = 2;

/// The pitch bend value that bends nothing.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;

/// A pitch bend, less its centre, times this and shifted right by
/// [`PITCH_BEND_SHIFT`], is the bend in Q24 of an octave:
/// `PITCH_BEND_SEMITONES / 12 × 2^24 / 8192 × 2^7`, rounded.
const PITCH_BEND_SCALE: i32 = (2 * PITCH_BEND_SEMITONES * (Q24_ONE >> 6) + 12) / (2 * 12);

/// See [`PITCH_BEND_SCALE`].
const PITCH_BEND_SHIFT: u32 = 7;

/// The largest value of a MIDI data byte.
const DATA_MAX: u8 = 0x7f;

/// The volume of a part before any message sets it: no attenuation.
///
/// Source: own choice: the instrument at full volume.
const VOLUME_DEFAULT: u8 = DATA_MAX;

/// The sustain pedal is down at this value and above.
///
/// Source: MIDI 1.0: a switch controller is on from 64.
const PEDAL_DOWN: u8 = 64;

/// MIDI status: note off.
const STATUS_NOTE_OFF: u8 = 0x80;
/// MIDI status: note on.
const STATUS_NOTE_ON: u8 = 0x90;
/// MIDI status: control change.
const STATUS_CONTROL_CHANGE: u8 = 0xb0;
/// MIDI status: program change.
const STATUS_PROGRAM_CHANGE: u8 = 0xc0;
/// MIDI status: pitch bend.
const STATUS_PITCH_BEND: u8 = 0xe0;
/// Controller: modulation wheel.
const CONTROL_MODULATION_WHEEL: u8 = 1;
/// Controller: volume.
const CONTROL_VOLUME: u8 = 7;
/// Controller: sustain pedal.
const CONTROL_SUSTAIN: u8 = 64;
/// Controller: all sound off.
const CONTROL_ALL_SOUND_OFF: u8 = 120;
/// Controller: reset all controllers.
const CONTROL_RESET_CONTROLLERS: u8 = 121;
/// Controller: all notes off.
const CONTROL_ALL_NOTES_OFF: u8 = 123;

/// Where each part's sample-and-hold sequence starts: part `n` starts from
/// `SEED + n × SEED_STEP`, in eight bits.
///
/// Source: own choice. The sequence itself is the parameter layer's.
const SEED: u8 = 0x5a;
/// See [`SEED`]. Odd, so that the sixteen parts differ.
const SEED_STEP: u8 = 0x1d;

/// The life of a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Not sounding.
    Free,
    /// Its key is down.
    Held,
    /// Its key is up and the sustain pedal holds it.
    Sustained,
    /// Its key is up and it is dying away.
    Released,
}

/// One of the sixteen notes and whose it is.
#[derive(Clone, Copy, Debug)]
struct Slot {
    note: Note,
    stage: Stage,
    /// Started since the last block began: it sounds from the next.
    pending: bool,
    part: u8,
    key: u8,
    /// When it started, on the rack's clock of messages.
    started: u64,
    /// When it was released.
    released: u64,
}

impl Slot {
    const fn new() -> Self {
        Self { note: Note::new(), stage: Stage::Free, pending: false, part: 0, key: 0, started: 0, released: 0 }
    }
}

/// One part: a MIDI channel's patch and controllers.
#[derive(Clone, Copy, Debug)]
struct Part {
    program: u8,
    /// The pitch bend, less its centre.
    bend: i32,
    wheel: u8,
    volume: u8,
    sustain: bool,
    lfo: Lfo,
}

impl Part {
    fn new(index: usize) -> Self {
        let seed = SEED.wrapping_add(SEED_STEP.wrapping_mul(index.to_le_bytes()[0]));
        Self { program: 0, bend: 0, wheel: 0, volume: VOLUME_DEFAULT, sustain: false, lfo: Lfo::new(seed) }
    }

    /// Sets the oscillator from the part's patch.
    fn load(&mut self, patch: &Patch) {
        let params = patch.params();
        let waveform = Waveform::from_number(params.lfo_wave).unwrap_or(Waveform::Triangle);
        self.lfo.reset(params.lfo_speed, params.lfo_delay, params.lfo_key_sync != 0, waveform);
    }
}

/// Sixteen parts of the instrument sharing sixteen notes.
///
/// It is about 12 kilobytes, holds no reference and allocates nothing. Two
/// racks given the same messages and rendered in calls of the same lengths
/// give the same samples, whatever else exists.
#[derive(Clone, Debug)]
pub struct Rack {
    bank: Bank,
    parts: [Part; PARTS],
    slots: [Slot; NOTES],
    /// Counts messages, to order notes by age.
    clock: u64,
    /// Samples left of the block being rendered.
    block_remaining: usize,
    ignored: u64,
}

impl Default for Rack {
    fn default() -> Self {
        Self::new()
    }
}

impl Rack {
    /// A rack with an empty bank: every part plays [`Patch::DEFAULT`].
    #[must_use]
    pub fn new() -> Self {
        let mut rack = Self {
            bank: Bank::new(),
            parts: core::array::from_fn(Part::new),
            slots: [Slot::new(); NOTES],
            clock: 0,
            block_remaining: 0,
            ignored: 0,
        };
        rack.load_parts();
        rack
    }

    /// Gives the rack a bank, which it copies. Notes that are sounding keep
    /// the patch they started with; each part's low frequency oscillator
    /// takes the settings of the part's program in the new bank.
    pub fn set_bank(&mut self, bank: &Bank) {
        self.bank = *bank;
        self.load_parts();
    }

    /// Puts the rack back as [`Rack::new`] made it, but for the bank, which
    /// it keeps: silent, every part at program 0 with its controllers at
    /// rest, the count of ignored messages at zero.
    pub fn reset(&mut self) {
        let bank = self.bank;
        *self = Self::new();
        self.set_bank(&bank);
    }

    /// How many messages [`Rack::event`] has ignored since the rack was
    /// made or reset.
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// How many notes are sounding, 0 to 16.
    #[must_use]
    pub fn sounding_notes(&self) -> usize {
        self.slots.iter().filter(|slot| slot.stage != Stage::Free).count()
    }

    /// Whether a note of `part` (0 to 15) on `key` is sounding: held by its
    /// key or the pedal, or dying away.
    #[must_use]
    pub fn is_sounding(&self, part: u8, key: u8) -> bool {
        self.slots.iter().any(|slot| slot.stage != Stage::Free && slot.part == part && slot.key == key)
    }

    fn load_parts(&mut self) {
        for part in &mut self.parts {
            part.load(self.bank.get(usize::from(part.program)).unwrap_or(&Patch::DEFAULT));
        }
    }

    /// Takes one MIDI 1.0 channel voice message. The low four bits of
    /// `status` choose the part.
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n).
    /// - Control change (0xBn) of the modulation wheel (1), volume (7), the
    ///   sustain pedal (64, down from 64), all sound off (120), reset all
    ///   controllers (121, which leaves the volume) and all notes off (123).
    /// - Program change (0xCn) to a program of 0 to 31, which chooses the
    ///   patch of the notes the part starts afterwards; `data2` is not
    ///   looked at.
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven.
    ///
    /// Anything else is ignored and counted ([`Rack::ignored_events`]): any
    /// other status or controller, a program above 31, a data byte above
    /// 127. Nothing is an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        self.clock = self.clock.saturating_add(1);
        let part = status & 0x0f;
        let kind = status & 0xf0;
        let two_data_bytes = kind != STATUS_PROGRAM_CHANGE;
        if data1 > DATA_MAX || (two_data_bytes && data2 > DATA_MAX) {
            self.ignore();
            return;
        }
        match kind {
            STATUS_NOTE_OFF => self.note_off(part, data1),
            STATUS_NOTE_ON if data2 == 0 => self.note_off(part, data1),
            STATUS_NOTE_ON => self.note_on(part, data1, data2),
            STATUS_CONTROL_CHANGE => self.control_change(part, data1, data2),
            STATUS_PROGRAM_CHANGE => self.program_change(part, data1),
            STATUS_PITCH_BEND => {
                if let Some(part) = self.parts.get_mut(usize::from(part)) {
                    part.bend = (i32::from(data2) << 7 | i32::from(data1)).saturating_sub(PITCH_BEND_CENTRE);
                }
            }
            _ => self.ignore(),
        }
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn note_on(&mut self, part_number: u8, key: u8, velocity: u8) {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else {
            return;
        };
        let sounding = |slot: &&mut Slot| slot.stage != Stage::Free && slot.part == part_number;
        // The instrument starts the oscillator's delay, and its waveform if
        // the patch says so, when a key goes down with no other held.
        if !self.slots.iter().any(|slot| slot.stage == Stage::Held && slot.part == part_number) {
            part.lfo.keydown();
        }
        let patch = self.bank.get(usize::from(part.program)).unwrap_or(&Patch::DEFAULT);

        // In order: the note already sounding on this key of this part; a
        // free note; the note released longest ago; the oldest note held.
        let again = self.slots.iter_mut().find(|slot| sounding(slot) && slot.key == key);
        let slot = match again {
            Some(slot) => Some(slot),
            None => self.slots.iter_mut().min_by_key(|slot| match slot.stage {
                Stage::Free => (0_u8, 0),
                Stage::Released => (1, slot.released),
                Stage::Held | Stage::Sustained => (2, slot.started),
            }),
        };
        if let Some(slot) = slot {
            slot.note.start(patch, key, velocity, slot.stage != Stage::Free);
            slot.stage = Stage::Held;
            slot.pending = true;
            slot.part = part_number;
            slot.key = key;
            slot.started = self.clock;
        }
    }

    fn note_off(&mut self, part_number: u8, key: u8) {
        let sustain = self.parts.get(usize::from(part_number)).is_some_and(|part| part.sustain);
        let clock = self.clock;
        for slot in &mut self.slots {
            if slot.stage == Stage::Held && slot.part == part_number && slot.key == key {
                if sustain {
                    slot.stage = Stage::Sustained;
                } else {
                    slot.stage = Stage::Released;
                    slot.released = clock;
                    slot.note.release();
                }
            }
        }
    }

    /// Releases the notes of a part that are in one of `stages`.
    fn release_part(&mut self, part_number: u8, stages: &[Stage]) {
        let clock = self.clock;
        for slot in &mut self.slots {
            if slot.part == part_number && stages.contains(&slot.stage) {
                slot.stage = Stage::Released;
                slot.released = clock;
                slot.note.release();
            }
        }
    }

    fn control_change(&mut self, part_number: u8, controller: u8, value: u8) {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else {
            return;
        };
        match controller {
            CONTROL_MODULATION_WHEEL => part.wheel = value,
            CONTROL_VOLUME => part.volume = value,
            CONTROL_SUSTAIN => {
                part.sustain = value >= PEDAL_DOWN;
                if !part.sustain {
                    self.release_part(part_number, &[Stage::Sustained]);
                }
            }
            CONTROL_ALL_SOUND_OFF => {
                let clock = self.clock;
                for slot in &mut self.slots {
                    if slot.stage != Stage::Free && slot.part == part_number {
                        slot.stage = Stage::Released;
                        slot.released = clock;
                        slot.note.cut();
                    }
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                part.wheel = 0;
                part.bend = 0;
                part.sustain = false;
                self.release_part(part_number, &[Stage::Sustained]);
            }
            CONTROL_ALL_NOTES_OFF => {
                // As a note off for every key: the pedal still holds.
                if part.sustain {
                    for slot in &mut self.slots {
                        if slot.stage == Stage::Held && slot.part == part_number {
                            slot.stage = Stage::Sustained;
                        }
                    }
                } else {
                    self.release_part(part_number, &[Stage::Held]);
                }
            }
            _ => self.ignore(),
        }
    }

    fn program_change(&mut self, part_number: u8, program: u8) {
        if usize::from(program) >= BANK_VOICES {
            self.ignore();
            return;
        }
        if let Some(part) = self.parts.get_mut(usize::from(part_number)) {
            part.program = program;
            part.load(self.bank.get(usize::from(program)).unwrap_or(&Patch::DEFAULT));
        }
    }

    /// Releases every note of every part, held by a key or by a pedal, and
    /// lifts every pedal: for when the input stops and no note off will
    /// come.
    pub fn release_all(&mut self) {
        self.clock = self.clock.saturating_add(1);
        for part in &mut self.parts {
            part.sustain = false;
        }
        let clock = self.clock;
        for slot in &mut self.slots {
            if matches!(slot.stage, Stage::Held | Stage::Sustained) {
                slot.stage = Stage::Released;
                slot.released = clock;
                slot.note.release();
            }
        }
    }

    /// Starts a block: advances every part's oscillator and every note's
    /// envelopes, and frees the notes that have ended.
    fn control(&mut self) {
        let mut controls = [Controls { lfo_value: 0, lfo_delay: 0, bend: 0, wheel: 0, volume: 0 }; PARTS];
        for (part, controls) in self.parts.iter_mut().zip(&mut controls) {
            *controls = Controls {
                lfo_value: part.lfo.getsample(),
                lfo_delay: part.lfo.getdelay(),
                bend: part.bend.saturating_mul(PITCH_BEND_SCALE) >> PITCH_BEND_SHIFT,
                wheel: part.wheel,
                volume: u32::from(VOLUME_ATTENUATION.get(usize::from(part.volume)).copied().unwrap_or(0))
                    << VOLUME_SHIFT,
            };
        }
        for slot in &mut self.slots {
            if slot.stage == Stage::Free {
                continue;
            }
            slot.pending = false;
            let sounds = controls.get(usize::from(slot.part)).is_some_and(|controls| slot.note.control(controls));
            if !sounds && slot.stage == Stage::Released {
                slot.stage = Stage::Free;
                slot.note.clear();
            }
        }
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a block of 32 samples is at most that of
    /// sixteen notes of six operators, whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        let mut rest = out;
        while !rest.is_empty() {
            if self.block_remaining == 0 {
                self.control();
                self.block_remaining = BLOCK_SAMPLES;
            }
            let count = rest.len().min(self.block_remaining);
            let Some((now, later)) = rest.split_at_mut_checked(count) else {
                return;
            };
            let mut mix = [0_i32; BLOCK_SAMPLES];
            if let Some(mix) = mix.get_mut(..count) {
                for slot in &mut self.slots {
                    if slot.stage != Stage::Free && !slot.pending {
                        slot.note.render(mix);
                    }
                }
            }
            for (sample, &sum) in now.iter_mut().zip(&mix) {
                *sample = crate::output::limit(sum >> MIX_SHIFT);
            }
            self.block_remaining = self.block_remaining.saturating_sub(count);
            rest = later;
        }
    }
}

/// The shift that takes [`VOLUME_ATTENUATION`], in the log-sine table's
/// units, to the kernel's attenuation.
const VOLUME_SHIFT: u32 = super::kernel::ATTENUATION_FRACTION_BITS - super::tables::LOG_ONE.trailing_zeros();
