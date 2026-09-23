import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { corpusHash } from "../eval/corpus-hash.mjs";
import {
  authEnvironment,
  nextAttemptNumber,
  platformSandboxArgs,
  prepareIsolatedCodexHome,
  unresolvedAttemptForCase,
} from "../eval/run-longitudinal-arm.mjs";
import {
  applyScheduledIntervention,
  validateInterventionSchedule,
} from "../eval/source-intervention.mjs";

test("numbers attempts without overwriting earlier evidence", () => {
  const state = {
    attempts: [
      { caseId: "q01", number: 1, status: "completed" },
      { caseId: "q02", number: 1, status: "completed" },
      { caseId: "q01", number: 2, status: "completed" },
    ],
  };
  assert.equal(nextAttemptNumber(state, "q01"), 3);
  assert.equal(nextAttemptNumber(state, "q03"), 1);
});

test("requires reconciliation after a failed or interrupted attempt", () => {
  const failed = { caseId: "q01", number: 1, status: "failed" };
  const state = {
    attempts: [
      { caseId: "q00", number: 1, status: "completed" },
      failed,
    ],
  };
  assert.equal(unresolvedAttemptForCase(state, "q01"), failed);
  assert.equal(unresolvedAttemptForCase(state, "q00"), undefined);
});

test("isolates evaluation history while reusing the cached login", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "stateful-auth-home-"));
  const authHome = path.join(root, "auth");
  const codexHome = path.join(root, "isolated");
  const sqliteHome = path.join(root, "sqlite");
  try {
    await mkdir(authHome);
    await writeFile(path.join(authHome, "auth.json"), "cached login");
    await prepareIsolatedCodexHome(codexHome, authHome);
    const [source, target] = await Promise.all([
      stat(path.join(authHome, "auth.json")),
      stat(path.join(codexHome, "auth.json")),
    ]);
    assert.equal(source.dev, target.dev);
    assert.equal(source.ino, target.ino);

    const environment = authEnvironment(sqliteHome, codexHome);
    assert.equal(environment.CODEX_HOME, codexHome);
    assert.equal(environment.CODEX_SQLITE_HOME, sqliteHome);
    assert.equal(environment.OPENAI_API_KEY, undefined);
    assert.equal(environment.CODEX_API_KEY, undefined);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("selects the Windows sandbox without changing other platforms", () => {
  assert.deepEqual(platformSandboxArgs("win32"), [
    "-c",
    'windows.sandbox="unelevated"',
  ]);
  assert.deepEqual(platformSandboxArgs("linux"), []);
  assert.deepEqual(platformSandboxArgs("darwin"), []);
});

test("applies a hash-pinned source intervention once and resumes idempotently", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "stateful-intervention-"));
  const workspace = path.join(root, "workspace");
  const manifestDirectory = path.join(root, "manifest");
  await Promise.all([mkdir(workspace), mkdir(manifestDirectory)]);
  const target = path.join(workspace, "facts.md");
  const replacementPath = path.join(manifestDirectory, "facts-v2.md");
  const beforeContent = Buffer.from("The limit is ten.\n");
  const afterContent = Buffer.from("The amended limit is six.\n");
  try {
    await writeFile(target, beforeContent);
    await writeFile(replacementPath, afterContent);
    const beforeCorpusRevision = `sha256:${(await corpusHash(workspace)).sha256}`;
    await writeFile(target, afterContent);
    const afterCorpusRevision = `sha256:${(await corpusHash(workspace)).sha256}`;
    await writeFile(target, beforeContent);
    const state = {
      corpusRevision: beforeCorpusRevision,
      appliedInterventions: [],
    };
    const intervention = {
      id: "amendment-v2",
      expectedBeforeCorpusRevision: beforeCorpusRevision,
      expectedAfterCorpusRevision: afterCorpusRevision,
      files: [
        {
          path: "facts.md",
          replacement: "facts-v2.md",
          expectedBeforeSha256: hash(beforeContent),
          replacementSha256: hash(afterContent),
        },
      ],
    };
    assert.deepEqual(
      await validateInterventionSchedule(
        [{ id: "q02", intervention }],
        beforeCorpusRevision,
        manifestDirectory,
      ),
      [
        {
          caseId: "q02",
          id: intervention.id,
          beforeCorpusRevision,
          afterCorpusRevision,
          files: [
            {
              path: "facts.md",
              beforeSha256: hash(beforeContent),
              afterSha256: hash(afterContent),
            },
          ],
        },
      ],
    );
    const applied = await applyScheduledIntervention({
      workspace,
      manifestDirectory,
      intervention,
      state,
    });
    assert.equal(applied.applied, true);
    assert.equal(await readFile(target, "utf8"), afterContent.toString());
    assert.equal(state.corpusRevision, afterCorpusRevision);
    assert.equal(state.appliedInterventions.length, 1);

    const resumed = await applyScheduledIntervention({
      workspace,
      manifestDirectory,
      intervention,
      state,
    });
    assert.equal(resumed.applied, false);
    assert.deepEqual(resumed.record, applied.record);
    assert.equal(state.appliedInterventions.length, 1);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

function hash(value) {
  return createHash("sha256").update(value).digest("hex");
}
