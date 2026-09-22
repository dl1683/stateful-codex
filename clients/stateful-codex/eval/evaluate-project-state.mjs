import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export function scoreProjectState(hits, manifest, { truncated = false } = {}) {
  const entries = hits.map((hit) => hit.entry);
  const concepts = manifest.expectedConcepts.map((concept) => {
    const match = hits.find((hit) => matchesConcept(hit.entry, concept));
    return {
      name: concept.name,
      matched: Boolean(match),
      entryId: match?.entry.id ?? null,
      supported: match ? isCurrentlySupported(match) : false,
    };
  });
  const supportedEntries = hits.filter(isCurrentlySupported);
  const forbiddenClaims = manifest.forbiddenPhrases.flatMap((phrase) =>
    entries
      .filter((entry) => includes(entry.content, phrase))
      .map((entry) => ({ phrase, entryId: entry.id })),
  );
  const semanticRecall = ratio(
    concepts.filter((concept) => concept.matched).length,
    concepts.length,
  );
  const supportedConceptRecall = ratio(
    concepts.filter((concept) => concept.supported).length,
    concepts.length,
  );
  const supportedEntryPrecision = ratio(supportedEntries.length, hits.length);

  return {
    manifest: manifest.name,
    completeSnapshot: !truncated,
    entries: {
      total: hits.length,
      currentlySupported: supportedEntries.length,
      supportedEntryPrecision,
    },
    concepts: {
      total: concepts.length,
      matched: concepts.filter((concept) => concept.matched).length,
      currentlySupported: concepts.filter((concept) => concept.supported).length,
      semanticRecall,
      supportedConceptRecall,
      probes: concepts,
    },
    forbiddenClaims,
    passed:
      !truncated &&
      semanticRecall === 1 &&
      supportedConceptRecall === 1 &&
      supportedEntryPrecision === 1 &&
      forbiddenClaims.length === 0,
  };
}

function matchesConcept(entry, concept) {
  if (concept.kinds && !concept.kinds.includes(entry.kind)) return false;
  return concept.termGroups.every((alternatives) =>
    alternatives.some((term) => includes(entry.content, term)),
  );
}

function isCurrentlySupported(hit) {
  return (
    hit.entry.state === "active" &&
    hit.effectiveVerification === "sourceVerified" &&
    hit.evidenceFreshness === "current" &&
    hit.entry.evidence.length > 0
  );
}

function includes(content, term) {
  return content
    .toLocaleLowerCase("en-US")
    .includes(term.toLocaleLowerCase("en-US"));
}

function ratio(numerator, denominator) {
  return denominator === 0 ? 1 : numerator / denominator;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  const token = await sessionToken(options.gateway);
  const [status, blackboard] = await Promise.all([
    rpc(options.gateway, token, 1, "projectIntelligence/status", {
      projectId: options.project,
    }),
    rpc(options.gateway, token, 2, "blackboard/query", {
      projectId: options.project,
      limit: 50,
    }),
  ]);
  const report = {
    projectId: options.project,
    intelligenceRevision: status.revision,
    initialized: status.initialized,
    ...scoreProjectState(blackboard.data, manifest, {
      truncated: blackboard.truncated,
    }),
  };
  console.log(JSON.stringify(report, null, 2));
  if (!report.initialized || !report.passed) process.exitCode = 2;
}

function parseArgs(args) {
  const options = {
    gateway: "http://127.0.0.1:4174",
    manifest: new URL("./manifests/procurement-state.json", import.meta.url),
  };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    const value = args[index + 1];
    if (argument === "--gateway") options.gateway = value;
    else if (argument === "--project") options.project = value;
    else if (argument === "--manifest") options.manifest = value;
    else throw new Error(`unknown argument: ${argument}`);
    index += 1;
  }
  if (!options.project) {
    throw new Error(
      "usage: --project PROJECT_ID [--gateway URL] [--manifest PATH]",
    );
  }
  return options;
}

async function sessionToken(gateway) {
  const response = await fetch(`${gateway}/`);
  if (!response.ok) throw new Error(`gateway setup returned ${response.status}`);
  const html = await response.text();
  const match = html.match(/<meta name="stateful-session" content="([^"]+)"/);
  if (!match) throw new Error("gateway session token is missing");
  return match[1];
}

async function rpc(gateway, token, id, method, params) {
  const response = await fetch(`${gateway}/rpc`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-stateful-session": token,
    },
    body: JSON.stringify({ id, method, params }),
  });
  const payload = await response.json();
  if (!response.ok || payload.error) {
    throw new Error(
      payload.error?.message ?? `gateway RPC returned ${response.status}`,
    );
  }
  return payload.result;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
