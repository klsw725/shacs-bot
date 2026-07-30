# 030. policy, permission, redaction, and containment model 아키텍처 명세

Status: Open

Origin specs: 004, 005, 007, 010, 011, 022, 023

## 문서 목적

이 문서는 이미 닫힌 001부터 027의 구현 범위를 다시 열지 않고, 004, 005, 007, 010, 011, 022, 023이 남긴 policy, permission, redaction, containment 교차 계약을 현재 구현 기준으로 정리한다.

030의 현재 계약은 하나의 새 중앙 모델을 이미 요구하지 않는다. 현재 Rust 구현에 존재하는 분산 primitive, 즉 permission mode snapshot, permissioned action normalization, static policy decision, approval correlation, remembered permission rule, permission ceiling, redaction helper, containment evidence, diagnostics, read-only skill discovery metadata를 서로 충돌하지 않는 기준선으로 인정한다.

목표는 다음과 같다.

1. 현재 permission action, audit, diagnostics, containment projection이 어떤 보장을 실제로 제공하는지 분명히 한다.
2. Permission 판단을 user prompt, tool 내부, plugin hook, child runtime, skill content가 임의로 확장하지 못하게 한다.
3. Redaction과 digest가 raw payload 무결성 proof나 secret exfiltration prevention이 아니라, 현재 표면의 best-effort projection과 correlation evidence임을 고정한다.
4. Containment가 permission을 대체하지 않고, unknown containment가 safe 또는 sandboxed 증거로 표시되지 않게 한다.
5. Self-hosted / personal-use 제품에서 사용자가 직접 이해하고 복구할 수 있는 redacted evidence를 남긴다.

핵심 문장:

```text
현재 기준선은 분산된 permission, approval, redaction, containment 기록의 조합이며, 어떤 기록도 단독으로 권한을 확장하거나 안전을 보장하지 않는다.
```

## 구현된 기준선

이 문서가 받는 origin spec의 닫힌 범위는 다음을 기준선으로 본다.

1. 004는 tool registry, runtime executor, tool result, interrupt, skipped call, execution event 경계를 닫았다. Tool 내부 구현이나 모든 process start의 공통 permission gate를 닫은 것은 아니다.
2. 005는 read-only skill discovery, descriptor, body hash, requirements/install metadata inspect, context injection 경계를 닫았다. Skill content, skill name, Markdown instruction은 permission grant source가 아니다.
3. 007은 strong orchestrator authority, turn ownership, policy decision owner 의미를 닫았다. 완성된 중앙 policy engine을 닫은 것은 아니다.
4. 010은 current local host safety, workspace guard, secret boundary, redaction 원칙의 기초를 닫았다. Typed secret ref나 complete secret prevention을 닫은 것은 아니다.
5. 011은 subagent tool registry restriction, execution config inheritance, parent boundary보다 넓어지지 않는 child runtime 의미를 닫았다. 완전한 containment inheritance proof를 닫은 것은 아니다.
6. 022는 permission mode, capability taxonomy, permissioned action normalization, static rule, runtime policy gate, approval correlation, inherited ceiling, audit diagnostics, replay matrix, guarded classifier fallback의 구현 범위를 닫았다. Classifier 비용, 지연, model routing accounting의 완성 계약은 아니다.
7. 023은 Docker/Compose containment evidence, native unknown fallback, exec workspace narrowing, MCP/subagent snapshot inheritance, release evidence lane을 닫았다. Kernel isolation, universal process gating, native unknown의 안전 보장은 아니다.

따라서 030의 현재 앞부분은 이 분산 기준선을 conformant baseline으로 둔다. 더 강한 통합 typed snapshot, typed secret ref, 공통 process envelope, classifier accounting, containment inheritance proof, trust-derived package install과 verified entrypoint authorization은 현재 runtime 사용을 막지 않는 deferred 또는 non-guaranteed contract이며, Spec 030 최종 closure에서는 아래 implementation PRD owner가 닫아야 할 명시적 대상이다.

## 소유하는 open scope

030은 다음 계약을 소유한다.

1. 현재 permissioned action normalization, action digest, snapshot digest, target summary, audit record가 어떤 correlation evidence로 충분한지 설명한다.
2. 현재 approval correlation, expiry, consumed state, exact retry, stale rejection이 어떤 실행 표면에 적용되는지 설명한다.
3. Current static policy rule, protected target rule, permission ceiling, guarded classifier fallback이 권한 확장이 아니라 실행 직전 gate로 소비되는 범위를 설명한다.
4. Current redaction helper와 diagnostics projection의 보장 범위를 설명하고, raw payload integrity proof와 complete secret prevention은 비보장으로 둔다.
5. Docker/Compose containment evidence, native unknown fallback, exec workspace narrowing, MCP/subagent inheritance projection이 어떤 evidence로 충분한지 설명한다.
6. Unknown containment는 safe 또는 sandboxed 증거가 아니며, 현재 ask, deny, narrowing policy로만 다뤄진다는 점을 소유한다.
7. Read-only skill discovery, descriptor, body hash, requirements/install metadata inspect가 permission input으로 승격되지 않는 경계를 소유한다.
8. Trust-derived package install, dependency preparation, verified entrypoint authorization은 032와 035가 더 구체적인 trust record를 제공하기 전까지 현재 runtime use blocker가 아니며, 최종 closure에서는 아래 implementation PRD owner와 external gate를 통과해야 함을 소유한다.
9. Plugin command와 process path가 존재하더라도 모든 process start가 같은 permission gate를 통과한다는 보장은 현재 비보장으로 둔다.
10. 통합 typed snapshot, typed secret ref, 공통 process envelope, classifier accounting, containment inheritance proof는 current baseline과 혼동하지 않는 closure target으로 표시한다.

## Implementation PRDs

이 섹션은 Spec 030의 current baseline을 구현 완료로 바꾸지 않는다. 아래 문서는 모두 계획된 spec-local PRD이며, 문서가 존재하거나 색인된 사실만으로 구현되었다고 보지 않는다.

1. `000-unified-policy-safety-correlation-snapshot.md`
2. `001-typed-secret-references-and-redaction-provenance.md`
3. `002-common-process-envelope-and-side-effect-gate.md`
4. `003-containment-inheritance-and-permission-ceiling-proof.md`
5. `004-classifier-routing-budget-and-accounting.md`
6. `005-skill-trust-permission-provenance-and-verified-entrypoints.md`
7. `006-sequential-integration-and-spec030-closure.md`

### Stronger Contract Owner Map

각 stronger contract는 정확히 하나의 Spec 030 implementation PRD owner를 가진다.

| Stronger contract | Sole Spec 030 owner | Boundary |
|---|---|---|
| Unified policy and safety correlation snapshot | PRD 000 | Permission, approval, audit, replay, diagnostics, and process consumers share one immutable typed correlation ref. Spec 035 still owns config, context, provider execution snapshot persistence, and migration. |
| Typed secret references and redaction provenance | PRD 001 | Raw secret values stay outside snapshots, receipts, and diagnostics. Secret resolution lifecycle from app binding and config persistence remains external to Specs 032 and 035. |
| Common process envelope and side-effect permission gate | PRD 002 | Exec, plugin command/tool, MCP stdio, app process, dependency preparation, and verified entrypoint launches use one pre-spawn policy envelope. AppSupervisor lifecycle, installer behavior, MCP protocol semantics, and physical snapshot persistence remain external. |
| Containment inheritance and permission-ceiling proof | PRD 003 | Parent-child containment evidence and ceiling comparison must prove equal-or-narrower execution before admission. UI, diagnostics, and release projection remain Spec 031 output work. |
| Classifier routing, budget, and accounting | PRD 004 | Classifier route, latency, token, cost, fallback, and unavailable accounting become typed evidence that cannot override static deny or ceiling. Projection and release rendering remain Spec 031, and provider execution snapshot persistence remains Spec 035. |
| Skill trust permission provenance and verified entrypoints | PRD 005 | Active trust provenance from external lifecycle owners becomes a bounded permission input for dependency preparation and verified entrypoints. Trust registry transitions, dependency installation, runner lifecycle, and persistence remain Specs 032 and 035. |

PRD 006 is not a seventh domain owner. It is the only sequential integration and closure owner, and it may authorize changing this document from `Status: Open` to `Complete (Scoped)` only after PRDs 000 through 005 pass and every external dependency gate below has evidence.

### Internal Dependency Gates

1. PRD 000 establishes the immutable policy and safety correlation ref before PRDs 002, 003, 004, 005, or 006 can use it as closure evidence.
2. PRD 001 establishes typed secret refs and redaction provenance before PRDs 002, 005, or 006 can carry secret-safe process and trust evidence.
3. PRD 002 establishes the common process envelope before PRDs 003, 005, or 006 can require shared side-effect admission and receipt correlation.
4. PRDs 003, 004, and 005 each close their own stronger domain before PRD 006 can run the final integration gate.
5. PRD 006 cannot introduce a new domain model, cannot accept partial closure, and cannot close Spec 030 by grep-only or prose-only evidence.

### External Dependency Gates

Spec 030 consumes the following external owner outputs but does not own their implementation.

1. Spec 031 owns UI projection, diagnostics projection parity, release evidence rendering, and release runner outputs used to display or package Spec 030 evidence.
2. Spec 032 owns AppSupervisor, app start/stop/recover lifecycle, trust registry state transitions, inspect/revoke lifecycle, and app/skill lifecycle binding that supplies active trust provenance.
3. Spec 035 owns config persistence, runtime layout, physical execution snapshots, provider/context snapshot persistence, immutable snapshot storage, migration, and trust persistence consumed by Spec 030.

### Baseline Conformance vs Final Closure

The current implemented baseline remains conformant for runtime use while Spec 030 is Open. Current conformance means the distributed permission, approval, redaction, containment, diagnostics, and read-only skill evidence does not contradict this spec and does not grant permissions by itself.

Final closure is stricter. It requires all six stronger domain contracts above, the internal dependency gates, and the 031, 032, and 035 external gates. Until PRD 006 records that evidence, deferred and non-guaranteed clauses remain non-blockers for current runtime use and explicit closure targets for later implementation.

## Invariants

1. 현재 기준선은 normalized permissioned action, action digest, snapshot digest, target refs, capability set, permission mode snapshot, containment snapshot ref, remembered permission rule id의 조합이다. 이 digest와 rule id는 correlation/UX evidence이며 raw-payload integrity proof가 아니다.
2. Approval은 approval request id, action digest, snapshot digest, requested scope, expiry, consumed state와 상관되어야 한다. Expired, consumed, mismatched, inspect-only approval은 실행 허가로 쓰지 않는다. Project remembered rule은 approval decision에서 파생되더라도 다음 action에서 다시 static rule, protected target rule, ceiling, containment rule을 통과해야 한다.
3. Static rule은 protected target, raw credential export, proc exec summary, containment 상태를 classifier, approval, remembered allow보다 먼저 소비한다. Static deny는 classifier allow, approval, remembered allow보다 우선하고, static ask는 현재 interactive ask 또는 deny semantics를 따른다.
4. Permission ceiling과 child execution context는 parent보다 넓어질 수 없다. 현재 evidence는 inherited context와 ceiling projection으로 충분하며, 하나의 통합 snapshot type을 요구하지 않는다.
5. Redaction은 action normalization, approval text, diagnostics, skill disclosure가 공유 helper를 소비하는 best-effort projection이다. Redaction은 secret exfiltration prevention이 아니며, 모든 민감 정보 제거를 보장하지 않는다.
6. Containment evidence는 official container, non-privileged confirmation, native unknown 같은 실행 환경 projection이다. Unknown containment는 safe 또는 sandboxed evidence가 아니며, 현재 ask, deny, narrowing policy semantics로만 다룬다.
7. Diagnostics와 release evidence는 raw provider payload, raw tool args, raw secret value 대신 redacted reason, digest, locator, summary를 남겨야 한다.
8. Skill boundary는 read-only discovery, descriptor, body hash, requirements or install metadata inspect, context injection에 머문다. Skill name, Markdown instruction, skill content는 permission grant source가 아니다.
9. Plugin command와 process path는 존재하지만 보편적으로 같은 permission gate를 통과한다고 볼 수 없다. 030은 현재 registry 또는 dispatcher별 경계를 있는 그대로 설명한다.
10. User prompt, tool result, plugin hook output, subagent prompt, skill content는 permission mode나 ceiling을 높일 수 없다.
11. Remembered permission store는 config data directory의 `permissions.json`에 project bucket별 rule로 저장된다. Store key는 canonical workspace id이고, projection은 현재 workspace bucket만 보여준다. Missing store는 empty rule set으로 읽히지만 malformed, oversized, symlink, non-regular store는 unavailable evidence로 fail closed한다.
12. Remembered matcher precedence는 deny가 allow보다 먼저이며, session-scoped remembered rules와 project-scoped remembered rules는 같은 action normalization, matcher, static safety, ceiling, containment decision 이후에만 소비된다. Project revoke는 현재 canonical workspace bucket의 rule id prefix에만 적용되고, absent/ambiguous prefix는 mutation 없이 실패해야 한다.

## Must Have

1. Runtime tool call과 deferred bridge call은 redacted arguments, argument digest, action digest, snapshot digest, target refs, capability set으로 normalize되어야 한다.
2. Approval request와 decision은 approval request id, expiry, action digest, snapshot digest, requested or approved scope, consumed state를 상관해야 한다.
3. Approval mismatch, expiry, inspect-only decision, consumed decision은 allow가 아니어야 하며, current policy는 interactive context에 따라 ask 또는 deny로 좁혀야 한다. Approval에서 파생된 remembered allow도 protected/static safety 또는 permission ceiling을 우회하지 못해야 한다.
4. Static policy는 protected target, raw credential export, missing proc exec summary, unknown containment, bypass-permissions containment 확인 실패를 현재 rule decision으로 표현해야 한다.
5. Permission mode baseline과 inherited ceiling은 side-effect 실행 전에 소비되어야 하며, child나 workflow가 parent ceiling보다 넓은 capability를 얻지 못하게 해야 한다.
6. Shared redaction helper는 secret key, token, auth header, credential path, private key block 같은 현재 분류를 value와 string 표면에 적용해야 한다. 이 조항은 best-effort redaction을 요구할 뿐 complete exfiltration prevention을 요구하지 않는다.
7. Containment evidence는 confirmed non-privileged와 unknown을 구분해야 한다. Unknown containment는 safe 또는 sandboxed evidence로 승격하지 않고, proc exec와 bypass permissions에서 현재 ask, deny, narrowing semantics를 따라야 한다.
8. Diagnostics, approval prompt, release evidence는 self-hosted 사용자가 이해할 수 있는 redacted reason, digest, summary, locator를 남겨야 한다. Redacted digest는 correlation evidence이며 raw executable payload replay나 raw-payload integrity proof가 아니다.
9. Skill disclosure는 list, view, reference boundary와 descriptor, body digest, redacted body, requirements or install metadata inspect를 제공해야 한다. 이 read-only evidence는 package install, shell command, verified entrypoint authorization의 auto allow 근거가 아니다.
10. Plugin command와 process 실행 경로는 현재 구현된 dispatcher, tool registry, timeout, env clearing, redacted output 경계로 설명해야 한다. 모든 plugin command, app process, MCP stdio child가 하나의 공통 process envelope이나 같은 policy pipeline을 통과한다고 요구하지 않는다.
11. Stronger `PolicySnapshot`, `SafetySnapshot`, typed secret ref, structured process envelope, classifier budget or latency accounting, trust-derived package preparation은 current baseline Must Have가 아니라 deferred 또는 non-guaranteed contract로 남아야 하며, final closure에서는 각 implementation PRD owner가 닫아야 한다.
12. Remembered permission read surfaces는 rule id prefix, effect, matcher summary, created/last-used/use-count, store health 같은 redacted projection만 보여줘야 하며 raw action payload, raw provider content, secret-like malformed store text를 출력하지 않아야 한다.

## Must Not Have

1. 조직 RBAC, 관리자 승인 console, fleet policy rollout을 기본 제품 흐름으로 도입하지 않는다.
2. 중앙 secret vault를 필수 dependency로 삼지 않는다.
3. Docker, Compose, bwrap, process envelope을 kernel isolation 보증으로 광고하지 않는다.
4. User prompt, tool 내부 코드, plugin hook, subagent prompt, skill content가 permission mode나 ceiling을 높이게 하지 않는다.
5. Classifier allow만으로 protected target, raw credential export, missing proc exec summary, unknown containment, secret exposure approval gate를 우회하지 않는다.
6. Redaction을 complete secret prevention, exfiltration prevention, raw-payload integrity proof로 설명하지 않는다.
7. Redacted digest를 raw executable payload의 durable replay나 raw-payload integrity proof로 만들지 않는다.
8. Native unknown containment를 harmless, sandboxed, safe, warning-only, always executable 상태로 낮추지 않는다.
9. Plugin command, app process, MCP stdio child, 모든 side-effect action이 하나의 universal permission gate나 같은 policy pipeline을 통과한다고 주장하지 않는다.
10. 설치된 skill 이름, skill Markdown, skill content digest만 보고 package install, shell command, entrypoint 실행을 자동 승인하지 않는다.
11. Manifest에 없는 package, global install, 미선언 lifecycle script/native build, Python/Node runtime 자체 설치를 skill trust에서 파생해 허용하지 않는다.
12. `PolicySnapshot`, `SafetySnapshot`, typed secret ref, structured process envelope 같은 아직 없는 unified type을 현재 conformance의 필수 조건으로 되살리지 않는다.
13. Remembered allow, session approval, project approval, prompt text, tool result, plugin hook output을 protected target, static deny, permission ceiling, malformed store fail-closed, containment precondition 우회 근거로 쓰지 않는다.

## Acceptance Criteria

1. Runtime tool call과 deferred bridge call은 현재 permissioned action shape로 normalize되고, provider call id, tool name, action id, action digest, argument digest, snapshot digest, target refs, capability set이 redacted action record에 남는다.
2. Action digest와 snapshot digest는 target, capability, permission mode, containment context 차이를 구분하는 correlation evidence로 검증된다. 이 기준은 raw payload replay나 raw-payload integrity proof를 요구하지 않는다.
3. Approval correlation은 request id, action digest, snapshot digest, approved scope, expiry, inspect-only decision, consumed state mismatch를 allow로 쓰지 않는다. 유효한 approval만 ask-required action의 실행 handoff를 허용하며, protected/static deny와 ceiling은 approval 또는 remembered allow보다 우선한다.
4. Permission mode snapshot과 inherited ceiling은 baseline capability를 좁히며, child, app task, deferred bridge가 parent보다 넓은 mode나 capability를 얻지 못한다.
5. Static policy는 protected target, raw credential export, unknown target classification, secret read, dangerous or unsummarized proc exec, unknown containment, bypass-permissions containment failure를 현재 ask 또는 deny decision으로 좁힌다.
6. Shared redaction helper는 secret key, token, auth header, credential path, private key block, inline env assignment를 value와 string projection에서 가린다. 이 기준은 best-effort redaction 검증이며 complete exfiltration prevention을 요구하지 않는다.
7. Permission action, audit record, diagnostics, runtime diagnostics projection은 raw secret value 대신 redacted argument, digest, reason, summary, locator를 남긴다.
8. Containment projection은 official or recognized container evidence, native unknown, optional hardening, unsafe privileged evidence를 구분한다. Unknown 또는 unsafe evidence는 safe나 sandboxed 증거가 아니며, 현재 policy는 proc exec와 bypass-permissions 경로를 ask, deny, or default fallback으로 좁힌다.
9. Skill surface는 read-only discovery, descriptor, body hash, requirements/install metadata inspect, context injection evidence로 제한된다. Skill name, Markdown body, body digest, plugin-provided skill content는 permission mode source나 package install, shell command, verified entrypoint authorization grant가 아니다.
10. Plugin command, MCP stdio child, app process start, classifier route and accounting, typed secret ref, structured process envelope, unified PolicySnapshot or SafetySnapshot은 current baseline acceptance blocker가 아니다. 이들은 final closure target이며, implementation PRD owner와 external owner가 더 강한 trust, entrypoint, execution snapshot 계약을 제공할 때 별도 evidence로 닫는다.
11. Remembered permission projection은 `permissions.json` store의 현재 workspace bucket을 대상으로 하고, matcher summary는 exact action digest prefix, exec arity prefix, workspace exact/subtree path, web origin, MCP tool name 중 구현된 형태만 표시한다. Corrupt store는 CLI/slash/API/TUI read surface와 runtime decision에서 raw content 없이 unavailable/fail-closed evidence로 나타난다.

## Source Handoff Table

| Origin spec | 닫힌 범위 | 030으로 넘어온 open 계약 |
|---|---|---|
| 004 tool runtime | Tool execution, result, interrupt, skipped event | Current tool and deferred bridge calls consume permissioned action normalization, redacted arguments, digests, target refs, and capability mapping. A shared process envelope for every process path is deferred. |
| 005 skill system | Read-only skill discovery, descriptor, body hash, requirements/install metadata inspect, context injection | Current 030 consumes skill metadata only as read-only evidence and as a non-grant boundary. Trust-derived dependency install, package preparation, and verified entrypoint authorization are deferred to later trust and execution contracts. |
| 007 main orchestrator policy | Orchestrator authority, turn ownership | Current 030 consumes permission mode snapshot, approval correlation, and turn ownership as distributed policy evidence. A formal unified policy and safety snapshot schema is deferred. |
| 010 host safety, permissions, and secrets | Current local safety guard, secret boundary, redaction principle | Current 030 consumes shared redaction helpers, secret-read denial, raw auth export rules, and redacted diagnostics. Typed secret refs and complete secret prevention are not current guarantees. |
| 011 subagent runtime | Child tool restriction, execution config inheritance | Current 030 consumes inherited permission context and ceiling projection so child and deferred boundaries cannot widen capability or bypass per-action evaluation. A complete containment inheritance proof is deferred. |
| 022 auto approval permissions | Permission mode, approval correlation, audit, replay, guarded classifier fallback | Current 030 consumes mode baselines, static policy decisions, approval mismatch/expiry/consumed rejection, audit diagnostics, and replay invariants. Classifier route, latency, and cost accounting are not current acceptance requirements. |
| 023 zero-setup sandbox execution | Docker/Compose evidence, native unknown handling, containment inheritance baseline | Current 030 consumes containment projection that distinguishes confirmed non-privileged evidence, native unknown, optional hardening, and unsafe privileged evidence. Unknown containment is not safe evidence, and universal process gating or kernel isolation proof is deferred. |

## Closure Evidence

030은 Status Open이다. 아래 증거는 현재 relaxed open contract가 구현 기준선과 충돌하지 않는다는 문서 conformance 증거이며, Spec 030 완료 선언이 아니다.

1. Static evidence: current permission mode snapshot, permissioned action, approval correlation, permission ceiling, static rule, redaction helper, containment projection type이 각 구현 경계에서 소비된다.
2. Unit evidence: permission_action tests가 direct tool, deferred bridge, capability mapping, stable digest, target-sensitive digest, redacted argument, unsafe raw secret, permission context digest 변화를 검증한다.
3. Policy evidence: permission_policy tests가 protected target, raw auth export, secret read, unknown target, dangerous or unsummarized proc exec, unknown containment, bypass-permissions containment failure를 ask 또는 deny로 좁히는 현재 decision을 검증한다.
4. Approval evidence: approval correlation tests가 request mismatch, action mismatch, snapshot mismatch, scope mismatch, expiry, inspect-only decision, consumed decision rejection과 valid approval acceptance를 검증한다.
5. Ceiling evidence: inherited ceiling tests가 subagent, app task, deferred bridge boundary에서 mode widening, capability widening, app declaration-only grant, deferred gate bypass를 거절한다.
6. Redaction evidence: shacs-redaction tests와 permission action/audit tests가 shared helper redaction과 redacted diagnostics projection을 검증한다. 이 증거는 complete secret prevention이나 raw-payload integrity proof가 아니다.
7. Containment evidence: runtime containment classifier and permission policy tests가 native unknown, unsafe privileged, official container, optional hardening, confirmed non-privileged projection을 구분하고, unknown 또는 unsafe evidence를 safe evidence로 승격하지 않음을 검증한다.
8. Skill evidence: config and CLI skill tests가 prompt, skill instruction, app manifest, session memory, tool result를 trusted permission mode source로 보지 않고, skill list/show가 descriptor와 body hash 같은 read-only evidence만 노출함을 검증한다.
9. Documentation evidence: 이 문서의 acceptance, handoff, closure evidence는 current distributed baseline을 설명하고, unified snapshot, typed secret ref, shared process envelope, classifier accounting, trust-derived dependency install, verified entrypoint authorization을 current baseline acceptance blocker로 되살리지 않는다. 이 대상들은 Implementation PRDs section의 owner map과 PRD 006 closure gate에서만 final closure evidence로 판정한다.
10. Remembered permission evidence: CLI/runtime tests verify project allow persistence, new-session reuse, external revoke then re-prompt, protected store target blocking, malformed store fail-closed behavior, redacted projection, and current workspace-only revoke. This is current baseline evidence, not Spec 030 closure.
