import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const READ_PATTERN = /\b(Get-ChildItem|Get-Content|Select-String|evidence_read|read_file|read_text_file|cat|sed|rg|grep)\b/i;

export function summarizeEvents(events, expectedTerms = []) {
  let metadata = null;
  let turnContext = null;
  let usage = null;
  let finalAnswer = "";
  let userPrompt = "";
  let modelResponses = 0;
  const calls = [];

  for (const event of events) {
    if (event.type === "session_meta") metadata = event.payload;
    if (event.type === "turn_context") turnContext = event.payload;
    if (event.type === "token_usage_record") modelResponses += 1;
    if (event.type === "event_msg" && event.payload?.type === "token_count") {
      usage = event.payload.info?.total_token_usage ?? usage;
    }
    if (event.type !== "response_item") continue;
    const item = event.payload;
    if (item?.type === "message") {
      const text = messageText(item);
      if (item.role === "assistant" && text) finalAnswer = text;
      if (item.role === "user" && text) userPrompt = text;
    }
    if (
      item?.type === "function_call" ||
      item?.type === "custom_tool_call"
    ) {
      calls.push({
        name: item.name ?? "unknown",
        input: toolInput(item),
      });
    }
  }

  if (!metadata) throw new Error("rollout is missing session metadata");
  if (!turnContext) throw new Error("rollout is missing turn context");
  if (!usage) throw new Error("rollout is missing final token usage");
  if (!userPrompt) throw new Error("rollout is missing a user prompt");
  if (!finalAnswer) throw new Error("rollout is missing a final assistant answer");

  const normalizedAnswer = finalAnswer.toLocaleLowerCase();
  const termChecks = expectedTerms.map((term) => ({
    term,
    present: normalizedAnswer.includes(term.toLocaleLowerCase()),
  }));
  const uncachedInputTokens = Math.max(
    0,
    usage.input_tokens - usage.cached_input_tokens,
  );

  return {
    sessionId: metadata.id ?? metadata.session_id,
    originator: metadata.originator,
    source: metadata.source,
    model: turnContext.model,
    reasoningEffort: turnContext.effort,
    approvalPolicy: turnContext.approval_policy,
    sandboxPolicy: turnContext.sandbox_policy?.type,
    permissionProfile: turnContext.permission_profile?.type,
    cwd: turnContext.cwd,
    workspaceRoots: turnContext.workspace_roots,
    userPrompt,
    usage: {
      inputTokens: usage.input_tokens,
      cachedInputTokens: usage.cached_input_tokens,
      uncachedInputTokens,
      outputTokens: usage.output_tokens,
      reasoningOutputTokens: usage.reasoning_output_tokens,
      totalTokens: usage.total_tokens,
      uncachedTotalTokens: uncachedInputTokens + usage.output_tokens,
    },
    modelResponses,
    calls: {
      total: calls.length,
      readBearingToolCalls: calls.filter((call) => READ_PATTERN.test(call.input))
        .length,
      blackboardQueries: namedCalls(calls, "blackboard_query"),
      contextMapQueries: namedCalls(calls, "context_map_query"),
      evidenceReads: namedCalls(calls, "evidence_read"),
      obligationUpdates: namedCalls(calls, "obligation_update"),
      runUpdates: namedCalls(calls, "stateful_run_update"),
    },
    expectations: {
      allPresent: termChecks.every((check) => check.present),
      terms: termChecks,
    },
    finalAnswer,
  };
}

export function compareSummaries(baseline, stateful) {
  const parity = {
    originator: sameDefined(baseline.originator, stateful.originator),
    source: sameDefined(baseline.source, stateful.source),
    model: sameDefined(baseline.model, stateful.model),
    reasoningEffort: sameDefined(
      baseline.reasoningEffort,
      stateful.reasoningEffort,
    ),
    approvalPolicy: sameDefined(
      baseline.approvalPolicy,
      stateful.approvalPolicy,
    ),
    sandboxPolicy: sameDefined(
      baseline.sandboxPolicy,
      stateful.sandboxPolicy,
    ),
    permissionProfile: sameDefined(
      baseline.permissionProfile,
      stateful.permissionProfile,
    ),
    cwd: sameDefined(baseline.cwd, stateful.cwd),
    workspaceRoots:
      JSON.stringify(baseline.workspaceRoots) ===
        JSON.stringify(stateful.workspaceRoots) &&
      baseline.workspaceRoots != null,
    prompt: normalizePrompt(baseline.userPrompt) === normalizePrompt(stateful.userPrompt),
  };
  return {
    parity,
    comparable: Object.values(parity).every(Boolean),
    baseline,
    stateful,
    delta: {
      totalTokens: delta(
        baseline.usage.totalTokens,
        stateful.usage.totalTokens,
      ),
      uncachedTotalTokens: delta(
        baseline.usage.uncachedTotalTokens,
        stateful.usage.uncachedTotalTokens,
      ),
      readBearingToolCalls:
        stateful.calls.readBearingToolCalls -
        baseline.calls.readBearingToolCalls,
      modelResponses: stateful.modelResponses - baseline.modelResponses,
    },
  };
}

async function readEvents(path) {
  const text = await readFile(path, "utf8");
  return text
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`${path}:${index + 1}: ${error.message}`);
      }
    });
}

function messageText(item) {
  return (item.content ?? [])
    .filter((content) => ["input_text", "output_text", "text"].includes(content.type))
    .map((content) => content.text ?? "")
    .join("\n");
}

function toolInput(item) {
  if (typeof item.input === "string") return item.input;
  if (typeof item.arguments === "string") return item.arguments;
  return JSON.stringify(item.input ?? item.arguments ?? {});
}

function namedCalls(calls, name) {
  return calls.filter(
    (call) => call.name === name || call.input.includes(`${name}(`),
  ).length;
}

function normalizePrompt(value) {
  return value.trim().toLocaleLowerCase().replaceAll(/\s+/g, " ");
}

function sameDefined(left, right) {
  return left != null && right != null && left === right;
}

function delta(baseline, stateful) {
  const absolute = stateful - baseline;
  return {
    absolute,
    percent: baseline === 0 ? null : (absolute / baseline) * 100,
  };
}

function parseArgs(args) {
  const parsed = { expectedTerms: [] };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    const value = args[index + 1];
    if (argument === "--baseline") parsed.baseline = value;
    else if (argument === "--stateful") parsed.stateful = value;
    else if (argument === "--expect") parsed.expectedTerms.push(value);
    else throw new Error(`unknown argument: ${argument}`);
    index += 1;
  }
  if (!parsed.baseline || !parsed.stateful) {
    throw new Error("usage: --baseline PATH --stateful PATH [--expect TEXT ...]");
  }
  if (parsed.expectedTerms.some((term) => !term)) {
    throw new Error("--expect requires non-empty text");
  }
  return parsed;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const [baselineEvents, statefulEvents] = await Promise.all([
    readEvents(options.baseline),
    readEvents(options.stateful),
  ]);
  const report = compareSummaries(
    summarizeEvents(baselineEvents, options.expectedTerms),
    summarizeEvents(statefulEvents, options.expectedTerms),
  );
  console.log(JSON.stringify(report, null, 2));
  if (!report.comparable) process.exitCode = 2;
  if (
    !report.baseline.expectations.allPresent ||
    !report.stateful.expectations.allPresent
  ) {
    process.exitCode = 3;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
