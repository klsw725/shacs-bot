# SPEC: Policy / Approval / Trust Model

> **Prompt**: workspace-level approval gate를 assistant 제품 수준의 user/channel/action-aware policy 및 trust model로 확장한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`policy-trust.md`](./prds/policy-trust.md) | actor/channel/action 정책, quota 규칙, approval 통합을 구현 태스크로 분해 |

## TL;DR

> **목적**: 멀티채널 assistant에서 도구 실행, planner/workflow 단계, 비용이 큰 동작을 사용자/채널/행위 단위로 통제한다.
>
> **Deliverables**:
> - `shacs_bot/agent/policy.py` — policy evaluator
> - `shacs_bot/agent/approval.py` — approval와 policy 결합
> - `shacs_bot/agent/usage.py` — quota 판단 입력 보강
> - `shacs_bot/config/schema.py` — trust/policy 설정 추가
> - `docs/specs/policy-trust/checklists/requirements.md` — 스펙 품질 체크리스트
>
> **Estimated Effort**: Medium (4-6시간)

## User Scenarios & Testing

### Scenario 1 - 저신뢰 환경에서는 고위험 동작이 추가 승인된다

shared channel이나 신규 사용자처럼 신뢰가 낮은 상황에서는 같은 요청이라도 더 엄격한 검사가 필요하다.

**테스트**: low-trust actor의 위험 작업 요청이 manual approval 또는 거부로 분기되는지 확인한다.

### Scenario 2 - 신뢰된 개인 채널에서는 불필요한 승인 마찰이 줄어든다

이미 신뢰된 DM 환경에서는 읽기/조회 위주의 작업이 빠르게 처리되어야 한다.

**테스트**: trusted actor의 low-risk 동작이 자동 승인되는지 확인한다.

### Scenario 3 - 비용 제한을 넘는 동작이 예측 가능하게 처리된다

assistant 운영자는 과도한 비용 사용 시 degrade 또는 차단이 일관되게 동작하길 원한다.

**테스트**: quota 초과 상태에서 고비용 작업이 fallback 또는 거부되는지 확인한다.

## Functional Requirements

- **FR-001**: 시스템은 사용자, 채널, 행위, 비용 상태를 기준으로 정책 결정을 내려야 한다.
- **FR-002**: 정책 결정은 tool 실행뿐 아니라 planner와 workflow 단계에도 적용되어야 한다.
- **FR-003**: 신뢰 수준이 높은 actor는 저위험 작업에서 승인 마찰이 줄어들어야 한다.
- **FR-004**: 비용 제한을 초과한 상태에서는 고비용 동작이 차단되거나 더 낮은 비용 경로로 강등되어야 한다.
- **FR-005**: 기존 auto/manual/off approval 모드는 하위 호환되어야 한다.

## Key Entities

- **Actor Context**: sender, session, channel, trust 정보를 묶은 정책 입력
- **Policy Decision**: allow, deny, escalate, degrade 중 하나의 판단 결과
- **Quota State**: session/day 수준의 비용 및 호출량 상태

## Success Criteria

- 동일 요청이라도 actor/channel 차이에 따라 정책 결과가 구분된다.
- 기존 skill approval 사용 흐름이 깨지지 않는다.
- quota 초과 시 일관된 강등 또는 차단이 적용된다.
- planner/workflow 경로에도 동일 정책 규칙이 적용된다.

## Assumptions

- 1단계는 로컬 config와 runtime context만으로 정책을 판단한다.
- 조직 단위 RBAC, 외부 IAM 연동은 범위 밖이다.
- 사용자 신뢰도는 초기엔 정적 설정으로 시작한다.

## 현재 상태 분석

- `agent/approval.py`는 읽기 도구 즉시 허용, 위험 exec 패턴 즉시 거부, workspace 내 파일 쓰기 허용, 그 외 LLM 분류기/수동 승인 구조를 제공한다.
- 이 모델은 workspace skill 보호에는 유효하지만 assistant 전반의 actor/channel/trust 관점은 부족하다.

## 설계

### 정책 계층

```text
Tier 0: hard deny
Tier 1: always allow
Tier 2: trust rule
Tier 3: quota rule
Tier 4: approval rule
Tier 5: reasoning-blind classifier
```

### 범위

- user/channel/action-aware evaluator
- quota-aware decision
- planner/workflow 재사용
- 기존 approval gate와 하위 호환

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/agent/policy.py` | 신규 | actor/channel/action-aware policy evaluator |
| `shacs_bot/agent/approval.py` | 수정 | PolicyEvaluator 연동 |
| `shacs_bot/agent/usage.py` | 수정 | quota 판단용 summary helper 확장 |
| `shacs_bot/config/schema.py` | 수정 | PolicyConfig 추가 |
| `shacs_bot/agent/loop.py` | 수정 | tool/planner/workflow 실행 전 policy 적용 |

## 검증 기준

- [ ] 기존 auto/manual/off skill approval 모드가 유지됨
- [ ] channel/user trust rule이 실제 승인 결정에 반영됨
- [ ] quota 초과 시 degrade 또는 차단이 일관되게 동작함
- [ ] workflow/planner 단계에도 동일 정책이 적용됨

## Must NOT

- 정책 판단을 특정 채널 구현 내부에 흩뿌리지 않는다.
- usage enforcement 때문에 기존 usage 기록 경로가 깨지지 않는다.
