# Planner Heuristic 확장

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> gogogogo

---

## 작업 내용

- `shacs_bot/agent/loop.py`
  - `_WAIT_UNTIL_RE` 확장
    - `지나서`, `지나고`, `지나면`, `지난 후`
    - `wait for N ...`
    - `after N ...`
  - `_ASK_USER_DETECT_RE` 확장
    - `묻고 나서`, `묻고 이후`, `묻고 ... 조사/처리/진행` 류 표현
  - `_SCHEDULE_RE` 확장
    - `every Monday` 등 요일 기반 영어 패턴
    - `weekly`, `monthly`
    - `매월`, `매달`, `매주 월요일` 류 한국어 패턴
- `scripts/smoke_planner_metadata.py`
  - 신규 heuristic 표현 검증 추가
    - `30분 지나서`
    - `wait for 20 minutes`
    - `after 1 hour`
    - `묻고 나서`
    - `매주 월요일`
    - `every Monday`
    - `monthly`

## 검증

- `uv run python scripts/smoke_planner_metadata.py`
- `uv run python scripts/smoke_e2e_planner_to_workflow.py`
- `uv run python scripts/smoke_wait_until.py`
- `uv run python scripts/smoke_request_approval.py`
- `uv run python scripts/smoke_ask_user_resume.py`
- `uv run python scripts/smoke_step_cursor.py`
- `uv run python -m py_compile shacs_bot/agent/loop.py scripts/smoke_planner_metadata.py scripts/smoke_e2e_planner_to_workflow.py`

## 메모

- `request_approval`의 `확인하고`는 기존 regex가 이미 커버하고 있어 추가 수정은 하지 않았다.
- generic schedule 확장은 planner가 더 자주 `planned_workflow`로 들어가게 만들지만, 짧은 입력은 여전히 direct answer로 남도록 기존 길이 필터를 유지했다.
