//! The instrument: its voices, its panel and the messages that play it
//! ([`super`] says what it is and how it is played).

use crate::SAMPLE_RATE;
use crate::analog::allocator::{Allocator, Assign, Mode, Stage};
use crate::analog::chorus::Chorus;
use crate::analog::decimator::Decimator;
use crate::analog::envelope::{ENVELOPE_ONE, RcEnvelope, apply, time_constant};
use crate::analog::filter::{ONE_Q28, OnePole};
use crate::analog::fixed::amplify;
use crate::analog::oscillator::Noise;
use crate::analog::volume::{self, VolumeGain};
use crate::analog::{frequency, to_sample};

use crate::midi::common::{Action, Common};
use crate::midi::{Banks, ChannelMessage, KIND_MASK, Message, Outcome, PITCH_BEND_CENTRE, controller, status};

use super::interface::{self, CONFIG};
use super::law::{self, OCTAVE};
use super::patch::{self, Control, Function, Global, Kind, Patch};
use super::voice::{Channel, Controls, OVERSAMPLING, Shared, TICK_RATE, TICK_SAMPLES, Voice, sine};

/// Voices of the instrument.
///
/// Source: owner's manual, page 31, "Overall Picture": "The CS-80 is an
/// eight note polyphonic synthesizer. However, there are really 16 main
/// oscillators divided into two ranks".
pub const VOICES: usize = 8;

/// How far the modulation wheel at its top lets the sub oscillator move
/// the pitch either way, in cents.
///
/// Source: own choice: the instrument has no such wheel.
pub const WHEEL_CENTS: i32 = 50;

/// Cents in an octave, for the bend's and the tuning's offsets.
const CENTS_PER_OCTAVE: i64 = 1_200;

/// The seed of the instrument's noise and of its oscillators' places and
/// tolerances.
///
/// Source: own choice: any number; the same instrument every time.
const SEED: u32 = 0x4353_3830;

/// A slow oscillator's frequency with 5 V at its speed input, in
/// thousandths of a hertz: a cycle in 150 ms. The pulse width's, the sub
/// oscillator's and the ring modulator's are all trimmed to it.
///
/// Source: adjustment procedure, pages 17 and 19: pulse width modulator,
/// step 1; sub oscillator, step 1; ring modulator, step 3: each "150 ms
/// (± 5%)" with "+5.0V" at its terminal.
const SPEED_AT_FIVE_VOLTS: u32 = 6_667;
/// The octaves a volt speeds the pulse width's oscillator: from 150 ms
/// at 5 V to 5 ms at the lever's top, log₂ 30 in five volts.
///
/// Source: adjustment procedure, page 17, pulse width modulator: "5 ms
/// (± 5%)" at FAST. Own choice: the same below 5 V, which puts the
/// lever's foot at 4.5 s; and the lever's top is taken as 10 V, the
/// lever being one of a tone's, on the tone selector's line (service
/// manual, page 62).
const FAST_OCTAVES_A_VOLT: i32 = law::octaves(981_378, 1_000_000);
/// The same for the ring modulator's oscillator, whose SPEED lever's top
/// is the panel's 10.6 V ([`PANEL_SUPPLY_TENTHS`]): log₂ 30 in 5.6 volts,
/// which puts the lever's foot at 3.1 s.
///
/// Source: adjustment procedure, page 19, ring modulator, step 3: "150
/// ms" with "+5.0V at VC1 terminal" and "5 ms (± 5%)" at "SPEED ... max".
/// Own choice: the same below 5 V.
const RING_OCTAVES_A_VOLT: i32 = law::octaves(876_230, 1_000_000);
/// The same for the sub oscillator, whose SPEED lever is on the same
/// line: to 18 ms, log₂ (150 / 18) in 5.6 volts, which puts the lever's
/// foot at 1.0 s.
///
/// Source: adjustment procedure, page 17, sub oscillator: "18ms (± 5%)"
/// at FAST.
const SUB_OCTAVES_A_VOLT: i32 = law::octaves(546_231, 1_000_000);
/// The share of the mixed output that is a sample, in Q28: 1288 / 1000.
/// One line's sawtooth through the open filters then peaks 14.5 dB below
/// full scale, and eight keys struck together 0.4 dB below, the tops of
/// them bent by the output's limiter ([`crate::output::limit`]), as is
/// whatever else nears full scale.
///
/// Source: own choice: the gain at which the reference note stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with the whole
/// of the output, as it was first, the note stood 2.2 dB below it.
pub const OUTPUT_GAIN: i32 = (ONE_Q28 as i64 * 1288 / 1000) as i32;
/// The volts pressure adds to the sub oscillator's speed with the touch
/// response's SPEED lever at the top.
///
/// Source: adjustment procedure, page 25, step 6: "+6.0 ± .3V" at the SP
/// terminal, the key firmly depressed.
const TOUCH_SPEED_VOLTS: i32 = 6;
/// The volts the ring modulator's envelope adds to its speed at its peak
/// with DEPTH at the top.
///
/// Source: own choice: the half of the SPEED lever's travel.
const RING_DEPTH_VOLTS: i32 = 5;
/// The ring modulator's shortest attack, in millionths of a second: the
/// 31 ms of 5 V halved for each of five volts more.
///
/// Source: adjustment procedure, page 19, step 2: "31ms (± 5%)" with
/// "+5.0V at A terminal". Own choice: an octave to the volt, as the
/// voices' envelopes.
const RING_ATTACK_SHORTEST: u32 = 31_000 >> 5;

/// The fastest cycles of the effect's two oscillators, in thousandths of
/// a hertz: 56 ms and 0.5 s, with SPEED at its top and the TREMOLO tab
/// down. Both run under either tab.
///
/// Source: adjustment procedure, page 21, tremolo/chorus, step 1: with
/// "SPEED ... max." and "TREMOLO tab ... on", "56ms (± 5%)" at DO1 and
/// ".5s (± 5%)" at DO2. Service manual, page 55, OE1 circuit diagram:
/// the two oscillators (IC3 and IC5, IG00150) take the one speed voltage
/// C through 100 kΩ each, later 180 kΩ.
const EFFECT_FASTEST: (u32, u32) = (17_857, 2_000);
/// The supply of the levers and knobs outside the tones, in tenths of a
/// volt.
///
/// Source: service manual, page 66, Panel 3 circuit diagram: the SPEED
/// knob (VR3, B 10 kΩ) stands between earth and the line of the SUSTAIN
/// lever's end, "RA1-BA-+10.6(20)"; page 64, Panel 2 circuit diagram:
/// the same orange wire, labelled so at the touch response's levers, is
/// the top of the sub oscillator's and the ring modulator's SPEED levers
/// (VR8 and VR6).
const PANEL_SUPPLY_TENTHS: i32 = 106;
/// A tenth of a volt, in the units of an octave to the volt.
const TENTH_OF_A_VOLT: i32 = law::VOLT / 10;
/// The SPEED knob and what loads its wiper, in hundreds of ohms, which
/// keep the products of [`effect_speed_volts`] inside a word: the knob's
/// 10 kΩ,
/// and the two dividers that make the tabs' speed voltages of it, 22 kΩ
/// over 100 kΩ for TREMOLO and 8.2 kΩ over 1.2 kΩ for CHORUS.
///
/// Source: service manual, page 55, OE1 circuit diagram, between the
/// terminals SP, T and CH; page 66: "B10K VR3 SPEED".
const SPEED_KNOB: i64 = 100;
/// See [`SPEED_KNOB`]: each divider's upper and lower resistor.
const SPEED_DIVIDERS: [(i64, i64); 2] = [(220, 1_000), (82, 12)];
/// The octaves a volt of the speed voltage C speeds the effect's
/// oscillators: the pulse width's oscillator's, 437 / 510 of it.
///
/// Source: own reading; no document gives the chip's law. The effect's
/// oscillators and the pulse width's are the same chip (service manual,
/// pages 30 and 56: IG00150), and each has its speed voltage through a
/// resistor to pin 1 and 330 kΩ from pin 1 to -15 V (pages 29 and 55):
/// 82 kΩ and a trimmer of 50 kΩ at the pulse width's, taken at its
/// middle, 107 kΩ; 180 kΩ here (drawn 100 kΩ and changed by hand). The
/// pin is taken to follow the voltage the two resistors leave it, "The
/// frequency is variable in accordance with the voltage supplied" (page
/// 52, VCO II IC, pin 1): 330 / 437 of the lever's there and 330 / 510 of
/// C here. The pulse width's oscillator's octaves a volt are the
/// adjustment procedure's two points ([`FAST_OCTAVES_A_VOLT`]).
const EFFECT_OCTAVES_A_VOLT: i32 = ((FAST_OCTAVES_A_VOLT as i64) * 437 / 510) as i32;
/// The delay of the effect's lines, in millionths of a second: 512
/// stages clocked with a period of 10 µs.
///
/// Source: service manual, page 58, OE2 board, note 2: "IC 4: MN3001",
/// clocked through a TC4049 by one oscillator (IC1, BA617), so that both
/// lines are of one length and do not move; adjustment procedure, page
/// 21, step 2: the clock is set "between 7 - 12.5 µs". That the MN3001
/// is two lines of 512 stages is general knowledge of the part; its data
/// sheet is not among the documents filed.
const EFFECT_DELAY: u32 = 2_560;
/// The cutoff of the filters about the lines, in thousandths of a hertz.
///
/// Source: service manual, page 57, OE2 circuit diagram: the filter
/// before the lines and the one after each are 10 kΩ twice with 0.0018
/// µF and 680 pF, 1 / (2 π R √(C1 C2)) = 14.4 kHz.
const EFFECT_FILTER: u32 = 14_400_000;
/// The cutoff of the low-pass filter each of the four modulation
/// amplifiers' outputs makes, in thousandths of a hertz: 499 Hz, 6 dB an
/// octave.
///
/// Source: service manual, page 57, OE2 circuit diagram, and page 58,
/// the board as printed: each amplifier (IC7 to IC10, IG00151) puts its
/// output current into 27 kΩ with 0.015 µF across it (drawn 0.0056 µF
/// and changed by hand; the board has 0.015), and page 55: the OE1 board
/// takes each through 100 kΩ to a summing amplifier's input. 27 kΩ and
/// 100 kΩ side by side are 21.26 kΩ, and 1 / (2 π R C) is 499.1 Hz.
const EFFECT_WET_FILTER: u32 = 499_100;
/// The share of the dry sound that is left with the effect on, in Q30:
/// 68 / 168.
///
/// Source: service manual, page 55, OE1 circuit diagram: each output
/// amplifier (IC2) has the sound at its input through 100 kΩ always, and
/// through 68 kΩ what two switches choose: the sound again with the
/// effect off (FET2 and FET3, gated by G1), the delayed sound with it on
/// (FET1 and FET4, gated by G2; page 66, SW7 ON-OFF). Off, the sound
/// comes through both, 1/100 + 1/68; on, through the 100 kΩ alone, which
/// is 68 / 168 of that.
const EFFECT_DRY: i32 = (ENVELOPE_ONE as i64 * 68 / 168) as i32;
/// The share that the delayed sound has at low frequencies with the
/// effect on, in Q28, where the sound with the effect off is one: 100 /
/// 168 of 2.2365, which is 1.3313. It is taken from the dry sound: the
/// delayed sound is inverted.
///
/// Source: the same drawing for the 100 / 168, the delayed sound's 68 kΩ
/// against both, and for the inversion: the two amplifiers' outputs of
/// each side are summed by an inverting amplifier (IC1 and IC7, 100 kΩ
/// each into 82 kΩ of feedback), and nothing else on the way inverts as
/// far as the drawings tell (the sound enters the modulation amplifiers
/// at "+IN", page 52). For the 2.2365, own reading of the adjustment
/// procedure, page 21, step 4: with the sine of the key B4 (the manual's
/// C1 is the lowest key, so 987.8 Hz at 8') the four amplifiers are
/// trimmed to ".65", ".58", ".58" and ".65 Vp-p" at the OE2 board's
/// outputs; each side sums one of 0.65 V and one of 0.58 V (page 58, the
/// wiring to I2 to I5), times 82 / 100, 1.009 Vp-p. The delayed sound is
/// taken to be trimmed to stand as large as the dry at that key, which
/// is to say that the tone enters the board at 1 Vp-p: no page prints
/// the level there, and the documents do not settle it. The "1.00 ± .02
/// Vp-p" of the same tone (page 15, VCA, step 1) is at CP4, behind the
/// first of a voice's amplifiers; two amplifiers on, the voice leaves
/// its board at "80 ± 5 mVp-p" (step 4, terminal O), and the amplifier
/// that brings the mixed voices on to the effect (service manual, page
/// 53, PRA, IC7) is drawn without its feedback resistor. For 1 Vp-p:
/// the effect on is then as loud as off at the trimmed key, 68 / 168 +
/// 100 / 168 × 1.009 = 1.005, which reads as what the 0.65 and 0.58 are
/// for; and the wah's output, which is the board's input when the wah
/// is on, is trimmed to "1.0 ± .1 Vp-p" (adjustment procedure, page 29).
/// Against it: the board as printed has a 27 kΩ beside IC7 that the
/// drawing does not place (page 54); were that the feedback of its two
/// 10 kΩ inputs, the tone would enter at 0.22 Vp-p, and the delayed
/// sound would be 4.7 times the dry at that key. At that key the amplifiers' low-pass
/// filter ([`EFFECT_WET_FILTER`]) has taken the delayed sound down by
/// √(1 + (987.8 / 499.1)²), 2.2176, which low frequencies are spared:
/// 1.0086 × 2.2176 = 2.2365 (`tests/synth80_modulation.rs` works it out).
const EFFECT_WET: i32 = 357_360_555;
/// What the effect's switches move by in a sample, in Q30: from off to on
/// or back in 22 ms, in a straight line.
///
/// Source: service manual, page 55, OE1 circuit diagram: the two pairs
/// of switches (FET2 and FET3 for the sound itself, FET1 and FET4 for the
/// delayed sound) have their gates from the lines G1 and G2 through 10 kΩ
/// into 2.2 µF, 22 ms, so that one pair closes as the other opens and the
/// ON-OFF switch does not click. Own choice: the straight line, in place
/// of what a FET makes of its gate's voltage.
const EFFECT_SWITCH_STEP: i32 = (ENVELOPE_ONE as i64 * 1_000_000 / (22_000 * SAMPLE_RATE as i64)) as i32;
/// How far the effect at full depth under the CHORUS tab moves the
/// delayed sound's level in the mono output either way for each of its
/// two oscillators, in Q30: 7 / 246.
///
/// Source: adjustment procedure, page 21, step 4: the four modulation
/// amplifiers are trimmed to ".65", ".58", ".58" and ".65 Vp-p"; service
/// manual, pages 55 to 58: the first line feeds the two amplifiers of
/// 0.65 V and the second the two of 0.58 V; each output is one amplifier
/// of each line; each oscillator drives one amplifier of each output,
/// oppositely (LO1 and its inverse LO3, LO2 and LO4). The lines carry the
/// same sound, so in the sum of the two outputs an oscillator's share is
/// (0.65 - 0.58) / (2 × (0.65 + 0.58)), the first oscillator's upwards
/// when the second's is downwards. Own choice: the amplifiers swing
/// their whole gain at full depth.
const EFFECT_MONO_SHARE: i32 = (ENVELOPE_ONE as i64 * 7 / 246) as i32;
/// The share of that depth that the TREMOLO tab gives, in Q30: 333 / 483.
///
/// Source: service manual, page 55: each oscillator's output reaches the
/// DEPTH knob directly under the CHORUS tab (DO1, DO2) and through 150 kΩ
/// under the TREMOLO tab (DO1', DO2'); page 66: the knob is 500 kΩ (VR4,
/// B 500 kΩ) and page 55 puts 1 MΩ on its wiper, which side by side at
/// the knob's top are 333 kΩ: 333 / (150 + 333).
const EFFECT_TREMOLO_DEPTH: i32 = (ENVELOPE_ONE as i64 * 333 / 483) as i32;

/// Thousandths of a fall's time that are its time constant: the time to
/// a tenth of the way, as the voices' envelopes.
const FALL_THOUSANDTHS: u64 = 434;
/// Thousandths of an attack's time that are its time constant.
const RISE_THOUSANDTHS: u64 = 1_000_000 / RcEnvelope::ATTACK_TIME_CONSTANTS_THOUSANDTHS as u64;
/// Thousandths.
const THOUSAND: u64 = 1_000;
/// Samples of output in a tick of the voices.
const TICK_OUTPUT_SAMPLES: u32 = TICK_SAMPLES / OVERSAMPLING.factor();
/// Ticks of the output's slow oscillators a second: they move with the
/// voices' ticks.
const _: () = assert!(TICK_RATE * TICK_OUTPUT_SAMPLES == SAMPLE_RATE);

/// Coarse or fine pitch (S80-47): where its controller put it, and where
/// it has slid to under a sounding note.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Tuning {
    held: u8,
    wanted: u8,
    /// How far it reaches either way, in cents.
    reach: u16,
}

impl Tuning {
    /// At its centre, which moves nothing (S80-47: 64 before any
    /// arrives).
    const fn new(reach: u16) -> Self {
        Self { held: law::LEVER_CENTRE, wanted: law::LEVER_CENTRE, reach }
    }

    /// Its controller moved: at once with no note sounding, else a step a
    /// tick (S80-37).
    const fn set(&mut self, value: u8, at_once: bool) {
        self.wanted = value;
        if at_once {
            self.held = value;
        }
    }

    /// A tick: a step towards where its controller put it.
    fn slide(&mut self) {
        self.held = match self.held.cmp(&self.wanted) {
            core::cmp::Ordering::Less => self.held.saturating_add(1),
            core::cmp::Ordering::Equal => self.held,
            core::cmp::Ordering::Greater => self.held.saturating_sub(1),
        };
    }

    /// Its offset in octaves: `(v − 64) × reach ÷ 64` cents below the
    /// centre and `(v − 64) × reach ÷ 63` above, a straight line in cents
    /// from each end to the centre (S80-47, the law of [`law::centred`]).
    fn octaves(self) -> i32 {
        let centre = i64::from(law::LEVER_CENTRE);
        let offset = i64::from(self.held).saturating_sub(centre);
        let travel = if offset < 0 { centre } else { i64::from(law::LEVER_MAX).saturating_sub(centre) };
        let cents = offset.saturating_mul(i64::from(self.reach)).saturating_mul(i64::from(OCTAVE));
        i32::try_from(cents.checked_div(travel.saturating_mul(CENTS_PER_OCTAVE)).unwrap_or(0)).unwrap_or(0)
    }
}

/// A slow oscillator: a phase and its step.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Wave {
    phase: u32,
    step: u32,
}

impl Wave {
    /// Moves on, and gives the phase it had.
    const fn tick(&mut self) -> u32 {
        let phase = self.phase;
        self.phase = phase.wrapping_add(self.step);
        phase
    }
}

/// The left shift from a signal's one to a level's.
const SINE_SHIFT: u32 = 6;
const _: () = assert!(crate::analog::LEVEL << SINE_SHIFT == ENVELOPE_ONE);

/// The sine of a phase as a level, in Q30 either way.
const fn sine_level(phase: u32) -> i32 {
    sine(phase) << SINE_SHIFT
}

/// A time of an envelope as its coefficient at the ticks' rate.
fn coefficient(time: u32, thousandths: u64) -> i32 {
    let constant = u64::from(time).saturating_mul(thousandths) / THOUSAND;
    time_constant(u32::try_from(constant).unwrap_or(u32::MAX), TICK_RATE)
}

/// The speed voltage C that a tab's divider makes of the SPEED knob at a
/// position, in the units of an octave to the volt: the knob is a
/// divider of the panel's supply, loaded at its wiper by both of
/// [`SPEED_DIVIDERS`].
fn effect_speed_volts(knob: u8, tab: usize) -> i32 {
    const FULL: i64 = law::LEVER_MAX as i64;
    let [(tremolo_upper, tremolo_lower), (chorus_upper, chorus_lower)] = SPEED_DIVIDERS;
    let (tremolo, chorus) = (tremolo_upper.saturating_add(tremolo_lower), chorus_upper.saturating_add(chorus_lower));
    // The two dividers side by side are tremolo × chorus / (tremolo +
    // chorus) ohms; a knob at x of its travel gives x of the supply
    // behind x (1 - x) of its own resistance.
    let (load_product, load_sum) = (tremolo.saturating_mul(chorus), tremolo.saturating_add(chorus));
    let position = i64::from(knob.min(law::LEVER_MAX));
    let behind = position.saturating_mul(FULL.saturating_sub(position)).saturating_mul(SPEED_KNOB);
    let supply = i64::from(PANEL_SUPPLY_TENTHS.saturating_mul(TENTH_OF_A_VOLT));
    let whole = FULL.saturating_mul(FULL).saturating_mul(load_product).saturating_add(behind.saturating_mul(load_sum));
    let wiper = supply
        .saturating_mul(position)
        .saturating_mul(FULL)
        .saturating_mul(load_product)
        .checked_div(whole)
        .unwrap_or(0);
    let (upper, lower) = SPEED_DIVIDERS.get(tab).copied().unwrap_or((0, 0));
    let volts = wiper.saturating_mul(lower).checked_div(upper.saturating_add(lower)).unwrap_or(0);
    i32::try_from(volts).unwrap_or(0)
}

/// The effect's delay lines and their filters, empty.
fn effect_line() -> Chorus {
    let mut line = Chorus::new(SAMPLE_RATE);
    line.set_filter_cutoff(frequency(EFFECT_FILTER, SAMPLE_RATE));
    line.set_delay(EFFECT_DELAY, 0);
    line
}

/// Messages kept while all sound off takes the sound out, to be taken
/// when it has gone (S80-29): more than come in the 5.3 ms that lasts but
/// from a stream that means to flood, whose next message ends the wait.
///
/// Source: own choice.
const DEFERRED: usize = 16;

/// What waits while all sound off takes the sound out: what the common
/// layer answered for a message, which it has already taken in order, or
/// the patch a program change chose, read from the banks when it came.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Deferred {
    /// An action for the voices.
    Act(Action),
    /// A patch to set.
    Patch(Patch),
}

/// What a deferred slot holds before anything waits in it.
const NOTHING_DEFERRED: Deferred = Deferred::Act(Action::None);

/// Whether the instrument acts on what the common layer answered, and so
/// how the receiver counts it (S80-1, MIDI-10).
fn classify(action: Action) -> Outcome {
    match action {
        // S80-7: every registered parameter but number 0, and every
        // non-registered one, selects nothing of synth80.
        Action::PassedOver | Action::Registered { .. } | Action::NonRegistered { .. } => Outcome::PassedOver,
        Action::Control { controller, .. } if !interface::moves(controller) => Outcome::PassedOver,
        _ => Outcome::Applied,
    }
}

/// synth80.
///
/// It holds no reference and allocates nothing. Two instruments given the
/// same patch and messages and rendered in calls of any lengths give the
/// same samples.
#[derive(Clone, Debug)]
pub struct Synth80 {
    /// The patch the instrument was given.
    wanted: Patch,
    /// A patch given by messages and not yet set: messages at one instant
    /// are set together, when the instrument next sounds or a key moves,
    /// so that a restatement that ends where the panel stands changes
    /// nothing (S80-46, MIDI-97).
    pending: Option<Patch>,
    /// The panel as it stands and is played: the given patch's switches
    /// and selectors, and its levers each on its way to where the patch
    /// has it ([`Synth80::slide`]).
    patch: Patch,
    /// All sound off has come and the sound is on its way out: the gain
    /// that takes it there, which the voices are cut at the end of; and
    /// the messages that came meanwhile, in order, and how many.
    cut: Option<VolumeGain>,
    deferred: [Deferred; DEFERRED],
    deferred_count: usize,
    /// The common layer (MIDI-84 to MIDI-92), as one channel: every
    /// channel's messages come to it as channel 1's (S80-2, S80-23).
    common: Common,
    channels: [Channel; 2],
    shared: Shared,
    allocator: Allocator<VOICES>,
    voices: [Voice; VOICES],
    /// The trigger count each voice last acted on.
    triggers: [u32; VOICES],
    /// Each channel's noise generator, and the sub oscillator's.
    noise: [Noise; 2],
    sub_noise: Noise,
    sub: Wave,
    /// The pressure the sub oscillator's speed was last set for.
    sub_pressed: i32,
    pwm: [Wave; 2],
    ring: Wave,
    ring_envelope: RcEnvelope,
    /// The ring modulator's share of the sound, in Q30.
    ring_modulation: i32,
    /// The effect's two oscillators, and how far each moves the delayed
    /// sound's level, in Q30.
    effect: [Wave; 2],
    effect_depth: i32,
    /// How far the effect's switches have gone over, in Q30: nothing with
    /// the effect off, one with it on ([`EFFECT_SWITCH_STEP`]).
    effect_on: i32,
    line: Chorus,
    /// The low-pass filter of the modulation amplifiers' outputs.
    wet_filter: OnePole,
    /// The pitch bend, in octaves, at the range registered parameter 0
    /// set (S80-7).
    bend: i32,
    /// Coarse and fine pitch (S80-47), each a value on its way to where
    /// its controller put it.
    tuning: [Tuning; 2],
    wheel: u8,
    volume: u8,
    /// The volume's gain as it stands, in Q28, on its way to what the
    /// controller asks.
    volume_gain: VolumeGain,
    /// Samples of the voices until the next tick.
    tick_wait: u32,
    decimator: Decimator,
    ignored: u64,
}

impl Default for Synth80 {
    fn default() -> Self {
        Self::new()
    }
}

impl Synth80 {
    /// An instrument at rest playing [`Patch::DEFAULT`].
    #[must_use]
    pub fn new() -> Self {
        let mut number = 0u32;
        let voices = core::array::from_fn(|_| {
            let voice = Voice::new(SEED, number);
            number = number.wrapping_add(1);
            voice
        });
        let mut synth80 = Self {
            wanted: Patch::DEFAULT,
            pending: None,
            cut: None,
            deferred: [NOTHING_DEFERRED; DEFERRED],
            deferred_count: 0,
            common: Common::new(CONFIG),
            patch: Patch::DEFAULT,
            channels: [Channel::new(&Patch::DEFAULT, 0), Channel::new(&Patch::DEFAULT, 1)],
            shared: Shared::new(&Patch::DEFAULT),
            // Each key takes the next voice round the eight: adjustment
            // procedure, page 6, the note beside figure 4: depressing a
            // key repeatedly "will allow you to select a desired master
            // board".
            allocator: Allocator::new(Mode::Poly(Assign::Cyclic)),
            voices,
            triggers: [0; VOICES],
            noise: [Noise::new(SEED), Noise::new(SEED.rotate_left(8))],
            sub_noise: Noise::new(SEED.rotate_left(16)),
            sub: Wave::default(),
            sub_pressed: -1,
            pwm: [Wave::default(); 2],
            ring: Wave::default(),
            ring_envelope: RcEnvelope::new(),
            ring_modulation: 0,
            effect: [Wave::default(); 2],
            effect_depth: 0,
            effect_on: 0,
            line: effect_line(),
            wet_filter: OnePole::new(),
            bend: 0,
            tuning: [Tuning::new(interface::COARSE_CENTS), Tuning::new(interface::FINE_CENTS)],
            wheel: 0,
            volume: crate::midi::common::VOLUME_FULL,
            volume_gain: VolumeGain::full(),
            tick_wait: 0,
            decimator: Decimator::sharp(),
            ignored: 0,
        };
        synth80.apply_patch();
        synth80
    }

    /// Puts the instrument back as [`Synth80::new`] made it, but for the
    /// patch, which it keeps: silent, its controllers at rest, the count
    /// of ignored messages at zero.
    pub fn reset(&mut self) {
        let patch = *self.patch();
        *self = Self::new();
        self.set_patch(&patch);
    }

    /// Gives the instrument a patch, which it copies. It takes effect on
    /// the voices that are sounding too, as moving the panel's controls
    /// does: the tabs and selectors at once, and each lever a step of
    /// its 128 at every tick, its whole travel in 21 thousandths of a
    /// second, as a hand moves it through every place between. With no
    /// voice sounding, all of it is there at once.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.pending = Some(*patch);
        self.flush();
    }

    /// Sets the patch messages gave, if it differs from the one given.
    fn flush(&mut self) {
        if let Some(patch) = self.pending.take()
            && patch != self.wanted
        {
            self.give(&patch);
        }
    }

    fn give(&mut self, patch: &Patch) {
        // The effect's lines are fed only while its switches are over at
        // all, so what they held when the switches came to rest at off is
        // not let out when it is switched on again. Switched on again
        // before that, they have been fed all along.
        if self.effect_on == 0 && patch.on(Global::Effect) {
            self.line = effect_line();
            self.wet_filter.reset();
        }
        // With nothing sounding there is nothing for the switches to
        // click on: they are where the patch has them at once.
        if self.allocator.sounding() == 0 {
            self.effect_on = if patch.on(Global::Effect) { ENVELOPE_ONE } else { 0 };
        }
        self.wanted = *patch;
        let mut bytes = patch.to_bytes();
        if self.allocator.sounding() > 0 {
            let levers = Self::levers();
            for ((byte, held), lever) in bytes.iter_mut().zip(self.patch.to_bytes()).zip(levers) {
                if lever {
                    *byte = held;
                }
            }
        }
        self.patch = Patch::from_bytes(&bytes).unwrap_or(*patch);
        self.apply_patch();
    }

    /// The patch the instrument was given.
    #[must_use]
    #[allow(
        clippy::misnamed_getters,
        reason = "the patch is the one given; the field of that name is the panel on its way to it"
    )]
    pub const fn patch(&self) -> &Patch {
        match &self.pending {
            Some(patch) => patch,
            None => &self.wanted,
        }
    }

    /// Which bytes of a patch are levers, which pass every place between
    /// two, and not tabs or selectors, which have none between (S80-37).
    fn levers() -> impl Iterator<Item = bool> {
        patch::KINDS.iter().map(|&kind| kind == Kind::Lever)
    }

    /// A tick of the levers: each goes one step towards where the given
    /// patch has it.
    ///
    /// Source: own choice of the rate. A lever is a potentiometer under
    /// a hand; a patch set under a sounding voice put it at its new
    /// place in one sample, which no hand does and which steps the
    /// signal. A step of a 128th at each tick of a sixth of a
    /// millisecond: long against a sample, short against a hand.
    fn slide(&mut self) {
        if self.patch == self.wanted {
            return;
        }
        let mut bytes = self.patch.to_bytes();
        for (held, wanted) in bytes.iter_mut().zip(self.wanted.to_bytes()) {
            *held = match (*held).cmp(&wanted) {
                core::cmp::Ordering::Less => held.saturating_add(1),
                core::cmp::Ordering::Equal => *held,
                core::cmp::Ordering::Greater => held.saturating_sub(1),
            };
        }
        self.patch = Patch::from_bytes(&bytes).unwrap_or(self.wanted);
        self.apply_patch();
    }

    fn apply_patch(&mut self) {
        let patch = self.patch;
        let lever = |global: Global| patch.get(global);
        self.channels = [Channel::new(&patch, 0), Channel::new(&patch, 1)];
        self.shared = Shared::new(&patch);
        for voice in &mut self.voices {
            voice.configure(&self.channels);
        }
        for (wave, tone) in self.pwm.iter_mut().zip(&patch.tones) {
            let speed = law::speed(SPEED_AT_FIVE_VOLTS, FAST_OCTAVES_A_VOLT, tone.get(Control::PwmSpeed), 0);
            wave.step = frequency(speed, TICK_RATE);
        }
        // The speed is set again at the next tick.
        self.sub_pressed = -1;

        self.ring_modulation = law::level(lever(Global::RingModulation));
        let attack = law::time(RING_ATTACK_SHORTEST, lever(Global::RingAttack));
        let decay = law::time(law::FALL_SHORTEST, lever(Global::RingDecay));
        self.ring_envelope.set_attack(coefficient(attack, RISE_THOUSANDTHS));
        self.ring_envelope.set_decay(coefficient(decay, FALL_THOUSANDTHS));
        self.ring_envelope.set_sustain(0);

        // The TREMOLO tab takes the speed voltage and the oscillators'
        // outputs from the CHORUS tab's contacts when it is down, so with
        // both down it is the tremolo; with neither, the speed voltage's
        // line and the DEPTH knob's top are open, and nothing moves
        // (service manual, page 66, SW5 and SW6).
        let (tab, share) = if patch.on(Global::Tremolo) {
            (Some(0), EFFECT_TREMOLO_DEPTH)
        } else if patch.on(Global::Chorus) {
            (Some(1), ENVELOPE_ONE)
        } else {
            (None, 0)
        };
        let volts = tab.map_or(0, |tab| effect_speed_volts(lever(Global::EffectSpeed), tab));
        let below = i64::from(effect_speed_volts(law::LEVER_MAX, 0).saturating_sub(volts));
        let slower =
            i32::try_from(below.saturating_mul(i64::from(EFFECT_OCTAVES_A_VOLT)) >> law::OCTAVE_BITS).unwrap_or(0);
        let [first, second] = &mut self.effect;
        // The steps are slowed, not the frequencies: a thousandth of a
        // hertz is a thirtieth of the slow oscillator's under CHORUS.
        first.step = law::raise(frequency(EFFECT_FASTEST.0, SAMPLE_RATE), slower.saturating_neg());
        second.step = law::raise(frequency(EFFECT_FASTEST.1, SAMPLE_RATE), slower.saturating_neg());
        self.effect_depth = apply(apply(law::level(lever(Global::EffectDepth)), EFFECT_MONO_SHARE), share);
        self.wet_filter.set_cutoff(frequency(EFFECT_WET_FILTER, SAMPLE_RATE));
    }

    /// How many messages the instrument has passed over since it was made
    /// or reset (S80-1: "counted as ignored").
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// How many voices are sounding, 0 to [`VOICES`].
    #[must_use]
    pub fn sounding_voices(&self) -> usize {
        self.allocator.sounding()
    }

    /// The common layer's state: the pitch bend and its range, the pedal,
    /// the parameter selected, the bank select value (MIDI-84 to MIDI-92).
    #[must_use]
    pub const fn common(&self) -> &Common {
        &self.common
    }

    /// Coarse and fine pitch as their controllers last set them, 64 at
    /// their centres (S80-47).
    #[must_use]
    pub const fn tuning(&self) -> (u8, u8) {
        let [coarse, fine] = self.tuning;
        (coarse.wanted, fine.wanted)
    }

    /// The modulation wheel and the volume, as their controllers last set
    /// them (S80-24, S80-25).
    #[must_use]
    pub const fn wheel_and_volume(&self) -> (u8, u8) {
        (self.wheel, self.volume)
    }

    /// Takes one MIDI 1.0 channel message, as [`Synth80::midi`] does, with
    /// no bank: a program change then sets the default patch. The tests'
    /// and the examples' way in.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        let _ = self.midi(&Message::Channel(ChannelMessage { status, data1, data2 }), &Banks::none());
    }

    /// Readies the instrument at a sound's start: program 0 of bank index
    /// 0, or the default patch where that is missing or faulty (S80-35,
    /// MIDI-61).
    pub fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let (patch, outcome) = interface::program_patch(banks, 0, 0);
        self.set_patch(&patch);
        // A bank that lacks program 0 leaves the default, which is no
        // fault of a message.
        if outcome == Outcome::PassedOver { Outcome::Applied } else { outcome }
    }

    /// Takes one message of a block (S80-1): what it does is the family
    /// document's, and every message it does not act on is counted and
    /// changes nothing. Never panics, whatever the message holds.
    pub fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        // S80-9: no system exclusive message is taken.
        let Message::Channel(message) = message else { return self.pass_over() };
        // S80-2: the sixteen channels are one keyboard and one panel, and
        // their state is one (S80-23).
        let message = ChannelMessage { status: message.status & KIND_MASK, ..*message };
        // S80-36: controller 32 selects nothing.
        if message.kind() == status::CONTROL_CHANGE && message.data1 == controller::BANK_SELECT_FINE {
            return self.pass_over();
        }
        let taken = self.common.take(message);
        if let Action::ProgramChange { program, bank_coarse, .. } = taken.action {
            let (patch, outcome) = interface::program_patch(banks, bank_coarse, program);
            if outcome == Outcome::PassedOver {
                self.ignore();
            }
            self.perform(Deferred::Patch(patch));
            return outcome;
        }
        let outcome = classify(taken.action);
        if outcome == Outcome::PassedOver {
            self.ignore();
        } else {
            self.perform(Deferred::Act(taken.action));
        }
        outcome
    }

    /// Input was lost (S80-39, MIDI-102): every key released, the pedal up
    /// and no parameter selected; the panel, the wheel, the bend and its
    /// range, the volume and the tuning stay.
    pub fn lost(&mut self) {
        self.release_all();
    }

    fn pass_over(&mut self) -> Outcome {
        self.ignore();
        Outcome::PassedOver
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    /// Does what the common layer answered, or keeps it while all sound
    /// off takes the sound out; with no room to keep it, the sound is cut
    /// now (S80-29).
    fn perform(&mut self, deferred: Deferred) -> bool {
        if self.cut.is_some() {
            if let Some(slot) = self.deferred.get_mut(self.deferred_count) {
                *slot = deferred;
                self.deferred_count = self.deferred_count.saturating_add(1);
                return false;
            }
            self.cut();
        }
        match deferred {
            Deferred::Patch(patch) => self.pending = Some(patch),
            Deferred::Act(action) => self.act(action),
        }
        true
    }

    fn act(&mut self, action: Action) {
        // What the panel's messages set is set before a key or the pedal
        // moves, since each may start or end a sound.
        if !matches!(
            action,
            Action::Control { .. }
                | Action::PitchBend { .. }
                | Action::BendRange { .. }
                | Action::Volume { .. }
                | Action::None
                | Action::PassedOver
        ) {
            self.flush();
        }
        match action {
            Action::NoteOn { key, velocity } => self.key_down(key, velocity),
            Action::NoteOff { key, .. } => self.key_up(key),
            // S80-26, MIDI-87: down, a released key goes on as if held;
            // up, every note so held is released.
            Action::Sustain { down } => self.allocator.sustain(down),
            Action::AllNotesOff => self.all_notes_off(),
            // The sound goes out over the 5.3 ms a volume's gain takes,
            // and the voices are cut and the keys let go when it has
            // gone: cut where they stood, the signal would fall to
            // nothing in one sample (S80-29).
            Action::AllSoundOff => {
                if self.allocator.sounding() == 0 {
                    self.cut();
                } else if self.cut.is_none() {
                    self.cut = Some(VolumeGain::full());
                }
            }
            // S80-28, MIDI-84, RP-015: the common layer has centred the
            // bend, cleared the parameter selection and the pressure; here
            // the wheel goes to 0, the pedal up and every key's pressure to
            // 0. The volume, the patch, the tuning and the bend range stay.
            Action::ResetAllControllers => {
                self.wheel = 0;
                self.allocator.sustain(false);
                for voice in &mut self.voices {
                    voice.press(0);
                }
                self.bend = self.bend_octaves();
            }
            Action::PitchBend { .. } | Action::BendRange { .. } => self.bend = self.bend_octaves(),
            Action::Volume { value } => self.volume = value,
            Action::ChannelPressure { pressure } => {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Held {
                        voice.press(pressure);
                    }
                }
            }
            Action::KeyPressure { key: pressed, pressure } => {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Held && key.key == pressed {
                        voice.press(pressure);
                    }
                }
            }
            Action::Control { controller, value } => self.control_change(controller, value),
            Action::ProgramChange { .. }
            | Action::Registered { .. }
            | Action::NonRegistered { .. }
            | Action::None
            | Action::PassedOver => {}
        }
    }

    /// The pitch bend in octaves: `(bend − 8,192) ÷ 8,192` of the range
    /// (S80-7, MIDI-91), linear in pitch.
    fn bend_octaves(&self) -> i32 {
        let state = self.common.channel(0);
        let offset = i64::from(state.bend).saturating_sub(i64::from(PITCH_BEND_CENTRE));
        let cents = offset.saturating_mul(i64::from(state.bend_range_cents));
        let whole = i64::from(PITCH_BEND_CENTRE).saturating_mul(CENTS_PER_OCTAVE);
        let octaves = cents.saturating_mul(i64::from(OCTAVE)).checked_div(whole).unwrap_or(0);
        i32::try_from(octaves).unwrap_or(0)
    }

    /// A control change the common layer handed on: the wheel, the tuning
    /// or a control of the panel (S80-10, S80-11, S80-18, S80-20, S80-24,
    /// S80-47).
    fn control_change(&mut self, number: u8, value: u8) {
        match number {
            controller::MODULATION => self.wheel = value,
            interface::COARSE_PITCH | interface::FINE_PITCH => {
                let index = usize::from(number == interface::FINE_PITCH);
                let quiet = self.allocator.sounding() == 0;
                if let Some(tuning) = self.tuning.get_mut(index) {
                    tuning.set(value, quiet);
                }
            }
            _ => {
                if let Some(place) = patch::place_of(number) {
                    let mut patch = *self.patch();
                    patch.set_byte(place, value);
                    self.pending = Some(patch);
                }
            }
        }
    }

    fn key_down(&mut self, key: u8, velocity: u8) {
        if !self.allocator.voices().iter().any(|voice| voice.stage == Stage::Held) {
            // The ring modulator's envelope starts with the first key
            // (owner's manual, page 14, the note under D), and a sustain
            // of the second kind ends: "The next chord or note played
            // ends any previous sustain" (page 11, B; S80-32).
            self.ring_envelope.retrigger();
            if self.patch.on(Global::SustainMode) {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Released {
                        voice.damp();
                    }
                }
            }
        }
        self.allocator.key_down(key, velocity);
        // A key newly down is not yet pressed (S80-5).
        for (voice, held) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if held.stage == Stage::Held && held.key == key {
                voice.press(0);
            }
        }
    }

    /// A key goes up; a note the pedal holds has no pressure on it, its
    /// key being up (S80-4).
    fn key_up(&mut self, key: u8) {
        self.allocator.key_up(key);
        for (voice, held) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if held.stage == Stage::Sustained && held.key == key {
                voice.press(0);
            }
        }
    }

    /// Every key held goes up as a note off would; what the pedal holds
    /// stays (S80-28, MIDI-85).
    fn all_notes_off(&mut self) {
        let mut keys = [None; VOICES];
        for (slot, voice) in keys.iter_mut().zip(self.allocator.voices()) {
            if voice.stage == Stage::Held {
                *slot = Some(voice.key);
            }
        }
        for key in keys.into_iter().flatten() {
            self.key_up(key);
        }
    }

    /// Silences every voice at once and lets every key go: what all
    /// sound off comes to. What waited is then done, in order.
    fn cut(&mut self) {
        self.flush();
        self.cut = None;
        self.allocator.all_keys_up();
        // The pedal stays where it was (S80-29); the allocator let it go
        // with the keys.
        self.allocator.sustain(self.common.channel(0).sustain);
        self.patch = self.wanted;
        self.apply_patch();
        for (index, voice) in self.voices.iter_mut().enumerate() {
            voice.cut(&self.channels);
            self.allocator.finished(index);
        }
        let (deferred, count) = (self.deferred, self.deferred_count);
        self.deferred_count = 0;
        for &waiting in deferred.iter().take(count) {
            let _ = self.perform(waiting);
        }
        self.flush();
    }

    /// Releases every voice and lifts the pedal: for when the input stops
    /// and no note off will come.
    pub fn release_all(&mut self) {
        self.flush();
        self.common.lost();
        self.allocator.all_keys_up();
    }

    /// The sub oscillator's waveform at a phase, in Q30 either way.
    fn sub_wave(&mut self, phase: u32) -> i32 {
        /// The shift from a phase to a level that falls or rises through
        /// two in a cycle.
        const RAMP_SHIFT: u32 = 1;
        let rising = (phase >> RAMP_SHIFT).cast_signed().wrapping_sub(ENVELOPE_ONE);
        match Function::from_value(self.patch.get(Global::SubFunction)) {
            Function::Sine => sine_level(phase),
            Function::Sawtooth => rising.wrapping_neg(),
            Function::InvertedSawtooth => rising,
            Function::Square => {
                if phase < 1 << 31 {
                    ENVELOPE_ONE
                } else {
                    -ENVELOPE_ONE
                }
            }
            Function::Noise => self.sub_noise.tick_level(),
            Function::External => 0,
        }
    }

    /// A tick: the slow oscillators move on, and the voices take what the
    /// keys and the controls ask.
    fn control(&mut self) {
        self.slide();
        let patch = self.patch;
        let keys = *self.allocator.voices();
        // One sub oscillator serves every key: the hardest pressed sets
        // its speed.
        let mut pressed = 0;
        for (voice, key) in self.voices.iter().zip(&keys) {
            if key.stage == Stage::Held {
                pressed = pressed.max(voice.pressure());
            }
        }
        if pressed != self.sub_pressed {
            self.sub_pressed = pressed;
            let volts = law::share(TOUCH_SPEED_VOLTS * OCTAVE, patch.get(Global::TouchSpeed));
            let speed = law::speed_under(
                SPEED_AT_FIVE_VOLTS,
                SUB_OCTAVES_A_VOLT,
                PANEL_SUPPLY_TENTHS,
                patch.get(Global::SubSpeed),
                law::scaled(volts, pressed),
            );
            self.sub.step = frequency(speed, TICK_RATE);
        }
        let sub_phase = self.sub.tick();
        let sub = self.sub_wave(sub_phase);
        let [pwm_one, pwm_two] = &mut self.pwm;
        let pwm = [sine_level(pwm_one.tick()), sine_level(pwm_two.tick())];

        let rise = self.ring_envelope.tick();
        let volts = law::share(RING_DEPTH_VOLTS * OCTAVE, patch.get(Global::RingDepth));
        let speed = law::speed_under(
            SPEED_AT_FIVE_VOLTS,
            RING_OCTAVES_A_VOLT,
            PANEL_SUPPLY_TENTHS,
            patch.get(Global::RingSpeed),
            law::scaled(volts, rise),
        );
        self.ring.step = frequency(speed, SAMPLE_RATE);

        // The tuning slides a step a tick, as a lever does (S80-47,
        // S80-37), and adds to the bend: both move the pitch and the
        // filters as the keys do (S80-7).
        let mut tuned = self.bend;
        for tuning in &mut self.tuning {
            tuning.slide();
            tuned = tuned.saturating_add(tuning.octaves());
        }
        let controls = Controls { sub, pwm, bend: tuned, wheel: law::share(law::cents(WHEEL_CENTS), self.wheel) };
        for (index, ((voice, key), seen)) in self.voices.iter_mut().zip(&keys).zip(&mut self.triggers).enumerate() {
            if key.stage == Stage::Rest {
                continue;
            }
            if *seen != key.triggers {
                *seen = key.triggers;
                voice.start(key.key, key.velocity, &self.shared);
            }
            if !key.gate() {
                voice.release();
            }
            voice.tick(&self.channels, &self.shared, controls);
            if key.stage == Stage::Released && voice.silent() {
                self.allocator.finished(index);
            }
        }
    }

    /// One sample of the voices' sum, at their rate.
    #[inline]
    fn voices_sample(&mut self) -> i32 {
        if self.tick_wait == 0 {
            self.tick_wait = TICK_SAMPLES;
            self.control();
        }
        self.tick_wait = self.tick_wait.saturating_sub(1);
        let [noise_one, noise_two] = &mut self.noise;
        let noise = [noise_one.tick(), noise_two.tick()];
        let mut sum = 0i32;
        for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if key.stage != Stage::Rest {
                sum = sum.saturating_add(voice.sample(&self.channels, noise));
            }
        }
        sum
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is at most that of sixteen lines and
    /// the effect, whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        if !out.is_empty() {
            self.flush();
        }
        let mut volume = volume::wanted(self.volume);
        for sample in out {
            let mut earlier = self.voices_sample();
            let mut later = self.voices_sample();
            // Faded before the decimator, which then ends on silence:
            // what it still holds when the voices are cut is nothing.
            if let Some(cut) = &mut self.cut {
                let gain = cut.towards(0);
                (earlier, later) = (amplify(earlier, gain), amplify(later, gain));
                if gain == 0 {
                    // The messages that waited are taken now, the
                    // volume's among them.
                    self.cut();
                    volume = volume::wanted(self.volume);
                }
            }
            // Read at every sample: a tick inside this call may have
            // moved the levers they come from.
            let ring = self.ring_modulation;
            let effect = self.patch.on(Global::Effect);
            let depth = self.effect_depth;
            let mut signal = self.decimator.process(earlier, later);
            if ring != 0 {
                // The sound times the oscillator takes the place of the
                // sound, by the MODULATION lever's share.
                let carrier = sine_level(self.ring.tick());
                let gain = ENVELOPE_ONE.saturating_sub(ring).saturating_add(apply(carrier, ring));
                signal = apply(signal, gain);
            }
            if effect || self.effect_on != 0 {
                // The dry sound's share, less the delayed sound, whose
                // level the two oscillators move against each other.
                let [first, second] = &mut self.effect;
                let moved = apply(sine_level(first.tick()).saturating_sub(sine_level(second.tick())), depth);
                let [line, _] = self.line.process(signal);
                let delayed = amplify(self.wet_filter.low_pass(line), EFFECT_WET);
                let delayed = apply(delayed, ENVELOPE_ONE.saturating_add(moved));
                let step = if effect { EFFECT_SWITCH_STEP } else { EFFECT_SWITCH_STEP.saturating_neg() };
                self.effect_on = self.effect_on.saturating_add(step).clamp(0, ENVELOPE_ONE);
                signal = if self.effect_on == ENVELOPE_ONE {
                    apply(signal, EFFECT_DRY).saturating_sub(delayed)
                } else {
                    // On the way over, the second path has the sound
                    // itself by what one pair of switches still passes
                    // and the delayed sound by what the other does.
                    let through = apply(ENVELOPE_ONE.saturating_sub(EFFECT_DRY), self.effect_on);
                    apply(signal, ENVELOPE_ONE.saturating_sub(through)).saturating_sub(apply(delayed, self.effect_on))
                };
            }
            *sample = to_sample(amplify(amplify(signal, self.volume_gain.towards(volume)), OUTPUT_GAIN));
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "a test, in f64"
)]
mod tests {
    use super::{
        EFFECT_DELAY, EFFECT_FASTEST, EFFECT_WET, EFFECT_WET_FILTER, Global, OCTAVE, Patch, SAMPLE_RATE, Synth80,
        effect_speed_volts,
    };

    /// The frequencies in hertz of the effect's two oscillators under a
    /// patch.
    fn speeds(tremolo: u8, chorus: u8, knob: u8) -> [f64; 2] {
        let mut patch = Patch::DEFAULT;
        patch.set(Global::Effect, super::patch::ON);
        patch.set(Global::Tremolo, tremolo * super::patch::ON);
        patch.set(Global::Chorus, chorus * super::patch::ON);
        patch.set(Global::EffectSpeed, knob);
        let mut synth80 = Synth80::new();
        synth80.set_patch(&patch);
        synth80.effect.map(|wave| f64::from(wave.step) * f64::from(SAMPLE_RATE) / 4_294_967_296.0)
    }

    #[test]
    fn the_effects_figures_are_the_drawings() {
        // The delayed sound's low-pass filter: 27 kΩ beside 100 kΩ, with
        // 0.015 µF.
        let resistance = 27.0e3 * 100.0e3 / 127.0e3;
        let cutoff = 1.0 / (std::f64::consts::TAU * resistance * 0.015e-6);
        assert!((cutoff / (f64::from(EFFECT_WET_FILTER) / 1.0e3) - 1.0).abs() < 1.0e-4, "{cutoff}");
        // The delayed sound: trimmed to stand as large as the dry at the
        // key B4 of 8', where the filter has already taken it down.
        let b4 = 440.0 * ((83.0 - 69.0) / 12.0f64).exp2();
        let wet = (0.65 + 0.58) * 82.0 / 100.0 * (b4 / cutoff).hypot(1.0) * 100.0 / 168.0;
        assert!((wet / (f64::from(EFFECT_WET) / 268_435_456.0) - 1.0).abs() < 1.0e-4, "{wet}");
        // 512 stages, two to a period of the clock of 10 µs.
        assert_eq!(EFFECT_DELAY, 512 / 2 * 10);
    }

    #[test]
    fn the_effects_speeds_follow_the_knob_through_each_tabs_divider() {
        let volts = |knob: u8, tab: usize| f64::from(effect_speed_volts(knob, tab)) / f64::from(OCTAVE);
        // The knob's top is the supply; its middle is loaded.
        assert!(
            (volts(127, 0) - 10.6 * 100.0 / 122.0).abs() < 1.0e-3 && (volts(127, 1) - 10.6 * 1.2 / 9.4).abs() < 1.0e-3
        );
        let x = 64.0 / 127.0;
        let wiper = 10.6 * x / (1.0 + x * (1.0 - x) * 10.0 / (122.0 * 9.4 / 131.4));
        assert!((volts(64, 0) - wiper * 100.0 / 122.0).abs() < 1.0e-3 && volts(0, 0).abs() < 1.0e-9);
        // The adjustment procedure's two cycles under TREMOLO at the top.
        let fastest = [f64::from(EFFECT_FASTEST.0) / 1.0e3, f64::from(EFFECT_FASTEST.1) / 1.0e3];
        let near = |found: [f64; 2], octaves: f64| {
            found
                .iter()
                .zip(&fastest)
                .all(|(found, fastest)| (found / (fastest * (-octaves).exp2()) - 1.0).abs() < 2.0e-3)
        };
        assert!(near(speeds(1, 0, 127), 0.0) && near(speeds(1, 1, 127), 0.0), "{:?}", speeds(1, 0, 127));
        // 0.8409 octave to the volt of the speed voltage.
        let slope = 0.981_378 * 437.0 / 510.0;
        let top = 10.6 * 100.0 / 122.0;
        assert!(near(speeds(0, 1, 127), slope * (top - 10.6 * 1.2 / 9.4)), "{:?}", speeds(0, 1, 127));
        assert!(near(speeds(1, 0, 64), slope * (top - wiper * 100.0 / 122.0)), "{:?}", speeds(1, 0, 64));
        assert!(near(speeds(1, 0, 0), slope * top) && near(speeds(0, 1, 0), slope * top), "{:?}", speeds(1, 0, 0));
    }
}
