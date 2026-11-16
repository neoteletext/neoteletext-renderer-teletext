# Sources record

The standards clauses and tables each module is written from (REQ-DOC-005 and REQ-156, REQ-GEN-012; the requirements are in rdm). Clause and table numbers are those of ETSI EN 300 706 V1.2.1 unless another document is named. Copies of the standards are kept outside the repository (`~/devel/teletext-reference/teletext-standards/`).

| Module | Sources |
|---|---|
| `neoteletext-core::consts` | Clauses 5.3, 6.1, 6.2, 7.1 (framing and packet layout), Table 1 (page numbers), clause 9.3.1.3 (control bits), Table 26 (spacing attributes), Table 30 (CLUT 0), clause 9.8.1 (service data); Table 9 (X/28/3 page function), Table 15 (the links of X/27/4 and X/27/5), Tables 20 and 22 (the Magazine Organization Table and its pattern page links), Table 24 (the Magazine Inventory Page's codes, clause 11.3.3), Table 25 (the undefined G3 positions); EN 300 472 clause 4.4 (bit order); decisions E.12, E.14, E.19 |
| `neoteletext-core::ecc` | Clause 8.1 (odd parity), 8.2 (Hamming 8/4), 8.3 (Hamming 24/18), 9.6.1 and Figure 13 (page check word) |
| `neoteletext-core::packet` | Clauses 7.1.2 (packet address), 9.3.1 (header), 9.3.2 (rows), 9.4.1 (X/26), 9.4.2 and Tables 3 and 4 (X/28/0 format 1), 9.4.2 and Table 10 (X/28/4), 9.5 and Tables 11 to 13 (M/29/0 and M/29/4), Table 15 (X/27/4 and X/27/5), Table 20 (the Magazine Organization Table), 9.6.1, Figure 12 and Table 14 (X/27/0), 9.8.1 and Table 18 (8/30) |
| `neoteletext-core::fsm`, `acq` | Clause 7.2.1, Annexes B.2 to B.4 and B.9 (page boundaries and completion); decisions E.6 to E.8, E.11, E.19, E.20 |
| `neoteletext-core::service` | Clause 9.8.1, Table 18; the date digit order verified against live packets (research/answers-teletext-sources.md section 2.1) |
| `neoteletext-core::charset` | Clause 15; Tables 32 and 33 (designation), 34 (the Modified G0 and G2 Character Set Designation), 35 to 46 (G0 and G2 sets), 47 (G1), 48 (G3); decisions E.4, E.21, E.26 |
| `neoteletext-core::level1` | Clause 12.2 and Table 26; Annexes C.3 and G.3; decisions E.10, E.15 to E.18, E.22, E.23 |
| `neoteletext-core::level15` | Clauses 12.3.1 to 12.3.4, Tables 27 to 29; Annexes A.3, A.4 and D.1.3; decisions E.1 to E.5, E.9 |
| `neoteletext-core::render`, `glyphs` | SAA5050 datasheet (character rounding, mosaic geometry, flash timing) as recorded in decisions E.12 to E.14; glyph data from Noto Teletext (DEC-1761, `third_party/noto/PROVENANCE.md`) |
| `neoteletext-core::nav`, `viewer` | Annex H (Fastext), clause 11.1; the TTI Link Extension (`docs/spec/design/tti-link-extension.md`); decisions E.10, E.36 |
| `neoteletext-core::policy` | RFC 1918, RFC 6598, RFC 3927, RFC 4193, RFC 4291 (blocked address ranges); area SEC of the requirements in rdm |
| `neoteletext-core::limits` | Annexes A.1 and A.10 (maxima), Annex E.15, clauses 7.1.2, 9.3.1.2 and 10.1; clause 11.3.3 and Table 24 (sub-pages a page may have); EN 300 708 clause 4.3.1.2 and Table 2 (the sub-code of a data broadcasting page); `neoteletext-common/STANDARD-LIMITS.md`; decisions E.122 and E.130 |
| `neoteletext-core::colour` | Clause 12.4, Tables 4 and 10 (the colour words of X/28/0 and X/28/4), Table 30 (the default Colour Map), Annex D.2.1 |
| `neoteletext-core::objects` | Clauses 10.5.1.2, 10.6.1, 13.1 and 13.8; Tables 15 and 16 (object links), 20 (the Magazine Organization Table) and 28 (object triplets); Annex A.8; decision E.51 |
| `neoteletext-core::enhance` | Clause 13.2; Tables 26 (spacing attributes), 28 and 29 (triplet functions), 34 (character set switching); Annexes A.7.1, A.8 and C.3 |
| `neoteletext-core::joining` | Table 44 (the Arabic set's joined forms, as the receiver joins letters); the Unicode joining types; decision E.79 (DEC-603) |
| `neoteletext-core::controls` | Table 29 (the reserved column mode the control triplet uses, DOC-1844); decision E.127 |
| `neoteletext-formats` | Clauses 5.2 and 7.1 (T42 packets); EN 300 472 clauses 4 to 4.4 (teletext in DVB), EN 301 775, EN 300 468 and ISO/IEC 13818-1 clauses 2.4.3.3, 2.4.3.6 and 2.4.4 (transport streams and PES); the TTI page file and its Link Extension as the protocol document defines them (`neoteletext-common/PROTOCOL.html`); the BDF format, Adobe's Glyph Bitmap Distribution Format 2.1, for `bdf` |
| `neoteletext-slicer` | Clauses 5.2, 5.4, 6.1, 6.3 and 7.1 of EN 300 706 (the data signal and the packet's clock run-in and framing code, as the slicer cites them) |
| `neoteletext-web` | RFC 9110 and RFC 9112 (HTTP), RFC 6455 (WebSocket), RFC 6265 (cookies), RFC 3986 (addresses), RFC 8259 (JSON), RFC 3174 (SHA-1), RFC 4648 (base64), RFC 1928 (SOCKS5), RFC 9651 (structured fields, for `dbsc`); the zxnet feed as recorded in `research/` |
| `neoteletext-opus` | RFC 6716 (the Opus codec) with its errata of RFC 8251, bit-exact with libopus 1.5.2's fixed-point decoder (decision E.123) |
| `neoteletext-image::png` | PNG specification (ISO/IEC 15948) sections 5, 9, 11; RFC 1950 (zlib) and RFC 1951 (deflate, fixed Huffman codes section 3.2.6) |
| `neoteletext-image::palette` | Table 30 (CLUT 0) |
| `neottx` iTerm2 output | iTerm2 inline images protocol (OSC 1337 `File=`), as documented by iTerm2; RFC 4648 (base64) |
| `neoteletext-audio::idl`, `neoteletext-core::consts` (independent data lines) | EN 300 708 V1.2.1 clauses 6.4.1 and 6.4.2 (designation and data channel, Table 16), 6.5.5 (continuity, for the record id), 6.5.8.1 (the 16-bit check), 6.7.1.1 and 6.7.3 (low bit-rate audio, Table 21); decision E.84 |
| `neoteletext-audio::ima` | The IMA's 1992 recommended practices (IMA ADPCM step and index tables, not held locally); Microsoft RIFF WAVE format tag 0x0011 (block layout) |
| `neoteletext-audio::record`, `store`, `control`, `receiver` | `docs/spec/design/audio.md` sections 4 to 8; RFC 1982 (serial number arithmetic); CRC-32/ISO-HDLC; decisions E.84 to E.86 |
| `neoteletext-audio-instruments::sid` | The MOS 6581 data sheet and the Commodore 64 Programmer's Reference Guide, appendix O; the interview with the chip's designer (1996); United States patent 4,677,890; published measurements of the chip, each named at the constant it gives (the module's own list is in `sid/mod.rs`); DEC-6418 |
| `neoteletext-audio-instruments::dx7` | Yamaha's DX7 manual (MIDI data format) and the 1993 transcription of its data-format sheet; the published reverse-engineering of the instrument's sound chip (the logarithmic sine and exponent tables, the algorithm and carrier-compensation tables); music-synthesizer-for-android's source and notes for `dx7::msfa`; DEC-6420 |
| `neoteletext-audio-mix` | EN 300 708 Table 19 (ducking steps); the Kaiser-windowed sinc of `tools/sinc_table.py`; decision E.85 |
