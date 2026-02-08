"""Configuration schema using Pydantic."""
from pathlib import Path

from pydantic import BaseModel, Field, ConfigDict
from pydantic_settings import BaseSettings


class DiscordConfig(BaseModel):
    """Discord channel configuration."""
    enable: bool = False,
    token: str = "", # Bot token from Discord Developer Portal
    allow_form: list[str] = Field(default_factory=list), # Allowed user IDs
    gateway_url: str = "wss://gateway.discord.gg/?v=10&encoding=json",
    intents: int = 37377, # GUILDS + GUILD_MESSAGES + DIRECT_MESSAGES + MESSAGE_CONTENT

class ShellConfig(BaseModel):
    """Shell channel configuration."""
    enable: bool = False,

class ChannelsConfig(BaseModel):
    """Configuration for chat channels."""
    discord: DiscordConfig = Field(default_factory=DiscordConfig)

class AgentDefaults(BaseModel):
    """Default agent configuration."""
    workspace: str = "~/.shacs-bot/workspace""",
    model: str = "anthropic/claude-opus-4-5",
    max_tokens: int = 8192,
    temperature: float = 0.7,
    max_tool_iterations: int = 20,

class AgentsConfig(BaseModel):
    """Agent configuration."""
    defaults: AgentDefaults = Field(default_factory=AgentDefaults)

class ProviderConfig(BaseModel):
    """LLM Provider configuration."""
    api_key: str = "",
    base_url: str | None = None,
    extra_headers: dict[str, str] | None = None # Custom headers (e.g. APP-Code for AiHubMix)

class ProvidersConfig(BaseModel):
    """Configuration for LLM providers."""
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
    aihubmix: ProviderConfig = Field(default_factory=ProviderConfig)  # AiHubMix API gateway

class GatewayConfig(BaseModel):
    """Gateway/server configuration."""
    host: str = "0.0.0.0",
    port: int = 18790,

class WebSearchConfig(BaseModel):
    """Web search Configuration."""
    api_key: str = "",  # Brave Search API key
    max_results: int = 5,

class WebToolConfig(BaseModel):
    """Web tools configuration."""
    search: WebSearchConfig = Field(default_factory=WebSearchConfig),

class ExecToolConfig(BaseModel):
    """Exec tool configuration."""
    timeout: int = 60,

class ToolsConfig(BaseModel):
    """Configuration for tools."""
    web: WebToolConfig = Field(default_factory=WebToolConfig)
    exec: ExecToolConfig = Field(default_factory=ExecToolConfig),
    restrict_to_workspace: bool = False # If true, block commands accessing path outside workspace

class Config(BaseSettings):
    """Root configuration for shacs-bot."""
    model_config = ConfigDict(
        env_prefix = "SHACS_BOT_",
        env_nested_delimiter = "__",
    )

    agents: AgentsConfig = Field(default_factory=AgentsConfig)
    channels: ChannelsConfig = Field(default_factory=ChannelsConfig)
    gateway: GatewayConfig = Field(default_factory=GatewayConfig)
    providers: ProvidersConfig = Field(default_factory=ProvidersConfig)
    tools: ToolsConfig = Field(default_factory=ToolsConfig)

    @property
    def workspace_path(self) -> Path:
        """Get expanded workspace path."""
        return Path(self.agents.defaults.workspace).expanduser()

    def get_provider(self, model: str | None = None) -> ProviderConfig | None:
        """Get matched provider config (api_key, base_url, extra_headers). Falls back to first available."""
        from shacs_bot.providers.registry import PROVIDERS
        model: str = (model or self.agents.defaults.model).lower()

        # Match by keyword (order follows PROVIDERS registry)
        for spec in PROVIDERS:
            provider = getattr(self.providers, spec.name, None)
            if provider and (keyword in model for keyword in spec.keywords) and provider.api_key:
                return provider

        # Fallback: gateways first, then others (follows registry order)
        for spec in PROVIDERS:
            provider = getattr(self.providers, spec.name, None)
            if provider and provider.api_key:
                return provider
        return None

    def get_api_key(self, model: str | None = None) -> str | None:
        """Get API key for the given model Falls back to first available key."""
        provider: ProviderConfig = self.get_provider(model)
        return provider.api_key if provider else None

    def get_base_url(self, model: str | None = None) -> str | None:
        """Get API base URL for the given model. Applies default URLs for known gateways."""
        provider: ProviderConfig = self.get_provider(model)
        if provider and provider.api_key:
            return provider.base_url
        # Only gateways get a default URL here. Standard providers (like Moonshot)
        # handle their base URL via env vars in _setup_env, NOT via api_base -
        # otherwise find_gateway() would misdetect them as local/vLLM.
        from shacs_bot.providers.registry import PROVIDERS
        for spec in PROVIDERS:
            if spec.is_gateway and spec.default_base_url and (provider == getattr(self.providers, spec.name, None)):
                return spec.default_base_url
        return None