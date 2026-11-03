#!/usr/bin/env python3
"""Drives `neottx view` through a pseudo-terminal over a channel of files
whose page 100 has COUNT sub-pages, each saying `SUB NNNN` in row 1: Right
from the first reaches each in ascending order and wraps to the first, and
Left from the first wraps to the last (REQ-328, REQ-1888). Test code.

Usage: drive_subpages.py TTX PORT COUNT. Exit status 0 when every step passed.
"""

import os
import pty
import re
import select
import sys
import time

# How long one sub-page may take to come: a request to the local server.
STEP_SECONDS = 5
RIGHT = b"\x1b[C"
LEFT = b"\x1b[D"


def main():
    neottx, port, count = sys.argv[1], sys.argv[2], int(sys.argv[3])
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

    def press(key, text):
        output.clear()
        os.write(fd, key)
        return wait_for(text)

    failures = []
    if not wait_for("SUB 0001"):
        failures.append("the first sub-page")
    for place in range(2, count + 1):
        if not press(RIGHT, f"SUB {place:04d}"):
            failures.append(f"Right to {place:04d}")
            break
    if not failures and not press(RIGHT, "SUB 0001"):
        failures.append("Right wraps to the first")
    if not failures and not press(LEFT, f"SUB {count:04d}"):
        failures.append("Left wraps to the last")
    try:
        os.write(fd, b"q")
        time.sleep(0.5)
    except OSError:
        pass
    print("ok" if not failures else "FAILED: " + ", ".join(failures))
    sys.exit(0 if not failures else 1)


main()
