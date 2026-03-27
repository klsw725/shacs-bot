# LiteLLM 제거 → Native SDK 마이그레이션

## 프롬프트

> nanobot commit 확인하여 최신 litellm 대체 commit과 interactive setup wizard commit을 우리에게 반영할수 있는지 설계해서 보여줘봐
> prd 작성하고 진행하자

## 변경 요약

nanobot upstream 커밋 `3dfdab70` (2026-03-24)를 기반으로 litellm 의존성을 완전 제거하고, Anthropic SDK + OpenAI SDK 직접 호출로 교체.

## 변경 파일

| 파일 | 변경 유형 | 설명 |
|------|----------|------|
| `docs/specs/litellm-removal/prds/litellm-removal-and-setup-wizard.md` | 신규 | PRD: LiteLLM 제거 + Interactive Setup Wizard 설계 |
| `shacs_bot/providers/registry.py` | 수정 | `litellm_prefix`/`skip_prefixes` → `backend` 필드. 23개 provider에 `default_base_url` 매핑. `find_by_model()`/`find_gateway()` 제거. |
| `shacs_bot/providers/anthropic_provider.py` | 신규 | AsyncAnthropic 기반 네이티브 provider. chat, tool call, prompt caching, extended thinking 지원. |
| `shacs_bot/providers/openai_compat_provider.py` | 신규 | AsyncOpenAI 기반 통합 provider. 기존 custom.py 기능 병합. 20+ provider 호환. |
| `shacs_bot/providers/litellm.py` | 삭제 | LiteLLM 기반 provider (435줄) |
| `shacs_bot/providers/custom.py` | 삭제 | CustomProvider (openai_compat_provider에 병합) |
| `shacs_bot/providers/failover.py` | 수정 | `LiteLLMProvider` import → backend 분기 (anthropic/openai_compat) |
| `shacs_bot/cli/commands.py` | 수정 | `_make_provider()`: spec.backend 기반 라우팅. GitHub Copilot OAuth trigger: litellm.acompletion → openai.AsyncOpenAI |
| `shacs_bot/agent/usage.py` | 수정 | `litellm.completion_cost()` → 자체 가격 테이블 (`_PRICE_PER_MILLION`) |
| `shacs_bot/config/schema.py` | 수정 | `get_base_url()`: gateway뿐 아니라 모든 provider의 default_base_url 반환 |
| `pyproject.toml` | 수정 | `litellm>=1.82.1` 제거, `anthropic>=0.45.0` 추가 |

## 아키텍처 변경

### Before (LiteLLM 경유)
```
_make_provider() → LiteLLMProvider → litellm.acompletion() → provider API
                 → CustomProvider  → openai.AsyncOpenAI → provider API
```

### After (직접 SDK)
```
_make_provider() → spec.backend switch
  "anthropic"    → AnthropicProvider  → anthropic.AsyncAnthropic → Anthropic API
  "openai_compat"→ OpenAICompatProvider → openai.AsyncOpenAI     → provider API
  "openai_codex" → OpenAICodexProvider → (기존 OAuth)
```

## 미완료

- Track B (Interactive Setup Wizard): 별도 세션에서 진행 필요. PRD에 설계 완료, 구현 미착수.
