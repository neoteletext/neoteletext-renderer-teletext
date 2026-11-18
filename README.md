# neoteletext

A software receiver for World System Teletext (ETSI EN 300 706, Presentation Levels 1.0, 1.5, 2.5 and 3.5), written in Rust. The core library is `no_std` with no allocation and runs on microcontrollers; the `neottx` command-line program decodes recordings, live feeds and web teletext services, and shows pages in the terminal (iTerm2 inline images or coloured text), as text, or as images.

```sh
cargo build --release -p neoteletext-cli
target/release/neottx show recording.t42 100
target/release/neottx view zxnet:teefax
```

While changing the code, build with the `iterate` profile instead: release optimisation without link-time optimisation, built incrementally, so an edit rebuilds in 2.7 seconds (to the program) or 3.6 seconds (to the core), against 76 seconds for `--release` (measured on 14 October 2026). `neoteletext-common/DEVELOPMENT.md` section 2.2 says how it is used.

```sh
cargo build --profile iterate -p neoteletext-cli
target/iterate/neottx show recording.t42 100
```

A windowed browser built on the engine, with tabs, channels and a tuning screen, is maintained in the separate neoteletext-browser repository (decision E.76).

Each crate's optional parts are Cargo features, on by default, so a port or a smaller `neottx` compiles in only what it uses. The features are listed in the user guide and the porting guide. The decoder's capabilities from the later teletext levels are features too: the recommended `modern-classic` profile keeps those that preserve teletext's look or make it more usable, and switches off those that would dilute it, such as colour bitmap characters, the 32-colour palette and animation ([docs/spec/design/capabilities.md](docs/spec/design/capabilities.md)).

## Documents

| Document | Contents |
|---|---|
| [docs/user-guide.md](docs/user-guide.md) | The `neottx` program: commands, inputs, outputs, profiles and settings. |
| [docs/spec/design/porting.md](docs/spec/design/porting.md) | Writing a port of the library for a new display or device. |
| [docs/spec/design/font-security.md](docs/spec/design/font-security.md) | The security analysis of the font strategy: one compiled font, other BDF fonts loaded when the user names them. |
| [docs/spec/design/capabilities.md](docs/spec/design/capabilities.md) | The compile-time capability features and the modern-classic profile: what keeps teletext's distinctive look and is kept, and what would dilute it and is switched off. |
| [docs/spec/](docs/spec/README.md) | The design documents, glossary and sources. The requirements, decisions and open questions are in rdm. |
| `neoteletext-common/PROTOCOL.html`, DOC-2182 and DOC-2183 | How teletext came to have its G0 and G2 character sets, and how a page's sets, glyphs and stand-ins work together; DOC-2028 holds every set's code table. |
| [ARCHITECTURE.md](ARCHITECTURE.md) | The layers, their boundaries and allowed dependencies. |
| [THREAT-MODEL.md](THREAT-MODEL.md), [SECURITY.md](SECURITY.md) | Security design and reporting. |
| [docs/spec/module-sources.md](docs/spec/module-sources.md) | Where test data and live feeds come from. |
| [IMPLEMENTATION-LOG.md](IMPLEMENTATION-LOG.md) | Questions met during implementation and the decisions taken. |

## Crates

| Crate | Role |
|---|---|
| `neoteletext-core` | Error protection, packets, acquisition, the page and enhancement stores, character sets, Level 1.0 and 1.5 decoding, the Level 2.5 and 3.5 enhancement processor with objects and the Colour Map, the state machines, the row renderer and the viewer model. `no_std`, no allocation, no panics. |
| `neoteletext-formats` | Readers and writers: T42, transport streams, PES, sliced VBI, TTI (with enhancement data and enhancement pages), EP1, edit.tf and screen dumps; and a builder for Level 2.5 and 3.5 test pages. |
| `neoteletext-slicer` | A software data slicer for raw sampled video. |
| `neoteletext-web` | The sans-IO web agent: service descriptors, HTTP, WebSockets, viewer state's records and signed bytes (no cookies) and the zxnet feed. |
| `neoteletext-image` | Palettes, pixel formats, scaling, PNG and PPM writers. |
| `neoteletext-cli` | The `neottx` program. |
| `xtask` | Project checks: layers, dependencies, constants, glyphs (including the generated G3 set) and the test corpus; `cargo xtask level25-stream` writes the authored Level 2.5 and 3.5 test stream. |

## Checks

`scripts/check.sh commit | push | full` runs the gates, and the git hooks in `.githooks` run it; `~/devel/neoteletext-common/DEVELOPMENT.md` section 9 says how the gates and the work branch `livedevelopment` work. The full gate runs every target and fuzzing on the Linux build host (`NEOTELETEXT_REMOTE`). Tests that need the real broadcast corpus read it from `$NEOTELETEXT_CORPUS` (see `cargo xtask corpus`) and say so when it is absent.

## Licence

The project is licensed under the MIT licence; see [LICENSE](LICENSE). Bundled third-party material keeps its own licence, recorded beside it. [LICENSING.md](LICENSING.md) says which other licences the project may use.

## Licensing of data

No standards documents, GPL code or broadcast recordings are kept in this repository. The one font is Noto Teletext (`third_party/noto/`, DEC-1761), whose provenance, licence and checksums are recorded in `third_party/noto/PROVENANCE.md`. The one other font kept here is the public-domain X11 misc-fixed 6x10 font (`third_party/misc-fixed/`, REQ-2223), at 12 × 20 for loading at run time; it is not compiled in. Other fonts are 12 × 10 BDF files loaded when the user names them (area FNT); a channel or a page cannot name one (decision E.111).
