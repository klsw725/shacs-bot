from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from shacs_bot.config.schema import PolicyConfig

PolicyResult = Literal["allow", "deny", "escalate", "degrade"]
ActionKind = Literal["tool", "planner", "workflow"]
ActionRisk = Literal["low", "medium", "high"]


@dataclass(frozen=True)
class ActorContext:
    user_id: str | None = None
    channel: str = ""
    is_dm: bool = False
    is_trusted_user: bool = False
    is_trusted_channel: bool = False


@dataclass(frozen=True)
class ActionContext:
    kind: ActionKind
    name: str
    risk: ActionRisk = "low"
    estimated_cost: float = 0.0


@dataclass(frozen=True)
class QuotaContext:
    daily_cost: float = 0.0
    daily_limit: float | None = None

    @property
    def exceeded(self) -> bool:
        return (
            self.daily_limit is not None
            and self.daily_limit > 0
            and self.daily_cost >= self.daily_limit
        )


@dataclass(frozen=True)
class PolicyDecision:
    result: PolicyResult
    reason: str


class PolicyEvaluator:
    def __init__(self, config: PolicyConfig):
        self._config: PolicyConfig = config

    def build_actor_context(
        self,
        *,
        user_id: str | None,
        channel: str,
        is_dm: bool,
    ) -> ActorContext:
        return ActorContext(
            user_id=user_id,
            channel=channel,
            is_dm=is_dm,
            is_trusted_user=bool(user_id) and user_id in self._config.trusted_users,
            is_trusted_channel=bool(channel) and channel in self._config.trusted_channels,
        )

    def build_tool_action_context(
        self,
        tool_name: str,
        *,
        estimated_cost: float = 0.0,
    ) -> ActionContext:
        return ActionContext(
            kind="tool",
            name=tool_name,
            risk=self.classify_tool_risk(tool_name),
            estimated_cost=estimated_cost,
        )

    def build_quota_context(self, *, daily_cost: float) -> QuotaContext:
        daily_limit: float | None = (
            self._config.daily_cost_limit if self._config.daily_cost_limit > 0 else None
        )
        return QuotaContext(daily_cost=daily_cost, daily_limit=daily_limit)

    def classify_tool_risk(self, tool_name: str) -> ActionRisk:
        if tool_name in self._config.high_risk_tools:
            return "high"
        return "low"

    def evaluate(
        self,
        actor: ActorContext,
        action: ActionContext,
        quota: QuotaContext,
    ) -> PolicyDecision:
        if not self._config.enabled:
            return PolicyDecision(result="allow", reason="policy disabled")

        if quota.exceeded:
            if action.risk == "high":
                return PolicyDecision(
                    result="deny",
                    reason="daily cost limit exceeded for high-risk action",
                )
            return PolicyDecision(
                result="degrade",
                reason="daily cost limit exceeded",
            )

        if action.risk == "high":
            return PolicyDecision(
                result="escalate",
                reason="high-risk action requires escalation",
            )

        if actor.is_trusted_user:
            return PolicyDecision(result="allow", reason="trusted user")

        if actor.is_trusted_channel:
            return PolicyDecision(result="allow", reason="trusted channel")

        return PolicyDecision(result="allow", reason="default allow")
