import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { runAttempt } from "../harness/attempt-runner.mjs";
import { prepareIsolatedHome } from "../harness/isolated-home.mjs";

const scenario = { id: "surface.tui.fake", fixture: { sha256: "sha256:fixture" }, conversation: { turns: [], turnTimeoutSeconds: 1 } };

test("seeds an isolated home and hard-links only auth.json", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "scbench-home-"));
  const auth = path.join(root, "auth");
  await mkdir(path.join(auth, ".sandbox"), { recursive: true });
  await mkdir(path.join(auth, ".sandbox-bin"), { recursive: true });
  await writeFile(path.join(auth, "auth.json"), "auth");
  await writeFile(path.join(auth, "cap_sid"), "sid");
  const { codexHome } = await prepareIsolatedHome(path.join(root, "attempt"), auth);
  assert.equal((await readFile(path.join(codexHome, "auth.json"), "utf8")), "auth");
  assert.equal(existsSync(path.join(codexHome, ".sandbox")), true);
  assert.equal(existsSync(path.join(codexHome, ".sandbox-bin")), true);
  assert.equal(existsSync(path.join(codexHome, "cap_sid")), true);
});

test("records a timeout and retains stdout, stderr, and an atomic manifest", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "scbench-attempt-"));
  const fixture = path.join(root, "fixture");
  const auth = path.join(root, "auth");
  await mkdir(fixture);
  await mkdir(path.join(auth, ".sandbox"), { recursive: true });
  await mkdir(path.join(auth, ".sandbox-bin"), { recursive: true });
  await writeFile(path.join(auth, "auth.json"), "auth");
  await writeFile(path.join(auth, "cap_sid"), "sid");
  scenario.fixture.resolvedSource = fixture;
  const manifestResult = await runAttempt({
    scenario,
    rep: 1,
    out: root,
    workRoot: path.join(root, "workspaces"),
    codex: process.execPath,
    authHome: auth,
    adapter: async () => ({ exitReason: "timedOut", code: null, signal: "SIGTERM" }),
  });
  const manifest = manifestResult.attempt;
  const attemptRoot = path.join(root, "attempts", scenario.id, "stateful", "1");
  assert.equal(manifest.exitReason, "timedOut");
  assert.equal(manifest.retryClassification, "none");
  assert.deepEqual(JSON.parse(await readFile(path.join(attemptRoot, "attempt.json"), "utf8")), manifest);
  assert.equal(manifest.evidence.includes("attempt.json"), false);
  assert.equal(manifest.evidence.includes("evidence/workspace.patch"), true);
});
