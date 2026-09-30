"""Drive a real TUI conversation through a Windows pseudo-console.

The driver records transport state only. Product success is decided later from
the exact rollout and the sealed evidence bundle.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time

BUSY = (
    "esc to interrupt",
    "Working (",
    "Starting MCP",
    "Resuming session",
    "Running ",
    "model: loading",
)
APPROVAL_PATTERNS = (
    ("trust-folder", re.compile(r"Trust this folder", re.I)),
    ("command", re.compile(r"Would you like to run the following command\?", re.I)),
    ("confirm", re.compile(r"Press enter to confirm(?: or esc to cancel)?", re.I)),
    ("mcp", re.compile(r"Allow the .+ MCP server to run tool .+\?", re.I)),
)
SETUP_FAILURE_MARKERS = (
    "Set up the Codex agent sandbox",
    "Use non-admin sandbox",
)


def is_busy(screen):
    return any(marker in screen for marker in BUSY)


def approval_prompt(screen):
    return approval_identity(screen) is not None


def approval_identity(screen):
    for kind, pattern in APPROVAL_PATTERNS:
        match = pattern.search(screen)
        if match:
            excerpt = " ".join(screen[max(0, match.start() - 240) : match.end() + 500].split())
            if kind == "command":
                command_match = re.search(
                    r"Environment:\s*local\s*\$\s*(.*?)(?:›|Press enter|$)",
                    screen,
                    re.I | re.S,
                )
                approval_key = " ".join((command_match.group(1) if command_match else excerpt).split())
            elif kind == "mcp":
                approval_key = " ".join(match.group(0).split())
            else:
                approval_key = kind
            return {
                "kind": kind,
                "id": hashlib.sha256(f"{kind}:{approval_key}".encode()).hexdigest()[:20],
                "excerpt": excerpt,
            }
    return None


def setup_failure(screen):
    return any(marker in screen for marker in SETUP_FAILURE_MARKERS)


def ready_screen(screen):
    return bool(screen.strip()) and "Ask Codex" in screen and not is_busy(screen)


def stable_idle(screen, quiet_seconds, required_seconds, seen_busy=True):
    return (
        seen_busy
        and ready_screen(screen)
        and not approval_prompt(screen)
        and quiet_seconds >= required_seconds
    )


def append_jsonl(path, value):
    with open(path, "a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, ensure_ascii=False) + "\n")


def append_transcript(session, value):
    append_jsonl(os.path.join(session, "transcript.jsonl"), value)
    with open(os.path.join(session, "transcript.txt"), "a", encoding="utf-8") as stream:
        stream.write(f"\n===== {value['type']} {value.get('id', '')} =====\n")
        if value.get("message"):
            stream.write(f"USER: {value['message']}\n")
        if value.get("status"):
            stream.write(f"STATE: {value['status']}\n")


def drive(options):
    os.makedirs(options.session, exist_ok=True)
    for name in ("actions.jsonl", "approvals.jsonl", "transcript.jsonl"):
        open(os.path.join(options.session, name), "a", encoding="utf-8").close()
    with open(options.conversation, encoding="utf-8") as file:
        conversation = json.load(file)
    session_script = os.path.join(os.path.dirname(__file__), "tui_session.py")
    command = [sys.executable, session_script, options.session, options.workspace, options.codex]
    if options.arm == "stateful":
        command += ["--stateful", options.mode]
    command += ["-C", options.workspace, conversation["initialMessage"]]
    driver = subprocess.Popen(command, env=os.environ.copy())
    turns = []
    try:
        initial = wait_ready(options, driver)
        append_transcript(options.session, {"type": "session", "status": initial})
        if initial != "awaitingInput":
            raise RuntimeError(f"initial TUI state was {initial}, not awaitingInput")
        record_action(options.session, "initial", conversation["initialMessage"], [], "submitted")
        for number, message in enumerate(conversation["turns"], 1):
            turn = {
                "id": f"turn-{number:02d}",
                "message": message,
                "status": "sent",
                "state": "submitted",
                "startedAt": time.time(),
            }
            turn["before"] = snapshot(options.session, f"turn-{number:02d}-before.txt")
            append_transcript(options.session, {"type": "user", "id": turn["id"], "message": message, "status": "sent"})
            if not send(options, driver, {"id": turn["id"], "send": message, "keys": ["enter"]}):
                turn.update(status=terminal_status(driver), state="processExited" if driver.poll() else "timedOut")
                turns.append(turn)
                raise RuntimeError(f"scripted message was not delivered: {turn['id']}")
            record_action(options.session, turn["id"], message, ["enter"], "delivered")
            succeeded, state = wait_turn(options, driver, turn["id"])
            turn["state"] = state
            turn["status"] = "idle" if succeeded else state
            turn["after"] = snapshot(options.session, f"turn-{number:02d}-after.txt")
            turn["endedAt"] = time.time()
            turns.append(turn)
            append_transcript(options.session, {"type": "turn", "id": turn["id"], "status": turn["status"], "state": state})
            if not succeeded:
                raise RuntimeError(f"turn did not become terminal and idle: {turn['id']} ({state})")
        snapshot(options.session, "screen-final.txt")
        send(options, driver, {"id": "quit", "quit": True})
        driver.wait(timeout=30)
        return True
    finally:
        with open(os.path.join(options.session, "turns.json"), "w", encoding="utf-8") as file:
            json.dump(turns, file, indent=2)
        if not os.path.exists(os.path.join(options.session, "screen-final.txt")):
            snapshot(options.session, "screen-final.txt")
        if driver.poll() is None:
            driver.terminate()


def wait_ready(options, driver):
    deadline = time.time() + options.turn_timeout
    quiet_since = None
    seen = set()
    while time.time() < deadline and driver.poll() is None:
        screen = read_screen(options.session)
        if setup_failure(screen):
            return "sandboxSetupRequired"
        if handle_approval(options, driver, screen, seen):
            quiet_since = None
            continue
        if is_busy(screen):
            quiet_since = None
        elif ready_screen(screen):
            quiet_since = quiet_since or time.time()
        if quiet_since and time.time() - quiet_since >= min(3, options.idle_stable):
            return "awaitingInput"
        time.sleep(0.25)
    return terminal_status(driver, timed_out=True)


def wait_turn(options, driver, turn_id):
    deadline = time.time() + options.turn_timeout
    quiet_since = None
    seen_busy = False
    seen = set()
    while time.time() < deadline and driver.poll() is None:
        screen = read_screen(options.session)
        if setup_failure(screen):
            return False, "sandboxSetupRequired"
        if handle_approval(options, driver, screen, seen):
            quiet_since = None
            continue
        if is_busy(screen):
            seen_busy = True
            quiet_since = None
        elif ready_screen(screen):
            quiet_since = quiet_since or time.time()
        if quiet_since and stable_idle(screen, time.time() - quiet_since, options.idle_stable, seen_busy):
            return True, "idle"
        time.sleep(0.25)
    return False, terminal_status(driver, timed_out=True)


def handle_approval(options, driver, screen, seen):
    identity = approval_identity(screen)
    if identity is None or identity["id"] in seen:
        return False
    seen.add(identity["id"])
    record = {"timestamp": time.time(), "dialogId": identity["id"], "kind": identity["kind"], "screenExcerpt": identity["excerpt"], "response": "enter" if options.approve else "none"}
    if options.approve:
        if not send(options, driver, {"id": f"approval-{identity['id']}", "keys": ["enter"], "approval": True}):
            record["response"] = "deliveryFailed"
        else:
            record["disappeared"] = wait_dialog_gone(options, identity["id"])
    append_jsonl(os.path.join(options.session, "approvals.jsonl"), record)
    return True


def wait_dialog_gone(options, dialog_id):
    deadline = time.time() + min(10, options.idle_stable)
    while time.time() < deadline:
        identity = approval_identity(read_screen(options.session))
        if identity is None or identity["id"] != dialog_id:
            return True
        time.sleep(0.25)
    return False


def send(options, driver, command):
    path = os.path.join(options.session, "cmd.json")
    deadline = time.time() + 10
    while os.path.exists(path) and time.time() < deadline:
        time.sleep(0.05)
    if os.path.exists(path):
        return False
    temporary = path + ".tmp"
    with open(temporary, "w", encoding="utf-8") as file:
        json.dump(command, file)
    os.replace(temporary, path)
    if command.get("quit"):
        return True
    ack = os.path.join(options.session, "acknowledgements.jsonl")
    while time.time() < deadline and driver.poll() is None:
        if os.path.exists(ack):
            with open(ack, encoding="utf-8") as file:
                if any(json.loads(line).get("id") == command.get("id") for line in file):
                    return True
        time.sleep(0.05)
    return False


def read_screen(session):
    try:
        with open(os.path.join(session, "screen.txt"), encoding="utf-8") as file:
            return file.read()
    except OSError:
        return ""


def snapshot(session, name):
    with open(os.path.join(session, name), "w", encoding="utf-8") as file:
        file.write(read_screen(session))
    return name


def record_action(session, identifier, message, keys, status):
    append_jsonl(os.path.join(session, "actions.jsonl"), {"timestamp": time.time(), "id": identifier, "send": message, "keys": keys, "status": status})


def terminal_status(driver, timed_out=False):
    if timed_out and driver.poll() is None:
        return "timedOut"
    return "processExited"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", required=True)
    parser.add_argument("--conversation", required=True)
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--codex", required=True)
    parser.add_argument("--mode", required=True)
    parser.add_argument("--arm", choices=("base", "stateful"), default="stateful")
    parser.add_argument("--approve", action="store_true", default=True)
    parser.add_argument("--turn-timeout", type=float, default=2700)
    parser.add_argument("--idle-stable", type=float, default=16)
    return parser.parse_args()


if __name__ == "__main__":
    try:
        drive(parse_args())
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
