# PRD 005. diagnostics trajectory replay and release evidence integration

상태: 완료

완료 근거:

1. `crates/shacs-core/src/runtime/tool_search.rs`는 Tool Search activation mode, actual activated 여부, reason family, visible/deferred count, catalog scope digest를 `ToolSearchDiagnosticsSummary`로 표현한다.
2. `crates/shacs-core/src/runtime/runner.rs`는 provider iteration마다 `tool_search_activation` event를 남기고, bridge `tool_search`, `tool_describe`, `tool_call` event를 raw full schema 없이 bounded/redacted evidence로 변환한다.
3. `tool_search` evidence는 redacted query, requested limit, bounded matched names, scope digest를 남기며, `tool_describe` evidence는 requested name과 found 여부만 남긴다.
4. `tool_call` evidence는 bridge call id, bridge name, underlying tool name, scope digest mapping을 남기고 secret-bearing underlying arguments를 diagnostics event에 넣지 않는다.
5. `crates/shacs-utils/src/progress_events.rs`와 `crates/shacs-cli/src/lib.rs`는 CLI observability start/finish payload와 verbose args preview에서 Tool Search bridge arguments를 safe projection으로 줄여 raw nested `tool_call.arguments`를 저장하거나 송신하지 않는다.
6. subagent progress는 sanitized core `ToolEvent` serialization regression으로 raw bridge arguments, raw schema, secret text가 들어가지 않음을 고정했다.
7. replay는 live tool dispatch를 추가하지 않고 `live_tool_dispatch_count == 0` recorded-evidence-only 경로를 regression으로 고정했다.
8. `bridge_underlying_mapping_evidence_ref`는 bridge call id, bridge name, underlying name, scope digest mapping을 redacted `EvidenceRef`로 만들어 trajectory `tool_refs`에 연결할 수 있게 한다.
9. `tool_search_prd005_release_evidence_checklist`는 config, assembler, bridge, runner wiring, MCP default-deny, subagent scope, replay safety, diagnostics evidence bucket을 모두 요구하고, 각 bucket은 label과 owner/redaction이 유효한 `EvidenceRef`를 함께 가져야 covered로 본다.

검증:

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check` 통과.
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings` 통과.
3. `cargo test --manifest-path crates/shacs-utils/Cargo.toml progress_events` 통과: 3 passed.
4. `cargo test --manifest-path crates/shacs-cli/Cargo.toml tool_observability_projects_bridge_arguments_for_start_and_pending_finish` 통과.
5. `cargo test --manifest-path crates/shacs-cli/Cargo.toml runtime_verbose_preview_helpers_redact_before_truncating` 통과.
6. `cargo test --manifest-path crates/shacs-core/Cargo.toml tool_search_prd005_release_evidence_checklist_requires_all_buckets` 통과.
7. `cargo test --manifest-path crates/shacs-core/Cargo.toml bridge_underlying_mapping_evidence_ref_is_safe_for_trajectory_tool_refs` 통과.
8. `cargo test --manifest-path crates/shacs-core/Cargo.toml core_bridge_tool_events_serialize_safe_for_subagent_progress` 통과.
9. `cargo test --manifest-path crates/shacs-core/Cargo.toml` 통과: lib 16, app_environment 11, runtime 17, runtime_agent 49, runtime_loop 105, tools 75, doctest 0.
10. `lsp_diagnostics` 확인: PRD 005 관련 Rust 변경 파일에서 diagnostics 없음.

## 목표

이 문서는 Tool Search의 여섯 번째 구현 PRD다.

목표는 활성화, 검색, bridge unwrap, underlying execution mapping을 diagnostics와 trajectory evidence에 연결하고 release gate에서 검증 가능한 증거로 남기는 것이다.

Tool Search 관측 정보는 권한이나 schema를 새로 노출하는 경로가 아니다.

특히 raw full schema를 diagnostics bundle에 그대로 넣지 않고, replay는 destructive underlying tool을 live-dispatch하지 않아야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`다.
2. 선행 PRD는 `prds/002-bridge-dispatcher-and-executor-unwrap.md`, `prds/003-agent-runner-provider-request-live-wiring.md`, `prds/004-mcp-default-deny-and-subagent-scope-regression.md`다.
3. diagnostics 기준은 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 따른다.
4. release gate 기준은 `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`를 따른다.
5. trajectory, replay, evidence safety는 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`가 소유한다.

## Dependency Cut

1. PRD 003은 runner iteration의 activation summary와 catalog scope를 제공한다.
2. PRD 002는 bridge call id와 underlying tool name mapping을 제공한다.
3. PRD 004는 disabled MCP와 subagent scope regression evidence를 제공한다.
4. 014는 diagnostics surface와 redaction 기준을 제공한다.
5. 018은 trajectory, replay, evaluation ledger safety를 소유한다.
6. 020은 Tool Search에 필요한 evidence field의 의미만 정의한다.

## 범위

1. activation mode와 activation reason diagnostics.
2. visible count와 deferred count evidence.
3. catalog scope digest evidence.
4. search query와 matched names evidence.
5. bridge call id와 underlying mapping evidence.
6. `tools_used` underlying name preference.
7. replay destructive dispatch 금지 기준.
8. release gate evidence checklist.

## 범위 제외

1. raw full schema diagnostics dump.
2. persistent catalog export.
3. vector ranking telemetry service.
4. provider-native beta usage analytics.
5. hosted SaaS admin dashboard.
6. team fleet compliance workflow.
7. replay에서 destructive tool을 재실행하는 검증 방식.

## 구현 요구사항

1. diagnostics는 Tool Search activation mode와 actual activated 여부를 구분해 남겨야 한다.
2. activation reason은 off, threshold, forced on, no deferrable tools, bridge collision, unknown context window 같은 family로 남길 수 있어야 한다.
3. diagnostics는 visible count와 deferred count를 남겨야 한다.
4. diagnostics는 catalog scope digest를 남겨야 한다.
5. `tool_search` event는 redacted query와 matched names를 남겨야 한다.
6. matched names는 bounded list여야 한다.
7. `tool_describe` event는 requested name과 success 여부를 남기되 raw full schema를 diagnostics에 넣지 않는다.
8. `tool_call` event는 bridge call id, bridge name, underlying tool name, scope digest를 남겨야 한다.
9. activity summary와 `tools_used`는 가능한 한 underlying tool name을 우선 표시한다.
10. provider history repair에 필요한 original bridge call id correlation은 보존해야 한다.
11. trajectory evidence는 bridge call과 underlying call mapping을 redacted form으로 저장한다.
12. replay는 기록된 destructive underlying tool을 live-dispatch하면 안 된다.
13. replay는 recorded outcome, redacted evidence, ledger reference로 behavior를 재평가해야 한다.
14. release gate는 config, assembler, bridge, runner wiring, MCP default-deny, subagent scope, replay safety evidence를 모두 확인해야 한다.

## 데이터/상태 모델

1. `ToolSearchDiagnosticsSummary`는 mode, activated, reason, visible count, deferred count, scope digest를 가진다.
2. `ToolSearchQueryEvidence`는 redacted query, limit, matched names, scope digest를 가진다.
3. `ToolDescribeEvidence`는 requested name, found 여부, scope digest를 가진다.
4. `BridgeUnderlyingMappingEvidence`는 bridge call id, bridge name, underlying name, scope digest를 가진다.
5. `ToolSearchReleaseEvidence`는 relevant test names와 manual QA result references를 묶는다.
6. raw full schema와 secret-bearing arguments는 diagnostics summary에 들어가지 않는다.

## 정상 시퀀스

1. runner가 provider iteration에서 Tool Search surface를 assemble한다.
2. runner가 activation summary를 diagnostics context에 기록한다.
3. 모델이 `tool_search`를 호출한다.
4. dispatcher가 query evidence와 matched names를 기록한다.
5. 모델이 `tool_describe`를 호출한다.
6. dispatcher가 describe success 여부를 기록한다.
7. 모델이 `tool_call`을 호출한다.
8. dispatcher가 underlying call로 unwrap하고 mapping evidence를 기록한다.
9. runtime executor가 underlying tool을 실행한다.
10. activity와 `tools_used`가 underlying tool name을 우선 표시한다.
11. trajectory ledger가 redacted mapping과 outcome reference를 저장한다.
12. release gate가 evidence checklist를 확인한다.

## 실패 시퀀스

1. Tool Search가 threshold 미만으로 비활성화되면 reason과 count만 남긴다.
2. bridge name collision으로 pass-through되면 collision reason을 남기되 schema dump를 남기지 않는다.
3. search query가 비어 있으면 invalid query evidence를 남긴다.
4. describe target이 catalog에 없으면 missing name evidence를 남긴다.
5. call target이 out-of-scope이면 underlying execution 없이 scope error evidence를 남긴다.
6. underlying validation failure는 기존 tool error와 mapping evidence를 함께 남긴다.
7. replay가 destructive underlying call을 만나면 live dispatch를 건너뛰고 recorded evidence만 사용한다.
8. release evidence가 부족하면 PRD 020 gate는 완료로 보지 않는다.

## 검증 관점

1. activation, deferred count, scope digest가 diagnostics에서 확인되는지 본다.
2. search query와 matched names가 bounded evidence로 남는지 본다.
3. describe diagnostics에 raw full schema가 없는지 본다.
4. bridge call id와 underlying name mapping이 trajectory에 남는지 본다.
5. `tools_used`가 bridge name보다 underlying name을 우선하는지 본다.
6. disabled MCP와 parent-only subagent case가 release evidence에 포함되는지 본다.
7. replay가 destructive underlying tool을 live-dispatch하지 않는지 본다.
8. release gate가 provider-native beta 없이 pass할 수 있는지 본다.

## 완료 기준

1. Tool Search activation과 catalog scope가 diagnostics로 관찰 가능하다.
2. search, describe, call evidence가 raw full schema 없이 남는다.
3. activity와 `tools_used`는 underlying tool name을 우선한다.
4. trajectory와 replay는 bridge-to-underlying mapping을 안전하게 해석한다.
5. replay는 destructive underlying tools를 실제로 재실행하지 않는다.
6. release gate evidence가 여섯 PRD의 핵심 regression을 포함한다.
