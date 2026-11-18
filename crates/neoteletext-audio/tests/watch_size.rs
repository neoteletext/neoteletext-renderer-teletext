//! What the `watch` feature costs a receiver in memory (rdm TODO-6551
//! step 1; REQ-6527; `neoteletext-common/docs/page-rules.md` RULE-30): with
//! it, each of the receiver's lanes has room for what a page's rules read
//! of the sound it renders. The test prints the receiver's size in the
//! build it runs in, so that running it with and without the feature gives
//! the difference, and with the feature holds the room of one lane to a
//! budget.
#![cfg(any(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
#![allow(clippy::print_stdout, reason = "a measurement prints its figures")]

use neoteletext_audio::receiver::Receiver;

/// The objects and the reassembly slots of the receiver measured: those
/// the crate's own tests use.
type Measured = Receiver<'static, 8, 2>;

/// The most bytes one lane's tracked state may take: measured at 7,144 on
/// x86-64 on 18 November 2026, with one family built and with every
/// family, and a tenth again allowed over it. The receiver of this test
/// was 83,376 bytes without the feature and 111,960 with it, every family
/// built: 28,584 more, the four lanes' tracked states and the byte of
/// watched channels.
#[cfg(feature = "watch")]
const WATCHED_BUDGET_BYTES: usize = 7_860;

/// TODO-6551 step 1 (REQ-6527): the receiver's size, and with `watch` the
/// room each lane keeps for a sound's tracked state, which is what the
/// feature adds: one for each of the receiver's lanes.
#[test]
fn the_watch_feature_adds_one_tracked_state_for_each_lane() {
    let receiver = size_of::<Measured>();
    println!("watch feature: {}", cfg!(feature = "watch"));
    println!("size of Receiver<8, 2>: {receiver} bytes");
    #[cfg(feature = "watch")]
    {
        let watched = size_of::<Option<neoteletext_audio_instruments::watch::Watched>>();
        let lanes = neoteletext_audio::INSTRUMENT_LANES;
        println!("size of one lane's tracked state: {watched} bytes");
        println!("lanes: {lanes}; tracked state of all lanes: {} bytes", watched * lanes);
        assert!(watched <= WATCHED_BUDGET_BYTES, "one lane's tracked state is {watched} bytes");
        assert!(receiver >= watched * lanes);
    }
}
