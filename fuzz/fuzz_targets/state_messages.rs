//! Viewer state's messages, receiver to server, and the channel identity a
//! server reads from its configuration (DEC-6471, VS-203, VS-204,
//! VS-301; REQ-TST-040): the readers never panic; every `STATE_PROOF`,
//! `STATE_NONE` and `STATE_OFFER` they accept writes back to the same bytes;
//! and every identity accepted is canonical, writes back the same, and
//! builds bytes to sign of the length VS-203 gives.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_input::event::{Event, Item, read, write};
use neoteletext_input::state::{CHALLENGE_BYTES, read_none, read_offer, read_proof};
use neoteletext_web::viewer_state::{
    Algorithm, ChannelIdentity, Lifetime, MAX_SIGNED_BYTES, SIGNED_PREFIX_BYTES, signed_bytes,
};

/// Room for any one of the three messages.
const MESSAGE_BYTES: usize = 128;

fuzz_target!(|data: &[u8]| {
    let Some((&choice, body)) = data.split_first() else { return };
    match choice % 3 {
        0 => {
            // Payloads, through each reader.
            let event = match (read_proof(body), read_none(body), read_offer(body)) {
                (Ok(proof), ..) => Event::StateProof(proof),
                (_, Ok(reason), _) => Event::StateNone(reason),
                (.., Ok(())) => Event::StateOffer,
                _ => return,
            };
            let mut out = [0_u8; MESSAGE_BYTES];
            let length = write(&Item::Event(event), &mut out).expect("what was read is written");
            let (again, used) = read(&out[..length]).expect("what was written reads").expect("an item");
            assert_eq!((again, used), (Item::Event(event), length));
            assert!(out[..length].ends_with(body), "the payload is written as it was read");
        }
        1 => {
            // The whole stream: an item of any kind, written back the same
            // when it is one of viewer state's.
            let Ok(Some((Item::Event(event @ (Event::StateProof(_) | Event::StateNone(_) | Event::StateOffer)), used))) =
                read(body)
            else {
                return;
            };
            let mut out = [0_u8; MESSAGE_BYTES];
            let length = write(&Item::Event(event), &mut out).expect("what was read is written");
            assert_eq!(&out[..length], &body[..used]);
        }
        _ => {
            let Ok(text) = core::str::from_utf8(body) else { return };
            let Ok(identity) = ChannelIdentity::read(text) else { return };
            assert_eq!(identity.as_str(), text);
            let mut out = [0_u8; MAX_SIGNED_BYTES];
            // Under each version spoken (DEC-6953 item 2), and no other.
            let challenge = [0; CHALLENGE_BYTES];
            for version in 0..=u32::from(choice) {
                let signed =
                    signed_bytes(version, Algorithm::Ed25519, Lifetime::Visit, &challenge, &identity, &mut out);
                if neoteletext_input::speaks(version) {
                    assert_eq!(signed.expect("a channel identity is signed"), SIGNED_PREFIX_BYTES + text.len());
                } else {
                    assert!(signed.is_err(), "version {version} is not spoken");
                }
            }
        }
    }
});
