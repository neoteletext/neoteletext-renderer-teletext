//! Capabilities (DOC-1922, REQ-INT-106):
//! what a receiver can do, offered in HELLO and agreed in WELCOME. There are
//! no input modes: every event is sent and a channel ignores what it does
//! not use (DOC-1922, decision E.125).

/// Bits 0 to 15 were the input modes until decision E.125: reserved, never
/// set, and ignored when a peer still sets them, so an older channel's
/// WELCOME is not refused for asking for a mode.
pub const RETIRED_MODE_BITS: u32 = 0xFFFF;

/// Bit 16 was the capability of PAGE until DEC-1538 (REQ-1556): withdrawn,
/// never set, and a protocol error when set in HELLO or WELCOME.
pub const RETIRED_PAGE_BIT: u32 = 1 << 16;
/// Capability: the receiver draws the channel's cursor record.
pub const CURSOR_CAP: u32 = 1 << 17;
/// Bit 18 was the capability of provisional echo until decision E.134,
/// which withdrew echo: reserved, never set, and ignored when a peer still
/// sets it, so an older channel's WELCOME is not refused for agreeing it.
pub const RETIRED_ECHO_BIT: u32 = 1 << 18;
/// Capability: HELLO carries the user agent string (HELLO only).
pub const AGENT: u32 = 1 << 19;
/// Capability: touch (REQ-RCH-041 to REQ-RCH-043): MOUSE's device bits and
/// the TOUCH extension are sent; without it touch goes as the mouse, the
/// first contact alone.
pub const TOUCH_CAP: u32 = 1 << 20;
/// Capability: a lost connection's session may be resumed (REQ-RCH-013):
/// WELCOME carries a token, and a reconnection's first message carries
/// RESUME after HELLO (DOC-1923).
pub const RESUME_CAP: u32 = 1 << 21;
/// Capability: the receiver shows teletext pages and sends SELECT, the
/// connection's selection (DOC-1753, REQ-INT-140); a channel that sets it sends the
/// one sub-page selected, never a page's sub-pages together (DEC-1500).
pub const SELECT_CAP: u32 = 1 << 22;
/// Capability: the receiver draws a sub-page's ReGIS from the REGIS record
/// (type 14), with `DRAWING_LINKS` and `REGIS_CURSOR`, as REQ-6341 and the
/// ReGIS repository's `docs/regis-v2.md` say (renderer parity PAR-1,
/// REQ-6761). A server agrees it only when offered; a receiver that does not
/// offer it is given the mosaic fallback for ReGIS pictures (PAR-24).
pub const REGIS_CAP: u32 = 1 << 23;
/// Capability: the receiver draws a sub-page's NAPLPS from the NAPLPS record
/// (type 17), with `NAPLPS_OBJECTS`, `DRAWING_LINKS` and `REGIS_CURSOR` for
/// its objects, on the 480 by 460 dots from line 20 of the teletext raster
/// (REQ-5819, DEC-6857), every NeoTeletext coordinate of the page being a
/// dot of that raster (DEC-6853): neoteletext-common
/// `docs/naplps-raster.md` RAS-9, renderer parity PAR-1, REQ-6761. A
/// receiver drops the NAPLPS and `NAPLPS_OBJECTS` records on a connection
/// whose WELCOME did not agree it.
pub const NAPLPS_CAP: u32 = 1 << 24;
/// Capability: the receiver asks for one page as a stranger sees it and
/// nothing else (REQ-6879 item 1, DEC-6875 item 3; neoteletext-common
/// `docs/tab-thumbnails.md` section 3). A channel that agrees it creates no
/// viewer, sends no viewer-state challenge, tells its program no connection
/// exists (the program may be asked to draw the page for no one in
/// particular, a request that names no connection and no viewer), answers
/// one SELECT with that page or DENIED, and closes. The lowest free bit:
/// 23 and 24 are the picture languages'. A receiver that offers it closes
/// at once, with a normal close, when WELCOME does not agree it
/// ([`crate::snapshot::welcome_agrees_snapshot`]), so a channel that does
/// not know the bit, and so never agrees it, makes no viewer of it.
pub const SNAPSHOT_CAP: u32 = 1 << 25;
/// Capability: the receiver plays the channel's music and a magazine's and
/// applies a page's two music switches (REQ-6927 item 1, DEC-6924 item 9):
/// a channel that agrees it may send a CONTROL record naming a level of
/// music, the blocks of that music's live sound and the `PAGE_MUSIC` record
/// of a page's switches, and sends none of them on a connection whose
/// WELCOME did not agree it. It is offered only under protocol version 11
/// ([`caps_of`]), and never agreed with [`SNAPSHOT_CAP`]
/// ([`agreeable`]).
pub const MAGAZINE_SOUND_CAP: u32 = 1 << 26;
/// What a snapshot receiver offers (REQ-6879 item 2): SNAPSHOT, SELECT and
/// both picture languages; never CURSOR, TOUCH, RESUME or AGENT, so that
/// HELLO carries no user agent text and nothing can be resumed.
pub const SNAPSHOT_OFFER: u32 = SNAPSHOT_CAP | SELECT_CAP | REGIS_CAP | NAPLPS_CAP;
/// Every capability defined.
pub const ALL_CAPS: u32 = CURSOR_CAP
    | AGENT
    | TOUCH_CAP
    | RESUME_CAP
    | SELECT_CAP
    | REGIS_CAP
    | NAPLPS_CAP
    | SNAPSHOT_CAP
    | MAGAZINE_SOUND_CAP;

/// The capabilities protocol version `version` defines (DEC-6953 item 3):
/// every one for [`VERSION`](crate::VERSION), and every one but
/// [`MAGAZINE_SOUND_CAP`] for a version before 11, which never offers or
/// agrees it.
#[must_use]
pub const fn caps_of(version: u32) -> u32 {
    if version >= MAGAZINE_SOUND_VERSION { ALL_CAPS } else { ALL_CAPS & !MAGAZINE_SOUND_CAP }
}

/// The first protocol version with [`MAGAZINE_SOUND_CAP`] (REQ-6927
/// item 2).
pub const MAGAZINE_SOUND_VERSION: u32 = 11;
const _: () = assert!(MAGAZINE_SOUND_VERSION <= crate::VERSION);

/// Whether a WELCOME may agree `caps` together (REQ-6927 item 4): a
/// snapshot connection draws one page and plays nothing, so
/// [`SNAPSHOT_CAP`] and [`MAGAZINE_SOUND_CAP`] are never agreed together.
#[must_use]
pub const fn agreeable(caps: u32) -> bool {
    caps & SNAPSHOT_CAP == 0 || caps & MAGAZINE_SOUND_CAP == 0
}
// A retired bit is never a capability.
const _: () = assert!(ALL_CAPS & (RETIRED_MODE_BITS | RETIRED_ECHO_BIT | RETIRED_PAGE_BIT) == 0);
// A snapshot receiver offers nothing that identifies or resumes it
// (REQ-6879 item 2).
const _: () = assert!(SNAPSHOT_OFFER & (CURSOR_CAP | TOUCH_CAP | RESUME_CAP | AGENT) == 0);
// Nor magazine music, which it could never be agreed (REQ-6927 item 4).
const _: () = assert!(SNAPSHOT_OFFER & MAGAZINE_SOUND_CAP == 0);
