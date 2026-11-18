//! One voice: two lines, each an oscillator, a high-pass and a low-pass
//! filter in series, the filters' envelope, and an amplifier with its
//! envelope.
//!
//! A voice runs at twice the output rate. What changes slowly is set at
//! intervals: every [`TICK_SAMPLES`] samples a tick moves the envelopes
//! on and sets the pitch, the pulse's width, the filters and the
//! amplifier's slope.

use crate::analog::drift::Variation;
use crate::analog::envelope::{ENVELOPE_ONE, IMMEDIATE, RcEnvelope, Stage, apply, time_constant};
use crate::analog::filter::{ONE_Q28, StateVariable};
use crate::analog::fixed::{amplify, mul_high, mul_high_unsigned, scale};
use crate::analog::oscillator::{Oscillator, waveform};
use crate::analog::{LEVEL, Oversampling, STEP_LIMIT, frequency};

use super::law::{self, LEVER_MAX, OCTAVE, SEMITONES};
use super::patch::{Control, Feet, GlideMode, Global, Patch, Tone};

/// How far above the output rate the voices run.
///
/// Source: own choice: the oscillators' edges are 91 dB clean at twice
/// the output rate and 48 dB at the rate itself
/// ([`crate::analog::oscillator`]), and the filter block's cutoff reaches
/// a quarter of its rate, which at the output rate would be 12 kHz.
pub const OVERSAMPLING: Oversampling = Oversampling::Twice;
/// The voices' rate in hertz.
pub const RATE: u32 = OVERSAMPLING.rate();
/// Bits of the samples in a tick.
const TICK_BITS: u32 = 4;
/// Samples of the voices between two ticks: a sixth of a millisecond.
///
/// Source: own choice: short against the envelopes' shortest attack of 1
/// ms, and a power of two for the amplifier's slope.
pub const TICK_SAMPLES: u32 = 1 << TICK_BITS;
/// Ticks a second.
pub const TICK_RATE: u32 = RATE / TICK_SAMPLES;
const _: () = assert!(TICK_RATE == 6_000 && RATE.is_multiple_of(TICK_SAMPLES));

/// The pitch of the A above middle C at 8', in thousandths of a hertz,
/// and its key.
///
/// Source: service manual, page 42, "VCO III IC (IG00153)", the table
/// under pin 2: 0.25 V, 130.8 Hz; 0.5 V, 261.6 Hz; 1 V, 523.2 Hz; 2 V,
/// 1046.0 Hz; 4 V, 2093.0 Hz: the Cs of the scale whose A is 440 Hz, at
/// 523.25 Hz to the volt. MIDI 1.0 for the key.
pub const A4: (u32, u8) = (440_000, 69);

/// The interval of a stop from 8'. The stops are whole shares of the
/// oscillator's charging current, so the fifths are pure: 5 1/3' is three
/// halves of 8'.
///
/// Source: service manual, page 41, M circuit diagram, "Transposition
/// Gate": four resistors R1 to R4 of 10, 20, 40 and 80 kΩ (0.1%) switched
/// onto the oscillator's FT pin by the gates VIII, IV, II and I, currents
/// of 8, 4, 2 and 1; 5 1/3' is 2 and 1 together, 2 2/3' is 4 and 2.
/// log₂ 1.5 = 0.584963.
const fn feet_octaves(feet: Feet) -> i32 {
    const FIFTH: i32 = law::octaves(584_963, 1_000_000);
    match feet {
        Feet::Sixteen => -OCTAVE,
        Feet::Eight => 0,
        Feet::FiveAndThird => FIFTH,
        Feet::Four => OCTAVE,
        Feet::TwoAndTwoThirds => OCTAVE + FIFTH,
        Feet::Two => 2 * OCTAVE,
    }
}

/// How far DETUNE CH II moves channel II either way.
///
/// Source: own choice: no document read gives the lever's range.
const DETUNE_RANGE: i32 = law::cents(100);
/// How far each oscillator stands off its pitch, fixed for the
/// oscillator.
///
/// Source: own choice, inside the adjustment procedure's tolerance (page
/// 5, steps 8 to 12: each stop is tuned "Within ± 1 cent").
const PITCH_TOLERANCE: i32 = law::cents(8) / 10;

/// The pulse's width at the lever's foot, a square, and how much wider
/// the lever's top makes it: 50% and 40% of a cycle.
///
/// Source: owner's manual, page 32, item 22: "from a symmetrical wave (50%)
/// to a narrow pulse (90%)".
const PULSE_WIDTH: (u32, u32) = (1 << 31, ((40u64 << 32) / 100) as u32);
/// How far the pulse width's oscillator at its peak moves the width
/// either way with PWM at 10: 20% of a cycle, half the lever's travel.
///
/// Source: owner's manual, page 15, item 21: with PW at 75% and PWM at its
/// top the effect "is identical to manually moving the PW slider back
/// and forth all the way". Own choice of the swing from that.
const PWM_SWING: u32 = ((20u64 << 32) / 100) as u32;
// The whole of the width's travel is under half a cycle, so the
// difference of any two widths is a signed word ([`Line::tick`]).
const _: () = assert!(PULSE_WIDTH.1 < 1 << 31);

/// The noise at NOISE 10 against a waveform, in Q30: 3.0 V of 3.5 V.
///
/// Source: adjustment procedure, page 17, noise generator: "3.0 ± .1
/// Vp-p"; service manual, page 42, VCO III, pin 7: the sawtooth is "3.5
/// Vp-p". Own choice: the two meet at the filter in that proportion.
const NOISE_LEVEL: i32 = (ENVELOPE_ONE as i64 * 30 / 35) as i32;
/// The right shift of the waveforms' sum into the filters: a waveform at
/// full level then stands a quarter of [`LEVEL`], and a filter's peak of
/// a Q of 12.5 on three such inputs stays inside what a signal holds.
///
/// Source: own choice.
const DRIVE_SHIFT: u32 = 2;
/// The gain from the filters to the amplifier with VCF LEVEL at 10, in
/// Q28: the drive given back, times 0.8.
///
/// Source: adjustment procedure, page 15, VCA, steps 1 and 2: the sine at
/// 10 alone is trimmed to "1.00 ± .02 Vp-p" at the amplifier, and the
/// square wave through open filters to "800 ± 20 mVp-p" (the page has
/// "VCA LEVEL ... 10" there, read as the VCF LEVEL lever, which step 1
/// has at 0).
const VCF_GAIN: i32 = (ONE_Q28 as i64 * (1 << DRIVE_SHIFT) * 8 / 10) as i32;

/// The right shift from the level input's share in Q28 to the
/// amplifier's gain in Q30: the gain with the LEVEL lever alone at its top
/// is an eighth, which leaves 18 dB for what the touch and the keyboard
/// control add, 9.3 dB with every lever at its top ([`LEVEL_RESISTORS`]).
///
/// Source: own choice.
const GAIN_SHIFT: u32 = 1;
const _: () = assert!((ONE_Q28 >> GAIN_SHIFT) == ENVELOPE_ONE >> 3);
/// The right shift of a line's output into the sum of sixteen: with
/// [`GAIN_SHIFT`], one line's sine at full level peaks 18 dB below the
/// mixed output's full scale, before the instrument's output gain.
///
/// Source: own choice.
const OUTPUT_SHIFT: u32 = 0;

/// The volts the filters' envelope reaches either way at its levers'
/// tops: 5 V, which [`law::ENVELOPE`] makes 1.95 octaves of the high-pass
/// filter and 5.07 of the low-pass.
///
/// Source: service manual, page 42, "VCF-EG IC (IG00152)", pins 14 and
/// 16: "the attack level is controlled from 0V until +5V", "the initial
/// level is controlled from zero to minus 5 volt".
const ENVELOPE_REACH: i32 = 5 * law::VOLT;
/// The volts the overall BRILLIANCE lever adds either way: the supply's
/// 15 V, which [`law::BRILLIANCE`] makes 0.86 octave of the high-pass
/// filter and 3.25 of the low-pass.
///
/// Source: service manual, page 64, Panel 2 circuit diagram: the lever
/// (VR13, B 10 kΩ) has its end 3 on the tone selector's +15 V, its centre
/// tap (5) on earth and its wiper on BR, so that it rests at exactly
/// nothing; its end 1 is on the line of the other levers that rest at
/// their centres (DETUNE and the keyboard control), whose supply the
/// page does not name. Own reading: that line is the -15 V. The lever
/// "adds voltage to the VCF's high pass and low pass control inputs"
/// (owner's manual, page 38, item 7).
const BRILLIANCE_REACH: i32 = 15 * law::VOLT;
/// The volts the touch response's amplifiers give at their levers' tops
/// for the voltage they are trimmed with: 6.4 V, which
/// [`law::MODULATION`] makes 2.08 octaves of the high-pass filter and
/// 7.82 of the low-pass. Pressure reaches it; a stroke does not
/// ([`STROKE_HARDEST`]).
///
/// Source: adjustment procedure, page 23, steps 1 and 3: initial-touch
/// and after-touch brilliance are each trimmed to "+6.4 ± .3V" at the
/// levers' tops; the after-touch "while firmly depressing C1 key", so
/// that a firm pressure is the 6.4 V itself. Service manual, page 39,
/// R51-52 circuit diagram: the two (IB, AB) reach each voice's FL line
/// through 39 kΩ, as the low-pass lever does, and its FH line through
/// 120 kΩ.
const TOUCH_BRILLIANCE: i32 = law::octaves(64, 10);
/// The most a stroke gives of [`TOUCH_BRILLIANCE`], as a fraction: 7.9 V
/// of the 8.5 V that the amplifier is trimmed with.
///
/// Source: adjustment procedure, page 23, step 1: the 6.4 V are trimmed
/// with the key's own signal taken off and the +8.5 V supply put in its
/// place ("Disconnect wire from terminal (b1 - b8) ... Connect jumper
/// wire from +8.5 (+V) terminal on KBC board"), so they are the
/// amplifier's gain, 6.4 / 8.5, and not what a stroke gives. Service
/// manual, page 11, TSB 1 circuit diagram, "DET": a key's signal reaches
/// that terminal through a silicon diode (1S1555) into 0.047 µF across 1
/// MΩ, so the terminal stands a diode's 0.6 V or more below whatever the
/// key gives, which the supply bounds. Own choice: the hardest stroke,
/// a velocity of 127, is that bound, and the voltage is in proportion to
/// the velocity below it; how many volts a stroke of a given speed gives
/// is in no document read. Owner's manual, page 36, item 42: the lever
/// "increases the brightness of any note when you play harder", so the
/// softest stroke adds nothing.
const STROKE_HARDEST: (i32, i32) = (79, 85);
/// The resistors through which the LEVEL lever and each of the touch
/// response's two level voltages and the keyboard control's reach a
/// voice's level input, in thousands of ohms: 560 and 270. The input is
/// the amplifier's linear one, so the gain is in proportion to the sum of
/// the currents: a volt of the touch or of the keyboard control is 560 /
/// 270 of a volt of the LEVEL lever, and 6.4 V of touch on a LEVEL lever at
/// its top (10 V, [`law::LEVER_VOLTS`]) make the gain 2.33 times what the
/// lever gives alone, 7.3 dB.
///
/// Source: service manual, page 39, R51-52 circuit diagram: each voice's
/// line LC is fed by LV (the LEVEL lever) through 560 kΩ, by TI, TA and
/// KB (the touch response's initial and after level and the keyboard
/// control's) through 270 kΩ each, and by AM (the sub oscillator) through
/// 180 kΩ; page 41, M circuit diagram: LC (terminal 31) goes, with 0.01 µF
/// to earth, to pin 2 of IC5, the third of the voice's four amplifiers
/// (IG00151); page 54, "VCA IC (IG00151)": pin 2 is LI, "Input of the
/// control voltage is provided for linear change of the level", drawn as
/// a straight line through nothing, where pin 1, EI, changes "the level
/// exponentially" and has only the trimmer here. Until this was read the
/// scale was an own choice in decibels to the volt, last 0.9 dB by twelve
/// recordings of a real instrument's BRASS 1, whose INITIAL LEVEL row
/// stands at the top: a stroke's voltage drains away in 3.3 s
/// ([`TOUCH_HOLD`]) and its level with it, and the recordings'
/// fundamentals fall 3.0 to 5.3 dB from 1 s to 10 s after the key, 4.0 dB
/// in the mean (`~/devel/teletext-reference/synthesizers/cs-80/validation/`,
/// `h1.py`). By this law, the hardest stroke taken as 7.9 V and the
/// overall RESONANCE lever at its foot, the same notes fall 5.3 to 5.6 dB
/// here, 5.4 dB in the mean, of which 2.3 dB are the amplifier's envelope
/// and 0.9 dB the filter's (`tests/synth80_filter.rs`); at 0.9 dB to the
/// volt they fell 4.0 dB.
const LEVEL_RESISTORS: (i64, i64) = (560, 270);
/// What a voltage at the touch's or the keyboard control's level
/// terminals adds to the level input, as a share in Q28 of what the LEVEL
/// lever gives at its top.
const fn level_share(volts: i32) -> i32 {
    /// A volt in Q28 over a volt in the units of an octave to the volt.
    const FINER: i64 = 1 << (28 - law::OCTAVE_BITS);
    let (lever, other) = LEVEL_RESISTORS;
    let product = (volts as i64).wrapping_mul(lever).wrapping_mul(FINER);
    match product.checked_div(other.wrapping_mul(law::LEVER_VOLTS as i64)) {
        Some(share) => share as i32,
        None => 0,
    }
}
const _: () = assert!(ONE_Q28 == 1 << 28 && law::VOLT == 1 << law::OCTAVE_BITS);
/// What the touch response's level levers add at their tops, as a share
/// in Q28 of the LEVEL lever's top: 6.4 V, 1.327. Pressure reaches it; a
/// stroke does not ([`STROKE_HARDEST`]).
///
/// Source: adjustment procedure, page 23, steps 2 and 4: "+6.4 ± .3V" at
/// the terminals L11 to L28, through [`LEVEL_RESISTORS`].
const TOUCH_LEVEL: i32 = level_share(law::octaves(64, 10));
/// The volts the sub oscillator at its peak adds to the brilliance
/// either way with its VCF lever at the top: 1.6 V, which
/// [`law::MODULATION`] makes 0.52 octave of the high-pass filter and 1.96
/// of the low-pass.
///
/// Source: service manual, page 64, Panel 2 circuit diagram: the VCF
/// lever (VR10, B 10 kΩ) stands between earth and the sub oscillator
/// board's output SUO, its wiper on the line SUB of the R52 board; page
/// 29, SUB circuit diagram: SUO is a follower of what the FUNCTION
/// switch chooses, the chip's sine among them; page 52, "VCO II IC
/// (IG00150)", pin 6: the sine is "3.20Vpp". The sawtooth's 3.5 Vp-p
/// (pin 7) would be 1.75 V; every function is given the sine's here.
const SUB_VCF_REACH: i32 = law::octaves(16, 10);
/// What pressure on a key lets the sub oscillator add with the touch
/// response's VCF lever at the top: 1.1 V either way.
///
/// Source: adjustment procedure, page 25, step 8: "2.2 ± .2 Vp-p" at the
/// terminals F1 to F8, the key firmly depressed.
const TOUCH_VCF_REACH: i32 = law::octaves(11, 10);
/// The octaves the sub oscillator at its peak moves the pitch either way
/// with its VCO lever at the top; the lever's position is squared, so
/// that its first half is a vibrato.
///
/// Source: own choice: the owner's manual (page 10, C) has a vibrato at
/// slow speeds and "a sound that resembles ring modulation" at fast.
const SUB_VCO_REACH: i32 = OCTAVE;
/// The same by pressure on a key with the touch response's VCO lever at
/// the top.
///
/// Source: own choice.
const TOUCH_VCO_REACH: i32 = law::cents(100);
/// How far below its note the hardest stroke starts with PITCHBEND at the
/// top, and the time constant of its return, in millionths of a second.
///
/// Source: own choice. Owner's manual, page 10, A: the pitch begins
/// "below the note you play" and will "quickly slide up"; the adjustment
/// procedure (page 25, step 5) gives the control's -4.2 V but not what a
/// volt of it is in pitch.
const TOUCH_BEND: (i32, u32) = (law::cents(200), 40_000);
/// The time constant with which a stroke's voltage dies away once it has
/// been taken, in millionths of a second: 10 MΩ across 0.33 µF, 3.3 s. A
/// stroke's brilliance and level are therefore the start of a note, and
/// a held note comes to rest where the softest stroke would leave it.
///
/// Source: service manual, page 17, TWS circuit diagram, and page 18,
/// the board as printed ("22K", "10M", "0.33/16" beside each of FET1 to
/// FET8): at a key's trigger a FET puts the stroke's voltage on a
/// tantalum capacitor of 0.33 µF at a follower's input, whose output is
/// the voice's initial touch (b1 to b8); the 10 MΩ goes from that input
/// to the capacitor's earthed side in each of the eight (the FET's gate
/// has its own 1 MΩ and a diode to the trigger). Pages 19 and 23: b1 to
/// b8 go only to the cells that the INITIAL levers scale (TRG1 and
/// TRG4), so nothing holds them further.
const TOUCH_HOLD: u32 = 3_300_000;

/// The key the keyboard control's levers do nothing at.
///
/// Source: owner's manual, page 9, item 9: "a transition occurs somewhere
/// near the middle of the keyboard"; the middle of the 61 keys from MIDI
/// 36 to 96.
const KEYBOARD_CENTRE: i32 = 66;
/// Semitones from the centre to the keys the keyboard control is trimmed
/// at, below and above.
///
/// Source: adjustment procedure, page 27, keyboard control: the low
/// levers are trimmed "Depressing C2 key", the high ones "Depressing C5#
/// key"; the manual's C1 is the lowest key (page 6, figure 5), MIDI 36.
const KEYBOARD_TRIM_KEYS: (i32, i32) = (KEYBOARD_CENTRE - 48, 85 - KEYBOARD_CENTRE);
/// The volts of brilliance of a keyboard control lever at its end, at
/// its trimmed key: 2.76 V, which [`law::MODULATION`] makes 0.90 octave
/// of the high-pass filter and 3.37 of the low-pass.
///
/// Source: the same table: "-2.76 ± .14V" and "+2.76 ± .14V". Own
/// choice: the control grows as the square of the distance from the
/// centre, for the "smooth curve" of the owner's manual (page 38, item 9).
const KEYBOARD_BRILLIANCE: i32 = law::octaves(276, 100);
/// The same for the level levers, as a share in Q28 of the LEVEL lever's
/// top: 2.76 V through [`LEVEL_RESISTORS`], 0.572 either way, which on a
/// LEVEL lever at its top is 3.9 dB up and 7.4 dB down.
const KEYBOARD_LEVEL: i32 = level_share(law::octaves(276, 100));

/// Thousandths of a time that are its time constant: for an attack, the
/// envelope block's 1,466 thousandths of a time constant to the peak; for
/// a fall, the time to a tenth of the way, 1 / ln 10.
///
/// Source: own choice of what the manual's times measure. The adjustment
/// procedure's figures (pages 7 and 13) mark the attack from the key to
/// the peak of a curve that is still rising when it ends.
const TIME_CONSTANT_THOUSANDTHS: (u64, u64) = (1_000_000 / RcEnvelope::ATTACK_TIME_CONSTANTS_THOUSANDTHS as u64, 434);
/// Thousandths.
const THOUSAND: u64 = 1_000;
/// The time in which a sustain of the second kind ends when the next key
/// is played, in millionths of a second.
///
/// Source: own choice: short enough to be an end, long enough not to
/// click.
const DAMP_TIME: u32 = 5_000;

/// The time constant of a time of the envelopes, as a coefficient.
fn coefficient(time: u32, thousandths: u64) -> i32 {
    let time_constant_of = u64::from(time).saturating_mul(thousandths) / THOUSAND;
    time_constant(u32::try_from(time_constant_of).unwrap_or(u32::MAX), TICK_RATE)
}

/// What a patch asks of every line of one channel, worked out once when
/// it is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Channel {
    /// The oscillator's waveforms in use.
    waves: u8,
    /// Whether anything goes through the filters and comes out.
    filtered: bool,
    /// Whether the line can be heard at all.
    pub audible: bool,
    noise_level: i32,
    sine_level: i32,
    vcf_gain: i32,
    width: u32,
    pwm: u32,
    /// The stop and the detuning, in octaves.
    pitch: i32,
    /// Octaves from the cutoff with nothing at the control input to each
    /// filter's, by its lever and the overall brilliance.
    hpf: i32,
    lpf: i32,
    /// Each filter's damping in Q28, without the overall lever.
    hpf_damping: i32,
    lpf_damping: i32,
    /// The filters' envelope: where it starts, in volts, and how far it
    /// is from there to the attack's peak.
    initial: i32,
    span: i32,
    filter_times: [i32; 3],
    filter_sustain: i32,
    amp_times: [i32; 3],
    amp_sustain: i32,
    /// The amplifier's release while the foot switch holds the sustain.
    amp_sustained: i32,
    /// The LEVEL lever's share of its top, and the mix's, in Q30.
    lever: i32,
    mix: i32,
    /// What the level input's sum is scaled by, in Q30, so that the
    /// hardest stroke on the LEVEL lever's top is nominal.
    nominal: i32,
    touch_brilliance: i32,
    pressure_brilliance: i32,
    touch_level: i32,
    pressure_level: i32,
}

impl Channel {
    /// The settings of one channel of a patch.
    pub fn new(patch: &Patch, channel: usize) -> Self {
        let tone = patch.tones.get(channel).copied().unwrap_or(Tone::DEFAULT);
        let lever = |control: Control| tone.get(control);
        let second = channel != 0;
        let mut waves = 0;
        if tone.on(Control::Sawtooth) {
            waves |= waveform::SAWTOOTH;
        }
        if tone.on(Control::Square) {
            waves |= waveform::PULSE;
        }
        let feet = Feet::from_value(patch.get(if second { Global::FeetTwo } else { Global::FeetOne }));
        let detune = if second { law::centred(DETUNE_RANGE, patch.get(Global::Detune)) } else { 0 };
        // The mix is a balance: each channel's share falls in a straight
        // line from all at its own end to none at the other (own choice).
        let mix = patch.get(Global::Mix);
        let share = if second { mix } else { LEVER_MAX.saturating_sub(mix) };
        let mix = law::level(share);
        // The stroke's one voltage makes its level as its brilliance.
        let touch_level =
            law::share_of(law::share(TOUCH_LEVEL, lever(Control::TouchLevel)), STROKE_HARDEST.0, STROKE_HARDEST.1);
        let pressure_level = law::share(TOUCH_LEVEL, lever(Control::PressureLevel));
        // The hardest stroke with the LEVEL lever at its top is nominal
        // (own choice of where nominal is: the levels of the presets,
        // which all have INITIAL LEVEL at 10, against the panel's): the
        // sum is divided by what that stroke makes it.
        let nominal = (i64::from(ONE_Q28) << 30).checked_div(i64::from(ONE_Q28).saturating_add(i64::from(touch_level)));
        let nominal = i32::try_from(nominal.unwrap_or(0)).unwrap_or(ENVELOPE_ONE);
        let noise = lever(Control::Noise);
        let vcf_level = lever(Control::VcfLevel);
        let filtered = (waves != 0 || noise != 0) && vcf_level != 0;
        let sine = lever(Control::Sine);

        let brilliance = law::centred(BRILLIANCE_REACH, patch.get(Global::Brilliance));
        let cutoff = |control: Control, lever_weight: i32, brilliance_weight: i32| {
            law::scaled(law::share(law::LEVER_VOLTS * law::VOLT, lever(control)), lever_weight)
                .saturating_add(law::scaled(brilliance, brilliance_weight))
        };
        // The chip has the mean of each filter's own lever and the
        // overall one ([`law::damping`]).
        let resonance = u32::from(patch.get(Global::Resonance));
        let damping = |control: Control| {
            let damping = law::damping(u32::from(lever(control)).saturating_add(resonance));
            i32::try_from(damping).unwrap_or(i32::MAX)
        };
        let initial = law::share(ENVELOPE_REACH, lever(Control::InitialLevel)).saturating_neg();
        let span = law::share(ENVELOPE_REACH, lever(Control::AttackLevel)).saturating_sub(initial);
        // The envelope rests at the levers' own setting, which is this
        // share of the way from where it starts to its peak.
        let filter_sustain =
            if span == 0 { ENVELOPE_ONE } else { law::share_of(ENVELOPE_ONE, initial.saturating_neg(), span) };

        let (rise, fall) = TIME_CONSTANT_THOUSANDTHS;
        let times = |attack: Control, decay: Control, release: Control| {
            [
                coefficient(law::time(law::ATTACK_SHORTEST, lever(attack)), rise),
                coefficient(law::time(law::FALL_SHORTEST, lever(decay)), fall),
                coefficient(law::time(law::FALL_SHORTEST, lever(release)), fall),
            ]
        };
        // SUSTAIN lengthens the release it finds, and never shortens it;
        // it is always in effect, as on the instrument with its SUSTAIN
        // FOOT SWITCH assigner off (S80-26; owner's manual, page 11, item
        // 13 D).
        let sustained = lever(Control::Release).max(patch.get(Global::Sustain));
        Self {
            waves,
            filtered,
            // The touch and the keyboard control reach the level input
            // beside the LEVEL lever, so the lever at its foot does not
            // by itself silence a line.
            audible: mix != 0 && (filtered || sine != 0),
            noise_level: apply(law::level(noise), NOISE_LEVEL),
            sine_level: law::level(sine),
            vcf_gain: law::share(VCF_GAIN, vcf_level),
            width: PULSE_WIDTH
                .0
                .saturating_add(mul_high_unsigned(PULSE_WIDTH.1, level_q32(lever(Control::PulseWidth)))),
            pwm: mul_high_unsigned(PWM_SWING, level_q32(lever(Control::Pwm))),
            pitch: feet_octaves(feet).saturating_add(detune),
            hpf: cutoff(Control::Hpf, law::LEVER.0, law::BRILLIANCE.0),
            lpf: cutoff(Control::Lpf, law::LEVER.1, law::BRILLIANCE.1),
            hpf_damping: damping(Control::HpfResonance),
            lpf_damping: damping(Control::LpfResonance),
            initial,
            span,
            filter_times: times(Control::FilterAttack, Control::FilterDecay, Control::FilterRelease),
            filter_sustain,
            amp_times: times(Control::Attack, Control::Decay, Control::Release),
            amp_sustain: law::level(lever(Control::Sustain)),
            amp_sustained: coefficient(law::time(law::FALL_SHORTEST, sustained), fall),
            lever: law::level(lever(Control::Level)),
            mix,
            nominal,
            touch_brilliance: law::share_of(
                law::share(TOUCH_BRILLIANCE, lever(Control::TouchBrilliance)),
                STROKE_HARDEST.0,
                STROKE_HARDEST.1,
            ),
            pressure_brilliance: law::share(TOUCH_BRILLIANCE, lever(Control::PressureBrilliance)),
            touch_level,
            pressure_level,
        }
    }
}

/// A lever's position as a share of one in Q32, the top a unit short.
fn level_q32(lever: u8) -> u32 {
    let share = (u64::from(lever.min(LEVER_MAX)) << 32).checked_div(u64::from(LEVER_MAX)).unwrap_or(0);
    u32::try_from(share).unwrap_or(u32::MAX)
}

/// What a patch asks of every voice outside its two tones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shared {
    /// The keyboard control's levers at their trimmed keys: brilliance
    /// low and high in octaves, level low and high as shares of the
    /// LEVEL lever's top in Q28.
    keyboard: [i32; 4],
    sub_vco: i32,
    sub_vcf: i32,
    /// How far the sub oscillator moves the level either way, in Q30.
    sub_vca: i32,
    touch_vco: i32,
    touch_vcf: i32,
    touch_bend: i32,
    /// The octaves a glide moves in a tick, and whether it moves in
    /// semitones.
    glide_step: i32,
    glissando: bool,
    /// SUSTAIN II: a glissando goes on after its key is released
    /// (S80-27).
    sustain_two: bool,
    bend_return: i32,
    /// The share of a stroke's voltage that drains away in a tick.
    touch_fall: i32,
    damp: i32,
}

/// The period of the glide's clock at the lever's foot, in billionths of
/// a second, and the octaves of time to its top: 0.17 ms and 0.16 s, a
/// semitone to the period.
///
/// Source: adjustment procedure, page 27, portamento/glissando, clock
/// oscillator: ".17ms" with the lever at SHORT and ".16s" at LONG. Owner's
/// manual, page 12, A: at LONG "about 2 seconds per octave", which twelve
/// periods of 0.16 s are. log₂ (160 / 0.17) = 9.878. Own choice: the
/// period is exponential in the lever between the two. The lever's foot
/// itself switches the glide off ([`Shared::new`]): the 0.17 ms is the
/// clock there, which then moves nothing.
const GLIDE_CLOCK: (u32, i32) = (170_000, law::octaves(9_878, 1_000));
/// Billionths of a second in a second.
const NANOSECONDS: u64 = 1_000_000_000;

impl Shared {
    /// The settings of a patch.
    pub fn new(patch: &Patch) -> Self {
        let lever = |global: Global| patch.get(global);
        let mode = GlideMode::from_value(lever(Global::GlideMode));
        let squared = law::scaled(law::level(lever(Global::SubVco)), law::level(lever(Global::SubVco)));
        let period = u64::from(law::raise(GLIDE_CLOCK.0, law::share(GLIDE_CLOCK.1, lever(Global::GlideTime))));
        // A semitone in each period, in octaves a tick.
        let semitone = u64::from((OCTAVE / SEMITONES).cast_unsigned());
        let ticks = period.saturating_mul(u64::from(TICK_RATE));
        let step = semitone.saturating_mul(NANOSECONDS).checked_div(ticks).unwrap_or(u64::MAX);
        Self {
            keyboard: [
                law::centred(KEYBOARD_BRILLIANCE, lever(Global::KeyboardBrillianceLow)),
                law::centred(KEYBOARD_BRILLIANCE, lever(Global::KeyboardBrillianceHigh)),
                law::centred(KEYBOARD_LEVEL, lever(Global::KeyboardLevelLow)),
                law::centred(KEYBOARD_LEVEL, lever(Global::KeyboardLevelHigh)),
            ],
            sub_vco: law::scaled(SUB_VCO_REACH, squared),
            sub_vcf: law::share(SUB_VCF_REACH, lever(Global::SubVcf)),
            sub_vca: law::level(lever(Global::SubVca)),
            touch_vco: law::share(TOUCH_VCO_REACH, lever(Global::TouchVco)),
            touch_vcf: law::share(TOUCH_VCF_REACH, lever(Global::TouchVcf)),
            touch_bend: law::share(TOUCH_BEND.0, lever(Global::TouchPitchBend)),
            // At the lever's foot the glide is off, and a voice takes its
            // key's voltage at once: service manual, page 15, "D-A
            // CONVERTER LSI (YM26700)", pin 3, POR: "When the portament
            // VR is turned on, +8.5V is supplied to the pin and actuate".
            // The selector's centre is off too (S80-27).
            glide_step: if lever(Global::GlideTime) == 0 || mode == GlideMode::Off {
                i32::MAX
            } else {
                i32::try_from(step).unwrap_or(i32::MAX).max(1)
            },
            glissando: mode == GlideMode::Glissando,
            sustain_two: patch.on(Global::SustainMode),
            bend_return: time_constant(TOUCH_BEND.1, TICK_RATE),
            touch_fall: time_constant(TOUCH_HOLD, TICK_RATE),
            damp: coefficient(DAMP_TIME, TIME_CONSTANT_THOUSANDTHS.1),
        }
    }

    /// The keyboard control at a key: octaves of brilliance, and the share
    /// of the LEVEL lever's top that it adds to the level input, in Q28.
    fn keyboard(&self, key: u8) -> (i32, i32) {
        let [brilliance_low, brilliance_high, level_low, level_high] = self.keyboard;
        let distance = i32::from(key).saturating_sub(KEYBOARD_CENTRE);
        let (trim, brilliance, level) = if distance < 0 {
            (KEYBOARD_TRIM_KEYS.0, brilliance_low, level_low)
        } else {
            (KEYBOARD_TRIM_KEYS.1, brilliance_high, level_high)
        };
        let square = distance.saturating_mul(distance);
        let whole = trim.saturating_mul(trim);
        (law::share_of(brilliance, square, whole), law::share_of(level, square, whole))
    }
}

/// What every voice is given at a tick: the instrument's modulation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Controls {
    /// The sub oscillator, in Q30 either way.
    pub sub: i32,
    /// Each channel's pulse width oscillator, in Q30 either way.
    pub pwm: [i32; 2],
    /// The pitch bend and the tuning, in octaves, which move every
    /// pitch and the filters with it (S80-7, S80-47).
    pub bend: i32,
    /// Octaves the modulation wheel lets the sub oscillator move the
    /// pitch, at its peak.
    pub wheel: i32,
}

/// The sine of a phase in units of 2⁻³² cycle, between -[`LEVEL`] and
/// [`LEVEL`]: the odd polynomial of the fifth degree through the quarter
/// cycle's ends with the sine's slopes there, `x (π/2 - x² (π - 5/2 - x²
/// (π/2 - 3/2)))` for x from -1 to 1. It is within 0.07% of the sine, and
/// its harmonics are 77 dB down (`tests/synth80_pitch.rs`).
///
/// Source: own derivation. The instrument's sine is itself shaped from a
/// triangle (service manual, page 42, "WSC IC (IG00158)", pin 15, STI).
#[inline]
#[must_use]
pub const fn sine(phase: u32) -> i32 {
    /// π/2, π - 5/2 and π/2 - 3/2 in Q29.
    const COEFFICIENTS: [i32; 3] = [843_314_857, 344_452_802, 38_008_945];
    let [first, third, fifth] = COEFFICIENTS;
    // The triangle of the phase, x in Q29: up from the cycle's start.
    let turned = phase.wrapping_add(1 << 30);
    let folded = if turned < 1 << 31 { turned } else { turned.wrapping_neg() };
    let x = (folded >> 1).cast_signed().wrapping_sub(1 << 29);
    let square = mul_high(x, x) << 4;
    let inner = third.wrapping_sub(mul_high(square, fifth) << 2);
    let outer = first.wrapping_sub(mul_high(square, inner) << 2);
    // Q29 times Q29 over 2³² is Q26; a signal's one is Q24.
    mul_high(x, outer) >> 2
}
const _: () = assert!(LEVEL == 1 << 24);

/// One line: an oscillator, the two filters, the two envelopes and the
/// amplifier.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Line {
    oscillator: Oscillator,
    hpf: StateVariable,
    lpf: StateVariable,
    filter_envelope: RcEnvelope,
    amp_envelope: RcEnvelope,
    /// The amplifier's gain in Q30 and what it gains each sample.
    gain: i32,
    gain_step: i32,
    /// The pulse's width and what it gains each sample, in units of 2⁻³²
    /// cycle: the width moves in a straight line from one tick to the
    /// next, as the gain does. Set once a tick it would be a staircase,
    /// which adds components the ticks' rate away from the sound's own
    /// (36 dB below the strongest harmonic with PWM and its SPEED at
    /// their tops, `tests/synth80_modulation.rs`).
    width: u32,
    width_step: i32,
    /// The oscillator's own offset of pitch, in octaves.
    trim: i32,
    /// What the oscillator and the filters were last set from, so that a
    /// tick that changes nothing sets nothing.
    set: [i64; 3],
}

/// A setting no tick asks for: the first tick sets everything.
const UNSET: i64 = i64::MIN;

impl Line {
    fn new(variation: &mut Variation) -> Self {
        let mut oscillator = Oscillator::new();
        // An oscillator runs whether a key is down or not, so a note
        // finds it anywhere in its cycle: it is left at a place of its
        // own, and the edges that took it there are let out.
        oscillator.enable(0);
        oscillator.set_step(variation.offset(i32::MAX).unsigned_abs() >> 1);
        for _ in 0..PLACING_TICKS {
            let _ = oscillator.tick(None);
        }
        oscillator.set_step(0);
        Self {
            oscillator,
            hpf: StateVariable::new(),
            lpf: StateVariable::new(),
            filter_envelope: RcEnvelope::new(),
            amp_envelope: RcEnvelope::new(),
            gain: 0,
            gain_step: 0,
            width: PULSE_WIDTH.0,
            width_step: 0,
            trim: variation.offset(PITCH_TOLERANCE),
            set: [UNSET; 3],
        }
    }

    fn configure(&mut self, channel: &Channel) {
        self.oscillator.enable(channel.waves);
        let [attack, decay, release] = channel.filter_times;
        self.filter_envelope.set_attack(attack);
        self.filter_envelope.set_decay(decay);
        self.filter_envelope.set_release(release);
        self.filter_envelope.set_sustain(channel.filter_sustain);
        let [attack, decay, _] = channel.amp_times;
        self.amp_envelope.set_attack(attack);
        self.amp_envelope.set_decay(decay);
        self.amp_envelope.set_sustain(channel.amp_sustain);
        self.set = [UNSET; 3];
    }

    fn trigger(&mut self) {
        self.filter_envelope.retrigger();
        self.amp_envelope.retrigger();
    }

    fn silent(&self) -> bool {
        self.amp_envelope.stage() == Stage::Rest && self.gain.unsigned_abs() < GAIN_REST
    }

    /// A tick. `pitch` is the oscillator's interval from the A and `key`
    /// the key voltage's from the filters' reference key, in octaves;
    /// `brilliance` is what the keys, the touch and the sub oscillator
    /// add to the filters' control, in volts; `added` is what they add to
    /// the level input, as a share in Q28 of the LEVEL lever's top.
    #[allow(clippy::too_many_arguments, reason = "one line's share of a voice's tick, taken apart once")]
    fn tick(
        &mut self,
        channel: &Channel,
        gate: bool,
        release: i32,
        pitch: i32,
        key: i32,
        brilliance: i32,
        added: i32,
        pwm: i32,
        tremolo: i32,
    ) {
        self.filter_envelope.gate(gate);
        self.amp_envelope.gate(gate);
        self.amp_envelope.set_release(release);
        let filter_level = self.filter_envelope.tick();
        let amp_level = self.amp_envelope.tick();

        let pitch = pitch.saturating_add(channel.pitch).saturating_add(self.trim);
        let [pitch_set, hpf_set, lpf_set] = &mut self.set;
        let asked = i64::from(pitch);
        if *pitch_set != asked {
            *pitch_set = asked;
            self.oscillator.set_step(frequency(law::raise(A4.0, pitch), RATE));
        }
        let swing = mul_high(channel.pwm.cast_signed(), pwm) << 2;
        let width = channel
            .width
            .saturating_add_signed(swing)
            .clamp(PULSE_WIDTH.0, PULSE_WIDTH.0.saturating_add(PULSE_WIDTH.1));
        // Both are inside the lever's travel, 40% of a cycle, so their
        // difference is a signed word.
        self.width_step = width.wrapping_sub(self.width).cast_signed() >> TICK_BITS;

        let envelope = channel.initial.saturating_add(law::scaled(channel.span, filter_level));
        let filters = [
            (&mut self.hpf, hpf_set, channel.hpf, channel.hpf_damping, law::ENVELOPE.0, law::MODULATION.0),
            (&mut self.lpf, lpf_set, channel.lpf, channel.lpf_damping, law::ENVELOPE.1, law::MODULATION.1),
        ];
        for (filter, set, lever, damping, envelope_weight, modulation_weight) in filters {
            let interval = key
                .saturating_add(lever)
                .saturating_add(law::scaled(envelope, envelope_weight))
                .saturating_add(law::scaled(brilliance, modulation_weight));
            let asked = i64::from(interval);
            if *set != asked {
                *set = asked;
                let cutoff = law::raise(law::CUTOFF_AT_NOTHING, interval);
                let cutoff = cutoff.clamp(law::CUTOFF_FLOOR, law::CUTOFF_CEILING);
                filter.set(frequency(cutoff, RATE).min(STEP_LIMIT), damping);
            }
        }

        // The level input has the sum of the lever and what is added,
        // and nothing below nothing ([`LEVEL_RESISTORS`]).
        let sum = (channel.lever >> LEVER_SHIFT).saturating_add(added).max(0);
        let gain = apply(sum, channel.nominal) >> GAIN_SHIFT;
        // The tremolo's factor is given as its half.
        let level = apply(apply(amp_level, channel.mix), gain);
        let level = apply(level, tremolo).min(ENVELOPE_ONE >> 1) << 1;
        self.gain_step = level.saturating_sub(self.gain) >> TICK_BITS;
    }

    /// One sample. `noise` is the channel's noise generator.
    #[inline]
    fn sample(&mut self, channel: &Channel, noise: i32) -> i32 {
        self.width = self.width.wrapping_add_signed(self.width_step);
        self.oscillator.set_width(self.width);
        let waves = self.oscillator.tick(None);
        let mut signal = 0;
        if channel.filtered {
            // The instrument's sawtooth falls (service manual, page 42,
            // VCO III, pin 7).
            let mixed = waves.pulse.wrapping_sub(waves.sawtooth).wrapping_add(apply(noise, channel.noise_level));
            let high = self.hpf.tick(mixed >> DRIVE_SHIFT).high;
            signal = amplify(self.lpf.tick(high).low, channel.vcf_gain);
        }
        if channel.sine_level != 0 {
            signal = signal.wrapping_add(apply(sine(self.oscillator.phase()), channel.sine_level));
        }
        self.gain = self.gain.wrapping_add(self.gain_step);
        apply(signal, self.gain) >> OUTPUT_SHIFT
    }
}

/// The right shift from a level in Q30 to a share in Q28.
const LEVER_SHIFT: u32 = 2;
const _: () = assert!(ENVELOPE_ONE >> LEVER_SHIFT == ONE_Q28);
/// Ticks that leave a new oscillator at a place of its own and let the
/// edges on the way out of its rings.
const PLACING_TICKS: u32 = 3 + 2 * Oscillator::DELAY as u32;
/// The gain below which an amplifier whose envelope is at rest is
/// silent, in Q30.
const GAIN_REST: u32 = 1 << 10;

/// One of the eight voices.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Voice {
    lines: [Line; 2],
    key: u8,
    gate: bool,
    /// Whether a sustain of the second kind has been ended.
    damped: bool,
    /// The key voltage's interval from the A, in octaves, where the glide
    /// has brought it.
    glide: i32,
    /// What is left of how hard the key was struck, which drains away
    /// ([`TOUCH_HOLD`]), and how hard the key is pressed, in Q30.
    velocity: i32,
    pressure: i32,
    /// The touch's pitch bend, in octaves, on its way back to nothing.
    bend: i32,
}

/// The largest value of a MIDI data byte, which a velocity and a pressure
/// are shares of.
const DATA_MAX: i32 = 127;

impl Voice {
    /// A voice at rest; `number` tells its oscillators' places and
    /// tolerances from the other voices'.
    pub fn new(seed: u32, number: u32) -> Self {
        let mut variation = Variation::new(seed, number);
        Self {
            lines: [Line::new(&mut variation), Line::new(&mut variation)],
            key: A4.1,
            gate: false,
            damped: false,
            glide: 0,
            velocity: 0,
            pressure: 0,
            bend: 0,
        }
    }

    /// Takes a patch's settings.
    pub fn configure(&mut self, channels: &[Channel; 2]) {
        for (line, channel) in self.lines.iter_mut().zip(channels) {
            line.configure(channel);
        }
    }

    /// A key for this voice: the envelopes start again, and the pitch
    /// sets out from where it was.
    pub fn start(&mut self, key: u8, velocity: u8, shared: &Shared) {
        self.key = key;
        self.gate = true;
        self.damped = false;
        self.velocity = law::share_of(ENVELOPE_ONE, i32::from(velocity), DATA_MAX);
        self.bend = law::scaled(shared.touch_bend, self.velocity).saturating_neg();
        for line in &mut self.lines {
            line.trigger();
        }
    }

    /// The key is up, and with it whatever pressed it: the after-touch
    /// answers "when you press down on a key after it hits bottom"
    /// (owner's manual, page 36, items 44 and 45), and a key that is up
    /// is not at the bottom. A note therefore dies away as one that was
    /// never pressed; no message is needed, or could be given, to take
    /// the pressure off a key that is up.
    pub const fn release(&mut self) {
        self.gate = false;
        self.pressure = 0;
    }

    /// Ends the voice's sustain: the next key of a sustain of the second
    /// kind has been played.
    pub const fn damp(&mut self) {
        self.damped = true;
    }

    /// How hard the key is pressed, of 127.
    pub fn press(&mut self, pressure: u8) {
        self.pressure = law::share_of(ENVELOPE_ONE, i32::from(pressure), DATA_MAX);
    }

    /// How hard the key is pressed, in Q30.
    pub const fn pressure(&self) -> i32 {
        self.pressure
    }

    /// Silences the voice at once, keeping its oscillators where they
    /// are.
    pub fn cut(&mut self, channels: &[Channel; 2]) {
        self.gate = false;
        for (line, channel) in self.lines.iter_mut().zip(channels) {
            line.hpf.reset();
            line.lpf.reset();
            line.filter_envelope = RcEnvelope::new();
            line.amp_envelope = RcEnvelope::new();
            line.gain = 0;
            line.gain_step = 0;
            line.configure(channel);
        }
    }

    /// Channel I's amplifier envelope's position, as a page's rules read
    /// it (page-rules.md, RULE-31).
    pub fn meter(&self) -> (crate::midi::MeterStage, u8) {
        let [first, _] = &self.lines;
        first.amp_envelope.meter()
    }

    /// Whether the voice, its key up, has fallen silent.
    pub fn silent(&self) -> bool {
        !self.gate && self.lines.iter().all(Line::silent)
    }

    /// A tick: the glide moves on, and each line takes its pitch, its
    /// filters' cutoffs and its level.
    pub fn tick(&mut self, channels: &[Channel; 2], shared: &Shared, controls: Controls) {
        // The glide: a fixed interval in each tick towards the key. A
        // glissando stops where it is when its key is released, unless
        // SUSTAIN II carries it on (S80-27; owner's manual, page 12).
        let target = law::keys(A4.1, self.key);
        let distance = target.saturating_sub(self.glide);
        let step = if shared.glissando && !self.gate && !shared.sustain_two { 0 } else { shared.glide_step };
        self.glide = self.glide.saturating_add(distance.clamp(step.saturating_neg(), step));
        let key_voltage = if shared.glissando && self.glide != target {
            // Whole semitones only, counted from the key it is going to:
            // it stays on each until the glide has left it behind.
            let semitone = OCTAVE / SEMITONES;
            let left = self.glide.saturating_sub(target);
            let whole =
                left.saturating_abs().saturating_add(semitone.saturating_sub(1)).checked_div(semitone).unwrap_or(0);
            target.saturating_add(whole.saturating_mul(semitone).saturating_mul(left.signum()))
        } else {
            self.glide
        };
        self.bend = self.bend.saturating_sub(scale(self.bend, shared.bend_return));
        self.velocity = self.velocity.saturating_sub(scale(self.velocity, shared.touch_fall));

        let sub = controls.sub;
        let vibrato =
            shared.sub_vco.saturating_add(controls.wheel).saturating_add(law::scaled(shared.touch_vco, self.pressure));
        // The bend and the tuning move the key voltage, so the filters
        // follow them as they follow the keys (S80-7, S80-47).
        let key_voltage = key_voltage.saturating_add(controls.bend);
        let pitch = key_voltage.saturating_add(self.bend).saturating_add(law::scaled(vibrato, sub));
        let wah = shared.sub_vcf.saturating_add(law::scaled(shared.touch_vcf, self.pressure));
        let (keyboard_brilliance, keyboard_level) = shared.keyboard(self.key);
        let key = key_voltage.saturating_add(law::keys(law::CUTOFF_REFERENCE_KEY, A4.1));
        // The sub oscillator moves the level about nominal: at full
        // depth from nothing at its trough to twice at its crest. Half
        // of that factor, in Q30.
        let tremolo = (ENVELOPE_ONE.saturating_add(apply(shared.sub_vca, sub)) >> 1).clamp(0, ENVELOPE_ONE);

        for ((line, channel), &pwm) in self.lines.iter_mut().zip(channels).zip(&controls.pwm) {
            if !channel.audible {
                line.gain = 0;
                line.gain_step = 0;
                // The envelopes still say when the voice is done.
                line.amp_envelope.gate(self.gate);
                line.amp_envelope.set_release(IMMEDIATE);
                let _ = line.amp_envelope.tick();
                continue;
            }
            let brilliance = keyboard_brilliance
                .saturating_add(law::scaled(channel.touch_brilliance, self.velocity))
                .saturating_add(law::scaled(channel.pressure_brilliance, self.pressure))
                .saturating_add(law::scaled(wah, sub));
            // What is left of the stroke, the pressure and the keyboard
            // control add their voltages to the LEVEL lever's.
            let added = keyboard_level
                .saturating_add(law::scaled(channel.touch_level, self.velocity))
                .saturating_add(law::scaled(channel.pressure_level, self.pressure));
            let release = if self.damped { shared.damp } else { channel.amp_sustained };
            line.tick(channel, self.gate, release, pitch, key, brilliance, added, pwm, tremolo);
        }
    }

    /// One sample of both lines. `noise` is each channel's noise.
    #[inline]
    pub fn sample(&mut self, channels: &[Channel; 2], noise: [i32; 2]) -> i32 {
        let [first, second] = &mut self.lines;
        let [one, two] = channels;
        let [noise_one, noise_two] = noise;
        let mut sum = 0;
        if one.audible {
            sum = first.sample(one, noise_one);
        }
        if two.audible {
            sum = sum.wrapping_add(second.sample(two, noise_two));
        }
        sum
    }
}
