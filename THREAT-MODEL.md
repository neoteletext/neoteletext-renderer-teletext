# Threat model

This is the threat model REQ-SEC-002 requires. Security overrides every other requirement (decision E.32). Review and update it at the end of every milestone, and whenever an input source or output is added.

## Assets

| Asset | Why it matters |
|---|---|
| The host device and its resources | A decoder bug must not become code execution, file access or denial of service. |
| The user's session cookies | They identify the user to web teletext services. |
| The user's trust in what is displayed | A page must not be able to fake receiver messages, security markers or link destinations. |
| The user's local network | A web-native client on a home network can be used to reach routers and other devices. |
| The user's terminal | Escape sequences in output can rewrite the screen, change titles or worse. |
| The integrity of the build and release | Dependencies and tools can be replaced by attackers. |
| The user's privacy | Requests, cookies and history reveal what the user reads. |

## Attackers

1. **Anyone able to supply input**: a file, a broadcast or recording, a web service, a pushed message or a live feed.
2. **Anyone on the network path**, especially for plain HTTP, which is allowed (decision E.32).
3. **The software supply chain**: dependencies, toolchains and build tools.

## Trust boundaries and protections

| Boundary | Attacker | Asset | Protection |
|---|---|---|---|
| Untrusted bytes into the format readers (L0) | 1 | Host | Streaming parsers with bounded memory, no panics, checked arithmetic, fuzzing (REQ-GEN-008, REQ-SEC-003, REQ-TST-040). |
| Packets into acquisition (L1, L2) | 1 | Host | Fixed-size store over caller memory, bounded work per packet (REQ-SEC-012). |
| Page content into the grid (L3) | 1 | Trust | Row 0 columns 0 to 7 cannot be written by any page route (REQ-SEC-005, E.5 (j)). |
| Output to the terminal (L5) | 1 | Terminal | Allow-listed characters only; only our own fixed escape sequences; image data encoded from pixels (REQ-SEC-010, REQ-RND-029). |
| Untrusted bytes into the web user agent (L7) | 1, 2 | Host, network, cookies | Strict address parser, minimal user agent profile, strict HTTP framing, no compression, size and time limits, policy checks on every connection and redirect (REQ-WEB-014 to 016, REQ-SEC-006 to 011). |
| Plain HTTP connections | 2 | Cookies, trust | Unencrypted marker in the status area, `Secure` cookies never sent, no downgrade (REQ-SEC-007, REQ-SEC-008). |
| A page's form lines | 1 | Trust | None to protect: the TTI Form Extension is withdrawn (decision E.126), and its lines are unknown commands, ignored with a diagnostic (REQ-INP-013). A channel asks for text over the input protocol instead (area INT). |
| Links | 1 | Trust | Receiver-drawn link row, other services marked and confirmed (REQ-SEC-024, REQ-NAV-011, REQ-NAV-012). |
| Device-bound sessions (feature `dbsc`) | 1, 2 | Cookies, the device's keys | Over HTTPS only; registration, scope and refresh on the responding origin; private keys only in the port's store; every header and the instructions read by bounded, fuzzed readers; at most four sessions (REQ-WEB-017, decision E.110). Reviewed 23 October 2026. |
| A channel's log lines | 1 | A developer's console, trust | `LG` lines are refused whole unless one of three levels and 1 to 200 bytes of UTF-8 with no control or bidirectional formatting character, at most 16 per page; never drawn on a page; fuzzed (REQ-INP-026, decision E.109). Reviewed 23 October 2026. |
| The `neottx` process | 1 | Host | No `unsafe` code; the library crates have no operating-system access (REQ-SEC-016); Landlock and seccomp on Linux after inputs and outputs are open and before any of them is parsed (REQ-SEC-017); where neither the seccomp filter nor Landlock's network rules exist, `neottx` refuses unless told to continue (DEC-2351). |
| Fetcher to decoder (web mode) | 1, 2 | Host | Privilege separation (REQ-SEC-018): the fetcher process holds the network and TLS, the decoder holds the terminal and files. They speak a framed pipe protocol whose every frame is length-checked and validated by the receiver; the decoder ignores frame kinds only it may send; the frame kinds are distinct and never reuse a retired one, which the build asserts; the frames are fuzzed (`pipe_frames`). |
| Requests the receiver makes | 1, 2 | Privacy | No `Referer` header; a minimal, fixed request profile with `Accept-Encoding: identity` and no identifying headers beyond the `User-Agent` product (REQ-WEB-031); cookies only as REQ-WEB-006 allows, never saved between runs unless the port enables it, and a response from outside the channel stores none (REQ-SEC-013, decision E.114 item 2); pre-fetching only from the service of the page being viewed (REQ-NAV-004). The owner is asked to confirm these rules (TODO-2253). |
| Cookies kept on disc | 1 | Privacy, cookies | Off by default (`persist_cookies`); a port that saves them stores them as secrets (REQ-SEC-013). |
| The return channel: keys, pointer and touch sent to a channel (decision E.125, DEC-657; area INT) | 1 | Privacy, trust | Sent only while the channel's push connection is open and its descriptor declares input; each message bounded (`MAX_INPUT_MESSAGE`, 8,192 bytes) on both sides of the pipe and read by the bounded readers of `neoteletext-input`, which are fuzzed (`input_session`, `input_stream`). Reviewed 3 November 2026. |
| A channel's records on the push connection: index, `LOG`, `SET_COOKIE`, `LINKS`, `SUBPAGES`, `DENIED`, `REDIRECT` and the input messages | 1 | Host, trust, cookies | Each read by its own bounded reader in `neoteletext-web` (`records/`), refused whole when malformed, each with a fuzz target (`links_records`, `subpages`, `redirect_records`, `control_records`, `cursor_records`, `input_stream`, `channel_log`); `SET_COOKIE` goes through the cookie jar's rules. Reviewed 3 November 2026. |
| A program's redirect to another channel (REQ-1688) | 1 | Trust | Shown as the one-second "Redirecting to:" notice, bounded in number of hops, and the viewer's own choice wins; a redirect from `https` to plain `http` is refused (REQ-SEC-007, DEC-2378). The security review owed by TODO-1691 is open. |
| Sound: the audio profile's packets and Opus (decision E.123, DEC-647) | 1 | Host | Bounded record store and arena, a decoder with no panics and no unchecked arithmetic, compared bit for bit with libopus, fuzzed (`audio_packets`, `audio_records`, `audio_messages`, `audio_file`, `opus_packets`); `docs/spec/design/opus-security.md`. Reviewed 26 October 2026. |
| `neottx qr` and `neottx keyboard` with the qrkeyboard relay (decision E.93) | 1, 2 | Host, the user's typing | TLS to the relay; pairing by SPAKE2 with a PIN, then end-to-end encryption with a counter that refuses replay; secret-derived bytes compared in constant time (`subtle`, DEC-640); releases passed on only for held keys. `neottx keyboard` holds the network and runs without the sandbox (DEC-618). The relay's responses have no fuzz target yet (TODO-2093). |
| `neoteletext-transport`: sockets, TLS and proxies for `neottx` and the browser (DEC-1905) | 1, 2 | Host | An application crate, never a library crate (`check-io` stays whole); reads are bounded by the callers' frame and response limits; the proxy handshake is fuzzed (`proxy`). |
| `neottx probe` (DEC-1759 lesson 20) | 1 | Host, terminal | Runs as a live input: two processes, the decoder sandboxed, every line it prints through the terminal allow-list (REQ-SEC-010). |
| Dependencies and tools | 3 | Build | Every library approved by the owner by name; microcontroller crates take only the dependencies `cargo xtask check-deps` permits (decision E.38, as amended); `Cargo.lock` and `fuzz/Cargo.lock` committed; the toolchain and the development tools pinned and checked by SHA-256 (`scripts/tools.sh`); `cargo-deny` and `cargo-machete` in the full gate, which cannot skip them (REQ-SEC-014). |

## Status at this milestone

Checked on 3 November 2026.

| Item | State |
|---|---|
| REQ-SEC-005 status area | Enforced in the grid builder and the Level 1.5 walk, with tests. On a newsflash or subtitle page the receiver's cells are boxed while an indicator is on, so the indicator shows. Open: at Level 3.5 a page that redefines CLUT 0 can recolour the status area and the receiver's row 24 (TODO-2090). |
| REQ-SEC-008 unencrypted marker | Follows the page on screen: set when it came over plain HTTP, cleared when it came encrypted. `Secure` cookies never go over plain HTTP. |
| REQ-SEC-010 terminal output | Allow-list and injection test in `neottx`. |
| REQ-SEC-016 no ambient authority | Checked by `cargo xtask check-io`, which reads `use` trees and paths whole. |
| REQ-SEC-017 Linux sandbox | Landlock (files, TCP) and seccomp (no programs, no new processes), applied by every command after it opens its inputs and outputs and before it parses them; tested by `crates/neoteletext-cli/tests/sandbox.rs`. Open: `neottx contents` and `neottx html` parse before the sandbox, and macOS, Windows and OpenBSD are Phase 2 (`neottx` prints a notice; TODO-1743). |
| REQ-SEC-018 privilege separation | Done for every live input (`zxnet:`, `wss://`, `http://`, `https://`). On Linux both processes sandbox themselves: the fetcher may use the network and read only the resolver files (IMPLEMENTATION-LOG.md decision 28); the decoder has no network. The pipe's frames are fuzzed. |
| REQ-SEC-024 links | A coloured key follows a link only while its own prompt is shown whole (`Grid::prompts_shown`, DEC-2115). |
| REQ-SEC-014 supply chain | Lockfiles, pinned toolchain and tools; `cargo-deny` and `cargo-machete` in the full gate. Open: a schedule for new advisories, Espressif's toolchain checked by checksum (TODO-2092); the bill of materials, release signing and reproducible builds wait until the owner is ready for releases. |
| REQ-TST-040 fuzzing | 32 targets in `fuzz/fuzz_targets/`, each run in the full gate for `NEOTELETEXT_FUZZ_SECONDS` (20 by default) after every input that ever found a fault is replayed. Open: targets for the qrkeyboard client's readers (TODO-2093), and longer runs on a schedule (TODO-1742). |

## Review record

| Date | Milestone | Reviewer | Result |
|---|---|---|---|
| 10 October 2026 | Implementation in progress | Initial model | Gaps listed in the status table above. |
| 10 October 2026 | amd64 and iTerm2 implementation complete | Implementation review | REQ-SEC-018 and REQ-SEC-024 done. Open: fuzzing, `cargo-deny`, the bill of materials, release signing, and the macOS and Windows sandboxes. |
| 24 October 2026 | The security review of the engine and the browser after the reorganisation | NeoTeletext's security review, findings F-01 onwards | Its findings are cited by the decisions that answered them (among them E.114, E.115 and E.116). |
| 26 October 2026 | The Opus decoder (decision E.123) | Three independent reviews (the Opus layer and tooling, CELT, SILK), fuzzing and the known Opus CVEs | No exploitable fault; three latent faults and the test gaps fixed (f5c6d9a, ce95ba6, f9d3cb6). `docs/spec/design/opus-security.md`. |
| 3 November 2026 | The return channel, the channel's records, the program's redirect, qrkeyboard, `neoteletext-transport` and `neottx probe` added to the model | Audit of area SEC against the code (TODO-2090 to TODO-2094) | Every input and output now has a row. Fixed: four commands unsandboxed and the others sandboxed after parsing; the pipe unfuzzed; the unencrypted marker never cleared; the status area hidden on boxed pages; redirects out of the channel followed by default. Open items are in the status table. |
