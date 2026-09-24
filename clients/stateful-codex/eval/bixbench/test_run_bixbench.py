import json
import sqlite3
import tempfile
import unittest
import zipfile
from contextlib import closing
from pathlib import Path

from artifact_validation import (
    assess_operational_validity,
    classify_provider_failure,
    inspect_stateful_state,
)
from protocol import grade_deterministic, normalized_answer, sha256_file
from runner_support import (
    audit_agent_log,
    failed_agent_grade,
    parse_usage,
    preprocess_capsule,
    workspace_manifest,
)


class BixBenchRunnerTests(unittest.TestCase):
    def test_normalizes_xml_answer(self) -> None:
        self.assertEqual(normalized_answer(" <answer> 12.5 </answer> "), "12.5")

    def test_grades_string_and_range_modes(self) -> None:
        self.assertEqual(
            grade_deterministic("str_verifier", "12,000", "12000"),
            {
                "status": "metadata_verifier",
                "official": False,
                "correct": True,
                "mode": "str_verifier",
                "upstreamNormalizedMatch": True,
                "normalizationCollision": False,
            },
        )
        self.assertEqual(
            grade_deterministic("range_verifier", "(10.0, 11.0)", "10.5"),
            {
                "status": "metadata_verifier",
                "official": False,
                "correct": True,
                "mode": "range_verifier",
                "formatOnlyFailure": False,
            },
        )

    def test_rejects_numeric_collisions_in_upstream_string_normalization(self) -> None:
        self.assertEqual(
            grade_deterministic("str_verifier", "-0.5", "0.5"),
            {
                "status": "metadata_verifier",
                "official": False,
                "correct": False,
                "mode": "str_verifier",
                "upstreamNormalizedMatch": True,
                "normalizationCollision": True,
            },
        )
        self.assertEqual(
            grade_deterministic("range_verifier", "(12000, 13000)", "12,500"),
            {
                "status": "metadata_verifier",
                "official": False,
                "correct": False,
                "mode": "range_verifier",
                "reason": "answer is not a single number",
                "formatOnlyFailure": True,
            },
        )

    def test_hashes_artifact_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact"
            path.write_bytes(b"stateful")
            self.assertEqual(
                sha256_file(path),
                "58bdfeb61cba4c3ca0a276b86e54c7ebadb30bded1e0de68838af234f8ffbb0a",
            )

    def test_aggregates_usage_across_completed_turns(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "codex.jsonl"
            events = [
                {
                    "type": "turn.completed",
                    "usage": {"input_tokens": 10, "output_tokens": 2},
                },
                {"type": "item.completed", "usage": {"input_tokens": 999}},
                {
                    "type": "turn.completed",
                    "usage": {"input_tokens": 7, "output_tokens": 3},
                },
            ]
            log.write_text("\n".join(map(json.dumps, events)), encoding="utf-8")
            self.assertEqual(parse_usage(log), {"input_tokens": 17, "output_tokens": 5})

    def test_preprocesses_only_the_single_data_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "capsule.zip"
            with zipfile.ZipFile(archive, "w") as file:
                file.writestr("capsule/Data/input.csv", "x,y\n1,2\n")
                file.writestr("capsule/Notebook/answer.ipynb", "secret")
                file.writestr("capsule/notes.txt", "not task data")
            destination = root / "workspace"
            preprocess_capsule(archive, destination)
            self.assertEqual(
                [path.name for path in destination.iterdir()], ["input.csv"]
            )
            self.assertEqual(
                workspace_manifest(destination)["files"][0]["path"], "input.csv"
            )

    def test_rejects_ambiguous_data_directories(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "capsule.zip"
            with zipfile.ZipFile(archive, "w") as file:
                file.writestr("a/Data/one.csv", "1")
                file.writestr("b/Data/two.csv", "2")
            with self.assertRaisesRegex(ValueError, "exactly one Data directory"):
                preprocess_capsule(archive, Path(directory) / "workspace")

    def test_agent_failures_are_scored_wrong(self) -> None:
        self.assertEqual(
            failed_agent_grade("agent timed out"),
            {
                "status": "agent_failure",
                "official": False,
                "correct": False,
                "reason": "agent timed out",
            },
        )

    def test_flags_prohibited_network_access(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "codex.jsonl"
            log.write_text(
                json.dumps(
                    {
                        "type": "item.completed",
                        "command": "curl https://huggingface.co/datasets/futurehouse/BixBench",
                    }
                ),
                encoding="utf-8",
            )
            self.assertEqual(
                audit_agent_log(log),
                {
                    "passed": False,
                    "findings": ["line 1: benchmark-source network access"],
                },
            )

    def test_classifies_provider_failures(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stderr = Path(directory) / "codex.stderr"
            stderr.write_text("request failed: 429 Too Many Requests", encoding="utf-8")
            self.assertEqual(
                classify_provider_failure(stderr),
                "provider or authentication failure: too many requests",
            )

    def test_assesses_operational_validity_separately_from_correctness(self) -> None:
        result = {
            "agentStarted": True,
            "timedOut": False,
            "exitCode": 0,
            "answer": "wrong but parseable",
            "notebook": {"status": "valid"},
            "notebookReplay": {"status": "reproducible"},
            "integrityAudit": {"passed": True},
            "imageId": "sha256:image",
            "bundleSha256": "bundle",
            "workspaceManifestSha256": "workspace",
            "control": {"promptSha256": "prompt"},
            "statefulState": {"valid": True},
        }
        self.assertEqual(
            assess_operational_validity(result, stateful=True),
            {
                "valid": True,
                "checks": {
                    "agentCompleted": True,
                    "structuredAnswer": True,
                    "submittedNotebook": True,
                    "offlineNotebookReplay": True,
                    "protocolAudit": True,
                    "artifactHashes": True,
                    "terminalStatefulRun": True,
                },
                "failures": [],
            },
        )

        result["notebookReplay"] = {"status": "failed"}
        result["statefulState"] = {"valid": False}
        self.assertEqual(
            assess_operational_validity(result, stateful=True)["failures"],
            ["offlineNotebookReplay", "terminalStatefulRun"],
        )

    def test_inspects_terminal_stateful_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            state_dir = Path(directory)
            runtime_path = state_dir / "stateful_runtime_1.sqlite"
            with closing(sqlite3.connect(runtime_path)) as runtime:
                runtime.executescript(
                    """
                    CREATE TABLE stateful_runs (
                        id TEXT, project_id TEXT, mode TEXT, status TEXT,
                        revision INTEGER, strategy_revision INTEGER, result TEXT,
                        continuations_used INTEGER, updated_at_ms INTEGER
                    );
                    CREATE TABLE stateful_obligations (id TEXT);
                    INSERT INTO stateful_runs VALUES
                        ('run-1', 'project-1', 'autonomous', 'completed',
                         3, 2, 'done', 0, 1);
                    INSERT INTO stateful_obligations VALUES ('obligation-1');
                    """
                )
            intelligence_path = state_dir / "project_intelligence_1.sqlite"
            with closing(sqlite3.connect(intelligence_path)) as intelligence:
                intelligence.executescript(
                    """
                    CREATE TABLE hierarchy_nodes (id TEXT, project_id TEXT);
                    CREATE TABLE context_map_entries (id TEXT, project_id TEXT);
                    CREATE TABLE blackboard_entries (id TEXT, project_id TEXT);
                    CREATE TABLE blackboard_relations (id TEXT, project_id TEXT);
                    INSERT INTO hierarchy_nodes VALUES ('node-1', 'project-1');
                    INSERT INTO context_map_entries VALUES ('map-1', 'project-1');
                    INSERT INTO blackboard_entries VALUES ('entry-1', 'project-1');
                    """
                )
            self.assertEqual(
                inspect_stateful_state(state_dir),
                {
                    "status": "valid",
                    "valid": True,
                    "databases": {
                        "runtime": {
                            "exists": True,
                            "integrity": ["ok"],
                            "files": {
                                runtime_path.name: sha256_file(runtime_path),
                            },
                        },
                        "projectIntelligence": {
                            "exists": True,
                            "integrity": ["ok"],
                            "files": {
                                intelligence_path.name: sha256_file(intelligence_path),
                            },
                        },
                    },
                    "run": {
                        "id": "run-1",
                        "project_id": "project-1",
                        "mode": "autonomous",
                        "status": "completed",
                        "revision": 3,
                        "strategy_revision": 2,
                        "result": "done",
                        "continuations_used": 0,
                    },
                    "counts": {
                        "runs": 1,
                        "obligations": 1,
                        "hierarchyNodes": 1,
                        "contextMapEntries": 1,
                        "blackboardEntries": 1,
                        "blackboardRelations": 0,
                    },
                },
            )


if __name__ == "__main__":
    unittest.main()
