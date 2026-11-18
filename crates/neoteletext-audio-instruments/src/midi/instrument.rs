//! The one trait the receiver plays every family through.

use super::{MAX_CUES_PER_BLOCK, MOST_BANKS, Message};

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

/// Where an amplitude envelope stands, as a page's rules read it
/// (`neoteletext-common/docs/page-rules.md`, RULE-28 class 5, RULE-31).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MeterStage {
    /// No voice sounds.
    #[default]
    Idle,
    /// Rising from the key.
    Attack,
    /// Falling to the level it holds.
    Decay,
    /// Holding while the key is down.
    Sustain,
    /// Dying away after the key.
    Release,
}

impl MeterStage {
    /// The stage's number in a rule's value: 0 idle, 1 attack, 2 decay,
    /// 3 sustain, 4 release (RULE-28).
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Attack => 1,
            Self::Decay => 2,
            Self::Sustain => 3,
            Self::Release => 4,
        }
    }
}

/// The largest level a [`Meter`] gives: the envelope at its full scale
/// (RULE-31: "scaled linearly to 0 to 255").
pub const METER_FULL: u8 = u8::MAX;

/// An envelope's value of 0 to `full` scaled linearly to 0 to
/// [`METER_FULL`] (RULE-31); a value outside is held to the nearer end.
#[must_use]
pub fn meter_level(value: i32, full: i32) -> u8 {
    let scaled = i64::from(value.clamp(0, full.max(0)))
        .saturating_mul(i64::from(METER_FULL))
        .checked_div(i64::from(full))
        .unwrap_or(0);
    u8::try_from(scaled).unwrap_or(METER_FULL)
}

/// The envelope position of a part and the instrument's voice count, as a
/// page's rules read them (RULE-28 classes 5 to 7, RULE-31).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Meter {
    /// The stage of the amplitude envelope of the part's most recently
    /// started voice that is not idle; [`MeterStage::Idle`] where none is.
    pub stage: MeterStage,
    /// That envelope's present value, scaled linearly to 0 to
    /// [`METER_FULL`] of its full scale; 0 where the stage is idle.
    pub level: u8,
    /// How many of the instrument's voices are not idle, on every part.
    pub voices: u8,
}

impl Meter {
    /// No voice sounds.
    pub const IDLE: Self = Self { stage: MeterStage::Idle, level: 0, voices: 0 };

    /// The position of a voice whose envelope stands at `position` and
    /// whose key is down or not. An instrument moves its envelopes at its
    /// next sample, so between a key and that sample the envelope has not
    /// heard of it: a voice given a key whose envelope is still at rest is
    /// at the foot of its attack, and one let go whose envelope has not
    /// yet turned is at the head of its release.
    #[must_use]
    pub fn keyed(position: (MeterStage, u8), key_down: bool) -> (MeterStage, u8) {
        match position {
            (MeterStage::Idle, _) if key_down => (MeterStage::Attack, 0),
            (MeterStage::Attack | MeterStage::Decay | MeterStage::Sustain, level) if !key_down => {
                (MeterStage::Release, level)
            }
            position => position,
        }
    }

    /// The meter of an instrument `voices` of whose voices are not idle,
    /// the envelope asked for standing at `position`, or at none.
    #[must_use]
    pub fn of(position: Option<(MeterStage, u8)>, voices: usize) -> Self {
        let voices = u8::try_from(voices).unwrap_or(u8::MAX);
        match position {
            Some((stage, level)) if stage != MeterStage::Idle => Self { stage, level, voices },
            _ => Self { stage: MeterStage::Idle, level: 0, voices },
        }
    }
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

    /// The patch at `program` of bank index `bank` whose image is fixed
    /// patches of `size` bytes back to back (MIDI-56 form 2: patch k at
    /// byte `k × size`), or `None` where the image holds no whole patch
    /// there.
    #[must_use]
    pub fn fixed_patch(&self, bank: usize, program: usize, size: usize) -> Option<&'a [u8]> {
        let start = program.checked_mul(size)?;
        self.get(bank)?.get(start..start.checked_add(size)?)
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

    /// The envelope position of part `part`, its MIDI channel less one, 0
    /// to 15, and the count of the instrument's voices that are not idle
    /// (RULE-31): of the part's most recently started voice that is not
    /// idle, the stage and the present value of its amplitude envelope.
    /// An instrument that is one part, whatever the channel (synth106,
    /// synth60, synthD, the synth53 family and synth80), answers the same
    /// for every part; synth8 answers its upper patch's voices for part 0
    /// and its lower patch's for part 1; the synth6581 family takes the
    /// voice, 0 to 2, as the part. A part the instrument does not have is
    /// idle. It reads and changes nothing, and the provided answer is idle
    /// with no voice.
    fn meter(&self, part: u8) -> Meter {
        let _ = part;
        Meter::IDLE
    }
}

/// A cue the reader passed while a block was played (page-rules.md
/// RULE-109; REQ-6614 item 3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlayedCue {
    /// The cue's number, 0 to [`super::DATA_MAX`].
    pub number: u8,
    /// The sample of the block, 0 to 955, at which the cue stands: where
    /// an event of its tick takes effect ([`super::sample_of_tick`]).
    pub sample: u16,
}

/// The cues of one block, in the order they stand: at most
/// [`MAX_CUES_PER_BLOCK`], held in place.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlayedCues {
    cues: [PlayedCue; MAX_CUES_PER_BLOCK],
    count: usize,
}

impl PlayedCues {
    /// The cues, in the order they stand in the block.
    #[must_use]
    pub fn as_slice(&self) -> &[PlayedCue] {
        self.cues.get(..self.count).unwrap_or(&[])
    }

    /// Adds a cue; past [`MAX_CUES_PER_BLOCK`], which the reader never
    /// gives, it does nothing.
    fn push(&mut self, cue: PlayedCue) {
        if let Some(place) = self.cues.get_mut(self.count) {
            *place = cue;
            self.count = self.count.saturating_add(1);
        }
    }
}

/// What playing one block found, for the receiver's counters (MIDI-123).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Played {
    /// The cues the block holds, each with its sample (RULE-109): the
    /// caller, which renders ahead of what is heard, marks each at its
    /// place in the stream. A cue changes nothing in the instrument.
    pub cues: PlayedCues,
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
            super::block::Read::Cue(cue) => {
                let sample = u16::try_from(super::sample_of_tick(cue.time)).unwrap_or(u16::MAX);
                played.cues.push(PlayedCue { number: cue.number, sample });
            }
            super::block::Read::PassedOver => played.passed_over = played.passed_over.saturating_add(1),
            super::block::Read::Malformed => played.malformed = true,
        }
    }
    instrument.render(rest);
    played
}
