"""Run one Codex process in a Windows pseudo-console and record screen evidence."""
import json
import os
import queue
import sys
import threading
import time

import pyte
import winpty

KEYS = {"enter": "\r", "esc": "\x1b", "ctrl-c": "\x03", "ctrl-d": "\x04", "tab": "\t", "shift-tab": "\x1b[Z", "up": "\x1b[A", "down": "\x1b[B", "left": "\x1b[D", "right": "\x1b[C", "backspace": "\x7f"}


def append_jsonl(path, value):
    with open(path, "a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, ensure_ascii=False) + "\n")


def run(session, workdir, exe, extra):
    os.makedirs(session, exist_ok=True)
    screen = pyte.Screen(int(os.environ.get("TUI_COLS", "160")), int(os.environ.get("TUI_ROWS", "48")))
    stream = pyte.ByteStream(screen)
    proc = winpty.PtyProcess.spawn([exe, *extra], cwd=workdir, dimensions=(screen.lines, screen.columns))
    chunks, dead = queue.Queue(), threading.Event()

    def reader():
        while not dead.is_set():
            try:
                data = proc.read(65536)
                if data: chunks.put(data)
            except (EOFError, OSError):
                dead.set()
                break

    threading.Thread(target=reader, daemon=True).start()
    last, start = "", time.time()
    cmd_path = os.path.join(session, "cmd.json")
    while True:
        deadline = time.time() + 0.25
        while time.time() < deadline:
            try: stream.feed(chunks.get(timeout=0.05).encode("utf-8", "replace"))
            except queue.Empty: pass
        now = time.time()
        stamp = {"timestamp": time.time(), "elapsedSeconds": round(now - start, 3)}
        text = "\n".join(line.rstrip() for line in screen.display)
        with open(os.path.join(session, "screen.txt"), "w", encoding="utf-8") as file: file.write(text + "\n")
        if text != last:
            with open(os.path.join(session, "transcript.txt"), "a", encoding="utf-8") as file: file.write(f"\n===== {stamp['elapsedSeconds']}s =====\n{text}\n")
            last = text
        if os.path.exists(cmd_path):
            try:
                with open(cmd_path, encoding="utf-8") as file: command = json.load(file)
                os.remove(cmd_path)
                if command.get("quit"):
                    break
                if command.get("send"): proc.write(command["send"])
                for key in command.get("keys", []): proc.write(KEYS.get(key, key))
                append_jsonl(os.path.join(session, "actions.jsonl"), {**stamp, "id": command.get("id"), "send": command.get("send"), "keys": command.get("keys", [])})
                append_jsonl(os.path.join(session, "acknowledgements.jsonl"), {**stamp, "id": command.get("id"), "delivered": True})
            except (json.JSONDecodeError, OSError): pass
        if dead.is_set() and chunks.empty(): break
    try: proc.terminate(force=True)
    except Exception: pass


if __name__ == "__main__":
    run(sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:])
