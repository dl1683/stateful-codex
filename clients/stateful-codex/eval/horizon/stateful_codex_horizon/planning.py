"""Deterministic trace-chunk planning for the Horizon adapter."""

from dataclasses import dataclass


@dataclass(frozen=True)
class TraceChunk:
    """A contiguous, chronological portion of a JSONL prior-session trace."""

    index: int
    start_line: int
    end_line: int
    text: str


def plan_trace_chunks(
    raw_trace: str,
    *,
    max_chunks: int = 3,
    ingestion_budget_seconds: int = 360,
    session_budget_seconds: int = 120,
) -> list[TraceChunk]:
    """Split a trace into as many bounded-budget chronological chunks as fit.

    The budget is deliberately conservative: three 120-second ingestion slots
    leave more than half of Horizon's 900-second public-task timeout for the
    final task session. The trace is uploaded as files, so chunk size does not
    inflate the ingestion prompt itself.
    """
    if max_chunks < 1:
        raise ValueError("max_chunks must be positive")
    if ingestion_budget_seconds < 0:
        raise ValueError("ingestion_budget_seconds cannot be negative")
    if session_budget_seconds < 1:
        raise ValueError("session_budget_seconds must be positive")

    lines = [line for line in raw_trace.splitlines(keepends=True) if line.strip()]
    allowed_chunks = min(
        max_chunks,
        ingestion_budget_seconds // session_budget_seconds,
        len(lines),
    )
    if allowed_chunks == 0:
        return []

    chunks: list[TraceChunk] = []
    start = 0
    for index in range(allowed_chunks):
        remaining_lines = len(lines) - start
        remaining_chunks = allowed_chunks - index
        count = (remaining_lines + remaining_chunks - 1) // remaining_chunks
        end = start + count
        chunks.append(
            TraceChunk(
                index=index,
                start_line=start + 1,
                end_line=end,
                text="".join(lines[start:end]),
            )
        )
        start = end
    return chunks
