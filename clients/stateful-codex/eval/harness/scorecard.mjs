import { writeFile } from "node:fs/promises";
import path from "node:path";

export function buildScorecard({ manifest, plan, grades }) {
  const findings = grades.flatMap(({ scenarioId, repetition, findings }) => findings.map((finding) => ({ scenarioId, repetition, ...finding })));
  return { schemaVersion: 1, provenance: { plan, executable: manifest.codex, startedAt: manifest.startedAt, finishedAt: manifest.finishedAt }, attempts: manifest.attempts.map(({ scenarioId, repetition, arm, exitReason, retryClassification }) => ({ scenarioId, repetition, arm, exitReason, retryClassification })), findings, validity: { validAttempts: grades.filter(({ valid }) => valid).length, totalAttempts: grades.length } };
}

export async function writeScorecard(out, scorecard) {
  await writeFile(path.join(out, "scorecard.json"), `${JSON.stringify(scorecard, null, 2)}\n`);
  const lines = ["# scbench scorecard", "", `Attempts: ${scorecard.validity.validAttempts}/${scorecard.validity.totalAttempts} valid`, "", "## Findings", "", "| Scenario | Arm | Rep | Observation | Status | Evidence |", "|---|---|---:|---|---|---|"];
  for (const finding of scorecard.findings) lines.push(`| ${finding.scenarioId} | ${finding.arm ?? ""} | ${finding.repetition} | ${finding.id} | ${finding.status} | ${finding.evidence} |`);
  lines.push("", "No synthetic aggregate quality score is computed; observations remain independently gradable.", "");
  await writeFile(path.join(out, "scorecard.md"), lines.join("\n"));
}
