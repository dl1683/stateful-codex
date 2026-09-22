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
  let durableCompletion = null;
  let completionAttempts = 0;
  const completionRequests = new Map();
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
      item?.type === "custom_tool_call_output" ||
      item?.type === "function_call_output"
    ) {
      const submittedResult = completionRequests.get(item.call_id);
      const output = submittedResult ? completionOutput(item) : null;
      if (output?.status === "completed") {
        const checklist = output.finalAnswerChecklist
          .map((entry) => entry.text)
          .filter((text) => typeof text === "string");
        durableCompletion = {
          runId: output.runId,
          revision: output.revision,
          omittedChecklistItems: output.omittedChecklistItems,
          submittedResult,
          checklist,
          coverageText: [submittedResult, ...checklist].join("\n"),
        };
      }
    }
    if (
      item?.type === "function_call" ||
      item?.type === "custom_tool_call"
    ) {
      const call = {
        name: item.name ?? "unknown",
        input: toolInput(item),
      };
      calls.push(call);
      const completedResults = completedRunResults(call);
      completionAttempts += completedResults.attempts;
      const submittedResult = completedResults.results.at(-1);
      if (submittedResult != null) {
        completionRequests.set(item.call_id, submittedResult);
      }
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
    durableCompletion,
    calls: {
      total: calls.length,
      readBearingToolCalls: calls.filter((call) => READ_PATTERN.test(call.input))
        .length,
      blackboardQueries: namedCalls(calls, "blackboard_query"),
      contextMapQueries: namedCalls(calls, "context_map_query"),
      evidenceReads: namedCalls(calls, "evidence_read"),
      obligationUpdates: namedCalls(calls, "obligation_update"),
      runUpdates: namedCalls(calls, "stateful_run_update"),
      completionAttempts,
    },
    expectations: {
      allPresent: termChecks.every((check) => check.present),
      terms: termChecks,
    },
    finalAnswer,
  };
}

function completionOutput(item) {
  const output = Array.isArray(item.output)
    ? item.output.map((content) => content.text ?? "").join("\n")
    : String(item.output ?? "");
  for (const line of output.split(/\r?\n/).reverse()) {
    const candidate = line.trim();
    if (!candidate.startsWith("{")) continue;
    try {
      const parsed = JSON.parse(candidate);
      if (Array.isArray(parsed.finalAnswerChecklist)) return parsed;
      const nested = Object.values(parsed).find((value) =>
        Array.isArray(value?.finalAnswerChecklist),
      );
      if (nested) return nested;
    } catch {
      // Continue past non-JSON process output.
    }
  }
  return null;
}

function completedRunResults(call) {
  if (
    call.name !== "stateful_run_update" &&
    !call.input.includes("stateful_run_update")
  ) {
    return { attempts: 0, results: [] };
  }

  if (call.name === "stateful_run_update") {
    try {
      const input = JSON.parse(call.input);
      return {
        attempts: input.status === "completed" ? 1 : 0,
        results:
          input.status === "completed" && typeof input.result === "string"
            ? [input.result]
            : [],
      };
    } catch {
      // Fall through to the code-mode representation.
    }
  }

  const starts = [...call.input.matchAll(/stateful_run_update\s*\(/g)].map(
    (match) => match.index,
  );
  const results = [];
  let attempts = 0;
  for (const [index, start] of starts.entries()) {
    const end = starts[index + 1] ?? call.input.length;
    const source = call.input.slice(start, end);
    if (!/status\s*:\s*["']completed["']/.test(source)) continue;
    attempts += 1;
    const result = jsStringProperty(source, "result");
    if (result != null) results.push(result);
  }
  return { attempts, results };
}

function jsStringProperty(source, property) {
  const match = new RegExp("\\b" + property + "\\s*:\\s*([\"'`])").exec(
    source,
  );
  if (!match) return null;
  const quote = match[1];
  let value = "";
  for (let index = match.index + match[0].length; index < source.length; index += 1) {
    const character = source[index];
    if (character === quote) return value;
    if (character !== "\\") {
      value += character;
      continue;
    }
    index += 1;
    if (index >= source.length) return null;
    const escaped = source[index];
    value +=
      escaped === "n"
        ? "\n"
        : escaped === "r"
          ? "\r"
          : escaped === "t"
            ? "\t"
            : escaped;
  }
  return null;
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

export async function readEvents(path) {
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

export function normalizePrompt(value) {
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

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
