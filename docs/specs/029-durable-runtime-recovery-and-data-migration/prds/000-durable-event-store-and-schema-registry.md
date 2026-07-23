# PRD 000: durable event store and schema registry

## 목표

Spec 029의 모든 replay, recovery, durable work, migration이 소비할 append-only event truth substrate를 만든다. 이 PRD는 event schema, monotonic sequence, 저장 format, reader/writer, version compatibility까지만 소유한다.

Status: Complete (Scoped). Session JSONL이나 diagnostics는 formal durable event store가 아니며, replay/checkpoint/admission은 PRD 001에서 계속 열린 범위다.

구현 evidence:

1. `shacs-session::durable_event`가 versioned record, stable kind registry, framed SHA-256 corruption checksum, bounded reader/writer를 소유한다.
2. `${data_dir}/runtime/durable-events/events.log`는 OS-level exclusive lock과 monotonic local-root sequence를 사용한다.
3. `AgentLoop`는 normal turn, workflow, command의 accepted/completed/failed fact만 기록하고 raw user/provider/channel payload를 기록하지 않는다.
4. Multi-process append, incomplete tail, sequence/checksum corruption, unknown compatibility, redaction/envelope bound, symlink, pre/partial/post-write failure injection을 Cargo tests로 고정했다.

## 범위

1. `event_id`, `sequence`, `session_id`, optional `turn_id`, `causation_id`, `correlation_id`, `kind`, `payload`, `recorded_at`, schema version을 가진 event record
2. Runtime root 아래의 append-only 저장 layout
3. Monotonic sequence 할당과 validation
4. Record checksum과 incomplete final record 식별에 필요한 framing
5. Reader, append writer, bounded scan, schema registry
6. Per-turn skill registry/body hash와 execution identity를 payload reference로 남기는 최소 provenance 규칙

## 비범위

- checkpoint와 state replay
- durable queue와 scheduler
- stored-data transform migration
- distributed log, external database, exactly-once delivery

## SPEC 입력

1. 주관 spec: `../SPEC.md`
2. 선행 구현 계약: `../../028-formal-execution-reentry-and-outcome-contracts/SPEC.md`
3. 현재 session baseline: `../../006-session-store/SPEC.md`
4. 이 PRD는 후속 모든 PRD의 필수 선행 조건이다.

## Dependency Cut

1. Durable event는 `AgentLoop` 또는 동등한 오케스트레이터가 확정한 사실만 기록한다.
2. Executor raw output, channel delivery hint, diagnostics record는 직접 truth event가 될 수 없다.
3. Session message JSONL은 유지할 수 있지만 event store를 대신하지 않는다.
4. Sequence는 단일 local runtime root 범위에서만 보장한다.

## 구현 요구사항

1. Event record는 schema version과 stable kind를 가져야 한다.
2. Sequence는 append 성공 순서와 일치하고 gap/duplicate/reorder를 reader가 감지해야 한다.
3. Append는 partial record가 유효한 event로 보이지 않도록 framing해야 한다.
4. Payload는 bounded typed envelope 또는 runtime-managed artifact reference여야 한다.
5. Secret, process handle, transport handle, raw oversized output은 payload에 넣지 않는다.
6. Unknown future schema는 writable runtime에서 조용히 무시하지 않고 compatibility result로 반환한다.
7. Reader는 전체 file을 무제한 메모리에 올리지 않고 순차 scan할 수 있어야 한다.

## 정상 시퀀스

1. Orchestrator가 session truth로 채택할 fact를 만든다.
2. Schema registry가 event kind/version을 검증한다.
3. Writer가 다음 sequence와 checksum을 부여해 append한다.
4. Append 성공 후에만 caller가 durable commit을 관찰한다.
5. Reader는 sequence 순서로 같은 record를 복원한다.

## 실패 시퀀스

1. Partial final record는 incomplete tail로 구분하고 정상 event로 반환하지 않는다.
2. Sequence duplicate/gap/reorder는 corruption evidence로 반환한다.
3. Unknown incompatible version은 normal writable start를 허용하지 않는다.
4. Payload serialization 실패는 append 이전에 실패하며 session truth를 durable하다고 광고하지 않는다.

## 검증 관점

1. Empty store, first append, multiple append, reopen 후 append를 검증한다.
2. Sequence duplicate, gap, reorder, checksum mismatch, truncated final record fixture를 둔다.
3. Unknown version과 unsupported kind가 compatibility result를 반환하는지 확인한다.
4. Secret-like payload와 oversized payload가 raw record에 남지 않는지 확인한다.
5. Crash 직전/직후 failure injection으로 committed record와 incomplete tail을 구분한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`
2. `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/Cargo.toml --workspace durable_event`

## 완료 기준

- Rust event type, writer, reader, schema registry가 존재한다.
- Sequence/framing/checksum invariant와 corruption fixture가 테스트로 고정된다.
- Event store를 replay나 exactly-once delivery로 과장하지 않는다.
