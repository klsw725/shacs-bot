# App Maker와 app authoring 아키텍처 명세

Status: Draft. PRD 000의 안전한 `apps init` authoring draft baseline은 구현됐다. 이 문서는 App Maker 전체의 app 작성, 검토, 제안, 설치 인계 owner contract를 계속 열린 상태로 고정한다.

## 문서 목적
이 문서는 `docs/SYSTEM-FOUNDATION.md`와 numbered spec set 전체를 바탕으로 `shacs-bot`의 App Maker 경계를 정의한다.
목표는 다음과 같다.
1. App Maker를 app runtime이 아니라 app authoring과 proposal 표면으로 고정한다.
2. 사용자가 직접 설치하고 운영하는 개인용 런타임 관점에서 안전한 app 초안 작성 흐름을 정의한다.
3. AI assisted authoring이 manifest, skill, device, tool, service 선언 후보를 만들 수 있지만 실행 권한을 얻지 못한다는 점을 명확히 한다.
4. 작성 결과가 018의 proposal, approval, checkpoint, apply, verify 흐름을 거쳐야만 설치 가능한 bundle로 넘어간다는 계약을 세운다.
5. 구현된 PRD 000 baseline과 이후 App Maker 절편에서 draft, candidate, validation report, authoring proposal, receipt, install handoff 타입과 테스트를 도출할 수 있게 한다.
App Maker가 편해질수록 권한 경계가 흐려질 위험이 커진다. 이 문서는 무엇을 만들 수 있는지보다 무엇을 자동으로 하면 안 되는지를 먼저 고정한다.

---

## 제품 정의
App Maker는 사용자가 새 `.shacsapp` app을 만들거나 기존 app bundle 변경안을 준비하도록 돕는 authoring surface다.
App Maker가 하는 일:
1. 사용자의 의도와 입력 자료를 바탕으로 app authoring draft를 만든다.
2. scaffold plan과 manifest candidate를 제안한다.
3. skill draft와 device/tool/service declaration candidate를 만든다.
4. 정적 검증, dry-run, diff, risk summary를 제공한다.
5. authoring proposal을 생성해 018의 approval/checkpoint/apply/verify 흐름으로 넘긴다.
6. 승인되고 적용된 결과를 017의 app install 흐름으로 인계한다.
App Maker가 하지 않는 일:
1. app process를 시작하지 않는다.
2. MCP server를 자동 실행하지 않는다.
3. secret value를 읽거나 주입하지 않는다.
4. permission grant를 만들거나 승인하지 않는다.
5. skill을 active 상태로 직접 주입하지 않는다.
6. 설치된 app registry를 조용히 변경하지 않는다.
7. app runtime 또는 process supervisor가 되지 않는다.
핵심 문장:
```text
App Maker는 app을 실행하는 곳이 아니라, app 변경을 설명 가능한 proposal로 만드는 곳이다.
```

---

## 상위 기준과의 관계
이 문서는 다음 기준을 전제로 한다.
1. `shacs-bot`은 self-hosted / personal-use assistant runtime이다. 기본 주체는 사용자가 직접 설치하고 운영하는 본인이다.
2. `MainOrchestrator`는 세션 상태, permission 확정, effect 실행의 유일한 권한자다.
3. App Maker는 authoring draft와 proposal evidence를 만들 수 있지만 owner primitive를 직접 대체하지 않는다.
4. app install은 실행이 아니다. 이 원칙은 App Maker가 만든 bundle에도 그대로 적용된다.
교차 spec 관계:
| spec | App Maker가 소비하는 것 | App Maker가 소유하는 것 |
|---|---|---|
| 005 skill system | Markdown skill, read-only context, discovery와 주입 경계 | skill draft와 authoring validation. active 주입은 소유하지 않음 |
| 008 configuration profiles and runtime layout | config, data-dir, runtime layout, storage location owner 경계 | authoring draft storage를 008의 data-dir/runtime layout 아래에서 소비함. 별도 installed app path를 만들지 않음 |
| 010 host safety, permissions, and secrets | permission declaration, approval boundary, secret redaction, MCP default-deny | authoring 중 위험 선언을 감지하고 보고하는 계약. 승인이나 secret 처리는 소유하지 않음 |
| 013 user interfaces and session UX | CLI, future TUI, local API projection과 approval surface 의미 | authoring projection과 review loop의 제품 의미. transport 세부 구현은 소유하지 않음 |
| 014 observability, diagnostics, and inspection | observability, diagnostics, trace, redaction evidence surface | authoring receipt의 의미와 app authoring evidence 구성. diagnostics, trace, redaction 표면은 014를 소비 |
| 016 verification matrix and release gates | 정적, 단위, 통합, UX, 안전성, release gate 관점 | App Maker 전용 검증 관점과 완료 기준 |
| 017 app operating environment | `.shacsapp` bundle, manifest, registry, install/list/inspect/show/enable/disable/uninstall 의미 | 설치 전 authoring과 install handoff. app runtime은 소유하지 않음 |
| 018 evaluation, automation, and self-improvement | improvement proposal, approval, checkpoint, apply, verify, rollback 순서 | authoring proposal 생성과 evidence 구성. approval/checkpoint/apply는 소유하지 않음 |
| 020 tool search and provider tool surface | provider-visible tool surface와 tool schema 노출 경계 | tool declaration candidate 작성 의미. generated tool을 provider-visible surface에 노출하지 않음 |
이 문서는 017을 약화하지 않는다. App Maker가 만든 결과도 017의 app manifest, bundle, install semantics를 통과해야 한다.
이 문서는 018을 우회하지 않는다. App Maker가 만든 변경은 승인 전까지 runtime behavior를 바꾸지 않는다.
이 문서는 010을 과장하지 않는다. formal permission engine이 future work인 경우에도 App Maker는 현재 local safety baseline을 넘는 보장을 현재 기능처럼 말하면 안 된다.

---

## 범위
이 문서는 다음을 정의한다.
1. App Maker owner boundary.
2. app authoring draft와 scaffold plan lifecycle.
3. manifest candidate, skill draft, device/tool/service declaration candidate 작성 규칙.
4. validation report와 dry-run 의미.
5. authoring proposal과 authoring receipt 형식 요구.
6. install handoff의 의미와 금지 행동.
7. CLI `apps init` baseline 의미, AI assisted authoring proposal, edit/review loop, validation/dry-run, explicit install handoff.
8. future local API와 TUI projection 의미.
9. 상태 모델, 불변식, 금지 패턴, Rust checkpoint, 검증 관점.
이 문서는 다음을 정의하지 않는다.
1. app process 실행 모델. 이는 017이 소유한다.
2. app registry install/list/inspect/show/enable/disable/uninstall semantics. 이는 017이 소유한다.
3. permission approval engine과 secret vault backend. 이는 010이 소유한다.
4. skill discovery, precedence, active injection 전체. 이는 005가 소유한다.
5. MCP server 내부 구현이나 process supervisor. 이는 004, 012, 017의 경계를 따른다.
6. TUI widget 설계, HTTP framework, visual design. 이는 013의 projection 경계를 소비한다.
7. 조직 app catalog, 관리자 승인, fleet rollout, remote marketplace 운영.
8. PRD 구현 계획. 이 문서는 owner spec이고 PRD를 만들지 않는다.

---

## 현재 구현 상태
현재 저장소에는 PRD 000의 안전한 `apps init` authoring draft baseline이 구현되어 있다.
구현된 절편:
1. `apps init <app-id>` parser와 CLI command.
2. app id validation.
3. data dir 아래 authoring draft store.
4. scaffold plan, manifest candidate, README candidate 생성.
5. idempotency, conflict, path safety 처리.
6. installed app registry mutation 없음.
구현 증거:
1. `crates/shacs-app/src/app_authoring.rs`
2. `crates/shacs-app/tests/app_authoring.rs`
3. `crates/shacs-cli/src/lib.rs`
4. `crates/shacs-core/tests/app_compat.rs`
이 baseline은 app을 install, enable, start하지 않는다. permission grant를 생성하지 않고, tool/service를 등록하지 않고, secret을 읽지 않고, installed app registry를 변경하지 않는다.
아직 열린 범위:
1. AI assisted authoring.
2. authoring proposal store.
3. baseline을 넘는 validation report.
4. authoring receipt.
5. approval/apply integration과 install handoff.
6. local API와 TUI projection.
따라서 full Spec 021과 full App Maker는 아직 닫힌 상태가 아니다. app bundle과 manifest baseline은 017, improvement proposal과 checkpoint/apply/verify는 018, host safety와 permission은 010의 구현 상태를 따른다.

---

## 핵심 정의

### App Maker
App Maker는 app bundle을 만들거나 수정하기 위한 authoring subsystem이다. 사용자는 App Maker를 통해 새 app 초안을 만들고, AI에게 manifest와 skill 초안 작성을 요청하고, validation report를 확인하고, 설치 인계 전 명시적으로 승인할 수 있다. App Maker는 runtime executor가 아니다.

### app authoring draft
app authoring draft는 아직 설치되지 않은 작업 초안이다. draft는 사용자의 의도, 입력 자료, 생성된 파일 후보, 검증 결과, review comment를 묶는다. draft는 app registry entry가 아니며, draft가 존재해도 app은 installed, enabled, active 상태가 되지 않는다.

### scaffold plan
scaffold plan은 App Maker가 만들 파일과 디렉터리, 그 이유, 예상 owner를 설명하는 계획이다. `manifest.json` candidate, `skills/SKILL.md` draft, `README.md` draft, `devices/mcp/*.json` candidate, validation plan을 포함할 수 있다. scaffold plan 승인은 permission approval이 아니다.

### manifest candidate
manifest candidate는 017의 app manifest schema로 검증될 수 있는 후보 문서다. candidate의 permission, secret, device, tool 선언은 요청 후보일 뿐이다. grant, secret binding, MCP registration, process start를 만들지 않는다.

### skill draft
skill draft는 app bundle에 포함될 수 있는 Markdown skill 후보 문서다. draft 상태에서는 active skill registry에 들어가지 않고 provider context에 자동 주입되지 않는다.

### device/tool/service declaration candidate
device declaration candidate는 MCP server, local service, remote adapter 같은 실행 경계를 설명한다. tool declaration candidate는 app이 노출하려는 tool surface를 설명한다. service declaration candidate는 scheduled job, channel worker, background helper 같은 runtime service 요구를 설명한다. 세 candidate 모두 실행, 등록, 노출, 시작을 만들지 않는다.

### validation report
validation report는 draft와 candidate가 spec 계약을 만족하는지 점검한 결과다. 최소 항목은 manifest schema validation result, app id와 bundle path 규칙 검사, permission과 secret declaration summary, MCP/device command static risk summary, skill draft parse status와 forbidden runtime instruction 검사, install readiness와 blocker 목록, 018 proposal 가능 여부다. validation report는 allow/deny 권한 판정이 아니다.

### authoring proposal
authoring proposal은 App Maker가 만든 draft를 실제 bundle 생성 또는 기존 bundle 변경 후보로 넘기는 018 improvement proposal의 app authoring 특화 형태다. 필수 내용은 proposal id, draft id, target kind, diff summary, risk summary, expected behavior, validation report reference, checkpoint requirement, rollback plan, install handoff plan이다. 승인 전에도 isolated authoring draft store에는 draft, candidate, validation report, receipt 같은 authoring artifact를 쓸 수 있다. 그러나 승인 전에는 installed app bundle path mutation, app registry mutation, active skill registry changes, tool exposure changes, service state changes 같은 target/runtime side effect를 만들면 안 된다. secret value write, process start, MCP start도 금지한다.

### authoring receipt
authoring receipt는 App Maker authoring session의 설명 가능한 기록이다. receipt는 draft id, proposal id, 사용자 intent summary, 생성된 candidate 목록과 digest, validation report digest, 사용자 review decision, proposal handoff result, redacted warning과 blocker를 담는다. secret value, provider hidden reasoning, raw token, 필요 이상의 file content는 저장하지 않는다.

### install handoff
install handoff는 승인되고 checkpoint/apply/verify 흐름을 통과한 authoring output을 017의 install 표면으로 넘기는 명시적 단계다. install handoff는 자동 설치가 아니다. 사용자는 bundle path, manifest digest, permission과 secret 요청을 확인해야 한다.

---

## 사용자 흐름

### 1. `apps init` 목표 흐름
`apps init`의 PRD 000 baseline은 현재 CLI에 있다. 이 명령은 authoring draft와 최소 candidate만 만들며 full App Maker 흐름을 닫지 않는다.
```text
shacs-bot apps init <app-id> --workspace <path>
```
구현된 baseline 단계는 draft 생성, scaffold plan 작성, manifest candidate와 README candidate 작성, idempotency/conflict/path safety 확인이다. AI assisted authoring, baseline을 넘는 validation report, review loop, authoring proposal 생성은 아직 열린 범위다.
금지:
1. app install 처리.
2. generated manifest의 permission 승인.
3. MCP command 실행.
4. secret prompt로 secret value 저장.

### 2. AI assisted authoring proposal
사용자는 자연어로 app 목적을 설명할 수 있다. App Maker는 목적을 capability 후보로 요약하고, manifest candidate, skill draft, secret key 이름, 최소 permission declaration, MCP/device 후보, risk summary, validation report를 제안한다.
App Maker는 사용자의 로컬 환경에서 package manager를 실행하거나, secret value를 읽거나, test call을 위해 secret을 요청하거나, 기존 installed app을 묵시적으로 수정하면 안 된다.

### 3. edit/review loop
review loop는 draft 내부에서만 일어난다.
```text
draft created
-> scaffold plan generated
-> user reviews
-> candidates generated
-> validation report updated
-> user edits
-> validation report updated
-> proposal requested
```
규칙:
1. 모든 edit은 draft revision을 만든다.
2. validation report는 revision digest로 연결한다.
3. AI rewrite는 사용자 검토 전 다음 lifecycle 단계로 넘어갈 수 없다.
4. draft store와 installed app store는 분리한다.
5. 기존 installed app을 대상으로 할 때도 draft는 원본 app registry를 직접 바꾸지 않는다.

### 4. validation과 dry-run
validation은 정적 검증과 안전한 구조 검사를 뜻한다. dry-run은 manifest parse, bundle layout 검사, skill draft parse, permission/secret summary, device command 문자열 risk summary, install handoff preview만 허용한다.
dry-run은 process spawn, MCP server start, network probe, package install, secret read, permission grant 생성, app registry mutation을 금지한다.

### 5. explicit install handoff
proposal이 승인되고 checkpoint/apply/verify 흐름을 통과하면 App Maker는 install handoff를 표시한다. handoff 출력은 bundle path, manifest digest, app id, name, version, declared skills, declared devices/tools/services, requested secret key names, requested permissions, validation summary, 다음 명시 action을 포함해야 한다.
handoff 이후 실제 install은 017의 app operating environment가 소유한다. App Maker는 install 결과를 자기 상태처럼 확정하지 않는다.

### 6. future local API와 TUI projection
local API와 TUI는 013의 projection 원칙을 따른다.
필수 projection 범주:
1. `AuthoringDraftProjection`: draft id, target, current revision, validation status, blockers.
2. `ScaffoldPlanProjection`: 생성 예정 파일, 이유, risk label.
3. `ManifestCandidateProjection`: identity, entry kind, permission/secret summary, schema status.
4. `AuthoringValidationProjection`: report id, checked revision, warnings, blockers, install readiness.
5. `AuthoringProposalProjection`: proposal id, approval state, checkpoint state, apply state, verify state.
6. `InstallHandoffProjection`: approved bundle target, manifest digest, required next command.
projection은 표시 모델이다. projection이 draft, proposal, registry truth가 되면 안 된다.

---

## 상태 모델
App Maker lifecycle은 다음 상태를 가진다.
```text
draft
-> generated
-> validated
-> proposal_pending
-> approved
-> checkpointed
-> applied
-> install_handoff
```
대체 종료 상태는 `rejected`, `failed`, `archived`다.
세부 의미:
1. `draft`: 사용자 intent와 초기 metadata만 있다.
2. `generated`: scaffold plan 또는 candidate files가 생성됐다.
3. `validated`: 현재 revision에 대한 validation report가 있다.
4. `proposal_pending`: authoring proposal이 생성됐고 승인 대기 중이다.
5. `approved`: 018 approval flow에서 proposal scope가 승인됐다.
6. `checkpointed`: 적용 전 복구 기준이 확보됐다.
7. `applied`: owner primitive를 통해 authoring output이 draft store 또는 target staging area에 적용됐다.
8. `install_handoff`: 017 install로 넘길 준비가 됐고 사용자의 명시 action을 기다린다.
9. `rejected`: 사용자가 proposal을 거절했거나 scope가 맞지 않아 종료됐다.
10. `failed`: validation, checkpoint, apply, verify 중 실패했고 recovery hint가 필요하다.
11. `archived`: 사용자가 draft를 보관했고 active authoring 목록에서 빠진다.
허용 전이:
1. `draft`에서 `generated`, `archived`.
2. `generated`에서 `validated`, `failed`, `archived`.
3. `validated`에서 `proposal_pending`, `generated`, `archived`.
4. `proposal_pending`에서 `approved`, `rejected`, `failed`.
5. `approved`에서 `checkpointed`, `failed`.
6. `checkpointed`에서 `applied`, `failed`.
7. `applied`에서 `install_handoff`, `failed`.
8. `failed`에서 `generated`, `validated`, `archived`.
금지 전이는 `draft`에서 `install_handoff`, `generated`에서 `approved`, `validated`에서 `applied`, `proposal_pending`에서 `applied`, `install_handoff`에서 installed app registry 확정이다.

---

## 저장과 파일 경계
App Maker는 installed app bundle 저장소와 draft 저장소를 분리해야 한다.
```text
<data-dir>/authoring/apps/<draft-id>/
  draft.json
  revisions/
  candidates/
  reports/
  receipts/
```
이 layout은 설계 어휘다. 실제 Rust 구현은 008의 runtime layout과 017의 app bundle layout을 소비해 결정해야 한다.
규칙:
1. draft store는 installed registry가 아니다.
2. generated candidate는 명시 apply 전 installed app bundle path를 덮어쓰면 안 된다.
3. existing app edit은 원본 manifest를 읽기 전용 snapshot으로 복사한 뒤 draft에서 diff를 만들어야 한다.
4. receipt와 report는 digest와 redacted summary를 저장하고 raw secret value를 저장하지 않는다.
5. archive는 draft discovery에서 제외하는 의미이며, historical receipt 삭제가 아니다.

---

## safety first authoring 규칙
Secret handling:
1. App Maker는 secret key name과 required 여부만 다룬다.
2. App Maker는 secret value를 읽지 않는다.
3. App Maker는 secret value를 manifest, skill draft, receipt, validation report에 쓰지 않는다.
4. App Maker는 secret 존재 여부를 확인할 필요가 있더라도 raw value를 노출하지 않는 owner API만 사용해야 한다.
5. secret binding은 install 또는 process 실행 단계의 owner가 010과 017 계약 아래에서 처리한다.
Permission handling:
1. manifest candidate의 permission은 요청 후보이다.
2. App Maker는 grant를 생성하지 않는다.
3. App Maker는 approval을 대신하지 않는다.
4. broad permission은 validation warning 또는 blocker가 될 수 있다.
5. wildcard widening은 별도 proposal이 필요하다.
6. permission summary는 사용자가 이해할 수 있는 target, duration, reason을 포함해야 한다.
Process, MCP, skill handling:
1. App Maker는 process를 자동 실행하지 않는다.
2. App Maker는 MCP server를 자동 시작하지 않는다.
3. App Maker는 package manager, shell command, local service health check를 authoring validation이라는 이름으로 실행하지 않는다.
4. App Maker는 command string을 정적으로 분석하고 위험을 설명할 수 있다.
5. skill draft는 active skill로 자동 노출되지 않는다.
6. 실제 device 준비, process start, skill discovery와 active injection은 각 owner spec 경계를 따른다.

---

## hard invariants
1. App Maker는 app runtime executor가 아니다.
2. authoring draft는 installed app이 아니다.
3. scaffold plan은 permission approval이 아니다.
4. manifest candidate는 grant가 아니다.
5. skill draft는 active skill이 아니다.
6. device declaration candidate는 running MCP server가 아니다.
7. validation report는 permission decision이 아니다.
8. authoring proposal은 승인 전 runtime behavior를 바꾸지 않는다.
9. install handoff는 install 완료가 아니다.
10. App Maker는 secret value를 읽거나 저장하지 않는다.
11. App Maker는 installed app registry를 묵시적으로 변경하지 않는다.
12. App Maker는 existing app을 수정할 때도 018 proposal, approval, checkpoint, apply, verify 순서를 건너뛰지 않는다.
13. App Maker는 017의 install, enable, start 의미를 재정의하지 않는다.
14. App Maker는 010의 permission boundary를 약화하지 않는다.
15. 모든 authoring 결과는 사용자가 draft id, proposal id, validation report, receipt로 추적할 수 있어야 한다.

---

## 금지 패턴
다음 패턴은 구현 편의를 위해서도 허용하지 않는다.
1. `apps init` 직후 자동 `apps install` 실행.
2. manifest candidate에 적힌 MCP command를 validation 중 실행.
3. authoring 중 package install, network probe, auth test call 수행.
4. secret key name을 보고 local env나 config에서 raw value를 읽기.
5. generated skill draft를 바로 active skill registry에 넣기.
6. generated tool declaration을 현재 provider-visible tool surface에 추가하기.
7. generated service declaration을 scheduler나 service worker에 등록하기.
8. proposal approval 없이 installed app bundle을 덮어쓰기.
9. checkpoint 없이 기존 app manifest를 수정하기.
10. validation warning을 숨기고 install ready로 표시하기.
11. TUI 또는 local API projection이 승인된 것처럼 상태를 꾸미기.
12. app task나 AI authoring agent가 자기 proposal을 직접 승인하기.
13. App Maker가 registry install semantics를 재구현하기.
14. 조직 관리자 승인, 중앙 catalog 운영, fleet rollout을 기본 흐름으로 끌어오기.
15. 실패한 draft를 자동 삭제해 사용자가 원인과 receipt를 볼 수 없게 만들기.

---

## 정상 및 실패 시퀀스
새 app 작성:
```text
user intent
-> authoring draft created
-> scaffold plan generated
-> user review
-> candidates generated
-> validation report created
-> authoring proposal created
-> 018 approval pending
-> approval accepted
-> checkpoint created by owner flow
-> apply through owner primitive
-> verify
-> authoring receipt recorded
-> explicit install handoff
-> 017 install flow begins only after user action
```
기존 app 수정:
```text
installed app selected for edit
-> read-only app snapshot captured
-> draft created from snapshot
-> manifest candidate diff generated
-> validation report created
-> authoring proposal targets app_manifest_ref
-> 018 approval/checkpoint/apply/verify
-> install or update handoff through 017 owner boundary
```
거절 또는 검증 실패 흐름은 registry, skill, tool, service, secret, process mutation 없이 `rejected` 또는 `failed`로 기록되어야 한다. 사용자는 draft를 다시 편집하거나 archive할 수 있어야 한다.

---

## Rust 구현 체크포인트 이름
full App Maker 구현은 아래 이름을 직접 도출할 수 있어야 한다. 이 목록 전체가 현재 구현됐다는 뜻은 아니다.
Core types:
```text
AppAuthoringDraft
AppAuthoringDraftId
AppAuthoringRevision
AppAuthoringState
AppScaffoldPlan
AppScaffoldFilePlan
AppManifestCandidate
AppSkillDraft
AppDeviceDeclarationCandidate
AppToolDeclarationCandidate
AppServiceDeclarationCandidate
AppAuthoringValidationReport
AppAuthoringValidationFinding
AppAuthoringProposal
AppAuthoringReceipt
AppInstallHandoff
AppAuthoringStore
AppAuthoringProjection
AppAuthoringCommand
AppAuthoringEvent
```
State candidates: `Draft`, `Generated`, `Validated`, `ProposalPending`, `Approved`, `Checkpointed`, `Applied`, `InstallHandoff`, `Rejected`, `Failed`, `Archived`.
Command candidates: `CreateDraft`, `GenerateScaffoldPlan`, `GenerateCandidates`, `EditDraft`, `ValidateDraft`, `CreateAuthoringProposal`, `ArchiveDraft`, `CreateInstallHandoff`.
Event candidates: `DraftCreated`, `ScaffoldPlanGenerated`, `CandidatesGenerated`, `DraftEdited`, `ValidationCompleted`, `AuthoringProposalCreated`, `AuthoringProposalRejected`, `AuthoringCheckpointLinked`, `AuthoringApplyRecorded`, `AuthoringVerifyRecorded`, `InstallHandoffCreated`, `DraftArchived`, `AuthoringFailed`.

---

## 검증 관점
016의 verification family를 기준으로 App Maker는 아래 관점을 통과해야 한다.
정적 검증:
1. authoring state transition이 enum exhaustiveness로 닫혀 있는지.
2. secret value와 secret key name 타입이 분리되는지.
3. draft store와 installed registry store 타입이 섞이지 않는지.
4. install handoff가 install result와 다른 타입인지.
단위 테스트:
1. `apps init` 목표 parser가 draft 생성 command로만 매핑되는지.
2. manifest candidate validation이 permission declaration을 grant로 바꾸지 않는지.
3. device declaration validation이 command를 실행하지 않는지.
4. lifecycle forbidden transition이 거부되는지.
5. validation report가 blocker와 warning을 구분하는지.
6. authoring receipt가 secret-like field를 redaction하는지.
통합 테스트:
1. new app draft 생성부터 proposal pending까지 registry mutation이 없는지.
2. existing app edit draft가 installed bundle을 덮어쓰지 않는지.
3. authoring proposal이 018 proposal state와 연결되지만 직접 apply하지 않는지.
4. approved/checkpointed/applied/verified 흐름 뒤에만 install handoff가 생기는지.
5. install handoff 이후 실제 install은 017 command boundary를 통과하는지.
안전성, UX, 복구 테스트:
1. secret value가 draft, report, receipt, diagnostics에 남지 않는지.
2. MCP command가 validation/dry-run 중 실행되지 않는지.
3. broad permission과 wildcard tool exposure가 warning 또는 blocker로 표시되는지.
4. app task 또는 authoring agent가 자기 proposal을 승인하지 못하는지.
5. generated skill이 active context에 자동 주입되지 않는지.
6. CLI, future TUI, local API가 draft/proposal/handoff 상태를 같은 의미로 표시하는지.
7. validation blocker가 install ready처럼 보이지 않는지.
8. rejected와 failed가 사용자가 이해할 수 있는 next action을 제공하는지.
9. install handoff가 명시 action을 요구하는지.
10. proposal pending 상태에서 재시작해도 apply가 자동 재개되지 않는지.
11. checkpoint unavailable이면 apply가 blocked 상태로 남는지.
12. failed apply와 failed verify가 receipt와 projection에 남는지.

---

## 완료 기준
App Maker full implementation은 아래 조건을 충족해야 한다.
1. `apps init` 목표 surface가 draft 생성으로만 동작하고 install, enable, start를 하지 않는다.
2. AI assisted authoring이 manifest candidate, skill draft, device/tool/service declaration candidate를 만들 수 있다.
3. validation/dry-run은 정적 검사만 수행하고 process, MCP, package, network, secret access를 실행하지 않는다.
4. authoring proposal은 018의 approval/checkpoint/apply/verify 순서를 소비한다.
5. install handoff는 승인되고 검증된 output만 017 install 흐름으로 넘긴다.
6. secret value, grant, active skill, running process, service registration, tool exposure가 authoring만으로 생기지 않는다.
7. CLI, future TUI, local API projection이 같은 authoring 상태를 표시할 수 있다.
8. 016 관점의 정적, 단위, 통합, 안전성, UX, 복구 테스트가 App Maker 범위를 검증한다.

---

## 결론
App Maker의 제품 가치는 app authoring을 쉽게 만드는 데 있지만, 안전한 경계는 더 중요하다.
따라서 App Maker는 사용자가 app을 만들도록 돕는 제안 표면이어야 한다. 실행, 승인, secret 주입, permission grant, registry install, process start는 각 owner spec의 경계를 통과해야 한다.
이 계약을 지키면 사용자는 저수준 manifest와 skill 파일을 전부 손으로 쓰지 않아도 된다. 동시에 어떤 app이 어떤 권한을 요청하고, 어떤 파일이 생성됐고, 어떤 검증을 통과했으며, 언제 설치 경계로 넘어갔는지 설명받을 수 있다.
