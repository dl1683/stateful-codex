import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import scodex


class ProjectHomeTests(unittest.TestCase):
    def test_attached_cd_flags_route_native_home_to_the_selected_project(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            alpha, beta = base / "alpha", base / "beta"
            alpha.mkdir()
            beta.mkdir()
            for flag in ("-C../beta", "-C=../beta"):
                with (
                    self.subTest(flag=flag),
                    patch.dict(os.environ, {"STATEFUL_STORE_ROOT": ""}),
                    patch.object(scodex.Path, "cwd", return_value=alpha),
                    patch.object(scodex, "TEMPLATE_HOME", base / "empty-template"),
                    patch.object(scodex.subprocess, "call", return_value=0) as call,
                ):
                    self.assertEqual(scodex.launch(["exec", flag, "hello"]), 0)
                    self.assertEqual(
                        call.call_args.kwargs["env"]["CODEX_HOME"],
                        str(beta / ".stateful"),
                    )
                    self.assertEqual(
                        call.call_args.kwargs["env"]["CODEX_SQLITE_HOME"],
                        str(beta / ".stateful"),
                    )
            self.assertFalse((alpha / ".stateful").exists())

    def test_git_subdirectory_and_cd_select_one_project_home(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            subprocess.run(["git", "init", "-q", str(project)], check=True)
            nested = project / "nested"
            nested.mkdir()
            selected = scodex.selected_directory(
                ["exec", "-C", "nested", "hello"], project
            )
            self.assertEqual(scodex.project_directory(selected), project)
            self.assertEqual(scodex.store_directory(project, {}), project / ".stateful")
            self.assertEqual(
                scodex.selected_directory(
                    ["--cd=nested", "--", "-C", "missing"], project
                ),
                nested,
            )

    def test_fresh_home_seeds_configuration_and_login_but_never_knowledge(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            template = project / "template"
            template.mkdir()
            (template / "config.toml").write_text('[windows]\nsandbox = "elevated"\n')
            (template / "auth.json").write_text('{"fixture":"not-a-token"}')
            (template / "project_intelligence_1.sqlite").write_text("old knowledge")
            (template / "sessions").mkdir()
            root = scodex.store_directory(project, {"STATEFUL_STORE_ROOT": "custom"})
            scodex.prepare_home(root, template)
            self.assertEqual(
                sorted(p.name for p in root.iterdir()),
                [".gitignore", "README.md", "auth.json", "config.toml"],
            )
            self.assertEqual(
                (root / "auth.json").read_text(), '{"fixture":"not-a-token"}'
            )
            (root / "config.toml").write_text('model = "fixture"\n')
            scodex.prepare_home(root, template)
            self.assertEqual((root / "config.toml").read_text(), 'model = "fixture"\n')
            (root / ".gitignore").write_text("!auth.json\n")
            with self.assertRaises(ValueError):
                scodex.prepare_home(root, template)

    def test_two_plain_projects_launch_native_storage_with_distinct_homes(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            homes = []
            for name in ("alpha", "beta"):
                project = base / name
                project.mkdir()
                with (
                    patch.dict(
                        os.environ,
                        {"STATEFUL_STORE_ROOT": "", "CODEX_SQLITE_HOME": "old-shared"},
                    ),
                    patch.object(scodex.Path, "cwd", return_value=project),
                    patch.object(scodex, "TEMPLATE_HOME", base / "empty-template"),
                    patch.object(scodex.subprocess, "call", return_value=0) as call,
                ):
                    self.assertEqual(
                        scodex.launch(["exec", "--skip-git-repo-check", "hello"]), 0
                    )
                    args, kwargs = call.call_args
                    homes.append(kwargs["env"]["CODEX_HOME"])
                    self.assertEqual(
                        kwargs["env"]["CODEX_SQLITE_HOME"], str(project / ".stateful")
                    )
                    self.assertNotIn("OPENAI_API_KEY", kwargs["env"])
                    self.assertEqual(
                        args[0][1:3],
                        [
                            "-c",
                            f"sqlite_home={scodex.json.dumps(str(project / '.stateful'))}",
                        ],
                    )
                    self.assertEqual(
                        args[0][3:],
                        [
                            "-m",
                            "gpt-6-luna",
                            "--stateful",
                            "exec",
                            "--skip-git-repo-check",
                            "hello",
                        ],
                    )
            self.assertNotEqual(*homes)

    def test_old_sqlite_preference_does_not_rewrite_user_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / ".stateful"
            root.mkdir()
            text = 'sqlite_home = "old-shared"\n'
            (root / "config.toml").write_text(text)
            scodex.prepare_home(root, Path(directory) / "template")
            self.assertEqual((root / "config.toml").read_text(), text)

    def test_store_override_cannot_turn_the_project_or_an_ancestor_into_a_home(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            for root in (".", "..", project.anchor):
                with self.assertRaises(ValueError):
                    scodex.store_directory(project, {"STATEFUL_STORE_ROOT": root})


if __name__ == "__main__":
    unittest.main()
