import assert from "node:assert/strict";
import test from "node:test";

import { visibleAnswer } from "../public/outcome-trailer.mjs";

const ANSWER = "A leap year has 366 days.";
const BLOCK =
  "[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

test("a complete trailing outcome block is never shown", () => {
  assert.deepEqual(
    [
      `${ANSWER}\n\n${BLOCK}`,
      `${ANSWER}\r\n\r\n${BLOCK.replaceAll("\n", "\r\n")}\r\n`,
      `${ANSWER}\n${BLOCK}\n\n`,
      `${ANSWER}\n[stateful-outcome]\ndisposition: blocked\nopen-issues:\n- The source is missing.\n[/stateful-outcome]`,
    ].map((text) => visibleAnswer(text)),
    [ANSWER, ANSWER, ANSWER, ANSWER],
  );
});

test("ordinary prose and quoted examples stay visible", () => {
  const fenced = `Write it like this:\n\`\`\`\n${BLOCK}\n\`\`\`\nThen stop.`;
  const inline = `The block starts with [stateful-outcome] on its own line.`;
  const unclosed = `${ANSWER}\n[stateful-outcome]\ndisposition: answer`;
  const foreign = `${ANSWER}\n[stateful-outcome]\nsomething else\n[/stateful-outcome]`;
  const oversized = `${ANSWER}\n[stateful-outcome]\n${"- x\n".repeat(1500)}[/stateful-outcome]`;
  for (const text of [fenced, inline, unclosed, foreign, oversized, ANSWER]) {
    assert.equal(visibleAnswer(text), text);
  }
});

test("a streamed trailer is withheld at every split, including a partial opener", () => {
  const full = `${ANSWER}\n\n${BLOCK}`;
  const shown = [];
  for (let end = 0; end <= full.length; end += 1) {
    const visible = visibleAnswer(full.slice(0, end), { streaming: true });
    assert.ok(!visible.includes("[stateful"), `prefix ${end}: ${visible}`);
    assert.ok(!visible.includes("disposition"), `prefix ${end}: ${visible}`);
    shown.push(visible);
  }
  assert.equal(shown.at(-1), ANSWER);
  assert.equal(visibleAnswer(full), ANSWER);
});

test("streamed text that turns out not to be a trailer is released", () => {
  const text = `${ANSWER}\n[stateful-outcome]\nnot a trailer line\nmore`;
  assert.equal(visibleAnswer(text, { streaming: true }), text);
  assert.equal(
    visibleAnswer(`${ANSWER}\n[state of the art]`, { streaming: true }),
    `${ANSWER}\n[state of the art]`,
  );
});
