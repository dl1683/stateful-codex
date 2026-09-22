const VISIBLE_DIMENSIONS = [
  "correctness",
  "evidenceTraceability",
  "decisiveDetail",
  "uncertaintyCalibration",
  "contradictionAndFreshness",
  "usefulness",
];
const DURABLE_DIMENSIONS = [
  "semanticFidelity",
  "evidenceTraceability",
  "compressionValue",
  "uncertaintyPreservation",
  "futureUsability",
];
const SOURCE_COUNT_FIELDS = [
  "uniqueFilesRead",
  "exactRegionsRead",
  "repeatedRegions",
  "broadReads",
  "failedReads",
];
const STATE_COUNT_FIELDS = [
  "projectRevision",
  "hierarchyNodes",
  "blackboardEntries",
  "contextMapEntries",
  "relationships",
  "rootEntries",
  "candidateEntries",
  "staleRecords",
  "maintenanceRecords",
];

export function validateObservationField(observation, field, arm) {
  if (!isObject(observation?.[field])) return [field];
  if (field === "quality") return validateQuality(observation.quality, arm);
  if (field === "sourceAudit") return validateSourceAudit(observation.sourceAudit);
  if (field === "state") return validateState(observation.state);
  return [];
}

function validateQuality(quality, arm) {
  const missing = [];
  if (quality.blinded !== true) missing.push("quality.blinded");
  requireString(missing, quality.rubricVersion, "quality.rubricVersion");
  if (!isObject(quality.grader)) {
    missing.push("quality.grader");
  } else {
    requireString(missing, quality.grader.id, "quality.grader.id");
    requireString(missing, quality.grader.model, "quality.grader.model");
    if (quality.grader.independent !== true) missing.push("quality.grader.independent");
  }
  validateEvidenceReferences(missing, quality.evidenceReferences);
  validateScoreGroup(
    missing,
    quality.scores?.visibleAnswer,
    quality.rationales?.visibleAnswer,
    "quality.scores.visibleAnswer",
    "quality.rationales.visibleAnswer",
    VISIBLE_DIMENSIONS,
  );
  if (arm === "stateful") {
    validateScoreGroup(
      missing,
      quality.scores?.durableState,
      quality.rationales?.durableState,
      "quality.scores.durableState",
      "quality.rationales.durableState",
      DURABLE_DIMENSIONS,
    );
  }
  return missing;
}

function validateSourceAudit(sourceAudit) {
  const missing = [];
  requireString(missing, sourceAudit.method, "sourceAudit.method");
  requireString(missing, sourceAudit.auditor, "sourceAudit.auditor");
  for (const field of SOURCE_COUNT_FIELDS) {
    requireCount(missing, sourceAudit[field], `sourceAudit.${field}`);
  }
  if (!/^sha256:[0-9a-f]{64}$/i.test(sourceAudit.corpusRevision ?? "")) {
    missing.push("sourceAudit.corpusRevision");
  }
  return missing;
}

function validateState(state) {
  const missing = [];
  requireString(missing, state.method, "state.method");
  requireCount(missing, state.observedAtMs, "state.observedAtMs");
  for (const field of STATE_COUNT_FIELDS) {
    requireCount(missing, state[field], `state.${field}`);
  }
  return missing;
}

function validateEvidenceReferences(missing, references) {
  if (!Array.isArray(references) || references.length === 0) {
    missing.push("quality.evidenceReferences");
    return;
  }
  for (const [index, reference] of references.entries()) {
    if (!isObject(reference)) {
      missing.push(`quality.evidenceReferences.${index}`);
      continue;
    }
    requireString(missing, reference.path, `quality.evidenceReferences.${index}.path`);
    requireString(missing, reference.locator, `quality.evidenceReferences.${index}.locator`);
    requireString(missing, reference.note, `quality.evidenceReferences.${index}.note`);
  }
}

function validateScoreGroup(
  missing,
  scores,
  rationales,
  scorePath,
  rationalePath,
  dimensions,
) {
  if (!isObject(scores)) {
    missing.push(scorePath);
    return;
  }
  if (!isObject(rationales)) missing.push(rationalePath);
  for (const dimension of dimensions) {
    const score = scores[dimension];
    if (!Number.isInteger(score) || score < 0 || score > 4) {
      missing.push(`${scorePath}.${dimension}`);
    }
    if (!isObject(rationales) || !nonEmptyString(rationales[dimension])) {
      missing.push(`${rationalePath}.${dimension}`);
    }
  }
}

function requireString(missing, value, path) {
  if (!nonEmptyString(value)) missing.push(path);
}

function requireCount(missing, value, path) {
  if (!Number.isSafeInteger(value) || value < 0) missing.push(path);
}

function nonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function isObject(value) {
  return value != null && typeof value === "object" && !Array.isArray(value);
}
