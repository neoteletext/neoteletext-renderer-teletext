# neottx user guide

`neottx` is the command-line receiver of neoteletext (REQ-DOC-002). It decodes World System Teletext (ETSI EN 300 706, Presentation Levels 1.0, 1.5, 2.5 and 3.5) from recordings, live feeds and web services, and shows pages in the terminal, writes them as text or images, or converts them between formats.

`neottx` is built for Linux on x86_64, aarch64, 32-bit Arm and 64-bit RISC-V, for macOS on Apple silicon and Intel, and for Windows with the MinGW toolchain (`scripts/build-targets.sh`, run by the full gate). It sandboxes itself on Linux (REQ-SEC-017). Graphics in the terminal use the kitty graphics protocol, Sixel or iTerm2 inline images, whichever the terminal answers it has; every other terminal gets coloured text.

## Building

```sh
cargo build --release -p neoteletext-cli
# the program is target/release/neottx
```

Every part below is on by default. A smaller program leaves parts out (decision E.61):

| Feature | What it adds |
|---|---|
| `view` | `neottx view`, the interactive viewer, and image sizing to fit the terminal in `show` |
| `live` | Live feeds and web services (`zxnet:`, `wss://`, `http(s)://`), `check --descriptor`, and the TLS code they need |
| `slice` | `neottx slice` |
| `write-text` | `neottx write-text`, pages from text in any of the character sets ("Writing pages from text" below) |
| `sound` | `neottx sound`, sounds as the audio profile's packets |
| `keyboard` | `neottx qr` and `neottx keyboard`, a phone as the keyboard through qrkeyboard, with its TLS and cryptography |

```sh
cargo build --release -p neoteletext-cli --no-default-features --features view
```

Two capabilities of Levels 2.5 and 3.5 are off by default (decision E.105) and are named to build them in: `objects` and `additional-flash-functions`:

```sh
cargo build --release -p neoteletext-cli --features objects,additional-flash-functions
```

That program with `--no-default-features --features view` reads files, shows and converts pages, and is about half the size, with no TLS code. Every command stays in the help text. A command whose part is left out exits with status 2 and says the build does not include it. Without `view`, `show` draws images at scale 1 unless `--scale` is given.

The toolchain is pinned in `rust-toolchain.toml`. `scripts/check.sh full` runs every gate: formatting, clippy, the tests, the layer and constant checks, and the build for the bare-metal target.

## Commands

| Command | What it does |
|---|---|
| `neottx list INPUT` | Lists the pages and sub-pages found, with reception counts and error statistics, including how many pages named a character set designation that Table 32 reserves and so fell back to another set. |
| `neottx show INPUT PAGE` | Shows one page in the terminal and exits. |
| `neottx view INPUT` | Browses pages interactively. |
| `neottx probe CHANNEL` | Opens a channel as a receiver does and reports what it found, for a channel's developer. |
| `neottx text INPUT PAGE` | Writes one page, or `all`, as UTF-8 text on standard output. |
| `neottx render INPUT PAGE -o FILE` | Writes one page as a PNG or PPM image; with `all`, `-o` names a directory. With `--frames TICKS` it plays the page into a sequence of images. See "Image sequences" below. |
| `neottx convert INPUT --to t42\|tti\|edittf\|channel -o FILE` | Converts a recording or page files to T42 packets, TTI pages, edit.tf page addresses (one page a line, each `https://edit.tf/#` and the page, which the editor opens), or a static channel's folder. With `--to channel`, `-o` names the folder, which is created: each page as `NNN.t42`, a page of several sub-pages also as a folder `NNN/` holding each sub-page as `NNN/SSSS.t42` and the page's list of sub-pages in parts `NNN/P.sub`, and the bundle `index.t42` (DEC-2413); a file already in the folder is never written over. `--page 100` keeps one page, `--page 100/0001` one sub-page. |
| `neottx slice RAW -o OUT.t42` | Slices raw sampled video into T42 packets and prints quality statistics. `--preset bt8x8\|cx88\|saa7131\|tbc\|tbc-vbi` names a capture device's geometry (bt8x8 by default); `--sample-rate HZ`, `--samples-per-line N`, `--sample-size 8\|16`, `--lines-per-field N`, `--used-lines FIRST-LAST` (counted from 0) and `--first-lines A,B` (the television line number of each field's first line) replace parts of it. A geometry the reader cannot use exits with status 2. |
| `neottx services INPUT` | Lists the teletext streams, languages and subtitle pages of a transport stream. |
| `neottx service-info INPUT` | Prints the broadcast service data of packet 8/30: network, time, programme label and status message. |
| `neottx check FILES...` | Checks TTI page files for authoring problems, a row longer than the page among them. `--descriptor FILE` resolves link targets against a channel's `channel.ttx` or a service's descriptor. |
| `neottx rules-run TABLE INPUTS` | Evaluates a page rule table against timed inputs and writes the script of an image sequence. See "Image sequences" below. |
| `neottx font convert INPUT OUTPUT` | Converts a BDF font of another cell size to a 12 × 10 or 12 × 20 font that `neottx render --font` loads, only where the conversion is exact. See "Fonts" below. |
| `neottx font sets FONT` | Reports, for each of the 55 character sets fonts draw (the standard's 24 and NeoTeletext's own 31), whether a 12 × 10 or 12 × 20 font conforms for it (REQ-FNT-011), how many of its 96 positions it draws, and the first rule it fails; `--library-line NAME` prints the font's `c` line for a library's `fonts.txt`. |
| `neottx font library LIBRARY` | Lists a font library's fonts by title, from its `fonts.txt` (the folder or the file), with each font's file name or address, its SHA-256 and the sets its `c` line states; each malformed line is reported on standard error. No font is read or fetched. |
| `neottx write-text --set SET --page PAGE INPUT OUTPUT` | Writes a TTI page from a UTF-8 text file in the named character set, or with `--language TAG` in the set a language takes. See "Writing pages from text" below. |
| `neottx sound --page PAGE IN.wav OUT.t42` | Writes a sound as the audio profile's packets, with the command that plays it on `PAGE`: IMA ADPCM by default, `--codec pcm` for 16-bit PCM. The WAVE file is 16-bit PCM at 8, 11.025, 16, 22.05, 24, 32, 44.1 or 48 kHz; stereo is mixed to mono. With `--codec opus` the input is instead an encoder's Ogg Opus file, whose packets are carried as they are: mono, 20 ms frames, constant rate, 1 to 160 bytes a packet (for example `ffmpeg -i in.wav -ac 1 -c:a libopus -b:a 16k -vbr off -frame_duration 20 out.opus`); a file or packet outside that is refused with status 3, naming the byte offset of the page or the packet's number (DEC-2448). `--channel 0-3`, `--gain 0-100`, `--loop` and `--repeat N` set the rest. Append the output to a channel's T42 stream, and a receiver that plays sound plays it while that page is open. See `docs/spec/design/audio.md`. |
| `neottx qr TEXT --page PAGE OUT.tti` | Writes `TEXT`, such as a qrkeyboard join address, as a QR code in a page's mosaics, from row 2 unless `--row` says otherwise, so a channel can show a code a phone scans. See DOC-2029. |
| `neottx keyboard WS-ADDRESS` | Makes a phone the keyboard of a channel that takes input: it shows a qrkeyboard QR in the terminal (and with `--qr-page PAGE --qr-output FILE` writes it as a page), pairs with the phone that scans it, and sends what is typed on the phone to the channel's push WebSocket in the input protocol, once the channel has answered its HELLO; it stops if the channel does not answer within 10 seconds. `--pin` pairs with a PIN instead of a key in the QR; `--relay` names another relay. Anyone who can see the code can type. |
| `neottx contents FOLDER` | Writes a channel's `contents.txt` to standard output from a folder of page files (`NNN.tti`, `NNN.t42`, `index.t42`, and a page held only in its folder `NNN/` as `NNN/SSSS.t42` or `NNN/SSSS.tti`): a section for each magazine and a dated entry for each page. Its format is DOC-1868 of `neoteletext-common/PROTOCOL.html`. Exits with 4 when a file was skipped, or when the listing stopped before a page because it would pass the 64 kilobytes or 1,000 lines a contents file may hold. |
| `neottx html FOLDER --address ADDRESS [--public PAGE]...` | Writes a static channel's pages for a web browser into its folder: `index.html`, the small page with its name and an "Open in NeoTeletext" link to `ADDRESS`, and `NNN/index.html` for each page, the same and `noindex`, so that a shared address leads somewhere and not to the host's 404. A page named with `--public` also carries its title, its picture, and its text hidden and declared with the structured data Google documents for paywalled content: all of that is readable by anyone. |

`PAGE` is a page number such as `100`, or a page and sub-code such as `100/0003`. Pages are hexadecimal, as on a television: `1FF` is a valid diagnostic page.

### Exit status

| Status | Meaning |
|---|---|
| 0 | Success. |
| 1 | The requested page was not found. |
| 2 | Invalid usage. |
| 3 | The input could not be read or recognised. |
| 4 | `neottx check` found errors. |

## Inputs

`INPUT` is one of:

- a file;
- `-` for standard input;
- `zxnet:NAME` for a live zxnet feed. The names are `teefax`, `nms-ceefax`, `chunkytext`, `spark`, `ch4` and `test`. `zxnet:` also takes a channel identifier, and a `wss://` address works too: a known feed's by itself, and for any other the last part of its path names the service;
- an `https://` or `http://` address of a channel, its folder ending in `/`, or its `channel.ttx`, such as `https://app.neottx.com/demosite/`. Any address of the channel URL map opens (DOC-1779): `<channel>302` starts at page 302, `<channel>302/0002` at that sub-page, and `<channel>302?name=value` sends the channel's program those parameters with that page, and no other. `<channel>contents` and `<channel>wall` start at the channel's first page, as the viewer shows neither. `<channel>302.tti` and `<channel>302/` read as `<channel>302`. An address with a fragment, such as `<channel>#302`, is refused: on the web a page is part of the path;
  A live channel, whose `channel.ttx` names a push connection, serves its pages only on that connection: every command selects the page it wants there (`text`, `show` and `render` the page given, any of its sub-pages; `list` and the rest no page, which the channel answers with its initial page), and `neottx list` prints the channel's index, each page with its number of sub-pages and its title, before the pages that came. A sub-page's links come from the channel's LINKS records as a page file's link lines would give them, and in `neottx view` Right and Left step through a page's sub-pages as the channel's list of them names (DEC-2445);
- any other `https://` or `http://` address, of a web teletext service descriptor (area WEB);
- a `neottx://` link, which opens the `https://` or `http://` address it wraps, as in `neottx://https://app.neottx.com/demosite/`. `neottx` registers nothing with the operating system: the browser does that. A fragment after the address, which names a page in the browser, is refused here.

The format of a file is detected from its content. `--format` overrides detection:

| `--format` | Format |
|---|---|
| `t42` | 42-byte packets. |
| `t42-reversed` | 42-byte packets with each byte bit-reversed. |
| `t42-framed` | 43-byte packets that start with the framing code. |
| `t42-run-in` | 45-byte packets with clock run-in and framing code. |
| `ts`, `m2ts`, `ts204` | MPEG transport streams with 188, 192 or 204-byte packets (EN 300 472). |
| `pes` | Raw packetised elementary stream. |
| `sliced`, `sliced-be` | Linux sliced VBI records, little or big-endian. |
| `tti` | TTI page files, with the Link Extension (Appendix E). The lines of the withdrawn Form Extension are ignored, with a diagnostic. |
| `ep1` | EP1 page files. |
| `edittf` | edit.tf and zxnet page addresses (the page is in the URL fragment). |
| `dump` | Raw 40 by 25 screen dumps. |

In a transport stream, the first teletext stream is used unless `--pid` names one, or `--program` names the programme whose first teletext stream to use. `neottx services` lists the choices.

### Live feeds and web services

A live input runs in two processes (REQ-SEC-018). A fetcher process does the networking and TLS; the decoder process never touches the network. Both sandbox themselves on Linux with Landlock and seccomp. On macOS the processes are separated but not sandboxed (Phase 2).

- `--duration SECONDS` sets how long the non-interactive commands collect pages from a live feed before acting. The default is 20 seconds, and 600 with `--walk`. `show` and `text` stop as soon as the requested page is complete.
- `--walk` walks a zxnet feed's whole service: the feed asks the server for each page its list of pages names, in turn, holding each until its rows have arrived or its time is up, at the pace a person browsing would go, and the command collects what comes, as in `neottx list zxnet:teefax --walk`. These are hobby servers, so the walk is never faster (REQ-186, DEC-2456). A web service or channel has no walk, and says so.
- Web services on local-network addresses are refused (REQ-SEC-006). `--allow-local 127.0.0.1` allows one address, for example a test server. The risk: any service you open can then make `neottx` contact that address, so allow a test server's address only, never a device you care about.
- `--proxy http://host:port` or `--proxy socks5://host:port` sends every connection through that proxy, TLS still end to end (REQ-WEB-032); with a proxy, `localhost`, names of one label such as `router`, and names under `.local`, `.internal`, `.home.arpa`, `.lan`, `.corp`, `.intranet`, `.private` and `.localdomain` are refused too, since the proxy resolves the rest and only the name can be checked. A proxy that needs a password is not supported. The risk: the proxy sees every host `neottx` contacts, so use only a proxy you chose. The settings a port can change, with their defaults and risks, are listed in `docs/spec/design/porting.md`, "Security settings and their defaults".
- Plain `http://` works, by the owner's decision E.32. The viewer marks pages that arrived unencrypted.
- `--no-sandbox` turns the operating-system sandbox off, for debugging. Every command that reads a file or a feed sandboxes itself once its inputs and outputs are open, and refuses to run when the sandbox cannot be applied. The risk of turning it off: a fault in a reader of the untrusted input can then reach every file and the network you can.
- Offline (REQ-WEB-009): when the network goes, pages already received stay viewable. A page that has not been received or confirmed current for 60 seconds shows "?" in the status area. Asking for a page that was never received shows "Page not available offline". The page on screen is asked for again every 10 seconds, so it is the first one updated when the network returns.

## Outputs

### The terminal

`show` and `view` choose the graphics with `--graphics`:

- `auto` (the default): the terminal is asked, before anything is drawn, with the kitty graphics query, XTVERSION and Primary Device Attributes, and the first it has of kitty, Sixel and iTerm2 is used (iTerm2 also when `TERM_PROGRAM` is `iTerm.app` or `LC_TERMINAL` is `iTerm2`); otherwise text. Its replies are read strictly and bounded, and a terminal that has not answered within half a second gets text. Inside tmux or GNU screen, which may not pass graphics through, it is always text. With `-v` both commands say why text was used (DEC-2459).
- `kitty`: the page is sent as a PNG by the kitty graphics protocol. `view` sends each frame of a page once, its flash phases among them, and then only switches between them.
- `sixel`: the page is sent as Sixel graphics, each palette index its own colour register (DEC-561), so the terminal needs as many registers as the page has colours: up to 144 at Levels 1 and 1.5. When the page changes in `view`, a flash phase, the clock or a row, only the text rows that changed are sent again, as a band placed at its row, where the terminal reports its size in pixels and its rows divide that height exactly; otherwise, and when the page's colours or size change or the terminal is resized, the whole page is sent.
- `iterm2`: the page is rendered at full resolution and sent as a PNG inline image (decision E.37). Every change sends the whole page again: an iTerm2 image cannot be placed in part over another.
- `text`: characters with ANSI colour. Mosaics use the Unicode block mosaic characters; separated mosaics need a font with Unicode 16 separated mosaics, or `--legacy-mosaics` draws them contiguous. `--colours8` uses the eight basic colours instead of 24-bit colour.

`--scale N` sets a whole-number scale factor; 0 fits the terminal. `--aspect` stretches the width by 16/13 (about 1.23) for the proportions of a 4:3 television, on which each pixel was wider than tall (REQ-RND-004). `--reveal` shows concealed text.

### Images

`render` writes PNG (the default, with alpha for transparent cells) or PPM with `--image ppm`. The picture is 480 by 500 pixels at scale 1, or 480 by 250 in the 12-by-10 cell mode of the `saa5246a` profile. The palette is the page's 32-entry Colour Map.

### Image sequences

`neottx render INPUT PAGE -o FILE --frames TICKS` plays one page on the presentation clock, sixty ticks a second, for 1 to 36,000 ticks from tick 0, and writes an image of the page as it is shown at each tick: flash in the phase of that tick (57 ticks shown and 19 hidden for the Level 1 Flash attribute), sprites where the animation has put them, controls with the looks forced on them. Each image is named with `-NNNNN`, the tick in five digits, before the extension, so `-o out/page.png` writes `out/page-00000.png`, `out/page-00001.png` and so on. `--frame-step K` writes an image every `K` ticks, and always the last. No clock is read, so a run gives the same images on every machine and every time; they are for tests and golden files (animation.md ANIM-18).

`--script FILE` (`-` for standard input) is a text file of timed operations, one a line, `<tick> <operation>`, in ascending tick. An empty line, and a line beginning with `#`, is skipped.

| Operation | What it does |
|---|---|
| `move <n> <x> <y> [relative]` | Puts sprite `n` at column `x`, row `y`, or moves it by that much |
| `show <n>`, `hide <n>` | Shows or hides it |
| `look <n> <0-3>` | Forces a look: 0 none, 1 hover, 2 pressed, 3 alternative |
| `order <n> <layer\|keep> <order>` | Sets its layer, 0 to 3, or keeps it, and its order within the layer |
| `frame <n> <f>` | Shows frame `f`, 0 to 15, and stops a cycle |
| `cycle <n> <first> <last> <period> [once]` | Cycles its frames, one every `period` ticks |
| `stop_cycle <n>`, `reset <n>` | Stops a cycle; forgets everything the script did to it |
| `record <type> <hexadecimal body>` | Applies a record at that tick: `ANIMATE`, `SPRITES` or `FRAME` |

`neottx rules-run TABLE INPUTS` makes such a script from a page rule table (page-rules.md): the table is the program protocol's `rules` message as JSON (RULE-35), which names its own page, or a table's image as bytes or as hexadecimal text, for the page `--page` names. It evaluates the table against a file of timed inputs and writes the animation operations its groups' commands make, with the table's reports and its other commands as comment lines. The inputs are one a line in ascending tick: `<tick> key down|up char|name|physical <key>`, `<tick> pointer <x> <y> <buttons>`, `<tick> level <audio channel> <0-255>`, `<tick> rectangle <n> <x> <y> <width> <height>` (object `n`'s bounding rectangle for the table's pointer and collision tests), `<tick> cue <audio channel> <n>` (a cue heard) and `<tick> command <record type> <hexadecimal body>` (a `VARIABLE`, `TIMER`, `REPORT`, `RUN` or `SEQUENCE` record the server sends directly). As in the browser, each input acts at once when it is given (DEC-6635): the lines of tick `t` are given after tick step `t` has run, in file order, and their outputs follow that tick step's. `--frames TICKS` sets how many ticks are evaluated (by default, to the tick after the last input), and `--picture teletext|regis|naplps` whose picture the page is (a teletext page and a NAPLPS picture are both the 480 by 500 dots of the raster, positions of a teletext sprite in cells and of a NAPLPS object in dots; a ReGIS picture is 800 by 480 pixels). The output is a script for `neottx render --frames` and for the ReGIS and NAPLPS renderers' programs, so a rule-driven page can be played into images without the browser:

```
neottx rules-run table.bin inputs.txt | neottx render page.t42 150 --frames 120 --script - -o out/page.png
```

`<n>` is a sprite's number; `control:<n>` names the page's `n`th control, for `show`, `hide`, `look` and `reset`. Sprites are placements of the page's objects, so they are drawn at Level 2.5 and above by a `neottx` built with the `objects` feature; a build without it says so when a script gives a page sprites.

### Text

`text` writes the page as UTF-8, one line per row. Mosaics become Unicode block mosaics and national characters their Unicode equivalents. At Levels 2.5 and 3.5 G3 smooth mosaics and line-drawing characters become the Unicode characters for them (REQ-CHR-011).

### Levels 2.5 and 3.5

At the default level, 3.5, `neottx` also keeps the enhancement pages a service sends (object pages and the Magazine Organization Table) and the magazine packets M/29, and draws each page with them: the page's own Colour Map of 32 colours (CLUTs 2 and 3 from X/28/0 and M/29/0, and at Level 3.5 CLUTs 0 and 1 from X/28/4 and M/29/4), screen and row colours, the non-spacing attributes (colours, double width and double size, underline, invert and the rest of Table 27) and the G3 set. Objects and the Additional Flash Functions are drawn by a build that names them (above). Side panels are never drawn: a page is always 40 columns (decision DEC-1636). Downloadable characters (DRCS) are not drawn: the Level 1 character beneath shows (decision E.95). Nor are the Level 3.5 font styles, bold, italic and proportional spacing (decision E.96). A fault in any of these removes only what depends on it; the Level 1 page always shows. Reading a channel or a web service, `neottx view` asks the service, once each, for the Organization Table of the shown page's magazine (page xFE) and the object pages the table and the page name, when it does not hold them already; the page is drawn with what has come and again as the rest arrives (REQ-511). `--level 1.5` shows a page as a Level 1.5 receiver would, without re-acquiring anything. `cargo xtask level25-stream FILE.t42` writes a test stream with a page for each feature, indexed on page 100.

## The viewer

`neottx view INPUT` opens on the initial page (the service's own, or 100).

| Key | Action |
|---|---|
| `0` to `9` | Enter a page number. Where the page shown has its own link for the number (an `LK` line), or else the channel's `channel.ttx` has an alias for it, the number goes there (REQ-509, DEC-2480). |
| `r` `g` `y` `c`, or F1 to F4 | Follow the red, green, yellow and cyan links. They work only while row 24 is shown in full (REQ-SEC-024). |
| `i` | Index link. Held, where the terminal reports key releases (the Kitty keyboard protocol): back through the history (DEC-2477). |
| `h` or Home | Initial page. |
| Up or `+`, Down or `-` | Next and previous page. |
| Right and Left | Next and previous sub-page. |
| `s`, then four digits | Show that sub-page, keyed as on a television: 0001 to 3979, the third digit 0 to 7 and the first 0 to 3 (REQ-NUM-017). A digit that cannot belong is ignored, and any other key leaves the entry. |
| Alt+Left or Backspace, Alt+Right | Back and forward through the history. `--back-key KEY` and `--forward-key KEY` add a key of your own for each, as a remote control's configurable keys: one printable character the viewer does not already use. |
| `?` | Reveal concealed text. |
| `H` | Hold the page, so that new receptions do not update it. |
| `m` | Mix: cell backgrounds become transparent, as when text is superimposed on the picture. |
| `z` | Zoom: top half, bottom half, off. |
| `q`, Esc or Ctrl+C | Quit. |

Where the terminal reports key releases, `i` acts when it is let go: let go at once it is Index; held until the keyboard repeats it, or for half a second, it goes back, once a hold. On a terminal that reports no releases a hold cannot be told from a press, so `i` is Index at once and Back is Backspace, Alt+Left or the `--back-key`.

A typed number whose link or alias leads to another service asks in row 24, `Enter: Weather at weather.example`, unless that origin was confirmed already; Enter goes there and any other key stays. An alias to another channel's address is not opened: row 24 says the page is another channel's, and the address is printed when the viewer ends.

The history keeps the service of each page. Following a link to another service keeps the pages before it, so Back returns across the link to the service and page you left, and Forward goes on to the service followed again; each is opened again without a question, since its origin was confirmed or you opened it yourself. The history holds 32 pages, the oldest dropped first, and goes when the viewer ends.

A link to another service asks for confirmation: the status area shows `LEAVE?`, and row 24 the service's name and its origin, as in `Same key: Weather at weather.example` (the scheme is shown when it is not `https`), and pressing the same key again goes there; any other key stays. Once an origin is confirmed it is not asked about again for as long as the viewer runs, in any service (REQ-288, DEC-2454).

With `-vv` the viewer writes the packet trace and the state machines' transitions, as the other commands do, to standard error; since the page is drawn on the terminal, standard error must be redirected for them to be written (`neottx view -vv INPUT 2>trace.txt`). The walk of a page's presentation level is traced once for each reception of the page shown, and the viewer's own machine reports each key and decoder event that changes its state or does something. With `-vvv` each cell's step of the row attribute machine is traced as well: a thousand lines a page. Without the redirection the trace is dropped, and the viewer says so when it ends.

On a channel with a push connection, the viewer greets the channel when its WebSocket opens, and once the channel answers, the keys are the page's, as a terminal's keys are its host's: digits, Escape and Ctrl+C included. What reaches the channel is what the page asked for. Each page of a channel has a rule table (the channel's server sends it before the page; `neoteletext-common/docs/page-rules.md`), and a key, a paste or a change of focus goes to the channel only inside the report of a rule of that table that asked for it, at the table's next tick, a sixtieth of a second away at most; a page with no table is sent nothing the viewer types. A channel whose configuration says `"default_rules": "send_all"` gives every page a table that sends every key, text and focus change, which is what the channels deployed before the rule table do. The descriptor's `push.input` means nothing any more. Printable keys go as text and the rest as key events, with Shift, Control and Alt. The channel decides what they mean; videotex channels take `*nnn#` for a page, `#` for the next and `*#` for the one before. A paste goes as one paste, and the terminal's focus as the page's, where the terminal reports them; in a terminal with the Kitty keyboard protocol, releases and repeats go too, and in any other a key goes as a press alone. A terminal has no pointer, plays no sound and draws no sprite, so a table's rules on those do nothing in `neottx view`; its key, text, focus and timer rules, its variables and its state query work as in the browser. The channel is told the page or sub-page selected whenever it changes, whatever the focus, and every report is stamped with the page and sub-code the viewer is drawing. Keys stop going when the terminal loses the focus or after Ctrl+], which leaves the keys to the viewer and gives them back: the channel is told the focus went, with a lost release of each key held where the terminal reports releases. A channel that does not answer within 10 seconds is said not to take input. neottx keeps no cookies and no viewer key: when a channel asks to know the viewer (a viewer-state challenge), `neottx` and `neottx keyboard` answer that they keep no state, so a channel sees them as a new viewer each time (DEC-6471, VS-501). See DOC-1781 of `neoteletext-common/PROTOCOL.html`.

Row 0 columns 0 to 7 always belong to the receiver: they show the page number being searched for and the security indicators, which a page cannot forge (REQ-SEC-005).

The status area in row 0's first seven columns shows the page number, and (DEC-2349):
- on a page whose sub-pages rotate, how many are stored, after `+`: `P100+12`;
- on a sub-page chosen with Left, Right or a link, its place among those stored, after `/`: `P100/03`;
- `?` in place of the `+` or `/` when the page has not been received again for a while (`P100?03`), or after the number (`P100 ?`).

A place or count of three digits gives up the `P` (`100/123`), and one of four digits gives up the page number (`P/8190`), whose header still shows it.

A page may have up to 3,199 sub-pages (area NUM). Right and Left step through those stored, and the sub-page key followed by four digits asks for one by its number (DEC-2363). On a channel whose page keeps its sub-pages in a folder (`NNN/SSSS.t42`, with its list of sub-pages in parts), Right and Left step through every sub-page the list names, in ascending order and wrapping at either end, whether or not the viewer holds it: each is fetched as it is reached, and the parts of the list only as stepping needs them (REQ-328). A sub-page the viewer already holds is shown without asking for it again while the list says its file has not changed since it came (REQ-523).

## Checking a channel

`neottx probe CHANNEL` opens a channel the way a receiver does and says what happened at each step: the entry file, the push connection's handshake and subprotocol, the input protocol, and each sample selection and how it ended, with a page or with DENIED and its status. It selects the place the address names, or the initial page; then each `--select PLACE`; and then a page few channels have, to see a selection refused. A local server needs `--allow-local 127.0.0.1`.

The exit status is 0 when nothing was wrong. It is not 0 when the entry file cannot be read, the upgrade is refused or names another subprotocol, the channel closes the connection with a stop code, HELLO is not answered, a selection is refused with status 503, or a selection has no ending within the stated deadline (DOC-1920 and DOC-1921).

## Options for the decoder

Every command accepts these options. Each one changes a setting that a decision of Part E (in rdm) defines.

| Option | Setting | Default |
|---|---|---|
| `--profile NAME` | The compatibility profile (area PRF). | `en300706` |
| `--region N` | The character set group of Table 32, 0 to 15, used when the page does not give one (decision E.4). It stands above a channel's `region`. | From the channel's `region` or the transport stream's language, otherwise 0 |
| `--parity keep-good\|space\|marker` | How characters that fail their parity check are shown (decision E.8). | `keep-good` |
| `--level 1\|1.5\|2.5\|3.5` | The presentation level, overriding the profile's (REQ-CLI-007, decision E.40). | The profile's level |
| `--no-rounding` | Turns character rounding off (REQ-RND-012). | Rounding on |
| `--single-start-box` | Accepts a single Start Box code (decision E.17). | Two required |
| `--dump-repeated-rows` | In a raw screen dump, leaves out a row that repeats, byte for byte, the double-height row above it, as the BBC Microcomputer's mode 7 dumps hold it; teletext ignores the row under a double-height row (REQ-INP-017). | Off |
| `--variant LETTER` | The SAA5246A's national option ROM variant, E, H, T, J, K, L, I or S: C12 to C14 choose among its sets alone, a combination it lacks gives its default language, and X/28 and M/29 are not read (REQ-PRF-003). | E under `saa5246a`; none otherwise |
| `--colours LIST` | Your own colours in place of Table 30's, before any page redefines them (REQ-RND-005): up to 32 colours separated by commas, each three hexadecimal digits of red, green and blue, replacing the Colour Map from entry 0. The first eight are the Level 1 colours, which the status area and a row 24 the receiver draws also take; entry 8 stays transparent. For example `--colours 000,F00,0F0,FF0,00F,F0F,0FF,CCC` makes white a light grey. | Table 30's colours |
| `--pages N` | Pages and sub-pages kept in memory. | 4096 |
| `-v`, `-vv`, `-vvv` | Diagnostics, and a channel's log lines (`LG`, REQ-INP-026); `-vv` adds a line for every packet read, `packet M/N/D: fields; protection`, with the fields the decoder read from it (a header's page, sub-code and control bits, a row's parity errors, a designation code) and what its Hamming codes corrected or could not (REQ-CLI-002), and the state machine trace (REQ-FSM-010); `-vvv` adds each cell's step of the row attribute machine (REQ-FSM-006). | Off |

### Profiles

| Setting | `en300706` | `saa5050` | `saa5246a` |
|---|---|---|---|
| Presentation level (E.40) | 3.5 | 1 | 1.5 |
| X/26/15 processed (E.2) | Yes | Yes | No |
| Character set | X/28, M/29 and C12 to C14 | Fixed: Latin English, whatever the page designates | The ROM variant's set for C12 to C14 (`--variant`, /E by default) |
| Black colour codes 0/0 and 1/0 (E.18) | Black | No function | Black |
| Row 24 (E.23) | Annex H flag | Never | Whenever X/24 is present |
| Cell size | 12 by 20 | 12 by 20 | 12 by 10 |
| Rounding | On | On | Off |
| Reveal | Latched | Momentary, 160 ms | Latched |

### Every Part E setting

The library's `neoteletext_core::config::Config` holds every setting below. `neottx` exposes the ones in the table above through options and profiles; the rest keep the default.

| Field | Decision | Default |
|---|---|---|
| `level` | REQ-PRF-006, E.40: Level 1, 1.5, 2.5 or 3.5 | Level 3.5 |
| `process_x26_15` | E.2 | On |
| `strict_row_setting` | E.3: the clause 12.3.2 rule instead of Annex A.3 | Off |
| `ignore_out_of_order_triplets` | REQ-L15-009 | Off |
| `default_region` | E.4 | 0 |
| `fixed_designation` | REQ-PRF-002: one Table 32 value for every page, no designation read | None |
| `rom_variant` | REQ-PRF-003: the SAA5246A ROM variant whose sets C12 to C14 choose among | None |
| `parity_policy` | E.8 | Keep the last good character |
| `rolling_header` | E.11 | On |
| `flash_shown_ticks`, `flash_hidden_ticks` | E.12: 3 to 1, in ticks of one sixtieth of a second | 57 ticks (950 ms), 19 ticks (317 ms) |
| `rounding` | REQ-RND-012 | On |
| `double_start_box` | E.17 | On |
| `black_colour_codes` | E.18 | On |
| `page_completion_timeout_us` | E.19 | 100 ms |
| `row24` | E.23 | Annex H |
| `framing_tolerance` | E.24 as DEC-1092 limits it: accept one wrong framing-code bit on a line that carried three packets with the exact code within a second, each within a second of the one before; off, the exact code on every line | On |
| `store_hexadecimal_pages` | REQ-ACQ-013: also store pages with a hexadecimal digit, for diagnostics; never the reserved numbers mFD, mFE, mFF, 1BE and 1F0, whose tables are kept as received apart from the pages (DEC-2341) | Off |
| `stale_after_us` | REQ-ACQ-012 | 60 s |
| `reveal`, `momentary_reveal_us` | REQ-NAV-007 | Latched; 160 ms |
| `step_order` | REQ-NAV-002 | Numerical |
| `cell_mode` | REQ-RND-002, REQ-RND-003 | 12 by 20 |
| `trace` | REQ-FSM-010 | Off |
| `status_on_suppressed_header` | REQ-L1-014 | Off |
| `row_set_triplets` | REQ-2180, DEC-2179: NeoTeletext's row set triplet, column mode 01010, read; off, the mode is reserved as Table 29 has it | On |

## Examples

```sh
neottx list recording.t42
neottx show recording.t42 100
neottx view broadcast.ts --pid 0x46
neottx text zxnet:teefax 100
neottx render recording.t42 all -o pages/
neottx convert broadcast.ts --to t42 -o broadcast.t42
neottx view https://example.org/service.json
neottx view https://app.neottx.com/demosite/
neottx check pages/*.tti --descriptor service.json
neottx sound --page 100 chime.wav chime.t42 && cat pages.t42 chime.t42 > channel.t42
```

## Pictures to pages: `neottx-picture`

`neottx-picture` turns a picture into a teletext page of mosaic graphics (DEC-2325, REQ-2337). It is a program of its own, not part of `neottx`:

```sh
cargo build --release -p neoteletext-picture-cli
target/release/neottx-picture photo.jpg page.tti --t42 page.t42 --level 3.5
neottx render page.t42 100 -o page.png
```

It reads PNG, JPEG, GIF, BMP and WebP files of up to 64 MiB and 32,768 pixels a side, turns a photograph upright by its orientation tag, and writes page 100 as a TTI file, and as T42 with `--t42`. Its options:

| Option | What it does |
|---|---|
| `--level 1\|1.5\|2.5\|3.5` | The presentation level; 3.5 by default. Levels 1 and 1.5 use the eight colours and spacing attributes; 2.5 and 3.5 redefine colours and change them with X/26 triplets. |
| `--objects` | Levels 2.5 and 3.5: carry the colour changes in Active objects on page 1A0, which holds far more of them. The default `neottx` build does not show objects; build it with `--features objects`. |
| `--hold-mosaics` | Levels 1 and 1.5: an attribute's cell shows the last mosaic rather than the background. |
| `--display neottx\|tv` | The shape to crop for: square dots as `neottx` draws them (the default), or a 4:3 television's. |
| `--smooth area\|lanczos` | The scaling filter: the mean of each area (the default), or a sharper Lanczos filter. |
| `--fit crop\|contain\|stretch` | Fill the page and cut off the rest (the default), show the whole picture between bars, or distort it to fill. |
| `--align ALIGN` | Which part a crop keeps, or where the picture sits between bars: `center`, `top`, `bottom`, `left`, `right`, `top-left`, `top-right`, `bottom-left` or `bottom-right`. |
| `--background COLOUR` | The colour of bars and of transparent areas: a Level 1 colour name or `#RRGGBB`; black by default. |
| `--pixel-art` | Each pixel is one sixel, enlarged by the largest whole number that fits; a picture over 80 by 75 is refused. |
| `--no-auto-orient` | Take the picture as stored, ignoring its orientation tag. |
| `--crop X,Y,WIDTH,HEIGHT` | Cut this region out first. |
| `--no-upscale` | Never enlarge: a small picture keeps its size on bars. |
| `--focus auto` | Crop where the picture has the most detail. |
| `--keep-header` | Draw rows 1 to 24 only and leave row 0 to the page's header. |

With `--to regis` or `--to naplps` it writes the picture as a ReGIS or NAPLPS picture instead of a page (renderer parity, REQ-6763): fitted to the language's canvas (ReGIS the VT340's 800 by 480 screen; NAPLPS the 480 by 460 display area, lines 20 to 479 of the teletext raster, that a NAPLPS page is drawn on), reduced to `--colours N` colours (2 to 16, 16 by default) and drawn as filled rectangles. ReGIS is shortened by the ReGIS renderer's minifier; both are checked by drawing them before they are written, and a picture over 262,144 bytes is refused. `--t42` and `--level` are for pages only.

```sh
target/release/neottx-picture photo.jpg photo.regis --to regis --colours 16
target/release/neottx-picture photo.jpg photo.nap --to naplps
```

Options that contradict each other, such as `--hold-mosaics` at Level 3.5, are refused with the reason. The library's `keep_footer` option, which leaves row 24 to the page, and its `convert_raster`, which takes pixels already decoded, are what a server's mosaic fallback uses. The library behind it, `neoteletext-picture`, takes the picture's bytes and returns the page, so other programs can use it; `docs/spec/design/pictures.md` explains how it works.

## Pictures: ReGIS and NAPLPS

`neottx render`, `show`, `view` and `check` take ReGIS and NAPLPS pictures as well as teletext pages (renderer parity, REQ-6763):

- a ReGIS file, `.regis`, of ReGIS commands with or without the device control string `ESC P p … ESC \`;
- a NAPLPS file, `.naplps` or `.nap`;
- a `.t42` file, such as a static channel's sub-page, whose records carry a picture for the page named.

A picture is drawn as the browser draws it. ReGIS fills the VT340's 800 by 480 screen in its colour map, with numbered drawings composed; its alphabet 0 is the VT340 font compiled in, or `--regis-font FILE`. NAPLPS is drawn on the teletext raster, 480 by 500: its display area is lines 20 to 479, teletext rows 1 to 23, and rows 0 and 24 are in the picture's screen colour; every place a NeoTeletext NAPLPS page gives, such as an object's rectangle, is a dot of that raster, with the stroke font compiled in or `--naplps-font FILE`. Text in either language may use the 55 provided character sets, drawn in Noto Teletext, with the plain Arabic-script sets' letters joined. A sub-page's NAPLPS objects are composed over its picture. `show` and `view` need a terminal that shows images (kitty, Sixel or iTerm2); a sub-page's picture from a `.t42` file is shown with row 0 of its teletext page above it and row 24 beneath, as the browser lays it out (a ReGIS picture between the two rows at its width, a NAPLPS picture with them on lines 0 to 19 and 480 to 499, outside its display area).

`neottx view` on a live channel draws the pictures the channel sends, and says so in HELLO, offering the ReGIS and NAPLPS capabilities, when the terminal shows images; in a terminal of characters it offers neither and the channel sends its mosaic page instead. The one-shot commands (`render`, `show`, `text`) offer neither, and show that mosaic page. No `neottx` command offers SNAPSHOT (capability bit 25): that capability is the browser's, for the pictures of its tabs, and asks a channel for one page as a stranger sees it.

`neottx check` reads a picture as a NeoTeletext server reads it and prints what it finds: for ReGIS a failed commit, with its code and drawing, or a limit reached; for NAPLPS each refusal and the bytes passed over. It exits with status 4 when there is a refusal.

`neottx convert --to channel --page 200/0001` carries a picture in that sub-page's file: `--regis FILE` or `--naplps FILE`, with `--links FILE`, a JSON array such as `[{"drawing": 3, "target": "300"}]` (`"object"` for a NAPLPS object), and `--cursor N`, the drawing or object that is the sub-page's cursor. A NAPLPS picture's objects are `--objects FILE`, the JSON of the program protocol's `naplps_objects` message without `type`, `connections`, `page` and `sub`, such as `{"objects": [{"number": 7, "kind": "region", "shown": false, "detectable": true, "layer": 1, "order": 0, "x": 0, "y": 0, "width": 200, "height": 200, "origin_x": 0, "origin_y": 0, "frames": [], "cover": 3, "label": "A door"}], "title": "The harbour"}`. A picture over 1,048,576 bytes is refused, as a server refuses to keep one. `neottx html` draws a public page whose sub-page carries a picture as `render` draws it, writes it beside the page's HTML as `NNN/picture.png`, and gives the picture's title as its `alt` text.

```sh
neottx render map.regis 100 -o map.png
neottx check map.regis weather.nap
neottx convert pages.tti --to channel -o site --page 200/0001 --regis map.regis --links links.json
neottx render site/200.t42 200/0001 -o map.png
```

## Glyphs

Pages are drawn in Noto Teletext, the one compiled font (REQ-FNT-001, DEC-1761): the Noto fonts converted to teletext's 12 × 20 cell, drawn line for line. It holds every character of the standard's 24 sets and of neoteletext's own sets, with every letter a base and a diacritical mark compose to. A mark on a letter Unicode does not compose is laid over the letter, and a character the font does not hold is drawn as a hollow box. `neottx render --font` draws in another font: a 12 × 10 or 12 × 20 BDF file (area FNT).

## Fonts

Pages are drawn in Noto Teletext unless another font is named. `neottx render --font FILE` draws a page's text in a loaded font: a BDF file of 12 × 10 or 12 × 20 cells, encoded in Unicode, with at most 1,024 characters (REQ-FNT-002); anything else is refused with the line at fault. A character the font lacks is drawn in Noto Teletext. Row 0 is always drawn in Noto Teletext, so that a loaded font cannot change what the status area says. A channel or a page cannot name a font (decision E.111): `channel.ttx` refuses a `font` member, and the page checker reports a TTI `TF` line as a withdrawn command.

A font library is a folder with a `fonts.txt` index listing fonts by title, each with its SHA-256 (`f`, the file name or address, the title and the checksum, separated by tabs). `third_party/noto/` is such a library, and so is `third_party/misc-fixed/`, which holds the X11 misc-fixed 6x10 font at 12 × 20: load `third_party/misc-fixed/misc-fixed-6x10-12x20.bdf` with `neottx render --font` to draw a page in it, code page 437's box-drawing and block characters included. The windowed browser that chooses fonts from libraries is maintained in the separate neoteletext-browser repository.

### Character sets and conformance

A teletext decoder held a glyph at every one of the 96 code positions, 2/0 to 7/F, of each character set it supported. A font *conforms* for a character set when, by the rules of REQ-FNT-011: it is a valid 12 × 10 or 12 × 20 font; it has a glyph for every character the standard assigns to the set's positions; its space is blank, which also draws the positions the standard leaves empty; and none of its characters is blank. The rules look only at the glyphs, never at the font's name. There are 55 sets: the standard's 24 (G0 Latin and its 13 national options, the three G0 Cyrillic options, G0 Greek, Arabic and Hebrew, and G2 Latin, Cyrillic, Greek and Arabic) and NeoTeletext's own 31 ("Languages" below). A font conforming for a set may also meet the *SAA5050 cell* profile: every glyph leaves the top line and the leftmost dot column blank, as the chip did. `neottx font sets FONT` reports all of this:

```text
G0-Cyrillic-2                      96/96  conforms, SAA5050 cell
G0-Cyrillic-3                      90/96  rule 2: no glyph at 2/6
```

A library's `fonts.txt` may state each font's conformance on a `c` line: `c`, the font's file name or address as its `f` line gives it, and the sets separated by spaces, each followed by `*` for the SAA5050 cell profile. `neottx font sets --library-line NAME FONT` writes that line. A program can use it to list fonts before loading them, and recomputes conformance from the glyphs once a font has loaded.

`neottx font library LIBRARY` lists a library: one line per font, giving its title, its file name or address, its SHA-256 and the sets its `c` line states, separated by tabs. A malformed line (a field missing or extra, a title of more than 32 characters, a checksum that is not 64 hexadecimal digits, a second `f` line for a font, a `c` line for a font no `f` line lists or a second one for a font, or an unknown set name) is reported with its number on standard error and ignored; a line of an unknown kind is ignored. An index over 64 kilobytes or 1,000 lines is refused. Listing a library reads only its index: no font is fetched (REQ-FNT-004).

`neottx render --font FILE --font-sha256 HEX` draws in the font only when the file's SHA-256 is the one named, as a library gives it; a file with any other is refused before it is read (REQ-FNT-003). `--font` also takes an `http` or `https` address, such as one a library's `f` line gives, and then needs `--font-sha256` (REQ-FNT-003, DEC-2481): `neottx` starts its confined fetcher process for that one font, which fetches it under the rules of a channel's files (no cookie; a local-network address only with `--allow-local`; no fall from `https` to `http`; the media type `application/x-font-bdf` or `application/octet-stream`; at most 256 kilobytes), and checks the checksum itself before reading the font. A font that came over plain HTTP is said to on standard error.

`neottx font convert INPUT OUTPUT` makes a loadable font from fonts of other sizes, only where nothing is lost (REQ-FNT-010, decision E.77):

| Source cell | What the converter does |
|---|---|
| 6 × 10, 5 × 10 | Makes every pixel two pixels wide; a 5-pixel font sits at the right of the 12-pixel cell, leaving the blank column at the left, as the SAA5050 does. |
| 6 × 9, 5 × 9 | The same, below one blank line at the top, where the SAA5050 leaves it. |
| 12 × 20 | Keeps every second line, making a 12 × 10 font, when every pair of lines of every glyph (lines 1 and 2, 3 and 4, and so on) is identical; otherwise writes the font unchanged as a 12 × 20 font, such as one drawn with character rounding, since 12 × 20 fonts load as they are. |

Any other cell, or a glyph larger than its cell, is refused, naming the first character that fails. A font whose `CHARSET_REGISTRY` is not ISO10646 needs `--map FILE`: one line for each character, its code as the font's `ENCODING` writes it, a tab, and its Unicode code point:

```
65	U+0041
35	U+00A3
```

`FAMILY_NAME`, `COPYRIGHT` and `NOTICE` are carried over. The converter reads only its two files and never the network, and runs under the sandbox on Linux.


## Writing pages from text

`neottx write-text --set SET --page PAGE INPUT OUTPUT` writes a TTI page from a UTF-8 text file in one character set (REQ-95, decision E.82). The build needs the feature `write-text`, on by default; without it the command says writing pages from text is not in this build.

- `--set SET` names the set: `english`, `german`, `swedish-finnish-hungarian`, `italian`, `french`, `portuguese-spanish`, `czech-slovak`, `polish`, `turkish`, `serbian-croatian-slovenian`, `romanian`, `estonian`, `lettish-lithuanian`, `cyrillic-serbian-croatian`, `russian-bulgarian`, `cyrillic-ukrainian`, `greek`, `hebrew` and `arabic-table44` (the standard's), and the sets of NeoTeletext's own listed under "Languages" below. `neottx write-text --help` prints the list the build has.
- `--language TAG` names a language instead, as a BCP 47 tag such as `ru`, `sr-Latn` or `ja-Latn`, and the engine's table of languages gives its set (REQ-2181).
- `--level neoteletext`, the default, writes for NeoTeletext's own receivers, which also read row set triplets; `--level 3.5` uses only the means of the standard, for Level 3.5 and below (REQ-2151).
- `--page` is the page number, 100 to 8FF. The text file may be at most 16 KB.

The page designates its set, in packet X/28/0 Format 1 and the header's control bits C12 to C14. Each line of the text starts a row, from row 1, and a line longer than 40 cells is broken into rows, at spaces where it can. Each character is written by the first of these means that can write it:
1. a code of the page's set in the row;
2. after an ESC, a code of the page's second set;
3. a letter in the row with its accent or tone as an X/26 diacritical mark over it, as Vietnamese and Igbo need;
4. a letter found only in a G2 set as an X/26 character, over a stand-in that a Level 1 receiver, which ignores X/26, shows instead: D for Đ, d for đ, N for Ŋ, n for ŋ, O for Ø, o for ø, L for Ł, l for ł, i for ı and s for ß, and a space for any other;
5. at `--level neoteletext`, a row set triplet that starts a run of the row in another set (REQ-2180).

A character none of these can write is refused, naming the character and its line. Text in the Arabic-script sets is broken into rows and then put in visual order, right to left, with runs of digits and Latin left to right and brackets mirrored; the receiver joins the letters (REQ-84). Kana written as half-width characters, or with combining voicing marks, is taken as the full-width voiced kana. A page whose X/26 triplets would need more than the sixteen packets X/26 carries, or whose text needs more rows than the page has, is refused. On success `neottx` reports the rows used and the X/26 triplets written.

```sh
neottx write-text --set vietnamese --page 100 tin-tuc.txt 100.tti
neottx write-text --language uk --page 120 novyny.txt 120.tti
```

With `--picture regis` or `--picture naplps` the text is written as a ReGIS or NAPLPS picture instead (REQ-6763): 40 columns on rows 1 to 23, each line broken at spaces as for a page, in the provided set of `--language` (Latin English when none is named) and Latin English for what that set cannot show. A character no set shows is written as `?`. ReGIS cells are 20 by 20 pixels, row `r` at y = 10 + 20 (r − 1); NAPLPS cells are exactly the teletext cells of rows 1 to 23, 12 dots by 20 lines of the raster, so each character is one Noto Teletext cell: the picture selects six-byte operands, its text field is 3,276 by 4,452 steps of 1/131072 of the unit screen, a hair under one cell each way, and each row is written from the lower left of its first cell. `--page` is not used, except with `--to channel`: then OUTPUT is a channel's folder and the picture is written as the sub-page `--page` names (`200` or `200/0001`), as `convert --to channel` carries one.

```sh
neottx write-text --language el --picture naplps --page 100 news.txt news.nap
neottx write-text --language el --picture regis --to channel --page 200/0001 news.txt site
```

## Languages

Beyond the standard's character sets, NeoTeletext defines sets of its own, so that languages the standard never served can be written on a teletext page and shown (decisions E.77 and E.78, REQ-81). A page selects one by its designation, a group and a national option of Table 32 of EN 300 706: in packet X/28/0 Format 1 or M/29/0, or, for the national option alone, with the group from `--region` or the channel. The code tables, position by position, are DOC-2028 of `neoteletext-common/PROTOCOL.html`, and DOC-2182 and DOC-2183 there explain how a page's G0 and G2 sets, glyphs and stand-ins work together.

| Set (`--set`) | Group and option | Languages |
|---|---|---|
| `hausa-fula` | 0101 000 | Hausa, Fula |
| `west-african` | 0101 001 | Akan, Bambara, Dyula, Dinka, Fon, Krio, Lingala, Mandinka, Mende, Nuer |
| `ewe` | 0101 010 | Ewe |
| `kabyle-tamazight` | 0101 011 | Kabyle, Central Atlas Tamazight (Latin script) |
| `azerbaijani` | 0101 100 | Azerbaijani |
| `kanuri-mossi` | 0101 101 | Kanuri, Mossi |
| `igbo` | 0101 110 | Igbo |
| `vietnamese` | 0101 111 | Vietnamese |
| `ukrainian` | 0111 000 | Ukrainian |
| `belarusian` | 0111 001 | Belarusian |
| `macedonian` | 0111 010 | Macedonian |
| `serbian` | 0111 011 | Serbian (Cyrillic script) |
| `kyrgyz-mongolian` | 0111 101 | Kyrgyz, Mongolian |
| `tatar` | 0111 110 | Tatar |
| `tajik` | 0111 111 | Tajik |
| `persian` | 1001 000 | Persian, Dari |
| `urdu` | 1001 001 | Urdu |
| `pashto` | 1001 010 | Pashto |
| `sindhi` | 1001 011 | Sindhi |
| `sorani` | 1001 100 | Central Kurdish |
| `uyghur` | 1001 101 | Uyghur |
| `balochi` | 1001 110 | Balochi |
| `arabic` | 1001 111 | Arabic, with letters joined by the receiver |
| `georgian` | 1011 000 | Georgian |
| `armenian` | 1011 001 | Armenian |
| `tifinagh` | 1011 010 | Standard Moroccan Tamazight, Central Atlas Tamazight (Tifinagh script) |
| `japanese-romaji` | 1101 000 | Japanese in rōmaji |
| `hiragana` | 1101 001 | Japanese in hiragana |
| `katakana` | 1101 010 | Japanese in katakana |

The engine's table of languages (REQ-2181), which `--language` reads, also maps the languages the standard's own sets serve, such as Indonesian, Swahili and Kazakh. Thai, Lao and Yoruba are not served (decisions DEC-2239 and DEC-2240).
