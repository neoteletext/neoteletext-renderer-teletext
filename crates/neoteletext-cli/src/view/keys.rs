//! The viewer's keys: a terminal key as a viewer key, and the key that
//! leaves a channel's keys.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use neoteletext_core::viewer::Key;
use std::time::Duration;

/// The Index key, which held means Back where the terminal reports
/// releases (REQ-NAV-010, DEC-2477).
const INDEX: char = 'i';

/// The key that quits the viewer, besides Escape and Control-C.
const QUIT: char = 'q';

/// How long Index must be held, from its press to its release, to mean
/// Back on a terminal that reports releases but sent no repeat (DEC-2477):
/// half a second, the usual delay before a keyboard starts to repeat (the
/// Windows default, and within the range of the X server's and macOS's),
/// so a hold long enough to repeat is a hold here too.
pub(super) const HOLD_FOR_BACK: Duration = Duration::from_millis(500);

/// The two keys a user gives `neottx view` for Back and Forward, as a
/// remote control's configurable keys (REQ-NAV-010, DEC-2477): none by
/// default.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct RemoteKeys {
    back: Option<char>,
    forward: Option<char>,
}

impl RemoteKeys {
    /// The keys `--back-key` and `--forward-key` name.
    ///
    /// # Errors
    /// A message for the user when a key is not a printable character, is
    /// one of the viewer's own keys, or is given for both.
    pub fn new(back: Option<char>, forward: Option<char>) -> Result<Self, String> {
        for (name, key) in [("--back-key", back), ("--forward-key", forward)] {
            let Some(key) = key else { continue };
            if key.is_control() || key.is_whitespace() {
                return Err(format!("{name}: {key:?} is not a printable key"));
            }
            if key == QUIT || map_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)).is_some() {
                return Err(format!("{name}: {key:?} is already one of the viewer's keys"));
            }
        }
        if back.is_some() && back == forward {
            return Err("--back-key and --forward-key name the same key".to_owned());
        }
        Ok(Self { back, forward })
    }

    /// Back or Forward, when `key` is one of the configured keys, typed
    /// without Control or Alt.
    pub(super) fn key(self, key: KeyEvent) -> Option<Key> {
        let KeyCode::Char(typed) = key.code else { return None };
        if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return None;
        }
        if Some(typed) == self.back {
            Some(Key::Back)
        } else if Some(typed) == self.forward {
            Some(Key::Forward)
        } else {
            None
        }
    }
}

/// Whether `key` is the Index key, typed without Control or Alt.
pub(super) fn is_index(key: KeyEvent) -> bool {
    key.code == KeyCode::Char(INDEX) && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// The key that leaves a channel's keys, as telnet's escape character
/// leaves its host's: Control and the right square bracket.
const LEAVE_TYPING: char = ']';

/// The same key on a terminal without the Kitty keyboard protocol: it sends
/// the control byte 0x1D, which crossterm reads as Control and 5, naming
/// 0x1C to 0x1F Control-4 to Control-7. Control and 5 itself cannot be told
/// from it there, and leaves on every terminal.
const LEAVE_TYPING_AS_READ: char = '5';

/// Whether `key` leaves a channel's keys, or takes them back.
pub(super) fn leaves_typing(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(LEAVE_TYPING | LEAVE_TYPING_AS_READ))
        && key.kind != KeyEventKind::Release
}

pub(super) fn map_key(key: KeyEvent) -> Option<Key> {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    Some(match key.code {
        KeyCode::Char(c @ '0'..='9') => Key::Digit((c as u8).saturating_sub(b'0')),
        KeyCode::Char('r') | KeyCode::F(1) => Key::Red,
        KeyCode::Char('g') | KeyCode::F(2) => Key::Green,
        KeyCode::Char('y') | KeyCode::F(3) => Key::Yellow,
        KeyCode::Char('c') | KeyCode::F(4) => Key::Cyan,
        KeyCode::Char(INDEX) => Key::Index,
        KeyCode::Char('h') | KeyCode::Home => Key::Home,
        KeyCode::Up | KeyCode::Char('+') => Key::NextPage,
        KeyCode::Down | KeyCode::Char('-') => Key::PreviousPage,
        KeyCode::Left if alt => Key::Back,
        KeyCode::Right if alt => Key::Forward,
        KeyCode::Right => Key::NextSubPage,
        KeyCode::Left => Key::PreviousSubPage,
        KeyCode::Char('?') => Key::Reveal,
        KeyCode::Char('H') => Key::Hold,
        KeyCode::Char('m') => Key::Mix,
        KeyCode::Char('z') => Key::Zoom,
        // DEC-2363: `s`, then four digits, keys a sub-page.
        KeyCode::Char('s') => Key::SubPage,
        KeyCode::Backspace => Key::Back,
        _ => return None,
    })
}
