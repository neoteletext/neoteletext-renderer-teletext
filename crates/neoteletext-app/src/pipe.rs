//! The framed messages between the fetcher and decoder processes
//! (REQ-SEC-018, decision 26 of IMPLEMENTATION-LOG.md). Each frame is a
//! kind byte, a 32-bit big-endian length and the payload. Both sides bound
//! the length, parse strictly and reject unknown kinds.

mod codec;

pub use codec::{read, write};

/// The longest input message a frame carries (DOC-1918):
/// the web module's `INPUT_MESSAGE_MAX` and the input crate's
/// `MAX_MESSAGE`, which this crate may be built without. The command-line
/// program asserts that they agree.
pub const MAX_INPUT_MESSAGE: usize = 8192;
/// The largest payload: one page response (REQ-WEB-008 default).
pub const MAX_FRAME: usize = 256 * 1024;
/// The most rows a part of a page's list of sub-pages holds (DOC-2038): the
/// web module's `subpages::ROWS_PER_PART`, which this crate may be built
/// without. The fetcher asserts that they agree.
pub const MAX_LIST_ROWS: usize = 64;
/// The longest version of a page's list: 64 bits of a digest in hexadecimal,
/// the web module's `subpages::MAX_VALIDATOR`, asserted by the fetcher.
pub const MAX_LIST_VERSION: usize = 16;
/// The most pages an index record holds: it is one magazine's.
pub const MAX_INDEX_PAGES: usize = neoteletext_core::limits::PAGE_NUMBERS_PER_MAGAZINE;
/// The most sub-codes of one record of a live list: a page's every sub-page.
pub const MAX_LIVE_LIST: usize = neoteletext_core::limits::MAX_SUBPAGES;

/// Where page bytes came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// A page fetch.
    Page,
    /// The push connection.
    Push,
    /// The front door of a site without a descriptor: any number of the
    /// site's pages.
    FrontDoor,
}

/// Page bytes from the fetcher.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PageData {
    /// Came over an encrypted connection.
    pub secure: bool,
    /// Where the bytes came from.
    pub origin: Origin,
    /// TTI or T42 bytes.
    pub bytes: Vec<u8>,
}

/// A message on the pipe.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Frame {
    /// Fetcher to decoder: one 42-byte packet.
    Packet([u8; neoteletext_core::consts::PACKET_BYTES]),
    /// Fetcher to decoder: TTI page bytes.
    Tti(PageData),
    /// Fetcher to decoder: T42 page bytes.
    T42(PageData),
    /// Fetcher to decoder: page bytes whose format the server did not name
    /// (`text/plain`, `application/octet-stream`), for the decoder to detect
    /// (REQ-WEB-003).
    Undeclared(PageData),
    /// Fetcher to decoder: a notice for the viewer.
    Notice(Notice),
    /// Decoder to fetcher: request a page (`0xMPP`, sub-code or 0xFFFF).
    Request(u16, u16),
    /// Decoder to fetcher: follow a link to another service.
    Follow(String, u16, u16),
    /// Decoder to fetcher: fetch this part of page `0xMPP`'s list of
    /// sub-pages (REQ-1888), answered by [`Notice::SubPages`] or
    /// [`Notice::SubPagesMissing`].
    SubPages(u16, u8),
    /// Decoder to fetcher: one input message for the channel's push
    /// WebSocket (area INT, REQ-INT-101), at most [`MAX_INPUT_MESSAGE`]
    /// bytes.
    Input(Vec<u8>),
}

/// Notices the fetcher sends.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Notice {
    /// The service's initial page, `0xMPP`.
    InitialPage(u16),
    /// The page on screen came over plain HTTP (REQ-SEC-008).
    Unencrypted,
    /// A message for the user.
    Message(String),
    /// A new service is ready; its linked services' short names in
    /// descriptor order (Appendix E).
    Service(Vec<String>),
    /// A new service's default character set region, or none: then the
    /// receiver's own (REQ-WEB-002, decision E.4). Read before its pages.
    Region(Option<u8>),
    /// The server confirmed page `0xMPP` unchanged (REQ-WEB-004): the
    /// stored copy is current (REQ-WEB-009).
    Current(u16),
    /// Fetching page `0xMPP` failed for want of a network (REQ-WEB-009).
    Offline(u16),
    /// A request was redirected out of the channel, or the channel's
    /// program sent the viewer to another channel, and it was not followed:
    /// its address (REQ-WEB-021, REQ-1688).
    RedirectToOtherChannel(String),
    /// The channel's program sent the viewer to page `0xMPP`, at this
    /// sub-code, with the query it keeps, if any (DOC-1960).
    PageRedirect(u16, u16, Option<String>),
    /// A selection ended with this HTTP status, for this place, or for the
    /// whole channel when empty (DOC-1961).
    Denied(u16, String),
    /// The push connection ended for good and is not tried again, and why
    /// (DOC-1920).
    PushStopped(String),
    /// The service's name, from its descriptor or its host.
    ServiceName(String),
    /// The descriptor's aliases: page numbers, `0xMPP`, and their targets,
    /// `name:mpp` or an address (concept section 5 item 4).
    Aliases(Vec<(u16, String)>),
    /// Ask for the page on screen again this often, in seconds: the
    /// descriptor's refresh interval for a service without push.
    Refresh(u32),
    /// A bundle of pages arrived (a file, or a site's front door); its
    /// first page, `0xMPP`, is shown if the page asked for is not in it
    /// and the user has not moved.
    Bundle(u16),
    /// Keys may go to the channel now (true) or not (false): its push
    /// WebSocket opened or closed, and its descriptor declares input.
    Input(bool),
    /// A part of page `0xMPP`'s list of sub-pages (REQ-1888), read and
    /// checked by the fetcher: its rows' sub-codes, in the list's ascending
    /// order, at most [`MAX_LIST_ROWS`].
    SubPages(ListPart),
    /// Page `0xMPP`'s list has no such part, or it could not be fetched or
    /// read: the part number. Part 0 missing means the page keeps its
    /// sub-pages in its page file and has no list (DOC-2038).
    SubPagesMissing(u16, u8),
    /// A record of a live channel's index (DOC-1955): pages shown at once,
    /// added or replacing what was there.
    Index(IndexPart),
    /// Every record of the index sending of this generation has arrived:
    /// the pages they listed are the whole index.
    IndexComplete(u16),
    /// A record of a live page's list of sub-pages (DOC-1959, REQ-1503):
    /// a record of another version begins a new list.
    SubPageList(LiveList),
    /// Every record of page `0xMPP`'s list of this version has arrived: it
    /// is the page's whole list, and replaces the list held.
    SubPageListComplete(u16, u16),
    /// A record broke page `0xMPP`'s sending of this version: what came of
    /// it is discarded, and the page keeps the list it had.
    SubPageListRefused(u16, u16),
    /// A live sub-page's TTI Link Extension links from its LINKS records
    /// (DOC-1958, REQ-1494), written as a TTI page file of its `PN` and `SC`
    /// and its `LN`, `LK` and `LT` lines, for the same store as a page
    /// file's; none replaces the links held for the sub-page.
    Links(Vec<u8>),
}

/// One page of a live channel's index.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexEntry {
    /// The page, `0xMPP`.
    pub page: u16,
    /// Its number of sub-pages.
    pub subpages: u16,
    /// Its title.
    pub title: String,
}

/// One record of a live channel's index, as the fetcher passes it on: one
/// magazine's pages, at most [`MAX_INDEX_PAGES`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexPart {
    /// The sending's generation.
    pub generation: u16,
    /// The pages.
    pub pages: Vec<IndexEntry>,
}

/// One record of a live page's list of sub-pages: at most
/// [`MAX_LIVE_LIST`] sub-codes, in the list's ascending order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LiveList {
    /// The page, `0xMPP`.
    pub page: u16,
    /// The list's version.
    pub version: u16,
    /// The record's sub-codes.
    pub subcodes: Vec<u16>,
}

/// A part of a page's list of sub-pages, as the fetcher passes it on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListPart {
    /// The page, `0xMPP`.
    pub page: u16,
    /// The part's number: it holds the rows from place
    /// `part × MAX_LIST_ROWS`.
    pub part: u8,
    /// The page's number of sub-pages.
    pub total: u16,
    /// The list's version: parts of another version belong to another list.
    pub version: String,
    /// The rows' sub-codes, in order.
    pub subcodes: Vec<u16>,
}

#[cfg(test)]
mod tests;
