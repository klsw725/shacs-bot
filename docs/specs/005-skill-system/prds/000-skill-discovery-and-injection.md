# PRD 000. skill discovery and injection

## 목표

이 문서는 `docs/specs/005-skill-system/SPEC.md`의 하위 실행 문서다. 목표는 파일 시스템 기반 스킬 탐색, 파싱, 레지스트리 구성, read-only 문맥 주입까지를 완전 구현 기준으로 구체화하는 것이다.

- 지정된 경로 규약에서 `SKILL.md`를 탐색하고 우선순위 규칙대로 canonical skill을 선택한다.
- malformed, conflicted, shadowed 상태를 진단 가능한 레지스트리로 노출한다.
- 선택된 스킬이 상태 권한 없이 context building 단계의 read-only 입력으로만 주입되게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/005-skill-system/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`

## Dependency Cut

이 PRD는 스킬이 코드 실행 단위가 아닌 Markdown 지식 팩이라는 전제를 구현한다. plugin 설치 시스템, 원격 배포, 추천 랭킹은 다루지 않는다. 범위는 로컬 탐색, 정규화, 레지스트리, 주입 경계까지다.

## 범위

- 스킬 탐색 루트와 precedence 적용
- `SKILL.md` 최소 파싱 규칙
- `SkillDescriptor`와 parse status 모델
- 충돌, 손상, shadow 처리
- 레지스트리 조회와 canonical source 선택
- 오케스트레이터의 context building 단계에서의 read-only 주입

## 범위 제외

- 원격 스킬 저장소
- 실행 가능한 플러그인 코드
- 자동 추천 점수 시스템
- 스킬 편집 UI

## 현재 구현 상태

### 이미 반영된 것

- filesystem 기반 `SKILL.md` discovery, precedence, malformed/conflicted/shadowed 상태, canonical registry 조회가 `crates/shacs-skills/src/lib.rs`에 구현돼 있다.
- 선택된 스킬 본문은 context building 단계의 read-only 입력으로 주입되고, 선택 이유는 session state와 inspect surface에 남는다.
- 선택된 스킬의 `source_path`, `source_kind`, `body_hash` provenance snapshot이 context/effect/inspect/replay 경로에 보존된다.
- FullSpec 범위의 discovery, precedence, parser diagnostics, context injection, provenance, replay, inspect, permission boundary evidence가 반영돼 있다.

### 비범위 / 후속 확장

- plugin 자산 설치/제거/업데이트 lifecycle, 스킬 편집 UI, 원격 저장소, 실행 가능한 plugin 권한 위임은 본 PRD 범위 밖이다.

### 로컬 근거

- `crates/shacs-skills/src/lib.rs`
- `crates/shacs-core/src/core/context.rs`
- `crates/shacs-core/src/core/orchestrator.rs`
- `crates/shacs-core/src/core/observability.rs`
- `crates/shacs-core/tests/skill_discovery.rs`
- `crates/shacs-core/tests/context_builder.rs`
- `crates/shacs-core/tests/command_event_effect.rs`
- `crates/shacs-core/tests/session_store_replay.rs`
- `crates/shacs-core/tests/observability.rs`

## TDD 계획

1. 허용된 경로에서만 `SKILL.md`가 발견되는 테스트를 작성한다.
2. precedence 규칙에 따라 canonical skill이 선택되는 테스트를 작성한다.
3. 같은 계층 중복 발견 시 `conflicted` 상태가 되는 테스트를 작성한다.
4. malformed 파일이 전체 레지스트리를 깨뜨리지 않는 테스트를 작성한다.
5. 선택된 스킬 본문이 context building에서만 읽히고 상태 변경 권한을 얻지 못하는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. discovery 경로와 source kind 모델링

- bundled, user_global, workspace_local, plugin_provided source kind를 타입으로 고정한다.
- 지정된 경로 규약에서만 `SKILL.md`를 찾는 탐색기를 구현한다.
- 탐색 결과에 source path와 plugin name optional을 보존한다.

### Wave 2. Markdown 파싱과 진단 상태

- UTF-8 읽기, optional frontmatter 분리, 표시 이름 결정, 본문 비어 있음 검증을 구현한다.
- `valid`, `malformed`, `conflicted`, `shadowed` 상태를 레지스트리 진단으로 남긴다.
- 불완전 입력이 panic으로 이어지지 않도록 방어한다.

### Wave 3. precedence와 canonical registry 구축

- `bundled < user-global < workspace-local < plugin-provided` 우선순위를 코드로 고정한다.
- 같은 계층 중복은 병합하지 않고 충돌로 남긴다.
- 조회 API는 canonical source와 진단 메타데이터를 함께 제공한다.

### Wave 4. context injection 경계 연결

- context building 단계에서 선택된 스킬 본문 일부 또는 전체를 read-only 입력으로 주입한다.
- 스킬 본문이 effect 생성, permission 변경, 세션 상태 수정 권한을 갖지 못하도록 경계를 고정한다.
- 왜 특정 스킬이 선택되었는지 설명 가능한 메타데이터를 남긴다.
- 선택된 스킬의 `source_path`, `source_kind`, `body_hash` provenance snapshot을 context/effect/inspect에 보존한다.

## Verification Evidence

- Unit FullSpec evidence: `crates/shacs-core/tests/skill_discovery.rs`
  - `discovers_only_skill_md_from_allowed_roots`
  - `selects_highest_precedence_canonical_skill`
  - `plugin_provided_skill_has_highest_precedence_and_records_plugin_name`
  - `marks_lower_precedence_candidates_as_shadowed`
  - `marks_same_tier_plugin_duplicates_as_conflicted`
  - `higher_tier_conflict_blocks_lower_tier_canonical_fallback`
  - `malformed_skill_does_not_block_lower_valid_candidate`
  - `invalid_directory_skill_name_is_malformed_and_does_not_block_valid_candidate`
  - `malformed_only_skill_has_no_canonical_entry`
  - `discovering_and_parsing_skills_does_not_mutate_session_state`
  - `unclosed_frontmatter_is_malformed`
  - `invalid_utf8_skill_is_malformed`
  - `empty_frontmatter_name_is_malformed`
  - `invalid_frontmatter_name_is_malformed`
- Integration FullSpec evidence: `crates/shacs-core/tests/context_builder.rs`, `crates/shacs-core/tests/session_store_replay.rs`, `crates/shacs-core/tests/observability.rs`
  - `builder_sorts_skills_and_tool_schema_deterministically`
  - `builder_keeps_skill_body_out_of_messages_tool_schema_and_system_context`
  - `builder_skips_conflicted_and_malformed_selected_skills_without_snapshot_body`
  - `replay_persists_selected_skill_snapshot_for_same_turn_retry`
  - `inspect_snapshot_exposes_selected_skills_and_reasons`
- SafetyRedaction FullSpec evidence: `crates/shacs-core/tests/command_event_effect.rs`
  - `submit_user_input_emits_events_in_kernel_order`
  - `orchestrator_populates_skill_and_tool_schema_snapshots`
  - `orchestrator_auto_selects_skill_from_user_input_and_records_reason`
  - `skill_content_cannot_change_permission_mode_or_emit_host_effects`
  - `plan_mode_tool_request_is_denied_before_dispatch`
- Matrix evidence: `crates/shacs-contracts/src/verification.rs`, `crates/shacs-core/tests/verification_matrix.rs`
  - Spec005 declares `CoverageLevel::FullSpec` with `CoverageStatus::Verified` evidence for `Unit`, `Integration`, and `SafetyRedaction`.

## Open Risks

- auto-selection은 단순 문자열 기반이다. 고급 추천/랭킹 개선은 본 PRD 범위 밖이다.
- skill body는 로컬 Markdown context input이다. 실행 권한, permission 변경, tool dispatch 권한을 갖지 않는다.
- plugin-provided skill 경로는 전제하지만 plugin 자산 lifecycle 자체는 본 PRD 범위 밖이므로, 설치/제거/업데이트 계약은 별도 문서 정리가 필요하다.

## 종료 기준

- 지정된 경로 규약과 precedence가 코드로 고정된다.
- malformed와 conflicted 스킬은 진단되지만 세션 전체를 망가뜨리지 않는다.
- canonical skill만 주입되며, 주입은 read-only 문맥 보강으로 제한된다.
- `docs/specs/005-skill-system/SPEC.md`의 스킬 정의와 금지 패턴이 구현에 반영된다.
