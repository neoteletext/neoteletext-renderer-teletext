//! Yamaha's CS-80 (1977): eight voices of two complete lines each, every
//! line an oscillator, a high-pass and a low-pass filter in series, a
//! filter envelope and an amplifier with its own; keys that sense how
//! hard they are struck and how hard each is pressed; a sub oscillator, a
//! ring modulator and a tremolo, rendered from MIDI messages to 16-bit
//! mono samples at 48,000 Hz.
//!
//! # The instrument
//!
//! - **Two channels.** Every key sounds two lines, one of channel I and
//!   one of channel II, each with its own tone (a row of the panel, a
//!   memory or a preset), its own stop of 16' to 2', and the MIX lever
//!   between them. Channel II can be detuned.
//! - **Oscillator** (IG00153, with the waveshaper IG00158). Its frequency
//!   is in proportion to the key voltage, 523.25 Hz to the volt at 8'. It
//!   gives a sawtooth, a square whose width is set from 50% to 90% and
//!   moved by the channel's own slow oscillator, noise, and a sine that
//!   goes past the filters to the amplifier.
//! - **Filters** (IG00156, twice): a high-pass then a low-pass, each a
//!   state-variable filter of 12 dB an octave with its own resonance
//!   ([`StateVariable`](crate::analog::filter::StateVariable)). Their
//!   cutoffs follow the key, and move an octave for each volt. The
//!   resonance is set by a filter's own lever and the overall lever
//!   together, a Q of 0.5 with both at their feet and of 5 with both at
//!   their tops; the filters do not oscillate by themselves.
//! - **Envelopes.** The filters' (IG00152) starts at an initial level
//!   below the levers' setting, rises to an attack level above it, decays
//!   to the setting and on release returns to the initial level. The
//!   amplifier's (IG00159) is attack, decay, sustain and release. Both
//!   are capacitors charged and discharged
//!   ([`RcEnvelope`]).
//! - **Touch.** How hard a key is struck (MIDI velocity) and how hard it
//!   is then pressed, each key by itself (MIDI polyphonic key pressure),
//!   move the brilliance and the level by each tone's four levers; what
//!   a stroke gives drains away in a time constant of 3.3 s, as the
//!   board that holds its voltage drains it;
//!   pressure also speeds the sub oscillator and deepens its vibrato and
//!   wah, and a hard stroke can start a note flat.
//! - **After the voices**, at the output rate: the ring modulator, whose
//!   own oscillator an attack and decay envelope speeds up; the
//!   tremolo/chorus, which adds the sound delayed by 2.56 ms, inverted
//!   and dulled, to the sound itself, and is switched in and out over 22
//!   ms; the volume.
//!
//! # What MIDI's controls are on this instrument
//!
//! - **Pitch bend is the ribbon**, which adds to or takes from the key
//!   voltage, and so multiplies every frequency: full up doubles it, an
//!   octave, as the owner's manual gives the ribbon's upward range; full
//!   down takes it to nothing, the manual's "sub-audio frequency", held
//!   here to eight octaves down. A bend is therefore a straight line in
//!   frequency, not in pitch, and far wider downwards than up. The
//!   filters follow it as they follow the keys.
//! - **Controller 64 is the foot switch** with its SUSTAIN assigner on:
//!   down, a released note dies away in the SUSTAIN lever's time instead
//!   of the tone's release time, if that is longer. It does not hold
//!   notes, as the instrument's does not.
//! - **Controller 1**, the modulation wheel, which the instrument has
//!   not, lets the sub oscillator move the pitch by up to
//!   [`WHEEL_CENTS`], as pressure does through the touch response's VCO
//!   lever.
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from
//! Yamaha's service manual (KEC-4223), its adjustment procedure and its
//! owner's manual, the pages of its CS-series servicing guide that Jürgen
//! Haible published, and Haible's analysis of the filter, with no
//! code and no table from any emulator or plugin. One reference program
//! was run as a sealed program and its output measured. Each constant
//! names its source where it is defined, in [`law`], the voice and here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **The custom chips' insides** are not published. What a control
//!   voltage does between a chip's documented ends is an own choice
//!   wherever its `Source:` line says so: above all how many octaves a
//!   volt of the sub oscillator is, how many volts a stroke gives, and
//!   how far the overall levers reach. The level is not among them: the
//!   touch and the keyboard control add their voltages to the LEVEL
//!   lever's at the amplifier's linear input, through resistors that are
//!   drawn (`voice`, `LEVEL_RESISTORS`).
//! - **BRASS 1 is brighter than the one real instrument heard.** Twelve
//!   keys of a real instrument's BRASS 1, struck hard, come to rest 1.39
//!   to 2.00 octaves above their notes; this reading comes to rest at
//!   2.92 on every key however it is struck, 1.19 octaves above them in
//!   the mean (10 s after the hardest stroke it still stands at 3.02).
//!   The real filter's peak also comes sooner (116 to 260 ms after the
//!   key for 170 to 390 ms). The fall from the peak agrees since the
//!   stroke's voltage drains away as the touch board drains it: on
//!   middle C a time constant of 2.73 s for the recording's 2.80 s. The
//!   fundamental falls 5.4 dB from 1 s to 10 s for the recordings' 4.0 dB
//!   in the mean (3.0 to 5.3 dB), by the drawings' resistors and a
//!   hardest stroke of 7.9 V. The figures, and a test that states the rest
//!   level and fails, are in `tests/cs80_filter.rs`
//!   (`brass_1_rests_where_a_real_instruments_does`, ignored). What the
//!   documents were asked:
//!   - *The preset's parts* are as drawn (T51, column C: 5.1 kΩ on the
//!     low-pass row), their supply is the diode's drop above ten volts,
//!     and the row's load is counted ([`presets`]). The gap would need
//!     the row at 3.0 V where the parts put it at 4.04 V.
//!   - *The filter's law* is the servicing guide's own formula
//!     ([`law::CUTOFF_AT_NOTHING`]), and the alignment fixes it at 3.3 V,
//!     two octaves above the note: the real instrument rests *below* its
//!     alignment point, so no slope of the lever's law closes the gap.
//!   - *The envelope* rests at zero (service manual, page 42, IG00152,
//!     pin 15: the sustain level is "normally fixed to zero(0) volt", and
//!     page 41 ties the pin to earth), starts at the initial level and
//!     returns to it: as it is here.
//!   - *The stroke's voltage is not held for ever*: the touch board
//!     keeps it on 0.33 µF with 10 MΩ across it (service manual, pages
//!     17 and 18), and it drains to nothing in a time constant of 3.3 s,
//!     as it does here. It ends at nothing, not below, so a held note
//!     rests where the softest stroke leaves it, and that is where the
//!     gap is measured.
//!   - *The overall BRILLIANCE lever* of the recorded instrument would
//!     have to stand a third of its travel from its centre.
//!
//!   How many volts a stroke gives is in no document either: a velocity
//!   of 127 is taken as the most the detector can pass, in a straight
//!   line below (own choice), and a pressure of 127 as the firm pressure
//!   the after-touch is trimmed with
//!   (`~/devel/teletext-reference/synthesizers/cs-80/validation/`).
//! - **The resonance between its ends** follows the same recordings and
//!   not a document ([`law::damping`]). What the drawings do say is that
//!   the chip has the mean of a filter's own lever and the overall
//!   RESONANCE lever, 220 kΩ from each and no resistor to earth (service
//!   manual, pages 39 and 41), and its Q of 5 with both at their most
//!   (page 42; adjustment procedure, page 11). So one lever alone reaches
//!   a Q of 2.75, and where the overall lever stands matters to every
//!   sound. It rests with the lever up, at its least (owner's manual,
//!   page 38, item 8: "Nominal setting is always with the lever up for
//!   minimum additional resonance"; page 31 draws it so), and so does
//!   the panel at rest here. BRASS 1's Q is then 1.58 by the straight
//!   line, where the recordings fit 2.7; it would be 2.71 with the lever
//!   at its centre, but where the recorded instrument's lever stood is
//!   not known, and the fundamental's fall on the same twelve keys is
//!   met with the lever up and not with it at its centre (5.4 dB and 8.3
//!   dB for the recordings' 4.0 dB).
//! - **The real sine is not pure**: recordings have its second and third
//!   harmonics 20 to 25 dB down, where the sine here has them 77 dB
//!   down. The documents give no figure for it: the waveshaper's sheet
//!   (service manual, page 42, IG00158) only names the pins and the
//!   sine's 3 V, and the adjustment procedure has no step for it.
//! - **The effect's levels rest on one reading.** The drawings give
//!   every part of the tremolo/chorus (service manual, pages 55 to 58,
//!   and the panel's wiring on page 66), and it is built from them: the
//!   dry sound stays in, the delayed sound is added to it inverted, and
//!   both oscillators run under either tab. What no page prints is how
//!   large the sound is where it enters the board, so how large the
//!   delayed sound is against the dry is a reading of the adjustment
//!   procedure's trimming figures (`EFFECT_WET`); and how many octaves
//!   a volt is at the oscillators' chip is taken from the pulse width's
//!   oscillator, which is the same chip behind another pair of resistors
//!   (`EFFECT_OCTAVES_A_VOLT`). The drawings also carry later changes
//!   by hand (the delayed sound's coupling capacitors of 1 µF made 0.01
//!   µF, among others) that the boards as printed do not have; the
//!   boards as printed are what is built.
//! - **The filters do not distort**, and the loss of resonance at high
//!   frequencies that Haible finds (180 pF across the damping) is left
//!   out.
//! - **The tremolo/chorus is what the mono output has of it.** The
//!   circuit puts the sound through two delay lines of one fixed length
//!   and moves the delayed sound's level in the left and right outputs
//!   against each other. In their sum the delayed sound is steady but
//!   for the amplifiers' imbalance, a few per cent: what is heard is the
//!   comb that the delayed sound makes with the dry, which the owner's
//!   manual calls "a phasing-type TREMOLO" (page 13, item 15). The
//!   profile is mono (DEC-6437 item 2).
//! - **No expression pedal and no wah**, no external input (the sub
//!   oscillator's EXT position is its off position), no coarse and fine
//!   PITCH knob, no HIGH/LOW output switch.
//! - **Portamento is per voice**, each from the pitch it last had; the
//!   instrument's key assigner was not read closely enough to say that
//!   this is what it does. With the lever at its foot there is none.
//! - **The four memories** are not kept: a memory is a tone, and
//!   [`Patch`] holds two.

pub mod law;
pub mod patch;
pub mod presets;
mod voice;

use crate::SAMPLE_RATE;
use crate::analog::allocator::{Allocator, Assign, Mode, Stage};
use crate::analog::chorus::Chorus;
use crate::analog::decimator::Decimator;
use crate::analog::envelope::{ENVELOPE_ONE, RcEnvelope, apply, time_constant};
use crate::analog::filter::{ONE_Q28, OnePole};
use crate::analog::fixed::amplify;
use crate::analog::oscillator::Noise;
use crate::analog::{frequency, to_sample};

use law::OCTAVE;
pub use patch::{Control, Feet, Function, Global, PATCH_BYTES, Patch, PatchError, TONE_BYTES, Tone};
use voice::{Channel, Controls, RIBBON_REST, Shared, TICK_RATE, TICK_SAMPLES, Voice, sine};
pub use voice::{OVERSAMPLING, RATE};

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

/// The pitch bend value that bends nothing, and the bits below it.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;
/// The least the ribbon makes of a frequency, in Q28: eight octaves down.
///
/// Source: own choice of the floor. Owner's manual, page 14, item 19: "you
/// can move the highest note on the keyboard down to a sub-audio
/// frequency", and upwards "approximately one octave".
const RIBBON_FLOOR: u32 = RIBBON_REST >> 8;
/// The largest value of a MIDI data byte.
const DATA_MAX: u8 = 0x7f;
/// The foot switch is down at this value and above.
///
/// Source: MIDI 1.0: a switch controller is on from 64.
const PEDAL_DOWN: u8 = 64;
/// MIDI status: note off.
const STATUS_NOTE_OFF: u8 = 0x80;
/// MIDI status: note on.
const STATUS_NOTE_ON: u8 = 0x90;
/// MIDI status: polyphonic key pressure.
const STATUS_KEY_PRESSURE: u8 = 0xa0;
/// MIDI status: control change.
const STATUS_CONTROL_CHANGE: u8 = 0xb0;
/// MIDI status: channel pressure.
const STATUS_CHANNEL_PRESSURE: u8 = 0xd0;
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
/// wiring to I2 to I5), times 82 / 100, 1.009 Vp-p. The sine at 10 is
/// trimmed to "1.00 ± .02 Vp-p" at a voice's amplifier (page 15, VCA,
/// step 1), and the delayed sound is taken to be trimmed to stand as
/// large as the dry at that key: that the tone enters the board at that
/// 1 V is not printed anywhere. At that key the amplifiers' low-pass
/// filter ([`EFFECT_WET_FILTER`]) has taken the delayed sound down by
/// √(1 + (987.8 / 499.1)²), 2.2176, which low frequencies are spared:
/// 1.0086 × 2.2176 = 2.2365 (`tests/cs80_modulation.rs` works it out).
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
/// The square of the largest volume, which the volume's square is a share
/// of.
const VOLUME_FULL: i64 = (DATA_MAX as i64) * (DATA_MAX as i64);
/// Samples of output in a tick of the voices.
const TICK_OUTPUT_SAMPLES: u32 = TICK_SAMPLES / OVERSAMPLING.factor();
/// Ticks of the output's slow oscillators a second: they move with the
/// voices' ticks.
const _: () = assert!(TICK_RATE * TICK_OUTPUT_SAMPLES == SAMPLE_RATE);

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

/// The CS-80.
///
/// It holds no reference and allocates nothing. Two instruments given the
/// same patch and messages and rendered in calls of any lengths give the
/// same samples.
#[derive(Clone, Debug)]
pub struct Cs80 {
    patch: Patch,
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
    /// The pitch bend, less its centre.
    bend: i32,
    wheel: u8,
    volume: u8,
    pedal: bool,
    /// Samples of the voices until the next tick.
    tick_wait: u32,
    decimator: Decimator,
    ignored: u64,
}

impl Default for Cs80 {
    fn default() -> Self {
        Self::new()
    }
}

impl Cs80 {
    /// An instrument at rest playing [`Patch::DEFAULT`].
    #[must_use]
    pub fn new() -> Self {
        let mut number = 0u32;
        let voices = core::array::from_fn(|_| {
            let voice = Voice::new(SEED, number);
            number = number.wrapping_add(1);
            voice
        });
        let mut cs80 = Self {
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
            wheel: 0,
            volume: DATA_MAX,
            pedal: false,
            tick_wait: 0,
            decimator: Decimator::sharp(),
            ignored: 0,
        };
        cs80.apply_patch();
        cs80
    }

    /// Puts the instrument back as [`Cs80::new`] made it, but for the
    /// patch, which it keeps: silent, its controllers at rest, the count
    /// of ignored messages at zero.
    pub fn reset(&mut self) {
        let patch = self.patch;
        *self = Self::new();
        self.set_patch(&patch);
    }

    /// Gives the instrument a patch, which it copies. It takes effect at
    /// once, on the voices that are sounding too, as moving the panel's
    /// levers does.
    pub fn set_patch(&mut self, patch: &Patch) {
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
        self.patch = *patch;
        self.apply_patch();
    }

    /// The patch being played.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        &self.patch
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

    /// How many messages [`Cs80::event`] has ignored since the
    /// instrument was made or reset.
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// How many voices are sounding, 0 to [`VOICES`].
    #[must_use]
    pub fn sounding_voices(&self) -> usize {
        self.allocator.sounding()
    }

    /// Takes one MIDI 1.0 channel voice message. The instrument is one
    /// part and hears every channel.
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n); the velocity is how hard the key is struck.
    /// - Polyphonic key pressure (0xAn): how hard the key `data1` is
    ///   pressed, `data2`.
    /// - Channel pressure (0xDn): how hard every key that is down is
    ///   pressed, `data1`; `data2` is not used.
    /// - Control change (0xBn) of the modulation wheel (1), volume (7),
    ///   the foot switch (64, down from 64), all sound off (120), reset
    ///   all controllers (121, which leaves the volume) and all notes off
    ///   (123).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven: the ribbon.
    ///
    /// Anything else is ignored and counted ([`Cs80::ignored_events`]):
    /// any other status or controller, a data byte above 127. Nothing is
    /// an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        if data1 > DATA_MAX || data2 > DATA_MAX {
            self.ignore();
            return;
        }
        match status & 0xf0 {
            STATUS_NOTE_OFF => self.allocator.key_up(data1),
            STATUS_NOTE_ON if data2 == 0 => self.allocator.key_up(data1),
            STATUS_NOTE_ON => self.key_down(data1, data2),
            STATUS_KEY_PRESSURE => {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Held && key.key == data1 {
                        voice.press(data2);
                    }
                }
            }
            STATUS_CHANNEL_PRESSURE => {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Held {
                        voice.press(data1);
                    }
                }
            }
            STATUS_CONTROL_CHANGE => self.control_change(data1, data2),
            STATUS_PITCH_BEND => {
                self.bend = (i32::from(data2) << 7 | i32::from(data1)).saturating_sub(PITCH_BEND_CENTRE);
            }
            _ => self.ignore(),
        }
    }

    fn key_down(&mut self, key: u8, velocity: u8) {
        if !self.allocator.voices().iter().any(|voice| voice.stage == Stage::Held) {
            // The ring modulator's envelope starts with the first key
            // (owner's manual, page 14, the note under D), and a sustain
            // of the second kind ends: "The next chord or note played
            // ends any previous sustain" (page 11, B).
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
        // A key newly down is not yet pressed.
        for (voice, held) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if held.stage == Stage::Held && held.key == key {
                voice.press(0);
            }
        }
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            CONTROL_MODULATION_WHEEL => self.wheel = value,
            CONTROL_VOLUME => self.volume = value,
            CONTROL_SUSTAIN => self.pedal = value >= PEDAL_DOWN,
            CONTROL_ALL_SOUND_OFF => {
                self.allocator.all_keys_up();
                for (index, voice) in self.voices.iter_mut().enumerate() {
                    voice.cut(&self.channels);
                    self.allocator.finished(index);
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                self.pedal = false;
                for voice in &mut self.voices {
                    voice.press(0);
                }
            }
            CONTROL_ALL_NOTES_OFF => self.allocator.all_keys_up(),
            _ => self.ignore(),
        }
    }

    /// Releases every voice: for when the input stops and no note off
    /// will come.
    pub fn release_all(&mut self) {
        self.allocator.all_keys_up();
    }

    /// The sub oscillator's waveform at a phase, in Q30 either way.
    fn sub_wave(&mut self, phase: u32) -> i32 {
        /// The shift from a phase to a level that falls or rises through
        /// two in a cycle.
        const RAMP_SHIFT: u32 = 1;
        let rising = (phase >> RAMP_SHIFT).cast_signed().wrapping_sub(ENVELOPE_ONE);
        match Function::from_position(self.patch.get(Global::SubFunction)) {
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

        // The ribbon adds its share of the key voltage: a bend of b
        // makes every frequency 1 + b times what it was.
        let ribbon = law::share_of(RIBBON_REST.cast_signed(), self.bend, PITCH_BEND_CENTRE);
        let controls = Controls {
            sub,
            pwm,
            ribbon: RIBBON_REST.saturating_add_signed(ribbon).max(RIBBON_FLOOR),
            wheel: law::share(law::cents(WHEEL_CENTS), self.wheel),
            pedal: self.pedal,
        };
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
        // The square of the controller's share, in Q28 (own choice of
        // the law, as a volume control's taper).
        let square = i64::from(self.volume).saturating_mul(i64::from(self.volume));
        let volume = i32::try_from(square.saturating_mul(i64::from(ONE_Q28)) / VOLUME_FULL).unwrap_or(ONE_Q28);
        let ring = self.ring_modulation;
        let effect = self.patch.on(Global::Effect);
        let depth = self.effect_depth;
        for sample in out {
            let earlier = self.voices_sample();
            let later = self.voices_sample();
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
            *sample = to_sample(amplify(amplify(signal, volume), OUTPUT_GAIN));
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
        Cs80, EFFECT_DELAY, EFFECT_FASTEST, EFFECT_WET, EFFECT_WET_FILTER, Global, OCTAVE, Patch, SAMPLE_RATE,
        effect_speed_volts,
    };

    /// The frequencies in hertz of the effect's two oscillators under a
    /// patch.
    fn speeds(tremolo: u8, chorus: u8, knob: u8) -> [f64; 2] {
        let mut patch = Patch::DEFAULT;
        patch.set(Global::Effect, 1);
        patch.set(Global::Tremolo, tremolo);
        patch.set(Global::Chorus, chorus);
        patch.set(Global::EffectSpeed, knob);
        let mut cs80 = Cs80::new();
        cs80.set_patch(&patch);
        cs80.effect.map(|wave| f64::from(wave.step) * f64::from(SAMPLE_RATE) / 4_294_967_296.0)
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
