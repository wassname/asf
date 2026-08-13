#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["pyte"]
# ///
"""Drive the picker in a pty and print what the screen ended up showing.

    tests/drive.py 'target/release/asf -i steer' Down Down PageDown --sleep 3

Keys are names from KEYS below, or literal text to type. The last screen is what
a person would see, so that is what gets printed: escape codes stripped.
"""
import os
import pty
import select
import subprocess
import sys
import time
import fcntl
import termios
import struct

import pyte

KEYS = {
    "Enter": "\r",
    "Esc": "\x1b",
    "Up": "\x1b[A",
    "Down": "\x1b[B",
    "Right": "\x1b[C",
    "Left": "\x1b[D",
    "PageUp": "\x1b[5~",
    "PageDown": "\x1b[6~",
    "ShiftUp": "\x1b[1;2A",
    "ShiftDown": "\x1b[1;2B",
    "Tab": "\t",
    "F1": "\x1bOP",
    "F2": "\x1bOQ",
    "F3": "\x1bOR",
    "F4": "\x1bOS",
    "F5": "\x1b[15~",
    "F6": "\x1b[17~",
    "F7": "\x1b[18~",
    "BS": "\x7f",
    **{f"C-{c}": chr(ord(c) - 96) for c in "abcdefghijklmnopqrstuvwxyz"},
    **{f"A-{c}": f"\x1b{c}" for c in "abcdefghijklmnopqrstuvwxyz"},
}

def drive(argv, keys, rows=45, cols=140, settle=2.0):
    screen = pyte.Screen(cols, rows)
    stream = pyte.Stream(screen)
    primary, secondary = pty.openpty()
    fcntl.ioctl(secondary, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    env = dict(os.environ, TERM="xterm-256color", LINES=str(rows), COLUMNS=str(cols))
    def own_the_tty():
        # skim opens /dev/tty, so the pty has to be this process's controlling terminal
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    child = subprocess.Popen(
        argv,
        stdin=secondary,
        stdout=secondary,
        stderr=secondary,
        env=env,
        close_fds=True,
        preexec_fn=own_the_tty,
    )
    os.close(secondary)

    def pump(seconds):
        end = time.time() + seconds
        while time.time() < end:
            ready, _, _ = select.select([primary], [], [], 0.1)
            if ready:
                try:
                    stream.feed(os.read(primary, 65536).decode("utf8", "replace"))
                except OSError:
                    return

    pump(settle)
    for key in keys:
        os.write(primary, KEYS.get(key, key).encode())
        pump(1.2)
    child.terminate()
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        child.kill()
    os.close(primary)
    return "\n".join(line.rstrip() for line in screen.display)


if __name__ == "__main__":
    args = sys.argv[1:]
    settle = 2.0
    if "--sleep" in args:
        at = args.index("--sleep")
        settle = float(args[at + 1])
        args = args[:at] + args[at + 2 :]
    print(drive(args[0].split(), args[1:], settle=settle))
