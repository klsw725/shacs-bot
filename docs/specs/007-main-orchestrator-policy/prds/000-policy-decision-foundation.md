# PRD 000. policy decision foundation

## 목표

이 문서는 `docs/specs/007-main-orchestrator-policy/SPEC.md`의 하위 실행 문서다. 목표는 `MainOrchestrator`가 소유해야 하는 정책 결정을 코드 구조, 결정표, snapshot 전달, recovery 연결까지 포함해 실행 가능한 구현 계획으로 고정하는 것이다.

- 새 턴 수용, approval, selection, retry, abort, timeout, late result 판단을 오케스트레이터 단일 권한으로 구현한다.
- durable policy state와 turn-local policy state를 분리한다.
- policy snapshot이 provider/tool effect로 내려가되, 실행기가 policy owner를 대체하지 못하게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/007-main-orchestrator-policy/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`

## Dependency Cut

이 PRD는 정책 판단의 소유권과 평가 시점을 구현 대상으로 삼는다. 개별 executor 내부 알고리즘이나 UI 승인 화면은 범위 밖이다. 목표는 오케스트레이터 정책 계층의 최소 완성본을 먼저 세우는 것이다.

## 범위

- policy state 모델, durable/turn-local 경계
- 새 턴 수용 정책
- selection 정책과 snapshot 생성
- approval 정책과 대기/거절/승인 분기
- retry, abort, timeout, late result 결정
- recovery 이후 정책 재구성 연결

## 범위 제외

- 관리자 승인 체인
- 통계 기반 자동 최적화
- 멀티유저 역할 기반 정책
- provider 벤더별 상세 튜닝 UI

## 현재 구현 상태

### 이미 반영된 것

- recovery gate, retry/abort/late-result 판단, retry ceiling, resumed session 정리 정책이 `crates/shacs-core/src/core/policy.rs`, `orchestrator.rs`에 구현돼 있다.
- provider retry, recovery 중 input/reentry 차단, duplicate reentry after resume, pending effect replay 복원이 테스트로 증명돼 있다.
- tool reentry acceptance도 현재는 오케스트레이터 policy gate 아래에서 `ToolPending + RunTool` 조건으로 제한된다.
- Spec016 matrix에서 Unit, Integration, SafetyRedaction이 FullSpec verified evidence로 승격돼 있다.

### 아직 남은 것

- approval/selection policy와 durable/turn-local policy state의 더 분리된 모델은 아직 얕다.
- 정책 snapshot은 effect에 실리기 시작했지만, 더 풍부한 executor-facing policy surface는 아직 미구현이다.
- 위 항목은 현재 MainOrchestrator 단일 정책 소유권 FullSpec slice의 blocker가 아니라 후속 policy surface 확장 범위다.

### 로컬 근거

- `crates/shacs-core/src/core/policy.rs`
- `crates/shacs-core/src/core/orchestrator.rs`
- `crates/shacs-core/tests/command_event_effect.rs`
- `crates/shacs-core/tests/session_store_replay.rs`

## TDD 계획

1. 열린 턴이 없을 때만 새 턴이 수용되는 테스트를 작성한다.
2. permission mode와 capability 조합에 따라 approval 결정표가 동작하는 테스트를 작성한다.
3. provider/tool 실패 결과별 retry 또는 abort 결정 테스트를 작성한다.
4. late result가 현재 활성 correlation 집합 밖이면 폐기되는 테스트를 작성한다.
5. recovery 후 durable policy state가 다시 구성되고 turn-local 판단은 승계되지 않는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. policy state와 결정 API 골격

- durable policy state와 turn-local policy state를 서로 다른 타입 또는 필드 영역으로 분리한다.
- `PolicyDecisionEngine`에 해당하는 평가 API를 도입한다.
- 새 턴 수용, selection, approval, retry/abort, late result 판단 entrypoint를 고정한다.

### Wave 2. 결정표 구현과 snapshot 생성

- spec의 결정표를 코드 분기와 테스트 케이스로 옮긴다.
- provider invocation과 tool execution에 필요한 policy snapshot을 생성한다.
- snapshot이 effect executor 계약일 뿐 정책 권한 위임이 아님을 코드 구조로 보장한다.

### Wave 3. 실패 및 late result 처리 통합

- provider/tool/subagent 결과 재진입 시 retry, abort, alternate selection 분기를 연결한다.
- 활성 correlation 집합 기반 late result 판별을 구현한다.
- 이미 닫힌 턴 결과는 관찰 이벤트 후보로만 남기고 상태에는 반영하지 않는다.

### Wave 4. recovery와 durable 재구성

- replay/resume 이후 durable policy state를 복원한다.
- approval 대기 중 effect, retry count, timeout deadline 같은 turn-local 상태는 새 턴으로 넘기지 않는다.
- recovery 중 열린 턴 정리와 policy decision 연계를 검증한다.

## Verification Evidence

- 정책 결정표 단위 테스트
- approval mode별 테스트
- retry/abort/late result 테스트
- snapshot 생성과 executor 경계 검증
- durable/turn-local 경계에 대한 replay 검증
- durable policy state와 turn-local policy state가 별도 snapshot으로 분리되는 unit 테스트

## Open Risks

- 정책 판단이 여러 모듈로 흩어지면 ownership이 무너질 수 있다.
- decision API가 과도하게 범용화되면 결정 시점이 흐려질 수 있다.
- durable과 turn-local 경계가 약하면 recovery 후 잘못된 승인 상태가 남을 수 있다.

## 종료 기준

- 핵심 정책 판단이 `MainOrchestrator` 단일 경로로 수렴한다.
- durable 정책과 turn-local 정책이 분리되어 replay와 resume 후에도 일관된다.
- effect snapshot은 생성되지만 executor는 정책 확정자가 되지 않는다.
- `docs/specs/007-main-orchestrator-policy/SPEC.md`의 결정표와 금지 패턴이 코드와 테스트에 반영된다.
