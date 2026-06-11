# PRD 003: plugin skills and commands

## 목표

Plugin이 Markdown skill과 user command를 제공할 때 기존 skill system과 command router를 우회하지 않도록 계약을 고정한다.

## 범위

- plugin-provided skill namespace
- disabled plugin skill exclusion
- command manifest and router registration
- command execution as orchestrator reentry
- conflict diagnostics

## 비범위

- executable skill code
- remote skill marketplace
- arbitrary command modifying session files directly

## 구현 요구사항

1. Plugin skill은 `PluginProvided` source kind와 plugin manifest digest를 가져야 한다.
2. Plugin disabled/blocked 상태에서는 해당 skill이 active/available set에 나타나면 안 된다.
3. Plugin skill 이름 충돌은 자동 병합하지 않고 conflict diagnostic으로 남겨야 한다.
4. Plugin command는 router를 통해 command로 재진입해야 하며 session store를 직접 수정하면 안 된다.
5. Plugin command help/inspect는 plugin source와 required permissions를 표시해야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`다.
2. Markdown skill boundary는 `docs/specs/005-skill-system/SPEC.md`를 소비한다.
3. command routing과 user-facing command semantics는 `docs/specs/002-command-event-effect/SPEC.md`와 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`를 소비한다.
4. session truth authority는 `docs/specs/001-session-kernel/SPEC.md`와 `docs/specs/007-main-orchestrator-policy/SPEC.md`를 소비한다.
5. permission과 diagnostics는 010/014/022를 소비한다.

## Dependency Cut

1. PRD 000의 enabled plugin state가 선행되어야 한다.
2. Plugin skill은 read-only context input이며 executable code가 아니다.
3. Plugin command는 command router로 재진입해야 하며 session store를 직접 수정하지 않는다.
4. Skill conflict와 command conflict는 silent override가 아니라 diagnostic이다.
5. remote skill marketplace와 executable skill code는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| plugin skill source kind | `crates/shacs-skills/src/lib.rs` | source precedence and namespace regression |
| skill conflict diagnostics | `crates/shacs-skills/src/lib.rs` | conflict is not merged silently |
| plugin command registration | `crates/shacs-command/src/lib.rs`, `crates/shacs-cli/src/lib.rs` | command reenters router |
| command inspect/help | `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs` | source plugin and permissions visible |

## 데이터/상태 모델

1. `PluginProvidedSkill`: plugin name, skill name, namespace, body digest, manifest digest, enabled state를 가진다.
2. `PluginCommandDefinition`: command name, help text, source plugin, required permission metadata, router target을 가진다.
3. `PluginSkillConflict`: duplicate namespace, duplicate display name, disabled source collision을 구분한다.
4. `PluginCommandDispatch`: command router input이지 session state mutation이 아니다.
5. `PluginSurfaceProjection`: skill/command가 enabled plugin에서 온 것인지 inspect할 수 있는 read model이다.

## 정상 시퀀스

1. enabled plugin이 Markdown skill bundle을 제공한다.
2. skill registry가 `plugin:<plugin-name>/<skill-name>` namespace로 등록한다.
3. user가 plugin command를 호출하면 command router가 일반 command dispatch처럼 처리한다.
4. command handler는 필요한 action을 MainOrchestrator/effect boundary로 넘긴다.
5. inspect/help는 source plugin과 required permission을 보여준다.

## 실패 시퀀스

1. disabled plugin의 skill/command는 active set에 나타나지 않는다.
2. skill namespace conflict는 conflict diagnostic으로 남고 자동 병합하지 않는다.
3. command conflict는 existing command를 silent override하지 않는다.
4. plugin command가 direct session-store mutation을 요구하면 거부한다.
5. missing permission은 command error 또는 approval flow로 올라가며 plugin이 승인하지 않는다.

## 검증 관점

1. 첫 failing test는 disabled plugin skill이 available skill list에서 제외되는지 확인한다.
2. plugin skill namespace와 conflict behavior를 registry fixture로 고정한다.
3. plugin command가 router path를 통과하고 direct mutation을 하지 않는지 확인한다.
4. help/inspect snapshot은 source plugin과 permission metadata를 표시해야 한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-skills/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-skills/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-skills/Cargo.toml plugin`
4. command를 건드렸다면 `cargo test --manifest-path crates/shacs-command/Cargo.toml plugin`

## 완료 기준

- Plugin-provided skill은 read-only context input으로만 동작한다.
- Plugin command는 MainOrchestrator/command router boundary를 우회하지 않는다.
