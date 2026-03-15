# PRD: 역할 기반 서브에이전트 시스템

> **SPEC**: [`docs/specs/2026-03-15-01-30-sisyphus-orchestration-spec.md`](../specs/2026-03-15-01-30-sisyphus-orchestration-spec.md) — 변경 2, 변경 3
> **관련 PRD**: [`prds/orchestration-prompt.md`](./orchestration-prompt.md) — 시스템 프롬프트에서 서브에이전트 역할을 안내하므로 함께 적용 시 효과 극대화

---

## 문제

현재 `SubagentManager`는 모든 서브에이전트에 **동일한 프롬프트와 동일한 도구 세트**를 제공한다:

```python
# 현재 프롬프트 (전체)
"당신은 메인 에이전트에 의해 특정 작업을 수행하기 위해 생성된 서브에이전트입니다.
 할당된 작업에 집중하세요."
```

**문제점**:
1. 웹 검색만 하면 되는 조사 작업에도 `write_file`, `edit_file` 도구가 노출됨 → 불필요한 파일 생성 위험
2. 모든 서브에이전트가 동일한 범용 프롬프트 → 역할에 맞는 행동 유도 불가
3. `spawn` 도구에 역할 지정 파라미터 없음 → 메인 에이전트가 용도에 맞는 서브에이전트를 선택할 수 없음

## 해결책

oh-my-opencode의 에이전트 역할 분화 패턴을 적용한다:

1. **SubagentRole 정의** — 역할별 프롬프트 + 허용 도구 목록 + 반복 횟수 제한
2. **3개 역할** — researcher(정보 수집), analyst(분석/요약), executor(작업 실행)
3. **spawn 도구에 role 파라미터 추가** — 메인 에이전트가 용도에 맞는 역할을 선택

**역할별 도구 제한**:

| 역할 | 허용 도구 | 금지 도구 | max_iterations |
|---|---|---|---|
| researcher | read_file, list_dir, exec, web_search, web_fetch | write_file, edit_file | 10 |
| analyst | read_file, list_dir, exec, web_search, web_fetch | write_file, edit_file | 10 |
| executor | 전체 | 없음 | 15 |

## 사용자 영향

| Before | After |
|---|---|
| 조사 서브에이전트가 파일을 임의 생성할 수 있음 | researcher는 읽기 전용 — 조사 결과만 보고 |
| 모든 서브에이전트가 동일한 "작업에 집중하세요" 프롬프트 | 역할별 전문 프롬프트 (출처 명시, 구조적 보고 등) |
| 메인 에이전트가 서브에이전트 용도를 구분할 수 없음 | `role="researcher"` 로 명시적 선택 |
| `spawn(task="...")` → 범용 실행 | `spawn(task="...", role="researcher")` → 전문 실행 |

## 기술적 범위

- **변경 파일**: `shacs_bot/agent/subagent.py`, `shacs_bot/agent/tools/spawn.py` (2개)
- **변경 유형**: Python 코드 추가/수정
- **의존성**: 없음. 기존 패키지만 사용.
- **하위 호환성**: `role` 미지정 시 기본값 `"executor"` → 기존 동작 100% 유지

### 변경할 코드 요약

**subagent.py**:
- `SubagentRole` dataclass 추가 (system_prompt, allowed_tools, max_iterations)
- `RESEARCHER_PROMPT`, `ANALYST_PROMPT`, `EXECUTOR_PROMPT` 상수 추가
- `SUBAGENT_ROLES` 딕셔너리 추가
- `spawn()` 에 `role` 파라미터 추가
- `_run_subagent()` 에서 역할에 따라 도구 필터링, 프롬프트 선택, 반복 횟수 조절
- `_build_subagent_prompt()` 시그니처 변경 → `SubagentRole` 받도록

**spawn.py**:
- `parameters`에 `role` 필드 추가 (enum: researcher, analyst, executor)
- `execute()`에 `role` 파라미터 추가 → `SubagentManager.spawn()`에 전달

## 성공 기준

1. `spawn(task="...")` (role 없이) 호출 → 기존과 동일하게 executor로 동작 (하위 호환)
2. `spawn(task="...", role="researcher")` → write_file, edit_file 도구 없이 실행
3. researcher 서브에이전트가 파일 생성/수정을 시도할 수 없음 (도구 자체가 미등록)
4. 각 역할의 프롬프트가 적용됨 — researcher는 출처 명시, analyst는 구조적 분석 보고
5. 기존 서브에이전트 기능 (결과 announce, 세션별 취소 등) 정상 동작

---

## 마일스톤

- [x] **M1: SubagentRole 구조 및 역할 프롬프트 구현**
  `SubagentRole` dataclass, 3개 역할 프롬프트(RESEARCHER/ANALYST/EXECUTOR), `SUBAGENT_ROLES` 딕셔너리를 `subagent.py`에 추가. 기존 코드 동작에 영향 없음 (아직 사용되지 않는 코드).

- [x] **M2: spawn/run 메서드에 역할 시스템 통합**
  `spawn()` 에 `role` 파라미터 추가, `_run_subagent()` 에서 역할 기반 도구 필터링 및 프롬프트 선택 적용, `spawn.py`에 `role` 파라미터 추가. 기본값 `"executor"` 로 하위 호환성 유지.

- [x] **M3: 역할별 동작 검증**
  researcher 역할로 spawn 시 write/edit 도구 미등록 확인. executor 역할로 spawn 시 전체 도구 등록 확인. 기존 spawn(role 없이) 호출이 깨지지 않는지 확인.

- [x] **M4: 실사용 시나리오 검증 및 프롬프트 튜닝**
  실제 채팅 환경에서 메인 에이전트가 적절한 역할을 선택하는지 확인. 역할별 프롬프트 품질 미세 조정 (보고 형식, 행동 규칙 등).

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 기존 spawn 호출이 깨짐 | 낮음 | 높음 | `role` 기본값 `"executor"`, 코드 경로 분기 최소화 |
| 역할에 필요한 도구가 누락됨 | 중간 | 중간 | MCP 동적 도구는 별도 처리 필요 → 초기 버전에서는 정적 도구만 필터링 |
| 프롬프트가 너무 제한적이어서 서브에이전트 성능 저하 | 중간 | 중간 | M4에서 실사용 피드백 기반 튜닝 |
| exec 도구가 researcher/analyst에 허용되어 간접 파일 수정 가능 | 낮음 | 낮음 | exec 도구의 기존 안전 장치(위험 명령 차단)로 충분. 필요시 추후 제한 |

---

## 종속성

- **MCP 동적 도구**: 현재 설계는 정적 등록 도구(read_file, write_file 등)만 필터링. MCP 서버를 통해 동적 등록되는 도구는 이 역할 시스템에 포함되지 않음. 추후 확장 가능.
- **오케스트레이션 프롬프트 PRD**: SOUL.md에 역할 가이드가 있어야 메인 에이전트가 적절한 역할을 선택함. 이 PRD 단독으로도 동작하지만, 프롬프트 PRD와 함께 적용해야 효과 극대화.

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-15 | PRD 초안 작성. SPEC에서 변경 2, 변경 3 통합. |
| 2026-03-15 | M1+M2 완료. SubagentRole dataclass, 3개 역할 프롬프트, SUBAGENT_ROLES 딕셔너리 추가. spawn()/\_run_subagent()/\_build_subagent_prompt()에 role 파라미터 통합. SpawnTool에 role 파라미터 추가. |
| 2026-03-15 | M3 완료. 코드 리뷰 기반 검증: (1) researcher/analyst의 allowed_tools에 write_file/edit_file 미포함 → 필터링 정상, (2) executor의 allowed_tools=[] → 빈 리스트 falsy로 전체 도구 등록, (3) spawn/\_run_subagent/SpawnTool.execute 모두 role="executor" 기본값 → 하위 호환 유지. MCP 동적 도구는 서브에이전트에 미포함 (설계 의도). |
| 2026-03-15 | M4 완료. spawn tool의 role description 강화 — "작업 목적에 맞는 역할을 반드시 지정하세요" 원칙 추가. CLI 검증: "AGENTS.md 분석해서 요약해줘"→analyst✅, "최신 AI 뉴스 웹에서 검색해줘"→researcher✅, "모든 .txt 파일 찾아서 summary.md 정리"→executor✅. 모델이 이미 아는 내용(2025년 트렌드)은 직접 처리 — 합리적 판단. |
