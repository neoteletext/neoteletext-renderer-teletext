//! synth8: the instrument (feature `synth8`) and its interface (feature
//! `interface`), programmed as `neoteletext-common/docs/audio/synth8.md`
//! specifies, cited here as "S8-n".
//!
//! - [`patch`] and [`interface`] hold the patch of 42 control-change values
//!   (S8-23), the laws of the values (S8-21, S8-30), which messages synth8
//!   takes (S8-1, S8-2), the bank image (S8-29) and the tracker the
//!   server restates synth8 from (S8-29 (e)). They hold no synthesis, and
//!   the server builds them alone (server.md, SRV-88).
//! - The instrument, [`Synth8`] and its front end to the receiver
//!   [`midi::Midi`], is behind the feature `synth8`.

pub mod interface;
pub mod patch;

#[cfg(feature = "synth8")]
mod assign;
#[cfg(feature = "synth8")]
mod instrument;
#[cfg(feature = "synth8")]
pub mod law;
#[cfg(feature = "synth8")]
pub mod midi;
#[cfg(feature = "synth8")]
mod panel;
#[cfg(feature = "synth8")]
mod voice;

#[cfg(feature = "synth8")]
pub use instrument::*;
pub use patch::{
    AssignMode, Kind, LfoWave, PATCH_BYTES, Param, Patch, PatchError, PwmSource, Vco1Range, Vco1Wave, Vco2Wave,
    VcoModulator,
};
