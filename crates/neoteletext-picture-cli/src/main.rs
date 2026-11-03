//! `neottx-picture`: a picture file to a teletext page file (DEC-2142).
//!
//! It reads the picture, bounded by the library's largest input, hands the
//! bytes to `neoteletext-picture`, and writes the page as TTI, and as T42
//! when asked. Every choice is an option of the library's [`Options`].
#![forbid(unsafe_code)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

use std::fs::File;
use std::io::Read;
use std::process::ExitCode;

use neoteletext_picture::{
    Align, Display, Fit, Focus, Level, MAX_INPUT_BYTES, Options, Region, Smooth, convert, parse_colour,
};

/// How the program is used.
const USAGE: &str = "usage: neottx-picture PICTURE PAGE.tti [--t42 PAGE.t42] [--level 1|1.5|2.5|3.5] [--objects]
    [--hold-mosaics] [--display neottx|tv] [--smooth area|lanczos] [--fit crop|contain|stretch]
    [--align center|top|bottom|left|right|top-left|top-right|bottom-left|bottom-right]
    [--background NAME|#RRGGBB] [--pixel-art] [--no-auto-orient] [--crop X,Y,WIDTH,HEIGHT]
    [--no-upscale] [--focus auto] [--keep-header]";

/// What the command line asked for.
struct Request {
    picture: String,
    tti: String,
    t42: Option<String>,
    options: Options,
}

/// The value after a flag, parsed, or why not.
fn value<T>(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<T, String> {
    let text = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
    parse(&text).ok_or_else(|| format!("{flag}: {text} is not one of its values"))
}

/// The request of the arguments after the program's name.
fn request(mut args: impl Iterator<Item = String>) -> Result<Request, String> {
    let mut positional = Vec::new();
    let mut t42 = None;
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--t42" => t42 = Some(value(&mut args, "--t42", |text| Some(text.to_string()))?),
            "--level" => options.level = value(&mut args, "--level", Level::parse)?,
            "--objects" => options.objects = true,
            "--hold-mosaics" => options.hold_mosaics = true,
            "--display" => {
                options.display = value(&mut args, "--display", |text| match text {
                    "neottx" => Some(Display::Square),
                    "tv" => Some(Display::Television),
                    _ => None,
                })?;
            }
            "--smooth" => {
                options.smooth = value(&mut args, "--smooth", |text| match text {
                    "area" => Some(Smooth::Area),
                    "lanczos" => Some(Smooth::Lanczos),
                    _ => None,
                })?;
            }
            "--fit" => {
                options.fit = value(&mut args, "--fit", |text| match text {
                    "crop" => Some(Fit::Crop),
                    "contain" => Some(Fit::Contain),
                    "stretch" => Some(Fit::Stretch),
                    _ => None,
                })?;
            }
            "--align" => options.align = Some(value(&mut args, "--align", Align::parse)?),
            "--background" => options.background = value(&mut args, "--background", parse_colour)?,
            "--pixel-art" => options.pixel_art = true,
            "--no-auto-orient" => options.auto_orient = false,
            "--crop" => options.crop = Some(value(&mut args, "--crop", Region::parse)?),
            "--no-upscale" => options.upscale = false,
            "--focus" => options.focus = value(&mut args, "--focus", |text| (text == "auto").then_some(Focus::Detail))?,
            "--keep-header" => options.keep_header = true,
            "--help" | "-h" => return Err(USAGE.to_string()),
            flag if flag.starts_with("--") => return Err(format!("{flag} is not an option\n{USAGE}")),
            _ => positional.push(arg),
        }
    }
    let mut positional = positional.into_iter();
    let (Some(picture), Some(tti), None) = (positional.next(), positional.next(), positional.next()) else {
        return Err(USAGE.to_string());
    };
    Ok(Request { picture, tti, t42, options })
}

/// The picture's bytes, refused past the library's largest input.
fn read(path: &str) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("{path}: {error}"))?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_INPUT_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    file.take(limit).read_to_end(&mut bytes).map_err(|error| format!("{path}: {error}"))?;
    Ok(bytes)
}

fn run() -> Result<(), String> {
    let request = request(std::env::args().skip(1))?;
    let bytes = read(&request.picture)?;
    let page = convert(&bytes, &request.options).map_err(|error| format!("{}: {error}", request.picture))?;
    std::fs::write(&request.tti, &page.tti).map_err(|error| format!("{}: {error}", request.tti))?;
    if let Some(path) = &request.t42 {
        let packets = page.t42().map_err(|error| error.to_string())?;
        std::fs::write(path, packets).map_err(|error| format!("{path}: {error}"))?;
    }
    eprintln!(
        "Level {}: {}, mean squared error {}",
        request.options.level.name(),
        page.summary,
        page.mean_squared_error
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
