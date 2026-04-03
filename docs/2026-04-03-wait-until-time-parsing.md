# wait_until 실시간 스케줄링 구현

**날짜:** 2026-04-03  
**브랜치:** feature/planned-workflow-executor

---

## 사용자 프롬프트

> 1. TASK: Replace the current fixed 5-minute `wait_until` retry with the smallest codebase-consistent implementation of real time-based waiting.
> 2. EXPECTED OUTCOME: A surgical change where `wait_until` computes a meaningful `nextRunAt` from the step description and/or plan metadata, stores it through the existing retry_wait path, and resumes correctly when due. Include deterministic smoke verification for wait-until scheduling and a dated docs work-log file under `docs/` with the user's prompt text.
> 3. REQUIRED TOOLS: read, grep, apply_patch, lsp_diagnostics, bash.
> 4. MUST DO: Reuse existing ISO datetime and retry/recover patterns already present in `shacs_bot/workflow/runtime.py`, `shacs_bot/workflow/models.py`, and cron-related code where appropriate. Keep the implementation minimal: support explicit ISO datetimes and a small set of relative-duration patterns first (for example minutes/hours/days and simple 'tomorrow HH:MM'). Keep timezone behavior aligned with `datetime.now().astimezone()` conventions. Preserve current `retry_wait` and redispatch behavior. Add at least one deterministic smoke verification covering: parsed future time -> `retry_wait` with correct `nextRunAt`; due retry -> resume from next step; restart/reload preserves the scheduled timestamp.
> 5. MUST NOT DO: Do not add external dependencies. Do not build a natural-language parser. Do not refactor unrelated workflow code. Do not commit.

---

## 변경 사항

### `shacs_bot/workflow/wait_until.py` (신규)

`parse_wait_until_time(description: str) -> datetime` 순수 함수.

지원 패턴 (우선순위 순):
1. **ISO 8601** — `2026-04-03T14:00`, `2026-04-03 14:00:00` 등. timezone 없으면 로컬 timezone 적용.
2. **상대 기간** — `N분`, `N minutes`, `N시간`, `N hours`, `N일`, `N days`
3. **내일/tomorrow HH:MM** — `내일 09:30`, `tomorrow 14:00`
4. **폴백** — 파싱 실패 시 5분 후

외부 의존성 없음. `re`, `datetime`, `timedelta` 표준 라이브러리만 사용.

### `shacs_bot/agent/loop.py`

- `from shacs_bot.workflow.wait_until import parse_wait_until_time` import 추가
- `wait_until` 블록: 하드코딩된 `timedelta(minutes=5)` → `parse_wait_until_time(step.description)` 호출
- 메시지에 실제 재시도 시각(`%Y-%m-%d %H:%M %Z`) 포함

### `scripts/smoke_wait_until.py` (신규)

결정적 스모크 테스트 11개:
- 검증 1: ISO datetime 파싱
- 검증 2: `30분` 상대 기간
- 검증 3: `2 hours` 상대 기간
- 검증 4/4b: `내일 09:30` / `tomorrow 14:00`
- 검증 5: 폴백 5분
- 검증 6: `schedule_retry` → `retry_wait` + `next_run_at` 저장
- 검증 7: 미래 `next_run_at` → `_is_retry_due = False`
- 검증 8: 과거 `next_run_at` → `_is_retry_due = True`
- 검증 9: 재시작/재로드 후 `next_run_at` 보존
- 검증 10: `recover_restart` → 만료 `retry_wait` → `queued` 복구

---

## 설계 결정

- **파서를 별도 모듈(`wait_until.py`)로 분리**: 스모크 테스트에서 `loop.py`의 무거운 의존성 없이 직접 import 가능.
- **기존 `schedule_retry` / `_is_retry_due` / `recover_restart` 흐름 그대로 유지**: 새로운 상태 전환 경로 없음.
- **자연어 파서 미도입**: regex 기반 제한적 패턴만 지원. 모호한 표현은 5분 폴백.
