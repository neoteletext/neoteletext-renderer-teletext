# Porting guide

This is the whole of a port (REQ-ARCH-009, decision E.29). A port supplies the items below and nothing else; the library never touches the hardware.

## The checklist

| Item | How | Required |
|---|---|---|
| A data source | Implement `neoteletext_core::port::DataSource` (modelled on `embedded-io` `Read`), or call the format readers with bytes yourself. | Yes |
| Somewhere to put pixels | Call the row renderer with your own buffer: `neoteletext_core::render::render_row` (a text row) or `render_line` (one screen line). The port asks; the library answers. | Yes |
| A time source | Implement `neoteletext_core::port::Clock`: monotonic microseconds. Used for page completion, flash and reveal. | Yes |
| Key input | Implement `neoteletext_core::port::Keys` returning `neoteletext_core::viewer::Key`. | For interactive ports |
| Sleep | Implement `neoteletext_core::port::Sleep` if your platform must yield. | Optional |
| A network transport | Implement `neoteletext_web::port::Transport` (connect, send, non-blocking receive, close; TLS is yours). | Only for web sources |
| Random bytes | Implement `neoteletext_web::port::RandomSource`: the WebSocket handshake key and every frame mask (RFC 6455 sections 4.1 and 5.3). Use a cryptographic generator, since RFC 6455 requires masks an attacker cannot predict. The web agent takes one whatever its features. | Only for web sources |
| An audio sink | Implement `neoteletext_audio::port::VoiceSink`, and pass each packet to `neoteletext_audio::receiver::Receiver::push_packet` before the decoder; on a desktop, `neoteletext-audio-mix` mixes for you (area AUD, `audio-platforms.md`). | Only for sound |
| Cookie storage | Implement `neoteletext_web::port::CookieStorage`. Store saved cookies as secrets (REQ-SEC-013). | Optional |
| Device keys | Implement `neoteletext_web::dbsc::DeviceKeys`: create a key pair bound to the device, sign with it, and never let the private key leave the device's store (REQ-WEB-017, decision E.110). | Only with the feature `dbsc` |

A complete minimal port is `crates/neoteletext-core/examples/minimal_port.rs`, under 100 lines; it needs the core features `level15` and `render`.

## Choosing what to compile in

Every crate's optional areas are Cargo features. The defaults are each crate's optional areas of function; `std`, `alloc`, the formatting of `error-trait`, the writers' `encode` and `write-text`, the profiles and the features a decision leaves off (`dbsc`, and the core's `objects` and `additional-flash-functions`) are never default (REQ-1642 rule 5, DEC-2397). A small port turns the defaults off and names what it uses:

```toml
neoteletext-core = { version = "...", default-features = false, features = ["level15", "render"] }
neoteletext-formats = { version = "...", default-features = false }  # T42 only
```

| Crate | Features |
|---|---|
| `neoteletext-core` | `level15`, `level25`, `render`, `viewer`, `extended-sets`; the capabilities of Levels 2.5 and 3.5 `nonspacing-attributes`, `level25-32colors`, `level35-32colors` (default, decision E.105), `objects`, `additional-flash-functions` (not default); and the profiles `classic` (Level 1.5 alone, DEC-1757) and `modern-classic` |
| `neoteletext-formats` | `transport`, `sliced`, `pages`, `tti`, `ep1`, `edittf`, `dump`, `check`, `bdf` (default); `alloc`, `std`, `write-text` |
| `neoteletext-web` | `push`, `cookies` (default); `dbsc`, `error-trait`, `alloc`, `std` |

`level25` alone gives the enhancement processor's non-spacing attributes and characters. Each other capability of Levels 2.5 and 3.5 is a feature of its own (decision E.71); a capability left out is ignored in the page data, never an error. [capabilities.md](capabilities.md) explains the mechanism, which capabilities the profile keeps and why. The `modern-classic` profile keeps the classic display with objects, and leaves out the 32-colour palette and the additional flash functions:

```toml
neoteletext-core = { version = "...", default-features = false, features = ["modern-classic"] }
```

Measured flash on Cortex-M0 on 1 November 2026 (`cargo xtask size`, `opt-level = "z"`, link-time optimisation):

| Build | Flash |
|---|---|
| Core with no optional area: acquisition, the Level 1 grid, composing | 25,870 bytes |
| Level 1.5 receiver with the renderer and the viewer, without the font: the `classic` profile | 58,570 bytes |
| The same with the compiled font's standard sets | 98,450 bytes |
| The same at Level 3.5, every capability, without the font | 74,770 bytes |
| The same with the `modern-classic` profile's capabilities, without the font | 74,606 bytes |
| Every core area, with the whole compiled font | 147,282 bytes |
| Format detection reading T42 only, with the bare core | 42,774 bytes |
| Format detection reading every format, with the bare core | 56,037 bytes |

Notes on the builds:
- **A configured level that is not compiled in** falls back to the highest one that is (`PresentationLevel::available`).
- **Glyphs:** Noto Teletext is the one compiled font (`GlyphSet::noto_teletext`). A build links it only if it calls that function, and `extended-sets` adds the part of it only neoteletext's own sets use. A port short of flash can hold a subset of the same font with `GlyphSet::from_entries`.
- **The minimal port example** needs `level15` and `render`.

## Colours

A port that lets its user choose the colours (REQ-RND-005) gives the decoder the 32 entries of a Colour Map in `Config::user_colours`, a reference to a value the port keeps for as long as the configuration (a `static`, or one array made once for the run): its entries replace Table 30's before any page redefines them, at every level, and its CLUT 0 becomes the receiver's own colours, `Grid::receiver_colours`, in which the status area and a receiver-drawn row 24 are drawn and which no page can change (DEC-2464, DEC-2482). Without it the decoder uses Table 30. The palettes take the grid's colours: `neoteletext_image::palette::palette(&clut0_of(&grid.colour_map), ...)` in the palette mode and `colour_map_palette(&grid.colour_map, &grid.receiver_colours, ...)` in the resolved mode.

## Memory

All state lives in memory you provide (REQ-GEN-003):

- The page store is a slice of `StoredPage` you allocate: `[StoredPage::EMPTY; N]` in a `static`, or on the heap. Each slot is 2,688 bytes.
- For Levels 2.5 and 3.5, the enhancement store is two more slices: `EnhancementSubPage::EMPTY` entries of 120 bytes, and `EnhancementPacket::EMPTY` slots of 42 bytes, one per packet. Annex E.8 bounds a service at 500 packets at Level 2.5 and 1200 at Level 3.5, about 21 and 50 kilobytes, plus 24 packets for each magazine's Magazine Organization Table. Pass them to `Decoder::with_enhancement_store`; `Decoder::new` keeps no enhancement pages.
- A `Grid` is 1000 cells of 16 bytes and the page-wide state: 16,120 bytes. Build it in place with `build_grid(&mut grid, ...)`.
- The sizes of the state types, reported and bounded by the test `every_state_type_has_a_bounded_size` (`crates/neoteletext-core/tests/state_sizes.rs`; REQ-GEN-003), measured on 3 November 2026 and again on 4 November 2026 on 64-bit Linux (after the user's colours of DEC-2482: the configuration's pointer to them, 8 bytes there and 4 on a 32-bit processor, and the grid's eight receiver colours, 16 bytes), the same with every feature and with only `level15`, `render` and `viewer` (a 32-bit microcontroller's are the same or smaller, its pointers and `usize` being half as wide):

  | Type | Bytes |
  |---|---|
  | `StoredPage` (one slot of the page store) | 2,688 |
  | `EnhancementSubPage` | 120 |
  | `EnhancementPacket` | 42 |
  | `Decoder` (without its store) | 1,944 |
  | `Grid` | 16,120 |
  | `Viewer` | 320 |
  | `GlyphSet` | 1,416 |

- **What a decoder needs in all**, from those sizes: in the minimum configuration, one requested page at Level 1.5, a decoder with a store of one page, a `Grid` and a viewer take 1,944 + 2,688 + 16,120 + 320 = 21,072 bytes, about 20.6 kilobytes; `neottx`'s default store of 4,096 pages (`--pages`) adds 4,096 × 2,688 = 11,010,048 bytes, about 10.5 megabytes; and at Level 3.5 the enhancement store for Annex E.8's bound of 1,200 packets and its sub-pages adds about 50 kilobytes (1,200 × 42 bytes of packets, with the sub-page entries at 120 bytes each), plus 24 packets for each magazine's Magazine Organization Table.
- A rendered text row at 480 by 20 needs 9.6 kilobytes at 8 bits per pixel or 4.8 kilobytes at 4 bits.
- **Stack** (REQ-GEN-016). The core's largest stack frame is `level1::build_grid`'s: 1,632 bytes in the `level15` build of the test image and 1,552 bytes in its Level 3.5 build (`level15 capabilities render viewer`), on `thumbv6m-none-eabi`, measured on 4 November 2026 by building `firmware/test-image` with the pinned nightly and `-Z emit-stack-sizes` and reading the image's `.stack_sizes` section with `llvm-readobj --stack-sizes`. The next largest are `render::raster::font_or_text_line` (768 bytes), the Level 3.5 enhancement walk (`enhance::walk::Walker::walk`, 840 bytes) and `Decoder::push` (640 bytes at Level 3.5). These are single frames: allow for the calls beneath them, and for the `Decoder` and `Grid` values themselves wherever the port keeps them, which belong in a `static` or the port's own buffers rather than on a small stack. `neoteletext-opus` needs about 2.5 kilobytes of stack for a decode (`ARCHITECTURE.md`).

## Sub-pages

A page may have up to 3,199 sub-pages (area NUM, decision E.130), each in its own slot of the page store. `PageStore` lists and steps them without any memory of its own: `subcodes` iterates a page's stored sub-codes in order, and `subpage_count`, `first_subpage`, `last_subpage`, `next_subpage`, `previous_subpage` and `subpage_place` (a sub-page's place, counted from 1, as the status area shows it) each make one bounded pass over the slots you gave it. A port that shows a list of sub-pages calls them per frame or keeps its own copy; the store never allocates.

## Pixels and colour

- The renderer writes palette indices, never colours (REQ-RND-023): 0 to 7 the colours, 8 transparent, 16 to 79 flashing, 80 to 143 concealed.
- Flash and reveal are palette changes: refill your palette with `neoteletext_core::render::fill_palette` when the flash phase (`render::flash_visible`) or reveal changes. If your display has a hardware palette, no redraw is needed.
- `neoteletext_image::convert` turns indices into RGB565, RGB888, RGBA8888, grey or 1-bit, with byte order, stride and rotation.

## Aspect ratio

Teletext pixels are wider than tall on a 4:3 television: measured against the picture's width, a half-dot of the 12-by-20 cell is 1/624 of it and a line 1/768 of it, so each pixel is 768/624 = 16/13 (about 1.23) times wider than tall (REQ-RND-004, REQ-RND-024; the core's `ASPECT_NUMERATOR` and `ASPECT_DENOMINATOR`). The library always renders 480 by 500 (or 480 by 250); to show a page in a 4:3 picture, stretch the width in the port, for example with `neoteletext_image::scale::scale_indices(..., aspect: true)`, which maps each output column back through the stretch. Do not stretch the height: that makes the picture too tall.

## What the library will not do

It does not drive displays, generate video, keep display timing, buffer frames or contain code for any particular chip (REQ-ARCH-010). Buffering strategy, direct memory access and scan-out are yours.

## Security settings and their defaults

Every security setting defaults to the safer choice (REQ-SEC-020); a port that changes one takes on the risk named here. The test `the_default_configuration_is_the_safer_choice` in `crates/neoteletext-web/tests/agent_replay.rs` names each default of the web agent's `Config`.

| Setting | Default | The risk of changing it |
|---|---|---|
| `Config::allowed_local` (`neottx --allow-local`) | Empty: no local-network address may be contacted (REQ-SEC-006). | Each address allowed can be reached by any web service the receiver opens, so a hostile page can make the receiver probe or use a router, printer or other device at that address. Allow one test server's address, never a range. |
| `Config::persist_cookies` | Off: cookies live for the run only (REQ-SEC-013). | Saved cookies identify the user to every service across runs, and are secrets on the device: a port that saves them must store them as secrets and offer a wipe. |
| `Config::redirects_within_channel` | On: a redirect that leaves the channel is reported as `Event::RedirectToOtherChannel`, not followed (REQ-WEB-021). | Turned off, a channel can send the receiver to another site's content without the viewer confirming it; `neottx` turns it off only for a service opened by its descriptor, whose origin is the channel. |
| `Config::proxied` (`neottx --proxy`) | Off: connections are direct. | The proxy sees every host the receiver contacts, and the local-network check can then judge only the host's name, not the address the proxy resolves it to. Set it only for a proxy the user chose. |
| `Config::tls_available` | On: HTTPS addresses are used. | A port without TLS cannot open HTTPS services at all (they are reported unreachable); it does not fall back to plain HTTP. Turn it off only when the port has no TLS. |
| `--no-sandbox` (`neottx`) | Off: `neottx` sandboxes itself and refuses to run when it cannot (REQ-SEC-017). | A fault in a parser of untrusted input can then reach every file and the network the user can. For debugging only. |
| The security indicator | Always on; there is no setting (REQ-SEC-005). | None: a page cannot hide it, and no option turns it off. |

## Isolation on microcontrollers

REQ-SEC-021 (Phase 2 guidance). The library cannot enforce any of this: it has no hardware code and does not know the chip (REQ-ARCH-010). It is what a port does around it. Untested on hardware: no port in this repository does it yet.

**Why.** The network stack is the part that reads what an attacker on the network sends first, and the decoder reads what a hostile page sends. On a chip without an operating system both run with the rights of everything else, so a fault in either can rewrite the other's memory, the keys of a TLS session or the frame buffer. A memory protection unit lets the port give each its own regions, so that a fault stops at its own boundary.

**What each kind of chip can separate.**
- **Arm Cortex-M with a memory protection unit** (the Armv7-M and Armv8-M profiles: Cortex-M3, M4, M7, M23, M33; the Cortex-M0+ has it as an option, the Cortex-M0 never): eight or sixteen regions, each with a base, a size, read, write and execute permissions for privileged and unprivileged code. The usual split runs the decoder unprivileged with its own regions, and the network stack and the port privileged, or the other way round, switching regions on each change of task.
- **RISC-V with physical memory protection** (PMP, in the privileged specification): up to sixteen or sixty-four entries giving read, write and execute rights to machine-mode software's less privileged modes. With user mode, the decoder runs in user mode inside its entries and calls the port only through traps; without user mode, PMP protects only against machine mode's own mistakes when entries are locked, which is much weaker.
- **Xtensa (ESP32, ESP32-S3)**: the vendor's memory protection (the ESP32-S3's permission control, the ESP32's region protection), which the vendor's SDK configures; consult its documentation.

**The regions the decoder needs.**
1. Its code and read-only data, including the compiled font: read and execute, never write.
2. Its state, all of it in memory the port provides (REQ-GEN-003): the page store (`[StoredPage; N]`), the enhancement store if Levels 2.5 or 3.5 are compiled in, the `Grid`, the viewer and its history. Read and write, never execute.
3. Its stack: read and write, never execute, with a guard region below it, since a fixed stack can overflow into its neighbour.
4. The buffer it shares with the network stack (below), and the row buffer it renders into for the display: read and write.

Nothing else: no peripheral, no other task's memory, no network buffer beyond the shared one.

**What passes between them, and how.** Only a stream of bytes, in one direction, through one shared buffer: the network stack writes the bytes of a response or a pushed message (TTI, T42 or the channel's records), and the decoder reads them, just as `neottx`'s fetcher process writes frames into a pipe that its decoder reads (REQ-SEC-018). Use a single-producer, single-consumer ring buffer of a fixed size in its own region, writable by the network stack and readable by the decoder, with the read and write positions as the only shared variables. Requests the other way (the next page wanted, an input message) go through a second, small buffer of the same kind. Treat every byte read from the buffer as untrusted, as the decoder already does: the length of a frame is bounded before it is used. Never share a pointer, a callback or a structure with fields that the other side interprets.

**What a fault does.** A memory protection fault in the decoder's task means the decoder met input it could not handle, or has a fault: stop that task, clear its regions (the page store and the grid), drop the bytes in the shared buffer, and start it again with a fresh store, showing the receiver's own message that the page could not be shown. Never resume the faulted task, and never let the fault handler run code from the decoder's regions. A fault in the network stack's task is handled by restarting the network. Count faults, and stop restarting after a few in a short time, so that a hostile service cannot keep the device busy.

**What this does not protect.** The display: the decoder writes the row buffer, so a hostile page can draw whatever a page may draw, which the status area's rules (REQ-SEC-005) already govern. Timing: the decoder's work for each packet is bounded (REQ-SEC-012), but the port still needs a watchdog on its task.
