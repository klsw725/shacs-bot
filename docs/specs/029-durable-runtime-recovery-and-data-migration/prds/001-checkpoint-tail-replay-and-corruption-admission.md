# PRD 001: checkpoint tail replay and corruption admission

## 목표

PRD 000 event store 위에 checkpoint plus tail replay를 구현하고, 손상 상태에서 normal writable runtime을 열지 않는 recovery admission을 고정한다.

Status: Complete (Scoped). Checkpoint는 event prefix와 일치할 때만 채택되며, 후속 durable work/channel/child/migration/supervision 범위는 PRD 002-007에서 계속 열린다.

구현 evidence:

1. `shacs-session::durable_replay`가 versioned checksummed checkpoint, event-prefix 검증, previous checkpoint/event-zero fallback, deterministic tail reducer를 소유한다.
2. Recovery admission은 `healthy`, `recoverable`, `inspect_only`, `blocked`와 typed issue/safe-action hint를 제공하며 healthy일 때만 writable runtime을 허용한다.
3. `runtime inspect`와 diagnostics는 같은 admission을 투영하고, `runtime recover`는 event truth로 검증된 상태에서 unusable checkpoint를 evidence-preserving quarantine한 뒤 checkpoint를 다시 쓴다.
4. Checkpoint checksum/digest/forgery/ahead/fallback, incomplete tail, malformed event, sequence gap, unknown schema, reducer failure atomicity, command-less workflow response를 Cargo tests로 고정했다.
5. Workspace fmt, clippy `-D warnings`, tests, 실제 CLI inspect/recover/start admission, 5-lane goal/QA/code/security/context review가 통과했다.

## 범위

1. Checkpoint schema와 `included_sequence`
2. Stable state snapshot과 event tail replay
3. Replay stop condition과 deterministic reducer boundary
4. Checkpoint corruption, event corruption, incomplete tail, sequence gap 판정
5. `healthy`, `recoverable`, `inspect_only`, `blocked` admission state
6. `runtime inspect`와 `runtime recover`가 소비할 recovery projection

## 비범위

- durable work queue
- channel 또는 child domain recovery
- migration transform 실행
- checkpoint 단독 truth

## SPEC 입력

1. 주관 spec: `../SPEC.md`
2. 필수 선행 PRD: `000-durable-event-store-and-schema-registry.md`
3. Current recovery marker baseline은 `../../001-session-kernel/SPEC.md`와 `../../006-session-store/SPEC.md`를 소비한다.

## Dependency Cut

1. Checkpoint는 replay 최적화이며 event truth를 대체하지 않는다.
2. Replay reducer는 live provider/tool/channel effect를 실행하지 않는다.
3. Corruption 판정은 evidence를 남기며 silent repair를 하지 않는다.
4. Recovery command는 사용자가 선택할 수 있는 안전한 action만 제안한다.

## 구현 요구사항

1. Checkpoint는 schema version, included sequence, state digest, recorded time을 가진다.
2. Replay는 checkpoint 다음 sequence부터 tail을 적용한다.
3. 이전 유효 checkpoint fallback과 event-from-zero replay 가능 여부를 구분한다.
4. Malformed middle record와 incomplete final record를 다르게 처리한다.
5. Sequence gap/checksum mismatch/unknown version은 inspectable corruption class가 된다.
6. Replay 중 live side effect가 호출되지 않는 architecture assertion을 둔다.
7. Admission result는 corruption, partial migration, stale owner, pending cancellation을 후속 PRD가 확장할 수 있어야 한다.

## 정상 시퀀스

1. Runtime start가 최신 compatible checkpoint를 읽는다.
2. Checkpoint digest와 included sequence를 검증한다.
3. 이후 event tail을 순서대로 replay한다.
4. Stable state와 replay diagnostics를 만든다.
5. Healthy admission일 때만 writable runtime을 연다.

## 실패 시퀀스

1. 최신 checkpoint 손상 시 이전 checkpoint 또는 event-from-zero 가능성을 평가한다.
2. Middle event 손상/sequence gap은 normal start를 차단한다.
3. Incomplete final record는 evidence와 함께 제한적으로 tail discard 후보가 될 수 있다.
4. 복구 불가능하거나 호환성 미확정이면 inspect-only로 남긴다.

## 검증 관점

1. 정상 checkpoint + tail replay와 empty checkpoint replay를 검증한다.
2. Checkpoint checksum mismatch, malformed body, missing included sequence를 검증한다.
3. Tail truncation, malformed middle event, sequence gap, unknown event version을 검증한다.
4. Replay가 provider/tool/channel 호출을 하지 않는지 확인한다.
5. `runtime inspect`가 상태와 안전한 recovery hint를 보여주는지 확인한다.

## Cargo 검증

1. Workspace fmt/clippy 기본 gate
2. `cargo test --manifest-path crates/Cargo.toml --workspace durable_replay`
3. `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_recover`

## 완료 기준

- 정상/손상 replay matrix가 구현되고 테스트된다.
- 손상 또는 불확실 상태에서 writable runtime이 열리지 않는다.
- Inspect와 recover가 같은 typed admission result를 소비한다.
