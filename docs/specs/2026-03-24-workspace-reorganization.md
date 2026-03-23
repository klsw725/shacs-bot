# SPEC: Workspace 디렉토리 재구성

> **Prompt**: 스킬마다 생성하는 파일로 워크스페이스가 어지러워지는 느낌이야. 약간 분리를 시키는게 좋지 않을까? 하위 폴더를 만든다던지.

## TL;DR

> **목적**: `~/.shacs-bot/` 디렉토리 내 파일들을 역할별로 분리하여, 스킬/에이전트가 늘어나도 깔끔한 구조를 유지한다.
>
> **Deliverables**:
> - `shacs_bot/config/paths.py` — 경로 헬퍼 재구성
> - `shacs_bot/config/loader.py` — 마이그레이션 로직 추가
> - 관련 모듈들의 경로 참조 업데이트
>
> **Estimated Effort**: Medium (1일)

## 현재 상태 분석

### 현재 디렉토리 구조

```
~/.shacs-bot/
├── config.json                         # 설정
├── auth/                               # OAuth 토큰
├── cron/jobs.json                      # 크론 (CLI 모드)
├── media/                              # 채널에서 다운로드된 미디어
├── usage/{YYYY-MM-DD}.jsonl            # 사용량 추적
├── logs/                               # 로그
├── history/cli_history                 # CLI 히스토리
├── bridge/                             # WhatsApp 브리지
├── sessions/                           # 레거시 세션
├── mochat/                             # Mochat 상태
│
└── workspace/                          # ← 여기가 문제
    ├── SOUL.md                         # 페르소나 템플릿
    ├── AGENTS.md                       # 에이전트 지시 템플릿
    ├── HEARTBEAT.md                    # 하트비트 템플릿
    ├── TOOLS.md                        # 도구 사용 노트
    ├── USER.md                         # 사용자 프로필
    ├── .clawhub/lock.json              # ClaWHub 락
    ├── cron/jobs.json                  # 크론 (게이트웨이 모드) ← 중복!
    ├── media/                          # 생성된 미디어 ← 중복!
    ├── memory/MEMORY.md                # 장기 메모리
    ├── memory/HISTORY.md               # 히스토리 로그
    ├── sessions/{key}.jsonl            # 세션 데이터
    ├── skills/                         # 워크스페이스 스킬
    └── tmp_youtube_transcript_*.txt    # 스킬 임시 파일 ← 어지러움
```

### 문제점

| # | 문제 | 상세 |
|---|---|---|
| 1 | **역할 혼재** | workspace에 템플릿(.md), 런타임(sessions), 스킬 출력(tmp_*), 메모리가 뒤섞임 |
| 2 | **중복 경로** | `cron/`이 상위와 workspace 양쪽에 존재 (CLI vs 게이트웨이 비일관) |
| 3 | **중복 경로** | `media/`가 상위(채널 다운로드)와 workspace(생성 미디어) 양쪽에 존재 |
| 4 | **스킬 오염** | 스킬이 생성하는 파일이 workspace 루트에 쌓임 (유튜브 트랜스크립트 등) |
| 5 | **정리 없음** | 임시 파일을 정리하는 로직이 없음 |
| 6 | **확장성** | 에이전트 스토어 도입 시 agents/ 디렉토리까지 추가되면 더 혼잡해짐 |

### 파일 생성 주체별 정리

| 역할 | 파일 | 생성 주체 | 현재 위치 |
|---|---|---|---|
| **설정** | config.json | loader.py | `~/.shacs-bot/` |
| **템플릿** | SOUL.md, AGENTS.md, HEARTBEAT.md, TOOLS.md, USER.md | sync_workspace_template() | `workspace/` |
| **메모리** | MEMORY.md, HISTORY.md | memory.py | `workspace/memory/` |
| **세션** | {key}.jsonl | session/manager.py | `workspace/sessions/` |
| **크론** | jobs.json | cron/service.py | `cron/` 또는 `workspace/cron/` (비일관) |
| **사용량** | {date}.jsonl | usage.py | `usage/` |
| **미디어 (다운)** | {file_id}.ext | 채널 어댑터 | `media/` |
| **미디어 (생성)** | image_*.png, video_*.mp4 | media.py | `workspace/media/` |
| **CLI 히스토리** | cli_history | prompt_toolkit | `history/` |
| **OAuth** | 토큰 파일 | oauth providers | `auth/` |
| **브리지** | node_modules, dist 등 | npm | `bridge/` |
| **스킬 출력** | tmp_*.txt 등 | 스킬 스크립트 / LLM 도구 | `workspace/` (루트에 산재) |
| **에이전트** | agent.yaml, SKILL.md 등 | AgentManager (신규) | `agents/` (계획) |

## 설계

### 새 디렉토리 구조

```
~/.shacs-bot/
├── config.json                         # 설정 (변경 없음)
│
├── workspace/                          # 에이전트 작업 공간 (write_file 도구의 기본 cwd)
│   ├── SOUL.md                         # 템플릿 (변경 없음)
│   ├── AGENTS.md
│   ├── HEARTBEAT.md
│   ├── TOOLS.md
│   ├── USER.md
│   ├── skills/                         # 워크스페이스 스킬 (변경 없음)
│   └── sandbox/                        # 스킬/에이전트 출력물 (스킬별 하위 폴더)
│       └── youtube/                    # 예: youtube 스킬 출력
│           └── transcript_*.txt
│
├── data/                               # 런타임 데이터 (시스템이 관리)
│   ├── memory/                         # 장기 메모리
│   │   ├── MEMORY.md
│   │   └── HISTORY.md
│   ├── sessions/                       # 세션 히스토리
│   │   └── {key}.jsonl
│   ├── cron/                           # 크론 작업
│   │   └── jobs.json
│   ├── usage/                          # 사용량 추적
│   │   └── {date}.jsonl
│   └── clawhub/                        # ClaWHub 락
│       └── lock.json
│
├── media/                              # 모든 미디어 (다운로드 + 생성)
│   ├── downloads/                      # 채널에서 받은 미디어
│   │   └── {channel}/                  # 채널별 하위 폴더
│   └── generated/                      # 에이전트가 생성한 미디어
│
├── agents/                             # 설치된 에이전트 (Agent Store)
│   ├── registry.json
│   └── {agent-name}/
│
├── auth/                               # OAuth (변경 없음)
├── bridge/                             # WhatsApp 브리지 (변경 없음)
├── logs/                               # 로그 (변경 없음)
└── history/                            # CLI 히스토리 (변경 없음)
```

### 핵심 변경 사항

| # | 변경 | 이전 | 이후 | 영향 |
|---|---|---|---|---|
| 1 | **메모리 분리** | `workspace/memory/` | `data/memory/` | memory.py |
| 2 | **세션 분리** | `workspace/sessions/` | `data/sessions/` | session/manager.py |
| 3 | **크론 통합** | `cron/` 또는 `workspace/cron/` | `data/cron/` | commands.py, cron/service.py |
| 4 | **사용량 이동** | `usage/` | `data/usage/` | paths.py |
| 5 | **ClaWHub 이동** | `workspace/.clawhub/` | `data/clawhub/` | clawhub 스킬 |
| 6 | **미디어 통합** | `media/` + `workspace/media/` | `media/downloads/` + `media/generated/` | 채널 어댑터, media.py |
| 7 | **출력 디렉토리** | workspace 루트에 산재 | `workspace/sandbox/{source}/` | 스킬, 에이전트 도구 |
| 8 | **workspace 정리** | 템플릿 + 런타임 + 출력 혼재 | 템플릿 + 스킬 정의 + sandbox 출력 | write_file 도구의 기본 cwd 유지 |

### paths.py 변경

```python
# config/paths.py — 새 경로 헬퍼

def get_data_dir() -> Path:
    """인스턴스 수준의 런타임 데이터 디렉터리를 반환한다."""
    return ensure_dir(get_config_path().parent)      # ~/.shacs-bot/ (변경 없음)

# --- 신규 ---

def get_data_subdir(name: str) -> Path:
    """data/ 하위 디렉토리를 반환한다."""
    return ensure_dir(get_data_dir() / "data" / name)

def get_memory_dir() -> Path:
    return get_data_subdir("memory")

def get_sessions_dir() -> Path:
    return get_data_subdir("sessions")

def get_cron_dir() -> Path:
    return get_data_subdir("cron")                   # 기존: get_runtime_subdir("cron")

def get_usage_dir() -> Path:
    return get_data_subdir("usage")                  # 기존: get_runtime_subdir("usage")

def get_clawhub_dir() -> Path:
    return get_data_subdir("clawhub")

# --- 미디어 ---

def get_media_downloads_dir(channel: str | None = None) -> Path:
    base = ensure_dir(get_data_dir() / "media" / "downloads")
    return ensure_dir(base / channel) if channel else base

def get_media_generated_dir() -> Path:
    return ensure_dir(get_data_dir() / "media" / "generated")

# --- 샌드박스 (스킬/에이전트 출력) ---

def get_sandbox_dir(source: str | None = None) -> Path:
    """스킬/에이전트 출력 디렉토리를 반환한다. workspace/sandbox/ 하위."""
    base = ensure_dir(get_workspace_path() / "sandbox")
    return ensure_dir(base / source) if source else base

# --- 에이전트 스토어 ---

def get_agents_dir() -> Path:
    return ensure_dir(get_data_dir() / "agents")

# --- 기존 (변경 없음) ---

def get_workspace_path(workspace: str | None = None) -> Path:
    path = Path(workspace).expanduser() if workspace else get_data_dir() / "workspace"
    return ensure_dir(path)

def get_cli_history_path() -> Path:
    return get_data_dir() / "history" / "cli_history"

def get_bridge_install_dir() -> Path:
    return get_data_dir() / "bridge"

def get_logs_dir() -> Path:
    return ensure_dir(get_data_dir() / "logs")
```

### 마이그레이션

기존 사용자의 데이터를 새 구조로 자동 이전해야 한다. `loader.py`의 기존 `_migration_config()` 패턴을 따른다.

```python
# config/loader.py에 추가

def _migrate_workspace_layout(data_dir: Path) -> None:
    """기존 워크스페이스 레이아웃을 새 구조로 마이그레이션합니다. 멱등."""
    workspace = data_dir / "workspace"
    if not workspace.exists():
        return

    moves = [
        # (이전 경로, 새 경로)
        (workspace / "memory",     data_dir / "data" / "memory"),
        (workspace / "sessions",   data_dir / "data" / "sessions"),
        (workspace / ".clawhub",   data_dir / "data" / "clawhub"),
        (workspace / "cron",       data_dir / "data" / "cron"),
        (data_dir / "cron",        data_dir / "data" / "cron"),      # 상위 cron도 통합
        (data_dir / "usage",       data_dir / "data" / "usage"),
        (data_dir / "sessions",    data_dir / "data" / "sessions"),  # 레거시 세션
        (workspace / "media",      data_dir / "media" / "generated"),
        (data_dir / "media",       data_dir / "media" / "downloads"),# 기존 media → downloads
    ]

    for src, dst in moves:
        if src.exists() and not dst.exists():
            dst.parent.mkdir(parents=True, exist_ok=True)
            src.rename(dst)
            logger.info(f"마이그레이션: {src} → {dst}")
```

**호출 시점**: `load_config()` 직후, 앱 시작 시 한 번.

### sync_workspace_template 변경

```python
# utils/helpers.py — 변경사항

def sync_workspace_template(workspace: Path, data_dir: Path | None = None, silent: bool = False):
    """번들된 템플릿을 동기화합니다."""
    tpl = pkg_files("shacs_bot") / "templates"

    # workspace에는 템플릿 .md 파일만
    for item in tpl.iterdir():
        if item.name.endswith(".md"):
            _write(item, workspace / item.name)

    (workspace / "skills").mkdir(exist_ok=True)

    # 메모리는 data/memory/로 분리
    if data_dir:
        memory_dir = data_dir / "data" / "memory"
    else:
        memory_dir = workspace / "memory"    # 폴백 (하위 호환)

    _write(tpl / "memory" / "MEMORY.md", memory_dir / "MEMORY.md")
    _write(None, memory_dir / "HISTORY.md")
```

### 모듈별 경로 변경

| 모듈 | 현재 | 변경 |
|---|---|---|
| `memory.py:44-47` | `workspace / "memory"` | `get_memory_dir()` |
| `session/manager.py` | `workspace / "sessions"` | `get_sessions_dir()` |
| `commands.py:360` | `config.workspace_path / "cron" / "jobs.json"` | `get_cron_dir() / "jobs.json"` |
| `commands.py:561` | `get_cron_dir() / "jobs.json"` | (변경 없음 — 이미 올바름) |
| `media.py:57` | `config.save_dir` (기본: `workspace/media`) | `get_media_generated_dir()` |
| `channels/*.py` | `get_media_dir(channel)` | `get_media_downloads_dir(channel)` |
| `usage.py` | `get_usage_dir()` | (내부 구현만 변경) |
| `context.py` | `workspace / "memory"` 참조 | `get_memory_dir()` |

### 스킬 출력 디렉토리 가이드라인

스킬이 파일을 생성할 때는 `sandbox/{스킬이름}/` 디렉토리를 사용해야 한다.

`sandbox/`는 workspace 하위에 있으므로, `write_file("sandbox/youtube/transcript.txt", ...)` 처럼 상대경로로 자연스럽게 동작한다.

기존 스킬의 경우 — LLM이 `write_file` 도구로 파일을 생성할 때 workspace 루트에 쓰므로, 이것은 **스킬 SKILL.md에서 가이드**하는 방식으로 해결한다:

```markdown
<!-- SKILL.md에 추가할 가이드라인 -->
## 파일 출력 규칙
파일을 생성할 때는 반드시 `sandbox/{스킬이름}/` 디렉토리에 저장하세요.
예: `sandbox/youtube/transcript_abc.txt`
```

시스템 프롬프트에도 동일한 가이드라인을 포함하여, LLM이 workspace 루트 대신 `sandbox/` 하위에 파일을 생성하도록 유도한다.

> **한계**: write_file 도구 자체를 제한하지는 않는다. LLM의 행동을 프롬프트로 유도하는 소프트 가이드라인이다.

## 구현 단계

| 단계 | 작업 | 파일 | 난이도 |
|---|---|---|---|
| 1 | paths.py 경로 헬퍼 재구성 | `config/paths.py` | 낮음 |
| 2 | 마이그레이션 로직 작성 | `config/loader.py` | 중간 |
| 3 | MemoryStore 경로 변경 | `agent/memory.py` | 낮음 |
| 4 | SessionManager 경로 변경 | `agent/session/manager.py` | 낮음 |
| 5 | 크론 경로 통합 (gateway 비일관 수정) | `cli/commands.py` | 낮음 |
| 6 | 미디어 경로 분리 | `agent/tools/media.py`, `channels/*.py` | 중간 |
| 7 | sync_workspace_template 변경 | `utils/helpers.py` | 낮음 |
| 8 | ContextBuilder 메모리 경로 변경 | `agent/context.py` | 낮음 |
| 9 | 시스템 프롬프트에 sandbox 가이드라인 추가 | `templates/TOOLS.md` 또는 `context.py` | 낮음 |

## 하위 호환성

| 항목 | 대응 |
|---|---|
| 기존 사용자 데이터 | `_migrate_workspace_layout()`이 자동으로 이전. 멱등 (이미 이전된 경우 무시) |
| 커스텀 workspace 경로 | `workspace` 설정값은 그대로 유지. data/는 항상 `~/.shacs-bot/data/` 고정 |
| write_file 도구 | workspace가 여전히 기본 cwd. 기존 동작 변경 없음 |
| 기존 스킬 | 파일 경로를 하드코딩한 스킬은 동작 유지 (마이그레이션이 처리) |

## 비변경 사항

다음은 이 스펙 범위에서 **의도적으로 변경하지 않는** 항목이다:

- `config.json` 위치 (`~/.shacs-bot/config.json`)
- `auth/` 위치
- `bridge/` 위치
- `logs/` 위치
- `history/` 위치
- `write_file` / `edit_file` 도구의 workspace 기반 경로 해석 로직
- 스킬 SKILL.md 포맷
