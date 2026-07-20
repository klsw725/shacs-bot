# subagent runtime 아키텍처 명세

Status: Complete (Scoped)

Implemented scope: 현재 구현은 `SubagentRuntime`, spawn tool wiring, child lifecycle and correlation, stale discard, synthetic inbound reentry, bounded parallelism, cancellation cleanup, and restricted child tool registry를 current subagent runtime scope로 지원한다.

Open work moved to: [028 formal execution reentry and outcome contracts](../028-formal-execution-reentry-and-outcome-contracts/SPEC.md), [029 durable runtime recovery and data migration](../029-durable-runtime-recovery-and-data-migration/SPEC.md), [030 policy, permission, redaction, and containment model](../030-policy-permission-redaction-and-containment-model/SPEC.md), [031 ui projection, diagnostics, and release evidence parity](../031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md)

Not carried forward: remote agent fleet management, agent marketplace, agent billing, multi-user shared task board, remote team inbox, agent-to-agent direct negotiation, distributed consensus는 제품 범위에 넣지 않는다.

## 문서 목적

이 문서는 `shacs-bot`의 subagent runtime을 현재 구현과 앞으로 남은 작업으로 나누어 정리한다. Spec 011은 2026-05-17 현재 아키텍처 매핑 기준으로 종료됐다. 현재 코드는 child identity, lifecycle, spawn, stale discard, synthetic reentry, parallelism, tool registry 제한을 갖고 있지만, formal inherited snapshot, budget, timeout, retry, full merge policy가 완성된 상태는 아니다.

이 문서의 현재 역할은 다음과 같다.

1. 현재 구현된 subagent runtime 경계를 정확히 설명한다.
2. 현재 아키텍처 매핑으로 인정할 수 있는 범위를 고정한다.
3. current architecture 기준 종료 범위와 future formal subagent runtime 작업을 분리한다.

`shacs-bot`은 `self-hosted/personal-use` 성격의, 사용자가 직접 설치하고 운영하는 개인용 런타임을 기본으로 본다. 따라서 subagent runtime의 핵심은 단일 사용자 작업을 안전하게 병렬 보조하고, 결과를 다시 부모 오케스트레이터 아래로 회수하는 것이다. 원격 agent fleet, marketplace, billing, shared task board를 기본 제품 범위로 보지 않는다.

## 상위 기준과의 관계

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 전제로 한다.

현재 완료 판정은 하나의 완성된 formal child task contract가 runtime 전체에 적용됐다는 뜻이 아니다. 완료의 의미는 `SubagentRuntime`이 child 상태와 result correlation을 관리하고, CLI adapter가 spawn tool과 background 실행을 연결하며, tool registry와 execution config가 child 실행 범위를 제한하는 현재 경계를 Spec 011의 current architecture로 문서화했다는 뜻이다.

따라서 이 문서는 다음 두 층을 구분한다.

1. 현재 아키텍처 매핑: 이미 존재하는 identity, lifecycle, spawn path, stale discard, synthetic inbound, bounded parallelism, status/progress snapshot, cancellation cleanup, tool registry restriction.
2. future formal model: 아직 도입되지 않은 typed inherited snapshots, budget evaluator, wall clock timeout enforcement, retry orchestration, full merge decision layer, durable recovery.

## 범위

현재 문서에서 다루는 범위는 다음과 같다.

1. 현재 구현된 child identity와 lifecycle tracking.
2. spawn tool에서 subagent runtime으로 이어지는 실행 경로.
3. parent/child correlation과 stale discard 규칙.
4. synthetic inbound reentry와 직접 session write 금지.
5. bounded parallelism, status/progress snapshot, cancellation cleanup.
6. subagent tool registry 제한과 workspace-bound execution config.
7. 현재 테스트 증거로 확인되는 동작.
8. 아직 future work로 남겨야 하는 formal subagent runtime 항목.

이 문서는 다음을 현재 기능으로 선언하지 않는다.

1. formal `ChildTaskId`, `InheritedContextSnapshot`, `InheritedPolicySnapshot`, `InheritedSafetySnapshot`, budget snapshot 타입 경계가 JSON snapshot 이상으로 완성되었다는 주장.
2. 실제 `spawn_sequence` 단조 증가 tracking이 완성되었다는 주장.
3. nested subagent depth ceiling이 완성되었다는 주장.
4. same-scope dedupe와 rationale policy가 adopted scope handling 이상으로 완성되었다는 주장.
5. formal inherited policy, safety, budget evaluator가 완성되었다는 주장.
6. `timeout_ms` 기반 wall clock timeout enforcement가 완성되었다는 주장.
7. timed out, failed child retry orchestration이 완성되었다는 주장.
8. full merge, summary-only, failure-fact, abort parent를 포괄하는 `MainOrchestrator` decision layer가 완성되었다는 주장.
9. process restart 이후 durable child task recovery가 완성되었다는 주장.
10. 현재 synthetic announcement와 status report를 넘어서는 user-visible lifecycle projection이 완성되었다는 주장.

## 현재 구현 요약

현재 구현은 `SubagentRuntime` 중심의 runtime mapping이다.

1. `crates/shacs-core/src/runtime/subagent.rs`는 `SubagentState`, `ChildResultStatus`, `SpawnEnvelope`, `ChildResultEnvelope`, `MergeDecision`, `SubagentStatus`, `SubagentProgressUpdate`, `SubagentRuntimeConfig`, `SubagentExecutionConfig`, `SubagentRuntime`을 제공한다.
2. 같은 파일의 `spawn_from_request`, `register_spawn`, `mark_running`, `cancel_by_session`, `running_count_by_session`, `snapshot`, `update_progress`는 child lifecycle과 status/progress snapshot을 관리한다.
3. `classify_result`, `classify_active_result`, `finish_child`, `publish_child_result`는 child 결과를 분류하고, active child를 닫고, synthetic inbound로 되돌리는 경로를 제공한다.
4. `run_spawn`, `spawn_and_run_background`는 child 실행과 background execution을 runtime에 연결한다.
5. `synthetic_inbound_for_result`는 child 결과를 직접 session write로 저장하지 않고 synthetic inbound로 변환한다.
6. `build_subagent_tool_registry`는 parent-only `spawn`을 제외하고 workspace, side-effect, exec, web 설정에 맞게 child tool registry를 제한한다.
7. `correlation_decision`은 `child_task_id`, `session_id`, `parent_turn_id`, `spawn_effect_id`를 확인하고 mismatch를 stale로 분류한다.
8. `synthetic_command_for`는 `completed`, `failed`, `timed_out`, `cancelled`, `stale`을 synthetic subagent command 또는 progress observed로 매핑한다.
9. `crates/shacs-core/src/tools/spawn.rs`는 `SpawnRequest`, `SubagentSpawner`, `SpawnTool`, thread-local spawn context를 제공하며, public tool parameter는 `task`와 `label`만 둔다.
10. `crates/shacs-cli/src/lib.rs`는 CLI adapter에서 `SpawnTool`을 등록하고 `spawn_and_run_background`를 연결하며, loop config와 adapter settings에서 `SubagentExecutionConfig`를 만든다.

현재 구현은 subagent runtime의 핵심 통로를 갖고 있다. 이것이 Spec 011의 current architecture 기준 완료 범위다.

## 현재 아키텍처 매핑 기준

Spec 011의 현재 매핑은 다음 조건을 만족하는 구현 증거로 인정한다.

1. child identity와 lifecycle은 runtime state로 추적되어야 한다.
2. spawn tool 호출은 thread-local context와 `SubagentSpawner`를 거쳐 `SubagentRuntime`으로 들어가야 한다.
3. parent/child correlation은 `child_task_id`, `session_id`, `parent_turn_id`, `spawn_effect_id`를 기준으로 확인되어야 한다.
4. correlation mismatch와 inactive child result는 stale로 분류되어야 한다.
5. child result는 direct session write가 아니라 synthetic inbound reentry로 부모 runtime에 들어가야 한다.
6. stale inbound는 session content로 persistence되면 안 된다.
7. parallelism은 runtime config의 active child ceiling으로 제한되어야 한다.
8. status와 progress는 `snapshot`과 `update_progress`로 관찰 가능해야 한다.
9. background child execution은 `spawn_and_run_background` 경로로 연결되어야 한다.
10. subagent tool registry는 parent-only `spawn`을 제외하고 workspace-bound execution config를 따라야 한다.
11. parent session cancellation은 active child cleanup을 수행해야 한다.

이 매핑은 future formal model을 일부 대체해 주는 근거다. formal inherited snapshot과 full merge policy가 완성되었다는 뜻은 아니다.

## 핵심 정의의 현재 상태

### subagent

subagent는 부모 턴이 가진 작업을 보조하기 위해 runtime이 실행하는 제한된 child executor다. 현재 구현에서는 별도 세션 진실 원천이 아니라 `SubagentRuntime`에 등록된 child task와 background execution 경로로 표현된다.

### child task

child task는 child 실행의 identity, lifecycle, progress, result correlation을 추적하는 단위다. 현재 구현은 `SubagentState`, `SpawnEnvelope`, `ChildResultEnvelope`, `SubagentStatus`로 이 역할을 수행한다. formal `ChildTaskId` newtype 경계와 durable recovery는 아직 future work다.

### spawn envelope

spawn envelope는 runtime이 child 실행을 추적하고 상관관계를 검증하기 위한 현재 실행 record다. 현재 envelope는 JSON snapshot 기반의 context와 실행 설정을 포함하지만, formal `InheritedContextSnapshot`, `InheritedPolicySnapshot`, `InheritedSafetySnapshot`, budget snapshot 타입이 완성된 것은 아니다.

### merge

현재 구현의 merge는 full `MainOrchestrator` decision layer라기보다 child result를 synthetic inbound로 되돌리고, matching result를 accepted summary로 처리하는 runtime mapping에 가깝다. full merge, summary-only, failure-fact, retry, abort parent policy는 future work다.

### stale child result

stale child result는 correlation mismatch, inactive child, closed flow에 의해 현재 부모 흐름에 받아들일 수 없는 결과다. 현재 구현은 mismatch를 stale로 분류하고, stale inbound가 session content로 persisted되지 않게 한다.

### synthetic inbound reentry

synthetic inbound reentry는 child result를 session에 직접 쓰지 않고 runtime 진입점으로 되돌리는 경로다. 현재 구현은 `synthetic_inbound_for_result`, `synthetic_command_for`, `publish_child_result`, `finish_child`로 이 원칙을 반영한다.

## child identity와 lifecycle의 현재 상태

현재 구현에서 필요한 식별자는 다음 의미로 다뤄진다.

1. `session_id`.
2. `parent_turn_id`.
3. `child_task_id`.
4. `spawn_effect_id`.
5. `subagent_kind` 또는 label에 해당하는 실행 구분.

`correlation_decision`은 `child_task_id`, `session_id`, `parent_turn_id`, `spawn_effect_id`를 모두 확인한다. mismatch는 stale로 분류된다. 이것은 현재 필요한 parent/child correlation의 핵심이다.

아직 없는 것은 실제 `spawn_sequence` 단조 증가 tracking이다. 같은 parent turn 안에서 sequence를 formal하게 비교하고 retry precedence를 판단하는 정책은 future work로 둔다.

현재 lifecycle은 다음 수준까지 매핑된다.

1. spawn request를 `SpawnRequest`와 `spawn_from_request`로 runtime envelope에 변환한다.
2. `register_spawn`으로 active child를 등록한다.
3. `mark_running`과 `update_progress`로 running/progress 상태를 관찰한다.
4. `finish_child`와 `publish_child_result`로 child를 닫고 synthetic inbound를 발행한다.
5. `cancel_by_session`으로 session 단위 cancellation cleanup을 수행한다.

formal state machine과 모든 terminal transition assertion은 더 보강할 수 있다. 현재 문서는 이를 완료로 주장하지 않는다.

## spawn tool와 runtime 연결

현재 public spawn tool surface는 작다.

1. `SpawnRequest`의 public tool parameter는 `task`와 `label`이다.
2. `SpawnTool`은 thread-local spawn context를 통해 현재 runtime context와 연결된다.
3. `SubagentSpawner`가 tool 호출을 runtime spawn으로 위임한다.
4. CLI adapter는 `SpawnTool`을 registry에 등록하고 `spawn_and_run_background`를 실제 background 실행으로 연결한다.

이 구조는 personal-use runtime에 맞다. 사용자가 직접 운영하는 로컬 환경에서 필요한 것은 복잡한 agent marketplace가 아니라, parent turn이 제한된 child work를 만들고 그 결과를 설명 가능하게 회수하는 경로다.

## inherited context, policy, safety, budget의 현재 상태

현재 구현에는 JSON snapshot과 execution config 기반의 부분 상속 제한이 있다.

1. child는 spawn envelope에 담긴 context snapshot을 받는다.
2. `SubagentExecutionConfig`는 workspace, side-effect, exec, web 관련 실행 제한을 child tool registry 구성에 반영한다.
3. `build_subagent_tool_registry`는 parent-only `spawn`을 제외하고 child가 쓸 수 있는 tool surface를 제한한다.
4. CLI adapter는 loop config와 adapter settings에서 child execution config를 만든다.

이것은 current architecture mapping으로 인정할 수 있다. 하지만 formal inherited `PolicySnapshot`, `SafetySnapshot`, input/output budget snapshot은 아니다. 특히 `inherited_safety_snapshot: { inherits_parent_safety: true }` 같은 placeholder를 완성된 safety model로 보면 안 된다.

future work는 다음이다.

1. 부모 context snapshot을 typed `InheritedContextSnapshot`으로 분리한다.
2. parent policy와 safety ceiling을 child execution에 formal하게 전달한다.
3. input/output budget과 `timeout_ms`를 실제 evaluator와 enforcement에 연결한다.
4. nested subagent depth ceiling을 추가한다.
5. same-scope dedupe와 rationale policy를 adopted scope handling 이상으로 정의한다.

## result, stale, synthetic reentry의 현재 상태

현재 child result는 `ChildResultEnvelope`와 `ChildResultStatus`로 표현된다. `classify_result`와 `classify_active_result`는 result status와 correlation을 확인하고, active child에 대한 결과만 정상 처리한다.

현재 synthetic command mapping은 다음 의미를 갖는다.

1. `completed`는 synthetic subagent completed command로 변환된다.
2. `failed`는 synthetic subagent failed command로 변환된다.
3. `timed_out`는 synthetic subagent timed out command로 변환된다.
4. `cancelled`는 synthetic subagent cancelled command로 변환된다.
5. `stale`은 merge 대상이 아니라 progress observed 성격으로 다뤄진다.

현재 구현은 stale result를 publish하거나 active child를 닫는 경로로 잘못 처리하지 않도록 검증한다. 하지만 full merge policy는 아직 future work다.

## parallelism, cancellation, status의 현재 상태

현재 runtime은 bounded parallelism을 갖는다.

1. `SubagentRuntimeConfig`는 active child limit을 둔다.
2. `running_count_by_session`과 active child tracking은 session 기준 병렬 수를 계산한다.
3. limit을 초과하는 spawn은 거절된다.

현재 cancellation cleanup은 session 단위로 동작한다.

1. `cancel_by_session`은 active child를 정리한다.
2. 실행 전 cancellation은 announcement 없이 cleanup될 수 있다.
3. stale result는 session content로 persisted되지 않는다.

현재 status/progress 관찰은 다음으로 매핑된다.

1. `snapshot`은 현재 active child 상태를 보여 준다.
2. `SubagentProgressUpdate`와 `update_progress`는 partial progress, completed step, failure 정보를 format할 수 있다.
3. loop lifecycle은 structured status report를 낸다.

user-visible lifecycle projection은 여기까지가 현재 mapping이다. 더 풍부한 UI projection은 future work다.

## 현재 검증 증거

현재 구현을 뒷받침하는 테스트 증거는 다음 이름들로 정리한다.

Identity, correlation, stale:

1. `subagent_result_with_wrong_child_id_is_stale`.
2. `subagent_result_with_matching_parent_and_child_accepts_summary`.
3. `subagent_result_with_wrong_parent_session_is_stale`.
4. `subagent_stale_result_does_not_publish_or_close_active_child`.
5. `subagent_stale_inbound_is_not_persisted_as_session_content`.

Lifecycle, cancellation, execution:

1. `subagent_spawn_registers_active_task_and_cancels_by_session`.
2. `subagent_spawn_inherits_snapshot_contract`.
3. `subagent_finish_publishes_synthetic_inbound_and_closes_active_task`.
4. `subagent_run_spawn_executes_agent_and_publishes_result`.
5. `subagent_cancel_before_run_cleans_without_announcement`.
6. `subagent_parallelism_limit_rejects_excess_children`.

Tool registry, progress, adapter:

1. `subagent_tool_registry_excludes_parent_only_tools`.
2. `subagent_partial_progress_formats_completed_steps_and_failure`.
3. `spawn_tool_can_delegate_to_subagent_runtime`.
4. `loop_lifecycle_reports_structured_status`.
5. `loop_preserves_channel_chat_and_session_key_in_tool_context`.
6. `adapter_wires_exec_env_to_context_and_subagents`.

이 증거는 current architecture mapping 기준 Spec 011 종료의 근거다. formal inherited safety, timeout, retry, full merge, durable recovery 완료 증거로 읽으면 안 된다.

## Future gaps

다음 항목은 현재 blocker가 아니라 future subagent runtime 작업이다.

1. formal `ChildTaskId`, `InheritedContextSnapshot`, `InheritedPolicySnapshot`, `InheritedSafetySnapshot`, budget snapshot 타입 경계.
2. JSON snapshot 이상으로 typed inheritance contract를 세우는 작업.
3. 실제 `spawn_sequence` 단조 증가 tracking.
4. nested subagent depth ceiling.
5. same-scope dedupe와 rationale policy.
6. formal inherited policy, safety, budget evaluation.
7. `timeout_ms` 기반 wall clock timeout enforcement.
8. timed out 또는 failed child retry orchestration.
9. full merge, summary-only, failure-fact, abort parent policy를 포함한 `MainOrchestrator` decision layer.
10. process restart 이후 durable child task recovery.
11. 현재 synthetic announcement와 status report를 넘어서는 user-visible lifecycle projection.

## 명시적 비범위

다음은 이 제품에서 지금 필요하지 않다.

1. remote agent fleet management.
2. agent marketplace.
3. agent billing 또는 cost distribution.
4. multi-user task ownership 또는 shared task board.
5. remote team inbox.
6. agent-to-agent direct negotiation protocol.
7. distributed consensus 또는 coordinator-less orchestration.

이 항목들은 current Spec 011 작업의 품질 기준이 아니다. `shacs-bot`의 기준은 개인이 직접 운영하는 runtime에서 child result를 설명 가능하게 제한하고 회수하는 것이다.

## Open risks

1. current snapshot은 JSON 중심이므로 future typed snapshot과 표현이 어긋날 수 있다.
2. timeout과 retry가 formal orchestration으로 연결되지 않아 timed out child의 후속 정책은 아직 제한적이다.
3. full merge decision layer가 없으므로 현재 accepted summary mapping을 완성된 merge policy로 오해할 수 있다.
4. durable recovery가 없으므로 process restart 뒤 child task 상태 복구는 future work로 남아 있다.
5. current user-visible projection은 synthetic announcement와 status report 중심이라 상세 lifecycle UX는 아직 부족하다.

## 종료 기준

현재 문서 정합성 기준은 다음이다.

1. Spec 011과 PRD가 현재 구현을 formal subagent runtime 완료로 주장하지 않는다.
2. child identity, lifecycle, spawn path, correlation, stale discard, synthetic reentry, parallelism, progress/status, tool registry restriction, cancellation cleanup을 current architecture mapping으로 설명한다.
3. inherited context, policy, safety, budget, timeout, retry, merge, durable recovery, lifecycle projection의 future gap을 현재 구현과 분리한다.
4. self-hosted/personal-use 제품 관점을 유지하고 multi-user/admin/operator platform scope를 도입하지 않는다.
5. verification evidence는 current architecture 종료 근거로 쓰되, formal future model 완료 증거로 쓰지 않는다.

## 결론

현재 `shacs-bot`의 subagent runtime은 이미 중요한 실행 경계를 갖고 있다. child를 등록하고, background로 실행하고, 결과를 correlation으로 검증하고, stale을 버리고, synthetic inbound로 부모 runtime에 되돌리는 경로가 있다.

동시에 Spec 011의 종료는 완료된 formal contract를 뜻하지 않는다. 남은 작업은 typed inherited snapshot, budget과 timeout enforcement, retry orchestration, full merge decision, durable recovery를 현재 runtime 위에 정확히 얹는 것이다.

이 구분이 지켜져야 subagent 기능이 개인용 self-hosted runtime의 단순성과 설명 가능성을 해치지 않는다.
