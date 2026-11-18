#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use std::borrow::ToOwned;
use std::vec::Vec;

/// A challenge of recognisable bytes: 0 to 31.
fn counting_challenge() -> [u8; CHALLENGE_BYTES] {
    core::array::from_fn(|index| u8::try_from(index).unwrap())
}

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes().chunks(2).map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap()).collect()
}

fn signed(lifetime: Lifetime, challenge: &[u8; CHALLENGE_BYTES], identity: &str) -> Result<Vec<u8>, StateError> {
    let identity = ChannelIdentity::read(identity)?;
    let mut out = [0_u8; MAX_SIGNED_BYTES];
    let length = signed_bytes(Algorithm::Ed25519, lifetime, challenge, &identity, &mut out)?;
    Ok(out[..length].to_vec())
}

/// VS-203: the signed bytes are exactly the domain and a zero byte, the
/// version, the algorithm, the lifetime, the challenge, the identity's
/// length in two bytes, big-endian, and the identity. The expected bytes
/// were written by an independent program (Python's own byte strings).
#[test]
fn the_signed_bytes_are_exactly_vs_203s() {
    // "NeoTeletext viewer state v1", 0; then the version, written in below.
    let mut expected = hex("4e656f54656c65746578742076696577657220737461746520763100");
    expected.push(SIGNED_VERSION);
    // Ed25519, remembered; the challenge 0 to 31; 25 bytes of identity.
    expected.extend(hex("0101000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f0019\
         68747470733a2f2f6578616d706c652e6f72672f6e6577732f"));
    let bytes = signed(Lifetime::Remembered, &counting_challenge(), "https://example.org/news/").unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), SIGNED_PREFIX_BYTES + "https://example.org/news/".len());
    assert_eq!(SIGNED_VERSION, u8::try_from(neoteletext_input::VERSION).unwrap(), "the push protocol's version");
}

/// VS-203: changing any one field changes the bytes, so a proof for one
/// lifetime, challenge or channel is no proof for another.
#[test]
fn every_field_changes_the_signed_bytes() {
    let challenge = counting_challenge();
    let base = signed(Lifetime::Visit, &challenge, "https://example.org/news/").unwrap();
    let mut other_challenge = challenge;
    other_challenge[31] ^= 1;
    for (name, other) in [
        ("lifetime", signed(Lifetime::Remembered, &challenge, "https://example.org/news/")),
        ("challenge", signed(Lifetime::Visit, &other_challenge, "https://example.org/news/")),
        ("scheme", signed(Lifetime::Visit, &challenge, "http://example.org/news/")),
        ("folder", signed(Lifetime::Visit, &challenge, "https://example.org/news/sport/")),
        ("port", signed(Lifetime::Visit, &challenge, "https://example.org:8443/news/")),
    ] {
        assert_ne!(other.unwrap(), base, "{name}");
    }
}

/// DOC-1863, VS-102: only the canonical form of a channel's folder is a
/// channel identity, so that browser and server sign and verify the same
/// bytes.
#[test]
fn only_a_canonical_channel_folder_is_an_identity() {
    for good in ["http://a/", "https://example.org/news/", "http://[::1]:8080/a/b/", "https://example.org:8443/"] {
        assert_eq!(
            ChannelIdentity::read(good).map(|identity| identity.as_str().to_owned()),
            Ok(good.to_owned()),
            "{good}"
        );
    }
    for (bad, error) in [
        ("http://a", StateError::IdentityLength),
        ("https://Example.org/news/", StateError::NotCanonical),
        ("HTTPS://example.org/news/", StateError::NotCanonical),
        ("https://example.org:443/news/", StateError::NotCanonical),
        ("http://example.org:80/", StateError::NotCanonical),
        ("https://example.org/news", StateError::NotCanonical),
        ("https://example.org/news/100.tti", StateError::NotCanonical),
        ("https://example.org/news/?a=1", StateError::NotCanonical),
        ("ws://example.org/news/", StateError::NotCanonical),
        ("https://example.org//", StateError::NotCanonical),
    ] {
        assert_eq!(ChannelIdentity::read(bad), Err(error), "{bad}");
        assert_eq!(signed(Lifetime::Visit, &counting_challenge(), bad), Err(error), "{bad}");
    }
}

/// VS-203: the identity's bounds, 9 to 523 bytes, are those of the
/// longest host and folder the channel reader takes.
#[test]
fn the_longest_channel_identity_fits() {
    let label = "a".repeat(63);
    let host = [label.as_str(), &label, &label, &"b".repeat(61)].join(".");
    assert_eq!(host.len(), MAX_HOST_NAME_LENGTH);
    let segment = "c".repeat(30);
    let folder = std::format!("/{}", std::format!("{segment}/").repeat(8));
    assert!(folder.len() <= MAX_FOLDER_PATH);
    let identity = std::format!("https://{host}:65535{folder}");
    assert!(identity.len() <= MAX_CHANNEL_IDENTITY_BYTES);
    assert!(ChannelIdentity::read(&identity).is_ok());
    let bytes = signed(Lifetime::Visit, &counting_challenge(), &identity).unwrap();
    assert_eq!(bytes.len(), SIGNED_PREFIX_BYTES + identity.len());
    let too_long = "a".repeat(MAX_CHANNEL_IDENTITY_BYTES + 1);
    assert_eq!(ChannelIdentity::read(&too_long), Err(StateError::IdentityLength));
}

/// The identity of a folder the channel reader made is its text.
#[test]
fn a_folders_identity_is_its_canonical_text() {
    let read = channel::parse_address("https://EXAMPLE.org:443/news/channel.ttx").unwrap();
    assert_eq!(ChannelIdentity::of(&read.folder).unwrap().as_str(), "https://example.org/news/");
    let read = channel::parse_address("http://Example.org:8080/").unwrap();
    assert_eq!(ChannelIdentity::of(&read.folder).unwrap().as_str(), "http://example.org:8080/");
    let page = Url::parse("https://example.org/news/100.tti").unwrap();
    assert_eq!(ChannelIdentity::of(&page), Err(StateError::NotCanonical), "a file is no folder");
    let push = Url::parse("wss://example.org/news/").unwrap();
    assert_eq!(ChannelIdentity::of(&push), Err(StateError::NotCanonical), "the push address is no channel");
}

/// VS-302: the key identifier is SHA-256 of "NeoTeletext key id v1", a
/// zero byte, the algorithm and the public key. The expected digests were
/// computed by an independent program (Python's `hashlib`); the first key
/// is RFC 8032 section 7.1's TEST 1 public key.
#[test]
fn the_key_identifier_is_vs_302s() {
    let rfc_8032_test_1 = hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    let public_key: [u8; PUBLIC_KEY_BYTES] = rfc_8032_test_1.try_into().unwrap();
    assert_eq!(
        key_id(Algorithm::Ed25519, &public_key).to_vec(),
        hex("0794d12c541ab1fe32f80407c22937485ab4261e8bf15fbbc2c84adfab2fa1eb")
    );
    assert_eq!(
        key_id(Algorithm::Ed25519, &[0x11; PUBLIC_KEY_BYTES]).to_vec(),
        hex("614932e34a749d2cf2372597d301ddd528d285dd2dd4f70f17bbf6e03d7e6588")
    );
}
