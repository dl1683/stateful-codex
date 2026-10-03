// What the agent is doing right now, kept apart from the durable run. The page starts not
// knowing ("checking"), and only an authoritative read (thread/read and the newest turn) or a
// live turn event may say Working, Answer finished or Not working. A read that began before a
// newer live event is discarded, so an older snapshot never overwrites what was seen after it.
// A dropped live connection makes the state unknown again until the next read.

export function createExecution() {
  return { phase: "checking", events: 0, reads: 0, appliedRead: 0 };
}

// Phases: checking (nothing authoritative yet), unknown (the reads could not decide), working,
// waiting (a live turn waits on the person), finished, stopped, failed, idle, error.

// A live turn event. Returns true when it changed the phase.
export function noteTurnEvent(execution, method, params) {
  if (method === "turn/started") {
    execution.events += 1;
    return setPhase(execution, "working");
  }
  if (method === "turn/completed") {
    execution.events += 1;
    return setPhase(execution, terminalPhase(params?.turn?.status ?? "completed"));
  }
  return false;
}

// The live connection dropped: events may have been missed.
export function noteConnectionLost(execution) {
  execution.events += 1;
  return setPhase(execution, "checking");
}

// Starts an authoritative read; pass the token to applySnapshot when it returns.
export function beginSnapshot(execution) {
  execution.reads += 1;
  return { events: execution.events, read: execution.reads };
}

// Applies a read unless a live event arrived after it began, or a newer read already applied.
// `thread` is thread/read's thread (or null when the read failed); `latestTurn` is the newest
// turn (or null when there is none, undefined when it could not be read).
export function applySnapshot(execution, token, { thread, latestTurn }) {
  if (token.events !== execution.events || token.read < execution.appliedRead) return false;
  execution.appliedRead = token.read;
  return setPhase(execution, snapshotPhase(thread, latestTurn));
}

export function snapshotPhase(thread, latestTurn) {
  const status = thread?.status;
  if (!status) return "unknown";
  if (status.type === "active") {
    const flags = status.activeFlags ?? [];
    return flags.includes("waitingOnApproval") || flags.includes("waitingOnUserInput")
      ? "waiting"
      : "working";
  }
  if (status.type === "systemError") return "error";
  if (latestTurn === undefined) return "unknown";
  if (latestTurn === null) return "idle";
  // A turn recorded as in progress on a thread no live process holds may still be running
  // elsewhere, or may have been cut off: say so rather than guess.
  if (latestTurn.status === "inProgress") return "unknown";
  return terminalPhase(latestTurn.status);
}

function terminalPhase(status) {
  if (status === "completed") return "finished";
  if (status === "interrupted") return "stopped";
  if (status === "failed") return "failed";
  return "unknown";
}

function setPhase(execution, phase) {
  if (execution.phase === phase) return false;
  execution.phase = phase;
  return true;
}

const PHASE_LABELS = {
  checking: "Checking what is running…",
  unknown: "Not sure whether work is running",
  working: "Working",
  waiting: "Waiting for you",
  finished: "Answer finished",
  stopped: "Stopped",
  failed: "The last turn failed",
  idle: "Not working right now",
  error: "The session hit an error",
};

// The current execution in plain words; a waiting approval comes first.
export function executionLabel(execution, pendingCount = 0) {
  if (pendingCount > 0) return PHASE_LABELS.waiting;
  return PHASE_LABELS[execution?.phase ?? "checking"] ?? PHASE_LABELS.unknown;
}

// True only when a live turn is known to be running.
export function isWorking(execution) {
  return execution?.phase === "working" || execution?.phase === "waiting";
}
