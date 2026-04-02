from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta
import importlib
from pathlib import Path
from typing import TYPE_CHECKING, Callable, cast

from shacs_bot.agent.session.manager import SessionManager
from shacs_bot.agent.tools.cron.service import CronService
from shacs_bot.agent.tools.cron.types import CronJob, CronSchedule
from shacs_bot.evals.extractor import SessionCaseExtractor, build_auto_cases_path
from shacs_bot.evals.models import EvalCandidate, EvaluationCase, HarnessVariant, RunSummary
from shacs_bot.evals.runner import (
    EvaluationRunner,
    get_default_cases_path,
    load_cases_file,
    resolve_variant,
)
from shacs_bot.evals.state import (
    AutoEvalState,
    append_variant_history,
    apply_weighted_scores,
    build_candidate_key,
    calculate_candidate_score,
    compare_to_baseline,
    compute_recommended_provider_model,
    compute_recommended_runtime_variant,
    compute_trigger_variants,
    read_auto_eval_state,
    read_run_summary,
    get_eval_state_path,
    write_auto_eval_state,
)
from shacs_bot.evals.storage import EvaluationStorage

if TYPE_CHECKING:
    from shacs_bot.agent.loop import AgentLoop


@dataclass
class AutoEvalRunResult:
    state: AutoEvalState
    state_path: Path
    cases_path: Path
    run_dir: Path | None
    summary_path: Path


class AutoEvalService:
    def __init__(
        self,
        workspace: Path,
        agent_loop: object,
        session_manager: SessionManager,
    ) -> None:
        self._workspace: Path = workspace
        self._agent_loop: object = agent_loop
        self._sessions: SessionManager = session_manager

    def prepare_trigger(self, session_key: str) -> bool:
        if session_key.startswith("eval:") or session_key.startswith("scheduled:"):
            return False

        state: AutoEvalState = read_auto_eval_state(self._workspace) or AutoEvalState()
        if not state.trigger_enabled:
            return False

        state.completed_turns_since_trigger += 1
        now = datetime.now().astimezone()
        should_trigger = state.completed_turns_since_trigger >= state.trigger_turn_threshold

        if should_trigger and state.last_triggered_at:
            last_triggered = datetime.fromisoformat(state.last_triggered_at)
            if now - last_triggered < timedelta(minutes=state.trigger_min_interval_minutes):
                should_trigger = False

        if not should_trigger:
            _ = write_auto_eval_state(self._workspace, state)
            return False

        state.completed_turns_since_trigger = 0
        state.last_triggered_at = now.isoformat()
        state.last_triggered_session_key = session_key
        state.last_trigger_status = "running"
        state.last_trigger_error = ""
        _ = write_auto_eval_state(self._workspace, state)
        return True

    def sync_schedule(self, cron_service: CronService) -> CronJob | None:
        state: AutoEvalState = read_auto_eval_state(self._workspace) or AutoEvalState()

        for job in cron_service.list_jobs(include_disabled=True):
            if job.payload.metadata.get("eval_trigger"):
                _ = cron_service.remove_job(job.id)

        state.last_scheduled_job_id = ""

        if state.trigger_schedule_kind == "off":
            _ = write_auto_eval_state(self._workspace, state)
            return None

        schedule: CronSchedule
        if state.trigger_schedule_kind == "every":
            if state.trigger_schedule_every_minutes <= 0:
                _ = write_auto_eval_state(self._workspace, state)
                return None
            schedule = CronSchedule(
                kind="every",
                every_ms=state.trigger_schedule_every_minutes * 60 * 1000,
            )
        elif state.trigger_schedule_kind == "cron":
            if not state.trigger_schedule_cron_expr:
                _ = write_auto_eval_state(self._workspace, state)
                return None
            schedule = CronSchedule(
                kind="cron",
                expr=state.trigger_schedule_cron_expr,
                tz=state.trigger_schedule_tz or None,
            )
        else:
            _ = write_auto_eval_state(self._workspace, state)
            return None

        job = cron_service.add_job(
            name="self-eval",
            schedule=schedule,
            message="scheduled self evaluation",
            deliver=False,
            metadata={"eval_trigger": True},
        )
        state.last_scheduled_job_id = job.id
        _ = write_auto_eval_state(self._workspace, state)
        return job

    async def run_auto_eval(
        self,
        *,
        session_filter: str | None = None,
        session_limit: int | None = None,
        case_limit: int | None = None,
        variant_names: list[str] | None = None,
        baseline: bool = False,
        compare: bool = True,
        output: Path | None = None,
        cases_output: Path | None = None,
        include_eval_sessions: bool = False,
        triggered: bool = False,
        trigger_session_key: str = "",
        persist_state: bool = True,
    ) -> AutoEvalRunResult:
        previous_state: AutoEvalState = read_auto_eval_state(self._workspace) or AutoEvalState()
        default_cases_path: Path = get_default_cases_path(self._workspace)
        default_cases = load_cases_file(default_cases_path)

        effective_session_limit: int = session_limit or previous_state.trigger_session_limit
        effective_case_limit: int = case_limit or previous_state.trigger_case_limit
        effective_variant_names: list[str] = variant_names or list(previous_state.trigger_variants)

        extractor = SessionCaseExtractor(self._workspace, self._sessions)
        extracted = extractor.extract_cases(
            session_filter=session_filter,
            session_limit=effective_session_limit,
            case_limit=effective_case_limit,
            include_eval_sessions=include_eval_sessions,
        )
        combined_cases = [*default_cases, *extracted]
        if not combined_cases:
            raise ValueError("no evaluation cases available")

        variants = [resolve_variant(name) for name in effective_variant_names]
        run_id: str = datetime.now().strftime("%Y-%m-%d-%H-%M-%S")
        bundle_path: Path = cases_output or build_auto_cases_path(
            self._workspace, f"auto-run-{run_id}"
        )
        _ = extractor.write_cases_file(bundle_path, combined_cases)

        configured_candidates = self._parse_candidates(previous_state.autonomous_candidates)
        if configured_candidates:
            (
                summary,
                run_dir,
                recommended_provider_name,
                recommended_model,
                candidate_scores,
                candidate_best,
            ) = await self._run_candidates(
                configured_candidates,
                combined_cases,
                variants,
                output,
                bundle_path,
                run_id,
            )
        else:
            runner = EvaluationRunner(
                agent_loop=cast("AgentLoop", self._agent_loop),
                storage=EvaluationStorage(self._workspace),
                session_manager=self._sessions,
            )
            summary = await runner.run_cases(
                cases=combined_cases,
                variants=variants,
                output_dir=output,
                cases_file=bundle_path,
                run_id=run_id,
            )
            run_dir = runner.last_run_dir
            recommended_provider_name = ""
            recommended_model = ""
            candidate_scores = {}
            candidate_best = ""
        summary_path: Path = (run_dir / "summary.json") if run_dir else Path("summary.json")
        actual_run_id: str = run_dir.name if run_dir else run_id
        baseline_run_id: str = (
            actual_run_id if baseline else (previous_state.baseline_run_id or actual_run_id)
        )
        baseline_run_dir: str = (
            str(run_dir)
            if baseline and run_dir
            else (previous_state.baseline_run_dir or (str(run_dir) if run_dir else ""))
        )
        baseline_summary = (
            read_run_summary(Path(baseline_run_dir))
            if compare and baseline_run_dir and baseline_run_id != actual_run_id
            else None
        )
        variant_health, regressions = compare_to_baseline(
            summary.variants,
            baseline_summary.variants if baseline_summary else summary.variants,
        )
        variant_health = apply_weighted_scores(variant_health, previous_state.variant_history)
        for item in summary.variants:
            health = variant_health[item.variant]
            health.last_run_id = actual_run_id
            health.baseline_run_id = baseline_run_id

        next_trigger_variants: list[str] = compute_trigger_variants(
            effective_variant_names,
            variant_health,
            previous_state.variant_history,
        )
        variant_history = append_variant_history(
            previous_state.variant_history,
            run_id=actual_run_id,
            variant_health=variant_health,
        )
        recommended_runtime_variant: str = compute_recommended_runtime_variant(
            next_trigger_variants,
            variant_health,
            previous_state.variant_history,
        )
        if not recommended_provider_name or not recommended_model:
            recommended_provider_name, recommended_model = compute_recommended_provider_model(
                current_model=getattr(self._agent_loop, "model", "") or "",
                current_provider_name=getattr(self._agent_loop, "_provider_name", "") or "",
                recommended_runtime_variant=recommended_runtime_variant,
                regressions=regressions,
            )

        state = AutoEvalState(
            last_auto_run_id=actual_run_id,
            baseline_run_id=baseline_run_id,
            baseline_run_dir=baseline_run_dir,
            last_auto_run_at=datetime.now().astimezone().isoformat(),
            last_cases_path=str(bundle_path),
            last_summary_path=str(summary_path),
            last_run_dir=str(run_dir) if run_dir else "",
            default_case_count=len(default_cases),
            extracted_case_count=len(extracted),
            total_case_count=len(combined_cases),
            variants=[item.variant for item in summary.variants],
            variant_health=variant_health,
            variant_history=variant_history,
            regressions=regressions,
            session_filter=session_filter or "",
            include_eval_sessions=include_eval_sessions,
            recommended_runtime_variant=recommended_runtime_variant,
            recommended_provider_name=recommended_provider_name,
            recommended_model=recommended_model,
            autonomous_candidates=list(previous_state.autonomous_candidates),
            candidate_scores=candidate_scores
            or (
                {
                    build_candidate_key(
                        recommended_provider_name, recommended_model
                    ): calculate_candidate_score(variant_health)
                }
                if recommended_provider_name and recommended_model
                else {}
            ),
            candidate_best=candidate_best
            or (
                build_candidate_key(recommended_provider_name, recommended_model)
                if recommended_provider_name and recommended_model
                else ""
            ),
            trigger_enabled=previous_state.trigger_enabled,
            trigger_turn_threshold=previous_state.trigger_turn_threshold,
            trigger_min_interval_minutes=previous_state.trigger_min_interval_minutes,
            trigger_session_limit=previous_state.trigger_session_limit,
            trigger_case_limit=previous_state.trigger_case_limit,
            trigger_variants=next_trigger_variants,
            completed_turns_since_trigger=previous_state.completed_turns_since_trigger,
            last_triggered_at=previous_state.last_triggered_at,
            last_triggered_session_key=trigger_session_key
            or previous_state.last_triggered_session_key,
            last_trigger_status="success" if triggered else previous_state.last_trigger_status,
            last_trigger_error="",
        )
        state_path: Path = (
            write_auto_eval_state(self._workspace, state)
            if persist_state
            else get_eval_state_path(self._workspace)
        )
        return AutoEvalRunResult(
            state=state,
            state_path=state_path,
            cases_path=bundle_path,
            run_dir=run_dir,
            summary_path=summary_path,
        )

    def _parse_candidates(self, raw_candidates: list[str]) -> list[EvalCandidate]:
        parsed: list[EvalCandidate] = []
        for raw in raw_candidates:
            if ":" not in raw:
                continue
            provider_name, model = raw.split(":", 1)
            if not provider_name or not model:
                continue
            parsed.append(EvalCandidate(provider_name=provider_name, model=model))
        return parsed

    async def _run_candidates(
        self,
        candidates: list[EvalCandidate],
        cases: list[EvaluationCase],
        variants: list[HarnessVariant],
        output: Path | None,
        cases_file: Path,
        run_id: str,
    ) -> tuple[RunSummary, Path | None, str, str, dict[str, float], str]:
        candidate_scores: dict[str, float] = {}
        best_summary: RunSummary | None = None
        best_run_dir: Path | None = None
        best_provider_name: str = ""
        best_model: str = ""
        best_key: str = ""
        best_score: float = -1.0

        create_eval_runtime = cast(
            Callable[..., tuple[object, object]],
            getattr(importlib.import_module("shacs_bot.cli.commands"), "create_eval_runtime"),
        )

        for item in candidates:
            candidate_runtime: tuple[object, object] = create_eval_runtime(
                self._load_config(),
                model_override=item.model,
                provider_name_override=item.provider_name,
                use_state_recommendation=False,
            )
            candidate_loop = cast("AgentLoop", candidate_runtime[0])
            candidate_sessions = cast(SessionManager, candidate_runtime[1])
            try:
                runner = EvaluationRunner(
                    agent_loop=candidate_loop,
                    storage=EvaluationStorage(self._workspace),
                    session_manager=candidate_sessions,
                )
                summary = await runner.run_cases(
                    cases=cases,
                    variants=variants,
                    output_dir=output,
                    cases_file=cases_file,
                    run_id=f"{run_id}-{item.provider_name}-{item.model.replace('/', '_')}",
                )
                score = calculate_candidate_score(
                    apply_weighted_scores(
                        compare_to_baseline(summary.variants, summary.variants)[0],
                        [],
                    )
                )
                candidate_key = build_candidate_key(item.provider_name, item.model)
                candidate_scores[candidate_key] = score
                if score > best_score:
                    best_score = score
                    best_summary = summary
                    best_run_dir = runner.last_run_dir
                    best_provider_name = item.provider_name
                    best_model = item.model
                    best_key = candidate_key
            finally:
                await candidate_loop.close_mcp()

        if best_summary is None:
            raise ValueError("no autonomous candidate evaluation result")
        return (
            best_summary,
            best_run_dir,
            best_provider_name,
            best_model,
            candidate_scores,
            best_key,
        )

    def _load_config(self):
        from shacs_bot.config.loader import load_config

        return load_config()

    def mark_trigger_failure(self, session_key: str, error: str) -> Path:
        state: AutoEvalState = read_auto_eval_state(self._workspace) or AutoEvalState()
        state.last_triggered_at = datetime.now().astimezone().isoformat()
        state.last_triggered_session_key = session_key
        state.last_trigger_status = "error"
        state.last_trigger_error = error
        return write_auto_eval_state(self._workspace, state)
