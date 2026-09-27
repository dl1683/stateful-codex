// based on event types from codex-rs/exec/src/exec_events.rs

import type { ThreadItem } from "./items";

/** Emitted when a new thread is started as the first event. */
export type ThreadStartedEvent = {
  type: "thread.started";
  /** The identifier of the new thread. Can be used to resume the thread later. */
  thread_id: string;
};

/**
 * Emitted when a turn is started by sending a new prompt to the model.
 * A turn encompasses all events that happen while the agent is processing the prompt.
 */
export type TurnStartedEvent = {
  type: "turn.started";
};

/** Describes the usage of tokens during a turn. */
export type Usage = {
  /** The number of input tokens used during the turn. */
  input_tokens: number;
  /** The number of cached input tokens used during the turn. */
  cached_input_tokens: number;
  /** The number of input tokens written to the prompt cache during the turn. */
  cache_write_input_tokens: number;
  /** The number of output tokens used during the turn. */
  output_tokens: number;
  /** The number of reasoning output tokens used during the turn. */
  reasoning_output_tokens: number;
};

/** Cumulative, content-free trajectory observed by one headless invocation. */
export type RunTrajectory = {
  invocation_duration_ms: number;
  completed_model_responses: number;
  compactions: number;
  model_tool_calls: number;
  model_shell_tool_calls: number;
  model_function_tool_calls: number;
  model_custom_tool_calls: number;
  model_tool_search_calls: number;
  model_web_search_calls: number;
  model_image_generation_calls: number;
  tool_output_bytes: number;
};

/** Cumulative Stateful contribution observed by one headless invocation. */
export type StatefulAttribution = {
  turns: number;
  completed_turns: number;
  failed_turns: number;
  aborted_turns: number;
  duration_ms: number;
  world_state_samples: number;
  root_entries_loaded: number;
  root_evidence_routes_checked: number;
  root_evidence_routes_current: number;
  root_evidence_routes_stale: number;
  root_evidence_routes_unavailable: number;
  root_evidence_routes_unchecked: number;
  root_unique_sources_observed: number;
  root_source_bytes_hashed: number;
  stateful_tool_calls: number;
  failed_stateful_tool_calls: number;
  knowledge_query_calls: number;
  route_query_calls: number;
  evidence_read_calls: number;
  steering_query_calls: number;
  blackboard_write_calls: number;
  context_refresh_calls: number;
  obligation_write_calls: number;
  run_update_calls: number;
  steering_write_calls: number;
  material_findings_reused: number;
  invocation_duration_ms: number;
  completed_model_responses: number;
  compactions: number;
  model_tool_calls: number;
  model_shell_tool_calls: number;
  model_function_tool_calls: number;
  model_custom_tool_calls: number;
  model_tool_search_calls: number;
  model_web_search_calls: number;
  model_image_generation_calls: number;
  tool_output_bytes: number;
};

/** Emits a cumulative in-flight resource snapshot. */
export type TurnProgressEvent = {
  type: "turn.progress";
  usage: Usage;
  trajectory: RunTrajectory;
  /** @deprecated Use trajectory.invocation_duration_ms. */
  elapsed_ms: number;
  /** @deprecated Use trajectory.completed_model_responses. */
  completed_model_responses: number;
  /** @deprecated Use trajectory.compactions. */
  compactions: number;
  /** @deprecated Use trajectory.model_tool_calls. */
  model_tool_calls: number;
  /** @deprecated Use trajectory.tool_output_bytes. */
  tool_output_bytes: number;
};

/** Emitted when a turn is completed. Typically right after the assistant's response. */
export type TurnCompletedEvent = {
  type: "turn.completed";
  usage: Usage;
  trajectory: RunTrajectory;
  stateful_attribution?: StatefulAttribution;
};

/** Indicates that a turn failed with an error. */
export type TurnFailedEvent = {
  type: "turn.failed";
  error: ThreadError;
  usage: Usage;
  trajectory: RunTrajectory;
  stateful_attribution?: StatefulAttribution;
};

/** Emits cumulative Stateful contribution after a Stateful turn stops. */
export type StatefulAttributionEvent = {
  type: "stateful.attribution";
  turn_status: "completed" | "failed" | "aborted";
  attribution: StatefulAttribution;
};

/** Emitted when a new item is added to the thread. Typically the item is initially "in progress". */
export type ItemStartedEvent = {
  type: "item.started";
  item: ThreadItem;
};

/** Emitted when an item is updated. */
export type ItemUpdatedEvent = {
  type: "item.updated";
  item: ThreadItem;
};

/** Signals that an item has reached a terminal state—either success or failure. */
export type ItemCompletedEvent = {
  type: "item.completed";
  item: ThreadItem;
};

/** Fatal error emitted by the stream. */
export type ThreadError = {
  message: string;
};

/** Represents an unrecoverable error emitted directly by the event stream. */
export type ThreadErrorEvent = {
  type: "error";
  message: string;
};

/** Top-level JSONL events emitted by codex exec. */
export type ThreadEvent =
  | ThreadStartedEvent
  | TurnStartedEvent
  | TurnProgressEvent
  | TurnCompletedEvent
  | TurnFailedEvent
  | ItemStartedEvent
  | ItemUpdatedEvent
  | ItemCompletedEvent
  | StatefulAttributionEvent
  | ThreadErrorEvent;
