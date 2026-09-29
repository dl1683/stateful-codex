import os from "node:os";

const REPS = { surface: 1, smoke: 1, full: 3, claim: 5 };

export function planScenarios(scenarios, { tier = "surface", reps = REPS[tier], jobs, parallelism = os.availableParallelism(), maxWallMinutes, maxAttempts } = {}) {
  if (!REPS[tier]) throw new Error(`unknown tier ${tier}`);
  if (!Number.isInteger(reps) || reps < 1) throw new Error("reps must be a positive integer");
  const attempts = scenarios.length * reps;
  const sessions = attempts;
  const userTurns = scenarios.reduce((sum, scenario) => sum + 1 + scenario.conversation.turns.length, 0) * reps;
  const effectiveJobs = Math.max(1, Math.min(24, jobs ?? parallelism));
  const serialMinutes = scenarios.reduce((sum, scenario) => sum + (1 + scenario.conversation.turns.length) * scenario.conversation.turnTimeoutSeconds / 60, 0) * reps;
  const concurrentMinutes = Math.ceil(serialMinutes / effectiveJobs);
  const plan = {
    schemaVersion: 1,
    tier,
    reps,
    attempts,
    sessions,
    userTurns,
    selectedSurfaces: [...new Set(scenarios.map(({ surface }) => surface))],
    selectedModes: [...new Set(scenarios.map(({ conversation: { mode } }) => mode))],
    maximumTheoreticalParallelism: effectiveJobs,
    jobs: effectiveJobs,
    historicalEstimate: null,
    projectedWallMinutes: { serial: serialMinutes, concurrent: concurrentMinutes },
    privateFixtures: scenarios.filter(({ fixture }) => fixture.private).map(({ id, fixture }) => ({ id, source: fixture.resolvedSource, sha256: fixture.sha256 })),
    scenarioIds: scenarios.map(({ id }) => id),
  };
  if (maxWallMinutes !== undefined && concurrentMinutes > maxWallMinutes) throw new Error(`plan exceeds --max-wall-minutes ${maxWallMinutes}`);
  if (maxAttempts !== undefined && attempts > maxAttempts) throw new Error(`plan exceeds --max-attempts ${maxAttempts}`);
  return plan;
}

export { REPS };
