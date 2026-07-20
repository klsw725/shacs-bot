# session store 아키텍처 명세

Status: Complete (Scoped)
Implemented scope: 세션별 JSONL `SessionManager`, metadata header, message records, `last_consolidated`, `save_with_fsync`, recovery marker materialization, CLI session inspect 표면을 current session store로 닫았다.
Open work moved to: [028 formal execution reentry and outcome contracts](../028-formal-execution-reentry-and-outcome-contracts/SPEC.md), [029 durable runtime recovery and data migration](../029-durable-runtime-recovery-and-data-migration/SPEC.md)
Not carried forward: 외부 DB, 분산 저장, 멀티유저 동시성 제어를 006 또는 029의 기본 완료 조건으로 가져가지 않는다.

## 문서 목적

이 문서는 현재 코드베이스의 session store 구조를 기준으로, 로컬 세션 파일 저장과 crash 이후 재진입 경계를 정리한다. Spec 001은 세션 커널을, Spec 002는 Command, Event, Effect 용어를 아키텍처 권한 경계로 정리했다. 이 문서도 같은 기준을 따른다.

현재 구현은 formal append-only Event log와 checkpoint tail replay engine을 중심으로 하지 않는다. 실제 저장 경계는 `shacs-session`의 JSONL `SessionManager`와 `shacs-core`의 `AgentLoop` recovery marker 처리로 구성된다.

핵심 불변식은 유지한다. 세션 파일은 사용자가 직접 운영하는 self-hosted, local single-user 환경에서 한 세션의 대화 이력과 recovery marker를 보존해야 한다. 저장 계층은 외부 실행을 자동 성공시키지 않고, 열린 턴이 남아 있으면 사용자가 볼 수 있는 중단 상태로 물질화해야 한다.

## 현재 범위

이 문서는 다음을 설명한다.

- 한 세션당 하나의 JSONL 파일을 쓰는 현재 `SessionManager` 저장 형식
- metadata header와 message JSONL record의 의미
- `metadata`, `messages`, `last_consolidated`가 현재 session store에서 맡는 역할
- `save_with_fsync`의 temp file, flush, optional fsync, rename, optional directory fsync durability 의미
- `pending_user_turn`, `runtime_checkpoint`를 통한 `AgentLoop` recovery marker 처리
- CLI `session inspect`, `session history`, `session export`, `session diagnostics`가 제공하는 inspect 표면

현재 완료 판정은 다음을 blocker로 보지 않는다. 이 항목들은 구현됐다고 주장하지 않고, 후속 owner 작업으로 남긴다.

- formal append-only Event log
- `event_id`, `sequence`, `causation_id`, `correlation_id`를 가진 Event record
- checkpoint와 event tail을 조합하는 replay engine
- `last_committed_sequence`와 `last_included_sequence` 기반 recovery protocol
- checkpoint corruption fallback
- incomplete event tail discard

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 Spec 006은 current architecture 기준으로 완료로 닫는다. 완료의 의미는 formal event-sourcing store, append-only Event log, checkpoint replay engine, sequence 기반 tail recovery를 구현했다는 뜻이 아니다.

완료의 의미는 현재 코드의 `SessionManager` JSONL persistence, metadata header, message records, `last_consolidated`, `save_with_fsync`, `AgentLoop`의 `pending_user_turn`과 `runtime_checkpoint` recovery marker 처리, CLI session inspect 표면이 session store의 현재 경계로 문서화됐고, 기존 테스트 증거가 그 범위 안에서 유지된다는 뜻이다.

### 이미 반영된 것

- `SessionManager`는 workspace 아래 `sessions/`에 세션별 JSONL 파일 하나를 저장한다.
- 파일 첫 줄은 `{"_type":"metadata"}` header이며 `key`, `created_at`, `updated_at`, `metadata`, `last_consolidated`를 담는다.
- 그 뒤 줄들은 `messages`의 각 message JSON object를 순서대로 기록한다.
- load 경로는 metadata header를 읽어 `Session.metadata`와 `Session.last_consolidated`를 복원하고, 나머지 JSON line을 `Session.messages`로 복원한다.
- `Session::payload`와 CLI JSON export는 inspect 편의를 위해 `metadata`와 `messages`를 포함한 JSON object로 보여준다. 이것은 파일의 물리 저장 형식이 아니라 읽기 표면이다.
- `last_consolidated`는 compacted memory 이후 history suffix를 계산하는 현재 필드다. event sequence나 checkpoint sequence가 아니다.
- `save_with_fsync`는 고유 temp file을 만들고, metadata header와 message line을 쓰고, flush한 뒤 optional `file.sync_all()`을 수행하고, rename으로 canonical session path를 교체한다. fsync 모드에서는 rename 뒤 session directory도 fsync한다.
- `AgentLoop`는 기존 세션에 `pending_user_turn`이 남아 있으면 이전 열린 턴을 정상 성공으로 처리하지 않고 interrupted assistant message로 물질화한 뒤 marker를 지운다.
- `AgentLoop`는 `runtime_checkpoint`가 남아 있으면 checkpoint 안의 assistant message와 pending tool call을 placeholder message로 물질화하고 marker를 지운다.
- tool 실행 중 checkpoint callback은 `runtime_checkpoint`를 세션 metadata에 저장하고, 성공 경로는 해당 metadata를 정리한다.
- CLI `session inspect`, `session history`, `session export`, `session diagnostics`는 저장된 session JSONL과 metadata를 확인하는 inspect 표면이다. formal replay engine이 아니다.

### 후속 비목표 / 별도 owner로 넘길 것

- formal append-only Event log 도입
- `event_id`, `sequence`, `causation_id`, `correlation_id`를 포함한 Event record 저장 형식
- checkpoint와 event tail을 입력으로 받는 replay engine
- `last_committed_sequence`와 `last_included_sequence`를 분리한 metadata protocol
- checkpoint corruption fallback
- incomplete event tail discard
- event tail replay 기반 deterministic resume API
- log compaction, rotation, checksum family, checkpoint family 선택 전략
- 외부 DB, 분산 저장, 멀티유저 동시성 제어

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- 세션 visible state는 runtime 또는 orchestrator 경계가 결정한다.
- provider, tool, transport는 세션 파일을 직접 공식 상태로 확정하지 않는다.
- 저장 계층은 self-hosted, local single-user 환경에서 사용자가 직접 inspect하고 복구 판단을 할 수 있어야 한다.
- crash 이후 열린 턴은 자동 성공으로 보이면 안 된다.

따라서 session store는 독립 정책 엔진이 아니다. 현재 구현에서 session store는 로컬 파일 저장과 읽기, metadata marker 보존, inspect 가능한 payload 제공을 맡는다. recovery marker를 어떤 message로 물질화할지는 `AgentLoop`가 맡는다.

## 저장 모델

### 물리 파일 단위

현재 저장 단위는 세션 하나당 JSONL 파일 하나다.

- canonical path는 `SessionManager::session_path(key)`가 정한다.
- 파일명은 session key를 안전한 문자열로 바꾼 뒤 `.jsonl` 확장자를 붙인다.
- legacy nanobot filename은 compatibility 경로로 읽고 정리할 수 있지만, 현재 canonical 저장 형식은 workspace `sessions/` 아래 JSONL 파일이다.

### JSONL header

첫 줄은 metadata header다.

```json
{"_type":"metadata","key":"cli:direct","created_at":"...","updated_at":"...","metadata":{},"last_consolidated":0}
```

header의 의미는 다음과 같다.

- `key`는 세션 식별자다.
- `created_at`과 `updated_at`은 현재 session metadata의 시간 필드다.
- `metadata`는 runtime recovery marker와 session 부가 metadata를 담는다.
- `last_consolidated`는 history 조회에서 이미 compacted memory로 흡수된 message prefix 길이를 표시한다.

### message record

두 번째 줄부터는 `Session.messages`의 JSON object가 한 줄에 하나씩 저장된다.

message record는 현재 대화 이력이다. formal Event record가 아니며, `event_id`, `sequence`, `causation_id`, `correlation_id`를 요구하지 않는다.

### 읽기 표면

`read_session_file`과 CLI JSON export는 파일을 다시 object 형태로 조립해 보여준다.

```json
{
  "key": "cli:direct",
  "created_at": "...",
  "updated_at": "...",
  "metadata": {},
  "last_consolidated": 0,
  "messages": []
}
```

이 object는 inspect와 export를 위한 읽기 표면이다. 저장 파일의 첫 줄 header와 message JSONL record 구조를 숨기지 않는다.

## durability 의미

현재 durability는 session file 교체 단위로 정의된다.

1. canonical session path 옆에 고유 temp file path를 만든다.
2. temp file을 `create_new`로 연다.
3. metadata header를 한 줄 쓴다.
4. 모든 message record를 한 줄씩 쓴다.
5. file buffer를 flush한다.
6. `save_with_fsync` 경로면 temp file에 `sync_all`을 호출한다.
7. temp file을 canonical session path로 rename한다.
8. `save_with_fsync` 경로면 session directory도 fsync한다.
9. 실패하면 남은 temp file을 best-effort로 제거한다.

이 절차의 의미는 "세션 파일 하나를 이전 완성본 또는 새 완성본으로 보이게 한다"는 것이다. formal event append durability나 committed sequence protocol을 제공한다는 뜻은 아니다.

## 현재 recovery 의미론

### `pending_user_turn`

`pending_user_turn`은 이전 실행이 user turn을 열어 둔 채 끝났음을 나타내는 metadata marker다.

현재 `AgentLoop`는 새 입력을 처리하기 전에 이 marker를 확인한다. marker가 있으면 이전 열린 턴을 성공으로 만들지 않고, interrupted assistant message를 추가해 사용자에게 중단 사실을 보인다. 이후 marker를 지운다.

### `runtime_checkpoint`

`runtime_checkpoint`는 provider/tool loop 중간에 runtime이 저장한 recovery marker다.

현재 `AgentLoop`는 checkpoint 안의 assistant message와 pending tool call을 읽어 placeholder message로 물질화한다. pending tool result는 lost 또는 interrupted placeholder로 보이며, 열린 턴이 자동 성공으로 닫히지 않는다. 물질화 뒤에는 marker를 지운다.

### 자동 성공 금지

현재 recovery의 핵심은 "남아 있는 marker를 완료 결과로 추측하지 않는다"는 점이다. provider socket, tool process, streaming chunk, 임시 cache는 session store의 공식 resume 입력이 아니다.

## inspect 표면

CLI session command는 현재 session store를 확인하는 사용자 표면이다.

- `session inspect`는 path, metadata key, message count, `last_consolidated`, recovery marker, checkpoint phase를 보여준다.
- `session history`는 `last_consolidated` 이후의 legal history suffix를 보여준다.
- `session export`는 raw local session content를 JSON 또는 JSONL로 내보낸다.
- `session diagnostics`는 session file과 metadata 상태를 진단한다.

이 명령들은 replay engine이 아니다. 저장된 JSONL과 현재 metadata marker를 사용자가 확인하기 위한 inspect surface다.

## Verification Evidence

- `session_manager_writes_metadata_header_then_message_jsonl`은 `save_with_fsync`가 metadata header를 먼저 쓰고 user, assistant message JSONL line을 이어 쓰는 현재 파일 형식을 검증한다.
- `session_manager_saves_loads_metadata_and_history`는 metadata, `last_consolidated`, history suffix가 저장과 load 뒤 유지되는지 검증한다.
- `session_manager_exposes_python_compatibility_paths_and_payload`는 compatibility path와 inspect payload 표면을 검증한다.
- `session_manager_reads_clears_and_deletes_legacy_nanobot_filename`는 legacy nanobot filename 정리 경로를 검증한다.
- `loop_pending_user_turn_recovery_closes_interrupted_prior_turn`는 `pending_user_turn`이 남은 열린 턴을 interrupted assistant message로 물질화하고 marker를 지우는지 검증한다.
- `loop_runtime_checkpoint_materializes_placeholders_and_clears_metadata`는 `runtime_checkpoint`가 assistant/tool placeholder로 물질화되고 metadata에서 제거되는지 검증한다.
- `loop_checkpoint_callback_persists_during_tool_execution_and_success_clears`는 tool execution 중 checkpoint callback이 metadata를 저장하고 성공 뒤 정리하는지 검증한다.
- CLI `session_inspect`, `session_history`, `session_export`, `session_diagnostics` 함수는 inspect, history, export, diagnostics 표면의 로컬 코드 증거다.

## 금지 패턴

### formal event-sourcing 구현으로 과장

현재 JSONL message file을 append-only Event log라고 부르면 안 된다. message line은 Event record가 아니며 sequence, causation, correlation 정보를 갖지 않는다.

### checkpoint replay 구현으로 과장

`runtime_checkpoint`는 replay 시작점이 아니다. 현재 구현은 marker를 사용자 visible interrupted 또는 placeholder message로 물질화한다.

### 열린 턴 자동 성공

`pending_user_turn`이나 `runtime_checkpoint`가 남은 상태를 정상 assistant 완료로 처리하면 안 된다. 중단 사실이 드러나야 한다.

### 외부 저장소 전제 추가

현재 Spec 006 완료 범위에 외부 DB, 분산 저장, 운영자 전용 recovery workflow를 끼워 넣지 않는다. 기본 주체는 로컬에서 직접 설치하고 운영하는 사용자 본인이다.

## 결론

Spec 006의 현재 완료 범위는 JSONL `SessionManager` persistence와 `AgentLoop` recovery marker 처리다. 이 범위 안에서 세션 파일은 metadata header, message record, `metadata`, `messages`, `last_consolidated`를 안정적으로 저장하고, recovery marker는 열린 턴을 자동 성공시키지 않는 방향으로 사용자 visible message로 정리된다.

formal append-only Event log, sequence 기반 checkpoint replay, corruption fallback, incomplete tail discard는 현재 구현이라고 주장하지 않는다. 이들은 future session store owner가 별도 설계와 테스트로 가져가야 할 gap이다.
