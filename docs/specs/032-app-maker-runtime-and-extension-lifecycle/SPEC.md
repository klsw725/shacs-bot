# 032. App Maker runtime과 extension lifecycle 아키텍처 명세

Status: Open

Origin specs: 005, 017, 021, 025

## 목적

이 문서는 기존 specs 005, 017, 021, 025가 implemented scope로 닫힌 뒤 남는 app product work의 owner boundary를 연다.

핵심 목적은 App Maker가 만든 proposal과 현재 사용자의 authoring/apply decision을 설치 가능한 `.shacsapp` bundle, 실제 app process, AppSupervisor, extension lifecycle provenance로 연결하는 것이다. Install은 process start와 executable activation이 아니다. Executable resource의 activation eligibility와 trusted-code disclosure는 030을 소비하고, 032는 app-level lifecycle transition·blocker·receipt를, 035는 activation persistence와 snapshot reference를 소유한다.

이 문서는 기존 spec의 완료 선언을 되돌리지 않는다. 005는 read-only skill registry와 context injection, 017은 app manifest와 registry baseline, 021은 안전한 authoring draft baseline, 025는 self-hosted local plugin manifest와 제한된 executable surface를 닫은 것으로 본다. 032는 이 baseline을 소비하는 app lifecycle과 authoring-to-install 제품 흐름만 소유한다.

## 현재 구현 baseline

현재 구현은 다음 baseline을 가진다.

1. 005는 Markdown skill discovery, source kind, status, descriptor, context injection, CLI inspect surface를 갖는다. Skill은 read-only 지식 팩이며 permission이나 tool visibility를 직접 얻지 않는다.
2. 017은 local `.shacsapp` manifest, app registry, lifecycle state, process snapshot projection, task ledger receipt baseline을 갖는다. Install은 process start가 아니며 secret value와 permission grant를 만들지 않는다.
3. 021은 `apps init` authoring draft baseline을 갖는다. Draft store, scaffold plan, manifest candidate, README candidate, idempotency, path safety가 구현됐고 installed app registry를 바꾸지 않는다.
4. 025는 local plugin manifest discovery, config activation gate, hook dispatch, `tool:before` block-only behavior, command-backed plugin tool, plugin MCP declaration, plugin skill root, plugin command router, replay live-dispatch rejection을 갖는다.

이 baseline은 아래를 완료로 주장하지 않는다.

1. App process의 실제 start, stop, restart, recover lifecycle.
2. AppSupervisor가 app device, service, MCP, skill exposure를 live runtime에서 함께 관리하는 경계.
3. App Maker proposal이 current-user authoring/apply decision, checkpoint, apply, verify, receipt, install handoff를 거쳐 설치로 이어지는 end-to-end flow.
4. Existing app edit이 안전한 snapshot, diff, proposal, apply, install 또는 update handoff를 거쳐 완료되는 flow.
5. Extension lifecycle provenance가 app install, enable, start, disable, uninstall, recover receipt와 하나로 이어지는 증거 체계.

## owned open scope

032가 소유하는 open scope는 다음이다.

1. 실제 app process start, stop, restart, recover.
2. `AppSupervisor`의 owner boundary, state model, recovery input, shutdown behavior, stale process handling.
3. App manifest의 runtime requirement와 credential declaration이 trusted runtime profile, credential binding status, process environment로 넘어가는 handoff.
4. AI-assisted app proposal, validation, receipt, apply, install flow.
5. Existing app edit flow, read-only installed snapshot, draft diff, proposal, checkpoint, apply, verify, install 또는 update handoff.
6. App lifecycle 안에서 발생하는 extension, skill, plugin, hook, command, MCP provenance의 소유 위치.
7. App install, enable, start, stop, disable, uninstall, recover의 domain state vocabulary와 projection input contract. Shared surface adapter와 parity smoke는 031이 소유한다.
8. App process와 extension action이 task ledger, diagnostics, replay evidence에 redacted receipt로 남는 규칙.
9. App bundle이 선언한 executable resource metadata와 install provenance, 030 activation result를 소비하는 app-level lifecycle blocker, source invalidation, inspect/disable/revoke linkage.

032가 다른 spec에서 소비하는 것과 다시 소유하지 않는 것은 명확히 분리한다. 004/005/010/012/013/014/016/017/021/022/025는 구현 baseline을 제공한다. 030은 trusted runtime profile, executable resource activation eligibility/disclosure, credential status, sandbox/data disclosure를 소유하고 035는 config/profile auth source와 activation persistence를 소유한다. Shared UI adapter와 parity smoke는 031이 소유하며 032는 app/resource lifecycle state와 receipt를 생산한다.

## invariants

1. Discovery는 descriptor와 source를 등록할 수 있지만 executable surface를 노출하지 않는다. App install은 bundle과 metadata를 저장할 뿐 app execution이나 executable activation이 아니다.
2. App Maker proposal의 current-user apply decision은 draft/manifest/diff mutation을 허용하는 authoring decision이다. Tool authorization, permission grant, remembered allow, credential authorization, executable activation, replay authorization이 아니다.
3. AppSupervisor는 `MainOrchestrator`를 대체하지 않는다. AppSupervisor는 process lifecycle executor와 evidence producer다.
4. App process start 전에는 현재 사용자가 선택한 config 또는 trusted-workspace state, trusted runtime profile, credential binding status, 030 activation result를 참조한 extension activation snapshot이 확정돼야 한다. AppSupervisor가 이를 새 권한이나 approval cache로 만들지 않는다.
5. Secret value는 app bundle, manifest, receipt, ledger, diagnostics, provider prompt에 raw로 저장되지 않는다.
6. App process는 start 시점의 manifest digest, trusted runtime profile ref, credential source names/status, extension activation set을 receipt에 남겨야 한다.
7. Markdown skill은 005, command-backed plugin/hook은 025, trusted executable resource는 030, config/auth source persistence는 035 경계를 소비한다.
8. Disabled, blocked, missing-credential, untrusted-workspace app은 active process, active executable skill, provider-visible tool, live hook, MCP startup을 만들 수 없다.
9. Existing app edit은 installed bundle을 직접 덮어쓰지 않는다. Snapshot, draft, diff, proposal, checkpoint, apply, verify를 거쳐야 한다.
10. Uninstall과 recover는 historical ledger와 session reference를 조용히 파괴하지 않는다.
11. Replay는 destructive app process나 plugin command를 live-dispatch하지 않는다. Recorded evidence만 해석한다.
12. Executable resource activation eligibility는 030의 explicit config 또는 trusted workspace assertion과 source identity, content digest, dependency manifest digest에 묶인다. 032는 그 결과를 app lifecycle에서 소비한다.
13. 활성 resource의 Python/Node package 준비·검증·entrypoint 실행은 030 trusted runtime profile과 adapter별 process/sandbox control 범위에서만 수행할 수 있다. 032는 결과와 blocker를 receipt에 기록한다.
14. Skill content, dependency manifest, resolved version, source identity가 바뀌면 activation record는 stale이 되고 다음 session에서 재발견·재활성화해야 한다.
15. Python/Node package 누락과 Python/Node runtime 자체 누락은 다른 상태다. Runtime 자체 누락은 managed runtime 또는 prerequisite로 처리하며 package trust로 임의 system installer를 허용하지 않는다.
16. Executable resource activation lifecycle은 discovery registry와 분리된 inspectable·disable/revoke 가능한 state다. 030이 eligibility와 disclosure를, 032가 app-level transition/linkage를, 035가 persistence/migration을 소유하며 permission grant나 approval cache가 아니다.

## Must Have

1. `AppSupervisor` typed model, app process id, app process state, lifecycle event, recovery input, shutdown reason을 정의해야 한다.
2. Start flow는 registry lookup, enabled state check, manifest digest check, missing credential check, trusted runtime profile check, extension activation snapshot, process creation, ledger receipt를 포함해야 한다.
3. Stop flow는 user requested stop, graceful shutdown attempt, timeout result, cancellation evidence, final process state를 포함해야 한다.
4. Recover flow는 interrupted process, stale marker, missing receipt, partial extension activation, device startup failure를 redacted diagnostics와 next action으로 보여줘야 한다.
5. Credential binding handoff는 manifest declaration을 030/035 credential source request로 바꾸고 032 receipt에는 raw secret을 저장하지 않아야 한다.
6. AI-assisted proposal flow는 user intent, generated candidates, validation report, risk summary, receipt, authoring/apply decision linkage, checkpoint linkage, apply result, install handoff를 추적해야 한다. 이 decision은 runtime authorization으로 재사용하지 않는다.
7. Existing app edit flow는 installed snapshot digest, draft revision, manifest diff, extension diff, validation report, proposal, checkpoint, apply, verify, update handoff를 가져야 한다.
8. Extension lifecycle provenance는 app이 제공한 skill, plugin, hook, command, MCP declaration의 source app id, manifest digest, enabled state, 030 activation ref/status, blocked reason을 inspect와 receipt에 남겨야 한다. Activation decision 자체는 032가 재결정하지 않는다.
9. 032는 app registry state, process state, missing credential, untrusted workspace, extension blocker, last receipt의 domain vocabulary와 evidence를 생산해야 하며, 031의 shared adapter가 CLI, local API, future TUI에서 같은 의미로 투영할 수 있어야 한다.
10. Release tests는 normal path, blocked path, untrusted workspace, missing credential, failed start, failed stop, recover after interruption, replay-safe diagnostics를 포함해야 한다.
11. Executable resource proposal은 source identity, content digest, dependency manifest digest, dependency resolution, required runtime, lifecycle/native build 요구사항, trusted-code disclosure를 구조화해야 한다.
12. Activation record는 activation source, workspace trust ref, source/content/dependency digest, active/stale/disabled/removed 상태와 사유를 가져야 한다.
13. Resource 사용 전에는 현재 digest와 activation record를 대조하고, declared Python/Node package가 없으면 030 runtime profile이 허용한 manifest 범위와 process/sandbox control 안에서 준비한 뒤 expected package/version을 검증해야 한다.
14. 사용자는 활성 executable resource, source, digest 일치 여부, stale/disabled reason을 inspect하고 개별 activation을 disable 또는 revoke할 수 있어야 한다.
15. Resource install과 entrypoint execution은 030 trusted runtime diagnostics와 035 execution snapshot/lifecycle persistence에 연결하되 permission provenance를 만들지 않는다.

## Must Not Have

1. Marketplace, remote catalog, public app store, signed distribution protocol을 이 spec closure 조건으로 끌어오지 않는다.
2. Explicit config 또는 trusted workspace assertion 없이 in-process code, command-backed executable surface, MCP startup, live hook, provider-visible tool, executable skill을 노출하지 않는다. Discovery와 install만으로 activation을 추론하지 않는다.
3. Rust dynamic ABI, dynamic library plugin ABI, WASM 또는 scripting loader를 app runtime 기본 경로로 만들지 않는다.
4. SaaS control plane, admin approval console, organization catalog, fleet rollout을 기본 흐름으로 두지 않는다.
5. App Maker validation이라는 이름으로 package manager, shell command, MCP server, network auth test를 실행하지 않는다.
6. App manifest의 capability/resource declaration은 request와 disclosure metadata일 뿐이며 durable tool authorization, permission grant, credential grant, executable activation을 만들지 않는다.
7. Secret key name을 보고 raw secret value를 authoring, validation, diagnostics에 복사하지 않는다.
8. Generated skill, tool, hook, MCP declaration을 explicit activation 또는 trusted workspace assertion 없이 live runtime에 노출하지 않는다.
9. Disable이나 uninstall을 historical evidence 삭제로 해석하지 않는다.
10. AppSupervisor는 app lifecycle truth와 evidence만 확정한다. Session truth, credential status, activation eligibility, authoring decision을 runtime authorization truth로 직접 확정하지 않는다.
11. Markdown 본문의 자연어 `pip install`, `npm install`, shell snippet을 구조화된 dependency manifest 또는 사용자 승인으로 해석하지 않는다.
12. Skill 이름이 activation 목록에 있다는 이유로 변경된 script, 미선언 package, global install, lifecycle script, native build를 자동 실행하지 않는다.
13. Python/Node runtime 자체가 없을 때 임의의 `apt`, `brew`, global installer를 package trust에서 파생해 실행하지 않는다.
14. Dependency의 구체적인 설치 위치나 directory layout을 이 spec의 closure 조건으로 고정하지 않는다.

## executable resource install과 activation boundary

Executable resource의 discovery, install, activation, execution은 다음 단계로 분리한다.

1. **Discover:** source와 manifest를 찾아 descriptor, source identity, content digest를 만든다. Executable surface는 아직 없다.
2. **Propose:** App Maker 또는 installer가 candidate, dependency manifest, required runtime, lifecycle/native build 요구사항, trusted-code disclosure를 표시한다. Proposal은 authorization이 아니다.
3. **Install:** current-user apply decision에 따라 bundle, manifest, resource metadata를 registry에 저장한다. Process, hook, tool, MCP, entrypoint를 시작하지 않는다.
4. **Activate:** 030의 explicit config 또는 trusted workspace assertion과 source/content/dependency identity가 일치할 때 activation result가 생성·갱신되고 035가 persistence/migration한다.
5. **Prepare and verify:** 030 trusted runtime profile이 허용한 declared dependency 범위에서만 준비·검증한다. Manifest 밖 network source, lifecycle script, native build, global mutation은 중단한다.
6. **Execute:** expected package/version, digest, activation status, credential status, adapter별 process control, sandbox policy를 확인한 뒤 entrypoint를 실행한다.
7. **Invalidate:** source/content/dependency digest 변경은 stale이며 다음 session에서 재검토·재활성화해야 한다.
8. **Disable/revoke/remove:** 032는 app-level blocker와 historical receipt를 유지하고 030 activation semantics와 035 persistence를 소비한다.
9. 구체적인 dependency 설치 위치와 filesystem layout은 이 lifecycle 계약의 범위가 아니다.

## acceptance criteria

1. `AppSupervisor`와 app process lifecycle 타입이 코드에 존재하고 start, stop, restart, recover 상태 전이를 테스트로 고정한다.
2. `apps start`, `apps stop`, `apps restart`, `apps recover` 또는 같은 의미의 CLI/local API command가 installed app registry와 AppSupervisor boundary를 통과한다.
3. Missing credential, untrusted workspace, disabled app, blocked extension은 process start를 막고 user-facing projection과 diagnostics receipt를 남긴다.
4. Successful process start는 manifest digest, app id, trusted runtime profile ref, credential source names/status, extension activation snapshot, device state, receipt id를 남긴다.
5. Stop과 recover는 partial process와 stale marker를 숨기지 않고 next action과 evidence를 제공한다.
6. AI-assisted new app flow는 draft에서 validation, proposal, current-user authoring/apply decision linkage, checkpoint linkage, apply, verify, install handoff까지 traceable하며 그 decision이 runtime authorization으로 재사용되지 않는다.
7. Existing app edit flow는 installed snapshot을 직접 mutate하지 않고 diff와 checkpoint를 거친 뒤 install 또는 update handoff를 만든다.
8. Plugin-provided skill, command-backed tool, hook, MCP declaration은 app lifecycle provenance와 연결되지만 기존 owner runtime gate를 우회하지 않는다.
9. Replay runner와 diagnostics reader는 app process와 extension action을 live 재실행하지 않고 recorded evidence만 해석한다.
10. 016 기준 정적, 단위, 통합, 안전성, UX, 복구, replay release evidence가 032 coverage entry로 남는다.
11. Discovery-only와 installed resource가 provider-visible tool, live hook, MCP startup, executable skill exposure를 만들지 않고, activation lifecycle은 inspect, disable/revoke, stale, removed 상태를 재현할 수 있다.
12. Declared Python/Node package 누락은 030 trusted runtime profile이 허용할 때만 준비·검증되며, manifest 밖 install은 새 proposal과 030 activation decision 전까지 멈춘다.
13. 같은 skill 이름이어도 source/content/dependency digest가 바뀌면 기존 activation이 재사용되지 않는다.
14. Python/Node runtime 자체 누락은 package 누락과 구분되어 prerequisite 또는 별도 managed runtime 상태로 표시된다.
15. App start receipt가 030 activation ref, 035 execution snapshot ref, credential status를 연결하고 raw secret을 포함하지 않는다.
16. App-level active 상태가 adapter별 process control이나 sandbox 상태를 하나의 권한·격리 보장으로 뭉개지 않는다.

## source handoff table

| origin spec | 닫힌 implemented scope | 032로 넘어오는 open work |
|---|---|---|
| 005 skill system | Skill registry, discovery, status, descriptor, body hash, requirements/install metadata, read-only context injection, CLI inspect baseline | App bundle이 제공한 resource metadata, app-level provenance, activation snapshot/receipt linkage |
| 017 app operating environment | Local app manifest, registry, lifecycle state, process projection, task ledger receipt baseline | Actual app process start/stop/recover, AppSupervisor, process receipt, trusted-runtime/credential handoff, extension activation lifecycle |
| 021 app maker and app authoring | `apps init` draft baseline, scaffold and manifest candidate, installed registry non-mutation | AI-assisted proposal, validation, receipt, authoring decision/checkpoint/apply/verify integration, install handoff, existing-app edit flow |
| 025 user-extensible hooks and plugins | Local plugin manifest, activation gate, hook/tool/skill/MCP/command slice, replay live-dispatch rejection | App-owned extension provenance, app lifecycle tied enable/disable/start/recover evidence, extension blocker projection |
| 030 trusted agent runtime | Trusted profile, activation eligibility/disclosure, credential status, path-specific controls, sandbox status | App start gate가 owner facts를 소비하고 activation/process blocker를 receipt에 연결 |
| 035 configuration and snapshots | Config/profile/auth-source persistence, activation persistence, execution snapshot | App receipt가 snapshot/activation refs를 연결하되 schema·storage를 재소유하지 않음 |
| 031 projection parity | Shared projection schema and adapters | App/resource lifecycle owner facts와 safe receipts를 projection input으로 생산 |

## Implementation PRDs

Spec 032는 app authoring에서 process lifecycle과 closure까지 아래 단계로 구현한다. 각 PRD는 자신의 산출물만으로 종료되며 다른 spec의 `Complete` 상태를 요구하지 않는다. 외부 owner가 제공하는 exact fact가 없으면 fixture contract와 blocked handoff evidence를 남기되 해당 기능을 성공으로 주장하지 않는다.

| PRD | Sole owner scope | Depends on |
|---|---|---|
| [PRD 000](prds/000-app-supervisor-and-process-lifecycle.md) | AppSupervisor, start/stop/restart/recover state와 process receipt | 017 baseline, Specs 030/035 fact contracts |
| [PRD 001](prds/001-app-maker-proposal-apply-and-install.md) | Proposal, current-user authoring/apply decision, checkpoint, apply, verify, install/update | 021 baseline |
| [PRD 002](prds/002-extension-provenance-and-activation-boundary.md) | App-owned extension provenance, discovery/install/activation/execution boundary, lifecycle blocker | PRDs 000-001, Specs 025/030/035 fact contracts |
| [PRD 003](prds/003-sequential-integration-and-spec032-closure.md) | End-to-end app lifecycle integration, coverage/evidence, final Spec032 closure | PRDs 000-002, required owner-fact audits |

Current PRD status:

| PRD | Status |
|---|---|
| PRD 000 | Planned |
| PRD 001 | Planned |
| PRD 002 | Planned |
| PRD 003 | Planned |

Dependency rules:

1. PRD 001은 app process를 시작하거나 executable activation을 만들지 않는다.
2. PRD 002는 030 activation eligibility와 035 persistence를 소비하고 재정의하지 않는다.
3. PRD 003은 다른 spec closure status가 아니라 exact owner facts와 local evidence만 검사한다.
4. PRD 003의 requirement mapping, real-surface QA, artifact audit가 통과하기 전에는 Spec 032를 닫지 않는다.

## closure evidence

032는 아직 open 상태다. 이 spec을 닫으려면 아래 evidence가 저장소에 있어야 한다.

1. 코드 증거: `AppSupervisor`, app process lifecycle, app start/stop/recover command path, 030 credential/trusted-runtime owner-fact consumption, 035 snapshot linkage, extension provenance model.
2. 테스트 증거: lifecycle state tests, missing credential and untrusted workspace tests, extension blocked tests, existing app edit tests, proposal/apply/install handoff tests, replay safety tests.
3. 인터페이스 증거: registry, process, blocker, receipt, recover state에 대한 CLI와 local API output 또는 snapshot tests. Future TUI가 구현된 경우 같은 projection 의미를 써야 한다.
4. Diagnostics 증거: redacted app process receipt, extension activation receipt, failed start/stop/recover diagnostics, no raw secret regression.
5. Release 증거: 032를 이름으로 가리키고 reproducible command 또는 artifact를 연결하는 016 coverage entry.
6. Documentation 증거: marketplace, dynamic ABI, SaaS/admin, fleet behavior를 구현 완료처럼 주장하지 않고 app start, stop, recover, edit, install handoff를 설명하는 user-facing docs.
7. Resource lifecycle 증거: discovery/install/activation/execution 분리, app-level inspect/disable/revoke/stale/removed blocker, 030-controlled dependency preparation, manifest 밖 install 차단, runtime prerequisite 구분을 검증하는 코드·테스트·receipt.

현재 closure evidence는 없다. 기존 specs 005, 017, 021, 025의 implemented evidence는 032의 baseline일 뿐이며, 032 completion evidence가 아니다.
