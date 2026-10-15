import ast
import os
import runpy
from pathlib import Path

import pytest


@pytest.mark.parametrize("inherited_path", ["", "missing formatter directory"])
def test_codegen_finds_formatter_in_selected_python_environment(
    monkeypatch, tmp_path, inherited_path
):
    script = Path(__file__).resolve().parents[1] / "scripts" / "update_sdk_artifacts.py"
    workflow = runpy.run_path(str(script))
    schema = tmp_path / "input.json"
    schema.write_text(
        '{"title":"Widget","type":"object",'
        '"properties":{"name":{"type":"string"}},"required":["name"]}'
    )
    output = tmp_path / "generated.py"
    monkeypatch.setenv("PATH", inherited_path)

    workflow["run_python_module"](
        "datamodel_code_generator",
        [
            "--input",
            str(schema),
            "--input-file-type",
            "jsonschema",
            "--output",
            str(output),
            "--output-model-type",
            "pydantic_v2.BaseModel",
            "--target-python-version",
            "3.11",
            "--disable-timestamp",
            "--formatters",
            "ruff-format",
        ],
        cwd=tmp_path,
    )

    model = next(
        node for node in ast.parse(output.read_text()).body if isinstance(node, ast.ClassDef)
    )
    assert model.name == "Widget"
    fields = [node.target.id for node in model.body if isinstance(node, ast.AnnAssign)]
    assert fields == ["name"]
    assert os.environ["PATH"] == inherited_path
