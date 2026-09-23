import tempfile
import unittest
from pathlib import Path

from protocol import grade_deterministic, normalized_answer, sha256_file


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


if __name__ == "__main__":
    unittest.main()
