//! The services a web session opened, numbered in the order they opened
//! (REQ-NAV-010, REQ-295): the fetcher holds their addresses, and the
//! decoder names one by its number to return to it, never by an address
//! (REQ-SEC-018), so it can reopen only a service the fetcher opened.

use crate::pipe::MAX_SESSION_SERVICES;

/// How a service of the session is opened again.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Opened {
    /// The address it was opened at: its descriptor's, or its channel's
    /// folder.
    pub(super) address: String,
    /// It is a channel (REQ-WEB-018), opened by its folder.
    pub(super) channel: bool,
}

/// The session's services by number: the first opened is 0, and each link
/// followed to another service takes the next, as the decoder numbers them.
/// A link whose service the descriptor does not name still takes its number,
/// with nothing to reopen, so that the two processes keep counting alike.
#[derive(Debug)]
pub(super) struct SessionServices(Vec<Option<Opened>>);

impl SessionServices {
    /// The session of the service or channel opened first.
    pub(super) fn new(first: Opened) -> Self {
        Self(vec![Some(first)])
    }

    /// A link was followed, to `opened` when the descriptor names the
    /// service. False when the session already holds
    /// [`MAX_SESSION_SERVICES`], and the link is not followed, as the
    /// decoder does not follow it either.
    pub(super) fn followed(&mut self, opened: Option<Opened>) -> bool {
        if self.0.len() >= MAX_SESSION_SERVICES {
            return false;
        }
        self.0.push(opened);
        true
    }

    /// The service numbered `number`, when the session opened one by it.
    pub(super) fn get(&self, number: u8) -> Option<&Opened> {
        self.0.get(usize::from(number)).and_then(Option::as_ref)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]
mod tests {
    use super::*;

    fn opened(address: &str, channel: bool) -> Opened {
        Opened { address: address.to_owned(), channel }
    }

    // Verifies REQ-295 (REQ-NAV-010): the services are numbered in the order
    // opened, an unknown link keeps its number with nothing to reopen, a
    // number never given reopens nothing, and the session stops at its bound.
    #[test]
    fn the_services_are_numbered_as_they_open_and_bounded() {
        let mut services = SessionServices::new(opened("https://first.example/service.ttx", false));
        assert!(services.followed(Some(opened("https://second.example/news/", true))));
        assert!(services.followed(None));
        assert_eq!(services.get(0), Some(&opened("https://first.example/service.ttx", false)));
        assert_eq!(services.get(1), Some(&opened("https://second.example/news/", true)));
        assert_eq!(services.get(2), None, "a link to a name the descriptor did not hold");
        assert_eq!(services.get(3), None, "never given");
        while services.followed(None) {}
        assert_eq!(services.0.len(), MAX_SESSION_SERVICES);
        assert!(!services.followed(Some(opened("https://third.example/", true))));
        assert_eq!(services.get(u8::MAX), None);
    }
}
