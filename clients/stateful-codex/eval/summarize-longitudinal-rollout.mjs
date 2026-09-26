import {
  completedRunResults,
  completionOutput,
  messageText,
  toolInput,
} from "./compare-rollouts.mjs";

const READ_OPERATION_PATTERN = /\b(Get-ChildItem|Get-Content|Select-String|evidence_read|read_file|read_text_file|cat|sed|rg|grep)\b/gi;
const PROJECT_START = "<stateful_project>";
const PROJECT_END = "</stateful_project>";
const PROJECT_UPDATE_START = "<stateful_project_update>";
const PROJECT_UPDATE_END = "</stateful_project_update>";

export function summarizeLongitudinalEvents(events) {
  let metadata = null;
  let activeTurnId = null;
  let latestProjectState = null;
  let unattributedCompactions = 0;
  const turns = new Map();
  const order = [];
  const callsById = new Map();
  const issues = [];
  const ensureTurn = (turnId) => {
    if (!turnId) return null;
    if (!turns.has(turnId)) {
      turns.set(turnId, newTurn(turnId, latestProjectState));
      order.push(turnId);
    }
    return turns.get(turnId);
  };

  events.forEach((event, index) => {
    if (event.type === "session_meta") {
      metadata = event.payload;
      return;
    }
    if (event.type === "world_state") {
      const revision = event.payload?.state?.stateful_project?.rootRevision;
      if (Number.isInteger(revision) && latestProjectState) {
        latestProjectState = { ...latestProjectState, revision };
        const turn = ensureTurn(activeTurnId);
        if (turn) {
          turn.projectStateAtLastResponse = cloneProjectState(latestProjectState);
        }
      }
      return;
    }
    if (event.type === "event_msg" && event.payload?.type === "task_started") {
      if (activeTurnId && !turns.get(activeTurnId)?.complete) {
        issues.push(`turn ${activeTurnId} was superseded before task_complete`);
      }
      activeTurnId = event.payload.turn_id;
      ensureTurn(activeTurnId).started = event.payload;
      return;
    }
    if (event.type === "turn_context") {
      const turn = ensureTurn(event.payload.turn_id ?? activeTurnId);
      if (turn) turn.context = event.payload;
      return;
    }
    if (event.type === "compacted") {
      const turn = ensureTurn(activeTurnId);
      if (!turn) {
        unattributedCompactions += 1;
        return;
      }
      turn.compactions.push({
        line: index + 1,
        windowNumber: event.payload.window_number ?? null,
        responseId: event.payload.compaction_response_id ?? null,
        hasReplacementHistory: event.payload.replacement_history != null,
        threadUsageAtCheckpoint: compactUsage(
          event.payload.latest_token_usage_record?.thread_token_usage,
        ),
      });
      return;
    }
    if (
      event.type === "event_msg" &&
      event.payload?.type === "context_compacted"
    ) {
      const turn = ensureTurn(activeTurnId);
      if (turn) turn.legacyCompactionSignals += 1;
      else unattributedCompactions += 1;
      return;
    }
    if (event.type === "token_usage_record") {
      const turn = ensureTurn(event.payload.turn_id ?? activeTurnId);
      if (!turn) return;
      turn.tokenRecords.push(event.payload);
      turn.lastTurnUsage = compactUsage(event.payload.turn_token_usage);
      turn.threadUsageAtEnd = compactUsage(event.payload.thread_token_usage);
      if (turn.userPrompt) {
        turn.projectStateAtFirstResponse ??=
          cloneProjectState(latestProjectState);
      }
      turn.projectStateAtLastResponse = cloneProjectState(latestProjectState);
      return;
    }
    if (event.type === "response_item") {
      const item = event.payload;
      const turn = ensureTurn(
        item?.internal_chat_message_metadata_passthrough?.turn_id ?? activeTurnId,
      );
      if (item?.type === "message") {
        const text = messageText(item);
        if (item.role === "developer") {
          const fragment = extractProjectFragment(text);
          if (fragment) {
            latestProjectState = summarizeProjectFragment(fragment);
            if (turn) {
              turn.projectStateAtLastResponse = cloneProjectState(latestProjectState);
            }
          } else {
            const revision = extractProjectUpdateRevision(text);
            if (revision != null && latestProjectState) {
              latestProjectState = { ...latestProjectState, revision };
              if (turn) {
                turn.projectStateAtLastResponse = cloneProjectState(latestProjectState);
              }
            }
          }
        } else if (turn && item.role === "user" && text) {
          turn.userPrompt = text;
        } else if (turn && item.role === "assistant" && text) {
          turn.finalAnswer = text;
        }
        return;
      }
      if (item?.type === "function_call" || item?.type === "custom_tool_call") {
        if (!turn) return;
        const call = {
          callId: item.call_id ?? null,
          name: item.name ?? "unknown",
          input: toolInput(item),
        };
        turn.calls.push(call);
        if (call.callId) callsById.set(call.callId, turn);
        const completion = completedRunResults(call);
        turn.completionAttempts += completion.attempts;
        for (const result of completion.results) {
          if (call.callId) turn.completionRequests.set(call.callId, result);
        }
        return;
      }
      if (
        item?.type === "function_call_output" ||
        item?.type === "custom_tool_call_output"
      ) {
        const outputTurn = callsById.get(item.call_id) ?? turn;
        if (!outputTurn) return;
        if (isRejectedToolOutput(callOutputText(item))) {
          outputTurn.rejectedToolResults += 1;
        }
        const submittedResult = outputTurn.completionRequests.get(item.call_id);
        const output = submittedResult ? completionOutput(item) : null;
        if (output?.status === "completed") {
          outputTurn.durableCompletion = {
            runId: output.runId,
            revision: output.revision,
            omittedChecklistItems: output.omittedChecklistItems,
            submittedResult,
          };
        }
      }
      return;
    }
    if (event.type === "event_msg" && event.payload?.type === "task_complete") {
      const turn = ensureTurn(event.payload.turn_id ?? activeTurnId);
      if (!turn) return;
      turn.complete = true;
      turn.completion = event.payload;
      turn.finalAnswer ||= event.payload.last_agent_message ?? "";
      turn.projectStateAtLastResponse ??= cloneProjectState(latestProjectState);
      if (activeTurnId === turn.turnId) activeTurnId = null;
    }
  });

  if (!metadata) throw new Error("rollout is missing session metadata");
  return {
    sessionId: metadata.id ?? metadata.session_id,
    originator: metadata.originator,
    source: metadata.source,
    issues,
    unattributedCompactions,
    turns: order.map((turnId, index) => finalizeTurn(turns.get(turnId), index)),
  };
}

function newTurn(turnId, projectState) {
  return {
    turnId,
    complete: false,
    context: null,
    started: null,
    completion: null,
    userPrompt: "",
    finalAnswer: "",
    tokenRecords: [],
    lastTurnUsage: null,
    threadUsageAtEnd: null,
    calls: [],
    completionAttempts: 0,
    completionRequests: new Map(),
    durableCompletion: null,
    rejectedToolResults: 0,
    compactions: [],
    legacyCompactionSignals: 0,
    projectStateAtStart: cloneProjectState(projectState),
    projectStateAtFirstResponse: null,
    projectStateAtLastResponse: null,
  };
}

function finalizeTurn(turn, index) {
  const usage = turn.tokenRecords.reduce(
    (total, record) => addUsage(total, compactUsage(record.usage)),
    emptyUsage(),
  );
  const measurementIssues = [];
  if (turn.tokenRecords.length === 0) measurementIssues.push("has no token usage records");
  if (turn.lastTurnUsage && !usageEquals(usage, turn.lastTurnUsage)) {
    measurementIssues.push("summed response usage differs from reported turn usage");
  }
  if (!turn.userPrompt) measurementIssues.push("has no user prompt");
  if (!turn.finalAnswer) measurementIssues.push("has no final assistant answer");
  return {
    index: index + 1,
    turnId: turn.turnId,
    complete: turn.complete,
    measurementIssues,
    error: turn.completion?.error ?? null,
    userPrompt: turn.userPrompt,
    finalAnswer: turn.finalAnswer,
    configuration: compactConfiguration(turn.context),
    usage,
    reportedTurnUsage: turn.lastTurnUsage,
    threadUsageAtEnd: turn.threadUsageAtEnd,
    modelResponses: turn.tokenRecords.length,
    latency: {
      startedAt: turn.completion?.started_at ?? turn.started?.started_at ?? null,
      completedAt: turn.completion?.completed_at ?? null,
      durationMs: turn.completion?.duration_ms ?? null,
      timeToFirstTokenMs: turn.completion?.time_to_first_token_ms ?? null,
    },
    calls: summarizeCalls(turn),
    compaction: {
      observed: Math.max(turn.compactions.length, turn.legacyCompactionSignals),
      canonicalEvents: turn.compactions,
      legacySignals: turn.legacyCompactionSignals,
    },
    projectState: {
      atStart: turn.projectStateAtStart,
      atFirstResponse: turn.projectStateAtFirstResponse ?? turn.projectStateAtStart,
      atLastResponse:
        turn.projectStateAtLastResponse ??
        turn.projectStateAtFirstResponse ??
        turn.projectStateAtStart,
    },
    durableCompletion: turn.durableCompletion,
  };
}

function compactConfiguration(context) {
  if (!context) return null;
  return {
    model: context.model,
    reasoningEffort: context.effort,
    approvalPolicy: context.approval_policy,
    sandboxPolicy: context.sandbox_policy?.type,
    permissionProfile: context.permission_profile?.type,
    cwd: context.cwd,
    workspaceRoots: context.workspace_roots,
  };
}

function summarizeCalls(turn) {
  const readOperations = turn.calls.reduce(
    (count, call) => count + (call.input.match(READ_OPERATION_PATTERN)?.length ?? 0),
    0,
  );
  const evidenceReads = turn.calls.flatMap(extractEvidenceReads);
  const uniqueEvidencePaths = [...new Set(evidenceReads.map((read) => read.relativePath))];
  return {
    total: turn.calls.length,
    readBearingOuterCalls: turn.calls.filter((call) =>
      new RegExp(READ_OPERATION_PATTERN.source, "i").test(call.input),
    ).length,
    readOperations,
    evidenceReads,
    uniqueEvidencePaths,
    repeatedEvidenceReads: evidenceReads.length - uniqueEvidencePaths.length,
    blackboardQueries: namedInvocations(turn.calls, "blackboard_query"),
    blackboardEntryScopes: extractBlackboardEntryScopes(turn.calls),
    contextMapQueries: namedInvocations(turn.calls, "context_map_query"),
    obligationUpdates: namedInvocations(turn.calls, "obligation_update"),
    runUpdates: namedInvocations(turn.calls, "stateful_run_update"),
    completionAttempts: turn.completionAttempts,
    rejectedToolResults: turn.rejectedToolResults,
  };
}

function extractBlackboardEntryScopes(calls) {
  const scopes = [];
  for (const call of calls) {
    if (call.name === "blackboard_query") {
      try {
        scopes.push(JSON.parse(call.input).entryScope ?? "active");
      } catch {
        scopes.push("invalid");
      }
    }
    for (const match of call.input.matchAll(/\bblackboard_query\s*\(\s*\{([\s\S]*?)\}\s*\)/g)) {
      scopes.push(stringProperty(match[1], "entryScope") ?? "active");
    }
  }
  return scopes;
}

function namedInvocations(calls, name) {
  const pattern = new RegExp(`\\b${name}\\s*\\(`, "g");
  return calls.reduce(
    (count, call) =>
      count + (call.name === name ? 1 : call.input.match(pattern)?.length ?? 0),
    0,
  );
}

function extractEvidenceReads(call) {
  if (call.name === "evidence_read") {
    try {
      const input = JSON.parse(call.input);
      return [{
        relativePath: input.relativePath ?? null,
        lineStart: input.lineRange?.start ?? input.lineStart ?? null,
        lineEnd: input.lineRange?.end ?? input.lineEnd ?? null,
      }];
    } catch {
      return [];
    }
  }
  const reads = [];
  for (const match of call.input.matchAll(/\bevidence_read\s*\(\s*\{([\s\S]*?)\}\s*\)/g)) {
    const relativePath = stringProperty(match[1], "relativePath");
    if (relativePath) {
      reads.push({
        relativePath,
        lineStart:
          nestedNumberProperty(match[1], "lineRange", "start") ??
          numberProperty(match[1], "lineStart"),
        lineEnd:
          nestedNumberProperty(match[1], "lineRange", "end") ??
          numberProperty(match[1], "lineEnd"),
      });
    }
  }
  return reads;
}

function stringProperty(source, property) {
  return new RegExp(`\\b${property}\\s*:\\s*(["'\`])([^"'\`]+)\\1`).exec(source)?.[2] ?? null;
}

function numberProperty(source, property) {
  const match = new RegExp(`\\b${property}\\s*:\\s*(\\d+)`).exec(source);
  return match ? Number(match[1]) : null;
}

function nestedNumberProperty(source, object, property) {
  const match = new RegExp(
    `\\b${object}\\s*:\\s*\\{[\\s\\S]*?\\b${property}\\s*:\\s*(\\d+)`,
  ).exec(source);
  return match ? Number(match[1]) : null;
}

function extractProjectFragment(text) {
  const start = text.indexOf(PROJECT_START);
  const end = text.indexOf(PROJECT_END, start);
  return start < 0 || end < 0 ? null : text.slice(start, end + PROJECT_END.length);
}

function extractProjectUpdateRevision(text) {
  const start = text.indexOf(PROJECT_UPDATE_START);
  const end = text.indexOf(PROJECT_UPDATE_END, start);
  if (start < 0 || end < 0) return null;
  return numberMatch(
    text.slice(start, end + PROJECT_UPDATE_END.length),
    /Project intelligence revision advanced from \d+ to (\d+)/,
  );
}

function cloneProjectState(state) {
  return state == null ? null : structuredClone(state);
}

function summarizeProjectFragment(fragment) {
  if (!fragment) return null;
  const start = fragment.indexOf("Project intelligence revision:");
  const root = start < 0 ? "" : fragment.slice(start, -PROJECT_END.length);
  return {
    projectContextBytes: Buffer.byteLength(fragment, "utf8"),
    rootBlackboardBytes: Buffer.byteLength(root, "utf8"),
    revision: numberMatch(root, /Project intelligence revision:\s*(\d+)/),
    rootEntries: [...root.matchAll(/^- E\d+\s/gm)].length,
    evidenceRoutes: [...root.matchAll(/^- S\d+=/gm)].length,
    omittedRootEntries:
      numberMatch(root, /^- (\d+) root entries omitted by the context bound;/m) ?? 0,
    entryEvidenceFreshness: freshnessCounts(root, /evidence=(\w+)/g),
    routeFreshness: freshnessCounts(root, /^- S\d+=.*\((\w+)\)$/gm),
  };
}

function freshnessCounts(value, pattern) {
  const counts = { current: 0, stale: 0, sourceUnavailable: 0 };
  for (const match of value.matchAll(pattern)) {
    if (Object.hasOwn(counts, match[1])) counts[match[1]] += 1;
  }
  return counts;
}

function numberMatch(value, pattern) {
  const match = pattern.exec(value);
  return match ? Number(match[1]) : null;
}

function callOutputText(item) {
  return Array.isArray(item.output)
    ? item.output.map((content) => content.text ?? "").join("\n")
    : String(item.output ?? "");
}

function isRejectedToolOutput(output) {
  if (/^Script (failed|timed out)\b/m.test(output) || /^Tool (failed|error)\b/im.test(output)) {
    return true;
  }
  for (const line of output.split(/\r?\n/).reverse()) {
    const candidate = line.trim();
    if (!candidate.startsWith("{") && !candidate.startsWith("[")) continue;
    try {
      const value = JSON.parse(candidate);
      const values = Array.isArray(value) ? value : [value];
      if (values.some((item) => item?.isError === true || item?.ok === false || item?.error)) {
        return true;
      }
    } catch {
      // Continue past non-JSON process output.
    }
  }
  return false;
}

function compactUsage(usage) {
  if (!usage) return null;
  const uncachedInputTokens = Math.max(0, usage.input_tokens - usage.cached_input_tokens);
  return {
    inputTokens: usage.input_tokens,
    cachedInputTokens: usage.cached_input_tokens,
    uncachedInputTokens,
    outputTokens: usage.output_tokens,
    reasoningOutputTokens: usage.reasoning_output_tokens,
    totalTokens: usage.total_tokens,
    uncachedTotalTokens: uncachedInputTokens + usage.output_tokens,
  };
}

export function emptyUsage() {
  return {
    inputTokens: 0,
    cachedInputTokens: 0,
    uncachedInputTokens: 0,
    outputTokens: 0,
    reasoningOutputTokens: 0,
    totalTokens: 0,
    uncachedTotalTokens: 0,
  };
}

export function addUsage(total, usage) {
  if (usage) {
    for (const field of Object.keys(total)) total[field] += usage[field] ?? 0;
  }
  return total;
}

function usageEquals(left, right) {
  return Object.keys(left).every((field) => left[field] === right[field]);
}
