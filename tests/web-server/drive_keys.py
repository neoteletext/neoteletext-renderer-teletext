#!/usr/bin/env python3
"""Drives `neottx view` through a pseudo-terminal over a channel of files
whose pages 100, 101 and 102 each say `PAGE NNN` in row 1, page 101's index
link is 102, and whose `channel.ttx` aliases 400 to another channel's
address: next page from 100 shows 101, Index there shows 102, Home shows
100 again, and 400 typed says in row 24 that the page is another channel's
(REQ-496, REQ-509). The test that runs it checks the requests the keys made
of the server. Test code.

Usage: drive_keys.py NEOTTX PORT. Exit status 0 when every step passed.
"""

import os
import pty
import re
import select
import sys
import time

# How long one page may take to come: a request to the local server.
STEP_SECONDS = 5
UP = b"\x1b[A"


def main():
    neottx, port = sys.argv[1], sys.argv[2]
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.execv(neottx, ["neottx", "view", f"http://127.0.0.1:{port}/ch/",
                          "--allow-local", "127.0.0.1", "--graphics", "text", "--no-sandbox"])
    output = bytearray()

    def screen():
        return re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", output.decode("utf-8", "replace"))

    def wait_for(text):
        end = time.time() + STEP_SECONDS
        while time.time() < end:
            if text in screen():
                return True
            ready, _, _ = select.select([fd], [], [], 0.05)
            if ready:
                try:
                    output.extend(os.read(fd, 65536))
                except OSError:
                    return False
        return False

    def press(keys, text):
        output.clear()
        os.write(fd, keys)
        return wait_for(text)

    failures = []
    steps = [
        (None, "PAGE 100", "the initial page"),
        (UP, "PAGE 101", "next page"),
        (b"i", "PAGE 102", "Index, by the page's index link"),
        (b"h", "PAGE 100", "Home, the initial page"),
        (b"400", "another channel", "an alias to another channel's address"),
    ]
    for keys, text, name in steps:
        passed = wait_for(text) if keys is None else press(keys, text)
        if not passed:
            failures.append(name)
            break
    try:
        os.write(fd, b"q")
        time.sleep(0.5)
    except OSError:
        pass
    print("ok" if not failures else "FAILED: " + ", ".join(failures))
    sys.exit(0 if not failures else 1)


main()
