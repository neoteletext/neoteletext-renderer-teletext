//! A font library's index, `fonts.txt` (REQ-124, REQ-FNT-004, decision
//! E.75), and the check of a font's bytes against the SHA-256 its library
//! names (REQ-123, REQ-FNT-003).
//!
//! A library is a folder in the form of a channel's folder (REQ-WEB-023)
//! holding the index. Fields are separated by one tab, written `→` here;
//! comments and empty lines are as `contents.txt` has them (REQ-WEB-025):
//!
//! ```text
//! # comments and empty lines are ignored
//! f→english.bdf→Mode 7 English→<64 hexadecimal digits>
//! f→https://fonts.example/g.bdf→Elsewhere→<64 hexadecimal digits>
//! c→english.bdf→G0-Latin* G0-Latin-English* G2-Latin
//! ```
//!
//! The reader is strict and bounded and allocates nothing: [`read`]
//! checks the index's size and line count, then [`Entries`] yields each
//! line as an entry or a fault. A fault never stops the reading; the
//! caller counts and reports them. Reading the index fetches no font: a
//! port lists the fonts by title and fetches one only when the user
//! chooses it (REQ-124).
//!
//! The character sets a `c` line names are checked by the caller's
//! `is_set_name`, so that this crate needs nothing of the core's glyph
//! layer (ARCHITECTURE.md section 9); a port passes the core's
//! `glyphs::conformance::font_set_index` (REQ-FNT-011, DEC-2428).

use crate::sha256::{self, Digest};
use crate::url::{Scheme, Url, UrlError};

/// The index's file name in the library's folder (REQ-124).
pub const INDEX_FILE: &str = "fonts.txt";
/// The largest index read (REQ-124).
pub const MAX_BYTES: usize = 64 * 1024;
/// The most lines read (REQ-124).
pub const MAX_LINES: usize = 1000;
/// The longest title of a font, in characters (REQ-124: 1 to 32).
pub const MAX_TITLE: usize = 32;
/// The longest file name of a font in the library's folder: one segment of
/// a folder path (REQ-WEB-023).
pub const MAX_FILE_NAME: usize = crate::channel::MAX_SEGMENT;
/// The field separator.
const TAB: u8 = b'\t';
/// The first character of a comment line.
const COMMENT: u8 = b'#';
/// The separator of the set names of a `c` line.
const SET_SEPARATOR: char = ' ';
/// The mark after a set's name when the font also meets the SAA5050 cell
/// profile for it (REQ-124, REQ-FNT-011).
const SAA5050_MARK: char = '*';
/// The first and last printable ASCII characters, which with the tab are
/// the only bytes an index line may hold.
const PRINTABLE: core::ops::RangeInclusive<u8> = b' '..=b'~';

/// Why an index was refused whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LibraryError {
    /// More than [`MAX_BYTES`].
    TooLarge,
    /// More than [`MAX_LINES`].
    TooManyLines,
}

/// Why one line was ignored: each is a malformed line (REQ-124), except
/// [`Fault::UnknownKind`], which is ignored but is no fault of the index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// A kind other than `f` and `c`, kept for later versions; ignored and
    /// not counted as malformed.
    UnknownKind,
    /// The wrong number of fields for its kind.
    Fields,
    /// A byte outside printable ASCII, other than the tab.
    Character,
    /// A font that is neither a file name in the library's folder nor an
    /// `http` or `https` address.
    Location,
    /// A title that is empty or longer than [`MAX_TITLE`] characters.
    Title,
    /// A SHA-256 that is not 64 hexadecimal digits.
    Checksum,
    /// An `f` line for a font an earlier `f` line lists.
    DuplicateFont,
    /// A `c` line for a font no well-formed `f` line lists.
    Unlisted,
    /// A `c` line for a font an earlier `c` line names.
    SecondConformance,
    /// A set name the port does not know, or set names not separated by
    /// single spaces.
    SetName,
}

impl Fault {
    /// Whether the line is malformed, to be counted (REQ-124): every fault
    /// but an unknown kind.
    #[must_use]
    pub const fn is_malformed(self) -> bool {
        !matches!(self, Self::UnknownKind)
    }
}

/// Where a font is: a file in the library's folder, or an address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Location<'a> {
    /// A file name in the library's folder, one segment of a folder path
    /// (REQ-WEB-023).
    File(&'a str),
    /// An `http` or `https` address, checked by [`Url::parse`].
    Address(&'a str),
}

impl<'a> Location<'a> {
    /// The font's location as the index writes it.
    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        match self {
            Self::File(name) | Self::Address(name) => name,
        }
    }

    /// The address to fetch the font from, a file name resolved against
    /// the library's folder (REQ-123). Only a port that fetches calls it,
    /// once the user has chosen the font.
    ///
    /// # Errors
    ///
    /// The address does not parse, or resolves to another scheme.
    pub fn address(&self, library_folder: &Url) -> Result<Url, UrlError> {
        let url = match self {
            Self::File(name) => library_folder.join(name)?,
            Self::Address(address) => Url::parse(address)?,
        };
        if url.scheme().is_websocket() { Err(UrlError::Scheme) } else { Ok(url) }
    }
}

/// A font the library lists: an `f` line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Font<'a> {
    /// Where it is.
    pub location: Location<'a>,
    /// Its title, 1 to [`MAX_TITLE`] characters.
    pub title: &'a str,
    /// Its SHA-256, which the fetched file must have (REQ-123).
    pub sha256: Digest,
}

/// The sets a `c` line names: each set's name, and whether the font also
/// meets the SAA5050 cell profile for it.
#[derive(Clone, Debug)]
pub struct SetNames<'a> {
    names: core::str::Split<'a, char>,
}

impl<'a> Iterator for SetNames<'a> {
    type Item = (&'a str, bool);

    fn next(&mut self) -> Option<Self::Item> {
        let name = self.names.next()?;
        Some(name.strip_suffix(SAA5050_MARK).map_or((name, false), |plain| (plain, true)))
    }
}

/// The stated conformance of a font the library lists: a `c` line. It is
/// used only to list the font before it is loaded; once loaded, the
/// conformance computed from the font is used (decision E.75 item 3).
#[derive(Clone, Debug)]
pub struct Conformance<'a> {
    /// The font, as its `f` line names it.
    pub location: Location<'a>,
    sets: &'a str,
}

impl<'a> Conformance<'a> {
    /// The sets it states, in the line's order.
    #[must_use]
    pub fn sets(&self) -> SetNames<'a> {
        // An empty field names no set; `split` would give one empty name.
        let mut names = self.sets.split(SET_SEPARATOR);
        if self.sets.is_empty() {
            let _ = names.next();
        }
        SetNames { names }
    }
}

/// One entry of the index.
#[derive(Clone, Debug)]
pub enum Entry<'a> {
    /// An `f` line.
    Font(Font<'a>),
    /// A `c` line.
    Conformance(Conformance<'a>),
}

/// One line that was ignored, by its number from 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineFault {
    /// The line.
    pub line: usize,
    /// Why.
    pub fault: Fault,
}

/// The numbered lines of an index.
type Lines<'a> = core::iter::Enumerate<core::slice::Split<'a, u8, fn(&u8) -> bool>>;

/// The entries of an index, in order.
#[derive(Clone, Debug)]
pub struct Entries<'a> {
    bytes: &'a [u8],
    lines: Lines<'a>,
    is_set_name: fn(&str) -> bool,
}

#[allow(clippy::trivially_copy_pass_by_ref, reason = "the predicate `slice::split` calls takes a reference")]
fn is_line_end(byte: &u8) -> bool {
    *byte == b'\n'
}

/// The numbered lines of `bytes`.
fn lines_of(bytes: &[u8]) -> Lines<'_> {
    bytes.split(is_line_end as fn(&u8) -> bool).enumerate()
}

/// Checks an index's size and length, and returns its entries. A set name
/// of a `c` line is known when `is_set_name` says so.
///
/// # Errors
///
/// The index is larger than [`MAX_BYTES`] or longer than [`MAX_LINES`].
pub fn read(bytes: &[u8], is_set_name: fn(&str) -> bool) -> Result<Entries<'_>, LibraryError> {
    if bytes.len() > MAX_BYTES {
        return Err(LibraryError::TooLarge);
    }
    #[allow(clippy::naive_bytecount, reason = "at most 64 kilobytes, counted once; no crate is added for it")]
    let lines = bytes.iter().filter(|&&byte| byte == b'\n').count();
    let unterminated = usize::from(bytes.last().is_some_and(|&byte| byte != b'\n'));
    if lines.saturating_add(unterminated) > MAX_LINES {
        return Err(LibraryError::TooManyLines);
    }
    Ok(Entries { bytes, lines: lines_of(bytes), is_set_name })
}

/// Checks that `bytes` are the font `expected` names (REQ-123: "When a font
/// is named with a SHA-256, a file with any other checksum shall be
/// refused"). The process that draws computes the digest itself, from the
/// bytes it was given.
///
/// # Errors
///
/// The bytes' SHA-256 is another, which is returned.
pub fn check_font(bytes: &[u8], expected: &Digest) -> Result<(), Digest> {
    let actual = sha256::digest(bytes);
    if actual == *expected { Ok(()) } else { Err(actual) }
}

/// A line's text, without its carriage return; `None` for a comment or an
/// empty line.
fn content(raw: &[u8]) -> Option<&[u8]> {
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    (!raw.is_empty() && raw.first() != Some(&COMMENT)).then_some(raw)
}

/// A line's fields: its kind and up to four more, and whether there were
/// more than that.
struct Fields<'a> {
    kind: &'a str,
    rest: [Option<&'a str>; 3],
    extra: bool,
}

/// Splits a line into fields, once its bytes are checked.
fn fields(raw: &[u8]) -> Result<Fields<'_>, Fault> {
    if !raw.iter().all(|&byte| byte == TAB || PRINTABLE.contains(&byte)) {
        return Err(Fault::Character);
    }
    // Printable ASCII is UTF-8.
    let text = core::str::from_utf8(raw).map_err(|_| Fault::Character)?;
    let mut split = text.split(char::from(TAB));
    let kind = split.next().unwrap_or("");
    let rest = [split.next(), split.next(), split.next()];
    Ok(Fields { kind, rest, extra: split.next().is_some() })
}

/// A font's location as written.
fn location(field: &str) -> Result<Location<'_>, Fault> {
    if field.contains("://") {
        let url = Url::parse(field).map_err(|_| Fault::Location)?;
        return match url.scheme() {
            Scheme::Http | Scheme::Https => Ok(Location::Address(field)),
            Scheme::Ws | Scheme::Wss => Err(Fault::Location),
        };
    }
    crate::channel::check_segment(field.as_bytes()).map_err(|_| Fault::Location)?;
    Ok(Location::File(field))
}

/// A title: 1 to [`MAX_TITLE`] characters. The line is printable ASCII, so
/// its bytes are its characters.
const fn title(field: &str) -> Result<&str, Fault> {
    if field.is_empty() || field.len() > MAX_TITLE { Err(Fault::Title) } else { Ok(field) }
}

/// The font of an `f` line, judged on its own.
fn font<'a>(fields: &Fields<'a>) -> Result<Font<'a>, Fault> {
    let [Some(at), Some(name), Some(hash)] = fields.rest else { return Err(Fault::Fields) };
    if fields.extra {
        return Err(Fault::Fields);
    }
    let location = location(at)?;
    let title = title(name)?;
    let sha256 = sha256::parse_hex(hash.as_bytes()).ok_or(Fault::Checksum)?;
    Ok(Font { location, title, sha256 })
}

/// Checks the set names of a `c` line: names the port knows, each
/// followed by `*` or not, separated by single spaces. An empty field
/// states no set.
fn set_names(field: &str, is_set_name: fn(&str) -> bool) -> Result<(), Fault> {
    let probe = Conformance { location: Location::File(""), sets: field };
    if probe.sets().all(|(name, _)| is_set_name(name)) { Ok(()) } else { Err(Fault::SetName) }
}

impl<'a> Entries<'a> {
    /// The fields of the `f` and `c` lines before line `before` (from 0),
    /// or of every line when `before` is `None`, whose first field is
    /// `location`.
    fn lines_naming(
        &self,
        kind: &'static str,
        location: &'a str,
        before: Option<usize>,
    ) -> impl Iterator<Item = Fields<'a>> {
        let end = before.unwrap_or(usize::MAX);
        lines_of(self.bytes)
            .take_while(move |(index, _)| *index < end)
            .filter_map(|(_, raw)| fields(content(raw)?).ok())
            .filter(move |fields| fields.kind == kind && fields.rest[0] == Some(location))
    }

    /// The entry of a `c` line at `index`, judged against the whole index:
    /// its font must be listed by a well-formed `f` line, the first that
    /// names it, and no earlier well-formed `c` line may name it (REQ-124).
    /// An earlier malformed `c` line states nothing, so it does not make
    /// this one the second. Bounded by [`MAX_LINES`] lines read for each
    /// line, so at most a million lines for the whole index.
    fn conformance(&self, index: usize, fields: &Fields<'a>) -> Result<Conformance<'a>, Fault> {
        let conformance = conformance_alone(fields, self.is_set_name)?;
        let at = conformance.location.as_str();
        let listed = self.lines_naming("f", at, None).next().is_some_and(|first| font(&first).is_ok());
        if !listed {
            return Err(Fault::Unlisted);
        }
        let is_set_name = self.is_set_name;
        if self.lines_naming("c", at, Some(index)).any(|earlier| conformance_alone(&earlier, is_set_name).is_ok()) {
            return Err(Fault::SecondConformance);
        }
        Ok(conformance)
    }
}

/// The conformance of a `c` line, judged on its own.
fn conformance_alone<'a>(fields: &Fields<'a>, is_set_name: fn(&str) -> bool) -> Result<Conformance<'a>, Fault> {
    // A font that conforms for no set may end its line after its name, as
    // the Noto pipeline's `convert.py` writes it, or with an empty field,
    // as `neottx font sets --library-line` does: both state no set.
    let [Some(at), sets, None] = fields.rest else { return Err(Fault::Fields) };
    if fields.extra {
        return Err(Fault::Fields);
    }
    let sets = sets.unwrap_or("");
    let location = location(at)?;
    set_names(sets, is_set_name)?;
    Ok(Conformance { location, sets })
}

impl<'a> Iterator for Entries<'a> {
    type Item = Result<Entry<'a>, LineFault>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (index, raw) = self.lines.next()?;
            let Some(raw) = content(raw) else { continue };
            let line = index.saturating_add(1);
            let entry = fields(raw).and_then(|fields| match fields.kind {
                "f" => {
                    let font = font(&fields)?;
                    if self.lines_naming("f", font.location.as_str(), Some(index)).next().is_some() {
                        return Err(Fault::DuplicateFont);
                    }
                    Ok(Entry::Font(font))
                }
                "c" => self.conformance(index, &fields).map(Entry::Conformance),
                _ => Err(Fault::UnknownKind),
            });
            return Some(entry.map_err(|fault| LineFault { line, fault }));
        }
    }
}

#[cfg(test)]
mod tests;
