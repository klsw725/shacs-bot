# PRD 000. event log checkpoint and recovery

## 목표

이 문서는 `docs/specs/006-session-store/SPEC.md`의 하위 실행 문서다. 목표는 event log, checkpoint, replay, resume, recovery를 POC가 아닌 완전 구현 기준으로 쪼개고, crash 이후에도 단일 세션 정확성을 유지하는 저장 계층 계획을 고정하는 것이다.

- 공식 이력의 진실 원천을 append-only event log와 checkpoint 조합으로 구현한다.
- checkpoint가 event log를 대체하지 않고 replay 시작점으로만 동작하게 한다.
- resume 시 열린 턴을 자동 성공시키지 않고 recovery 규칙대로 정리한다.

## SPEC 입력

- 주관 spec: `docs/specs/006-session-store/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`

## Dependency Cut

이 PRD는 session store 의미론과 복구 절차에 집중한다. 개별 파일 형식 최적화, 분산 저장, 외부 DB 연동은 범위 밖이다. 로컬 단일 사용자 환경에서 결정적 replay와 안전한 recovery를 우선한다.

## 범위

- session metadata, event log, checkpoint의 최소 구조
- append와 durability 의미론
- replay 시작점 선택 규칙
- deterministic resume 절차
- 열린 턴 recovery 규칙
- checkpoint 손상과 event tail 손상 복구 규칙

## 범위 제외

- 분산 합의나 멀티노드 복구
- 외부 데이터베이스 백엔드 추상화
- 고급 log compaction 최적화
- 실시간 projection 캐시 시스템

## 현재 구현 상태

### 이미 반영된 것

- session meta, history persistence, checkpoint/resume snapshot은 `crates/shacs-session/src/lib.rs`와 runtime checkpoint 경계에 구현돼 있다.
- in-memory store와 file-backed store 모두 append/replay/checkpoint/resume 경로를 가진다.
- atomic checkpoint write, truncated tail 무시, stale meta보다 event log sequence 우선, corrupted checkpoint fallback이 구현돼 있다.
- Spec016 matrix에서 Unit, Integration, DurabilityRecovery가 FullSpec verified evidence로 승격돼 있다.

### 아직 남은 것

- 고급 compaction/rotation, 더 풍부한 durability tuning, 더 완전한 checkpoint family 선택 전략은 아직 없다.
- 현재 구현은 local single-user runtime 기준의 최소 저장 계층에 집중돼 있다.
- 위 항목은 Spec006 FullSpec slice의 blocker가 아니라 후속 저장 엔진 최적화 범위다.

### 로컬 근거

- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-session/src/lib.rs`

## TDD 계획

1. append된 event sequence가 세션별 단조 증가하는 테스트를 작성한다.
2. checkpoint와 그 이후 event tail로 동일 `SessionState`가 재구성되는 테스트를 작성한다.
3. 손상된 checkpoint를 버리고 이전 checkpoint 또는 전체 replay로 복구하는 테스트를 작성한다.
4. 열린 턴이 있는 상태로 resume하면 자동 성공이 아니라 recovery 종료 상태로 정리되는 테스트를 작성한다.
5. `last_committed_sequence` 이후의 불완전 tail이 공식 상태에 반영되지 않는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. 저장 모델과 메타데이터 타입 고정

- session meta, event record, checkpoint meta의 최소 필드를 정의한다.
- `last_committed_sequence`와 `last_included_sequence` 의미를 분리한다.
- event log가 공식 이력의 기준이라는 타입과 API 구조를 고정한다.

### Wave 2. append와 replay 엔진 구현

- 세션별 append-only event log 쓰기와 읽기를 구현한다.
- checkpoint 선택 후 tail event를 재적용하는 replay 엔진을 만든다.
- replay가 effect 재실행이 아니라 공식 event 재적용임을 코드 경계로 분명히 한다.

### Wave 3. checkpoint와 recovery 규칙 구현

- 닫힌 턴 이후에만 checkpoint를 생성하는 규칙을 적용한다.
- checkpoint 무결성 검증 실패 시 fallback 전략을 구현한다.
- resume 시 열린 턴을 `aborted` 또는 이에 준하는 복구 종료 상태로 정리한다.

### Wave 4. durability 및 손상 방어 검증

- `last_committed_sequence` 이후 부분 기록은 공식 이력으로 채택하지 않도록 한다.
- checkpoint 메타데이터 갱신 실패 시에도 기존 event log와 checkpoint로 복구 가능하게 한다.
- late result가 recovery 이후 공식 이력을 뒤집지 못하도록 오케스트레이터 연결 규칙을 검증한다.

## Verification Evidence

- replay 동치성 테스트
- 손상된 checkpoint fallback 테스트
- 불완전 tail 배제 테스트
- 열린 턴 recovery 테스트
- 닫힌 턴 이후만 checkpoint 가능하다는 검증 테스트
- stale meta보다 complete event log sequence가 resume 기준이 되는 file-backed 테스트
- checkpointed agent configuration과 compacted preserved context가 resume 이후 유지되는 file-backed 테스트

## Open Risks

- checkpoint와 event tail 경계를 잘못 잡으면 replay 동치성이 깨질 수 있다.
- recovery 과정에서 열린 턴 정리 이벤트를 어떻게 남길지 모호하면 사용자 관찰 가능성이 떨어질 수 있다.
- durability 표시와 실제 디스크 기록 시점이 어긋나면 deterministic resume이 무너질 수 있다.

## 종료 기준

- event log와 checkpoint 조합으로 같은 `SessionState`를 결정적으로 복원할 수 있다.
- 열린 턴은 resume 시 자동 성공하지 않는다.
- 손상된 checkpoint와 불완전 event tail에 대한 보수적 복구가 구현된다.
- `docs/specs/006-session-store/SPEC.md`의 replay, resume, recovery 의미론이 테스트로 증명된다.
