# PRD 000. child task spawn and merge

## 목표

이 PRD는 `docs/specs/011-subagent-runtime/SPEC.md`의 현재 아키텍처 매핑을 실행 계획으로 정리한다. 목표는 현재 구현된 child task tracking, spawn tool path, correlation, stale discard, synthetic inbound, bounded parallelism, tool registry restriction을 정확히 문서화하고, 2026-05-17 기준 Spec 011을 current architecture mapping으로 닫는 것이다.

이번 PRD의 완료 선언은 formal `ChildTaskId`, inherited policy/safety/budget snapshot, timeout enforcement, retry orchestration, full merge decision layer, durable child recovery가 완성됐다는 뜻이 아니다. 현재 코드가 제공하는 subagent runtime 핵심 통로를 Spec 011의 current architecture 범위로 인정한다는 뜻이다.

`shacs-bot`은 사용자가 직접 설치하고 운영하는 `self-hosted/personal-use` runtime이다. 이 PRD는 원격 agent 운영 플랫폼이 아니라, 부모 턴이 제한된 child work를 만들고 결과를 안전하게 회수하는 현재 구조에 맞춘다.

## SPEC 입력

1. 주관 spec: `docs/specs/011-subagent-runtime/SPEC.md`.
2. 선행 기준: `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`.
3. 교차 의존: `docs/specs/012-runtime-services/SPEC.md`, `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`, `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`, `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`.

## Dependency Cut

현재 dependency cut은 formal child task contract가 아니라 current runtime mapping 기준으로 잡는다.

1. 007은 spawn 판단과 merge policy의 future 위치를 제공하지만, 현재 구현은 full `MainOrchestrator` merge decision layer가 아니다.
2. 009는 child에 내려갈 context가 부모 transcript 전체 복제가 아니어야 한다는 기준을 제공하지만, 현재 구현은 typed `InheritedContextSnapshot`이 아니라 JSON snapshot 중심이다.
3. 010은 child 실행 제한이 부모 boundary를 넓히면 안 된다는 기준을 제공하지만, 현재 구현은 formal inherited `SafetySnapshot`이 아니라 `SubagentExecutionConfig`와 tool registry restriction이다.
4. 012는 child result가 direct session write가 아니라 synthetic inbound로 돌아오는 runtime 경계를 제공한다.
5. 013과 014는 status/progress projection과 diagnostics의 future 위치를 제공하지만, 현재 user-visible lifecycle projection은 synthetic announcement와 status report 중심이다.

## 범위

이번 PRD의 현재 범위는 다음이다.

1. current child identity와 lifecycle tracking 정리.
2. `SpawnTool`에서 `SubagentRuntime`으로 이어지는 spawn path 정리.
3. parent/child correlation과 stale discard 정리.
4. synthetic inbound reentry와 direct session write 금지 정리.
5. bounded parallelism과 cancellation cleanup 정리.
6. status/progress snapshot 정리.
7. subagent tool registry restriction과 workspace-bound execution config 정리.

## 범위 제외

다음은 현재 구현 완료로 쓰지 않는다.

1. formal `ChildTaskId`, `InheritedContextSnapshot`, `InheritedPolicySnapshot`, `InheritedSafetySnapshot`, budget snapshot 타입.
2. JSON snapshot을 넘어서는 formal inherited policy, safety, budget evaluation.
3. 실제 `spawn_sequence` monotonic tracking.
4. nested subagent depth ceiling.
5. same-scope dedupe와 rationale policy.
6. `timeout_ms` 기반 wall clock timeout enforcement.
7. timed out 또는 failed child retry orchestration.
8. full merge, summary-only, failure-fact, abort parent policy를 포괄하는 `MainOrchestrator` decision layer.
9. durable child task recovery across process restarts.
10. current synthetic announcements/status reports를 넘어서는 user-visible lifecycle projection.

다음은 제품 범위 밖이다.

1. remote agent fleet management.
2. agent marketplace.
3. agent billing 또는 cost distribution.
4. multi-user task ownership 또는 shared task board.
5. remote team inbox.
6. agent-to-agent direct negotiation protocol.
7. distributed consensus 또는 coordinator-less orchestration.

## 현재 구현 상태

2026-05-17 기준 Spec 011은 현재 아키텍처 매핑으로 종료한다. 이 종료는 아래 구현과 테스트가 child identity, lifecycle, spawn, stale discard, synthetic inbound, bounded parallelism, tool registry restriction, cancellation cleanup을 설명하고 검증한다는 뜻이다. formal inherited snapshot, timeout, retry, full merge, durable recovery 완료를 뜻하지 않는다.

### 이미 반영된 것

1. `crates/shacs-core/src/runtime/subagent.rs`는 `SubagentState`, `ChildResultStatus`, `SpawnEnvelope`, `ChildResultEnvelope`, `MergeDecision`, `SubagentStatus`, `SubagentProgressUpdate`, `SubagentRuntimeConfig`, `SubagentExecutionConfig`, `SubagentRuntime`을 제공한다.
2. `spawn_from_request`, `register_spawn`, `mark_running`, `cancel_by_session`, `running_count_by_session`, `snapshot`, `update_progress`는 child lifecycle, cancellation, active count, status/progress snapshot을 관리한다.
3. `classify_result`, `classify_active_result`, `finish_child`, `publish_child_result`는 child result를 분류하고 synthetic inbound로 되돌리는 경로를 제공한다.
4. `run_spawn`, `spawn_and_run_background`는 child execution과 background execution을 연결한다.
5. `synthetic_inbound_for_result`는 child result를 session content에 직접 쓰지 않고 runtime inbound로 변환한다.
6. `correlation_decision`은 `child_task_id`, `session_id`, `parent_turn_id`, `spawn_effect_id` mismatch를 stale로 분류한다.
7. `synthetic_command_for`는 `completed`, `failed`, `timed_out`, `cancelled`, `stale`을 synthetic subagent command 또는 progress observed로 매핑한다.
8. `build_subagent_tool_registry`는 parent-only `spawn`을 제외하고 workspace, side-effect, exec, web config로 child tool surface를 제한한다.
9. `crates/shacs-core/src/tools/spawn.rs`는 `SpawnRequest`, `SubagentSpawner`, `SpawnTool`, thread-local spawn context를 제공하며 public tool parameter를 `task`와 `label`로 제한한다.
10. `crates/shacs-cli/src/lib.rs`는 CLI adapter에서 `SpawnTool`을 등록하고 `spawn_and_run_background`를 연결하며 loop config와 adapter settings에서 `SubagentExecutionConfig`를 만든다.

### 아직 남은 것

1. formal child identity와 inherited snapshot type boundary.
2. `spawn_sequence` monotonic tracking과 retry precedence.
3. nested subagent depth ceiling.
4. formal policy, safety, budget evaluation.
5. wall clock timeout enforcement from `timeout_ms`.
6. retry orchestration for timed out or failed children.
7. full merge decision layer.
8. durable recovery after process restart.
9. richer user-visible lifecycle projection.

## 구현 웨이브

### Wave 1. 현재 runtime mapping 정합성 고정

1. Spec 011과 이 PRD가 현재 구현을 formal runtime contract 완료로 주장하지 않게 문구를 정리한다.
2. child identity, lifecycle, spawn path, stale discard, synthetic reentry, parallelism, cancellation cleanup을 current mapping으로 고정한다.
3. verification evidence를 current mapping의 근거로만 둔다.

### Wave 2. Inherited snapshot formalization

1. `InheritedContextSnapshot`, `InheritedPolicySnapshot`, `InheritedSafetySnapshot`, budget snapshot의 실제 타입 경계를 설계한다.
2. 현재 JSON snapshot과 `SubagentExecutionConfig`를 formal snapshot으로 어떻게 연결할지 정한다.
3. placeholder safety field를 완성된 `SafetySnapshot`처럼 쓰지 않도록 migration path를 정한다.

### Wave 3. Runtime limits와 retry policy

1. `spawn_sequence` monotonic tracking을 도입할지 결정한다.
2. nested subagent depth ceiling을 `SubagentRuntimeConfig` 또는 orchestrator policy에 연결한다.
3. `timeout_ms` wall clock enforcement와 timed out result normalization을 구현한다.
4. failed 또는 timed out child의 retry orchestration을 parent decision과 연결한다.

### Wave 4. Merge decision layer

1. full merge, summary-only, failure-fact, abort parent를 구분하는 `MainOrchestrator` decision layer를 설계한다.
2. same-scope sibling 또는 retry child precedence를 formal하게 정한다.
3. current accepted summary mapping을 full merge policy로 오해하지 않게 API 이름과 문서를 정리한다.

### Wave 5. Recovery와 projection

1. process restart 이후 durable child task recovery가 필요한지 personal-use 범위에서 판단한다.
2. 필요한 경우 active child persistence와 late result discard contract를 설계한다.
3. current synthetic announcement와 status report를 넘어서는 user-visible lifecycle projection을 013, 014와 맞춘다.

## Verification Evidence

현재 증거는 다음으로 본다.

Identity/correlation/stale:

1. `subagent_result_with_wrong_child_id_is_stale`.
2. `subagent_result_with_matching_parent_and_child_accepts_summary`.
3. `subagent_result_with_wrong_parent_session_is_stale`.
4. `subagent_stale_result_does_not_publish_or_close_active_child`.
5. `subagent_stale_inbound_is_not_persisted_as_session_content`.

Lifecycle/execution/cancellation:

1. `subagent_spawn_registers_active_task_and_cancels_by_session`.
2. `subagent_spawn_inherits_snapshot_contract`.
3. `subagent_finish_publishes_synthetic_inbound_and_closes_active_task`.
4. `subagent_run_spawn_executes_agent_and_publishes_result`.
5. `subagent_cancel_before_run_cleans_without_announcement`.
6. `subagent_parallelism_limit_rejects_excess_children`.

Tool registry/progress/adapter:

1. `subagent_tool_registry_excludes_parent_only_tools`.
2. `subagent_partial_progress_formats_completed_steps_and_failure`.
3. `spawn_tool_can_delegate_to_subagent_runtime`.
4. `loop_lifecycle_reports_structured_status`.
5. `loop_preserves_channel_chat_and_session_key_in_tool_context`.
6. `adapter_wires_exec_env_to_context_and_subagents`.

이 증거는 current architecture mapping 기준 Spec 011 종료의 근거다. formal inherited safety, timeout, retry, full merge, durable recovery 완료 증거로 읽으면 안 된다.

## Open Risks

1. current JSON snapshot이 future typed snapshot과 다르게 굳어질 수 있다.
2. current accepted summary mapping을 full merge policy로 오해하면 retry, failure-fact, abort parent 결정이 빠질 수 있다.
3. `timeout_ms`가 formal wall clock enforcement와 연결되지 않으면 timed out child 처리가 문서와 어긋날 수 있다.
4. durable child recovery가 없으므로 process restart 이후 late result 처리 정책은 아직 제한적이다.
5. user-visible lifecycle은 current status report 수준이라 future UI/diagnostics 요구가 늘면 보강이 필요하다.

## 종료 기준

현재 종료 기준은 다음이다.

1. Spec 011과 이 PRD가 현재 구현을 formal subagent runtime 완료로 주장하지 않는다.
2. current implemented mapping과 future gap이 분리되어 있다.
3. child identity/lifecycle, spawn tool path, correlation/stale, synthetic inbound, bounded parallelism, status/progress, background execution, tool registry restriction, cancellation cleanup이 현재 필요한 구조로 설명되어 있다.
4. formal inherited snapshot, `spawn_sequence`, depth ceiling, same-scope dedupe, policy/safety/budget evaluation, timeout, retry, full merge, durable recovery, lifecycle projection은 future work로 남아 있다.
5. remote fleet, marketplace, billing, multi-user ownership, remote team inbox, agent negotiation, distributed consensus는 현재 제품 범위 밖으로 남아 있다.
6. verification evidence는 current architecture 종료 근거로 쓰되, formal future model 완료 증거로 쓰지 않는다.
