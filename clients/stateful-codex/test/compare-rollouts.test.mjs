import assert from "node:assert/strict";
import test from "node:test";

import {
  compareSummaries,
  summarizeEvents,
} from "../eval/compare-rollouts.mjs";

function events({ id, stateful = false, input, cached, output }) {
  const toolInput = stateful
    ? "text(await tools.blackboard_query({text: 'files'})); text(await tools.evidence_read({relativePath: 'decision.md'})); text(await tools.obligation_update({}));"
    : "text(await tools.exec_command({cmd: 'Get-ChildItem'}));";
  return [
    {
      type: "session_meta",
      payload: {
        id,
        originator: "codex-tui",
        source: "cli",
        model: "test-model",
        cwd: "C:/work",
      },
    },
    {
      type: "turn_context",
      payload: {
        model: "test-model",
        effort: "high",
        approval_policy: "never",
        sandbox_policy: { type: "danger-full-access" },
        permission_profile: { type: "disabled" },
        cwd: "C:/work",
        workspace_roots: ["C:/work"],
      },
    },
    {
      type: "response_item",
      payload: {
        type: "message",
        role: "user",
        content: [{ type: "input_text", text: "Count files" }],
      },
    },
    {
      type: "response_item",
      payload: { type: "custom_tool_call", name: "exec", input: toolInput },
    },
    {
      type: "token_usage_record",
      payload: {},
    },
    {
      type: "response_item",
      payload: {
        type: "message",
        role: "assistant",
        content: [{ type: "output_text", text: "Verified 8 files" }],
      },
    },
    {
      type: "event_msg",
      payload: {
        type: "token_count",
        info: {
          total_token_usage: {
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
            reasoning_output_tokens: 10,
            total_tokens: input + output,
          },
        },
      },
    },
  ];
}

test("compares full and uncached usage without mixing their definitions", () => {
  const baseline = summarizeEvents(
    events({ id: "base", input: 100, cached: 40, output: 10 }),
    ["8 files"],
  );
  const stateful = summarizeEvents(
    events({ id: "stateful", stateful: true, input: 150, cached: 100, output: 20 }),
    ["8 files"],
  );
  const actual = compareSummaries(baseline, stateful);

  assert.equal(actual.comparable, true);
  assert.equal(actual.delta.totalTokens.absolute, 60);
  assert.ok(
    Math.abs(actual.delta.totalTokens.percent - 600 / 11) < 1e-12,
  );
  assert.deepEqual(actual.delta.uncachedTotalTokens, {
    absolute: 0,
    percent: 0,
  });
  assert.equal(actual.baseline.calls.readBearingToolCalls, 1);
  assert.equal(actual.stateful.calls.readBearingToolCalls, 1);
  assert.equal(actual.stateful.calls.blackboardQueries, 1);
  assert.equal(actual.stateful.calls.evidenceReads, 1);
  assert.equal(actual.stateful.calls.obligationUpdates, 1);
  assert.equal(actual.stateful.expectations.allPresent, true);
});
