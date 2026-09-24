import json
import tempfile
import unittest
import zipfile
from pathlib import Path

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
            {"status": "graded", "correct": True, "mode": "str_verifier"},
        )
        self.assertEqual(
            grade_deterministic("range_verifier", "(10.0, 11.0)", "10.5"),
            {"status": "graded", "correct": True, "mode": "range_verifier"},
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
                "status": "graded",
                "correct": False,
                "mode": "agent_failure",
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


if __name__ == "__main__":
    unittest.main()
