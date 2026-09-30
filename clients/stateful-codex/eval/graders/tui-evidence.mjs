import { access, readFile, stat } from "node:fs/promises";
import path from "node:path";

const REQUIRED = ["transcript.txt", "transcript.jsonl", "actions.jsonl", "approvals.jsonl", "turns.json", "screen-final.txt", "rollout.jsonl", "rollout-source.json", "project-state.json", "store-inventory.json", "store-snapshot.json", "sandbox-preflight.json", "workspace-before.json", "workspace-after.json", "workspace.patch"];

export async function gradeAttempt({ attemptRoot, scenario, attempt }) {
  const evidence = path.join(attemptRoot, "evidence");
  const files = await inspectFiles(evidence);
  const missing = REQUIRED.filter((file) => !files[file].exists);
  const empty = REQUIRED.filter((file) => files[file].exists && files[file].bytes === 0 && !["approvals.jsonl", "workspace.patch"].includes(file));
  const turns = await json(path.join(evidence, "turns.json"), []);
  const source = await json(path.join(evidence, "rollout-source.json"), null);
  const rollout = await rolloutMessages(path.join(evidence, "rollout.jsonl"));
  const expected = [scenario.conversation.initialMessage, ...scenario.conversation.turns];
  const messagesMatch = rollout.messages.length === expected.length && rollout.messages.every((message, index) => message === expected[index]);
  const actionRecords = await jsonl(path.join(evidence, "actions.jsonl"));
  const transcriptRecords = await jsonl(path.join(evidence, "transcript.jsonl"));
  const state = await json(path.join(evidence, "project-state.json"), null);
  const snapshot = await json(path.join(evidence, "store-snapshot.json"), null);
  const infraFailure = /model:\s*loading|sandbox setup failed|code-mode host closed|read-only behavior/i.test(await textFile(evidence, "screen-final.txt"));
  const stateValid = attempt.arm === "base" ? state?.applicable === false : state?.available === true && state?.run?.id && state?.projectId;
  const gate = {
    adapterSuccess: attempt.exitReason === "processExited" && attempt.process?.code === 0,
    infrastructureValid: !infraFailure,
    messagesDelivered: turns.length === expected.length - 1 && turns.every(({ status }) => status === "idle") && actionRecords.filter(({ status }) => status === "delivered").length === expected.length - 1,
    messageBoundariesConfirmed: messagesMatch && source?.threadId && source.candidates?.length === 1,
    rolloutNonempty: files["rollout.jsonl"].bytes > 0 && rollout.events > 0,
    requestedStateReadable: stateValid,
    evidenceNonPlaceholder: missing.length === 0 && empty.length === 0 && transcriptRecords.length > 0 && actionRecords.length >= expected.length,
    workspaceIsolated: Boolean(attempt.workspace?.baseline && attempt.workspace?.path) && path.resolve(attempt.workspace.path) !== path.resolve(attemptRoot, "workspace"),
    storeSnapshotSealed: snapshot?.consistent === true,
  };
  const failures = Object.entries(gate).filter(([, passed]) => !passed).map(([name]) => name);
  const findings = [
    finding("userMessagesDelivered", gate.messagesDelivered && gate.messageBoundariesConfirmed, gate.messagesDelivered ? `${rollout.messages.length}/${expected.length} exact rollout messages` : "driver turn/action records do not cover every message"),
    finding("statefulUpdatesVisible", /stateful update/i.test(await textFiles(evidence)), "screen/transcript text"),
    finding("runCellsVisible", /run cell|stateful run/i.test(await textFiles(evidence)), "screen/transcript text"),
    finding("durableRuns", attempt.arm === "base" ? "notGradable" : state?.counts?.blackboardEntries !== undefined ? "observed" : "notGradable", "project-state.json"),
    finding("durableObligations", attempt.arm === "base" ? "notGradable" : state?.counts?.obligations > 0 ? "observed" : "notObserved", "project-state.json"),
    finding("durableSteering", attempt.arm === "base" ? "notGradable" : state?.counts?.steering > 0 ? "observed" : "notObserved", "project-state.json"),
    finding("finalRunStatus", state?.run?.status ? "observed" : attempt.arm === "base" ? "notGradable" : "notObserved", "project-state.json"),
    finding("claimedTransitionsMatchStore", state?.run?.status ? "observed" : "notGradable", "project-state.json"),
    finding("evidenceComplete", missing.length || empty.length ? "blocked" : "observed", [...missing, ...empty].join(", ") || "all required artifacts are present and non-placeholder"),
    finding("attemptCompleted", gate.adapterSuccess ? "observed" : "blocked", attempt.exitReason),
  ];
  return { scenarioId: scenario.id, repetition: attempt.repetition, arm: attempt.arm, valid: failures.length === 0, validity: { gate, failures }, findings };
}

function finding(id, statusOrObserved, evidence) {
  const status = typeof statusOrObserved === "boolean" ? (statusOrObserved ? "observed" : "notObserved") : statusOrObserved;
  return { id, status, evidence };
}

async function rolloutMessages(file) {
  const events = await jsonl(file);
  const messages = [];
  for (const event of events) {
    const item = event.payload;
    if (event.type === "response_item" && item?.role === "user" && userTextItem(event)) messages.push(item.content?.filter(({ type }) => type === "input_text").map(({ text }) => text).join("\n") ?? "");
    if (event.type === "event_msg" && item?.type === "item_completed" && item.item?.type === "UserMessage") messages.push(item.item.content?.map(({ text }) => text ?? "").join("\n") ?? "");
  }
  return { events: events.length, messages: dedupeAdjacent(messages) };
}

function userTextItem(event) {
  const kinds = event.payload?.internal_chat_message_metadata_passthrough?.content_item_kinds;
  return kinds?.includes("user.text") || (!kinds && !event.payload.content?.some(({ text }) => text?.startsWith("<environment_context>")));
}

function dedupeAdjacent(values) {
  return values.filter((value, index) => value !== values[index - 1]);
}

async function inspectFiles(root) {
  const result = {};
  for (const file of REQUIRED) {
    try { result[file] = { exists: true, bytes: (await stat(path.join(root, file))).size }; }
    catch { result[file] = { exists: false, bytes: 0 }; }
  }
  return result;
}

async function jsonl(file) {
  try {
    return (await readFile(file, "utf8")).split(/\r?\n/).filter(Boolean).map((line) => JSON.parse(line));
  } catch { return []; }
}

async function json(file, fallback) {
  try { return JSON.parse(await readFile(file, "utf8")); }
  catch { return fallback; }
}

async function textFiles(root) {
  return (await Promise.all(["transcript.txt", "screen-final.txt"].map(async (file) => {
    try { return await readFile(path.join(root, file), "utf8"); }
    catch { return ""; }
  }))).join("\n");
}

async function textFile(root, file) {
  try { return await readFile(path.join(root, file), "utf8"); }
  catch { return ""; }
}
