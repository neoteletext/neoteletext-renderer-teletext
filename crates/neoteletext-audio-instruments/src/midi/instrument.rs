//! The one trait the receiver plays every family through.

use super::{MOST_BANKS, Message};

/// What an instrument did with one message, which the receiver counts
/// (MIDI-123).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Acted on, or taken whole by the common layer.
    Applied,
    /// Passed over and counted: a message the family document does not
    /// list, on a channel that addresses no part, or whose bytes break the
    /// family's rules (MIDI-10, MIDI-34, MIDI-67). Nothing changed.
    PassedOver,
    /// A patch, in a bank or sent inline, broke the family's rules: the
    /// family's default patch was set in its place, and the receiver counts
    /// it among patches refused (MIDI-63).
    PatchRefused,
}

/// The bank images of a sound, in the order its OBJECT names them: index 0
/// first (MIDI-57). Each is complete (MIDI-52) and at most
/// [`super::MOST_BANK_BYTES`] bytes (MIDI-55); its form is the family
/// document's (MIDI-56). A family reads its patches from here on every
/// program change, so its work is bounded by the image's bytes (MIDI-66),
/// and keeps no reference to them between calls.
#[derive(Clone, Copy, Debug, Default)]
pub struct Banks<'a> {
    images: [&'a [u8]; MOST_BANKS],
    count: usize,
}

impl<'a> Banks<'a> {
    /// No bank: the synth6581 family's, or a sound whose banks have not
    /// arrived.
    #[must_use]
    pub const fn none() -> Self {
        Self { images: [&[]; MOST_BANKS], count: 0 }
    }

    /// The banks `images` hold, the first [`MOST_BANKS`] of them.
    #[must_use]
    pub fn new(images: &[&'a [u8]]) -> Self {
        let mut banks = Self::none();
        for image in images.iter().take(MOST_BANKS) {
            banks.push(image);
        }
        banks
    }

    /// Adds the next bank index; past [`MOST_BANKS`] it does nothing.
    pub fn push(&mut self, image: &'a [u8]) {
        if let Some(place) = self.images.get_mut(self.count) {
            *place = image;
            self.count = self.count.saturating_add(1);
        }
    }

    /// The image of bank index `index`, or `None` past the sound's banks.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&'a [u8]> {
        if index < self.count { self.images.get(index).copied() } else { None }
    }

    /// How many banks the sound names.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether the sound names none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// A synthesizer as the receiver plays it: one family's instrument for one
/// sound (MIDI-3), made new when the sound starts, given each event's
/// message at its sample (MIDI-5) and rendered between.
///
/// The receiver drives it so, for every block in order: it calls
/// [`Instrument::start`] once, before the first block; then, for each
/// event of the block, renders the samples up to the event's sample
/// ([`super::sample_of_tick`]) and calls [`Instrument::midi`]; then renders
/// the block's remaining samples. For a live block played through as lost
/// it calls [`Instrument::lost`] and renders the block's 960 samples
/// (MIDI-102). Each call to [`Instrument::render`] may be of any length,
/// including 0, and the same messages give the same samples however the
/// calls are cut.
///
/// A family builds its implementation on [`super::common::Common`], which
/// settles the behaviours common to every family (MIDI-84 to MIDI-92), and
/// takes what remains as its family document says (MIDI-10).
pub trait Instrument {
    /// Readies every part at the sound's start: program 0 of bank index 0,
    /// or where that program is missing or faulty the family's default
    /// patch (MIDI-61). Answers [`Outcome::PatchRefused`] when a faulty
    /// patch was replaced.
    fn start(&mut self, banks: &Banks<'_>) -> Outcome;

    /// Takes one event's message, with the sound's banks for a program
    /// change. Never panics, whatever the message holds; a message the
    /// family does not take changes nothing (MIDI-34).
    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome;

    /// Input was lost: a live block was played through as missing
    /// (MIDI-102). A family that plays notes releases every note, lifts the
    /// sustain pedal on every part and clears every channel's parameter
    /// selection ([`super::common::Common::lost`]); the synth6581 family
    /// does nothing (S6581-48).
    fn lost(&mut self);

    /// Renders the next `out.len()` samples, 48,000 a second, mono.
    fn render(&mut self, out: &mut [i16]);
}

/// What playing one block found, for the receiver's counters (MIDI-123).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Played {
    /// Events given to the instrument.
    pub events: u32,
    /// Events passed over: by the reader (MIDI-29, MIDI-31, MIDI-34) or by
    /// the instrument ([`Outcome::PassedOver`]).
    pub passed_over: u32,
    /// Patches the instrument replaced by its default (MIDI-63).
    pub patches_refused: u32,
    /// The block had a framing error, and the rest of it was passed over
    /// (MIDI-33).
    pub malformed: bool,
}

/// Plays one block's event list into `instrument` and renders `out`, the
/// block's samples: each event's message given at its sample (MIDI-5), in
/// the order the events stand (MIDI-35); after a framing error, what was
/// applied stays and the rest of the samples are rendered from it
/// (MIDI-33). Its work is bounded by the block's bytes and `out`'s length.
pub fn play_block<I: Instrument + ?Sized>(
    instrument: &mut I,
    block: &[u8],
    banks: &Banks<'_>,
    out: &mut [i16],
) -> Played {
    let mut played = Played::default();
    let mut rest = out;
    let mut rendered: usize = 0;
    for read in super::block::Events::new(block) {
        match read {
            super::block::Read::Event(event) => {
                let at = super::sample_of_tick(event.time);
                let (now, later) = rest.split_at_mut(at.saturating_sub(rendered).min(rest.len()));
                instrument.render(now);
                rendered = rendered.saturating_add(now.len());
                rest = later;
                played.events = played.events.saturating_add(1);
                match instrument.midi(&event.message, banks) {
                    Outcome::Applied => {}
                    Outcome::PassedOver => played.passed_over = played.passed_over.saturating_add(1),
                    Outcome::PatchRefused => played.patches_refused = played.patches_refused.saturating_add(1),
                }
            }
            super::block::Read::PassedOver => played.passed_over = played.passed_over.saturating_add(1),
            super::block::Read::Malformed => played.malformed = true,
        }
    }
    instrument.render(rest);
    played
}
