import { createHash } from "node:crypto";
import { rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { pathToFileURL } from "node:url";

const FORMAT_VERSION = "stateful-project-state-v1";

export function readProjectState({ sqliteHome, threadId, runId = null }) {
  const runtime = new DatabaseSync(path.join(sqliteHome, "stateful_runtime_1.sqlite"), {
    readOnly: true,
  });
  const intelligence = new DatabaseSync(
    path.join(sqliteHome, "project_intelligence_1.sqlite"),
    { readOnly: true },
  );
  try {
    const run = selectRun(runtime, threadId, runId);
    const projectId = run.projectId;
    const hierarchy = rows(
      intelligence
        .prepare(
          `SELECT id, parent_id AS parentId, kind, project_root AS projectRoot,
                  relative_path AS relativePath, anchor_scheme AS anchorScheme,
                  anchor_locator AS anchorLocator, source_fingerprint AS sourceFingerprint,
                  lifecycle, revision, created_at_ms AS createdAtMs,
                  updated_at_ms AS updatedAtMs
             FROM hierarchy_nodes WHERE project_id = ? ORDER BY id`,
        )
        .all(projectId),
    );
    const contextMap = rows(
      intelligence
        .prepare(
          `SELECT entry.id, entry.node_id AS nodeId,
                  entry.source_fingerprint AS sourceFingerprint,
                  entry.description, entry.coverage, entry.revision,
                  entry.created_at_ms AS createdAtMs,
                  entry.updated_at_ms AS updatedAtMs,
                  entry.last_verified_at_ms AS lastVerifiedAtMs,
                  COALESCE((
                    SELECT json_group_array(ordered.term)
                      FROM (
                        SELECT term.term
                          FROM context_map_routing_terms AS term
                         WHERE term.entry_id = entry.id
                         ORDER BY term.position
                      ) AS ordered
                  ), '[]') AS routingTermsJson
             FROM context_map_entries AS entry
            WHERE entry.project_id = ?
            ORDER BY entry.id`,
        )
        .all(projectId),
    ).map(({ routingTermsJson, ...entry }) => ({
      ...entry,
      routingTerms: JSON.parse(routingTermsJson),
    }));
    const blackboardEntries = rows(
      intelligence
        .prepare(
          `SELECT id, node_id AS nodeId, revision, created_at_ms AS createdAtMs,
                  updated_at_ms AS updatedAtMs
             FROM blackboard_entries WHERE project_id = ? ORDER BY id`,
        )
        .all(projectId),
    );
    const blackboardRevisions = rows(
      intelligence
        .prepare(
          `SELECT revision.entry_id AS entryId, revision.revision, revision.kind,
                  revision.content, revision.structured_value AS structuredValue,
                  revision.structured_unit AS structuredUnit,
                  revision.confidence_basis_points AS confidenceBasisPoints,
                  revision.verification, revision.importance,
                  revision.root_promotion AS rootPromotion, revision.state,
                  revision.superseded_by AS supersededBy,
                  revision.provenance_kind AS provenanceKind,
                  revision.provenance_source_id AS provenanceSourceId,
                  revision.recorded_at_ms AS recordedAtMs
             FROM blackboard_entry_revisions AS revision
             JOIN blackboard_entries AS entry ON entry.id = revision.entry_id
            WHERE entry.project_id = ? ORDER BY revision.entry_id, revision.revision`,
        )
        .all(projectId),
    );
    const evidenceLinks = rows(
      intelligence
        .prepare(
          `SELECT link.entry_id AS entryId, link.revision, link.position,
                  link.context_map_entry_id AS contextMapEntryId,
                  link.source_fingerprint AS sourceFingerprint,
                  link.first_line AS firstLine, link.last_line AS lastLine
             FROM blackboard_evidence_links AS link
             JOIN blackboard_entries AS entry ON entry.id = link.entry_id
            WHERE entry.project_id = ?
            ORDER BY link.entry_id, link.revision, link.position`,
        )
        .all(projectId),
    );
    const relations = rows(
      intelligence
        .prepare(
          `SELECT id, from_entry_id AS fromEntryId, to_entry_id AS toEntryId,
                  kind, note, confidence_basis_points AS confidenceBasisPoints,
                  provenance_kind AS provenanceKind,
                  provenance_source_id AS provenanceSourceId, revision,
                  created_at_ms AS createdAtMs, updated_at_ms AS updatedAtMs
             FROM blackboard_relations WHERE project_id = ? ORDER BY id`,
        )
        .all(projectId),
    );
    const intelligenceRevision = intelligence
      .prepare(
        "SELECT revision FROM project_intelligence_revisions WHERE project_id = ?",
      )
      .get(projectId)?.revision ?? 0;
    const obligations = rows(
      runtime
        .prepare(
          `SELECT id, sequence, packet_json AS packetJson,
                  provenance_source_id AS provenanceSourceId, revision,
                  created_at_ms AS createdAtMs
             FROM stateful_obligations WHERE project_id = ? AND run_id = ?
            ORDER BY sequence`,
        )
        .all(projectId, run.id),
    ).map(({ packetJson, ...obligation }) => ({
      ...obligation,
      packet: JSON.parse(packetJson),
    }));
    const steering = rows(
      runtime
        .prepare(
          `SELECT id, input, affected_obligation_ids_json AS affectedIdsJson,
                  status, resulting_strategy_revision AS resultingStrategyRevision,
                  reason, revision, created_at_ms AS createdAtMs,
                  updated_at_ms AS updatedAtMs
             FROM stateful_steering WHERE project_id = ? AND run_id = ?
            ORDER BY created_at_ms, id`,
        )
        .all(projectId, run.id),
    ).map(({ affectedIdsJson, ...instruction }) => ({
      ...instruction,
      affectedObligationIds: JSON.parse(affectedIdsJson),
    }));
    return {
      formatVersion: FORMAT_VERSION,
      projectId,
      intelligenceRevision,
      run,
      obligations,
      steering,
      hierarchy,
      contextMap,
      blackboard: {
        entries: blackboardEntries,
        revisions: blackboardRevisions,
        evidenceLinks,
        relations,
      },
      counts: {
        hierarchyNodes: hierarchy.length,
        contextMapEntries: contextMap.length,
        blackboardEntries: blackboardEntries.length,
        blackboardRevisions: blackboardRevisions.length,
        evidenceLinks: evidenceLinks.length,
        relations: relations.length,
        obligations: obligations.length,
        steering: steering.length,
      },
    };
  } finally {
    intelligence.close();
    runtime.close();
  }
}

export async function writeProjectStateArtifact(options) {
  const state = readProjectState(options);
  const snapshot = {
    ...state,
    corpusRevision: options.corpusRevision ?? null,
    capturedAtMs: options.capturedAtMs ?? Date.now(),
  };
  const artifact = {
    ...snapshot,
    snapshotSha256: stateArtifactHash(snapshot),
  };
  const temporaryPath = `${options.output}.tmp-${process.pid}`;
  await writeFile(temporaryPath, `${JSON.stringify(artifact, null, 2)}\n`);
  await rename(temporaryPath, options.output);
  return artifact;
}

export function stateArtifactHash(artifact) {
  const { snapshotSha256: _snapshotSha256, ...snapshot } = artifact;
  return createHash("sha256").update(JSON.stringify(snapshot)).digest("hex");
}

function selectRun(database, threadId, runId) {
  const selected = runId
    ? database
        .prepare(
          `SELECT id, project_id AS projectId, goal, mode, status, strategy,
                  strategy_revision AS strategyRevision, result, revision,
                  max_continuations AS maxContinuations,
                  max_elapsed_seconds AS maxElapsedSeconds,
                  continuations_used AS continuationsUsed,
                  created_at_ms AS createdAtMs, updated_at_ms AS updatedAtMs
             FROM stateful_runs WHERE id = ?`,
        )
        .get(runId)
    : database
        .prepare(
          `SELECT run.id, run.project_id AS projectId, run.goal, run.mode,
                  run.status, run.strategy,
                  run.strategy_revision AS strategyRevision, run.result,
                  run.revision, run.max_continuations AS maxContinuations,
                  run.max_elapsed_seconds AS maxElapsedSeconds,
                  run.continuations_used AS continuationsUsed,
                  run.created_at_ms AS createdAtMs,
                  run.updated_at_ms AS updatedAtMs
             FROM stateful_runs AS run
             JOIN stateful_run_threads AS thread ON thread.run_id = run.id
            WHERE thread.thread_id = ? ORDER BY run.updated_at_ms DESC, run.id LIMIT 1`,
        )
        .get(threadId);
  if (!selected) throw new Error(`stateful run not found for ${runId ?? threadId}`);
  const run = { ...selected };
  run.threadIds = rows(
    database
      .prepare(
        "SELECT thread_id AS threadId FROM stateful_run_threads WHERE run_id = ? ORDER BY position",
      )
      .all(run.id),
  ).map((row) => row.threadId);
  return run;
}

function rows(values) {
  return values.map((value) => ({ ...value }));
}

function parseArgs(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const [argument, value] = args.slice(index, index + 2);
    if (argument === "--sqlite-home") options.sqliteHome = value;
    else if (argument === "--thread") options.threadId = value;
    else if (argument === "--run") options.runId = value;
    else if (argument === "--corpus-revision") options.corpusRevision = value;
    else if (argument === "--output") options.output = value;
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (
    !options.sqliteHome ||
    (!options.threadId && !options.runId) ||
    (options.threadId && options.runId) ||
    !options.output
  ) {
    throw new Error(
      "usage: --sqlite-home PATH (--thread ID | --run ID) --output PATH [--corpus-revision HASH]",
    );
  }
  options.sqliteHome = path.resolve(options.sqliteHome);
  options.output = path.resolve(options.output);
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await writeProjectStateArtifact(parseArgs(process.argv.slice(2)));
}
