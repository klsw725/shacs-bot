# PlanStep 구조화 메타데이터 필드 추가

**날짜:** 2026-04-03  
**브랜치:** feature/planned-workflow-executor

---

## 사용자 프롬프트

> 1. TASK: Implement the smallest safe follow-up so planner-emitted `PlanStep`s can carry structured metadata that the executor uses preferentially over free-form `description` parsing.
> 2. EXPECTED OUTCOME: A surgical change where `PlanStep` gains a structured metadata field, planner-created steps populate it for at least `ask_user`, `request_approval`, and `wait_until`, and executor/runtime code prefers that metadata while preserving description-based fallback. Include deterministic verification for both planner output and executor consumption, plus the required dated docs work-log file under `docs/` with the user's prompt text.
> 3. REQUIRED TOOLS: read, grep, apply_patch, lsp_diagnostics, bash.
> 4. MUST DO: Reuse existing Pydantic model and workflow serialization patterns. Keep changes minimal and backward-compatible: existing plans without structured metadata must still run. For `wait_until`, prefer explicit metadata fields (for example absolute ISO time and/or duration minutes) over parsing description text, with current parser retained as fallback. For `ask_user` and `request_approval`, add minimally useful prompt metadata so executor/user-facing messages can read from metadata first and description second. Update the smallest existing planner scenario verification asset or smoke script so planner output containing structured metadata is asserted. Add/update the smallest executor-facing smoke verification showing metadata-first wait_until consumption.
> 5. MUST NOT DO: Do not refactor unrelated planner heuristics. Do not remove description fallback. Do not add external dependencies. Do not commit. Do not introduce broad abstractions or a generic schema system beyond what PlanStep needs now.

---

## 변경 사항

### `shacs_bot/agent/planner.py`

`PlanStep`에 `step_meta: dict[str, object]` 필드 추가 (기본값 `{}`).

**규약화된 키:**
- `wait_until`: `iso_time` (str, ISO 8601), `duration_minutes` (int | float)
- `ask_user` / `request_approval`: `prompt` (str)

기존 플랜(`step_meta` 없음) → Pydantic 기본값 `{}` 적용으로 하위 호환 유지.

### `shacs_bot/agent/loop.py`

세 executor 블록 수정 (최소 변경):

| 블록 | 변경 내용 |
|---|---|
| `wait_until` | `iso_time` → `duration_minutes` → description 파서 순서로 우선 소비 |
| `ask_user` | `step_meta["prompt"]` 존재 시 description 대신 사용 |
| `request_approval` | `step_meta["prompt"]` 존재 시 description 대신 사용 |

### `scripts/smoke_wait_until.py`

검증 11–16 추가:

| 번호 | 내용 |
|---|---|
| 11 | `PlanStep` `step_meta` 직렬화/역직렬화 라운드트립 |
| 12 | `step_meta` 없는 구버전 스텝 → `step_meta={}` 하위 호환 |
| 13 | `AssistantPlan` 내 `PlanStep` `step_meta` 파싱 (세 step kind 모두) |
| 14 | executor: `iso_time` 메타데이터 우선 소비 |
| 15 | executor: `duration_minutes` 메타데이터 우선 소비 |
| 16 | executor: `step_meta` 없음 → description fallback |

---

## 설계 결정

- **`step_meta` 단일 dict 필드**: `wait_until_iso`, `wait_until_minutes` 등의 전용 필드 대신 단일 dict 채택. 각 kind별 키 네임스페이스가 겹칠 일이 없고, 미래 확장 시 스키마 변경 없이 키만 추가 가능.
- **executor 인라인 우선순위 로직**: 별도 helper 함수 없이 각 블록에 직접 작성. 블록별 우선순위가 달라지는 경우를 위해 공유 추상화 미도입.
- **description fallback 보존**: 이전에 저장된 플랜(메타데이터 없음)은 기존 `parse_wait_until_time` 경로로 계속 동작.
