# wait_until Time Parsing 구현

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> 그래

---

## 작업 내용

- `shacs_bot/workflow/wait_until.py` 추가
  - `parse_wait_until_time(description: str)` 구현
  - 지원 패턴:
    - ISO datetime (`2026-04-03T14:00`, `2026-04-03 14:00:00`)
    - 상대 기간 (`30분`, `2 hours`, `3 days`)
    - `내일 HH:MM`, `tomorrow HH:MM`
    - 파싱 실패 시 5분 폴백
- `shacs_bot/agent/loop.py`
  - 기존 5분 하드코딩 `wait_until` 처리 제거
  - 실제 파싱된 시각을 `nextRunAt`으로 저장
  - 사용자 안내 메시지에 실제 재시도 시각 포함
- `scripts/smoke_wait_until.py` 추가

## 검증

- `uv run python scripts/smoke_wait_until.py`
- `uv run python -m py_compile shacs_bot/agent/loop.py shacs_bot/workflow/runtime.py shacs_bot/workflow/wait_until.py scripts/smoke_wait_until.py`
- `shacs_bot/workflow/wait_until.py` LSP diagnostics clean

## 메모

- 기존 `schedule_retry()` / `_is_retry_due()` / `recover_restart()` 경로는 그대로 재사용했다.
- 자연어 전체를 해석하는 무거운 파서는 도입하지 않고, PRD에 필요한 최소 패턴만 지원했다.
