# PRD: OAuth 토큰 저장 경로 통합

---

## 문제

`oauth-cli-kit`이 OAuth 토큰을 shacs-bot 설정 디렉토리와 **별도 경로**에 저장한다:

```
~/.shacs-bot/config.json                              ← shacs-bot 설정
~/.local/share/oauth-cli-kit/auth/codex.json          ← OAuth 토큰 (Linux/Docker)
~/Library/Application Support/oauth-cli-kit/auth/codex.json  ← OAuth 토큰 (macOS)
```

**문제점**:
1. Docker에서 `~/.shacs-bot/`만 볼륨 마운트하면 OAuth 토큰이 유실됨
2. 컨테이너 재시작마다 `provider login openai-codex`를 다시 해야 함
3. 사용자가 토큰 저장 위치를 직관적으로 알 수 없음 (`platformdirs`가 OS별로 다른 경로 생성)
4. 백업/이관 시 두 곳을 챙겨야 함

### 원인

현재 코드에서 `get_token()`과 `login_oauth_interactive()`를 호출할 때 `storage` 파라미터를 전달하지 않아 `oauth-cli-kit`의 기본 경로(`platformdirs.user_data_dir`)를 사용한다:

```python
# providers/openai_codex.py:43
token: OAuthToken = await asyncio.to_thread(get_codex_token)  # storage 미전달

# cli/commands.py:961
token = login_oauth_interactive(
    print_fn=..., prompt_fn=..., originator="shacs-bot",
)  # storage 미전달
```

`oauth-cli-kit`의 `FileTokenStorage`는 `data_dir` 파라미터를 지원하지만, 현재 코드에서 활용하지 않고 있다.

## 해결책

`FileTokenStorage(data_dir=~/.shacs-bot)`을 생성하여 `get_token()`과 `login_oauth_interactive()`에 전달한다.

변경 후 토큰 저장 경로:

```
~/.shacs-bot/config.json        ← 설정
~/.shacs-bot/auth/codex.json    ← OAuth 토큰 (통합)
```

Docker에서는 `~/.shacs-bot/` 하나만 마운트하면 설정과 인증이 모두 유지된다.

## 사용자 영향

| Before | After |
|---|---|
| 토큰이 OS별 다른 경로에 저장 | `~/.shacs-bot/auth/codex.json`에 통일 |
| Docker 볼륨 마운트로 토큰 유지 불가 | `~/.shacs-bot/` 마운트만으로 해결 |
| 기존 사용자의 토큰이 갑자기 사라짐 | 마이그레이션으로 기존 토큰 자동 이전 |

## 기술적 범위

- **변경 파일**: `shacs_bot/providers/openai_codex.py`, `shacs_bot/cli/commands.py` (2개)
- **변경 유형**: Python 코드 수정 (기존 함수 호출에 파라미터 추가)
- **의존성**: 없음. `oauth-cli-kit`의 기존 `FileTokenStorage` API 활용.
- **하위 호환성**:
  - `oauth-cli-kit`의 `FileTokenStorage.load()`는 자체 경로에 토큰이 없으면 `~/.codex/auth.json`(공식 Codex CLI)에서 import를 시도하는 fallback이 내장되어 있음
  - 기존 경로(`platformdirs`)에 있는 토큰은 수동 안내 또는 자동 마이그레이션으로 처리

### 변경할 코드 요약

**공통 storage 팩토리** (신규 헬퍼 또는 인라인):

```python
from oauth_cli_kit.storage import FileTokenStorage
from shacs_bot.config.paths import get_config_dir  # ~/.shacs-bot

def _codex_token_storage() -> FileTokenStorage:
    return FileTokenStorage(data_dir=get_config_dir())
```

> `get_config_dir()` 또는 동등한 경로 함수가 이미 존재하는지 확인 필요. 없으면 `Path.home() / ".shacs-bot"` 직접 사용.

**providers/openai_codex.py**:
- `get_codex_token()` 호출 시 `storage=_codex_token_storage()` 전달

```python
# Before
token: OAuthToken = await asyncio.to_thread(get_codex_token)

# After
token: OAuthToken = await asyncio.to_thread(get_codex_token, storage=_codex_token_storage())
```

**cli/commands.py**:
- `get_token()` 호출 시 `storage` 전달
- `login_oauth_interactive()` 호출 시 `storage` 전달

```python
# Before
token = get_token()
token = login_oauth_interactive(print_fn=..., prompt_fn=..., originator="shacs-bot")

# After
storage = _codex_token_storage()
token = get_token(storage=storage)
token = login_oauth_interactive(print_fn=..., prompt_fn=..., originator="shacs-bot", storage=storage)
```

### 마이그레이션 (기존 사용자)

기존 경로에 토큰이 있고 새 경로에 없는 경우, 최초 `get_token()` 호출 시 자동 복사:

```python
def _codex_token_storage() -> FileTokenStorage:
    storage = FileTokenStorage(data_dir=get_config_dir())
    if not storage.get_token_path().exists():
        _migrate_legacy_token(storage)
    return storage

def _migrate_legacy_token(new_storage: FileTokenStorage) -> None:
    """기존 platformdirs 경로에서 토큰을 새 경로로 이전"""
    legacy = FileTokenStorage()  # 기본 경로 (platformdirs)
    token = legacy.load()
    if token:
        new_storage.save(token)
```

> `_try_import_codex_cli_token()` fallback은 `oauth-cli-kit` 내부에 이미 존재하므로 공식 Codex CLI → shacs-bot 이전도 자동 처리됨.

### config.json 변경

없음. config.json 스키마는 변경하지 않는다.

### 파일 시스템 결과

```
~/.shacs-bot/
  config.json          ← 기존 설정 (변경 없음)
  auth/
    codex.json         ← OAuth 토큰 (신규 위치)
    codex.json.lock    ← refresh 동시성 제어 (oauth-cli-kit 자동 생성)
  workspace/           ← 기존 워크스페이스 (변경 없음)
```

### Docker 사용 예시

```yaml
# docker-compose.yml — 변경 후 이것만으로 인증 유지
volumes:
  - ~/.shacs-bot:/root/.shacs-bot
```

## 성공 기준

1. `provider login openai-codex` 실행 후 `~/.shacs-bot/auth/codex.json`에 토큰 저장됨
2. `openai-codex` 모델 사용 시 `~/.shacs-bot/auth/codex.json`에서 토큰 로드됨
3. Docker에서 `~/.shacs-bot/` 볼륨 마운트만으로 인증 유지됨
4. 기존 `platformdirs` 경로에 토큰이 있는 사용자는 자동 마이그레이션됨
5. 공식 Codex CLI(`~/.codex/auth.json`)에서의 import fallback은 기존과 동일하게 동작

---

## 마일스톤

- [ ] **M1: storage 경로 통합**
  `_codex_token_storage()` 헬퍼 생성. `openai_codex.py`와 `commands.py`에서 `storage` 파라미터 전달.

- [ ] **M2: 레거시 마이그레이션**
  기존 `platformdirs` 경로 → `~/.shacs-bot/auth/` 자동 이전 로직 추가.

- [ ] **M3: 검증**
  로그인 → 토큰 저장 경로 확인 → 모델 호출 → 토큰 로드 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 기존 사용자가 업데이트 후 토큰 못 찾음 | 높음 | 중간 | M2 마이그레이션으로 자동 이전 |
| `~/.shacs-bot/auth/` 권한 문제 (Docker root vs user) | 낮음 | 중간 | `oauth-cli-kit`이 `0o600` 권한 설정, 디렉토리는 `mkdir(parents=True)` |
| `oauth-cli-kit` 업데이트로 `FileTokenStorage` API 변경 | 낮음 | 높음 | `data_dir`은 안정 API. `pyproject.toml`에서 버전 고정 가능 |
| 여러 프로세스가 동시에 refresh 시도 | 낮음 | 낮음 | `oauth-cli-kit`의 `_FileLock` 메커니즘이 이미 처리 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성. `oauth-cli-kit` 소스 분석 완료 (`storage.py`, `flow.py`, `providers/openai_codex.py`). `FileTokenStorage(data_dir=...)` 파라미터 확인. |
