# PRD: Operator Console (CLI/TUI)

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 shacs-bot은 운영자가 로컬 상태를 한눈에 점검할 공통 콘솔이 부족하다.

문제점:

1. 세션 목록은 일부 helper에 흩어져 있고 CLI 명령이 제한적이다.
2. approval 대기 상태는 내부 레지스트리에만 있다.
3. workflow/usage 상태를 운영자가 inspect-only로 조회할 통합 진입점이 없다.

## 해결책

웹 UI 대신 Rich 기반 admin CLI/TUI를 제공해 session, approval, workflow, usage 상태를 inspect-only 방식으로 조회한다.

## 사용자 영향

| Before | After |
|---|---|
| 운영 상태 확인을 위해 로그/파일을 직접 열어야 함 | admin 명령으로 상태를 한 번에 조회 |
| approval/workflow 가시성이 낮음 | 운영 핵심 상태를 표 형태로 확인 |
| 웹 대시보드가 없으면 운영이 불편 | 별도 인프라 없이 운영 표면 제공 |

## 기술적 범위

- **변경 파일**: 5개 수정
- **변경 유형**: CLI 조회 명령 추가
- **의존성**: 기존 `rich`
- **하위 호환성**: 기존 CLI 명령 유지

### 변경 1: admin 명령 추가 (`shacs_bot/cli/commands.py`)

- `admin sessions`
- `admin approvals`
- `admin workflows`
- `admin usage`

### 변경 2: 세션 inspect helper (`shacs_bot/agent/session/manager.py`)

- limit, metadata, key filter 지원

### 변경 3: approval/workflow 조회 helper (`shacs_bot/agent/approval.py`, `shacs_bot/workflow/store.py`)

- pending approval 목록 반환
- workflow list/inspect helper 추가

### 변경 4: usage summary helper 보강 (`shacs_bot/agent/usage.py`)

- CLI-friendly aggregate shape
- session/date filter 지원

## 성공 기준

1. 운영자는 CLI만으로 session/approval/workflow/usage 상태를 볼 수 있다.
2. 비어 있는 상태에서도 명령이 안전하게 동작한다.
3. inspect-only 단계에서 파괴적 조작이 포함되지 않는다.
4. 별도 웹 인프라 없이 기본 운영 가시성을 확보한다.

---

## 마일스톤

- [ ] **M1: admin command skeleton 추가**
  CLI 진입점과 빈 상태 출력 정의.

- [ ] **M2: session/approval/workflow/usage helper 연결**
  조회용 helper와 table 출력 연결.

- [ ] **M3: 필터 옵션 및 empty-state 검증**
  limit/date/session/state 옵션 검증.

- [ ] **M4: inspect-only 운영 검증**
  gateway 미실행/데이터 없음/부분 데이터 환경에서 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| CLI가 너무 많은 상태를 한 번에 노출 | 중간 | 낮음 | 명령을 영역별로 분리 |
| empty-state에서 출력 오류 | 중간 | 중간 | 모든 명령에 빈 상태 처리 추가 |
| inspect-only 범위를 넘는 파괴 기능 유입 | 낮음 | 높음 | 1단계 acceptance criteria에 명시적으로 금지 |

## Acceptance Criteria

- [ ] admin sessions/approvals/workflows/usage 명령이 동작한다.
- [ ] gateway 미실행 상태에서도 조회 가능하다.
- [ ] 빈 상태에서도 오류 없이 결과를 출력한다.
- [ ] inspect-only 범위를 유지한다.
