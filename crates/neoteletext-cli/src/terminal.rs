//! Terminal capabilities: graphics protocol detection (REQ-RND-029,
//! REQ-382, DEC-2459) and size.
//!
//! Detection asks the terminal, where the viewer's terminal library is
//! built in: the kitty graphics query, XTVERSION and Primary Device
//! Attributes, whose replies `neoteletext_app::terminal_reply` reads. The
//! order of preference is kitty, Sixel, iTerm2, then characters. A build
//! without the viewer reads only the environment, for iTerm2.

use neoteletext_app::terminal_reply::Replies;

/// How a page is shown in the terminal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Display {
    /// The kitty graphics protocol.
    Kitty,
    /// Sixel graphics.
    Sixel,
    /// iTerm2 inline images.
    Iterm2,
    /// Characters with ANSI colour.
    Text,
}

/// The display to use, and the reason when graphics were not chosen.
#[must_use]
pub fn detect(forced: Option<Display>, is_terminal: bool) -> (Display, Option<&'static str>) {
    if let Some(display) = forced {
        return (display, None);
    }
    if !is_terminal {
        return (Display::Text, Some("output is not a terminal"));
    }
    if std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some() {
        return (Display::Text, Some("inside tmux or GNU screen: graphics may not pass through"));
    }
    let iterm_environment = std::env::var("TERM_PROGRAM").is_ok_and(|value| value == "iTerm.app")
        || std::env::var("LC_TERMINAL").is_ok_and(|value| value == "iTerm2");
    choose(query(), iterm_environment)
}

/// The display the replies call for, in the order kitty, Sixel, iTerm2,
/// characters; `None` replies are a terminal that did not answer.
#[must_use]
pub fn choose(replies: Option<Replies>, iterm_environment: bool) -> (Display, Option<&'static str>) {
    match replies {
        Some(replies) if replies.kitty_graphics => (Display::Kitty, None),
        Some(replies) if replies.sixel => (Display::Sixel, None),
        Some(replies) if replies.is_iterm2() => (Display::Iterm2, None),
        _ if iterm_environment => (Display::Iterm2, None),
        Some(_) => (Display::Text, Some("the terminal reports no kitty, Sixel or iTerm2 graphics")),
        None => (Display::Text, Some("the terminal did not answer the graphics queries")),
    }
}

/// The longest the terminal is given to answer the queries. A terminal
/// answers Primary Device Attributes, the last query, as soon as it reads
/// it, so the wait ends with that reply; the ceiling only bounds a
/// terminal that never answers (DEC-2459).
#[cfg(feature = "view")]
const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Asks the terminal the graphics queries and reads its replies, in raw
/// mode, before the sandbox closes the terminal device. `None` when the
/// terminal cannot be opened or did not answer in time.
#[cfg(feature = "view")]
fn query() -> Option<Replies> {
    use neoteletext_app::terminal_reply::{MAX_REPLY_BYTES, QUERIES, parse};
    use std::io::{Read, Write};
    let mut tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?;
    let mut reader = tty.try_clone().ok()?;
    crossterm::terminal::enable_raw_mode().ok()?;
    let sent = tty.write_all(QUERIES).and_then(|()| tty.flush());
    let (bytes_sent, bytes) = std::sync::mpsc::channel::<Vec<u8>>();
    if sent.is_ok() {
        // The reader blocks on the terminal; it ends once the last reply is
        // in, or the bound is read, or nothing more comes. A terminal that
        // never answers leaves it waiting for a byte, which it then drops.
        std::thread::spawn(move || {
            let mut chunk = [0u8; MAX_REPLY_BYTES];
            let mut read = 0;
            while read < MAX_REPLY_BYTES {
                let Ok(count) = reader.read(&mut chunk) else { return };
                if count == 0 || bytes_sent.send(chunk.get(..count).unwrap_or(&[]).to_vec()).is_err() {
                    return;
                }
                read = read.saturating_add(count);
            }
        });
    }
    let deadline = std::time::Instant::now().checked_add(QUERY_TIMEOUT)?;
    let mut received = Vec::new();
    let mut replies = None;
    while sent.is_ok() && received.len() < MAX_REPLY_BYTES {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(more) = bytes.recv_timeout(left) else { break };
        received.extend_from_slice(&more);
        let read = parse(&received);
        if read.complete {
            replies = Some(read);
            break;
        }
    }
    let _ = crossterm::terminal::disable_raw_mode();
    replies
}

/// A build without the viewer has no terminal library to set raw mode
/// with, so it reads only the environment.
#[cfg(not(feature = "view"))]
const fn query() -> Option<Replies> {
    None
}

/// The terminal's text area in pixels, measured before the sandbox closes
/// the terminal device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextArea {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Text rows.
    pub rows: usize,
}

/// The text area, if the terminal reports its size in pixels.
#[cfg(feature = "view")]
#[must_use]
pub fn text_area() -> Option<TextArea> {
    let size = crossterm::terminal::window_size().ok()?;
    let area =
        TextArea { width: usize::from(size.width), height: usize::from(size.height), rows: usize::from(size.rows) };
    (area.width > 0 && area.height > 0 && area.rows > 0).then_some(area)
}

/// A build without the viewer has no terminal library to measure with,
/// so `show` draws images at scale 1 unless `--scale` names another.
#[cfg(not(feature = "view"))]
#[must_use]
pub const fn text_area() -> Option<TextArea> {
    None
}

/// The largest whole-number scale at which a page of `page_width` by
/// `page_height` pixels fits the text area, keeping one text row free for
/// the prompt (REQ-RND-029). At least 1.
#[must_use]
pub fn fit_scale(area: Option<TextArea>, page_width: usize, page_height: usize, aspect: bool) -> usize {
    let Some(area) = area else { return 1 };
    // A terminal that reports no rows keeps no row free.
    let row_height = area.height.checked_div(area.rows).unwrap_or(0);
    let usable_height = area.height.saturating_sub(row_height);
    let (page_width, page_height) = neoteletext_image::scale::scaled_size(page_width, page_height, 1, 1, aspect);
    let by_width = area.width.checked_div(page_width).unwrap_or(area.width);
    let by_height = usable_height.checked_div(page_height).unwrap_or(usable_height);
    by_width.min(by_height).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-382: kitty comes before Sixel, Sixel before iTerm2, and iTerm2
    /// before characters; a terminal that answered nothing, or no graphics,
    /// gets characters with the reason.
    #[test]
    fn the_order_of_preference() {
        let replies = |kitty_graphics, sixel, version: Option<&str>| {
            Some(Replies { kitty_graphics, sixel, version: version.map(str::to_owned), complete: true })
        };
        assert_eq!(choose(replies(true, true, Some("iTerm2 3.5")), true).0, Display::Kitty);
        assert_eq!(choose(replies(false, true, Some("iTerm2 3.5")), false).0, Display::Sixel);
        assert_eq!(choose(replies(false, false, Some("iTerm2 3.5")), false).0, Display::Iterm2);
        assert_eq!(choose(replies(false, false, None), true).0, Display::Iterm2);
        assert_eq!(
            choose(replies(false, false, Some("XTerm(390)")), false),
            (Display::Text, Some("the terminal reports no kitty, Sixel or iTerm2 graphics"))
        );
        assert_eq!(choose(None, false), (Display::Text, Some("the terminal did not answer the graphics queries")));
    }

    /// REQ-RND-029: the scale fits the width and the height.
    #[test]
    fn scale_fits_width_and_height() {
        let area = Some(TextArea { width: 1500, height: 1300, rows: 50 });
        assert_eq!(fit_scale(area, 480, 500, false), 2);
        assert_eq!(fit_scale(Some(TextArea { width: 700, height: 1300, rows: 50 }), 480, 500, false), 1);
        assert_eq!(fit_scale(area, 480, 500, true), 2);
        assert_eq!(fit_scale(None, 480, 500, false), 1);
        assert_eq!(fit_scale(Some(TextArea { width: 100, height: 100, rows: 5 }), 480, 500, false), 1);
    }
}
