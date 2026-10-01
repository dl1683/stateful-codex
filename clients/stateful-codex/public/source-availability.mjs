// Whether the project's folders exist on disk right now, and how source-access failures are
// explained to a person. The index keeps describing sources after their folder disappears, so
// the client checks each root itself (fs/getMetadata) instead of trusting stored freshness.

// Missing-path failures as the operating system reports them (Windows os error 2 and 3,
// POSIX ENOENT and ENOTDIR). Anything else (access denied, an unreachable share) is not
// evidence that a folder is gone.
const MISSING_PATH =
  /os error (2|3|20)\b|cannot find the (path|file)|no such file or directory|not a directory/i;
// A probe that does not answer in time leaves the root's availability unknown.
const PROBE_TIMEOUT_MS = 3_000;

// Paths from different responses may differ in separators, a trailing separator, or (on
// Windows, where paths are case-insensitive) letter case. POSIX paths keep their case.
export function rootKey(path) {
  const normalized = String(path ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
  const windows = /^[A-Za-z]:(\/|$)/.test(normalized) || normalized.startsWith("//");
  return windows ? normalized.toLowerCase() : normalized;
}

// Returns the keys of roots that are definitely unavailable. A root whose check fails for any
// other reason, or does not answer in time, is not reported, so an unrelated error never
// claims a folder is gone and a stalled share never holds up the workspace.
export async function findUnavailableRoots(rpc, roots, { timeoutMs = PROBE_TIMEOUT_MS } = {}) {
  const results = await Promise.all(
    (roots ?? []).map(async ({ path }) => {
      let timer;
      const timeout = new Promise((resolve) => {
        timer = setTimeout(() => resolve(null), timeoutMs);
      });
      const probe = rpc("fs/getMetadata", { path }).then(
        (metadata) => (metadata.isDirectory ? null : rootKey(path)),
        (error) => (MISSING_PATH.test(error?.message ?? "") ? rootKey(path) : null),
      );
      try {
        return await Promise.race([probe, timeout]);
      } finally {
        clearTimeout(timer);
      }
    }),
  );
  return new Set(results.filter(Boolean));
}

export function isRootUnavailable(unavailableRoots, projectRoot) {
  return Boolean(unavailableRoots?.has(rootKey(projectRoot)));
}

// The freshness a person should see for an indexed source: stored freshness can say
// "current" for a file whose whole project folder is gone.
export function effectiveFreshness(freshness, unavailableRoots, projectRoot) {
  return isRootUnavailable(unavailableRoots, projectRoot) ? "sourceUnavailable" : freshness;
}

// Plain-language explanations for the exact-source errors the app-server returns. The server
// reports every failure to open a root or file as "unavailable", so the operating system's
// reason decides between "missing" and "can't be read right now". Unknown messages pass
// through unchanged.
export function describeSourceError(message) {
  const text = String(message ?? "");
  const reason = text.replace(/^.*?unavailable:\s*/i, "");
  const missing = MISSING_PATH.test(text);
  if (/evidence root is unavailable/i.test(text)) {
    return missing
      ? {
          rootUnavailable: true,
          message:
            "The project folder can't be found on disk, so this source can't be checked. Restore the folder at its recorded path, then refresh the map.",
        }
      : {
          rootUnavailable: false,
          message: `The project folder can't be read right now (${reason}). Check that it is reachable and that you have access, then try again.`,
        };
  }
  if (/evidence source is unavailable/i.test(text)) {
    return {
      rootUnavailable: false,
      message: missing
        ? "This file can't be found on disk any more. Refresh the map to update the index."
        : `This file can't be read right now (${reason}). Check that you have access to it, then try again.`,
    };
  }
  if (/evidence source is SourceUnavailable/i.test(text)) {
    return {
      rootUnavailable: false,
      message: "This file can't be found where it was indexed. Refresh the map to update the index.",
    };
  }
  if (/source changed after indexing|evidence source is Stale/i.test(text)) {
    return {
      rootUnavailable: false,
      message: "This file changed after it was indexed. Refresh the map, then verify it again.",
    };
  }
  return { rootUnavailable: false, message: text };
}
