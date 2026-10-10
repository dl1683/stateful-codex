import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";

import { selectRun } from "../eval/export-project-state.mjs";

function runtimeWith({ answers }) {
  const database = new DatabaseSync(":memory:");
  database.exec(`
    CREATE TABLE stateful_runs (
      id TEXT, project_id TEXT, goal TEXT, mode TEXT, status TEXT, strategy TEXT,
      strategy_revision INTEGER, result TEXT, revision INTEGER, max_continuations INTEGER,
      max_elapsed_seconds INTEGER, continuations_used INTEGER, created_at_ms INTEGER,
      updated_at_ms INTEGER
    );
    CREATE TABLE stateful_run_threads (run_id TEXT, thread_id TEXT, position INTEGER);
    INSERT INTO stateful_runs VALUES
      ('run-done', 'p', 'g', 'autonomous', 'completed', '', 1, 'r', 1, 0, 0, 0, 1, 1),
      ('run-answered', 'p', 'g', 'autonomous', 'completed', '', 1, 'a', 1, 0, 0, 0, 1, 2);
    INSERT INTO stateful_run_threads VALUES ('run-done', 't1', 0), ('run-answered', 't2', 0);
  `);
  if (answers) {
    database.exec(`
      CREATE TABLE stateful_host_answers (run_id TEXT);
      INSERT INTO stateful_host_answers VALUES ('run-answered');
    `);
  }
  return database;
}

test("an answered run is exported as answered, never completed, in both lookup modes", () => {
  const database = runtimeWith({ answers: true });
  assert.deepEqual(
    [
      selectRun(database, null, "run-answered").status,
      selectRun(database, "t2", null).status,
      selectRun(database, null, "run-done").status,
      selectRun(database, "t1", null).status,
    ],
    ["answered", "answered", "completed", "completed"],
  );
});

test("a store from before answer records exports raw status", () => {
  const database = runtimeWith({ answers: false });
  assert.equal(selectRun(database, null, "run-answered").status, "completed");
});
