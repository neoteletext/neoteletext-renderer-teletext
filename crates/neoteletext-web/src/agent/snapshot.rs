//! Snapshot mode (REQ-6880, DEC-6875 items 2 and 3; neoteletext-common
//! `docs/tab-thumbnails.md` section 4): one page as a stranger sees it,
//! fetched with nothing that identifies the viewer, then an end.
//!
//! The agent's own machinery does the work, under every rule it always
//! keeps: the descriptor, the address rules, the local network's refusal,
//! the proxy, transport security, the bounds and the time limits. This
//! module narrows it. It lets through only the first open, the network and
//! the clock; it sends the snapshot request itself, once, on the one push
//! connection; it reads WELCOME before anything is given on; it turns what
//! would be a reconnection, a viewer-state record or a page's answer into
//! the snapshot's end; and at the end it closes everything and says
//! [`Event::SnapshotEnded`] once.

use neoteletext_core::packet::Packet;
use neoteletext_core::types::Instant;
use neoteletext_input::control::{Control, FRAGMENT_DATA, from_packet};
use neoteletext_input::snapshot::{welcome_agrees_snapshot, write_request};

use super::{
    Event, Failure, Input, Link, Output, PushPhase, PushProblem, PushStop, Sink, SnapshotEnd, SnapshotTarget, Source,
    UserAgent, Want, event,
};
use crate::consts::{CLOSE_NORMAL, OPCODE_BINARY, SNAPSHOT_TIME_LIMIT_US, STATUS_NOT_FOUND};
use crate::descriptor::Descriptor;
use crate::port::{ConnectionId, RandomSource};

/// The room for the snapshot request: HELLO is its two type bytes, the
/// version's one-byte varint and the capabilities' varint of at most five
/// bytes (8); SELECT is its two type bytes, a one-byte length and the
/// place `PPP/SSSS` (11). 19 bytes, with room to spare.
const REQUEST_ROOM: usize = 32;

/// Where a snapshot stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Not begun: the first open begins it.
    Idle,
    /// Begun, to end by `deadline` at the latest.
    Running { deadline: Instant },
    /// Ended and reported: nothing more is done.
    Ended,
}

/// A snapshot's progress (REQ-6880).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Run {
    phase: Phase,
    /// WELCOME agreeing SNAPSHOT has arrived on the push connection.
    welcomed: bool,
    /// A page header arrived after it.
    page_seen: bool,
    /// The close code the server's close frame gave, if it sent one.
    closed_with: Option<u16>,
    /// The sub-page's own files were missing, so the page's file is asked
    /// for instead.
    page_file: bool,
    /// The end found while an input was handled, applied when it is done.
    end: Option<SnapshotEnd>,
}

impl Run {
    pub(super) const fn new() -> Self {
        Self { phase: Phase::Idle, welcomed: false, page_seen: false, closed_with: None, page_file: false, end: None }
    }

    /// Records the snapshot's end, the first found winning.
    const fn note(&mut self, end: SnapshotEnd) {
        if self.end.is_none() {
            self.end = Some(end);
        }
    }

    /// A record read on a snapshot connection (REQ-6880 item 2): DENIED ends
    /// it, and any viewer-state record is the server breaking the protocol,
    /// never reported or answered. Returns false when the record is not to
    /// be given on.
    pub(super) const fn record(&mut self, record: &crate::records::ChannelRecord<'_>) -> bool {
        use crate::records::ChannelRecord;
        match record {
            ChannelRecord::Denied(denied) => {
                self.note(SnapshotEnd::Denied(denied.status));
                true
            }
            ChannelRecord::StateChallenge(_) | ChannelRecord::StateEnd(_) | ChannelRecord::StateKeepOffer => {
                self.note(SnapshotEnd::Failed(Failure::Content));
                false
            }
            _ => true,
        }
    }
}

/// What the watch on the agent's outputs saw while an input was handled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    /// Nothing that ends the snapshot.
    Nothing,
    /// The push connection was lost, with this problem if one was known:
    /// the end depends on what had arrived and how it closed.
    Lost(Option<PushProblem>),
    /// This end.
    End(SnapshotEnd),
}

/// The failure a push connection's problem means for a snapshot.
const fn failure_of(problem: Option<PushProblem>) -> Failure {
    match problem {
        Some(PushProblem::Failure(failure)) => failure,
        Some(PushProblem::WebSocket(_) | PushProblem::Base64) | None => Failure::Network,
    }
}

/// The failure a push connection's stop means for a snapshot: a refused
/// upgrade keeps its status; a stop code or another protocol is the
/// connection failing.
const fn stop_failure(stop: PushStop) -> Failure {
    match stop {
        PushStop::Refused(status) => Failure::Status(status),
        PushStop::Closed(_) | PushStop::WrongProtocol => Failure::Network,
    }
}

/// What an output of the agent means for a snapshot, and whether it goes
/// on to the caller. A reconnection's notice and a stop are the agent's
/// business in snapshot mode, so they are not given on: the end says it.
fn watch(output: &Output<'_>) -> (Seen, bool) {
    let Output::Event(happened) = output else {
        return match output {
            // A static channel's file is the answer (REQ-6880 item 3).
            Output::PageBytes(page) if matches!(page.source, Source::Page { .. }) => {
                (Seen::End(SnapshotEnd::Answered), true)
            }
            _ => (Seen::Nothing, true),
        };
    };
    match *happened {
        Event::PushLost { problem, .. } => (Seen::Lost(problem), false),
        Event::PushStopped(stop) => (Seen::End(SnapshotEnd::Failed(stop_failure(stop))), false),
        Event::PageNotInService { .. } => (Seen::End(SnapshotEnd::Failed(Failure::Status(STATUS_NOT_FOUND))), true),
        Event::FetchFailed { failure, .. } | Event::DescriptorFailed(failure) => {
            (Seen::End(SnapshotEnd::Failed(failure)), true)
        }
        // A fresh agent holds no copy, so a page that is current says the
        // server answered what was never asked.
        Event::DescriptorRejected(_) | Event::PageCurrent { .. } => {
            (Seen::End(SnapshotEnd::Failed(Failure::Content)), true)
        }
        Event::RedirectToOtherChannel { .. } => (Seen::End(SnapshotEnd::Failed(Failure::RedirectToOtherChannel)), true),
        Event::PushNotCompiledIn => (Seen::End(SnapshotEnd::Failed(Failure::Network)), true),
        _ => (Seen::Nothing, true),
    }
}

impl<R: RandomSource> UserAgent<'_, R> {
    /// [`UserAgent::handle`] in snapshot mode (REQ-6880): the first open
    /// begins the snapshot and its time limit; the network and the clock go
    /// on to the agent; every input message is refused with
    /// [`Event::InputDropped`] (item 4); everything else is ignored.
    pub(super) fn handle_snapshot(&mut self, now: Instant, input: Input<'_>, sink: &mut Sink<'_>) {
        match (self.snapshot.phase, input) {
            (Phase::Ended, _) => return,
            (_, Input::Send(_)) => {
                event(sink, Event::InputDropped);
                return;
            }
            (Phase::Idle, Input::OpenChannel(_) | Input::OpenService(_)) => {
                self.snapshot.phase =
                    Phase::Running { deadline: Instant(now.0.saturating_add(SNAPSHOT_TIME_LIMIT_US)) };
            }
            (Phase::Running { deadline }, Input::Tick | Input::Net(_)) => {
                // REQ-6880 item 5: everything ends by the time limit.
                if now >= deadline {
                    self.finish_snapshot(SnapshotEnd::Failed(Failure::Timeout), sink);
                    return;
                }
            }
            _ => return,
        }
        let mut seen = Seen::Nothing;
        {
            let mut watched = |output: Output<'_>| {
                let (now_seen, pass) = watch(&output);
                if seen == Seen::Nothing {
                    seen = now_seen;
                }
                if pass {
                    sink(output);
                }
            };
            self.handle_input(now, input, &mut watched);
        }
        let end = self.snapshot.end.take().or(match seen {
            Seen::Nothing => None,
            Seen::End(end) => Some(end),
            // REQ-6880 item 2: answered when the server closes normally after
            // sending a page; any other loss is a failure, never a retry.
            Seen::Lost(problem) => Some(
                if problem.is_none() && self.snapshot.page_seen && self.snapshot.closed_with == Some(CLOSE_NORMAL) {
                    SnapshotEnd::Answered
                } else {
                    SnapshotEnd::Failed(failure_of(problem))
                },
            ),
        });
        if let Some(end) = end {
            self.finish_snapshot(end, sink);
        }
    }

    /// The snapshot's deadline while it runs, for the port's timer.
    pub(super) const fn snapshot_deadline(&self) -> Option<Instant> {
        match self.snapshot.phase {
            Phase::Running { deadline } => Some(deadline),
            Phase::Idle | Phase::Ended => None,
        }
    }

    /// Whether this agent is in snapshot mode and its snapshot has an end
    /// waiting to be applied: what arrives after it is not given on.
    pub(super) const fn snapshot_ending(&self) -> bool {
        self.snapshot.end.is_some()
    }

    /// Records the snapshot's end, the first found winning.
    const fn end_snapshot(&mut self, end: SnapshotEnd) {
        self.snapshot.note(end);
    }

    /// Ends the snapshot: every connection closed (the push connection with
    /// a normal close while it is open), nothing left to fetch, and the end
    /// said once.
    fn finish_snapshot(&mut self, end: SnapshotEnd, sink: &mut Sink<'_>) {
        self.snapshot.phase = Phase::Ended;
        self.snapshot.end = None;
        match self.push.phase {
            PushPhase::Open { .. } => {
                self.send_push_close(CLOSE_NORMAL, sink);
                self.close_push(sink);
            }
            PushPhase::Connecting { .. } | PushPhase::Handshake { .. } => self.close_push(sink),
            PushPhase::Idle | PushPhase::Stopped | PushPhase::Backoff { .. } => {}
        }
        self.forget_push_writes();
        self.push.phase = PushPhase::Stopped;
        self.push.url = None;
        if self.fetch.link != Link::Closed {
            sink(Output::Close { connection: ConnectionId::Fetch });
            self.fetch.link = Link::Closed;
        }
        self.fetch.exchange = None;
        self.fetch.deadline = None;
        self.fetch.response = None;
        self.fetch.body = None;
        self.queue = [None; crate::consts::PAGE_QUEUE];
        self.second_try = None;
        self.pending_descriptor = None;
        self.pending_resource = None;
        self.front_door = None;
        self.contents = None;
        event(sink, Event::SnapshotEnded(end));
    }

    /// The push connection is upgraded: the snapshot request goes, alone,
    /// as one binary message (REQ-6880 item 2, REQ-6879 item 3), and only to
    /// a push address the descriptor lets take messages, as every input
    /// message (DOC-1865).
    pub(super) fn send_snapshot_request(&mut self, target: SnapshotTarget, sink: &mut Sink<'_>) {
        if !self.descriptor.as_ref().is_some_and(Descriptor::push_takes_messages) {
            self.end_snapshot(SnapshotEnd::Failed(Failure::Blocked));
            return;
        }
        let mut request = [0_u8; REQUEST_ROOM];
        let written = write_request(target.page.mpp(), target.sub.value(), &mut request);
        let sent = written.is_ok_and(|length| {
            let bytes = request.get(..length).unwrap_or(&[]);
            self.send_push_frame(OPCODE_BINARY, bytes, sink)
        });
        if !sent {
            self.end_snapshot(SnapshotEnd::Failed(Failure::RequestTooLarge));
        }
    }

    /// One pushed packet in snapshot mode, before it is read or given on:
    /// the first must be WELCOME agreeing SNAPSHOT (REQ-6879 item 4).
    /// Returns false when the snapshot ends here, and nothing of the
    /// message is given on: a WELCOME that does not agree it closes at
    /// once with a normal close; any other packet before WELCOME is the
    /// server breaking the protocol.
    pub(super) fn snapshot_packet(&mut self, packet: &Packet, header: bool) -> bool {
        if self.snapshot.welcomed {
            if header {
                self.snapshot.page_seen = true;
            }
            return true;
        }
        let mut buffer = [0_u8; FRAGMENT_DATA];
        match from_packet(packet, &mut buffer) {
            Some(Ok(Control::Welcome { caps, .. })) if welcome_agrees_snapshot(caps) => {
                self.snapshot.welcomed = true;
                true
            }
            // The close, a normal one, goes as the snapshot ends.
            Some(Ok(Control::Welcome { .. })) => {
                self.end_snapshot(SnapshotEnd::NotOffered);
                false
            }
            _ => {
                self.end_snapshot(SnapshotEnd::Failed(Failure::Content));
                false
            }
        }
    }

    /// The server's close frame on a snapshot connection, with its code.
    pub(super) const fn snapshot_closed(&mut self, code: Option<u16>) {
        self.snapshot.closed_with = code;
    }

    /// A static channel or a service is open in snapshot mode: its one file
    /// is asked for (REQ-6880 item 3): a sub-page's own file when a sub-code
    /// is named, as if the page's list had answered, and the page's file
    /// otherwise.
    pub(super) fn enqueue_snapshot(&mut self, target: SnapshotTarget, sink: &mut Sink<'_>) {
        let subcode = (!target.sub.is_any()).then_some(target.sub);
        if subcode.is_some() && self.channel.is_some() {
            self.set_catalogued(target.page, true);
        }
        self.enqueue(target.page, subcode, Want::Page, sink);
    }

    /// The sub-page's own files were both missing: the page's file is asked
    /// for instead, once (REQ-6880 item 3, the channel giving no sub-page
    /// files). Returns false when that was already done, so the page is
    /// missing.
    pub(super) fn snapshot_page_file(&mut self, sink: &mut Sink<'_>) -> bool {
        let Some(target) = self.config.snapshot else { return false };
        if self.snapshot.page_file {
            return false;
        }
        self.snapshot.page_file = true;
        self.set_catalogued(target.page, false);
        self.enqueue(target.page, Some(target.sub), Want::Page, sink);
        true
    }
}
