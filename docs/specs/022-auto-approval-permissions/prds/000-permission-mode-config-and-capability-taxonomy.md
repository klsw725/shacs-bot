# PRD 000. permission mode config and capability taxonomy

Status: Implemented. 이 PRD는 permission mode config와 capability taxonomy slice를 닫았고, Spec 022 전체 closure는 PRD 006까지의 runtime gate, audit, replay 증거와 함께 닫혔다.

## 목표

이 문서는 `docs/specs/022-auto-approval-permissions/SPEC.md`의 첫 구현 PRD다.

목표는 auto approval 구현 전에 `PermissionMode`, mode source, config boundary, `SafetyCapability` taxonomy를 먼저 고정하는 것이다.

이 단계는 tool 실행 경로를 바꾸지 않는다. provider tool call을 실행 전 gate로 연결하는 작업은 후속 PRD가 소유한다.

구현자는 이 PRD만으로 user-local 설정과 workspace 설정이 permission mode를 어떻게 만들 수 있는지, 어떤 capability 이름이 후속 gate에서 쓰이는지 확인할 수 있어야 한다.

## 구현 상태와 증거

현재 구현된 범위는 다음이다.

1. `PermissionMode`, `PermissionModeSource`, `SafetyCapability`, `AutoApprovalConfig`가 정의됐다.
2. Safe config normalization과 activation source constraints가 구현됐다.

증거 경로는 다음이다.

1. `crates/shacs-config/src/permissions.rs`

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. config 저장과 user-local/workspace boundary는 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 따른다.
3. host safety와 permission primitive 의미는 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 따른다.
4. orchestrator authority는 `docs/specs/007-main-orchestrator-policy/SPEC.md`를 따른다.
5. 이 PRD는 구현 완료된 slice이며, Spec 022 전체 완료 선언은 PRD 006까지의 통합 증거와 함께 판단한다.

## Dependency Cut

1. 022는 `PermissionMode`와 `SafetyCapability`의 최종 의미를 소유한다.
2. 008은 설정 파일 위치와 config merge의 물리 경계를 계속 소유한다.
3. 010은 기존 filesystem, process, network, secret guard의 current baseline을 계속 소유한다.
4. 007은 prompt, skill, app manifest, memory가 runtime permission authority가 될 수 없다는 orchestrator 경계를 제공한다.
5. 이 PRD는 tool call normalization, evaluator, approval cache, audit를 구현하지 않는다.

## 범위

1. `PermissionMode` enum 정의.
2. `PermissionModeSource` 정의.
3. user-local config와 workspace config의 권한 차이 정의.
4. `default`, `plan`, `accept_edits`, `auto`, `dont_ask`, `bypass_permissions`의 baseline 의미 정의.
5. `auto`와 `bypass_permissions` activation precondition 정의.
6. `SafetyCapability` taxonomy 정의.
7. malformed config safe fallback 기준 정의.
8. normalized permission config diagnostics summary 정의.

## 범위 제외

1. provider tool call 실행 전 gate 연결.
2. `PermissionedAction` action digest 생성.
3. Docker containment 실제 판정.
4. evaluator 호출.
5. approval request와 decision 저장.
6. user prompt UI 디자인.
7. 원격 운영 콘솔과 다중 사용자 권한 관리.

## 구현 요구사항

1. `PermissionMode`는 `Plan`, `Default`, `AcceptEdits`, `Auto`, `DontAsk`, `BypassPermissions`를 표현해야 한다.
2. 기본 mode는 `Default`다.
3. `Plan`은 read-only action만 허용하는 mode로 정규화되어야 한다.
4. `AcceptEdits`는 workspace 내부 일반 파일 edit/write 자동 승인 후보만 만든다. `proc_exec`는 이 mode만으로 자동 승인되지 않는다.
5. `Auto`는 user-local config, explicit CLI flag, local API request, session command 중 명시적 source에서만 켤 수 있어야 한다.
6. Workspace config는 `Auto`를 제안할 수 있지만 user-local opt-in 없이 활성화하면 안 된다.
7. Prompt, skill instruction, app manifest, session memory, tool result는 mode source가 될 수 없다.
8. `DontAsk`는 명시 allow rule과 mode baseline으로 해결되지 않은 action을 ask가 아니라 deny로 접는다.
9. `BypassPermissions`는 기본값이 될 수 없고 explicit opt-in과 containment precondition을 요구해야 한다.
10. Malformed mode 값은 permission widening이 아니라 `Default` safe fallback과 warning diagnostics로 이어져야 한다.
11. `SafetyCapability`는 최소한 `fs_read`, `fs_write`, `proc_exec`, `net_outbound`, `secret_read`, `external_delivery`, `automation_schedule`, `app_install`, `runtime_config_write`, `self_modification`을 포함해야 한다.
12. Unknown capability는 allow가 아니라 ask 또는 deny 후보로 정규화되어야 한다.
13. Config diagnostics는 raw secret이나 provider credential을 포함하면 안 된다.

## 데이터/상태 모델

1. `PermissionMode`: active run의 unmatched action 처리 mode.
2. `PermissionModeSource`: `UserLocalConfig`, `WorkspaceConfig`, `CliFlag`, `LocalApiRequest`, `SessionCommand`, `DefaultFallback`.
3. `PermissionConfigSnapshot`: mode, source, auto approval options, protected target summary, generated at.
4. `AutoApprovalConfig`: `permissions.mode: "auto"`에서 파생되는 enabled state와 require docker containment for exec, allow workspace edits, allow proc exec verification, protected targets.
5. `SafetyCapability`: canonical capability enum.
6. `PermissionConfigDiagnostics`: normalized mode, rejected source, malformed fields, safe fallback reason.

## 정상 시퀀스

1. 사용자가 config에 permission 설정을 생략한다.
2. config loader가 `Default` mode snapshot을 만든다.
3. Workspace config가 `auto`를 제안하지만 user-local opt-in이 없다.
4. runtime은 active mode를 `Default`로 유지하고 warning diagnostics를 남긴다.
5. CLI flag가 `--permission-mode auto`를 명시하면 user-visible source로 `Auto` snapshot을 만든다.
6. 후속 PRD의 decision gate가 이 snapshot을 소비할 수 있다.

## 실패 시퀀스

1. Workspace config가 `bypass_permissions`를 설정한다.
2. runtime은 mode activation을 거절하고 `Default` 또는 더 제한적인 fallback을 사용한다.
3. Prompt가 mode 변경을 지시한다.
4. runtime은 prompt를 mode source로 인정하지 않는다.
5. Config 값이 알 수 없는 문자열이면 safe fallback과 diagnostics를 남긴다.
6. Unknown capability가 분류되면 자동 allow 후보로 만들지 않는다.

## 검증 관점

1. config 생략 시 `Default`가 적용되는지 확인한다.
2. 각 mode 문자열이 정상 파싱되는지 확인한다.
3. malformed mode가 safe fallback으로 이어지는지 확인한다.
4. workspace config가 `auto` 또는 `bypass_permissions`를 단독 활성화하지 못하는지 확인한다.
5. prompt, skill, app manifest, memory가 mode source로 거부되는지 확인한다.
6. `SafetyCapability` unknown 값이 allow로 해석되지 않는지 확인한다.
7. diagnostics가 raw secret을 포함하지 않는지 확인한다.

## 완료 기준

1. `PermissionMode`와 `SafetyCapability` 타입이 후속 PRD에서 소비 가능한 형태로 정의된다.
2. user-local/workspace config boundary가 permission widening 없이 정규화된다.
3. `Auto`와 `BypassPermissions` activation precondition이 테스트로 고정된다.
4. malformed config가 safe fallback과 diagnostics로 처리된다.
5. 이 PRD 이후에도 provider-visible tool surface와 tool execution behavior는 바뀌지 않는다.
