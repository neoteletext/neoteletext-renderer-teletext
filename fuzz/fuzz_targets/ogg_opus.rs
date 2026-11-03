//! The Ogg Opus reader of `neottx sound --codec opus` (TODO-1228,
//! REQ-AUD-022, REQ-TST-040): any bytes as an Ogg Opus file. No input may
//! panic or overflow; what is accepted keeps to the profile, and written
//! out again it reads back the same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_audio::ogg::{read_ogg_opus, write_ogg_opus};
use neoteletext_audio::{OPUS_FRAME_SAMPLES, OPUS_MAX_PACKET};

fuzz_target!(|data: &[u8]| {
    let Ok(read) = read_ogg_opus(data) else { return };
    let count = read.count();
    assert!(read.packet_bytes >= 1 && read.packet_bytes <= OPUS_MAX_PACKET);
    assert!(count >= 1 && read.packets.len() == count * usize::from(read.packet_bytes));
    for packet in read.packets.chunks(usize::from(read.packet_bytes)) {
        assert!(neoteletext_opus::check_packet(packet).is_ok());
    }
    let samples = read.samples();
    assert!(samples as usize <= count * OPUS_FRAME_SAMPLES as usize);
    assert!(samples as usize > (count - 1) * OPUS_FRAME_SAMPLES as usize);
    let again = read_ogg_opus(&write_ogg_opus(&read.packets, read.packet_bytes, u64::from(samples), 1))
        .expect("what was read writes back");
    assert_eq!(again.samples(), samples);
    assert_eq!(again.packets, read.packets);
});
