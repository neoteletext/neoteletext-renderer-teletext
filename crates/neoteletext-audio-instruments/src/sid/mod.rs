//! The SID chip, MOS 6581 and 8580 (REQ-6412 item 4): the Commodore 64's
//! sound chip, emulated from its registers to 16-bit samples at
//! [`SAMPLE_RATE`](crate::SAMPLE_RATE).
//!
//! A [`Sid`] takes register writes, as a program on the machine would make
//! them, and renders samples; it advances the chip by exactly the chip
//! clock's cycles for each second rendered. Any byte in any register is
//! valid, nothing here panics or allocates, and a sample costs the same
//! bounded work whatever the registers hold: at most six steps of the
//! analogue half, twenty-four chip cycles and one pass of the output
//! filter.
//!
//! # How it is made
//!
//! - **Every chip cycle**: the three oscillators and their waveforms, the
//!   three envelope generators, each voice's level, and the first stage of
//!   the decimator.
//! - **Every fourth cycle**, the decimator's intermediate rate: the filter,
//!   the mixer and volume, and the board's output stage. The filter is
//!   linear but for the 6581's own distortion, so it may follow the first
//!   low-pass; a quarter of the clock is five times the output rate and
//!   twenty times the highest cutoff.
//! - **Every output sample**: the second stage of the decimator, a 160-tap
//!   low-pass ([`fir_table`]) read at the sample's exact instant.
//!
//! # Whose work this is
//!
//! It is written from public documents about the chip, with no code and
//! no table from any emulator (DEC-6418). Every constant says, where it is
//! defined, which document it comes from, or that it is an own choice and
//! why. The documents:
//!
//! - MOS Technology, "6581 Sound Interface Device (SID)" data sheet, and
//!   its text as appendix O of the "Commodore 64 Programmer's Reference
//!   Guide" (1982): <https://www.zimmers.net/cbmpics/cbm/c64/c64prg.txt>,
//!   and the transcription at
//!   <https://www.bellesondes.fr/wiki/doku.php?id=mos6581>.
//! - Andreas Varga, "Interview with Bob Yannes" (August 1996), the chip's
//!   designer on its architecture:
//!   <http://www.landley.net/history/mirror/commodore/yannes.html>.
//! - US patent 4,677,890, "Sound interface circuit" (Yannes, Commodore),
//!   for the register descriptions and the envelope's divider:
//!   <https://patents.google.com/patent/US4677890A/en>.
//! - Asger Alstrup, "Examination of SID noise waveform" and "Examination
//!   of SID triangle waveform", measurements through register $1B:
//!   <http://www.sidmusic.org/sid/sidtech5.html> and `sidtech4.html`.
//! - Kevin Horton, "New SID ADSR Info!" (2 August 2008), the envelope's
//!   rate table read from the die:
//!   <https://web.archive.org/web/2012/http://blog.kevtris.org/?p=13>.
//! - Plogue R&D, "SID 6581R3 ADSR tables, up close." (15 March 2010), the
//!   levels at which the falling envelope slows, recorded from a chip:
//!   <http://ploguechipsounds.blogspot.com/2010/03/sid-6581r3-adsr-tables-up-close.html>.
//! - Robin Harbron, Levente Harsfalvi and Stephen Judd, "The C64 Digi",
//!   C=Hacking 20, voltages measured at a 6581's output:
//!   <http://www.ffd2.com/fridge/chacking/c=hacking20.txt>.
//! - codebase64: "Classic hard-restart and about ADSR in generally", "A
//!   new kind of hard-restart", "Vicious Sid Demo Routine Explained" and
//!   "detecting sid type":
//!   <https://codebase64.net/doku.php?id=base:sid_programming>.
//! - oxyron.de, "SID 6581/8580 (Sound Interface Device) reference":
//!   <https://www.oxyron.de/html/registers_sid.html>.
//! - Commodore 64 schematic for assembly 250469, redrawn by Falk Brunner:
//!   <http://www.zimmers.net/anonftp/pub/cbm/schematics/computers/c64/250469-rev.A.pdf>.
//! - On how the two chips differ, in prose: Wikipedia, "MOS Technology
//!   6581"; <https://retroisle.com/general/sidchip.php>;
//!   <https://www.c64-wiki.com/wiki/SID>; Michiel Boland, "SID filter
//!   measurements", <http://www.michiel.boland.org/sid/filter_log_sweep.html>;
//!   and the forum thread "6581 filter curves",
//!   <https://csdb.dk/forums/?roomid=14&topicid=105719>.
//!
//! # What is not the chip
//!
//! No two 6581s sound alike, and this is one plausible chip, not a
//! measured one. The 6581 filter's cutoff curve, its highest resonance
//! and its level are fitted to the output of a reference program run
//! beside this one (DEC-6425), and so is the output level of both chips;
//! each such figure says so where it is defined. The 6581 filter's
//! distortion is an own model; so is what a 6581 does to combined
//! waveforms beyond the logical AND. That the 6581's cutoff rises with
//! the level of the signal in the filter is not modelled: its curve is
//! the one for a signal of middling level. The external audio input, the
//! two potentiometer registers and the chip's noise floor are not
//! modelled.

mod analogue;
mod decimator;
mod envelope;
pub mod fir_table;
mod oscillator;

use analogue::{Analogue, Levels};
use decimator::{CIC_RATIO, Cic, Resampler};
use envelope::Envelope;
use oscillator::{Oscillator, control};

/// The chip's writable registers, at addresses 0 to 24 (data sheet, Table
/// 1: seven for each voice, two of the cutoff, resonance with routing, and
/// mode with volume).
pub const REGISTERS: usize = 25;

/// Registers of one voice.
const VOICE_REGISTERS: u8 = 7;
/// The address of the first filter register, after the three voices.
const FILTER_REGISTERS: u8 = 3 * VOICE_REGISTERS;
/// The address that reads the upper eight bits of voice 3's waveform (data
/// sheet, register $1B, "OSC 3/RANDOM").
pub const OSCILLATOR_3: u8 = 0x1B;
/// The address that reads voice 3's envelope (data sheet, register $1C,
/// "ENV 3").
pub const ENVELOPE_3: u8 = 0x1C;

/// Which chip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChipModel {
    /// The MOS 6581 of the Commodore 64: a filter whose cutoff bends and
    /// which distorts, and offsets that make the volume register audible.
    Mos6581,
    /// The MOS 8580 of the Commodore 64C: a straight, clean filter with
    /// more resonance, and next to no offsets.
    Mos8580,
}

/// The clock the chip runs on, which sets its pitch and its timing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChipClock {
    /// A PAL Commodore 64.
    Pal,
    /// An NTSC Commodore 64.
    Ntsc,
}

/// The PAL machine's crystal in hertz, four times the PAL colour
/// subcarrier, and what divides it to the chip clock.
///
/// Source: oxyron.de SID reference, "Frequency": "PAL: x = f *
/// (18*2^24)/17734475". The schematic's note 3 gives the crystal as
/// "17,734472 MHz for PAL", the same to its sixth digit.
const PAL_CRYSTAL: (u32, u32) = (17_734_475, 18);
/// The NTSC machine's crystal in hertz, four times the NTSC colour
/// subcarrier, and what divides it to the chip clock.
///
/// Source: oxyron.de SID reference: "NTSC: x = f * (14*2^24)/14318182";
/// the schematic's note 3: "Y1 14,31818 MHz for NTSC".
const NTSC_CRYSTAL: (u32, u32) = (14_318_182, 14);
/// The two chip clocks in hertz.
const PAL_HZ: u32 = PAL_CRYSTAL.0 / PAL_CRYSTAL.1;
const NTSC_HZ: u32 = NTSC_CRYSTAL.0 / NTSC_CRYSTAL.1;

impl ChipClock {
    /// The clock in hertz: 985,248 for PAL and 1,022,727 for NTSC.
    #[must_use]
    pub const fn hz(self) -> u32 {
        match self {
            Self::Pal => PAL_HZ,
            Self::Ntsc => NTSC_HZ,
        }
    }
}

/// One of the chip's three voices.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Voice {
    oscillator: Oscillator,
    envelope: Envelope,
}

impl Voice {
    const fn new() -> Self {
        Self { oscillator: Oscillator::new(), envelope: Envelope::new() }
    }

    /// The voice's level in the cycle just run, in voice units: the
    /// waveform about its zero, times the envelope, on the voice's offset.
    /// A higher waveform value is a lower level, as measured on the chip.
    #[inline]
    fn level(&mut self, source_msb: bool, model: ChipModel, levels: Levels) -> i32 {
        let waveform = i32::from(self.oscillator.waveform(source_msb, model));
        let envelope = i32::from(self.envelope.level());
        levels.waveform_zero.wrapping_sub(waveform).wrapping_mul(envelope).wrapping_add(levels.voice_offset)
    }
}

/// Where a voice's level goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Route {
    /// Past the filter, to the mixer.
    Direct,
    /// Through the filter.
    Filter,
    /// Nowhere: voice 3, cut from the direct path.
    Off,
}

/// One SID chip, its board's output stage, and the decimator that brings
/// its output to [`SAMPLE_RATE`](crate::SAMPLE_RATE).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sid {
    model: ChipModel,
    clock: ChipClock,
    levels: Levels,
    voices: [Voice; 3],
    routes: [Route; 3],
    analogue: Analogue,
    /// The first decimator stage of the voices that go through the filter,
    /// and of those that go past it.
    filtered: Cic,
    direct: Cic,
    resampler: Resampler,
    cycles: u64,
}

impl Sid {
    /// A chip of `model` on `clock`, as its reset line leaves it: every
    /// register zero, so silent until the volume and a voice are written.
    #[must_use]
    pub const fn new(model: ChipModel, clock: ChipClock) -> Self {
        Self {
            model,
            clock,
            levels: Levels::of(model),
            voices: [Voice::new(), Voice::new(), Voice::new()],
            routes: [Route::Direct; 3],
            analogue: Analogue::new(model, clock),
            filtered: Cic::new(),
            direct: Cic::new(),
            resampler: Resampler::new(clock),
            cycles: 0,
        }
    }

    /// The chip's model.
    #[must_use]
    pub const fn model(&self) -> ChipModel {
        self.model
    }

    /// The chip's clock.
    #[must_use]
    pub const fn clock(&self) -> ChipClock {
        self.clock
    }

    /// Chip cycles run since the chip was made or reset. Rendering a
    /// second of samples runs the clock's hertz of them, to within the
    /// four cycles of one intermediate sample and with no drift.
    #[must_use]
    pub const fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Puts the chip, the output stage and the decimator back as
    /// [`Sid::new`] makes them.
    pub fn reset(&mut self) {
        *self = Self::new(self.model, self.clock);
    }

    /// Writes `value` to the register at `address`. An address of
    /// [`REGISTERS`] or above is not a writable register and is ignored.
    pub fn write(&mut self, address: u8, value: u8) {
        if address >= FILTER_REGISTERS {
            match address.wrapping_sub(FILTER_REGISTERS) {
                0 => self.analogue.write_cutoff_low(value),
                1 => self.analogue.write_cutoff_high(value),
                2 => self.analogue.write_resonance_routing(value),
                3 => self.analogue.write_mode_volume(value),
                _ => return,
            }
            self.route();
            return;
        }
        let [first, second, third] = &mut self.voices;
        let voice = match address / VOICE_REGISTERS {
            0 => first,
            1 => second,
            _ => third,
        };
        match address % VOICE_REGISTERS {
            0 => voice.oscillator.write_frequency_low(value),
            1 => voice.oscillator.write_frequency_high(value),
            2 => voice.oscillator.write_pulse_low(value),
            3 => voice.oscillator.write_pulse_high(value),
            4 => {
                voice.oscillator.write_control(value);
                voice.envelope.set_gate(value & control::GATE != 0);
            }
            5 => voice.envelope.write_attack_decay(value),
            _ => voice.envelope.write_sustain_release(value),
        }
    }

    /// Writes all the registers, register 0 first: one image of a stream
    /// of register images.
    pub fn write_image(&mut self, image: &[u8; REGISTERS]) {
        for (address, &value) in (0u8..).zip(image) {
            self.write(address, value);
        }
    }

    /// Reads the register at `address`: [`OSCILLATOR_3`] and [`ENVELOPE_3`]
    /// as of the cycle last run. Every other address reads zero here; the
    /// potentiometer registers are not modelled, and what the chip returns
    /// for a write-only register is left out.
    #[must_use]
    pub const fn read(&self, address: u8) -> u8 {
        let [_, _, third] = &self.voices;
        match address {
            OSCILLATOR_3 => third.oscillator.read(),
            ENVELOPE_3 => third.envelope.level(),
            _ => 0,
        }
    }

    /// Renders `out.len()` samples, advancing the chip by the clock's
    /// cycles for each [`SAMPLE_RATE`](crate::SAMPLE_RATE) of them.
    pub fn render(&mut self, out: &mut [i16]) {
        for sample in out {
            for _ in 0..self.resampler.advance() {
                self.intermediate();
            }
            *sample = self.resampler.output();
        }
    }

    /// Works out where each voice goes from the filter's registers (data
    /// sheet, registers $17 and $18).
    fn route(&mut self) {
        let off = self.analogue.voice_3_off();
        for (voice, route) in (0u8..).zip(&mut self.routes) {
            *route = if self.analogue.filters(voice) {
                Route::Filter
            } else if voice == 2 && off {
                Route::Off
            } else {
                Route::Direct
            };
        }
    }

    /// One intermediate sample: four chip cycles, then the analogue half.
    #[inline]
    fn intermediate(&mut self) {
        for _ in 0..CIC_RATIO {
            self.cycle();
        }
        self.cycles = self.cycles.wrapping_add(u64::from(CIC_RATIO));
        let sample = self.analogue.process(self.filtered.decimate(), self.direct.decimate());
        self.resampler.push(sample);
    }

    /// One chip cycle of the three voices.
    #[inline]
    fn cycle(&mut self) {
        let [first, second, third] = &mut self.voices;
        first.oscillator.advance();
        second.oscillator.advance();
        third.oscillator.advance();
        // Data sheet, voices 2 and 3: each voice is synchronised to, and
        // ring-modulated by, the one before it, and voice 1 by voice 3.
        let rose = (first.oscillator.msb_rose(), second.oscillator.msb_rose(), third.oscillator.msb_rose());
        first.oscillator.synchronise(rose.2);
        second.oscillator.synchronise(rose.0);
        third.oscillator.synchronise(rose.1);
        first.envelope.clock();
        second.envelope.clock();
        third.envelope.clock();

        let msb = (first.oscillator.msb(), second.oscillator.msb(), third.oscillator.msb());
        let levels = [
            first.level(msb.2, self.model, self.levels),
            second.level(msb.0, self.model, self.levels),
            third.level(msb.1, self.model, self.levels),
        ];
        let mut filtered = 0i32;
        let mut direct = 0i32;
        for (&level, &route) in levels.iter().zip(&self.routes) {
            match route {
                Route::Direct => direct = direct.wrapping_add(level),
                Route::Filter => filtered = filtered.wrapping_add(level),
                Route::Off => {}
            }
        }
        self.filtered.integrate(filtered);
        self.direct.integrate(direct);
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::unwrap_used, reason = "a test")]
mod tests {
    use super::{ChipClock, ChipModel, ENVELOPE_3, OSCILLATOR_3, Route, Sid};

    #[test]
    fn the_clocks_are_the_crystals_divided() {
        assert_eq!(ChipClock::Pal.hz(), 985_248);
        assert_eq!(ChipClock::Ntsc.hz(), 1_022_727);
    }

    #[test]
    fn the_registers_decode_to_their_voices() {
        let mut sid = Sid::new(ChipModel::Mos6581, ChipClock::Pal);
        // Voice 3: a sawtooth at the top frequency, gated at once to the
        // peak; the other voices silent.
        sid.write(14, 0xFF);
        sid.write(15, 0xFF);
        sid.write(20, 0xF0);
        sid.write(18, 0x21);
        let mut samples = [0i16; 480];
        sid.render(&mut samples);
        assert_eq!(sid.read(ENVELOPE_3), 0xFF);
        assert_eq!(sid.read(0), 0);
        let mut seen = [false; 2];
        for _ in 0..100 {
            sid.render(&mut samples[..1]);
            seen[usize::from(sid.read(OSCILLATOR_3) >> 7)] = true;
        }
        assert_eq!(seen, [true; 2]);
    }

    #[test]
    fn voices_are_routed_by_the_filter_registers() {
        let mut sid = Sid::new(ChipModel::Mos8580, ChipClock::Ntsc);
        assert_eq!(sid.routes, [Route::Direct; 3]);
        sid.write(23, 0x02);
        sid.write(24, 0x8F);
        assert_eq!(sid.routes, [Route::Direct, Route::Filter, Route::Off]);
        // A voice sent through the filter is not cut by "3 OFF".
        sid.write(23, 0x04);
        assert_eq!(sid.routes, [Route::Direct, Route::Direct, Route::Filter]);
        sid.write(24, 0x0F);
        sid.write(23, 0x0F);
        assert_eq!(sid.routes, [Route::Filter; 3]);
    }
}
