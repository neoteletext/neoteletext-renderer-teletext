# The X11 misc-fixed 6x10 font

**Status:** a second third-party font beside Noto Teletext, kept in the repository by the owner's request of 3 November 2026 (REQ-2223): "we need the x11 6x10 cp437 font added to neoteletext X11 misc-fixed 6x10.bdf". It covers all of code page 437 (the IBM PC's character set), with Latin, Greek, Cyrillic and Hebrew. It is a font loaded at run time like any other (REQ-FNT-005 to REQ-FNT-008, `neottx render --font`). It is not compiled into the core, it is not the default anywhere, and the browser does not carry it: Noto Teletext stays the compiled font and the browser's only font (REQ-121, REQ-1770). It holds no glyph from a character-generator ROM (REQ-401).

## What is here

| File | What it is |
|---|---|
| `6x10.bdf` | The original, byte for byte: Markus Kuhn's Unicode (ISO 10646-1) version of the classic X11 terminal font, `-Misc-Fixed-Medium-R-Normal--10-100-75-75-C-60-ISO10646-1`, 1,597 glyphs in a 6 × 10 cell. |
| `misc-fixed-6x10-12x20.bdf` | The converted font, 12 × 20 in the form of REQ-FNT-002, with every pixel of the original made two by two, so that the 6 × 10 cell fills the engine's 12 × 20 cell exactly and nothing is resampled. It holds 1,020 characters: U+0001 to U+052F (Basic Latin to Cyrillic Supplement), U+2000 to U+27FF (General Punctuation to Dingbats, which hold the box-drawing, block and symbol characters of code page 437) and U+FB00 to U+FFFF (the Latin ligatures and the replacement character). It has every graphic character of code page 437. |
| `misc-fixed-6x10-12x20-extended.bdf` | The rest of the original, converted the same way, 577 characters: its default glyph at U+0000, Hebrew, Runic, Latin Extended Additional, Greek Extended and Braille. A font file holds at most 1,024 characters (REQ-FNT-002), so the original's 1,597 cannot be one file. |
| `fonts.txt` | The folder's index as a font library (REQ-FNT-004), with each font's conformance as `neottx font sets` reports it (REQ-FNT-011). |

## Source and retrieval

- Address: `https://www.cl.cam.ac.uk/~mgk25/download/ucs-fonts.tar.gz`, archive member `6x10.bdf` (the font's page is `https://www.cl.cam.ac.uk/~mgk25/ucs-fonts.html`).
- Retrieved on 16 October 2026 into the owner's font collection, `teletext-reference/teletext-fonts/manufacturers/xorg/originals/ucs-fonts-2026-10-16/6x10.bdf`, whose `SOURCES.json` records the address, member, size and checksum; copied here unchanged on 3 November 2026.
- Size 184,612 bytes; SHA-256 `bb14d95828990eb3ada68a24d716ae640a674cd75ab6caca17c798e4cb385849`. The file's own revision line is `$Id: 6x10.bdf,v 1.35 2006-01-05 20:03:17+00 mgk25 Rel $`.

## Licence

Public domain. The font's `COPYRIGHT` property reads "Public domain terminal emulator font.  Share and enjoy.", and the archive's README and `SOURCES.json` say the same. The converted fonts carry the same `COPYRIGHT` property. Nothing here is under the GPL.

## How the fonts are made

```sh
cargo xtask misc-fixed
```

The command (`xtask/src/misc_fixed.rs`) refuses an original whose SHA-256 differs from the one above, reads it with the engine's own bounded reader of source fonts, makes every pixel two by two, splits the glyphs between the two files by the code point ranges above, and writes them with the engine's font writer. It then writes `fonts.txt` and the checksums below. It needs nothing outside the repository.

`cargo xtask check-misc-fixed`, run by the push gate, fails if a file here differs from its checksum below, if a font and its line of `fonts.txt` do not match, if the folder holds a file it does not account for, or if the converted fonts are not the original doubled pixel for pixel, every glyph in exactly one file. The tests of `xtask` check the same, and that the committed files are what the command writes.

## Checksums

<!-- checksums: written by cargo xtask misc-fixed -->
- `6x10.bdf`: `bb14d95828990eb3ada68a24d716ae640a674cd75ab6caca17c798e4cb385849`
- `misc-fixed-6x10-12x20.bdf`: `5c688c67f130c053cfd95c56b3a7d9fc245a69672888f0d39390ed46e6f793f9`
- `misc-fixed-6x10-12x20-extended.bdf`: `58dd0d141ef00d3f522d6c58f8eba9ab5e972636e4a052247969b9d83cf09bcd`
<!-- end checksums -->
