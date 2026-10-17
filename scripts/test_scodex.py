import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import scodex


class ProjectHomeTests(unittest.TestCase):
    def test_executables_require_absolute_path_entries_outside_the_project(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            trusted = root / "trusted"
            trusted.mkdir()
            with (
                patch.object(scodex.Path, "cwd", return_value=root),
                patch.object(
                    scodex.os,
                    "get_exec_path",
                    return_value=["", ".", str(root), str(trusted)],
                ),
                patch.object(
                    scodex.shutil, "which", return_value=str(trusted / "git.exe")
                ) as which,
            ):
                self.assertEqual(scodex.path_command("git"), str(trusted / "git.exe"))
                which.assert_called_once_with(
                    str(trusted / ("git.exe" if os.name == "nt" else "git"))
                )

    def test_attached_cd_flags_route_native_home_to_the_selected_project(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            alpha, beta = base / "alpha", base / "beta"
            alpha.mkdir()
            beta.mkdir()
            for command in (
                "exec -C../beta hello",
                "exec -C=../beta hello",
                "--stateful -C ../beta exec hello",
                "--stateful ask --cd=../beta exec hello",
            ):
                with (
                    self.subTest(command=command),
                    patch.dict(os.environ, {"STATEFUL_STORE_ROOT": ""}),
                    patch.object(scodex.Path, "cwd", return_value=alpha),
                    patch.object(scodex, "TEMPLATE_HOME", base / "empty-template"),
                    patch.object(scodex.subprocess, "call", return_value=0) as call,
                ):
                    self.assertEqual(scodex.launch(command.split()), 0)
                    env = call.call_args.kwargs["env"]
                    self.assertEqual(
                        [env[key] for key in ("CODEX_HOME", "CODEX_SQLITE_HOME")],
                        [str(beta / ".stateful")] * 2,
                    )
            self.assertFalse((alpha / ".stateful").exists())

    def test_git_subdirectory_and_cd_select_one_project_home(self):
        with tempfile.TemporaryDirectory() as directory:
            project = (Path(directory) / "भारत-🙂\N{NO-BREAK SPACE}").resolve()
            project.mkdir()
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

    def test_memory_words_are_not_directory_flags(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            for command in (
                "memory --thread fixture add rule Use -C missing",
                "exec memory --thread fixture correct entry@1 Use --cd=missing",
                "memory --thread fixture add decision --reason sample Use -Cmissing",
            ):
                self.assertEqual(
                    scodex.selected_directory(command.split(), project), project
                )
            self.assertEqual(
                scodex.selected_directory(["-m", "memory", "-C", directory], project),
                project,
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
                [".gitignore", ".ignore", "README.md", "auth.json", "config.toml"],
            )
            self.assertEqual(
                (root / "auth.json").read_text(), '{"fixture":"not-a-token"}'
            )
            (root / "config.toml").write_text('model = "fixture"\n')
            scodex.prepare_home(root, template)
            self.assertEqual((root / "config.toml").read_text(), 'model = "fixture"\n')
            for name in (".gitignore", ".ignore"):
                with self.subTest(policy=name):
                    (root / name).write_text("!auth.json\n")
                    with self.assertRaises(ValueError):
                        scodex.prepare_home(root, template)
                    (root / name).write_text("*\n")

    def test_two_plain_projects_launch_native_storage_with_distinct_homes(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            homes = []
            for name, model in (
                ("alpha-नमुना-🙂", []),
                ("beta", ["--model", "gpt-6-luna"]),
                ("gamma", ["-mgpt-6-luna"]),
            ):
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
                    command = [*model, "exec", "--skip-git-repo-check", "hello"]
                    self.assertEqual(scodex.launch(command), 0)
                    args, kwargs = call.call_args
                    homes.append(kwargs["env"]["CODEX_HOME"])
                    self.assertEqual(
                        kwargs["env"]["CODEX_SQLITE_HOME"], str(project / ".stateful")
                    )
                    self.assertNotIn("OPENAI_API_KEY", kwargs["env"])
                    if sys.version_info >= (3, 11):
                        import tomllib

                        self.assertEqual(
                            tomllib.loads(args[0][2])["sqlite_home"],
                            str(project / ".stateful"),
                        )
                    quoted = scodex.json.dumps(
                        str(project / ".stateful"), ensure_ascii=False
                    )
                    self.assertEqual(
                        args[0][1:],
                        ["-c", f"sqlite_home={quoted}"]
                        + ([] if model else ["-m", "gpt-6-luna"])
                        + ["--stateful=ask", *command],
                    )
            self.assertEqual(len(set(homes)), 3)

    def test_existing_ignore_policy_is_untouched_on_restart(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / ".stateful"
            scodex.prepare_home(root, Path(directory) / "template")
            policies = [root / name for name in (".gitignore", ".ignore")]
            for policy in policies:
                os.utime(policy, (1, 1))
            before = [
                (p.read_bytes(), p.stat().st_ino, p.stat().st_mtime_ns)
                for p in policies
            ]
            scodex.prepare_home(root, Path(directory) / "template")
            self.assertEqual(
                [
                    (p.read_bytes(), p.stat().st_ino, p.stat().st_mtime_ns)
                    for p in policies
                ],
                before,
            )

    def test_atomic_ignore_publication_preserves_a_concurrent_winner(self):
        real_link = os.link
        for name, policy in (
            (name, policy)
            for name in (".gitignore", ".ignore")
            for policy in (b"*\n", b"!auth.json\n")
        ):
            with (
                self.subTest(name=name, policy=policy),
                tempfile.TemporaryDirectory() as directory,
            ):
                root, template = (
                    Path(directory) / ".stateful",
                    Path(directory) / "template",
                )
                template.mkdir()
                (template / "auth.json").write_bytes(b"fixture-not-a-token")
                ignore = root / name

                def concurrent_link(source, destination):
                    if destination == ignore:
                        self.assertEqual(Path(source).read_bytes(), b"*\n")
                        self.assertFalse((root / "auth.json").exists())
                        ignore.write_bytes(policy)
                        raise FileExistsError()
                    return real_link(source, destination)

                with patch.object(scodex.os, "link", side_effect=concurrent_link):
                    if policy == b"*\n":
                        scodex.prepare_home(root, template)
                    else:
                        with self.assertRaises(ValueError):
                            scodex.prepare_home(root, template)
                self.assertEqual(ignore.read_bytes(), policy)
                self.assertEqual((root / "auth.json").exists(), policy == b"*\n")

    def test_store_override_cannot_turn_the_project_or_an_ancestor_into_a_home(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            for root in (".", "..", project.anchor):
                with self.assertRaises(ValueError):
                    scodex.store_directory(project, {"STATEFUL_STORE_ROOT": root})


if __name__ == "__main__":
    unittest.main()
