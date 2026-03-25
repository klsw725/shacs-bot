from __future__ import annotations

from typing import Any

from rich.console import Console
from rich.panel import Panel
from rich.table import Table

from shacs_bot.config.schema import (
    Config,
    ProviderConfig,
    AgentDefaults,
    ChannelsConfig,
    GatewayConfig,
    ToolsConfig,
)
from shacs_bot.providers.registry import PROVIDERS, ProviderSpec

console = Console()

_CHANNELS = (
    "telegram",
    "discord",
    "slack",
    "whatsapp",
    "feishu",
    "dingtalk",
    "qq",
    "email",
    "matrix",
    "mochat",
)


def _mask_value(value: str, visible: int = 4) -> str:
    if not value or len(value) <= visible:
        return value
    return "*" * (len(value) - visible) + value[-visible:]


def _questionary():
    try:
        import questionary

        return questionary
    except ImportError:
        console.print("[red]questionary 패키지가 필요합니다.[/red]")
        console.print("  [cyan]uv sync --extra wizard[/cyan] 로 설치하세요.")
        raise SystemExit(1)


def run_onboard(initial_config: Config | None = None) -> Config:
    q = _questionary()
    config = initial_config or Config()

    while True:
        action = q.select(
            "설정할 항목을 선택하세요:",
            choices=[
                q.Choice("[P] LLM Provider", value="provider"),
                q.Choice("[C] Chat Channel", value="channel"),
                q.Choice("[A] Agent Settings", value="agent"),
                q.Choice("[G] Gateway", value="gateway"),
                q.Choice("[V] View Summary", value="summary"),
                q.Choice("[S] Save and Exit", value="save"),
                q.Choice("[X] Exit Without Saving", value="exit"),
            ],
        ).ask()

        if action is None or action == "exit":
            return config
        if action == "save":
            return config
        if action == "provider":
            config = _configure_provider(q, config)
        elif action == "channel":
            config = _configure_channel(q, config)
        elif action == "agent":
            config = _configure_agent(q, config)
        elif action == "gateway":
            config = _configure_gateway(q, config)
        elif action == "summary":
            _show_summary(config)


def _configure_provider(q: Any, config: Config) -> Config:
    configurable = [s for s in PROVIDERS if not s.is_direct and s.env_key]
    names = [s.name for s in configurable]
    display = [f"{s.label} ({s.name})" for s in configurable]
    display.append("← Back")

    choice = q.select("Provider를 선택하세요:", choices=display).ask()
    if choice is None or choice == "← Back":
        return config

    idx = display.index(choice)
    spec = configurable[idx]
    pc: ProviderConfig = getattr(config.providers, spec.name, ProviderConfig())

    current_key = pc.api_key
    prompt_text = f"API Key"
    if current_key:
        prompt_text += f" (현재: {_mask_value(current_key)})"

    new_key = q.password(prompt_text + ":").ask()
    if new_key:
        pc.api_key = new_key

    if spec.default_base_url:
        current_base = pc.base_url or spec.default_base_url
        new_base = q.text(
            f"Base URL (기본: {spec.default_base_url}):",
            default=current_base,
        ).ask()
        if new_base:
            pc.base_url = new_base if new_base != spec.default_base_url else None

    setattr(config.providers, spec.name, pc)
    console.print(f"[green]✓[/green] {spec.label} 설정 완료")
    return config


def _configure_channel(q: Any, config: Config) -> Config:
    choices = [f"{ch.title()} ({ch})" for ch in _CHANNELS]
    choices.append("← Back")

    choice = q.select("Channel을 선택하세요:", choices=choices).ask()
    if choice is None or choice == "← Back":
        return config

    idx = choices.index(choice)
    ch_name = _CHANNELS[idx]
    ch_config = getattr(config.channels, ch_name, None)
    if ch_config is None:
        console.print(f"[yellow]{ch_name} 채널 설정을 찾을 수 없습니다.[/yellow]")
        return config

    enabled = q.confirm(f"{ch_name} 활성화?", default=ch_config.enabled).ask()
    ch_config.enabled = enabled

    if not enabled:
        setattr(config.channels, ch_name, ch_config)
        return config

    fields = ch_config.model_fields
    skip_fields = {"enabled", "model_config"}

    for field_name, field_info in fields.items():
        if field_name in skip_fields:
            continue
        current = getattr(ch_config, field_name)
        if isinstance(current, bool):
            continue
        if isinstance(current, str) and field_name in (
            "token",
            "secret",
            "app_secret",
            "api_key",
            "imap_password",
            "smtp_password",
            "claw_token",
            "access_token",
        ):
            display = _mask_value(current) if current else "(미설정)"
            new_val = q.password(f"{field_name} (현재: {display}):").ask()
            if new_val:
                setattr(ch_config, field_name, new_val)
        elif isinstance(current, str) and current == "":
            new_val = q.text(f"{field_name}:").ask()
            if new_val:
                setattr(ch_config, field_name, new_val)

    setattr(config.channels, ch_name, ch_config)
    console.print(f"[green]✓[/green] {ch_name} 설정 완료")
    return config


def _configure_agent(q: Any, config: Config) -> Config:
    from shacs_bot.cli.models import (
        get_model_suggestions,
        get_model_context_limit,
        format_token_count,
    )

    defaults: AgentDefaults = config.agents.defaults

    suggestions = get_model_suggestions()
    model = q.autocomplete(
        "모델:",
        choices=suggestions,
        default=defaults.model,
    ).ask()
    if model:
        defaults.model = model

    provider_choices = ["auto"] + [s.name for s in PROVIDERS if not s.is_direct]
    provider = q.select(
        "Provider (auto = 자동 감지):",
        choices=provider_choices,
        default=defaults.provider if defaults.provider in provider_choices else "auto",
    ).ask()
    if provider:
        defaults.provider = provider

    ctx_limit = get_model_context_limit(defaults.model)
    if ctx_limit:
        recommended_max = min(ctx_limit // 4, 16384)
        console.print(
            f"  [dim]모델 context: {format_token_count(ctx_limit)}, 권장 max_tokens: {format_token_count(recommended_max)}[/dim]"
        )

    max_tokens = q.text(
        f"max_tokens (현재: {defaults.max_tokens}):",
        default=str(defaults.max_tokens),
    ).ask()
    if max_tokens and max_tokens.isdigit():
        defaults.max_tokens = int(max_tokens)

    temp = q.text(
        f"temperature (현재: {defaults.temperature}):",
        default=str(defaults.temperature),
    ).ask()
    if temp:
        try:
            defaults.temperature = float(temp)
        except ValueError:
            pass

    config.agents.defaults = defaults
    console.print("[green]✓[/green] Agent 설정 완료")
    return config


def _configure_gateway(q: Any, config: Config) -> Config:
    gw: GatewayConfig = config.gateway

    port = q.text(f"포트 (현재: {gw.port}):", default=str(gw.port)).ask()
    if port and port.isdigit():
        gw.port = int(port)

    hb_enabled = q.confirm("Heartbeat 활성화?", default=gw.heartbeat.enabled).ask()
    gw.heartbeat.enabled = hb_enabled

    if hb_enabled:
        interval = q.text(
            f"Heartbeat 간격(초) (현재: {gw.heartbeat.interval_s}):",
            default=str(gw.heartbeat.interval_s),
        ).ask()
        if interval and interval.isdigit():
            gw.heartbeat.interval_s = int(interval)

    config.gateway = gw
    console.print("[green]✓[/green] Gateway 설정 완료")
    return config


def _show_summary(config: Config) -> None:
    table = Table(title="설정 요약", show_header=True, header_style="bold cyan")
    table.add_column("항목", style="bold")
    table.add_column("값")

    table.add_row("Model", config.agents.defaults.model)
    table.add_row("Provider", config.agents.defaults.provider)
    table.add_row("Max Tokens", str(config.agents.defaults.max_tokens))
    table.add_row("Temperature", str(config.agents.defaults.temperature))
    table.add_row("Gateway Port", str(config.gateway.port))

    configured_providers = []
    for spec in PROVIDERS:
        pc = getattr(config.providers, spec.name, None)
        if pc and pc.api_key:
            configured_providers.append(f"{spec.label} ({_mask_value(pc.api_key)})")
    table.add_row(
        "Providers", ", ".join(configured_providers) if configured_providers else "(없음)"
    )

    enabled_channels = []
    for ch_name in _CHANNELS:
        ch = getattr(config.channels, ch_name, None)
        if ch and getattr(ch, "enabled", False):
            enabled_channels.append(ch_name)
    table.add_row("Channels", ", ".join(enabled_channels) if enabled_channels else "(없음)")

    console.print()
    console.print(Panel(table))
    console.print()
