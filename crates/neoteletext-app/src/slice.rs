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

/// Slices the input, a raw capture of the geometry `preset` names with the
/// parts `choice` gives replaced, and writes the packets found to `output`
/// as T42. A geometry the reader cannot use is refused with the usage
/// status. Returns the slicer's quality statistics.
pub fn slice(options: &Options, output: &Path, preset: &str, choice: &GeometryChoice) -> Result<Counters, Failure> {
    let preset = match preset {
        "bt8x8" => Preset::Bt8x8,
        "cx88" => Preset::Cx88,
        "saa7131" => Preset::Saa7131,
        "tbc" => Preset::Tbc,
        "tbc-vbi" => Preset::TbcVbi,
        other => return Err((exit::USAGE, format!("unknown preset {other:?} (bt8x8, cx88, saa7131, tbc, tbc-vbi)"))),
    };
    let mut config = neoteletext_slicer::Config::default();
    config.framing_tolerance = input::config(options)?.framing_tolerance;
    let mut raw = RawReader::new(choice.applied_to(RawGeometry::preset(preset)), config)
        .map_err(|error| (exit::USAGE, format!("{error:?}")))?;
    let mut input = input::open(options)?;
    let file = File::create(output).map_err(|error| (exit::INPUT, format!("{}: {error}", output.display())))?;
    let mut writer = neoteletext_formats::write::T42Writer::new(std::io::BufWriter::new(file));
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
