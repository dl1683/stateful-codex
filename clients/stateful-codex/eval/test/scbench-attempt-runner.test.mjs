import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { runAttempt } from "../harness/attempt-runner.mjs";
import { prepareIsolatedHome } from "../harness/isolated-home.mjs";

const scenario = { id: "surface.tui.fake", fixture: { sha256: "sha256:fixture" }, conversation: { turnTimeoutSeconds: 1 } };

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
  const attemptRoot = path.join(root, "attempt");
  const manifest = await runAttempt({ attemptRoot, scenario, codex: process.execPath, fixtureRoot: fixture, authHome: auth, args: ["-e", "setTimeout(() => {}, 5000)"], timeoutSeconds: 0.05 });
  assert.equal(manifest.exitReason, "timedOut");
  assert.equal(manifest.retryClassification, "none");
  assert.deepEqual(JSON.parse(await readFile(path.join(attemptRoot, "attempt.json"), "utf8")), manifest);
  assert.equal(manifest.evidence.includes("stdout.txt"), true);
  assert.equal(manifest.evidence.includes("stderr.txt"), true);
});
