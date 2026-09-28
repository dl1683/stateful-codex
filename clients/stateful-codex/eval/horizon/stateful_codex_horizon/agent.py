"""Stateful Codex adapter for the Horizon long-horizon learning benchmark."""

import json
import shlex
import tempfile
import time
from pathlib import Path, PurePosixPath
from typing import Any, override

from agent_utils import read_trace_file
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from stateful_harbor.stateful_codex import StatefulCodex
from stateful_codex_horizon.planning import TraceChunk, plan_trace_chunks


TRACE_PATH = "/workdir/trace.jsonl"
HORIZON_TIMEOUT_SECONDS = 900
INGESTION_BUDGET_SECONDS = 360
INGESTION_SESSION_BUDGET_SECONDS = 120
MAX_INGESTION_CHUNKS = 3
REMOTE_INGESTION_ROOT = PurePosixPath("/tmp/stateful-horizon-ingestion")


class StatefulCodexHorizon(StatefulCodex):
    """Run trace learning sessions before a fresh Horizon task session."""

    @staticmethod
    @override
    def name() -> str:
        return "stateful-codex-horizon"

    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        if self._prompt_template_path is None:
            raise ValueError(
                "StatefulCodexHorizon requires Horizon's official prompt template; "
                "pass --ak prompt_template_path=agents/claude_code_horizon_prompt.j2"
            )

        started = time.monotonic()
        trace = await read_trace_file(environment, TRACE_PATH)
        chunks = plan_trace_chunks(
            trace,
            max_chunks=MAX_INGESTION_CHUNKS,
            ingestion_budget_seconds=INGESTION_BUDGET_SECONDS,
            session_budget_seconds=INGESTION_SESSION_BUDGET_SECONDS,
        )
        phases: list[dict[str, Any]] = []
        await self.exec_as_agent(
            environment,
            f"mkdir -p {shlex.quote(REMOTE_INGESTION_ROOT.as_posix())} "
            f"{shlex.quote((self.environment_logs_dir / 'phases').as_posix())}",
        )

        for chunk in chunks:
            if time.monotonic() - started >= INGESTION_BUDGET_SECONDS:
                break
            phase = await self._run_ingestion_phase(environment, context, chunk, len(chunks))
            phases.append(phase)

        task_phase = await self._run_task_phase(instruction, environment, context)
        phases.append(task_phase)
        await self._write_run_metadata(
            environment,
            phases=phases,
            total_seconds=time.monotonic() - started,
            trace_lines=len(trace.splitlines()),
            planned_chunks=len(chunks),
        )
        if task_phase["status"] == "failed":
            raise RuntimeError(task_phase.get("error", "Horizon task phase failed"))

    async def _run_ingestion_phase(
        self,
        environment: BaseEnvironment,
        context: AgentContext,
        chunk: TraceChunk,
        chunk_count: int,
    ) -> dict[str, Any]:
        remote_path = REMOTE_INGESTION_ROOT / f"chunk-{chunk.index:03d}.jsonl"
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", suffix=".jsonl", delete=False
        ) as handle:
            handle.write(chunk.text)
            local_path = Path(handle.name)
        try:
            await environment.upload_file(local_path, remote_path.as_posix())
        finally:
            local_path.unlink(missing_ok=True)
        await self.exec_as_root(
            environment,
            f"chmod 0644 {shlex.quote(remote_path.as_posix())}",
        )

        prompt = (
            "This is ingestion phase "
            f"{chunk.index + 1} of {chunk_count} for a Horizon task. "
            f"Read the chronological JSONL trace chunk at {remote_path}. "
            "Do not act on the live Horizon task or send messages in this phase. "
            "Extract durable, reusable project learnings: people and identities, "
            "preferences, commitments, conventions, broken tools and reliable "
            "workarounds, and important unresolved uncertainty. Record those "
            "learnings in Stateful Codex project state with appropriate scope, "
            "provenance, and uncertainty. Avoid recording transient narration or "
            "guesses. Read the whole chunk as needed, then finish after persisting "
            "the useful learnings."
        )
        return await self._run_phase(
            environment,
            context,
            phase_name=f"ingestion-{chunk.index:03d}",
            prompt=prompt,
            chunk={
                "index": chunk.index,
                "start_line": chunk.start_line,
                "end_line": chunk.end_line,
                "path": remote_path.as_posix(),
            },
            apply_template=False,
        )

    async def _run_task_phase(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> dict[str, Any]:
        return await self._run_phase(
            environment,
            context,
            phase_name="task",
            prompt=instruction,
            chunk=None,
            apply_template=True,
        )

    async def _run_phase(
        self,
        environment: BaseEnvironment,
        context: AgentContext,
        *,
        phase_name: str,
        prompt: str,
        chunk: dict[str, Any] | None,
        apply_template: bool,
    ) -> dict[str, Any]:
        started = time.monotonic()
        status = "completed"
        error: str | None = None
        original_template = self._prompt_template_path
        if not apply_template:
            self._prompt_template_path = None
        try:
            await super().run(prompt, environment, context)
        except Exception as exc:
            status = "failed"
            error = f"{type(exc).__name__}: {exc}"
            self.logger.exception("Horizon phase failed: %s", phase_name)
        finally:
            self._prompt_template_path = original_template

        duration = time.monotonic() - started
        usage = await self._read_phase_usage(environment)
        record: dict[str, Any] = {
            "name": phase_name,
            "status": status,
            "duration_seconds": round(duration, 3),
            "token_usage": usage,
        }
        if chunk is not None:
            record["trace_chunk"] = chunk
        if error is not None:
            record["error"] = error
        await self._snapshot_phase(environment, phase_name, record)
        return record

    async def _read_phase_usage(self, environment: BaseEnvironment) -> dict[str, int] | None:
        output_path = self.environment_logs_dir / self._OUTPUT_FILENAME
        result = await self.exec_as_agent(
            environment,
            f"grep -F '\"type\":\"token_count\"' "
            f"{shlex.quote(output_path.as_posix())} | tail -n 1 || true",
        )
        for line in reversed((result.stdout or "").splitlines()):
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            info = ((event.get("payload") or {}).get("info") or {})
            usage = info.get("total_token_usage")
            if not isinstance(usage, dict):
                continue
            fields = {
                "input_tokens": usage.get("input_tokens"),
                "output_tokens": usage.get("output_tokens"),
                "reasoning_output_tokens": usage.get("reasoning_output_tokens"),
                "cached_input_tokens": usage.get("cached_input_tokens"),
                "total_tokens": usage.get("total_tokens"),
            }
            return {key: int(value) for key, value in fields.items() if value is not None}
        return None

    async def _snapshot_phase(
        self,
        environment: BaseEnvironment,
        phase_name: str,
        record: dict[str, Any],
    ) -> None:
        root = self.environment_logs_dir
        phase_dir = root / "phases" / phase_name
        metadata = shlex.quote(json.dumps(record, sort_keys=True))
        command = (
            f"mkdir -p {shlex.quote(phase_dir.as_posix())}; "
            f"for file in codex.txt trajectory.json adapter.json codex-package.json "
            f"final.patch git-state.txt stateful-state.sha256; do "
            f"if test -e {shlex.quote(root.as_posix())}/$file; then "
            f"cp -a {shlex.quote(root.as_posix())}/$file {shlex.quote(phase_dir.as_posix())}/; "
            "fi; done; "
            f"if test -d {shlex.quote(root.as_posix())}/sessions; then "
            f"cp -a {shlex.quote(root.as_posix())}/sessions {shlex.quote(phase_dir.as_posix())}/; "
            "fi; "
            f"if test -d {shlex.quote(root.as_posix())}/stateful-state; then "
            f"cp -a {shlex.quote(root.as_posix())}/stateful-state "
            f"{shlex.quote(phase_dir.as_posix())}/; fi; "
            f"printf '%s\\n' {metadata} > {shlex.quote((phase_dir / 'phase.json').as_posix())}"
        )
        try:
            await self.exec_as_agent(environment, command)
        except Exception:
            self.logger.exception("Failed to snapshot Horizon phase: %s", phase_name)

    async def _write_run_metadata(
        self,
        environment: BaseEnvironment,
        *,
        phases: list[dict[str, Any]],
        total_seconds: float,
        trace_lines: int,
        planned_chunks: int,
    ) -> None:
        metadata = {
            "adapter": self.name(),
            "horizon_timeout_seconds": HORIZON_TIMEOUT_SECONDS,
            "ingestion_budget_seconds": INGESTION_BUDGET_SECONDS,
            "planned_ingestion_chunks": planned_chunks,
            "trace_lines": trace_lines,
            "total_duration_seconds": round(total_seconds, 3),
            "phases": phases,
        }
        command = (
            f"printf '%s\\n' {shlex.quote(json.dumps(metadata, sort_keys=True))} > "
            f"{shlex.quote((self.environment_logs_dir / 'adapter.json').as_posix())}"
        )
        await self.exec_as_agent(environment, command)
