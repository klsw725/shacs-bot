# PRD 000. open turn and terminal state

## 목표

이 문서는 `docs/specs/001-session-kernel/SPEC.md`의 하위 실행 문서다. 목표는 session kernel에서 열린 턴의 생성, 진행, 종료 상태를 Rust 구현 단위로 쪼개고, `completed`와 `aborted`로 닫히는 terminal state까지 포함한 전체 구현 계획을 고정하는 것이다. 이 문서는 SPEC를 대체하지 않으며, 구현 순서와 검증 기준을 구체화한다.

- 한 세션에 동시에 하나의 열린 턴만 존재하도록 강제한다.
- `accepted`부터 `completed` 또는 `aborted`까지의 턴 수명주기를 상태 모델과 전이 API로 구현한다.
- terminal state 진입 이후 동일 턴을 다시 열거나 늦게 도착한 외부 결과가 세션 기록을 되살리지 못하게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/001-session-kernel/SPEC.md`
- 교차 의존:
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`

## Dependency Cut

이 PRD는 session kernel 내부의 턴 상태 모델과 전이 규칙을 먼저 고정한다. provider 호출 세부 정규화, tool 결과 정규화, config discovery는 각 하위 spec에서 구현한다. 여기서는 그 결과를 받아들일 수 있는 열린 턴과 terminal state의 규칙만 구현 대상으로 본다.

## 범위

- `SessionState`와 `TurnState`의 최소 필드 정의
- 열린 턴 포인터 생성, 교체 금지, 제거 규칙
- phase 전이 API와 불변식 검증
- terminal state 진입 시 세션 측 정리 규칙
- 이미 닫힌 턴에 대한 late result 방어 규칙
- replay/resume 시 열린 턴 정리와 연결되는 상태 표현

## 범위 제외

- provider 벤더별 호출 세부 구현
- tool executor 세부 구현
- checkpoint 파일 포맷 결정
- 멀티턴 병렬 실행
- 멀티유저 세션 동기화

## 현재 구현 상태

### 이미 반영된 것

- `SessionState`, `TurnState`, `TurnPhase`, 열린 턴 1개 불변식이 `crates/shacs-core/src/core/state.rs`에 구현돼 있다.
- 열린 턴 생성, 금지된 phase 점프 차단, `completed`/`aborted` 진입 후 열린 턴 포인터 정리가 `crates/shacs-core/src/core/orchestrator.rs`에 구현돼 있다.
- 닫힌 턴 late result 방어와 resume 이후 recovery abort 경로가 테스트까지 포함해 구현돼 있다.
- terminal 진입 시 pending effect, pending approval, pending tool effect, tool output, child task, accepted subagent summary, seen reentry key 같은 turn-local 임시 산출물을 정리하고 terminal payload만 반환하는 경계가 테스트로 고정돼 있다.

### 아직 남은 것

- 멀티턴이나 더 넓은 recovery 상태 분류는 현재 범위 밖이다.

### 로컬 근거

- `crates/shacs-core/src/core/state.rs`
- `crates/shacs-core/src/core/orchestrator.rs`
- `crates/shacs-core/tests/session_kernel.rs`

## TDD 계획

1. 새 입력 수용 시 열린 턴이 없으면 `accepted` 턴이 생성되는 테스트를 먼저 작성한다.
2. 열린 턴이 있을 때 새 턴 수용이 거절되는 테스트를 작성한다.
3. 허용된 phase 전이만 성공하고 금지된 점프 전이는 실패하는 테스트를 작성한다.
4. `completed` 또는 `aborted` 진입 후 열린 턴 포인터가 정리되는 테스트를 작성한다.
5. 닫힌 턴으로 late result가 재진입해도 상태가 되살아나지 않는 테스트를 작성한다.
6. resume 시 열린 턴이 자동 성공 처리되지 않고 중단 방향으로 정리되는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. 상태 타입과 불변식 골격

- `SessionState`와 `TurnState`의 핵심 필드를 Rust 타입으로 고정한다.
- 열린 턴이 0개 또는 1개만 가능하다는 불변식을 생성자와 mutation API에 넣는다.
- terminal state를 일반 phase와 분리해 타입 또는 검증 계층에서 구분한다.

### Wave 2. 턴 개시와 phase 전이

- `accepted`, `context_building`, `model_pending`, `tool_pending`, `result_applying`, `completed`, `aborted` 전이를 구현한다.
- 각 전이마다 진입 조건과 금지 조건을 명시적으로 검증한다.
- phase 변경 시각과 마지막 갱신 시각을 함께 기록한다.

### Wave 3. terminal state 처리와 late result 방어

- terminal state 진입 시 열린 턴 포인터 제거, 임시 산출물 정리, 종료 이유 보존을 구현한다.
- 이미 닫힌 턴의 `turn_id`, `effect_id`, `correlation_id`에 대한 재진입은 수용하지 않고 무시 또는 관찰 이벤트 후보로만 넘긴다.
- 동일 턴 재오픈을 막는 보호 로직을 추가한다.

### Wave 4. replay/resume 연결

- 복원된 상태에 열린 턴이 있으면 recovery 경로가 그 턴을 자동 성공시키지 못하게 한다.
- session store와 연결될 수 있는 최소 resume 메타데이터 표현을 고정한다.
- terminal state 이후 다음 턴을 안전하게 받을 수 있는 안정 상태를 검증한다.

## Verification Evidence

- phase 전이 단위 테스트
- 단일 열린 턴 불변식 테스트
- terminal state 이후 late result 무시 테스트
- recovery 경로에서 열린 턴 중단 처리 테스트
- terminal state 진입 시 turn-local temporary artifact discard 테스트
- full-spec matrix evidence: `crates/shacs-contracts/src/verification.rs`에서 Spec001의 Unit/Integration/DurabilityRecovery family가 `CoverageLevel::FullSpec` verified evidence로 승격된다.
- owning spec의 phase 순서와 구현 타입 매핑 표

## Open Risks

- `TurnState` 필드가 과도하게 커지면 session-local과 turn-local 경계가 흐려질 수 있다.
- terminal state 진입 시 어떤 임시 산출물을 보존할지 모호하면 replay semantics가 흔들릴 수 있다.
- late result 무시 규칙이 느슨하면 닫힌 턴 결과가 뒤집힐 위험이 있다.

## 종료 기준

- 한 세션에 동시 열린 턴이 절대 2개 생기지 않는다.
- 허용된 phase 전이만 성공한다.
- `completed`와 `aborted` 진입 후 동일 턴은 다시 열리지 않는다.
- resume 이후 열린 턴은 자동 성공이 아니라 중단 방향으로 정리된다.
- 관련 테스트와 구현 문서가 `docs/specs/001-session-kernel/SPEC.md`의 불변식과 충돌하지 않는다.
