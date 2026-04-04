# PRD: Personal Inspect CLI

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 shacs-bot은 self-hosted 사용자가 로컬 상태를 한눈에 점검할 공통 inspect 표면이 부족하다.

문제점:

1. 세션 목록은 일부 helper에 흩어져 있고 CLI 명령이 제한적이다.
2. approval 대기 상태는 내부 레지스트리에만 있어 조회 성격이 불분명하다.
3. workflow/usage 상태를 사용자가 read-only로 조회할 통합 진입점이 없다.

## 해결책

웹 UI 대신 Rich 기반 `inspect` CLI를 제공해 session, approval, workflow, usage 상태를 read-only 방식으로 조회한다.

별도 운영자 조직을 전제하지 않고, 제품의 기본 주체인 사용자가 자신의 bot 상태를 점검하는 흐름에 맞춘다.

## 사용자 영향

| Before | After |
|---|---|
| 상태 확인을 위해 로그/파일을 직접 열어야 함 | inspect 명령으로 상태를 한 번에 조회 |
| approval/workflow 가시성이 낮음 | 운영 핵심 상태를 표 형태로 확인 |
| 웹 대시보드가 없으면 상태 점검이 불편 | 별도 인프라 없이 점검 표면 제공 |

## 기술적 범위

- **변경 파일**: 5개 수정
- **변경 유형**: CLI 조회 명령 추가
- **의존성**: 기존 `rich`
- **하위 호환성**: 기존 CLI 명령 유지

### 변경 1: inspect 명령 추가 (`shacs_bot/cli/commands.py`)

- `inspect sessions`
- `inspect approvals`
- `inspect workflows`
- `inspect usage`

### 변경 2: 세션 inspect helper (`shacs_bot/agent/session/manager.py`)

- limit, metadata, key filter 지원
- message count 포함

### 변경 3: approval/workflow 조회 helper (`shacs_bot/agent/approval.py`, `shacs_bot/workflow/store.py`)

- pending approval 목록 반환
- approval은 M1에서 process-local 상태만 다룸
- workflow list/inspect helper 또는 CLI 필터링 추가

### 변경 4: usage summary helper 보강 (`shacs_bot/agent/usage.py`)

- CLI-friendly aggregate shape
- session/date filter 지원

## 성공 기준

1. 사용자는 CLI만으로 session/approval/workflow/usage 상태를 볼 수 있다.
2. 비어 있는 상태에서도 명령이 안전하게 동작한다.
3. read-only 단계에서 파괴적 조작이 포함되지 않는다.
4. 별도 웹 인프라 없이 기본 상태 가시성을 확보한다.
5. 별도 운영자 역할을 기본 가정하지 않는다.

---

## 마일스톤

- [x] **M1: inspect command skeleton 추가**
  CLI 진입점과 빈 상태 출력 정의.

- [x] **M2: sessions/workflows/usage helper 연결**
  조회용 helper와 table 출력 연결.

- [x] **M3: approvals local-only 명시 + 필터 옵션 및 empty-state 검증**
  approvals 조회 한계와 limit/date/session/state 옵션 검증.

- [x] **M4: read-only inspect UX 검증**
  gateway 미실행/데이터 없음/부분 데이터 환경에서 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| CLI가 너무 많은 상태를 한 번에 노출 | 중간 | 낮음 | 명령을 영역별로 분리 |
| empty-state에서 출력 오류 | 중간 | 중간 | 모든 명령에 빈 상태 처리 추가 |
| read-only 범위를 넘는 파괴 기능 유입 | 낮음 | 높음 | 1단계 acceptance criteria에 명시적으로 금지 |
| approval이 프로세스 외부에서 보이지 않음 | 높음 | 중간 | M1 범위를 process-local inspect로 명시 |

## Acceptance Criteria

- [x] inspect sessions/approvals/workflows/usage 명령이 동작한다.
- [x] session/workflow/usage는 gateway 미실행 상태에서도 조회 가능하다.
- [x] approvals는 현재 프로세스 기준 pending 상태를 조회할 수 있다.
- [x] 빈 상태에서도 오류 없이 결과를 출력한다.
- [x] read-only 범위를 유지한다.
- [x] 관리자 전용 콘솔이 아니라 self-hosted 사용자용 inspect UX로 설명된다.

## 진행 로그

| 날짜 | 상태 | 메모 |
|---|---|---|
| 2026-04-04 | 구현 | `shacs_bot/cli/commands.py`에 `inspect sessions`, `inspect workflows`, `inspect usage`, `inspect approvals`를 추가하고 기존 `workflows status` 테이블 렌더링을 재사용했다. |
| 2026-04-04 | 구현 | `SessionManager.list_sessions()`, `list_pending_approvals()`, `WorkflowStore.list_incomplete()/list_all()`, `UsageTracker.get_session_summary()/get_daily_summary()`를 inspect 경로에 연결했다. |
| 2026-04-04 | 검증 | `tests/test_inspect_cli.py`로 sessions/workflows/usage/approvals/status empty-state와 필터 시나리오를 검증했다. |
| 2026-04-04 | polish | `inspect workflows --source`, `inspect usage --session` cross-day 집계, `status` personal inspect summary를 추가해 UX를 정리했다. |
| 2026-04-04 | 동기화 | 구현과 테스트가 완료된 상태에 맞춰 milestone 및 acceptance 체크박스를 최신화했다. |
