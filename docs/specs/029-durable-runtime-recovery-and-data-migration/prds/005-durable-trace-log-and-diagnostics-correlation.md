# PRD 005: durable trace log and diagnostics correlation

## 목표

Event sequence와 durable work/channel/child identity를 redacted trace, log, diagnostics evidence에 연결한다. Trace는 replay truth를 설명하지만 대체하지 않는다.

Status: Complete (Scoped). `shacs-session/src/durable_trace.rs`의 formal durable diagnostics evidence store와 runtime work/child transition best-effort append, CLI/API/bundle redacted projection, focused corruption/retention/redaction tests가 현재 Wave 6 범위를 닫았다. Trace는 replay truth나 writable admission owner가 아니다.

## 구현 evidence

- `crates/shacs-session/src/durable_trace.rs`는 `shacs.durable_diagnostics_evidence.v1` schema family/version, checksummed frame, redaction-before-persist, bounded preview/artifact reference, corrupt-tail scan, missing-store handling, active-recovery-biased retention을 구현한다.
- `crates/shacs-core/src/runtime/durable_dispatch.rs`와 `crates/shacs-session/src/durable_child.rs`는 authoritative durable event commit 이후에만 best-effort diagnostics evidence를 append한다. Trace append/open 실패는 committed event를 rollback하지 않고 replay/admission에도 입력되지 않는다.
- `crates/shacs-cli/src/lib.rs`는 `runtime inspect`와 `runtime diagnostics`에 `durable_diagnostics_evidence`를 별도 section으로 투영하고 `truth_role=diagnostics_evidence_not_replay_truth`를 명시한다. Local API `/v1/diagnostics`는 같은 redacted diagnostics snapshot boundary를 반환한다.
- Focused tests: `cargo test --manifest-path crates/Cargo.toml -p shacs-session durable_trace`, `cargo test --manifest-path crates/Cargo.toml -p shacs-core durable_dispatch`, `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_diagnostics_projects_durable_evidence_without_raw_secret`, `cargo test --manifest-path crates/Cargo.toml -p shacs-api api_diagnostics_inspect_is_read_only_redacted_and_matches_cli_projection`.

## 범위

1. Durable trace/log/diagnostics record
2. Event sequence와 session/turn/effect/child/service correlation
3. Shared redaction boundary를 소비하는 redaction-before-persist/export 규칙
4. Bounded retention과 artifact reference
5. Runtime/session/channel/recovery inspect projection
6. Diagnostics bundle durable recovery evidence

## 비범위

- remote telemetry SaaS
- metrics warehouse
- trace 기반 replay truth
- raw provider/tool/channel payload 보존

## SPEC 입력

1. 필수 선행 PRD: `001-checkpoint-tail-replay-and-corruption-admission.md`, `002-durable-work-queue-scheduler-retry-and-cancellation.md`, `003-channel-restart-state-and-conservative-delivery.md`, `004-durable-child-task-recovery.md`
2. Current diagnostics baseline: `../../014-observability-diagnostics-and-inspection/SPEC.md`
3. Trusted-runtime session/log/trace disclosure owner는 `../../030-trusted-agent-runtime-and-operational-controls/SPEC.md`다. 이 PRD의 durable redaction boundary는 029의 닫힌 계약으로 유지한다.

## Dependency Cut

1. Event store가 truth이고 trace/log는 evidence다.
2. Trace record 누락은 replay fact를 삭제하지 않는다.
3. Diagnostics export는 stored raw secret을 전제로 하지 않는다.
4. Absolute host path와 transport/process handle은 public projection에서 제외한다.
5. 이 PRD는 새 redaction taxonomy나 secret classification을 정의하지 않는다. 현재 shared redaction boundary를 그대로 소비하며, 030의 raw-content disclosure가 이 durable projection을 약화시키지 않는다.
6. 030 완료 여부와 무관하게 029 durable writer는 redaction boundary를 반드시 호출해야 하지만, 030의 trusted-runtime credential/data-disclosure model 자체를 029 closure로 주장하지 않는다.

## 구현 요구사항

1. Record는 trace id, kind, severity, event sequence ref, correlation refs, redaction status, artifact refs, timestamp를 가진다.
2. Correlation은 `session_id`, `turn_id`, `effect_id`, `event_id`, `approval_request_id`, `child_task_id`, `app_id`, `app_process_id`, `device_id`, `port_id`, `service_correlation_id`를 선택적으로 연결한다.
3. Secret-like value는 persistence 또는 export 전에 redaction된다.
4. Oversized detail은 bounded preview와 opaque artifact locator를 사용한다.
5. Retention/rotation이 active recovery evidence를 먼저 제거하지 않게 한다.
6. Inspect는 event truth와 diagnostics evidence를 명확히 구분한다.
7. Bundle 생성 실패가 runtime truth를 변경하지 않는다.

## 정상 시퀀스

1. Runtime transition이 event sequence를 확정한다.
2. Diagnostics layer가 redacted evidence를 해당 sequence에 연결한다.
3. Durable store가 bounded record를 보존한다.
4. Inspect/bundle이 event state와 evidence를 별도 section으로 투영한다.

## 실패 시퀀스

1. Redaction 실패 시 raw detail을 쓰지 않고 safe failure diagnostic을 남긴다.
2. Trace append 실패는 event commit을 취소하거나 성공으로 바꾸지 않는다.
3. Missing correlation은 unknown으로 표시하고 임의 연결하지 않는다.
4. Corrupt diagnostics tail은 replay truth corruption으로 오인하지 않는다.

## 검증 관점

1. Event-work-channel-child correlation chain과 approval/app/device/port optional foreign ref를 end-to-end 검증한다.
2. Secret, token, absolute path, raw payload가 persisted/exported record에 없는지 확인한다.
3. Trace missing/corrupt/rotated 상태에서도 replay truth가 유지되는지 확인한다.
4. Bundle과 CLI/API diagnostics가 같은 projection semantics를 갖는지 확인한다.
5. Bounded retention과 active recovery evidence 보호를 검증한다.

## 완료 기준

- Durable diagnostics가 event sequence와 correlation된다.
- Redaction과 bounded retention이 테스트된다.
- Trace가 session/event truth owner가 아니다.
