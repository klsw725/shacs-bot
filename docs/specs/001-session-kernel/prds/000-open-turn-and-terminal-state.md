# PRD 000. open turn and terminal state

## 목표

이 문서는 `docs/specs/001-session-kernel/SPEC.md`의 하위 실행 문서다. 목표는 현재 `AgentLoop`/`AgentRunner`/`SessionManager`/`SessionTurnLock` 구현을 전제로, 열린 턴의 생성, 진행, 종료 상태가 어떤 코드 경계와 evidence로 설명되는지 고정하고, `completed`와 `aborted`에 해당하는 terminal outcome까지 포함한 남은 보강 범위를 정리하는 것이다. 이 문서는 SPEC를 대체하지 않으며, 구현 순서와 검증 기준을 구체화한다.

- 한 세션에 동시에 하나의 열린 턴만 존재하도록 강제한다.
- `accepted`부터 `completed` 또는 `aborted`까지의 턴 수명주기를 현재 runtime loop, checkpoint, metadata, stop reason에 매핑한다.
- terminal state 진입 이후 동일 턴을 다시 열거나 늦게 도착한 외부 결과가 세션 기록을 되살리지 못하게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/001-session-kernel/SPEC.md`
- 교차 의존:
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`

## Dependency Cut

이 PRD는 session kernel 내부의 턴 상태 모델과 전이 규칙을 current code에 맞게 고정한다. provider 호출 세부 정규화, tool 결과 정규화, config discovery는 각 하위 spec에서 구현한다. 여기서는 그 결과를 받아들일 수 있는 열린 턴과 terminal state의 규칙, 그리고 이를 관측할 수 있는 marker/checkpoint/test evidence만 구현 대상으로 본다.

## 범위

- `Session`/`SessionManager`와 active turn marker/checkpoint의 `SessionState`/`TurnState` 개념 매핑
- 열린 턴 lock 획득, 교체 금지, 해제 규칙
- phase 개념과 current code의 `AgentLoop`/`AgentRunner`/`runtime_checkpoint` 매핑 검증
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

### 완료 판정

2026-05-12 기준 이 PRD는 완료로 닫는다. 완료의 의미는 새 `SessionState`/`TurnState` 저장소나 phase enum을 추가했다는 뜻이 아니라, current code의 `AgentLoop`/`AgentRunner`/`SessionManager`/`SessionTurnLock`/`runtime_checkpoint` 구조가 001의 open turn과 terminal state 계약을 충족한다고 문서상 확정한다는 뜻이다.

### 이미 반영된 것

- session history와 active turn runtime state는 `crates/shacs-session/src/lib.rs` 및 `crates/shacs-core/src/runtime/agent_loop.rs`/`runner.rs` 경계에 구현돼 있다.
- 열린 턴 직렬화, duplicate active session 차단, terminal 처리 후 runtime metadata 정리가 runtime loop 경로에 구현돼 있다.
- 닫힌 턴 late result 방어와 resume 이후 recovery abort 경로가 테스트까지 포함해 구현돼 있다.
- terminal 진입 시 pending effect, pending approval, pending tool effect, tool output, child task, accepted subagent summary, seen reentry key 같은 turn-local 임시 산출물을 정리하고 terminal payload만 반환하는 경계가 테스트로 고정돼 있다.

### 후속 비목표 / 별도 owner로 넘길 것

- 별도 durable `TurnState` 타입 도입은 현재 필수 작업이 아니다.
- `runtime_checkpoint`와 `pending_user_turn` marker의 schema/inspect projection은 더 명시할 여지가 있지만, 이는 013/014의 projection/inspection 또는 006의 store evidence 보강으로 다룬다.
- phase enum 전면 도입보다 current checkpoint/stop reason과 SPEC phase 사이의 mapping evidence가 우선이다.
- 멀티턴이나 더 넓은 recovery 상태 분류는 현재 범위 밖이다.

### 로컬 근거

- `crates/shacs-session/src/lib.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-core/tests/runtime_loop.rs`

## TDD 계획 결과

1. 새 입력 수용과 session 저장은 `runtime_loop` 및 `runtime_agent` 테스트에서 확인한다.
2. 열린 턴 중복 차단과 priority command 예외는 `runtime_loop` 테스트에서 확인한다.
3. `runtime_checkpoint` phase와 runner `stop_reason`은 `runtime_loop`/`runtime_agent`의 checkpoint 및 runner tool loop 테스트로 확인한다.
4. terminal outcome 이후 runtime marker 정리는 `runtime_loop`의 checkpoint cleanup 계열 테스트로 확인한다.
5. late/orphan tool result 방어는 `SessionManager` history repair와 runtime agent 테스트로 확인한다.
6. resume/recovery evidence는 `runtime_checkpoint` 보존 및 materialization 테스트로 확인한다.

## 구현 웨이브 결과

### Wave 1. 현재 구현 매핑과 불변식 문서화

- `Session`, `SessionManager`, `SessionTurnLock`, `AgentRunSpec`, `AgentRunResult`, `runtime_checkpoint`, `pending_user_turn`을 SPEC의 `SessionState`/`TurnState` 개념에 매핑한다.
- 열린 턴이 0개 또는 1개만 가능하다는 불변식이 `SessionTurnLock`과 테스트로 충분히 고정되는지 확인한다.
- terminal outcome을 phase enum이 아니라 runner `stop_reason`, marker cleanup, checkpoint evidence로 설명한다.

결과: 완료. SPEC의 current implementation mapping과 runtime tests가 이 wave를 닫는다.

### Wave 2. 턴 개시와 phase evidence 보강

- `accepted`, `context_building`, `model_pending`, `tool_pending`, `result_applying`, `completed`, `aborted`를 current code의 실행 지점과 checkpoint/stop reason에 매핑한다.
- 누락된 관측 지점이 있으면 새 core state type보다 inspect/read-model 또는 checkpoint schema 보강을 우선한다.
- phase 변경 시각 같은 세부 timestamp는 실제 사용자-visible debugging 필요가 생길 때만 추가한다.

결과: 완료. phase는 durable enum이 아니라 checkpoint/stop reason/read model evidence로 해석한다.

### Wave 3. terminal state 처리와 late result 방어

- terminal state 진입 시 turn lock 해제, runtime marker 정리, 종료 이유 보존이 현재 코드와 테스트에서 충분한지 확인한다.
- 이미 닫힌 턴의 late result 방어는 현재 message/history repair와 recovery marker 정책으로 설명 가능한지 확인하고, 부족하면 targeted test를 추가한다.
- 동일 턴 재오픈을 막는 보호 로직은 `SessionTurnLock`과 task registry 범위에서 보강한다.

결과: 완료. `runtime_loop`와 `runtime_agent` 테스트가 terminal cleanup, orphan/late result 방어, tool loop outcome을 덮는다.

### Wave 4. replay/resume 연결

- 복원된 상태에 열린 턴 marker가 있으면 recovery 경로가 그 턴을 자동 성공시키지 못하게 한다.
- session store와 연결될 수 있는 최소 resume 메타데이터 표현을 `runtime_checkpoint`/`pending_user_turn` 기준으로 고정한다.
- terminal state 이후 다음 턴을 안전하게 받을 수 있는 안정 상태를 검증한다.

결과: 완료. `runtime_checkpoint`/`pending_user_turn`은 crash/recovery evidence로 유지하고, 더 자세한 projection은 후속 owner spec에서 다룬다.

## Verification Evidence

- 문서 증거: `docs/specs/001-session-kernel/SPEC.md`의 current implementation mapping 표
- 통합 테스트: `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_loop --locked`
- 집중 테스트: `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_agent runtime_session_saves_loads_and_filters_history --locked`
- 집중 테스트: `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_agent runtime_runner_executes_tool_loop_and_accumulates_usage --locked`
- 코드 증거: `crates/shacs-core/src/runtime/agent_loop.rs`, `crates/shacs-core/src/runtime/runner.rs`, `crates/shacs-core/src/runtime/loop_control.rs`, `crates/shacs-session/src/lib.rs`

## Residual Risks

- `runtime_checkpoint` schema와 inspect projection은 더 명시될 수 있지만, 001 완료를 막는 blocker는 아니다.
- 더 넓은 recovery 분류와 app/process ledger 연계는 006/013/014/017에서 다룰 후속 범위다.
- durable `TurnState` 타입 도입은 필요가 입증될 때만 별도 설계로 다룬다.

## 종료 기준

- 한 세션에 동시 열린 턴이 절대 2개 생기지 않는다.
- SPEC phase가 current checkpoint/stop reason/marker와 설명 가능하게 매핑된다.
- `completed`와 `aborted` 진입 후 동일 턴은 다시 열리지 않는다.
- resume 이후 열린 턴은 자동 성공이 아니라 중단 방향으로 정리된다.
- 관련 테스트와 구현 문서가 `docs/specs/001-session-kernel/SPEC.md`의 불변식과 충돌하지 않는다.

위 기준은 current architecture 기준으로 충족된 것으로 판정한다. 이 PRD는 완료 상태이며, 이후 변경은 새 001 wave가 아니라 관련 owner spec의 좁은 보강 PRD로 추가한다.
