//! Reading inputs into a session: files and standard input through the
//! format readers (area INP), live feeds through the web module.

use crate::session::Session;
use crate::{Failure, exit};
use neoteletext_core::colour::ColourMap;
use neoteletext_core::config::{Config, ParityPolicy, PresentationLevel, RomVariant};
use neoteletext_core::types::PageAddress;
use neoteletext_formats::Item;
use neoteletext_formats::detect::{AutoConfig, AutoReader, Format};
use neoteletext_formats::item::Reader;
use neoteletext_formats::transport::TsConfig;
use std::fs::File;
use std::io::Read;
use std::net::IpAddr;

mod feed;
#[cfg(feature = "live")]
mod select;

pub use feed::{Opening, OpeningError, is_live, link_address, open_live, opening};

/// What to read and how to decode it: the options every `neottx` command
/// accepts (REQ-CLI-002), free of any argument parser.
// Each flag is a separate user choice.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct Options {
    /// A file, `-` for standard input, or a live feed or web service.
    pub input: String,
    /// The input format, overriding detection.
    pub format: Option<Format>,
    /// The compatibility profile's name.
    pub profile: String,
    /// The transport stream packet identifier of the teletext stream.
    pub pid: Option<u16>,
    /// The transport stream programme whose first teletext stream to use.
    pub program: Option<u16>,
    /// Seconds to collect pages from a live feed before acting.
    pub duration: u64,
    /// Walk a live feed's whole service (REQ-186).
    pub walk: bool,
    /// Pages and sub-pages to keep in memory.
    pub pages: usize,
    /// The Table 32 group 0 to 15 (decision E.4), overriding the
    /// language a transport stream declares.
    pub region: Option<u8>,
    /// The parity failure policy (decision E.8), overriding the profile's.
    pub parity: Option<ParityPolicy>,
    /// The presentation level, overriding the profile's (REQ-CLI-007).
    pub level: Option<PresentationLevel>,
    /// Turn character rounding off (REQ-RND-012).
    pub no_rounding: bool,
    /// Accept a single Start Box (decision E.17).
    pub single_start_box: bool,
    /// The SAA5246A national option ROM variant, overriding the profile's
    /// (REQ-PRF-003).
    pub rom_variant: Option<RomVariant>,
    /// The user's colours, replacing Table 30's before any page data
    /// redefines them (REQ-RND-005, DEC-2482).
    pub colours: Option<&'static ColourMap>,
    /// Leave out a screen dump's row that repeats the double-height row
    /// above it (REQ-181).
    pub dump_repeated_rows: bool,
    /// Diagnostics: 1 prints them, 2 adds the state machine trace
    /// (REQ-FSM-010).
    pub verbose: u8,
    /// Run without the operating-system sandbox (REQ-SEC-017).
    pub no_sandbox: bool,
    /// Local-network addresses a web service may use (REQ-SEC-006).
    pub allow_local: Vec<IpAddr>,
    /// The proxy every connection goes through, `http://host:port` or
    /// `socks5://host:port` (REQ-WEB-032).
    pub proxy: Option<String>,
}

/// Bytes read from the input at a time.
pub const CHUNK: usize = 64 * 1024;

/// The configuration from the profile and the overrides (REQ-PRF-005).
///
/// # Errors
/// A [`Failure`]: the exit status and the message for the user (DEC-2451),
/// for a profile name that is not one the engine knows.
pub fn config(options: &Options) -> Result<Config, Failure> {
    let mut config = Config::profile(&options.profile)
        .ok_or_else(|| (exit::USAGE, format!("unknown profile {:?} (en300706, saa5050, saa5246a)", options.profile)))?;
    if let Some(parity) = options.parity {
        config.parity_policy = parity;
    }
    if let Some(region) = options.region {
        config.default_region = region;
    }
    if let Some(level) = options.level {
        config.level = level;
    }
    if options.no_rounding {
        config.rounding = false;
    }
    if options.single_start_box {
        config.double_start_box = false;
    }
    if let Some(variant) = options.rom_variant {
        config.rom_variant = Some(variant);
    }
    if let Some(colours) = options.colours {
        config.user_colours = Some(colours);
    }
    config.trace = options.verbose >= 2;
    // A thousand entries a page: the row attribute machine's cells only at
    // the highest verbosity.
    config.trace_cells = options.verbose >= 3;
    Ok(config)
}

/// Opens the input: a file, or standard input for `-`.
///
/// # Errors
/// A [`Failure`]: the exit status and the message for the user (DEC-2451),
/// when the file cannot be opened, or a live input is given where only a
/// file can be read.
pub fn open(options: &Options) -> Result<Box<dyn Read>, Failure> {
    if is_live(&options.input) {
        return Err((exit::USAGE, "this command needs a file or standard input, not a live feed".into()));
    }
    if options.input == "-" {
        return Ok(Box::new(std::io::stdin().lock()));
    }
    File::open(&options.input)
        .map(|file| Box::new(file) as Box<dyn Read>)
        .map_err(|error| (exit::INPUT, format!("{}: {error}", options.input)))
}

/// Feeds every chunk of `input` to `reader`.
///
/// # Errors
/// A [`Failure`]: the exit status and the message for the user (DEC-2451),
/// when reading the input fails.
pub fn pump(input: &mut dyn Read, reader: &mut dyn Reader, out: &mut dyn FnMut(Item<'_>)) -> Result<(), Failure> {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = input.read(&mut buffer).map_err(|error| (exit::INPUT, error.to_string()))?;
        if read == 0 {
            break;
        }
        // A reader that reports more than the buffer holds is broken.
        let chunk = buffer
            .get(..read)
            .ok_or((exit::INPUT, "the input reported more bytes than it was given room for".to_string()))?;
        reader.feed(chunk, out);
    }
    reader.finish(out);
    Ok(())
}

/// The detecting reader for the input, with the format and stream the
/// options name. `encode_pages` turns page files into packets.
#[must_use]
pub fn reader(options: &Options, encode_pages: bool) -> Box<AutoReader> {
    Box::new(AutoReader::new(AutoConfig {
        format: options.format,
        encode_pages,
        transport: TsConfig { pid: options.pid, program: options.program, ..TsConfig::default() },
        source: None,
        dump_repeated_rows: options.dump_repeated_rows,
    }))
}

/// Reads the whole input into a new session.
///
/// # Errors
/// As [`load_for`].
pub fn load(options: &Options) -> Result<Session, Failure> {
    load_for(options, None)
}

/// Reads the whole input into a new session; for live feeds, asks for and
/// waits for `wanted`.
///
/// # Errors
/// A [`Failure`]: the exit status and the message for the user (DEC-2451),
/// when the input cannot be opened or read, the sandbox cannot be applied,
/// the input holds no teletext, or nothing arrives from a live feed.
pub fn load_for(options: &Options, wanted: Option<PageAddress>) -> Result<Session, Failure> {
    if is_live(&options.input) {
        return feed::load_live(options, wanted);
    }
    let mut input = open(options)?;
    load_from(options, input.as_mut())
}

/// Reads the whole input into a new session under the sandbox (REQ-SEC-017):
/// a file or standard input is opened, then `sandbox` is applied, and only
/// then is a byte of it parsed. A live feed starts its fetcher process
/// first, which must reach the network, and the decoder is sandboxed when
/// the feed has given `wanted`.
///
/// # Errors
/// An input that cannot be opened or read, or a sandbox that cannot be applied.
pub fn load_sandboxed(
    options: &Options,
    wanted: Option<PageAddress>,
    sandbox: impl FnOnce() -> Result<(), Failure>,
) -> Result<Session, Failure> {
    if is_live(&options.input) {
        let session = feed::load_live(options, wanted)?;
        sandbox()?;
        return Ok(session);
    }
    let mut input = open(options)?;
    sandbox()?;
    load_from(options, input.as_mut())
}

/// Applies the sandbox with no network, and `writable` the one directory the
/// command may still write in (REQ-SEC-017); `options.no_sandbox` is
/// `--no-sandbox`.
///
/// # Errors
/// A sandbox that cannot be applied, with the reason.
pub fn sandbox(options: &Options, writable: Option<&std::path::Path>) -> Result<(), Failure> {
    crate::sandbox::apply(
        crate::sandbox::Policy { writable_directory: writable, network: false, readable: &[] },
        options.no_sandbox,
    )
    .map_err(|error| (exit::INPUT, error.to_string()))
}

/// Reads all of an opened file or standard input into a new session.
///
/// # Errors
/// An input that cannot be read, or that holds no teletext.
pub fn load_from(options: &Options, input: &mut dyn Read) -> Result<Session, Failure> {
    let config = config(options)?;
    let mut session = Session::new(config, options.pages);
    let mut reader = reader(options, true);
    let mut packets = 0u64;
    let mut detected = None;
    let mut languages: Vec<(u16, [u8; 3])> = Vec::new();
    let mut selected_pid = None;
    let verbose = options.verbose;
    pump(input, reader.as_mut(), &mut |item| match item {
        Item::Packet { packet, meta } => {
            packets = packets.saturating_add(1);
            session.push(&packet, &meta);
        }
        Item::Detected(format) => {
            detected = Some(format);
            if verbose > 0 {
                eprintln!("neottx: detected format {format:?}");
            }
        }
        Item::Diagnostic(diagnostic) if verbose > 0 => {
            eprintln!("neottx: input byte {}: {:?}", diagnostic.offset, diagnostic.kind);
        }
        Item::StreamSelected { program, pid } => {
            selected_pid = Some(pid);
            if verbose > 0 {
                eprintln!("neottx: using teletext stream PID {pid:#06x}, program {program:?}");
            }
        }
        Item::TeletextEntry { pid, entry, .. } => languages.push((pid, entry.language)),
        // REQ-INP-026: a channel's log lines are for its author, shown
        // with -v and never on a page.
        Item::Log { line, page, line_number } if verbose > 0 => {
            let page = page.map_or_else(|| "before any page".to_owned(), |page| format!("page {:03X}", page.mpp()));
            eprintln!("neottx: log {:?} at line {line_number}, {page}: {}", line.level, line.text);
        }
        _ => {}
    })?;
    session.finish();
    if options.region.is_none()
        && let Some(&(_, language)) = languages.iter().find(|(pid, _)| Some(*pid) == selected_pid)
    {
        let mut config = *session.decoder().config();
        // REQ-2181 item 5: the core's one table of languages.
        config.default_region = neoteletext_core::charset::language::region_for_iso_639_2(language);
        if verbose > 0 {
            eprintln!("neottx: region {} from language {}", config.default_region, String::from_utf8_lossy(&language));
        }
        session.decoder_mut().set_config(config);
    }
    // Events are kept only with -vv (`config.trace`).
    session.print_trace();
    if packets == 0 {
        let reason = if detected.is_none() { "the input format was not recognised" } else { "no teletext found" };
        return Err((exit::INPUT, format!("{}: {reason}", options.input)));
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader of bytes that says it read more than the buffer holds.
    struct Overreporting;

    impl Read for Overreporting {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            Ok(buffer.len().saturating_add(1))
        }
    }

    /// A format reader that is never reached.
    struct Unfed;

    impl Reader for Unfed {
        fn feed(&mut self, _bytes: &[u8], _out: &mut dyn FnMut(Item<'_>)) {}
        fn finish(&mut self, _out: &mut dyn FnMut(Item<'_>)) {}
    }

    fn options(profile: &str) -> Options {
        Options {
            input: String::new(),
            format: None,
            profile: profile.to_owned(),
            pid: None,
            program: None,
            duration: 0,
            walk: false,
            pages: 0,
            region: None,
            parity: None,
            level: None,
            no_rounding: false,
            single_start_box: false,
            rom_variant: None,
            colours: None,
            dump_repeated_rows: false,
            verbose: 0,
            no_sandbox: false,
            allow_local: Vec::new(),
            proxy: None,
        }
    }

    /// REQ-371: each option changes its setting after the profile is
    /// applied, and leaves the profile's other settings as they were.
    #[test]
    #[allow(clippy::unwrap_used, reason = "a test")]
    fn each_option_overrides_its_profile() {
        let profile = Config::profile("saa5050").unwrap();
        assert_eq!(config(&options("saa5050")).unwrap(), profile);
        let given = Options {
            parity: Some(ParityPolicy::ErrorMarker),
            region: Some(3),
            level: Some(PresentationLevel::Level35),
            no_rounding: true,
            single_start_box: true,
            rom_variant: Some(RomVariant::GreekTurkish),
            colours: Some(&ColourMap::DEFAULT),
            ..options("saa5050")
        };
        let changed = Config {
            parity_policy: ParityPolicy::ErrorMarker,
            default_region: 3,
            level: PresentationLevel::Level35,
            rounding: false,
            double_start_box: false,
            rom_variant: Some(RomVariant::GreekTurkish),
            user_colours: Some(&ColourMap::DEFAULT),
            ..profile
        };
        assert_eq!(config(&given).unwrap(), changed);
    }

    /// REQ-430 (REQ-SEC-017): a file is opened, then the sandbox applied,
    /// and only then parsed. The file holds no teletext when it is opened,
    /// and the sandbox step replaces its content with a page: a load that
    /// parsed before the sandbox would find no teletext.
    #[test]
    #[allow(clippy::unwrap_used, reason = "a test")]
    fn a_file_is_parsed_only_after_the_sandbox() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/input-sandbox-order");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("page.tti");
        std::fs::write(&path, "not teletext\n").unwrap();
        let given = Options { input: path.to_string_lossy().into_owned(), pages: 16, ..options("en300706") };
        let mut applied = false;
        let session = load_sandboxed(&given, None, || {
            std::fs::write(&path, "PN,10000\nOL,1,HELLO\n").unwrap();
            applied = true;
            Ok(())
        })
        .unwrap();
        assert!(applied);
        assert_eq!(session.pages().len(), 1);
    }

    /// REQ-430: a sandbox that cannot be applied stops the load before a
    /// byte of the input is parsed.
    #[test]
    fn a_sandbox_that_fails_stops_the_load() {
        let given = Options { input: env!("CARGO_MANIFEST_DIR").to_owned() + "/Cargo.toml", ..options("en300706") };
        let result = load_sandboxed(&given, None, || Err((exit::INPUT, "refused".to_owned())));
        assert!(matches!(result, Err((exit::INPUT, ref message)) if message == "refused"), "{:?}", result.err());
    }

    /// REQ-SEC-003: a broken input is refused, not a panic.
    #[test]
    fn an_input_reporting_more_than_it_was_given_is_refused() {
        let result = pump(&mut Overreporting, &mut Unfed, &mut |_| {});
        assert!(matches!(result, Err((exit::INPUT, _))), "{result:?}");
    }
}
