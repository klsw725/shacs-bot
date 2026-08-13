# 031. configuration runtime layout and execution snapshots 아키텍처 명세

Status: Complete (Scoped)

Origin specs: 008, 009, 010, 015, 026

## 문서 목적

이 문서는 008, 009, 010, 015, 026이 구현 완료 범위를 닫은 뒤에도 남아 있는 configuration, runtime layout, execution snapshot 작업의 새 owner boundary를 고정한다. 008은 현재 `.shacs-bot/config.json`, `.shacs-bot/auth.json`, JSON config loader, env placeholder, legacy migration, path helper, current runtime dirs를 닫았다. 009는 현재 context assembly와 compaction input mapping을 닫았고, formal snapshot과 tokenizer-aware budget은 남겼다. 010은 local safety와 inspect redaction baseline을 닫았다. 030은 raw auth store lifecycle과 credential source resolution/status를 소유하고, 031은 config/profile source declaration schema와 migration을 소유한다. 015는 local process lifecycle baseline을 닫았고, config/profile stored-data transform migration과 더 엄격한 runtime directory ownership은 남겼다. Durable runtime family migration과 writable-start admission은 029가 소유한다. 026은 context files와 inline references의 CLI/core handoff를 닫았고, explicit config-provided extra context file live wiring은 닫지 않았다.

031의 목적은 이 잔여 작업을 하나의 실행 계약으로 묶는 것이다. 핵심은 schema-versioned config migration, profiles and auth source declarations, formal runtime directory ownership, config/context/provider execution snapshot과 030 trusted-runtime owner-fact reference, immutable diagnostic provenance, tokenizer-aware budget, explicit extra context config live wiring이다. Snapshot은 authorization이나 current live source truth가 아니다.

이 문서는 JSON 호환성을 보존한다. 명시적 migration 결정과 증거가 있기 전까지 TOML을 요구하지 않는다. TOML layered config가 더 낫다는 결정이 필요하다면, 사용자 데이터 호환성, migration path, rollback, tooling impact, 테스트 evidence가 먼저 있어야 한다.

## 현재 구현 기준선

현재 기준선은 다음과 같다.

1. 기본 설정 파일은 `.shacs-bot/config.json`이고, 인증 저장은 `.shacs-bot/auth.json`이다.
2. 현재 config loader는 JSON value를 읽고 env placeholder를 해석하며 일부 legacy key migration을 수행한다.
3. 현재 runtime path helper는 workspace, sessions, media, cron, logs, channels, skills 같은 current layout을 생성하고 설명한다.
4. 현재 provider 설정은 `ProviderConfig`의 `api_key`, `api_base`, `extra_headers`, `extra_body`를 중심으로 한다.
5. 현재 context assembly는 `ContextBuilder`, memory, recent history, skill, media message, runner-side governance, provider shaping으로 구성된다.
6. 현재 token governance는 formal `TokenBudget`이 아니라 compaction, microcompact, snip history, provider별 shaping이 나눠 가진 책임이다.
7. 현재 credential handling과 projection redaction은 env placeholder, auth store, CLI auth, inspect, diagnostics 등 여러 지점에 분산되어 있다. 030은 raw credential lifecycle과 data disclosure를 소유하고 031은 declaration/reference만 저장한다.
8. 현재 lifecycle은 ownership marker, stop request marker, update marker, runtime inspect, recover baseline을 가진다.
9. 현재 context file discovery는 default workspace/current-directory context files와 inline references를 live provider handoff에 연결하지만, explicit config-provided extra context files의 live wiring은 닫힌 범위가 아니다.

이 기준선은 폐기 대상이 아니다. 031은 기존 JSON config와 current runtime layout을 끊지 않고, schema와 snapshot 계약을 그 위에 추가하는 방향이어야 한다.

## 031이 소유하는 열린 범위

031은 다음 작업을 소유한다.

1. Schema-versioned config format과 migration entrypoint.
2. JSON compatibility-preserving migration과 rollback/recover evidence.
3. Provider, trusted runtime, context profile의 공식 model.
4. 030의 credential lifecycle이 정의하는 environment, local auth entry, literal, command-backed auth source를 config와 profile이 선언하는 방식.
5. Formal runtime directory ownership, owner markers, mutation admission, cleanup rules.
6. Config snapshot, context snapshot, provider execution snapshot의 생성·저장·provenance와 030이 소유하는 trusted runtime profile, adapter별 sandbox mode, credential source status를 호출 시점 fact로 참조하는 계약.
7. Strict snapshot immutability. Provider adapter와 runtime effect는 받은 snapshot의 source set을 바꾸면 안 된다.
8. Tokenizer-aware budget과 truncation plan.
9. Explicit extra context file config의 live agent loop wiring.
10. Snapshot diagnostics, replay, release evidence.
11. 032가 소유하는 executable resource activation record의 schema-versioned persistence, migration, mutation admission과 execution snapshot reference.

## 구현 불변식

1. JSON config compatibility는 명시적 migration 결정 전까지 유지되어야 한다.
2. TOML은 결정 evidence 없이 필수 format이 될 수 없다.
3. Config migration은 사용자의 secret, env placeholder, workspace override를 raw value로 덮어쓰면 안 된다.
4. Config와 profile은 credential source declaration 또는 local auth entry ref를 우선한다. Literal credential을 지원하면 user-local sensitive config로 표시하고 file permission, inspect 비표시, migration disclosure를 적용해야 한다.
5. Runtime directory ownership은 프로세스가 같은 runtime root를 무질서하게 공유하지 못하게 막아야 한다. 이는 security containment나 process privilege boundary가 아니다.
6. Execution snapshot은 provider 호출 직전의 config/profile selection, trusted runtime ref, sandbox adapter status, credential source status, context/provider input provenance를 설명해야 한다. 독립 `policy` 또는 permission/safety snapshot을 만들지 않는다.
7. Snapshot은 만든 뒤 해당 실행 안에서 불변이어야 한다. Adapter는 wire format shaping만 할 수 있고 source를 새로 읽거나 교체하면 안 된다. 다음 실행이나 retry는 current live source truth를 다시 resolve하고 새 snapshot을 만든다.
8. Context budget은 active user message와 required instructions를 밀어내면 안 된다.
9. Token budget은 provider/model tokenizer 차이를 설명할 수 있어야 하며, 추정 실패는 evidence와 fallback policy를 남겨야 한다.
10. Explicit extra context config는 default context discovery보다 낮거나 명시된 priority를 가져야 하며, trusted-resource source disclosure, precedence, budget gate를 우회하면 안 된다.
11. Executable resource activation persistence는 discovery registry와 분리되고, activation source, workspace trust ref, resource/source identity, content digest, dependency manifest digest, lifecycle status를 보존해야 한다. Active 상태도 자동 실행 허가가 아니며 이 record는 inspect/disable/revoke 및 provenance evidence다.

## Must Have

1. Config schema version field와 migration runner가 있어야 한다.
2. Migration은 dry run, apply, interrupted marker, recover path를 가져야 한다.
3. JSON config file은 기존 사용자가 계속 읽을 수 있어야 하며, migration 없이는 TOML만 요구하면 안 된다.
4. Profile model은 provider, trusted runtime, context 설정을 구분해야 한다.
5. Config/profile의 auth field는 env, local auth store, literal, command-backed source 중 지원 source locator/declaration을 schema-versioned로 저장하고 030 runtime resolution에 전달해야 한다. 030이 precedence와 status를 결정하고 031은 non-secret result ref만 snapshot에 기록한다.
6. Runtime layout은 config, auth, sessions, media, logs, channels, skills, cache, tmp, snapshots 같은 directory ownership을 공식 문서와 code helper에서 일치시켜야 한다.
7. Execution snapshot은 schema version/time, config source와 migration state, profile selection, trusted runtime profile ref, adapter-specific sandbox mode/fallback, credential source kind/status/fingerprint, context sources와 inclusion/truncation, selected tool/resource identities, resource activation refs, provider/model/shaping version, tokenizer/budget, data-disclosure warning, replay contract, provenance digest를 포함해야 한다. Selected identity는 capability grant가 아니다.
8. Snapshot immutability test는 adapter가 snapshot 밖 source를 다시 읽지 않는지 확인해야 한다.
9. Tokenizer-aware budget은 provider/model별 tokenizer 또는 설명 가능한 estimator를 선택해야 한다.
10. Truncation plan은 어떤 context block이 included, truncated, skipped 되었는지 evidence를 남겨야 한다.
11. Explicit extra context files config는 live agent loop에서 context builder handoff까지 연결되어야 한다.
12. Diagnostics와 replay는 raw secret 없이 snapshot id, digest, source summary, migration state를 보여야 하며 snapshot이 diagnostic-only이고 current execution authorization이 아님을 표시해야 한다.
13. Executable-resource dependency install 또는 entrypoint execution snapshot은 current resource source, content/dependency digest, activation/install status를 포함해야 한다. Stale/disabled/revoked/mismatched record는 diagnostics에 남기되 permission allow provenance로 표현하지 않는다.

## Must Not Have

1. GUI config editor를 031 완료 조건으로 삼으면 안 된다.
2. Cloud secret manager나 hosted vault를 요구하면 안 된다.
3. Remote config sync를 요구하면 안 된다.
4. Multi-user RBAC, admin workflow, 조직 정책 배포를 요구하면 안 된다.
5. Cluster layout이나 shared multi-node runtime root를 기본 모델로 삼으면 안 된다.
6. TOML 전환을 evidence 없이 필수 migration으로 만들면 안 된다.
7. Migration이 env placeholder를 실제 secret 값으로 writeback하면 안 된다.
8. Provider adapter가 snapshot 생성 뒤 config, policy, context source를 다시 읽으면 안 된다.
9. Budget overflow를 silent drop으로 처리하면 안 된다.
10. Explicit extra context files가 configured source precedence, path resolution, trusted-resource disclosure, token budget을 우회하면 안 된다.
11. Resource activation record의 구체적인 storage path나 dependency 설치 directory를 031 closure 조건으로 고정하지 않는다. 031은 schema, ownership, mutation admission, snapshot provenance만 소유한다.
12. `PolicySafetySnapshot`, centralized permission snapshot, capability ceiling, action/snapshot authorization correlation을 execution snapshot으로 재도입하지 않는다.
13. Snapshot id/digest를 permission grant, durable approval, remembered allow, replay authorization으로 사용하지 않는다.
14. Ephemeral confirmation을 snapshot에 durable approval로 저장하거나 replay 시 auto-allow에 사용하지 않는다.
15. Typed `SecretRef`, 전 표면 redaction provenance, exfiltration-prevention proof를 snapshot contract로 요구하지 않는다.
16. Sandbox mode나 runtime directory ownership을 universal containment, child inheritance, OS isolation proof로 설명하지 않는다.
17. Credential fingerprint/source digest를 secret safety나 redaction proof로 설명하지 않는다.
18. Activation/content/dependency digest를 executable authorization 또는 permission provenance로 설명하지 않는다.
19. Immutable diagnostic snapshot을 current config/profile/auth/resource truth의 대체물로 사용하지 않는다.
20. Replay 결과를 현재 source에 대한 재실행 승인이나 현재 credential resolution 결과로 취급하지 않는다.

## Acceptance Criteria

1. Existing JSON config가 migration 전후로 읽히고, migration이 필요 없는 경우에는 writeback이 발생하지 않는다.
2. Schema version validator가 current, legacy, future unsupported schema를 구분한다.
3. Migration test가 env placeholder, auth reference, workspace override, profile selection을 보존한다.
4. Interrupted migration test가 runtime start mutation을 차단하고 recover evidence를 남긴다.
5. Profile resolution test가 provider, trusted runtime, context profile source provenance를 보여준다.
6. Config/profile test가 environment, local auth entry, literal, command-backed credential source를 030의 resolution precedence에 넘기고 status-only inspect와 snapshot disclosure를 만든다.
7. Runtime layout test가 directory ownership, marker location, cleanup rule을 공식 helper와 일치시킨다.
8. Execution snapshot test가 config/profile selection, trusted runtime ref, sandbox/credential status, context, provider input digest와 provenance를 고정하고 중앙 policy/permission state를 포함하지 않음을 검증한다.
9. Immutability test가 provider shaping 이후에도 해당 실행의 snapshot source set과 digest가 변하지 않는지, 새 실행이 stale snapshot을 authorization source로 재사용하지 않고 current live truth에서 새 snapshot을 만드는지 확인한다.
10. Tokenizer-aware budget test가 provider/model별 budget selection, truncation, skipped evidence를 검증한다.
11. Extra context config live wiring test가 config-provided context files를 live provider handoff에 포함하고, default discovery와 중복을 피한다.
12. Diagnostics/replay test가 과거 snapshot을 live source 재조회 없이 해석하되 current live truth나 재실행 authorization으로 사용하지 않는다.
13. 사용자 문서가 JSON compatibility, migration 필요 여부, profile/auth source 사용법, raw auth store disclosure, 비범위를 정확히 말한다.
14. Resource activation persistence test가 active/stale/disabled/revoked/removed state, digest mismatch, migration, inspect/disable/revoke mutation을 검증한다. Snapshot은 정확한 activation ref를 diagnostic state로 소비하고, 새 실행은 current 030 eligibility와 032 lifecycle state를 다시 확인한다.

## Execution snapshot 의미

Snapshot이 기록하는 것은 다음이다.

1. 어떤 config locator/schema/migration state와 profile selection을 사용했는가.
2. 030에서 어떤 trusted runtime ref, sandbox adapter status/fallback, credential source kind/status를 관찰했는가.
3. 어떤 context source가 included/truncated/skipped 되었고 provider/model/shaping/tokenizer budget이 무엇이었는가.
4. 어떤 tool/resource identity와 activation ref를 사용했고 어떤 data-disclosure warning이 있었는가.

Snapshot은 중앙 permission/safety decision, durable approval, replay authorization, typed secret/redaction proof, universal sandbox, side-effect rollback, current live source immutability를 보장하지 않는다. 과거 snapshot은 diagnostic/replay evidence이고 새 실행은 current live truth를 resolve해 새 snapshot을 생성한다.

## 032 executable resource lifecycle handoff

1. 032는 resource install proposal, app-level activation lifecycle transition/linkage, inspect/disable/revoke domain contract를 소유한다.
2. 030은 activation eligibility, activated executable skill/extension의 trusted-code disclosure, resource precedence, dependency execution gate, load diagnostics를 소유한다.
3. 031은 activation record의 schema version, persistence, migration, owner-safe mutation, execution snapshot reference를 소유한다.
4. Activation record persistence에는 activation source, workspace trust ref, resource/source identity, content digest, dependency manifest digest, active/stale/disabled/revoked/removed status와 reason이 포함돼야 한다. 이 값은 immutable diagnostic provenance이지 permission grant나 verified-entrypoint authorization이 아니다.
5. Raw credential, full environment, executable retry payload는 resource activation record에 저장하지 않는다.
6. 구체적인 activation storage path와 dependency 설치 위치는 이 handoff에서 정의하지 않는다.

## Open-owner handoff

1. 030은 trusted runtime profile, sandbox adapter state, raw auth lifecycle, credential resolution/status, executable-resource eligibility와 data disclosure의 live truth다.
2. 035는 config/profile/auth-source/snapshot fact를 CLI/TUI/API에 투영하고 unknown/unavailable을 안전으로 추론하지 않는다.
3. 032는 app/resource install과 lifecycle transition을 소유하며 031은 persistence와 snapshot ref만 소유한다.
4. 033 evaluator/automation/replay는 031 snapshot을 input evidence로 소비하되 authorization으로 재사용하지 않는다.
5. 034 rich media/context는 source identity, digest, budget/truncation provenance를 제공하고 031이 snapshot에 기록한다.

## Source Handoff Table

<table>
  <thead>
    <tr>
      <th>Source spec</th>
      <th>닫힌 범위</th>
      <th>031로 넘어온 범위</th>
      <th>031의 처리 원칙</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>008 configuration profiles and runtime layout</td>
      <td>JSON config, auth store, env placeholder, current runtime path helper, current dirs</td>
      <td>Schema version, profiles, auth source declarations, formal runtime layout, deep provenance</td>
      <td>JSON compatibility를 보존하고 evidence 없는 TOML 필수화를 금지한다.</td>
    </tr>
    <tr>
      <td>009 context assembly and compaction input</td>
      <td>Current context builder, memory, compaction, runner governance, provider shaping</td>
      <td>Formal context snapshot, provider input snapshot, tokenizer-aware budget, strict immutability</td>
      <td>Provider adapter는 snapshot source selector가 아니라 wire format shaper로 남긴다.</td>
    </tr>
    <tr>
      <td>010 host safety permissions and secrets</td>
      <td>Local safety baseline, redaction points, auth handling, MCP default-deny</td>
      <td>030-owned credential source precedence의 config/profile consumption, status-only inspect, snapshot disclosure</td>
      <td>Credential lifecycle과 raw auth store persistence·raw-data disclosure는 030이 소유하고, 031은 config/profile source-declaration persistence, migration, snapshot reference를 소유한다.</td>
    </tr>
    <tr>
      <td>015 packaging process lifecycle and upgrades</td>
      <td>Local lifecycle baseline, ownership marker, update marker, recover baseline</td>
      <td>Config/profile transform migration과 schema marker, formal runtime directory ownership</td>
      <td>Config/profile migration과 layout admission을 같은 evidence chain으로 설명하고, durable runtime family migration과 writable-start admission은 이미 닫힌 029 boundary를 소비한다. 031은 029 closure의 선행 조건이 아니다.</td>
    </tr>
    <tr>
      <td>026 context files and inline references</td>
      <td>Default context files, inline references, CLI/core diagnostics, live provider handoff</td>
      <td>Explicit config-provided extra context file live wiring</td>
      <td>Configured context files도 source disclosure, precedence, budget, duplicate handling을 통과한다.</td>
    </tr>
  </tbody>
</table>

## Implementation PRDs

Spec 031은 config migration에서 runtime layout, immutable snapshot, context budget, activation persistence와 final closure까지 아래 단계로 구현한다. 각 PRD는 자신의 schema/evidence를 완결하며 외부 spec의 `Complete` 상태를 요구하지 않는다.

재번호화 이후 새 config writer는 `spec031-config-profile` owner와 `sha256:spec031-open` staleness token을 생성한다. 이미 저장된 `spec035-config-profile` owner와 `sha256:spec035-open` token은 persisted compatibility data로 계속 읽지만 새 데이터에는 생성하지 않는다.

| PRD | Sole owner scope | Depends on |
|---|---|---|
| [PRD 000](prds/000-config-profile-and-auth-source-migration.md) | Schema-versioned config/profile/auth-source declaration과 migration/recovery | 008/010 baselines, Spec 030 auth-source contract |
| [PRD 001](prds/001-runtime-layout-ownership-and-admission.md) | Runtime directory ownership, markers, mutation/cleanup admission | PRD 000, 015/029 baselines |
| [PRD 002](prds/002-immutable-execution-snapshot-and-provenance.md) | Config/context/provider snapshot, trusted-runtime refs, immutability | PRD 000, Spec 030 fact contract |
| [PRD 003](prds/003-token-budget-and-explicit-context-wiring.md) | Tokenizer-aware budget, truncation evidence, explicit extra-context live wiring | PRDs 000/002, 009/026 baselines |
| [PRD 004](prds/004-activation-persistence-replay-and-diagnostics.md) | Activation persistence/migration, snapshot refs, diagnostic-only replay | PRDs 000/002, Specs 030/032 fact contracts |
| [PRD 005](prds/005-sequential-integration-and-spec031-closure.md) | Migration/layout/snapshot/context/activation integration과 final Spec031 closure | PRDs 000-004, required owner-fact audits |

Current PRD status:

| PRD | Status |
|---|---|
| PRD 000 | Complete (Scoped) |
| PRD 001 | Complete (Scoped) |
| PRD 002 | Complete (Scoped) |
| PRD 003 | Complete (Scoped) |
| PRD 004 | Complete (Scoped) |
| PRD 005 | Complete (Scoped) |

Dependency rules:

1. PRD 000은 raw auth lifecycle을 재소유하지 않고 declaration/migration만 소유한다.
2. PRD 002 snapshot은 authorization cache나 current live source truth가 아니다.
3. PRD 004는 030 eligibility와 032 lifecycle을 소비하고 activation domain semantics를 재정의하지 않는다.
4. PRD 005는 external spec closure가 아니라 exact owner facts, compatibility evidence, local artifacts만 검사한다.

## Closure Evidence

031을 닫으려면 아래 증거가 같은 변경 안에 있어야 한다.

1. Config schema version, migration runner, migration marker, recover path를 검증하는 테스트.
2. JSON compatibility와 env placeholder preservation을 검증하는 회귀 테스트.
3. Profile resolution, auth source declaration, status-only inspect를 검증하는 config/provider 테스트.
4. Runtime directory ownership과 marker cleanup/admission을 검증하는 lifecycle 테스트.
5. Config/profile, trusted runtime ref, adapter별 sandbox mode/fallback, credential status, context, provider execution snapshot 생성과 diagnostic digest/provenance를 검증하는 테스트.
6. Strict snapshot immutability를 adapter와 runner 경계에서 검증하는 테스트.
7. Tokenizer-aware budget과 truncation plan을 provider/model별로 검증하는 테스트.
8. Explicit extra context config가 live agent loop에 연결되는 테스트.
9. Diagnostics와 replay가 raw credential 노출 없이 과거 snapshot evidence를 해석하고, current live source truth나 재실행 authorization으로 사용하지 않는 테스트.
10. README, usage, specs index 중 사용자에게 노출되는 문서가 migration, JSON compatibility, profiles, auth sources, 비범위를 정확히 반영한다.
11. 닫는 문서에는 구현 파일, 테스트 이름, migration compatibility 판단, snapshot immutability evidence, TOML 결정 여부가 함께 기록되어야 한다.
12. Executable resource activation schema, migration, mutation admission, inspect/disable/revoke, snapshot provenance evidence가 032 lifecycle 및 030 activation-eligibility/trusted-runtime evidence와 연결되고 permission provenance를 만들지 않아야 한다.

## Scoped Closure Record

Parent Must Have 1-6은 `spec031-test-lifecycle`, 7-11은 `spec031-test-projection-parity`, 12-13은 `spec031-test-surface-smoke` transcript가 일대일 primary evidence다. Acceptance 1-7, 8-13, 14와 Closure Evidence 1-4, 5-9, 10-12도 같은 세 command family에 각각 대응하며 `coverage-matrix.json`이 정확한 source line, command id, stdout hash를 기록한다. PRD005 통합 서사는 `spec031_sequential_integration`이 migration → layout admission → snapshot/context → activation ref → diagnostic replay 순서로 실행한다.

현재 구현 결정은 JSON compatibility 유지이며 TOML 전환을 하지 않는다. `runtime config-migrate --dry-run|--apply|--recover`는 config transform을, 기존 `runtime migrate`는 029 stored-data family를 다룬다. Snapshot과 activation replay는 diagnostics 전용이고 permission grant, 현재 auth/config truth, 재실행 승인을 보장하지 않는다. Runtime directory ownership은 local mutation admission이지 sandbox/OS isolation이 아니다.

외부 owner audit은 Specs 029/030/032/033/034/035 전체 완료 상태를 요구하지 않는다. 031이 소비하는 exact adapter test fact와 해당 Cargo command가 모두 존재하고 통과할 때만 PASS이며, unknown/missing fact는 계속 blocked다. 이 scoped closure는 Specs 032-035의 상태를 변경하지 않는다.
