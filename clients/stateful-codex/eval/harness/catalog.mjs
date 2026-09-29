import { createHash } from "node:crypto";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../catalog/surface");
const REQUIRED_CAPTURE = new Set(["screens", "transcript", "actions", "approvals", "rollout", "stateStore", "projectState"]);

export async function loadScenarios({ selector = "*", env = process.env, checkFixtures = false } = {}) {
  const names = (await readdir(ROOT)).filter((name) => name.endsWith(".json")).sort();
  const scenarios = [];
  for (const name of names) {
    const scenario = JSON.parse(await readFile(path.join(ROOT, name), "utf8"));
    if (matches(scenario.id, selector)) {
      scenarios.push(await validateScenario(scenario, { env, checkFixtures }));
    }
  }
  if (!scenarios.length) throw new Error(`no surface scenarios match ${selector}`);
  return scenarios;
}

export async function validateScenario(scenario, { env = process.env, checkFixtures = false } = {}) {
  const errors = [];
  if (scenario.schemaVersion !== 1 || scenario.kind !== "surface" || scenario.surface !== "tui") errors.push("schemaVersion/kind/surface");
  if (!scenario.id || !/^surface\.tui\.[a-z0-9-]+$/.test(scenario.id)) errors.push("id");
  const fixture = scenario.fixture ?? {};
  if (!/^sha256:[0-9a-f]{64}$/.test(fixture.sha256 ?? "")) errors.push("fixture.sha256");
  let source;
  try { source = resolveEnv(fixture.source, env); } catch (error) { errors.push(error.message); }
  const conversation = scenario.conversation ?? {};
  if (!conversation.mode || !conversation.initialMessage || !Array.isArray(conversation.turns) || conversation.turns.some((turn) => typeof turn !== "string" || !turn)) errors.push("conversation messages");
  if (!Number.isInteger(conversation.turnTimeoutSeconds) || conversation.turnTimeoutSeconds < 1 || !Number.isInteger(conversation.idleStableSeconds) || conversation.idleStableSeconds < 1) errors.push("conversation timeouts");
  if (!Array.isArray(scenario.capture) || ![...REQUIRED_CAPTURE].every((item) => scenario.capture.includes(item))) errors.push("capture");
  if (scenario.grading?.adapter !== "tui-evidence" || !scenario.grading?.observations?.length) errors.push("grading");
  if (fixture.private && fixture.type !== "external") errors.push("private fixture must be external");
  if (fixture.type === "pinned-git" && (!fixture.sourceUrl || !fixture.revision)) errors.push("pinned fixture metadata");
  if (checkFixtures && source) {
    const actual = `sha256:${await hashDirectory(source)}`;
    if (actual !== fixture.sha256) errors.push(`fixture hash mismatch: expected ${fixture.sha256}, got ${actual}`);
  }
  if (errors.length) throw new Error(`${scenario.id} invalid: ${errors.join(", ")}`);
  return { ...scenario, fixture: { ...fixture, resolvedSource: source } };
}

export function resolveEnv(value, env = process.env) {
  return String(value).replace(/\$\{([A-Z_][A-Z0-9_]*)\}/g, (_, name) => {
    if (!env[name]) throw new Error(`unresolved environment variable ${name}`);
    return env[name];
  });
}

export async function hashDirectory(root) {
  const files = [];
  async function visit(directory) {
    for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      if (entry.name === ".git") continue;
      const full = path.join(directory, entry.name);
      if (entry.isDirectory()) await visit(full);
      else if (entry.isFile()) files.push([path.relative(root, full).replaceAll(path.sep, "/"), await readFile(full)]);
    }
  }
  await visit(root);
  const hash = createHash("sha256");
  for (const [name, content] of files.sort(([a], [b]) => a.localeCompare(b))) hash.update(name).update("\0").update(content).update("\0");
  return hash.digest("hex");
}

function matches(id, selector) {
  if (selector === "*" || selector === id) return true;
  if (selector.endsWith("*")) return id.startsWith(selector.slice(0, -1));
  return false;
}
