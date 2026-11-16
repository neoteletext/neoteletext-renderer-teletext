//! The Yamaha DX7 (REQ-6412 items 5 and 6): a rack of sixteen parts
//! sharing sixteen notes of six operators, driven by MIDI messages and
//! rendered to 16-bit mono samples at 48,000 Hz, in integers.
//!
//! - [`patch`]: a voice of the instrument, unpacked from its 128 bytes and
//!   checked, and a bank of 32.
//! - [`msfa`]: the parameter layer, translated from Google's
//!   music-synthesizer-for-android and under Apache-2.0: the envelopes,
//!   the pitch envelope, the low frequency oscillator, and what a key and
//!   a velocity make of a patch.
//! - [`kernel`]: the sound, this crate's own, built as the instrument's
//!   sound chip is: a phase, a table of the logarithm of a sine, an added
//!   attenuation and a table of powers of two, combined by 32 algorithms.
//! - [`tables`]: the kernel's constant tables.
//! - [`rack`]: the parts, the notes and the MIDI messages.
//!
//! ```
//! use neoteletext_audio_instruments::dx7::Rack;
//!
//! let mut rack = Rack::new();
//! rack.event(0x90, 69, 100);
//! let mut samples = [0_i16; 960];
//! rack.render(&mut samples);
//! assert!(samples.iter().any(|&sample| sample != 0));
//! ```

pub mod kernel;
pub mod msfa;
mod note;
pub mod patch;
pub mod rack;
pub mod tables;

pub use patch::{Bank, BankError, Field, OperatorParams, Patch, PatchError, VoiceParams};
pub use rack::Rack;
