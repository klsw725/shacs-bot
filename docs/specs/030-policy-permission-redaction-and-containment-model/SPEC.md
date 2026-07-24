# 030. policy, permission, redaction, and containment model 아키텍처 명세

Status: Open

Origin specs: 004, 005, 007, 010, 011, 022, 023

## 문서 목적

이 문서는 이미 닫힌 001부터 027의 구현 범위를 다시 열지 않고, 그 문서들이 future gap 또는 교차 관심사로 남긴 policy, permission, redaction, containment 계약을 하나의 open owner boundary로 받는다.

목표는 다음과 같다.

1. Tool 실행, orchestrator policy, host safety, subagent inheritance, auto approval, containment evidence가 같은 snapshot 언어를 쓰게 한다.
2. Permission 판단을 user prompt, tool 내부, plugin hook, child runtime이 각자 해석하지 못하게 한다.
3. Secret과 redaction을 diagnostics, audit, replay, classifier accounting, process envelope 전반에서 같은 typed model로 다룬다.
4. Containment가 permission을 대체하지 않고, permission이 containment 증거 없이 host safety를 주장하지 않게 한다.
5. Self-hosted / personal-use 제품에서 사용자가 직접 이해하고 복구할 수 있는 fail-closed evidence를 남긴다.

핵심 문장:

```text
정책 판단은 실행 직전의 고정 snapshot, typed capability, redacted evidence, inherited ceiling을 함께 보아야 하며, 어느 하나만으로 권한을 확장할 수 없다.
```

## 구현된 기준선

이 문서가 받는 origin spec의 닫힌 범위는 다음을 기준선으로 본다.

1. 004는 tool registry, runtime executor, tool result, interrupt, skipped call, execution event 경계를 닫았다.
2. 007은 strong orchestrator authority, turn ownership, policy decision owner 의미를 닫았다.
3. 010은 current local host safety, workspace guard, secret boundary, redaction 원칙의 기초를 닫았다.
4. 011은 subagent tool registry restriction, execution config inheritance, parent boundary보다 넓어지지 않는 child runtime 의미를 닫았다.
5. 022는 permission mode, capability taxonomy, permissioned action normalization, static rule, runtime policy gate, approval correlation, inherited ceiling, audit diagnostics, replay matrix, guarded classifier fallback의 구현 범위를 닫았다.
6. 023은 Docker/Compose containment evidence, native unknown fallback, exec workspace narrowing, MCP/subagent snapshot inheritance, release evidence lane을 닫았다.
7. 005는 read-only skill discovery, descriptor, body hash, requirements/install metadata inspect, context injection 경계를 닫았으며 skill content 자체는 permission grant source가 아니다.

이 기준선은 이 문서의 open scope가 이미 구현됐다는 뜻이 아니다. 030은 흩어진 기준선을 하나의 formal model로 묶고, 현재 닫힌 spec들이 주장하지 않은 typed secret ref, unified redaction, classifier model routing/accounting, structured process envelope, containment inheritance evidence를 완성 조건으로 받는다.

## 소유하는 open scope

030은 다음 계약을 소유한다.

1. Formal policy snapshot과 safety snapshot의 생성 시점, 필드, 불변성, redacted display 의미.
2. Capability evaluator가 tool, command, plugin, MCP, subagent, app action을 canonical capability와 target ref로 정규화하는 규칙.
3. Approval correlation, expiry, consumed state, exact retry, stale rejection의 공통 모델.
4. Parent session, subagent, plugin, MCP, app process가 상속하는 permission ceiling과 containment ceiling.
5. Diagnostics, audit, replay, local API, TUI, channel projection에서 쓰는 unified redaction model.
6. Config, environment, provider auth, plugin requirement, MCP env, app manifest가 참조하는 typed secret ref 모델.
7. Process start, exec, MCP stdio, plugin command, app process를 설명하는 structured process envelope.
8. Classifier model routing, classifier capability ceiling, classifier latency/cost/accounting, denial fallback counter.
9. Containment inheritance와 fail-closed evidence가 action denial, degraded health, release evidence에 남는 방식.
10. 032가 생산하고 035가 영속하는 skill trust record를 dependency install과 검증된 skill entrypoint action의 permission 근거로 소비하는 규칙.

## Invariants

1. Policy snapshot은 action 실행 직전에 고정되어야 하며, 실행 중 prompt, tool result, hook output이 snapshot source가 될 수 없다.
2. Safety snapshot은 containment, workspace boundary, protected target, secret exposure 가능성을 함께 담아야 한다.
3. Capability evaluator는 allow 결정을 만들지 않는다. Capability와 target ref를 계산하고, policy evaluator가 그 결과를 소비한다.
4. Approval은 action digest, session or turn identity, capability set, target ref, policy snapshot digest와 상관되어야 한다.
5. Expired, consumed, mismatched approval은 fail closed 한다.
6. Child runtime은 parent permission ceiling과 containment ceiling을 넓힐 수 없다.
7. Redaction은 표시 표면마다 새로 추측하지 않고 같은 classification과 redaction rule을 소비해야 한다.
8. Secret 값은 config, diagnostics, replay, classifier prompt, process envelope에 raw로 들어가면 안 된다.
9. Classifier는 policy owner가 아니다. Classifier verdict는 bounded input과 accounting evidence를 가진 evaluator 신호다.
10. Containment evidence가 unknown이면 sandboxed 또는 safe로 표시하지 않는다.
11. Skill trust는 사용자가 설치 시 승인한 source identity, skill content digest, dependency manifest digest, capability scope가 모두 일치할 때만 유효하다. Skill 이름이나 Markdown 지시는 permission grant source가 아니다.

## Must Have

1. `PolicySnapshot`과 `SafetySnapshot`에 해당하는 typed record가 action, approval, audit, replay, diagnostics에서 같은 digest를 공유해야 한다.
2. 모든 side-effect action은 실행 전 `CapabilityEvaluation`을 가져야 한다.
3. Approval request와 approval decision은 correlation id, expiry, action digest, consumed state를 가져야 한다.
4. Subagent, MCP stdio child, plugin command, app process는 parent ceiling과 containment snapshot을 상속했다는 evidence를 남겨야 한다.
5. Redaction engine은 secret, token, credential path, signed URL, auth header, user-local absolute path, prompt-sensitive content의 분류와 출력 규칙을 제공해야 한다.
6. Typed secret ref는 secret value와 ref metadata를 분리해야 하며, missing, present, invalid, inaccessible 상태를 raw value 없이 표현해야 한다.
7. Structured process envelope은 command identity, args digest, cwd policy, env secret refs, timeout, exit status, containment snapshot, permission decision ref를 담아야 한다.
8. Classifier model routing은 main model과 분리 가능한 설정, capability ceiling, budget/accounting, failure reason을 가져야 한다.
9. Fail-closed decision은 사용자가 이해할 수 있는 redacted reason과 release evidence locator를 남겨야 한다.
10. Skill-derived action의 policy input은 032의 active trust record ref, current skill/content digest, dependency manifest digest, requested capability scope를 포함해야 한다.
11. 승인된 manifest에 고정된 Python/Node package 준비와 검증된 entrypoint 실행만 trust match를 auto allow 근거로 사용할 수 있다. Protected target, secret, containment static deny는 classifier allow보다 우선하되, interactive auto mode에서는 사용자에게 redacted approval을 요청하고 일치하는 명시 승인 뒤에만 실행할 수 있다.

## Must Not Have

1. 조직 RBAC, 관리자 승인 console, fleet policy rollout을 기본 제품 흐름으로 도입하지 않는다.
2. 중앙 secret vault를 필수 dependency로 삼지 않는다.
3. Docker 또는 process envelope을 kernel isolation 보증으로 광고하지 않는다.
4. Tool 내부 코드, plugin hook, subagent prompt가 permission mode나 ceiling을 높이게 하지 않는다.
5. Classifier allow만으로 protected target, missing containment, secret exposure approval gate를 우회하지 않는다. Interactive auto mode의 일치하는 명시 사용자 승인만 해당 action의 실행을 허용할 수 있다.
6. Redaction을 diagnostics 화면별 ad hoc string replace로 끝내지 않는다.
7. Secret ref metadata를 secret value 저장소로 쓰지 않는다.
8. Approval retry를 raw executable payload의 durable replay로 만들지 않는다.
9. Native unknown containment를 warning-only로 낮추고 side-effect 실행을 계속하지 않는다.
10. 설치된 skill 이름만 보고 package install, shell command, entrypoint 실행을 자동 승인하지 않는다.
11. Manifest에 없는 package, global install, 미선언 lifecycle script/native build, Python/Node runtime 자체 설치를 skill trust에서 파생해 허용하지 않는다.

## Acceptance Criteria

1. Policy snapshot과 safety snapshot이 permissioned action, approval, audit, replay, diagnostics에서 같은 digest lineage로 확인된다.
2. Capability evaluator가 core tool, deferred tool, MCP tool, plugin tool, subagent action, app process action을 같은 taxonomy로 분류한다.
3. Approval expiry, consumed state, mismatch, exact one-shot retry가 unit test와 runtime integration test로 검증된다.
4. Parent ceiling보다 넓은 child permission 또는 containment request가 거절되고, 그 이유가 redacted diagnostics에 남는다.
5. Unified redaction rule이 CLI, TUI, local API, diagnostics bundle, replay evidence, classifier input logging에서 같은 결과를 만든다.
6. Typed secret ref는 provider auth, channel credential, MCP env, plugin requirement, app manifest에서 raw value 없이 projection된다.
7. Structured process envelope이 exec, MCP stdio, plugin command, app process start에 공통으로 기록된다.
8. Classifier model routing은 별도 model 선택, unsupported capability fallback, latency/cost accounting, deny candidate visibility를 증명한다.
9. Containment inheritance가 unknown 또는 unsafe이면 fail closed 하거나 명시적으로 좁아진 scope로 내려가며, release evidence가 그 결정을 확인한다.
10. Skill trust match, stale/revoked/mismatched trust, manifest 밖 install, runtime 자체 누락이 각각 allow, ask/deny, prerequisite 상태로 구분되고 policy/audit test로 검증된다.

## Source Handoff Table

| Origin spec | 닫힌 범위 | 030으로 넘어온 open 계약 |
|---|---|---|
| 004 tool runtime | Tool execution, result, interrupt, skipped event | Tool 실행 전후에 붙는 policy snapshot, process envelope, capability evidence |
| 005 skill system | Read-only skill discovery, descriptor, body hash, requirements/install metadata inspect, context injection | Skill content를 grant로 승격하지 않으면서 032 lifecycle trust provenance를 action policy에 소비하는 규칙 |
| 007 main orchestrator policy | Orchestrator authority, turn ownership | Orchestrator가 소비하는 formal policy and safety snapshot schema |
| 010 host safety, permissions, and secrets | Current local safety guard, secret boundary, redaction principle | Unified redaction, typed secret refs, fail-closed safety snapshot evidence |
| 011 subagent runtime | Child tool restriction, execution config inheritance | Inherited permission and containment ceilings across child execution |
| 022 auto approval permissions | Permission mode, approval correlation, audit, replay, guarded classifier fallback | Cross-surface approval expiry, classifier routing/accounting, unified evaluator model |
| 023 zero-setup sandbox execution | Docker/Compose evidence, native unknown handling, containment inheritance baseline | Structured containment inheritance evidence and unsafe/unknown fail-closed release proof |

## Closure Evidence

030은 아래 증거가 모두 연결될 때 닫을 수 있다.

1. Static evidence: snapshot, capability, approval, redaction, secret ref, process envelope 타입이 compile-time boundary로 분리되어 있다.
2. Unit evidence: evaluator, redaction, secret ref projection, approval expiry, containment classification의 실패 경로가 테스트된다.
3. Integration evidence: direct tool, deferred tool, MCP stdio, plugin command, subagent, app process가 같은 policy pipeline을 통과한다.
4. Diagnostics evidence: CLI, TUI, local API, diagnostics bundle이 같은 redacted reason과 digest lineage를 보여 준다.
5. Replay evidence: recorded process envelope과 policy snapshot으로 live side effect 없이 decision을 설명할 수 있다.
6. Classifier evidence: classifier route, model id, capability ceiling, fallback reason, latency/cost counter가 raw prompt secret 없이 남는다.
7. Containment evidence: official container, native unknown, unsafe privileged, child inheritance case가 release smoke 또는 regression test로 증명된다.
8. Documentation evidence: old specs가 030을 open owner로 링크해도 기존 closed scope와 새 open scope가 충돌하지 않는다.
9. Skill trust evidence: exact digest/scope match만 선언된 dependency 준비를 허용하고 stale, revoked, manifest 밖 install, runtime 자체 설치는 fail closed 하는 테스트가 있다.
