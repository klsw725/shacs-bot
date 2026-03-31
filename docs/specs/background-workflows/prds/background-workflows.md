# PRD: Lightweight Background Workflows

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 shacs-bot에는 cron, heartbeat, subagent background task가 각각 존재하지만, 장기 assistant 작업을 공통 상태로 추적하는 구조가 없다.

문제점:

1. 진행 중인 assistant 작업을 상태 단위로 조회/재개하기 어렵다.
2. 프로세스 재시작 시 장기 작업 문맥을 잃는다.
3. cron/heartbeat/subagent가 각자 다른 방식으로 background 일을 표현한다.

## 해결책

file-backed workflow store와 runtime을 추가해, background 작업을 queued/running/waiting/retry/completed/failed 상태로 저장하고 재개 가능하게 만든다.

## 사용자 영향

| Before | After |
|---|---|
| 오래 걸리는 assistant 작업을 공통 상태로 추적하기 어려움 | 진행 중/대기/실패/완료 상태를 일관되게 조회 |
| 재시작 후 background 문맥 손실 | incomplete workflow 복구 |
| cron/heartbeat/subagent가 분리된 개념 | 공통 workflow runtime으로 수렴 |

## 기술적 범위

- **변경 파일**: 4개 수정 + 2개 신규
- **변경 유형**: 상태 저장소 + runtime 추가
- **의존성**: 없음
- **하위 호환성**: 기존 cron/heartbeat/subagent API 유지

### 변경 1: workflow state store (`shacs_bot/workflow/store.py`)

- workflow record 저장/로드
- 상태 필드: state, goal, retries, next_run_at, notify_target
- incomplete workflow 조회 helper

### 변경 2: workflow runtime (`shacs_bot/workflow/runtime.py`)

- 상태 전이 구현
- retry_wait, waiting_input, completed, failed 처리
- resume entrypoint 제공

### 변경 3: heartbeat / cron 연동 (`shacs_bot/heartbeat/service.py`, `shacs_bot/agent/tools/cron/service.py`)

- heartbeat가 찾은 작업을 workflow record로 실행
- cron 실행 결과를 workflow runtime으로 연결

### 변경 4: subagent 결과 연동 (`shacs_bot/agent/subagent.py`)

- background task 완료를 workflow completion으로 반영
- 필요 시 notify target으로 결과 전달

## 성공 기준

1. background assistant 작업이 공통 workflow 상태로 관리된다.
2. 재시작 후 incomplete workflow를 복구할 수 있다.
3. cron/heartbeat/subagent 기반 작업이 같은 runtime으로 연결된다.
4. 완료/실패 결과를 기존 outbound 경로로 알릴 수 있다.

---

## 마일스톤

- [ ] **M1: workflow store 추가**
  상태 persistence와 incomplete 조회 helper 구현.

- [ ] **M2: runtime 상태기계 추가**
  queued/running/waiting/retry/completed/failed 전이 구현.

- [ ] **M3: heartbeat/cron/subagent 연결**
  기존 background 기능을 runtime에 연결.

- [ ] **M4: 재시작/retry/notify 검증**
  restart recovery와 notify 흐름 검증.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| workflow와 planner 책임 경계 혼동 | 중간 | 중간 | planner는 계획, runtime은 상태 전이로 분리 |
| 파일 기반 저장 손상 | 낮음 | 중간 | atomic write와 corrupt fallback 적용 |
| 기존 cron/heartbeat 흐름 회귀 | 중간 | 높음 | 하위 호환 경로 유지 + 회귀 검증 |

## Acceptance Criteria

- [ ] background 작업 상태가 persisted 된다.
- [ ] 재시작 후 incomplete workflow를 다시 읽을 수 있다.
- [ ] cron/heartbeat/subagent가 공통 workflow runtime을 사용한다.
- [ ] 완료 결과를 outbound notify로 전달할 수 있다.
