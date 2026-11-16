//! Renders a short tune on the SID to a WAV file, for listening:
//!
//! `cargo run --release -p neoteletext-audio-instruments --example sid_demo -- 6581 out.wav`
//!
//! The first argument is the chip, `6581` or `8580`; the second the file,
//! 48 kHz, 16 bits, mono. The tune is written here, as the register images
//! a player on the machine would write fifty times a second: a pulse lead
//! with its width swept, a sawtooth bass through the low-pass with the
//! cutoff swept, a triangle arpeggio, and a noise drum on the bass's voice.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, composing a tune in f64 on indices it derives itself"
)]

#[cfg(feature = "sid")]
use neoteletext_audio_instruments::sid::{ChipClock, ChipModel, REGISTERS, Sid};

/// Register images a second: the PAL machine's frame rate, to the nearest
/// whole number.
const IMAGES_PER_SECOND: usize = 50;
/// Images in a step of the tune: a sixteenth note at 125 beats a minute.
const IMAGES_PER_STEP: usize = 6;
/// Steps in a bar.
const STEPS_PER_BAR: usize = 16;
/// Bars played; with the tail, a little over ten seconds.
const BARS: usize = 5;
/// Seconds of silence rendered after the last bar, for the release.
const TAIL_IMAGES: usize = 40;

/// Semitones from A4 of the bass note of each bar: A, F, C, G, A.
const BASS: [i32; BARS] = [-24, -28, -21, -26, -24];
/// The lead, in semitones from A4, one entry a step; -99 is a rest.
const LEAD: [i32; STEPS_PER_BAR] = [0, -99, 3, -99, 7, -99, 3, 0, 12, -99, 10, 7, 3, -99, 7, -99];
/// The chord the arpeggio runs through in each bar, in semitones from the
/// bar's bass note.
const CHORDS: [[i32; 3]; BARS] = [[12, 15, 19], [12, 16, 19], [12, 16, 19], [12, 16, 19], [12, 15, 19]];

/// The frequency register for a pitch in semitones from A4 (440 Hz): data
/// sheet, registers 0 and 1, "Fout = (Fn * Fclk / 16777216) Hz".
#[cfg(feature = "sid")]
fn frequency(semitones: i32, clock: ChipClock) -> u16 {
    let hz = 440.0 * (f64::from(semitones) / 12.0).exp2();
    (hz * 16_777_216.0 / f64::from(clock.hz())).round() as u16
}

/// Writes a 16-bit value to two registers, low byte first.
#[cfg(feature = "sid")]
fn set_word(image: &mut [u8; REGISTERS], address: usize, value: u16) {
    let [low, high] = value.to_le_bytes();
    if let Some(register) = image.get_mut(address) {
        *register = low;
    }
    if let Some(register) = image.get_mut(address.saturating_add(1)) {
        *register = high;
    }
}

/// The register image of one fiftieth of a second of the tune.
#[cfg(feature = "sid")]
fn image(index: usize, clock: ChipClock) -> [u8; REGISTERS] {
    let mut image = [0u8; REGISTERS];
    let step = index / IMAGES_PER_STEP;
    let within = index % IMAGES_PER_STEP;
    let bar = (step / STEPS_PER_BAR).min(BARS - 1);
    let in_bar = step % STEPS_PER_BAR;
    let playing = step < BARS * STEPS_PER_BAR;

    // Voice 1: the lead, a pulse whose width sweeps slowly.
    let note = LEAD[in_bar];
    let sounding = playing && note != -99;
    let held = (0..=in_bar).rev().map(|at| LEAD[at]).find(|&at| at != -99).unwrap_or(0);
    set_word(&mut image, 0, frequency(BASS[bar] + 24 + held, clock));
    let width = 0x800 + (1280.0 * (index as f64 / 37.0).sin()) as i32;
    set_word(&mut image, 2, width as u16);
    image[4] = 0x40 | u8::from(sounding && within < 4);
    image[5] = 0x18;
    image[6] = 0x86;

    // Voice 2: the bass, a sawtooth through the filter, with a noise drum
    // on every fourth step.
    let drum = playing && in_bar % 4 == 2 && within < 2;
    if drum {
        set_word(&mut image, 7, 0x3000 - 0x0800 * within as u16);
        image[11] = 0x81;
    } else {
        set_word(&mut image, 7, frequency(BASS[bar], clock));
        image[11] = 0x20 | u8::from(playing && in_bar % 4 != 3);
    }
    image[12] = 0x08;
    image[13] = 0xA8;

    // Voice 3: a triangle arpeggio, a chord note a frame.
    set_word(&mut image, 14, frequency(BASS[bar] + 12 + CHORDS[bar][index % 3], clock));
    image[18] = 0x10 | u8::from(playing && bar >= 1);
    image[19] = 0x4A;
    image[20] = 0x6A;

    // The filter: the bass through the low-pass, the cutoff sweeping up
    // and down once over the tune, with some resonance.
    let sweep = (index as f64 / (BARS * STEPS_PER_BAR * IMAGES_PER_STEP) as f64 * std::f64::consts::PI).sin();
    let cutoff = (200.0 + 1700.0 * sweep) as u16;
    image[21] = (cutoff & 7) as u8;
    image[22] = (cutoff >> 3) as u8;
    image[23] = 0xA2;
    image[24] = 0x1F;
    image
}

/// The samples as a WAV file's bytes: the 44-byte header of 16-bit mono
/// PCM, then the samples, little-endian.
#[cfg(feature = "sid")]
fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data = u32::try_from(samples.len().saturating_mul(2)).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(samples.len().saturating_mul(2).saturating_add(44));
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&data.saturating_add(36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&rate.saturating_mul(2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(feature = "sid")]
#[allow(clippy::print_stdout, clippy::print_stderr, reason = "a command-line example")]
fn main() -> std::process::ExitCode {
    use neoteletext_audio_instruments::SAMPLE_RATE;

    let arguments: Vec<String> = std::env::args().collect();
    let (Some(chip), Some(path)) = (arguments.get(1), arguments.get(2)) else {
        eprintln!("usage: sid_demo 6581|8580 OUTPUT.wav");
        return std::process::ExitCode::FAILURE;
    };
    let model = match chip.as_str() {
        "6581" => ChipModel::Mos6581,
        "8580" => ChipModel::Mos8580,
        other => {
            eprintln!("unknown chip {other}: 6581 or 8580");
            return std::process::ExitCode::FAILURE;
        }
    };
    let clock = ChipClock::Pal;
    let mut sid = Sid::new(model, clock);
    let per_image = (SAMPLE_RATE as usize).checked_div(IMAGES_PER_SECOND).unwrap_or(0);
    let images = (BARS * STEPS_PER_BAR * IMAGES_PER_STEP).saturating_add(TAIL_IMAGES);
    let mut samples = vec![0i16; images.saturating_mul(per_image)];
    for (index, block) in samples.chunks_mut(per_image).enumerate() {
        sid.write_image(&image(index, clock));
        sid.render(block);
    }

    let peak = samples.iter().map(|&sample| i32::from(sample).abs()).max().unwrap_or(0);
    let clipped = samples.iter().filter(|&&sample| sample == i16::MAX || sample == i16::MIN).count();
    let squares: f64 = samples.iter().map(|&sample| f64::from(sample) * f64::from(sample)).sum();
    let rms = (squares / samples.len().max(1) as f64).sqrt();
    println!(
        "{chip}: {} samples, {:.2} s; peak {peak} of 32767; rms {rms:.0}; {clipped} samples at the rails",
        samples.len(),
        samples.len() as f64 / f64::from(SAMPLE_RATE)
    );
    if let Err(error) = std::fs::write(path, wav(&samples, SAMPLE_RATE)) {
        eprintln!("{path}: {error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(not(feature = "sid"))]
fn main() {}
