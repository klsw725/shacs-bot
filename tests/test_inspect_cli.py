from __future__ import annotations

import asyncio
import json
from pathlib import Path

from typer.testing import CliRunner

from shacs_bot.agent import approval as approval_module
from shacs_bot.agent.session import manager as session_manager_module
from shacs_bot.cli import commands
from shacs_bot.config import paths as config_paths
from shacs_bot.config.schema import Config
from shacs_bot.workflow.store import WorkflowStore


runner = CliRunner()


def _make_config(workspace: Path) -> Config:
    return Config.model_validate({"agents": {"defaults": {"workspace": str(workspace)}}})


def _write_session_file(
    sessions_dir: Path,
    *,
    key: str,
    created_at: str,
    updated_at: str,
    metadata: dict[str, object] | None = None,
    message_count: int = 0,
) -> None:
    sessions_dir.mkdir(parents=True, exist_ok=True)
    path = sessions_dir / f"{key.replace(':', '_')}.jsonl"
    lines: list[str] = [
        json.dumps(
            {
                "_type": "metadata",
                "key": key,
                "created_at": created_at,
                "updated_at": updated_at,
                "metadata": metadata or {},
                "last_consolidated": 0,
            },
            ensure_ascii=False,
        )
    ]
    for index in range(message_count):
        lines.append(
            json.dumps(
                {
                    "role": "user",
                    "content": f"message-{index}",
                    "timestamp": updated_at,
                },
                ensure_ascii=False,
            )
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _write_usage_entries(usage_dir: Path, *, date: str, entries: list[dict[str, object]]) -> None:
    usage_dir.mkdir(parents=True, exist_ok=True)
    path = usage_dir / f"{date}.jsonl"
    payload = "\n".join(json.dumps(entry, ensure_ascii=False) for entry in entries)
    path.write_text(payload + "\n", encoding="utf-8")


def test_inspect_sessions_supports_prefix_and_metadata(
    tmp_path: Path,
    monkeypatch,
) -> None:
    workspace = tmp_path / "workspace"
    sessions_dir = tmp_path / "data" / "sessions"
    legacy_dir = tmp_path / "legacy-sessions"

    _write_session_file(
        sessions_dir,
        key="cli:direct",
        created_at="2026-04-03T20:20:00+09:00",
        updated_at="2026-04-03T20:45:00+09:00",
        metadata={"topic": "inspect"},
        message_count=3,
    )
    _write_session_file(
        sessions_dir,
        key="telegram:1",
        created_at="2026-04-03T20:00:00+09:00",
        updated_at="2026-04-03T20:10:00+09:00",
        message_count=1,
    )

    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))
    monkeypatch.setattr(session_manager_module, "get_sessions_dir", lambda: sessions_dir)
    monkeypatch.setattr(session_manager_module, "get_legacy_sessions_dir", lambda: legacy_dir)

    result = runner.invoke(
        commands.app,
        ["inspect", "sessions", "--key-prefix", "cli:", "--show-meta", "--limit", "5"],
    )

    assert result.exit_code == 0, result.stdout
    assert "cli:direct" in result.stdout
    assert "telegram:1" not in result.stdout
    assert "Messages" in result.stdout
    assert "topic" in result.stdout
    assert "3" in result.stdout
    assert "최신순" in result.stdout


def test_inspect_workflows_filters_incomplete_and_state(tmp_path: Path, monkeypatch) -> None:
    workspace = tmp_path / "workspace"
    store = WorkflowStore(workspace)
    captured: dict[str, object] = {}

    queued = store.create(source_kind="manual", goal="queued-alpha")
    store.upsert(queued)

    completed = store.create(source_kind="manual", goal="completed-beta")
    store.upsert(completed.model_copy(update={"state": "completed"}))

    failed = store.create(source_kind="manual", goal="failed-gamma")
    store.upsert(failed.model_copy(update={"state": "failed"}))

    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))
    monkeypatch.setattr(
        commands,
        "_render_workflow_table",
        lambda records, title: captured.update(
            {"ids": [r.workflow_id for r in records], "title": title}
        ),
    )

    default_result = runner.invoke(commands.app, ["inspect", "workflows"], terminal_width=200)
    assert default_result.exit_code == 0, default_result.stdout
    assert captured["ids"] == [queued.workflow_id]
    assert captured["title"] == "Workflows (incomplete)"

    captured.clear()
    filtered_result = runner.invoke(
        commands.app,
        ["inspect", "workflows", "--all", "--state", "completed"],
        terminal_width=200,
    )
    assert filtered_result.exit_code == 0, filtered_result.stdout
    assert captured["ids"] == [completed.workflow_id]
    assert captured["title"] == "Workflows (all)"


def test_inspect_workflows_filters_by_source(tmp_path: Path, monkeypatch) -> None:
    workspace = tmp_path / "workspace"
    store = WorkflowStore(workspace)
    captured: dict[str, object] = {}

    manual = store.create(source_kind="manual", goal="manual-alpha")
    store.upsert(manual)

    cron = store.create(source_kind="cron", goal="cron-beta")
    store.upsert(cron)

    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))
    monkeypatch.setattr(
        commands,
        "_render_workflow_table",
        lambda records, title: captured.update(
            {
                "ids": [r.workflow_id for r in records],
                "sources": [r.source_kind for r in records],
                "title": title,
            }
        ),
    )

    result = runner.invoke(
        commands.app,
        ["inspect", "workflows", "--all", "--source", "cron"],
        terminal_width=200,
    )

    assert result.exit_code == 0, result.stdout
    assert captured["ids"] == [cron.workflow_id]
    assert captured["sources"] == ["cron"]
    assert captured["title"] == "Workflows (all)"


def test_inspect_usage_supports_session_filter(tmp_path: Path, monkeypatch) -> None:
    workspace = tmp_path / "workspace"
    usage_dir = tmp_path / "data" / "usage"
    today = "2026-04-04"
    previous_day = "2026-04-03"

    _write_usage_entries(
        usage_dir,
        date=today,
        entries=[
            {
                "ts": "2026-04-04T10:00:00",
                "session": "cli:direct",
                "model": "anthropic/claude-sonnet-4-5",
                "provider": "anthropic",
                "prompt": 10,
                "completion": 5,
                "cost": 0.12,
                "calls": 1,
            },
            {
                "ts": "2026-04-04T10:05:00",
                "session": "cli:direct",
                "model": "anthropic/claude-sonnet-4-5",
                "provider": "anthropic",
                "prompt": 3,
                "completion": 2,
                "cost": 0.05,
                "calls": 1,
            },
            {
                "ts": "2026-04-04T10:10:00",
                "session": "telegram:1",
                "model": "anthropic/claude-sonnet-4-5",
                "provider": "anthropic",
                "prompt": 7,
                "completion": 1,
                "cost": 0.03,
                "calls": 1,
            },
        ],
    )
    _write_usage_entries(
        usage_dir,
        date=previous_day,
        entries=[
            {
                "ts": "2026-04-03T09:00:00",
                "session": "cli:direct",
                "model": "anthropic/claude-sonnet-4-5",
                "provider": "anthropic",
                "prompt": 4,
                "completion": 1,
                "cost": 0.02,
                "calls": 1,
            }
        ],
    )

    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))
    monkeypatch.setattr(config_paths, "get_usage_dir", lambda: usage_dir)

    session_result = runner.invoke(commands.app, ["inspect", "usage", "--session", "cli:direct"])
    assert session_result.exit_code == 0, session_result.stdout
    assert "all days" in session_result.stdout
    assert "17" in session_result.stdout
    assert "7" in session_result.stdout
    assert "25" in session_result.stdout
    assert "$0.1900" in session_result.stdout
    assert "3" in session_result.stdout

    dated_session_result = runner.invoke(
        commands.app,
        ["inspect", "usage", "--session", "cli:direct", "--date", today],
    )
    assert dated_session_result.exit_code == 0, dated_session_result.stdout
    assert "session: cli:direct" in dated_session_result.stdout
    assert today in dated_session_result.stdout
    assert "13" in dated_session_result.stdout
    assert "7" in dated_session_result.stdout
    assert "20" in dated_session_result.stdout
    assert "$0.1700" in dated_session_result.stdout
    assert "2" in dated_session_result.stdout

    empty_result = runner.invoke(commands.app, ["inspect", "usage", "--date", "2026-04-05"])
    assert empty_result.exit_code == 0, empty_result.stdout
    assert "기록된 사용량이 없습니다." in empty_result.stdout


def test_inspect_approvals_shows_empty_state(monkeypatch, tmp_path: Path) -> None:
    workspace = tmp_path / "workspace"
    approval_module.clear_pending_approvals_for_test()
    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))

    result = runner.invoke(commands.app, ["inspect", "approvals"])

    assert result.exit_code == 0, result.stdout
    assert "대기 중인 승인 요청이 없습니다" in result.stdout
    assert "프로세스 로컬" in result.stdout


def test_inspect_approvals_shows_process_local_pending_requests(
    monkeypatch, tmp_path: Path
) -> None:
    workspace = tmp_path / "workspace"
    loop = asyncio.new_event_loop()
    future = loop.create_future()
    request_id = "cli:direct:abcd1234"

    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))
    approval_module.clear_pending_approvals_for_test()
    approval_module.register_pending_approval_for_test(
        request_id,
        future,
        session_key="cli:direct",
        channel="cli",
        tool_name="exec",
        skill_name="workspace-tool",
    )

    try:
        result = runner.invoke(commands.app, ["inspect", "approvals"])
    finally:
        approval_module.clear_pending_approvals_for_test()
        loop.close()

    assert result.exit_code == 0, result.stdout
    assert request_id in result.stdout
    assert "cli:direct" in result.stdout
    assert "workspace-tool" in result.stdout
    assert "process-local" in result.stdout


def test_status_shows_personal_inspect_summary(tmp_path: Path, monkeypatch) -> None:
    workspace = tmp_path / "workspace"
    config_path = tmp_path / "config.json"
    config_path.write_text("{}", encoding="utf-8")

    sessions_dir = tmp_path / "data" / "sessions"
    usage_dir = tmp_path / "data" / "usage"
    legacy_dir = tmp_path / "legacy-sessions"

    _write_session_file(
        sessions_dir,
        key="cli:direct",
        created_at="2026-04-03T20:20:00+09:00",
        updated_at="2026-04-03T20:45:00+09:00",
        message_count=2,
    )
    _write_session_file(
        sessions_dir,
        key="cli:recent",
        created_at="2026-04-04T08:00:00+09:00",
        updated_at="2026-04-04T08:30:00+09:00",
        message_count=4,
    )
    _write_usage_entries(
        usage_dir,
        date="2026-04-04",
        entries=[
            {
                "ts": "2026-04-04T10:00:00",
                "session": "cli:direct",
                "model": "anthropic/claude-sonnet-4-5",
                "provider": "anthropic",
                "prompt": 8,
                "completion": 4,
                "cost": 0.11,
                "calls": 1,
            }
        ],
    )

    store = WorkflowStore(workspace)
    queued = store.create(source_kind="manual", goal="queued-alpha")
    store.upsert(queued)
    cron = store.create(source_kind="cron", goal="cron-beta")
    store.upsert(cron)

    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))
    monkeypatch.setattr(commands, "get_config_path", lambda: config_path)
    monkeypatch.setattr(session_manager_module, "get_sessions_dir", lambda: sessions_dir)
    monkeypatch.setattr(session_manager_module, "get_legacy_sessions_dir", lambda: legacy_dir)
    monkeypatch.setattr(config_paths, "get_usage_dir", lambda: usage_dir)

    result = runner.invoke(commands.app, ["status"], terminal_width=200)

    assert result.exit_code == 0, result.stdout
    assert "Personal Inspect Summary" in result.stdout
    assert "Sessions" in result.stdout
    assert "2" in result.stdout
    assert "Recent session" in result.stdout
    assert "cli:recent" in result.stdout
    assert "Incomplete workflows" in result.stdout
    assert "Recent workflow" in result.stdout
    assert "queued" in result.stdout
    assert "cron" in result.stdout
    assert "Today's usage" in result.stdout
    assert "$0.1100" in result.stdout
    assert "Recent usage session" in result.stdout
    assert "Pending approvals" in result.stdout
    assert "상세 조회:" in result.stdout
    assert "inspect sessions" in result.stdout
    assert "inspect workflows" in result.stdout
    assert "inspect usage" in result.stdout
    assert "approvals" in result.stdout


def test_status_handles_empty_personal_summary(tmp_path: Path, monkeypatch) -> None:
    workspace = tmp_path / "workspace"
    config_path = tmp_path / "config.json"
    config_path.write_text("{}", encoding="utf-8")

    sessions_dir = tmp_path / "data" / "sessions"
    usage_dir = tmp_path / "data" / "usage"
    legacy_dir = tmp_path / "legacy-sessions"

    monkeypatch.setattr(commands, "load_config", lambda: _make_config(workspace))
    monkeypatch.setattr(commands, "get_config_path", lambda: config_path)
    monkeypatch.setattr(session_manager_module, "get_sessions_dir", lambda: sessions_dir)
    monkeypatch.setattr(session_manager_module, "get_legacy_sessions_dir", lambda: legacy_dir)
    monkeypatch.setattr(config_paths, "get_usage_dir", lambda: usage_dir)

    result = runner.invoke(commands.app, ["status"], terminal_width=200)

    assert result.exit_code == 0, result.stdout
    assert "Personal Inspect Summary" in result.stdout
    assert "Recent session" in result.stdout
    assert "Recent workflow" in result.stdout
    assert "0 calls · 0 tokens · $0.0000" in result.stdout
    assert "Recent usage session" in result.stdout
    assert "0 (process-local)" in result.stdout
    assert "-" in result.stdout
