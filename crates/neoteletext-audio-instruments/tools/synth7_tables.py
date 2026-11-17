"""Writes the constant tables of synth7 in
crates/neoteletext-audio-instruments (REQ-6412 items 5 and 6).

The crate is integer-only and has no build script, so every table that
needs a logarithm, a sine or a power is computed here in floating point and
written into the source as integers. The crate's tests recompute each table
from its formula and compare (tests/synth7_tables.rs).

Run it from the repository's root, once for each file:

    python3 crates/neoteletext-audio-instruments/tools/synth7_tables.py own > crates/neoteletext-audio-instruments/src/synth7/tables.rs
    python3 crates/neoteletext-audio-instruments/tools/synth7_tables.py msfa > crates/neoteletext-audio-instruments/src/synth7/msfa/tables.rs

`own` writes the sound kernel's tables, which are this project's own work
from the published description of the instrument's sound chip. `msfa`
writes the two tables that Google's music-synthesizer-for-android builds
with floating point when it starts (freqlut.cc) or computes with floating
point for each note (dx7note.cc); that file stays under Apache-2.0.
"""
import math
import sys

# Entries in the quarter-wave log-sine table and in the exponent table: the
# sound chip's ROMs have a 10-bit address.
ROM_ENTRIES = 1024
# Units of the chip's logarithms: 1/1024 of a doubling.
LOG_ONE = 1024
# The exponent ROM's mantissa: 12 bits with the leading one.
EXPONENT_ONE = 2048
# The chip rounds its log-sine values with this constant, not 0.5.
LOG_SINE_ROUNDING = 0.5002
# The largest total attenuation of the kernel in LOG_ONE units: 16 doublings.
SILENT = 16 * LOG_ONE
# MIDI's largest controller value.
MIDI_MAX = 127

# Samples a second rendered.
SAMPLE_RATE = 48000
# freqlut.cc: LG_N_SAMPLES, N_SAMPLES and MAX_LOGFREQ_INT.
FREQUENCY_ENTRIES = 1024
FREQUENCY_MAX_INTEGER = 20
# dx7note.cc osc_freq: (1 << 24) / log(2).
FINE_SCALE = 24204406.323123

# The notice of freqlut.cc and dx7note.cc, less the spaces that end three
# of its lines, which the formatter removes.
APACHE_HEADER = """/*
 * Copyright 2012 Google Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */"""


def log_sine():
    values = []
    for n in range(ROM_ENTRIES):
        angle = (n + 0.5) / ROM_ENTRIES * math.pi / 2
        values.append(int(-math.log2(math.sin(angle)) * LOG_ONE + LOG_SINE_ROUNDING))
    return values


def exponent():
    return [round(2 ** (n / ROM_ENTRIES) * EXPONENT_ONE) for n in range(ROM_ENTRIES)]


def volume():
    values = [SILENT]
    for value in range(1, MIDI_MAX + 1):
        values.append(min(SILENT, round(2 * math.log2(MIDI_MAX / value) * LOG_ONE)))
    return values


def frequency():
    base = (1 << (24 + FREQUENCY_MAX_INTEGER)) / SAMPLE_RATE
    return [math.floor(base * 2 ** (i / FREQUENCY_ENTRIES) + 0.5) for i in range(FREQUENCY_ENTRIES + 1)]


def fine():
    return [math.floor(FINE_SCALE * math.log(1 + 0.01 * value) + 0.5) for value in range(100)]


def table(name, kind, values, per_line):
    print("#[rustfmt::skip]")
    print(f"pub const {name}: [{kind}; {len(values)}] = [")
    for start in range(0, len(values), per_line):
        # Thousands are separated, as the workspace's lints ask of a long literal.
        print("    " + ", ".join(format(v, "_") for v in values[start:start + per_line]) + ",")
    print("];")


def own():
    print("//! The sound kernel's constant tables, written by `crates/neoteletext-audio-instruments/tools/synth7_tables.py`:")
    print("//! do not edit. `tests/synth7_tables.rs` recomputes each from its formula.")
    print()
    print("/// Entries in [`LOG_SINE`] and in [`EXPONENT`].")
    print("///")
    print("/// Source: Ken Shirriff, \"synth7 reverse-engineering, part III: Inside")
    print("/// the log-sine ROM\" and \"part II: the exponential ROM\" (righto.com,")
    print("/// December and November 2021): both ROMs take a 10-bit address.")
    print(f"pub const ROM_ENTRIES: usize = {ROM_ENTRIES};")
    print()
    print("/// One doubling in the units of [`LOG_SINE`]: its values are logarithms to")
    print("/// base two with ten fractional bits.")
    print("///")
    print("/// Source: Shirriff, part III: \"represented as the integer")
    print("/// round(y×1024)\".")
    print(f"pub const LOG_ONE: u32 = {LOG_ONE};")
    print()
    print("/// A quarter of a sine wave as attenuation: entry `n` is")
    print("/// `int(-log2(sin((n + 0.5) / 1024 × π/2)) × 1024 + 0.5002)`.")
    print("///")
    print("/// Source: Shirriff, part III, which gives the formula and the rounding")
    print("/// constant 0.5002 that makes it equal the ROM in every entry.")
    table("LOG_SINE", "u16", log_sine(), 16)
    print()
    print("/// Two to the power of a fraction: entry `n` is `round(2^(n/1024) × 2048)`,")
    print("/// twelve bits with the leading one.")
    print("///")
    print("/// Source: Shirriff, part II, note 2: \"The 12-bit output value from the")
    print("/// ROM, after the delta adder and appending a leading 1 bit, is exactly")
    print("/// given by round(2^frac×2048)\".")
    table("EXPONENT", "u16", exponent(), 16)
    print()
    print("/// The attenuation of a part's volume (MIDI controller 7), in")
    print("/// [`LOG_ONE`] units: entry `v` is `round(2 × log2(127 / v) × 1024)`, and")
    print(f"/// entry 0 is {SILENT}, at which the kernel is silent.")
    print("///")
    print("/// Source: own choice. Amplitude follows the square of the controller")
    print("/// value, the usual law for a volume controller, and it is applied as a")
    print("/// logarithm so that the kernel needs no multiplication for it.")
    table("VOLUME_ATTENUATION", "u16", volume(), 16)


def msfa():
    print(APACHE_HEADER)
    print()
    print("// This file was translated to Rust from freqlut.cc and dx7note.cc of")
    print("// music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android)")
    print("// at commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff, by `crates/neoteletext-audio-instruments/tools/synth7_tables.py`,")
    print("// which writes it: do not edit. What was changed: the table that")
    print("// `Freqlut::init` builds in floating point when the program starts is a")
    print("// constant here, computed for 48,000 samples a second by the closed form")
    print("// of the loop's product; the logarithm that `osc_freq` computes in floating")
    print("// point for the fine frequency of each note is a constant table of its")
    print("// hundred possible values.")
    print()
    print("//! Constant tables in place of the floating point of the original,")
    print("//! written by `crates/neoteletext-audio-instruments/tools/synth7_tables.py`: do not edit.")
    print()
    print("/// The phase a sine advances in one sample at 48,000 samples a second,")
    print("/// for a frequency of `2^(20 + i/1024)` Hz, where a whole turn is 2^24:")
    print("/// `floor(2^44 / 48000 × 2^(i/1024) + 0.5)`.")
    print("///")
    print("/// Source: music-synthesizer-for-android `freqlut.cc`, `lut` as")
    print("/// `Freqlut::init(48000)` fills it.")
    table("FREQUENCY", "u32", frequency(), 8)
    print()
    print("/// The fine frequency setting as a logarithm of frequency, one octave")
    print("/// being 2^24: entry `fine` is")
    print("/// `floor(24204406.323123 × ln(1 + 0.01 × fine) + 0.5)`.")
    print("///")
    print("/// Source: music-synthesizer-for-android `dx7note.cc`, `osc_freq`.")
    table("FINE_LOGFREQ", "i32", fine(), 8)


if len(sys.argv) == 2 and sys.argv[1] == "own":
    own()
elif len(sys.argv) == 2 and sys.argv[1] == "msfa":
    msfa()
else:
    sys.exit(__doc__)
