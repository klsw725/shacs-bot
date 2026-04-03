from __future__ import annotations

import asyncio
import json
import re
from collections import defaultdict
from collections.abc import Mapping
from datetime import datetime
from pathlib import Path
from typing import cast, override

from shacs_bot.agent.context import ContextVariant
from shacs_bot.agent.loop import AgentLoop, AgentLoopObserver
from shacs_bot.agent.session.manager import SessionManager
from shacs_bot.evals.models import (
    EvalStatus,
    EvaluationCase,
    EvaluationResult,
    HarnessVariant,
    RunManifest,
    RunSummary,
    ToolEvent,
    TraceArtifact,
    VariantSummary,
)
from shacs_bot.evals.storage import EvaluationStorage
from shacs_bot.providers.base import LLMResponse


def get_default_cases_path(workspace: Path) -> Path:
    return workspace / "evals" / "cases" / "default.json"


def load_cases_file(path: Path) -> list[EvaluationCase]:
    try:
        raw_payload: object = cast(object, json.loads(path.read_text(encoding="utf-8")))
    except OSError as exc:
        raise ValueError(f"failed to read cases file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise ValueError(f"failed to parse cases json: {path}") from exc

    if not isinstance(raw_payload, dict):
        raise ValueError("cases file root must be an object")

    payload: dict[str, object] = cast(dict[str, object], raw_payload)
    raw_cases: object = payload.get("cases")
    if not isinstance(raw_cases, list):
        raise ValueError("cases file must contain a 'cases' array")

    parsed_cases: list[object] = cast(list[object], raw_cases)
    try:
        validated_cases: list[EvaluationCase] = []
        for candidate in parsed_cases:
            validated_cases.append(EvaluationCase.model_validate(candidate))
        return validated_cases
    except Exception as exc:
        raise ValueError("failed to validate cases payload") from exc


def resolve_variant(name: str) -> HarnessVariant:
    presets: dict[str, HarnessVariant] = {
        "default": HarnessVariant(name="default"),
        "bootstrap-off": HarnessVariant(name="bootstrap-off", environment_bootstrap=False),
        "minimal-context": HarnessVariant(name="minimal-context", context_profile="minimal"),
        "strict-completion": HarnessVariant(name="strict-completion", completion_policy="strict"),
    }
    if name not in presets:
        raise ValueError(f"unknown variant preset: {name}")
    return presets[name]


def build_variant_summary(variant: str, results: list[EvaluationResult]) -> VariantSummary:
    total: int = len(results)
    prompt_tokens: int = sum(result.usage.get("prompt_tokens", 0) for result in results)
    completion_tokens: int = sum(result.usage.get("completion_tokens", 0) for result in results)
    tool_call_total: int = sum(result.tool_call_count for result in results)
    return VariantSummary(
        variant=variant,
        total=total,
        success=sum(1 for result in results if result.status == "success"),
        task_failure=sum(1 for result in results if result.status == "task_failure"),
        infra_error=sum(1 for result in results if result.status == "infra_error"),
        avg_tool_calls=(tool_call_total / total) if total else 0.0,
        prompt_tokens=prompt_tokens,
        completion_tokens=completion_tokens,
    )


class TraceCollector(AgentLoopObserver):
    def __init__(self, model: str, provider: str) -> None:
        self._model: str = model
        self._provider: str = provider
        self._finish_reason: str = ""
        self._response: str = ""
        self._tool_events: list[ToolEvent] = []
        self._usage: dict[str, int] = {}

    @override
    def on_llm_response(self, response: LLMResponse) -> None:
        self._finish_reason = response.finish_reason
        if response.content:
            self._response = response.content
        for key, value in (response.usage or {}).items():
            self._usage[key] = self._usage.get(key, 0) + value

    @override
    def on_tool_result(self, tool_name: str, arguments: Mapping[str, object], result: str) -> None:
        preview: str = result[:200] + ("..." if len(result) > 200 else "")
        lowered: str = result.lower()
        self._tool_events.append(
            ToolEvent(
                name=tool_name,
                arguments=dict(arguments),
                result_preview=preview,
                is_error=lowered.startswith("error") or lowered.startswith("failed"),
            )
        )

    @override
    def on_final(self, final_content: str | None, finish_reason: str) -> None:
        self._finish_reason = finish_reason
        if final_content is not None:
            self._response = final_content

    @property
    def finish_reason(self) -> str:
        return self._finish_reason

    @property
    def response(self) -> str:
        return self._response

    @property
    def tool_events(self) -> list[ToolEvent]:
        return list(self._tool_events)

    @property
    def usage(self) -> dict[str, int]:
        return dict(self._usage)

    def build_trace(self, started_at: str, completed_at: str) -> TraceArtifact:
        return TraceArtifact(
            model=self._model,
            provider=self._provider,
            finish_reason=self._finish_reason,
            assistant_response=self._response,
            tool_events=self.tool_events,
            usage=self.usage,
            started_at=started_at,
            completed_at=completed_at,
        )


class EvaluationRunner:
    def __init__(
        self,
        agent_loop: AgentLoop,
        storage: EvaluationStorage,
        session_manager: SessionManager | None = None,
    ) -> None:
        self._agent_loop: AgentLoop = agent_loop
        self._storage: EvaluationStorage = storage
        self._session_manager: SessionManager | None = session_manager
        self._last_run_dir: Path | None = None

    @property
    def last_run_dir(self) -> Path | None:
        return self._last_run_dir

    async def run_cases(
        self,
        cases: list[EvaluationCase],
        variants: list[HarnessVariant],
        output_dir: Path | None = None,
        cases_file: Path | None = None,
        run_id: str | None = None,
    ) -> RunSummary:
        selected_variants: list[HarnessVariant] = variants or [resolve_variant("default")]
        run_dir: Path = self._storage.create_run_dir(output_dir, run_id=run_id)
        self._last_run_dir = run_dir
        manifest = RunManifest(
            run_id=run_dir.name,
            cases_file=str(cases_file.resolve()) if cases_file else "",
            variants=[variant.name for variant in selected_variants],
            created_at=datetime.now().astimezone().isoformat(),
        )
        _ = self._storage.write_manifest(run_dir, manifest)

        grouped_results: dict[str, list[EvaluationResult]] = defaultdict(list)
        for variant in selected_variants:
            for case in cases:
                result = await self._run_case(case=case, variant=variant, run_dir=run_dir)
                grouped_results[variant.name].append(result)

        summary = RunSummary(
            run_id=run_dir.name,
            variants=[
                build_variant_summary(variant.name, grouped_results[variant.name])
                for variant in selected_variants
            ],
        )
        _ = self._storage.write_summary(run_dir, summary)
        return summary

    async def _run_case(
        self,
        case: EvaluationCase,
        variant: HarnessVariant,
        run_dir: Path,
    ) -> EvaluationResult:
        started_at: str = datetime.now().astimezone().isoformat()
        collector = TraceCollector(model=self._agent_loop.model, provider=self._get_provider_name())
        final_response: str = ""
        error_message: str = ""
        session_key: str = self._build_session_key(run_dir, variant, case)

        self._save_session_metadata(
            session_key=session_key,
            run_id=run_dir.name,
            variant=variant.name,
            case=case,
        )

        try:
            final_response = await asyncio.wait_for(
                self._agent_loop.process_direct(
                    content=case.input,
                    session_key=session_key,
                    observer=collector,
                    variant=self._to_context_variant(variant),
                ),
                timeout=case.timeout_seconds,
            )
        except asyncio.TimeoutError:
            error_message = f"timeout after {case.timeout_seconds}s"
        except Exception as exc:
            error_message = str(exc) or exc.__class__.__name__

        completed_at: str = datetime.now().astimezone().isoformat()
        trace = collector.build_trace(started_at=started_at, completed_at=completed_at)
        trace_path = self._storage.write_trace(run_dir, variant.name, case.case_id, trace)

        status: EvalStatus = self._classify_status(case, final_response, collector, error_message)
        result = EvaluationResult(
            case_id=case.case_id,
            variant=variant.name,
            status=status,
            session_key=session_key,
            final_response=final_response,
            finish_reason=collector.finish_reason,
            tool_call_count=len(collector.tool_events),
            usage=collector.usage,
            trace_path=str(trace_path.relative_to(run_dir)),
            error_message=error_message,
        )
        _ = self._storage.write_result(run_dir, variant.name, result)
        return result

    def _classify_status(
        self,
        case: EvaluationCase,
        final_response: str,
        collector: TraceCollector,
        error_message: str,
    ) -> EvalStatus:
        if error_message or collector.finish_reason == "error":
            return "infra_error"
        if case.expected_response_pattern:
            matched = bool(re.search(case.expected_response_pattern, final_response, re.DOTALL))
            return "success" if matched else "task_failure"
        if case.expected_mode == "response":
            return "success" if final_response.strip() else "task_failure"
        if case.expected_mode == "tool_use":
            return "success" if collector.tool_events else "task_failure"
        if case.expected_mode == "failure_expected":
            return "success" if not final_response.strip() else "task_failure"
        return "task_failure"

    def _to_context_variant(self, variant: HarnessVariant) -> ContextVariant:
        return ContextVariant(
            environment_bootstrap=variant.environment_bootstrap,
            context_profile=variant.context_profile,
            completion_policy=variant.completion_policy,
        )

    def _build_session_key(
        self, run_dir: Path, variant: HarnessVariant, case: EvaluationCase
    ) -> str:
        return f"eval:{run_dir.name}:{variant.name}:{case.case_id}"

    def _save_session_metadata(
        self,
        session_key: str,
        run_id: str,
        variant: str,
        case: EvaluationCase,
    ) -> None:
        if self._session_manager is None:
            return

        session = self._session_manager.get_or_create(session_key)
        session.metadata.update(
            {
                "type": "eval",
                "run_id": run_id,
                "variant": variant,
                "case_id": case.case_id,
                "expected_mode": case.expected_mode,
                "tags": list(case.tags),
            }
        )
        self._session_manager.save(session)

    def _get_provider_name(self) -> str:
        provider_name = getattr(self._agent_loop, "_provider_name", "")
        return provider_name or ""
