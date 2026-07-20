# 032. App Maker runtime과 extension lifecycle 아키텍처 명세

Status: Open

Origin specs: 005, 017, 021, 025

## 목적

이 문서는 기존 specs 005, 017, 021, 025가 implemented scope로 닫힌 뒤 남는 app product work의 owner boundary를 연다.

핵심 목적은 App Maker가 만든 제안과 설치 가능한 `.shacsapp` bundle이 실제 app process, AppSupervisor, permission과 secret binding, extension lifecycle provenance로 이어지는 끝단을 한 곳에 고정하는 것이다.

이 문서는 기존 spec의 완료 선언을 되돌리지 않는다. 005는 read-only skill registry와 context injection, 017은 app manifest와 registry baseline, 021은 안전한 authoring draft baseline, 025는 self-hosted local plugin manifest와 제한된 executable surface를 닫은 것으로 본다. 032는 그 다음에 남은 실제 제품 흐름을 소유한다.

## 현재 구현 baseline

현재 구현은 다음 baseline을 가진다.

1. 005는 Markdown skill discovery, source kind, status, descriptor, context injection, CLI inspect surface를 갖는다. Skill은 read-only 지식 팩이며 permission이나 tool visibility를 직접 얻지 않는다.
2. 017은 local `.shacsapp` manifest, app registry, lifecycle state, process snapshot projection, task ledger receipt baseline을 갖는다. Install은 process start가 아니며 secret value와 permission grant를 만들지 않는다.
3. 021은 `apps init` authoring draft baseline을 갖는다. Draft store, scaffold plan, manifest candidate, README candidate, idempotency, path safety가 구현됐고 installed app registry를 바꾸지 않는다.
4. 025는 local plugin manifest discovery, config activation gate, hook dispatch, `tool:before` block-only behavior, command-backed plugin tool, plugin MCP declaration, plugin skill root, plugin command router, replay live-dispatch rejection을 갖는다.

이 baseline은 아래를 완료로 주장하지 않는다.

1. App process의 실제 start, stop, restart, recover lifecycle.
2. AppSupervisor가 app device, service, MCP, skill exposure를 live runtime에서 함께 관리하는 경계.
3. App Maker proposal이 approval, checkpoint, apply, verify, receipt, install handoff를 거쳐 설치로 이어지는 end-to-end flow.
4. Existing app edit이 안전한 snapshot, diff, proposal, apply, install 또는 update handoff를 거쳐 완료되는 flow.
5. Extension lifecycle provenance가 app install, enable, start, disable, uninstall, recover receipt와 하나로 이어지는 증거 체계.

## owned open scope

032가 소유하는 open scope는 다음이다.

1. 실제 app process start, stop, restart, recover.
2. `AppSupervisor`의 owner boundary, state model, recovery input, shutdown behavior, stale process handling.
3. App manifest의 permission과 secret declaration이 runtime grant, secret handle, process environment로 넘어가는 handoff.
4. AI-assisted app proposal, validation, receipt, apply, install flow.
5. Existing app edit flow, read-only installed snapshot, draft diff, proposal, checkpoint, apply, verify, install 또는 update handoff.
6. App lifecycle 안에서 발생하는 extension, skill, plugin, hook, command, MCP provenance의 소유 위치.
7. App install, enable, start, stop, disable, uninstall, recover의 domain state vocabulary와 projection input contract. Shared surface adapter와 parity smoke는 031이 소유한다.
8. App process와 extension action이 task ledger, diagnostics, replay evidence에 redacted receipt로 남는 규칙.

032가 다른 spec에서 소비하는 것과 다시 소유하지 않는 것은 명확히 분리한다. 004/005/010/012/013/014/016/017/021/022/025는 구현 baseline을 제공한다. Open formal policy·permission·redaction은 030, config와 secret-ref consumption은 035, shared UI adapter와 parity smoke는 031이 소유한다. 032는 app lifecycle state와 receipt를 생산한다.

## invariants

1. App install은 app execution이 아니다.
2. App Maker proposal은 approval이 아니며 permission grant도 아니다.
3. AppSupervisor는 `MainOrchestrator`를 대체하지 않는다. AppSupervisor는 process lifecycle executor와 evidence producer다.
4. App process start 전에는 permission snapshot과 secret binding plan이 확정돼야 한다.
5. Secret value는 app bundle, manifest, receipt, ledger, diagnostics, provider prompt에 raw로 저장되지 않는다.
6. App process는 start 시점의 manifest digest, permission snapshot, secret handle set, extension activation set을 receipt에 남겨야 한다.
7. Extension은 app이나 plugin이 제공해도 owner boundary를 우회하지 않는다. Skill/tool/hook baseline은 005/004/025를 소비하고, formal permission·redaction은 030, config/secret-ref consumption은 035 경계를 통과한다.
8. Disabled, blocked, missing-secret, missing-permission app은 active process, active skill, provider-visible tool, live hook, MCP startup을 만들 수 없다.
9. Existing app edit은 installed bundle을 직접 덮어쓰지 않는다. Snapshot, draft, diff, proposal, checkpoint, apply, verify를 거쳐야 한다.
10. Uninstall과 recover는 historical ledger와 session reference를 조용히 파괴하지 않는다.
11. Replay는 destructive app process나 plugin command를 live-dispatch하지 않는다. Recorded evidence만 해석한다.

## Must Have

1. `AppSupervisor` typed model, app process id, app process state, lifecycle event, recovery input, shutdown reason을 정의해야 한다.
2. Start flow는 registry lookup, enabled state check, manifest digest check, missing secret check, permission snapshot check, extension activation snapshot, process creation, ledger receipt를 포함해야 한다.
3. Stop flow는 user requested stop, graceful shutdown attempt, timeout result, cancellation evidence, final process state를 포함해야 한다.
4. Recover flow는 interrupted process, stale marker, missing receipt, partial extension activation, device startup failure를 redacted diagnostics와 next action으로 보여줘야 한다.
5. Permission and secret binding handoff는 manifest request를 grant request와 secret handle request로 바꾸되 raw secret을 읽거나 저장하지 않아야 한다.
6. AI-assisted proposal flow는 user intent, generated candidates, validation report, risk summary, receipt, approval linkage, checkpoint linkage, apply result, install handoff를 추적해야 한다.
7. Existing app edit flow는 installed snapshot digest, draft revision, manifest diff, extension diff, validation report, proposal, checkpoint, apply, verify, update handoff를 가져야 한다.
8. Extension lifecycle provenance는 app이 제공한 skill, plugin, hook, command, MCP declaration의 source app id, manifest digest, enabled state, activation decision, blocked reason을 inspect와 receipt에 남겨야 한다.
9. 032는 app registry state, process state, missing secret, permission blocker, extension blocker, last receipt의 domain vocabulary와 evidence를 생산해야 하며, 031의 shared adapter가 CLI, local API, future TUI에서 같은 의미로 투영할 수 있어야 한다.
10. Release tests는 normal path, blocked path, denied permission, missing secret, failed start, failed stop, recover after interruption, replay-safe diagnostics를 포함해야 한다.

## Must Not Have

1. Marketplace, remote catalog, public app store, signed distribution protocol을 이 spec closure 조건으로 끌어오지 않는다.
2. Arbitrary in-process third-party code loading을 허용하지 않는다.
3. Rust dynamic ABI, dynamic library plugin ABI, WASM 또는 scripting loader를 app runtime 기본 경로로 만들지 않는다.
4. SaaS control plane, admin approval console, organization catalog, fleet rollout을 기본 흐름으로 두지 않는다.
5. App Maker validation이라는 이름으로 package manager, shell command, MCP server, network auth test를 실행하지 않는다.
6. App manifest의 permission declaration을 persistent grant로 바꾸지 않는다.
7. Secret key name을 보고 raw secret value를 authoring, validation, diagnostics에 복사하지 않는다.
8. Generated skill, tool, hook, MCP declaration을 승인 없이 live runtime에 노출하지 않는다.
9. Disable이나 uninstall을 historical evidence 삭제로 해석하지 않는다.
10. AppSupervisor가 session truth, permission truth, approval truth를 직접 확정하게 하지 않는다.

## acceptance criteria

1. `AppSupervisor`와 app process lifecycle 타입이 코드에 존재하고 start, stop, restart, recover 상태 전이를 테스트로 고정한다.
2. `apps start`, `apps stop`, `apps restart`, `apps recover` 또는 같은 의미의 CLI/local API command가 installed app registry와 AppSupervisor boundary를 통과한다.
3. Missing secret, missing permission, disabled app, blocked extension은 process start를 fail-closed로 막고 user-facing projection과 diagnostics receipt를 남긴다.
4. Successful process start는 manifest digest, app id, permission snapshot id, secret handle names, extension activation snapshot, device state, receipt id를 남긴다.
5. Stop과 recover는 partial process와 stale marker를 숨기지 않고 next action과 evidence를 제공한다.
6. AI-assisted new app flow는 draft에서 validation, proposal, approval linkage, checkpoint linkage, apply, verify, install handoff까지 traceable하다.
7. Existing app edit flow는 installed snapshot을 직접 mutate하지 않고 diff와 checkpoint를 거친 뒤 install 또는 update handoff를 만든다.
8. Plugin-provided skill, command-backed tool, hook, MCP declaration은 app lifecycle provenance와 연결되지만 기존 owner runtime gate를 우회하지 않는다.
9. Replay runner와 diagnostics reader는 app process와 extension action을 live 재실행하지 않고 recorded evidence만 해석한다.
10. 016 기준 정적, 단위, 통합, 안전성, UX, 복구, replay release evidence가 032 coverage entry로 남는다.

## source handoff table

| origin spec | 닫힌 implemented scope | 032로 넘어오는 open work |
|---|---|---|
| 005 skill system | Skill registry, discovery, status, descriptor, read-only context injection, CLI inspect baseline | App bundle과 extension이 제공한 skill의 lifecycle provenance, app enable/start 시점의 activation snapshot, receipt linkage |
| 017 app operating environment | Local app manifest, registry, lifecycle state, process projection, task ledger receipt baseline | Actual app process start/stop/recover, AppSupervisor, process receipt, permission/secret handoff, extension activation lifecycle |
| 021 app maker and app authoring | `apps init` draft baseline, scaffold and manifest candidate, installed registry non-mutation | AI-assisted proposal, validation, receipt, approval/checkpoint/apply/verify integration, install handoff, existing-app edit flow |
| 025 user-extensible hooks and plugins | Local plugin manifest, activation gate, hook/tool/skill/MCP/command slice, replay live-dispatch rejection | App-owned extension provenance, app lifecycle tied enable/disable/start/recover evidence, extension blocker projection |

## closure evidence

032는 아직 open 상태다. 이 spec을 닫으려면 아래 evidence가 저장소에 있어야 한다.

1. 코드 증거: `AppSupervisor`, app process lifecycle, app start/stop/recover command path, permission/secret binding handoff, extension provenance model.
2. 테스트 증거: lifecycle state tests, missing secret and permission tests, extension blocked tests, existing app edit tests, proposal/apply/install handoff tests, replay safety tests.
3. 인터페이스 증거: registry, process, blocker, receipt, recover state에 대한 CLI와 local API output 또는 snapshot tests. Future TUI가 구현된 경우 같은 projection 의미를 써야 한다.
4. Diagnostics 증거: redacted app process receipt, extension activation receipt, failed start/stop/recover diagnostics, no raw secret regression.
5. Release 증거: 032를 이름으로 가리키고 reproducible command 또는 artifact를 연결하는 016 coverage entry.
6. Documentation 증거: marketplace, dynamic ABI, SaaS/admin, fleet behavior를 구현 완료처럼 주장하지 않고 app start, stop, recover, edit, install handoff를 설명하는 user-facing docs.

현재 closure evidence는 없다. 기존 specs 005, 017, 021, 025의 implemented evidence는 032의 baseline일 뿐이며, 032 completion evidence가 아니다.
