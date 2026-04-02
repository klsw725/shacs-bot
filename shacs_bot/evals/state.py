from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import cast

from pydantic import Field

from shacs_bot.evals.models import EvalBaseModel, RunSummary, VariantHealth, VariantSummary
from shacs_bot.utils.helpers import ensure_dir


class AutoEvalState(EvalBaseModel):
    last_auto_run_id: str = ""
    baseline_run_id: str = ""
    baseline_run_dir: str = ""
    last_auto_run_at: str = ""
    last_cases_path: str = ""
    last_summary_path: str = ""
    last_run_dir: str = ""
    default_case_count: int = 0
    extracted_case_count: int = 0
    total_case_count: int = 0
    variants: list[str] = Field(default_factory=list)
    variant_health: dict[str, VariantHealth] = Field(default_factory=dict)
    variant_history: list[dict[str, object]] = Field(default_factory=list)
    regressions: list[str] = Field(default_factory=list)
    session_filter: str = ""
    include_eval_sessions: bool = False
    recommended_runtime_variant: str = "default"
    recommended_provider_name: str = ""
    recommended_model: str = ""
    autonomous_candidates: list[str] = Field(default_factory=list)
    candidate_scores: dict[str, float] = Field(default_factory=dict)
    candidate_best: str = ""
    trigger_enabled: bool = True
    trigger_schedule_kind: str = "off"
    trigger_schedule_every_minutes: int = 0
    trigger_schedule_cron_expr: str = ""
    trigger_schedule_tz: str = ""
    last_scheduled_job_id: str = ""
    trigger_turn_threshold: int = 12
    trigger_min_interval_minutes: int = 90
    trigger_session_limit: int = 6
    trigger_case_limit: int = 12
    trigger_variants: list[str] = Field(default_factory=lambda: ["default", "strict-completion"])
    completed_turns_since_trigger: int = 0
    last_triggered_at: str = ""
    last_triggered_session_key: str = ""
    last_trigger_status: str = "idle"
    last_trigger_error: str = ""


def get_eval_state_path(workspace: Path) -> Path:
    return workspace / "evals" / "state.json"


def write_auto_eval_state(workspace: Path, state: AutoEvalState) -> Path:
    path: Path = get_eval_state_path(workspace)
    _ = ensure_dir(path.parent)
    _ = path.write_text(
        json.dumps(state.model_dump(mode="json", by_alias=True), ensure_ascii=False, indent=2)
        + "\n",
        encoding="utf-8",
    )
    return path


def update_auto_eval_state(workspace: Path, **updates: object) -> tuple[AutoEvalState, Path]:
    current: AutoEvalState = read_auto_eval_state(workspace) or AutoEvalState()
    state: AutoEvalState = current.model_copy(update=updates)
    path: Path = write_auto_eval_state(workspace, state)
    return state, path


def read_auto_eval_state(workspace: Path) -> AutoEvalState | None:
    path: Path = get_eval_state_path(workspace)
    if not path.exists():
        return None

    try:
        return AutoEvalState.model_validate_json(path.read_text(encoding="utf-8"))
    except Exception:
        return None


def read_run_summary(run_dir: Path) -> RunSummary | None:
    path: Path = run_dir / "summary.json"
    if not path.exists():
        return None

    try:
        return RunSummary.model_validate_json(path.read_text(encoding="utf-8"))
    except Exception:
        return None


def calculate_success_rate(summary: VariantSummary) -> float:
    if summary.total == 0:
        return 0.0
    return summary.success / summary.total


def compare_to_baseline(
    current: list[VariantSummary],
    baseline: list[VariantSummary],
    threshold: float = 0.1,
) -> tuple[dict[str, VariantHealth], list[str]]:
    baseline_map: dict[str, VariantSummary] = {item.variant: item for item in baseline}
    updated_at: str = datetime.now().astimezone().isoformat()
    health: dict[str, VariantHealth] = {}
    regressions: list[str] = []

    for current_item in current:
        current_rate: float = calculate_success_rate(current_item)
        baseline_item: VariantSummary | None = baseline_map.get(current_item.variant)
        baseline_rate: float = (
            calculate_success_rate(baseline_item) if baseline_item else current_rate
        )
        delta: float = current_rate - baseline_rate
        status: str = "healthy"
        if delta < -threshold:
            status = "regression"
            regressions.append(current_item.variant)
        elif delta < 0:
            status = "warning"

        health[current_item.variant] = VariantHealth(
            variant=current_item.variant,
            last_success_rate=current_rate,
            baseline_success_rate=baseline_rate,
            success_delta=delta,
            status=status,
            disabled=status == "regression" and current_item.variant != "default",
            recommended=status == "healthy",
            last_run_id="",
            baseline_run_id="",
            last_updated=updated_at,
        )

    return health, regressions


def calculate_weighted_score(
    *,
    current_success_rate: float,
    success_delta: float,
    recent_statuses: list[str],
) -> float:
    score: float = current_success_rate
    score += success_delta * 0.5
    penalties: dict[str, float] = {"regression": 0.25, "warning": 0.1, "healthy": 0.0}
    for index, status in enumerate(reversed(recent_statuses[-3:]), start=1):
        decay: float = 1.0 / index
        score -= penalties.get(status, 0.0) * decay
    return max(0.0, min(1.0, score))


def get_variant_status_history(
    history: list[dict[str, object]],
    variant: str,
    limit: int = 5,
) -> list[str]:
    statuses: list[str] = []
    for entry in history[-limit:]:
        variants_obj: object = entry.get("variants")
        if not isinstance(variants_obj, dict):
            continue
        variants: dict[str, object] = cast(dict[str, object], variants_obj)
        variant_state_obj: object = variants.get(variant)
        if not isinstance(variant_state_obj, dict):
            continue
        variant_state: dict[str, object] = cast(dict[str, object], variant_state_obj)
        status_obj: object = variant_state.get("status")
        status: str | None = status_obj if isinstance(status_obj, str) else None
        if isinstance(status, str):
            statuses.append(status)
    return statuses


def has_recent_regression(
    history: list[dict[str, object]],
    variant: str,
    lookback: int = 3,
) -> bool:
    return "regression" in get_variant_status_history(history, variant, limit=lookback)


def is_stably_healthy(
    history: list[dict[str, object]],
    variant: str,
    current_status: str,
    required_healthy_runs: int = 2,
) -> bool:
    if current_status != "healthy":
        return False
    prior_needed: int = max(required_healthy_runs - 1, 0)
    statuses: list[str] = get_variant_status_history(history, variant, limit=prior_needed)
    combined: list[str] = [*statuses, current_status]
    if len(combined) < required_healthy_runs:
        return False
    return all(status == "healthy" for status in combined[-required_healthy_runs:])


def compute_trigger_variants(
    current_variants: list[str],
    variant_health: dict[str, VariantHealth],
    variant_history: list[dict[str, object]],
) -> list[str]:
    filtered: list[str] = []
    for variant in current_variants:
        if variant == "default":
            filtered.append(variant)
            continue
        health = variant_health.get(variant)
        if health and health.disabled:
            continue
        if health and health.weighted_score < 0.6:
            continue
        if (
            health
            and has_recent_regression(variant_history, variant)
            and not is_stably_healthy(
                variant_history,
                variant,
                health.status,
            )
        ):
            continue
        filtered.append(variant)

    if not filtered:
        return ["default"]
    if "default" not in filtered:
        filtered.insert(0, "default")
    return filtered


def append_variant_history(
    history: list[dict[str, object]],
    *,
    run_id: str,
    variant_health: dict[str, VariantHealth],
) -> list[dict[str, object]]:
    entry: dict[str, object] = {
        "runId": run_id,
        "recordedAt": datetime.now().astimezone().isoformat(),
        "variants": {
            name: health.model_dump(mode="json", by_alias=True)
            for name, health in variant_health.items()
        },
    }
    updated: list[dict[str, object]] = [*history, entry]
    return updated[-20:]


def compute_recommended_runtime_variant(
    trigger_variants: list[str],
    variant_health: dict[str, VariantHealth],
    variant_history: list[dict[str, object]],
) -> str:
    if "strict-completion" in trigger_variants:
        health = variant_health.get("strict-completion")
        if (
            health
            and health.recommended
            and not health.disabled
            and health.weighted_score >= 0.75
            and is_stably_healthy(variant_history, "strict-completion", health.status)
        ):
            return "strict-completion"
    return "default"


def compute_recommended_provider_model(
    *,
    current_model: str,
    current_provider_name: str,
    recommended_runtime_variant: str,
    regressions: list[str],
) -> tuple[str, str]:
    if recommended_runtime_variant != "strict-completion":
        return "", ""
    if regressions:
        return "", ""
    if not current_model or not current_provider_name:
        return "", ""
    return current_provider_name, current_model


def build_candidate_key(provider_name: str, model: str) -> str:
    return f"{provider_name}:{model}"


def calculate_candidate_score(variant_health: dict[str, VariantHealth]) -> float:
    if not variant_health:
        return 0.0
    return sum(item.weighted_score for item in variant_health.values()) / len(variant_health)


def select_best_candidate(candidate_scores: dict[str, float]) -> str:
    if not candidate_scores:
        return ""
    return max(candidate_scores.items(), key=lambda item: item[1])[0]


def apply_weighted_scores(
    variant_health: dict[str, VariantHealth],
    variant_history: list[dict[str, object]],
) -> dict[str, VariantHealth]:
    for variant, health in variant_health.items():
        recent_statuses = get_variant_status_history(variant_history, variant, limit=3)
        health.weighted_score = calculate_weighted_score(
            current_success_rate=health.last_success_rate,
            success_delta=health.success_delta,
            recent_statuses=recent_statuses,
        )
        if variant != "default" and health.weighted_score < 0.5:
            health.disabled = True
            health.recommended = False
    return variant_health
