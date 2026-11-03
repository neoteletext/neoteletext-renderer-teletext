//! The picture converter's evaluation on a folder of pictures (TODO-1176),
//! repeatable: every picture in `$NEOTELETEXT_PICTURES` is converted in each
//! mode, each page is drawn by the engine's receiver and checked sixel for
//! sixel against what the converter predicts, and its error is measured
//! two ways. The results go to `report.tsv` and a browsing page,
//! `index.html`, in `$NEOTELETEXT_PICTURES_OUT`.
//!
//! The pictures are not in the repository: they are the owner's, or
//! others' under their own licences. `tests/data/evaluation-pictures.tsv`
//! lists the set measured on 4 November 2026 and where each came from.
//!
//! ```sh
//! NEOTELETEXT_PICTURES=tmp/image/web/masters75 NEOTELETEXT_PICTURES_OUT=tmp/evaluation \
//!   cargo test --release -p neoteletext-picture --test evaluation -- --ignored --nocapture
//! ```
//!
//! `NEOTELETEXT_PICTURE_MODES` chooses the modes, separated by `;`, each the
//! level and any of `objects`, `hold`, `header`, `oklab`, `rgb`, `dither`
//! and `no-dither`; the default is the six modes of the first evaluation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::print_stdout,
    reason = "an evaluation run by hand reports by panicking and printing, on data it reads to a known size"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use image::{ImageFormat, RgbImage};
use neoteletext_picture::{Level, Options, convert};

mod common;
use common::{HEIGHT, WIDTH, page_draws, rendered, sixel_middles};

/// The modes of the evaluation of 19 October 2026.
const DEFAULT_MODES: &str = "1.5;1.5 hold;2.5;2.5 objects;3.5;3.5 objects";
/// The picture's file types the converter reads (DEC-2114).
const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];
/// Oklab's lightness runs 0 to 1; the perceptual error is given in
/// hundredths of it, as the measurement of 22 October 2026 gave it.
const PERCEPTUAL_SCALE: f64 = 100.0;
/// The Gaussian blur of the perceptual error: one sixel's standard
/// deviation, cut at three.
const BLUR_RADIUS: usize = 3;

/// The options a mode names.
fn options_of(mode: &str) -> Options {
    let mut words = mode.split_whitespace();
    let level = Level::parse(words.next().expect("a level")).expect("a level of 1, 1.5, 2.5 or 3.5");
    let mut options = Options { level, ..Options::default() };
    for word in words {
        match word {
            "objects" => options.objects = true,
            "hold" => options.hold_mosaics = true,
            "header" => options.keep_header = true,
            other => panic!("unknown word {other} in mode {mode}"),
        }
    }
    options
}

/// A colour of IEC 61966-2-1's RGB to Oklab (Björn Ottosson, 2020): the
/// published matrices, after making each primary linear.
fn oklab([red, green, blue]: [u8; 3]) -> [f64; 3] {
    let linear = |value: u8| {
        let v = f64::from(value) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let (red, green, blue) = (linear(red), linear(green), linear(blue));
    // The responses of the long, medium and short cones.
    let long = (0.412_221_470_8 * red + 0.536_332_536_3 * green + 0.051_445_992_9 * blue).cbrt();
    let medium = (0.211_903_498_2 * red + 0.680_699_545_1 * green + 0.107_396_956_6 * blue).cbrt();
    let short = (0.088_302_461_9 * red + 0.281_718_837_6 * green + 0.629_978_700_5 * blue).cbrt();
    [
        0.210_454_255_3 * long + 0.793_617_785_0 * medium - 0.004_072_046_8 * short,
        1.977_998_495_1 * long - 2.428_592_205_0 * medium + 0.450_593_709_9 * short,
        0.025_904_037_1 * long + 0.782_771_766_2 * medium - 0.808_675_766_0 * short,
    ]
}

/// The sixels in Oklab, blurred by a Gaussian of one sixel, edges held.
fn blurred(sixels: &[[u8; 3]]) -> Vec<[f64; 3]> {
    let kernel: Vec<f64> =
        (0..=2 * BLUR_RADIUS).map(|k| (-(k.abs_diff(BLUR_RADIUS).pow(2) as f64) / 2.0).exp()).collect();
    let total: f64 = kernel.iter().sum();
    let lab: Vec<[f64; 3]> = sixels.iter().map(|&colour| oklab(colour)).collect();
    let pass = |source: &[[f64; 3]], across: bool| -> Vec<[f64; 3]> {
        let mut out = vec![[0.0; 3]; source.len()];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let mut sum = [0.0; 3];
                for (k, weight) in kernel.iter().enumerate() {
                    // The neighbour k - BLUR_RADIUS away, held at the edges.
                    let near = |at: usize, size: usize| (at + k).saturating_sub(BLUR_RADIUS).min(size - 1);
                    let (sx, sy) = if across { (near(x, WIDTH), y) } else { (x, near(y, HEIGHT)) };
                    for (channel, value) in sum.iter_mut().enumerate() {
                        *value += weight * source[sy * WIDTH + sx][channel];
                    }
                }
                out[y * WIDTH + x] = sum.map(|value| value / total);
            }
        }
        out
    };
    pass(&pass(&lab, true), false)
}

/// The perceptual error: the mean Oklab distance, in hundredths, between
/// the picture and the page after both are blurred, over the sixels the
/// page draws.
fn perceptual_error(picture: &[[u8; 3]], drawn: &[[u8; 3]], keep_header: bool) -> f64 {
    let (a, b) = (blurred(picture), blurred(drawn));
    let distances: Vec<f64> = (0..WIDTH * HEIGHT)
        .filter(|&index| page_draws(index, keep_header))
        .map(|index| a[index].iter().zip(b[index]).map(|(p, q)| (p - q) * (p - q)).sum::<f64>().sqrt())
        .collect();
    PERCEPTUAL_SCALE * distances.iter().sum::<f64>() / distances.len() as f64
}

/// One conversion's results.
struct Result {
    mismatched: usize,
    mean_squared_error: u64,
    perceptual: f64,
}

/// One picture's index, the mode, and the results or why it was refused.
type Outcome = (usize, String, std::result::Result<Result, String>);

fn write_png(path: &Path, width: usize, colours: &[[u8; 3]]) {
    let height = colours.len() / width;
    let image =
        RgbImage::from_fn(width as u32, height as u32, |x, y| image::Rgb(colours[y as usize * width + x as usize]));
    image.save_with_format(path, ImageFormat::Png).unwrap();
}

fn evaluate(picture: &Path, mode: &str, out: &Path, stem: &str) -> std::result::Result<Result, String> {
    let options = options_of(mode);
    let bytes = std::fs::read(picture).map_err(|error| error.to_string())?;
    let page = convert(&bytes, &options).map_err(|error| error.to_string())?;
    let (width, colours) = rendered(&page);
    let seen = sixel_middles(width, &colours);
    let mismatched = (0..WIDTH * HEIGHT)
        .filter(|&index| page_draws(index, options.keep_header) && seen[index] != page.drawn[index])
        .count();
    let file = mode.replace(' ', "-");
    write_png(&out.join(format!("{stem}_{file}.png")), width, &colours);
    std::fs::write(out.join(format!("{stem}_{file}.tti")), &page.tti).unwrap();
    Ok(Result {
        mismatched,
        mean_squared_error: page.mean_squared_error,
        perceptual: perceptual_error(&page.picture, &page.drawn, options.keep_header),
    })
}

fn pictures(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .expect("$NEOTELETEXT_PICTURES is a folder")
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        })
        .collect();
    found.sort();
    found
}

/// TODO-1176: the evaluation, run by hand on a folder of pictures. It
/// fails if any page is drawn otherwise than predicted.
#[test]
#[ignore = "an evaluation run by hand: needs a folder of pictures in $NEOTELETEXT_PICTURES and an output folder in $NEOTELETEXT_PICTURES_OUT"]
fn evaluate_a_folder_of_pictures() {
    let folder = PathBuf::from(std::env::var("NEOTELETEXT_PICTURES").expect("$NEOTELETEXT_PICTURES"));
    let out = PathBuf::from(std::env::var("NEOTELETEXT_PICTURES_OUT").expect("$NEOTELETEXT_PICTURES_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let modes_text = std::env::var("NEOTELETEXT_PICTURE_MODES").unwrap_or_else(|_| DEFAULT_MODES.to_string());
    let modes: Vec<&str> = modes_text.split(';').map(str::trim).collect();
    let pictures = pictures(&folder);
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Outcome>> = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(picture) = pictures.get(index) else { break };
                    let stem = picture.file_stem().unwrap().to_string_lossy().to_string();
                    for mode in &modes {
                        let result = evaluate(picture, mode, &out, &stem);
                        results.lock().unwrap().push((index, (*mode).to_string(), result));
                    }
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results
        .sort_by(|a, b| (a.0, modes.iter().position(|m| *m == a.1)).cmp(&(b.0, modes.iter().position(|m| *m == b.1))));
    let mut report = String::from("picture\tmode\tmismatched sixels\tmean squared error\tperceptual error\n");
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>Picture evaluation</title><style>body{background:#222;color:#ddd;font:14px sans-serif}img{width:240px;image-rendering:pixelated}td{vertical-align:top;padding:4px}</style><table><tr><th>picture",
    );
    for mode in &modes {
        write!(html, "<th>{mode}").unwrap();
    }
    let mut failures = Vec::new();
    let mut totals = vec![(0.0, 0.0, 0usize); modes.len()];
    for (index, picture) in pictures.iter().enumerate() {
        let stem = picture.file_stem().unwrap().to_string_lossy().to_string();
        write!(html, "<tr><td>{stem}").unwrap();
        for (position, (_, mode, result)) in results.iter().filter(|r| r.0 == index).enumerate() {
            let file = mode.replace(' ', "-");
            match result {
                Ok(result) => {
                    writeln!(
                        report,
                        "{stem}\t{mode}\t{}\t{}\t{:.3}",
                        result.mismatched, result.mean_squared_error, result.perceptual
                    )
                    .unwrap();
                    write!(
                        html,
                        "<td><img src=\"{stem}_{file}.png\"><br>error {} / {:.2}",
                        result.mean_squared_error, result.perceptual
                    )
                    .unwrap();
                    if result.mismatched > 0 {
                        failures.push(format!("{stem} {mode}: {} sixels", result.mismatched));
                    }
                    totals[position].0 += result.mean_squared_error as f64;
                    totals[position].1 += result.perceptual;
                    totals[position].2 += 1;
                }
                Err(error) => {
                    writeln!(report, "{stem}\t{mode}\trefused: {error}").unwrap();
                    write!(html, "<td>refused: {error}").unwrap();
                }
            }
        }
    }
    std::fs::write(out.join("report.tsv"), report).unwrap();
    std::fs::write(out.join("index.html"), html).unwrap();
    for (mode, (squared, perceptual, count)) in modes.iter().zip(&totals) {
        let count = (*count).max(1) as f64;
        println!(
            "{mode:<20} pages {:>5}  mean squared error {:>9.1}  perceptual error {:>6.3}",
            count,
            squared / count,
            perceptual / count
        );
    }
    assert!(failures.is_empty(), "pages drawn otherwise than predicted: {failures:?}");
}
