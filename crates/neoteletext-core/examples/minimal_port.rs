//! A complete minimal port (REQ-ARCH-009): reads T42 packets from standard
//! input, decodes them, takes the page number to show as key presses, and
//! prints that page as a block of palette-index characters, using only the
//! platform interface, the viewer and the row renderer.
//!
//! Run: `cargo run -p neoteletext-core --example minimal_port 101 < capture.t42`
//! (page 100 when no page is given).
//!
//! The port's work is in [`run`], which takes its data source, its keys and
//! its output, so that the test `tests/minimal_port.rs` drives the same code
//! a user runs (REQ-31's verification).

use neoteletext_core::acq::{Decoder, StoredPage};
use neoteletext_core::config::Config;
use neoteletext_core::consts::{CELL_HEIGHT, DEFAULT_INITIAL_PAGE, FASTEXT_KEYS, PACKET_BYTES, PAGE_WIDTH, ROWS};
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::{DisplayOptions, Grid, PageRows, build_grid};
use neoteletext_core::level15;
use neoteletext_core::nav::PageLinks;
use neoteletext_core::packet::{Packet, PacketMeta};
use neoteletext_core::port::{Clock, DataSource, Keys};
use neoteletext_core::render::{Field, IndexBuffer, IndexFormat, RenderOptions, render_row};
use neoteletext_core::types::{Instant, PageAddress, SubCode};
use neoteletext_core::viewer::{Action, Key, KeyContext, Viewer, ViewerEvent};
use std::io::{Read, Write};

/// Page slots of the store: a port sizes this to its memory.
const STORE_PAGES: usize = 512;
/// The time one packet takes in this example: one television line of a
/// 625-line system, 64 microseconds. The example's clock only has to move
/// forward; a port's clock is its own.
const LINE_MICROSECONDS: u64 = 64;
/// How long after the last packet the decoder's timers are run, so that a
/// page still being received is completed: one second, beyond any page's
/// transmission time.
const SETTLE_MICROSECONDS: u64 = 1_000_000;
/// Of each cell's pixel lines, every this many is printed.
const LINE_STEP: usize = 4;
/// Of each line's pixels, every this many is printed: one per glyph column
/// pair, so a row of 480 pixels fits 80 characters.
const PIXEL_STEP: usize = 6;
/// The lines of a cell printed from: the first field's ten.
const FIELD_LINES: usize = CELL_HEIGHT / 2;

/// The data source: standard input.
struct Stdin(std::io::Stdin);

impl DataSource for Stdin {
    type Error = std::io::Error;
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buffer)
    }
}

/// The keys: the digits of the program's first argument, pressed in order.
pub struct DigitKeys(std::vec::IntoIter<u8>);

impl DigitKeys {
    /// Keys that press each decimal digit of `text`, ignoring anything else.
    pub fn new(text: &str) -> Self {
        let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_digit).map(|byte| byte - b'0').collect();
        Self(digits.into_iter())
    }
}

impl Keys for DigitKeys {
    type Key = Key;
    fn poll(&mut self) -> Option<Key> {
        self.0.next().map(Key::Digit)
    }
}

/// The time source: one television line per packet.
struct LineClock(u64);

impl Clock for LineClock {
    fn now(&self) -> Instant {
        Instant(self.0)
    }
}

/// The port: decodes every packet of `source`, lets `keys` choose the page
/// through the viewer, and prints that page's rows to `out`.
///
/// # Errors
/// A source or output that fails, or a renderer that refuses its buffer.
pub fn run<S, K>(source: &mut S, keys: &mut K, out: &mut dyn Write) -> Result<(), std::io::Error>
where
    S: DataSource<Error = std::io::Error>,
    K: Keys<Key = Key>,
{
    let mut clock = LineClock(0);
    let mut slots = vec![StoredPage::EMPTY; STORE_PAGES];
    let config = Config::default();
    let mut decoder = Decoder::new(&mut slots, config);

    // Feed whole 42-byte packets from the source.
    let mut packet = [0u8; PACKET_BYTES];
    let mut filled = 0;
    loop {
        let read = source.read(&mut packet[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
        if filled == PACKET_BYTES {
            clock.0 += LINE_MICROSECONDS;
            decoder.push(&Packet { bytes: packet }, &PacketMeta::default(), clock.now(), &mut |_| {});
            filled = 0;
        }
    }
    clock.0 += SETTLE_MICROSECONDS;
    decoder.tick(clock.now(), &mut |_| {});

    // The viewer starts on the initial page, and each key may select another.
    let Some(initial) = PageAddress::from_mpp(DEFAULT_INITIAL_PAGE) else { return Ok(()) };
    let mut viewer = Viewer::new(initial);
    viewer.event(ViewerEvent::Started);
    let links = PageLinks::default();
    let context = KeyContext {
        links: &links,
        store: decoder.store(),
        initial,
        config: &config,
        now: clock.now(),
        confirmed: &|_| false,
        prompts_shown: [false; FASTEXT_KEYS],
    };
    while let Some(key) = keys.poll() {
        if let Action::Request(..) = viewer.key(key, &context) {
            viewer.event(ViewerEvent::PagePresent);
        }
    }
    let (address, subcode) = viewer.requested();
    let number = String::from_utf8_lossy(&address.ascii()).into_owned();
    let Some(page) = decoder.store().get(address, subcode).or_else(|| decoder.store().get(address, SubCode::ANY))
    else {
        writeln!(out, "page {number} not found")?;
        return Ok(());
    };
    writeln!(out, "page {number}")?;

    // Build the grid of the page and pull its rows from the renderer.
    let mut grid = Grid::EMPTY;
    let options = DisplayOptions::default();
    build_grid(&mut grid, Some(page), &decoder.magazine_data(address.magazine), &config, &options);
    let rows = PageRows::of(page, &config, &options);
    level15::apply(&mut grid, &page.enhancements, page.enhancements_present, &config, rows);

    // The port's buffer: one text row of 480 by 20 palette indices.
    let mut row = vec![0u8; PAGE_WIDTH * CELL_HEIGHT];
    let options = RenderOptions::from_config(&config, false);
    for text_row in 0..ROWS {
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut row, stride: PAGE_WIDTH };
        render_row(&grid, text_row, Field::First, &GlyphSet::noto_teletext(), options, &mut buffer)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        for line in (0..FIELD_LINES).step_by(LINE_STEP) {
            let pixels = &row[line * PAGE_WIDTH..(line + 1) * PAGE_WIDTH];
            let text: String =
                pixels.iter().step_by(PIXEL_STEP).map(|&index| if index == 0 { ' ' } else { '#' }).collect();
            writeln!(out, "{}", text.trim_end())?;
        }
    }
    Ok(())
}

#[allow(dead_code, reason = "the test includes this file as a module and calls run, not main")]
fn main() -> Result<(), std::io::Error> {
    let page = std::env::args().nth(1).unwrap_or_default();
    run(&mut Stdin(std::io::stdin()), &mut DigitKeys::new(&page), &mut std::io::stdout().lock())
}
