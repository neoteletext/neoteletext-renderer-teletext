//! Raw sampled video to T42 through the software data slicer (REQ-INP-011,
//! area SLC).

use crate::input::{self, CHUNK, Options};
use crate::{Failure, exit};
use neoteletext_slicer::{Counters, Preset, RawGeometry, RawReader, SampleFormat};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// Parts of a raw capture's geometry chosen by the user, each replacing the
/// preset's (REQ-INP-011, REQ-175); `None` keeps the preset's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeometryChoice {
    /// Sample rate in hertz.
    pub sample_rate_hz: Option<u32>,
    /// Samples per line.
    pub samples_per_line: Option<usize>,
    /// True for 16-bit samples, least significant byte first; false for
    /// one byte.
    pub sixteen_bit: Option<bool>,
    /// Lines per field.
    pub lines_per_field: Option<usize>,
    /// The first and last line of each field used, zero-based, inclusive.
    pub used_lines: Option<(usize, usize)>,
    /// The television line number of the first line of each field.
    pub first_line_numbers: Option<[u16; 2]>,
}

impl GeometryChoice {
    /// `geometry` with the parts chosen replaced.
    #[must_use]
    pub fn applied_to(self, mut geometry: RawGeometry) -> RawGeometry {
        if let Some(rate) = self.sample_rate_hz {
            geometry.sample_rate_hz = rate;
        }
        if let Some(samples) = self.samples_per_line {
            geometry.samples_per_line = samples;
        }
        if let Some(sixteen_bit) = self.sixteen_bit {
            geometry.format = if sixteen_bit { SampleFormat::U16Le } else { SampleFormat::U8 };
        }
        if let Some(lines) = self.lines_per_field {
            geometry.lines_per_field = lines;
        }
        if let Some((first, last)) = self.used_lines {
            geometry.first_used = first;
            geometry.last_used = last;
        }
        if let Some(numbers) = self.first_line_numbers {
            geometry.first_line_numbers = Some(numbers);
        }
        geometry
    }
}

/// The slicer's settings for a decoder configuration: the exact framing
/// code alternative of REQ-PRF-004 is the decoder's `framing_tolerance`,
/// whichever profile it came from.
fn slicer_config(config: &neoteletext_core::config::Config) -> neoteletext_slicer::Config {
    neoteletext_slicer::Config { framing_tolerance: config.framing_tolerance, ..neoteletext_slicer::Config::default() }
}

/// Slices the input, a raw capture of the geometry `preset` names with the
/// parts `choice` gives replaced, and writes the packets found to `output`
/// as T42. A geometry the reader cannot use is refused with the usage
/// status. Returns the slicer's quality statistics.
///
/// # Errors
/// A [`Failure`]: the exit status and the message for the user (DEC-2451),
/// for a preset or a geometry the reader cannot use, an input or output
/// that cannot be opened, or a sandbox that cannot be applied.
pub fn slice(options: &Options, output: &Path, preset: &str, choice: &GeometryChoice) -> Result<Counters, Failure> {
    let preset = match preset {
        "bt8x8" => Preset::Bt8x8,
        "cx88" => Preset::Cx88,
        "saa7131" => Preset::Saa7131,
        "tbc" => Preset::Tbc,
        "tbc-vbi" => Preset::TbcVbi,
        other => return Err((exit::USAGE, format!("unknown preset {other:?} (bt8x8, cx88, saa7131, tbc, tbc-vbi)"))),
    };
    let config = slicer_config(&input::config(options)?);
    let mut raw = RawReader::new(choice.applied_to(RawGeometry::preset(preset)), config)
        .map_err(|error| (exit::USAGE, format!("{error:?}")))?;
    let mut input = input::open(options)?;
    let file = File::create(output).map_err(|error| (exit::INPUT, format!("{}: {error}", output.display())))?;
    let mut writer = neoteletext_formats::write::T42Writer::new(std::io::BufWriter::new(file));
    // REQ-SEC-017: the capture and the output are open; nothing else is.
    input::sandbox(options, None)?;
    let mut buffer = vec![0u8; CHUNK];
    let mut failure = None;
    loop {
        let read = input.read(&mut buffer).map_err(|error| (exit::INPUT, error.to_string()))?;
        if read == 0 {
            break;
        }
        // A reader that reports more than the buffer holds is broken.
        let chunk = buffer
            .get(..read)
            .ok_or((exit::INPUT, "the input reported more bytes than it was given room for".to_string()))?;
        raw.feed(chunk, |packet, _meta| {
            if let Err(error) = writer.write_packet(&packet) {
                failure = Some(error);
            }
        });
    }
    if let Some(error) = failure {
        return Err((exit::INPUT, error.to_string()));
    }
    writer.into_inner().flush().map_err(|error| (exit::INPUT, error.to_string()))?;
    // REQ-184, as the formats' readers report a fault under -v.
    if let Some(cut) = raw.finish()
        && options.verbose > 0
    {
        eprintln!("neottx: input byte {}: a partial line of {} bytes", cut.offset, cut.bytes);
    }
    Ok(raw.slicer().counters())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a test reports a failure by panicking, so it uses `expect` naming what it expected"
)]
mod tests {
    use super::slicer_config;
    use neoteletext_core::config::Config;
    use neoteletext_slicer::generator::line_bytes;
    use neoteletext_slicer::generator::{Generator, SignalParams, quantise_u8};
    use neoteletext_slicer::{Line, Preset, RawGeometry, Rejection, SampleFormat, Slicer};

    /// The geometry the lines are made in.
    fn geometry() -> RawGeometry {
        RawGeometry::preset(Preset::Bt8x8)
    }

    /// The place in the field of the line the test carries teletext on.
    const POSITION: u16 = 10;

    /// A sampled line of one packet, with framing code bit 3 wrong when asked.
    #[allow(clippy::cast_precision_loss, reason = "a sample rate well within f32's exact range")]
    fn line_samples(framing_wrong: bool) -> Vec<u8> {
        let mut packet = [0u8; neoteletext_core::consts::PACKET_BYTES];
        packet[..2].copy_from_slice(&[0x02, 0x15]);
        let mut bytes = line_bytes(&neoteletext_core::packet::Packet { bytes: packet });
        if framing_wrong {
            bytes[2] ^= 0x08;
        }
        let geometry = geometry();
        let mut values = vec![0.0_f32; geometry.samples_per_line];
        // The bt8x8 capture starts about 7 microseconds after the
        // synchronising edge, as the slicer's own tests of it say.
        let params = SignalParams { start_ns: 7_000.0, ..SignalParams::nominal(geometry.sample_rate_hz as f32) };
        Generator::new(1).line(&params, &bytes, &mut values);
        let mut samples = vec![0u8; geometry.samples_per_line];
        quantise_u8(&values, &mut samples);
        samples
    }

    // Verifies REQ-370: the exact framing code takes effect from each
    // profile: with the decoder's framing tolerance off, a framing code with
    // one wrong bit is refused even on a line carrying teletext, and with it
    // on (every profile's default) it is accepted there (DEC-1092).
    #[test]
    fn the_exact_framing_code_from_each_profile() {
        let exact = line_samples(false);
        let wrong = line_samples(true);
        let at = |samples| Line {
            second_field: Some(false),
            position_in_field: Some(POSITION),
            ..Line::new(samples, SampleFormat::U8, geometry().sample_rate_hz)
        };
        for name in ["en300706", "saa5050", "saa5246a"] {
            for tolerance in [true, false] {
                let config = Config { framing_tolerance: tolerance, ..Config::profile(name).expect("a profile") };
                let mut slicer = Slicer::new(slicer_config(&config));
                for _ in 0..neoteletext_slicer::consts::EXPECTED_LINE_EXACT_PACKETS {
                    assert!(slicer.slice(&at(&exact)).is_ok(), "{name}: the exact packet");
                }
                let outcome = slicer.slice(&at(&wrong));
                if tolerance {
                    assert!(outcome.is_ok(), "{name}: {outcome:?}");
                } else {
                    assert_eq!(outcome, Err(Rejection::NoFramingCode), "{name}");
                }
            }
        }
    }
}
