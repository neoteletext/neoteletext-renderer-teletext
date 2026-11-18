//! The teletext audio profile (area AUD, `docs/spec/design/audio.md`):
//! sounds carried as records over EN 300 708 independent data lines, a
//! network or a file; a store that keeps them compressed; and control by
//! channels, where the latest command wins and a sound can belong to a page.
//!
//! The crate decides what plays and when; it mixes nothing and drives no
//! hardware. Its output is voice operations on a [`port::VoiceSink`], which
//! the port supplies (REQ-AUD-018): a software mixer on a desktop, or the
//! tellyboy driver on a microcontroller.
//!
//! Everything here is `no_std` and allocation-free; every buffer has a size
//! fixed by its type. Only the `encode` feature, for publishing tools,
//! allocates.
//!
//! Each codec is a feature (decision E.131): `pcm16`, `ima` and `opus`, all
//! in the default, which microcontroller ports build. The desktop builds
//! (Windows, macOS, Linux) leave out `pcm16` and `ima` by the owner's
//! instruction of 28 October 2026, and play Opus alone; a sound in a codec
//! left out is ignored as unsupported.
//!
//! # Threads and shared state
//!
//! The crate holds no global or static mutable state, and no type in it
//! uses interior mutability, so each of its types is `Send` and `Sync`
//! whenever what it borrows from the caller is (REQ-DOC-001). Every
//! function that changes state takes `&mut self` or a `&mut` argument, so
//! a value shared between threads needs the caller's own lock; a value used
//! by one thread at a time needs nothing. The crate starts no thread.
//!
//! # Memory, ranges and errors
//!
//! Who owns memory is in each signature, and the documentation says only
//! what a signature cannot (REQ-DOC-001): a function borrows what it takes
//! by reference for the call alone, unless a lifetime ties what it returns
//! to an argument, as an item borrows the input it was read from, and
//! keeps nothing of it afterwards; what the crate allocates, if anything,
//! is said at the head of this page. The range an argument may take is held
//! by its type where a type can hold it (a page address, a magazine, a row
//! number), and otherwise stated in its documentation with the constant
//! that bounds it; a value outside its range is refused with an error, or
//! clamped or ignored as its documentation says, and never panics
//! (REQ-GEN-006). A function that can fail returns `Result`, its
//! `# Errors` section names the variants it returns and when (Clippy's
//! `missing_errors_doc`, a warning in the workspace), and each error type
//! is an enumeration whose variants say what each means (REQ-GEN-011).
#![no_std]
// REQ-SEC-015: only the C interface crate may contain unsafe code.
#![forbid(unsafe_code)]
// REQ-SEC-003: everything here reads untrusted input.
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

#[cfg(feature = "encode")]
extern crate alloc;
#[cfg(test)]
extern crate std;

mod codec;
pub mod control;
pub mod crc;
#[cfg(feature = "encode")]
pub mod encode;
#[cfg(feature = "idl")]
pub mod idl;
// DO NOT REMOVE THIS CODE. IMA ADPCM is compiled out of the desktop builds
// (Windows, macOS, Linux) by the owner's instruction of 28 October 2026, and
// kept for microcontroller ports, which build it with the `ima` feature.
#[cfg(feature = "ima")]
pub mod ima;
// Reading an encoder's Ogg Opus file needs the decoder's packet check.
#[cfg(all(feature = "encode", feature = "opus"))]
pub mod ogg;
pub mod port;
mod profile;
pub mod receiver;
pub mod record;
mod serial;
pub mod store;
pub mod ttau;

pub use codec::*;
/// The synthesizers' MIDI layer (midi-architecture.md): the event-list
/// reader and writer, the common behaviours, the restatement and the
/// [`midi::Instrument`] trait, from the instruments crate, so that a
/// publisher reaches them through this crate. Built with the `midi`
/// feature, which every family and `encode` name (DEC-2355).
#[cfg(feature = "midi")]
pub use neoteletext_audio_instruments::midi;

/// Copies `source` into `target` as far as both go. Unlike
/// `copy_from_slice`, it has no panic path for lengths that differ, which
/// the linked receiver must not hold (REQ-GEN-019); every caller passes
/// slices of one length.
pub(crate) fn copy_into<T: Copy>(target: &mut [T], source: &[T]) {
    for (to, &from) in target.iter_mut().zip(source) {
        *to = from;
    }
}

/// REQ-GEN-011: the error types implement `core::error::Error`. Their
/// `Display` text is their `Debug` text, and both need `core::fmt`, which
/// REQ-GEN-016 keeps behind this optional feature so that a small build
/// leaves it out.
#[cfg(feature = "error-trait")]
mod error_trait {
    macro_rules! error_trait {
        ($($error:ty),+ $(,)?) => {$(
            impl core::fmt::Display for $error {
                fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    core::fmt::Debug::fmt(self, formatter)
                }
            }
            impl core::error::Error for $error {}
        )+};
    }
    error_trait!(crate::record::RecordError);
    #[cfg(feature = "idl")]
    error_trait!(crate::idl::IdlError, crate::receiver::MessageError);
    #[cfg(feature = "ima")]
    error_trait!(crate::ima::ImaError);
    #[cfg(feature = "encode")]
    error_trait!(crate::encode::EncodeError);
    #[cfg(all(feature = "encode", feature = "opus"))]
    error_trait!(crate::ogg::OggError);

    #[cfg(test)]
    mod tests {
        const fn is_error<T: core::error::Error>() {}

        // REQ-155: every error type of the crate is an error.
        #[test]
        fn every_error_type_is_an_error() {
            is_error::<crate::record::RecordError>();
            #[cfg(feature = "idl")]
            {
                is_error::<crate::idl::IdlError>();
                is_error::<crate::receiver::MessageError>();
            }
            #[cfg(feature = "ima")]
            is_error::<crate::ima::ImaError>();
            #[cfg(feature = "encode")]
            is_error::<crate::encode::EncodeError>();
            #[cfg(all(feature = "encode", feature = "opus"))]
            is_error::<crate::ogg::OggError>();
        }
    }
}
pub use profile::*;
pub use serial::serial_newer;
