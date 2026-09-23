"""Matched Harbor agents for the branch Codex binary.

Both agents install the same content-addressed Linux bundle and use Harbor's
native Codex trajectory conversion. StatefulCodex changes only the explicit
Stateful workflow selection.
"""

import hashlib
import json
import os
import re
import shlex
import time
from collections.abc import Mapping
from pathlib import Path, PurePosixPath
from typing import Any, ClassVar, override

from harbor.agents.installed.codex import Codex
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.trajectories import Trajectory
from harbor.utils.env import parse_bool_env_value


HARBOR_COMMIT = "15da91c18580a25489f5bdf2ee71029f3ff3bb2e"
_SHA256_RE = re.compile(r"^[0-9a-fA-F]{64}$")


class BundledCodex(Codex):
    """Ordinary Codex from the exact bundle used by the Stateful arm."""

    _REMOTE_ARCHIVE = PurePosixPath("/tmp/stateful-codex-bundle.tar.gz")
    _REMOTE_PACKAGE = PurePosixPath("/opt/stateful-codex")
    _REMOTE_STATE_HOME = PurePosixPath("/tmp/stateful-codex-state")
    _REMOTE_BASE_COMMIT = PurePosixPath("/tmp/stateful-codex-base-commit")
    stateful_mode: ClassVar[str | None] = None

    @staticmethod
    @override
    def name() -> str:
        return "bundled-codex"

    @classmethod
    @override
    def preflight(
        cls,
        kwargs: dict[str, Any] | None = None,
        env: Mapping[str, str] | None = None,
    ) -> None:
        super().preflight(kwargs, env)
        cls._bundle_settings(env, verify_content=True)
        cls._auth_path(env)

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self._bundle_path, self._bundle_sha256 = self._bundle_settings(
            self.extra_env,
            verify_content=True,
        )

    @classmethod
    def _env_value(cls, env: Mapping[str, str] | None, key: str) -> str | None:
        if env is not None and key in env:
            return env[key]
        return os.environ.get(key)

    @classmethod
    def _bundle_settings(
        cls,
        env: Mapping[str, str] | None,
        *,
        verify_content: bool,
    ) -> tuple[Path, str]:
        raw_path = cls._env_value(env, "STATEFUL_CODEX_BUNDLE_PATH")
        expected = cls._env_value(env, "STATEFUL_CODEX_BUNDLE_SHA256")
        if not raw_path or not expected:
            raise ValueError(
                "STATEFUL_CODEX_BUNDLE_PATH and STATEFUL_CODEX_BUNDLE_SHA256 "
                "are required for a reproducible matched run"
            )
        bundle_path = Path(raw_path).expanduser().resolve()
        if not bundle_path.is_file():
            raise ValueError("STATEFUL_CODEX_BUNDLE_PATH is not a file")
        if not _SHA256_RE.fullmatch(expected):
            raise ValueError(
                "STATEFUL_CODEX_BUNDLE_SHA256 must be 64 hexadecimal characters"
            )
        expected = expected.lower()
        if verify_content:
            with bundle_path.open("rb") as bundle:
                actual = hashlib.file_digest(bundle, "sha256").hexdigest()
            if actual != expected:
                raise ValueError(
                    "STATEFUL_CODEX_BUNDLE_SHA256 does not match the selected bundle"
                )
        return bundle_path, expected

    @classmethod
    def _auth_path(cls, env: Mapping[str, str] | None) -> Path:
        raw_path = cls._env_value(env, "CODEX_AUTH_JSON_PATH")
        if raw_path:
            auth_path = Path(raw_path).expanduser()
        elif parse_bool_env_value(
            cls._env_value(env, "CODEX_FORCE_AUTH_JSON"),
            name="CODEX_FORCE_AUTH_JSON",
            default=False,
        ):
            auth_path = Path.home() / ".codex" / "auth.json"
        else:
            raise ValueError(
                "CODEX_AUTH_JSON_PATH or CODEX_FORCE_AUTH_JSON is required; "
                "the matched benchmark intentionally uses the Codex login, not an API key"
            )
        if not auth_path.is_file():
            raise ValueError("the configured Codex auth.json file does not exist")
        return auth_path

    @override
    def _resolve_auth_json_path(self) -> Path | None:
        return self._auth_path(self.extra_env)

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await self.ensure_system_dependencies(
            environment,
            ("bash", "ca_certificates", "coreutils", "ripgrep", "tar"),
        )
        await environment.upload_file(
            self._bundle_path, self._REMOTE_ARCHIVE.as_posix()
        )
        archive = shlex.quote(self._REMOTE_ARCHIVE.as_posix())
        package = shlex.quote(self._REMOTE_PACKAGE.as_posix())
        expected = shlex.quote(self._bundle_sha256)
        await self.exec_as_root(
            environment,
            command=(
                "set -euo pipefail; "
                f"printf '%s  %s\\n' {expected} {archive} | sha256sum -c -; "
                f"install -d -m 0755 {package}; "
                f"tar -xzf {archive} -C {package} --no-same-owner; "
                f"test -x {package}/bin/codex; "
                f"test -x {package}/bin/codex-code-mode-host; "
                f"ln -sfn {package}/bin/codex /usr/local/bin/codex; "
                f"ln -sfn {package}/bin/codex-code-mode-host "
                "/usr/local/bin/codex-code-mode-host; "
                "codex --version"
            ),
        )

    @override
    def _build_effective_config(
        self,
        openai_base_url: str | None = None,
    ) -> dict[str, Any]:
        config = super()._build_effective_config(openai_base_url)
        configured_home = config.get("sqlite_home")
        state_home = self._REMOTE_STATE_HOME.as_posix()
        if configured_home not in (None, state_home):
            raise ValueError(
                "the benchmark owns sqlite_home so state remains isolated to one trial"
            )
        config["sqlite_home"] = state_home
        return config

    @override
    def build_cli_flags(self) -> str:
        flags = super().build_cli_flags()
        if self.stateful_mode is not None:
            flags = f"{flags} --stateful {self.stateful_mode}".strip()
        return flags

    @override
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        base_commit = shlex.quote(self._REMOTE_BASE_COMMIT.as_posix())
        try:
            await self.exec_as_agent(
                environment,
                command=(
                    "if git -C /app rev-parse --is-inside-work-tree "
                    ">/dev/null 2>&1; then "
                    f"git -C /app rev-parse HEAD > {base_commit}; "
                    "fi"
                ),
            )
        except Exception:
            self.logger.exception("Failed to record the benchmark base commit")
        started = time.monotonic()
        outcome = "completed"
        try:
            await super().run(instruction, environment, context)
        except BaseException:
            outcome = "failed"
            raise
        finally:
            await self._capture_attempt_artifacts(
                environment,
                outcome=outcome,
                duration_seconds=time.monotonic() - started,
            )

    async def _capture_attempt_artifacts(
        self,
        environment: BaseEnvironment,
        *,
        outcome: str,
        duration_seconds: float,
    ) -> None:
        metadata = json.dumps(
            {
                "adapter": self.name(),
                "bundleSha256": self._bundle_sha256,
                "durationSeconds": round(duration_seconds, 6),
                "harborCommit": HARBOR_COMMIT,
                "outcome": outcome,
                "statefulMode": self.stateful_mode,
            },
            sort_keys=True,
        )
        logs = shlex.quote(self.environment_logs_dir.as_posix())
        state_home = shlex.quote(self._REMOTE_STATE_HOME.as_posix())
        base_commit = shlex.quote(self._REMOTE_BASE_COMMIT.as_posix())
        try:
            await self.exec_as_agent(
                environment,
                command=(
                    f"mkdir -p {logs}; "
                    f"printf '%s\\n' {shlex.quote(metadata)} > {logs}/adapter.json; "
                    f"if test -d {state_home}; then "
                    f"rm -rf {logs}/stateful-state; "
                    f"mkdir -p {logs}/stateful-state; "
                    f"cp -a {state_home}/. {logs}/stateful-state/; "
                    f"(cd {logs}/stateful-state && "
                    "find . -type f -print0 | sort -z | "
                    f"xargs -0 -r sha256sum) > {logs}/stateful-state.sha256; "
                    "fi; "
                    f": > {logs}/final.patch; "
                    "if command -v git >/dev/null 2>&1 && "
                    "git -C /app rev-parse --is-inside-work-tree >/dev/null 2>&1; then "
                    f"if test -s {base_commit}; then "
                    f"git -C /app diff --binary --no-ext-diff "
                    f'"$(cat {base_commit})"..HEAD >> {logs}/final.patch; '
                    "fi; "
                    f"git -C /app diff --binary --no-ext-diff HEAD >> {logs}/final.patch; "
                    "git -C /app ls-files --others --exclude-standard | "
                    "while IFS= read -r file; do "
                    'git -C /app diff --binary --no-index -- /dev/null "$file" '
                    f">> {logs}/final.patch || test $? -eq 1; "
                    "done; "
                    "fi"
                ),
            )
        except Exception:
            self.logger.exception("Failed to preserve benchmark attempt metadata")

    @override
    def convert_trajectory(self, logs_dir: Path) -> Trajectory | None:
        trajectory = super().convert_trajectory(logs_dir)
        if trajectory is None or trajectory.agent is None:
            return trajectory
        trajectory.agent.name = self.name()
        extra = dict(trajectory.agent.extra or {})
        extra.update(
            {
                "bundleSha256": self._bundle_sha256,
                "harborCommit": HARBOR_COMMIT,
                "statefulMode": self.stateful_mode,
            }
        )
        trajectory.agent.extra = extra
        return trajectory


class StatefulCodex(BundledCodex):
    """The matched bundle with autonomous Stateful project intelligence enabled."""

    stateful_mode = "autonomous"

    @staticmethod
    @override
    def name() -> str:
        return "stateful-codex"
