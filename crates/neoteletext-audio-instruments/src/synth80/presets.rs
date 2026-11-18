//! The 22 presets: the tones that the tone selector's buttons recall,
//! eleven for each channel.
//!
//! The instrument holds each preset as a column of resistors on one of
//! four boards, T51 to T54. A button puts a voltage on its column; each
//! of the column's parts, a resistor and a diode in series, feeds the row
//! of one control, which a 10 kΩ resistor ties to earth. A row with a
//! diode alone stands at the full ten volts, a row with a resistor R at
//! 10 V × L / (R + L), where L is what the row is loaded with, and a row
//! with no part at nothing. The rows are the 26 controls of a tone in the
//! panel's order; rows 4 and 5, the two waveform switches, have a diode
//! or nothing, and are written as a switch's canonical values, 127 on
//! and 0 off (S80-14).
//!
//! The load L is the row's own 10 kΩ beside whatever the row drives in
//! the eight voices. For the two cutoff rows that is known: eight times
//! 39 kΩ in series with the 2.7 kΩ or 3.3 kΩ at the filter's control
//! input, which with the 10 kΩ is 3.43 kΩ and 3.46 kΩ; for the two
//! resonance rows, eight times 220 kΩ, 7.33 kΩ in all. Every other row
//! is taken as loaded by its 10 kΩ alone.
//!
//! A lever of a [`Tone`] here is 127 times that voltage's share of ten
//! volts, counted the way the lever is: the time levers and the two
//! resonance levers count down from ten volts, because the chips do (a
//! higher voltage is a shorter time, and 10 V is the least resonance:
//! service manual, page 42, VCF-EG pin 12 and VCF pin 7), and every
//! other lever counts up.
//!
//! Source: service manual, pages 31, 33, 35 and 37, the circuit diagrams
//! of T51 to T54, every part read from the scans (the comment at each
//! preset names its board and column); pages 39 and 41 for what loads
//! the filters' rows. Own reading: which board and column is which
//! preset (the boards in the order of their numbers, the columns in the
//! order of the buttons, channel I on T51 and T52, channel II on T53 and
//! T54), that the rows are in the panel's order, which rows count down,
//! and that the rows whose loads were not read have none.
//!
//! The reading was compared with the owner's manual, pages 28 to 30,
//! which draws a panel setting for each preset "as a good point of
//! departure" (not the presets themselves: the drawings and the boards
//! differ). Taking each drawn lever's place from the scan for 21 of the
//! presets, and for each drawn lever the mean difference from every row
//! in either direction: the row and the direction used here are the best
//! or within 0.02 of the best for 21 of the 24 levers, and the cutoff
//! rows' loads above halve their difference. Three are not borne out
//! that way: row 11 as the initial level (the drawings mostly show it
//! equal to the attack level, the boards lower), row 22 as LEVEL, and
//! row 24 as the touch response's INITIAL LEVEL, which is a diode alone,
//! the lever at 10, in every preset. One recording of a real instrument
//! bears out the low-pass row's load: BRASS 1's filter on middle C comes
//! to rest near 1 kHz, where this reading puts it at 2 kHz and the same
//! resistor without the load at 17 kHz.
//!
//! What could not be read with certainty, or was changed on the drawings:
//!
//! - **Revisions.** The drawings carry revision marks. A value struck
//!   through with another written beside it is taken as the other (T51 E
//!   and F row 22, T52 B row 9 and C row 22, T53 E row 23, T54 C row 22);
//!   a diode drawn dashed or struck through is taken as removed (rows 25
//!   and 26, the pressure levers, of the presets that die away, and T54
//!   C row 25).
//! - **Doubtful figures**, each the likeliest reading: T53 B row 14 (75
//!   kΩ) and row 15 (3.9 kΩ); T52 D row 13 (510 Ω), row 14 (3 kΩ) and row
//!   25 (390 Ω); T54 C row 20 (330 kΩ). T51 D row 7, FLUTE's high-pass
//!   row, is written "51K" with no point, where its neighbours' points
//!   are plain, and is taken as 51 kΩ; as 5.1 kΩ the filter would stand
//!   on the fourth harmonic of every note.
//! - **The diode's own drop** is made up by the column's supply: a row
//!   with a diode alone is taken as the full ten volts. Service manual,
//!   page 27, BA circuit diagram: each column is driven by a transistor
//!   whose collector is on a rail marked "+10.6", a diode's drop above
//!   ten volts. The drop is therefore not what makes BRASS 1 brighter
//!   than the recorded instrument ([`super`], "What is not here").
//! - **One 10 kΩ for each row of a channel.** They are on T51 (page 31);
//!   T52's rows join T51's and have none (page 33, whose primed
//!   terminals are struck out). T53 and T54 are taken to be alike.

use super::patch::Tone;

/// How many presets there are.
///
/// Source: service manual, page 1, Specifications, "Tone selectors":
/// eleven names under each channel before the memories and the panel.
pub const PRESET_COUNT: usize = 22;

/// The presets, each with the name on its button: channel I's eleven in
/// the order of its buttons, then channel II's.
pub const PRESETS: [(&str, Tone); PRESET_COUNT] = [
    // T51, column A (page 31).
    (
        "STRING 1",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 43, 55, 99, 0, 49, 93, 95, 69, 14, 127, 6, 109, 113, 113, 15, 127, 58, 127, 0, 127,
        ]),
    ),
    // T51, column B (page 31).
    (
        "STRING 3",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 0, 0, 127, 0, 0, 84, 112, 101, 67, 127, 0, 119, 127, 73, 61, 127, 45, 127, 0, 127,
        ]),
    ),
    // T51, column C (page 31).
    (
        "BRASS 1",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 0, 0, 51, 61, 24, 55, 109, 106, 0, 127, 6, 57, 101, 78, 0, 127, 45, 127, 64, 127,
        ]),
    ),
    // T51, column D (page 31).
    (
        "FLUTE",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 8, 85, 40, 102, 66, 9, 82, 106, 34, 127, 0, 87, 127, 66, 36, 127, 18, 127, 18, 127,
        ]),
    ),
    // T51, column E (page 31).
    (
        "ELECTRIC PIANO",
        Tone::from_levers([
            0, 8, 42, 127, 0, 0, 9, 50, 12, 79, 0, 70, 0, 82, 0, 127, 64, 32, 99, 7, 46, 58, 51, 127, 0, 0,
        ]),
    ),
    // T51, column F (page 31).
    (
        "CLAVICHORD 1",
        Tone::from_levers([
            0, 0, 89, 127, 127, 0, 45, 58, 57, 73, 0, 64, 0, 97, 32, 127, 0, 0, 85, 16, 0, 58, 14, 127, 0, 0,
        ]),
    ),
    // T52, column A (page 33).
    (
        "HARPSICHORD 1",
        Tone::from_levers([0, 0, 73, 127, 0, 0, 62, 0, 84, 22, 0, 0, 0, 0, 0, 127, 0, 0, 99, 0, 34, 127, 0, 127, 0, 0]),
    ),
    // T52, column B (page 33).
    (
        "ORGAN 1",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 0, 0, 33, 127, 0, 0, 0, 0, 0, 127, 0, 0, 127, 100, 0, 127, 0, 127, 0, 127,
        ]),
    ),
    // T52, column C (page 33).
    (
        "GUITAR 1",
        Tone::from_levers([
            0, 0, 55, 127, 0, 0, 30, 61, 57, 100, 6, 42, 0, 64, 0, 127, 0, 32, 78, 21, 46, 58, 40, 127, 0, 127,
        ]),
    ),
    // T52, column D (page 33).
    (
        "FUNKY 1",
        Tone::from_levers([
            45, 11, 78, 127, 127, 0, 37, 81, 13, 76, 127, 109, 6, 29, 72, 127, 66, 0, 67, 64, 29, 127, 76, 127, 122,
            127,
        ]),
    ),
    // T52, column E (page 33).
    (
        "FUNKY 3",
        Tone::from_levers([
            73, 10, 70, 127, 0, 0, 40, 127, 0, 127, 127, 127, 127, 127, 127, 127, 93, 0, 127, 113, 41, 127, 127, 127,
            49, 127,
        ]),
    ),
    // T53, column A (page 35).
    (
        "STRING 2",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 45, 0, 104, 0, 0, 0, 0, 0, 0, 127, 0, 23, 105, 113, 41, 127, 40, 127, 0, 127,
        ]),
    ),
    // T53, column B (page 35).
    (
        "STRING 4",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 14, 0, 87, 0, 86, 32, 57, 112, 36, 127, 0, 72, 0, 113, 27, 108, 40, 127, 55, 127,
        ]),
    ),
    // T53, column C (page 35).
    (
        "BRASS 2",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 0, 0, 57, 42, 66, 42, 117, 72, 0, 127, 0, 61, 99, 113, 0, 127, 40, 127, 64, 127,
        ]),
    ),
    // T53, column D (page 35).
    (
        "BRASS 3",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 14, 90, 27, 85, 34, 109, 76, 69, 36, 127, 0, 57, 72, 60, 43, 108, 40, 127, 64, 127,
        ]),
    ),
    // T53, column E (page 35).
    (
        "BASS",
        Tone::from_levers([
            0, 0, 73, 127, 0, 0, 0, 0, 28, 127, 9, 95, 0, 57, 18, 127, 0, 38, 97, 0, 0, 127, 40, 127, 0, 0,
        ]),
    ),
    // T53, column F (page 35).
    (
        "CLAVICHORD 2",
        Tone::from_levers([
            0, 11, 127, 127, 127, 0, 0, 0, 0, 0, 0, 127, 0, 87, 0, 127, 0, 0, 67, 24, 21, 58, 49, 127, 0, 0,
        ]),
    ),
    // T54, column A (page 37).
    (
        "HARPSICHORD 2",
        Tone::from_levers([0, 0, 73, 127, 0, 0, 71, 0, 84, 0, 0, 0, 0, 0, 0, 127, 0, 0, 78, 0, 0, 127, 0, 127, 0, 0]),
    ),
    // T54, column B (page 37).
    (
        "ORGAN 2",
        Tone::from_levers([
            0, 0, 0, 127, 0, 0, 0, 0, 45, 127, 0, 0, 0, 0, 0, 127, 0, 0, 46, 26, 0, 127, 0, 127, 0, 127,
        ]),
    ),
    // T54, column C (page 37).
    (
        "GUITAR 2",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 17, 127, 20, 127, 0, 102, 0, 99, 36, 127, 76, 0, 113, 4, 27, 58, 40, 127, 0, 0,
        ]),
    ),
    // T54, column D (page 37).
    (
        "FUNKY 2",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 40, 55, 57, 90, 91, 86, 78, 57, 0, 127, 0, 34, 72, 30, 0, 86, 70, 127, 0, 127,
        ]),
    ),
    // T54, column E (page 37).
    (
        "FUNKY 4",
        Tone::from_levers([
            0, 0, 0, 0, 127, 0, 43, 127, 45, 111, 0, 127, 67, 76, 0, 127, 127, 0, 67, 58, 0, 127, 58, 127, 81, 127,
        ]),
    ),
];
