# PRD: config.json 글로벌 환경변수 주입

---

## 문제

현재 config.json을 통해 환경변수를 주입하는 경로는 **provider API 키 전용**이다:

```python
# providers/litellm.py:81
os.environ[spec.env_key] = api_key  # e.g. ANTHROPIC_API_KEY
```

**문제점**:
1. `NOTION_KEY`, `GITHUB_TOKEN`, `BRAVE_API_KEY` 등 **LLM provider가 아닌** 환경변수를 config.json으로 관리할 방법이 없음
2. MCP 서버의 `env` 필드는 해당 **자식 프로세스에만** 전달됨 — shacs-bot 프로세스 자체에는 주입되지 않음
3. 결과적으로 사용자는 셸 `export`나 `.env` 파일 등 config.json 외부 메커니즘에 의존해야 함
4. API 키를 한 곳(`config.json`)에서 관리하고 싶은 사용자에게 일관성이 깨짐

## 해결책

config.json 최상위에 `env` 필드를 추가하고, 설정 로드 직후 `os.environ`에 주입한다.

```json
{
  "env": {
    "NOTION_KEY": "ntn_xxx",
    "BRAVE_API_KEY": "BSA_xxx",
    "MY_CUSTOM_VAR": "some-value"
  },
  "providers": { ... },
  "tools": { ... }
}
```

## 사용자 영향

| Before | After |
|---|---|
| provider API 키만 config.json으로 관리 가능 | 임의의 환경변수를 config.json으로 관리 가능 |
| MCP 서버 env는 자식 프로세스 전용 | 글로벌 env는 shacs-bot + 자식 프로세스 모두 적용 |
| 셸 export / .env 파일 의존 | config.json 한 곳에서 통합 관리 |
| 기존 설정에 `env` 없음 → 변화 없음 | 동일 (하위 호환) |

## 기술적 범위

- **변경 파일**: `shacs_bot/config/schema.py`, `shacs_bot/config/loader.py` (2개)
- **변경 유형**: Python 코드 추가/수정
- **의존성**: 없음. 기존 패키지만 사용.
- **하위 호환성**: `env` 미지정 시 기존과 동일하게 동작.

### 변경할 코드 요약

**schema.py**:
- `Config`에 `env: dict[str, str]` 필드 추가 (기본값: 빈 dict)
- `Config`는 `BaseSettings` 상속이므로 `alias_generator`가 최상위엔 적용 안 됨 — 필드명 `env`는 JSON에서도 `env`

**loader.py**:
- `load_config()` 반환 직후 호출되는 `apply_env(config)` 함수 추가
- 또는 `load_config()` 내부에서 Config 생성 후 즉시 주입
- `os.environ.setdefault(key, value)` 사용 — 기존 시스템 환경변수를 덮어쓰지 않음

### 주입 시점

```
프로세스 시작
  → load_config()
    → Config.model_validate(data)
    → config.env 순회 → os.environ.setdefault(k, v)  ← 여기
  → LiteLLMProvider 생성 (_setup_env)
  → MCP 서버 연결
  → 에이전트 루프 시작
```

provider `_setup_env`보다 **먼저** 실행되므로, `env`에 넣은 값이 provider env_key와 겹치면 provider 쪽이 이김 (provider는 gateway일 때 `os.environ[key] = value`로 덮어쓰기).

### config.json 예시

```json
{
  "env": {
    "NOTION_KEY": "ntn_xxxxxxxxxxxx",
    "BRAVE_API_KEY": "BSA_xxxxxxxxxxxx",
    "HTTP_PROXY": "http://127.0.0.1:7890"
  },
  "providers": {
    "anthropic": { "apiKey": "sk-ant-..." }
  },
  "tools": {
    "mcpServers": {
      "notion": {
        "command": "npx",
        "args": ["-y", "@notionhq/notion-mcp-server"],
        "enabledTools": ["search", "get_page"]
      }
    }
  }
}
```

위 예시에서:
- `NOTION_KEY`가 shacs-bot 프로세스에 주입됨 → MCP 서버 자식 프로세스도 상속받음
- MCP 서버 `env` 필드와 **별개** — `env` 필드는 해당 서버에만 추가 전달, 글로벌 `env`는 프로세스 전체

### 동작 규칙

1. **`setdefault` 사용**: 시스템 환경변수(`export`)가 이미 있으면 config.json 값은 무시됨 — 시스템 환경변수 우선
2. **provider env_key와 중복 시**: provider `_setup_env`가 나중에 실행되므로 provider 값이 이김 (gateway 모드) 또는 `setdefault`로 먼저 설정된 글로벌 값이 유지됨 (일반 모드)
3. **빈 값 허용하지 않음**: value가 빈 문자열이면 주입하지 않음 (실수 방지)

## 성공 기준

1. `config.json`의 `env`에 키-값 쌍 지정 → `os.environ`에서 접근 가능
2. `env` 미지정 또는 빈 dict → 기존과 동일하게 동작 (하위 호환)
3. 시스템 환경변수가 이미 존재하면 config 값이 덮어쓰지 않음 (`setdefault`)
4. MCP 서버 자식 프로세스도 글로벌 env를 상속받음 (OS 기본 동작)
5. 빈 문자열 value는 무시됨

---

## 마일스톤

- [ ] **M1: env 필드 추가 및 주입 구현**
  `Config`에 `env: dict[str, str]` 필드 추가, `load_config()` 내에서 `os.environ.setdefault` 주입.

- [ ] **M2: 실행 검증**
  `env`에 지정한 값이 프로세스 내에서 `os.environ`으로 접근 가능한지 확인. 시스템 환경변수 우선순위 확인. 미지정 시 기존 동작 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 민감한 키가 config.json에 평문 저장 | 높음 | 높음 | 기존 provider API 키와 동일한 위험 수준. `chmod 600` 권장 (SECURITY.md에 이미 명시됨) |
| 시스템 환경변수와 충돌 | 낮음 | 낮음 | `setdefault` 사용으로 시스템 값 우선. 로그에 스킵 사실 출력 |
| provider env_key와 중복 | 낮음 | 낮음 | provider `_setup_env`가 후순위로 실행되어 자연스럽게 해결 |
| 설정 마이그레이션 필요 | 없음 | 없음 | 새 필드의 기본값이 빈 dict → 기존 설정 그대로 동작 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-17 | PRD 초안 작성. 기존 env 주입 경로 분석 완료 (provider `_setup_env`, MCP `env`). 글로벌 `env` 필드 설계. |
