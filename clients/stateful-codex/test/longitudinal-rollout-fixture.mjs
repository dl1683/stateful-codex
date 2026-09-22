export const projectFragment = [
  "<stateful_project>",
  "Project intelligence revision: 7",
  "Root blackboard (active, explicitly promoted knowledge):",
  "Exact-source aliases:",
  "- S1=R1::decision.md (current)",
  "- E1 [high fact; verification=sourceVerified; declared=sourceVerified; evidence=current; confidence=9900; provenance=agent] content=Executed amendment controls sources=[S1:L2-L4] relations=[]",
  "- 2 root entries omitted by the context bound; query the blackboard for them.",
  "</stateful_project>",
].join("\n");

export function metadata(id) {
  return {
    type: "session_meta",
    payload: { id, originator: "codex-exec", source: "exec" },
  };
}

export function started(turnId, startedAt) {
  return {
    type: "event_msg",
    payload: {
      type: "task_started",
      turn_id: turnId,
      started_at: startedAt,
      model_context_window: 200000,
    },
  };
}

export function context(turnId) {
  return {
    type: "turn_context",
    payload: {
      turn_id: turnId,
      model: "gpt-test",
      effort: "high",
      approval_policy: "never",
      sandbox_policy: { type: "danger-full-access" },
      permission_profile: { type: "disabled" },
      cwd: "C:/project",
      workspace_roots: ["C:/project"],
    },
  };
}

export function message(turnId, role, text) {
  return {
    type: "response_item",
    payload: {
      type: "message",
      role,
      content: [
        { type: role === "assistant" ? "output_text" : "input_text", text },
      ],
      internal_chat_message_metadata_passthrough: { turn_id: turnId },
    },
  };
}

export function usage(input, cached, output, reasoning = 0) {
  return {
    input_tokens: input,
    cached_input_tokens: cached,
    output_tokens: output,
    reasoning_output_tokens: reasoning,
    total_tokens: input + output,
  };
}

export function usageRecord(turnId, responseId, response, turnTotal, threadTotal) {
  return {
    type: "token_usage_record",
    payload: {
      turn_id: turnId,
      response_id: responseId,
      usage: response,
      turn_token_usage: turnTotal,
      thread_token_usage: threadTotal,
    },
  };
}

export function completed(turnId, startedAt, durationMs) {
  return {
    type: "event_msg",
    payload: {
      type: "task_complete",
      turn_id: turnId,
      started_at: startedAt,
      completed_at: startedAt + Math.ceil(durationMs / 1000),
      duration_ms: durationMs,
      time_to_first_token_ms: 250,
    },
  };
}

export function toolCall(turnId, callId, input) {
  return {
    type: "response_item",
    payload: {
      type: "custom_tool_call",
      call_id: callId,
      name: "exec",
      input,
      internal_chat_message_metadata_passthrough: { turn_id: turnId },
    },
  };
}

export function toolOutput(callId, output) {
  return {
    type: "response_item",
    payload: { type: "custom_tool_call_output", call_id: callId, output },
  };
}

export function simpleRollout(id, arm, turnUsages) {
  const events = [metadata(id)];
  let threadInput = 0;
  let threadCached = 0;
  let threadOutput = 0;
  turnUsages.forEach((turnUsage, index) => {
    const turnId = `${arm}-turn-${index + 1}`;
    threadInput += turnUsage.input_tokens;
    threadCached += turnUsage.cached_input_tokens;
    threadOutput += turnUsage.output_tokens;
    events.push(
      started(turnId, 100 + index * 10),
      context(turnId),
      message(turnId, "user", `Question ${index + 1}?`),
      toolCall(
        turnId,
        `${arm}-call-${index + 1}`,
        arm === "stateful"
          ? `text(await tools.evidence_read({relativePath: 'source-${index + 1}.md'}));`
          : `text(await tools.exec_command({cmd: 'Get-Content source-${index + 1}.md'}));`,
      ),
      toolOutput(`${arm}-call-${index + 1}`, "Script completed"),
      usageRecord(
        turnId,
        `${arm}-response-${index + 1}`,
        turnUsage,
        turnUsage,
        usage(threadInput, threadCached, threadOutput),
      ),
      message(
        turnId,
        "assistant",
        index === 0 ? "The executed amendment controls." : "The cap is six percent.",
      ),
      completed(turnId, 100 + index * 10, 1000 + index * 100),
    );
  });
  return events;
}
