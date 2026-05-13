# PRD 000. skill discovery and injection

## 목표

이 문서는 `docs/specs/005-skill-system/SPEC.md`의 하위 실행 문서다. 목표는 현재 구현된 파일 시스템 기반 스킬 탐색, 파싱, 레지스트리 구성, read-only 문맥 주입, CLI inspect 표면을 같은 기준으로 설명하는 것이다.

- 지정된 source 규약에서 `SKILL.md`를 탐색하고 precedence 규칙대로 active skill을 선택한다.
- `Active`, `Shadowed`, `Conflicted`, `Malformed` 상태를 진단 가능한 레지스트리로 노출한다.
- 선택된 스킬이 상태 권한 없이 context building 단계의 read-only 입력으로만 주입되게 한다.
- CLI `skills list`와 `skills show`를 source, status, `body_hash`, diagnostics를 확인하는 inspect 표면으로 둔다.

## SPEC 입력

- 주관 spec: `docs/specs/005-skill-system/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`

## Dependency Cut

이 PRD는 스킬이 코드 실행 단위가 아닌 Markdown 지식 팩이라는 전제를 구현한다. app 설치 시스템, 원격 배포, 추천 랭킹, 실행 가능한 plugin code는 다루지 않는다. 범위는 로컬 탐색, 정규화, 레지스트리, 주입 경계, inspect 표면까지다.

## 범위

- 스킬 탐색 root와 precedence 적용
- `SKILL.md` 최소 파싱 규칙
- `SkillDescriptor`와 `SkillRegistryStatus` 모델
- 충돌, 손상, shadow 처리
- 레지스트리 조회와 active source 선택
- `ContextBuilder`의 read-only skill context 주입
- CLI `skills list`와 `skills show` inspect 출력

## 범위 제외

- formal per-turn registry snapshot
- replay/effect provenance snapshot
- 원격 스킬 저장소와 marketplace
- 실행 가능한 플러그인 코드
- app bundle 설치, 제거, 업데이트 lifecycle ownership
- 자동 추천 점수 시스템
- 스킬 편집 UI

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 이 PRD는 current architecture 기준으로 완료로 닫는다. 완료의 의미는 formal turn snapshot, replay/effect provenance snapshot, app bundle lifecycle ownership, remote marketplace, executable plugin code를 구현했다는 뜻이 아니다.

완료의 의미는 현재 코드의 `shacs-skills` registry/discovery, `SkillSourceKind`, `SkillRegistryStatus`, `SkillDescriptor`, `discover_skill_registry`, `ContextBuilder` skill injection, CLI `skills list`/`skills show` inspect 표면이 Spec 005의 read-only skill boundary를 만족한다고 문서상 확정한다는 뜻이다.

### 이미 반영된 것

- `crates/shacs-skills/src/lib.rs`는 filesystem 기반 `SKILL.md` discovery, precedence, malformed/conflicted/shadowed 상태, active registry 조회를 구현한다.
- `discover_skill_registry`는 virtual builtins, workspace `builtin_skills`, configured user data `skills`, workspace `.nanobot/skills`, workspace `.shacs-bot/skills`, workspace `skills`, `plugin_roots`를 탐색한다.
- `SkillSourceKind`는 `VirtualBuiltin`, `MaterializedBuiltin`, `UserGlobal`, `WorkspaceLegacy`, `WorkspaceLocal`, `PluginProvided`를 구분한다.
- `SkillRegistryStatus`는 `Active`, `Shadowed`, `Conflicted`, `Malformed`를 구분한다.
- `SkillDescriptor`는 `name`, `description`, `source_kind`, `source_path`, `body_hash`, `requirements`, `install_metadata`를 제공한다.
- `shacs-skills` registry 이름은 비어 있지 않은 frontmatter `name`이 있으면 그 값을 쓰고, 없으면 디렉터리 또는 fallback 이름을 진단과 함께 쓴다.
- `ContextBuilder::build_system_prompt`는 `# Active Skills`와 `# Available Skills`를 구성하고, `load_skills_for_context`는 선택된 Markdown 본문을 read-only context로 주입한다. context용 `SkillDocument`는 path-derived name과 frontmatter metadata를 쓰며 registry `SkillDescriptor`와 같은 모델로 보지 않는다.
- CLI `skills list`와 `skills show`는 registry entry를 inspect한다. `format_skills_show`는 status, source, `body_hash`, path, description, requirements, install metadata, diagnostics를 보여준다.

### 현재 완료의 한계

- CLI inspect는 현재 registry entry를 설명하는 표면이다. full replay provenance나 effect provenance 저장소가 아니다.
- `body_hash`는 스킬 본문 확인과 inspect에 쓰이는 값이다. 이것만으로 formal turn snapshot을 구현했다고 보지 않는다.
- `PluginProvided`는 `plugin_roots`에서 온 Markdown skill source를 뜻한다. 실행 가능한 plugin code 권한을 뜻하지 않는다.
- app bundle lifecycle ownership은 Spec 005 밖에 있다. 필요하면 `017-app-operating-environment/`와 `015-packaging-process-lifecycle-and-upgrades/`에서 다룬다.

### 로컬 근거

- `crates/shacs-skills/src/lib.rs`
- `crates/shacs-core/src/runtime/context.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-cli/src/lib.rs`

## 구현 매핑

### Discovery root와 source kind

- virtual builtins는 파일 시스템에 materialize되지 않아도 `VirtualBuiltin`으로 노출된다.
- workspace `builtin_skills`는 `MaterializedBuiltin`으로 노출된다.
- configured user data `skills`는 `UserGlobal`로 노출된다.
- workspace `.nanobot/skills`는 `WorkspaceLegacy`로 노출된다.
- workspace `.shacs-bot/skills`와 workspace `skills`는 `WorkspaceLocal`로 노출된다.
- `plugin_roots`는 `PluginProvided`로 노출된다.

### Registry status와 descriptor

- 같은 이름의 낮은 우선순위 후보는 `Shadowed`로 남는다.
- 같은 우선순위 계층에서 자동 병합할 수 없는 중복은 `Conflicted`로 남는다.
- 파싱할 수 없거나 root가 디렉터리 형태의 `SKILL.md` 규약을 만족하지 못하는 항목은 `Malformed`로 남는다.
- 빈 frontmatter `name`은 그 자체로 malformed가 아니며, registry는 fallback 이름과 진단을 남긴다.
- active 후보만 자동 주입 대상으로 쓰인다.
- descriptor는 source, path, `body_hash`, requirements, install metadata를 inspect 가능한 메타데이터로 보존한다.

### Context injection 경계

- 스킬 본문은 context building 단계의 Markdown 입력으로만 들어간다.
- 스킬은 `SessionState`를 직접 수정하지 않는다.
- 스킬은 effect 생성, permission 변경, tool dispatch 권한을 갖지 않는다.
- 스킬 본문에 실행 지시가 있어도 승인 권한은 오케스트레이터와 runtime policy 경계에 남는다.

## Verification Evidence

- Unit evidence: `registry_exposes_virtual_builtins_without_onboard`, `registry_workspace_skill_shadows_virtual_builtin`, `registry_applies_precedence_across_configured_roots`, `registry_conflicts_duplicate_plugin_roots`, `registry_reports_malformed_and_conflicted_skills`가 virtual builtin, source precedence, plugin root conflict, shadowing, malformed/conflicted registry 동작을 검증한다.
- Runtime evidence: `runtime_context_loads_extra_skill_roots_and_virtual_builtins`, `configured_env_satisfies_skill_requires_env`가 context loading, extra roots, virtual builtins, requirements 처리를 검증한다.
- CLI evidence: `skills_list_and_show_use_virtual_bundled_registry`가 CLI `skills list`와 `skills show` inspect 표면을 검증한다.
- 이 evidence는 discovery, precedence, diagnostics, context injection, CLI inspect 범위의 증거다. replay/effect provenance snapshot이 구현됐다는 증거로 쓰지 않는다.

## Open Risks

- formal per-turn registry snapshot과 replay/effect provenance snapshot은 현재 완료 범위 밖이다.
- CLI inspect는 source/status/`body_hash`/diagnostics 확인용이다. 세션 replay의 완전한 출처 기록으로 해석하면 안 된다.
- app bundle lifecycle은 이 PRD 밖이다. Spec 005는 app ownership 계약을 맡지 않는다.

## 종료 기준

- 현재 source 규약과 precedence가 코드로 고정돼 있다.
- malformed와 conflicted 스킬은 진단되지만 세션 전체를 망가뜨리지 않는다.
- active skill만 주입되며, 주입은 read-only 문맥 보강으로 제한된다.
- CLI inspect는 source, status, `body_hash`, diagnostics를 확인할 수 있다.
- Spec 005의 핵심 원칙인 "스킬은 read-only Markdown context이며 permission/effect authority가 아니다"가 구현과 문서 양쪽에서 유지된다.
