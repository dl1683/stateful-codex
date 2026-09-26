import assert from "node:assert/strict";
import test from "node:test";

import { summarizeLongitudinalEvents } from "../eval/summarize-longitudinal-rollout.mjs";
import {
  completed,
  context,
  message,
  metadata,
  projectFragment,
  started,
  toolCall,
  toolOutput,
  usage,
  usageRecord,
} from "./longitudinal-rollout-fixture.mjs";

test("attributes usage, compaction, state projection, reads, and failures per turn", () => {
  const firstUsage = usage(100, 40, 10, 2);
  const secondUsageA = usage(200, 150, 20, 4);
  const secondUsageB = usage(50, 0, 5, 1);
  const events = [
    metadata("thread-stateful"),
    started("turn-1", 100),
    message("turn-1", "developer", projectFragment),
    context("turn-1"),
    message("turn-1", "user", "Question one?"),
    toolCall(
      "turn-1",
      "read-1",
      "text(await tools.evidence_read({relativePath: 'decision.md', lineRange: {start: 2, end: 4}}));",
    ),
    toolOutput("read-1", "Script completed"),
    usageRecord("turn-1", "response-1", firstUsage, firstUsage, firstUsage),
    message(
      "turn-1",
      "developer",
      "<stateful_project_update>Project intelligence revision advanced from 7 to 8. The model-visible root blackboard knowledge and source routes are unchanged.</stateful_project_update>",
    ),
    message("turn-1", "assistant", "Answer one"),
    completed("turn-1", 100, 900),
    started("turn-2", 110),
    context("turn-2"),
    message("turn-2", "user", "Question two?"),
    {
      type: "compacted",
      payload: {
        window_number: 1,
        replacement_history: [],
        compaction_response_id: "compact-1",
        latest_token_usage_record: { thread_token_usage: firstUsage },
      },
    },
    context("turn-2"),
    toolCall(
      "turn-2",
      "bad-call",
      "text(await tools.obligation_update({next: 'answer'}));",
    ),
    toolOutput("bad-call", "Script failed\nScript error:\ninvalid type"),
    toolCall(
      "turn-2",
      "history-call",
      "text(await tools.blackboard_query({text: 'threshold', entryScope: 'historical'}));",
    ),
    toolOutput("history-call", "Script completed"),
    usageRecord(
      "turn-2",
      "response-2",
      secondUsageA,
      secondUsageA,
      usage(300, 190, 30, 6),
    ),
    toolCall(
      "turn-2",
      "read-2",
      [
        "text(await tools.evidence_read({relativePath: 'decision.md', lineStart: 2, lineEnd: 4}));",
        "text(await tools.evidence_read({relativePath: 'decision.md', lineStart: 5, lineEnd: 6}));",
      ].join("\n"),
    ),
    toolOutput("read-2", "Script completed"),
    usageRecord(
      "turn-2",
      "response-3",
      secondUsageB,
      usage(250, 150, 25, 5),
      usage(350, 190, 35, 7),
    ),
    message("turn-2", "assistant", "Answer two"),
    completed("turn-2", 110, 1200),
  ];

  const summary = summarizeLongitudinalEvents(events);

  assert.equal(summary.turns.length, 2);
  assert.deepEqual(summary.turns[0].usage, {
    inputTokens: 100,
    cachedInputTokens: 40,
    uncachedInputTokens: 60,
    outputTokens: 10,
    reasoningOutputTokens: 2,
    totalTokens: 110,
    uncachedTotalTokens: 70,
  });
  assert.equal(summary.turns[1].usage.totalTokens, 275);
  assert.equal(summary.turns[1].usage.uncachedTotalTokens, 125);
  assert.equal(summary.turns[1].modelResponses, 2);
  assert.equal(summary.turns[1].compaction.observed, 1);
  assert.equal(summary.turns[1].compaction.canonicalEvents[0].windowNumber, 1);
  assert.equal(summary.turns[1].calls.rejectedToolResults, 1);
  assert.deepEqual(summary.turns[1].calls.blackboardEntryScopes, ["historical"]);
  assert.equal(summary.turns[1].calls.evidenceReads.length, 2);
  assert.deepEqual(summary.turns[0].calls.evidenceReads, [
    { relativePath: "decision.md", lineStart: 2, lineEnd: 4 },
  ]);
  assert.equal(summary.turns[1].calls.repeatedEvidenceReads, 1);
  assert.equal(summary.turns[0].projectState.atFirstResponse.revision, 7);
  assert.equal(summary.turns[0].projectState.atLastResponse.revision, 8);
  assert.equal(summary.turns[1].projectState.atStart.revision, 8);
  assert.equal(summary.turns[1].projectState.atFirstResponse.revision, 8);
  assert.equal(summary.turns[1].projectState.atFirstResponse.rootEntries, 1);
  assert.equal(summary.turns[1].projectState.atFirstResponse.evidenceRoutes, 1);
  assert.equal(summary.turns[1].projectState.atFirstResponse.omittedRootEntries, 2);
  assert.deepEqual(
    summary.turns[1].projectState.atFirstResponse.entryEvidenceFreshness,
    { current: 1, stale: 0, sourceUnavailable: 0 },
  );
});
