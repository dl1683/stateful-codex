export function needsProjectRefresh(status) {
  return (
    status?.initialized !== true ||
    status.lastRefresh?.inventoryComplete !== true
  );
}
