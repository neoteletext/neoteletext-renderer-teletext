//! Tests of `neottx sound` (REQ-CLI-011, REQ-AUD-020): a WAVE file written as the
//! audio profile's packets plays back, through the receiver, as the same
//! sound on the page it was written for; written live (REQ-AUD-014), it
//! plays as it arrives at the player's pace.
#![cfg(feature = "sound")]
#![allow(missing_docs)]

mod common;

use common::{folder, neottx, wave};

use neoteletext_audio::port::{SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::Receiver;
use neoteletext_core::consts::PACKET_BYTES;
use neoteletext_core::packet::Packet;

/// A sink that keeps the samples it is given and the starts.
#[derive(Default)]
struct Keeper {
    pcm: Vec<i16>,
    starts: Vec<(u8, Start)>,
    stops: usize,
}

impl VoiceSink for Keeper {
    fn define(&mut self, _: SoundId, format: SoundFormat) {
        self.pcm = vec![0; format.samples as usize];
    }
    fn samples(&mut self, _: SoundId, first: u32, pcm: &[i16]) {
        self.pcm[first as usize..first as usize + pcm.len()].copy_from_slice(pcm);
    }
    fn forget(&mut self, _: SoundId) {}
    fn stream(&mut self, _: SoundId, _: u32) {
        self.pcm.clear();
    }
    fn stream_samples(&mut self, _: SoundId, pcm: &[i16]) {
        self.pcm.extend_from_slice(pcm);
    }
    fn start(&mut self, channel: u8, start: Start) {
        self.starts.push((channel, start));
    }
    fn stop(&mut self, _: u8, _: u16) {
        self.stops += 1;
    }
    fn gain(&mut self, _: u8, _: u8, _: u16) {}
}

/// Writes `samples` with `codec` for page 123, channel 2, and plays the
/// packets back.
fn round_trip(name: &str, samples: &[i16], codec: &str) -> Keeper {
    let folder = folder(name);
    let input = folder.join("sound.wav");
    let output = folder.join("sound.t42");
    std::fs::write(&input, wave(samples, 22_050)).unwrap();
    let run = neottx(&[
        "sound",
        "--page",
        "123",
        "--channel",
        "2",
        "--codec",
        codec,
        input.to_str().unwrap(),
        output.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let stream = std::fs::read(&output).unwrap();
    assert_eq!(stream.len() % PACKET_BYTES, 0, "whole packets");
    let mut arena = vec![0; 1 << 20];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Keeper::default();
    receiver.open_page(Some(0x123), &mut sink);
    for bytes in stream.as_chunks::<PACKET_BYTES>().0 {
        assert!(receiver.push_packet(&Packet { bytes: *bytes }, &mut sink), "every packet is audio");
    }
    sink
}

fn tone(count: usize) -> Vec<i16> {
    (0..count).map(|n| i16::try_from(i32::try_from(n % 100).unwrap() * 300 - 15_000).unwrap()).collect()
}

#[test]
fn pcm_plays_back_exactly_on_its_page() {
    let samples = tone(5000);
    let sink = round_trip("pcm", &samples, "pcm");
    assert_eq!(sink.starts.len(), 1);
    assert_eq!(sink.starts[0].0, 2);
    assert_eq!(sink.starts[0].1.gain, 80);
    assert_eq!(sink.pcm, samples);
}

#[test]
fn ima_adpcm_plays_back_closely() {
    let samples: Vec<i16> = (0..20_000).map(|n| (f64::from(n) * 0.05).sin().mul_add(12_000.0, 0.0) as i16).collect();
    let sink = round_trip("ima", &samples, "ima");
    assert_eq!(sink.starts.len(), 1);
    assert_eq!(sink.pcm.len(), samples.len());
    let signal: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    let noise: f64 = samples.iter().zip(&sink.pcm).map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2)).sum();
    let snr = 10.0 * (signal / noise).log10();
    assert!(snr > 25.0, "signal to noise {snr:.1} dB");
}

#[test]
fn a_rate_outside_the_profile_is_refused() {
    let folder = folder("rate");
    let input = folder.join("sound.wav");
    let mut bytes = wave(&tone(10), 22_050);
    bytes[24..28].copy_from_slice(&12_345_u32.to_le_bytes());
    std::fs::write(&input, bytes).unwrap();
    let run = neottx(&["sound", "--page", "100", input.to_str().unwrap(), folder.join("out.t42").to_str().unwrap()]);
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("Rate(12345)"), "{}", String::from_utf8_lossy(&run.stderr));
}

/// Signal to noise of `decoded` against `samples`, in decibels.
fn snr(samples: &[i16], decoded: &[i16]) -> f64 {
    let signal: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    let noise: f64 = samples.iter().zip(decoded).map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2)).sum();
    10.0 * (signal / noise).log10()
}

#[test]
fn a_live_sound_plays_as_the_player_reaches_it_and_fades_when_it_ends() {
    let folder = folder("live");
    let input = folder.join("sound.wav");
    let output = folder.join("sound.t42");
    // Two seconds at 8 kHz.
    let samples: Vec<i16> = (0..16_000).map(|n| (f64::from(n) * 0.05).sin().mul_add(12_000.0, 0.0) as i16).collect();
    std::fs::write(&input, wave(&samples, 8_000)).unwrap();
    let run = neottx(&["sound", "--page", "123", "--live", input.to_str().unwrap(), output.to_str().unwrap()]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    // REQ-96: the report names the samples, the rate, the bytes of
    // sound and the packets; 32 blocks of 256 bytes are 8,192.
    let report = String::from_utf8_lossy(&run.stderr);
    assert!(report.contains("16000 samples at 8000 Hz, 8192 bytes of ima sent live 1 times, 654 packets"), "{report}");
    let stream = std::fs::read(&output).unwrap();
    let packets = stream.as_chunks::<PACKET_BYTES>().0;
    // 32 blocks of 505 samples, the last due at 2.02 s: 646 packets at the
    // browser's 320 a second, then the last block's 8.
    assert_eq!(packets.len(), 654);
    let mut arena = vec![0; 1 << 20];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Keeper::default();
    receiver.open_page(Some(0x123), &mut sink);
    let mut heard_at_one_second = 0;
    for (index, bytes) in (0_u64..).zip(packets) {
        let now_ms = index * 1000 / 320;
        receiver.tick(now_ms, &mut sink);
        assert!(receiver.push_packet(&Packet { bytes: *bytes }, &mut sink), "every packet is audio");
        if now_ms == 1000 {
            heard_at_one_second = sink.pcm.len();
        }
    }
    assert_eq!(sink.starts.len(), 1, "one play");
    // Each block reaches the sink as the player reaches it: a second in, a
    // second of samples, less the block not yet whole and the three a lost
    // block would be waited for.
    assert!((7_000..=8_000).contains(&heard_at_one_second), "{heard_at_one_second} samples a second in");
    assert!(sink.pcm.len() >= 15_000, "{} samples", sink.pcm.len());
    let decoded = snr(&samples[..sink.pcm.len().min(samples.len())], &sink.pcm);
    assert!(decoded > 25.0, "signal to noise {decoded:.1} dB");
    // The sender stops with the last block, at packet 653 (2 040 ms): two
    // seconds later the sound fades out.
    receiver.tick(4_039, &mut sink);
    assert_eq!(sink.stops, 0);
    receiver.tick(4_040, &mut sink);
    assert_eq!(sink.stops, 1);
}

#[test]
fn a_live_sound_too_fast_for_the_pace_is_refused() {
    let folder = folder("live-fast");
    let input = folder.join("sound.wav");
    std::fs::write(&input, wave(&tone(48_000), 48_000)).unwrap();
    let run = neottx(&[
        "sound",
        "--page",
        "100",
        "--live",
        "--codec",
        "pcm",
        input.to_str().unwrap(),
        folder.join("out.t42").to_str().unwrap(),
    ]);
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("packets a second"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

/// TODO-1228: `neottx sound --codec opus` carries an encoder's Ogg Opus
/// packets as they are (decision E.124).
mod opus {
    use super::*;
    use neoteletext_audio::ogg::write_ogg_opus;
    use neoteletext_opus::{Decoder, FRAME_SAMPLES};

    /// A constant-rate packet's length: 16 kbit/s at 20 ms.
    const PACKET: u16 = 40;
    /// The table of contents of a CELT-only full-band 20 ms mono frame
    /// (RFC 6716 section 3.1: configuration 31, code 0).
    const CELT_FULL_BAND_20_MS: u8 = 31 << 3;
    /// The same at 10 ms (configuration 30), outside the profile.
    const CELT_FULL_BAND_10_MS: u8 = 30 << 3;

    /// `count` packets made from a seed: the engine holds no recording, and
    /// a CELT frame decodes whatever its bytes.
    fn packets(count: usize) -> Vec<u8> {
        let mut state = 0x2545_f491_u32;
        (0..count)
            .flat_map(|_| {
                let mut packet = vec![CELT_FULL_BAND_20_MS];
                packet.extend((1..PACKET).map(|_| {
                    // xorshift32 (Marsaglia 2003).
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state.to_le_bytes()[0]
                }));
                packet
            })
            .collect()
    }

    /// What one decoder writes for the packets, in order.
    fn decoded(data: &[u8]) -> Vec<i16> {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        for packet in data.chunks(usize::from(PACKET)) {
            let mut pcm = [0_i16; FRAME_SAMPLES];
            assert!(decoder.decode(packet, &mut pcm).is_ok());
            out.extend_from_slice(&pcm);
        }
        out
    }

    fn write(name: &str, file: &[u8], extra: &[&str]) -> (std::process::Output, std::path::PathBuf) {
        let folder = folder(name);
        let input = folder.join("sound.opus");
        let output = folder.join("sound.t42");
        std::fs::write(&input, file).unwrap();
        let mut arguments = vec!["sound", "--page", "123", "--codec", "opus"];
        arguments.extend_from_slice(extra);
        arguments.extend([input.to_str().unwrap(), output.to_str().unwrap()]);
        let leaked: Vec<String> = arguments.iter().map(|argument| (*argument).to_owned()).collect();
        let run = neottx(&leaked.iter().map(String::as_str).collect::<Vec<_>>());
        (run, output)
    }

    fn played(output: &std::path::Path) -> Keeper {
        let stream = std::fs::read(output).unwrap();
        let mut arena = vec![0; 1 << 20];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = Keeper::default();
        receiver.open_page(Some(0x123), &mut sink);
        for bytes in stream.as_chunks::<PACKET_BYTES>().0 {
            assert!(receiver.push_packet(&Packet { bytes: *bytes }, &mut sink), "every packet is audio");
        }
        sink
    }

    // Verifies REQ-96 and TODO-1228: an Ogg Opus file of the profile is
    // written as records the receiver decodes to exactly what one decoder
    // gives, cut where the last granule position says.
    #[test]
    fn an_ogg_opus_file_plays_back_exactly() {
        let data = packets(30);
        let samples = 29 * 960 + 400;
        let (run, output) = write("opus-stored", &write_ogg_opus(&data, PACKET, samples, 3), &[]);
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        let report = String::from_utf8_lossy(&run.stderr);
        assert!(report.contains("28240 samples at 48000 Hz, 1200 bytes of opus"), "{report}");
        let sink = played(&output);
        assert_eq!(sink.starts.len(), 1);
        let mut want = decoded(&data);
        want.truncate(usize::try_from(samples).unwrap());
        assert_eq!(sink.pcm, want);
    }

    // Verifies TODO-1228: sent live, the packets are the sound's blocks, one
    // a block, and play as one decoder's output.
    #[test]
    fn an_ogg_opus_file_plays_back_live() {
        let data = packets(20);
        let (run, output) = write("opus-live", &write_ogg_opus(&data, PACKET, 20 * 960, 3), &["--live"]);
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        let sink = played(&output);
        assert_eq!(sink.pcm, decoded(&data));
    }

    // Verifies REQ-AUD-022 through TODO-1228: a packet outside the profile is
    // refused with the input status and its number, and so is a damaged
    // page, with its byte offset.
    #[test]
    fn a_packet_outside_the_profile_and_a_damaged_page_are_refused() {
        let mut data = packets(5);
        data[3 * usize::from(PACKET)] = CELT_FULL_BAND_10_MS;
        let (run, _) = write("opus-ten-ms", &write_ogg_opus(&data, PACKET, 5 * 960, 3), &[]);
        assert_eq!(run.status.code(), Some(3));
        assert!(String::from_utf8_lossy(&run.stderr).contains("packet 4 is not one 20 ms mono frame"));
        let mut file = write_ogg_opus(&packets(5), PACKET, 5 * 960, 3);
        let last = file.len() - 1;
        file[last] ^= 1;
        let (run, _) = write("opus-damaged", &file, &[]);
        assert_eq!(run.status.code(), Some(3));
        assert!(
            String::from_utf8_lossy(&run.stderr).contains("fails its CRC"),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}
