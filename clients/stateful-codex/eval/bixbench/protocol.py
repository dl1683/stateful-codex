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


def normalized_answer(value: Any) -> str:
    answer = str(value).strip()
    match = re.fullmatch(r"<answer>\s*(.*?)\s*</answer>", answer, re.DOTALL)
    return match.group(1).strip() if match else answer


def grade_deterministic(eval_mode: str, ideal: str, predicted: str) -> dict[str, Any]:
    predicted = normalized_answer(predicted)
    if eval_mode == "str_verifier":
        clean = lambda value: re.sub(r"[^a-zA-Z0-9]", "", value).lower()
        correct = clean(predicted) == clean(ideal)
        return {"status": "graded", "correct": correct, "mode": eval_mode}
    if eval_mode == "range_verifier":
        lower, upper = ast.literal_eval(ideal)
        try:
            value = float(predicted)
        except ValueError:
            return {
                "status": "graded",
                "correct": False,
                "mode": eval_mode,
                "reason": "answer is not a single number",
            }
        return {
            "status": "graded",
            "correct": lower <= value <= upper,
            "mode": eval_mode,
        }
    return {
        "status": "requires_llm_grader",
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
