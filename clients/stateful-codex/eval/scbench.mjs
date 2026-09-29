#!/usr/bin/env node
import { loadScenarios } from "./harness/catalog.mjs";
import { planScenarios } from "./harness/planner.mjs";

const args = process.argv.slice(2);
const command = args.shift();
const selector = args[0] && !args[0].startsWith("--") ? args.shift() : "*";
if (!command || !["list", "validate", "plan"].includes(command)) {
  console.error("usage: scbench list|validate|plan [SELECTOR] [--tier TIER] [--jobs N] [--reps N]");
  process.exitCode = 2;
} else {
  try {
    const options = parseOptions(args);
    const scenarios = await loadScenarios({ selector, checkFixtures: command === "validate" });
    if (command === "list") console.log(scenarios.map(({ id }) => id).join("\n"));
    else if (command === "validate") console.log(JSON.stringify({ valid: scenarios.map(({ id }) => id) }, null, 2));
    else console.log(JSON.stringify(planScenarios(scenarios, { ...options, tier: options.tier ?? "surface" }), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

function parseOptions(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
    if (!name?.startsWith("--") || value === undefined) throw new Error(`expected value after ${name}`);
    if (!["--tier", "--jobs", "--reps", "--max-wall-minutes", "--max-attempts", "--codex"].includes(name)) throw new Error(`unknown option ${name}`);
    options[name.slice(2).replaceAll("-", "_")] = ["tier", "codex"].includes(name.slice(2)) ? value : Number(value);
  }
  return options;
}
