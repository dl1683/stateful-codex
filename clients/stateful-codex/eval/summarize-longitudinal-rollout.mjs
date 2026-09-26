import {
  completedRunResults,
  completionOutput,
  messageText,
  toolInput,
} from "./compare-rollouts.mjs";

const READ_OPERATION_PATTERN =
  /\b(Get-ChildItem|Get-Content|Select-String|evidence_read|read_file|read_text_file|cat|sed|rg|grep)\b/gi;
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
          turn.projectStateAtLastResponse =
            cloneProjectState(latestProjectState);
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
        item?.internal_chat_message_metadata_passthrough?.turn_id ??
          activeTurnId,
      );
      if (item?.type === "message") {
        const text = messageText(item);
        if (item.role === "developer") {
          const fragment = extractProjectFragment(text);
          if (fragment) {
            latestProjectState = summarizeProjectFragment(fragment);
            if (turn) {
              turn.projectStateAtLastResponse =
                cloneProjectState(latestProjectState);
            }
          } else {
            const revision = extractProjectUpdateRevision(text);
            if (revision != null && latestProjectState) {
              latestProjectState = { ...latestProjectState, revision };
              if (turn) {
                turn.projectStateAtLastResponse =
                  cloneProjectState(latestProjectState);
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
          output: null,
          rejected: false,
        };
        turn.calls.push(call);
        if (call.callId) callsById.set(call.callId, { turn, call });
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
        const recordedCall = callsById.get(item.call_id);
        const outputTurn = recordedCall?.turn ?? turn;
        if (!outputTurn) return;
        const outputText = callOutputText(item);
        const rejected = isRejectedToolOutput(outputText);
        if (recordedCall) {
          recordedCall.call.output = outputText;
          recordedCall.call.rejected = rejected;
        }
        if (rejected) {
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
  const finalizedTurns = order.map((turnId, index) =>
    finalizeTurn(turns.get(turnId), index),
  );
  addLongitudinalEvidenceMetrics(finalizedTurns);
  return {
    sessionId: metadata.id ?? metadata.session_id,
    originator: metadata.originator,
    source: metadata.source,
    issues,
    unattributedCompactions,
    turns: finalizedTurns,
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
  if (turn.tokenRecords.length === 0)
    measurementIssues.push("has no token usage records");
  if (turn.lastTurnUsage && !usageEquals(usage, turn.lastTurnUsage)) {
    measurementIssues.push(
      "summed response usage differs from reported turn usage",
    );
  }
  if (!turn.userPrompt) measurementIssues.push("has no user prompt");
  if (!turn.finalAnswer)
    measurementIssues.push("has no final assistant answer");
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
      startedAt:
        turn.completion?.started_at ?? turn.started?.started_at ?? null,
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
      atFirstResponse:
        turn.projectStateAtFirstResponse ?? turn.projectStateAtStart,
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
    (count, call) =>
      count + (call.input.match(READ_OPERATION_PATTERN)?.length ?? 0),
    0,
  );
  const evidenceReadObservations = turn.calls.map(
    extractEvidenceReadObservations,
  );
  const evidenceReads = evidenceReadObservations.flatMap(
    (observation) => observation.completed,
  );
  const evidenceReadAttempts = evidenceReadObservations.flatMap(
    (observation) => observation.attempts,
  );
  const uniqueEvidencePaths = [
    ...new Set(evidenceReads.map((read) => read.relativePath).filter(Boolean)),
  ];
  const evidenceReadIdentities = evidenceReads
    .map(evidenceReadIdentity)
    .filter(Boolean);
  return {
    total: turn.calls.length,
    readBearingOuterCalls: turn.calls.filter((call) =>
      new RegExp(READ_OPERATION_PATTERN.source, "i").test(call.input),
    ).length,
    readOperations,
    evidenceReads,
    evidenceReadAttempts,
    uniqueEvidencePaths,
    uniqueEvidenceReadIdentities: [...new Set(evidenceReadIdentities)],
    repeatedEvidenceReads:
      evidenceReadIdentities.length - new Set(evidenceReadIdentities).size,
    unattributedEvidenceReads: evidenceReads.filter(
      (read) => evidenceReadIdentity(read) == null,
    ).length,
    failedEvidenceReadAttempts: evidenceReadAttempts.filter(
      (attempt) => attempt.outcome === "failed",
    ).length,
    unresolvedEvidenceReadAttempts: evidenceReadAttempts.filter(
      (attempt) => attempt.outcome === "unresolved",
    ).length,
    unattributedEvidenceReadAttempts: evidenceReadAttempts.filter(
      (attempt) => evidenceReadIdentity(attempt) == null,
    ).length,
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
    for (const match of call.input.matchAll(
      /\bblackboard_query\s*\(\s*\{([\s\S]*?)\}\s*\)/g,
    )) {
      scopes.push(stringProperty(match[1], "entryScope") ?? "active");
    }
  }
  return scopes;
}

function namedInvocations(calls, name) {
  const pattern = new RegExp(`\\b${name}\\s*\\(`, "g");
  return calls.reduce(
    (count, call) =>
      count +
      (call.name === name ? 1 : (call.input.match(pattern)?.length ?? 0)),
    0,
  );
}

function extractEvidenceReadObservations(call) {
  const requests = extractEvidenceReadRequests(call);
  if (requests.length === 0) return { completed: [], attempts: [] };
  const resolvedReads = extractResolvedEvidenceReads(call.output);
  const unmatchedResolvedReads = [...resolvedReads];
  const unmatchedRequests = [];
  for (const request of requests) {
    const exactMatchIndex = unmatchedResolvedReads.findIndex(
      (resolved) =>
        evidenceReadMatches(request, resolved) &&
        request.lineStart === resolved.lineStart &&
        request.lineEnd === resolved.lineEnd,
    );
    const matchIndex =
      exactMatchIndex >= 0
        ? exactMatchIndex
        : unmatchedResolvedReads.findIndex((resolved) =>
            evidenceReadMatches(request, resolved),
          );
    if (matchIndex >= 0) {
      unmatchedResolvedReads.splice(matchIndex, 1);
    } else {
      unmatchedRequests.push(request);
    }
  }
  while (unmatchedRequests.length > 0 && unmatchedResolvedReads.length > 0) {
    const unattributedIndex = unmatchedRequests.findIndex(
      (request) => evidenceReadIdentity(request) == null,
    );
    if (unattributedIndex < 0) break;
    unmatchedRequests.splice(unattributedIndex, 1);
    unmatchedResolvedReads.shift();
  }
  const unresolvedAttempts = unmatchedRequests.map((request) => ({
    ...request,
    outcome: call.rejected ? "failed" : "unresolved",
  }));
  return {
    completed: resolvedReads,
    attempts: [
      ...resolvedReads.map((read) => ({ ...read, outcome: "completed" })),
      ...unresolvedAttempts,
    ],
  };
}

function extractEvidenceReadRequests(call) {
  if (call.name === "evidence_read") {
    try {
      const input = JSON.parse(call.input);
      return [evidenceReadFromInput(input)];
    } catch {
      return [unattributedEvidenceRead()];
    }
  }
  const reads = [];
  for (const match of call.input.matchAll(
    /\bevidence_read\s*\(\s*\{([\s\S]*?)\}\s*\)/g,
  )) {
    reads.push({
      contextMapEntryId: stringProperty(match[1], "contextMapEntryId"),
      sourceFingerprint: stringProperty(match[1], "sourceFingerprint"),
      projectRoot: stringProperty(match[1], "projectRoot"),
      relativePath: stringProperty(match[1], "relativePath"),
      lineStart:
        nestedNumberProperty(match[1], "lineRange", "start") ??
        numberProperty(match[1], "lineStart"),
      lineEnd:
        nestedNumberProperty(match[1], "lineRange", "end") ??
        numberProperty(match[1], "lineEnd"),
      attribution: "request",
    });
  }
  return reads;
}

function evidenceReadFromInput(input) {
  return {
    contextMapEntryId: input.evidenceRoute?.contextMapEntryId ?? null,
    sourceFingerprint: input.evidenceRoute?.sourceFingerprint ?? null,
    projectRoot: input.projectRoot ?? null,
    relativePath: input.relativePath ?? null,
    lineStart:
      input.evidenceRoute?.lineRange?.start ??
      input.lineRange?.start ??
      input.lineStart ??
      null,
    lineEnd:
      input.evidenceRoute?.lineRange?.end ??
      input.lineRange?.end ??
      input.lineEnd ??
      null,
    attribution: "request",
  };
}

function extractResolvedEvidenceReads(output) {
  if (!output) return [];
  const reads = [];
  for (const line of output.split(/\r?\n/)) {
    const candidate = line.trim();
    if (!candidate.startsWith("{") || !candidate.endsWith("}")) continue;
    try {
      const value = JSON.parse(candidate);
      if (!value.contextMapEntryId || !value.sourceFingerprint || !value.source)
        continue;
      reads.push({
        contextMapEntryId: value.contextMapEntryId,
        sourceFingerprint: value.sourceFingerprint,
        projectRoot: value.source.projectRoot ?? null,
        relativePath: value.source.relativePath ?? null,
        lineStart: value.firstLine ?? null,
        lineEnd: value.lastLine ?? null,
        truncated: value.truncated ?? null,
        attribution: "resolvedOutput",
      });
    } catch {
      // Tool output may contain ordinary process text or truncated JSON.
    }
  }
  return reads;
}

function evidenceReadMatches(request, resolved) {
  if (
    request.contextMapEntryId &&
    request.contextMapEntryId !== resolved.contextMapEntryId
  ) {
    return false;
  }
  if (
    request.sourceFingerprint &&
    request.sourceFingerprint !== resolved.sourceFingerprint
  ) {
    return false;
  }
  if (request.relativePath && request.relativePath !== resolved.relativePath)
    return false;
  if (request.projectRoot && request.projectRoot !== resolved.projectRoot)
    return false;
  if (
    request.lineStart != null &&
    (resolved.lineStart == null || resolved.lineStart < request.lineStart)
  )
    return false;
  if (
    request.lineEnd != null &&
    (resolved.lineEnd == null || resolved.lineEnd > request.lineEnd)
  )
    return false;
  return evidenceReadIdentity(request) != null;
}

function addLongitudinalEvidenceMetrics(turns) {
  const priorIdentities = new Set();
  for (const turn of turns) {
    const currentIdentities = new Set();
    let repeatedFromPriorTurns = 0;
    let repeatedWithinTurn = 0;
    let newEvidenceReads = 0;
    for (const read of turn.calls.evidenceReads) {
      const identity = evidenceReadIdentity(read);
      if (!identity) continue;
      if (priorIdentities.has(identity)) {
        repeatedFromPriorTurns += 1;
      } else if (currentIdentities.has(identity)) {
        repeatedWithinTurn += 1;
      } else {
        newEvidenceReads += 1;
      }
      currentIdentities.add(identity);
    }
    turn.calls.repeatedEvidenceReadsFromPriorTurns = repeatedFromPriorTurns;
    turn.calls.repeatedEvidenceReadsWithinTurn = repeatedWithinTurn;
    turn.calls.repeatedEvidenceReads =
      repeatedFromPriorTurns + repeatedWithinTurn;
    turn.calls.newEvidenceReads = newEvidenceReads;
    for (const identity of currentIdentities) priorIdentities.add(identity);
  }
}

function evidenceReadIdentity(read) {
  const range = `${read.lineStart ?? "*"}-${read.lineEnd ?? "*"}`;
  if (read.projectRoot && read.relativePath) {
    return `source:${read.projectRoot}::${read.relativePath}@${read.sourceFingerprint ?? "*"}:${range}`;
  }
  if (read.relativePath) {
    return `path:${read.relativePath}@${read.sourceFingerprint ?? "*"}:${range}`;
  }
  if (read.contextMapEntryId) {
    return `route:${read.contextMapEntryId}@${read.sourceFingerprint ?? "*"}:${range}`;
  }
  return null;
}

function unattributedEvidenceRead() {
  return {
    contextMapEntryId: null,
    sourceFingerprint: null,
    projectRoot: null,
    relativePath: null,
    lineStart: null,
    lineEnd: null,
    truncated: null,
    attribution: "unattributed",
  };
}

function stringProperty(source, property) {
  return (
    new RegExp(`\\b${property}\\s*:\\s*(["'\`])([^"'\`]+)\\1`).exec(
      source,
    )?.[2] ?? null
  );
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
  return start < 0 || end < 0
    ? null
    : text.slice(start, end + PROJECT_END.length);
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
      numberMatch(
        root,
        /^- (\d+) root entries omitted by the context bound;/m,
      ) ?? 0,
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
  if (
    /^Script (failed|timed out)\b/m.test(output) ||
    /^Tool (failed|error)\b/im.test(output)
  ) {
    return true;
  }
  for (const line of output.split(/\r?\n/).reverse()) {
    const candidate = line.trim();
    if (!candidate.startsWith("{") && !candidate.startsWith("[")) continue;
    try {
      const value = JSON.parse(candidate);
      const values = Array.isArray(value) ? value : [value];
      if (
        values.some(
          (item) => item?.isError === true || item?.ok === false || item?.error,
        )
      ) {
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
  const uncachedInputTokens = Math.max(
    0,
    usage.input_tokens - usage.cached_input_tokens,
  );
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
