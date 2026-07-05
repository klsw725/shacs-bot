# PRD 002. static rules, protected targets, and containment

## 구현 상태

Status: Implemented.

구현 증거는 `crates/shacs-core/src/runtime/permission_rules.rs`, `crates/shacs-core/src/runtime/permission_policy.rs`, `crates/shacs-core/src/runtime/permission_ceiling.rs`, `crates/shacs-core/tests/permission_policy.rs`다. 대표 테스트는 `protected_targets_fail_closed_before_policy_allow`, `unknown_target_and_invalid_action_are_never_allowable`, `unknown_containment_blocks_proc_exec_auto_and_bypass`, `secret_read_and_raw_auth_export_are_denied`다.

이 closure는 Docker containment evidence와 command rule 판단을 닫는다. Per-command sandbox backend 구현을 뜻하지 않는다.

## 목표

이 문서는 `PermissionedAction`에 적용할 capability classification, static deny rule, protected target rule, Docker containment snapshot 기준을 정의한다.

목표는 evaluator를 호출하기 전에 자동으로 거절하거나 확인해야 하는 action을 결정할 수 있는 deterministic rule layer를 만드는 것이다.

Docker는 primary containment로 인정하지만, `proc_exec` permission check를 대체하지 않는다.

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. PRD 000의 mode와 capability taxonomy를 소비한다.
3. PRD 001의 `PermissionedAction` envelope를 소비한다.
4. host safety guard 기준은 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 따른다.
5. app registry와 app lifecycle protected target은 `docs/specs/017-app-operating-environment/SPEC.md`를 따른다.

## Dependency Cut

1. 022는 capability classification과 Docker containment snapshot 의미를 소유한다.
2. 010은 현재 filesystem, shell, network, secret guard 구현 기준을 계속 소유한다.
3. 017은 app install, enable, disable, uninstall 의미를 계속 소유한다.
4. 이 PRD는 per-command sandbox backend를 구현하지 않는다.
5. 이 PRD는 evaluator나 approval cache를 구현하지 않는다.

## 범위

1. Capability classification table.
2. Protected target rule.
3. Static deny rule.
4. Docker containment snapshot field.
5. `proc_exec` command summary requirement.
6. `bypass_permissions` containment precondition.
7. Unknown containment fallback.
8. Rule diagnostics summary.

## 범위 제외

1. `bwrap`, seccomp, chroot 같은 per-command sandbox backend.
2. Docker image build 또는 compose 파일 작성.
3. Evaluator prompt 설계.
4. Approval request UI.
5. Audit persistence backend.
6. 원격 운영 콘솔과 다중 사용자 권한 관리.

## 구현 요구사항

1. Rule layer는 evaluator보다 먼저 실행되어야 한다.
2. `deny` rule은 `allow` rule보다 우선해야 한다.
3. `.git`, auth store, raw credential path, runtime permission config, app registry mutation target은 protected target으로 분류되어야 한다.
4. `fs_read`, `fs_write`, `proc_exec`, `net_outbound`, `secret_read`, `external_delivery`, `automation_schedule`, `app_install`, `runtime_config_write`, `self_modification` capability를 action에 부여할 수 있어야 한다.
5. `secret_read`, raw auth export, unknown host path escape는 auto allow 후보가 되면 안 된다.
6. `proc_exec` action은 Docker 안에서도 command summary와 dangerous command classification을 가져야 한다.
7. Command summary를 만들 수 없으면 `proc_exec`는 allow 후보가 되면 안 된다.
8. Docker snapshot은 contained, runtime, root user, privileged, host mounts summary, network mode를 표현해야 한다.
9. Docker snapshot이 unknown이면 `auto` mode의 `proc_exec` 자동 승인 범위는 좁아져야 한다. Direct classifier-backed `proc_exec` permission trust는 summary 문자열이 아니라 structured `official-container` backend evidence, non-root runtime, unsafe marker 부재를 함께 확인해야 한다.
10. `bypass_permissions`는 confirmed containment 없이는 활성화되면 안 된다.
11. Root 또는 privileged container에서 `bypass_permissions` activation은 deny 또는 explicit circuit breaker로 접어야 한다.
12. Exec sandbox backend 부재는 실패가 아니지만 command rule과 audit 부재는 실패다.

## 데이터/상태 모델

1. `CapabilityClassification`: action id, capability set, target classes, confidence.
2. `ProtectedTargetClass`: git state, auth store, runtime config, app registry, host mount root, startup hook, package lifecycle script.
3. `StaticRuleDecision`: allow candidate, ask required, deny, reason.
4. `DockerContainmentSnapshot`: contained, runtime, root user, privileged, host mounts, network mode.
5. `ProcExecSummary`: command family, target refs, destructive flag, network flag, secret exposure flag.
6. `RuleDiagnostics`: matched rules, protected targets, containment warning, unknown classification.

## 정상 시퀀스

1. Permissioned action이 rule layer에 들어온다.
2. Capability classifier가 action capability set을 계산한다.
3. Target classifier가 protected target 여부를 계산한다.
4. `proc_exec`이면 command summary를 만든다.
5. Docker containment snapshot을 action snapshot에 연결한다. Generic container 또는 summary-only official-looking text는 permission-rule trust를 만들지 않고, structured official backend evidence가 없으면 unknown으로 접는다.
6. Static deny와 protected target rule을 적용한다.
7. Rule layer가 evaluator 호출 가능 여부 또는 즉시 ask/deny 필요 여부를 반환한다.

## 실패 시퀀스

1. Action target이 `.shacs-bot/auth.json`이다.
2. Rule layer가 protected target으로 분류하고 auto allow 후보에서 제외한다.
3. `proc_exec` command가 secret env dump를 포함한다.
4. Rule layer가 deny 또는 ask required로 접는다.
5. Runtime이 `bypass_permissions`를 요청하지만 Docker containment가 unknown이다.
6. Mode activation을 deny한다.

## 검증 관점

1. Protected target write가 evaluator allow 없이 ask/deny로 접히는지 확인한다.
2. Auth store raw read가 deny되는지 확인한다.
3. Docker snapshot unknown이거나 summary-only official-looking evidence만 있는 `proc_exec`가 auto allow되지 않는지 확인한다.
4. `bypass_permissions`가 containment unknown에서 활성화되지 않는지 확인한다.
5. Root 또는 privileged container에서 bypass activation이 거부되는지 확인한다.
6. Command summary 실패가 allow로 이어지지 않는지 확인한다.
7. Exec sandbox backend 미설정이 이 PRD의 실패 조건이 아닌지 확인한다.

## 완료 기준

1. Capability와 target classification이 permission decision input으로 제공된다.
2. Static deny와 protected target rule이 evaluator보다 먼저 적용된다.
3. Docker containment snapshot이 mode activation과 `proc_exec` 판단에 들어간다.
4. Docker가 primary containment지만 `proc_exec` permission check와 audit가 유지된다.
5. Per-command sandbox는 비목표로 남고, command rule과 containment evidence가 테스트로 고정된다.
