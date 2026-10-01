// Whether the project's folders exist on disk right now, and how source-access failures are
// explained to a person. The index keeps describing sources after their folder disappears, so
// the client checks each root itself (fs/getMetadata) instead of trusting stored freshness.

// Missing-path failures as the operating system reports them (Windows os error 2 and 3,
// POSIX ENOENT and ENOTDIR).
const MISSING_PATH =
  /os error (2|3|20)\b|cannot find the (path|file)|no such file or directory|not a directory/i;

// Paths from different responses may differ only in separators or a trailing separator.
export function rootKey(path) {
  return String(path ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
}

// Returns the keys of roots that are definitely unavailable. A root whose check fails for any
// other reason is not reported, so an unrelated error never claims a folder is gone.
export async function findUnavailableRoots(rpc, roots) {
  const results = await Promise.all(
    (roots ?? []).map(async ({ path }) => {
      try {
        const metadata = await rpc("fs/getMetadata", { path });
        return metadata.isDirectory ? null : rootKey(path);
      } catch (error) {
        return MISSING_PATH.test(error?.message ?? "") ? rootKey(path) : null;
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

// Plain-language explanations for the exact-source errors the app-server returns. Unknown
// messages pass through unchanged.
export function describeSourceError(message) {
  const text = String(message ?? "");
  if (/evidence root is unavailable/i.test(text)) {
    return {
      rootUnavailable: true,
      message:
        "The project folder can't be found on disk, so this source can't be checked. Restore the folder at its recorded path, then refresh the map.",
    };
  }
  if (/evidence source is unavailable/i.test(text) || /evidence source is SourceUnavailable/i.test(text)) {
    return {
      rootUnavailable: false,
      message: "This file can't be found on disk any more. Refresh the map to update the index.",
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
