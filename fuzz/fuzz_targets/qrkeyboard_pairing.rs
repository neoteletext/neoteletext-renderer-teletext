//! The screen's side of pairing, fed arbitrary frames from the relay
//! (REQ-416, REQ-SEC-003; REQ-TST-040): a hostile relay or phone sends what
//! it likes before the keys are agreed, so every frame is answered or
//! refused without a panic. The first byte chooses no-PIN or PIN mode.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_qrkeyboard::crypto::{Role, SECRET_BYTES};
use neoteletext_qrkeyboard::pairing::{Pairing, Secret};

/// The most frames one input feeds, so that one input stays quick.
const MAX_FRAMES: usize = 16;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let secret = if mode & 1 == 0 { Secret::Key([7; SECRET_BYTES]) } else { Secret::Pin("1234".to_owned()) };
    let Ok(mut pairing) = Pairing::new(Role::Device, "CODE", &secret, &[0; 16]) else { return };
    let _ = pairing.start();
    // Frames are separated by their lengths: one byte, then that many bytes.
    let mut input = rest;
    for _ in 0..MAX_FRAMES {
        let Some((&length, tail)) = input.split_first() else { break };
        let (frame, tail) = tail.split_at(usize::from(length).min(tail.len()));
        let _ = pairing.receive(frame);
        input = tail;
    }
});
