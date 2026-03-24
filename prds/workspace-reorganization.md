# PRD: Workspace 디렉토리 재구성

> **Status**: Done
> **Spec**: [docs/specs/2026-03-24-workspace-reorganization.md](../docs/specs/2026-03-24-workspace-reorganization.md)
> **Estimated Effort**: Medium (1일)
> **Created**: 2026-03-24

---

## 문제

`~/.shacs-bot/workspace/` 디렉토리에 템플릿, 런타임 데이터, 스킬 출력물, 메모리가 뒤섞여 있다. 스킬/에이전트가 늘어날수록 workspace 루트에 파일이 산재하며, `cron/`과 `media/`는 상위 디렉토리와 workspace 양쪽에 중복 존재한다.

**사용자 영향**: workspace가 어지러워져 LLM이 파일을 탐색할 때 불필요한 런타임 파일에 노출되고, 스킬 출력물이 정리되지 않아 누적된다.

## 솔루션

역할 기반 디렉토리 분리:
- **`workspace/`** — LLM이 접근하는 데이터만 유지 (템플릿, 메모리, 생성 미디어, 스킬, sandbox 출력)
- **`data/`** — 시스템 전용 런타임 데이터 분리 (세션, 크론, 사용량, ClaWHub)
- **`workspace/sandbox/`** — 스킬/에이전트 출력물을 소스별 하위 폴더로 정리

## 설계 원칙

LLM 접근 가능한 데이터는 workspace 하위에 유지한다. `restrict_to_workspace=True`일 때 LLM이 접근해야 하는 파일이 경계 밖에 놓이는 문제를 방지한다.

## 성공 기준

- [ ] 기존 사용자의 데이터가 앱 시작 시 자동으로 새 구조로 마이그레이션된다 (멱등)
- [ ] 마이그레이션 후 모든 기능이 정상 동작한다 (세션, 크론, 메모리, 미디어, 스킬)
- [ ] workspace에는 LLM 접근 가능한 파일만 남는다
- [ ] 스킬 출력물이 `sandbox/` 하위에 정리된다

## 마일스톤

### M1: 경로 인프라 구축
- [x] `config/paths.py`에 신규 경로 헬퍼 추가 (`get_data_subdir`, `get_sessions_dir`, `get_cron_dir`, `get_usage_dir`, `get_clawhub_dir`, `get_media_downloads_dir`, `get_sandbox_dir`, `get_agents_dir`)
- [x] 기존 `get_runtime_subdir("cron")`, `get_runtime_subdir("usage")` 등의 내부 구현이 `data/` 하위를 가리키도록 변경

**검증**: `get_sessions_dir()` 호출 시 `~/.shacs-bot/data/sessions/` 반환 확인

### M2: 마이그레이션 로직 및 모듈 경로 업데이트
- [x] `config/loader.py`에 `_migrate_workspace_layout()` 추가 — sessions, clawhub, cron 통합, usage, 레거시 sessions 이전
- [x] `session/manager.py`가 `get_sessions_dir()` 사용하도록 변경
- [x] `cli/commands.py` 크론 경로를 `get_cron_dir()` 통합
- [x] `channels/*.py` 다운로드 미디어 경로를 `get_media_downloads_dir(channel)` 사용하도록 변경
- [x] `load_config()` 직후 마이그레이션 함수 호출

**검증**: 기존 레이아웃 → 새 레이아웃 마이그레이션 후 세션 조회, 크론 실행, 사용량 조회 정상 동작

### M3: Sandbox 도입 및 시스템 프롬프트 가이드라인
- [x] `templates/TOOLS.md` 또는 `context.py`에 sandbox 출력 가이드라인 추가
- [x] 기존 스킬 SKILL.md에 `sandbox/{스킬이름}/` 파일 출력 규칙 안내

**검증**: LLM이 스킬 실행 시 `workspace/sandbox/{source}/` 하위에 파일 생성

### M4: 통합 검증 및 엣지 케이스
- [x] 신규 설치 (빈 상태에서 시작) 정상 동작
- [x] 기존 설치 마이그레이션 정상 동작 (멱등성 — 두 번 실행해도 안전)
- [x] 커스텀 workspace 경로 사용 시 정상 동작
- [x] CLI 모드 + Gateway 모드 모두 정상 동작

**검증**: 위 4개 시나리오 모두 에러 없이 완료

### M5: 문서 업데이트
- [x] 작업 기록 문서 작성 (`docs/` 하위)
- [x] 변경 사항 커밋

**검증**: 문서에 변경 내역, 마이그레이션 동작, 새 디렉토리 구조 설명 포함

## 영향 범위

| 파일 | 변경 내용 |
|---|---|
| `config/paths.py` | 신규 경로 헬퍼 추가, 기존 헬퍼 내부 구현 변경 |
| `config/loader.py` | `_migrate_workspace_layout()` 추가 |
| `agent/session/manager.py` | `get_sessions_dir()` 사용 |
| `cli/commands.py` | 크론 경로 `get_cron_dir()` 통합 |
| `channels/*.py` | `get_media_downloads_dir(channel)` 사용 |
| `templates/TOOLS.md` 또는 `context.py` | sandbox 가이드라인 추가 |

## 비변경 사항

- `workspace/memory/`, `workspace/media/` — LLM 접근 데이터, 위치 유지
- `config.json`, `auth/`, `bridge/`, `logs/`, `history/` — 위치 유지
- `write_file` / `edit_file` 도구의 workspace 기반 경로 해석 로직
- `sync_workspace_template()` 로직
- `memory.py`, `context.py`, `media.py` — 경로 변경 없음

## 위험 요소

| 위험 | 완화 |
|---|---|
| 마이그레이션 실패 시 데이터 손실 | `src.rename(dst)` 전에 `dst.exists()` 확인 (멱등). 파일 복사가 아닌 이동이므로 중간 상태 없음 |
| 커스텀 workspace 사용자 | `data/`는 항상 `~/.shacs-bot/data/` 고정. workspace 경로와 독립적 |
| 하드코딩된 스킬 경로 | 마이그레이션이 기존 경로를 이전하므로 기존 스킬은 동작 유지 |

## 진행 로그

| 날짜 | 상태 | 메모 |
|---|---|---|
| 2026-03-24 | 스펙 작성 완료 | 초기 스펙 → LLM 접근 원칙 반영하여 수정 |
| 2026-03-24 | PRD 생성 | 구현 준비 완료 |
| 2026-03-24 | M1~M5 구현 완료 | 경로 인프라, 마이그레이션, 모듈 업데이트, sandbox 가이드라인, 통합 검증 |
