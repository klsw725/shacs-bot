# PRD 000. event log checkpoint and recovery

## 목표

이 문서는 `docs/specs/006-session-store/SPEC.md`의 하위 실행 문서다. SPEC을 대체하지 않고, 현재 구현된 JSONL `SessionManager` persistence와 `AgentLoop` recovery marker 처리를 완료 범위로 내리고, formal event log와 checkpoint replay 계열 작업을 후속 gap으로 분리한다.

이번 PRD의 목표는 Spec 006을 current architecture 기준으로 닫을 수 있게 만드는 것이다. 현재 저장 계층은 local single-user 세션 파일을 안전하게 쓰고, crash 이후 남은 marker를 자동 성공으로 꾸미지 않으며, CLI로 inspect 가능한 상태를 제공한다.

## SPEC 입력

- 주관 spec: `docs/specs/006-session-store/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
- 교차 의존:
  - `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 001과 002에서 세션 visible state와 Command, Event, Effect 용어의 권한 경계를 받는다.
- 003과 004에서 provider/tool 실행 결과가 세션에 직접 commit되지 않는다는 기준을 받는다.
- 009는 `last_consolidated` 이후 history suffix와 compacted memory 경계를 소비한다.
- 013과 014는 session inspect, history, export, diagnostics 표면을 사용자가 보는 읽기 모델로 다룬다.

이 PRD는 외부 DB, 분산 저장, 운영자 전용 복구 콘솔을 만들지 않는다. 기본 사용자는 로컬에서 직접 설치하고 운영하는 한 명의 사용자다.

## 범위

- 한 세션당 JSONL 파일 하나를 쓰는 현재 `SessionManager` 저장 형식 문서화
- metadata header와 message JSONL record 구조 문서화
- `metadata`, `messages`, `last_consolidated`의 현재 의미 고정
- `save_with_fsync` durability 의미 문서화
- `pending_user_turn`, `runtime_checkpoint` recovery marker 의미 문서화
- CLI inspect/history/export/diagnostics를 formal replay engine이 아닌 inspect surface로 문서화
- current architecture 기준 완료 판정 명시

## 범위 제외

- formal append-only Event log 구현
- `event_id`, `sequence`, `causation_id`, `correlation_id`를 가진 Event record 구현
- checkpoint와 event tail을 조합하는 replay engine 구현
- `last_committed_sequence`와 `last_included_sequence` 기반 metadata protocol 구현
- checkpoint corruption fallback 구현
- incomplete event tail discard 구현
- 외부 DB, 분산 저장, 멀티유저 session ownership

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 Spec 006은 current architecture 기준으로 완료로 닫는다. 완료의 의미는 formal event-sourcing 저장소가 아니라, 현재 JSONL session file과 runtime recovery marker 경계가 문서와 테스트 증거로 고정됐다는 뜻이다.

### 이미 반영된 것

- `SessionManager`는 workspace `sessions/` 아래 세션별 JSONL 파일을 저장한다.
- 첫 줄은 metadata header이며 `key`, `created_at`, `updated_at`, `metadata`, `last_consolidated`를 담는다.
- 뒤따르는 줄들은 `messages`의 user, assistant, tool 등 message JSON object다.
- `save_with_fsync`는 temp file에 header와 message line을 쓰고 flush한 뒤, fsync 모드에서 file fsync, rename, directory fsync 순서로 durable write를 수행한다.
- load 경로는 metadata header를 `Session.metadata`와 `Session.last_consolidated`로 복원하고, message line을 `Session.messages`로 복원한다.
- `pending_user_turn`은 이전 열린 턴을 interrupted assistant message로 물질화한 뒤 제거된다.
- `runtime_checkpoint`는 assistant message와 pending tool call placeholder로 물질화한 뒤 제거된다.
- checkpoint callback은 tool execution 중 `runtime_checkpoint`를 metadata에 저장하고, 성공 경로는 marker를 지운다.
- CLI `session inspect`, `session history`, `session export`, `session diagnostics`는 저장된 local session state를 확인하는 inspect 표면이다.

### 아직 남은 것

- formal append-only Event log
- `event_id`, `sequence`, `causation_id`, `correlation_id` record
- checkpoint와 event tail replay engine
- `last_committed_sequence`와 `last_included_sequence` 분리
- checkpoint corruption fallback
- incomplete event tail discard
- replay 결과와 runtime materialization 결과를 분리하는 formal resume API

위 항목은 current architecture 기준 Spec 006 완료의 blocker가 아니다. 후속 storage owner가 event-sourcing 저장소를 실제로 도입할 때 별도 spec update와 테스트로 닫아야 한다.

### 로컬 근거

- `crates/shacs-session/src/lib.rs`
- `crates/shacs-session/tests/session_manager.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-cli/src/lib.rs`

## TDD Evidence

- `session_manager_writes_metadata_header_then_message_jsonl`은 `save_with_fsync`가 metadata header를 먼저 쓰고 user, assistant message JSONL line을 이어 쓰는 현재 storage shape를 검증한다.
- `session_manager_saves_loads_metadata_and_history`는 metadata와 `last_consolidated`가 저장 뒤 load되고 history suffix 계산에 반영되는지 검증한다.
- `session_manager_exposes_python_compatibility_paths_and_payload`는 compatibility path와 JSON payload inspect 표면을 검증한다.
- `session_manager_reads_clears_and_deletes_legacy_nanobot_filename`는 legacy nanobot filename을 읽고 canonical path로 정리하는 경로를 검증한다.
- `loop_pending_user_turn_recovery_closes_interrupted_prior_turn`는 `pending_user_turn`이 자동 성공이 아니라 interrupted assistant message로 복구되는지 검증한다.
- `loop_runtime_checkpoint_materializes_placeholders_and_clears_metadata`는 `runtime_checkpoint`가 placeholder message로 물질화되고 metadata에서 제거되는지 검증한다.
- `loop_checkpoint_callback_persists_during_tool_execution_and_success_clears`는 tool execution 중 checkpoint marker가 저장되고 성공 뒤 정리되는지 검증한다.
- CLI `session_inspect`, `session_history`, `session_export`, `session_diagnostics` 함수는 사용자가 현재 저장 상태를 확인하는 inspect surface의 코드 증거다.

## 구현 웨이브 판정

### Wave 1. JSONL 저장 형식 고정

- 상태: 완료
- 결과: metadata header first line, message JSONL records, `metadata`, `messages`, `last_consolidated` 의미가 현재 구현과 테스트로 고정됐다.

### Wave 2. durability write 경계 고정

- 상태: 완료
- 결과: temp file, flush, optional file fsync, rename, optional directory fsync 순서가 `save_with_fsync` 의미로 문서화됐다.

### Wave 3. recovery marker materialization 고정

- 상태: 완료
- 결과: `pending_user_turn`과 `runtime_checkpoint`가 열린 턴을 자동 성공시키지 않고 interrupted 또는 placeholder message로 물질화된다.

### Wave 4. inspect surface 고정

- 상태: 완료
- 결과: CLI inspect/history/export/diagnostics가 formal replay가 아니라 local session file inspect surface로 정리됐다.

### Wave 5. formal event-sourcing store

- 상태: 후속 gap
- 결과: append-only Event log, sequence record, checkpoint tail replay, corruption fallback, incomplete tail discard는 아직 없다.

## Verification Evidence

- `session_manager_writes_metadata_header_then_message_jsonl`
- `session_manager_saves_loads_metadata_and_history`
- `session_manager_exposes_python_compatibility_paths_and_payload`
- `session_manager_reads_clears_and_deletes_legacy_nanobot_filename`
- `loop_pending_user_turn_recovery_closes_interrupted_prior_turn`
- `loop_runtime_checkpoint_materializes_placeholders_and_clears_metadata`
- `loop_checkpoint_callback_persists_during_tool_execution_and_success_clears`
- CLI `session_inspect`, `session_history`, `session_export`, `session_diagnostics`

## Open Risks

- JSONL message records를 formal Event log로 오해하면 구현 완료 범위를 과장하게 된다.
- `runtime_checkpoint`를 replay checkpoint로 오해하면 현재 marker materialization과 future replay engine의 경계가 흐려진다.
- future event-sourcing store를 도입할 때 `last_consolidated`를 event sequence처럼 재사용하면 history compaction 의미와 recovery sequence 의미가 섞일 수 있다.

## 종료 기준

- Spec 006은 current architecture 기준 완료로 표시된다.
- 문서는 현재 완료 범위를 JSONL `SessionManager` persistence와 `AgentLoop` recovery marker 처리로 한정한다.
- 문서는 formal append-only Event log, event sequence record, checkpoint tail replay, corruption fallback, incomplete tail discard가 구현됐다고 주장하지 않는다.
- current evidence는 session manager, runtime loop, CLI inspect surface 테스트와 코드 증거를 가리킨다.

위 기준은 이 PRD에서 충족됐다. 남은 항목은 현재 완료 범위의 결함이 아니라 future session store architecture gap이다.
