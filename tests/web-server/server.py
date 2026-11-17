#!/usr/bin/env python3
"""A local web teletext test server (REQ-WEB-013, REQ-WEB-012).

Test code, not part of the library. Serves a service descriptor, TTI pages
with ETag validators, a versioned session cookie that a NeoTeletext receiver
must ignore (and, at /sessions, how many requests brought it back, which must
stay none: the viewer-state design, VS-201; at /requests, the paths asked for), WebSocket push,
an ordinary web page at /web/ that points to the regional channel by a Link
header field, a <link> element and, for a receiver asking for teletext, a 303
that varies by Accept (REQ-501), a channel published as a folder at /channel/
(its settings, its bundle index.t42, contents.txt, page files, a page whose
sub-pages are files of their own with its list in parts, a 404 for any other
file, and a redirect kept inside the folder: REQ-501's guidance), and fault
injection (/fault/slow?seconds=N waits N seconds, 40 by default). Standard
library only.

Usage: server.py PORT
"""

import base64
import hashlib
import http.server
import os
import socketserver
import sys
import threading
import time

# REQ-WEB-012: the session lives in a cookie whose value starts with a
# format version.
SESSION_VERSION = "v1"
# The push connection's subprotocol: PUSH_SUBPROTOCOL in the engine, which
# the client insists on (DOC-1917).
SUBPROTOCOL = "neottx.7"


def esc(code):
    """A control code in the TTI escape form (REQ-INP-013)."""
    return "\x1b" + chr(code + 0x40)


# A channel that gives its pages' group (REQ-WEB-002): Greek, group 6, and a
# page that holds option 7 in its header and names no group of its own.
REGIONAL_CHANNEL = '{"name":"Regional","initial_page":"100","region":6}'
REGIONAL_PAGE = "PN,10000\r\nPS,8380\r\nOL,1,abc\r\n"


RED, GREEN, YELLOW, CYAN, WHITE = 1, 2, 3, 6, 7
DOUBLE_HEIGHT = 0x0D

PAGES = {
    "100": "\r\n".join([
        "DE,Test service index",
        "PN,10000",
        "PS,8000",
        "OL,1," + esc(YELLOW) + esc(DOUBLE_HEIGHT) + "NEOTELETEXT TEST SERVICE",
        "OL,4," + esc(WHITE) + "Weather ......................... 200",
        "OL,5," + esc(WHITE) + "News ............................ 300",
        "OL,7," + esc(CYAN) + "Served over plain HTTP from a",
        "OL,8," + esc(CYAN) + "local test server.",
        "LN,R,200,Weather",
        "LN,G,300,News",
        "LN,Y,other:100,Other",
        "LN,I,100,",
        # REQ-INP-026: a log line for the channel's author.
        "LG,I,page 100 served by the local test server",
        "",
    ]),
    "200": "\r\n".join([
        "PN,20000",
        "OL,1," + esc(GREEN) + "WEATHER",
        "OL,3," + esc(WHITE) + "Sunny, 21 degrees.",
        "LN,R,100,Index",
        "",
    ]),
    "300": "\r\n".join([
        "PN,30000",
        "OL,1," + esc(YELLOW) + "TEST NEWS",
        "OL,3," + esc(WHITE) + "The local test server is running.",
        "LN,R,100,Index",
        "",
    ]),
}

# Hamming 8/4 codes of the nibbles 0 to 15 (EN 300 706 clause 8.2, Table 3).
HAMMING_84 = [0x15, 0x02, 0x49, 0x5E, 0x64, 0x73, 0x38, 0x2F, 0xD0, 0xC7, 0x8C, 0x9B, 0xA1, 0xB6, 0xFD, 0xEA]
# Bytes of a packet after the framing code, and its text bytes.
PACKET_BYTES, ROW_BYTES, HEADER_TEXT = 42, 40, 32
# The erase bit C4 (in the S2 nibble) and serial mode C11 (EN 300 706
# clause 9.3.1).
C4_ERASE_IN_S2, C11_SERIAL = 0x8, 0x1


def odd_parity(code):
    """A 7-bit code with its odd parity bit (EN 300 706 clause 8.1)."""
    return code | (0x80 if bin(code).count("1") % 2 == 0 else 0)


def address(magazine, row):
    """A packet's two address bytes (clause 7.1.2)."""
    return bytes([HAMMING_84[(magazine & 7) | ((row & 1) << 3)], HAMMING_84[row >> 1]])


def row_codes(text, width):
    """A TTI row's text as 7-bit codes, escapes turned into control codes."""
    codes, escaped = [], False
    for char in text:
        if char == "\x1b":
            escaped = True
            continue
        codes.append((ord(char) - 0x40 if escaped else ord(char)) & 0x7F)
        escaped = False
    return bytes(odd_parity(code) for code in (codes + [0x20] * width)[:width])


def t42_page(tti, subcode=0):
    """A TTI page's header and rows as T42 packets (a test's own encoder:
    the sub-code given, the erase bit and serial mode set)."""
    lines = [line for line in tti.split("\r\n") if line]
    number = next(line[3:6] for line in lines if line.startswith("PN,"))
    magazine, tens, units = int(number[0]), int(number[1], 16), int(number[2], 16)
    # The sub-code's four nibbles S1 to S4 (clause 9.3.1.2): S2 has three
    # bits beside C4, S4 two beside C5 and C6.
    s1, s2, s3, s4 = subcode & 0xF, (subcode >> 4) & 0x7, (subcode >> 8) & 0xF, (subcode >> 12) & 0x3
    header = address(magazine, 0) + bytes(
        HAMMING_84[nibble] for nibble in [units, tens, s1, s2 | C4_ERASE_IN_S2, s3, s4, 0, C11_SERIAL]
    ) + row_codes("", HEADER_TEXT)
    packets = [header]
    for line in lines:
        if line.startswith("OL,"):
            _, row, text = line.split(",", 2)
            packets.append(address(magazine, int(row)) + row_codes(text, ROW_BYTES))
    assert all(len(packet) == PACKET_BYTES for packet in packets)
    return b"".join(packets)


DESCRIPTOR = """{{
  "name": "Test service",
  "initial_page": "100",
  "page_template": "/pages/{{mpp}}.tti",
  "push": {{"address": "ws://127.0.0.1:{port}/push"}},
  "services": [{{"name": "other", "display_name": "Other service", "descriptor": "http://127.0.0.1:{port}/other.json"}}]
}}"""


# A service whose pages are T42 under a media type that does not say so:
# /undeclared/{octet,plain}/ (REQ-WEB-003).
UNDECLARED_TYPES = {"octet": "application/octet-stream", "plain": "text/plain"}
# The same service with its pages as T42 (REQ-WEB-013: T42 pages with
# validators over HTTP).
T42_DESCRIPTOR = """{"name": "T42 service", "initial_page": "100", "page_template": "/pages/{mpp}.t42"}"""
UNDECLARED_DESCRIPTOR = """{{"name": "T", "initial_page": "100", "page_template": "/undeclared/{kind}/{{mpp}}"}}"""


# An ordinary web page that points to the regional channel, as REQ-501's
# guidance tells a publisher to: a `Link` header field and a `<link>` element
# in the first 8,192 bytes, both with the channel's media type (REQ-2476).
WEB_PAGE = """<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Test site</title>
<link rel="alternate" type="application/vnd.neoteletext+json" href="/regional/">
</head><body><p>This site has a teletext channel.</p></body></html>
"""
CHANNEL_POINTER = '</regional/>; rel="alternate"; type="application/vnd.neoteletext+json"'
# What a teletext receiver says it wants, for content negotiation.
TELETEXT_TYPES = ("application/vnd.neoteletext+json", "text/x-teletext-tti", "application/x-teletext-t42")
# How long /fault/slow waits when the request does not say (seconds).
SLOW_DEFAULT_SECONDS = 40


# A channel published as a folder (REQ-501, decision E.70): its files under
# their fixed names (REQ-WEB-019), each with its media type (REQ-WEB-027).
CHANNEL_FOLDER = "/channel/"
CHANNEL_SETTINGS = '{"name":"Folder channel","initial_page":"100"}'
# The page whose sub-pages are files of their own (REQ-1887), and its
# sub-codes.
SUBPAGED = "400"
SUBCODES = (0x0001, 0x0002, 0x0003)
# Page 300's file moved; its old name answers with a redirect to the new one,
# inside the folder (REQ-WEB-021).
MOVED_PAGE, MOVED_TO = "300.t42", "moved/300.t42"
# A file's validator in a list of sub-pages (DOC-2035): the first 64 bits of
# its SHA-256 digest in lower-case hexadecimal.
VALIDATOR_DIGITS = 16


def validator(data):
    """The validator the engine's writers give a file (DOC-2035)."""
    return hashlib.sha256(data).hexdigest()[:VALIDATOR_DIGITS]


def subpage_file(subcode):
    """Sub-page `subcode` of page 400, as T42."""
    tti = "\r\n".join([
        "PN," + SUBPAGED + "00",
        "OL,1," + esc(CYAN) + "SUB-PAGE %04X OF THE FOLDER" % subcode,
        "",
    ])
    return t42_page(tti, subcode)


def subpage_list():
    """Part 0 of page 400's list of sub-pages (REQ-1888, DOC-2038): the
    number of sub-pages and the page's version, then a row for each."""
    files = [subpage_file(subcode) for subcode in SUBCODES]
    version = validator(b"".join(files))
    rows = ["%d\t%s" % (len(files), version)]
    rows += ["%04X\t%s\tSub-page %d" % (subcode, validator(data), index + 1)
             for index, (subcode, data) in enumerate(zip(SUBCODES, files))]
    return "\r\n".join(rows) + "\r\n"


def channel_files():
    """Every file of the folder channel, by its name in the folder: the
    body and its media type."""
    t42 = "application/x-teletext-t42"
    files = {
        "channel.ttx": (CHANNEL_SETTINGS, "application/json"),
        # REQ-WEB-019: the bundle may hold any of the channel's pages; here
        # 100 and 200, so that 300 and 400 are fetched on their own.
        "index.t42": (t42_page(PAGES["100"]) + t42_page(PAGES["200"]), t42),
        "contents.txt": ("\r\n".join([
            "# The folder channel of the local test server (REQ-WEB-025).",
            "s\tPages",
            "p\t100\tIndex\t2026-11-04",
            "p\t200\tWeather",
            "p\t300\tNews",
            "p\t400\tSub-pages",
            "x\tother:100\tOther service",
            "",
        ]), "text/plain"),
        MOVED_TO: (t42_page(PAGES["300"]), t42),
        SUBPAGED + ".t42": (subpage_file(SUBCODES[0]), t42),
        SUBPAGED + "/0.sub": (subpage_list(), "text/plain"),
    }
    for page in ("100", "200"):
        files[page + ".t42"] = (t42_page(PAGES[page]), t42)
    for subcode in SUBCODES:
        files["%s/%04X.t42" % (SUBPAGED, subcode)] = (subpage_file(subcode), t42)
    return files


CHANNEL_FILES = channel_files()

# A service whose server closes each kept-alive connection after its answer
# without saying so (no `Connection: close`), as a server whose idle timeout
# is shorter than the client's does: the client meets the close only when it
# reuses the connection (REQ-502: behaviour that needs a real socket).
CLOSING = "/closing/"
CLOSING_DESCRIPTOR = '{"name": "Closing service", "initial_page": "100", "page_template": "/closing/pages/{mpp}.tti"}'


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        if os.environ.get("TTX_SERVER_LOG"):
            sys.stderr.write("server: " + fmt % args + "\n")

    def session(self):
        for part in self.headers.get("Cookie", "").split(";"):
            name, _, value = part.strip().partition("=")
            if name == "session" and value.startswith(SESSION_VERSION + "."):
                return value
        return None

    def send(self, status, body, media="text/x-teletext-tti", extra=()):
        data = body.encode("latin-1") if isinstance(body, str) else body
        self.send_response(status)
        self.send_header("Content-Type", media)
        self.send_header("Content-Length", str(len(data)))
        for name, value in extra:
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(data)

    def validated(self, body, media, cookies):
        """A page with its ETag (REQ-WEB-012: an ETag on every page), and 304
        to a request that names it (REQ-WEB-004)."""
        data = body.encode("latin-1") if isinstance(body, str) else body
        etag = '"' + hashlib.sha256(data).hexdigest()[:16] + '"'
        if self.headers.get("If-None-Match") == etag:
            self.send_response(304)
            self.send_header("ETag", etag)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        return self.send(200, data, media, cookies + [("ETag", etag)])

    def undeclared(self, path, cookies):
        kind, _, name = path[len("/undeclared/"):].partition("/")
        if kind not in UNDECLARED_TYPES:
            return self.send(404, "", "text/plain", cookies)
        if name == "service.json":
            return self.send(200, UNDECLARED_DESCRIPTOR.format(kind=kind), "application/json", cookies)
        if name not in PAGES:
            return self.send(404, "", "text/plain", cookies)
        return self.send(200, t42_page(PAGES[name]), UNDECLARED_TYPES[kind], cookies)

    def do_GET(self):
        path, _, query = self.path.partition("?")
        cookies = []
        session = self.session()
        if session is not None:
            with self.server.lock:
                self.server.sessions_returned += 1
        if path == "/requests":
            # The paths asked for so far, one a line (REQ-502: a test sees
            # which of a channel's files a receiver fetched).
            with self.server.lock:
                asked = "\n".join(self.server.requested)
            return self.send(200, asked, "text/plain")
        with self.server.lock:
            self.server.requested.append(path)
        if path == "/sessions":
            # How many requests have brought the session cookie back: none
            # may, since a receiver keeps no cookies (VS-201).
            return self.send(200, str(self.server.sessions_returned), "text/plain")
        if path == "/web/":
            return self.web_page()
        if session is None:
            session = SESSION_VERSION + "." + base64.urlsafe_b64encode(os.urandom(9)).decode()
            cookies.append(("Set-Cookie", "session=" + session + "; Path=/; HttpOnly; SameSite=Lax"))
        port = self.server.server_address[1]
        if path == "/service.json":
            return self.send(200, DESCRIPTOR.format(port=port), "application/json", cookies)
        if path == "/other.json":
            other = DESCRIPTOR.format(port=port).replace("Test service", "Other service")
            return self.send(200, other, "application/json", cookies)
        if path == "/regional/channel.ttx":
            return self.send(200, REGIONAL_CHANNEL, "application/json", cookies)
        if path == "/regional/100.tti":
            return self.validated(REGIONAL_PAGE, "text/x-teletext-tti", cookies)
        if path == "/t42.json":
            return self.send(200, T42_DESCRIPTOR, "application/json", cookies)
        if path.startswith("/pages/") and (path.endswith(".tti") or path.endswith(".t42")):
            page, _, extension = path[len("/pages/"):].partition(".")
            if page not in PAGES:
                return self.send(404, "", "text/plain", cookies)
            if extension == "t42":
                return self.validated(t42_page(PAGES[page]), "application/x-teletext-t42", cookies)
            return self.validated(PAGES[page], "text/x-teletext-tti", cookies)
        if path.startswith("/undeclared/"):
            return self.undeclared(path, cookies)
        if path.startswith(CLOSING):
            return self.closing(path[len(CLOSING) - 1:], cookies)
        if path.startswith(CHANNEL_FOLDER):
            return self.channel_file(path[len(CHANNEL_FOLDER):], cookies)
        if path == "/push":
            return self.push_websocket()
        if path == "/invalid.json":
            return self.send(200, DESCRIPTOR.format(port=port).replace("/push", "/push-invalid"), "application/json", cookies)
        if path == "/push-invalid":
            return self.push_websocket(invalid=True)
        return self.fault(path, query)

    def closing(self, path, cookies):
        """The closing service: its descriptor and pages, each answer
        followed by the connection's close."""
        self.close_connection = True
        if path == "/service.json":
            return self.send(200, CLOSING_DESCRIPTOR, "application/json", cookies)
        page = path[len("/pages/"):-len(".tti")] if path.startswith("/pages/") and path.endswith(".tti") else None
        if page not in PAGES:
            return self.send(404, "", "text/plain", cookies)
        return self.validated(PAGES[page], "text/x-teletext-tti", cookies)

    def channel_file(self, name, cookies):
        """A file of the folder channel (REQ-501): validated, as every page
        is; a redirect inside the folder for a moved file; and 404, never a
        page with status 200, for a file the channel does not have."""
        if name == MOVED_PAGE:
            self.send_response(302)
            self.send_header("Location", CHANNEL_FOLDER + MOVED_TO)
            self.send_header("Content-Length", "0")
            for header in cookies:
                self.send_header(*header)
            self.end_headers()
            return None
        if name not in CHANNEL_FILES:
            return self.send(404, "", "text/plain", cookies)
        body, media = CHANNEL_FILES[name]
        return self.validated(body, media, cookies)

    def web_page(self):
        """A web page that points to the channel (REQ-501's guidance): a
        receiver that asks for teletext is sent to the channel by 303, and
        the answer varies by Accept, so a cache keeps the two apart."""
        accept = self.headers.get("Accept", "")
        if any(kind in accept for kind in TELETEXT_TYPES):
            self.send_response(303)
            self.send_header("Location", "/regional/")
            self.send_header("Vary", "Accept")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        return self.send(200, WEB_PAGE, "text/html; charset=utf-8", [("Link", CHANNEL_POINTER), ("Vary", "Accept")])

    def push_websocket(self, invalid=False):
        key = self.headers.get("Sec-WebSocket-Key", "")
        accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.send_header("Sec-WebSocket-Protocol", SUBPROTOCOL)
        self.end_headers()
        # DEC-1459: a channel's receiver is sent T42 packets in binary
        # frames, never TTI text.
        page = PAGES["200"].replace("Sunny, 21 degrees.", "Pushed: rain, 15 degrees.")
        payload = t42_page(page)
        header = bytes([0x82, 126]) + len(payload).to_bytes(2, "big") if len(payload) >= 126 else bytes([0x82, len(payload)])
        # REQ-1462: a text message is discarded and counts as one invalid
        # packet, and comes first so that a report of it is seen at once.
        discarded = bytes([0x81, 1]) + b"x" if invalid else b""
        try:
            self.wfile.write(discarded + header + payload)
            self.wfile.flush()
            time.sleep(2)
        except (BrokenPipeError, ConnectionResetError):
            pass
        self.close_connection = True
        return None

    def fault(self, path, query=""):
        """Fault injection (REQ-WEB-013)."""
        if path == "/fault/slow":
            # ?seconds=N lets a test wait less than the default.
            seconds = SLOW_DEFAULT_SECONDS
            for pair in query.split("&"):
                name, _, value = pair.partition("=")
                if name == "seconds" and value.isdigit():
                    seconds = int(value)
            time.sleep(seconds)
            return self.send(200, DESCRIPTOR.format(port=self.server.server_address[1]), "application/json")
        if path == "/fault/drop":
            self.close_connection = True
            self.wfile.write(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\npartial")
            return None
        if path == "/fault/big":
            return self.send(200, "OL,1," + "X" * (300 * 1024))
        if path == "/fault/media":
            return self.send(200, "<html></html>", "text/html")
        if path == "/fault/loop":
            self.send_response(302)
            self.send_header("Location", "/fault/loop")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        if path == "/fault/ambiguous":
            self.wfile.write(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")
            return None
        return self.send(404, "", "text/plain")


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.lock = threading.Lock()
        self.sessions_returned = 0
        self.requested = []


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    server = Server(("127.0.0.1", port), Handler)
    print(server.server_address[1], flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
