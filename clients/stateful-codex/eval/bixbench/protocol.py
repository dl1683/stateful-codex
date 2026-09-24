import ast
import hashlib
import json
import re
from pathlib import Path
from typing import Any


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def prompt_for(question: dict[str, Any]) -> str:
    return f"""You are the analysis agent in a BixBench v1.5 evaluation.

The project directory /workspace contains the input data for exactly one
bioinformatics question. Analyze the files directly and answer this question:

<question>
{question["question"]}
</question>

Required deliverables:
1. Create /workspace/notebook.ipynb as a valid, executed Jupyter notebook.
2. Use Python code cells, matching the official BixBench run configuration.
   You may invoke installed R packages through rpy2 or a recorded subprocess
   when scientifically appropriate. Preserve the code, tables, statistics, and
   cell outputs needed for an independent reviewer to verify the answer.
3. Inspect the data rigorously, check shapes and missing values, and state any
   scientifically important assumptions in code comments and printed output.
4. Return one concise answer in the required JSON response. For a numerical
   question, the answer field must contain only the number, without units or
   explanatory prose. Put explanation in summary and evidence instead.
5. The final code cell must print exactly `BIXBENCH_ANSWER=<answer>`, using the
   same answer string returned in JSON and the value computed by the notebook.
6. Before finishing, reopen notebook.ipynb and verify that it is valid and that
   its recorded outputs support the answer.

Do not search for or infer a benchmark answer key. Solve the question from the
project data. Work autonomously until both the notebook and answer are complete.
"""


def write_control_files(control_dir: Path, question: dict[str, Any]) -> dict[str, str]:
    control_dir.mkdir(parents=True)
    prompt_path = control_dir / "prompt.txt"
    prompt_path.write_text(prompt_for(question), encoding="utf-8")
    schema = {
        "type": "object",
        "additionalProperties": False,
        "required": ["answer", "summary", "evidence"],
        "properties": {
            "answer": {"type": "string"},
            "summary": {"type": "string"},
            "evidence": {
                "type": "array",
                "maxItems": 5,
                "items": {"type": "string"},
            },
        },
    }
    schema_path = control_dir / "final.schema.json"
    schema_path.write_text(json.dumps(schema, indent=2), encoding="utf-8")
    return {
        "promptSha256": sha256_file(prompt_path),
        "schemaSha256": sha256_file(schema_path),
    }


def normalized_answer(value: Any) -> str:
    answer = str(value).strip()
    match = re.fullmatch(r"<answer>\s*(.*?)\s*</answer>", answer, re.DOTALL)
    return match.group(1).strip() if match else answer


def lenient_numeric_answer(value: str) -> float | None:
    match = re.fullmatch(
        r"\s*([+-]?(?:\d[\d,]*\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)\s*(?:%|[A-Za-z][A-Za-z0-9/^_-]*)?\s*",
        value,
    )
    if not match:
        return None
    try:
        return float(match.group(1).replace(",", ""))
    except ValueError:
        return None


def grade_deterministic(eval_mode: str, ideal: str, predicted: str) -> dict[str, Any]:
    predicted = normalized_answer(predicted)
    if eval_mode == "str_verifier":
        cleaned_predicted = re.sub(r"[^a-zA-Z0-9]", "", predicted).lower()
        cleaned_ideal = re.sub(r"[^a-zA-Z0-9]", "", ideal).lower()
        upstream_match = cleaned_predicted == cleaned_ideal
        ideal_number = lenient_numeric_answer(ideal)
        predicted_number = lenient_numeric_answer(predicted)
        normalization_collision = (
            upstream_match
            and ideal_number is not None
            and predicted_number is not None
            and ideal_number != predicted_number
        )
        return {
            "status": "metadata_verifier",
            "official": False,
            "correct": upstream_match and not normalization_collision,
            "mode": eval_mode,
            "upstreamNormalizedMatch": upstream_match,
            "normalizationCollision": normalization_collision,
        }
    if eval_mode == "range_verifier":
        lower, upper = ast.literal_eval(ideal)
        try:
            value = float(predicted)
        except ValueError:
            lenient_value = lenient_numeric_answer(predicted)
            return {
                "status": "metadata_verifier",
                "official": False,
                "correct": False,
                "mode": eval_mode,
                "reason": "answer is not a single number",
                "formatOnlyFailure": lenient_value is not None
                and lower <= lenient_value <= upper,
            }
        return {
            "status": "metadata_verifier",
            "official": False,
            "correct": lower <= value <= upper,
            "mode": eval_mode,
            "formatOnlyFailure": False,
            "percentScaleEquivalent": not lower <= value <= upper
            and (lower <= value * 100 <= upper or lower <= value / 100 <= upper),
        }
    return {
        "status": "requires_official_llm_grader",
        "official": False,
        "correct": None,
        "mode": eval_mode,
    }


def notebook_summary(path: Path) -> tuple[dict[str, Any], dict[str, int]]:
    notebook = json.loads(path.read_text(encoding="utf-8"))
    cells = notebook.get("cells")
    if notebook.get("nbformat") != 4 or not isinstance(cells, list) or not cells:
        raise ValueError("notebook.ipynb is not a non-empty nbformat 4 notebook")
    code_cells = [cell for cell in cells if cell.get("cell_type") == "code"]
    output_cells = [cell for cell in code_cells if cell.get("outputs")]
    error_outputs = [
        output
        for cell in code_cells
        for output in cell.get("outputs", [])
        if output.get("output_type") == "error"
    ]
    if not code_cells or not output_cells:
        raise ValueError("notebook.ipynb has no executed code-cell evidence")
    if error_outputs:
        raise ValueError("notebook.ipynb contains an execution error")
    return notebook, {
        "cells": len(cells),
        "codeCells": len(code_cells),
        "codeCellsWithOutput": len(output_cells),
    }


def notebook_answer_markers(notebook: dict[str, Any]) -> list[str]:
    markers: list[str] = []
    for cell in notebook.get("cells", []):
        if cell.get("cell_type") != "code":
            continue
        for output in cell.get("outputs", []):
            if output.get("output_type") != "stream":
                continue
            text = output.get("text", "")
            if isinstance(text, list):
                text = "".join(text)
            if not isinstance(text, str):
                continue
            for line in text.splitlines():
                if line.startswith("BIXBENCH_ANSWER="):
                    markers.append(
                        normalized_answer(line.removeprefix("BIXBENCH_ANSWER="))
                    )
    return markers
