//! `neottx slice`: raw sampled video through the software data slicer, or
//! its refusal in a build without it.

use neoteletext_app::Failure;
use neoteletext_app::input::Options;
use std::path::Path;

/// A raw capture's geometry as the command line gives it (REQ-INP-011,
/// REQ-175): each part given replaces the preset's, and the whole is
/// checked by the reader, which refuses one it cannot use with the usage
/// status.
#[derive(clap::Args, Clone, Debug, Default)]
pub struct SliceGeometry {
    /// Sample rate in hertz.
    #[arg(long, value_name = "HZ")]
    pub sample_rate: Option<u32>,
    /// Samples in each line of the file.
    #[arg(long, value_name = "N")]
    pub samples_per_line: Option<usize>,
    /// Bits in each sample: 8, or 16 as two bytes least significant first.
    #[arg(long, value_name = "BITS", value_parser = ["8", "16"])]
    pub sample_size: Option<String>,
    /// Lines in each field of the file.
    #[arg(long, value_name = "N")]
    pub lines_per_field: Option<usize>,
    /// The lines of each field that are sliced, counted from 0: FIRST-LAST.
    #[arg(long, value_name = "FIRST-LAST", value_parser = used_lines)]
    pub used_lines: Option<(usize, usize)>,
    /// The television line number of the first line of each field: A,B.
    #[arg(long, value_name = "A,B", value_parser = first_lines)]
    pub first_lines: Option<[u16; 2]>,
}

/// Reads `FIRST-LAST`.
fn used_lines(text: &str) -> Result<(usize, usize), String> {
    let (first, last) = text.split_once('-').ok_or("give FIRST-LAST")?;
    Ok((first.parse().map_err(|_| "FIRST is not a number")?, last.parse().map_err(|_| "LAST is not a number")?))
}

/// Reads `A,B`.
fn first_lines(text: &str) -> Result<[u16; 2], String> {
    let (first, second) = text.split_once(',').ok_or("give A,B")?;
    Ok([first.parse().map_err(|_| "A is not a line number")?, second.parse().map_err(|_| "B is not a line number")?])
}

/// `neottx slice`.
#[cfg(feature = "slice")]
pub fn slice(options: &Options, output: &Path, preset: &str, geometry: &SliceGeometry) -> Result<(), Failure> {
    use neoteletext_app::slice::GeometryChoice;
    let choice = GeometryChoice {
        sample_rate_hz: geometry.sample_rate,
        samples_per_line: geometry.samples_per_line,
        sixteen_bit: geometry.sample_size.as_deref().map(|bits| bits == "16"),
        lines_per_field: geometry.lines_per_field,
        used_lines: geometry.used_lines,
        first_line_numbers: geometry.first_lines,
    };
    let counters = neoteletext_app::slice::slice(options, output, preset, &choice)?;
    eprintln!("{counters:?}");
    Ok(())
}

/// `neottx slice` in a build without the slicer.
#[cfg(not(feature = "slice"))]
pub fn slice(_options: &Options, _output: &Path, _preset: &str, _geometry: &SliceGeometry) -> Result<(), Failure> {
    Err(neoteletext_app::not_in_build("raw video slicing", "slice"))
}
