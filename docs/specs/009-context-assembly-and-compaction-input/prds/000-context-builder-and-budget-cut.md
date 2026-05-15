# PRD 000. context builder and budget cut

## 목표

이 문서는 `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`의 하위 실행 문서다. 목표는 현재 구현을 formal `ProviderInputSnapshot`, `SemanticBlock`, `TokenBudget` 시스템으로 과장하지 않고, 지금 있는 context builder, memory, compaction, runner governance, provider shaping 경로를 Spec 009에 맞게 매핑하는 것이다.

이번 PRD는 2026-05-15 현재 아키텍처 매핑 기준으로 Spec 009를 닫는다. formal snapshot과 deterministic block budget은 구현 완료로 주장하지 않고 후속 작업으로 분리한다.

## SPEC 입력

- 주관 spec: `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
- 교차 의존:
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/011-subagent-runtime/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 001과 006에서 replay 가능한 session history와 durable state 경계를 받는다.
- 005와 008에서 skill, provider profile, runtime layout 기준을 받는다.
- 003은 canonical provider request를 provider별 wire format으로 바꾸는 계층이다.
- 010은 secret 원문이 context, compaction, provider input에 섞이지 않도록 하는 미래 통합 pass의 기준을 준다.
- 011은 subagent 결과가 수용 전 후보라는 전제를 준다.

현재 provider adapter는 snapshot을 그대로 실행하는 strict runtime이 아니다. adapter는 `ProviderRequest`의 canonical messages, tools, reasoning 입력을 provider wire format으로 shaping한다. 이 PRD는 그 shaping을 현재 구현으로 인정하되, provider가 source를 다시 선택하는 구조로 보지 않는다.

## 범위

- `ContextBuilder` 기반 context assembly의 현재 동작 정리
- `MemoryStore`, `ProviderArchiveConsolidator`, `TokenConsolidationConfig` 기반 memory와 archive 경로 정리
- `AgentLoop`와 `AutoCompact` 기반 context building 전 compaction 경로 정리
- `AgentRunner`의 `govern_messages_for_model`, `microcompact`, `snip_history`를 runner-side message governance로 분류
- `SessionHistoryOptions`, `get_history_with_options`를 current history selection 경로로 반영
- `ProviderRequest`와 provider별 message shaping을 현재 provider input 경계로 반영
- formal snapshot, semantic block, budget work를 미래 gap으로 유지

## 범위 제외

- Rust 코드나 테스트 변경
- provider별 프롬프트 문구 최적화
- 고급 summary compression
- semantic retrieval
- multi-session ranking
- UI reasoning display
- multi-user, admin, operator workflow

## 현재 구현 상태

2026-05-15 기준 Spec 009는 현재 아키텍처 매핑으로 종료한다. 이 종료는 아래 분산 경로가 문맥 조립과 compaction 입력 경계를 설명하고, 관련 테스트가 핵심 동작을 고정한다는 뜻이다. formal `ProviderInputSnapshot`, `SemanticBlock`, `TokenBudget` 모델 구현을 뜻하지 않는다.

### 이미 현재 아키텍처에 매핑되는 것

- `crates/shacs-core/src/runtime/context.rs`의 `ContextBuilder`, `ContextBuildRequest`, `build_system_prompt`, `build_runtime_context`, `build_messages`, `load_skills_for_context`가 system prompt, runtime context, messages, memory, recent history, skill, media input을 조립한다.
- `crates/shacs-core/src/runtime/memory.rs`의 `MemoryStore::memory_context_from_workspace`, `MemoryStore::recent_history_from_workspace`, `ProviderArchiveConsolidator`, `TokenConsolidationConfig`가 memory context, recent history, provider archive, token 기준 consolidation을 맡는다.
- `crates/shacs-core/src/runtime/agent_loop.rs`의 `AgentLoop::maybe_consolidate_session_by_tokens`는 context building 전에 over budget session을 줄이고 metadata와 agent configuration 보존을 검증한다.
- `crates/shacs-core/src/runtime/autocompact.rs`의 `AutoCompact`는 auto compact summary를 context building에 공급한다.
- `crates/shacs-core/src/runtime/runner.rs`의 `govern_messages_for_model`, `microcompact`, `snip_history`는 formal semantic block budget이 아니라 현재 runner-side message governance다.
- `crates/shacs-session/src/lib.rs`의 `SessionHistoryOptions`, `get_history_with_options`는 history input 범위를 조절한다.
- `crates/shacs-providers`의 `ProviderRequest`와 OpenAI-compatible, Responses, Anthropic, Codex, Azure builder는 canonical messages를 provider wire format으로 바꾼다.

### 아직 남은 formal work

- `ContextSourceSnapshot`, `SemanticBlock`, `TokenBudget`, `TruncationPlan`, `ProviderInputSnapshot`, `CompactionInputSnapshot` 타입 경계
- source hash, source sequence, effect id audit metadata
- provider profile, tool schema, selected skill body를 하나로 얼린 단일 source snapshot 타입
- formal block priority token budget과 truncation plan
- provider runtime strict snapshot immutability beyond provider wire-format shaping
- context, compaction, provider 입력을 관통하는 단일 secret exclusion pass
- provider tokenizer parity
- advanced summary compression, semantic retrieval, multi-session ranking, UI reasoning display

## 구현 웨이브

### Wave 1. 현재 context builder 매핑 고정

- `ContextBuilder`가 어떤 입력을 받아 system prompt, runtime context, messages를 만드는지 문서와 테스트 이름으로 고정했다.
- `runtime_context_merges_last_same_role_history_message`가 system-first, history-order-preserving, same-role merge, deterministic `ContextBuilder::build_messages` 동작을 고정한다.
- memory, recent history, skill roots, virtual builtin, media message가 들어가는 경로를 formal snapshot 대신 current architecture mapping으로 설명한다.
- 산출물은 현재 구현 설명이며, `ContextSourceSnapshot` 타입 도입이 아니다.

### Wave 2. memory와 compaction 경계 정리

- `MemoryStore`의 memory context와 recent history 경로를 context source로 정리했다.
- `ProviderArchiveConsolidator`와 `TokenConsolidationConfig`를 archive, token consolidation 경로로 정리했다.
- `AgentLoop::maybe_consolidate_session_by_tokens`와 `AutoCompact`를 context building 전 compaction 경로로 명시했다.

### Wave 3. runner governance와 provider shaping 분리

- `govern_messages_for_model`, `microcompact`, `snip_history`를 provider 호출 전 message governance로 분류했다.
- 이 governance가 formal `SemanticBlock`과 `TruncationPlan`은 아니라고 명시한다.
- provider adapter는 canonical messages를 provider wire format으로 바꾸는 책임으로 한정한다.

### Wave 4. future formal model 설계 보존

- 현재 매핑을 바탕으로 `ContextSourceSnapshot`, `SemanticBlock`, `TokenBudget`, `TruncationPlan`, `ProviderInputSnapshot`, `CompactionInputSnapshot` 도입 계획은 후속 작업으로 둔다.
- source hash, sequence, effect id, compaction boundary, truncation marker audit metadata 설계는 후속 작업으로 둔다.
- context, compaction, provider 입력의 secret exclusion을 하나의 pass로 묶는 방안은 후속 작업으로 둔다.

## Verification Evidence

현재 증거로 사용할 테스트 이름은 다음과 같다.

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

`runtime_context_merges_last_same_role_history_message`는 system-first, history-order-preserving, same-role merge, deterministic `ContextBuilder::build_messages` 동작을 증거로 둔다. 이 증거는 현재 아키텍처 매핑 기준 종료를 뒷받침한다. formal source snapshot hash, block priority budget, provider tokenizer parity, unified secret exclusion pass가 검증됐다는 뜻은 아니다.

## Open Risks

아래 항목은 Spec 009의 현재 아키텍처 매핑 종료를 막지 않는다. formal model로 올릴 때 남는 리스크다.

- runner-side `microcompact`와 `snip_history`는 동작상 유용하지만, block priority 기반 `TruncationPlan`보다 설명력이 낮다.
- provider별 tokenizer parity가 없어 provider 한도와 내부 추정이 어긋날 수 있다.
- provider adapter shaping과 formal snapshot immutability의 경계가 문서화되지 않으면 완료 기준이 다시 과장될 수 있다.
- secret exclusion이 context, compaction, provider 입력에서 단일 pass로 묶이지 않으면 경로별 차이가 남을 수 있다.

## 종료 기준

이 PRD의 현재 종료 기준은 2026-05-15 현재 아키텍처 매핑 기준 Spec 009 종료다.

- 현재 구현이 `ContextBuilder`, `ContextBuildRequest`, `MemoryStore`, `ProviderArchiveConsolidator`, `TokenConsolidationConfig`, `AgentLoop::maybe_consolidate_session_by_tokens`, `AutoCompact`, `AgentRunner` message governance, `SessionHistoryOptions`, `ProviderRequest`, provider-specific shaping으로 분산돼 있음을 문서가 정확히 설명한다.
- formal `ContextSourceSnapshot`, `SemanticBlock`, `TokenBudget`, `TruncationPlan`, `ProviderInputSnapshot`, `CompactionInputSnapshot`을 현재 동작으로 주장하지 않는다.
- provider adapter가 canonical messages를 wire format으로 shaping한다는 현재 경계를 명시한다.
- `runtime_context_merges_last_same_role_history_message`를 포함한 test evidence는 current architecture mapping 종료의 근거로 쓰되, formal snapshot과 token budget 구현 증거로 과장하지 않는다.
- self-hosted 사용자가 긴 세션, memory, compaction, provider 호출 경계를 이해하는 데 필요한 남은 future work가 보존된다.
