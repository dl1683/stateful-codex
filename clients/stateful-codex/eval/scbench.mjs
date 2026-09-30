#!/usr/bin/env node
import { loadScenarios } from "./harness/catalog.mjs";
import { runBundle } from "./harness/bundle.mjs";
import { planScenarios } from "./harness/planner.mjs";
import { readFile } from "node:fs/promises";

const args = process.argv.slice(2);
const command = args.shift();
if (!command || !["list", "validate", "plan", "run", "report"].includes(command)) {
  console.error("usage: scbench list|validate|plan|run [SELECTOR] [options] | report BUNDLE");
  process.exitCode = 2;
} else {
  try {
    if (command === "report") {
      const bundle = args.shift();
      if (!bundle || args.length) throw new Error("usage: scbench report BUNDLE");
      console.log(await readFile(`${bundle}/scorecard.md`, "utf8"));
    } else {
      const selector = args[0] && !args[0].startsWith("--") ? args.shift() : "*";
      const options = parseOptions(args);
      const scenarios = await loadScenarios({ selector, checkFixtures: command === "validate" || command === "run" });
      if (command === "list") console.log(scenarios.map(({ id }) => id).join("\n"));
      else if (command === "validate") console.log(JSON.stringify({ valid: scenarios.map(({ id }) => id) }, null, 2));
      else if (command === "plan") console.log(JSON.stringify(planScenarios(scenarios, { ...options, tier: options.tier ?? "surface" }), null, 2));
      else {
        if (!options.codex || !options.out) throw new Error("scbench run requires --codex PATH and --out PATH");
        const scorecard = await runBundle({ scenarios, ...options, tier: options.tier ?? "surface" });
        console.log(JSON.stringify(scorecard.validity, null, 2));
      }
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

function parseOptions(args) {
  const options = {};
  const names = new Set(["--tier", "--jobs", "--reps", "--max-wall-minutes", "--max-attempts", "--codex", "--out", "--auth-home", "--python", "--work-root"]);
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
    if (!name?.startsWith("--") || value === undefined) throw new Error(`expected value after ${name}`);
    if (!names.has(name)) throw new Error(`unknown option ${name}`);
    const key = name.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    if (["tier", "codex", "out", "authHome", "python", "workRoot"].includes(key)) options[key] = value;
    else {
      const numeric = Number(value);
      if (!Number.isInteger(numeric) || numeric < 1) throw new Error(`${name} must be a positive integer`);
      options[key] = numeric;
    }
  }
  return options;
}
