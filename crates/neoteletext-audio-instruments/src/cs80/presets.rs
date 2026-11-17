//! The 22 presets: the tones that the tone selector's buttons recall,
//! eleven for each channel.
//!
//! The instrument holds each preset as a column of resistors on one of
//! four boards, T51 to T54. A button puts a voltage on its column; each
//! of the column's parts, a resistor and a diode in series, feeds the row
//! of one control, which a 10 kΩ resistor ties to earth. A row with a
//! diode alone stands at the full ten volts, a row with a resistor R at
//! 10 V × 10 kΩ / (R + 10 kΩ), and a row with no part at nothing. The
//! rows are the 26 controls of a tone in the panel's order; rows 4 and 5,
//! the two waveform switches, have a diode or nothing.
//!
//! A lever of a [`Tone`] here is 127 times that voltage's share of ten
//! volts, counted the way the lever is: the time levers and the two
//! resonance levers count down from ten volts, because the chips do (a
//! higher voltage is a shorter time, and 10 V is the least resonance:
//! service manual, page 42, VCF-EG pin 12 and VCF pin 7), and every
//! other lever counts up.
//!
//! Source: service manual, pages 31, 33, 35 and 37, the circuit diagrams
//! of T51 to T54, every part read from the scans; the comment at each
//! preset names its board and column. Own reading: which board and
//! column is which preset (the boards in the order of their numbers, the
//! columns in the order of the buttons, channel I on T51 and T52, channel
//! II on T53 and T54), that the rows are in the panel's order, and which
//! rows count down. The reading was checked against the owner's manual,
//! pages 28 to 30, which draws a panel setting for each preset "as a
//! good point of departure": taking each lever's place from those
//! drawings, the mean difference over the presets is least, for every
//! row, with the order and the directions used here.
//!
//! What could not be read with certainty, or was changed on the drawings:
//!
//! - **Revisions.** The drawings carry revision marks. A value struck
//!   through with another written beside it is taken as the other (T51 E
//!   and F row 22, T52 B row 9 and C row 22, T53 E row 23, T54 C row 22);
//!   a diode drawn dashed or struck through is taken as removed (rows 25
//!   and 26, the pressure levers, of ten presets, and T54 C row 25).
//! - **Doubtful figures**, each the likeliest reading: T53 B row 14 (75
//!   kΩ) and row 15 (3.9 kΩ); T52 D row 13 (510 Ω), row 14 (3 kΩ) and row
//!   25 (390 Ω).
//! - **The diode's own drop** is taken as made up by the column's supply:
//!   a row with a diode alone is taken as the full ten volts.
//! - **The owner's manual's drawings differ** from these columns in
//!   places, most of all in the filter envelope's levels of BRASS 1 and
//!   in the filters of FLUTE; the resistors are followed.

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
            0, 0, 0, 0, 1, 0, 76, 46, 115, 0, 49, 93, 95, 69, 14, 127, 6, 109, 113, 113, 15, 127, 58, 127, 0, 127,
        ]),
    ),
    // T51, column B (page 31).
    (
        "STRING 3",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 0, 0, 127, 0, 0, 84, 112, 101, 67, 127, 0, 119, 127, 73, 61, 127, 45, 127, 0, 127,
        ]),
    ),
    // T51, column C (page 31).
    (
        "BRASS 1",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 0, 0, 84, 51, 24, 55, 109, 106, 0, 127, 6, 57, 101, 78, 0, 127, 45, 127, 64, 127,
        ]),
    ),
    // T51, column D (page 31).
    (
        "FLUTE",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 84, 76, 73, 95, 66, 9, 82, 106, 34, 127, 0, 87, 127, 66, 36, 127, 18, 127, 18, 127,
        ]),
    ),
    // T51, column E (page 31).
    (
        "ELECTRIC PIANO",
        Tone::from_levers([
            0, 8, 42, 1, 0, 0, 22, 41, 30, 69, 0, 70, 0, 82, 0, 127, 64, 32, 99, 7, 46, 58, 51, 127, 0, 0,
        ]),
    ),
    // T51, column F (page 31).
    (
        "CLAVICHORD 1",
        Tone::from_levers([
            0, 0, 89, 1, 1, 0, 78, 49, 89, 64, 0, 64, 0, 97, 32, 127, 0, 0, 85, 16, 0, 58, 14, 127, 0, 0,
        ]),
    ),
    // T52, column A (page 33).
    (
        "HARPSICHORD 1",
        Tone::from_levers([0, 0, 73, 1, 0, 0, 93, 0, 108, 17, 0, 0, 0, 0, 0, 127, 0, 0, 99, 0, 34, 127, 0, 127, 0, 0]),
    ),
    // T52, column B (page 33).
    (
        "ORGAN 1",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 0, 0, 64, 127, 0, 0, 0, 0, 0, 127, 0, 0, 127, 100, 0, 127, 0, 127, 0, 127,
        ]),
    ),
    // T52, column C (page 33).
    (
        "GUITAR 1",
        Tone::from_levers([
            0, 0, 55, 1, 0, 0, 60, 51, 89, 93, 6, 42, 0, 64, 0, 127, 0, 32, 78, 21, 46, 58, 40, 127, 0, 127,
        ]),
    ),
    // T52, column D (page 33).
    (
        "FUNKY 1",
        Tone::from_levers([
            45, 11, 78, 1, 1, 0, 70, 72, 32, 67, 127, 109, 6, 29, 72, 127, 66, 0, 67, 64, 29, 127, 76, 127, 122, 127,
        ]),
    ),
    // T52, column E (page 33).
    (
        "FUNKY 3",
        Tone::from_levers([
            73, 10, 70, 1, 0, 0, 73, 127, 0, 127, 127, 127, 127, 127, 127, 127, 93, 0, 127, 113, 41, 127, 127, 127, 49,
            127,
        ]),
    ),
    // T53, column A (page 35).
    (
        "STRING 2",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 78, 0, 118, 0, 0, 0, 0, 0, 0, 127, 0, 23, 105, 113, 41, 127, 40, 127, 0, 127,
        ]),
    ),
    // T53, column B (page 35).
    (
        "STRING 4",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 34, 0, 109, 0, 86, 32, 57, 112, 36, 127, 0, 72, 0, 113, 27, 108, 40, 127, 55, 127,
        ]),
    ),
    // T53, column C (page 35).
    (
        "BRASS 2",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 0, 0, 89, 34, 66, 42, 117, 72, 0, 127, 0, 61, 99, 113, 0, 127, 40, 127, 64, 127,
        ]),
    ),
    // T53, column D (page 35).
    (
        "BRASS 3",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 34, 82, 55, 76, 34, 109, 76, 69, 36, 127, 0, 57, 72, 60, 43, 108, 40, 127, 64, 127,
        ]),
    ),
    // T53, column E (page 35).
    (
        "BASS",
        Tone::from_levers([
            0, 0, 73, 1, 0, 0, 0, 0, 58, 127, 9, 95, 0, 57, 18, 127, 0, 38, 97, 0, 0, 127, 40, 127, 0, 0,
        ]),
    ),
    // T53, column F (page 35).
    (
        "CLAVICHORD 2",
        Tone::from_levers([
            0, 11, 127, 1, 1, 0, 0, 0, 0, 0, 0, 127, 0, 87, 0, 127, 0, 0, 67, 24, 21, 58, 49, 127, 0, 0,
        ]),
    ),
    // T54, column A (page 37).
    (
        "HARPSICHORD 2",
        Tone::from_levers([0, 0, 73, 1, 0, 0, 100, 0, 108, 0, 0, 0, 0, 0, 0, 127, 0, 0, 78, 0, 0, 127, 0, 127, 0, 0]),
    ),
    // T54, column B (page 37).
    (
        "ORGAN 2",
        Tone::from_levers([0, 0, 0, 1, 0, 0, 0, 0, 78, 127, 0, 0, 0, 0, 0, 127, 0, 0, 46, 26, 0, 127, 0, 127, 0, 127]),
    ),
    // T54, column C (page 37).
    (
        "GUITAR 2",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 40, 127, 45, 127, 0, 102, 0, 99, 36, 127, 76, 0, 113, 4, 27, 58, 40, 127, 0, 0,
        ]),
    ),
    // T54, column D (page 37).
    (
        "FUNKY 2",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 73, 46, 89, 82, 91, 86, 78, 57, 0, 127, 0, 34, 72, 30, 0, 86, 70, 127, 0, 127,
        ]),
    ),
    // T54, column E (page 37).
    (
        "FUNKY 4",
        Tone::from_levers([
            0, 0, 0, 0, 1, 0, 76, 127, 78, 106, 0, 127, 67, 76, 0, 127, 127, 0, 67, 58, 0, 127, 58, 127, 81, 127,
        ]),
    ),
];
