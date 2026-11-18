//! The command line's one specification (REQ-CLI-001, REQ-CLI-002): every
//! command and option, as clap parses them.

use clap::{Args, Parser, Subcommand, ValueEnum};
use neoteletext_core::colour::ColourMap;
use neoteletext_core::config::RomVariant;
use neoteletext_core::consts::{COLOUR_MAP_ENTRIES, COLOUR_TRANSPARENT};
use neoteletext_core::types::Rgb12;
use std::path::PathBuf;

/// neoteletext: a software teletext receiver (ETSI EN 300 706 Levels 1.0, 1.5, 2.5 and 3.5).
#[derive(Parser, Debug)]
#[command(name = "neottx", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Options every command accepts (REQ-CLI-002).
#[allow(clippy::struct_excessive_bools, reason = "each flag is a separate command-line switch")]
#[derive(Args, Debug, Clone)]
pub struct Common {
    /// Input: a file, `-` for standard input, `zxnet:<name>` for a live feed,
    /// or an `http` or `https` address: a folder ending in `/` is a channel,
    /// and any other address a service's descriptor. A `neottx://`
    /// link opens the address it wraps.
    pub input: String,
    /// Input format, overriding detection.
    #[arg(long, value_enum)]
    pub format: Option<FormatChoice>,
    /// Compatibility profile.
    #[arg(long, default_value = "en300706")]
    pub profile: String,
    /// Transport stream packet identifier of the teletext stream to use.
    #[arg(long)]
    pub pid: Option<u16>,
    /// Transport stream programme whose first teletext stream to use.
    #[arg(long)]
    pub program: Option<u16>,
    /// Seconds to collect pages from a live feed before acting (REQ-CLI-002):
    /// 20, or 600 with `--walk`.
    #[arg(long)]
    pub duration: Option<u64>,
    /// Walk a zxnet feed's whole service, page by page, at the feed's own
    /// pace (REQ-186, DEC-2456).
    #[arg(long)]
    pub walk: bool,
    /// Pages and sub-pages to keep in memory.
    #[arg(long, default_value_t = neoteletext_app::session::DEFAULT_PAGES)]
    pub pages: usize,
    /// Character set region, the Table 32 group 0 to 15 (decision E.4).
    /// By default it follows the language a transport stream declares.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..16))]
    pub region: Option<u8>,
    /// Parity failure policy (decision E.8).
    #[arg(long, value_enum)]
    pub parity: Option<ParityChoice>,
    /// Presentation level, overriding the profile's (REQ-CLI-007).
    #[arg(long, value_enum)]
    pub level: Option<LevelChoice>,
    /// Turn character rounding off (REQ-RND-012).
    #[arg(long)]
    pub no_rounding: bool,
    /// Accept a single Start Box (decision E.17).
    #[arg(long)]
    pub single_start_box: bool,
    /// The SAA5246A's national option ROM variant, E, H, T, J, K, L, I or
    /// S: C12 to C14 then choose among its sets alone (REQ-PRF-003). The
    /// saa5246a profile's is E.
    #[arg(long, value_name = "LETTER", value_parser = parse_rom_variant)]
    pub variant: Option<RomVariant>,
    /// Your own colours in place of Table 30's, before any page redefines
    /// them (REQ-RND-005): up to 32 colours separated by commas, each three
    /// hexadecimal digits of red, green and blue, replacing the Colour Map
    /// from entry 0; the first eight are the Level 1 colours, and entry 8
    /// stays transparent. For example `000,F00,0F0,FF0,00F,F0F,0FF,CCC`.
    #[arg(long, value_name = "COLOURS", value_parser = parse_colours)]
    pub colours: Option<&'static [Rgb12; COLOUR_MAP_ENTRIES]>,
    /// In a screen dump, leave out a row that repeats the double-height row
    /// above it, as the BBC Microcomputer's dumps hold it (REQ-INP-017).
    #[arg(long)]
    pub dump_repeated_rows: bool,
    /// Print diagnostics; with -vv the state machine trace (REQ-FSM-010),
    /// and with -vvv each cell's step of the row attribute machine as well.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Run without the operating-system sandbox (REQ-SEC-017).
    #[arg(long)]
    pub no_sandbox: bool,
    /// Allow a web service on this local-network address, for example a
    /// test server on 127.0.0.1 (REQ-SEC-006 is otherwise strict).
    #[arg(long, value_name = "IP")]
    pub allow_local: Vec<std::net::IpAddr>,
    /// Make every connection through this proxy: http://host:port or
    /// socks5://host:port (REQ-WEB-032).
    #[arg(long, value_name = "URL")]
    pub proxy: Option<String>,
}

/// Formats that can be named with `--format` (REQ-INP-005).
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatChoice {
    /// 42-byte packets.
    T42,
    /// 42-byte packets, bit-reversed.
    T42Reversed,
    /// 43-byte packets with the framing code.
    T42Framed,
    /// 45-byte packets with run-in and framing code.
    T42RunIn,
    /// MPEG transport stream, 188-byte packets.
    Ts,
    /// 192-byte transport stream packets.
    M2ts,
    /// 204-byte transport stream packets.
    Ts204,
    /// Raw PES packets.
    Pes,
    /// Linux sliced VBI records, little-endian.
    Sliced,
    /// Linux sliced VBI records, big-endian.
    SlicedBe,
    /// TTI page files.
    Tti,
    /// EP1 page files.
    Ep1,
    /// edit.tf or zxnet page addresses.
    Edittf,
    /// Raw screen dumps.
    Dump,
}

/// Parity policy names (decision E.8).
#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ParityChoice {
    /// Keep the last character that passed.
    KeepGood,
    /// Show failed characters as spaces.
    Space,
    /// Show a visible error marker.
    Marker,
}

/// Presentation levels (REQ-CLI-007, decision E.40).
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelChoice {
    /// Level 1.0.
    #[value(name = "1")]
    Level1,
    /// Level 1.5.
    #[value(name = "1.5")]
    Level15,
    /// Level 2.5.
    #[value(name = "2.5")]
    Level25,
    /// Level 3.5.
    #[value(name = "3.5")]
    Level35,
}

/// Terminal display choices (REQ-RND-029).
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsChoice {
    /// Detect.
    Auto,
    /// The kitty graphics protocol.
    Kitty,
    /// Sixel graphics.
    Sixel,
    /// iTerm2 inline images.
    Iterm2,
    /// Characters with ANSI colour.
    Text,
}

/// Raster options shared by `show`, `render` and `view`.
#[derive(Args, Debug, Clone)]
pub struct RasterArgs {
    /// Whole-number scale factor; 0 fits the terminal.
    #[arg(long, default_value_t = 0)]
    pub scale: usize,
    /// Stretch height by 1.2 for 4:3 proportions (REQ-RND-004).
    #[arg(long)]
    pub aspect: bool,
    /// Show concealed text.
    #[arg(long)]
    pub reveal: bool,
    /// Draw text in this loaded font, a 12 x 10 or 12 x 20 BDF file
    /// (REQ-FNT-002, REQ-FNT-008), instead of the compiled font: a file, or
    /// an `http` or `https` address, which needs `--font-sha256` and is
    /// fetched by the confined fetcher process (REQ-FNT-003).
    #[arg(long)]
    pub font: Option<PathBuf>,
    /// The SHA-256 the `--font` file must have, as a font library's index
    /// gives it (REQ-FNT-003, REQ-FNT-004): a file with any other is
    /// refused.
    #[arg(long, value_name = "HEX", requires = "font")]
    pub font_sha256: Option<String>,
}

/// The image sequence of `render`: a page played on the presentation
/// clock, sixty ticks a second, and written as numbered images.
#[derive(Args, Debug, Clone)]
pub struct SequenceArgs {
    /// Play the page for this many ticks of a sixtieth of a second, 1 to
    /// 36000, from tick 0, and write a sequence of images: each is named
    /// with -NNNNN, its tick in five digits, before the output's extension.
    #[arg(long, value_name = "TICKS", value_parser = clap::value_parser!(u32).range(1..=i64::from(crate::commands::MAX_SEQUENCE_TICKS)))]
    pub frames: Option<u32>,
    /// Write an image every this many ticks, and always the last.
    #[arg(long, value_name = "K", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..), requires = "frames")]
    pub frame_step: u32,
    /// A text file of timed operations, one a line, `<tick> <operation>`
    /// in ascending tick: `move`, `show`, `hide`, `look`, `order`, `frame`,
    /// `cycle`, `stop_cycle` and `reset` on a sprite's number (or
    /// `control:<n>`), and
    /// `record <ANIMATE|SPRITES|FRAME> <hexadecimal body>`. `-` reads
    /// standard input.
    #[arg(long, value_name = "FILE", requires = "frames")]
    pub script: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// List the pages and sub-pages found, with counts and error statistics.
    List {
        #[command(flatten)]
        common: Common,
    },
    /// Display one page in the terminal.
    Show {
        #[command(flatten)]
        common: Common,
        /// Page, e.g. 100 or 100/0003.
        page: String,
        #[command(flatten)]
        raster: RasterArgs,
        /// Graphics protocol.
        #[arg(long, value_enum, default_value = "auto")]
        graphics: GraphicsChoice,
        /// Write separated mosaics with contiguous characters, for older fonts.
        #[arg(long)]
        legacy_mosaics: bool,
        /// Use 8 colours instead of 24-bit colour.
        #[arg(long)]
        colours8: bool,
    },
    /// Browse pages interactively in the terminal.
    View {
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        raster: RasterArgs,
        /// Graphics protocol.
        #[arg(long, value_enum, default_value = "auto")]
        graphics: GraphicsChoice,
        /// Write separated mosaics with contiguous characters, for older fonts.
        #[arg(long)]
        legacy_mosaics: bool,
        /// Use 8 colours instead of 24-bit colour.
        #[arg(long)]
        colours8: bool,
        /// A key that goes Back, as a remote control's configurable key;
        /// one character the viewer does not already use.
        #[arg(long, value_name = "KEY")]
        back_key: Option<char>,
        /// A key that goes Forward, as `--back-key` goes Back.
        #[arg(long, value_name = "KEY")]
        forward_key: Option<char>,
    },
    /// Open a channel as a receiver does and report how it answers: the
    /// entry file, the handshake, the input protocol and how each sample
    /// selection ended. The exit status is non-zero on a fault.
    Probe {
        #[command(flatten)]
        common: Common,
        /// A place to select after the channel's own, such as 302 or
        /// 302/0002?a=1; give it more than once for several.
        #[arg(long = "select")]
        selections: Vec<String>,
    },
    /// Write one page, or all, as image files.
    Render {
        #[command(flatten)]
        common: Common,
        /// Page, e.g. 100, 100/0003, or `all`.
        page: String,
        /// Output file, or directory for `all`.
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        raster: RasterArgs,
        /// Image format.
        #[arg(long, value_enum, default_value = "png")]
        image: crate::commands::ImageFormat,
        #[command(flatten)]
        sequence: SequenceArgs,
    },
    /// Write one page, or all, as UTF-8 text.
    Text {
        #[command(flatten)]
        common: Common,
        /// Page, e.g. 100, 100/0003, or `all`.
        page: String,
        /// Show concealed text.
        #[arg(long)]
        reveal: bool,
        /// Write separated mosaics with contiguous characters, for older fonts.
        #[arg(long)]
        legacy_mosaics: bool,
    },
    /// Convert between formats.
    Convert {
        #[command(flatten)]
        common: Common,
        /// Output file; for `--to channel`, the channel's folder.
        #[arg(short, long)]
        output: PathBuf,
        /// Output format.
        #[arg(long, value_enum)]
        to: crate::commands::ConvertTarget,
        /// Only this magazine and page, e.g. 100.
        #[arg(long)]
        page: Option<String>,
    },
    /// Convert raw sampled video to T42 with quality statistics.
    Slice {
        #[command(flatten)]
        common: Common,
        /// Output T42 file.
        #[arg(short, long)]
        output: PathBuf,
        /// Capture geometry preset.
        #[arg(long, default_value = "bt8x8")]
        preset: String,
        /// The capture's geometry, each part replacing the preset's.
        #[command(flatten)]
        geometry: crate::commands::SliceGeometry,
    },
    /// List the teletext streams, languages and subtitle pages of a transport stream.
    Services {
        #[command(flatten)]
        common: Common,
    },
    /// Print the broadcast service data of packet 8/30.
    ServiceInfo {
        #[command(flatten)]
        common: Common,
    },
    /// Apply the sandbox, then try to open a file, start a program and open
    /// a network connection; all three must fail (REQ-SEC-017 verification).
    #[command(hide = true)]
    SandboxTest,
    /// The fetcher process of live mode (REQ-SEC-018); started by `view`.
    #[command(hide = true)]
    Fetch {
        /// The live source.
        address: String,
        /// Run without the sandbox.
        #[arg(long)]
        no_sandbox: bool,
        /// Local-network addresses allowed.
        #[arg(long)]
        allow_local: Vec<std::net::IpAddr>,
        /// The proxy.
        #[arg(long)]
        proxy: Option<String>,
        /// Walk a live feed's whole service (REQ-186).
        #[arg(long)]
        walk: bool,
        /// Fetch the font file at the address, send it and end (REQ-123);
        /// started by `render --font ADDRESS`.
        #[arg(long, conflicts_with = "walk")]
        font: bool,
    },
    /// Write a channel's contents.txt, to standard output, from a folder
    /// of page files (REQ-CLI-008).
    Contents {
        /// Common options; the input is the channel's folder.
        #[command(flatten)]
        common: Common,
    },
    /// Write a static channel's pages for a web browser into its folder:
    /// `index.html`, and `NNN/index.html` for each page. A page shows nothing
    /// of itself unless `--public` names it.
    Html {
        /// Common options; the input is the channel's folder.
        #[command(flatten)]
        common: Common,
        /// The channel's address on the web, ending in `/`: what the "Open
        /// in NeoTeletext" links name.
        #[arg(long)]
        address: String,
        /// A page to show the web: its title, its picture, and its text
        /// hidden and declared for search engines, all readable by anyone.
        /// Give it once for each page.
        #[arg(long)]
        public: Vec<String>,
    },
    /// Check TTI page files for authoring problems.
    Check {
        /// TTI files.
        files: Vec<PathBuf>,
        /// A channel's `channel.ttx`, or a service's descriptor, to resolve
        /// link targets against.
        #[arg(long)]
        descriptor: Option<PathBuf>,
        /// Run without the operating-system sandbox (REQ-SEC-017).
        #[arg(long)]
        no_sandbox: bool,
    },
    /// Write a TTI page from a UTF-8 text file in one of neoteletext's own
    /// character sets (REQ-CLI-010, REQ-CHR-012).
    WriteText {
        /// The set: english, german, swedish-finnish-hungarian, italian,
        /// french, portuguese-spanish, czech-slovak, polish, turkish,
        /// serbian-croatian-slovenian, romanian, estonian,
        /// lettish-lithuanian, cyrillic-serbian-croatian, russian-bulgarian,
        /// cyrillic-ukrainian, greek, hebrew, arabic-table44, hausa-fula,
        /// west-african, ewe, kabyle-tamazight, azerbaijani, kanuri-mossi,
        /// igbo, vietnamese, ukrainian, belarusian, macedonian,
        /// serbian, kyrgyz-mongolian, tatar, tajik, georgian, armenian,
        /// tifinagh, persian, urdu, pashto, sindhi, sorani, uyghur, balochi,
        /// arabic, japanese-romaji, hiragana or katakana.
        #[arg(long, required_unless_present = "language", conflicts_with = "language")]
        set: Option<String>,
        /// The language instead of a set, as a BCP 47 tag such as ru,
        /// sr-Latn or ja-Latn, from the engine's table of languages
        /// (REQ-2181).
        #[arg(long)]
        language: Option<String>,
        /// The presentation level the page is written for: neoteletext
        /// (NeoTeletext's own receivers, with row set triplets) or 3.5
        /// (Level 3.5 and below, the standard's means only) (REQ-2151).
        #[arg(long, value_parser = ["neoteletext", "3.5"], default_value = "neoteletext")]
        level: String,
        /// The page number, 100 to 8FF.
        #[arg(long)]
        page: String,
        /// The UTF-8 text.
        input: PathBuf,
        /// The TTI file to write.
        output: PathBuf,
        /// Run without the operating-system sandbox (REQ-SEC-017).
        #[arg(long)]
        no_sandbox: bool,
    },
    /// Write a sound as the audio profile's broadcast packets, with the
    /// command that plays it on a page (REQ-CLI-011, area AUD).
    Sound {
        /// The page the sound belongs to, 100 to 8FF.
        #[arg(long)]
        page: String,
        /// The channel, 0 to 3.
        #[arg(long, default_value_t = 0)]
        channel: u8,
        /// The gain, 0 to 100 per cent.
        #[arg(long, default_value_t = 80)]
        gain: u8,
        /// `ima` (IMA ADPCM, a quarter of the size) or `pcm`, from a WAVE
        /// file; or `opus`, an encoder's packets from an Ogg Opus file.
        #[arg(long, default_value = "ima")]
        codec: String,
        /// The sound's handle, 0 to 255.
        #[arg(long, default_value_t = 1)]
        handle: u8,
        /// Repeat the sound until a later command stops it.
        #[arg(long = "loop")]
        looped: bool,
        /// Times the whole sequence is written, as a carousel repeats it.
        /// With `--live`, times the sound is sent, one run after another.
        #[arg(long, default_value_t = 1)]
        repeat: u8,
        /// Send the sound live (REQ-AUD-014): block by block, at the pace a
        /// player reads the file, between repeats of the latest record.
        #[arg(long)]
        live: bool,
        /// With `--live`, the packets a second the file is played at.
        #[arg(long, default_value_t = crate::commands::PLAYER_PACKETS_PER_SECOND)]
        pace: u32,
        /// A WAVE file of 16-bit PCM at one of the profile's rates, or with
        /// `--codec opus` an Ogg Opus file of the profile: mono, 20 ms
        /// constant-rate packets of 1 to 160 bytes.
        input: PathBuf,
        /// The T42 file to write.
        output: PathBuf,
        /// Run without the operating-system sandbox (REQ-SEC-017).
        #[arg(long)]
        no_sandbox: bool,
    },
    /// Write text, such as a qrkeyboard join address, as a QR code in a
    /// TTI page's mosaics (REQ-CLI-012).
    Qr {
        /// The text.
        text: String,
        /// The page, 100 to 8FF.
        #[arg(long)]
        page: String,
        /// The first row of the code.
        #[arg(long, default_value_t = 2)]
        row: usize,
        /// The column the code starts at.
        #[arg(long, default_value_t = 0)]
        column: usize,
        /// The TTI file to write.
        output: PathBuf,
    },
    /// Make a phone the keyboard of a channel that takes input, through
    /// qrkeyboard's relay: the phone's keys go to the channel's push
    /// WebSocket as key records, as a browser would send them
    /// (REQ-CLI-013). It holds the network.
    Keyboard {
        /// The channel's push WebSocket, `ws://` or `wss://`.
        channel: String,
        /// The qrkeyboard relay.
        #[arg(long, default_value = "https://qrkeyboard.com")]
        relay: String,
        /// Pair with a PIN shown here, rather than a key in the QR.
        #[arg(long)]
        pin: bool,
        /// The name the phone shows.
        #[arg(long, default_value = "neottx")]
        name: String,
        /// Also write the QR as this page, for the channel to show.
        #[arg(long, requires = "qr_output")]
        qr_page: Option<String>,
        /// The TTI file of that page.
        #[arg(long, requires = "qr_page")]
        qr_output: Option<PathBuf>,
        /// Make every connection through this proxy: http://host:port or
        /// socks5://host:port (REQ-WEB-032).
        #[arg(long, value_name = "URL")]
        proxy: Option<String>,
    },
    /// Work with loaded fonts (area FNT).
    Font {
        #[command(subcommand)]
        action: FontCommand,
    },
}

/// The font commands (REQ-FNT-010).
#[derive(Subcommand, Debug)]
pub(crate) enum FontCommand {
    /// Convert a BDF font of another cell size to the 12 x 10 form, only
    /// where the conversion is exact: 5 x 10 and 6 x 10 with every pixel
    /// doubled in width, 5 x 9 and 6 x 9 the same below a blank line, and
    /// 12 x 20 with identical line pairs halved.
    Convert {
        /// The source font.
        input: PathBuf,
        /// The 12 x 10 font to write.
        output: PathBuf,
        /// For a font not in ISO10646: one `code<TAB>U+XXXX` line per
        /// character, the code as the font's ENCODING writes it.
        #[arg(long)]
        map: Option<PathBuf>,
        /// Run without the operating-system sandbox (REQ-SEC-017).
        #[arg(long)]
        no_sandbox: bool,
    },
    /// Report the character sets a loaded font conforms for (REQ-FNT-011):
    /// for each set, whether it conforms, the positions it
    /// draws, the SAA5050 cell profile, and the first rule that fails.
    Sets {
        /// The loaded font, 12 x 10 or 12 x 20.
        font: PathBuf,
        /// Print only the font library's `c` line (REQ-FNT-004) for the
        /// font under this file name.
        #[arg(long, value_name = "NAME")]
        library_line: Option<String>,
        /// Run without the operating-system sandbox (REQ-SEC-017).
        #[arg(long)]
        no_sandbox: bool,
    },
    /// List a font library's fonts by title, with their SHA-256 and the
    /// sets their `c` lines state, from its index (REQ-FNT-004); no font
    /// is read or fetched.
    Library {
        /// The library's folder, or its `fonts.txt`.
        library: PathBuf,
        /// Run without the operating-system sandbox (REQ-SEC-017).
        #[arg(long)]
        no_sandbox: bool,
    },
}

#[cfg(test)]
mod tests;

/// A ROM variant named by its datasheet letter (REQ-PRF-003).
/// The user's colours of `--colours` (REQ-RND-005): the entries of Table
/// 30's Colour Map with the first replaced by the list's. They are made once
/// for the run and live as long as it, as the configuration's reference to
/// them must (DEC-2482).
fn parse_colours(text: &str) -> Result<&'static [Rgb12; COLOUR_MAP_ENTRIES], String> {
    let mut map = ColourMap::DEFAULT;
    let mut given = 0usize;
    for (entry, colour) in map.entries.iter_mut().zip(text.split(',')) {
        *entry = parse_rgb12(colour.trim())
            .ok_or_else(|| format!("{colour:?} is not three hexadecimal digits of red, green and blue"))?;
        given = given.saturating_add(1);
    }
    if text.split(',').count() > given {
        return Err(format!("at most {} colours, one for each Colour Map entry", map.entries.len()));
    }
    // Table 30: CLUT 1 entry 0 is transparent whatever is given.
    if let Some(entry) = map.entries.get_mut(usize::from(COLOUR_TRANSPARENT)) {
        *entry = ColourMap::DEFAULT.entry(COLOUR_TRANSPARENT);
    }
    Ok(Box::leak(Box::new(map.entries)))
}

/// A 12-bit colour written as three hexadecimal digits, red first.
fn parse_rgb12(text: &str) -> Option<Rgb12> {
    let mut digits = text.chars().map(|digit| digit.to_digit(16).and_then(|value| u8::try_from(value).ok()));
    let colour = Rgb12::new(digits.next()??, digits.next()??, digits.next()??);
    digits.next().is_none().then_some(colour)
}

fn parse_rom_variant(text: &str) -> Result<RomVariant, String> {
    let mut letters = text.chars();
    match (letters.next().and_then(RomVariant::from_letter), letters.next()) {
        (Some(variant), None) => Ok(variant),
        _ => Err("expected one of E, H, T, J, K, L, I or S".to_owned()),
    }
}
