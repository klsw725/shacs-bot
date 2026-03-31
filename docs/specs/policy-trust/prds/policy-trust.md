# PRD: Policy / Approval / Trust Model

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 approval 체계는 workspace skill 중심으로 설계되어 있어 assistant 제품 전반의 신뢰/비용/행위 제어에는 한계가 있다.

문제점:

1. shared channel, 신규 사용자, trusted DM 같은 차이를 승인 판단에 반영하지 못한다.
2. tool 실행 외 planner/workflow 단계는 동일 정책을 재사용하기 어렵다.
3. usage 기록은 있으나 quota enforcement는 없다.

## 해결책

actor/channel/action-aware policy evaluator를 추가해 approval, usage, planner/workflow 경로를 하나의 정책 계층으로 묶는다.

## 사용자 영향

| Before | After |
|---|---|
| 모든 승인 판단이 workspace/도구 중심 | 사용자·채널·행위·비용 상태를 함께 고려 |
| trusted 환경에서도 승인 마찰이 큼 | 저위험/고신뢰 동작은 더 빠르게 통과 |
| 비용 초과 시 일관된 제어 없음 | degrade 또는 차단이 예측 가능하게 적용 |

## 기술적 범위

- **변경 파일**: 4개 수정 + 1개 신규
- **변경 유형**: 정책 엔진 추가 + approval 연동 확장
- **의존성**: 없음
- **하위 호환성**: 기존 auto/manual/off 모드 유지

### 변경 1: PolicyEvaluator 추가 (`shacs_bot/agent/policy.py`)

- actor context, action context, quota state 입력
- allow / deny / escalate / degrade 결과 반환

### 변경 2: ApprovalGate 통합 (`shacs_bot/agent/approval.py`)

- 기존 Tier 1 규칙 기반 판단 전후로 policy 연동
- 수동 승인/자동 승인 흐름과 충돌하지 않게 결합

### 변경 3: usage 기반 quota 입력 (`shacs_bot/agent/usage.py`)

- session/day summary helper 확장
- 고비용 모델 사용 여부 판단 입력 제공

### 변경 4: 실행 경로 연결 (`shacs_bot/agent/loop.py`, `config/schema.py`)

- tool/planner/workflow 실행 전 동일 evaluator 재사용
- trusted users/channels, daily cost limit, high-risk tools 설정 추가

## 성공 기준

1. 동일 요청이라도 actor/channel 차이에 따라 정책 결과가 달라진다.
2. trusted DM에서는 불필요한 승인 마찰이 줄어든다.
3. quota 초과 상태에서 고비용 요청은 일관되게 강등 또는 차단된다.
4. planner/workflow 단계도 같은 정책 엔진을 사용한다.

---

## 마일스톤

- [ ] **M1: policy evaluator와 config 모델 추가**
  `agent/policy.py`, `config/schema.py`에 정책 입력/설정 정의.

- [ ] **M2: ApprovalGate와 usage 통합**
  `agent/approval.py`, `agent/usage.py` 연동.

- [ ] **M3: planner/workflow 경로 확장**
  `agent/loop.py`에서 tool 외 실행 경로에 정책 재사용.

- [ ] **M4: actor/channel/quota 시나리오 검증**
  trusted/low-trust/quota-exceeded 흐름 검증.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 정책이 과도해 정상 작업을 막음 | 중간 | 높음 | allow/deny/escalate/degrade를 단계적으로 적용 |
| usage 연동이 기존 기록 경로를 깨뜨림 | 낮음 | 높음 | 기록과 enforcement를 분리 |
| 채널별 규칙이 복잡해짐 | 중간 | 중간 | actor/channel/action 입력 모델을 명시적으로 고정 |

## Acceptance Criteria

- [ ] trusted user/channel 규칙이 승인 결정에 반영된다.
- [ ] quota 초과 상태에서 일관된 강등/차단이 동작한다.
- [ ] 기존 auto/manual/off 모드가 유지된다.
- [ ] planner/workflow 경로에도 동일 정책 엔진이 적용된다.
