import assert from "node:assert/strict";
import test from "node:test";
import { tuiDriverArgs } from "../adapters/tui.mjs";

test("TUI adapter preserves the selected mode and isolated workspace", () => {
  const args = tuiDriverArgs({ conversation: { mode: "socratic" } }, "C:/workspace", "C:/codex.exe");
  assert.deepEqual(args, { mode: "socratic", workspace: "C:/workspace", codex: "C:/codex.exe" });
});
