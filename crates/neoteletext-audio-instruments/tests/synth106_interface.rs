//! The programming interface of synth106 and synth60, without the sound
//! (`neoteletext-common/docs/audio/synth106-synth60.md`, "S106-n"): the
//! three system exclusive messages with the Juno-106 manual's own examples
//! as vectors, the JU-06A's controllers and bands, the bend range, the
//! patch's forms, the banks and the restatement.

// Without the feature there is nothing here to test.
#![cfg(feature = "interface")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test"
)]

use neoteletext_audio_instruments::midi::{Banks, ChannelMessage, Message, Outcome, status};
use neoteletext_audio_instruments::synth106::interface::{
    self, Exclusive, MOST_PATCH_BYTES, PATCH_MESSAGE_BYTES, Refusal, Rule, SYNTH60_PATCH_BYTES, Switch, Tracker, ju06a,
};
use neoteletext_audio_instruments::synth106::{ChorusMode, Model, Patch, PatchError, PwmSource, Range, Slider};

const MODELS: [Model; 2] = [Model::Synth106, Model::Synth60];

/// The sixteen sliders, in parameter-number order.
const SLIDERS: [Slider; 16] = [
    Slider::LfoRate,
    Slider::LfoDelay,
    Slider::DcoLfo,
    Slider::DcoPwm,
    Slider::Noise,
    Slider::VcfFrequency,
    Slider::VcfResonance,
    Slider::VcfEnvelope,
    Slider::VcfLfo,
    Slider::VcfKeyboard,
    Slider::VcaLevel,
    Slider::Attack,
    Slider::Decay,
    Slider::Sustain,
    Slider::Release,
    Slider::Sub,
];

/// The JU-06A controller of each slider, in parameter-number order
/// (S106-22).
const SLIDER_CONTROLLERS: [u8; 16] = [3, 9, 13, 14, 19, 74, 71, 22, 23, 24, 26, 73, 75, 27, 72, 18];

/// The Juno-106 manual's example under section 3.2, page 35 (S106-15 item
/// 1).
const MANUAL_EXAMPLE: [u8; 24] = [
    0xF0, 0x41, 0x31, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x3F, 0x3C, 0x00, 0x00, 0x7F, 0x45, 0x00, 0x00, 0x7F,
    0x00, 0x00, 0x2A, 0x19, 0xF7,
];
/// The example under section 3.1, as S106-15 item 2 reads the scan: the
/// bytes as printed there, which a dot-matrix print leaves uncertain in
/// three digits; the test checks the decoding, not the patch's provenance.
const PATCH_EXAMPLE: [u8; 24] = [
    0xF0, 0x41, 0x30, 0x00, 0x00, 0x39, 0x2D, 0x00, 0x37, 0x00, 0x55, 0x00, 0x00, 0x00, 0x19, 0x34, 0x3B, 0x2D, 0x56,
    0x28, 0x00, 0x1A, 0x18, 0xF7,
];
/// The example under section 3.3 (S106-15 item 3).
const PARAMETER_EXAMPLE: [u8; 7] = [0xF0, 0x41, 0x32, 0x00, 0x03, 0x04, 0xF7];

fn sliders(patch: &Patch) -> [u8; 16] {
    SLIDERS.map(|slider| patch.get(slider))
}

fn read_patch(message: &[u8]) -> Patch {
    match interface::read_message(message) {
        Ok(Exclusive::Patch { patch, .. }) => patch,
        other => panic!("not a patch: {other:?}"),
    }
}

fn cc(number: u8, value: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, 0, number, value))
}

fn on(channel: u8, kind: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(kind, channel, data1, data2))
}

/// A tracker at the start of a sound with no bank.
fn tracker(model: Model) -> Tracker {
    let mut tracker = Tracker::new(model);
    assert_eq!(tracker.start(&Banks::none()), Outcome::Applied);
    tracker
}

/// Gives `tracker` a message as a block carries it.
fn take(tracker: &mut Tracker, message: &Message<'_>) -> Outcome {
    tracker.take(message, &Banks::none()).0
}

/// Gives `tracker` a whole system exclusive message from its `F0`.
fn exclusive(tracker: &mut Tracker, message: &[u8]) -> Outcome {
    take(tracker, &Message::SystemExclusive(&message[1..]))
}

// Verifies S106-15 items 1 and 2, S106-13, S106-14 and S106-18: the
// manual's two printed patch messages decode as the document says.
#[test]
fn s106_15_the_manuals_patch_examples_decode_as_printed() {
    let manual = read_patch(&MANUAL_EXAMPLE);
    assert_eq!(sliders(&manual), [3, 0, 0, 0, 0, 63, 60, 0, 0, 127, 69, 0, 0, 127, 0, 0]);
    assert_eq!(
        (manual.range, manual.pulse, manual.sawtooth, manual.chorus),
        (Range::Eight, true, false, ChorusMode::Off)
    );
    assert_eq!(
        (manual.pwm, manual.envelope_inverted, manual.vca_gate, manual.hpf),
        (PwmSource::Manual, false, false, 0)
    );
    let patch = read_patch(&PATCH_EXAMPLE);
    assert_eq!(sliders(&patch), [57, 45, 0, 55, 0, 85, 0, 0, 0, 25, 52, 59, 45, 86, 40, 0]);
    assert_eq!((patch.range, patch.pulse, patch.sawtooth, patch.chorus), (Range::Eight, true, true, ChorusMode::Two));
    assert_eq!((patch.pwm, patch.envelope_inverted, patch.vca_gate, patch.hpf), (PwmSource::Lfo, false, false, 0));
    // The program number byte is not used (S106-16): 0 and 127 give one
    // panel.
    let mut other = PATCH_EXAMPLE;
    other[4] = 127;
    assert_eq!(read_patch(&other), patch);
}

// Verifies S106-15 item 3 and S106-17: the manual's parameter example sets
// DCO PWM to 4 and nothing else.
#[test]
fn s106_15_the_manuals_parameter_example_sets_dco_pwm_alone() {
    assert_eq!(
        interface::read_message(&PARAMETER_EXAMPLE),
        Ok(Exclusive::Parameter { channel: 0, number: 3, value: 4 })
    );
    for model in MODELS {
        let mut tracker = tracker(model);
        let before = *tracker.panel();
        assert_eq!(exclusive(&mut tracker, &PARAMETER_EXAMPLE), Outcome::Applied);
        assert_eq!(*tracker.panel(), before.with(Slider::DcoPwm, 4), "{model:?}");
    }
}

// Verifies S106-17: each of parameters 0 to 15 at 0, 64 and 127 changes
// exactly its slider.
#[test]
fn s106_17_each_parameter_sets_exactly_its_slider() {
    for (number, slider) in (0u8..).zip(SLIDERS) {
        for value in [0, 64, 127] {
            let mut tracker = tracker(Model::Synth106);
            let before = *tracker.panel();
            let message = interface::parameter_message(0, number, value);
            assert_eq!(exclusive(&mut tracker, &message), Outcome::Applied);
            assert_eq!(*tracker.panel(), before.with(slider, value), "parameter {number} at {value}");
        }
    }
}

// Verifies S106-18: every range with every combination of bits 3 to 5
// and bit 6 of parameter 16, and every value of parameter 17.
#[test]
fn s106_18_the_switch_bytes_bit_by_bit() {
    for (bits, range) in [(0b001, Range::Sixteen), (0b010, Range::Eight), (0b100, Range::Four)] {
        for high in 0u8..16 {
            let value = high << 3 | bits;
            let mut patch = Patch::DEFAULT;
            patch.set_parameter(16, value).unwrap();
            let chorus = match (value >> 5) & 3 {
                0b01 | 0b11 => ChorusMode::Off,
                0b10 => ChorusMode::One,
                _ => ChorusMode::Two,
            };
            assert_eq!(
                (patch.range, patch.pulse, patch.sawtooth, patch.chorus),
                (range, value & 8 != 0, value & 16 != 0, chorus),
                "{value:#04x}"
            );
        }
    }
    for value in 0u8..32 {
        let mut patch = Patch::DEFAULT;
        patch.set_parameter(17, value).unwrap();
        let pwm = if value & 1 == 0 { PwmSource::Lfo } else { PwmSource::Manual };
        assert_eq!(
            (patch.pwm, patch.envelope_inverted, patch.vca_gate, patch.hpf),
            (pwm, value & 2 != 0, value & 4 != 0, 3 - (value >> 3)),
            "{value:#04x}"
        );
    }
}

// Verifies S106-19: each malformed form is refused whole, counted, and
// changes nothing.
#[test]
fn s106_19_malformed_messages_are_refused_whole() {
    let patch = |change: &dyn Fn(&mut Vec<u8>)| {
        let mut message = MANUAL_EXAMPLE.to_vec();
        message[2] = 0x30;
        change(&mut message);
        message
    };
    let parameter = |change: &dyn Fn(&mut Vec<u8>)| {
        let mut message = PARAMETER_EXAMPLE.to_vec();
        change(&mut message);
        message
    };
    let cases: Vec<(Vec<u8>, Rule)> = vec![
        (patch(&|m| m[1] = 0x43), Rule::Manufacturer),
        (patch(&|m| m[2] = 0x33), Rule::Function),
        (
            patch(&|m| {
                m.remove(22);
            }),
            Rule::Length,
        ),
        (patch(&|m| m.insert(22, 0)), Rule::Length),
        (
            parameter(&|m| {
                m.remove(5);
            }),
            Rule::Length,
        ),
        (parameter(&|m| m.insert(5, 0)), Rule::Length),
        (patch(&|m| m[3] = 0x10), Rule::Channel),
        (
            {
                let mut message = MANUAL_EXAMPLE.to_vec();
                message[4] = 1;
                message
            },
            Rule::Manual,
        ),
        (parameter(&|m| m[4] = 18), Rule::ParameterNumber),
        (patch(&|m| m[21] = 0x28), Rule::Patch(PatchError::Range)),
        (patch(&|m| m[21] = 0x2B), Rule::Patch(PatchError::Range)),
        (patch(&|m| m[21] = 0x2F), Rule::Patch(PatchError::Range)),
        (patch(&|m| m[22] = 0x20), Rule::Patch(PatchError::Switches)),
        (patch(&|m| m[22] = 0x40), Rule::Patch(PatchError::Switches)),
        (parameter(&|m| (m[4], m[5]) = (16, 0)), Rule::Patch(PatchError::Range)),
        (parameter(&|m| (m[4], m[5]) = (17, 0x20)), Rule::Patch(PatchError::Switches)),
    ];
    for (message, rule) in cases {
        assert_eq!(interface::read_message(&message).map_err(|refusal| refusal.rule), Err(rule), "{message:02x?}");
        for model in MODELS {
            let mut tracker = tracker(model);
            let before = *tracker.panel();
            assert_eq!(exclusive(&mut tracker, &message), Outcome::PassedOver, "{message:02x?}");
            assert_eq!(*tracker.panel(), before);
            assert!(!interface::accepts(model, &Message::SystemExclusive(&message[1..])));
        }
    }
}

// Verifies S106-2: the system exclusive channel byte 00 and 0F give the
// same panel, and a channel message acts alike on every channel.
#[test]
fn s106_2_every_channel_is_the_one_instrument() {
    let mut fifteen = PATCH_EXAMPLE;
    fifteen[3] = 0x0F;
    assert_eq!(read_patch(&fifteen), read_patch(&PATCH_EXAMPLE));
    for channel in 0..16 {
        let mut tracker = tracker(Model::Synth60);
        take(&mut tracker, &on(channel, status::CONTROL_CHANGE, ju06a::VCF_FREQUENCY, 9));
        take(&mut tracker, &on(15 - channel, status::CONTROL_CHANGE, controller_rpn::COARSE, 0));
        take(&mut tracker, &on(channel, status::CONTROL_CHANGE, controller_rpn::FINE, 0));
        take(&mut tracker, &on(15 - channel, status::CONTROL_CHANGE, controller_rpn::ENTRY, 5));
        assert_eq!(tracker.panel().get(Slider::VcfFrequency), 9);
        assert_eq!(tracker.common().channel(0).bend_range_cents, 500, "channel {channel}");
    }
}

/// Registered parameter number controllers.
mod controller_rpn {
    pub const COARSE: u8 = 101;
    pub const FINE: u8 = 100;
    pub const ENTRY: u8 = 6;
    pub const ENTRY_FINE: u8 = 38;
    pub const INCREMENT: u8 = 96;
    pub const DECREMENT: u8 = 97;
}

// Verifies S106-11: program numbers name the Juno-106's patches, checked
// against all 128 cells of the tables of page 27.
#[test]
fn s106_11_program_numbers_as_the_juno_106_names_them() {
    let mut program = 0u8;
    for group in ['A', 'B'] {
        for bank in 1..=8 {
            for patch in 1..=8 {
                assert_eq!(interface::program_name(program), (group, bank, patch));
                program = program.wrapping_add(1);
            }
        }
    }
    assert_eq!(program, 128);
    assert_eq!(
        [0, 7, 8, 63, 64, 127].map(interface::program_name),
        [('A', 1, 1), ('A', 1, 8), ('A', 2, 1), ('A', 8, 8), ('B', 1, 1), ('B', 8, 8)]
    );
}

// Verifies S106-21: a new synth106 and synth60 stand at the default patch,
// and its bytes decode to it.
#[test]
fn s106_21_the_default_patch() {
    let values = |last| [0, 0, 0, 0, 0, 0x7F, 0, 0, 0, 0, 0x40, 0, 0, 0x7F, 0, 0, 0x32, last];
    for (model, last, hpf) in [(Model::Synth106, 0x11, 1), (Model::Synth60, 0x19, 0)] {
        let default = interface::default_patch(model);
        assert_eq!(Patch::from_bytes(&values(last)), Ok(default), "{model:?}");
        assert_eq!(default.to_bytes(), values(last));
        assert_eq!(
            (default.range, default.sawtooth, default.pulse, default.chorus, default.pwm, default.hpf, default.sub),
            (Range::Eight, true, false, ChorusMode::Off, PwmSource::Manual, hpf, true)
        );
        assert_eq!(*Tracker::new(model).panel(), default);
        // The three synth60 settings at controller values 43, 64 and 0.
        let settings = interface::SYNTH60_SETTINGS.map(|switch| interface::switch_value(&default, switch));
        assert_eq!(settings, [43, 64, 0]);
    }
}

// Verifies S106-23 and S106-22: each slider's controller sets its slider
// to the value unchanged, the same slider as the parameter's message.
#[test]
fn s106_23_a_slider_controller_sets_the_value_unchanged() {
    for ((&number, slider), parameter) in SLIDER_CONTROLLERS.iter().zip(SLIDERS).zip(0u8..) {
        for value in [0, 1, 64, 126, 127] {
            for model in MODELS {
                let mut by_controller = tracker(model);
                let before = *by_controller.panel();
                assert_eq!(take(&mut by_controller, &cc(number, value)), Outcome::Applied);
                assert_eq!(*by_controller.panel(), before.with(slider, value));
                let mut by_message = tracker(model);
                exclusive(&mut by_message, &interface::parameter_message(0, parameter, value));
                assert_eq!(by_controller.panel(), by_message.panel());
            }
        }
    }
}

// Verifies S106-24: every value of each switch controller against its
// bands, the bands' edges by name, and each canonical value.
#[test]
fn s106_24_value_bands_and_canonical_values() {
    for n in [2u8, 3, 4] {
        for value in 0..=127u8 {
            assert_eq!(interface::position(value, n), (u16::from(value) * u16::from(n) / 128) as u8);
        }
    }
    let edges = |n| (0..=127u8).filter(move |&v| v == 0 || interface::position(v, n) != interface::position(v - 1, n));
    assert_eq!(edges(2).collect::<Vec<_>>(), [0, 64]);
    assert_eq!(edges(3).collect::<Vec<_>>(), [0, 43, 86]);
    assert_eq!(edges(4).collect::<Vec<_>>(), [0, 32, 64, 96]);
    for n in [2u8, 3, 4] {
        for position in 0..n {
            let value = interface::canonical(position, n);
            assert_eq!(interface::position(value, n), position);
            assert!(value == 0 || interface::position(value - 1, n) == position - 1);
        }
    }
    assert_eq!([2u8, 3, 4].map(|n| interface::position(42, n)), [0, 0, 1]);
    assert_eq!(interface::position(85, 3), 1);
}

/// The fields a switch moves, read back as a position.
fn switch_position(patch: &Patch, switch: Switch) -> u8 {
    interface::position(interface::switch_value(patch, switch), switch.positions())
}

// Verifies S106-25: each panel controller at each position's canonical
// value moves exactly its field; every controller the table does not name
// is counted as ignored and changes nothing.
#[test]
fn s106_25_the_controllers_move_exactly_their_fields() {
    let switches = [
        Switch::Range,
        Switch::PwmSource,
        Switch::Pulse,
        Switch::Sawtooth,
        Switch::Hpf,
        Switch::Polarity,
        Switch::Vca,
        Switch::Sub,
        Switch::Chorus,
    ];
    for switch in switches {
        for position in 0..switch.positions() {
            let mut tracker = tracker(Model::Synth60);
            let before = *tracker.panel();
            let value = interface::canonical(position, switch.positions());
            assert_eq!(take(&mut tracker, &cc(switch.controller(), value)), Outcome::Applied);
            let after = *tracker.panel();
            assert_eq!(switch_position(&after, switch), position, "{switch:?} {position}");
            for other in switches.iter().filter(|&&other| other != switch) {
                assert_eq!(
                    switch_position(&after, *other),
                    switch_position(&before, *other),
                    "{switch:?} moved {other:?}"
                );
            }
            assert_eq!(sliders(&after), sliders(&before));
        }
    }
    let named = |number: u8| {
        SLIDER_CONTROLLERS.contains(&number)
            || switches.iter().any(|switch| switch.controller() == number)
            || [1, 6, 7, 38, 64, 87, 96, 97, 100, 101].contains(&number)
            || number >= 120
    };
    for number in (0..=127u8).filter(|&number| !named(number)) {
        for model in MODELS {
            let mut tracker = tracker(model);
            let before = *tracker.panel();
            assert_eq!(take(&mut tracker, &cc(number, 64)), Outcome::PassedOver, "{model:?}: controller {number}");
            assert_eq!(*tracker.panel(), before);
            assert!(!interface::accepts(model, &cc(number, 64)));
        }
    }
    // Among them: bank select, NRPN, portamento, expression, LFO wave and
    // trigger, the delay, tempo sync and the assignment mode.
    for number in [0, 32, 98, 99, 5, 65, 11, 29, 30, 82, 83, 86, 88, 89, 91] {
        assert!(!named(number), "{number}");
    }
}

// Verifies S106-27: the synth60 positions sent to synth106 are the nearest
// it has, and controller 28 is ignored there.
#[test]
fn s106_27_synth60_settings_sent_to_synth106() {
    let mut tracker = tracker(Model::Synth106);
    take(&mut tracker, &cc(ju06a::DCO_PWM_SOURCE, 0));
    assert_eq!(take(&mut tracker, &cc(ju06a::DCO_PWM_SOURCE, 86)), Outcome::Applied);
    assert_eq!(tracker.panel().pwm, PwmSource::Manual);
    assert_eq!(take(&mut tracker, &cc(ju06a::CHORUS, 96)), Outcome::Applied);
    assert_eq!(tracker.panel().chorus, ChorusMode::Two);
    assert_eq!(take(&mut tracker, &cc(ju06a::DCO_SUB_SWITCH, 0)), Outcome::PassedOver);
    assert!(tracker.panel().sub);
    let mut sixty = self::tracker(Model::Synth60);
    take(&mut sixty, &cc(ju06a::DCO_PWM_SOURCE, 86));
    take(&mut sixty, &cc(ju06a::CHORUS, 96));
    take(&mut sixty, &cc(ju06a::DCO_SUB_SWITCH, 0));
    assert_eq!(
        (sixty.panel().pwm, sixty.panel().chorus, sixty.panel().sub),
        (PwmSource::Envelope, ChorusMode::Both, false)
    );
}

// Verifies S106-26: controller 87's bands, registered parameter 0, the
// last of either holding, at most twelve semitones, two by default, and
// left by a program change and by reset all controllers.
#[test]
fn s106_26_the_bend_range() {
    let range = |tracker: &Tracker| tracker.common().channel(0).bend_range_cents;
    let mut tracker = tracker(Model::Synth106);
    assert_eq!(range(&tracker), 200);
    for semitones in 0..=12u8 {
        let low = interface::bend_range_value(semitones);
        let high = if semitones == 12 { 127 } else { interface::bend_range_value(semitones + 1) - 1 };
        for value in [low, high] {
            take(&mut tracker, &cc(ju06a::BEND_RANGE, value));
            assert_eq!(range(&tracker), u16::from(semitones) * 100, "controller 87 at {value}");
        }
    }
    assert_eq!(
        (0..=12).map(interface::bend_range_value).collect::<Vec<_>>(),
        [0, 10, 20, 30, 40, 50, 60, 69, 79, 89, 99, 109, 119]
    );
    let rpn = |tracker: &mut Tracker, semitones: u8| {
        take(tracker, &cc(controller_rpn::COARSE, 0));
        take(tracker, &cc(controller_rpn::FINE, 0));
        take(tracker, &cc(controller_rpn::ENTRY, semitones))
    };
    for (semitones, cents) in [(1, 100), (7, 700), (24, 1_200)] {
        assert_eq!(rpn(&mut tracker, semitones), Outcome::Applied);
        assert_eq!(range(&tracker), cents);
    }
    rpn(&mut tracker, 7);
    take(&mut tracker, &cc(ju06a::BEND_RANGE, interface::bend_range_value(3)));
    assert_eq!(range(&tracker), 300);
    take(&mut tracker, &cc(controller_rpn::ENTRY_FINE, 120));
    assert_eq!(range(&tracker), 399, "cents of 100 to 127 are taken as 99");
    take(&mut tracker, &cc(controller_rpn::INCREMENT, 9));
    assert_eq!(range(&tracker), 400);
    take(&mut tracker, &cc(controller_rpn::DECREMENT, 0));
    assert_eq!(range(&tracker), 399);
    take(&mut tracker, &on(0, status::PROGRAM_CHANGE, 5, 0));
    take(&mut tracker, &cc(121, 0));
    assert_eq!(range(&tracker), 399, "a program change and controller 121 leave the range");
    // Reset all controllers selected no parameter: data entry is counted.
    assert_eq!(take(&mut tracker, &cc(controller_rpn::ENTRY, 2)), Outcome::PassedOver);
    // The null number selects none; another registered parameter is not
    // this family's.
    take(&mut tracker, &cc(controller_rpn::COARSE, 0));
    take(&mut tracker, &cc(controller_rpn::FINE, 1));
    assert_eq!(take(&mut tracker, &cc(controller_rpn::ENTRY, 2)), Outcome::PassedOver);
    take(&mut tracker, &cc(controller_rpn::FINE, 0));
    tracker.lost();
    assert_eq!(take(&mut tracker, &cc(controller_rpn::ENTRY, 2)), Outcome::PassedOver, "after a lost block");
    assert_eq!(range(&tracker), 399);
}

// Verifies S106-29 item 2 (MIDI-84, DEC-6464 version 2): reset all
// controllers sets modulation to 0, the bender to its centre, hold off and
// no parameter selected; it leaves the panel, controller 7 and the bend
// range, with the value 0 and with 127, on any channel.
#[test]
fn s106_29_reset_all_controllers_as_rp_015_says() {
    for (channel, value) in [(0, 0), (9, 127)] {
        let mut tracker = tracker(Model::Synth60);
        take(&mut tracker, &cc(1, 100));
        take(&mut tracker, &on(0, status::PITCH_BEND, 0, 100));
        take(&mut tracker, &cc(64, 127));
        take(&mut tracker, &cc(7, 50));
        take(&mut tracker, &cc(ju06a::VCF_FREQUENCY, 10));
        take(&mut tracker, &cc(ju06a::BEND_RANGE, 127));
        take(&mut tracker, &cc(controller_rpn::COARSE, 0));
        take(&mut tracker, &cc(controller_rpn::FINE, 0));
        let panel = *tracker.panel();
        assert_eq!(take(&mut tracker, &on(channel, status::CONTROL_CHANGE, 121, value)), Outcome::Applied);
        let state = tracker.common().channel(0);
        assert_eq!((state.modulation, state.bend, state.sustain), (0, 8_192, false));
        assert_eq!((state.volume, state.bend_range_cents), (50, 1_200));
        assert_eq!(*tracker.panel(), panel);
        assert_eq!(take(&mut tracker, &cc(controller_rpn::ENTRY, 2)), Outcome::PassedOver);
    }
}

// Verifies S106-30: pressures are counted and change nothing.
#[test]
fn s106_30_pressures_are_counted() {
    for model in MODELS {
        let mut tracker = tracker(model);
        assert_eq!(take(&mut tracker, &on(0, status::KEY_PRESSURE, 60, 9)), Outcome::PassedOver);
        assert_eq!(take(&mut tracker, &on(0, status::CHANNEL_PRESSURE, 9, 0)), Outcome::PassedOver);
        assert!(!interface::accepts(model, &on(0, status::CHANNEL_PRESSURE, 9, 0)));
    }
}

// Verifies S106-31 and S106-37: every one of the 24 combinations of the
// three synth60 settings packs to 33 bytes that check back to it, the 18
// values agreeing with the settings.
#[test]
fn s106_31_the_synth60_patch_is_the_18_values_and_three_controllers() {
    for pwm in [PwmSource::Lfo, PwmSource::Manual, PwmSource::Envelope] {
        for sub in [false, true] {
            for chorus in [ChorusMode::Off, ChorusMode::One, ChorusMode::Two, ChorusMode::Both] {
                let mut patch = Patch::DEFAULT.with(Slider::Sub, 77);
                (patch.pwm, patch.sub, patch.chorus) = (pwm, sub, chorus);
                let mut bytes = [0; MOST_PATCH_BYTES];
                assert_eq!(interface::pack_patch(Model::Synth60, &patch, 3, 2, &mut bytes), SYNTH60_PATCH_BYTES);
                let controls = &bytes[PATCH_MESSAGE_BYTES..];
                let pwm_value = [0, 43, 86][pwm as usize];
                let chorus_value = [0, 32, 64, 96][chorus as usize];
                assert_eq!(
                    controls,
                    [0xB2, 0x0F, pwm_value, 0xB2, 0x1C, if sub { 64 } else { 0 }, 0xB2, 0x5D, chorus_value]
                );
                assert_eq!(bytes[..5], [0xF0, 0x41, 0x30, 0x02, 0x03]);
                // The PWM bit is 1 for MAN and ENV; I+II is written as II.
                assert_eq!(bytes[5 + 17] & 1, u8::from(pwm != PwmSource::Lfo));
                let chorus_bits = (bytes[5 + 16] >> 5) & 3;
                assert_eq!(chorus_bits, [0b01, 0b10, 0b00, 0b00][chorus as usize]);
                assert_eq!(interface::check_patch(Model::Synth60, &bytes), Ok(patch));
                // The instrument reaches the same panel from the four
                // messages played one after another.
                let mut tracker = tracker(Model::Synth60);
                exclusive(&mut tracker, &bytes[..PATCH_MESSAGE_BYTES]);
                for control in bytes[PATCH_MESSAGE_BYTES..].chunks(3) {
                    take(&mut tracker, &on(2, status::CONTROL_CHANGE, control[1], control[2]));
                }
                assert_eq!(*tracker.panel(), patch);
            }
        }
    }
}

// Verifies S106-37: the server's check refuses a synth60 patch whose
// controllers are not canonical, not in order, or disagree with the 18
// values, naming the byte.
#[test]
fn s106_37_a_synth60_patch_that_is_not_canonical_or_disagrees_is_refused() {
    let mut patch = Patch::DEFAULT;
    patch.pwm = PwmSource::Envelope;
    patch.chorus = ChorusMode::Both;
    let mut bytes = [0; MOST_PATCH_BYTES];
    interface::pack_patch(Model::Synth60, &patch, 0, 0, &mut bytes);
    let refused = |change: &dyn Fn(&mut [u8; MOST_PATCH_BYTES])| {
        let mut changed = bytes;
        change(&mut changed);
        interface::check_patch(Model::Synth60, &changed)
    };
    assert_eq!(refused(&|b| b[26] = 87), Err(Refusal { byte: 26, rule: Rule::NotCanonical }));
    assert_eq!(refused(&|b| b[25] = 0x1C), Err(Refusal { byte: 24, rule: Rule::Controls }));
    assert_eq!(refused(&|b| b[24] = 0xB1), Err(Refusal { byte: 24, rule: Rule::Controls }));
    assert_eq!(refused(&|b| b[22] &= !1), Err(Refusal { byte: 22, rule: Rule::Disagrees }));
    assert_eq!(refused(&|b| b[21] = 0x5A), Err(Refusal { byte: 21, rule: Rule::Disagrees }));
    assert_eq!(interface::check_patch(Model::Synth60, &bytes[..24]).map_err(|r| r.rule), Err(Rule::Length));
    // A synth106 patch is the 24 bytes of a function type 30 or 31
    // message, and the manual's own example is one.
    assert_eq!(interface::check_patch(Model::Synth106, &MANUAL_EXAMPLE), Ok(read_patch(&MANUAL_EXAMPLE)));
    assert_eq!(interface::check_fields(Model::Synth106, &patch), Err(interface::FieldError::Pwm));
    assert_eq!(interface::check_fields(Model::Synth60, &patch), Ok(()));
}

// Verifies S106-32: an 18-value message alone on synth60 sets the three
// settings it implies; a parameter 17 message leaves the sub switch.
#[test]
fn s106_32_an_18_value_message_alone_on_synth60() {
    let mut tracker = tracker(Model::Synth60);
    take(&mut tracker, &cc(ju06a::DCO_PWM_SOURCE, 86));
    take(&mut tracker, &cc(ju06a::DCO_SUB_SWITCH, 0));
    take(&mut tracker, &cc(ju06a::CHORUS, 96));
    let mut message = MANUAL_EXAMPLE;
    message[22] &= !1;
    exclusive(&mut tracker, &message);
    let panel = tracker.panel();
    assert_eq!((panel.pwm, panel.sub, panel.chorus), (PwmSource::Lfo, true, ChorusMode::Off));
    take(&mut tracker, &cc(ju06a::DCO_SUB_SWITCH, 0));
    exclusive(&mut tracker, &interface::parameter_message(0, 17, 1));
    assert_eq!((tracker.panel().pwm, tracker.panel().sub), (PwmSource::Manual, false));
}

// Verifies S106-33: at one tick, the later of a controller and an
// exclusive message for the same field sets it.
#[test]
fn s106_33_the_later_message_sets_the_field() {
    let mut tracker = tracker(Model::Synth106);
    take(&mut tracker, &cc(ju06a::VCF_FREQUENCY, 10));
    exclusive(&mut tracker, &interface::parameter_message(0, 5, 100));
    assert_eq!(tracker.panel().get(Slider::VcfFrequency), 100);
    exclusive(&mut tracker, &interface::parameter_message(0, 5, 100));
    take(&mut tracker, &cc(ju06a::VCF_FREQUENCY, 10));
    assert_eq!(tracker.panel().get(Slider::VcfFrequency), 10);
}

/// 128 patches, each unlike the others.
fn many(count: u8) -> Vec<Patch> {
    (0..count).map(|k| Patch::DEFAULT.with(Slider::VcfFrequency, k).with(Slider::Release, 127 - k)).collect()
}

// Verifies S106-10, S106-12, S106-38 and S106-39: banks of 1 and 128
// patches of each model, byte for byte; program changes reach each patch;
// a program the bank lacks is the default and is counted; a faulty one is
// refused.
#[test]
fn s106_38_banks_of_up_to_128_patches_and_program_changes() {
    for model in MODELS {
        let size = interface::patch_bytes(model);
        let patches = many(128);
        let mut image = vec![0; 128 * size];
        assert_eq!(interface::build_bank(model, &patches, 0, &mut image), Ok(128 * size));
        assert_eq!(image.len(), [3_072, 4_224][model as usize]);
        assert_eq!(interface::check_bank(model, &image), Ok(128));
        for (k, patch) in patches.iter().enumerate() {
            let mut one = [0; MOST_PATCH_BYTES];
            interface::pack_patch(model, patch, k as u8, 0, &mut one);
            assert_eq!(image[k * size..(k + 1) * size], one[..size], "{model:?} patch {k}");
            assert_eq!(image[k * size + 4], k as u8);
        }
        let banks = Banks::new(&[&image]);
        let mut tracker = Tracker::new(model);
        assert_eq!(tracker.start(&banks), Outcome::Applied);
        assert_eq!(*tracker.panel(), patches[0]);
        for program in [0u8, 7, 8, 63, 64, 127] {
            let (outcome, _) = tracker.take(&on(program % 16, status::PROGRAM_CHANGE, program, 0), &banks);
            assert_eq!((outcome, *tracker.panel()), (Outcome::Applied, patches[usize::from(program)]));
        }
        // A bank of four: program 9 is the default, counted.
        let four = &image[..4 * size];
        let banks = Banks::new(&[four]);
        let (outcome, _) = tracker.take(&on(0, status::PROGRAM_CHANGE, 9, 0), &banks);
        assert_eq!((outcome, *tracker.panel()), (Outcome::PassedOver, interface::default_patch(model)));
        // Patch 2 renumbered 3 disagrees with its place: refused.
        let mut wrong = four.to_vec();
        wrong[2 * size + 4] = 3;
        assert_eq!(interface::check_bank(model, &wrong), Err(Refusal { byte: 2 * size + 4, rule: Rule::Program }));
        let banks = Banks::new(&[&wrong]);
        let (outcome, _) = tracker.take(&on(0, status::PROGRAM_CHANGE, 2, 0), &banks);
        assert_eq!((outcome, *tracker.panel()), (Outcome::PatchRefused, interface::default_patch(model)));
        // A function type 31 message is no bank's patch.
        let mut manual = four.to_vec();
        manual[size + 2] = 0x31;
        manual[size + 4] = 0;
        assert_eq!(interface::check_bank(model, &manual).map_err(|r| r.rule), Err(Rule::Function));
        // No patch, 129 and a part of one are no bank.
        let mut more = vec![0; 129 * size];
        assert_eq!(
            interface::build_bank(model, &many(128).repeat(2)[..129], 0, &mut more).map_err(|r| r.rule),
            Err(Rule::BankSize)
        );
        assert_eq!(interface::build_bank(model, &[], 0, &mut more).map_err(|r| r.rule), Err(Rule::BankSize));
        assert_eq!(interface::check_bank(model, &[]).map_err(|r| r.rule), Err(Rule::BankSize));
        assert_eq!(interface::check_bank(model, &image[..=size]).map_err(|r| r.rule), Err(Rule::BankSize));
    }
}

// Verifies S106-10: a program change leaves the bender, the modulation,
// hold, controller 7 and the bend range.
#[test]
fn s106_10_a_program_change_leaves_the_performance_controls() {
    let patches = many(2);
    let mut image = vec![0; 2 * PATCH_MESSAGE_BYTES];
    interface::build_bank(Model::Synth106, &patches, 0, &mut image).unwrap();
    let banks = Banks::new(&[&image]);
    let mut tracker = Tracker::new(Model::Synth106);
    tracker.start(&banks);
    for message in [cc(1, 9), cc(64, 127), cc(7, 33), cc(87, 50), on(0, status::PITCH_BEND, 1, 2)] {
        tracker.take(&message, &banks);
    }
    let before = *tracker.common().channel(0);
    tracker.take(&on(0, status::PROGRAM_CHANGE, 1, 0), &banks);
    assert_eq!(*tracker.common().channel(0), before);
    assert_eq!(*tracker.panel(), patches[1]);
}

/// Every message of `tracker`'s restatement on channel 0, as owned
/// messages: channel messages and system exclusive bytes after `F0`.
fn restated(tracker: &Tracker) -> Vec<Result<ChannelMessage, Vec<u8>>> {
    let mut out = Vec::new();
    tracker.restate(0, &mut |message| {
        out.push(match message {
            Message::Channel(message) => Ok(message),
            Message::SystemExclusive(bytes) => Err(bytes.to_vec()),
        });
    });
    out
}

/// Plays a restatement into `tracker`.
fn replay(tracker: &mut Tracker, messages: &[Result<ChannelMessage, Vec<u8>>], banks: &Banks<'_>) {
    for message in messages {
        let message = match message {
            Ok(message) => Message::Channel(*message),
            Err(bytes) => Message::SystemExclusive(bytes),
        };
        assert_ne!(tracker.take(&message, banks).0, Outcome::PassedOver, "{message:?}");
    }
}

// Verifies S106-40 (MIDI-93, MIDI-95): a restatement written from a live
// sound's state, played into a fresh instrument, gives the same state, in
// the document's order.
#[test]
fn s106_40_the_restatement_brings_a_fresh_part_to_the_state() {
    for model in MODELS {
        let patches = many(4);
        let mut image = vec![0; 4 * interface::patch_bytes(model)];
        interface::build_bank(model, &patches, 0, &mut image).unwrap();
        let banks = Banks::new(&[&image]);
        let mut sender = Tracker::new(model);
        sender.start(&banks);
        // Nothing moved: program 0 alone.
        assert_eq!(restated(&sender), [Ok(ChannelMessage::new(status::PROGRAM_CHANGE, 0, 0, 0))]);
        for message in [
            on(3, status::PROGRAM_CHANGE, 2, 0),
            cc(ju06a::VCF_FREQUENCY, 9),
            cc(ju06a::CHORUS, 96),
            cc(ju06a::BEND_RANGE, 99),
            on(0, status::PITCH_BEND, 5, 70),
            cc(7, 90),
            cc(1, 3),
            cc(64, 100),
        ] {
            sender.take(&message, &banks);
        }
        let messages = restated(&sender);
        let mut expected_panel = patches[2].with(Slider::VcfFrequency, 9);
        expected_panel.chorus = if model == Model::Synth60 { ChorusMode::Both } else { ChorusMode::Two };
        let mut packed = [0; MOST_PATCH_BYTES];
        let length = interface::pack_patch(model, &expected_panel, 2, 0, &mut packed);
        let mut expected = vec![Ok(ChannelMessage::new(status::PROGRAM_CHANGE, 0, 2, 0)), Err(packed[1..24].to_vec())];
        for control in packed[24..length].chunks(3) {
            expected.push(Ok(ChannelMessage::new(status::CONTROL_CHANGE, 0, control[1], control[2])));
        }
        expected.extend([
            Ok(ChannelMessage::new(status::CONTROL_CHANGE, 0, 87, 99)),
            Ok(ChannelMessage::new(status::PITCH_BEND, 0, 5, 70)),
            Ok(ChannelMessage::new(status::CONTROL_CHANGE, 0, 7, 90)),
            Ok(ChannelMessage::new(status::CONTROL_CHANGE, 0, 1, 3)),
            Ok(ChannelMessage::new(status::CONTROL_CHANGE, 0, 64, 127)),
        ]);
        assert_eq!(messages, expected, "{model:?}");
        let mut receiver = Tracker::new(model);
        receiver.start(&banks);
        replay(&mut receiver, &messages, &banks);
        assert_eq!(receiver.panel(), sender.panel(), "{model:?}");
        assert_eq!(receiver.common().channel(0), sender.common().channel(0), "{model:?}");
        assert_eq!(restated(&receiver), messages, "{model:?}: restated again, the same");
        // The bend range by registered parameter 0 is restated as its
        // sequence, and the selection put back to none.
        for message in [cc(101, 0), cc(100, 0), cc(6, 5), cc(38, 30), cc(101, 127), cc(100, 127)] {
            sender.take(&message, &banks);
        }
        let messages = restated(&sender);
        let mut receiver = Tracker::new(model);
        receiver.start(&banks);
        replay(&mut receiver, &messages, &banks);
        assert_eq!(receiver.common().channel(0), sender.common().channel(0), "{model:?}");
        assert_eq!(receiver.common().channel(0).bend_range_cents, 530);
    }
}

// Verifies S106-1: one message of each kind function III recognises is
// taken.
#[test]
fn s106_1_every_message_of_function_iii_is_taken() {
    for model in MODELS {
        let messages = [
            on(0, status::NOTE_ON, 60, 100),
            on(0, status::NOTE_OFF, 60, 0),
            cc(64, 127),
            cc(123, 0),
            cc(126, 1),
            on(0, status::PROGRAM_CHANGE, 0, 0),
            on(0, status::PITCH_BEND, 0, 70),
            cc(1, 50),
            Message::SystemExclusive(&PATCH_EXAMPLE[1..]),
            Message::SystemExclusive(&MANUAL_EXAMPLE[1..]),
            Message::SystemExclusive(&PARAMETER_EXAMPLE[1..]),
        ];
        let mut image = [0; MOST_PATCH_BYTES];
        let length = interface::pack_patch(model, &Patch::DEFAULT, 0, 0, &mut image);
        let banks = Banks::new(&[&image[..length]]);
        let mut tracker = Tracker::new(model);
        tracker.start(&banks);
        for message in messages {
            assert!(interface::accepts(model, &message), "{message:?}");
            assert_eq!(tracker.take(&message, &banks).0, Outcome::Applied, "{model:?}: {message:?}");
        }
    }
}
