#!/usr/bin/env python3
"""Run the real picker in a private tmux server: python3 tests/picker.py [asf-binary]. -- PI/OpenAI"""

import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/asf"
AGENTS = ["agy", "claude", "codex", "pi", "gemini", "opencode", "copilot", "hermes"]
ID = "38ca76d1-93ba-427e-9552-03a389428f0e"


def verify(tmp):
    home = tmp / "fixture's home"
    shutil.copytree(ROOT / "tests/fixtures/home", home, ignore=shutil.ignore_patterns(".cache"))
    native = home / ".gemini/antigravity-cli/conversations" / f"{ID}.db"
    native.parent.mkdir(parents=True)
    native.touch()
    exported = native.with_suffix(".trajectory.json")
    env = {"HOME": str(home), "XDG_CACHE_HOME": str(tmp / "cache"), "TZ": "UTC"}
    tmux_cmd = ["tmux", "-S", str(tmp / "sock")]

    def tmux(*args):
        return subprocess.check_output(tmux_cmd + list(args), text=True, timeout=5)

    def wait_screen(*matches):
        for _ in range(100):
            screen = tmux("capture-pane", "-p")
            assert "invalid value '' for '--line" not in screen, screen
            if all(re.search(pattern, screen) for pattern in matches):
                return screen
            time.sleep(.04)
        raise AssertionError((matches, screen))

    output = tmp / "stdout"
    run = 0

    def open_picker(*args, mode="name"):
        nonlocal run
        run += 1
        command = shlex.join(["env", *(f"{k}={v}" for k, v in env.items()), "SHELL=/bin/sh", str(BINARY), "-i", *args])
        tmux("send-keys", "-l", f"{command} >{shlex.quote(str(output))}; printf 'PICKER DONE {run}\\n'")
        tmux("send-keys", "Enter")
        wait_screen(rf"^{mode}>")

    def close_picker(key):
        tmux("send-keys", key)
        wait_screen(rf"(?m)^PICKER DONE {run}$")
        return output.read_text()

    def cli(*args):
        return subprocess.check_output([BINARY, *args], env={**os.environ, **env}, text=True, timeout=5)

    def choose(name):
        tmux("send-keys", "C-f")
        wait_screen(r"^agent>", "Filter agent")
        tmux("send-keys", "-l", name)
        wait_screen(r"^agent> " + name, r"(?m)^> " + name + r"\b")
        tmux("send-keys", "Enter")
        wait_screen(r"filter agent \(" + name + r"\)")

    tmux("-f", "/dev/null", "new-session", "-d", "-x", "120", "-y", "32", "/bin/sh")
    try:
        open_picker("-a", "agy")
        tmux("send-keys", "C-f")
        wait_screen(r"agy +0 exports +\| +1 native session +\| +Latest native:", r"claude +1 file +\| +Latest:", r"codex +2 files +\| +Latest:", r"pi +2 files +\| +Latest:")
        tmux("send-keys", "Escape")
        wait_screen(r"^name>")
        tmux("send-keys", "C-q")
        wait_screen(r"^transcript>", r"(?m)^ +0/0")
        tmux("send-keys", "-l", "not present")
        wait_screen(r"^transcript> not present", r"(?m)^ +0/0")
        assert close_picker("Enter") == ""
        print("Native-only agy: menu shows zero exports and one native session; empty search stays blank.")

        exported.write_text(json.dumps({"cascadeId": ID, "steps": [
            {"userInput": {"userResponse": "the agy widget factory opens on tuesday"}},
            {"plannerResponse": {"response": "checking the opening date"}},
        ]}))
        for agent in AGENTS:
            client = r"(?m)^client +" + agent + r"\b"
            heading = r"(?m)^## user #[1-9]\d*$"
            hit = cli("--paths", "-a", agent, "-c", "widget factory", "-n", "1").strip()
            assert hit, agent

            open_picker("-a", "copilot")
            choose(agent)
            wait_screen(client, r"(?m)^file +")
            tmux("send-keys", "-l", "no searchable result")
            wait_screen(r"^name> no searchable result", r"(?m)^ +0/\d+")
            assert close_picker("M-p") == ""

            open_picker("-a", agent)
            wait_screen(client)
            tmux("send-keys", "C-q")
            wait_screen(r"^transcript>")
            tmux("send-keys", "-l", "widget factory")
            wait_screen(r"^transcript> widget factory", client, heading)
            path = close_picker("M-p").strip()
            assert path == hit, (agent, path, hit)
            assert re.search(heading, cli("-r", path)), agent

            for key, expected in [("M-m", cli("-r", hit)), ("Enter", cli("-u", hit))]:
                open_picker("-a", agent, "-c", "widget factory", mode="transcript")
                wait_screen(r"^transcript> widget factory", client, heading)
                assert close_picker(key).strip() == expected.strip(), (agent, key)
            open_picker("-a", agent, "-c", "no searchable result", mode="transcript")
            wait_screen(r"^transcript> no searchable result", r"(?m)^ +0/0")
            assert close_picker("M-m") == ""
            print(f"{agent}: name and transcript previews, no-match, Alt-P, Alt-M, Enter, and CLI read passed.")
    finally:
        tmux("kill-server")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="asf-picker-") as tmp:
        verify(Path(tmp))
