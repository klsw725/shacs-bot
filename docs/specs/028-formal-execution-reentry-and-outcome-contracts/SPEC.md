# 028. formal execution reentry and outcome contracts 아키텍처 명세

Status: Open

Origin specs: 002, 003, 004, 006, 007, 011

## 목적

이 문서는 기존 002, 003, 004, 006, 007, 011에서 current architecture closure 밖으로 남긴 실행 재진입과 outcome 계약을 새 owner 범위로 모은다.

목표는 provider, tool, subagent 같은 외부 실행 경계가 결과를 어떻게 식별하고, 어떤 outcome으로 정규화하며, 어떤 command 또는 runtime entrypoint로 다시 돌아오는지 설명 가능한 계약으로 고정하는 것이다. 이 문서는 현재 구현 완료를 주장하지 않는다. 기존 런타임을 하나의 거대한 `Command`/`Event`/`Effect` enum으로 다시 쓰라는 요구도 아니다.

028은 필요한 곳에만 shared optional identifier와 envelope를 추가하는 점진적 계약이다. 현재 `AgentLoop`, `AgentRunner`, `RuntimeToolExecutor`, `SubagentRuntime`, `SessionManager` 경계를 존중하면서, 비동기 실행, 취소, timeout, late result, idempotency, artifact outcome을 같은 언어로 설명할 수 있게 한다.

## 현재 구현 baseline

현재 구현은 다음 범위까지 인정한다.

1. 002는 `Command`, `Event`, `Effect`를 권한 경계를 설명하는 개념어로 정리했고, 공용 Rust enum이나 trait가 없다는 사실을 closure 범위에 포함했다.
2. 003은 provider 호출을 `AgentRunner`, `ProviderRequest`, `ProviderClient`, `LlmResponse`, `ProviderEvent` 경계로 설명한다. shared `effect_id`, `correlation_id`, provider outcome, provider reentry command는 없다.
3. 004는 tool 실행을 `RuntimeToolCall`, `RuntimeToolExecutor`, `RuntimeToolMessage`, `RuntimeToolExecutionReport`, `ToolEvent` 경계로 설명한다. formal `RunToolEffect`, common outcome envelope, late result rule은 없다.
4. 006은 session JSONL과 recovery marker를 current session persistence로 인정한다. message JSONL은 formal event log가 아니다.
5. 007은 retry, abort, stale discard가 현재 runtime 여러 경계에 흩어져 있음을 인정한다. formal `LateResultDecision`, timeout table, policy-owned retry decision은 없다.
6. 011은 subagent의 `SpawnEnvelope`, `ChildResultEnvelope`, correlation stale discard, synthetic inbound reentry를 current mapping으로 인정한다. typed inherited snapshot, wall clock timeout, full merge policy, durable child recovery는 없다.

이 baseline은 새 작업의 출발점이다. 028 closure 전까지 위 gap을 구현된 것으로 말하면 안 된다.

## owned open scope

028이 소유하는 열린 범위는 다음이다.

1. provider, tool, subagent 실행에 선택적으로 붙일 shared identifiers. 예: `session_id`, `turn_id`, `effect_id`, `correlation_id`, `causation_id`, `attempt_id`, `child_task_id`.
2. 실행 요청 envelope와 결과 envelope의 최소 공통 필드. 단, 모든 실행 종류를 같은 payload enum으로 강제하지 않는다.
3. provider outcome. 예: completed, tool requested, failed, timed out, cancelled, stale, ignored.
4. tool outcome. 예: completed, failed, timed out, cancelled, interrupted, skipped, stale.
5. subagent outcome. 예: completed, failed, timed out, cancelled, stale, retry requested, merge rejected.
6. reentry command 또는 reentry entrypoint 계약. Provider, tool, subagent가 같은 이름의 command를 가져야 한다는 뜻은 아니며, 결과가 오케스트레이터 권한 경계로 돌아오는 조건을 뜻한다.
7. timeout, cancel, late result, idempotency 판단에 필요한 outcome field와 decision field.
8. artifact outcome envelope. 큰 text, file, binary, media, diagnostics artifact를 provider context나 session history에 직접 밀어 넣지 않고 안전한 reference로 남기는 계약.
9. 현재 subagent correlation model을 provider/tool에도 확장할 필요가 생겼을 때의 최소 기준.
10. 구현 증거, 테스트 이름, inspect 출력에서 current runtime과 formal contract를 구분하는 용어.

## invariants

1. 세션 visible state의 최종 반영 권한은 계속 `AgentLoop` 또는 그와 동등한 `MainOrchestrator` 경계에 있다.
2. Provider, tool, subagent executor는 실행할 수 있지만 session truth를 직접 확정할 수 없다.
3. Shared identifier는 correlation과 idempotency를 돕는 도구이지, 그 자체로 결과 채택을 승인하지 않는다.
4. Late result는 관찰될 수 있지만, 종료된 turn이나 superseded effect의 session truth를 뒤집을 수 없다.
5. Timeout과 cancellation은 success나 generic failure로 뭉개면 안 된다. 사용자가 볼 수 있는 outcome 차이를 남겨야 한다.
6. Artifact reference는 runtime-managed artifact root와 redaction, safety rule을 따라야 한다.
7. Reentry는 state patch가 아니다. 결과는 오케스트레이터가 다시 판단할 수 있는 사실로 돌아와야 한다.
8. 같은 실행을 재시도하거나 재전달할 수 있다면 idempotency key 또는 attempt identity가 설명 가능해야 한다.
9. Existing `SpawnEnvelope`와 `ChildResultEnvelope` correlation model은 보존한다.
10. Formal contract를 추가하더라도 현재 runner 중심 구조를 무조건 폐기하면 안 된다.

## Must Have

1. Provider, tool, subagent outcome을 각각 표현하되 공통 lifecycle 단어를 맞춘다.
2. `effect_id`와 `correlation_id`를 도입한다면 어느 경계에서 생성하고 어느 경계에서 검증하는지 명시한다.
3. `causation_id`와 `attempt_id`가 필요한 경우, retry와 idempotency 판단에만 필요한지, event replay에도 필요한지 구분한다.
4. Reentry command 또는 entrypoint는 세션 상태 patch가 아니라 outcome fact를 운반해야 한다.
5. Timeout, cancellation, late result, stale result를 테스트 가능한 decision으로 남긴다.
6. Tool result가 큰 text 또는 file을 만들 때 artifact outcome envelope로 저장, redaction, truncation, provider handoff 방식을 분리한다.
7. Provider streaming delta와 final provider outcome을 구분한다.
8. `AskUserInterrupt` 같은 paused outcome을 completed 또는 failed로 숨기지 않는다.
9. Subagent result는 기존 parent/child correlation을 통과해야 하며, mismatch는 stale로 남아야 한다.
10. Inspect와 diagnostics가 pending effect, terminal outcome, stale discard, artifact reference를 읽을 수 있어야 한다.

## Must Not Have

1. 전체 런타임을 하나의 거대한 `Command`/`Event`/`Effect` enum으로 다시 쓰는 요구.
2. Provider, tool, subagent payload를 하나의 mega enum variant로 강제하는 설계.
3. 외부 executor가 `Command::ApplyStatePatch` 같은 형태로 session truth를 직접 수정하는 경로.
4. Timeout 뒤 도착한 success가 이전 timeout outcome을 조용히 덮어쓰는 동작.
5. Cancellation을 단순 failure text로만 저장해 사용자가 취소와 실패를 구분하지 못하는 동작.
6. Artifact content를 무제한 provider context나 session history에 직접 넣는 동작.
7. Redaction 전 payload, secret, process handle, transport handle을 outcome envelope에 넣는 동작.
8. 현재 구현에 없는 shared identifier, reentry command, outcome envelope를 이미 구현됐다고 쓰는 문서.
9. Provider adapter나 tool executor가 retry, abort, late result 채택을 독자적으로 최종 결정하는 구조.
10. Subagent stale discard 규칙을 약화하는 일반화.

## acceptance criteria

028은 아래 조건을 모두 만족할 때 닫을 수 있다.

1. Provider, tool, subagent outcome contract가 Rust 타입 또는 명시적인 boundary type으로 구현되어 있다.
2. 필요한 실행 경계에 `effect_id` 또는 이에 준하는 correlation identity가 생성, 전달, 검증된다.
3. Timeout, cancellation, stale, late result, duplicate delivery에 대한 decision path가 테스트로 고정되어 있다.
4. Reentry entrypoint는 executor 결과를 오케스트레이터 판단 경계로 돌려보내며, executor가 session truth를 직접 쓰지 않는다.
5. Artifact outcome envelope는 text, JSON, file 또는 binary reference를 안전하게 표현하고 redaction과 runtime artifact root 규칙을 따른다.
6. Provider streaming delta와 final outcome이 세션 truth에서 구분된다.
7. Tool skipped, interrupted, fatal, recoverable failure outcome이 구분된다.
8. Subagent completed, failed, timed out, cancelled, stale 결과가 existing correlation invariant와 양립한다.
9. Inspect 또는 diagnostics surface에서 pending effect와 terminal outcome을 확인할 수 있다.
10. 구현 문서가 002, 003, 004, 006, 007, 011 closure 범위와 028 closure 범위를 혼동하지 않는다.

## handoff table back to source specs

| Source spec | 028이 인수하는 열린 작업 | 028에서의 closure 방향 |
| --- | --- | --- |
| 002 | Optional shared identifier, effect envelope, provider/tool async reentry, late result idempotency | 공용 mega enum이 아니라 필요한 실행 경계별 identifier와 outcome fact로 정의한다 |
| 003 | Provider invocation outcome, model reentry command, provider timeout/cancel/late result correlation | `AgentRunner` 중심 구조와 양립하는 provider outcome 및 final reentry boundary를 구현한다 |
| 004 | Formal `RunToolEffect`, tool outcome, permission snapshot guard, timed out/cancelled state, artifact outcome | Tool executor가 직접 상태를 쓰지 않는 outcome envelope와 artifact reference contract를 구현한다 |
| 006 | Runtime checkpoint에 남은 pending effect와 outcome의 formal 연결 | Event log와 replay 자체는 029가 소유하며, 028은 effect outcome 식별과 reentry 사실만 소유한다 |
| 007 | `LateResultDecision`, timeout policy table, retry/abort decision surface | Timeout, cancel, stale, duplicate result decision이 outcome contract에 남도록 한다 |
| 011 | Subagent timeout, retry, full merge decision input, durable recovery와 연결되는 result identity | 기존 `SpawnEnvelope`와 `ChildResultEnvelope`를 보존하며 typed outcome과 reentry contract를 맞춘다 |

## implementation evidence required for closure

Closure를 주장하려면 최소한 아래 증거가 있어야 한다.

1. Rust 타입 또는 trait boundary가 있는 파일 목록. Provider, tool, subagent outcome과 shared identifier가 어디에 있는지 보여야 한다.
2. Provider final result, provider timeout, provider cancellation, provider late result discard 테스트.
3. Tool completed, failed, timed out, cancelled, interrupted, skipped, artifact result 테스트.
4. Subagent completed, failed, timed out, cancelled, stale, duplicate result 테스트.
5. Reentry path가 `AgentLoop` 또는 동등한 오케스트레이터 boundary를 통과한다는 integration test.
6. Executor가 session store를 직접 수정하지 않는다는 regression test 또는 architecture assertion.
7. Artifact outcome이 runtime-managed root와 redaction rule을 통과한다는 테스트.
8. Inspect 또는 diagnostics output에서 pending effect, terminal outcome, stale discard, artifact reference를 확인한 CLI 또는 local API evidence.
9. Migration note. 기존 current architecture 문서가 완료로 말한 범위를 바꾸지 않고, 028의 새 구현 범위만 추가했음을 설명해야 한다.
10. `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`, `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`, `cargo test --manifest-path crates/Cargo.toml --workspace` 통과 기록.
