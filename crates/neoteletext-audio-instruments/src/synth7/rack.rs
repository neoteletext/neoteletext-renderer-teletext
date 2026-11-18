//! The rack: sixteen parts, one for each MIDI channel, sharing sixteen
//! notes, driven by MIDI messages and rendered to samples.
//!
//! # Gain
//!
//! A note's samples are the sum of its carriers, each attenuated for the
//! number of carriers, so a note at the patch's full output level (99) with
//! no velocity sensitivity peaks at about ±8,186, whatever its algorithm;
//! velocity sensitivity at the hardest velocity can raise that to about
//! ±15,000. The notes are added and [`MIX_GAIN`] of the sum is taken: a
//! full-level note then peaks 20 decibels below full scale, where every
//! instrument of the crate has its reference note
//! ([`crate::REFERENCE_LEVEL_TENTHS`]), and eight of them exactly in
//! phase pass the knee of the output's limiter
//! ([`crate::output::limit`]), which the sum goes through: it bends what
//! is near full scale under it, sixteen in phase among it, and never
//! wraps.
//!
//! # Time
//!
//! Envelopes, the low frequency oscillators and pitch advance once for
//! each [`BLOCK_SAMPLES`] samples, and levels glide between. Blocks are
//! counted across calls to [`Rack::render`], so the length of a call does
//! not matter. A note started from silence sounds from the sample its
//! message arrives at (`synth7.md`, S7-52): it runs the rest of the block
//! the rack is in. Every other message takes effect at the next block: at
//! most 0.67 milliseconds later, and at once when it arrives on a block's
//! boundary.
//!
//! # Parts
//!
//! Each part holds what one instrument modelled holds (S7-2): its current
//! voice and operator switch, its program, its fourteen function
//! parameters ([`Functions`]), its pitch bend, wheel, breath, foot and
//! pressure, its volume, its sustain pedal and portamento switch, its low
//! frequency oscillator and, in mono mode, the keys held on it.

use super::functions::{ASSIGN_AMPLITUDE, ASSIGN_EG_BIAS, ASSIGN_PITCH, ControllerSetting, Functions, number};
use super::interface::{BREATH, FOOT, PORTAMENTO_TIME, portamento_time};
use super::kernel::{ATTENUATION_ONE, ATTENUATION_SILENT, BLOCK_SAMPLES};
use super::msfa::Q24_ONE;
use super::msfa::lfo::{Lfo, Waveform};
use super::note::{Controls, FULL_DEPTH, Note};
use super::patch::{BANK_VOICES, Bank, MAX_99, Patch};
use super::tables::VOLUME_ATTENUATION;

/// Parts of the rack: one for each MIDI channel.
///
/// Source: MIDI 1.0: a channel is the low four bits of a status byte.
pub const PARTS: usize = 16;

/// Notes that sound at once, across all parts: the instrument's polyphony.
///
/// Source: DX7 manual, page 27: "POLY mode: 16 notes" (S7-3).
pub const NOTES: usize = 16;

/// The keys a part in mono mode remembers held (S7-26).
///
/// Source: NeoTeletext's own (`synth7.md` section 15 item 11).
pub const MONO_KEYS: usize = 16;

/// The share of the sum of all notes that is a sample: 821 / 2,048.
///
/// Source: own choice, explained under "Gain" above: the gain at which
/// one carrier at full output level stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with a quarter
/// of the sum, as it was first, it stood 4.1 dB below.
pub const MIX_GAIN: i32 = 821;
/// Bits of [`MIX_GAIN`]'s fraction.
pub const MIX_GAIN_BITS: u32 = 11;

/// How far the pitch bend wheel bends at either end before any function
/// parameter 65 or registered parameter number 0 sets it, in semitones
/// (S7-35).
pub const PITCH_BEND_SEMITONES: i32 = super::functions::DEFAULT_BEND_SEMITONES as i32;

/// The pitch bend value that bends nothing.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;

/// Semitones in an octave, for the bend's units.
const SEMITONES_PER_OCTAVE: i32 = 12;

/// A bend's offset from the centre times the range in semitones, times
/// [`bend_scale`] of it and shifted right by this, is the bend in Q24 of an
/// octave: `range ÷ 12 × 2^24 ÷ 8192 × 2^7` (S7-9).
const PITCH_BEND_SHIFT: u32 = 7;

/// The scale of a bend of `semitones` at either end, as
/// [`PITCH_BEND_SHIFT`] says: rounded, as it was for the fixed 2.
fn bend_scale(semitones: i32) -> i32 {
    let twice = semitones.saturating_mul(2).saturating_mul(Q24_ONE >> 6);
    twice.saturating_add(SEMITONES_PER_OCTAVE) / (2 * SEMITONES_PER_OCTAVE)
}

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
/// MIDI status: channel pressure.
const STATUS_CHANNEL_PRESSURE: u8 = 0xd0;
/// MIDI status: pitch bend.
const STATUS_PITCH_BEND: u8 = 0xe0;
/// Controller: modulation wheel.
const CONTROL_MODULATION_WHEEL: u8 = 1;
/// Controller: volume.
const CONTROL_VOLUME: u8 = 7;
/// The most a part's volume moves in a block, as attenuation: a quarter
/// of a doubling, 1.5 dB.
///
/// Source: own choice, measured (`tests/synth7_rack.rs`,
/// `tests/synth7_control_sweep.rs`). The kernel glides an operator's
/// attenuation from one block's to the next in a straight line, which
/// is a level that falls or rises by the same share at every sample;
/// a volume that went all its way in one block, 16 doublings in 32
/// samples, took 29% of the level at each sample, and from 127 to 0
/// stepped a note by 4.8 times its own largest step. At a quarter of a
/// doubling a block it is 0.5% a sample: 127 to 64, a quarter of the
/// level, takes eight blocks, 5.3 ms, which is the whole way of the
/// other instruments' volume, and 127 to 0 takes 56 blocks, 37 ms, of
/// which the last half is below a 250th of the level.
const VOLUME_STEP: u32 = ATTENUATION_ONE / 4;
/// Controller: sustain pedal.
const CONTROL_SUSTAIN: u8 = 64;
/// Controller: portamento switch.
const CONTROL_PORTAMENTO: u8 = 65;
/// Controller: all sound off.
const CONTROL_ALL_SOUND_OFF: u8 = 120;
/// Controller: reset all controllers.
const CONTROL_RESET_CONTROLLERS: u8 = 121;
/// Controller: all notes off.
const CONTROL_ALL_NOTES_OFF: u8 = 123;
/// Controller: poly on, the last of the mode messages, which are all notes
/// off and nothing more (S7-19).
const CONTROL_POLY_ON: u8 = 127;

/// Where each part's sample-and-hold sequence starts: part `n` starts from
/// `SEED + n × SEED_STEP`, in eight bits.
///
/// Source: own choice. The sequence itself is the parameter layer's.
const SEED: u8 = 0x5a;
/// See [`SEED`]. Odd, so that the sixteen parts differ.
const SEED_STEP: u8 = 0x1d;

/// How long a glide takes for an octave at each portamento time, in
/// milliseconds: 5 ms at time 0, doubling every ten steps, so time 1 is
/// 5.4 ms and time 99 4.8 seconds. This table is the fraction: `2^(k ÷
/// 10)` for k from 0 to 9, in thousandths.
///
/// Source: NeoTeletext's own (S7-27: "The time law … is NeoTeletext's own,
/// to be set and recorded when it is built"): a law in which each step of
/// the time is the same share longer, as a time control is heard.
const GLIDE_FRACTION_THOUSANDTHS: [u32; 10] = [1000, 1072, 1149, 1231, 1320, 1414, 1516, 1625, 1741, 1866];
/// The milliseconds of an octave at time 0, which glides not at all.
const GLIDE_OCTAVE_MS: u32 = 5;
/// The steps of time in which a glide doubles.
const GLIDE_DOUBLING_STEPS: u8 = 10;
/// Thousandths in one.
const THOUSANDTHS: u32 = 1000;
/// A rack block in milliseconds, as a fraction: 32 samples at 48,000 Hz
/// are 2/3 ms.
const BLOCK_MS_NUMERATOR: u32 = 2;
/// See [`BLOCK_MS_NUMERATOR`].
const BLOCK_MS_DENOMINATOR: u32 = 3;
const _: () =
    assert!(BLOCK_SAMPLES as u32 * BLOCK_MS_DENOMINATOR * THOUSANDTHS == BLOCK_MS_NUMERATOR * crate::SAMPLE_RATE);

/// What a glide at portamento time `time` (1 to 99) moves by in a block,
/// in Q24 of an octave; 0 at time 0, which does not glide (S7-27).
#[must_use]
pub fn glide_step(time: u8) -> i32 {
    if time == 0 {
        return 0;
    }
    let time = time.min(MAX_99);
    let fraction =
        GLIDE_FRACTION_THOUSANDTHS.get(usize::from(time % GLIDE_DOUBLING_STEPS)).copied().unwrap_or(THOUSANDTHS);
    let octave_ms = (GLIDE_OCTAVE_MS << (time / GLIDE_DOUBLING_STEPS)).saturating_mul(fraction) / THOUSANDTHS;
    let per_block = (Q24_ONE.cast_unsigned().saturating_mul(BLOCK_MS_NUMERATOR))
        .checked_div(octave_ms.saturating_mul(BLOCK_MS_DENOMINATOR))
        .unwrap_or(0);
    i32::try_from(per_block).unwrap_or(i32::MAX).max(1)
}

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
    /// Started from silence since the last block began, before it could
    /// sound: it sounds from the next.
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

    const fn sounding_on(&self, part: u8) -> bool {
        !matches!(self.stage, Stage::Free) && self.part == part
    }
}

/// One part: a MIDI channel's voice, function parameters and controllers
/// (S7-2).
#[derive(Clone, Copy, Debug)]
struct Part {
    program: u8,
    /// The current voice, its operator switch with it (S7-21, S7-30).
    voice: Patch,
    functions: Functions,
    /// The pitch bend, less its centre.
    bend: i32,
    wheel: u8,
    breath: u8,
    foot: u8,
    pressure: u8,
    volume: u8,
    /// The volume's attenuation as it stands, in the kernel's units, on
    /// its way to what the controller asks ([`VOLUME_STEP`]).
    attenuation: u32,
    sustain: bool,
    /// The portamento switch, controller 65: on until a message puts it
    /// off (S7-17).
    portamento: bool,
    /// The keys held in mono mode, the most recent last (S7-26).
    keys: [u8; MONO_KEYS],
    held: usize,
    /// The pitch of the key last pressed, where a glide starts (S7-27).
    last_pitch: Option<i32>,
    lfo: Lfo,
}

impl Part {
    fn new(index: usize, voice: Patch) -> Self {
        let seed = SEED.wrapping_add(SEED_STEP.wrapping_mul(index.to_le_bytes()[0]));
        let mut part = Self {
            program: 0,
            voice,
            functions: Functions::DEFAULT,
            bend: 0,
            wheel: 0,
            breath: 0,
            foot: 0,
            pressure: 0,
            volume: VOLUME_DEFAULT,
            attenuation: Self::wanted(VOLUME_DEFAULT),
            sustain: false,
            portamento: true,
            keys: [0; MONO_KEYS],
            held: 0,
            last_pitch: None,
            lfo: Lfo::new(seed),
        };
        part.load();
        part
    }

    /// The attenuation a volume asks for, in the kernel's units. It is
    /// held to the attenuation at which an operator is silent, beyond
    /// which more changes no sample and would only be more way to go.
    fn wanted(volume: u8) -> u32 {
        let attenuation = u32::from(VOLUME_ATTENUATION.get(usize::from(volume)).copied().unwrap_or(0)) << VOLUME_SHIFT;
        attenuation.min(ATTENUATION_SILENT)
    }

    /// The volume's attenuation for the next block: [`VOLUME_STEP`]
    /// nearer to what the controller asks, or there.
    fn volume(&mut self) -> u32 {
        let wanted = Self::wanted(self.volume);
        self.attenuation = if self.attenuation < wanted {
            self.attenuation.saturating_add(VOLUME_STEP).min(wanted)
        } else {
            self.attenuation.saturating_sub(VOLUME_STEP).max(wanted)
        };
        self.attenuation
    }

    /// Sets the oscillator from the part's voice.
    fn load(&mut self) {
        let params = self.voice.params();
        let waveform = Waveform::from_number(params.lfo_wave).unwrap_or(Waveform::Triangle);
        self.lfo.reset(params.lfo_speed, params.lfo_delay, params.lfo_key_sync != 0, waveform);
    }

    /// The bend in Q24 of an octave, at the part's range and step (S7-9):
    /// with a step, the bend in semitones rounded towards the centre to a
    /// whole multiple of it (NeoTeletext's own, section 15 item 6).
    fn bend_q24(&self) -> i32 {
        let range = i32::from(self.functions.bend_range());
        let step = i32::from(self.functions.bend_step());
        if step == 0 {
            return self.bend.saturating_mul(bend_scale(range)) >> PITCH_BEND_SHIFT;
        }
        let semitones = self.bend.saturating_mul(range) / PITCH_BEND_CENTRE;
        let stepped = semitones.checked_div(step).unwrap_or(0).saturating_mul(step);
        bend_scale(stepped).saturating_mul(PITCH_BEND_CENTRE) >> PITCH_BEND_SHIFT
    }

    /// What the four controllers add to the part's notes (S7-36): for each,
    /// `c` its value and `r` its range, a share of `c × r` of the full depth
    /// to pitch and amplitude, and `(1 − c) × r` to EG bias, the shares
    /// added and held to the full depth.
    fn modulation(&self) -> (u32, u32, u32) {
        let functions = &self.functions;
        let controllers = [
            (self.wheel, functions.wheel()),
            (self.foot, functions.foot()),
            (self.breath, functions.breath()),
            (self.pressure, functions.after_touch()),
        ];
        let (mut pitch, mut amplitude, mut bias) = (0_u32, 0_u32, 0_u32);
        for (value, ControllerSetting { range, assign }) in controllers {
            // 0 to 127 as 0 to 255, as the wheel always added its depth.
            let travel = u32::from(value) << 1 | u32::from(value) >> 6;
            let share =
                |travel: u32| travel.saturating_mul(u32::from(range)).checked_div(u32::from(MAX_99)).unwrap_or(0);
            if assign & ASSIGN_PITCH != 0 {
                pitch = pitch.saturating_add(share(travel));
            }
            if assign & ASSIGN_AMPLITUDE != 0 {
                amplitude = amplitude.saturating_add(share(travel));
            }
            if assign & ASSIGN_EG_BIAS != 0 {
                bias = bias.saturating_add(share(FULL_DEPTH.saturating_sub(travel)));
            }
        }
        (pitch.min(FULL_DEPTH), amplitude.min(FULL_DEPTH), bias.min(FULL_DEPTH))
    }

    /// The glide of a note on this part, by S7-27: its step a block, or 0
    /// where it does not glide.
    fn glide(&self) -> i32 {
        if self.portamento { glide_step(self.functions.portamento_time()) } else { 0 }
    }

    /// Forgets `key` among the keys held in mono mode.
    fn lift_key(&mut self, key: u8) {
        let held = self.held.min(MONO_KEYS);
        if let Some(keys) = self.keys.get_mut(..held)
            && let Some(at) = keys.iter().position(|&held| held == key)
            && let Some(rest) = keys.get_mut(at..)
        {
            rest.rotate_left(1);
            self.held = held.saturating_sub(1);
        }
    }

    /// Remembers `key` held in mono mode, the most recent; past
    /// [`MONO_KEYS`] the key held longest is forgotten (S7-26).
    fn hold_key(&mut self, key: u8) {
        self.lift_key(key);
        if self.held >= MONO_KEYS {
            self.keys.rotate_left(1);
            self.held = MONO_KEYS.saturating_sub(1);
        }
        if let Some(place) = self.keys.get_mut(self.held) {
            *place = key;
            self.held = self.held.saturating_add(1);
        }
    }

    /// The key held most recently in mono mode.
    fn top_key(&self) -> Option<u8> {
        self.held.checked_sub(1).and_then(|at| self.keys.get(at)).copied()
    }
}

/// What a message did, for the count of ignored messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// It acted, or changed nothing because it set what was there.
    Applied,
    /// The rack does not take it: nothing changed.
    Ignored,
}

/// Sixteen parts of the instrument sharing sixteen notes.
///
/// It is about 15 kilobytes, holds no reference and allocates nothing. Two
/// racks given the same messages at the same samples give the same
/// samples, whatever else exists and however the rendering is divided
/// into calls.
#[derive(Clone, Debug)]
pub struct Rack {
    bank: Bank,
    parts: [Part; PARTS],
    slots: [Slot; NOTES],
    /// What each part gave its notes at the last block, for a note that
    /// starts within the next.
    controls: [Controls; PARTS],
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
        Self {
            bank: Bank::new(),
            parts: core::array::from_fn(|index| Part::new(index, Patch::DEFAULT)),
            slots: [Slot::new(); NOTES],
            controls: [Controls::default(); PARTS],
            clock: 0,
            block_remaining: 0,
            ignored: 0,
        }
    }

    /// Gives the rack a bank, which it copies, for the program changes of
    /// [`Rack::event`]. Notes that are sounding keep the voice they started
    /// with; each part's current voice becomes its program's in the new
    /// bank, or [`Patch::DEFAULT`] where the bank has none.
    pub fn set_bank(&mut self, bank: &Bank) {
        self.bank = *bank;
        for part in &mut self.parts {
            part.voice = *self.bank.get(usize::from(part.program)).unwrap_or(&Patch::DEFAULT);
            part.load();
        }
    }

    /// Puts the rack back as [`Rack::new`] made it, but for the bank, which
    /// it keeps: silent, every part at program 0 with its controllers and
    /// function parameters at rest, the count of ignored messages at zero.
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

    /// Part `part`'s current voice, its operator switch with it.
    #[must_use]
    pub fn voice(&self, part: u8) -> Option<&Patch> {
        self.parts.get(usize::from(part)).map(|part| &part.voice)
    }

    /// Part `part`'s program number.
    #[must_use]
    pub fn program(&self, part: u8) -> Option<u8> {
        self.parts.get(usize::from(part)).map(|part| part.program)
    }

    /// Part `part`'s function parameters.
    #[must_use]
    pub fn functions(&self, part: u8) -> Option<&Functions> {
        self.parts.get(usize::from(part)).map(|part| &part.functions)
    }

    /// Part `part`'s portamento switch (S7-17).
    #[must_use]
    pub fn portamento_switch(&self, part: u8) -> Option<bool> {
        self.parts.get(usize::from(part)).map(|part| part.portamento)
    }

    /// Part `part`'s modulation wheel, breath controller, foot controller
    /// and channel pressure, 0 to 127 each.
    #[must_use]
    pub fn performance(&self, part: u8) -> Option<[u8; 4]> {
        self.parts.get(usize::from(part)).map(|part| [part.wheel, part.breath, part.foot, part.pressure])
    }

    /// Part `part`'s pitch bend, 14 bits, and whether its sustain pedal is
    /// down.
    #[must_use]
    pub fn bend_and_sustain(&self, part: u8) -> Option<(u16, bool)> {
        self.parts.get(usize::from(part)).map(|part| {
            let bend = u16::try_from(part.bend.saturating_add(PITCH_BEND_CENTRE)).unwrap_or(0);
            (bend, part.sustain)
        })
    }

    /// Takes one MIDI 1.0 channel voice message. The low four bits of
    /// `status` choose the part.
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n).
    /// - Control change (0xBn) of the modulation wheel (1), the breath
    ///   controller (2), the foot controller (4), portamento time (5),
    ///   volume (7, which moves to its new place by 1.5 dB at the most in
    ///   each block of 32 samples), the sustain pedal (64, down from 64),
    ///   the portamento switch (65, on from 64), all sound off (120), reset
    ///   all controllers (121, which leaves the volume) and all notes off
    ///   (123 to 127).
    /// - Program change (0xCn) to a program of 0 to 31 of the rack's bank,
    ///   which sets the part's current voice; `data2` is not looked at.
    /// - Channel pressure (0xDn).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven.
    ///
    /// Anything else is ignored and counted ([`Rack::ignored_events`]): any
    /// other status or controller, a program above 31, a data byte above
    /// 127. Nothing is an error. Registered parameter numbers and system
    /// exclusive are the receiver's front end's ([`super::midi::Midi`]).
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        let part = status & 0x0f;
        let kind = status & 0xf0;
        let two_data_bytes = kind != STATUS_PROGRAM_CHANGE && kind != STATUS_CHANNEL_PRESSURE;
        if data1 > DATA_MAX || (two_data_bytes && data2 > DATA_MAX) {
            self.ignore();
            return;
        }
        let taken = match kind {
            STATUS_NOTE_OFF => self.note_off(part, data1),
            STATUS_NOTE_ON if data2 == 0 => self.note_off(part, data1),
            STATUS_NOTE_ON => self.note_on(part, data1, data2),
            STATUS_CONTROL_CHANGE => self.control_change(part, data1, data2),
            STATUS_PROGRAM_CHANGE if usize::from(data1) < BANK_VOICES => {
                let voice = *self.bank.get(usize::from(data1)).unwrap_or(&Patch::DEFAULT);
                self.program_change(part, data1, &voice)
            }
            STATUS_CHANNEL_PRESSURE => self.channel_pressure(part, data1),
            STATUS_PITCH_BEND => self.pitch_bend(part, u16::from(data2) << 7 | u16::from(data1)),
            _ => Taken::Ignored,
        };
        if taken == Taken::Ignored {
            self.ignore();
        }
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    /// A key goes down on part `part_number` (S7-6, S7-25 to S7-27).
    pub fn note_on(&mut self, part_number: u8, key: u8, velocity: u8) -> Taken {
        self.clock = self.clock.saturating_add(1);
        let Some(part) = self.parts.get(usize::from(part_number)) else { return Taken::Ignored };
        if part.functions.mono() {
            self.mono_note_on(part_number, key, velocity)
        } else {
            self.poly_note_on(part_number, key, velocity)
        }
    }

    fn poly_note_on(&mut self, part_number: u8, key: u8, velocity: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        // The instrument starts the oscillator's delay, and its waveform if
        // the patch says so, when a key goes down with no other held.
        if !self.slots.iter().any(|slot| slot.stage == Stage::Held && slot.part == part_number) {
            part.lfo.keydown();
        }
        let voice = part.voice;
        let step = part.glide();
        let from = part.last_pitch;
        let follow = part.functions.portamento_mode() != 0;

        // In order: the note already sounding on this key of this part; a
        // free note; the note released longest ago; the oldest note held.
        let again = self.slots.iter().position(|slot| slot.sounding_on(part_number) && slot.key == key);
        let chosen = again.or_else(|| {
            self.slots
                .iter()
                .enumerate()
                .min_by_key(|(_, slot)| match slot.stage {
                    Stage::Free => (0_u8, 0),
                    Stage::Released => (1, slot.released),
                    Stage::Held | Stage::Sustained => (2, slot.started),
                })
                .map(|(index, _)| index)
        });
        let Some(index) = chosen else { return Taken::Ignored };
        let pitch = self.start_slot(index, part_number, &voice, key, velocity);
        if let (Some(from), Some(slot)) = (from, self.slots.get_mut(index)) {
            slot.note.glide_from(from, step);
        }
        // SUS-KEY P FOLLOW: the notes the pedal holds glide to the new key
        // (S7-27; the TX7 manual, printed page 40, note *1).
        if follow && step > 0 {
            for (other, slot) in self.slots.iter_mut().enumerate() {
                if other != index && slot.part == part_number && slot.stage == Stage::Sustained {
                    slot.note.retarget(&voice, key, step);
                    slot.key = key;
                }
            }
        }
        if let Some(part) = self.parts.get_mut(usize::from(part_number)) {
            part.last_pitch = pitch;
        }
        Taken::Applied
    }

    fn mono_note_on(&mut self, part_number: u8, key: u8, velocity: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        let legato = part.held > 0;
        part.hold_key(key);
        let voice = part.voice;
        let step = part.glide();
        let full_time = part.functions.portamento_mode() != 0;
        let from = part.last_pitch;
        if !legato {
            part.lfo.keydown();
        }
        let sounding =
            self.slots.iter().position(|slot| slot.sounding_on(part_number) && slot.stage != Stage::Released);
        let pitch = match sounding {
            // A key pressed while another is held takes the note without
            // starting its envelopes again (S7-26); fingered portamento
            // glides only here, full time everywhere (S7-27).
            Some(index) if legato => {
                let Some(slot) = self.slots.get_mut(index) else { return Taken::Ignored };
                slot.note.retarget(&voice, key, step);
                slot.key = key;
                slot.stage = Stage::Held;
                Some(slot.note.pitch())
            }
            _ => {
                let index = sounding
                    .or_else(|| self.slots.iter().position(|slot| slot.sounding_on(part_number)))
                    .or_else(|| {
                        self.slots
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, slot)| match slot.stage {
                                Stage::Free => (0_u8, 0),
                                Stage::Released => (1, slot.released),
                                Stage::Held | Stage::Sustained => (2, slot.started),
                            })
                            .map(|(index, _)| index)
                    });
                let Some(index) = index else { return Taken::Ignored };
                let pitch = self.start_slot(index, part_number, &voice, key, velocity);
                if full_time && let (Some(from), Some(slot)) = (from, self.slots.get_mut(index)) {
                    slot.note.glide_from(from, step);
                }
                pitch
            }
        };
        if let Some(part) = self.parts.get_mut(usize::from(part_number)) {
            part.last_pitch = pitch;
        }
        Taken::Applied
    }

    /// Starts slot `index` on `key` with `voice`, and gives the key's pitch.
    fn start_slot(&mut self, index: usize, part_number: u8, voice: &Patch, key: u8, velocity: u8) -> Option<i32> {
        let clock = self.clock;
        let remaining = self.block_remaining;
        let controls = self.controls.get(usize::from(part_number)).copied().unwrap_or_default();
        let slot = self.slots.get_mut(index)?;
        let continues = slot.stage != Stage::Free;
        slot.note.start(voice, key, velocity, continues);
        slot.stage = Stage::Held;
        slot.part = part_number;
        slot.key = key;
        slot.started = clock;
        // A note that takes over a sounding voice goes on sounding to the
        // end of this block, at the levels the voice has: left out of it,
        // the voice would have a notch of silence. A note from silence
        // starts at its own sample, for what is left of the block (S7-52).
        slot.pending = false;
        if !continues && remaining > 0 {
            slot.note.control_partial(&controls, remaining);
        } else if !continues {
            slot.pending = true;
        }
        Some(slot.note.pitch())
    }

    /// A key goes up on part `part_number` (S7-7, S7-26).
    pub fn note_off(&mut self, part_number: u8, key: u8) -> Taken {
        self.clock = self.clock.saturating_add(1);
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        let sustain = part.sustain;
        if part.functions.mono() {
            part.lift_key(key);
            let top = part.top_key();
            let voice = part.voice;
            let step = part.glide();
            let held = self
                .slots
                .iter()
                .position(|slot| slot.part == part_number && slot.stage == Stage::Held && slot.key == key);
            // The note passes to the key pressed most recently among those
            // still held, without starting its envelopes again (S7-26).
            if let (Some(index), Some(top)) = (held, top) {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.note.retarget(&voice, top, step);
                    slot.key = top;
                    let pitch = slot.note.pitch();
                    if let Some(part) = self.parts.get_mut(usize::from(part_number)) {
                        part.last_pitch = Some(pitch);
                    }
                }
                return Taken::Applied;
            }
        }
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
        Taken::Applied
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

    /// The sustain pedal of part `part_number` goes down or up (S7-16).
    pub fn sustain(&mut self, part_number: u8, down: bool) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        part.sustain = down;
        if !down {
            self.release_part(part_number, &[Stage::Sustained]);
        }
        Taken::Applied
    }

    /// All notes off on part `part_number` (S7-19, S7-20): as a note off of
    /// every held key; the pedal still holds.
    pub fn all_notes_off(&mut self, part_number: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        part.held = 0;
        if part.sustain {
            for slot in &mut self.slots {
                if slot.stage == Stage::Held && slot.part == part_number {
                    slot.stage = Stage::Sustained;
                }
            }
        } else {
            self.release_part(part_number, &[Stage::Held]);
        }
        Taken::Applied
    }

    /// All sound off on part `part_number` (S7-20 item 1): every note of the
    /// part faded to silence in 21 milliseconds.
    pub fn all_sound_off(&mut self, part_number: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        part.held = 0;
        let clock = self.clock;
        for slot in &mut self.slots {
            if slot.stage != Stage::Free && slot.part == part_number {
                slot.stage = Stage::Released;
                slot.released = clock;
                slot.note.cut();
            }
        }
        Taken::Applied
    }

    /// Reset all controllers on part `part_number` (S7-20 item 2, RP-015):
    /// the wheel, breath, foot and pressure to 0, the bend to its centre,
    /// the sustain pedal up, releasing what it held, and the portamento
    /// switch off. The volume, the voice, the operator switch, the program
    /// and every function parameter are left.
    pub fn reset_controllers(&mut self, part_number: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        part.wheel = 0;
        part.breath = 0;
        part.foot = 0;
        part.pressure = 0;
        part.bend = 0;
        part.sustain = false;
        part.portamento = false;
        self.release_part(part_number, &[Stage::Sustained]);
        Taken::Applied
    }

    /// A control change the rack takes itself: 1, 2, 4, 5, 7, 64, 65, 120,
    /// 121 and 123 to 127 (S7-10). Any other is [`Taken::Ignored`].
    pub fn control_change(&mut self, part_number: u8, controller: u8, value: u8) -> Taken {
        match controller {
            CONTROL_SUSTAIN => return self.sustain(part_number, value >= PEDAL_DOWN),
            CONTROL_ALL_SOUND_OFF => return self.all_sound_off(part_number),
            CONTROL_RESET_CONTROLLERS => return self.reset_controllers(part_number),
            CONTROL_ALL_NOTES_OFF..=CONTROL_POLY_ON => return self.all_notes_off(part_number),
            PORTAMENTO_TIME => {
                return self.set_function(part_number, number::PORTAMENTO_TIME, portamento_time(value));
            }
            _ => {}
        }
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        match controller {
            CONTROL_MODULATION_WHEEL => part.wheel = value,
            BREATH => part.breath = value,
            FOOT => part.foot = value,
            CONTROL_VOLUME => part.volume = value,
            CONTROL_PORTAMENTO => part.portamento = value >= PEDAL_DOWN,
            _ => return Taken::Ignored,
        }
        Taken::Applied
    }

    /// Channel pressure on part `part_number` (S7-22).
    pub fn channel_pressure(&mut self, part_number: u8, pressure: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        part.pressure = pressure.min(DATA_MAX);
        Taken::Applied
    }

    /// A pitch bend on part `part_number`, 14 bits, centre 8,192 (S7-9).
    pub fn pitch_bend(&mut self, part_number: u8, value: u16) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        part.bend = i32::from(value.min(crate::midi::PITCH_BEND_MAX)).saturating_sub(PITCH_BEND_CENTRE);
        Taken::Applied
    }

    /// A program change on part `part_number` to `program`, whose voice is
    /// `voice` (S7-21): the part's current voice becomes it with every
    /// operator on; sounding notes keep theirs. A program change to the
    /// program and voice the part already plays changes nothing that is
    /// heard (MIDI-97).
    pub fn program_change(&mut self, part_number: u8, program: u8, voice: &Patch) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        let voice = voice.all_operators_on();
        part.program = program;
        if part.voice != voice {
            part.voice = voice;
            part.load();
        }
        Taken::Applied
    }

    /// A single-voice dump on part `part_number` (S7-40): its current voice
    /// becomes `voice`, with every operator on; the program stays.
    pub fn set_voice(&mut self, part_number: u8, voice: &Patch) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        let voice = voice.all_operators_on();
        if part.voice != voice {
            part.voice = voice;
            part.load();
        }
        Taken::Applied
    }

    /// A parameter change of voice parameter `number` (0 to 155) on part
    /// `part_number` (S7-44): the current voice changes, and the notes the
    /// part sounds take the parameters the engine reads continuously. A
    /// value out of range is [`Taken::Ignored`] (S7-29).
    pub fn set_voice_parameter(&mut self, part_number: u8, number: u16, value: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        let Ok(voice) = part.voice.with_parameter(number, value) else { return Taken::Ignored };
        if voice == part.voice {
            return Taken::Applied;
        }
        let lfo_changed = {
            let (old, new) = (part.voice.params(), voice.params());
            (old.lfo_speed, old.lfo_delay, old.lfo_key_sync, old.lfo_wave)
                != (new.lfo_speed, new.lfo_delay, new.lfo_key_sync, new.lfo_wave)
        };
        part.voice = voice;
        if lfo_changed {
            part.load();
        }
        for slot in &mut self.slots {
            if slot.sounding_on(part_number) {
                slot.note.follow(&voice);
            }
        }
        Taken::Applied
    }

    /// A parameter change of function parameter `number` (64 to 77) on part
    /// `part_number` (S7-33): it applies from the next block, to sounding
    /// notes too. A change from poly to mono releases the part's notes but
    /// the one pressed most recently (NeoTeletext's own, section 15 item
    /// 12). A number or value out of range is [`Taken::Ignored`].
    pub fn set_function(&mut self, part_number: u8, number: u16, value: u8) -> Taken {
        let Some(part) = self.parts.get_mut(usize::from(part_number)) else { return Taken::Ignored };
        let was_mono = part.functions.mono();
        let Ok(changed) = part.functions.set(number, value) else { return Taken::Ignored };
        if changed && number == number::MONO_POLY && part.functions.mono() != was_mono {
            part.held = 0;
            if part.functions.mono() {
                let newest = self
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(_, slot)| slot.part == part_number && slot.stage == Stage::Held)
                    .max_by_key(|(_, slot)| slot.started)
                    .map(|(index, slot)| (index, slot.key));
                if let Some((_, key)) = newest {
                    part.hold_key(key);
                }
                let clock = self.clock;
                for (index, slot) in self.slots.iter_mut().enumerate() {
                    let keep = newest.is_some_and(|(newest, _)| newest == index);
                    if slot.sounding_on(part_number) && !keep && slot.stage != Stage::Released {
                        slot.stage = Stage::Released;
                        slot.released = clock;
                        slot.note.release();
                    }
                }
            }
        }
        Taken::Applied
    }

    /// Releases every note of every part, held by a key or by a pedal, and
    /// lifts every pedal: for when the input stops and no note off will
    /// come (S7-53).
    pub fn release_all(&mut self) {
        self.clock = self.clock.saturating_add(1);
        for part in &mut self.parts {
            part.sustain = false;
            part.held = 0;
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
        for (part, controls) in self.parts.iter_mut().zip(&mut self.controls) {
            let (pitch_depth, amplitude_depth, bias) = part.modulation();
            *controls = Controls {
                lfo_value: part.lfo.getsample(),
                lfo_delay: part.lfo.getdelay(),
                bend: part.bend_q24(),
                pitch_depth,
                amplitude_depth,
                bias,
                glissando: part.functions.glissando(),
                volume: part.volume(),
            };
        }
        for slot in &mut self.slots {
            if slot.stage == Stage::Free {
                continue;
            }
            slot.pending = false;
            let sounds = self.controls.get(usize::from(slot.part)).is_some_and(|controls| slot.note.control(controls));
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
                *sample = crate::output::limit(sum.saturating_mul(MIX_GAIN) >> MIX_GAIN_BITS);
            }
            self.block_remaining = self.block_remaining.saturating_sub(count);
            rest = later;
        }
    }
}

/// The shift that takes [`VOLUME_ATTENUATION`], in the log-sine table's
/// units, to the kernel's attenuation.
const VOLUME_SHIFT: u32 = super::kernel::ATTENUATION_FRACTION_BITS - super::tables::LOG_ONE.trailing_zeros();
