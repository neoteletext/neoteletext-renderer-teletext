//! synth80: eight voices of two complete lines each, every
//! line an oscillator, a high-pass and a low-pass filter in series, a
//! filter envelope and an amplifier with its own; keys that sense how
//! hard they are struck and how hard each is pressed; a sub oscillator, a
//! ring modulator and a tremolo, rendered from MIDI messages to 16-bit
//! mono samples at 48,000 Hz.
//!
//! Which instrument it models is said in
//! `neoteletext-common/docs/audio/synthesizers.md`.
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
//!   ([`RcEnvelope`](crate::analog::envelope::RcEnvelope)).
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
//! # How it is played and programmed
//!
//! As `neoteletext-common/docs/audio/synth80.md` specifies (cited as
//! "S80-n"), on the adopted interface of Deckard's Dream's controller
//! table and the common behaviours of `midi-architecture.md` ("MIDI-n"):
//!
//! - **Every channel is the one keyboard and panel** (S80-2): a message
//!   acts alike whatever its channel, and no MPE (S80-8).
//! - **Control changes move the panel** ([`patch::CONTROLLERS`]): channel
//!   I on 40 to 63, 65 and 66, channel II on 67 to 73, 119 and 75 to 92,
//!   the master controls on the adopted table's numbers, GLIDE MODE on 3
//!   (controller 120 is all sound off), and NeoTeletext's own 20 to 31 for
//!   the ring modulator, the SUSTAIN section and the tremolo/chorus
//!   (S80-10, S80-11, S80-18, S80-20). A lever slides a step a tick under
//!   a sounding note (S80-37).
//! - **A patch is the 85 values** of those controllers in a fixed order
//!   ([`Patch`], S80-33); a program change, after controller 0's bank
//!   select among up to three banks, sets one (S80-35, S80-36).
//! - **Pitch bend** is 14 bits over a range in semitones set by registered
//!   parameter number 0, 3 at first and 99 at most (S80-7), and the
//!   adopted interface's COARSE and FINE PITCH, controllers 93 and 94, add
//!   ±500 and ±100 cents outside the patch (S80-47). The filters follow
//!   both as they follow the keys.
//! - **Controller 64 is MIDI's sustain pedal**, holding released keys;
//!   the SUSTAIN lever is always in effect, as on the instrument with its
//!   SUSTAIN FOOT SWITCH assigner off (S80-26).
//! - **Velocity and pressure**, polyphonic and channel alike, drive the
//!   touch response's routes (S80-3, S80-5, S80-6); the modulation wheel,
//!   controller 1, which the instrument has not, lets the sub oscillator
//!   move the pitch by up to [`WHEEL_CENTS`] (S80-24).
//! - **The common messages** are `midi-architecture.md`'s: reset all
//!   controllers by RP-015 (S80-28), all notes off and the mode messages
//!   (S80-28), all sound off (S80-29), and after a lost block every key
//!   released and the pedal up (S80-39).
//!
//! [`interface`] holds what the server needs of this without the
//! synthesis: which messages are taken, the patch's packer and check, the
//! bank's builder and check, and the tracker that writes the
//! restatement (S80-46).
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
//!   level and fails, are in `tests/synth80_filter.rs`
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
//!   (`~/devel/synthesizer-reference/synth80/validation/`).
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
//!   (`EFFECT_OCTAVES_A_VOLT`). The drawings also carry changes by
//!   hand. The OE1 board as printed (page 56) has one of them, 150 kΩ in
//!   place of 39 kΩ at its two output amplifiers, which makes the board
//!   louder with the effect on and off alike and is nothing here; it
//!   does not have the others: the delayed sound's coupling capacitors
//!   are 1 µF on it where a hand has written 0.01 µF, which with their
//!   10 kΩ would keep the delayed sound to what is above 1.6 kHz and
//!   leave almost nothing of the comb below it. No page says which
//!   instruments have that change, and the one bulletin filed (E-46,
//!   January 1978) is about the pitch: the board as printed is what is
//!   built, and an instrument with the change would sound otherwise.
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
//!   oscillator's EXT position is its off position), no HIGH/LOW output
//!   switch. The instrument's PITCH knob is the adopted interface's
//!   coarse and fine pitch (S80-47).
//! - **Portamento is per voice**, each from the pitch it last had; the
//!   instrument's key assigner was not read closely enough to say that
//!   this is what it does. With the lever at its foot, or the selector
//!   at its centre, there is none (S80-27).
//! - **The four memories** are not kept: a memory is a tone, and
//!   [`Patch`] holds two.

#[cfg(feature = "synth80")]
mod engine;
pub mod interface;
#[cfg(feature = "synth80")]
pub mod law;
#[cfg(feature = "synth80")]
pub mod midi;
pub mod patch;
pub mod presets;
#[cfg(feature = "synth80")]
mod voice;

#[cfg(feature = "synth80")]
pub use engine::{OUTPUT_GAIN, Synth80, VOICES, WHEEL_CENTS};
pub use patch::{Control, Feet, Function, GlideMode, Global, PATCH_BYTES, Patch, PatchError, TONE_BYTES, Tone};
#[cfg(feature = "synth80")]
pub use voice::{OVERSAMPLING, RATE};
