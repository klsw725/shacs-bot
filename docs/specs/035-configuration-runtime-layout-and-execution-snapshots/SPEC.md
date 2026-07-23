# 035. configuration runtime layout and execution snapshots 아키텍처 명세

Status: Open

Origin specs: 008, 009, 010, 015, 026

## 문서 목적

이 문서는 008, 009, 010, 015, 026이 구현 완료 범위를 닫은 뒤에도 남아 있는 configuration, runtime layout, execution snapshot 작업의 새 owner boundary를 고정한다. 008은 현재 `.shacs-bot/config.json`, `.shacs-bot/auth.json`, JSON config loader, env placeholder, legacy migration, path helper, current runtime dirs를 닫았다. 009는 현재 context assembly와 compaction input mapping을 닫았고, formal snapshot과 tokenizer-aware budget은 남겼다. 010은 local safety baseline을 닫았고, secret reference와 unified redaction model은 남겼다. 015는 local process lifecycle baseline을 닫았고, config/profile stored-data transform migration과 더 엄격한 runtime directory ownership은 남겼다. Durable runtime event/checkpoint/work/channel/child/diagnostics family의 migration과 writable-start admission은 029가 소유한다. 029는 current config compatibility와 runtime path-helper boundary로 먼저 닫히며, 035는 그 boundary를 확장하되 029를 다시 열지 않는다. 026은 context files와 inline references의 CLI/core handoff를 닫았고, explicit config-provided extra context file live wiring은 닫지 않았다.

035의 목적은 이 잔여 작업을 하나의 실행 계약으로 묶는 것이다. 핵심은 schema-versioned config migration, profiles and secret refs, formal runtime directory ownership, config/policy/context/provider execution snapshots and provenance, strict snapshot immutability, tokenizer-aware budget, explicit extra context config live wiring이다.

이 문서는 JSON 호환성을 보존한다. 명시적 migration 결정과 증거가 있기 전까지 TOML을 요구하지 않는다. TOML layered config가 더 낫다는 결정이 필요하다면, 사용자 데이터 호환성, migration path, rollback, tooling impact, 테스트 evidence가 먼저 있어야 한다.

## 현재 구현 기준선

현재 기준선은 다음과 같다.

1. 기본 설정 파일은 `.shacs-bot/config.json`이고, 인증 저장은 `.shacs-bot/auth.json`이다.
2. 현재 config loader는 JSON value를 읽고 env placeholder를 해석하며 일부 legacy key migration을 수행한다.
3. 현재 runtime path helper는 workspace, sessions, media, cron, logs, channels, skills 같은 current layout을 생성하고 설명한다.
4. 현재 provider 설정은 `ProviderConfig`의 `api_key`, `api_base`, `extra_headers`, `extra_body`를 중심으로 한다.
5. 현재 context assembly는 `ContextBuilder`, memory, recent history, skill, media message, runner-side governance, provider shaping으로 구성된다.
6. 현재 token governance는 formal `TokenBudget`이 아니라 compaction, microcompact, snip history, provider별 shaping이 나눠 가진 책임이다.
7. 현재 safety와 secret handling은 env placeholder, auth store, CLI auth, inspect redaction, diagnostics redaction 등 여러 지점에 분산되어 있다.
8. 현재 lifecycle은 ownership marker, stop request marker, update marker, runtime inspect, recover baseline을 가진다.
9. 현재 context file discovery는 default workspace/current-directory context files와 inline references를 live provider handoff에 연결하지만, explicit config-provided extra context files의 live wiring은 닫힌 범위가 아니다.

이 기준선은 폐기 대상이 아니다. 035는 기존 JSON config와 current runtime layout을 끊지 않고, schema와 snapshot 계약을 그 위에 추가하는 방향이어야 한다.

## 035가 소유하는 열린 범위

035는 다음 작업을 소유한다.

1. Schema-versioned config format과 migration entrypoint.
2. JSON compatibility-preserving migration과 rollback/recover evidence.
3. Provider, permission, runtime, context profile의 공식 model.
4. 030이 소유하는 typed secret ref를 config와 profile이 참조하는 방식.
5. Formal runtime directory ownership, owner markers, mutation admission, cleanup rules.
6. Config snapshot, context snapshot, provider execution snapshot의 생성·저장·provenance와 030이 소유하는 policy/safety snapshot digest 참조 계약.
7. Strict snapshot immutability. Provider adapter와 runtime effect는 받은 snapshot의 source set을 바꾸면 안 된다.
8. Tokenizer-aware budget과 truncation plan.
9. Explicit extra context file config의 live agent loop wiring.
10. Snapshot diagnostics, replay, release evidence.

## 구현 불변식

1. JSON config compatibility는 명시적 migration 결정 전까지 유지되어야 한다.
2. TOML은 결정 evidence 없이 필수 format이 될 수 없다.
3. Config migration은 사용자의 secret, env placeholder, workspace override를 raw value로 덮어쓰면 안 된다.
4. Config와 profile은 raw secret value 대신 030이 소유하는 typed secret ref만 persistence해야 한다.
5. Runtime directory ownership은 프로세스가 같은 runtime root를 무질서하게 공유하지 못하게 막아야 한다.
6. Execution snapshot은 provider 호출 직전의 config, policy, context, provider input provenance를 설명해야 한다.
7. Snapshot은 만든 뒤 불변이어야 한다. Adapter는 wire format shaping만 할 수 있고 source를 새로 읽거나 교체하면 안 된다.
8. Context budget은 active user message와 required instructions를 밀어내면 안 된다.
9. Token budget은 provider/model tokenizer 차이를 설명할 수 있어야 하며, 추정 실패는 evidence와 fallback policy를 남겨야 한다.
10. Explicit extra context config는 default context discovery보다 낮거나 명시된 priority를 가져야 하며, permission, redaction, budget gate를 우회하면 안 된다.

## Must Have

1. Config schema version field와 migration runner가 있어야 한다.
2. Migration은 dry run, apply, interrupted marker, recover path를 가져야 한다.
3. JSON config file은 기존 사용자가 계속 읽을 수 있어야 하며, migration 없이는 TOML만 요구하면 안 된다.
4. Profile model은 provider, permission, runtime, context 설정을 구분해야 한다.
5. Config/profile의 secret field는 030의 typed secret ref를 소비하고 env, auth store, local secret store 중 지원 source만 가리켜야 한다.
6. Runtime layout은 config, auth, sessions, media, logs, channels, skills, cache, tmp, snapshots 같은 directory ownership을 공식 문서와 code helper에서 일치시켜야 한다.
7. Execution snapshot은 config source, profile selection, policy/safety snapshot ref, context sources, selected tools, provider/model, budget, redaction status, provenance digest를 포함해야 한다.
8. Snapshot immutability test는 adapter가 snapshot 밖 source를 다시 읽지 않는지 확인해야 한다.
9. Tokenizer-aware budget은 provider/model별 tokenizer 또는 설명 가능한 estimator를 선택해야 한다.
10. Truncation plan은 어떤 context block이 included, truncated, skipped 되었는지 evidence를 남겨야 한다.
11. Explicit extra context files config는 live agent loop에서 context builder handoff까지 연결되어야 한다.
12. Diagnostics와 replay는 raw secret 없이 snapshot id, digest, source summary, migration state를 보여야 한다.

## Must Not Have

1. GUI config editor를 035 완료 조건으로 삼으면 안 된다.
2. Cloud secret manager나 hosted vault를 요구하면 안 된다.
3. Remote config sync를 요구하면 안 된다.
4. Multi-user RBAC, admin workflow, 조직 정책 배포를 요구하면 안 된다.
5. Cluster layout이나 shared multi-node runtime root를 기본 모델로 삼으면 안 된다.
6. TOML 전환을 evidence 없이 필수 migration으로 만들면 안 된다.
7. Migration이 env placeholder를 실제 secret 값으로 writeback하면 안 된다.
8. Provider adapter가 snapshot 생성 뒤 config, policy, context source를 다시 읽으면 안 된다.
9. Budget overflow를 silent drop으로 처리하면 안 된다.
10. Explicit extra context files가 protected path, symlink escape, redaction, token budget을 우회하면 안 된다.

## Acceptance Criteria

1. Existing JSON config가 migration 전후로 읽히고, migration이 필요 없는 경우에는 writeback이 발생하지 않는다.
2. Schema version validator가 current, legacy, future unsupported schema를 구분한다.
3. Migration test가 env placeholder, auth reference, workspace override, profile selection을 보존한다.
4. Interrupted migration test가 runtime start mutation을 차단하고 recover evidence를 남긴다.
5. Profile resolution test가 provider, permission, runtime, context profile source provenance를 보여준다.
6. Config/profile test가 raw secret persistence 없이 030의 typed secret ref를 provider invocation 직전 resolution 경계로 넘기고 redacted snapshot을 만든다.
7. Runtime layout test가 directory ownership, marker location, cleanup rule을 공식 helper와 일치시킨다.
8. Execution snapshot test가 config, policy, context, provider input digest와 provenance를 고정한다.
9. Immutability test가 provider shaping 이후에도 snapshot source set과 digest가 변하지 않는지 확인한다.
10. Tokenizer-aware budget test가 provider/model별 budget selection, truncation, skipped evidence를 검증한다.
11. Extra context config live wiring test가 config-provided context files를 live provider handoff에 포함하고, default discovery와 중복을 피한다.
12. Diagnostics/replay test가 snapshot을 live source 재조회 없이 해석한다.
13. 사용자 문서가 JSON compatibility, migration 필요 여부, profile/secret ref 사용법, 비범위를 정확히 말한다.

## Source Handoff Table

<table>
  <thead>
    <tr>
      <th>Source spec</th>
      <th>닫힌 범위</th>
      <th>035로 넘어온 범위</th>
      <th>035의 처리 원칙</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>008 configuration profiles and runtime layout</td>
      <td>JSON config, auth store, env placeholder, current runtime path helper, current dirs</td>
      <td>Schema version, profiles, secret refs, formal runtime layout, deep provenance</td>
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
      <td>030-owned typed secret ref의 config/profile consumption, redacted snapshot, persistence safety</td>
      <td>Secret type과 redaction semantics는 030이 소유하고, 035는 raw secret value를 config와 execution snapshot에 직접 persistence하지 않는다.</td>
    </tr>
    <tr>
      <td>015 packaging process lifecycle and upgrades</td>
      <td>Local lifecycle baseline, ownership marker, update marker, recover baseline</td>
      <td>Config/profile transform migration과 schema marker, formal runtime directory ownership</td>
      <td>Config/profile migration과 layout admission을 같은 evidence chain으로 설명하고, durable runtime family migration과 writable-start admission은 이미 닫힌 029 boundary를 소비한다. 035는 029 closure의 선행 조건이 아니다.</td>
    </tr>
    <tr>
      <td>026 context files and inline references</td>
      <td>Default context files, inline references, CLI/core diagnostics, live provider handoff</td>
      <td>Explicit config-provided extra context file live wiring</td>
      <td>Configured context files도 permission, redaction, budget, duplicate handling을 통과한다.</td>
    </tr>
  </tbody>
</table>

## Closure Evidence

035를 닫으려면 아래 증거가 같은 변경 안에 있어야 한다.

1. Config schema version, migration runner, migration marker, recover path를 검증하는 테스트.
2. JSON compatibility와 env placeholder preservation을 검증하는 회귀 테스트.
3. Profile resolution과 secret ref redaction을 검증하는 config/provider 테스트.
4. Runtime directory ownership과 marker cleanup/admission을 검증하는 lifecycle 테스트.
5. Config, policy, context, provider execution snapshot 생성과 digest/provenance를 검증하는 테스트.
6. Strict snapshot immutability를 adapter와 runner 경계에서 검증하는 테스트.
7. Tokenizer-aware budget과 truncation plan을 provider/model별로 검증하는 테스트.
8. Explicit extra context config가 live agent loop에 연결되는 테스트.
9. Diagnostics와 replay가 live source 재조회와 raw secret 노출 없이 snapshot evidence를 해석하는 테스트.
10. README, usage, specs index 중 사용자에게 노출되는 문서가 migration, JSON compatibility, profiles, secret refs, 비범위를 정확히 반영한다.
11. 닫는 문서에는 구현 파일, 테스트 이름, migration compatibility 판단, snapshot immutability evidence, TOML 결정 여부가 함께 기록되어야 한다.

현재 이 문서는 Open 상태다. 위 evidence가 없으면 035의 범위를 구현 완료로 닫을 수 없다.
