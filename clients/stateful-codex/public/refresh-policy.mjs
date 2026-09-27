export function needsProjectRefresh(status) {
  return (
    status?.initialized !== true ||
    status.lastRefresh?.inventoryComplete !== true
  );
}

export function createRefreshGate(operation) {
  let activePromise = null;
  let refreshQueued = false;

  return function refresh() {
    if (activePromise) {
      refreshQueued = true;
      return activePromise;
    }

    activePromise = (async () => {
      try {
        do {
          refreshQueued = false;
          await operation();
        } while (refreshQueued);
      } finally {
        activePromise = null;
      }
    })();
    return activePromise;
  };
}
