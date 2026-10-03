// The authoritative reads behind the header badge, the memory status line and the return card.
// Each failure is reported as "unknown" or unavailable rather than failing the workspace refresh.

// thread/read gives the live thread status; the newest turn says how the last work ended.
// `latestTurn` is undefined when the turn list could not be read, null when there is none.
export async function readExecutionSnapshot(rpc, threadId) {
  const [thread, latestTurn] = await Promise.all([
    rpc("thread/read", { threadId })
      .then((response) => response.thread ?? null)
      .catch(() => null),
    rpc("thread/turns/list", { threadId, limit: 1, sortDirection: "desc" })
      .then((response) => response.data?.[0] ?? null)
      .catch(() => undefined),
  ]);
  return { thread, latestTurn };
}

const watermarkKey = (threadId) => `stateful-memory-since:${threadId}`;

// The project's memory counts, plus what changed since this page session began. The session
// start watermark is the journal sequence at the first read, kept across reloads of this tab.
export async function readMemorySummary(rpc, { threadId, storage }) {
  const stored = readWatermark(storage, threadId);
  try {
    const summary = await rpc("statefulMemory/summary", {
      threadId,
      sinceSequence: stored,
      threadIds: [threadId],
    });
    if (stored === null) {
      try {
        storage?.setItem(watermarkKey(threadId), String(summary.latestSequence ?? 0));
      } catch {
        // Without storage the session totals restart on reload; the counts stay exact.
      }
    }
    return summary;
  } catch (error) {
    return { error: error.message };
  }
}

function readWatermark(storage, threadId) {
  try {
    const value = storage?.getItem(watermarkKey(threadId));
    const parsed = value === null || value === undefined ? NaN : Number(value);
    return Number.isInteger(parsed) && parsed >= 0 ? parsed : null;
  } catch {
    return null;
  }
}

export async function readRecap(rpc, threadId) {
  try {
    return await rpc("statefulMemory/recap", { threadId });
  } catch (error) {
    return { error: error.message };
  }
}
