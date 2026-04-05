from __future__ import annotations

from pathlib import Path

import pytest

from shacs_bot.agent.approval import ApprovalGate
from shacs_bot.agent.policy import ActionContext
from shacs_bot.agent.policy import ActorContext
from shacs_bot.agent.policy import PolicyEvaluator
from shacs_bot.agent.usage import TurnUsage
from shacs_bot.agent.usage import UsageTracker
from shacs_bot.bus.events import OutboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.config.schema import Config
from shacs_bot.config.schema import PolicyConfig
from shacs_bot.providers.base import LLMProvider
from shacs_bot.providers.base import LLMResponse


class DummyProvider(LLMProvider):
    def __init__(self, response_content: str | None = None) -> None:
        super().__init__()
        self.called: bool = False
        self._response_content: str | None = response_content

    async def chat(
        self,
        messages: list[dict[str, object]],
        tools: list[dict[str, object]] | None = None,
        model: str | None = None,
        max_tokens: int = 4096,
        temperature: float = 0.7,
        reasoning_effort: str | None = None,
        tool_choice: str | dict[str, object] | None = None,
    ) -> LLMResponse:
        self.called = True
        if self._response_content is not None:
            return LLMResponse(content=self._response_content, finish_reason="stop")
        raise AssertionError("chat should not be called")

    def get_default_model(self) -> str:
        return "dummy-model"


class DummyBus(MessageBus):
    async def publish_outbound(self, msg: OutboundMessage) -> None:
        raise AssertionError("publish_outbound should not be called")


def test_policy_config_accepts_camel_case_keys() -> None:
    config = Config.model_validate(
        {
            "policy": {
                "enabled": True,
                "trustedUsers": ["user-1"],
                "trustedChannels": ["telegram:dm"],
                "dailyCostLimit": 3.5,
                "highRiskTools": ["exec"],
            }
        }
    )

    assert config.policy.enabled is True
    assert config.policy.trusted_users == ["user-1"]
    assert config.policy.trusted_channels == ["telegram:dm"]
    assert config.policy.daily_cost_limit == 3.5
    assert config.policy.high_risk_tools == ["exec"]


def test_policy_config_dumps_camel_case_keys() -> None:
    payload = PolicyConfig(
        enabled=True,
        trusted_users=["user-1"],
        trusted_channels=["telegram:dm"],
        daily_cost_limit=2.0,
        high_risk_tools=["exec"],
    ).model_dump(by_alias=True)

    assert payload["trustedUsers"] == ["user-1"]
    assert payload["trustedChannels"] == ["telegram:dm"]
    assert payload["dailyCostLimit"] == 2.0
    assert payload["highRiskTools"] == ["exec"]


def test_policy_disabled_defaults_to_allow() -> None:
    evaluator = PolicyEvaluator(PolicyConfig())

    decision = evaluator.evaluate(
        actor=ActorContext(user_id="user-1", channel="cli", is_dm=True),
        action=ActionContext(kind="tool", name="exec", risk="high", estimated_cost=1.0),
        quota=evaluator.build_quota_context(daily_cost=99.0),
    )

    assert decision.result == "allow"
    assert decision.reason == "policy disabled"


def test_build_actor_context_marks_trusted_user_and_channel() -> None:
    evaluator = PolicyEvaluator(
        PolicyConfig(enabled=True, trusted_users=["user-1"], trusted_channels=["telegram:dm"])
    )

    actor = evaluator.build_actor_context(user_id="user-1", channel="telegram:dm", is_dm=True)

    assert actor.is_trusted_user is True
    assert actor.is_trusted_channel is True


def test_build_tool_action_context_marks_high_risk_tool() -> None:
    evaluator = PolicyEvaluator(PolicyConfig(enabled=True, high_risk_tools=["exec"]))

    action = evaluator.build_tool_action_context("exec", estimated_cost=1.25)

    assert action.kind == "tool"
    assert action.name == "exec"
    assert action.risk == "high"
    assert action.estimated_cost == 1.25


def test_high_risk_action_escalates_before_quota_limit() -> None:
    evaluator = PolicyEvaluator(PolicyConfig(enabled=True, high_risk_tools=["exec"]))

    decision = evaluator.evaluate(
        actor=ActorContext(user_id="user-2", channel="cli", is_dm=True),
        action=evaluator.build_tool_action_context("exec", estimated_cost=0.5),
        quota=evaluator.build_quota_context(daily_cost=0.2),
    )

    assert decision.result == "escalate"
    assert decision.reason == "high-risk action requires escalation"


def test_trusted_user_low_risk_action_allows() -> None:
    evaluator = PolicyEvaluator(PolicyConfig(enabled=True, trusted_users=["user-1"]))
    actor = evaluator.build_actor_context(user_id="user-1", channel="cli", is_dm=True)

    decision = evaluator.evaluate(
        actor=actor,
        action=evaluator.build_tool_action_context("read_file"),
        quota=evaluator.build_quota_context(daily_cost=0.1),
    )

    assert decision.result == "allow"
    assert decision.reason == "trusted user"


def test_trusted_channel_low_risk_action_allows() -> None:
    evaluator = PolicyEvaluator(PolicyConfig(enabled=True, trusted_channels=["slack:deploy-room"]))
    actor = evaluator.build_actor_context(
        user_id="user-2",
        channel="slack:deploy-room",
        is_dm=False,
    )

    decision = evaluator.evaluate(
        actor=actor,
        action=evaluator.build_tool_action_context("read_file"),
        quota=evaluator.build_quota_context(daily_cost=0.1),
    )

    assert decision.result == "allow"
    assert decision.reason == "trusted channel"


def test_low_trust_low_risk_action_defaults_to_allow() -> None:
    evaluator = PolicyEvaluator(PolicyConfig(enabled=True))

    decision = evaluator.evaluate(
        actor=ActorContext(user_id="user-9", channel="discord:general", is_dm=False),
        action=evaluator.build_tool_action_context("read_file"),
        quota=evaluator.build_quota_context(daily_cost=0.1),
    )

    assert decision.result == "allow"
    assert decision.reason == "default allow"


def test_trusted_user_high_risk_action_still_escalates() -> None:
    evaluator = PolicyEvaluator(
        PolicyConfig(enabled=True, trusted_users=["user-1"], high_risk_tools=["exec"])
    )
    actor = evaluator.build_actor_context(user_id="user-1", channel="cli", is_dm=True)

    decision = evaluator.evaluate(
        actor=actor,
        action=evaluator.build_tool_action_context("exec", estimated_cost=0.5),
        quota=evaluator.build_quota_context(daily_cost=0.1),
    )

    assert decision.result == "escalate"
    assert decision.reason == "high-risk action requires escalation"


def test_trusted_channel_high_risk_action_still_escalates() -> None:
    evaluator = PolicyEvaluator(
        PolicyConfig(enabled=True, trusted_channels=["slack:deploy-room"], high_risk_tools=["exec"])
    )
    actor = evaluator.build_actor_context(
        user_id="user-2",
        channel="slack:deploy-room",
        is_dm=False,
    )

    decision = evaluator.evaluate(
        actor=actor,
        action=evaluator.build_tool_action_context("exec", estimated_cost=0.5),
        quota=evaluator.build_quota_context(daily_cost=0.1),
    )

    assert decision.result == "escalate"
    assert decision.reason == "high-risk action requires escalation"


def test_quota_exceeded_degrades_low_risk_action() -> None:
    evaluator = PolicyEvaluator(PolicyConfig(enabled=True, daily_cost_limit=1.0))

    decision = evaluator.evaluate(
        actor=ActorContext(user_id="user-2", channel="cli", is_dm=True),
        action=evaluator.build_tool_action_context("read_file", estimated_cost=0.05),
        quota=evaluator.build_quota_context(daily_cost=1.0),
    )

    assert decision.result == "degrade"
    assert decision.reason == "daily cost limit exceeded"


def test_quota_exceeded_denies_high_risk_action() -> None:
    evaluator = PolicyEvaluator(
        PolicyConfig(enabled=True, daily_cost_limit=1.0, high_risk_tools=["exec"])
    )

    decision = evaluator.evaluate(
        actor=ActorContext(user_id="user-2", channel="cli", is_dm=True),
        action=evaluator.build_tool_action_context("exec", estimated_cost=3.0),
        quota=evaluator.build_quota_context(daily_cost=1.0),
    )

    assert decision.result == "deny"
    assert decision.reason == "daily cost limit exceeded for high-risk action"


@pytest.mark.asyncio
async def test_approval_gate_allows_trusted_user_without_provider_call(tmp_path: Path) -> None:
    provider = DummyProvider()
    gate = ApprovalGate(
        mode="auto",
        provider=provider,
        model="test-model",
        session_history=[],
        bus=DummyBus(),
        origin={"channel": "cli", "chat_id": "user-1", "metadata": {}},
        skill_name="workspace-skill",
        workspace=tmp_path,
        policy_config=PolicyConfig(enabled=True, trusted_users=["user-1"]),
    )

    decision = await gate.check("exec", {"command": "pwd"})

    assert decision.denied is False
    assert decision.reason == "trusted user"
    assert provider.called is False


@pytest.mark.asyncio
async def test_approval_gate_denies_when_daily_limit_exceeded(tmp_path: Path) -> None:
    provider = DummyProvider()
    usage_tracker = UsageTracker(tmp_path / "usage")
    turn = TurnUsage(prompt_tokens=1000, completion_tokens=1000, total_tokens=2000, cost_usd=1.5)
    usage_tracker.record("session-1", turn)
    gate = ApprovalGate(
        mode="auto",
        provider=provider,
        model="test-model",
        session_history=[],
        bus=DummyBus(),
        origin={"channel": "cli", "chat_id": "user-2", "metadata": {}},
        skill_name="workspace-skill",
        workspace=tmp_path,
        policy_config=PolicyConfig(enabled=True, daily_cost_limit=1.0, high_risk_tools=["exec"]),
        usage_tracker=usage_tracker,
    )

    decision = await gate.check("exec", {"command": "pwd"})

    assert decision.denied is True
    assert decision.reason == "daily cost limit exceeded for high-risk action"
    assert provider.called is False


@pytest.mark.asyncio
async def test_approval_gate_allows_trusted_channel_without_provider_call(tmp_path: Path) -> None:
    provider = DummyProvider()
    gate = ApprovalGate(
        mode="auto",
        provider=provider,
        model="test-model",
        session_history=[],
        bus=DummyBus(),
        origin={"channel": "slack:deploy-room", "chat_id": "chat-1", "metadata": {}},
        skill_name="workspace-skill",
        workspace=tmp_path,
        policy_config=PolicyConfig(enabled=True, trusted_channels=["slack:deploy-room"]),
    )

    decision = await gate.check("exec", {"command": "pwd"})

    assert decision.denied is False
    assert decision.reason == "trusted channel"
    assert provider.called is False


@pytest.mark.asyncio
async def test_approval_gate_trusted_user_high_risk_uses_llm_path(tmp_path: Path) -> None:
    provider = DummyProvider('{"approved": true, "reason": "llm allow"}')
    gate = ApprovalGate(
        mode="auto",
        provider=provider,
        model="test-model",
        session_history=[],
        bus=DummyBus(),
        origin={"channel": "cli", "chat_id": "user-1", "metadata": {}},
        skill_name="workspace-skill",
        workspace=tmp_path,
        policy_config=PolicyConfig(
            enabled=True,
            trusted_users=["user-1"],
            high_risk_tools=["exec"],
        ),
    )

    decision = await gate.check("exec", {"command": "pwd"})

    assert decision.denied is False
    assert decision.reason == "llm allow"
    assert provider.called is True


@pytest.mark.asyncio
async def test_approval_gate_low_trust_high_risk_uses_llm_path(tmp_path: Path) -> None:
    provider = DummyProvider('{"approved": false, "reason": "llm deny"}')
    gate = ApprovalGate(
        mode="auto",
        provider=provider,
        model="test-model",
        session_history=[],
        bus=DummyBus(),
        origin={"channel": "cli", "chat_id": "user-9", "metadata": {}},
        skill_name="workspace-skill",
        workspace=tmp_path,
        policy_config=PolicyConfig(enabled=True, high_risk_tools=["exec"]),
    )

    decision = await gate.check("exec", {"command": "pwd"})

    assert decision.denied is True
    assert decision.reason == "llm deny"
    assert provider.called is True
