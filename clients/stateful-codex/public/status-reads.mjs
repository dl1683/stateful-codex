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
const lateKey = (threadId) => `stateful-memory-started-late:${threadId}`;

// Reads the project's memory counts plus what changed since this page session began. The
// session starts at the journal sequence of the first successful read, kept in memory for this
// page and in storage (when available) across reloads of the tab. Read it once before the
// opening turn is sent, so that turn's saves count as this session's.
// Reads run one at a time, so a later read always answers from a newer journal position than
// an earlier one. If the session could not establish its start before work began, its totals
// are marked as starting late instead of silently omitting what was saved meanwhile.
export function createSummaryReader(rpc, { threadId, storage }) {
  let watermark = readWatermark(storage, threadId);
  // Restored with the watermark: a session that started late stays qualified across reloads.
  let startedLate = watermark !== null && readFlag(storage, lateKey(threadId));
  let queue = Promise.resolve();
  const read = async () => {
    try {
      const summary = await rpc("statefulMemory/summary", {
        threadId,
        sinceSequence: watermark,
        threadIds: [threadId],
      });
      if (watermark === null && Number.isInteger(summary.latestSequence)) {
        watermark = summary.latestSequence;
        try {
          if (startedLate) storage?.setItem(lateKey(threadId), "1");
          else storage?.removeItem?.(lateKey(threadId));
          storage?.setItem(watermarkKey(threadId), String(watermark));
        } catch {
          // Without storage the session totals restart on reload; this page keeps them.
        }
        // The first read only sets where the session starts: nothing changed in it yet.
        return { ...summary, since: summary.since ?? emptyTotals(), sessionStartedLate: startedLate };
      }
      return { ...summary, sessionStartedLate: startedLate };
    } catch (error) {
      if (watermark === null) startedLate = true;
      return { error: error.message };
    }
  };
  return function readMemorySummary() {
    const result = queue.then(read);
    queue = result;
    return result;
  };
}

// The summary to show: the newest by journal position; an error replaces nothing it outdates.
export function newerSummary(current, next) {
  if (!current || current.error) return next;
  if (next.error) return next;
  return (next.latestSequence ?? 0) >= (current.latestSequence ?? 0) ? next : current;
}

function emptyTotals() {
  return {
    saved: 0,
    commitsRemembered: 0,
    promoted: 0,
    corrected: 0,
    forgotten: 0,
    invalidated: 0,
    scopesEnded: 0,
    captureIncomplete: 0,
  };
}

function readFlag(storage, key) {
  try {
    return storage?.getItem(key) === "1";
  } catch {
    return false;
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
