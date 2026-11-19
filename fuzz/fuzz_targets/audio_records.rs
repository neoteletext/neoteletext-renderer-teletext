//! The audio profile's record parser and receiver, from arbitrary records
//! (REQ-AUD-021, REQ-TST-040): each input is split into records by a
//! two-byte length, read in order, with page changes between them.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_audio::port::{Music, SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::{Receiver, Sounding};

/// A small store, so that sounds are evicted.
const ARENA: usize = 8 * 1024;

/// Whether `music` is a level that exists: the channel's, or a magazine's
/// 1 to 8.
fn a_level(music: Music) -> bool {
    match music {
        Music::Channel => true,
        Music::Magazine(magazine) => (1..=8).contains(&magazine),
    }
}

/// A sink that checks what it is given and keeps nothing.
struct Checker;

impl VoiceSink for Checker {
    fn define(&mut self, _: SoundId, format: SoundFormat) {
        assert!(format.samples > 0);
    }
    fn samples(&mut self, _: SoundId, _: u32, pcm: &[i16]) {
        assert!(pcm.len() <= neoteletext_audio::MAX_BLOCK_SAMPLES);
    }
    fn forget(&mut self, _: SoundId) {}
    fn end_stream(&mut self, _: u8, _: SoundId) {}
    fn stream(&mut self, _: u8, _: SoundId, rate_hz: u32) {
        assert!(neoteletext_audio::SAMPLE_RATES.contains(&rate_hz));
    }
    fn stream_samples(&mut self, _: u8, _: SoundId, pcm: &[i16]) {
        assert!(pcm.len() <= neoteletext_audio::MAX_BLOCK_SAMPLES);
    }
    fn start(&mut self, channel: u8, start: Start) {
        assert!(
            channel < neoteletext_audio::CHANNELS
                && start.gain <= 100
                && start.fade_ms >= neoteletext_audio::MIN_FADE_MS
        );
        // REQ-6925 item 6: a magazine is 1 to 8.
        assert!(start.music.is_none_or(a_level));
    }
    fn stream_music(&mut self, channel: u8, _: SoundId, music: Option<Music>) {
        assert!(channel < neoteletext_audio::CHANNELS);
        assert!(music.is_none_or(a_level));
    }
    fn music_heard(&mut self, channel: u8, music: Music, _: bool) {
        assert!(channel < neoteletext_audio::CHANNELS && a_level(music));
    }
    fn stop(&mut self, channel: u8, _: u16) {
        assert!(channel < neoteletext_audio::CHANNELS);
    }
    fn gain(&mut self, channel: u8, gain: u8, _: u16) {
        assert!(channel < neoteletext_audio::CHANNELS && gain <= 100);
    }
}

fuzz_target!(|data: &[u8]| {
    let mut arena = vec![0u8; ARENA];
    let mut receiver = Receiver::<4, 2>::new(&mut arena);
    let mut sink = Checker;
    let mut rest = data;
    // The port's clock, moved on by each record so live sounds can stall.
    let mut now_ms: u64 = 0;
    while let [high, low, tail @ ..] = rest {
        now_ms += u64::from(*high) * 16;
        receiver.tick(now_ms, &mut sink);
        let length = usize::from(u16::from_be_bytes([*high, *low])) % 1100;
        let (record, after) = tail.split_at(length.min(tail.len()));
        // Half the records get a correct header and CRC-32, so the fuzzer
        // reaches the store and the commands, not only the frame check.
        let mut repaired = record.to_vec();
        if length & 1 == 0 && repaired.len() >= 8 {
            let body = repaired.len() - 8;
            // Either version a record may carry, and each type the receiver
            // reads: OBJECT, DATA, CONTROL and PAGE_MUSIC (REQ-6925,
            // REQ-6926).
            repaired[0] = if repaired[0] & 1 == 0 {
                neoteletext_audio::BASE_PROFILE_VERSION
            } else {
                neoteletext_audio::PROFILE_VERSION
            };
            // The three older types keep the bytes that chose them, so the
            // inputs kept from before read as they did.
            repaired[1] = match repaired[1] {
                0xF0.. => neoteletext_audio::record::TYPE_PAGE_MUSIC,
                kind => 1 + kind % 3,
            };
            repaired[2..4].copy_from_slice(&u16::try_from(body).unwrap_or(0).to_be_bytes());
            let crc = neoteletext_audio::crc::crc32(&repaired[..4 + body]);
            repaired[4 + body..].copy_from_slice(&crc.to_be_bytes());
        }
        receiver.push_record(&repaired, &mut sink);
        if let Some(&page) = record.first() {
            // Pages of two magazines and a few sub-codes, the capability of
            // magazine music agreed and not, and the user's setting.
            let number = 0x100 + u16::from(page % 4) + (u16::from(page >> 2 & 1) << 8);
            receiver.open(Some(number), u16::from(page >> 3 & 3), &mut sink);
            receiver.set_magazine_sound(page >> 5 & 3 != 0, &mut sink);
            receiver.set_page_sounds(page >> 7 == 0, &mut sink);
        }
        // What is armed as music is a level that exists, and the most
        // specific level sounding is one of them (REQ-6926 items 4 and 6).
        let mut heard = [false; 2];
        for channel in 0..neoteletext_audio::CHANNELS {
            if let Some(armed) = receiver.music(channel) {
                assert!(a_level(armed.music));
                heard[usize::from(armed.music == Music::Channel)] |= armed.heard;
            }
        }
        match receiver.sounding() {
            Sounding::Magazine(magazine) => assert!(heard[0] && (1..=8).contains(&magazine)),
            Sounding::Channel => assert!(heard[1] && !heard[0]),
            Sounding::Nothing => assert_eq!(heard, [false; 2]),
            Sounding::Page => {}
        }
        rest = after;
    }
});
