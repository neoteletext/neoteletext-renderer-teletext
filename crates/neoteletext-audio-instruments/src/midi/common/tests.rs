#![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::arithmetic_side_effects, reason = "tests")]

use super::super::{ChannelMessage, PITCH_BEND_CENTRE, status};
use super::{Action, Common, Config, DataEntry, Selected, bend_for_fixed_range, volume_gain_q28};

const CONFIG: Config =
    Config { bend_range_cents: 200, most_bend_range_cents: 1_200, bank_select: true, non_registered: true };

fn cc(common: &mut Common, channel: u8, controller: u8, value: u8) -> Action {
    common.take(ChannelMessage::new(status::CONTROL_CHANGE, channel, controller, value)).action
}

/// MIDI-88: a note on of velocity 0 is a note off of velocity 64.
#[test]
fn a_note_on_of_velocity_0_is_a_note_off() {
    let mut common = Common::new(CONFIG);
    let taken = common.take(ChannelMessage::new(status::NOTE_ON, 3, 60, 0));
    assert_eq!(taken.channel, 3);
    assert_eq!(taken.action, Action::NoteOff { key: 60, velocity: 64 });
}

/// MIDI-87: down at 64 to 127, up at 0 to 63.
#[test]
fn the_sustain_pedal_is_down_from_64() {
    let mut common = Common::new(CONFIG);
    for (value, down) in [(0, false), (1, false), (63, false), (64, true), (127, true)] {
        assert_eq!(cc(&mut common, 0, 64, value), Action::Sustain { down });
        assert_eq!(common.channel(0).sustain, down);
    }
}

/// MIDI-85, MIDI-89: 123 to 127 are all notes off, whatever the value.
#[test]
fn mode_messages_are_all_notes_off() {
    let mut common = Common::new(CONFIG);
    for controller in 123..=127 {
        for value in [0, 16, 127] {
            assert_eq!(cc(&mut common, 0, controller, value), Action::AllNotesOff);
        }
    }
    assert_eq!(cc(&mut common, 0, 120, 5), Action::AllSoundOff);
    assert_eq!(cc(&mut common, 0, 122, 0), Action::PassedOver);
}

/// MIDI-84: reset all controllers as RP-015 says: what it sets, and the
/// volume, bank select and bend range it leaves.
#[test]
fn reset_all_controllers_follows_rp_015() {
    let mut common = Common::new(CONFIG);
    for (controller, value) in [(1, 90), (7, 30), (11, 20), (64, 127), (65, 127), (66, 127), (67, 127), (0, 2), (32, 3)]
    {
        cc(&mut common, 5, controller, value);
    }
    // RPN 0 to 7 semitones, then NRPN 18 selected.
    cc(&mut common, 5, 101, 0);
    cc(&mut common, 5, 100, 0);
    cc(&mut common, 5, 6, 7);
    cc(&mut common, 5, 99, 0);
    cc(&mut common, 5, 98, 18);
    common.take(ChannelMessage::new(status::PITCH_BEND, 5, 0, 100));
    common.take(ChannelMessage::new(status::CHANNEL_PRESSURE, 5, 50, 0));
    for value in [0, 127] {
        assert_eq!(cc(&mut common, 5, 121, value), Action::ResetAllControllers);
        let state = common.channel(5);
        assert_eq!((state.modulation, state.expression, state.channel_pressure), (0, 127, 0));
        assert!(!state.sustain && !state.portamento && !state.sostenuto && !state.soft);
        assert_eq!(state.bend, PITCH_BEND_CENTRE);
        assert_eq!(state.selected, Selected::None);
        assert_eq!((state.volume, state.bank_coarse, state.bank_fine, state.bend_range_cents), (30, 2, 3, 700));
    }
    // No parameter is selected, so a data entry changes nothing.
    assert_eq!(cc(&mut common, 5, 6, 1), Action::None);
}

/// MIDI-90: registered parameter 0 sets the range in semitones and cents,
/// steps by one cent, and is held to the family's most.
#[test]
fn registered_parameter_0_sets_the_bend_range() {
    let mut common = Common::new(CONFIG);
    cc(&mut common, 0, 101, 0);
    cc(&mut common, 0, 100, 0);
    assert_eq!(cc(&mut common, 0, 6, 1), Action::BendRange { cents: 100 });
    assert_eq!(cc(&mut common, 0, 38, 50), Action::BendRange { cents: 150 });
    assert_eq!(cc(&mut common, 0, 96, 9), Action::BendRange { cents: 151 });
    assert_eq!(cc(&mut common, 0, 97, 9), Action::BendRange { cents: 150 });
    assert_eq!(cc(&mut common, 0, 6, 24), Action::BendRange { cents: 1_200 });
    assert_eq!(cc(&mut common, 0, 6, 0), Action::BendRange { cents: 0 });
    assert_eq!(cc(&mut common, 0, 97, 0), Action::BendRange { cents: 0 });
    // A full bend up at 7 semitones.
    cc(&mut common, 0, 6, 7);
    common.take(ChannelMessage::new(status::PITCH_BEND, 0, 127, 127));
    assert_eq!(common.bend_cents(0), 8_191 * 700 / 8_192);
    // A program change and the other channels leave it.
    common.take(ChannelMessage::new(status::PROGRAM_CHANGE, 0, 3, 0));
    assert_eq!(common.channel(0).bend_range_cents, 700);
    assert_eq!(common.channel(1).bend_range_cents, 200);
}

/// MIDI-76: NRPN selection, data entry to it, the null number, and a lost
/// block clearing the selection.
#[test]
fn non_registered_parameters_are_selected_and_cleared() {
    let mut common = Common::new(CONFIG);
    cc(&mut common, 2, 99, 0);
    cc(&mut common, 2, 98, 18);
    assert_eq!(cc(&mut common, 2, 6, 40), Action::NonRegistered { parameter: 18, entry: DataEntry::Coarse(40) });
    assert_eq!(cc(&mut common, 2, 38, 1), Action::NonRegistered { parameter: 18, entry: DataEntry::Fine(1) });
    cc(&mut common, 2, 101, 127);
    cc(&mut common, 2, 100, 127);
    assert_eq!(cc(&mut common, 2, 6, 40), Action::None);
    cc(&mut common, 2, 99, 1);
    cc(&mut common, 2, 98, 2);
    assert_eq!(cc(&mut common, 2, 96, 0), Action::NonRegistered { parameter: 130, entry: DataEntry::Increment(0) });
    common.lost();
    assert_eq!(cc(&mut common, 2, 6, 40), Action::None);
}

/// MIDI-64, MIDI-74: bank select and NRPN controllers are ordinary
/// controllers in a family that does not use them.
#[test]
fn a_family_without_bank_select_or_nrpn_gets_the_controllers() {
    let mut common = Common::new(Config { bank_select: false, non_registered: false, ..CONFIG });
    assert_eq!(cc(&mut common, 0, 0, 1), Action::Control { controller: 0, value: 1 });
    assert_eq!(cc(&mut common, 0, 99, 1), Action::Control { controller: 99, value: 1 });
    let mut common = Common::new(CONFIG);
    assert_eq!(cc(&mut common, 0, 0, 1), Action::None);
    assert_eq!(cc(&mut common, 0, 32, 4), Action::None);
    assert_eq!(
        common.take(ChannelMessage::new(status::PROGRAM_CHANGE, 0, 9, 0)).action,
        Action::ProgramChange { program: 9, bank_coarse: 1, bank_fine: 4 }
    );
}

/// MIDI-82: the volume law.
#[test]
fn the_volume_law_is_the_square() {
    assert_eq!(volume_gain_q28(0), 0);
    assert_eq!(volume_gain_q28(127), 1 << 28);
    assert_eq!(i64::from(volume_gain_q28(64)), 4_096 * (1_i64 << 28) / 16_129);
}

/// MIDI-90: a family of a fixed range is given the bend that sounds the
/// channel's range.
#[test]
fn a_bend_for_a_fixed_range() {
    assert_eq!(bend_for_fixed_range(0, 200), PITCH_BEND_CENTRE);
    assert_eq!(bend_for_fixed_range(100, 200), PITCH_BEND_CENTRE + 4_096);
    assert_eq!(bend_for_fixed_range(-200, 200), 0);
    assert_eq!(bend_for_fixed_range(5_000, 200), 0x3FFF);
}
