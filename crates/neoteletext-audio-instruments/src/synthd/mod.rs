//! synthD: one voice of three oscillators, noise
//! and an external input mixed into the four-pole transistor ladder
//! filter, two contour generators and an amplifier, rendered from MIDI
//! messages to 16-bit mono samples at 48,000 Hz.
//!
//! Which instrument it models is said in
//! `neoteletext-common/docs/audio/synthesizers.md`.
//!
//! # The instrument
//!
//! - **Oscillators.** Three, each with a range switch of six positions
//!   (LO, 32' to 2') and a waveform switch of six (triangle, the triangle
//!   with a little sawtooth, sawtooth, square, and a wide and a narrow
//!   rectangular wave; oscillator 3 has a reversed sawtooth in the second
//!   place). Oscillators 2 and 3 have a FREQUENCY knob of seven semitones
//!   either way. Oscillator 3 may be taken off the keyboard, when the
//!   same knob covers 2.75 octaves either way, and at LO it is the
//!   instrument's slow oscillator. Each drifts a little, from a seed.
//! - **Mixer.** The three oscillators, white or pink noise, and the
//!   external input, which here is fed from the instrument's own output,
//!   as the well-known patch feeds it with a cord: its volume is how much
//!   returns. What the mixer sums drives the filter and the amplifier
//!   after it into their curve: one oscillator at 10 a little, three
//!   clearly.
//! - **Filter.** The ladder ([`Ladder`](crate::analog::filter::Ladder)): 24 dB an octave, its EMPHASIS
//!   bringing it to oscillation from 7.5 on the dial, its cutoff set by
//!   the knob over ten octaves, by a third, two thirds or all of the
//!   keyboard's voltage, by its contour and by the modulation.
//! - **Contours.** Two, of attack, decay and sustain, each a capacitor
//!   charged and discharged
//!   ([`RcEnvelope`](crate::analog::envelope::RcEnvelope)); the DECAY switch makes the
//!   decay time the release too, and without it a released note is cut
//!   in a few milliseconds.
//! - **Modulation.** Oscillator 3 and noise, mixed by MODULATION MIX and
//!   let through by the modulation wheel, to the oscillators' pitch, the
//!   cutoff, or both.
//! - **Keyboard.** One note, the lowest key held; the contours start only
//!   when a key goes down with none held; GLIDE slides the pitch.
//!
//! # How it is played
//!
//! synthD is played and programmed as `neoteletext-common/docs/audio/
//! synthd.md` says (cited as "SD-n"): the Minimoog Voyager's controller
//! map for every control the two panels share, NeoTeletext's controllers
//! 89, 90 and 102 to 113 for the rest, a patch of the 60 controllers'
//! values, and the common layer of every synthesizer
//! (`midi-architecture.md`, MIDI-84 to MIDI-92). [`interface`] holds the
//! map, the patch's bytes, the bank and the restatement without the
//! synthesis, behind the feature `interface`; [`midi::Midi`] is the
//! instrument the receiver plays.
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from Moog's
//! service manual and its schematics, R. J. Folkman's Technical Service
//! Manual (1973), the owner's manual, the 1974 sound charts and Timothy
//! Stinchcombe's analysis of the ladder, with no code and no table from
//! any emulator or plugin. One reference program was run as a sealed
//! program and its output measured; no figure here was taken from it.
//! Each constant names its source where it is defined, in [`law`] and
//! here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **The ladder is the building block's**: the textbook hyperbolic
//!   tangent in each stage at one saturation level, which passes what
//!   is well below its cutoff unbent at any level. What bends a loud
//!   mixer is here the amplifier after the ladder, taken as one more
//!   such curve at the same level ([`law::OUTPUT_STAGE`]): the level is
//!   an own choice, and so is the feedback above oscillation. The
//!   filter's input and output coupling, its bias and its transistors'
//!   mismatch are not modelled.
//! - **The contours' peak, the attack's asymptote and the cut of a
//!   released note** are own readings of a schematic that was hard to
//!   read ([`law::CONTOUR`], [`law::ATTACK_TARGET`],
//!   [`law::DUMP_TIME`]).
//! - **The knobs' laws between their documented ends** are own choices
//!   for the EMPHASIS, the GLIDE, the volumes and the modulation wheel.
//! - **The external input** is only the instrument's own output, through
//!   a preamplifier whose gain is an own choice.
//! - **The pink and red noise** are an own design, not the instrument's
//!   three filters; the level of the noise is an own reading of "-5dB".
//! - **The oscillators' waveforms are ideal**: the sawtooth's reset
//!   time, the triangle's and the rectangular waves' shaping errors and
//!   the levels' small differences between positions are left out, and
//!   so are the tracking errors the HI END COMP trimmers correct.
//! - **Oscillator 3's pitch off the keyboard** is an own choice
//!   ([`law::FREE_KEY`]).
//! - **What the ladder's curve folds back.** The oscillators by
//!   themselves leave nothing below 20 kHz that is no harmonic within
//!   91 dB (`analog::oscillator`). The ladder's stages bend their edges,
//!   and at four times the output rate, where the voice runs, what that
//!   makes above 172 kHz comes back below 20 kHz. Against the note's
//!   strongest harmonic, with the filter open: one sawtooth at 10 leaves
//!   92 dB at 110 Hz, 74 dB at 880 Hz and 61 dB at 4,186 Hz, the top key
//!   at 2'; at 5, 80 dB or better to 1,760 Hz and 72 dB at the top key;
//!   the narrow rectangular wave 4 to 6 dB less than the sawtooth; three
//!   sawtooths at 10 in step, 78 dB at 110 Hz and 46 dB at 4,186 Hz; a
//!   triangle, 99 dB at worst. From 880 Hz up at 10 that is short of the
//!   80 dB the oscillators are held to; at twice the output rate every
//!   figure was 6 to 9 dB worse. The amplifier's curve after the ladder
//!   is not the cause: without it one oscillator's figures move by less
//!   than 1 dB (`tests/synthd_aliasing.rs` holds the figures).
//! - **The noise is made at twice the output rate** and each of its
//!   samples held for two of the voice's, which takes 0.6 dB off it at
//!   20 kHz.
//! - **Velocity is ignored**, as the keyboard has none; there is no
//!   A-440 tone and no external control input.

pub mod interface;
#[cfg(feature = "synthd")]
pub mod law;
#[cfg(feature = "synthd")]
pub mod midi;
#[cfg(feature = "synthd")]
pub mod patch;
#[cfg(feature = "synthd")]
mod voice;

#[cfg(feature = "synthd")]
pub use interface::PatchFault;
#[cfg(feature = "synthd")]
pub use patch::{ContourPatch, OscillatorPatch, Patch, Range, Waveform};
#[cfg(feature = "synthd")]
pub use voice::{OSCILLATORS, OUTPUT_GAIN, SynthD};

/// Bytes of a patch (SD-22).
pub const PATCH_BYTES: usize = interface::PATCH_BYTES;
