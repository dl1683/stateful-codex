#!/usr/bin/env node
import { loadScenarios } from "./harness/catalog.mjs";

const [command, selector = "*"] = process.argv.slice(2);
if (!command || !["list", "validate"].includes(command)) {
  console.error("usage: scbench list|validate [SELECTOR]");
  process.exitCode = 2;
} else {
  try {
    const scenarios = await loadScenarios({ selector, checkFixtures: command === "validate" });
    if (command === "list") console.log(scenarios.map(({ id }) => id).join("\n"));
    else console.log(JSON.stringify({ valid: scenarios.map(({ id }) => id) }, null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
