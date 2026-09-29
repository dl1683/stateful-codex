"""Drive a scripted multi-turn conversation through the ConPTY session."""
import argparse
import json
import os
import re
import subprocess
import sys
import time

BUSY = ("esc to interrupt", "Working (", "Starting MCP", "Resuming session", "Running ")
APPROVAL = re.compile(r"Trust this folder|Would you like me to .*\?|Press enter to confirm", re.I)


def is_busy(screen):
    return any(marker in screen for marker in BUSY)


def approval_prompt(screen):
    return APPROVAL.search(screen) is not None


def stable_idle(screen, quiet_seconds, required_seconds):
    return bool(screen.strip()) and not is_busy(screen) and quiet_seconds >= required_seconds


def append_jsonl(path, value):
    with open(path, "a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, ensure_ascii=False) + "\n")


def drive(options):
    os.makedirs(options.session, exist_ok=True)
    with open(options.conversation, encoding="utf-8") as file: conversation = json.load(file)
    session_script = os.path.join(os.path.dirname(__file__), "tui_session.py")
    command = [sys.executable, session_script, options.session, options.workspace, options.codex, "--stateful", options.mode, "-C", options.workspace, conversation["initialMessage"]]
    driver = subprocess.Popen(command, env=os.environ.copy())
    turns = []
    try:
        if not wait_idle(options, driver): raise RuntimeError("initial TUI state did not become idle")
        record_action(options.session, "initial", conversation["initialMessage"], [])
        for number, message in enumerate(conversation["turns"], 1):
            turn = {"id": f"turn-{number:02d}", "message": message, "status": "sent", "startedAt": time.time()}
            turn["before"] = snapshot(options.session, f"turn-{number:02d}-before.txt")
            if not send(options, driver, {"id": turn["id"], "send": message, "keys": ["enter"]}):
                turn["status"] = "processExited" if driver.poll() is not None else "timedOut"
                turns.append(turn)
                raise RuntimeError(f"scripted message was not delivered: {turn['id']}")
            if not wait_idle(options, driver):
                turn["status"] = "processExited" if driver.poll() is not None else "timedOut"
                turns.append(turn)
                raise RuntimeError(f"turn did not become idle: {turn['id']}")
            turn["status"] = "idle"
            turn["after"] = snapshot(options.session, f"turn-{number:02d}-after.txt")
            turn["endedAt"] = time.time()
            turns.append(turn)
        snapshot(options.session, "screen-final.txt")
        send(options, driver, {"id": "quit", "quit": True})
        driver.wait(timeout=60)
        return True
    finally:
        with open(os.path.join(options.session, "turns.json"), "w", encoding="utf-8") as file: json.dump(turns, file, indent=2)
        if driver.poll() is None: driver.terminate()


def wait_idle(options, driver):
    quiet_since = None
    deadline = time.time() + options.turn_timeout
    while time.time() < deadline and driver.poll() is None:
        screen = read_screen(options.session)
        if approval_prompt(screen):
            append_jsonl(os.path.join(options.session, "approvals.jsonl"), {"timestamp": time.time(), "screenExcerpt": screen[-600:], "response": "enter"})
            send(options, driver, {"id": f"approval-{int(time.time() * 1000)}", "keys": ["enter"], "approval": True})
            quiet_since = None
            continue
        if is_busy(screen): quiet_since = None
        elif screen.strip(): quiet_since = quiet_since or time.time()
        if quiet_since and stable_idle(screen, time.time() - quiet_since, options.idle_stable): return True
        time.sleep(0.25)
    return False


def send(options, driver, command):
    path = os.path.join(options.session, "cmd.json")
    deadline = time.time() + 10
    while os.path.exists(path) and time.time() < deadline: time.sleep(0.05)
    if os.path.exists(path): return False
    temporary = path + ".tmp"
    with open(temporary, "w", encoding="utf-8") as file: json.dump(command, file)
    os.replace(temporary, path)
    if command.get("quit"): return True
    ack = os.path.join(options.session, "acknowledgements.jsonl")
    while time.time() < deadline and driver.poll() is None:
        if os.path.exists(ack) and any(json.loads(line).get("id") == command.get("id") for line in open(ack, encoding="utf-8")): return True
        time.sleep(0.05)
    return False


def read_screen(session):
    try:
        with open(os.path.join(session, "screen.txt"), encoding="utf-8") as file: return file.read()
    except OSError: return ""


def snapshot(session, name):
    screen = read_screen(session)
    with open(os.path.join(session, name), "w", encoding="utf-8") as file: file.write(screen)
    return name


def record_action(session, identifier, message, keys):
    append_jsonl(os.path.join(session, "actions.jsonl"), {"timestamp": time.time(), "id": identifier, "send": message, "keys": keys})


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", required=True); parser.add_argument("--conversation", required=True); parser.add_argument("--workspace", required=True); parser.add_argument("--codex", required=True); parser.add_argument("--mode", required=True)
    parser.add_argument("--turn-timeout", type=float, default=2700); parser.add_argument("--idle-stable", type=float, default=16)
    return parser.parse_args()


if __name__ == "__main__":
    try: drive(parse_args())
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
