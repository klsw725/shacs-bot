from __future__ import annotations

_MODELS: dict[str, dict[str, int | str]] = {
    "anthropic/claude-sonnet-4-20250514": {"ctx": 200_000, "provider": "anthropic"},
    "anthropic/claude-opus-4-20250514": {"ctx": 200_000, "provider": "anthropic"},
    "anthropic/claude-3-5-haiku-20241022": {"ctx": 200_000, "provider": "anthropic"},
    "gpt-4o": {"ctx": 128_000, "provider": "openai"},
    "gpt-4o-mini": {"ctx": 128_000, "provider": "openai"},
    "gpt-4.1": {"ctx": 1_047_576, "provider": "openai"},
    "gpt-4.1-mini": {"ctx": 1_047_576, "provider": "openai"},
    "gpt-4.1-nano": {"ctx": 1_047_576, "provider": "openai"},
    "o3": {"ctx": 200_000, "provider": "openai"},
    "o3-mini": {"ctx": 200_000, "provider": "openai"},
    "o4-mini": {"ctx": 200_000, "provider": "openai"},
    "deepseek-chat": {"ctx": 128_000, "provider": "deepseek"},
    "deepseek-reasoner": {"ctx": 128_000, "provider": "deepseek"},
    "gemini-2.5-pro": {"ctx": 1_048_576, "provider": "gemini"},
    "gemini-2.5-flash": {"ctx": 1_048_576, "provider": "gemini"},
    "gemini-2.0-flash": {"ctx": 1_048_576, "provider": "gemini"},
    "qwen-max": {"ctx": 128_000, "provider": "dashscope"},
    "kimi-k2.5": {"ctx": 131_072, "provider": "moonshot"},
    "llama-3.3-70b-versatile": {"ctx": 128_000, "provider": "groq"},
}


def get_model_suggestions(prefix: str = "", provider: str = "", limit: int = 20) -> list[str]:
    results = []
    for name, meta in _MODELS.items():
        if provider and meta.get("provider") != provider:
            continue
        if prefix and prefix.lower() not in name.lower():
            continue
        results.append(name)
        if len(results) >= limit:
            break
    return results


def get_model_context_limit(model_name: str, provider: str = "") -> int | None:
    meta = _MODELS.get(model_name)
    if meta:
        return int(meta["ctx"])
    model_lower = model_name.lower()
    for name, meta in _MODELS.items():
        if name in model_lower or model_lower in name:
            return int(meta["ctx"])
    return None


def format_token_count(count: int) -> str:
    if count >= 1_000_000:
        return f"{count / 1_000_000:.1f}M"
    if count >= 1_000:
        return f"{count / 1_000:.0f}K"
    return str(count)
