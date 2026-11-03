# Noto-derived fonts

**Status:** these files are Noto Teletext, neoteletext's one font (DEC-1761, REQ-FNT-001), and the only compiled glyph data; the one other font in the repository is the X11 misc-fixed 6x10 font in `third_party/misc-fixed/` (REQ-2223), loaded at run time only. `cargo xtask glyphs` merges them into `crates/neoteletext-core/src/glyphs/noto.rs`, which is compiled into the core. A font file holds at most 1,024 characters (REQ-FNT-002), so the font is spread over four files, one font together; they agree wherever two hold the same character, and the generator refuses them if they do not. Each file can also be loaded at run time like any other font (REQ-FNT-005 to REQ-FNT-008).

## What is here

| File | What it is |
|---|---|
| `convert.py` | The converter. It shapes every character with HarfBuzz and draws it with FreeType in monochrome at the weight, hinting and metrics of its script, then places it in the 12 × 20 cell. Its dependencies are pinned in its inline metadata. |
| `test_convert.py` | The converter's tests (REQ-2165): a glyph too large for the cell is drawn smaller, never clipped. They draw with a stand-in face, so they need no source; the push gate runs them. |
| `settings.txt` | Each script's sources, weight, hinting and metrics, and each output font's characters. The values were measured in the prototypes of October 2026 (decision E.77, rdm DEC-601, which gives the reasons for them). |
| `sources.txt` | The Noto source fonts: their addresses in the google/fonts repository, pinned to commit `47d54ca1ebb14fd864695f1d39dc734f39017c33` (16 October 2026), and their SHA-256. The converter downloads them to a cache outside the repository and refuses any file whose checksum differs. The sources are never committed. |
| `*.bdf` | The converted fonts, 12 × 20, in the form of REQ-FNT-002: `noto-teletext.bdf`, the standard's 24 sets and neoteletext's Latin, Cyrillic, Georgian, Armenian and Tifinagh sets; `noto-teletext-composed.bdf`, every letter a Latin base and a diacritical mark compose to; `noto-teletext-arabic.bdf`, the Arabic-script sets and their joined forms; `noto-teletext-japanese.bdf`, the Japanese kana sets and the rōmaji option (DEC-2184, DEC-2187, REQ-2189), drawn from Noto Sans JP. |
| `fonts.txt` | The folder's index as a font library (REQ-FNT-004), with each font's conformance (REQ-FNT-011). |
| `OFL.txt` | The SIL Open Font License 1.1, with the copyright line of every source. |

## Licence

Noto is licensed under the SIL Open Font License 1.1, which `OFL.txt` reproduces with each source's copyright line. Its notices declare no Reserved Font Name for "Noto", so the converted fonts may keep "Noto" in their names. Noto Sans JP, the source of the Japanese file, is Adobe's Source Han Sans with the Reserved Font Name "Source": no converted font's name uses that word. A converted font is a Modified Version under the licence and is distributed under it.

## How the fonts are made

Noto Teletext is made by one pipeline, always in this order (REQ-2163). This section is the one place the order is written; the converter, the Arabic-script generator and `xtask`'s glyph generator point here.

1. **The character tables.** The code tables in `crates/neoteletext-core/src/charset/` are the input: they decide which characters exist, never the fonts. They are written by hand from the standards and datasheets, except the Arabic-script tables of group 1001, which `uv run scripts/arabic-sets.py` writes between its marker lines in `charset/arabic_script.rs` (REQ-2164). `uv run scripts/arabic-sets.py --check` changes nothing and fails, naming the first line that differs, when the committed tables are not what the script would write.
2. **The lists for the converter.** `cargo xtask glyphs` writes `crates/neoteletext-core/src/glyphs/required.txt` (every character a glyph set must draw) and `forms.txt` (the code point drawn for each joined form) from the tables.
3. **The fonts.** The converter draws every character of those lists from the pinned Noto sources (REQ-2165):

   ```sh
   cargo build --profile iterate -p neoteletext-cli
   uv run third_party/noto/convert.py --neottx target/iterate/neottx
   ```

   It writes the four BDF files, `fonts.txt` with each font's conformance, and the checksums below.
4. **The compiled table.** `cargo xtask glyphs` again, which compiles the four files into `crates/neoteletext-core/src/glyphs/noto.rs`.
5. **The checks.** `cargo xtask check-noto`, run by every gate, fails if a committed font differs from its checksum here, if a font and its line in `fonts.txt` do not match, or if the folder holds a file it does not account for. `xtask`'s tests fail if a stored `noto.rs`, `required.txt`, `forms.txt` or G3 table differs from what they regenerate, and the push gate runs `scripts/arabic-sets.py --check`.

**When it runs.** From the first step a change affects, and the commit carries every file the run changes:

- a change to a character table: from step 1;
- a change to `xtask`'s generators: from step 2;
- a change to `settings.txt` or `convert.py`: from step 3;
- a new Noto source, or a move of the pin in `sources.txt`: from step 3.

A glyph is never edited by hand in a BDF file or in `noto.rs`: a change to glyph data is made by changing a table, `settings.txt` or the converter and running the pipeline.

**Moving the source pin.** `sources.txt` names each source by its address in the google/fonts repository at one commit, with its SHA-256. Moving the pin changes the commit and every checksum together, in a commit of its own that says what changed in the fonts.

## Checksums

<!-- checksums: written by convert.py -->
- `noto-teletext.bdf`: `cfbd975333d14d8f65099e66e760a514314fe13c667672575ee47d67636288db`
- `noto-teletext-arabic.bdf`: `17cfa3817914c2a05ffadc1dd55f834356397ac5e9864fc82a0d6337b9c45629`
- `noto-teletext-composed.bdf`: `e500e27361addb68119d02834192206216a3fd672c6926d082947d9f6092fe87`
- `noto-teletext-japanese.bdf`: `a6bcc0a95f0746cbba59a82cbc871fa6c2727611716e279fe589f2b5a7eea398`
<!-- end checksums -->
