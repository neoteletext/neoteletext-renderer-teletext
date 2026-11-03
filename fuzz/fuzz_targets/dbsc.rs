//! Device Bound Session Credentials' readers (REQ-TST-040, REQ-WEB-017):
//! the registration and challenge headers and the session instructions,
//! whose values must stay within their bounds; and the import of saved
//! sessions (TODO-1207), which never panics, and whose sessions export and
//! import again the same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::bytes::Writer;
use neoteletext_web::dbsc::{
    Answer, MAX_CHALLENGE, MAX_EXPORT_BYTES, MAX_SESSION_ID, MAX_SESSION_URL, Sessions, read_challenge,
    read_instructions, read_registration,
};

/// The store's seconds at which saved data is read.
const NOW: u64 = 1_000;

fuzz_target!(|data: &[u8]| {
    if let Some(registration) = read_registration(data) {
        assert!(registration.path.as_bytes().len() <= MAX_SESSION_URL);
        assert!(registration.challenge.is_none_or(|challenge| challenge.as_bytes().len() <= MAX_CHALLENGE));
    }
    if let Some(challenge) = read_challenge(data) {
        assert!(challenge.session.as_bytes().len() <= MAX_SESSION_ID);
        // An sf-string decodes to printable ASCII only.
        assert!(challenge.challenge.as_bytes().iter().all(|byte| (0x20..=0x7E).contains(byte)));
    }
    if let Ok(Answer::Continue(instructions)) = read_instructions(data) {
        assert!(instructions.credentials.iter().any(Option::is_some));
        assert!(instructions.credentials.iter().flatten().all(|name| !name.as_bytes().is_empty()));
    }
    let mut sessions = Sessions::new();
    if sessions.import(data, NOW).is_ok() {
        let mut saved = vec![0_u8; MAX_EXPORT_BYTES];
        let mut out = Writer::new(&mut saved);
        sessions.export(NOW, &mut out).expect("what was read fits the bound");
        let length = out.len();
        let mut again = Sessions::new();
        again.import(&saved[..length], NOW).expect("what was written reads");
        assert_eq!(again, sessions);
    }
});
