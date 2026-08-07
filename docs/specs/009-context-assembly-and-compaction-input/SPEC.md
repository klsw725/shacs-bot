# context assembly and compaction input 아키텍처 명세

Status: Complete (Scoped)
Implemented scope: `ContextBuilder`, memory, recent history, skill and media message injection, compaction, auto compact, runner message governance, provider shaping의 current context assembly 경계를 닫았다.
Open work moved to: [031 configuration runtime layout and execution snapshots](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md), [033 evaluation automation live integration](../033-evaluation-automation-live-integration/SPEC.md)
Not carried forward: advanced semantic retrieval, multi-session ranking, UI reasoning display, multi-user context policy를 후속 owner 범위로 가져가지 않는다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/005-skill-system/SPEC.md`, `docs/specs/006-session-store/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 바탕으로 `shacs-bot`의 문맥 조립과 compaction 입력 경계를 정리한다.

이 문서는 지금 있는 코드가 어떤 책임을 이미 나눠 맡는지 밝히고, 아직 formal snapshot, semantic block, token budget 모델로 고정되지 않은 부분을 미래 작업으로 분리한다.

목표는 다음과 같다.

- provider 호출 직전 어떤 입력이 문맥에 들어가는지 설명한다.
- memory, recent history, skill, tool 결과, compaction 결과가 현재 구조에서 어떻게 조립되는지 정리한다.
- 현재 아키텍처 매핑과 미래 formal 모델의 차이를 명확히 한다.
- Spec 009의 종료 범위와 후속 formal model gap을 분리한다.

Spec 009는 2026-05-15 현재 아키텍처 매핑 기준으로 종료됐다. 이는 `ContextBuilder`, memory, compaction, runner governance, provider shaping 경로가 현 구조에서 문맥 조립과 compaction 입력 경계를 설명하고 검증한다는 뜻이다. `ProviderInputSnapshot`, `SemanticBlock`, `TokenBudget` 같은 formal 타입 체계가 구현됐다는 뜻은 아니다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- session store는 durable한 사실을 제공하며, replay와 history 조회가 문맥 조립의 주된 원천이다.
- skill은 read-only 지식 팩이며 문맥 보강에 쓰인다.
- compaction은 닫힌 턴 또는 session boundary를 기준으로 긴 기록을 줄인다.
- provider runtime은 canonical message와 도구 정의를 provider wire format으로 바꾼다.

현재 provider adapter는 canonical message를 OpenAI-compatible, Responses, Anthropic, Codex, Azure 형식에 맞게 shaping한다. 따라서 “provider runtime이 snapshot을 절대 수정하지 않는다”는 formal 불변성은 아직 현재 구현 설명이 아니다. 현재 설명은 “provider adapter가 문맥 원천을 새로 선택하지 않고, 받은 메시지를 provider wire format에 맞게 변환한다”가 정확하다.

---

## 범위

이 문서는 다음을 정의한다.

- 현재 구현된 context assembly 경로
- 현재 구현된 compaction, archive, auto compact 경로
- runner와 provider 계층의 현재 message governance 책임
- formal snapshot, semantic block, token budget 모델로 남은 미래 작업
- 현재 테스트 증거와 남은 검증 관점

이 종료 선언은 다음을 완료로 선언하지 않는다.

- `ContextSourceSnapshot`, `SemanticBlock`, `TokenBudget`, `TruncationPlan`, `ProviderInputSnapshot`, `CompactionInputSnapshot` 타입 체계
- source hash, source sequence, effect id를 묶은 전체 audit metadata
- provider profile, tool schema, selected skill body를 하나로 얼린 단일 source snapshot 타입
- formal block priority 기반 token budget과 truncation plan
- provider runtime의 strict snapshot immutability
- context, compaction, provider 입력을 관통하는 단일 secret exclusion pass
- provider tokenizer parity
- advanced summary compression
- semantic retrieval
- multi-session ranking
- UI reasoning display

---

## 핵심 정의

### context assembly

context assembly는 현재 턴의 provider 호출에 필요한 system prompt, runtime context, recent history, memory, skill, media message를 조립하는 과정이다. 현재 중심 구현은 `crates/shacs-core/src/runtime/context.rs`의 `ContextBuilder`, `ContextBuildRequest`, `build_system_prompt`, `build_runtime_context`, `build_messages`, `load_skills_for_context`다.

### current architecture mapping

current architecture mapping은 formal 타입이 없더라도 현재 코드가 Spec 009의 의도를 어디까지 수행하는지 대응시키는 기준이다. 예를 들어 `ContextBuilder`가 runtime context와 messages를 조립하고, `AgentLoop`가 context building 전에 token 기준 compaction을 실행하며, `AgentRunner`가 provider 호출 직전 message governance를 수행하면 이는 현재 아키텍처 매핑에 포함된다.

이 매핑은 2026-05-15 기준 Spec 009의 종료 범위다. formal snapshot과 deterministic block budget이 없는 부분은 후속 formal model 작업으로 남는다.

### compaction input

compaction input은 긴 세션 기록을 줄이기 위해 요약 또는 archive에 넣는 입력이다. 현재 구현은 `MemoryStore::memory_context_from_workspace`, `MemoryStore::recent_history_from_workspace`, `ProviderArchiveConsolidator`, `TokenConsolidationConfig`, `AutoCompact`, `AgentLoop::maybe_consolidate_session_by_tokens`에 나뉘어 있다.

### provider input

현재 provider input은 formal `ProviderInputSnapshot` 타입이 아니라 `ProviderRequest`와 provider별 builder가 받는 canonical message, tool, reasoning 관련 입력으로 표현된다. provider adapter는 이 입력을 provider wire format으로 바꾼다.

### token governance

현재 token governance는 formal `TokenBudget`과 `TruncationPlan`이 아니다. `AgentLoop`의 loop-level compaction, `AgentRunner`의 `govern_messages_for_model`, `microcompact`, `snip_history`, session history option, provider별 message shaping이 함께 맡는 분산 책임이다.

---

## 현재 구현된 아키텍처 매핑

### context builder 계층

`crates/shacs-core/src/runtime/context.rs`는 현재 context assembly의 중심이다.

- `ContextBuilder`는 build 요청을 받아 system prompt, runtime context, message list를 만든다.
- `ContextBuildRequest`는 현재 요청, agent 설정, workspace, memory, skill root 같은 입력을 묶는다.
- `build_system_prompt`는 agent 정책과 실행 제약을 system prompt로 만든다.
- `build_runtime_context`는 memory, recent history, helper context를 runtime context로 구성한다.
- `build_messages`는 현재 요청, media, runtime context를 provider에 넘길 message 구조로 만든다.
- `load_skills_for_context`는 선택된 skill과 extra skill roots, virtual builtin을 문맥에 넣는다.

테스트 증거는 다음 이름으로 남아 있다.

- `runtime_context_builds_system_runtime_and_media_messages`
- `runtime_context_injects_memory_recent_history_skills_and_helpers`
- `runtime_context_loads_extra_skill_roots_and_virtual_builtins`
- `runtime_context_merges_last_same_role_history_message`

`runtime_context_merges_last_same_role_history_message`는 `ContextBuilder::build_messages`의 현재 계약을 고정한다. system message가 먼저 오고, history 순서를 보존하며, 현재 요청이 마지막 history message와 같은 role이면 그 message에 병합된다. 같은 입력을 반복해도 같은 message list가 나온다.

### memory와 compaction 계층

`crates/shacs-core/src/runtime/memory.rs`는 durable memory와 recent history, archive consolidation을 제공한다.

- `MemoryStore::memory_context_from_workspace`는 workspace 기준 memory context를 만든다.
- `MemoryStore::recent_history_from_workspace`는 최근 history를 context에 공급한다.
- `ProviderArchiveConsolidator`는 provider를 통해 archive summary를 만들고 실패 시 raw archive 경로를 제공한다.
- `TokenConsolidationConfig`는 token 기준 consolidation 정책을 담는다.

관련 테스트 증거는 다음과 같다.

- `runtime_memory_store_appends_sanitizes_cursors_and_feeds_context`
- `runtime_archive_consolidator_summarizes_or_raw_archives_on_provider_failure`
- `runtime_token_consolidation_archives_on_user_boundary_and_preserves_agent_configuration`

### loop와 auto compact 계층

`crates/shacs-core/src/runtime/agent_loop.rs`는 context building 전에 session token 상태를 보고 compaction을 실행한다.

- `AgentLoop::maybe_consolidate_session_by_tokens`는 over budget session을 provider context 조립 전에 줄인다.
- loop-level compaction은 agent configuration과 metadata를 보존해야 한다.
- `crates/shacs-core/src/runtime/autocompact.rs`의 `AutoCompact`는 auto compact summary를 context building 경로에 공급한다.

관련 테스트 증거는 다음과 같다.

- `loop_consolidates_over_budget_session_before_building_context_and_preserves_metadata`
- `loop_consumes_auto_compact_summary_when_building_context`

### runner-side message governance

`crates/shacs-core/src/runtime/runner.rs`는 provider 호출 직전의 message governance를 맡는다.

- `govern_messages_for_model`은 모델별 message 제약에 맞춰 입력을 조정한다.
- `microcompact`는 과도한 message를 더 짧게 만든다.
- `snip_history`는 오래된 history를 잘라 provider 호출 가능 상태로 만든다.

이 계층은 현재 formal semantic block budgeting이 아니다. runner-side message governance로 분류한다.

### session history 계층

`crates/shacs-session/src/lib.rs`는 `SessionHistoryOptions`, `get_history_with_options`로 context에 들어갈 history 범위를 조절한다. 이는 source snapshot 타입은 아니지만, 현재 history input selection의 구현 경로다.

### provider shaping 계층

`crates/shacs-providers`는 `ProviderRequest`를 받아 provider별 wire format으로 바꾼다.

- OpenAI-compatible과 Responses builder는 message, tool, reasoning 입력을 provider API 형식으로 변환한다.
- Anthropic builder는 message, tool, thinking, cache 관련 입력을 Anthropic 형식으로 변환한다.
- Codex와 Azure 경로도 canonical request를 각 provider 요구에 맞춘다.

관련 테스트 증거는 다음과 같다.

- `responses_builder_converts_messages_tools_and_reasoning`
- `provider_spec_sanitizes_openai_compatible_history_and_tool_ids`
- `anthropic_builder_converts_messages_tools_thinking_and_cache`

---

## 현재 원칙

현재 아키텍처에서 지켜야 할 원칙은 다음과 같다.

1. context는 session history, memory, selected skill, current request, accepted result처럼 설명 가능한 원천에서 조립한다.
2. 열린 턴의 partial provider delta나 late result를 durable context처럼 취급하지 않는다.
3. compaction은 사용자 경계 또는 닫힌 경계를 기준으로 실행하고 agent configuration을 보존한다.
4. runner-side governance는 provider 호출 가능성을 높이는 현재 장치이며, formal block budget으로 과장하지 않는다.
5. provider adapter는 canonical message를 wire format으로 shaping하지만, 새 durable source를 임의 선택하지 않는다.
6. self-hosted 사용자가 resume, archive, compaction 이후에도 같은 작업 맥락을 이해할 수 있어야 한다.

---

## 미래 formal 모델로 남은 작업

다음 항목은 현재 blocker가 아니라 Spec 009 종료 범위 밖에 남는 후속 formal model 작업이다.

- `ContextSourceSnapshot`, `SemanticBlock`, `TokenBudget`, `TruncationPlan`, `ProviderInputSnapshot`, `CompactionInputSnapshot` 타입 도입
- source hash, source sequence, effect id audit metadata
- provider profile, tool schema, selected skill body를 하나로 얼린 단일 source snapshot 타입
- formal block priority token budget과 truncation plan
- provider runtime strict snapshot immutability beyond provider wire-format shaping
- context, compaction, provider 입력을 관통하는 단일 secret exclusion pass
- provider tokenizer parity
- advanced summary compression
- semantic retrieval
- multi-session ranking
- UI reasoning display

이 작업들은 현재 구현을 부정하지 않는다. 다만 현재 분산 책임을 더 설명 가능하고 테스트 가능한 단일 모델로 끌어올리는 단계다.

---

## 금지 패턴

### 문자열 concat 중심의 무차별 조립

- 모든 history, output, skill을 순서 없이 이어 붙인 뒤 provider limit만 맞추면 안 된다.
- 현재 `ContextBuilder` 경로와 runner-side governance를 우회하면 재현성이 떨어진다.

### provider adapter를 source selector로 사용

- provider adapter가 session store나 skill registry를 직접 다시 읽어 입력을 고르면 안 된다.
- adapter의 책임은 canonical input을 provider wire format으로 바꾸는 것이다.

### compaction에 열린 턴 포함

- partial provider delta, approval 대기 tool call, late result를 durable summary에 섞으면 안 된다.

### formal 타입을 현재 구현처럼 문서화

- `ProviderInputSnapshot`, `SemanticBlock`, `TokenBudget`이 이미 구현된 것처럼 적으면 안 된다.
- 현재는 분산 구현과 테스트 증거를 기준으로 말해야 한다.

---

## 테스트 관점에서 확인할 것

현재 증거로 인정하는 테스트 이름은 다음과 같다.

- `runtime_context_builds_system_runtime_and_media_messages`
- `runtime_context_injects_memory_recent_history_skills_and_helpers`
- `runtime_context_loads_extra_skill_roots_and_virtual_builtins`
- `runtime_context_merges_last_same_role_history_message`
- `runtime_memory_store_appends_sanitizes_cursors_and_feeds_context`
- `runtime_archive_consolidator_summarizes_or_raw_archives_on_provider_failure`
- `runtime_token_consolidation_archives_on_user_boundary_and_preserves_agent_configuration`
- `loop_consolidates_over_budget_session_before_building_context_and_preserves_metadata`
- `loop_consumes_auto_compact_summary_when_building_context`
- `responses_builder_converts_messages_tools_and_reasoning`
- `provider_spec_sanitizes_openai_compatible_history_and_tool_ids`
- `anthropic_builder_converts_messages_tools_thinking_and_cache`

특히 `runtime_context_merges_last_same_role_history_message`는 system-first, history-order-preserving, same-role merge, deterministic `ContextBuilder::build_messages` 동작을 현재 아키텍처 매핑의 증거로 삼는다.

미래 검증은 다음을 추가로 요구한다.

- 같은 source snapshot에서 같은 provider input이 나오는지 확인
- block priority와 truncation plan의 독립 테스트
- compaction input이 닫힌 경계까지만 수집되는지 확인
- provider, compaction, context builder가 같은 secret exclusion 규칙을 쓰는지 확인
- provider tokenizer parity에 따른 budget 예측 차이 확인

---

## 결론

Spec 009의 현재 상태는 “2026-05-15 현재 아키텍처 매핑 기준 완료, formal snapshot과 semantic block budget은 후속 작업”이다.

`shacs-bot`은 이미 `ContextBuilder`, `ContextBuildRequest`, `MemoryStore`, `ProviderArchiveConsolidator`, `TokenConsolidationConfig`, `AgentLoop::maybe_consolidate_session_by_tokens`, `AutoCompact`, `AgentRunner` message governance, `SessionHistoryOptions`, `ProviderRequest`, provider-specific shaping을 통해 문맥 조립과 긴 세션 축약의 주요 경로를 갖고 있다. 하지만 이것을 `ProviderInputSnapshot`과 `TokenBudget` 기반의 완전한 formal 계약으로 표현하는 단계는 아직 남아 있다.

따라서 이 spec은 현재 구현 기준으로 닫는다. self-hosted 사용자가 긴 세션과 compaction 이후에도 작업 맥락을 잃지 않게 만드는 formal snapshot, budget, audit, retrieval 개선은 별도 후속 작업으로 유지한다.
