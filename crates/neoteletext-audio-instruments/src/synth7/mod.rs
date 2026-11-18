//! synth7 (REQ-6412 items 5 and 6): a rack of sixteen parts
//! sharing sixteen notes of six operators, driven by MIDI messages and
//! rendered to 16-bit mono samples at 48,000 Hz, in integers.
//!
//! Which instrument it models is said in
//! `neoteletext-common/docs/audio/synthesizers.md`.
//!
//! - [`patch`]: a voice of the instrument, read from its 155 parameters or
//!   its 128 packed bytes and checked, and a bank of 32.
//! - [`functions`]: a part's fourteen function parameters.
//! - [`sysex`]: the single-voice dump, the 32-voice dump and the parameter
//!   change, read and written.
//! - [`interface`]: what the server needs of synth7 with no synthesis code
//!   (`server.md`, SRV-88): which messages it takes, the patch's packer and
//!   check, the bank's builder and check, and each part's tracker and
//!   restatement. It and the three modules above are built under the
//!   feature `interface` as well as `synth7`; everything else needs
//!   `synth7`.
//! - [`msfa`]: the parameter layer, translated from Google's
//!   music-synthesizer-for-android and under Apache-2.0: the envelopes,
//!   the pitch envelope, the low frequency oscillator, and what a key and
//!   a velocity make of a patch.
//! - [`kernel`]: the sound, this crate's own, built as the instrument's
//!   sound chip is: a phase, a table of the logarithm of a sine, an added
//!   attenuation and a table of powers of two, combined by 32 algorithms.
//! - [`tables`]: the kernel's constant tables.
//! - [`rack`]: the parts, the notes and the MIDI messages.
//! - [`midi`]: synth7 as the receiver plays it, through the common layer.
//!
//! The example plays the rack, which the feature `interface` alone leaves
//! out, so it is built only with the feature `synth7`:
//!
//! ```
//! # #[cfg(feature = "synth7")]
//! # {
//! use neoteletext_audio_instruments::synth7::Rack;
//!
//! let mut rack = Rack::new();
//! rack.event(0x90, 69, 100);
//! let mut samples = [0_i16; 960];
//! rack.render(&mut samples);
//! assert!(samples.iter().any(|&sample| sample != 0));
//! # }
//! ```

#[cfg(feature = "synth7")]
pub mod kernel;
#[cfg(feature = "synth7")]
pub mod midi;
#[cfg(feature = "synth7")]
pub mod msfa;
#[cfg(feature = "synth7")]
mod note;
#[cfg(feature = "synth7")]
pub mod rack;
#[cfg(feature = "synth7")]
pub mod tables;

pub mod functions;
pub mod interface;
pub mod patch;
pub mod sysex;

pub use functions::Functions;
pub use patch::{Bank, BankError, Field, OperatorParams, Patch, PatchError, VoiceParams};
#[cfg(feature = "synth7")]
pub use rack::Rack;
