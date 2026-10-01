// What the latest answer on a thread rested on. The per-turn Stateful measurement says whether
// saved project memory was in context and whether that context's source links had changed, and
// counts exact source reads; the turn's recorded items show commands, patches and other tools.
// Only observed mechanisms are reported; nothing here proves where an answer's content came from.

const MEASUREMENT_PAGE = 50;
const MAX_MEASUREMENT_PAGES = 3;

export function normalizeThreadItem(entry) {
  return entry?.item ? { ...entry.item, turnId: entry.turnId } : entry;
}

// The turn of the newest recorded agent message, if no newer turn has started since.
export function latestAnswerTurn(state) {
  const items = (state.activity ?? []).map(normalizeThreadItem);
  const answerTurn = items.findLast((item) => item?.type === "agentMessage")?.turnId ?? null;
  const newestTurn = items.at(-1)?.turnId ?? null;
  if (!answerTurn || (newestTurn && newestTurn !== answerTurn)) return null;
  if (state.liveTurnId && state.liveTurnId !== answerTurn) return null;
  return answerTurn;
}

// Finds the measurement for one turn of this thread, paging back through the project's
// measurements (newest first) a bounded distance. A found measurement is kept: it is final.
export async function findTurnMeasurement(rpc, { projectId, threadId, turnId, known }) {
  if (!turnId) return null;
  if (known?.threadId === threadId && known.turnId === turnId) return known;
  let cursor = null;
  for (let page = 0; page < MAX_MEASUREMENT_PAGES; page += 1) {
    let response;
    try {
      response = await rpc("statefulMeasurement/list", {
        projectId,
        cursor,
        limit: MEASUREMENT_PAGE,
      });
    } catch (error) {
      if (error.code === -32601) return null;
      throw error;
    }
    const match = response.data.find(
      (measurement) => measurement.threadId === threadId && measurement.turnId === turnId,
    );
    if (match) return match;
    cursor = response.nextCursor;
    if (!cursor) return null;
  }
  return null;
}

// The facts for the trust line, or null when there is no measured latest answer to describe.
export function describeAnswer(state) {
  const answerTurn = latestAnswerTurn(state);
  const measurement = state.answerMeasurement;
  if (
    !answerTurn ||
    measurement?.turnId !== answerTurn ||
    measurement.threadId !== state.threadId
  ) {
    return null;
  }
  const items = state.activity.map(normalizeThreadItem);
  const turnItems = items.filter((item) => item.turnId === answerTurn);
  const counters = measurement.counters ?? {};
  const ran = (item) => ["completed", "failed"].includes(item.status);
  return {
    status: measurement.status,
    // The item page is bounded; if it starts inside this turn, earlier items may be missing.
    partial: Boolean(state.activityTruncated && items[0]?.turnId === answerTurn),
    usedMemory: (counters.rootEntriesLoaded ?? 0) > 0,
    changedSources: (counters.rootEvidenceRoutesStale ?? 0) > 0,
    missingSources: (counters.rootEvidenceRoutesUnavailable ?? 0) > 0,
    sourceReads: counters.evidenceReadCalls ?? 0,
    commands: turnItems.filter((item) => item.type === "commandExecution" && ran(item)).length,
    declined: turnItems.filter((item) => item.status === "declined").length,
    patches: turnItems.filter((item) => item.type === "fileChange" && item.status === "completed")
      .length,
    otherTools: turnItems.filter((item) =>
      ["mcpToolCall", "dynamicToolCall", "webSearch", "collabAgentToolCall"].includes(item.type),
    ).length,
  };
}
