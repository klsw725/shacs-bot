# PRD 000. context builder and budget cut

## 목표

이 문서는 `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`의 하위 실행 문서다. SPEC을 대체하지 않고, context builder, compaction input filtering, token budgeting, deterministic truncation을 실제 구현 단위로 쪼개어 완료 기준까지 내린다.

이번 PRD의 목표는 provider 호출 직전의 입력 조립 경계를 코드로 고정하는 것이다. 같은 durable state와 같은 turn input이면 항상 같은 provider input snapshot이 나오도록 만들고, budget 초과 시에도 어떤 블록이 유지되고 어떤 블록이 잘리는지 설명 가능하게 한다.

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

- 001에서 turn open/close와 replay 가능한 상태 경계를 받는다.
- 006에서 replay, checkpoint, event tail을 받아 source snapshot을 고정한다.
- 005와 008에서 skill registry snapshot, provider profile snapshot, tool schema snapshot을 받는다.
- 003은 이 PRD가 만든 snapshot을 그대로 실행해야 하며, provider adapter가 문맥을 임의 수정하면 안 된다.
- 010은 secret 원문과 미확정 민감 값이 snapshot, compaction input, diagnostics에 들어가지 않도록 경계를 제공한다.
- 011은 subagent 결과가 merge 전 후보 결과라는 전제를 제공한다.

## 범위

- context source snapshot 고정 단계 구현
- semantic block 모델과 정규화 규칙 구현
- token estimation, budget policy, truncation order 구현
- provider input snapshot 직렬화 규약 구현
- compaction input 후보 필터링과 durable summary 입력 경계 구현
- 결정성, redaction, stale exclusion 검증 테스트 추가

## 범위 제외

- provider별 프롬프트 문구 최적화
- 고급 압축 알고리즘 연구
- UI용 reasoning 표시
- 멀티세션 검색 및 랭킹

## 현재 구현 상태

### 이미 반영된 것

- `ContextBuilder`가 system policy, compacted memory, recent conversation, tool result, subagent result, skill, current request block을 결정적으로 조립한다.
- provider input snapshot과 compaction input snapshot이 같은 source truth에서 목적별로 분리된다.
- token estimate, deterministic budget cut, truncation marker, tool/subagent/skill snapshot, redaction 경계가 테스트로 검증된다.
- compaction 이후 file-backed checkpoint에서 provider profile snapshot, selected skill body, tool schema가 복원되는 경로가 검증된다.
- Spec016 matrix에서 Unit, Integration, SafetyRedaction이 FullSpec verified evidence로 승격돼 있다.

### 아직 남은 것

- token estimation은 제품 최소 범위의 근사치이며 provider별 tokenizer parity는 아직 별도 범위다.
- 고급 summary 압축 알고리즘과 멀티세션 검색/랭킹은 구현 범위 밖이다.
- 위 항목은 현재 deterministic context assembly와 compaction input FullSpec slice의 blocker가 아니라 후속 context optimization 범위다.

### 로컬 근거

- `crates/shacs-core/src/runtime/context.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-core/tests/runtime_loop.rs`

## TDD 계획

1. source snapshot이 같은 입력에서 동일 snapshot을 만드는 단위 테스트를 먼저 만든다.
2. tool, subagent, skill, summary block이 올바른 우선순위와 정규화 규칙을 따르는 테스트를 추가한다.
3. secret, partial chunk, late result, UI projection 필드가 제외되는 안전성 테스트를 추가한다.
4. 긴 세션에서 compaction input과 budget cut이 같이 동작하는 통합 테스트를 추가한다.
5. replay 후 재실행해도 snapshot hash 또는 동등 비교가 유지되는 내구성 테스트를 추가한다.

## 구현 웨이브

### Wave 1. Source snapshot과 block 모델 고정

- `SessionState`, skill snapshot, provider profile snapshot, tool schema snapshot, current turn input을 하나의 context source bundle로 고정한다.
- checkpoint에서 복원된 preselected skill은 현재 registry가 비어 있어도 checkpointed skill body snapshot을 우선 사용한다.
- semantic block 타입을 만들고 system/policy, compacted memory, recent conversation, tool result, subagent result, skill, current request를 분리한다.
- block 생성 시점에 ephemeral candidate를 거르는 필터를 넣는다.

### Wave 2. 정규화와 budget cut 구현

- 각 block을 provider 전달용 공통 표현으로 정규화한다.
- token estimation 계층을 만들고 block별 estimate를 기록한다.
- budget 초과 시 유지 우선순위, 축약 규칙, 완전 제외 규칙을 deterministic하게 적용한다.
- truncation 결과와 근거를 snapshot metadata에 남긴다.

### Wave 3. Compaction input과 provider snapshot 연결

- compaction input builder를 분리해 durable summary 입력 집합을 별도로 만든다.
- provider input snapshot과 compaction input이 같은 source truth를 공유하되 목적별 출력 구조는 분리한다.
- snapshot 직렬화, 비교, 테스트 fixture를 고정한다.

### Wave 4. 오케스트레이터 통합과 회귀 검증

- `MainOrchestrator`의 `context_building` phase에서 source snapshot 고정과 builder 호출을 연결한다.
- provider runtime, tool/subagent merge 이후 새 턴에서 snapshot 재구성이 일관되는지 검증한다.
- long-session, summary 존재, summary 부재, over-budget, stale-result 유입 케이스를 회귀 테스트로 묶는다.

## Verification Evidence

- 단위 테스트: context source selection, block normalization, token budgeting, truncation priority
- 단위 테스트: `tool_result_context_preserves_correlation_metadata_and_limits_raw_output`이 provider message snapshot의 tool result block에 `effect_id`, `correlation_id`, `tool_call_id`를 보존하고 raw tool output/error를 redaction 후 제한하는지 검증한다.
- 단위 테스트: `budget_cut_preserves_current_request_and_policy_blocks`와 `budget_cut_records_estimates_and_keeps_compaction_input_source_truth`가 block별 token estimate, input/output budget 분리, low-priority block truncation marker, provider snapshot과 compaction input source truth 분리를 검증한다.
- 통합 테스트: long-session compaction path, provider snapshot 생성, replay 후 동일 snapshot 재생성
- 경계 테스트: `compaction_input_excludes_open_turn_user_request_but_messages_keep_it`, `retry_invoke_uses_rebuilt_context_snapshot`, `resume_after_completed_turn_restores_replaced_preserved_context`가 provider messages에는 열린 턴 요청을 유지하되 durable compaction input은 닫힌 턴 source만 수집하고 completion-boundary preserved context는 기존 완료 턴 입력을 보존함을 검증한다.
- 내구성 테스트: `file_store_resume_restores_checkpointed_agent_configuration_after_compaction`이 compaction 이후 file-backed checkpoint에서 provider profile snapshot, selected skill body snapshot, injected tool schema를 복원하는지 검증한다.
- 안전성 테스트: secret exclusion, partial provider chunk exclusion, late result exclusion
- 안전성 테스트: `builder_redacts_secret_like_tool_payloads_from_snapshots`와 `replacement_preserved_context_redacts_secret_like_completed_turn_payloads`가 provider message snapshot 및 completion-boundary preserved context에 raw secret-like 값이 남지 않고 `[REDACTED_SECRET]`로 대체되는지 검증한다.
- 내구성 테스트: checkpoint + event tail replay 뒤 snapshot 동일성
- 문서 증거: snapshot 필드와 block 우선순위를 SPEC과 1:1로 매핑한 표

## Open Risks

- token estimation 오차가 크면 truncation 결과가 provider 한도와 어긋날 수 있다.
- tool 또는 subagent 결과의 구조화 수준이 낮으면 정규화 규칙이 흔들릴 수 있다.
- summary block과 recent conversation block의 중복 정보가 커지면 budget 효율이 떨어질 수 있다.

## 종료 기준

- provider input snapshot이 공식 source만으로 결정적으로 생성된다.
- compaction input이 durable 사실만 포함하고 partial, stale, secret 원문을 제외한다.
- budget 초과 시 block 유지/축약/제외 규칙이 자동 테스트로 증명된다.
- replay 후 동일 입력에서 동일 snapshot이 다시 만들어진다.
- 009와 016이 요구하는 단위, 통합, 안전성 검증 증거가 모두 준비된다.
