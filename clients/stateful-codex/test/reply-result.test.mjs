import assert from "node:assert/strict";
import test from "node:test";

import { interpretReplyResult } from "../public/reply-result.mjs";

test("an accepted reply is not retired", () => {
  assert.deepEqual(interpretReplyResult(202, null), { retired: false });
});

test("a request answered elsewhere is retired quietly", () => {
  assert.deepEqual(interpretReplyResult(409, { accepted: false, code: "resolved" }), {
    retired: true,
  });
});

test("a reply for another thread or a transport failure is an error", () => {
  assert.throws(
    () =>
      interpretReplyResult(409, {
        accepted: false,
        code: "foreignThread",
        reason: "request belongs to another thread",
      }),
    /another thread/,
  );
  assert.throws(() => interpretReplyResult(500, { error: { message: "boom" } }), /boom/);
  assert.throws(() => interpretReplyResult(502, null), /Codex reply failed/);
});
