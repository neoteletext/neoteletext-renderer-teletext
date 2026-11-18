//! synth106 and synth60: six voices of one
//! oscillator each, a high-pass filter, a four-pole low-pass filter that
//! oscillates, one envelope, one low-frequency oscillator and a chorus of
//! two bucket-brigade delay lines, rendered from MIDI messages to 16-bit
//! mono samples at 48,000 Hz.
//!
//! Which instrument it models is said in
//! `neoteletext-common/docs/audio/synthesizers.md`.
//!
//! # The instrument
//!
//! - **Oscillator.** A clock divided by a 16-bit timer, so a key's pitch
//!   is the clock over a whole number. synth106 divides 8 MHz, and
//!   its processor puts the bender and the low-frequency oscillator into
//!   the divisor; synth60 divides a master oscillator of 1.9 MHz that
//!   the bender and the low-frequency oscillator move. From each cycle
//!   come a sawtooth, a pulse whose width is set by hand or moved, a
//!   square an octave below, and there is noise. synth106's sawtooth
//!   is charged by a voltage from its 12-bit converter, so its height is
//!   12 V only as nearly as the converter's steps allow, and falls off
//!   above the highest key.
//! - **Filter.** The IR3109's four stages with feedback through one more
//!   amplifier ([`OtaCascade`](crate::analog::filter::OtaCascade)); the
//!   input is raised with the resonance, as the circuit does; from the
//!   slider's last tenth it oscillates, in tune.
//! - **Envelope.** synth106's is computed by its processor every
//!   4.2 ms and smoothed by 1 ms of resistor and capacitor; synth60's
//!   is a chip charging a capacitor.
//! - **After the voices**, at the output rate: the capacitor the sum
//!   passes into the jack board through, the high-pass filter (a
//!   switch of four positions), the amplifier's level, and the chorus:
//!   two delay lines swept oppositely by one slow triangle, mixed with
//!   the dry signal as the instrument's mono output mixes them
//!   (DEC-6437 item 2).
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from
//! Roland's service notes and owner's manuals of the Juno-106, Juno-60
//! and Juno-6, Roland's specification pages, the chorus block's figures
//! of the MN3009 and
//! the published write-ups named where each figure is used, with no code
//! and no table from any emulator or plugin. Two reference programs were
//! run as sealed programs and their output measured; where a figure here
//! was chosen with such a measurement in view, its `Source:` line says
//! so. Each constant names its source where it is defined, in
//! [`law`], the voice and here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **How a slider travels between its documented ends** is an own
//!   choice for the envelope's times, the low-frequency oscillator's
//!   rate, the resonance and the amplifier's level ([`law`]).
//! - **synth106's cutoff is in open conflict with its calibration.**
//!   The slider covers the specification's 5 Hz to 50 kHz, which puts
//!   the service notes' test program 3 (FREQ 6.3, RES 10, KYBD 10, key
//!   C4) at 1,650 Hz, where adjustment 7-1 trims every voice to 248 Hz,
//!   2.7 octaves below. No one exponential passes through both ends of
//!   the specification and that point, and a law hung from the point
//!   with its top at 50 kHz was tried and is no synth106: flat over the
//!   slider's lowest third and 41 Hz at its middle. What was read and
//!   what would settle it are at [`law::SYNTH106`]; the test of the
//!   calibration is in `tests/synth106_filter.rs`, ignored until it is
//!   settled.
//! - **The chorus's delay and sweep** are in no document read; the
//!   lines' filters are the building block's third-order ones at the
//!   schematic's 10 kHz, where the instrument has fourth-order ones; the
//!   chip's noise, which the instrument is known for, is left out.
//! - **synth60's low-frequency oscillator** is taken as a triangle.
//! - **synth60's sliders have synth106's 128 places**, the
//!   patch's. The instrument reads each to 8 bits and puts it out once
//!   in its processor's loop of 7.2 ms (service notes, pages 16 and 17,
//!   "ADC", "Pot data output" and figure 10), so a step of its cutoff
//!   slider is 81 cents where a step here is 163, taken at the next tick,
//!   a sixth of a millisecond. Under a resonant note a step is a step of
//!   the ringing's pitch on the instrument as it is here: its cutoff
//!   voltage is held on a capacitor with no resistor before it (CPU
//!   board schematic, C14). No sample steps for it: the control sweep
//!   (`tests/synth106_control_sweep.rs`) has a step of the slider under a
//!   resonant note at 0.88 of the note's own largest step.
//! - **The bass lift** of synth106's high-pass switch at 0 is one
//!   pole's shelf from two resistors and a capacitor of a path whose
//!   other parts were not read.
//! - **No portamento, no arpeggiator, no unison or Poly 2**, no key
//!   transpose, no bender on the filter (S106-35; TODO-6467); the wheel's
//!   depth, a knob beside the instrument's keyboard and no part of a
//!   patch, is fixed ([`WHEEL_CENTS`]), and the bender's is set by
//!   controller 87 and registered parameter 0 (S106-26).
//! - **Velocity is ignored**, as both instruments ignore it.
//!
//! # How it is played and programmed
//!
//! As `neoteletext-common/docs/audio/synth106-synth60.md` says (cited as
//! "S106-n"): the Juno-106's three system exclusive messages and its
//! channel messages, the JU-06A's controllers in its sound-module mode,
//! and the behaviours common to every synthesizer
//! (`midi-architecture.md`, MIDI-84 to MIDI-92). The messages' bytes, the
//! patch's forms, the banks and the restatement are [`interface`], which
//! is built without the sound engine behind the feature `interface` for
//! the server (`server.md`, SRV-88); [`midi::Midi`] is the instrument the
//! receiver plays.

#[cfg(feature = "synth106")]
mod engine;
pub mod interface;
#[cfg(feature = "synth106")]
pub mod law;
#[cfg(feature = "synth106")]
pub mod midi;
pub mod patch;
#[cfg(feature = "synth106")]
mod voice;

#[cfg(feature = "synth106")]
pub use engine::{PITCH_BEND_SEMITONES, Synth106, VOICES, WHEEL_CENTS};
pub use patch::{ChorusMode, Model, Patch, PatchError, PwmSource, Range, Slider};
