"""shacs-bot CLI 커멘드"""

import asyncio
import os
import select
import signal
import sys
from pathlib import Path
from typing import Text

import typer
from loguru import logger
from prompt_toolkit import PromptSession, HTML
from prompt_toolkit.history import FileHistory
from prompt_toolkit.patch_stdout import patch_stdout
from rich.console import Console
from rich.markdown import Markdown
from rich.table import Table

from shacs_bot import __logo__, __version__
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


def _resolve_media_key(config: Config) -> str | None:
    return config.providers.image_gen.api_key or None


def _resolve_media_base_url(config: Config) -> str | None:
    return config.providers.image_gen.base_url or None


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


def _make_provider(config: Config) -> LLMProvider:
    from shacs_bot.providers.registry import find_by_name

    model: str = config.agents.defaults.model
    provider_name: str = config.get_provider_name(model)
    provider: ProviderConfig = config.get_provider(model)
    spec: ProviderSpec = find_by_name(provider_name)

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

    bus: MessageBus = MessageBus()
    provider = _make_provider(config)
    session_manager: SessionManager = SessionManager(config.workspace_path)

    # 먼저, 크론 서비스 생성 (에이전트 생성 이후에 callback 설정)
    cron_store_path: Path = get_cron_dir() / "jobs.json"
    cron: CronService = CronService(cron_store_path)

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
        media_config=config.tools.media,
        media_api_key=_resolve_media_key(config),
        media_base_url=_resolve_media_base_url(config),
        skill_approval=config.tools.skill_approval,
    )

    # 크론 callback 설정 (에이전트 필요)
    async def on_cron_job(job: CronJob) -> str | None:
        """에이전트를 통해 cron job 실행"""
        from _contextvars import Token
        from shacs_bot.agent.tools.base import Tool
        from shacs_bot.agent.tools.cron.cron import CronTool
        from shacs_bot.agent.tools.message import MessageTool
        from shacs_bot.bus.events import OutboundMessage

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

        return response

    cron.set_job(on_cron_job)

    hb_cfg: HeartbeatConfig = config.gateway.heartbeat
    # 채널 관리자 생성
    channels: ChannelManager = ChannelManager(config, bus)
    if channels.enabled_channels:
        console.print(f"[green]✓[/green] 활성화된 채널: {', '.join(channels.enabled_channels)}")
    else:
        console.print("[yellow]경고: 활성화된 채널이 없습니다[/yellow]")

    cron_status = cron.status()
    if cron_status["jobs"] > 0:
        console.print(f"[green]✓[/green] Cron: 예약된 작업 {cron_status['jobs']}개")

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

    async def on_heartbeat_execute(tasks: str) -> str:
        """Phase 2: full 에이전트 로프를 통해 heartbeat 태스크 실행"""
        channel, chat_id = await _pick_heartbeat_target()

        async def _silent(*_args, **_kwargs):
            pass

        return await agent_loop.process_direct(
            content=tasks,
            session_key="heartbeat",
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
    )

    async def run() -> None:
        try:
            await cron.start()
            await heartbeat.start()
            await asyncio.gather(agent_loop.run(), channels.start_all())
        except KeyboardInterrupt:
            console.print("\n종료 중...")
        finally:
            await agent_loop.close_mcp()
            heartbeat.stop()
            cron.stop()
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

    bus: MessageBus = MessageBus()
    provider = _make_provider(config)

    # 도구 사용을 위한 cron 서비스를 생성합니다 (CLI에서는 실행 중이 아닌 한 콜백이 필요하지 않습니다).
    cron_store_path: Path = get_cron_dir() / "jobs.json"
    cron: CronService = CronService(cron_store_path)

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
        media_config=config.tools.media,
        media_api_key=_resolve_media_key(config),
        media_base_url=_resolve_media_base_url(config),
        skill_approval=config.tools.skill_approval,
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


if __name__ == "__main__":
    app()
