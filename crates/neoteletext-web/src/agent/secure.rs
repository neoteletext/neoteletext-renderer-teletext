//! The memory of hosts reached over HTTPS this session (REQ-SEC-007): a
//! later plain address on such a host is a downgrade. It is held in memory
//! only; with cookies gone (the viewer-state design, VS-201) nothing else
//! says a host was reached securely.

use super::UserAgent;
use crate::bytes::TextBuf;
use crate::port::RandomSource;
use crate::url::Url;
use neoteletext_core::policy;

impl<R: RandomSource> UserAgent<'_, R> {
    /// REQ-SEC-007: the host was reached over HTTPS this session, and `url`
    /// is plain. Decided by the policy.
    pub(super) fn is_downgrade(&self, url: &Url) -> bool {
        let host = url.host_text();
        let reached = self.secure_hosts.iter().flatten().any(|known| known.as_str() == host);
        policy::is_downgrade(reached, url.origin().parts(), host)
    }

    /// Remembers that `host` was reached over HTTPS (REQ-SEC-007). The
    /// memory is kept most recently seen first; a host seen again moves to
    /// the front, and a new host when the memory is full takes the place
    /// of the one least recently seen, so the rule keeps holding for the
    /// hosts in use (decision E.114 item 3, finding F-43).
    pub(super) fn remember_secure(&mut self, host: &str) {
        let Some(entry) = TextBuf::from_bytes(host.as_bytes()) else { return };
        let hosts = &mut self.secure_hosts;
        let seen = hosts.iter().position(|slot| slot.is_none_or(|known| known.as_str() == host));
        let last = seen.unwrap_or(hosts.len().saturating_sub(1));
        if let Some(moved) = hosts.get_mut(..=last) {
            moved.rotate_right(1);
        }
        if let Some(first) = hosts.first_mut() {
            *first = Some(entry);
        }
    }
}
