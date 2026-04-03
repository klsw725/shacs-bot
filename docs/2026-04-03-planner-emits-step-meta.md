# 플래너 step_meta 방출 구현

**날짜:** 2026-04-03  
**브랜치:** feature/planned-workflow-executor

---

## 사용자 프롬프트

> Fix the remaining gap in the structured step metadata work.
>
> Problem:
> - `PlanStep.step_meta` exists and executor consumes it.
> - But the current rule-based planner in `shacs_bot/agent/loop.py::_classify_request()` still only emits generic `research/summarize/send_result` steps and does not populate structured metadata for the targeted waiting-step cases.
>
> TASK: Implement the smallest safe follow-up so the planner actually emits structured step metadata for planner-created waiting-step cases, and add the missing docs work log.
>
> EXPECTED OUTCOME:
> - `_classify_request()` recognizes a minimal set of cases that produce planner steps with metadata for at least:
>   1. `wait_until` (populate `step_meta` with `iso_time` or `duration_minutes` when derivable)
>   2. `ask_user` and/or `request_approval` when rule-based heuristics choose those step kinds
> - Existing fallback behavior remains intact for older/generic cases.
> - Add or update deterministic smoke verification so planner output itself is asserted, not just the model round-trip.

---

## 변경 사항

### `shacs_bot/agent/loop.py::_classify_request()`

기존 `is_compound` 복합 검출 분기 **이전**에 세 가지 특수 step kind 감지 heuristic 추가:

| 분기 | 패턴 예시 | 생성되는 플랜 |
|---|---|---|
| `_WAIT_UNTIL_RE` | `30분 후`, `2시간 뒤`, `내일 09:00`, `wait 45 minutes` | `wait_until`(iso_time) → `research` → `send_result` |
| `_APPROVAL_DETECT_RE` | `확인 후에`, `confirm before`, `get approval` | `research` → `request_approval`(prompt) → `send_result` |
| `_ASK_USER_DETECT_RE` | `물어보고`, `ask me`, `get input` | `ask_user`(prompt) → `research` → `send_result` |

**우선순위**: wait_until > request_approval > ask_user > 기존 compound 감지 > direct_answer

**`wait_until` 메타데이터**: `parse_wait_until_time(text)` 를 호출해 `iso_time` 을 계획 수립 시점에 확정. executor는 이를 description 파싱 없이 바로 소비.

**`request_approval` 메타데이터**: 사용자 원문 120자를 `prompt` 에 포함.

**`ask_user` 메타데이터**: 일반 안내 문구를 `prompt` 로 설정.

기존 generic compound/direct_answer/clarification 경로는 변경 없음.

### `scripts/smoke_planner_metadata.py` (신규)

`AgentLoop._classify_request()` 출력을 직접 검증하는 12개 검증:

| 번호 | 내용 |
|---|---|
| 1–4 | wait_until 감지 (30분, 2시간, wait N, 내일 HH:MM) + iso_time 값 |
| 5–6 | request_approval 감지 (한/영) + prompt 포함 |
| 7–8 | ask_user 감지 (한/영) + prompt 포함 |
| 9–11 | 회귀: generic compound / 짧은 텍스트 / clarification |
| 12 | wait_until 플랜 step 순서 검증 |

---

## 설계 결정

- **인라인 regex, `@staticmethod` 내부**: 기존 패턴과 동일하게 메서드 로컬 변수로 정의. 모듈 상수로 승격하지 않음 (기존 코드 스타일 유지).
- **`iso_time` 단일 키**: 플래너 감지 시점에 절대 시각으로 확정. executor가 `duration_minutes` 를 계산할 필요 없음.
- **wait_until 감지 패턴**: `N분/시간/일 후/뒤/있다가`, `내일 HH:MM`, `wait N min/hours`, `in N min/hours`, ISO datetime 으로 제한. "30분 동안 편집" 같은 기간-지속 표현과 혼동 방지.
