//! The controls' laws: what each slider's position means in hertz,
//! octaves and seconds, and the arithmetic that turns an interval in
//! octaves into a frequency.
//!
//! Roland's documents give the ends of most ranges and, in the service
//! notes' adjustments, a number of calibration points; how a slider
//! travels between them they do not say. Each figure below names its
//! document, and each law between documented points says that it is an
//! own choice and why.
//!
//! The service notes' adjustments name keys by the instrument's own
//! keyboard, C0 to C5 (service notes, appendix I, the keyboard drawn
//! above the module controller board). The owner's manual tunes the
//! instrument to "A-440" (printed page 9) and adjustment 6 trims the key
//! A3 to twice the 442 Hz of its reference at 8' and to eight times it
//! at 2', so the instrument's A2 is the A above middle C and its C0 is
//! MIDI key 36.

use crate::analog::STEP_LIMIT;
use crate::analog::envelope::ENVELOPE_ONE;
use crate::analog::fixed::mul_high_unsigned;
pub use crate::analog::octave::{OCTAVE, OCTAVE_BITS, cents, keys, octaves};
use crate::analog::octave::{ONE_Q30, square_root};

/// The largest value of a slider's byte.
pub const SLIDER_MAX: u8 = 255;
/// The largest magnitude of a signed slider.
pub const SIGNED_MAX: i32 = 127;

/// Sixty-fourths of an octave: the table's steps.
const TABLE_BITS: u32 = 6;
/// Entries of the table: one for each step and one for the octave itself.
const TABLE_LENGTH: usize = (1 << TABLE_BITS) + 1;
/// Bits of an interval below a step of the table.
const REST_BITS: u32 = OCTAVE_BITS - TABLE_BITS;
/// The mask of those bits.
const REST_MASK: u32 = (1 << REST_BITS) - 1;
/// The left shifts of the difference between two entries, which is below
/// 2²⁵, and of the rest, before their product's upper word is taken: the
/// two and the 32 bits of the word come to [`REST_BITS`].
const DIFFERENCE_SHIFT: u32 = 4;
/// See [`DIFFERENCE_SHIFT`].
const REST_SHIFT: u32 = 32 - REST_BITS - DIFFERENCE_SHIFT;

/// 2^(k/64) in Q30 for k from 0 to 64: each is a product of the square
/// root of two taken one to six times.
///
/// Source: computed here; the roots are whole square roots of numbers in
/// Q60, so each entry is within a few units of 2⁻³⁰ of its value.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const POWERS: [u32; TABLE_LENGTH] = {
    // roots[i] = 2^(1 / 2^(i+1)).
    let mut roots = [0u64; TABLE_BITS as usize];
    let mut value = 2 * ONE_Q30;
    let mut index = 0;
    while index < roots.len() {
        value = square_root((value as u128) << 30) as u64;
        roots[index] = value;
        index += 1;
    }
    let mut table = [0u32; TABLE_LENGTH];
    let mut entry = 0;
    while entry < table.len() - 1 {
        let mut power = ONE_Q30;
        let mut bit = 0;
        while bit < roots.len() {
            // The top bit of the entry's number is the half octave.
            if entry & (1 << (roots.len() - 1 - bit)) != 0 {
                power = (power * roots[bit]) >> 30;
            }
            bit += 1;
        }
        table[entry] = power as u32;
        entry += 1;
    }
    table[TABLE_LENGTH - 1] = (2 * ONE_Q30) as u32;
    table
};

/// 2 to the power of the fraction of an interval, in Q30, from one to
/// two: the table's two nearest entries and the straight line between
/// them, which is within 0.03 cent of the power. It takes one product
/// and no division, and may be called for every sample.
#[inline]
#[must_use]
pub fn power_of_fraction(octaves: i32) -> u32 {
    let fraction = (octaves & (OCTAVE - 1)).cast_unsigned();
    let index = (fraction >> REST_BITS) as usize;
    let (Some(&before), Some(&after)) = (POWERS.get(index), POWERS.get(index.wrapping_add(1))) else {
        return ONE_Q30 as u32;
    };
    let rest = fraction & REST_MASK;
    before.wrapping_add(mul_high_unsigned(after.wrapping_sub(before) << DIFFERENCE_SHIFT, rest << REST_SHIFT))
}

/// A frequency step raised by an interval in octaves, held to
/// [`STEP_LIMIT`]: `step * 2^octaves`, within 0.03 cent. It takes two
/// products and no division, and may be called for every sample.
#[inline]
#[must_use]
pub fn raise_step(step: u32, octaves: i32) -> u32 {
    /// Bits a word has.
    const WORD: i32 = u32::BITS.cast_signed();
    let whole = octaves >> OCTAVE_BITS;
    // The step in Q32 of the limit's four, times the ratio in Q30.
    let raised = mul_high_unsigned(step.min(STEP_LIMIT.wrapping_sub(1)) << 2, power_of_fraction(octaves));
    if whole >= 0 {
        if whole >= WORD || raised > STEP_LIMIT >> whole { STEP_LIMIT } else { raised << whole }
    } else if whole <= -WORD {
        0
    } else {
        raised >> whole.unsigned_abs()
    }
}

/// `value * 2^octaves`, held to a word, within 0.03 cent: for hertz and
/// seconds, where a control changes.
///
/// It is this instrument's own and not [`crate::analog::octave::raise`]
/// so that a control and the loop that renders samples take the same
/// power, [`power_of_fraction`], and a frequency set by one agrees to the
/// last place with a step made by the other.
#[must_use]
pub fn raise(value: u32, octaves: i32) -> u32 {
    let whole = octaves >> OCTAVE_BITS;
    let scaled = u64::from(value).wrapping_mul(u64::from(power_of_fraction(octaves)));
    let shifted = match whole {
        ..=-34 => 0,
        -33..=-1 => scaled >> whole.unsigned_abs().saturating_add(30),
        0..=31 => {
            let wide = u128::from(scaled) << whole.unsigned_abs();
            u64::try_from(wide >> 30).unwrap_or(u64::MAX)
        }
        _ => u64::MAX,
    };
    u32::try_from(shifted).unwrap_or(u32::MAX)
}

/// `amount * slider / 255`, rounded towards zero.
#[must_use]
pub fn share(amount: i32, slider: u8) -> i32 {
    const FULL: i64 = SLIDER_MAX as i64;
    let product = i64::from(amount).saturating_mul(i64::from(slider)) / FULL;
    i32::try_from(product).unwrap_or(0)
}

/// `amount * slider / 127` for a signed slider, held to -127 to 127.
#[must_use]
pub fn signed_share(amount: i32, slider: i32) -> i32 {
    const FULL: i64 = SIGNED_MAX as i64;
    let slider = slider.clamp(-SIGNED_MAX, SIGNED_MAX);
    let product = i64::from(amount).saturating_mul(i64::from(slider)) / FULL;
    i32::try_from(product).unwrap_or(0)
}

/// `amount * part / whole`, rounded towards zero; nothing for a whole of
/// zero.
#[must_use]
pub fn share_of(amount: i32, part: i32, whole: i32) -> i32 {
    let product = i64::from(amount).saturating_mul(i64::from(part)).checked_div(i64::from(whole));
    i32::try_from(product.unwrap_or(0)).unwrap_or(0)
}

/// A slider's position as a level in Q30: 255 is one.
#[must_use]
pub fn level(slider: u8) -> i32 {
    share(ENVELOPE_ONE, slider)
}

/// The pitch of the A above middle C, in thousandths of a hertz, and its
/// MIDI key.
///
/// Source: owner's manual, printed page 9: "reference very near to A-440
/// if the Master Tune knob ... is in the 12 o'clock position"; service
/// notes, adjustment 6: the key A3 at 8' is trimmed to 884 Hz against an
/// A-442 reference, an octave above it. MIDI 1.0 for the key.
pub const A4: (u32, u8) = (440_000, 69);
/// The key the filter's key follow turns about: middle C.
///
/// Source: owner's manual, printed page 19: "Key Follow's center point is
/// center C of keyboard".
pub const MIDDLE_C: u8 = 60;
/// The lowest key of the instrument's keyboard, its C0.
///
/// Source: service notes, appendix I: the keyboard runs from C0 to C5,
/// 61 keys; see the head of this file for the octave.
pub const LOWEST_KEY: u8 = 36;

/// The cutoff with the slider at 0 and nothing modulating, in thousandths
/// of a hertz: 442 Hz less four tenths of the slider's span.
///
/// Source: service notes, adjustment 16, VCF TUNE: with ENV MOD at 0 and
/// "CO FREQ to 5 (S/N **0600 - 4)" the oscillating filter is trimmed to
/// the A-442 reference, "Press a key ... for 1:1 Lissajous". Own reading:
/// the later serial numbers' position 4 is taken, as [`CUTOFF_SPAN`]'s
/// figures are, and "a key" is taken to mean any key, the key follow
/// being down; had the key follow stood at 100% and the key been A2, the
/// cutoff at middle C would be three quarters of an octave lower.
/// 442 / 2^(0.4 * 8.966) = 36.8 Hz.
pub const CUTOFF_BASE: u32 = 36_800;
/// The octaves the cutoff slider covers from 0 to 10: log₂ 500 = 8.966.
///
/// Source: service notes, adjustment 14, VCF WIDTH, the figures for
/// serial numbers "**0600 and subsequent": the oscillating filter shows
/// one cycle at 2 ms a division (20 ms, 50 Hz), then with "CO FREQ to 10"
/// five cycles at 20 µs a division (25 kHz). Own reading: the first step
/// is taken with the slider at 0. The earlier instruments' figures are
/// 100 Hz and 20 kHz, 7.64 octaves.
pub const CUTOFF_SPAN: i32 = octaves(8_966, 1_000);
/// The lowest and highest cutoff the filter chip reaches, in thousandths
/// of a hertz.
///
/// Source: own choice. synth8's documents give no limits; these
/// are the JUNO-6 service notes' "VCF 4Hz to 40kHz" for the same chip in
/// the same circuit (page 1, Specifications).
pub const CUTOFF_BOUNDS: (u32, u32) = (4_000, 40_000_000);
/// The octaves the envelope at its peak moves the cutoff with ENV MOD at
/// 10.
///
/// Source: service notes, adjustment 15, VCF ENV MOD: one cycle at 0.2 ms
/// a division (500 Hz) becomes, with "VCF ENV MOD to 10", 16 cycles at
/// 50 µs a division (32 kHz): six octaves.
pub const CUTOFF_ENVELOPE: i32 = 6 * OCTAVE;
/// The octaves the low-frequency oscillator at its peak moves the cutoff
/// either way with LFO MOD at 10.
///
/// Source: service notes, adjustment 23, VCF LFO MOD LEVEL: "Press C2
/// key (S/N **0600— C4 key), adjust CON VR7 for 50 percentage
/// modulation", with a figure of a square wave whose height at its
/// least is marked 1 against 2 at its most. The adjustment's drawing of
/// the panel, read at the scan's 400 dots an inch, has VCO-1 alone, its
/// square at 2', the cutoff at 10, the resonance, ENV MOD and KEY FOLLOW
/// at 0, the slope at -12 dB and LFO MOD at 10. A square wave through
/// two equal poles is half its height from peak to peak with the cutoff
/// at 0.762 of its frequency (worked out from its harmonics); the later
/// instruments' key C4 at 2' is 4,186 Hz, so the cutoff falls to
/// 3,189 Hz from adjustment 14's 25 kHz: 2.97 octaves. Own reading: the
/// later serial numbers' figures, as [`CUTOFF_SPAN`]'s, and the three
/// octaves either way of the slider's 10.
pub const CUTOFF_LFO: i32 = 3 * OCTAVE;
/// The cutoff's key follow, as points of the slider's 127 steps and the
/// share of the key's interval from middle C that the cutoff follows
/// there, in hundredths: none at 0, all of it at 7.5 and 120% at 10.
///
/// Source: service notes, adjustment 13, VCF KEY FOLLOWER: the filter
/// oscillating, the key C2 is set to "display one complete cycle" and a
/// trimmer then makes the key C4 "display 4 complete cycles", two
/// octaves for two; the adjustment's drawing of the panel has the KEY
/// FOLLOW slider's knob marked "7.5". Roland's specifications (service
/// notes, page 1; owner's manual, printed page 29): "Key Follow: 0 -
/// 120%". Own choice: straight lines between the three.
pub const KEY_FOLLOW_LAW: [(i32, i32); 3] = [(0, 0), (95, 100), (SIGNED_MAX, 120)];
const _: () = assert!(KEY_FOLLOW_LAW[1].0 == (SIGNED_MAX * 75) / 100);

/// The share of the key's interval from middle C that the cutoff follows
/// for a signed slider's position, in hundredths, signed as the slider
/// is.
#[must_use]
pub fn key_follow(slider: i32) -> i32 {
    let [first, middle, last] = KEY_FOLLOW_LAW;
    let position = slider.clamp(-SIGNED_MAX, SIGNED_MAX);
    let size = position.saturating_abs();
    let (from, to) = if size <= middle.0 { (first, middle) } else { (middle, last) };
    let along = share_of(to.1.saturating_sub(from.1), size.saturating_sub(from.0), to.0.saturating_sub(from.0));
    let share = from.1.saturating_add(along);
    if position < 0 { share.saturating_neg() } else { share }
}

/// The octaves the low-frequency oscillator at its peak moves the pitch
/// either way with the VCO modulator's LFO MOD at 10.
///
/// Source: service notes, adjustment 22: with "VCO LFO MOD to 10" the
/// trimmer is set "so that Lissajous becomes 2:1 at the highest pitch.
/// Note that LFO modulated VCO swing equals 2 oct's".
pub const PITCH_LFO: i32 = OCTAVE;
/// The octaves envelope 1 at its peak moves the pitch with the VCO
/// modulator's ENV MOD at 10.
///
/// Source: service notes, adjustment 8, VCO ENV MOD DEPTH: with the
/// sustain at 10, VCO-1 at 16' with "VCO MOD ENV to 10" is trimmed to
/// the pitch it had at 2' with none: three octaves.
pub const PITCH_ENVELOPE: i32 = 3 * OCTAVE;
/// The octaves that ten volts of VCO-2's waveform move VCO-1's pitch
/// upwards with CROSS MOD at 10.
///
/// Source: service notes, adjustment 10, VCO CROSS MOD DEPTH: "sets
/// modulating voltage to the value by which VCO-1 frequency is shifted by
/// 3 octaves when CROSS MOD is set at 5", with VCO-2 a square wave in its
/// low range. The module board's schematic draws that wave from 0 V to
/// 10 V, so the three octaves are its ten volts against none, and VCO-1
/// goes nowhere below its own pitch under it; what each of VCO-2's
/// waveforms does with its own voltage is in the voice (the sawtooth as
/// the pulse, the sine half the depth either way, the noise a quarter).
/// Own reading: the slider is a straight line.
pub const CROSS_MODULATION: i32 = 6 * OCTAVE;
/// VCO-2's fine tune at either end, in octaves.
///
/// Source: Roland's specifications: "VCO-2 Fine Tune Range: ±50 Cents".
pub const FINE_TUNE: i32 = cents(50);
/// The octaves below its normal pitch at middle C that VCO-2 runs in its
/// low range, where it no longer follows the keyboard.
///
/// Source: own choice. Owner's manual, printed page 16: in "Low
/// Frequency range, VCO-2 will produce only low frequency signals which
/// cannot be heard", and "the frequencies produced in this range are not
/// effected by the keyboard"; no document gives the frequencies. Seven
/// octaves put the range knob's 16' to 2' at 1 Hz to 8 Hz.
pub const LOW_RANGE: i32 = 7 * OCTAVE;

/// The low-frequency oscillator's lowest rate in thousandths of a hertz.
///
/// Source: Roland's specifications: "LFO Rate: 0.05 - 40Hz".
pub const LFO_RATE_BASE: u32 = 50;
/// The octaves from the lowest rate to the highest, 30 Hz: log₂ 600 =
/// 9.229.
///
/// Source: service notes, adjustment 21, LFO MODULATION: with the scope
/// at "10ms/div", "Adjust CON VR3 to display exactly 3 complete cycles",
/// 30 Hz; its drawing of the panel has the RATE slider at 10, with DELAY
/// TIME, which the same adjustment trims to 4 s. The specifications'
/// 40 Hz is a quarter more than the instrument is trimmed to; the
/// trimmed figure is taken, as the attack's 6 s is ([`ATTACK_LAW`]). Own
/// choice: the rate is exponential in the slider.
pub const LFO_RATE_SPAN: i32 = octaves(9_229, 1_000);
/// The longest delay of the low-frequency oscillator, in millionths of a
/// second.
///
/// Source: Roland's specifications: "Delay Time: 0 - 4s"; service notes,
/// adjustment 21, step 4: "VCO becomes being modulated approx. 4 sec
/// after the key first depressed". Own choice: a straight line in the
/// slider.
pub const LFO_DELAY: u32 = 4_000_000;

/// The high-pass filter's cutoff with its slider at the top, in
/// thousandths of a hertz, and the octaves the slider covers below it.
/// With the slider at 0 there is no filter.
///
/// Source: own choice. Owner's manual, printed page 18: "The Source
/// Mixer output is unchanged with the HPF Cutoff Frequency set at 'Low',
/// becoming progressively less full in body ... The cutoff slope of the
/// HPF is -6dB/octave"; no document gives its frequencies. The module
/// board's schematic (page 15 of the file, HPF), read at the scan's 400
/// dots an inch, has the mixer's output pass C45A, 680 pF, to a node
/// that an amplifier of variable transconductance (IC26A, 470 Ω at each
/// input, R141A of 150 kΩ back from the follower TR19A) holds to
/// ground, so the cutoff is in proportion to that amplifier's control
/// current, which the transistor TR18A sets from the slider's voltage
/// by its own base voltage: no figure of the drawing fixes it. 20 Hz to
/// 5 kHz spans what "less full in body" can mean.
pub const HPF_TOP: (u32, i32) = (5_000_000, octaves(7_966, 1_000));

/// A point of an envelope time's law: a slider position of 255 and the
/// octaves above the shortest time.
type TimePoint = (u8, i32);

/// The shortest time of every stage, in millionths of a second.
///
/// Source: Roland's specifications: "Attack Time: 1ms - 5s", "Decay
/// Time: 1ms - 10s", "Release Time: 1ms - 10s".
pub const TIME_BASE: u32 = 1_000;
/// The attack's law: 1 ms at 0, 100 ms at 4.5 and 6 s at 10, in octaves
/// above [`TIME_BASE`] (log₂ 100 = 6.644, log₂ 6000 = 12.551), a straight
/// line in octaves between the points.
///
/// Source: service notes, adjustment 20, ENVELOPE TOTAL TIME: the
/// trimmer is set "for 6-sec attack period" with the slider at its top
/// (the specifications say 5 s); then, at 20 ms a division, "adjust both
/// ENV-1 ATTACK (around 4-5) and timebase VARI or vernier so that
/// envelope's falling edge is centered on the screen", for the other
/// voices to be trimmed to: 100 ms, as nearly as a slider "around 4-5"
/// and a vernier off its calibrated stop say it. Own choice: the middle
/// point at 4.5 and 100 ms, and the lines between; the sliders' taper resistors
/// (panel board D: 4.7 kΩ on each) and the chip's law are not known well
/// enough to do better.
pub const ATTACK_LAW: [TimePoint; 3] = [(0, 0), (115, octaves(6_644, 1_000)), (255, octaves(12_551, 1_000))];
/// The decay's and the release's law: 1 ms at 0 and 10 s at 10 (log₂
/// 10,000 = 13.288), with the middle point at the attack's share of the
/// whole.
///
/// Source: the specifications for the ends. Own choice: the attack's
/// shape, as the three sliders and the chip's three inputs are alike.
pub const FALL_LAW: [TimePoint; 3] = [(0, 0), (115, octaves(7_034, 1_000)), (255, octaves(13_288, 1_000))];

/// A stage's time in millionths of a second for a slider's position.
#[must_use]
pub fn time(law: &[TimePoint; 3], slider: u8) -> u32 {
    let [first, middle, last] = *law;
    let (from, to) = if slider <= middle.0 { (first, middle) } else { (middle, last) };
    let along = i32::from(slider).saturating_sub(i32::from(from.0));
    let span = i32::from(to.0).saturating_sub(i32::from(from.0));
    raise(TIME_BASE, from.1.saturating_add(share_of(to.1.saturating_sub(from.1), along, span)))
}

/// Keys the envelopes' key follow has a figure for, from MIDI key 0.
const KEY_FOLLOW_KEYS: usize = 84;
/// One in the fixed point of [`KEY_FOLLOW`].
pub const KEY_FOLLOW_ONE: u32 = 1 << 12;
/// How much longer an envelope's attack, decay and release are with its
/// KEY FOLLOW on, for each key from MIDI key 0, in Q12; a key above the
/// table has [`KEY_FOLLOW_ONE`], the slider's own time.
///
/// Source: service notes, the module controller's schematic (ENV KF OSC
/// and ENV KF SW) and the module board's (KF CONT, ENV), read with
/// `AMSynths`, "Roland IR3R01 Design & Replica":
///
/// - A 555 (IC50) resets a capacitor, C70 3,300 pF, that charges through
///   R199, 8.2 kΩ, from the emitter of TR32, whose base stands between
///   R201 and R202, 15 kΩ each, from the supply to ground; the schematic
///   draws the result as a rounded sawtooth
///   from 0 V to 7.5 V with a period of 35 µs (`AMSynths`: "a 28 kHz saw
///   wave"). The switch IC40 passes it to an envelope's circuit when that
///   envelope's key follow is on, and grounds the line when it is off.
/// - In each voice a comparator (IC38A, IC35A) has the sawtooth on its
///   inverting input through 10 kΩ, and on the other the voice's key
///   voltage through R208A, 27 kΩ, with R211A, 56 kΩ, to the supply. Its
///   output, through a transistor that inverts it, is the chip's pin 1,
///   "MODE INPUT"; the schematic marks that pin "ENV2 KF ON" with a
///   square wave of 0 V and 15 V, and "K.F Off = GND level".
///
/// What is inferred, since no document says what pin 1 does:
///
/// - With the key follow off the pin is at ground and the envelope has
///   the times of its sliders; so the pin at ground lets the envelope
///   run, and the pin high is taken to hold it. The envelope then runs
///   for the share d of each 35 µs in which the sawtooth is below the
///   comparator's other input, and its times are longer by 1 / d.
/// - The sawtooth is taken as the capacitor's charge, 1 - e^(-t / RC)
///   with RC = 27.06 µs, reaching the drawn 7.5 V at the drawn 35 µs.
///   That asks for a charge towards 10.3 V, where TR32's divider offers
///   half the supply less a junction; had the drawn 7.5 V been what the
///   capacitor charges towards, it would stand near 5 V after 35 µs and
///   the comparator's other input, as taken below, would be above it on
///   every key but the lowest. The drawn figures are kept, and this
///   inference is the weakest of the three.
/// - The key voltage is taken as 1 V an octave from 0 V at the lowest
///   key, as the interface board's converter is trimmed (adjustment 4:
///   C0 0.000 V, C5 5.000 V), and the supply as 15 V: the comparator's
///   input is 4.88 V at the lowest key and passes 7.5 V, where d is one,
///   47 semitones higher.
///
/// So with the key follow on the times are twice the sliders' at the
/// lowest key, 1.4 times at middle C and the sliders' own from the
/// eleventh key from the top: shorter "in proportion to the height of
/// any pitch", as the owner's manual has it (printed page 3), by lengthening the low
/// keys' and not by shortening the high keys'. The figures are computed
/// by `validation/derive_tables.py` of the reference folder, and
/// `tests/synth8_envelope.rs` checks each against the formula.
pub const KEY_FOLLOW: [u16; KEY_FOLLOW_KEYS] = [
    16384, 16011, 15651, 15305, 14971, 14649, 14338, 14038, 13748, 13467, 13195, 12933, //
    12678, 12431, 12192, 11960, 11734, 11515, 11303, 11096, 10895, 10699, 10509, 10324, //
    10143, 9968, 9796, 9629, 9466, 9307, 9152, 9000, 8852, 8707, 8566, 8428, //
    8292, 8160, 8030, 7904, 7780, 7658, 7539, 7422, 7308, 7196, 7086, 6978, //
    6872, 6768, 6666, 6566, 6468, 6371, 6277, 6183, 6092, 6001, 5913, 5825, //
    5740, 5655, 5572, 5490, 5410, 5330, 5252, 5175, 5099, 5024, 4950, 4877, //
    4805, 4734, 4664, 4595, 4527, 4460, 4393, 4327, 4262, 4198, 4134, 4096,
];

/// A stage's time on a key with the key follow on.
#[must_use]
pub fn key_followed(time: u32, key: u8) -> u32 {
    const ONE: u64 = KEY_FOLLOW_ONE as u64;
    let factor = KEY_FOLLOW.get(usize::from(key)).map_or(KEY_FOLLOW_ONE, |&factor| u32::from(factor));
    u32::try_from(u64::from(time).saturating_mul(u64::from(factor)) / ONE).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects, reason = "a test, in f64")]
mod tests {
    use super::{ATTACK_LAW, FALL_LAW, OCTAVE, raise, raise_step, time};
    use crate::analog::STEP_LIMIT;

    #[test]
    fn raising_is_the_power_of_two_to_a_twentieth_of_a_cent() {
        let mut worst = 0.0f64;
        for octaves in (-12 * OCTAVE..12 * OCTAVE).step_by(250_007) {
            let ratio = (f64::from(octaves) / f64::from(OCTAVE)).exp2();
            let found = f64::from(raise(1_000_000, octaves));
            // Whole units are all a result has.
            worst = worst.max(((found - 1.0e6 * ratio).abs() - 1.0).max(0.0) / (1.0e6 * ratio));
            let step = 1u32 << 20;
            let exact = (f64::from(step) * ratio).min(f64::from(STEP_LIMIT));
            let found = f64::from(raise_step(step, octaves));
            if exact > 1.0e4 {
                worst = worst.max(((found - exact).abs() - 1.0).max(0.0) / exact);
            }
        }
        assert!(worst < 0.05 * 2.0f64.ln() / 1200.0, "{worst}");
        assert_eq!((raise(5, 40 * OCTAVE), raise(u32::MAX, -70 * OCTAVE), raise(8, -3 * OCTAVE)), (u32::MAX, 0, 1));
        assert_eq!((raise_step(5, 40 * OCTAVE), raise_step(u32::MAX, -70 * OCTAVE)), (STEP_LIMIT, 0));
    }

    #[test]
    fn the_times_keep_their_documented_points() {
        let near = |found: u32, wanted: f64| (f64::from(found) / wanted - 1.0).abs() < 0.002;
        assert!(near(time(&ATTACK_LAW, 0), 1.0e3) && near(time(&ATTACK_LAW, 115), 1.0e5));
        assert!(near(time(&ATTACK_LAW, 255), 6.0e6) && near(time(&FALL_LAW, 255), 1.0e7));
    }
}
