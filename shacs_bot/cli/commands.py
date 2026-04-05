"""shacs-bot CLI 커멘드"""

import asyncio
from datetime import datetime
import os
import select
import signal
import sys
from pathlib import Path
from typing import Text, TYPE_CHECKING

import typer
from loguru import logger
from prompt_toolkit import PromptSession, HTML
from prompt_toolkit.history import FileHistory
from prompt_toolkit.patch_stdout import patch_stdout
from rich.console import Console
from rich.markdown import Markdown
from rich.table import Table

from shacs_bot import __logo__, __version__
from shacs_bot.agent.hooks import HookRegistry, NoOpHookRegistry, register_example_hooks
from shacs_bot.agent.loop import AgentLoop
from shacs_bot.agent.tools.cron.service import CronService
from shacs_bot.bus.events import OutboundMessage, InboundMessage
from shacs_bot.bus.networks import MessageBus
from shacs_bot.config.loader import load_config, get_config_path
from shacs_bot.config.paths import (
    get_cli_history_path,
    get_cron_dir,
    get_data_dir,
    get_bridge_install_dir,
)
from shacs_bot.config.schema import (
    Config,
    ProviderConfig,
    HeartbeatConfig,
    ChannelsConfig,
    WhatsAppConfig,
    DiscordConfig,
    FeishuConfig,
    MochatConfig,
    TelegramConfig,
    SlackConfig,
    DingTalkConfig,
    QQConfig,
    EmailConfig,
)
from shacs_bot.providers.base import LLMProvider
from shacs_bot.providers.registry import ProviderSpec, PROVIDERS
from shacs_bot.utils.helpers import sync_workspace_template

if TYPE_CHECKING:
    from shacs_bot.workflow import WorkflowRecord


def _resolve_media_key(config: Config) -> str | None:
    return config.providers.image_gen.api_key or None


def _resolve_media_base_url(config: Config) -> str | None:
    return config.providers.image_gen.base_url or None


def _resolve_runtime_model(
    config: Config,
    *,
    model_override: str | None = None,
    use_state_recommendation: bool = True,
) -> str:
    from shacs_bot.evals import read_auto_eval_state

    model: str = model_override or config.agents.defaults.model
    if (
        use_state_recommendation
        and config.agents.defaults.provider == "auto"
        and model_override is None
    ):
        state = read_auto_eval_state(config.workspace_path)
        if state and state.recommended_provider_name and state.recommended_model:
            model = state.recommended_model
    return model


# 윈도우 콘솔을 위한 강제 UTF-8 인코딩
if sys.platform == "win32":
    import locale

    if sys.stdout.encoding != "utf-8":
        os.environ["PYTHONIOENCODING"] = "utf-8"
        # utf-8 인코딩으로 stdout/stderr 재 오픈
        try:
            sys.stdout.reconfigure(encoding="utf-8", errors="replace")
            sys.stderr.reconfigure(encoding="utf-8", errors="replace")
        except Exception:
            pass


app: typer.Typer = typer.Typer(
    name="shacs-bot",
    help=f"{__logo__} shacs-bot - Personal AI Assistant",
    no_args_is_help=True,
)

console: Console = Console()
EXIT_COMMANDS: set[str] = {"exit", "quit", "/exit", "quit", ":q"}

# ---------------------------------------------------------------------------
# CLI input: prompt_toolkit for editing, paste, history, and display
# ---------------------------------------------------------------------------

_PROMPT_SESSION: PromptSession | None = None
_SAVED_TERM_ATTRS: list | None = None  # 종료 시 복원되는 원래의 termios 설정


def _flush_pending_tty_input() -> None:
    """모델이 출력을 생성하는 동안 입력되었지만 아직 읽히지 않은 키 입력을 버린다."""
    try:
        fd: int = sys.stdin.fileno()
        if not os.isatty(fd):
            return
    except Exception:
        return

    try:
        import termios

        termios.tcflush(fd, termios.TCIFLUSH)
    except Exception:
        pass

    try:
        while True:
            ready, _, _ = select.select([fd], [], [], 0)
            if not ready:
                break

            if not os.read(fd, 4096):
                break
    except Exception:
        return


def _restore_terminal() -> None:
    """터미널을 원래 상태(에코, 줄 단위 버퍼링 등)로 복원한다."""
    if _SAVED_TERM_ATTRS is None:
        return

    try:
        import termios

        termios.tcsetattr(sys.stdin.fileno(), termios.TCSADRAIN, _SAVED_TERM_ATTRS)
    except Exception:
        pass


def _init_prompt_session() -> None:
    """지속적인 파일 히스토리를 사용하는 prompt_toolkit 세션을 생성한다."""
    global _PROMPT_SESSION, _SAVED_TERM_ATTRS

    # 종료 시 복원할 수 있도록 터미널 상태를 저장한다.
    try:
        import termios

        _SAVED_TERM_ATTRS = termios.tcgetattr(sys.stdin.fileno())
    except Exception:
        pass

    history_file: Path = get_cli_history_path()
    history_file.parent.mkdir(parents=True, exist_ok=True)

    _PROMPT_SESSION = PromptSession(
        history=FileHistory(str(history_file)),
        enable_open_in_editor=False,
        multiline=False,  # 엔터 전송 (싱글 라인 모드)
    )


def _print_agent_response(response: str, render_markdown: bool) -> None:
    """일관된 터미널 스타일로 어시스턴트 응답을 렌더링한다."""
    content: str = response or ""
    body: Markdown | Text = Markdown(content) if render_markdown else Text(content)

    console.print()
    console.print(f"[cyan]{__logo__} shacs-bot[/cyan]")
    console.print(body)
    console.print()


def _is_exit_command(command: str) -> bool:
    """입력이 인터랙티브 채팅을 종료해야 하는 경우 True를 반환한다."""
    return command.lower() in EXIT_COMMANDS


async def _read_interactive_input_async() -> str:
    """
    prompt_toolkit을 사용하여 사용자 입력을 읽는다 (붙여넣기, 히스토리, 화면 표시 처리).

    prompt_toolkit은 기본적으로 다음을 처리한다:
        •	멀티라인 붙여넣기 (bracketed paste mode)
        •	히스토리 탐색 (위/아래 화살표)
        •	깨끗한 화면 표시 (고스트 문자나 화면 깨짐 없음)
    """
    if _PROMPT_SESSION is None:
        raise RuntimeError("_init_prompt_session()을 먼저 호출하세요.")

    try:
        with patch_stdout():
            return await _PROMPT_SESSION.prompt_async(
                HTML("<b fg='ansiblue'>You:</b> "),
            )
    except EOFError as exc:
        raise KeyboardInterrupt from exc


def version_callback(value: bool) -> None:
    if value:
        console.print(f"{__logo__} shacs-bot v{__version__}")
        raise typer.Exit()


# ============================================================================
# Onboard / Setup
# ============================================================================


@app.callback()
def main(
    version: bool = typer.Option(
        None,
        "--version",
        "-v",
        callback=version_callback,
        is_eager=True,
    ),
):
    """shacs-bot - Personal AI Assistant."""
    pass


@app.command()
def onboard(
    wizard: bool = typer.Option(False, "--wizard", "-w", help="대화형 설정 마법사"),
):
    """shacs-bot 워크스페이스와 설정 초기화"""
    from shacs_bot.config.loader import get_config_path, load_config, save_config
    from shacs_bot.config.paths import get_workspace_path
    from shacs_bot.utils.helpers import sync_workspace_template

    config_path: Path = get_config_path()

    if wizard:
        from shacs_bot.cli.onboard import run_onboard

        existing = load_config(config_path) if config_path.exists() else None
        result = run_onboard(initial_config=existing)
        if result is None:
            console.print("[yellow]설정 변경을 저장하지 않았습니다.[/yellow]")
            return

        save_config(result, config_path)
        console.print(f"[green]✓[/green] {config_path}에 설정을 저장했습니다.")
    elif config_path.exists():
        console.print(f"[yellow]{config_path} 위치에 이미 설정 파일이 존재합니다.[/yellow]")
        console.print("  [bold]y[/bold] = 기본값으로 덮어쓰기 (기존 값은 모두 삭제됩니다)")
        console.print("  [bold]N[/bold] = 기존 값을 유지하면서 새 필드만 추가하여 설정을 갱신")

        if typer.confirm("덮어쓰시겠습니까?"):
            config: Config = Config()
            save_config(config)

            console.print(f"[green]✓[/green] {config_path}에서 설정이 기본값으로 초기화되었습니다.")
        else:
            config: Config = load_config(config_path)
            save_config(config)

            console.print(
                f"[green]✓[/green] {config_path}의 설정이 갱신되었습니다. (기존 값은 유지됨)"
            )
    else:
        save_config(Config())

        console.print(f"[green]✓[/green] {config_path}에 설정 파일을 생성했습니다.")

    workspace: Path = get_workspace_path()
    if not workspace.exists():
        workspace.mkdir(parents=True, exist_ok=True)

        console.print(f"[green]✓[/green] {workspace}에 워크스페이스를 생성했습니다.")

    sync_workspace_template(workspace)

    if not wizard:
        console.print(f"\n{__logo__} shacs-bot이 준비되었습니다!")
        console.print("\n다음 단계:")
        console.print("  1. [cyan]~/.shacs-bot/config.json[/cyan] 파일에 API 키를 추가하세요")
        console.print("     발급: https://openrouter.ai/keys")
        console.print('  2. 채팅 시작: [cyan]shacs-bot agent -m "Hello!"[/cyan]')
        console.print(
            "\n[dim]Telegram / WhatsApp 연동이 필요하신가요? https://github.com/klsw725/shacs-bot?tab=readme-ov-file#%EC%B1%84%EB%84%90 참고[/dim]"
        )


def _make_provider(
    config: Config,
    *,
    model_override: str | None = None,
    provider_name_override: str | None = None,
    use_state_recommendation: bool = True,
) -> LLMProvider:
    from shacs_bot.providers.registry import find_by_name

    model: str = _resolve_runtime_model(
        config,
        model_override=model_override,
        use_state_recommendation=use_state_recommendation,
    )
    provider_name: str | None = provider_name_override or config.get_provider_name(model)
    provider: ProviderConfig | None = (
        getattr(config.providers, provider_name_override, None)
        if provider_name_override
        else config.get_provider(model)
    )
    spec: ProviderSpec | None = find_by_name(provider_name) if provider_name else None

    if (
        not model.startswith("bedrock/")
        and not (provider and provider.api_key)
        and not (spec and spec.is_oauth)
        and not (spec and spec.is_local)
    ):
        if provider_name and spec:
            env_hint: str = f" 또는 환경변수 {spec.env_key}" if spec.env_key else ""
            console.print(f"[red]에러: provider '{spec.label}'의 API 키가 없습니다.[/red]")
            console.print(
                f"  설정: ~/.shacs-bot/config.json → providers.{provider_name}.apiKey{env_hint}"
            )
        else:
            console.print(f"[red]에러: 모델 '{model}'에 맞는 provider를 찾을 수 없습니다.[/red]")
            console.print("  ~/.shacs-bot/config.json의 providers 섹션에 API 키를 설정하세요.")
        raise typer.Exit(1)

    backend = spec.backend if spec else "openai_compat"
    api_key = provider.api_key if provider else None
    base_url = config.get_base_url(model)
    extra_headers = provider.extra_headers if provider else None

    if backend == "openai_codex" or model.startswith("openai-codex/"):
        from shacs_bot.providers.openai_codex import OpenAICodexProvider

        return OpenAICodexProvider(default_model=model)

    if backend == "anthropic":
        from shacs_bot.providers.anthropic_provider import AnthropicProvider

        return AnthropicProvider(
            api_key=api_key,
            base_url=base_url,
            default_model=model,
            extra_headers=extra_headers,
        )

    from shacs_bot.providers.openai_compat_provider import OpenAICompatProvider

    return OpenAICompatProvider(
        api_key=api_key,
        base_url=base_url,
        default_model=model,
        extra_headers=extra_headers,
        spec=spec,
    )


# ============================================================================
# Gateway / Server
# ============================================================================


@app.command()
def gateway(
    port: int = typer.Option(18790, "--port", "-p", help="게이트웨이 포트"),
    workspace: str | None = typer.Option(None, "--workspace", "-w", help="workspace 디렉토리"),
    config: str | None = typer.Option(None, "--config", "-c", help="Config 파일 경로"),
    verbose: bool = typer.Option(False, "--verbose", "-v", help="자세한 출력"),
) -> None:
    """shacs-bot 게이트웨이 시작"""
    import logging
    from shacs_bot.agent.loop import AgentLoop
    from shacs_bot.bus.networks import MessageBus
    from shacs_bot.channels.manager import ChannelManager
    from shacs_bot.config.loader import load_config
    from shacs_bot.agent.tools.cron.service import CronService
    from shacs_bot.agent.tools.cron.types import CronJob
    from shacs_bot.heartbeat.service import HeartbeatService
    from shacs_bot.agent.session.manager import SessionManager
    from shacs_bot.workflow import WorkflowRuntime
    from shacs_bot.workflow.redispatcher import WorkflowRedispatcher

    if verbose:
        logging.basicConfig(level=logging.DEBUG)

    config_path: Path | None = Path(config) if config else None
    config: Config = load_config(config_path)

    if workspace:
        config.agents.defaults.workspace = workspace

    console.print(f"{__logo__} shacs-bot 게이트웨이 포트 번오 {port}에서 시작 중...")
    sync_workspace_template(config.workspace_path)

    from shacs_bot.observability.tracing import init_tracing

    init_tracing(config)

    hooks: HookRegistry = HookRegistry() if config.hooks.enabled else NoOpHookRegistry()
    if config.hooks.enabled:
        register_example_hooks(hooks, redact_payloads=config.hooks.redact_payloads)
    bus: MessageBus = MessageBus()
    provider = _make_provider(config)
    session_manager: SessionManager = SessionManager(config.workspace_path)
    workflow_runtime: WorkflowRuntime = WorkflowRuntime(config.workspace_path)
    recovered_workflows = workflow_runtime.recover_restart()

    # 먼저, 크론 서비스 생성 (에이전트 생성 이후에 callback 설정)
    cron_store_path: Path = get_cron_dir() / "jobs.json"
    cron: CronService = CronService(cron_store_path, workflow_runtime=workflow_runtime)

    provider.generation.temperature = config.agents.defaults.temperature
    provider.generation.max_tokens = config.agents.defaults.max_tokens
    provider.generation.reasoning_effort = config.agents.defaults.reasoning_effort

    from shacs_bot.providers.failover import FailoverManager

    gw_provider_name: str | None = config.get_provider_name()
    failover: FailoverManager | None = FailoverManager(config) if config.failover.enabled else None

    agent_loop: AgentLoop = AgentLoop(
        bus=bus,
        provider=provider,
        workspace=config.workspace_path,
        model=config.agents.defaults.model,
        max_iterations=config.agents.defaults.max_tool_iterations,
        memory_window=config.agents.defaults.memory_window,
        brave_api_key=config.tools.web.search.api_key or None,
        web_proxy=config.tools.web.proxy or None,
        exec_config=config.tools.exec,
        cron_service=cron,
        restrict_to_workspace=config.tools.restrict_to_workspace,
        session_manager=session_manager,
        mcp_servers=config.tools.mcp_servers,
        channels_config=config.channels,
        failover_manager=failover,
        provider_name=gw_provider_name,
        usage_config=config.usage,
        policy_config=config.policy,
        media_config=config.tools.media,
        media_api_key=_resolve_media_key(config),
        media_base_url=_resolve_media_base_url(config),
        skill_approval=config.tools.skill_approval,
        hooks=hooks,
        workflow_runtime=workflow_runtime,
    )
    redispatcher: WorkflowRedispatcher = WorkflowRedispatcher(
        workflow_runtime=workflow_runtime,
        cron_service=cron,
        subagent_manager=agent_loop.subagent_manager,
        agent_loop=agent_loop,
    )

    # 크론 callback 설정 (에이전트 필요)
    async def on_cron_job(job: CronJob) -> str | None:
        """에이전트를 통해 cron job 실행"""
        from _contextvars import Token
        from shacs_bot.agent.tools.base import Tool
        from shacs_bot.agent.tools.cron.cron import CronTool
        from shacs_bot.agent.tools.message import MessageTool
        from shacs_bot.bus.events import OutboundMessage
        from shacs_bot.evals import AutoEvalService

        if job.payload.metadata.get("eval_trigger"):
            service = AutoEvalService(config.workspace_path, agent_loop, session_manager)
            result = await service.run_auto_eval(
                triggered=True,
                trigger_session_key=f"scheduled:{job.id}",
                include_eval_sessions=False,
            )
            workflow_id_obj = job.payload.metadata.get("workflowId")
            workflow_id = workflow_id_obj if isinstance(workflow_id_obj, str) else ""
            if workflow_id:
                _ = workflow_runtime.annotate_result(
                    workflow_id,
                    f"self-eval completed: {result.state.last_auto_run_id}",
                )
            return f"self-eval completed: {result.state.last_auto_run_id}"

        reminder_note: str = (
            "[예약된 작업] 타이머가 완료되었습니다.\n\n"
            f"작업 '{job.name}'이(가) 실행되었습니다.\n"
            f"예약된 지시: {job.payload.message}"
        )

        # 실행 중에는 에이전트가 새로운 cron 작업을 예약하지 못하도록 합니다.
        cron_tool: Tool = agent_loop.tools.get("cron")
        cron_token: Token | None = None
        if isinstance(cron_tool, CronTool):
            cron_token = cron_tool.set_cron_context(True)

        try:
            response: str = await agent_loop.process_direct(
                content=reminder_note,
                session_key=f"cron:{job.id}",
                channel=job.payload.channel or "cli",
                chat_id=job.payload.to or "direct",
            )
        finally:
            if isinstance(cron_tool, CronTool) and cron_token is not None:
                cron_tool.reset_cron_context(cron_token)

        message_tool: Tool = agent_loop.tools.get("message")
        if isinstance(message_tool, MessageTool) and message_tool.sent_in_turn:
            workflow_id_obj = job.payload.metadata.get("workflowId")
            workflow_id = workflow_id_obj if isinstance(workflow_id_obj, str) else ""
            if workflow_id:
                _ = workflow_runtime.mark_notify_delegated(workflow_id)
            return response

        if job.payload.deliver and job.payload.to and response:
            await bus.publish_outbound(
                OutboundMessage(
                    channel=job.payload.channel or "cli",
                    chat_id=job.payload.to,
                    content=response,
                    metadata=job.payload.metadata or {},
                )
            )
            workflow_id_obj = job.payload.metadata.get("workflowId")
            workflow_id = workflow_id_obj if isinstance(workflow_id_obj, str) else ""
            if workflow_id:
                _ = workflow_runtime.mark_notified(
                    workflow_id,
                    channel=job.payload.channel or "cli",
                    chat_id=job.payload.to,
                )

        return response

    cron.set_job(on_cron_job)
    from shacs_bot.evals import AutoEvalService

    _ = AutoEvalService(config.workspace_path, agent_loop, session_manager).sync_schedule(cron)

    hb_cfg: HeartbeatConfig = config.gateway.heartbeat
    # 채널 관리자 생성
    channels: ChannelManager = ChannelManager(config, bus, hooks=hooks)
    if channels.enabled_channels:
        console.print(f"[green]✓[/green] 활성화된 채널: {', '.join(channels.enabled_channels)}")
    else:
        console.print("[yellow]경고: 활성화된 채널이 없습니다[/yellow]")

    cron_status = cron.status()
    if cron_status["jobs"] > 0:
        console.print(f"[green]✓[/green] Cron: 예약된 작업 {cron_status['jobs']}개")
    if recovered_workflows:
        console.print(f"[green]✓[/green] Workflow recovery: {len(recovered_workflows)}개 복구")

    console.print(f"[green]✓[/green] Heartbeat: {hb_cfg.interval_s}s 마다 실행")

    async def on_heartbeat_notify(response: str) -> None:
        """heartbeat 응답 유저 채널의 전달"""
        from shacs_bot.bus.events import OutboundMessage

        channel, chat_id = await _pick_heartbeat_target()
        if channel == "cli":
            return  # 전달 가능한 외부 채널 존재하지 않음.

        await bus.publish_outbound(
            OutboundMessage(channel=channel, chat_id=chat_id, content=response)
        )

    async def _pick_heartbeat_target() -> tuple[str, str]:
        """heartbeat에 의해 트리거된 메시지를 전달할 수 있는 채널/채팅 대상(routable target)을 선택한다."""
        enabled: set[str] = set(channels.enabled_channels)

        # 활성화된 채널에서 내부용이 아닌 세션 중 가장 최근에 업데이트된 세션을 우선 선택한다.
        for item in session_manager.list_sessions():
            key: str = item.get("key") or ""
            if ":" not in key:
                continue

            channel, chat_id = key.split(":", 1)
            if channel in {"cli", "system"}:
                continue
            if channel in enabled and chat_id:
                return channel, chat_id

        # Fallback은 기존 동작을 유지하면서도 명시적으로 처리한다
        return "cli", "direct"

    async def on_heartbeat_execute(tasks: str, workflow_id: str) -> str:
        """Phase 2: full 에이전트 로프를 통해 heartbeat 태스크 실행"""
        channel, chat_id = await _pick_heartbeat_target()
        session_key = f"{channel}:{chat_id}"
        if workflow_id:
            _ = workflow_runtime.update_notify_target(
                workflow_id,
                channel=channel,
                chat_id=chat_id,
                session_key=session_key,
            )

        async def _silent(*_args, **_kwargs):
            pass

        return await agent_loop.process_direct(
            content=tasks,
            session_key=session_key,
            channel=channel,
            chat_id=chat_id,
            on_progress=_silent,
        )

    heartbeat: HeartbeatService = HeartbeatService(
        workspace=config.workspace_path,
        provider=provider,
        model=agent_loop.model,
        on_execute=on_heartbeat_execute,
        on_notify=on_heartbeat_notify,
        interval_s=hb_cfg.interval_s,
        enabled=hb_cfg.enabled,
        hooks=hooks,
        workflow_runtime=workflow_runtime,
    )

    async def run() -> None:
        try:
            await cron.start()
            await redispatcher.start()
            await heartbeat.start()
            await asyncio.gather(agent_loop.run(), channels.start_all())
        except KeyboardInterrupt:
            console.print("\n종료 중...")
        finally:
            await agent_loop.close_mcp()
            heartbeat.stop()
            cron.stop()
            redispatcher.stop()
            agent_loop.stop()
            await channels.stop_all()

    asyncio.run(run())


# ============================================================================
# Agent Commands
# ============================================================================


@app.command()
def agent(
    message: str = typer.Option(None, "--message", "-m", help="에이전트에게 보낼 메시지"),
    session_id: str = typer.Option("cli:direct", "--session", "-s", help="Session ID"),
    markdown: bool = typer.Option(
        True, "--markdown/--no-markdown", help="어시스턴트 출력을 Markdown 형식으로 렌더링합니다."
    ),
    logs: bool = typer.Option(
        False, "--logs/--no-logs", help="채팅 중 shacs-bot 런타임 로그를 표시합니다."
    ),
):
    """에이전트와 직접 상호작용합니다."""

    config: Config = load_config()
    sync_workspace_template(workspace=config.workspace_path)

    from shacs_bot.observability.tracing import init_tracing

    init_tracing(config)

    hooks: HookRegistry = HookRegistry() if config.hooks.enabled else NoOpHookRegistry()
    if config.hooks.enabled:
        register_example_hooks(hooks, redact_payloads=config.hooks.redact_payloads)
    bus: MessageBus = MessageBus()
    provider = _make_provider(config)
    from shacs_bot.workflow.runtime import WorkflowRuntime

    # 도구 사용을 위한 cron 서비스를 생성합니다 (CLI에서는 실행 중이 아닌 한 콜백이 필요하지 않습니다).
    cron_store_path: Path = get_cron_dir() / "jobs.json"
    workflow_runtime: WorkflowRuntime = WorkflowRuntime(config.workspace_path)
    cron: CronService = CronService(cron_store_path, workflow_runtime=workflow_runtime)

    if logs:
        logger.enable("shacs-bot")
    else:
        logger.disable("shacs-bot")

    provider.generation.temperature = config.agents.defaults.temperature
    provider.generation.max_tokens = config.agents.defaults.max_tokens
    provider.generation.reasoning_effort = config.agents.defaults.reasoning_effort

    from shacs_bot.providers.failover import FailoverManager

    cli_provider_name: str | None = config.get_provider_name()
    cli_failover: FailoverManager | None = (
        FailoverManager(config) if config.failover.enabled else None
    )

    agent_loop: AgentLoop = AgentLoop(
        bus=bus,
        provider=provider,
        workspace=config.workspace_path,
        model=config.agents.defaults.model,
        max_iterations=config.agents.defaults.max_tool_iterations,
        memory_window=config.agents.defaults.memory_window,
        brave_api_key=config.tools.web.search.api_key or None,
        web_proxy=config.tools.web.proxy or None,
        exec_config=config.tools.exec,
        cron_service=cron,
        restrict_to_workspace=config.tools.restrict_to_workspace,
        mcp_servers=config.tools.mcp_servers,
        channels_config=config.channels,
        failover_manager=cli_failover,
        provider_name=cli_provider_name,
        usage_config=config.usage,
        policy_config=config.policy,
        media_config=config.tools.media,
        media_api_key=_resolve_media_key(config),
        media_base_url=_resolve_media_base_url(config),
        skill_approval=config.tools.skill_approval,
        hooks=hooks,
        workflow_runtime=workflow_runtime,
    )

    if message:
        # 싱글 메시지 모드 - 직접 call, 버스는 필요하지 않음

        def _thinking_ctx():
            from contextlib import contextmanager

            @contextmanager
            def _thinking():
                console.print("[dim]shacs-bot 생각 중...[/dim]")
                yield

            return _thinking()

        async def _cli_progress(
            content: str, *, tool_hint: bool = False, skill_hint: bool = False
        ) -> None:
            ch: ChannelsConfig = agent_loop.channels_config
            if not skill_hint:
                if ch and tool_hint and not ch.send_tool_hints:
                    return
                if ch and not tool_hint and not ch.send_progress:
                    return
            console.print(f"  [dim]↳ {content}[/dim]")

        async def run_once():
            with _thinking_ctx():
                response: str = await agent_loop.process_direct(
                    content=message, session_key=session_id, on_progress=_cli_progress
                )

            _print_agent_response(response=response, render_markdown=markdown)
            await agent_loop.close_mcp()

        asyncio.run(run_once())
    else:
        # 인터렉티브 모드 - 버스를 통해서 다른 채널들로 라우팅합니다.
        _init_prompt_session()

        console.print(
            f"{__logo__} 인터랙티브 모드 (종료하려면 [bold]exit[/bold] 또는 [bold]Ctrl+C[/bold] 입력)\n"
        )

        if ":" in session_id:
            cli_channel, cli_chat_id = session_id.split(":", 1)
        else:
            cli_channel, cli_chat_id = "cli", session_id

        def _handle_signal(signum: int, frame):
            sig_name: str = signal.Signals(signum).name
            _restore_terminal()
            console.print(f"\nReceived {sig_name}, goodbye!")

            sys.exit(0)

        signal.signal(signal.SIGINT, _handle_signal)
        signal.signal(signal.SIGTERM, _handle_signal)

        # SIGHUP은 Windows에서 사용할 수 없습니다.
        if hasattr(signal, "SIGHUP"):
            signal.signal(signal.SIGHUP, _handle_signal)

        # 닫힌 파이프에 쓰기 작업을 할 때 프로세스가 조용히 종료되는 것을 방지하기 위해 SIGPIPE를 무시합니다.
        # SIGPIPE Windows에서 사용할 수 없습니다.
        if hasattr(signal, "SIGPIPE"):
            signal.signal(signal.SIGPIPE, signal.SIG_IGN)

        async def run_interactive():
            bus_task: asyncio.Task = asyncio.create_task(agent_loop.run())

            turn_done: asyncio.Event = asyncio.Event()
            turn_done.set()

            turn_response: list[str] = []

            async def _consume_outbound():
                while True:
                    try:
                        msg: OutboundMessage = await asyncio.wait_for(
                            bus.consume_outbound(), timeout=1.0
                        )
                        if msg.metadata.get("_progress"):
                            is_skill_hint: bool = msg.metadata.get("_skill_hint", False)
                            is_memory_hint: bool = msg.metadata.get("_memory_hint", False)
                            is_tool_hint: bool = msg.metadata.get("_tool_hint", False)
                            ch: ChannelsConfig = agent_loop.channels_config
                            if is_skill_hint:
                                console.print(f"  [dim]↳ {msg.content}[/dim]")
                            elif is_memory_hint:
                                if not ch or ch.send_memory_hints:
                                    console.print(f"  [dim]↳ {msg.content}[/dim]")
                            elif ch and is_tool_hint and not ch.send_tool_hints:
                                pass
                            elif ch and not is_tool_hint and not ch.send_progress:
                                pass
                            else:
                                console.print(f"  [dim]↳ {msg.content}[/dim]")
                        elif not turn_done.is_set():
                            if msg.content:
                                turn_response.append(msg.content)

                            turn_done.set()
                        elif msg.content:
                            console.print()
                            _print_agent_response(msg.content, render_markdown=markdown)
                    except asyncio.TimeoutError:
                        continue
                    except asyncio.CancelledError:
                        break

            outbound_task: asyncio.Task = asyncio.create_task(_consume_outbound())

            try:
                while True:
                    try:
                        _flush_pending_tty_input()

                        user_input: str = await _read_interactive_input_async()
                        command: str = user_input.strip()
                        if not command:
                            continue

                        if _is_exit_command(command):
                            _restore_terminal()
                            console.print("\nGoodbye!")
                            break

                        turn_done.clear()
                        turn_response.clear()

                        await bus.publish_inbound(
                            InboundMessage(
                                channel=cli_channel,
                                sender_id="user",
                                chat_id=cli_chat_id,
                                content=user_input,
                            )
                        )

                        with _thinking_ctx():
                            await turn_done.wait()

                        if turn_response:
                            _print_agent_response(turn_response[0], render_markdown=markdown)
                    except KeyboardInterrupt:
                        _restore_terminal()
                        console.print("\nGoodbye!")
                        break
                    except EOFError:
                        _restore_terminal()
                        console.print("\nGoodbye!")
                        break
            finally:
                agent_loop.stop()
                outbound_task.cancel()
                await asyncio.gather(bus_task, outbound_task, return_exceptions=True)
                await agent_loop.close_mcp()

        asyncio.run(run_interactive())


eval_app = typer.Typer(help="Evaluation harness commands")
app.add_typer(eval_app, name="eval")


def create_eval_runtime(
    config: Config,
    *,
    model_override: str | None = None,
    provider_name_override: str | None = None,
    use_state_recommendation: bool = True,
) -> tuple[AgentLoop, "SessionManager"]:
    from shacs_bot.agent.session.manager import SessionManager
    from shacs_bot.providers.failover import FailoverManager
    from shacs_bot.workflow.runtime import WorkflowRuntime

    sync_workspace_template(workspace=config.workspace_path)

    from shacs_bot.observability.tracing import init_tracing

    init_tracing(config)

    hooks: HookRegistry = HookRegistry() if config.hooks.enabled else NoOpHookRegistry()
    if config.hooks.enabled:
        register_example_hooks(hooks, redact_payloads=config.hooks.redact_payloads)

    bus: MessageBus = MessageBus()
    effective_model: str = _resolve_runtime_model(
        config,
        model_override=model_override,
        use_state_recommendation=use_state_recommendation,
    )
    provider = _make_provider(
        config,
        model_override=model_override,
        provider_name_override=provider_name_override,
        use_state_recommendation=use_state_recommendation,
    )
    session_manager: SessionManager = SessionManager(config.workspace_path)
    workflow_runtime: WorkflowRuntime = WorkflowRuntime(config.workspace_path)
    cron_store_path: Path = get_cron_dir() / "jobs.json"
    cron: CronService = CronService(cron_store_path, workflow_runtime=workflow_runtime)

    provider.generation.temperature = config.agents.defaults.temperature
    provider.generation.max_tokens = config.agents.defaults.max_tokens
    provider.generation.reasoning_effort = config.agents.defaults.reasoning_effort

    cli_provider_name: str | None = provider_name_override or config.get_provider_name(
        effective_model
    )
    cli_failover: FailoverManager | None = (
        FailoverManager(config) if config.failover.enabled else None
    )

    agent_loop: AgentLoop = AgentLoop(
        bus=bus,
        provider=provider,
        workspace=config.workspace_path,
        model=effective_model,
        max_iterations=config.agents.defaults.max_tool_iterations,
        memory_window=config.agents.defaults.memory_window,
        brave_api_key=config.tools.web.search.api_key or None,
        web_proxy=config.tools.web.proxy or None,
        exec_config=config.tools.exec,
        cron_service=cron,
        restrict_to_workspace=config.tools.restrict_to_workspace,
        session_manager=session_manager,
        mcp_servers=config.tools.mcp_servers,
        channels_config=config.channels,
        failover_manager=cli_failover,
        provider_name=cli_provider_name,
        usage_config=config.usage,
        policy_config=config.policy,
        media_config=config.tools.media,
        media_api_key=_resolve_media_key(config),
        media_base_url=_resolve_media_base_url(config),
        skill_approval=config.tools.skill_approval,
        hooks=hooks,
        workflow_runtime=workflow_runtime,
    )
    return agent_loop, session_manager


@eval_app.command("run")
def eval_run(
    cases: Path | None = typer.Argument(None, help="평가 케이스 JSON 파일 경로"),
    variant: list[str] | None = typer.Option(None, "--variant", help="평가 variant preset 이름"),
    case_id: str | None = typer.Option(None, "--case", help="특정 caseId만 실행"),
    output: Path | None = typer.Option(None, "--output", help="결과 출력 디렉터리"),
) -> None:
    from shacs_bot.evals import (
        EvaluationRunner,
        EvaluationStorage,
        get_default_cases_path,
        load_cases_file,
        resolve_variant,
    )

    config: Config = load_config()
    cases_path: Path = cases or get_default_cases_path(config.workspace_path)
    agent_loop, session_manager = create_eval_runtime(config)

    try:
        try:
            loaded_cases = load_cases_file(cases_path)
        except ValueError as exc:
            console.print(f"[red]에러:[/red] {exc}")
            raise typer.Exit(1) from exc

        selected_cases = loaded_cases
        if case_id is not None:
            selected_cases = [case for case in loaded_cases if case.case_id == case_id]
            if not selected_cases:
                console.print(f"[red]에러:[/red] caseId '{case_id}'를 찾을 수 없습니다.")
                raise typer.Exit(1)

        if not selected_cases:
            console.print("[red]에러:[/red] 실행할 평가 케이스가 없습니다.")
            raise typer.Exit(1)

        variant_names: list[str] = variant or ["default"]
        try:
            variants = [resolve_variant(name) for name in variant_names]
        except ValueError as exc:
            console.print(f"[red]에러:[/red] {exc}")
            raise typer.Exit(1) from exc

        base_run_id: str = datetime.now().strftime("%Y-%m-%d-%H-%M-%S")
        run_id: str = base_run_id

        runner = EvaluationRunner(
            agent_loop=agent_loop,
            storage=EvaluationStorage(config.workspace_path),
            session_manager=session_manager,
        )

        async def _run_eval() -> None:
            summary = await runner.run_cases(
                cases=selected_cases,
                variants=variants,
                output_dir=output,
                cases_file=cases_path,
                run_id=run_id,
            )
            run_dir: Path | None = runner.last_run_dir
            summary_path: Path = (run_dir / "summary.json") if run_dir else Path("summary.json")

            console.print(
                f"[green]✓[/green] 평가 완료: {len(selected_cases)}개 case, {len(variants)}개 variant"
            )
            console.print(f"[cyan]cases[/cyan]: {cases_path}")
            console.print(f"[cyan]run[/cyan]: {run_dir if run_dir else '-'}")
            console.print(f"[cyan]summary[/cyan]: {summary_path}")

            table: Table = Table(title="Evaluation Summary")
            table.add_column("Variant", style="cyan")
            table.add_column("Total", justify="right")
            table.add_column("Success", justify="right", style="green")
            table.add_column("Task Fail", justify="right", style="yellow")
            table.add_column("Infra Err", justify="right", style="red")
            table.add_column("Avg Tools", justify="right")
            table.add_column("Prompt", justify="right")
            table.add_column("Completion", justify="right")

            for item in summary.variants:
                table.add_row(
                    item.variant,
                    str(item.total),
                    str(item.success),
                    str(item.task_failure),
                    str(item.infra_error),
                    f"{item.avg_tool_calls:.2f}",
                    str(item.prompt_tokens),
                    str(item.completion_tokens),
                )

            console.print(table)

        asyncio.run(_run_eval())
    finally:
        asyncio.run(agent_loop.close_mcp())


@eval_app.command("extract")
def eval_extract(
    session_filter: str | None = typer.Option(None, "--session", help="세션 키 부분 문자열 필터"),
    session_limit: int = typer.Option(10, "--session-limit", help="읽을 세션 수 상한"),
    case_limit: int = typer.Option(20, "--case-limit", help="추출할 case 수 상한"),
    output: Path | None = typer.Option(None, "--output", help="출력 JSON 파일 경로"),
    include_eval_sessions: bool = typer.Option(
        False, "--include-eval-sessions", help="eval: 세션도 포함"
    ),
) -> None:
    from shacs_bot.agent.session.manager import SessionManager
    from shacs_bot.evals import SessionCaseExtractor, build_auto_cases_path

    config: Config = load_config()
    sync_workspace_template(workspace=config.workspace_path)

    session_manager: SessionManager = SessionManager(config.workspace_path)
    extractor = SessionCaseExtractor(config.workspace_path, session_manager)
    extracted = extractor.extract_cases(
        session_filter=session_filter,
        session_limit=session_limit,
        case_limit=case_limit,
        include_eval_sessions=include_eval_sessions,
    )

    if not extracted:
        console.print("[yellow]추출할 세션 케이스가 없습니다.[/yellow]")
        raise typer.Exit(0)

    output_path: Path = output or build_auto_cases_path(config.workspace_path)
    extractor.write_cases_file(output_path, extracted)

    console.print(f"[green]✓[/green] {len(extracted)}개 case 추출 완료")
    console.print(f"[cyan]output[/cyan]: {output_path}")


@eval_app.command("auto-run")
def eval_auto_run(
    session_filter: str | None = typer.Option(None, "--session", help="세션 키 부분 문자열 필터"),
    session_limit: int = typer.Option(10, "--session-limit", help="읽을 세션 수 상한"),
    case_limit: int = typer.Option(20, "--case-limit", help="추출할 case 수 상한"),
    variant: list[str] | None = typer.Option(None, "--variant", help="평가 variant preset 이름"),
    candidate: list[str] | None = typer.Option(
        None, "--candidate", help="provider:model 형식의 비교 후보"
    ),
    baseline: bool = typer.Option(False, "--baseline", help="현재 run을 새 baseline으로 저장"),
    compare: bool = typer.Option(True, "--compare/--no-compare", help="baseline과 비교"),
    output: Path | None = typer.Option(None, "--output", help="결과 출력 디렉터리"),
    cases_output: Path | None = typer.Option(
        None, "--cases-output", help="auto-run용 case bundle 경로"
    ),
    include_eval_sessions: bool = typer.Option(
        False, "--include-eval-sessions", help="eval: 세션도 포함"
    ),
) -> None:
    from shacs_bot.evals import AutoEvalService

    config: Config = load_config()
    agent_loop, session_manager = create_eval_runtime(config)
    service = AutoEvalService(config.workspace_path, agent_loop, session_manager)

    try:
        try:
            if candidate is not None:
                for raw in candidate:
                    if ":" not in raw:
                        raise ValueError(f"invalid candidate format: {raw}")
                from shacs_bot.evals import update_auto_eval_state

                _ = update_auto_eval_state(config.workspace_path, autonomous_candidates=candidate)

            result = asyncio.run(
                service.run_auto_eval(
                    session_filter=session_filter,
                    session_limit=session_limit,
                    case_limit=case_limit,
                    variant_names=variant,
                    baseline=baseline,
                    compare=compare,
                    output=output,
                    cases_output=cases_output,
                    include_eval_sessions=include_eval_sessions,
                )
            )
        except ValueError as exc:
            console.print(f"[red]에러:[/red] {exc}")
            raise typer.Exit(1) from exc

        console.print(
            f"[green]✓[/green] auto eval 완료: 기본 {result.state.default_case_count}개 + 추출 {result.state.extracted_case_count}개"
        )
        console.print(f"[cyan]cases[/cyan]: {result.cases_path}")
        console.print(f"[cyan]run[/cyan]: {result.run_dir if result.run_dir else '-'}")
        console.print(f"[cyan]summary[/cyan]: {result.summary_path}")
        console.print(f"[cyan]state[/cyan]: {result.state_path}")
        console.print(f"[cyan]baseline[/cyan]: {result.state.baseline_run_id or '-'}")
        if result.state.candidate_best:
            console.print(f"[cyan]best candidate[/cyan]: {result.state.candidate_best}")
        console.print(
            f"[cyan]next trigger variants[/cyan]: {', '.join(result.state.trigger_variants)}"
        )
        if result.state.regressions:
            console.print(f"[red]regressions[/red]: {', '.join(result.state.regressions)}")

        table: Table = Table(title="Auto Evaluation Summary")
        table.add_column("Variant", style="cyan")
        table.add_column("Health", justify="right")
        table.add_column("Δ Success", justify="right")
        table.add_column("Score", justify="right")
        table.add_column("Avg Tools", justify="right")
        table.add_column("Avg Tokens", justify="right")
        table.add_column("Disabled", justify="right")
        table.add_column("Recommended", justify="right")

        for variant_name in result.state.variants:
            health = result.state.variant_health[variant_name]
            table.add_row(
                variant_name,
                health.status,
                f"{health.success_delta:+.1%}",
                f"{health.weighted_score:.2f}",
                f"{health.avg_tool_calls:.2f}",
                f"{health.avg_total_tokens:.0f}",
                "yes" if health.disabled else "no",
                "yes" if health.recommended else "no",
            )

        console.print(table)
    finally:
        asyncio.run(agent_loop.close_mcp())


@eval_app.command("status")
def eval_status() -> None:
    from shacs_bot.evals import read_auto_eval_state

    config: Config = load_config()
    state = read_auto_eval_state(config.workspace_path) or None
    if state is None:
        console.print("[yellow]self-eval 상태 파일이 없습니다.[/yellow]")
        raise typer.Exit(0)

    summary = Table(title="Self-Eval Status")
    summary.add_column("Field", style="cyan")
    summary.add_column("Value")
    summary.add_row("Last Run", state.last_auto_run_id or "-")
    summary.add_row("Baseline", state.baseline_run_id or "-")
    summary.add_row("Recommended Runtime", state.recommended_runtime_variant)
    summary.add_row("Recommended Provider", state.recommended_provider_name or "-")
    summary.add_row("Recommended Model", state.recommended_model or "-")
    summary.add_row("Best Candidate", state.candidate_best or "-")
    summary.add_row("Trigger Enabled", "yes" if state.trigger_enabled else "no")
    summary.add_row("Turn Threshold", str(state.trigger_turn_threshold))
    summary.add_row("Min Interval (min)", str(state.trigger_min_interval_minutes))
    summary.add_row("Schedule", state.trigger_schedule_kind)
    summary.add_row("Schedule Every (min)", str(state.trigger_schedule_every_minutes))
    summary.add_row("Schedule Cron", state.trigger_schedule_cron_expr or "-")
    summary.add_row("Autonomous Candidates", ", ".join(state.autonomous_candidates) or "-")
    summary.add_row("Next Trigger Variants", ", ".join(state.trigger_variants))
    summary.add_row("Last Trigger Status", state.last_trigger_status)
    summary.add_row("Last Trigger Session", state.last_triggered_session_key or "-")
    console.print(summary)

    health_table = Table(title="Variant Health")
    health_table.add_column("Variant", style="cyan")
    health_table.add_column("Health")
    health_table.add_column("Δ Success")
    health_table.add_column("Score")
    health_table.add_column("Avg Tools")
    health_table.add_column("Avg Tokens")
    health_table.add_column("Disabled")
    health_table.add_column("Recommended")
    for variant_name, health in state.variant_health.items():
        health_table.add_row(
            variant_name,
            health.status,
            f"{health.success_delta:+.1%}",
            f"{health.weighted_score:.2f}",
            f"{health.avg_tool_calls:.2f}",
            f"{health.avg_total_tokens:.0f}",
            "yes" if health.disabled else "no",
            "yes" if health.recommended else "no",
        )
    if state.variant_health:
        console.print(health_table)

    if state.candidate_scores:
        candidate_table = Table(title="Candidate Scores")
        candidate_table.add_column("Candidate", style="cyan")
        candidate_table.add_column("Score", justify="right")
        for candidate_key, score in state.candidate_scores.items():
            candidate_table.add_row(candidate_key, f"{score:.2f}")
        console.print(candidate_table)


@eval_app.command("policy")
def eval_policy(
    trigger_enabled: bool | None = typer.Option(None, "--trigger-enabled/--no-trigger-enabled"),
    trigger_turn_threshold: int | None = typer.Option(None, "--turn-threshold"),
    trigger_min_interval_minutes: int | None = typer.Option(None, "--min-interval-minutes"),
    trigger_session_limit: int | None = typer.Option(None, "--session-limit"),
    trigger_case_limit: int | None = typer.Option(None, "--case-limit"),
    trigger_variants: list[str] | None = typer.Option(None, "--trigger-variant"),
    autonomous_candidates: list[str] | None = typer.Option(None, "--candidate"),
    schedule_kind: str | None = typer.Option(None, "--schedule-kind"),
    schedule_every_minutes: int | None = typer.Option(None, "--schedule-every-minutes"),
    schedule_cron_expr: str | None = typer.Option(None, "--schedule-cron"),
    schedule_tz: str | None = typer.Option(None, "--schedule-tz"),
) -> None:
    from shacs_bot.evals import read_auto_eval_state, update_auto_eval_state

    config: Config = load_config()
    current = read_auto_eval_state(config.workspace_path) or None

    updates: dict[str, object] = {}
    if trigger_enabled is not None:
        updates["trigger_enabled"] = trigger_enabled
    if trigger_turn_threshold is not None:
        updates["trigger_turn_threshold"] = trigger_turn_threshold
    if trigger_min_interval_minutes is not None:
        updates["trigger_min_interval_minutes"] = trigger_min_interval_minutes
    if trigger_session_limit is not None:
        updates["trigger_session_limit"] = trigger_session_limit
    if trigger_case_limit is not None:
        updates["trigger_case_limit"] = trigger_case_limit
    if trigger_variants is not None:
        updates["trigger_variants"] = trigger_variants or ["default"]
    if autonomous_candidates is not None:
        for raw in autonomous_candidates:
            if ":" not in raw:
                console.print(f"[red]에러:[/red] invalid candidate format: {raw}")
                raise typer.Exit(1)
        updates["autonomous_candidates"] = autonomous_candidates
    if schedule_kind is not None:
        if schedule_kind not in {"off", "every", "cron"}:
            console.print("[red]에러:[/red] --schedule-kind는 off/every/cron 중 하나여야 합니다.")
            raise typer.Exit(1)
        updates["trigger_schedule_kind"] = schedule_kind
    if schedule_every_minutes is not None:
        updates["trigger_schedule_every_minutes"] = schedule_every_minutes
    if schedule_cron_expr is not None:
        updates["trigger_schedule_cron_expr"] = schedule_cron_expr
    if schedule_tz is not None:
        updates["trigger_schedule_tz"] = schedule_tz

    if not updates:
        if current is None:
            console.print("[yellow]설정된 self-eval policy가 없습니다.[/yellow]")
            raise typer.Exit(0)
        console.print("[yellow]변경할 옵션이 없습니다. 현재 상태를 표시합니다.[/yellow]")
        eval_status()
        raise typer.Exit(0)

    state, path = update_auto_eval_state(config.workspace_path, **updates)
    console.print(f"[green]✓[/green] self-eval policy 업데이트 완료")
    console.print(f"[cyan]state[/cyan]: {path}")
    console.print(f"[cyan]trigger enabled[/cyan]: {'yes' if state.trigger_enabled else 'no'}")
    console.print(f"[cyan]turn threshold[/cyan]: {state.trigger_turn_threshold}")
    console.print(f"[cyan]min interval[/cyan]: {state.trigger_min_interval_minutes}")
    console.print(f"[cyan]schedule[/cyan]: {state.trigger_schedule_kind}")
    console.print(f"[cyan]trigger variants[/cyan]: {', '.join(state.trigger_variants)}")


workflows_app = typer.Typer(help="Workflow 상태 및 복구")
app.add_typer(workflows_app, name="workflows")


def _format_workflow_time(raw: str) -> str:
    if not raw:
        return "-"
    try:
        parsed = datetime.fromisoformat(raw)
    except ValueError:
        return raw
    return parsed.strftime("%Y-%m-%d %H:%M")


def _render_workflow_table(records: list["WorkflowRecord"], *, title: str) -> None:
    table: Table = Table(title=title)
    table.add_column("ID", style="cyan")
    table.add_column("Source")
    table.add_column("State")
    table.add_column("Retries", justify="right")
    table.add_column("Updated")
    table.add_column("Next Run")
    table.add_column("Goal", style="yellow")

    for record in records:
        goal: str = record.goal if len(record.goal) <= 60 else f"{record.goal[:57]}..."
        table.add_row(
            record.workflow_id,
            record.source_kind,
            record.state,
            str(record.retries),
            _format_workflow_time(record.updated_at),
            _format_workflow_time(record.next_run_at),
            goal,
        )

    console.print(table)


@workflows_app.command("status")
def workflows_status(
    all_records: bool = typer.Option(False, "--all", help="완료된 workflow도 포함합니다."),
    state: str | None = typer.Option(None, "--state", help="특정 상태만 표시합니다."),
    source: str | None = typer.Option(None, "--source", help="특정 source_kind만 표시합니다."),
    limit: int = typer.Option(20, "--limit", min=1, help="최대 표시 개수"),
) -> None:
    from shacs_bot.workflow import WorkflowRecord, WorkflowRuntime

    config: Config = load_config()
    runtime: WorkflowRuntime = WorkflowRuntime(config.workspace_path)
    records: list[WorkflowRecord] = (
        runtime.store.list_all() if all_records else runtime.store.list_incomplete()
    )

    if state:
        records = [record for record in records if record.state == state]
    if source:
        records = [record for record in records if record.source_kind == source]

    records.sort(key=lambda record: record.updated_at, reverse=True)
    records = records[:limit]

    if not records:
        console.print("[yellow]표시할 workflow가 없습니다.[/yellow]")
        return

    title: str = "Workflow Status (all)" if all_records else "Workflow Status (incomplete)"
    _render_workflow_table(records, title=title)


@workflows_app.command("recover")
def workflows_recover(workflow_id: str = typer.Argument(..., help="복구할 workflow ID")) -> None:
    from shacs_bot.workflow import ManualRecoverResult, WorkflowRuntime

    config: Config = load_config()
    runtime: WorkflowRuntime = WorkflowRuntime(config.workspace_path)
    if workflow_id == "all":
        console.print(
            "[red]에러:[/red] `all` recover는 지원하지 않습니다. `workflows recover <id>`를 사용하세요."
        )
        raise typer.Exit(1)
    result: ManualRecoverResult = runtime.manual_recover(
        workflow_id,
        channel="cli",
        chat_id="direct",
        sender_id="cli",
    )

    if result.status == "missing":
        console.print(f"[red]에러:[/red] workflow `{workflow_id}`를 찾을 수 없습니다.")
        raise typer.Exit(1)

    record = result.record
    if record is None:
        console.print(f"[red]에러:[/red] workflow `{workflow_id}`를 찾을 수 없습니다.")
        raise typer.Exit(1)

    if result.status == "terminal":
        console.print(
            f"[yellow]복구 불가[/yellow] workflow `{workflow_id}` 는 terminal 상태입니다: {record.state}"
        )
        raise typer.Exit(0)

    if result.status == "already_queued":
        console.print(
            f"[yellow]대기 중[/yellow] workflow `{workflow_id}` 는 이미 queued 상태입니다."
        )
        return

    if result.status == "cooldown":
        console.print(
            f"[yellow]잠시 후 재시도[/yellow] workflow `{workflow_id}` 는 최근 recover 되었습니다."
        )
        return

    previous_state: str = result.previous_state or "unknown"
    console.print(f"[green]✓[/green] workflow `{workflow_id}` 를 queued 상태로 복구했습니다.")
    console.print(f"[cyan]previous state[/cyan]: {previous_state}")
    console.print(f"[cyan]current state[/cyan]: {record.state}")
    console.print("[cyan]execution[/cyan]: 큐 시스템이 처리 가능한 시점에 다시 실행됩니다.")


# ============================================================================
# Channel Commands
# ============================================================================


channels_app = typer.Typer(help="채널 관리자")
app.add_typer(channels_app, name="channels")


@channels_app.command("status")
def channels_status():
    """채널 상태를 보여줍니다."""

    config: Config = load_config()

    table: Table = Table(title="채널 상태")
    table.add_column("Channel", style="cyan")
    table.add_column("Enabled", style="green")
    table.add_column("Configuration", style="yellow")

    # WhatsApp
    wa: WhatsAppConfig = config.channels.whatsapp
    table.add_row("WhatsApp", "✓" if wa.enabled else "✗", wa.bridge_url)

    dc: DiscordConfig = config.channels.discord
    table.add_row("Discord", "✓" if dc.enabled else "✗", dc.gateway_url)

    # Feishu
    fs: FeishuConfig = config.channels.feishu
    fs_config: str = f"app_id: {fs.app_id[:10]}..." if fs.app_id else "[dim]not configured[/dim]"
    table.add_row("Feishu", "✓" if fs.enabled else "✗", fs_config)

    # Mochat
    mc: MochatConfig = config.channels.mochat
    mc_base: str = mc.base_url or "[dim]not configured[/dim]"
    table.add_row("Mochat", "✓" if mc.enabled else "✗", mc_base)

    # Telegram
    tg: TelegramConfig = config.channels.telegram
    tg_config: str = f"token: {tg.token[:10]}..." if tg.token else "[dim]not configured[/dim]"
    table.add_row("Telegram", "✓" if tg.enabled else "✗", tg_config)

    # Slack
    slack: SlackConfig = config.channels.slack
    slack_config: str = (
        "socket" if slack.app_token and slack.bot_token else "[dim]not configured[/dim]"
    )
    table.add_row("Slack", "✓" if slack.enabled else "✗", slack_config)

    # DingTalk
    dt: DingTalkConfig = config.channels.dingtalk
    dt_config: str = (
        f"client_id: {dt.client_id[:10]}..." if dt.client_id else "[dim]not configured[/dim]"
    )
    table.add_row("DingTalk", "✓" if dt.enabled else "✗", dt_config)

    # QQ
    qq: QQConfig = config.channels.qq
    qq_config: str = f"app_id: {qq.app_id[:10]}..." if qq.app_id else "[dim]not configured[/dim]"
    table.add_row("QQ", "✓" if qq.enabled else "✗", qq_config)

    # Email
    em: EmailConfig = config.channels.email
    em_config: str = em.imap_host if em.imap_host else "[dim]not configured[/dim]"
    table.add_row("Email", "✓" if em.enabled else "✗", em_config)

    console.print(table)


def _get_bridge_dir() -> Path:
    """필요한 경우 설정을 수행하여 브리지 디렉토리를 가져옵니다."""
    import shutil
    import subprocess

    # 사용자의 bridge 위치
    user_bridge: Path = get_bridge_install_dir()

    # 이미 존재하는 지 확인
    if (user_bridge / "dist" / "index.js").exists():
        return user_bridge

    # npm 확인
    if not shutil.which("npm"):
        console.print("red]npm을 찾을 수 없습니다. Node.js 18 이상을 설치하세요.[/red]")
        raise typer.Exit(1)

    # bridge 소스 위치 탐색: 먼저 패키지 데이터에서 확인한 뒤, 소스 디렉토리를 확인
    pkg_bridge: Path = Path(__file__).parent.parent / "bridge"  # shacs-bot/bridge (installed)
    src_bridge: Path = Path(__file__).parent.parent.parent / "bridge"  # repo root/bridge (dev)

    source: Path | None = None
    if (pkg_bridge / "package.json").exists():
        source = pkg_bridge
    elif (src_bridge / "package.json").exists():
        source = src_bridge

    if not source:
        console.print("[red]Bridge 소스를 찾을 수 없습니다.[/red]")
        console.print("다시 설치해 보세요: pip install --force-reinstall shacs-bot")
        raise typer.Exit(1)

    console.print(f"{__logo__} 브리지를 설정하는 중...")

    # 사용자 디렉토리 복사
    user_bridge.parent.mkdir(parents=True, exist_ok=True)
    if user_bridge.exists():
        shutil.rmtree(user_bridge)

    shutil.copytree(source, user_bridge, ignore=shutil.ignore_patterns("node_modules", "dist"))

    # Install and build
    try:
        console.print("  의존성 설치 중...")
        subprocess.run(["npm", "install"], cwd=user_bridge, check=True, capture_output=True)

        console.print("  빌드 중...")
        subprocess.run(["npm", "run", "build"], cwd=user_bridge, check=True, capture_output=True)

        console.print("[green]✓[/green] Bridge 준비\n")
    except subprocess.CalledProcessError as e:
        console.print(f"[red]빌드 실패: {e}[/red]")

        if e.stderr:
            console.print(f"[dim]{e.stderr.decode()[:500]}[/dim]")

        raise typer.Exit(1)

    return user_bridge


@channels_app.command("login")
def channels_login():
    """Link device via QR code."""
    import subprocess

    config: Config = load_config()
    bridge_dir: Path = _get_bridge_dir()

    console.print(f"{__logo__} 브릿지 시작 중...")
    console.print("QR 코드를 스캔하여 연결하세요.\n")

    env: dict[str, str] = {**os.environ}
    if config.channels.whatsapp.bridge_token:
        env["BRIDGE_TOKEN"] = config.channels.whatsapp.bridge_token

    try:
        subprocess.run(["npm", "start"], cwd=bridge_dir, check=True, env=env)
    except subprocess.CalledProcessError as e:
        console.print(f"[red]Bridge failed: {e}[/red]")
    except FileNotFoundError:
        console.print("[red]npm을 찾을 수 없습니다. Node.js를 설치하세요.[/red]")


# ============================================================================
# Status Commands
# ============================================================================


@app.command()
def status():
    """Show shacs-bot status."""
    from shacs_bot.agent.approval import list_pending_approvals
    from shacs_bot.agent.session.manager import SessionManager
    from shacs_bot.agent.usage import UsageTracker
    from shacs_bot.config.paths import get_usage_dir
    from shacs_bot.workflow.store import WorkflowStore

    config_path: Path = get_config_path()

    config: Config = load_config()
    workspace: Path = config.workspace_path

    console.print(f"{__logo__} shacs-bot 상태\n")

    console.print(
        f"Config: {config_path} {'[green]✓[/green]' if config_path.exists() else '[red]✗[/red]'}"
    )
    console.print(
        f"Workspace: {workspace} {'[green]✓[/green]' if workspace.exists() else '[red]✗[/red]'}"
    )

    sessions = SessionManager(workspace).list_sessions()
    incomplete_workflows = WorkflowStore(workspace).list_incomplete()
    incomplete_workflows.sort(key=lambda record: record.updated_at, reverse=True)
    _tracker = UsageTracker(get_usage_dir())
    usage_summary = _tracker.get_daily_summary()
    recent_usage_session = _tracker.get_recent_session()
    pending_approvals = list_pending_approvals()
    latest_session = sessions[0] if sessions else None
    latest_workflow = incomplete_workflows[0] if incomplete_workflows else None

    summary: Table = Table(title="Personal Inspect Summary")
    summary.add_column("Area", style="cyan")
    summary.add_column("Value")
    summary.add_row("Sessions", str(len(sessions)))
    summary.add_row(
        "Recent session",
        (
            f"{latest_session['key']} · {latest_session.get('updated_at') or '-'}"
            if latest_session
            else "-"
        ),
    )
    summary.add_row("Incomplete workflows", str(len(incomplete_workflows)))
    summary.add_row(
        "Recent workflow",
        (
            f"{latest_workflow.state} · {latest_workflow.source_kind} · {latest_workflow.updated_at}"
            if latest_workflow
            else "-"
        ),
    )
    summary.add_row(
        "Today's usage",
        (
            f"{int(usage_summary['calls'])} calls · {int(usage_summary['total']):,} tokens · "
            f"${float(usage_summary['cost']):.4f}"
        ),
    )
    summary.add_row("Recent usage session", recent_usage_session or "-")
    summary.add_row("Pending approvals", f"{len(pending_approvals)} (process-local)")
    console.print(summary)
    console.print(
        "[dim]상세 조회: inspect sessions | inspect workflows | inspect usage | inspect approvals[/dim]"
    )

    if config_path.exists():
        console.print(f"Model: {config.agents.defaults.model}")

        # Check API keys from registry
        for spec in PROVIDERS:
            p: ProviderConfig = getattr(config.providers, spec.name, None)
            if p is None:
                continue

            if spec.is_oauth:
                console.print(f"{spec.label}: [green]✓ (OAuth)[/green]")
            elif spec.is_local:
                # Local deployments show base_url instead of api_key
                if p.base_url:
                    console.print(f"{spec.label}: [green]✓ {p.base_url}[/green]")
                else:
                    console.print(f"{spec.label}: [dim]설정 되지 않음[/dim]")
            else:
                has_key = bool(p.api_key)
                console.print(
                    f"{spec.label}: {'[green]✓[/green]' if has_key else '[dim]설정 되지 않음[/dim]'}"
                )


# ============================================================================
# OAuth Login
# ============================================================================

provider_app = typer.Typer(help="providers 관리")
app.add_typer(provider_app, name="provider")


_LOGIN_HANDLERS: dict[str, callable] = {}


def _register_login(name: str):
    def decorator(fn):
        _LOGIN_HANDLERS[name] = fn
        return fn

    return decorator


@provider_app.command("login")
def provider_login(
    provider: str = typer.Argument(..., help="OAuth 제공자 (예: ‘openai-codex’, ‘github-copilot’)"),
):
    """OAuth provider로 인증"""

    key: str = provider.replace("-", "_")
    spec: ProviderSpec = next((s for s in PROVIDERS if s.name == key and s.is_oauth), None)
    if not spec:
        names: str = ", ".join(s.name.replace("_", "-") for s in PROVIDERS if s.is_oauth)
        console.print(f"[red]Unknown OAuth provider: {provider}[/red]  Supported: {names}")
        raise typer.Exit(1)

    handler: callable = _LOGIN_HANDLERS.get(spec.name)
    if not handler:
        console.print(f"[red]Login not implemented for {spec.label}[/red]")
        raise typer.Exit(1)

    console.print(f"{__logo__} OAuth Login - {spec.label}\n")
    handler()


@_register_login("openai_codex")
def _login_openai_codex() -> None:
    try:
        from oauth_cli_kit import get_token, login_oauth_interactive, OAuthToken
        from shacs_bot.providers.openai_codex import codex_token_storage

        storage = codex_token_storage()
        token: OAuthToken | None = None
        try:
            token = get_token(storage=storage)
        except Exception:
            pass
        if not (token and token.access):
            console.print("[cyan]Starting interactive OAuth login...[/cyan]\n")
            token = login_oauth_interactive(
                print_fn=lambda s: console.print(s),
                prompt_fn=lambda s: typer.prompt(s),
                originator="shacs-bot",
                storage=storage,
            )
        if not (token and token.access):
            console.print("[red]✗ Authentication failed[/red]")
            raise typer.Exit(1)
        console.print(
            f"[green]✓ Authenticated with OpenAI Codex[/green]  [dim]{token.account_id}[/dim]"
        )
    except ImportError:
        console.print("[red]oauth_cli_kit not installed. Run: pip install oauth-cli-kit[/red]")
        raise typer.Exit(1)


@_register_login("github_copilot")
def _login_github_copilot() -> None:
    import asyncio

    console.print("[cyan]Starting GitHub Copilot device flow...[/cyan]\n")

    async def _trigger():
        from openai import AsyncOpenAI

        client = AsyncOpenAI(
            api_key="copilot-token",
            base_url="https://api.githubcopilot.com",
        )
        await client.chat.completions.create(
            model="gpt-4o",
            messages=[{"role": "user", "content": "hi"}],
            max_tokens=1,
        )

    try:
        asyncio.run(_trigger())
        console.print("[green]✓ Authenticated with GitHub Copilot[/green]")
    except Exception as e:
        console.print(f"[red]Authentication error: {e}[/red]")
        raise typer.Exit(1)


# ============================================================================
# Inspect Commands (Personal Inspect CLI)
# ============================================================================

inspect_app = typer.Typer(help="세션, 워크플로우, 사용량, 승인 상태를 읽기 전용으로 조회합니다.")
app.add_typer(inspect_app, name="inspect")


@inspect_app.command("sessions")
def inspect_sessions(
    limit: int = typer.Option(20, "--limit", min=1, help="최대 표시 개수"),
    prefix: str | None = typer.Option(None, "--key-prefix", "--prefix", help="세션 키 prefix 필터"),
    show_meta: bool = typer.Option(False, "--show-meta", "--meta", help="메타데이터 열도 표시"),
) -> None:
    """저장된 세션 목록을 표시합니다."""
    from shacs_bot.agent.session.manager import SessionManager

    config: Config = load_config()
    manager: SessionManager = SessionManager(config.workspace_path)
    sessions = manager.list_sessions()

    if prefix:
        sessions = [s for s in sessions if s["key"].startswith(prefix)]

    total_count = len(sessions)
    sessions = sessions[:limit]

    if not sessions:
        empty_msg = (
            f"[yellow]'{prefix}' 에 일치하는 세션이 없습니다.[/yellow]"
            if prefix
            else "[yellow]저장된 세션이 없습니다.[/yellow]"
        )
        console.print(empty_msg)
        return

    table: Table = Table(title=f"Sessions ({total_count}개)")
    table.add_column("Key", style="cyan")
    table.add_column("Messages", justify="right")
    table.add_column("Created")
    table.add_column("Updated")
    if show_meta:
        table.add_column("Metadata", style="dim")

    for s in sessions:
        created = s.get("created_at") or "-"
        updated = s.get("updated_at") or "-"
        try:
            created = datetime.fromisoformat(created).strftime("%Y-%m-%d %H:%M")
        except Exception:
            pass
        try:
            updated = datetime.fromisoformat(updated).strftime("%Y-%m-%d %H:%M")
        except Exception:
            pass

        message_count: int = int(s.get("message_count", 0))
        row = [s["key"], str(message_count), created, updated]
        if show_meta:
            metadata = s.get("metadata") or {}
            meta_str = str(metadata)
            if len(meta_str) > 80:
                meta_str = f"{meta_str[:77]}..."
            row.append(meta_str)
        table.add_row(*row)

    console.print(table)
    if total_count > limit:
        console.print(f"[dim]최신순 · 전체 {total_count}개 중 {limit}개 표시[/dim]")
    else:
        console.print(f"[dim]최신순 · {total_count}개 세션[/dim]")


@inspect_app.command("workflows")
def inspect_workflows(
    all_records: bool = typer.Option(False, "--all", help="완료된 workflow도 포함합니다."),
    state: str | None = typer.Option(None, "--state", help="특정 상태만 표시합니다."),
    source: str | None = typer.Option(None, "--source", help="특정 source_kind만 표시합니다."),
    limit: int = typer.Option(20, "--limit", min=1, help="최대 표시 개수"),
) -> None:
    """저장된 워크플로우 목록을 읽기 전용으로 표시합니다."""
    from shacs_bot.workflow.store import WorkflowStore

    config: Config = load_config()
    store: WorkflowStore = WorkflowStore(config.workspace_path)
    records: list[WorkflowRecord] = store.list_all() if all_records else store.list_incomplete()

    if state:
        records = [r for r in records if r.state == state]
    if source:
        records = [r for r in records if r.source_kind == source]

    records.sort(key=lambda r: r.updated_at, reverse=True)
    records = records[:limit]

    if not records:
        console.print("[yellow]표시할 workflow가 없습니다.[/yellow]")
        return

    title: str = "Workflows (all)" if all_records else "Workflows (incomplete)"
    _render_workflow_table(records, title=title)


@inspect_app.command("usage")
def inspect_usage(
    date: str | None = typer.Option(None, "--date", help="조회할 날짜 (YYYY-MM-DD). 기본값: 오늘"),
    session: str | None = typer.Option(None, "--session", help="특정 세션 키만 집계"),
) -> None:
    """토큰 사용량과 비용 요약을 표시합니다."""
    from shacs_bot.agent.usage import UsageTracker
    from shacs_bot.config.paths import get_usage_dir

    tracker: UsageTracker = UsageTracker(get_usage_dir())

    if session:
        summary = tracker.get_session_summary(session, target_date=date)
        title = (
            f"Usage — session: {session} ({date})"
            if date
            else f"Usage — session: {session} (all days)"
        )
    else:
        summary = tracker.get_daily_summary(date)
        target = date or "오늘"
        title = f"Usage — {target}"

    table: Table = Table(title=title)
    table.add_column("항목", style="cyan")
    table.add_column("값", justify="right")

    prompt: int = int(summary.get("prompt", 0))
    completion: int = int(summary.get("completion", 0))
    total: int = int(summary.get("total", 0))
    cost: float = float(summary.get("cost", 0.0))
    calls: int = int(summary.get("calls", 0))
    sessions: int = int(summary.get("sessions", 0))

    if total == 0:
        console.print("[yellow]기록된 사용량이 없습니다.[/yellow]")
        return

    table.add_row("Prompt tokens", f"{prompt:,}")
    table.add_row("Completion tokens", f"{completion:,}")
    table.add_row("Total tokens", f"{total:,}")
    table.add_row("Cost (USD)", f"${cost:.4f}")
    table.add_row("LLM calls", str(calls))
    if not session:
        table.add_row("Sessions", str(sessions))

    console.print(table)


@inspect_app.command("approvals")
def inspect_approvals() -> None:
    """현재 프로세스에서 대기 중인 승인 요청을 표시합니다 (프로세스 로컬).

    참고: 승인 상태는 프로세스 메모리에만 존재합니다.
    이 명령을 실행하는 프로세스와 게이트웨이 프로세스가 다르면 항상 빈 목록이 표시됩니다.
    """
    from shacs_bot.agent.approval import list_pending_approvals

    approvals = list_pending_approvals()

    if not approvals:
        console.print("[yellow]대기 중인 승인 요청이 없습니다. (프로세스 로컬)[/yellow]")
        return

    table: Table = Table(title="Pending Approvals (process-local)")
    table.add_column("Request ID", style="cyan")
    table.add_column("Session")
    table.add_column("Channel")
    table.add_column("Tool")
    table.add_column("Skill")
    table.add_column("Done")

    for item in approvals:
        table.add_row(
            str(item.get("request_id", "")),
            str(item.get("session_key", "") or "-"),
            str(item.get("channel", "") or "-"),
            str(item.get("tool_name", "") or "-"),
            str(item.get("skill_name", "") or "-"),
            "yes" if bool(item.get("done", False)) else "no",
        )

    console.print(table)
    console.print("[dim]참고: 이 목록은 현재 프로세스 메모리의 상태만 반영합니다.[/dim]")


if __name__ == "__main__":
    app()
