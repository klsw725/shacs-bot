# PRD: LiteLLM 제거 및 Interactive Setup Wizard

> **출처**: `HKUDS/nanobot` 커밋 `3dfdab70` (2026-03-24), `b2a55017` (2026-03-17) 외 관련 커밋 조사 결과

---

## 문제

### 1. LiteLLM 의존성 — 보안 및 유지보수 부담

nanobot upstream이 2026-03-24에 litellm을 **완전 제거**했다 (`3dfdab70`). 제거 이유:

- **Supply chain risk**: litellm 패키지의 보안 이슈 (선행 커밋 `38ce054b`에서 버전 핀 고정 + advisory 추가)
- **무거운 의존성**: litellm이 끌어오는 transitive dependency가 수백 개 — 설치 시간, 컨테이너 크기, 공격 표면 증가
- **불필요한 추상화**: shacs-bot이 실제 사용하는 litellm 기능은 `acompletion()` + `completion_cost()` 두 가지뿐
- **런타임 부작용**: `LiteLLMProvider.__init__`에서 `os.environ`을 직접 조작하여 환경변수 오염 발생

현재 shacs-bot의 litellm 의존 포인트:

| 위치 | 용도 | 영향 |
|------|------|------|
| `providers/litellm.py` (435줄) | 모든 LLM 호출 | 핵심 경로 |
| `providers/registry.py` (20+ 엔트리) | `litellm_prefix` 매핑 | 전 provider 영향 |
| `agent/usage.py` (L156) | `completion_cost()` 비용 계산 | 보조 기능 |
| `cli/commands.py` (L1040) | OAuth trigger용 `acompletion` | 단일 호출 |
| `pyproject.toml` (L9) | 의존성 선언 | 패키지 메타 |

### 2. Onboard — 수동 JSON 편집 강제

현재 `shacs-bot onboard`는 config 파일 생성만 하고 "API 키를 직접 편집하세요"로 끝난다 (45줄).
사용자가 JSON 구조와 키 이름을 알아야 하며, 오타 하나로 전체 설정이 깨질 수 있다.

nanobot은 2026-03-17에 `questionary` 기반 interactive wizard를 도입하여:
- Provider 선택 → API 키 입력 → 모델 autocomplete → 모델 context 기반 `max_tokens` 추천
- Channel 선택 → 토큰/설정 입력
- 변경 사항 Summary → Save/Discard 선택

## 해결책

두 개의 독립 트랙으로 upstream 패턴을 선별 도입한다.

### Track A: LiteLLM → Native SDK 마이그레이션

litellm 의존성을 완전 제거하고 `anthropic` SDK + `openai` SDK 직접 호출로 교체한다.

### Track B: Interactive Setup Wizard

`shacs-bot onboard --wizard` 플래그로 대화형 설정 마법사를 추가한다.
기존 비대화형 onboard는 그대로 유지하여 CI/스크립트 호환성을 보존한다.

## 사용자 영향

| Before | After |
|---|---|
| litellm transitive deps 수백 개 설치 | openai + anthropic SDK만 설치 (경량) |
| litellm 보안 이슈에 노출 | 직접 SDK만 사용하여 공격 표면 축소 |
| `os.environ` 런타임 오염 | provider별 클라이언트 인스턴스 격리 |
| onboard 후 JSON 수동 편집 필요 | wizard로 대화형 설정 가능 |
| 모델명/키 이름 외워야 함 | autocomplete + 검증 제공 |
| 설정 오류 시 디버깅 어려움 | Summary 패널로 변경 확인 |

## 기술적 범위

### Track A: LiteLLM 제거

- **변경 파일(예상)**:
  - `shacs_bot/providers/litellm.py` → **삭제**
  - `shacs_bot/providers/custom.py` → **삭제** (openai_compat_provider에 병합)
  - `shacs_bot/providers/anthropic_provider.py` → **신규**
  - `shacs_bot/providers/openai_compat_provider.py` → **신규**
  - `shacs_bot/providers/registry.py` → **수정** (backend 필드 도입)
  - `shacs_bot/providers/__init__.py` → **수정** (lazy import 변경)
  - `shacs_bot/providers/base.py` → **수정** (공통 유틸 추가 가능)
  - `shacs_bot/cli/commands.py` → **수정** (`_make_provider` 라우팅 변경)
  - `shacs_bot/agent/usage.py` → **수정** (`completion_cost` 대체)
  - `pyproject.toml` → **수정** (litellm 제거, anthropic 추가)
- **변경 유형**: provider 계층 전면 교체 + registry 스키마 변경
- **의존성 변경**: `litellm>=1.82.1` 제거 → `anthropic>=0.45.0` 추가 (openai는 이미 존재)
- **하위 호환성**:
  - config.json 형식 변경 없음 (provider 설정 키는 동일)
  - LLM 호출 인터페이스(`LLMProvider.chat()`, `chat_stream()`) 변경 없음
  - `ProviderSpec`에서 `litellm_prefix`, `skip_prefixes` 필드 제거 — 이 필드를 직접 참조하는 외부 코드가 있다면 깨짐

### Track B: Interactive Setup Wizard

- **변경 파일(예상)**:
  - `shacs_bot/cli/onboard.py` → **신규** (nanobot `cli/onboard.py` 포팅)
  - `shacs_bot/cli/commands.py` → **수정** (onboard 커맨드에 `--wizard` 플래그 추가)
  - `shacs_bot/cli/models.py` → **신규** (모델 데이터베이스/autocomplete 지원)
  - `shacs_bot/config/loader.py` → **수정** (`model_dump(mode="json")` crash fix)
  - `pyproject.toml` → **수정** (wizard optional dependency 추가)
- **변경 유형**: 신규 모듈 추가 + CLI 확장
- **의존성 추가**: `questionary>=2.1.0` (optional, `[wizard]` extra)
- **하위 호환성**:
  - `--wizard` 없이 실행하면 기존 동작과 100% 동일
  - questionary 미설치 시에도 기본 onboard는 정상 동작

## 상세 범위

### Track A — Phase 1: Registry 스키마 전환

**대상 파일**: `shacs_bot/providers/registry.py`

#### 문제

현재 `ProviderSpec`은 litellm 전용 필드(`litellm_prefix`, `skip_prefixes`)에 의존한다.
이 필드들은 litellm의 모델명 prefix 규칙을 위한 것으로, 네이티브 SDK에서는 불필요하다.

#### 해결

- `litellm_prefix: str` → `backend: str` 필드로 교체
  - 값: `"openai_compat"` | `"anthropic"` | `"azure_openai"` | `"openai_codex"`
- `skip_prefixes: tuple[str, ...]` → 삭제
- `litellm_kwargs: dict` → 삭제 (이미 사용되지 않음)
- `default_base_url: str` — 각 provider의 OpenAI-compatible endpoint URL을 명시
- `find_by_model()`, `find_gateway()` → 삭제 (dead heuristics)
- `find_by_name()` — 유지 (유일하게 필요한 lookup)

**각 provider별 `backend` + `default_base_url` 매핑:**

| Provider | backend | default_base_url |
|----------|---------|------------------|
| custom | openai_compat | (사용자 지정) |
| azure_openai | azure_openai | (사용자 지정) |
| openrouter | openai_compat | `https://openrouter.ai/api/v1` |
| aihubmix | openai_compat | `https://aihubmix.com/v1` |
| siliconflow | openai_compat | `https://api.siliconflow.cn/v1` |
| volcengine | openai_compat | `https://ark.cn-beijing.volces.com/api/v3` |
| anthropic | anthropic | (SDK 기본값) |
| openai | openai_compat | (SDK 기본값) |
| openai_codex | openai_codex | `https://chatgpt.com/backend-api` |
| github_copilot | openai_compat | `https://api.githubcopilot.com` |
| deepseek | openai_compat | `https://api.deepseek.com` |
| gemini | openai_compat | `https://generativelanguage.googleapis.com/v1beta/openai/` |
| zhipu | openai_compat | `https://open.bigmodel.cn/api/paas/v4` |
| dashscope | openai_compat | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| moonshot | openai_compat | `https://api.moonshot.ai/v1` |
| minimax | openai_compat | `https://api.minimax.io/v1` |
| mistral | openai_compat | `https://api.mistral.ai/v1` |
| vllm | openai_compat | (사용자 지정) |
| ollama | openai_compat | `http://localhost:11434/v1` |
| ovms | openai_compat | `http://localhost:8000/v3` |
| groq | openai_compat | `https://api.groq.com/openai/v1` |

#### 성공 기준

1. 모든 `ProviderSpec`에 `backend` 필드가 있고 `litellm_prefix`는 존재하지 않는다.
2. 기존 `_make_provider()`에서 `litellm_prefix`를 참조하는 코드가 없다.
3. `find_by_name()`으로 모든 provider를 정확히 조회할 수 있다.

### Track A — Phase 2: Native Provider 구현

**대상 파일**: `shacs_bot/providers/anthropic_provider.py` (신규), `shacs_bot/providers/openai_compat_provider.py` (신규)

#### 문제

현재 모든 LLM 호출이 `LiteLLMProvider.chat()` → `litellm.acompletion()`을 경유한다.
이 경로를 네이티브 SDK 호출로 교체해야 한다.

#### 해결

**`AnthropicProvider`** (nanobot `anthropic_provider.py` 포팅):
- `anthropic.AsyncAnthropic` 클라이언트 직접 생성
- OpenAI chat format → Anthropic Messages API 변환 (`_convert_messages`)
- system message 분리, tool result block 변환
- prompt caching (`cache_control` 마커 주입)
- extended thinking 지원
- streaming 지원 (`chat_stream`)
- tool call 파싱 (JSON repair 포함)

**`OpenAICompatProvider`** (nanobot `openai_compat_provider.py` 포팅 + 기존 `custom.py` 병합):
- `openai.AsyncOpenAI` 클라이언트 직접 생성
- `ProviderSpec`을 받아 `base_url` 자동 설정
- provider별 `env_extras` 환경변수 설정
- prompt caching (Anthropic via OpenRouter 등)
- streaming 지원
- tool call 파싱
- `strip_model_prefix` 처리 (gateway용)
- `model_overrides` 적용 (예: kimi-k2.5 temperature 강제)

**공통 요구사항**:
- `LLMProvider` 인터페이스 준수 (`chat()`, `chat_stream()`, `LLMResponse` 반환)
- 기존 `_strip_content_meta()` 호출 유지 (multimodal history fidelity)
- error handling: generic Exception → `LLMResponse(finish_reason="error")` 패턴 유지

#### 성공 기준

1. `AnthropicProvider`로 Claude 모델 chat + streaming이 정상 동작한다.
2. `OpenAICompatProvider`로 OpenAI, DeepSeek, Gemini, OpenRouter 등 기존 provider 전체가 정상 동작한다.
3. 기존 `LiteLLMProvider`의 모든 public 메서드가 새 provider에 동등하게 구현되어 있다.
4. tool call, prompt caching, extended thinking이 provider별로 정상 동작한다.

### Track A — Phase 3: 라우팅 및 정리

**대상 파일**: `shacs_bot/cli/commands.py`, `shacs_bot/agent/usage.py`, `pyproject.toml`

#### 문제

`_make_provider()`가 여전히 `LiteLLMProvider`를 기본으로 생성하고, `usage.py`가 `litellm.completion_cost()`에 의존한다.

#### 해결

- `_make_provider()`: `spec.backend`로 분기
  ```python
  match spec.backend:
      case "anthropic":
          return AnthropicProvider(...)
      case "openai_compat":
          return OpenAICompatProvider(..., spec=spec)
      case "openai_codex":
          return OpenAICodexProvider(...)
      case "azure_openai":
          return AzureOpenAIProvider(...)
  ```
- `usage.py`: `litellm.completion_cost()` → stub 함수로 교체 (가격 테이블 미구현 시 0 반환 + warning)
- `commands.py` L1040: OAuth trigger의 `litellm.acompletion()` → `openai.AsyncOpenAI()` 직접 호출
- `pyproject.toml`: `litellm>=1.82.1` 제거, `anthropic>=0.45.0` 추가
- `providers/litellm.py` 삭제, `providers/custom.py` 삭제
- `providers/__init__.py` lazy import 업데이트

#### 성공 기준

1. `import litellm`이 코드베이스 어디에도 없다.
2. `pyproject.toml`에 litellm 의존성이 없다.
3. `uv sync` 후 litellm이 설치되지 않는다.
4. 기존 config.json 변경 없이 모든 provider가 정상 동작한다.

### Track B — Phase 1: Wizard 기반 구축

**대상 파일**: `shacs_bot/cli/onboard.py` (신규), `shacs_bot/cli/models.py` (신규)

#### 문제

nanobot의 onboard.py를 그대로 가져올 수 없다. namespace, config 스키마, 채널 구조가 다르다.

#### 해결

nanobot `cli/onboard.py`를 기반으로 포팅하되 다음을 조정한다:

- 모든 `nanobot.*` import → `shacs_bot.*`으로 변경
- `nanobot.config.schema.Config` → `shacs_bot.config.schema.Config`
- `nanobot.providers.registry` → `shacs_bot.providers.registry`
- `nanobot.channels.registry.discover_all()` → shacs-bot의 채널 목록 하드코딩 또는 동적 탐색
- config 필드명 차이 대응: nanobot `api_base` vs shacs-bot `base_url` 등
- `cli/models.py`: 모델 이름 제안 + context window 추천을 위한 최소 데이터베이스
  - Phase 2 (LiteLLM 제거) 전: litellm의 모델 리스트 활용 가능
  - Phase 2 이후: 자체 정적 테이블 또는 외부 API

**Wizard 메뉴 구조** (nanobot 동일):
```
[P] LLM Provider     — provider 선택 → API 키 → 모델 → endpoint
[C] Chat Channel     — 채널 선택 → 토큰/설정
[A] Agent Settings   — model, temperature, max_tokens 등
[G] Gateway          — port, heartbeat 등
[T] Tools            — web search, exec, media 등
[V] View Summary     — 전체 설정 요약
[S] Save and Exit
[X] Exit Without Saving
```

**핵심 포팅 기능**:
- `_configure_pydantic_model()`: Pydantic 모델 필드를 자동으로 입력 폼으로 변환
- `_select_with_back()`: Escape/← 키로 뒤로 가기
- `_input_model_with_autocomplete()`: 모델명 자동완성
- `_input_max_tokens_with_recommendation()`: 모델 context 기반 `max_tokens` 추천
- `_show_summary()`: Rich 기반 설정 요약 패널
- `_mask_value()`: API 키 마스킹 (마지막 4자만 표시)

#### 성공 기준

1. `shacs-bot onboard --wizard`로 대화형 wizard가 실행된다.
2. Provider, Channel, Agent Settings, Gateway, Tools를 wizard에서 설정할 수 있다.
3. Save 시 `~/.shacs-bot/config.json`이 올바른 camelCase로 저장된다.
4. `--wizard` 없이 실행하면 기존 비대화형 onboard와 100% 동일하게 동작한다.
5. `questionary` 미설치 시 `--wizard` 플래그에 대해 명확한 에러 메시지를 보여준다.

### Track B — Phase 2: CLI 통합 및 의존성

**대상 파일**: `shacs_bot/cli/commands.py`, `shacs_bot/config/loader.py`, `pyproject.toml`

#### 해결

- `commands.py` onboard 커맨드에 `--wizard` / `-w` 옵션 추가
- `config/loader.py` `save_config()`: `model_dump(mode="json", by_alias=True)` 적용 (nanobot crash fix `d70ed0d9`)
- `pyproject.toml`: `[project.optional-dependencies]` wizard extra 추가

```toml
[project.optional-dependencies]
wizard = [
    "questionary>=2.1.0,<3.0.0",
]
```

#### 성공 기준

1. `uv sync --extra wizard` 후 `shacs-bot onboard --wizard`가 정상 실행된다.
2. `uv sync` (wizard 없이) 후 `shacs-bot onboard`은 기존과 동일하게 동작한다.
3. `save_config()`가 Pydantic 모델의 non-JSON-serializable 타입을 올바르게 처리한다.

## 마일스톤

### Track A: LiteLLM 제거

- [x] **A-M1: Registry 스키마 전환**
  `ProviderSpec`에 `backend` 필드 도입, `litellm_prefix`/`skip_prefixes` 제거, provider별 `default_base_url` 매핑.

- [x] **A-M2: AnthropicProvider 구현**
  네이티브 Anthropic SDK 기반 provider. chat, streaming, tool call, prompt caching, extended thinking 지원.

- [x] **A-M3: OpenAICompatProvider 구현**
  AsyncOpenAI 기반 통합 provider. 기존 custom.py 기능 병합. 20+ provider 호환.

- [x] **A-M4: 라우팅 변경 및 정리**
  `_make_provider()` backend 분기. litellm.py/custom.py 삭제. pyproject.toml 정리. usage.py 대체.

### Track B: Interactive Setup Wizard

- [x] **B-M1: Wizard 모듈 포팅**
  nanobot onboard.py 기반으로 `shacs_bot/cli/onboard.py` 구현. Provider/Channel/Settings 메뉴.

- [x] **B-M2: CLI 통합**
  `--wizard` 플래그, loader crash fix, questionary optional dependency 추가.

## 우선순위

| 항목 | 우선순위 | 이유 |
|---|---|---|
| Track B (Setup Wizard) | **P0** | Track A와 독립. 즉시 착수 가능. 사용자 UX 직접 개선. |
| Track A Phase 1 (Registry) | **P0** | 후속 Phase의 기반. 단독으로도 코드 정리 가치. |
| Track A Phase 2 (Providers) | **P1** | 핵심 구현. Phase 1 완료 후 착수. |
| Track A Phase 3 (정리) | **P1** | 최종 정리. Phase 2 완료 후 착수. |

**실행 순서**:
```
[Track B: Wizard]  ←── 독립, 병렬 가능
   B-M1 → B-M2

[Track A: LiteLLM 제거]
   A-M1 → A-M2 → A-M3 → A-M4
```

Track B는 Track A와 완전 독립이므로 병렬 진행 가능하다.
Track A 내부는 순차 의존성이 있다 (Phase 1 → 2 → 3).

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| Provider별 API 차이로 일부 provider 동작 깨짐 | 중간 | 높음 | nanobot 구현이 이미 20+ provider를 검증함. 포팅 후 provider별 수동 테스트. |
| Anthropic Messages API 변환 버그 | 중간 | 높음 | nanobot의 441줄 구현을 그대로 포팅. edge case는 기존 세션으로 회귀 테스트. |
| `completion_cost()` 제거로 usage tracking 정확도 하락 | 높음 | 중간 | Phase 3에서 자체 가격 테이블 구현 또는 "비용 미지원" warning으로 단계적 대응. |
| Streaming 동작 차이 (provider별) | 중간 | 높음 | OpenAI SDK의 streaming이 litellm보다 안정적. provider별 streaming 테스트 필수. |
| questionary가 일부 터미널에서 깨짐 | 낮음 | 중간 | optional dependency로 격리. `--wizard` 없이는 기존 동작 유지. |
| config 필드명 차이 (nanobot vs shacs-bot) | 높음 | 중간 | 포팅 시 diff 기반으로 필드명 매핑 테이블 작성 후 진행. |
| `find_by_model()` 제거 후 model→provider 자동 매칭 실패 | 중간 | 높음 | 현재 `_make_provider`가 이미 `find_by_name`만 사용하는지 검증. 자동 매칭 경로가 있다면 대안 구현. |
| nanobot의 channel registry 구조 차이 | 높음 | 중간 | shacs-bot은 아직 channel plugin 아키텍처가 없으므로, wizard에서 채널 목록을 하드코딩으로 시작. |

## 참고 커밋

| SHA | 날짜 | 내용 | Track |
|-----|------|------|-------|
| `3dfdab70` | 2026-03-24 | `refactor: replace litellm with native openai + anthropic SDKs` | A |
| `38ce054b` | 2026-03-24 | `fix(security): pin litellm and add supply chain advisory note` | A (배경) |
| `c3031c9c` | 2026-03-24 | `docs: update news section about litellm` | A (배경) |
| `b2a55017` | 2026-03-17 | `feat(onboard): align setup with config and workspace flags` | B |
| `40a022af` | 2026-03-17 | `fix(onboard): use configured workspace path on setup` | B |
| `d70ed0d9` | 2026-03-19 | `fix: nanobot onboard update config crash` | B |
| `20494a2c` | 2026-03-23 | `refactor command routing for future plugins and clearer CLI structure` | B (참고) |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-25 | nanobot 최신 커밋 조사. `3dfdab70` (litellm 제거, 18파일 변경, -1034줄), `b2a55017` (interactive onboard wizard) 확인. shacs-bot 코드베이스 대비 영향도 분석 완료. PRD 초안 작성. |
| 2026-03-25 | Track A 완료. registry.py backend 필드 도입(23 providers), anthropic_provider.py/openai_compat_provider.py 신규 생성, litellm.py/custom.py 삭제, commands.py/failover.py/usage.py/schema.py 수정, pyproject.toml litellm→anthropic 교체. `uv sync` 후 litellm 완전 제거 확인. Track B 미착수. |
| 2026-03-25 | Track B 완료. `onboard --wizard`에 Tools 메뉴(web/exec/media), 실제 discard 동작, summary 표시를 추가. Agent Settings 설명을 shipped behavior에 맞춰 `context_window` 대신 모델 context 기반 `max_tokens` 추천으로 정리. |
