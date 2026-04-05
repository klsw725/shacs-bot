"""Heartbeat 워크플로우 재실행 및 알림 메타데이터 검증."""

from __future__ import annotations

from pathlib import Path
from typing import cast

import pytest

from shacs_bot.agent.tools.cron.service import CronService
from shacs_bot.heartbeat.service import HeartbeatService
from shacs_bot.providers.base import LLMProvider
from shacs_bot.workflow.models import NotifyTarget
from shacs_bot.workflow.redispatcher import WorkflowRedispatcher
from shacs_bot.workflow.runtime import WorkflowRuntime


# ──────────────────────────────────────────────
# 스텁
# ──────────────────────────────────────────────


class _StubCronService:
    pass


class _StubProvider:
    pass


class _StubHeartbeatService:
    """execute_existing_workflow 호출을 기록하는 스텁."""

    def __init__(self) -> None:
        self.dispatched: list[str] = []

    async def execute_existing_workflow(self, workflow_id: str) -> bool:
        self.dispatched.append(workflow_id)
        return True


class _TestRedispatcher(WorkflowRedispatcher):
    async def run_tick(self) -> None:
        await self._tick()


# ──────────────────────────────────────────────
# 검증 1: WorkflowRedispatcher가 heartbeat 워크플로우를 라우팅
# ──────────────────────────────────────────────


@pytest.mark.asyncio
async def test_redispatcher_routes_heartbeat(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    stub_hb = _StubHeartbeatService()

    record = rt.store.create(
        source_kind="heartbeat",
        goal="heartbeat 리디스패치 테스트",
        metadata={"heartbeatFile": "/tmp/HEARTBEAT.md"},
    )
    _ = rt.store.upsert(record)
    assert record.state == "queued"

    redispatcher = _TestRedispatcher(
        workflow_runtime=rt,
        cron_service=cast(CronService, cast(object, _StubCronService())),
        heartbeat_service=cast(HeartbeatService, cast(object, stub_hb)),
        poll_interval_s=9999,
    )
    await redispatcher.run_tick()

    assert record.workflow_id in stub_hb.dispatched, (
        f"heartbeat 워크플로우가 dispatch되지 않음: {stub_hb.dispatched}"
    )


# ──────────────────────────────────────────────
# 검증 2: heartbeat_service 없으면 skip (경고 로그)
# ──────────────────────────────────────────────


@pytest.mark.asyncio
async def test_redispatcher_skips_heartbeat_without_service(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)

    record = rt.store.create(
        source_kind="heartbeat",
        goal="서비스 없는 heartbeat",
        metadata={},
    )
    _ = rt.store.upsert(record)

    redispatcher = _TestRedispatcher(
        workflow_runtime=rt,
        cron_service=cast(CronService, cast(object, _StubCronService())),
        heartbeat_service=None,
        poll_interval_s=9999,
    )
    # 예외 없이 skip
    await redispatcher.run_tick()

    # 여전히 queued 상태 (처리되지 않음)
    stored = rt.store.get(record.workflow_id)
    assert stored is not None and stored.state == "queued"


# ──────────────────────────────────────────────
# 검증 3: HeartbeatService.execute_existing_workflow 통합
# ──────────────────────────────────────────────


@pytest.mark.asyncio
async def test_heartbeat_execute_existing_workflow(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    executed: list[str] = []

    async def fake_execute(tasks: str, workflow_id: str) -> str:
        executed.append(workflow_id)
        return f"결과: {tasks}"

    service = HeartbeatService(
        workspace=tmp_path,
        provider=cast(LLMProvider, cast(object, _StubProvider())),
        model="test",
        on_execute=fake_execute,
        enabled=False,
        workflow_runtime=rt,
    )

    # heartbeat 워크플로우 생성
    record = rt.store.create(
        source_kind="heartbeat",
        goal="재실행 테스트 태스크",
        metadata={"heartbeatFile": str(tmp_path / "HEARTBEAT.md")},
    )
    _ = rt.store.upsert(record)

    result = await service.execute_existing_workflow(record.workflow_id)
    assert result is True
    assert record.workflow_id in executed

    # 워크플로우가 completed 상태
    stored = rt.store.get(record.workflow_id)
    assert stored is not None and stored.state == "completed"
    assert "resultPreview" in stored.metadata


# ──────────────────────────────────────────────
# 검증 4: source_kind 불일치 시 False 반환
# ──────────────────────────────────────────────


@pytest.mark.asyncio
async def test_heartbeat_execute_rejects_wrong_source_kind(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)

    async def fake_execute(_tasks: str, _workflow_id: str) -> str:
        return "should not run"

    service = HeartbeatService(
        workspace=tmp_path,
        provider=cast(LLMProvider, cast(object, _StubProvider())),
        model="test",
        on_execute=fake_execute,
        enabled=False,
        workflow_runtime=rt,
    )

    record = rt.store.create(source_kind="cron", goal="cron 태스크", metadata={})
    _ = rt.store.upsert(record)

    result = await service.execute_existing_workflow(record.workflow_id)
    assert result is False


@pytest.mark.asyncio
async def test_heartbeat_execute_rejects_non_queued_state(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)

    async def fake_execute(_tasks: str, _workflow_id: str) -> str:
        return "should not run"

    service = HeartbeatService(
        workspace=tmp_path,
        provider=cast(LLMProvider, cast(object, _StubProvider())),
        model="test",
        on_execute=fake_execute,
        enabled=False,
        workflow_runtime=rt,
    )

    record = rt.store.create(source_kind="heartbeat", goal="running 태스크", metadata={})
    _ = rt.store.upsert(record.model_copy(update={"state": "running"}))

    result = await service.execute_existing_workflow(record.workflow_id)
    assert result is False


@pytest.mark.asyncio
async def test_heartbeat_mark_notified_with_target(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)
    notified: list[str] = []

    async def fake_execute(_tasks: str, _workflow_id: str) -> str:
        return "결과"

    async def fake_notify(response: str, workflow_id: str) -> tuple[str, str] | None:
        notified.append(response)
        assert workflow_id == record.workflow_id
        return ("telegram", "123")

    service = HeartbeatService(
        workspace=tmp_path,
        provider=cast(LLMProvider, cast(object, _StubProvider())),
        model="test",
        on_execute=fake_execute,
        on_notify=fake_notify,
        enabled=False,
        workflow_runtime=rt,
    )

    record = rt.store.create(
        source_kind="heartbeat",
        goal="알림 테스트",
        notify_target=NotifyTarget(channel="telegram", chat_id="123", session_key="tg:123"),
        metadata={},
    )
    _ = rt.store.upsert(record)

    # notify_target 설정
    _ = rt.update_notify_target(
        record.workflow_id, channel="telegram", chat_id="123", session_key="tg:123"
    )

    _ = await service.execute_existing_workflow(record.workflow_id)

    stored = rt.store.get(record.workflow_id)
    assert stored is not None
    assert stored.metadata.get("notifyEnqueued") is True
    assert stored.metadata.get("notifyChannel") == "telegram"
    assert stored.metadata.get("notifyChatId") == "123"
    assert stored.metadata.get("notifyDelivered") is None


# ──────────────────────────────────────────────
# 검증 6: 알림 메타데이터 — notify_target 없으면 mark_notify_delegated
# ──────────────────────────────────────────────


@pytest.mark.asyncio
async def test_heartbeat_mark_delegated_without_target(tmp_path: Path) -> None:
    rt = WorkflowRuntime(workspace=tmp_path)

    async def fake_execute(_tasks: str, _workflow_id: str) -> str:
        return "결과"

    async def fake_notify(_response: str, _workflow_id: str) -> tuple[str, str] | None:
        return None

    service = HeartbeatService(
        workspace=tmp_path,
        provider=cast(LLMProvider, cast(object, _StubProvider())),
        model="test",
        on_execute=fake_execute,
        on_notify=fake_notify,
        enabled=False,
        workflow_runtime=rt,
    )

    record = rt.store.create(
        source_kind="heartbeat",
        goal="위임 알림 테스트",
        metadata={},
    )
    _ = rt.store.upsert(record)

    _ = await service.execute_existing_workflow(record.workflow_id)

    stored = rt.store.get(record.workflow_id)
    assert stored is not None
    # delegated delivery: notifyDelegated=True, notifyChannel 없음
    assert stored.metadata.get("notifyDelegated") is True
    assert stored.metadata.get("notifyChannel") is None


def test_update_notify_target_preserves_notify_target_model() -> None:
    rt = WorkflowRuntime(workspace=Path("/tmp"))
    record = rt.store.create(source_kind="heartbeat", goal="target update", metadata={})
    _ = rt.store.upsert(record)

    updated = rt.update_notify_target(
        record.workflow_id,
        channel="telegram",
        chat_id="123",
        session_key="telegram:123",
    )

    assert updated is not None
    assert isinstance(updated.notify_target, NotifyTarget)
    assert updated.notify_target.channel == "telegram"
    assert updated.notify_target.chat_id == "123"
    assert updated.notify_target.session_key == "telegram:123"
