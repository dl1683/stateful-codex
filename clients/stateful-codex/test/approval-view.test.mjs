import assert from "node:assert/strict";
import test from "node:test";

import { displayCommand, splitShellWords } from "../public/approval-view.mjs";
import { createWorkspaceDom } from "../public/workspace-dom.mjs";
import { applyWorkspaceEvent } from "../public/workspace-events.mjs";
import { createDocument } from "./mini-dom.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

const fileApproval = {
  id: 21,
  method: "item/fileChange/requestApproval",
  params: { threadId: "thread-a", turnId: "turn-5", itemId: "call-7", startedAtMs: 1, reason: null },
};

function fileChangeItem(diffLines = 3) {
  return {
    type: "fileChange",
    id: "call-7",
    status: "inProgress",
    changes: [
      {
        path: "textkit/legacy/dates.py",
        kind: { type: "update", move_path: null },
        diff: Array.from({ length: diffLines }, (_, index) => `+line ${index + 1}`).join("\n"),
      },
      { path: "textkit/new_module.py", kind: { type: "add" }, diff: "+print('hi')" },
    ],
  };
}

function workspace() {
  const document = createDocument();
  const root = document.createElement("div");
  document.body.append(root);
  const state = {
    ...workspaceFixture(),
    threadId: "thread-a",
    projectId: "project-1",
    liveText: "",
    pendingRequests: [],
    requestItems: new Map(),
  };
  const view = createWorkspaceDom(root, { schedule: () => {} });
  view.update(state);
  const apply = (message) => {
    const effect = applyWorkspaceEvent(state, message);
    if (effect.sections.length) view.update(state, effect.sections);
    return effect;
  };
  const card = () => root.querySelector("[data-request-key]");
  return { state, apply, card };
}

test("a file-change approval shows the changed paths and diff before it can be approved", () => {
  const { apply, card } = workspace();
  apply({
    method: "item/started",
    params: { threadId: "thread-a", turnId: "turn-5", item: fileChangeItem(), startedAtMs: 1 },
  });
  const effect = apply(fileApproval);

  assert.deepEqual(effect, { sections: ["requests"], refresh: false });
  assert.deepEqual(
    card().querySelectorAll(".approval-file code").map((code) => code.textContent),
    ["textkit/legacy/dates.py", "textkit/new_module.py"],
  );
  assert.deepEqual(
    card().querySelectorAll(".approval-file .badge").map((badge) => badge.textContent),
    ["Edit", "Add"],
  );
  assert.equal(card().querySelector(".approval-diff").textContent, "+line 1\n+line 2\n+line 3");
  assert.equal(card().querySelector('[data-action="approve"]').disabled, false);
});

test("an approval whose files are unknown cannot be approved until they arrive", () => {
  const { apply, card, state } = workspace();
  const effect = apply(fileApproval);
  assert.deepEqual(effect, { sections: ["requests"], refresh: true });
  assert.equal(card().querySelector('[data-action="approve"]').disabled, true);
  assert.match(card().textContent, /changed files for this request are not available yet/);
  assert.equal(card().querySelector('[data-action="decline"]').disabled, false);

  // A streamed patch is shown but cannot be approved; only the canonical item enables it.
  const partial = apply({
    method: "item/fileChange/patchUpdated",
    params: { threadId: "thread-a", turnId: "turn-5", itemId: "call-7", changes: fileChangeItem().changes.slice(0, 1) },
  });
  assert.deepEqual(partial.sections, ["requests"]);
  assert.equal(card().querySelector('[data-action="approve"]').disabled, true);
  assert.match(card().textContent, /still being prepared/);

  apply({
    method: "item/started",
    params: { threadId: "thread-a", turnId: "turn-5", item: fileChangeItem(), startedAtMs: 1 },
  });
  assert.equal(card().querySelector('[data-action="approve"]').disabled, false);
  assert.equal(card().querySelectorAll(".approval-file").length, 2);
  assert.equal(state.requestItems.get("call-7").partial, false);

  // A late streamed update cannot replace the complete item.
  apply({
    method: "item/fileChange/patchUpdated",
    params: { threadId: "thread-a", turnId: "turn-5", itemId: "call-7", changes: [] },
  });
  assert.equal(card().querySelectorAll(".approval-file").length, 2);
});

test("recorded activity outranks a partial streamed patch", () => {
  const { apply, card, state } = workspace();
  apply({
    method: "item/fileChange/patchUpdated",
    params: { threadId: "thread-a", turnId: "turn-5", itemId: "call-7", changes: fileChangeItem().changes.slice(1) },
  });
  state.activity = [...state.activity, { turnId: "turn-5", item: fileChangeItem() }];
  apply(fileApproval);

  assert.deepEqual(
    card().querySelectorAll(".approval-file code").map((code) => code.textContent),
    ["textkit/legacy/dates.py", "textkit/new_module.py"],
  );
  assert.equal(card().querySelector('[data-action="approve"]').disabled, false);
});

test("a reconnect snapshot with an unknown approval item asks for a refresh", () => {
  const { apply } = workspace();
  const snapshot = { method: "gateway/pendingRequests", params: { threadId: "thread-a", requests: [fileApproval] } };
  assert.deepEqual(apply(snapshot), { sections: ["requests"], refresh: true });
});

test("recorded activity supplies the changed files after a reconnect", () => {
  const { apply, card, state } = workspace();
  state.activity = [...state.activity, { turnId: "turn-5", item: fileChangeItem() }];
  apply({ method: "gateway/pendingRequests", params: { threadId: "thread-a", requests: [fileApproval] } });

  assert.equal(card().querySelector('[data-action="approve"]').disabled, false);
  assert.equal(card().querySelector(".approval-file code").textContent, "textkit/legacy/dates.py");
});

test("long diffs show a bounded preview with the rest behind a disclosure", () => {
  const { apply, card } = workspace();
  apply({
    method: "item/started",
    params: { threadId: "thread-a", turnId: "turn-5", item: fileChangeItem(45), startedAtMs: 1 },
  });
  apply(fileApproval);

  const file = card().querySelector(".approval-file");
  assert.equal(file.querySelector(".approval-diff").textContent.split("\n").length, 40);
  assert.equal(file.querySelector("details summary").textContent, "Show all 45 lines");
  assert.equal(file.querySelector("details .approval-diff").textContent.split("\n").length, 45);
});

test("another thread's items never describe this thread's approvals", () => {
  const { apply, card } = workspace();
  apply({
    method: "item/started",
    params: { threadId: "thread-b", turnId: "turn-5", item: fileChangeItem(), startedAtMs: 1 },
  });
  apply(fileApproval);
  assert.equal(card().querySelector('[data-action="approve"]').disabled, true);
});

// The server joins argv with POSIX shell quoting; mirror it so tests use real wire input.
function shellJoin(words) {
  return words
    .map((word) => (/^[\w@%+=:,./-]+$/.test(word) ? word : `'${word.replaceAll("'", `'"'"'`)}'`))
    .join(" ");
}

test("command approvals show the command without its shell wrapper, and its directory", () => {
  const { apply, card } = workspace();
  const command = shellJoin([
    "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
    "-NoProfile",
    "-Command",
    "git commit -m 'fix dates'",
  ]);
  apply({
    id: 22,
    method: "item/commandExecution/requestApproval",
    params: { threadId: "thread-a", turnId: "t", itemId: "c1", startedAtMs: 1, command, cwd: "C:/work/textkit" },
  });

  assert.equal(card().querySelector(".approval-command").textContent, "git commit -m 'fix dates'");
  assert.equal(card().querySelector(".path").textContent, "in C:/work/textkit");
  assert.equal(card().querySelector("details .approval-command").textContent, command);
});

test("shell wrappers are removed only for a shell running exactly one script", () => {
  const script = `python -c "print('ok')"`;
  assert.deepEqual(
    [
      shellJoin(["/bin/bash", "-lc", script]),
      `bash -lc "python -c \\"print('ok')\\""`,
      shellJoin(["pwsh.exe", "-Command", "Get-ChildItem src"]),
      shellJoin(["powershell", "-NoLogo", "-NoProfile", "-c", "dir"]),
    ].map(displayCommand),
    [script, script, "Get-ChildItem src", "dir"],
  );
  for (const unchanged of [
    "git status --short",
    "pwsh.exe script.ps1",
    "ssh -c aes128-ctr server 'rm -rf ~/Documents'",
    "mypwsh -Command 'rm -rf /'",
    "bash -c 'echo one' extra-argument",
    "pwsh -Command 'Get-Item a' 'Remove-Item b'",
    "pwsh -Version -Command 'Write-Output should-not-run'",
    "bash -nc 'printf should-not-run'",
    "bash -lc 'unterminated",
  ]) {
    assert.equal(displayCommand(unchanged), unchanged);
  }
  assert.deepEqual(splitShellWords(`a 'b c' "d\\"e" f\\ g`), ["a", "b c", 'd"e', "f g"]);
});

test("oversized diffs are bounded in memory and on screen, with a warning", () => {
  const { apply, card, state } = workspace();
  const huge = { ...fileChangeItem(), changes: [{ path: "big.txt", kind: { type: "add" }, diff: `+${"x".repeat(300 * 1024)}` }] };
  apply({ method: "item/started", params: { threadId: "thread-a", turnId: "t", item: huge, startedAtMs: 1 } });
  apply(fileApproval);

  const stored = state.requestItems.get("call-7").changes[0];
  assert.equal(stored.diff.length, 256 * 1024);
  assert.equal(stored.diffTruncated, true);
  assert.equal(card().querySelector(".approval-diff").textContent.length < 500, true);
  assert.match(card().textContent, /too large to show here/);
});

test("replacing an approval card keeps an open diff and its focused summary", () => {
  const { apply, card, state } = workspace();
  apply({ method: "item/started", params: { threadId: "thread-a", turnId: "t", item: fileChangeItem(45), startedAtMs: 1 } });
  apply(fileApproval);
  const details = card().querySelector(".approval-file details");
  details.open = true;
  details.querySelector("summary").focus();
  const diff = card().querySelector(".approval-file .approval-diff");
  diff.scrollTop = 45;
  diff.scrollLeft = 180;

  apply({ ...fileApproval, params: { ...fileApproval.params, reason: "Also update the changelog" } });

  assert.equal(card().querySelector(".approval-file details").open, true);
  const restored = card().querySelector(".approval-file .approval-diff");
  assert.deepEqual([restored.scrollTop, restored.scrollLeft], [45, 180]);
  assert.equal(
    card().ownerDocument.activeElement.getAttribute("data-disclosure"),
    "diff-0",
  );
  assert.match(card().textContent, /Also update the changelog/);
  assert.equal(state.pendingRequests.length, 1);
});

test("remembered approval items are bounded", () => {
  const { apply, state } = workspace();
  for (let index = 0; index < 60; index += 1) {
    apply({
      method: "item/completed",
      params: { threadId: "thread-a", turnId: "t", item: { ...fileChangeItem(), id: `call-${index}` }, completedAtMs: 1 },
    });
  }
  assert.equal(state.requestItems.size, 50);
  assert.equal(state.requestItems.has("call-9"), false);
  assert.equal(state.requestItems.has("call-59"), true);
});
