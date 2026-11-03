//! The linked microcontroller test image (REQ-GEN-009, REQ-GEN-019).
//!
//! Every entry point a port calls is rooted in a `#[used]` function pointer.
//! The functions take their inputs as arguments, so nothing is evaluated at
//! compile time, and link-time optimisation keeps exactly the code a port
//! using the same features would link: no more, because nothing else is
//! reachable, and no less, because the pointers are kept. The image is
//! never run; `cargo xtask size` measures it and checks its symbols.
//!
//! The image's features choose the build being measured (decision E.61).
#![no_std]
#![no_main]
#![allow(linker_messages, reason = "the image is never run, so it has no entry point, and the linker says so")]

use core::hint::black_box;
use neoteletext_core::acq::{Decoder, StoredPage};
use neoteletext_core::compose::compose_level;
use neoteletext_core::config::Config;
use neoteletext_core::level1::{DisplayOptions, Grid, build_grid};
use neoteletext_core::packet::{Packet, PacketMeta, ServicePacket};
use neoteletext_core::service;
use neoteletext_core::types::{Instant, PageAddress, SubCode};

/// The image's own panic handler: the only one REQ-GEN-019 allows.
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Acquisition: a decoder over the caller's store, fed packets and time.
fn receive(pages: &mut [StoredPage], config: Config, packet: &Packet, meta: &PacketMeta, now: Instant) {
    let mut decoder = Decoder::new(pages, config);
    decoder.push(packet, meta, now, &mut |event| {
        black_box(event);
    });
    decoder.tick(now, &mut |event| {
        black_box(event);
    });
    black_box(decoder.statistics());
}

/// Composing a page at the configured level.
fn compose(decoder: &Decoder<'_>, address: PageAddress, config: &Config, grid: &mut Grid) {
    let page = decoder.store().get(address, SubCode::ANY);
    let magazine = decoder.magazine_data(address.magazine);
    let options = DisplayOptions::default();
    build_grid(grid, page, &magazine, config, &options);
    if let Some(page) = page {
        black_box(compose_level(grid, page, &magazine, &decoder.enhancement_lookup(), config, &options));
    }
}

/// Broadcast service data, packet 8/30.
fn service_data(packet: &ServicePacket) {
    let info = service::interpret(packet);
    black_box(service::initial_page(Some(&info)));
}

#[used]
static RECEIVE: fn(&mut [StoredPage], Config, &Packet, &PacketMeta, Instant) = receive;
#[used]
static COMPOSE: fn(&Decoder<'_>, PageAddress, &Config, &mut Grid) = compose;
#[used]
static SERVICE: fn(&ServicePacket) = service_data;

/// Levels 2.5 and 3.5: a decoder that also stores enhancement pages, and
/// records the processor's report.
#[cfg(feature = "level25")]
mod level25 {
    use super::{Config, Decoder, Instant, Packet, PacketMeta, StoredPage, black_box};
    use neoteletext_core::acq::{EnhancementPacket, EnhancementReport, EnhancementStore, EnhancementSubPage};

    fn receive(
        pages: &mut [StoredPage],
        sub_pages: &mut [EnhancementSubPage],
        packets: &mut [EnhancementPacket],
        config: Config,
        packet: &Packet,
        now: Instant,
        report: &EnhancementReport,
    ) {
        let mut decoder = Decoder::with_enhancement_store(pages, EnhancementStore::new(sub_pages, packets), config);
        decoder.push(packet, &PacketMeta::default(), now, &mut |event| {
            black_box(event);
        });
        black_box(decoder.record_enhancement(report.build, &report.faults));
    }

    #[used]
    static RECEIVE: fn(
        &mut [StoredPage],
        &mut [EnhancementSubPage],
        &mut [EnhancementPacket],
        Config,
        &Packet,
        Instant,
        &EnhancementReport,
    ) = receive;
}

/// The row renderer, with whatever glyphs the port supplies.
#[cfg(feature = "render")]
mod render {
    use super::{Grid, black_box};
    use neoteletext_core::glyphs::GlyphSet;
    use neoteletext_core::render::{Field, IndexBuffer, RenderOptions, render_row};

    fn render(grid: &Grid, row: usize, glyphs: &GlyphSet, options: RenderOptions, buffer: &mut IndexBuffer<'_>) {
        let _ = black_box(render_row(grid, row, Field::Both, glyphs, options, buffer));
    }

    #[used]
    static RENDER: fn(&Grid, usize, &GlyphSet, RenderOptions, &mut IndexBuffer<'_>) = render;
}

/// Noto Teletext, the one compiled font, as a port that draws links it. The
/// glyph set's address is passed on so that the tables stay in the image.
#[cfg(feature = "font")]
mod font {
    use super::black_box;
    use neoteletext_core::glyphs::GlyphSet;

    static GLYPHS_IN_IMAGE: GlyphSet<'static> = GlyphSet::noto_teletext();

    fn glyphs() {
        black_box(black_box(&GLYPHS_IN_IMAGE).lookup('A'));
    }

    #[used]
    static GLYPHS: fn() = glyphs;
}

/// The viewer: keys, decoder events, the status area and composing.
#[cfg(feature = "viewer")]
mod viewer {
    use super::{Grid, StoredPage, black_box};
    use neoteletext_core::viewer::{ComposeContext, Key, KeyContext, Viewer, ViewerEvent};

    fn view(
        viewer: &mut Viewer,
        key: Key,
        event: ViewerEvent,
        keys: &KeyContext<'_>,
        compose: &ComposeContext<'_>,
        page: Option<&StoredPage>,
        grid: &mut Grid,
    ) {
        black_box(viewer.key(key, keys));
        black_box(viewer.event(event));
        black_box(viewer.compose(grid, page, compose));
    }

    #[used]
    static VIEW: fn(
        &mut Viewer,
        Key,
        ViewerEvent,
        &KeyContext<'_>,
        &ComposeContext<'_>,
        Option<&StoredPage>,
        &mut Grid,
    ) = view;
}

/// Reading input: format detection and whichever readers are compiled in.
#[cfg(feature = "formats")]
mod formats {
    use super::black_box;
    use neoteletext_formats::detect::{AutoConfig, AutoReader};
    use neoteletext_formats::item::Reader;

    fn read(config: AutoConfig, bytes: &[u8]) {
        let mut reader = AutoReader::new(config);
        reader.feed(bytes, &mut |item| {
            black_box(item);
        });
        reader.finish(&mut |item| {
            black_box(item);
        });
    }

    #[used]
    static READ: fn(AutoConfig, &[u8]) = read;
}

/// The web user agent, with a port's random source.
#[cfg(feature = "web")]
mod web {
    use super::{Instant, black_box};
    use neoteletext_web::agent::{Input, UserAgent};
    use neoteletext_web::bytes::Writer;
    use neoteletext_web::map::MapAddress;
    use neoteletext_web::port::RandomSource;

    /// A stand-in for the port's random source.
    pub struct Random;

    impl RandomSource for Random {
        fn fill(&mut self, bytes: &mut [u8]) {
            for byte in bytes {
                *byte = black_box(0);
            }
        }
    }

    /// Slots and cookie bytes as a small port might choose.
    type Agent<'m> = UserAgent<'m, Random, 4, 4096>;

    fn handle(agent: &mut Agent<'_>, now: Instant, input: Input<'_>) {
        agent.handle(now, input, &mut |output| {
            black_box(output);
        });
    }

    #[used]
    static HANDLE: fn(&mut Agent<'_>, Instant, Input<'_>) = handle;

    /// An address read and written back in its canonical form (REQ-1693).
    fn normalise(address: &str, output: &mut [u8]) {
        if let Ok(address) = MapAddress::read(address) {
            black_box(address.write(&mut Writer::new(output)).is_ok());
        }
    }

    #[used]
    static NORMALISE: fn(&str, &mut [u8]) = normalise;
}

/// The Opus decoder (decision E.123, REQ-AUD-024): a packet decoded, or a loss concealed, into a
/// frame.
#[cfg(feature = "opus")]
mod opus {
    use super::black_box;
    use neoteletext_opus::{Decoder, FRAME_SAMPLES};

    /// A decoder made in the caller's place, as a port that keeps it in a static would.
    fn new(place: &mut Decoder) {
        *place = Decoder::new();
    }

    fn decode(decoder: &mut Decoder, packet: &[u8], pcm: &mut [i16; FRAME_SAMPLES]) {
        black_box(decoder.decode(packet, pcm).is_ok());
    }

    fn conceal(decoder: &mut Decoder, pcm: &mut [i16; FRAME_SAMPLES]) {
        decoder.conceal(pcm);
    }

    fn reset(decoder: &mut Decoder) {
        decoder.reset();
    }

    fn final_range(decoder: &Decoder) -> u32 {
        decoder.final_range()
    }

    // Every entry point, so that a panic path in any of them reaches the symbol check.
    #[used]
    static NEW: fn(&mut Decoder) = new;
    #[used]
    static DECODE: fn(&mut Decoder, &[u8], &mut [i16; FRAME_SAMPLES]) = decode;
    #[used]
    static CONCEAL: fn(&mut Decoder, &mut [i16; FRAME_SAMPLES]) = conceal;
    #[used]
    static RESET: fn(&mut Decoder) = reset;
    #[used]
    static FINAL_RANGE: fn(&Decoder) -> u32 = final_range;
}

/// The web module's readers a port calls itself, outside the user agent: a
/// channel's records, its contents file, its descriptor, a sub-page list,
/// and a cookie header (REQ-GEN-019: every public entry point).
#[cfg(feature = "web")]
mod web_readers {
    use super::black_box;
    use neoteletext_web::cookie::SetCookie;
    use neoteletext_web::descriptor::Descriptor;
    use neoteletext_web::records::{cookie, denied, index, links, log, redirect, subpages as subpage_records};
    use neoteletext_web::url::Url;
    use neoteletext_web::{contents, subpages};

    fn records(body: &[u8]) {
        black_box(index::read(body).is_ok());
        black_box(links::read(body).is_ok());
        black_box(log::read(body).is_ok());
        black_box(redirect::read(body).is_ok());
        black_box(subpage_records::read(body).is_ok());
        black_box(denied::read(body).is_ok());
        black_box(cookie::read(body).is_ok());
    }

    fn files(bytes: &[u8], base: &Url) {
        if let Ok(entries) = contents::read(bytes) {
            for entry in entries {
                let _ = black_box(entry);
            }
        }
        black_box(subpages::read(bytes).is_ok());
        black_box(Descriptor::parse(bytes, base).is_ok());
        black_box(Descriptor::parse_channel(bytes, base).is_ok());
    }

    fn cookie_header(header: &[u8], wall_clock: bool) {
        black_box(SetCookie::parse(header, wall_clock).is_ok());
    }

    #[used]
    static RECORDS: fn(&[u8]) = records;
    #[used]
    static FILES: fn(&[u8], &Url) = files;
    #[used]
    static COOKIE_HEADER: fn(&[u8], bool) = cookie_header;
}

/// The software data slicer (area SLC): raw lines into packets. REQ-GEN-016
/// allows it floating point, so the size check leaves this build out of the
/// float check by name.
#[cfg(feature = "slicer")]
mod slicer {
    use super::black_box;
    use neoteletext_slicer::{Config, Line, RawGeometry, RawReader, Slicer};

    fn raw(geometry: RawGeometry, config: Config, chunk: &[u8]) {
        if let Ok(mut reader) = RawReader::new(geometry, config) {
            reader.feed(chunk, |packet, meta| {
                black_box((packet, meta));
            });
            black_box(reader.finish());
        }
    }

    fn slice(slicer: &mut Slicer, line: &Line<'_>) {
        black_box(slicer.slice(line).is_ok());
    }

    #[used]
    static RAW: fn(RawGeometry, Config, &[u8]) = raw;
    #[used]
    static SLICE: fn(&mut Slicer, &Line<'_>) = slice;
}

/// The pixel and image helpers that need no allocation (REQ-RND-018,
/// REQ-RND-024, REQ-RND-030): conversion, scaling, palettes and the PNG and
/// PPM writers into a port's sink.
#[cfg(feature = "image")]
mod image {
    use super::black_box;
    use neoteletext_core::colour::ColourMap;
    use neoteletext_core::consts::COLOURS;
    use neoteletext_core::render::IndexFormat;
    use neoteletext_image::Sink;
    use neoteletext_image::convert::{ByteOrder, PixelFormat, Rotation, convert};
    use neoteletext_image::palette::{Rgba, colour_map_palette, palette};
    use neoteletext_image::scale::{scale_indices, scale_rgb_smoothed};
    use neoteletext_image::{png, ppm};

    /// A stand-in for the port's output.
    pub struct Out;

    impl Sink for Out {
        type Error = ();
        fn write_all(&mut self, bytes: &[u8]) -> Result<(), ()> {
            black_box(bytes);
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the geometry and format of one conversion")]
    fn pixels(
        indices: &[u8],
        width: usize,
        height: usize,
        colours: &[Rgba],
        format: PixelFormat,
        order: ByteOrder,
        rotation: Rotation,
        out: &mut [u8],
    ) {
        for index_format in [IndexFormat::Bits8, IndexFormat::Bits4] {
            black_box(
                convert(indices, index_format, width, height, width, colours, format, order, rotation, out, width)
                    .is_ok(),
            );
        }
        black_box(scale_indices(indices, width, height, 2, 2, true, out).is_ok());
        black_box(scale_rgb_smoothed(indices, width, height, 2, 2, true, out).is_ok());
    }

    fn colours(map: &ColourMap, clut: &[Rgba; COLOURS], picture: Rgba) {
        black_box(colour_map_palette(map, picture));
        black_box(palette(clut, picture, true, false));
    }

    fn files(indices: &[u8], width: usize, height: usize, colours: &[Rgba], scratch: &mut [u8]) {
        black_box(ppm::write(&mut Out, indices, width, height, colours, Rgba::default()).is_ok());
        let mut row = |y: usize, out: &mut [u8]| {
            black_box((y, &out));
        };
        black_box(png::write(&mut Out, width, height, colours, &mut row, scratch).is_ok());
    }

    #[used]
    static PIXELS: fn(&[u8], usize, usize, &[Rgba], PixelFormat, ByteOrder, Rotation, &mut [u8]) = pixels;
    #[used]
    static PALETTES: fn(&ColourMap, &[Rgba; COLOURS], Rgba) = colours;
    #[used]
    static FILES: fn(&[u8], usize, usize, &[Rgba], &mut [u8]) = files;
}

/// The audio receiver (area AUD, REQ-AUD-017): every entry point a port
/// calls, with a voice sink that keeps nothing, sized at audio Level A's
/// minimums for the microcontroller shape and Level B's for the desktop
/// shape (`LEVEL_A_MINIMUM`, `LEVEL_B_MINIMUM`). The arena is the port's
/// and is not counted here.
#[cfg(any(feature = "audio-microcontroller", feature = "audio-desktop"))]
mod audio {
    use super::black_box;
    use neoteletext_audio::port::{SoundFormat, SoundId, Start, VoiceSink};
    use neoteletext_core::packet::Packet;

    /// Sounds and records in flight the receiver holds: Level A's minimum
    /// in the microcontroller shape, Level B's in the desktop shape.
    #[cfg(feature = "audio-microcontroller")]
    const OBJECTS: usize = neoteletext_audio::LEVEL_A_MINIMUM.objects;
    #[cfg(feature = "audio-microcontroller")]
    const SLOTS: usize = neoteletext_audio::LEVEL_A_MINIMUM.records_in_flight;
    #[cfg(all(feature = "audio-desktop", not(feature = "audio-microcontroller")))]
    const OBJECTS: usize = neoteletext_audio::LEVEL_B_MINIMUM.objects;
    #[cfg(all(feature = "audio-desktop", not(feature = "audio-microcontroller")))]
    const SLOTS: usize = neoteletext_audio::LEVEL_B_MINIMUM.records_in_flight;

    type Receiver<'a> = neoteletext_audio::receiver::Receiver<'a, OBJECTS, SLOTS>;

    /// A sink that keeps nothing: the port's own sink is not measured.
    struct Quiet;

    impl VoiceSink for Quiet {
        fn define(&mut self, sound: SoundId, format: SoundFormat) {
            black_box((sound, format));
        }
        fn samples(&mut self, sound: SoundId, first: u32, pcm: &[i16]) {
            black_box((sound, first, pcm));
        }
        fn forget(&mut self, sound: SoundId) {
            black_box(sound);
        }
        fn stream(&mut self, sound: SoundId, rate_hz: u32) {
            black_box((sound, rate_hz));
        }
        fn stream_samples(&mut self, sound: SoundId, pcm: &[i16]) {
            black_box((sound, pcm));
        }
        fn start(&mut self, channel: u8, start: Start) {
            black_box((channel, start));
        }
        fn stop(&mut self, channel: u8, fade_ms: u16) {
            black_box((channel, fade_ms));
        }
        fn gain(&mut self, channel: u8, gain: u8, fade_ms: u16) {
            black_box((channel, gain, fade_ms));
        }
    }

    /// A receiver made over the caller's arena, as a port keeps one.
    fn new<'a>(place: &mut Option<Receiver<'a>>, arena: &'a mut [u8]) {
        *place = Some(Receiver::new(arena));
    }

    fn packet(receiver: &mut Receiver<'_>, packet: &Packet) {
        black_box(receiver.push_packet(packet, &mut Quiet));
    }

    fn message(receiver: &mut Receiver<'_>, message: &[u8]) {
        black_box(receiver.push_message(message, &mut Quiet).is_ok());
    }

    fn file(receiver: &mut Receiver<'_>, file: &[u8]) {
        black_box(receiver.push_file(file, &mut Quiet));
    }

    fn tick(receiver: &mut Receiver<'_>, now_ms: u64) {
        receiver.tick(now_ms, &mut Quiet);
        black_box(receiver.next_tick_ms());
    }

    fn page(receiver: &mut Receiver<'_>, page: Option<u16>, on: bool) {
        receiver.open_page(page, &mut Quiet);
        receiver.set_page_sounds(on, &mut Quiet);
    }

    #[used]
    static NEW: for<'a> fn(&mut Option<Receiver<'a>>, &'a mut [u8]) = new;
    #[used]
    static PACKET: fn(&mut Receiver<'_>, &Packet) = packet;
    #[used]
    static MESSAGE: fn(&mut Receiver<'_>, &[u8]) = message;
    #[used]
    static FILE: fn(&mut Receiver<'_>, &[u8]) = file;
    #[used]
    static TICK: fn(&mut Receiver<'_>, u64) = tick;
    #[used]
    static PAGE: fn(&mut Receiver<'_>, Option<u16>, bool) = page;
}

/// The input protocol's client (area INT): the session, the channel's
/// control records and the cursor record.
#[cfg(feature = "input")]
mod input {
    use super::black_box;
    use neoteletext_input::event::Item;
    use neoteletext_input::session::Session;
    use neoteletext_input::{control, cursor};

    fn session(caps: u32, now_ms: u32, record: &[u8], item: &Item<'_>, out: &mut [u8]) {
        let mut session = Session::new(caps);
        black_box(session.open(now_ms, None, out).is_ok());
        if let Ok(control) = control::read(record) {
            black_box(session.control(&control).is_ok());
        }
        black_box(session.send(item, now_ms, out).is_ok());
        session.tick(now_ms);
        session.close();
    }

    fn records(record: &[u8]) {
        black_box(control::read(record).is_ok());
        black_box(cursor::read(record).is_ok());
    }

    #[used]
    static SESSION: fn(u32, u32, &[u8], &Item<'_>, &mut [u8]) = session;
    #[used]
    static RECORDS: fn(&[u8]) = records;
}

/// REQ-GEN-019: one floating-point operation, which the symbol check must
/// find on targets without a floating-point unit.
#[cfg(feature = "float-probe")]
mod float_probe {
    use super::black_box;

    fn scale(value: f32) -> f32 {
        black_box(value) * black_box(1.5)
    }

    #[used]
    static SCALE: fn(f32) -> f32 = scale;
}
