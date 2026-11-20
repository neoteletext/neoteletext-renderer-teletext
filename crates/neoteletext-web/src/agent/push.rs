//! The push connection (REQ-WEB-005): connecting over WebSocket or
//! the handshake, the backoff and the heartbeat.

use neoteletext_core::types::Instant;

use super::{Event, Failure, Output, PushPhase, PushProblem, PushStop, PushStream, Sink, UserAgent, event};
use crate::bytes::{Overflow, Writer};
use crate::consts::{
    CRLF, OPCODE_PING, PUSH_PING_AFTER_US, PUSH_PONG_WITHIN_US, PUSH_RETRY_FIRST_US, PUSH_RETRY_MAX_US,
    PUSH_SUBPROTOCOL, ROUND_TRIP_EVERY_US, STATUS_CLIENT_ERROR_FIRST, STATUS_OK, STATUS_SERVER_ERROR_FIRST,
    STATUS_SWITCHING_PROTOCOLS, STATUS_TOO_MANY_REQUESTS,
};
use crate::http::{self, ResponseHead};
use crate::port::{ConnectionId, RandomSource, Security};
use crate::url::IpAddress;
use crate::ws::{self, FrameReader, WsError};

/// Why a handshake's answer means no attempt will ever do better (DOC-1920),
/// or `None` when it may: a client error but for a limit, or
/// an upgrade to something other than the push subprotocol, which is another
/// server or another version (DEC-1759 lesson 14).
fn handshake_stop(head: &ResponseHead) -> Option<PushStop> {
    match head.status {
        STATUS_SWITCHING_PROTOCOLS => {
            let named = head.websocket_protocol.as_ref().map(crate::bytes::TextBuf::as_bytes);
            (named != Some(PUSH_SUBPROTOCOL.as_bytes())).then_some(PushStop::WrongProtocol)
        }
        STATUS_TOO_MANY_REQUESTS => None,
        status => (STATUS_CLIENT_ERROR_FIRST..STATUS_SERVER_ERROR_FIRST)
            .contains(&status)
            .then_some(PushStop::Refused(status)),
    }
}

impl<R: RandomSource> UserAgent<'_, R> {
    pub(super) fn push_connect(&mut self, now: Instant, sink: &mut Sink<'_>) {
        let Some(url) = self.push.url else {
            self.push.phase = PushPhase::Idle;
            return;
        };
        if let Some(failure) = self.refusal(&url) {
            Self::report_refusal(ConnectionId::Push, &url, failure, sink);
            self.push_lost(now, Some(PushProblem::Failure(failure)), false, sink);
            return;
        }
        self.push.phase =
            PushPhase::Connecting { deadline: Instant(now.0.saturating_add(self.config.connect_timeout_us)) };
        self.push.healthy = false;
        self.forget_push_writes();
        // REQ-6984 item 2: HTTP/3 has no plain form, so only a secure
        // address is ever a stream.
        self.push.opening_stream = self.push.over_stream && url.is_secure();
        if self.push.opening_stream {
            sink(Output::OpenPushStream {
                host: url.host_text(),
                port: url.port(),
                authority: url.authority(),
                path: url.target(),
                subprotocol: PUSH_SUBPROTOCOL,
                user_agent: self.named_as.as_bytes(),
            });
            return;
        }
        let security = if url.is_secure() { Security::Tls } else { Security::Plain };
        sink(Output::Connect { connection: ConnectionId::Push, host: url.host_text(), port: url.port(), security });
    }

    /// The port says whether push connections are opened as streams it
    /// presents (REQ-6984 item 1); the next connection is opened so.
    pub(super) const fn push_over_stream(&mut self, on: bool) {
        self.push.over_stream = on;
    }

    /// The port's answer to [`Output::OpenPushStream`] (REQ-6984 items 3
    /// and 4).
    pub(super) fn push_stream(&mut self, now: Instant, answer: PushStream<'_>, sink: &mut Sink<'_>) {
        if !matches!(self.push.phase, PushPhase::Connecting { .. }) || !self.push.opening_stream {
            return;
        }
        let Some(url) = self.push.url else { return };
        self.push.opening_stream = false;
        let (peer, status, subprotocol) = match answer {
            PushStream::Unavailable => {
                // The channel has not failed, a transport has: HTTP/1.1 at
                // once, with no wait and nothing counted against it.
                self.push.over_stream = false;
                self.push.phase = PushPhase::Idle;
                self.push_connect(now, sink);
                return;
            }
            PushStream::Answered { peer, status, subprotocol } => (peer, status, subprotocol),
        };
        if self.peer_blocked(peer) {
            event(sink, Event::Blocked { connection: ConnectionId::Push, host: url.host_text() });
            self.push_lost(now, Some(PushProblem::Failure(Failure::Blocked)), true, sink);
            return;
        }
        self.remember_secure(url.host_text());
        // RFC 8441 section 5: the 200 stands for the 101, and there is no
        // `Sec-WebSocket-Accept`. Any other answer is treated as the same
        // answer to an HTTP/1.1 handshake (DOC-1920).
        let named = subprotocol == Some(PUSH_SUBPROTOCOL.as_bytes());
        let stop = match status {
            STATUS_OK if named => {
                self.push.frames = FrameReader::new();
                self.push_opened(now, sink);
                return;
            }
            STATUS_OK => Some(PushStop::WrongProtocol),
            STATUS_TOO_MANY_REQUESTS => None,
            status => (STATUS_CLIENT_ERROR_FIRST..STATUS_SERVER_ERROR_FIRST)
                .contains(&status)
                .then_some(PushStop::Refused(status)),
        };
        match stop {
            Some(stop) => self.push_stop(stop, sink),
            None => self.push_lost(now, Some(PushProblem::WebSocket(WsError::Handshake)), true, sink),
        }
    }

    pub(super) fn push_connected(&mut self, now: Instant, peer: IpAddress, sink: &mut Sink<'_>) {
        // A stream is answered by `Input::PushStream`, never by a TCP
        // connection's opening.
        if !matches!(self.push.phase, PushPhase::Connecting { .. }) || self.push.opening_stream {
            return;
        }
        let Some(url) = self.push.url else { return };
        if self.peer_blocked(peer) {
            event(sink, Event::Blocked { connection: ConnectionId::Push, host: url.host_text() });
            self.push_lost(now, Some(PushProblem::Failure(Failure::Blocked)), true, sink);
            return;
        }
        if url.is_secure() {
            self.remember_secure(url.host_text());
        } else {
            event(sink, Event::Unencrypted { connection: ConnectionId::Push, host: url.host_text() });
        }
        self.push.head.reset();
        self.push.frames = FrameReader::new();
        let mut writer = Writer::new(self.buffers.push_request);
        self.push.key = ws::new_key(&mut self.random);
        // No `Cookie` header: viewer state is proven on the connection
        // itself, never in a header (VS-201, VS-116).
        let written = (|| -> Result<(), Overflow> {
            let subprotocol = Some(PUSH_SUBPROTOCOL);
            ws::write_handshake_headers(&mut writer, &url, &self.push.key, self.named_as.as_bytes(), subprotocol)?;
            writer.put(CRLF)
        })();
        if written.is_err() {
            self.push_lost(now, Some(PushProblem::Failure(Failure::RequestTooLarge)), true, sink);
            return;
        }
        self.push.phase =
            PushPhase::Handshake { deadline: Instant(now.0.saturating_add(self.config.response_timeout_us)) };
        let len = writer.len();
        let bytes = self.buffers.push_request.get(..len).unwrap_or(&[]);
        sink(Output::Send { connection: ConnectionId::Push, bytes });
    }

    /// The push connection failed or closed: wait, then reconnect, with a
    /// ceiling that doubles and a random delay under it (REQ-WEB-005,
    /// DOC-1920).
    pub(super) fn push_lost(&mut self, now: Instant, problem: Option<PushProblem>, close: bool, sink: &mut Sink<'_>) {
        if self.push.url.is_none() || self.push.phase == PushPhase::Idle && problem.is_none() {
            return;
        }
        if close {
            self.close_push(sink);
        }
        self.forget_push_writes();
        if self.push.healthy {
            self.push.delay_us = PUSH_RETRY_FIRST_US;
        }
        let ceiling = self.push.delay_us;
        self.push.delay_us = ceiling.saturating_mul(2).min(PUSH_RETRY_MAX_US);
        let delay = self.spread(ceiling);
        self.push.phase = PushPhase::Backoff { until: Instant(now.0.saturating_add(delay)) };
        self.push.healthy = false;
        event(sink, Event::PushLost { retry_in_us: delay, problem });
    }

    /// A delay in the upper half of `ceiling`, at random: viewers a restart
    /// sent away together do not all come back at once, and none comes back
    /// at once. At most the ceiling, so `RESUME_HOLD_MS`, which counts the
    /// ceilings of the first five retries, still covers them.
    fn spread(&mut self, ceiling: u64) -> u64 {
        let mut drawn = [0_u8; size_of::<u64>()];
        self.random.fill(&mut drawn);
        let half = ceiling / 2;
        let above = u64::from_le_bytes(drawn).checked_rem(ceiling.saturating_sub(half).saturating_add(1));
        half.saturating_add(above.unwrap_or(0))
    }

    /// The push connection ended for good (DOC-1920): no
    /// reconnection follows, and only opening the channel again starts one.
    pub(super) fn push_stop(&mut self, stop: PushStop, sink: &mut Sink<'_>) {
        self.close_push(sink);
        self.forget_push_writes();
        self.push.phase = PushPhase::Stopped;
        self.push.healthy = false;
        event(sink, Event::PushStopped(stop));
    }

    pub(super) fn push_received(&mut self, now: Instant, mut input: &[u8], sink: &mut Sink<'_>) {
        // A snapshot that has found its end reads nothing more (REQ-6880).
        while !input.is_empty() && !self.snapshot_ending() {
            match self.push.phase {
                PushPhase::Handshake { .. } => {
                    let limit = self.config.header_limit.min(self.buffers.push_head.len());
                    let buffer = self.buffers.push_head.get_mut(..limit).unwrap_or(&mut []);
                    match self.push.head.feed(buffer, input) {
                        Ok((used, complete)) => {
                            input = input.get(used..).unwrap_or(&[]);
                            let head = self.buffers.push_head.get(..self.push.head.len()).unwrap_or(&[]);
                            if complete && http::is_interim(head) {
                                // RFC 9110 section 15.2: the final head follows.
                                self.push.head.reset();
                            } else if complete && !self.push_head_complete(now, sink) {
                                return;
                            }
                        }
                        Err(error) => {
                            self.push_lost(now, Some(PushProblem::Failure(Failure::Http(error))), true, sink);
                            return;
                        }
                    }
                }
                PushPhase::Open { .. } => {
                    // Anything heard is a sign of life (REQ-WEB-005).
                    self.push.phase = PushPhase::Open { heard: now, pinged: None };
                    let used = self.push_frames(now, input, sink);
                    match used {
                        Some(used) if used > 0 => input = input.get(used..).unwrap_or(&[]),
                        _ => return,
                    }
                }
                PushPhase::Idle | PushPhase::Stopped | PushPhase::Backoff { .. } | PushPhase::Connecting { .. } => {
                    return;
                }
            }
        }
    }

    fn push_head_complete(&mut self, now: Instant, sink: &mut Sink<'_>) -> bool {
        let head_len = self.push.head.len();
        let head_bytes = self.buffers.push_head.get(..head_len).unwrap_or(&[]);
        // A `Set-Cookie` on the 101 is ignored with every other field this
        // client does not read (VS-201).
        let head = match http::parse_head(head_bytes) {
            Ok(head) => head,
            Err(error) => {
                self.push_lost(now, Some(PushProblem::Failure(Failure::Http(error))), true, sink);
                return false;
            }
        };
        let accepted = ws::check_handshake(&head, &self.push.key, Some(PUSH_SUBPROTOCOL));
        if let Err(problem) = accepted.map_err(PushProblem::WebSocket) {
            match handshake_stop(&head) {
                Some(stop) => self.push_stop(stop, sink),
                None => self.push_lost(now, Some(problem), true, sink),
            }
            return false;
        }
        self.push_opened(now, sink);
        true
    }

    /// The push WebSocket is open, by an accepted 101 or by an accepted
    /// stream (REQ-6984 item 5): from here both are one.
    fn push_opened(&mut self, now: Instant, sink: &mut Sink<'_>) {
        self.push.phase = PushPhase::Open { heard: now, pinged: None };
        self.push.invalid = 0;
        self.push.log_lines = 0;
        self.records.clear();
        // A sending never continues on another connection (DOC-1952).
        self.links = crate::records::links::Sending::new();
        self.subpage_list = crate::records::subpages::Sending::new();
        event(sink, Event::PushConnected);
        // REQ-6880 item 2: a snapshot sends its request and nothing else.
        match self.config.snapshot {
            Some(target) => self.send_snapshot_request(target, sink),
            None => self.send_subscribe(sink),
        }
    }

    /// The push half of [`Self::tick`]: the backoff, the set-up's time
    /// limits and the heartbeat.
    pub(super) fn push_tick(&mut self, now: Instant, sink: &mut Sink<'_>) {
        if !cfg!(feature = "push") {
            return;
        }
        match self.push.phase {
            PushPhase::Backoff { until } if now >= until => {
                self.push.phase = PushPhase::Idle;
                self.push_connect(now, sink);
            }
            PushPhase::Connecting { deadline } | PushPhase::Handshake { deadline } if now >= deadline => {
                self.push_lost(now, Some(PushProblem::Failure(Failure::Timeout)), true, sink);
            }
            PushPhase::Open { .. } => {
                if self.heartbeat_due().is_some_and(|due| now >= due) {
                    self.heartbeat(now, sink);
                }
                if self.round_trip_due().is_some_and(|due| now >= due) {
                    self.ping_round_trip(now, sink);
                }
            }
            _ => {}
        }
    }

    /// When the next ping that measures the round trip goes (REQ-RCH-016):
    /// while it is measured on an open WebSocket and no ping waits for its
    /// pong, a second after the last, or at once when none has gone.
    pub(super) fn round_trip_due(&self) -> Option<Instant> {
        let measured = self.push.round_trip.on
            && matches!(self.push.phase, PushPhase::Open { .. })
            && self.push.round_trip.waiting.is_none();
        measured.then(|| {
            self.push.round_trip.last.map_or(Instant(0), |last| Instant(last.0.saturating_add(ROUND_TRIP_EVERY_US)))
        })
    }

    /// Sends the ping that measures the round trip: its payload one byte,
    /// counted on, so its pong is told from the heartbeat's, which has none.
    fn ping_round_trip(&mut self, now: Instant, sink: &mut Sink<'_>) {
        let sequence = self.push.round_trip.sequence.wrapping_add(1);
        if self.send_push_frame(OPCODE_PING, &[sequence], sink) {
            self.push.round_trip.sequence = sequence;
            self.push.round_trip.waiting = Some((sequence, now));
            self.push.round_trip.last = Some(now);
        }
    }

    /// A pong: when it answers the round trip's ping, the round trip
    /// (REQ-RCH-016).
    pub(super) fn round_trip_pong(&mut self, now: Instant, payload: &[u8], sink: &mut Sink<'_>) {
        if let Some((sequence, sent)) = self.push.round_trip.waiting
            && payload == [sequence]
        {
            self.push.round_trip.waiting = None;
            event(sink, Event::RoundTrip { microseconds: now.since(sent) });
        }
    }

    /// Turns the round trip's measurement on or off (REQ-RCH-016).
    pub(super) fn measure_round_trip(&mut self, on: bool) {
        self.push.round_trip.on = on;
        if !on {
            self.push.round_trip.waiting = None;
        }
    }

    /// When the heartbeat next acts on the open push connection
    /// (REQ-WEB-005): a ping after a silence, or the end of the wait for its
    /// pong.
    pub(super) fn heartbeat_due(&self) -> Option<Instant> {
        // A snapshot pings nothing: its time limit ends a silent connection
        // (REQ-6880 items 2 and 5).
        if self.config.snapshot.is_some() {
            return None;
        }
        let PushPhase::Open { heard, pinged } = self.push.phase else { return None };
        let after = |at: Instant, wait: u64| Instant(at.0.saturating_add(wait));
        Some(pinged.map_or_else(|| after(heard, PUSH_PING_AFTER_US), |pinged| after(pinged, PUSH_PONG_WITHIN_US)))
    }

    /// A silent connection is pinged, and one that does not answer is
    /// reconnected with the backoff, which sends the subscription again.
    fn heartbeat(&mut self, now: Instant, sink: &mut Sink<'_>) {
        let PushPhase::Open { heard, pinged } = self.push.phase else { return };
        if pinged.is_none() {
            self.send_push_frame(OPCODE_PING, b"", sink);
            self.push.phase = PushPhase::Open { heard, pinged: Some(now) };
            return;
        }
        self.push_lost(now, Some(PushProblem::Failure(Failure::Timeout)), true, sink);
    }
}
