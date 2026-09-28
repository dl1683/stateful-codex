import pytest

from stateful_codex_horizon.planning import plan_trace_chunks


def test_plan_preserves_order_and_balances_lines() -> None:
    trace = "\n".join(f"{{\"n\": {index}}}" for index in range(7))

    chunks = plan_trace_chunks(trace, max_chunks=3)

    assert [chunk.index for chunk in chunks] == [0, 1, 2]
    assert [(chunk.start_line, chunk.end_line) for chunk in chunks] == [
        (1, 3),
        (4, 5),
        (6, 7),
    ]
    assert "\n".join(chunk.text.strip() for chunk in chunks).replace(" ", "") == (
        "{\"n\":0}\n{\"n\":1}\n{\"n\":2}\n"
        "{\"n\":3}\n{\"n\":4}\n{\"n\":5}\n{\"n\":6}"
    )


def test_budget_can_reduce_the_number_of_sessions() -> None:
    trace = "a\nb\nc\nd"

    chunks = plan_trace_chunks(
        trace,
        max_chunks=3,
        ingestion_budget_seconds=239,
        session_budget_seconds=120,
    )

    assert len(chunks) == 1
    assert chunks[0].text == "a\nb\nc\nd"


def test_empty_trace_and_invalid_limits() -> None:
    assert plan_trace_chunks("") == []
    with pytest.raises(ValueError):
        plan_trace_chunks("x", max_chunks=0)
    with pytest.raises(ValueError):
        plan_trace_chunks("x", session_budget_seconds=0)
