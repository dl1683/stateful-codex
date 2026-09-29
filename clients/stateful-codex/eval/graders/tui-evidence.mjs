import { access, readFile } from "node:fs/promises";
import path from "node:path";

const REQUIRED = ["transcript.txt", "actions.jsonl", "approvals.jsonl", "turns.json", "rollout.jsonl", "project-state.json", "store-inventory.json", "workspace.patch"];

export async function gradeAttempt({ attemptRoot, scenario, attempt }) {
  const evidence = path.join(attemptRoot, "evidence");
  const missing = [];
  for (const file of REQUIRED) try { await access(path.join(evidence, file)); } catch { missing.push(file); }
  const turns = await json(path.join(evidence, "turns.json"), []);
  const text = await textFiles(evidence);
  const delivered = turns.length === scenario.conversation.turns.length && turns.every(({ status }) => status === "idle");
  const findings = [
    finding("userMessagesDelivered", delivered, `${turns.filter(({ status }) => status === "idle").length}/${scenario.conversation.turns.length} follow-up turns idle`),
    finding("statefulUpdatesVisible", /stateful update/i.test(text), "screen/transcript text"),
    finding("runCellsVisible", /run cell|\bstateful run\b/i.test(text), "screen/transcript text"),
    finding("durableRuns", await hasState(evidence, "runs"), "project-state.json"),
    finding("durableObligations", await hasState(evidence, "obligations"), "project-state.json"),
    finding("durableSteering", await hasState(evidence, "steering"), "project-state.json"),
    finding("finalRunStatus", /completed|paused|running/i.test(text), "screen/transcript text"),
    finding("claimedTransitionsMatchStore", false, "requires project-state export"),
    finding("evidenceComplete", missing.length === 0, missing.length ? `missing: ${missing.join(", ")}` : "all required files present"),
    finding("attemptCompleted", attempt.exitReason === "processExited", attempt.exitReason),
  ];
  return { scenarioId: scenario.id, repetition: attempt.repetition, valid: missing.length === 0 && delivered, findings };
}

function finding(id, observed, evidence) { return { id, status: observed ? "observed" : "notObserved", evidence }; }
async function hasState(evidence, key) { return /"available"\s*:\s*true/.test(await readFile(path.join(evidence, "project-state.json"), "utf8")) && new RegExp(`"${key}"`).test(await readFile(path.join(evidence, "project-state.json"), "utf8")); }
async function json(file, fallback) { try { return JSON.parse(await readFile(file, "utf8")); } catch { return fallback; } }
async function textFiles(root) { const files = ["transcript.txt", "screen-final.txt"]; return (await Promise.all(files.map(async (file) => { try { return await readFile(path.join(root, file), "utf8"); } catch { return ""; } }))).join("\n"); }
