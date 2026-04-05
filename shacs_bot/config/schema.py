"""Configuration schema using Pydantic."""

from pathlib import Path
from typing import ClassVar, Literal

from loguru import logger
from pydantic import BaseModel, Field, ConfigDict
from pydantic.alias_generators import to_camel
from pydantic_settings import BaseSettings


class Base(BaseModel):
    """camelCase와 snake_case 키를 모두 받아들이는 기본(Base) 모델."""

    model_config: ClassVar[ConfigDict] = ConfigDict(alias_generator=to_camel, populate_by_name=True)


class WhatsAppConfig(Base):
    """WhatsApp channel configuration."""

    enabled: bool = False
    bridge_url: str = "ws://localhost:3001"
    bridge_token: str = ""  # Shared token for bridge auth (optional, recommended)
    allow_from: list[str] = Field(default_factory=list)  # Allowed phone numbers


class TelegramConfig(Base):
    """Telegram channel configuration."""

    enabled: bool = False
    token: str = ""  # Bot token from @BotFather
    allow_from: list[str] = Field(default_factory=list)  # Allowed user IDs or usernames
    proxy: str | None = (
        None  # HTTP/SOCKS5 proxy URL, e.g. "http://127.0.0.1:7890" or "socks5://127.0.0.1:1080"
    )
    reply_to_message: bool = False  # If true, bot replies quote the original message


class FeishuConfig(Base):
    """Feishu/Lark channel configuration using WebSocket long connection."""

    enabled: bool = False
    app_id: str = ""  # App ID from Feishu Open Platform
    app_secret: str = ""  # App Secret from Feishu Open Platform
    encrypt_key: str = ""  # Encrypt Key for event subscription (optional)
    verification_token: str = ""  # Verification Token for event subscription (optional)
    allow_from: list[str] = Field(default_factory=list)  # Allowed user open_ids
    react_emoji: str = (
        "THUMBSUP"  # Emoji type for message reactions (e.g. THUMBSUP, OK, DONE, SMILE)
    )


class DingTalkConfig(Base):
    """DingTalk channel configuration using Stream mode."""

    enabled: bool = False
    client_id: str = ""  # AppKey
    client_secret: str = ""  # AppSecret
    allow_from: list[str] = Field(default_factory=list)  # Allowed staff_ids


class DiscordConfig(Base):
    """Discord channel configuration."""

    enabled: bool = False
    token: str = ""  # Bot token from Discord Developer Portal
    allow_from: list[str] = Field(default_factory=list)  # Allowed user IDs
    gateway_url: str = "wss://gateway.discord.gg/?v=10&encoding=json"
    intents: int = 37377  # GUILDS + GUILD_MESSAGES + DIRECT_MESSAGES + MESSAGE_CONTENT
    group_policy: Literal["mention", "open"] = "mention"
    reply_in_thread: bool = False  # If true, bot replies in a new thread (guild channels only)
    thread_auto_archive_minutes: int = 1440  # 60 / 1440 / 4320 / 10080


class MatrixConfig(Base):
    """Matrix (Element) channel configuration."""

    enabled: bool = False
    homeserver: str = "https://matrix.org"
    access_token: str = ""
    user_id: str = ""  # @bot:matrix.org
    device_id: str = ""
    e2ee_enabled: bool = True  # Enable Matrix E2EE support (encryption + encrypted room handling).
    sync_stop_grace_seconds: int = (
        2  # Max seconds to wait for sync_forever to stop gracefully before cancellation fallback.
    )
    max_media_bytes: int = (
        20 * 1024 * 1024
    )  # Max attachment size accepted for Matrix media handling (inbound + outbound).
    allow_from: list[str] = Field(default_factory=list)
    group_policy: Literal["open", "mention", "allowlist"] = "open"
    group_allow_from: list[str] = Field(default_factory=list)
    allow_room_mentions: bool = False


class EmailConfig(Base):
    """Email channel configuration (IMAP inbound + SMTP outbound)."""

    enabled: bool = False
    consent_granted: bool = False  # Explicit owner permission to access mailbox data

    # IMAP (receive)
    imap_host: str = ""
    imap_port: int = 993
    imap_username: str = ""
    imap_password: str = ""
    imap_mailbox: str = "INBOX"
    imap_use_ssl: bool = True

    # SMTP (send)
    smtp_host: str = ""
    smtp_port: int = 587
    smtp_username: str = ""
    smtp_password: str = ""
    smtp_use_tls: bool = True
    smtp_use_ssl: bool = False
    from_address: str = ""

    # Behavior
    auto_reply_enabled: bool = (
        True  # If false, inbound email is read but no automatic reply is sent
    )
    poll_interval_seconds: int = 30
    mark_seen: bool = True
    max_body_chars: int = 12000
    subject_prefix: str = "Re: "
    allow_from: list[str] = Field(default_factory=list)  # Allowed sender email addresses


class MochatMentionConfig(Base):
    """Mochat mention behavior configuration."""

    require_in_groups: bool = False


class MochatGroupRule(Base):
    """Mochat per-group mention requirement."""

    require_mention: bool = False


class MochatConfig(Base):
    """Mochat channel configuration."""

    enabled: bool = False
    base_url: str = "https://mochat.io"
    socket_url: str = ""
    socket_path: str = "/socket.io"
    socket_disable_msgpack: bool = False
    socket_reconnect_delay_ms: int = 1000
    socket_max_reconnect_delay_ms: int = 10000
    socket_connect_timeout_ms: int = 10000
    refresh_interval_ms: int = 30000
    watch_timeout_ms: int = 25000
    watch_limit: int = 100
    retry_delay_ms: int = 500
    max_retry_attempts: int = 0  # 0 means unlimited retries
    claw_token: str = ""
    agent_user_id: str = ""
    sessions: list[str] = Field(default_factory=list)
    panels: list[str] = Field(default_factory=list)
    allow_from: list[str] = Field(default_factory=list)
    mention: MochatMentionConfig = Field(default_factory=MochatMentionConfig)
    groups: dict[str, MochatGroupRule] = Field(default_factory=dict)
    reply_delay_mode: str = "non-mention"  # off | non-mention
    reply_delay_ms: int = 120000


class SlackDMConfig(Base):
    """Slack DM policy configuration."""

    enabled: bool = True
    policy: str = "open"  # "open" or "allowlist"
    allow_from: list[str] = Field(default_factory=list)  # Allowed Slack user IDs


class SlackConfig(Base):
    """Slack channel configuration."""

    enabled: bool = False
    mode: str = "socket"  # "socket" supported
    webhook_path: str = "/slack/events"
    bot_token: str = ""  # xoxb-...
    app_token: str = ""  # xapp-...
    user_token_read_only: bool = True
    reply_in_thread: bool = True
    react_emoji: str = "eyes"
    group_policy: str = "mention"  # "mention", "open", "allowlist"
    group_allow_from: list[str] = Field(default_factory=list)  # Allowed channel IDs if allowlist
    dm: SlackDMConfig = Field(default_factory=SlackDMConfig)


class QQConfig(Base):
    """QQ channel configuration using botpy SDK."""

    enabled: bool = False
    app_id: str = ""  # 机器人 ID (AppID) from q.qq.com
    secret: str = ""  # 机器人密钥 (AppSecret) from q.qq.com
    allow_from: list[str] = Field(
        default_factory=list
    )  # Allowed user openids (empty = public access)


class ChannelsConfig(Base):
    """Configuration for chat channels."""

    send_progress: bool = False  # stream agent's text progress to the channel
    send_tool_hints: bool = False  # stream tool-call hints (e.g. read_file("…"))
    send_memory_hints: bool = True  # notify user when memory consolidation occurs
    whatsapp: WhatsAppConfig = Field(default_factory=WhatsAppConfig)
    telegram: TelegramConfig = Field(default_factory=TelegramConfig)
    discord: DiscordConfig = Field(default_factory=DiscordConfig)
    feishu: FeishuConfig = Field(default_factory=FeishuConfig)
    mochat: MochatConfig = Field(default_factory=MochatConfig)
    dingtalk: DingTalkConfig = Field(default_factory=DingTalkConfig)
    email: EmailConfig = Field(default_factory=EmailConfig)
    slack: SlackConfig = Field(default_factory=SlackConfig)
    qq: QQConfig = Field(default_factory=QQConfig)
    matrix: MatrixConfig = Field(default_factory=MatrixConfig)


class AgentDefaults(Base):
    """Default agent configuration."""

    workspace: str = "~/.shacs-bot/workspace"
    model: str = "anthropic/claude-opus-4-5"
    provider: str = "auto"  # Provider 이름 (e.g. "anthropic", "openai", "deepseek", "groq", "zhipu", "dashscope", "gemini", "moonshot", "aihubmix") or "auto" for auto-detection
    max_tokens: int = 8192
    temperature: float = 0.1
    max_tool_iterations: int = 40
    memory_window: int = 100
    reasoning_effort: str | None = None  # low / medium / high — enables LLM thinking mode


class AgentsConfig(Base):
    """Agent configuration."""

    defaults: AgentDefaults = Field(default_factory=AgentDefaults)


class ProviderConfig(Base):
    """LLM Provider configuration."""

    api_key: str = ""
    base_url: str | None = None
    extra_headers: dict[str, str] | None = None  # Custom headers (e.g. APP-Code for AiHubMix)


class ProvidersConfig(BaseModel):
    """Configuration for LLM providers."""

    custom: ProviderConfig = Field(default_factory=ProviderConfig)  # Any OpenAI-compatible endpoint
    anthropic: ProviderConfig = Field(default_factory=ProviderConfig)
    openai: ProviderConfig = Field(default_factory=ProviderConfig)
    openrouter: ProviderConfig = Field(default_factory=ProviderConfig)
    deepseek: ProviderConfig = Field(default_factory=ProviderConfig)
    groq: ProviderConfig = Field(default_factory=ProviderConfig)
    zhipu: ProviderConfig = Field(default_factory=ProviderConfig)
    dashscope: ProviderConfig = Field(default_factory=ProviderConfig)  # 阿里云通义千问
    vllm: ProviderConfig = Field(default_factory=ProviderConfig)
    gemini: ProviderConfig = Field(default_factory=ProviderConfig)
    moonshot: ProviderConfig = Field(default_factory=ProviderConfig)
    minimax: ProviderConfig = Field(default_factory=ProviderConfig)
    aihubmix: ProviderConfig = Field(default_factory=ProviderConfig)  # AiHubMix API gateway
    siliconflow: ProviderConfig = Field(
        default_factory=ProviderConfig
    )  # SiliconFlow (硅基流动) API gateway
    volcengine: ProviderConfig = Field(
        default_factory=ProviderConfig
    )  # VolcEngine (火山引擎) API gateway
    azure_openai: ProviderConfig = Field(default_factory=ProviderConfig)  # Azure OpenAI
    volcengine_coding_plan: ProviderConfig = Field(
        default_factory=ProviderConfig
    )  # VolcEngine Coding Plan
    byteplus: ProviderConfig = Field(default_factory=ProviderConfig)  # BytePlus
    byteplus_coding_plan: ProviderConfig = Field(
        default_factory=ProviderConfig
    )  # BytePlus Coding Plan
    ollama: ProviderConfig = Field(default_factory=ProviderConfig)  # Ollama (local)
    openai_codex: ProviderConfig = Field(default_factory=ProviderConfig)  # OpenAI Codex (OAuth)
    github_copilot: ProviderConfig = Field(default_factory=ProviderConfig)  # Github Copilot (OAuth)
    image_gen: ProviderConfig = Field(default_factory=ProviderConfig)


class HeartbeatConfig(Base):
    """Heartbeat service configuration."""

    enabled: bool = True
    interval_s: int = 30 * 60  # 30분


class FailoverRule(Base):
    from_provider: str = ""
    to_provider: str = ""
    model_map: dict[str, str] = Field(default_factory=dict)


class FailoverConfig(Base):
    enabled: bool = False
    cooldown_seconds: int = 300
    rules: list[FailoverRule] = Field(default_factory=list)


class UsageConfig(Base):
    """Usage tracking configuration."""

    enabled: bool = True  # 사용량 추적 활성화
    footer: str = "off"  # 응답 footer 모드: "off" | "tokens" | "full"


class PolicyConfig(Base):
    enabled: bool = False
    trusted_users: list[str] = Field(default_factory=list)
    trusted_channels: list[str] = Field(default_factory=list)
    daily_cost_limit: float = 0.0
    high_risk_tools: list[str] = Field(default_factory=list)


class ObservabilityConfig(Base):
    enabled: bool = False
    otlp_endpoint: str = "http://localhost:4317"
    service_name: str = "shacs-bot"
    sample_rate: float = 1.0


class GatewayConfig(Base):
    """Gateway/server configuration."""

    host: str = "0.0.0.0"
    port: int = 18790
    heartbeat: HeartbeatConfig = Field(default_factory=HeartbeatConfig)


class WebSearchConfig(Base):
    """Web search Configuration."""

    api_key: str = ""  # Brave Search API key
    max_results: int = 5


class WebToolConfig(Base):
    """Web tools configuration."""

    proxy: str | None = (
        None  # HTTP/SOCKS5 proxy URL, e.g. "http://127.0.0.1:7890" or "socks5://127.0.0.1:1080"
    )
    search: WebSearchConfig = Field(default_factory=WebSearchConfig)


class ExecToolConfig(Base):
    """Exec tool configuration."""

    timeout: int = 60
    path_append: str = ""


class MediaConfig(Base):
    """미디어 생성 도구 설정."""

    enabled: bool = False
    backend: str = "openai-compatible"  # "gemini" | "openai-compatible"
    model: str = ""  # 이미지 생성 모델명 (빈 문자열이면 프로바이더 기본값)
    save_dir: str = "~/.shacs-bot/workspace/media"
    video_duration_seconds: int = 8


class MCPServerConfig(Base):
    """MCP 서버 연결 설정 (stdio or HTTP)."""

    command: str = ""  # stdio: command to run (e.g. "npx")
    args: list[str] = Field(
        default_factory=list
    )  # stdio: command args (e.g. ["mcp-server", "--stdio"])
    env: dict[str, str] = Field(default_factory=dict)  # stdio: extra env vars
    url: str = ""  # HTTP: streamable HTTP endpoint URL
    headers: dict[str, str] = Field(default_factory=dict)  # HTTP: Custom HTTP 헤더들
    tool_timeout: int = 30  # 도구 호출이 취소되기 전까지의 시간(초)
    enabled_tools: list[str] = Field(
        default_factory=list
    )  # 활성화할 도구 목록 (비어있으면 전체 활성화)


class ToolsConfig(Base):
    """Configuration for tools."""

    web: WebToolConfig = Field(default_factory=WebToolConfig)
    exec: ExecToolConfig = Field(default_factory=ExecToolConfig)
    media: MediaConfig = Field(default_factory=MediaConfig)
    restrict_to_workspace: bool = False  # If true, block commands accessing path outside workspace
    skill_approval: Literal["auto", "manual", "off"] = "auto"  # 스킬 도구 호출 승인 모드
    mcp_servers: dict[str, MCPServerConfig] = Field(
        default_factory=dict
    )  # MCP 서버별 설정 (키는 서버 식별자)


class HooksConfig(Base):
    """Lifecycle hooks configuration."""

    enabled: bool = False
    outbound_mutation_enabled: bool = False
    redact_payloads: bool = True


class Config(BaseSettings):
    """Root configuration for shacs-bot."""

    model_config = ConfigDict(
        env_prefix="SHACS_BOT_",
        env_nested_delimiter="__",
    )

    env: dict[str, str] = Field(default_factory=dict)  # Global env vars injected into os.environ
    agents: AgentsConfig = Field(default_factory=AgentsConfig)
    channels: ChannelsConfig = Field(default_factory=ChannelsConfig)
    gateway: GatewayConfig = Field(default_factory=GatewayConfig)
    providers: ProvidersConfig = Field(default_factory=ProvidersConfig)
    tools: ToolsConfig = Field(default_factory=ToolsConfig)
    usage: UsageConfig = Field(default_factory=UsageConfig)
    policy: PolicyConfig = Field(default_factory=PolicyConfig)
    failover: FailoverConfig = Field(default_factory=FailoverConfig)
    observability: ObservabilityConfig = Field(default_factory=ObservabilityConfig)
    hooks: HooksConfig = Field(default_factory=HooksConfig)

    @property
    def workspace_path(self) -> Path:
        """Get expanded workspace path."""
        return Path(self.agents.defaults.workspace).expanduser()

    def _match_provider(self, model: str | None = None) -> tuple[ProviderConfig | None, str | None]:
        """프로바이더 설정과 해당 레지스트리 이름을 매칭합니다. (config, spec_name)을 반환합니다."""
        forced: str = self.agents.defaults.provider
        if forced != "auto":
            p: ProviderConfig = getattr(self.providers, forced, None)
            return (p, forced) if p else (None, None)

        model_lower: str = (model or self.agents.defaults.model).lower()
        model_normalized: str = model_lower.replace("-", "_")
        model_prefix: str = model_lower.split("/", 1)[0] if "/" in model_lower else ""
        normalized_prefix: str = model_prefix.replace("-", "_")

        # 명시적인 provider 접두어가 우선 적용됨 — github-copilot/...codex 가 openai_codex 로 잘못 매칭되는 것을 방지합니다.
        from shacs_bot.providers.registry import PROVIDERS

        for spec in PROVIDERS:
            p: ProviderConfig = getattr(self.providers, spec.name, None)
            if p and model_prefix and normalized_prefix == spec.name:
                if spec.is_oauth or p.api_key:
                    return p, spec.name

        # 키워드로 매칭 (순서는 PROVIDERS 레지스트리 정의를 따름)
        def _kw_matches(keyword: str) -> bool:
            kw: str = keyword.lower()
            return (kw in model_lower) or (kw.replace("-", "_") in model_normalized)

        for spec in PROVIDERS:
            p: ProviderConfig = getattr(self.providers, spec.name, None)
            if p and any(_kw_matches(kw) for kw in spec.keywords):
                if spec.is_oauth or p.api_key:
                    return p, spec.name
                env_hint: str = f" (env: {spec.env_key})" if spec.env_key else ""
                logger.warning(
                    "모델 '{}'이 provider '{}'와 매칭되었으나 API 키가 없습니다. "
                    "config.json의 providers.{}.apiKey를 설정하세요{}",
                    model_lower,
                    spec.name,
                    spec.name,
                    env_hint,
                )

        # 폴백: 먼저 게이트웨이(provider)들을 시도하고, 그 다음 나머지를 시도함 (순서는 PROVIDERS 레지스트리를 따름)
        # OAuth 기반 provider는 폴백 대상이 아님 — 반드시 명시적으로 모델을 지정해야 함
        for spec in PROVIDERS:
            if spec.is_oauth:
                continue

            p: ProviderConfig = getattr(self.providers, spec.name, None)
            if p and p.api_key:
                return p, spec.name

        return None, None

    def get_provider(self, model: str | None = None) -> ProviderConfig | None:
        """Get matched provider config (api_key, base_url, extra_headers). Falls back to first available."""
        provider, _ = self._match_provider(model)
        return provider

    def get_provider_name(self, model: str | None = None) -> str | None:
        """매칭된 provider의 레지스트리 이름을 반환합니다 (예: "deepseek", "openrouter")."""
        _, name = self._match_provider(model)
        return name

    def get_api_key(self, model: str | None = None) -> str | None:
        """Get API key for the given model Falls back to first available key."""
        provider: ProviderConfig = self.get_provider(model)
        return provider.api_key if provider else None

    def get_base_url(self, model: str | None = None) -> str | None:
        """Get API base URL for the given model. Applies default URLs for known gateways."""
        provider, name = self._match_provider(model)
        if provider and provider.base_url:
            return provider.base_url

        if name:
            from shacs_bot.providers.registry import ProviderSpec
            from shacs_bot.providers.registry import find_by_name

            spec: ProviderSpec = find_by_name(name)
            if spec and spec.default_base_url:
                return spec.default_base_url

        return None
