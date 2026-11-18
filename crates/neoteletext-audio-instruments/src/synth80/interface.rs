//! What the server needs of synth80, with no synthesis: which messages it
//! takes, its patch's packer and check, its bank's builder and check, the
//! choice of a patch by bank select and program change, and the tracker
//! that writes its restatement (`neoteletext-common/docs/audio/server.md`,
//! SRV-24, SRV-43 to SRV-45, SRV-55, SRV-60, SRV-68, SRV-69; `synth80.md`,
//! S80-1, S80-33, S80-36, S80-40, S80-41, S80-46).
//!
//! It is built with the crate's `interface` feature alone, which carries
//! no family's synthesis code, as well as with `synth80`; the receiver's
//! instrument uses the same tables.

use crate::midi::common::{Common, Config, PEDAL_DOWN, Selected, VOLUME_FULL};
use crate::midi::{
    Banks, ChannelMessage, DATA_MAX, KIND_MASK, MOST_BANK_BYTES, MOST_BANK_PATCHES, Message, Outcome,
    PITCH_BEND_CENTRE, RPN_PITCH_BEND_SENSITIVITY, controller, status,
};

pub use super::patch::{PATCH_BYTES, Patch, PatchError, Tone, controller_of, place_of};
pub use super::presets::PRESETS;

/// The pitch bend range before any message: 3 semitones (S80-7; DD
/// manual, PDF page 60: "The default setting is 3 semitones").
pub const BEND_RANGE_CENTS: u16 = 300;
/// The largest pitch bend range, a larger being set to it: 99 semitones
/// (S80-7; DD manual, PDF page 60: "The allowable range is 1 to 99
/// semitones").
pub const MOST_BEND_RANGE_CENTS: u16 = 9_900;
/// The banks controller 0 reaches: values 0, 1 and 2 (S80-36; DD manual,
/// PDF page 18: "0 = User Bank 1, 1 = User Bank 2, and 2 = Factory
/// Bank").
pub const BANKS: usize = 3;
/// COARSE PITCH, a performance controller outside the patch (S80-47; DD
/// manual, PDF page 73).
pub const COARSE_PITCH: u8 = 93;
/// FINE PITCH (S80-47; DD manual, PDF page 73).
pub const FINE_PITCH: u8 = 94;
/// How far coarse pitch reaches either way, in cents: the manual's
/// "approximately 5 semitones" taken as exactly 500 (S80-47; DD manual,
/// PDF page 34).
pub const COARSE_CENTS: u16 = 500;
/// How far fine pitch reaches either way: "approximately 1 semitone"
/// (S80-47; DD manual, PDF page 34).
pub const FINE_CENTS: u16 = 100;
/// The tuning controllers' value before any arrives: their centre
/// (S80-47).
pub const TUNING_CENTRE: u8 = super::patch::LEVER_CENTRE;
/// The bytes of a bank of the most patches: 128 × 85 = 10,880 (S80-41
/// (a)).
pub const MOST_BANK_IMAGE_BYTES: usize = MOST_BANK_PATCHES * PATCH_BYTES;
const _: () = assert!(MOST_BANK_IMAGE_BYTES <= MOST_BANK_BYTES);

/// synth80's figures for the common layer: the bend range of S80-7, bank
/// select by controller 0 (S80-36), and non-registered parameter numbers
/// selected, so that a data entry after one goes to no parameter of
/// synth80 and is counted (S80-7).
pub const CONFIG: Config = Config {
    bend_range_cents: BEND_RANGE_CENTS,
    most_bend_range_cents: MOST_BEND_RANGE_CENTS,
    bank_select: true,
    non_registered: true,
};

/// Whether a controller that the common layer hands on moves something of
/// synth80: the modulation wheel (S80-24), coarse and fine pitch (S80-47)
/// or a control of the patch (S80-10, S80-11, S80-18, S80-20).
#[must_use]
pub fn moves(number: u8) -> bool {
    matches!(number, controller::MODULATION | COARSE_PITCH | FINE_PITCH) || place_of(number).is_some()
}

/// Whether synth80 acts on a message, which the server checks every
/// event against (S80-1, SRV-24): note off and on, both pressures,
/// program change and pitch bend; a control change of the controllers
/// S80-7, S80-10, S80-11, S80-18, S80-20, S80-24 to S80-29, S80-36 and
/// S80-47 name; and never a system exclusive message (S80-9) or a data
/// byte above 127.
#[must_use]
pub fn accepts(message: &Message<'_>) -> bool {
    let Message::Channel(message) = message else { return false };
    if message.status & crate::midi::STATUS_BIT == 0 || message.data1 > DATA_MAX || message.data2 > DATA_MAX {
        return false;
    }
    match message.status & KIND_MASK {
        status::NOTE_OFF
        | status::NOTE_ON
        | status::KEY_PRESSURE
        | status::PROGRAM_CHANGE
        | status::CHANNEL_PRESSURE
        | status::PITCH_BEND => true,
        status::CONTROL_CHANGE => takes_controller(message.data1),
        _ => false,
    }
}

/// Whether a control change of `number` is one synth80 takes (S80-1).
fn takes_controller(number: u8) -> bool {
    moves(number)
        || matches!(
            number,
            controller::BANK_SELECT
                | controller::DATA_ENTRY
                | controller::VOLUME
                | controller::DATA_ENTRY_FINE
                | controller::SUSTAIN
                | controller::DATA_INCREMENT
                | controller::DATA_DECREMENT
                | controller::NRPN_FINE
                | controller::NRPN_COARSE
                | controller::RPN_FINE
                | controller::RPN_COARSE
                | controller::ALL_SOUND_OFF
                | controller::RESET_ALL_CONTROLLERS
                | controller::ALL_NOTES_OFF..=controller::POLY_ON
        )
}

/// A patch's 85 bytes (S80-33), as the server writes it from named
/// fields (SRV-55).
#[must_use]
pub fn pack_patch(patch: &Patch) -> [u8; PATCH_BYTES] {
    patch.to_bytes()
}

/// The patch `bytes` hold, checked as the receiver checks it: exactly 85
/// bytes, each 0 to 127 (S80-33).
///
/// # Errors
///
/// [`PatchError::Length`] or [`PatchError::Value`].
pub fn check_patch(bytes: &[u8]) -> Result<Patch, PatchError> {
    Patch::read(bytes)
}

/// Why bytes are not a synth80 bank (S80-41 (a)).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankError {
    /// No patch, more than 128, or a length that is not whole patches:
    /// the length.
    Length(usize),
    /// The patch of this number is faulty.
    Patch(usize, PatchError),
}

/// Writes a bank image of `patches` into `out`: 1 to 128 patches of 85
/// bytes back to back, patch k at byte `k × 85` (S80-41 (a), MIDI-56 form
/// 2), and answers its length.
///
/// # Errors
///
/// [`BankError::Length`] for no patch or more than 128.
pub fn build_bank(patches: &[Patch], out: &mut [u8; MOST_BANK_IMAGE_BYTES]) -> Result<usize, BankError> {
    if patches.is_empty() || patches.len() > MOST_BANK_PATCHES {
        return Err(BankError::Length(patches.len().saturating_mul(PATCH_BYTES)));
    }
    for (place, patch) in out.as_chunks_mut::<PATCH_BYTES>().0.iter_mut().zip(patches) {
        *place = patch.to_bytes();
    }
    Ok(patches.len().saturating_mul(PATCH_BYTES))
}

/// Checks a bank image as the receiver reads it, and answers how many
/// patches it holds: 1 to 128 whole patches, each of which
/// [`check_patch`] takes (S80-41 (a), MIDI-53, MIDI-63).
///
/// # Errors
///
/// [`BankError::Length`] or the first faulty patch.
pub fn check_bank(image: &[u8]) -> Result<usize, BankError> {
    if image.is_empty() || image.len() > MOST_BANK_IMAGE_BYTES || !image.len().is_multiple_of(PATCH_BYTES) {
        return Err(BankError::Length(image.len()));
    }
    for (number, bytes) in image.as_chunks::<PATCH_BYTES>().0.iter().enumerate() {
        check_patch(bytes).map_err(|error| BankError::Patch(number, error))?;
    }
    Ok(image.len() / PATCH_BYTES)
}

/// The patch a program change chooses (S80-35, S80-36): controller 0's
/// value 0, 1 or 2 is the bank index, and the program the patch of that
/// number in it. Where controller 0 is 3 or more, the sound names no
/// such bank, or the bank holds no such patch, the default patch and
/// [`Outcome::PassedOver`]; where the patch is faulty, the default patch
/// and [`Outcome::PatchRefused`] (MIDI-59, MIDI-63).
#[must_use]
pub fn program_patch(banks: &Banks<'_>, bank: u8, program: u8) -> (Patch, Outcome) {
    let index = usize::from(bank);
    if index >= BANKS {
        return (Patch::DEFAULT, Outcome::PassedOver);
    }
    match banks.fixed_patch(index, usize::from(program), PATCH_BYTES) {
        None => (Patch::DEFAULT, Outcome::PassedOver),
        Some(bytes) => match check_patch(bytes) {
            Ok(patch) => (patch, Outcome::Applied),
            Err(_) => (Patch::DEFAULT, Outcome::PatchRefused),
        },
    }
}

/// A control change on `channel`.
const fn control(channel: u8, number: u8, value: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value))
}

/// The 85 control changes a patch stands for, in its byte order, on
/// `channel` (S80-40): what the server writes for an inline patch, at one
/// tick of one block.
pub fn patch_messages(patch: &Patch, channel: u8) -> impl Iterator<Item = Message<'static>> {
    super::patch::CONTROLLERS
        .into_iter()
        .zip(patch.to_bytes())
        .map(move |(number, value)| control(channel, number, value))
}

/// The tone of a preset by its name on the button, as `PRESETS` holds it
/// (S80-42): "STRING 1", "BRASS 3" and so on.
#[must_use]
pub fn preset(name: &str) -> Option<Tone> {
    PRESETS.iter().find(|(button, _)| *button == name).map(|&(_, tone)| tone)
}

/// The channel synth80's restatement is written on: channel 1, which every
/// channel is to synth80 (S80-2, S80-46).
pub const RESTATEMENT_CHANNEL: u8 = 0;

/// How the patch was last set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Base {
    /// Not since the sound began: program 0 of bank index 0 (MIDI-61).
    Start,
    /// By a program change, with controller 0's value then in force.
    Program {
        /// Controller 0's value at the program change.
        bank: u8,
        /// The program.
        program: u8,
    },
    /// Inline, as 85 control changes (S80-40).
    Inline(Patch),
}

/// Controllers 0 to 127.
const CONTROLLERS: usize = 128;

/// synth80's state as the server writes it, and its restatement (S80-46,
/// MIDI-93 to MIDI-99, SRV-68, SRV-69). Every channel is one state, as it
/// is to the instrument (S80-2), and its size is fixed: the patch's
/// controllers once each and the common layer's one channel (MIDI-94).
#[derive(Clone, Debug)]
pub struct Tracker {
    base: Base,
    /// Every control of the patch moved since it was set, by controller.
    moved: [Option<u8>; CONTROLLERS],
    /// The common layer as the instrument keeps it: bank select, the
    /// parameter selected, the bend and its range, the pressure, volume,
    /// the wheel and the pedal.
    common: Common,
    /// Coarse and fine pitch (S80-47).
    tuning: [u8; 2],
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    /// A synth80 as a sound starts it.
    #[must_use]
    pub fn new() -> Self {
        Self { base: Base::Start, moved: [None; CONTROLLERS], common: Common::new(CONFIG), tuning: [TUNING_CENTRE; 2] }
    }

    /// Takes one message the server wrote, in order. An inline patch's 85
    /// control changes are given through [`Tracker::inline_patch`] instead
    /// (taken one by one they leave the same state, restated by
    /// controller rather than as the patch).
    pub fn take(&mut self, message: &Message<'_>) {
        let Message::Channel(message) = message else { return };
        let message = ChannelMessage { status: message.status & KIND_MASK, ..*message };
        if message.kind() == status::CONTROL_CHANGE && message.data1 == controller::BANK_SELECT_FINE {
            return;
        }
        let taken = self.common.take(message);
        match taken.action {
            crate::midi::common::Action::ProgramChange { program, bank_coarse, .. } => {
                self.base = Base::Program { bank: bank_coarse, program };
                self.moved = [None; CONTROLLERS];
            }
            crate::midi::common::Action::Control { controller: number, value } => match number {
                COARSE_PITCH | FINE_PITCH => {
                    if let Some(tuning) = self.tuning.get_mut(usize::from(number == FINE_PITCH)) {
                        *tuning = value;
                    }
                }
                _ if place_of(number).is_some() => {
                    if let Some(place) = self.moved.get_mut(usize::from(number)) {
                        *place = Some(value);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// An inline patch was written (S80-40).
    pub fn inline_patch(&mut self, patch: &Patch) {
        self.base = Base::Inline(*patch);
        self.moved = [None; CONTROLLERS];
    }

    /// Gives the restatement to `out`, in S80-46's order, on channel 1:
    /// (1) the bank select and program change, or the inline patch's 85
    /// control changes; (2) every control of the patch moved since, by
    /// controller number; (3) the bend range where it is not 3
    /// semitones; (4) the pitch bend, channel pressure, controllers 7, 1
    /// and 64 where they are not as reset all controllers leaves them; (5)
    /// controllers 93 and 94 where they are not at 64.
    ///
    /// Beyond S80-46's list, so that the state restated is the state held
    /// (MIDI-93): controller 0 again after the program change where it has
    /// changed since, which only the next program change reads (S80-36);
    /// and the registered or non-registered parameter selection where it
    /// is not none, after the bend range.
    pub fn restate(&self, out: &mut dyn FnMut(Message<'_>)) {
        let channel = RESTATEMENT_CHANNEL;
        let state = *self.common.channel(channel);
        // (1)
        let bank_after = match self.base {
            Base::Start => 0,
            Base::Program { bank, program } => {
                out(control(channel, controller::BANK_SELECT, bank));
                out(Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, channel, program, 0)));
                bank
            }
            Base::Inline(patch) => {
                patch_messages(&patch, channel).for_each(&mut *out);
                0
            }
        };
        if state.bank_coarse != bank_after {
            out(control(channel, controller::BANK_SELECT, state.bank_coarse));
        }
        // (2)
        for (number, value) in (0..=DATA_MAX).zip(&self.moved) {
            if let Some(value) = value {
                out(control(channel, number, *value));
            }
        }
        // (3)
        let mut selected_now = Selected::None;
        if state.bend_range_cents != BEND_RANGE_CENTS {
            let semitones = state.bend_range_cents / crate::midi::common::CENTS_PER_SEMITONE;
            let cents = state.bend_range_cents % crate::midi::common::CENTS_PER_SEMITONE;
            out(control(channel, controller::RPN_COARSE, 0));
            out(control(channel, controller::RPN_FINE, 0));
            out(control(channel, controller::DATA_ENTRY, u8::try_from(semitones).unwrap_or(DATA_MAX)));
            out(control(channel, controller::DATA_ENTRY_FINE, u8::try_from(cents).unwrap_or(0)));
            selected_now = Selected::Registered(RPN_PITCH_BEND_SENSITIVITY);
        }
        if state.selected != selected_now {
            let seven = u16::from(DATA_MAX);
            let byte = |number: u16, shift: u32| u8::try_from((number >> shift) & seven).unwrap_or(DATA_MAX);
            let (coarse, fine, number) = match state.selected {
                Selected::NonRegistered(number) => (controller::NRPN_COARSE, controller::NRPN_FINE, number),
                Selected::Registered(number) => (controller::RPN_COARSE, controller::RPN_FINE, number),
                Selected::None => (controller::RPN_COARSE, controller::RPN_FINE, crate::midi::NULL_PARAMETER),
            };
            out(control(channel, coarse, byte(number, 7)));
            out(control(channel, fine, byte(number, 0)));
        }
        // (4)
        if state.bend != PITCH_BEND_CENTRE {
            let seven = u16::from(DATA_MAX);
            let low = u8::try_from(state.bend & seven).unwrap_or(0);
            let high = u8::try_from((state.bend >> 7) & seven).unwrap_or(0);
            out(Message::Channel(ChannelMessage::new(status::PITCH_BEND, channel, low, high)));
        }
        if state.channel_pressure != 0 {
            out(Message::Channel(ChannelMessage::new(status::CHANNEL_PRESSURE, channel, state.channel_pressure, 0)));
        }
        if state.volume != VOLUME_FULL {
            out(control(channel, controller::VOLUME, state.volume));
        }
        if state.modulation != 0 {
            out(control(channel, controller::MODULATION, state.modulation));
        }
        if state.sustain {
            out(control(channel, controller::SUSTAIN, DATA_MAX));
        }
        // (5)
        let [coarse, fine] = self.tuning;
        for (number, value) in [(COARSE_PITCH, coarse), (FINE_PITCH, fine)] {
            if value != TUNING_CENTRE {
                out(control(channel, number, value));
            }
        }
    }
}

// The pedal is restated down with 127, which is down (MIDI-87).
const _: () = assert!(DATA_MAX >= PEDAL_DOWN);
