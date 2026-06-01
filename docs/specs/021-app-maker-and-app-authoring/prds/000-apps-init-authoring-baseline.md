# PRD 000. apps init authoring baseline

## 목표

이 문서는 `docs/specs/021-app-maker-and-app-authoring/SPEC.md`의 첫 구현 PRD다.

목표는 App Maker 전체가 아니라, `apps init <app-id>`가 안전한 authoring draft와 최소 scaffold 후보만 만드는 첫 절편을 고정하는 것이다.

이 단계의 `apps init`은 app을 설치하지 않는다. app을 enable하지 않는다. app process, MCP server, package manager, network probe, secret read, grant creation, active skill injection, app registry mutation도 만들지 않는다.

구현자는 이 PRD만으로 현재 수동 `.shacsapp` authoring에서 future full App Maker로 넘어가는 가장 작은 안전한 다리를 만들 수 있어야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/021-app-maker-and-app-authoring/SPEC.md`다.
2. config, data-dir, runtime layout, storage location owner 경계는 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 따른다.
3. app bundle, manifest, registry, install 의미는 `docs/specs/017-app-operating-environment/SPEC.md`를 따른다.
4. authoring proposal, approval, checkpoint, apply, verify 의미는 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`를 따른다.
5. host safety, permission, secret boundary는 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 따른다.
6. diagnostics, trace, redaction evidence surface는 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 따른다.
7. provider-visible tool surface와 tool schema 노출 경계는 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`를 따른다.
8. skill draft의 문서 형식과 active injection 경계는 `docs/specs/005-skill-system/SPEC.md`를 따른다.
9. CLI projection과 future TUI, local API 표시 의미는 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`를 따른다.
10. 검증 기준과 release evidence 분류는 `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`를 따른다.
11. 현재 CLI에는 `apps init`이 없으며, 공개 app CLI baseline은 install, list, inspect, show, enable, disable, uninstall로 읽는다.

## Dependency Cut

1. 021은 `apps init` authoring draft와 scaffold 후보의 의미를 소유한다.
2. 008은 config, data-dir, runtime layout을 계속 소유한다. 이 PRD는 authoring draft storage를 그 layout 아래에서 소비하며 별도 installed app path를 만들지 않는다.
3. 017은 installed app registry, `.shacsapp` bundle validation, install, enable, disable, uninstall 의미를 계속 소유한다. 이 PRD는 017의 install을 다시 만들지 않는다.
4. 018은 approval, checkpoint, apply, verify flow를 계속 소유한다. 이 PRD의 출력은 future handoff preview까지만 만들 수 있고, 승인이나 apply를 수행하지 않는다.
5. 010은 permission grant, secret value, host safety decision을 계속 소유한다. 이 PRD의 permission, secret, device, tool, service 정보는 정적 선언 후보일 뿐이다.
6. 014는 observability, diagnostics, trace, redaction evidence surface를 계속 소유한다. 이 PRD는 authoring receipt의 의미를 소유하지만 diagnostics, trace, redaction 표면은 014를 소비한다.
7. 020은 provider-visible tool surface를 계속 소유한다. 이 PRD의 tool declaration은 후보일 뿐이며 generated tool을 provider-visible surface에 노출하지 않는다.
8. 005는 skill discovery와 active context injection을 계속 소유한다. 이 PRD의 skill draft placeholder는 active skill이 아니다.
9. 013은 CLI, future TUI, local API projection 의미를 소비한다. 이 PRD는 표시할 draft summary를 만들 수 있지만 UI transport를 고정하지 않는다.
10. 016은 test family와 release gate를 소유한다. 이 PRD는 first slice에 맞는 테스트 이름과 완료 기준을 제공한다.

## 범위

1. `apps init <app-id>` CLI surface를 future 구현 대상으로 정의한다.
2. `<app-id>`를 검증하고 normalized app id를 draft identity의 일부로 쓴다.
3. draft store 또는 staging area 아래에 새 authoring draft directory를 만든다.
4. 최소 파일 후보를 생성한다.
5. 파일 후보는 draft metadata, scaffold plan, manifest candidate, README candidate, optional skill draft placeholder다.
6. device, tool, service declaration을 포함하더라도 정적 declaration candidate로만 저장한다.
7. 같은 app id로 다시 실행할 때의 conflict와 idempotency 규칙을 정의한다.
8. 생성 출력과 diagnostics에서 secret-like value, control character, terminal escape를 안전하게 다룬다.
9. install handoff preview는 future 또는 non-goal로 표시하고, 첫 절편에서는 자동 설치로 이어지지 않게 한다.

## 범위 제외

1. AI assisted proposal flow 전체 구현.
2. 자연어 intent를 받아 manifest와 skill을 자동 확장하는 authoring agent.
3. 018 authoring proposal 생성, approval, checkpoint, apply, verify 실제 연결.
4. 017 app install, app registry mutation, enable, start, uninstall 변경.
5. MCP process start, local service start, package install, build command, health check, network probe.
6. secret value 읽기, secret prompt, secret binding, grant creation.
7. generated skill의 active registry 등록 또는 provider context 주입.
8. device, tool, service runtime registration 또는 exposure.
9. 기존 installed app edit flow.
10. TUI widget, local API endpoint, remote marketplace, 조직 관리자 승인, fleet rollout.
11. 사용자용 USAGE/README command 문서 변경. spec index 링크를 영구 제외한다는 뜻은 아니며, 첫 Rust 구현 절편은 CLI command가 생기기 전까지 USAGE/README command docs를 요구하지 않는다.

## 구현 요구사항

1. `apps init <app-id>`는 성공 시 draft directory만 만들고 installed app registry를 읽기 projection 이상으로 바꾸면 안 된다.
2. draft directory는 installed bundle 위치인 `<data-dir>/apps/<app-id>.shacsapp/`가 아니라 authoring 전용 draft 또는 staging area 아래에 있어야 한다.
3. 권장 layout은 `<data-dir>/authoring/apps/<draft-id>/`다. 실제 Rust 구현은 runtime layout owner와 맞추되 installed registry와 분리해야 한다.
4. `draft.json`은 draft id, app id, created at, source command, state, generated file list, current revision digest, redaction status를 담는다.
5. `scaffold-plan.json`은 생성할 후보 파일, 이유, owner boundary, risk label, install blocker를 담는다.
6. `candidates/manifest.json`은 017 manifest schema로 나중에 검증 가능한 후보여야 한다.
7. `candidates/README.md`는 사용자가 draft 목적과 수동 검토 지점을 이해할 수 있는 최소 설명 후보여야 한다.
8. `candidates/skills/SKILL.md`는 명시 옵션 또는 기본 placeholder 정책이 있을 때만 만들 수 있다. 생성되더라도 active skill이 아니다.
9. `candidates/declarations/` 아래 device, tool, service 후보를 두는 경우 command, endpoint, schedule, permission은 실행 대상이 아니라 정적 문자열과 설명이어야 한다.
10. app id는 비어 있지 않아야 하고 ASCII lower case letter, digit, hyphen, underscore, dot만 허용한다. 값 자체가 `.` 또는 `..`이면 거부한다. 길이는 구현에서 명시 상한을 둔다.
11. app id에 slash(`/`), backslash(`\`), whitespace, control character, shell metacharacter, Unicode confusable이 있으면 draft를 만들지 않는다. dot과 underscore는 현재 core `AppId` 동작과 맞춰 안전한 경우 허용한다.
12. 017의 bundle name compatibility는 app id에서 dot과 underscore를 무조건 금지해 깨뜨리면 안 된다. `demo.app` 같은 이름은 path guard를 통과하면 bundle name 예시로 보존해야 한다.
13. draft path 계산은 canonical base directory 아래에서만 성공해야 한다. symlink escape, absolute path override, `..` escape는 거부한다.
14. 같은 app id와 같은 draft가 이미 있고 내용이 같은 경우 idempotent success로 기존 draft summary를 보여줄 수 있다.
15. 같은 app id의 draft가 있지만 generated content나 revision digest가 다르면 conflict로 멈추고 기존 draft id와 안전한 next action만 보여준다.
16. 같은 app id가 이미 installed registry에 있더라도 이 PRD는 installed app을 수정하지 않는다. 새 draft를 만들려면 future edit flow가 필요하다는 blocker를 보여준다.
17. stdout, stderr, diagnostics, receipt candidate에 control character와 terminal escape를 그대로 출력하지 않는다.
18. generated output은 raw secret value로 보이는 값을 저장하지 않는다. secret은 key name 후보와 reason만 허용한다.
19. dry run 또는 validation 이름으로 process execution, MCP start, package install, network probe, secret read, grant creation, active skill injection, registry mutation을 수행하면 안 된다.
20. 성공 메시지는 draft path, draft id, manifest candidate path, validation status, next manual review action을 보여준다.
21. 성공 메시지는 install 완료, enable 완료, app ready, running 같은 runtime 상태로 오해될 문구를 쓰면 안 된다.

## 데이터/상태 모델

1. `AppAuthoringDraftId`: authoring store 안에서만 유효한 draft id다. installed app id가 아니다.
2. `AppAuthoringDraft`: draft id, app id, state, created at, updated at, source command, current revision, generated files, warning summary를 가진다.
3. `AppAuthoringState`: 이 PRD에서는 `DraftCreated`, `ScaffoldGenerated`, `Conflict`, `Failed`, `Archived`만 필요하다.
4. `AppIdCandidate`: CLI 입력에서 검증된 app id 후보다. 017의 installed `AppId`로 확정된 값이 아니다.
5. `AppScaffoldPlan`: 생성할 파일 후보와 owner boundary를 설명한다.
6. `AppScaffoldFileCandidate`: draft 내부 상대 경로, file kind, digest, redaction status, overwrite policy를 가진다.
7. `AppManifestCandidate`: 017 manifest로 넘어갈 수 있는 정적 JSON 후보다. permission과 secret은 request candidate다.
8. `AppReadmeCandidate`: 사용자가 읽을 draft 설명 후보다.
9. `AppSkillDraftPlaceholder`: 005 active skill registry 밖에 있는 Markdown placeholder다.
10. `StaticDeclarationCandidate`: device, tool, service 선언 후보를 표현한다. 실행 handle, registered tool id, running process id를 담지 않는다.
11. `InstallHandoffPreview`: future 타입이다. 이 PRD에서는 생성하지 않거나 non-goal preview 문구로만 남긴다.
12. `AppInitOutcome`: `Created`, `AlreadyExistsSameContent`, `Conflict`, `InvalidAppId`, `UnsafePath`, `BlockedByInstalledApp`, `IoFailed` 중 하나로 접는다.

## 정상 시퀀스

1. 사용자가 `shacs-bot apps init <app-id>`를 실행한다.
2. CLI parser가 `<app-id>`를 `CreateAppAuthoringDraft` command로 매핑한다.
3. app id validator가 입력을 정규화하고 안전하지 않은 문자를 거부한다.
4. runtime layout이 authoring draft base directory를 계산한다.
5. path guard가 draft target이 base directory 안에 있음을 확인한다.
6. installed app registry에는 mutation을 만들지 않는다.
7. 기존 draft conflict 여부를 확인한다.
8. 새 draft directory를 만든다.
9. `draft.json`과 `scaffold-plan.json`을 쓴다.
10. `candidates/manifest.json`과 `candidates/README.md`를 쓴다.
11. 옵션 또는 기본 placeholder 정책이 켜져 있으면 `candidates/skills/SKILL.md`를 쓴다.
12. 모든 생성 파일의 digest와 redaction status를 draft metadata에 기록한다.
13. CLI가 draft id, draft path, 후보 파일 목록, no-run boundary, future install handoff가 별도 단계임을 출력한다.

## 실패 시퀀스

1. app id가 비어 있거나 규칙을 어기면 파일을 만들지 않고 `InvalidAppId`를 반환한다.
2. app id가 path escape를 만들 수 있으면 파일을 만들지 않고 `UnsafePath`를 반환한다.
3. draft base directory가 symlink escape로 판정되면 파일을 만들지 않고 `UnsafePath`를 반환한다.
4. 같은 app id의 동일 content draft가 있으면 파일을 덮어쓰지 않고 `AlreadyExistsSameContent`를 반환한다.
5. 같은 app id의 다른 draft가 있으면 새 파일을 만들지 않고 `Conflict`를 반환한다.
6. 같은 app id가 installed registry에 있으면 registry를 수정하지 않고 `BlockedByInstalledApp` 또는 future edit flow 안내로 멈춘다.
7. 파일 생성 중 실패하면 partial directory를 installed app으로 승격하지 않는다. 가능한 경우 draft metadata에 failed status를 남기거나 안전하게 삭제 가능한 empty directory만 정리한다.
8. manifest candidate에 permission, secret, device, tool, service 후보가 있더라도 grant, secret read, process start, MCP registration, tool exposure는 만들지 않는다.
9. 출력 redaction이 실패하면 generated content를 보여주지 않고 redaction failure를 blocker로 남긴다.

## 검증 관점

1. `apps_init_maps_to_authoring_draft_only`: CLI parser가 `apps init`을 install, enable, start command가 아니라 draft creation command로 매핑하는지 확인한다.
2. `apps_init_rejects_invalid_app_id`: whitespace, slash, backslash, 값 `.` 또는 `..`, control character, Unicode confusable, shell metacharacter가 거부되고, `demo.app`과 `demo_app`은 그 외 안전하면 허용되는지 확인한다.
3. `apps_init_creates_draft_under_authoring_store`: 생성 경로가 authoring draft base 아래에 있고 installed apps directory가 아닌지 확인한다.
4. `apps_init_writes_minimal_candidates`: `draft.json`, `scaffold-plan.json`, `candidates/manifest.json`, `candidates/README.md`가 생성되는지 확인한다.
5. `apps_init_skill_placeholder_is_not_active`: optional skill placeholder가 있어도 005 active registry나 provider context에 들어가지 않는지 확인한다.
6. `apps_init_static_declarations_do_not_register_runtime`: device, tool, service 후보가 있어도 process, MCP, scheduler, tool registry에 등록되지 않는지 확인한다.
7. `apps_init_does_not_mutate_app_registry`: 성공과 실패 모두에서 017 installed app registry가 바뀌지 않는지 확인한다.
8. `apps_init_existing_same_content_is_idempotent`: 같은 content의 기존 draft가 있을 때 덮어쓰기 없이 성공 summary를 반환하는지 확인한다.
9. `apps_init_existing_different_content_conflicts`: 다른 revision이 있으면 conflict로 멈추는지 확인한다.
10. `apps_init_blocks_installed_app_id_without_edit_flow`: installed app id와 충돌할 때 registry를 바꾸지 않고 edit flow blocker를 반환하는지 확인한다.
11. `apps_init_never_executes_process_or_network`: init 중 process execution, MCP start, package install, network probe가 호출되지 않는지 fake executor로 확인한다.
12. `apps_init_never_reads_secret_or_creates_grant`: secret value read와 permission grant creation이 호출되지 않는지 확인한다.
13. `apps_init_redacts_secret_like_and_control_output`: diagnostics와 CLI output에서 secret-like value, control character, terminal escape가 가려지는지 확인한다.
14. `apps_init_handoff_preview_is_not_install`: 출력이 install handoff preview를 future action으로만 설명하고 install 완료처럼 표시하지 않는지 확인한다.

## 완료 기준

1. `apps init <app-id>` 구현 계획이 authoring draft 생성으로만 닫혀 있다.
2. 생성 대상은 authoring draft 또는 staging area이며 installed app bundle과 registry를 바꾸지 않는다.
3. 최소 후보 파일은 draft metadata, scaffold plan, manifest candidate, README candidate, optional skill draft placeholder로 제한된다.
4. device, tool, service 정보는 포함되더라도 정적 declaration candidate이며 실행, 등록, 노출을 만들지 않는다.
5. no-run 규칙이 테스트 이름과 실패 시퀀스에 반복되어 있다.
6. safe path, app id validation, conflict, idempotency, redaction, control character handling 기준이 구현자가 바로 테스트로 옮길 수 있게 정의되어 있다.
7. install handoff preview는 future 또는 non-goal로 고정되어 첫 절편이 017 install을 우회하지 않는다.
8. 005, 008, 010, 013, 014, 016, 017, 018, 020 owner boundary가 약해지지 않는다.
9. 이 PRD는 구현 완료를 주장하지 않으며, full App Maker나 AI assisted proposal flow를 요구하지 않는다.

## Future Rust 구현 웨이브

### Wave 1. CLI와 command model

1. `apps init <app-id>` parser를 추가한다.
2. parser output을 `CreateAppAuthoringDraft` 계열 command로만 연결한다.
3. install, enable, start, approve, inject, register command와 같은 enum variant로 섞지 않는다.

### Wave 2. App id와 path guard

1. `AppIdCandidate` validator를 추가한다.
2. authoring draft base path 계산과 canonical boundary check를 추가한다.
3. symlink escape, absolute path override, 값 `.` 또는 `..`, slash, backslash를 회귀 테스트로 고정한다.

### Wave 3. Draft store와 scaffold writer

1. `AppAuthoringDraft`, `AppScaffoldPlan`, `AppScaffoldFileCandidate` 타입을 추가한다.
2. `draft.json`, `scaffold-plan.json`, manifest candidate, README candidate writer를 추가한다.
3. same content idempotency와 different content conflict를 digest로 판정한다.

### Wave 4. Static candidate safety

1. manifest candidate의 permission, secret, device, tool, service 값을 request candidate로만 표현한다.
2. fake executor, fake registry, fake skill registry, fake grant store를 써서 호출이 없음을 검증한다.
3. generated output redaction과 control character scrub을 CLI summary와 diagnostics에 적용한다.

### Wave 5. Future handoff seam

1. install handoff preview 타입 이름과 출력 문구를 non-goal로 고정한다.
2. 018 proposal flow와 017 install flow로 넘길 future seam만 남긴다.
3. 이 wave에서도 install, enable, start, approval, registry mutation을 구현하지 않는다.
