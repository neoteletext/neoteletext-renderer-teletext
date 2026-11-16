# Licensing

neoteletext is licensed under the MIT licence ([LICENSE](LICENSE)). This document says which other licences the project may use, and why each one leaves the MIT licence intact. NeoTeletext (the neoteletext-browser repository) follows the same rule; its own details are in that repository's `docs/licensing.md`.

This is the project's reading of the licences, not legal advice.

## 1. The rule

Using third-party material must never compromise the MIT licence. Anyone who receives neoteletext's code must be able to use, change and ship it, open or closed, under the MIT licence alone. They must never become bound by another licence's terms just because they took our code.

A licence is acceptable if, used the way we use it, it puts no conditions on our code beyond keeping a notice.

## 2. What is acceptable

| Kind | Licences | Why our licence is not compromised | Conditions |
|---|---|---|---|
| **Code compiled into our programs** | MIT, Apache-2.0 (also with the LLVM exception), BSD-2-Clause, BSD-3-Clause, Zlib, ISC, CC0-1.0: the list of REQ-GEN-017, which `deny.toml` enforces | They are permissive: they ask for a notice and nothing more of the code that uses them | Keep their notices in any binary distribution |
| **Data compiled into our programs** | Unicode-3.0 (Unicode's tables), on REQ-GEN-017's list since DEC-1093 (confirmed by the owner on 31 October 2026), because more than one crate carries Unicode's data; CDLA-Permissive-2.0 (Mozilla's root certificates) | Permissive data licences with no copyleft | Unicode-3.0 is allowed for any crate; CDLA-Permissive-2.0 for `webpki-roots` alone, as a per-crate exception in `deny.toml` (decision 32 of IMPLEMENTATION-LOG.md, DEC-1093) |
| **Fonts compiled into our programs** | SIL Open Font License 1.1 (Noto Teletext, converted from Noto) | The OFL binds the font, not the program that draws it. A modified font must stay under the OFL, but the program's code is untouched | Ship the OFL text and copyright lines with any binary that holds the font (`third_party/noto/OFL.txt`); never sell the font on its own |
| **System libraries loaded at run time and not shipped** | LGPL-2.1-or-later (glibc, ALSA's `libasound`), GPL with the GCC Runtime Library Exception (`libgcc_s`) | Linked dynamically from the user's own system, so the user can always replace them, which is all the LGPL asks. The GCC exception exists to allow any program to link the runtime | Never link them statically and never bundle them (see section 3) |
| **Test tools, built and run only on the build host and never shipped** | NCSA (the University of Illinois/NCSA Open Source License), for `libfuzzer-sys` alone, which bundles LLVM's libFuzzer for the fuzz targets in `fuzz/` | Permissive, with no copyleft; and nothing built with it is distributed, so no one receives it from us | A per-crate exception in `deny.toml`; never in a crate linked into a shipped program (DEC-2434, waiting for the owner's confirmation) |
| **A choice of licences** | For example `r-efi` (MIT, Apache-2.0 or LGPL-2.1-or-later) or `byteorder` (Unlicense or MIT) | We take it under the permissive choice | None beyond that choice's own |

The owner accepted LGPL system libraries on 19 October 2026: the system's `libasound` for sound on Linux (decision D3 of `docs/spec/design/audio-platforms.md`, and NeoTeletext's decision 40).

## 3. What is not acceptable

- **GPL and AGPL code in anything we ship** (REQ-GEN-017). Linking it into a program puts the whole program under the GPL. The repository keeps no GPL code. The GPL decoders in CREDITS.md are used only as test oracles: they run as separate programs, and nothing we ship contains their code.
- **LGPL code compiled in or linked statically.** That would oblige anyone who ships our program to let users relink it against their own copy of the library.
- **LGPL libraries bundled with a release** (for example inside an AppImage or Flatpak) unless the release also carries the LGPL text and offers that library's source. The preferred course is not to bundle them: a package lists `libasound` as a dependency instead.
- **File-level copyleft code compiled in,** such as MPL-2.0 or EPL-2.0. It would not spread to our files, but changed files would have to be published. It needs the owner's decision first.
- **Licences that are not open source:** the Business Source License, the Server Side Public License, the Commons Clause, non-commercial or field-of-use limits.
- **Code with no licence.** It cannot legally be copied at all.

## 4. How it is enforced

- **Crates:** `cargo-deny` checks every crate's licence against the allow list in `deny.toml` on every full gate (`scripts/check.sh full`), in the main workspace and in the fuzz workspace (`fuzz/`, which has its own lockfile), both against the one `deny.toml`. An exception names one crate and one licence, with its reason.
- **System libraries** are outside cargo-deny's view. A new one is recorded here before it is used.
- **Bundled material** (`third_party/`) keeps its licence and provenance beside it, and is credited in CREDITS.md.

## 5. What is in use

Measured on 20 October 2026 from `cargo metadata --all-features`.

- **Crates:** about 110, all under the licences of section 2: MIT or Apache-2.0 for most; `ring` (Apache-2.0 and ISC), `rustls` and `rustls-webpki` (ISC among their choices), `subtle` (BSD-3-Clause), `unicode-ident` and `unicode-normalization` (Unicode-3.0), `webpki-roots` (CDLA-Permissive-2.0). The library crates that run on a microcontroller have no dependencies at all (REQ-GEN-017).
- **Ported code:** `neoteletext-opus` is ported from libopus 1.5.2 (BSD-3-Clause; its notice kept beside the source as `LICENSE-libopus` and in each ported file), so the crate is licensed `MIT AND BSD-3-Clause`; CREDITS.md names it (decision E.123).
- **Translated code:** `neoteletext-audio-instruments` holds, under `src/dx7/msfa/`, code translated to Rust from Google's music-synthesizer-for-android (Apache-2.0; commit `f67d41d`), each file keeping its notice and saying what was changed; the licence's text is kept beside the crate as `LICENSE-APACHE-2.0`, so the crate is licensed `MIT AND Apache-2.0`; CREDITS.md names it (DEC-6420). Nothing of any program under the GNU General Public License is in it: the SID emulator was written from the chip's public documentation (DEC-6418).
- **Fonts:** Noto Teletext, the one font, converted from Noto and compiled into `neoteletext-core` (SIL OFL 1.1, `third_party/noto/`, DEC-1761). Its notice is compiled in with it as `GlyphSet::provenance`. The X11 misc-fixed 6x10 font, public domain, is kept in `third_party/misc-fixed/` for loading at run time and is not compiled in (REQ-2223).
- **System libraries:** the C library of each platform. The engine itself plays no sound; `neoteletext-audio-mix` only mixes.
