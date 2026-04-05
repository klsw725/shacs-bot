from __future__ import annotations

import asyncio
import json
import inspect
from pathlib import Path
from typing import cast

from shacs_bot.agent.loop import AgentLoop
from shacs_bot.evals.models import EvaluationCase, EvaluationResult, ToolEvent, TraceArtifact
from shacs_bot.evals.runner import EvaluationRunner, resolve_variant
from shacs_bot.evals.storage import EvaluationStorage


class _StubEvalLoop:
    def __init__(self, behavior: str = "response") -> None:
        self.model = "stub-model"
        self._provider_name = "stub-provider"
        self._behavior = behavior

    async def process_direct(
        self,
        content: str,
        session_key: str = "cli:direct",
        channel: str = "cli",
        chat_id: str = "direct",
        on_progress=None,
        observer=None,
        variant=None,
    ) -> str:
        del content, session_key, channel, chat_id, on_progress, variant

        if self._behavior == "error":
            raise RuntimeError("provider failed")
        if self._behavior == "timeout":
            await asyncio.sleep(0.05)
            return "late response"
        if self._behavior == "tool":
            if observer is not None:
                observer.on_tool_result("read", {"path": "AGENTS.md"}, "ok")
                observer.on_final("tool response", "stop")
            return "tool response"

        if observer is not None:
            observer.on_final("plain response", "stop")
        return "plain response"


def test_evaluation_storage_writes_snake_case_artifacts(tmp_path: Path) -> None:
    storage = EvaluationStorage(tmp_path)
    run_dir = storage.create_run_dir(run_id="eval-storage")

    result = EvaluationResult(
        case_id="case-1",
        variant="default",
        status="success",
        final_response="done",
        finish_reason="stop",
        tool_call_count=1,
        usage={"prompt_tokens": 10},
        trace_path="default/case-1.trace.json",
    )
    trace = TraceArtifact(
        model="stub-model",
        provider="stub-provider",
        finish_reason="stop",
        assistant_response="done",
        tool_events=[ToolEvent(name="read", arguments={"path": "AGENTS.md"}, result_preview="ok")],
    )

    result_path = storage.write_result(run_dir, "default", result)
    trace_path = storage.write_trace(run_dir, "default", "case-1", trace)

    result_payload = json.loads(result_path.read_text(encoding="utf-8"))
    trace_payload = json.loads(trace_path.read_text(encoding="utf-8"))

    assert "final_response" in result_payload
    assert "finish_reason" in result_payload
    assert "tool_call_count" in result_payload
    assert "trace_path" in result_payload
    assert "finalResponse" not in result_payload
    assert "tool_events" in trace_payload
    assert "assistant_response" in trace_payload
    assert "finish_reason" in trace_payload
    assert "toolEvents" not in trace_payload


async def test_evaluation_runner_separates_variants_and_marks_tool_use_failures(
    tmp_path: Path,
) -> None:
    runner = EvaluationRunner(
        agent_loop=cast(AgentLoop, cast(object, _StubEvalLoop())),
        storage=EvaluationStorage(tmp_path),
    )
    summary = await runner.run_cases(
        cases=[
            EvaluationCase(
                case_id="tool-use-miss",
                input="AGENTS.md를 읽고 요약해줘",
                expected_mode="tool_use",
            )
        ],
        variants=[resolve_variant("default"), resolve_variant("bootstrap-off")],
        run_id="variant-check",
    )

    assert [item.variant for item in summary.variants] == ["default", "bootstrap-off"]

    default_result = (
        tmp_path / "evals" / "runs" / "variant-check" / "default" / "tool-use-miss.result.json"
    )
    bootstrap_result = (
        tmp_path
        / "evals"
        / "runs"
        / "variant-check"
        / "bootstrap-off"
        / "tool-use-miss.result.json"
    )
    assert default_result.exists()
    assert bootstrap_result.exists()

    default_payload = json.loads(default_result.read_text(encoding="utf-8"))
    bootstrap_payload = json.loads(bootstrap_result.read_text(encoding="utf-8"))
    assert default_payload["status"] == "task_failure"
    assert bootstrap_payload["status"] == "task_failure"


async def test_evaluation_runner_records_provider_errors_as_infra_error(tmp_path: Path) -> None:
    runner = EvaluationRunner(
        agent_loop=cast(AgentLoop, cast(object, _StubEvalLoop("error"))),
        storage=EvaluationStorage(tmp_path),
    )
    await runner.run_cases(
        cases=[EvaluationCase(case_id="provider-error", input="안녕", expected_mode="response")],
        variants=[resolve_variant("default")],
        run_id="infra-error",
    )

    result_path = (
        tmp_path / "evals" / "runs" / "infra-error" / "default" / "provider-error.result.json"
    )
    payload = json.loads(result_path.read_text(encoding="utf-8"))
    assert payload["status"] == "infra_error"
    assert payload["error_message"] == "provider failed"


async def test_evaluation_runner_records_timeouts_as_infra_error(tmp_path: Path) -> None:
    runner = EvaluationRunner(
        agent_loop=cast(AgentLoop, cast(object, _StubEvalLoop("timeout"))),
        storage=EvaluationStorage(tmp_path),
    )
    await runner.run_cases(
        cases=[
            EvaluationCase(
                case_id="timeout-case",
                input="느린 요청",
                expected_mode="response",
                timeout_seconds=0,
            )
        ],
        variants=[resolve_variant("default")],
        run_id="timeout-error",
    )

    result_path = (
        tmp_path / "evals" / "runs" / "timeout-error" / "default" / "timeout-case.result.json"
    )
    payload = json.loads(result_path.read_text(encoding="utf-8"))
    assert payload["status"] == "infra_error"
    assert payload["error_message"] == "timeout after 0s"


def test_agent_loop_process_direct_keeps_optional_eval_args() -> None:
    params = inspect.signature(AgentLoop.process_direct).parameters
    assert params["observer"].default is None
    assert params["variant"].default is None
