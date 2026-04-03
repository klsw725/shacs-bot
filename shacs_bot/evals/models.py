from __future__ import annotations

from typing import ClassVar, Literal

from pydantic import BaseModel, ConfigDict, Field
from pydantic.alias_generators import to_camel


EvalStatus = Literal["success", "task_failure", "infra_error"]
ExpectedMode = Literal["response", "tool_use", "failure_expected"]


class EvalBaseModel(BaseModel):
    model_config: ClassVar[ConfigDict] = ConfigDict(alias_generator=to_camel, populate_by_name=True)


class EvaluationCase(EvalBaseModel):
    case_id: str
    input: str
    expected_mode: ExpectedMode = "response"
    expected_response_pattern: str = ""
    tags: list[str] = Field(default_factory=list)
    timeout_seconds: int = 120
    notes: str = ""
    source_session_key: str = ""
    source_message_index: int = 0
    source_timestamp: str = ""
    source_channel: str = ""


class EvalCandidate(EvalBaseModel):
    provider_name: str
    model: str


class HarnessVariant(EvalBaseModel):
    name: str
    environment_bootstrap: bool = True
    context_profile: str = "default"
    completion_policy: str = "default"


class ToolEvent(EvalBaseModel):
    name: str
    arguments: dict[str, object] = Field(default_factory=dict)
    result_preview: str = ""
    is_error: bool = False


class TraceArtifact(EvalBaseModel):
    model: str = ""
    provider: str = ""
    finish_reason: str = ""
    assistant_response: str = ""
    tool_events: list[ToolEvent] = Field(default_factory=list)
    usage: dict[str, int] = Field(default_factory=dict)
    started_at: str = ""
    completed_at: str = ""


class EvaluationResult(EvalBaseModel):
    case_id: str
    variant: str
    status: EvalStatus
    session_key: str = ""
    final_response: str = ""
    finish_reason: str = ""
    tool_call_count: int = 0
    usage: dict[str, int] = Field(default_factory=dict)
    trace_path: str = ""
    error_message: str = ""


class VariantSummary(EvalBaseModel):
    variant: str
    total: int = 0
    success: int = 0
    task_failure: int = 0
    infra_error: int = 0
    avg_tool_calls: float = 0.0
    prompt_tokens: int = 0
    completion_tokens: int = 0


class VariantHealth(EvalBaseModel):
    variant: str
    last_success_rate: float = 0.0
    baseline_success_rate: float = 0.0
    success_delta: float = 0.0
    weighted_score: float = 0.0
    avg_tool_calls: float = 0.0
    avg_total_tokens: float = 0.0
    status: str = "unknown"
    disabled: bool = False
    recommended: bool = False
    last_run_id: str = ""
    baseline_run_id: str = ""
    last_updated: str = ""


class RunSummary(EvalBaseModel):
    run_id: str
    variants: list[VariantSummary] = Field(default_factory=list)


class RunManifest(EvalBaseModel):
    run_id: str
    cases_file: str = ""
    variants: list[str] = Field(default_factory=list)
    created_at: str = ""
