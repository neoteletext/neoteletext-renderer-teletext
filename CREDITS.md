# Credits

neoteletext is written from the teletext standards (ETSI EN 300 706 and the documents cited in
REQUIREMENTS.md). These outside works are part of it, or shaped it. Each is credited for what it
contributes, with its licence and how neoteletext uses it.

## Bundled

**Noto** — the Noto Project Authors and Google, <https://github.com/notofonts>
- *What it provides:* neoteletext's one font, Noto Teletext, the font of every page and of the
  receiver's own drawing (DEC-1761, REQ-FNT-001). `third_party/noto/` holds the fonts converted from
  the Noto sources to teletext's 12 by 20 cell, the converter and its settings; the sources themselves
  are downloaded pinned by checksum and never committed.
- *Licence:* the SIL Open Font License 1.1, which `third_party/noto/OFL.txt` reproduces with the
  copyright line of every source. No Reserved Font Name is declared, so the converted font may keep
  the name.
- *Effect on neoteletext's licence:* none. The OFL binds the font, which stays under it; a binary
  that holds the font must carry its licence and copyright lines.

**X11 misc-fixed 6x10** — the X11 misc-fixed authors, with the Unicode (ISO 10646-1) extensions by
Markus Kuhn and contributors, <https://www.cl.cam.ac.uk/~mgk25/ucs-fonts.html>
- *What it provides:* a terminal font covering all of code page 437 (the IBM PC's character set),
  with Latin, Greek, Cyrillic and Hebrew, loaded at run time (REQ-2223). `third_party/misc-fixed/`
  holds the original `6x10.bdf` and its conversion to the 12 by 20 cell, each pixel made two by two.
  It is not compiled in.
- *Licence:* public domain ("Public domain terminal emulator font.  Share and enjoy.").
- *Effect on neoteletext's licence:* none.

## Ported

**libopus 1.5.2** — Xiph.Org Foundation, Skype Limited, Octasic, Jean-Marc Valin, Timothy B.
Terriberry, CSIRO, Gregory Maxwell, Mark Borgerding, Erik de Castro Lopo, Mozilla and Amazon,
<https://opus-codec.org/>
- *What it provides:* the Opus decoder (RFC 6716, RFC 8251) that `neoteletext-opus` is ported from,
  function for function from the C of its fixed-point build (decision E.123): the range decoder,
  SILK, CELT and the Opus layer, so that the port's output is bit-exact with it (REQ-AUD-023).
- *Licence:* BSD-3-Clause. Its copyright notice and licence travel with the ported source
  (`crates/neoteletext-opus/LICENSE-libopus`, and the copyright line of each ported file) and must
  be reproduced in the documentation of any binary distribution. Opus is also covered by
  royalty-free patent licences from Xiph.Org, Broadcom and Microsoft for implementations of the
  specification; they are patent grants, not copyright, and apply to any implementation.
- *Effect on neoteletext's licence:* none beyond keeping the notice. BSD-3-Clause is on the list of
  LICENSING.md section 2; the crate's licence is `MIT AND BSD-3-Clause`.

**opus-decoder 0.1.1** — the opus-decoder authors, <https://github.com/TadeuszWolfGang/Rusopus>
- *What it provides:* the starting point the owner chose for the decoder ("use opus-decoder and
  convert it to integer only"): a pure-Rust Opus decoder whose structure the first version
  followed, and its range decoder and SILK were the first versions of those parts. Its floating
  point could not be made bit-exact with the reference, so both were rewritten as ports of libopus;
  what still resembles it (names, tables, a few lines) is what both take from libopus.
- *Licence:* MIT OR Apache-2.0, the project's own terms.
- *Effect on neoteletext's licence:* none.

**Pillow 13.0.0** — Jeffrey A. Clark and contributors, on the Python Imaging Library of Secret Labs AB
and Fredrik Lundh, <https://python-pillow.org/>
- *What it provides:* the picture converter's resampler and starting palettes, ported step for step
  into `neoteletext-picture` (DEC-2142, DEC-2325). The problem it solved: the converter's pages had
  been made with Pillow by `scripts/image-to-teletext.py`, and the port had to reproduce them, so
  `raster.rs` follows Pillow's fixed-point resampler (its box and Lanczos filters, rounding and which
  pass runs first) and `quant.rs` its three quantizers, median cut, maximum coverage and the fast
  octree, hash table and heap included, since their iteration order decides the palette. The one
  departure is the octree's ties, which keep bucket order (DEC-2142 item 3). The port made the same
  pages as the Python script for all 2,605 crop and scale pages measured on 2 November 2026
  (TODO-2132).
- *Licence:* MIT-CMU (the Historical Permission Notice and Disclaimer form). Its copyright notice and
  permission notice travel with the ported source in `crates/neoteletext-picture/LICENSE-Pillow` and
  must appear in the documentation of any binary distribution.
- *Effect on neoteletext's licence:* none; the notice is kept as the licence asks.

## Used at test time only (never copied, never linked into a release)

These programs are run as separate programs or test oracles, in containers with no network access
(REQ-TST-001). Their code and test files are never copied into this repository.

**libopus 1.5.2, fixed-point build** — <https://opus-codec.org/>
- *What it provides:* the oracle of REQ-AUD-023. `tests/oracles/run_opus.sh` downloads its source,
  checks it against its pinned SHA-256 and builds it; its `opus_demo` encodes the test streams and
  `tests/oracles/opus_decode.c`, neoteletext's own, decodes them with it, for a byte-for-byte
  comparison with `neoteletext-opus`.
- *Licence:* BSD-3-Clause; nothing of the build is kept in the repository.

**vbit2** — Peter Kwan and contributors, <https://github.com/peterkvt80/vbit2>
**QTeletextMaker** — gkthemac, <https://github.com/gkthemac/QTeletextMaker>
- *What they provide:* independent test oracles and fuzzing inputs (REQ-TST-001).
- *Licence:* QTeletextMaker is GPL-3.0; vbit2's repository states no licence identifier. Both are
  run as external programs only.

## Formats and services neoteletext reads

**zxnet** — <https://zxnet.co.uk/teletext/viewer/>
- *What it provides:* the live teletext feeds neoteletext can show (REQ-INP-022), and the page
  formats its editor exports. neoteletext implements the feed protocol and the formats from their
  observed behaviour; no zxnet code is used.

**edit.tf** — <https://edit.tf/>
- *What it provides:* the URL encoding of teletext pages that neoteletext reads and writes.
  Implemented from the encoding itself; no edit.tf code is used.

## Fonts loaded at run time

Fonts other than Noto Teletext and the misc-fixed font above are not part of neoteletext: the user loads them from font libraries
at run time (area FNT), and each keeps its own licence and record of where it came from.
