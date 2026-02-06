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

class ProvidersConfig(BaseModel):
    """Configuration for LLM providers."""
    anthropic: ProviderConfig = Field(default_factory=ProviderConfig)
    openai: ProviderConfig = Field(default_factory=ProviderConfig)
    openrouter: ProviderConfig = Field(default_factory=ProviderConfig)
    deepseek: ProviderConfig = Field(default_factory=ProviderConfig)
    groq: ProviderConfig = Field(default_factory=ProviderConfig)
    zhipu: ProviderConfig = Field(default_factory=ProviderConfig)
    vllm: ProviderConfig = Field(default_factory=ProviderConfig)
    gemini: ProviderConfig = Field(default_factory=ProviderConfig)
    moonshot: ProviderConfig = Field(default_factory=ProviderConfig)

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
    restrict_to_workspace: bool = False # If true, block commands accessing path outside workspace

class ToolsConfig(BaseModel):
    """Configuration for tools."""
    web: WebToolConfig = Field(default_factory=WebToolConfig)
    exec: ExecToolConfig = Field(default_factory=ExecToolConfig),

class Config(BaseSettings):
    """Root configuration for shacs-bot."""
    model_config = ConfigDict(
        env_prefix = "SHACS_BOT_",
        env_nested_delimiter = "__",
    )

    agents: AgentsConfig = Field(default_factory=AgentsConfig)
    providers: ProvidersConfig = Field(default_factory=ProvidersConfig)
    gateway: GatewayConfig = Field(default_factory=GatewayConfig)
    tools: ToolsConfig = Field(default_factory=ToolsConfig)
    channels: ChannelsConfig = Field(default_factory=ChannelsConfig)

    @property
    def workspace_path(self) -> Path:
        """Get expanded workspace path."""
        return Path(self.agents.defaults.workspace).expanduser()

    def _match_provider(self, model: str | None = None) -> ProviderConfig | None:
        """Match a provider based on model name."""
        model: str = (model or self.agents.defaults.model).lower()
        # Map of keywords to provider configs
        providers: dict[str, ProviderConfig] = {
            "openrouter": self.providers.openrouter,
            "deepseek": self.providers.deepseek,
            "anthropic": self.providers.anthropic,
            "claude": self.providers.anthropic,
            "openai": self.providers.openai,
            "gpt": self.providers.openai,
            "gemini": self.providers.gemini,
            "zhipu": self.providers.zhipu,
            "glm": self.providers.zhipu,
            "zai": self.providers.zhipu,
            "groq": self.providers.groq,
            "moonshot": self.providers.moonshot,
            "kimi": self.providers.moonshot,
            "vllm": self.providers.vllm,
        }
        for keyword, provider in providers.items():
            if keyword in model and provider.api_key:
                return provider
        return None

    def get_api_key(self, model: str | None = None) -> str | None:
        """Get API key for the given model (or default model). Falls back to first available key."""
        # Try matching by model name first
        matched: ProviderConfig | None = self._match_provider(model)
        if matched:
            return matched.api_key
        # Fallback: return first available key
        for provider in [
            self.providers.openrouter, self.providers.deepseek,
            self.providers.anthropic, self.providers.openai,
            self.providers.gemini, self.providers.zhipu,
            self.providers.moonshot, self.providers.vllm,
            self.providers.groq,
        ]:
            if provider.api_key:
                return provider.api_key
        return None

    def get_base_url(self, model: str | None = None) -> str | None:
        """Get API base URL based on model name."""
        model: str  = (model or self.agents.defaults.model).lower()
        if "openrouter" in model:
            return self.providers.openrouter.base_url or "https://openrouter.ai/api/v1"
        if any(k in model for k in ("zhipu", "glm", "zai")):
            return self.providers.zhipu.base_url
        if "vllm" in model:
            return self.providers.vllm.base_url
        return None
