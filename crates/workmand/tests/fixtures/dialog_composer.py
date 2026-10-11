"""Headless Claude-like PTY: explicit Ink wraps, a replacing dialog, and raw key receipts.

Control-E dismisses the dialog, Control-F ends the current turn. Bracketed paste
is enabled so a body redraw and its later Enter are distinct observable writes.
"""
import os
import sys
import textwrap
import tty

tty.setraw(0)
mode = sys.argv[1]
tool = sys.argv[2] if len(sys.argv) > 2 else "claude"
marker = {"claude": "❯", "codex": "›", "generic": ">"}[tool]
draft = ""
dialog = mode == "before"
opened = False
busy = False
receipts = []


def render():
    screen = "\x1b[2J\x1b[H" + "\r\n".join(receipts) + "\r\nENTER_COUNT:" + str(len(receipts)) + "\r\n"
    if dialog:
        screen += "Bash command\r\nDo you want to proceed?\r\n❯ 1. Yes\r\n  2. No\r\nEsc to cancel"
    else:
        # These rows are explicit newlines, NOT terminal soft wraps.
        lines = textwrap.wrap(draft, width=40 if tool == "codex" else 38) or [""]
        if tool == "generic":
            screen += "1. prior result\r\n2. another result\r\n"
        screen += "─" * 50 + "\r\n" + marker + " " + "\r\n   ".join(lines) + "\r\n" + "─" * 50
        if busy:
            screen += "\r\nthinking...\r\nesc to interrupt"
    os.write(1, screen.encode())


os.write(1, b"\x1b[?2004h")
render()
while True:
    key = os.read(0, 1)
    if not key:
        break
    if key == b"\x1b":
        paste_marker = key + os.read(0, 5)
        if paste_marker != b"\x1b[200~":
            raise RuntimeError(repr(paste_marker))
        body = b""
        while not body.endswith(b"\x1b[201~"):
            body += os.read(0, 1)
        if dialog:
            receipts.append("BODY_IN_DIALOG")
        draft += body[:-6].decode()
        if mode == "after" and not opened and draft.startswith("[workman]"):
            dialog = opened = True
        render()
    elif key == b"\x05":
        dialog = False
        render()
    elif key == b"\x06":
        busy = False
        render()
    elif key == b"\r":
        if dialog:
            receipts.append("APPROVED_BY_ENTER")
        else:
            receipts.append("RECEIVED:" + draft)
            draft = ""
            busy = True
        render()
    else:
        draft += key.decode()
        render()
