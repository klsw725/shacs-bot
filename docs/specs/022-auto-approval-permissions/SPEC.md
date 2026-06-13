# auto approval permissions 아키텍처 명세

Status: Draft, partially implemented. 이 문서는 `shacs-bot`의 최종 permission mode와 auto approval 계약을 고정한다. 현재 구현은 permission mode/capability taxonomy와 permissioned action normalization slice까지만 닫혔다.

## 문서 목적

이 문서는 사용자의 명시 동의가 필요한 action을 runtime이 실행 직전에 한 번 더 평가하고, 조건을 만족하면 사용자에게 다시 묻지 않고 진행하는 최종 상태를 정의한다.

목표는 다음과 같다.

1. Claude Code의 auto mode와 비슷한 제품 경험을 `shacs-bot`의 self-hosted / personal-use 관점으로 재정의한다.
2. auto approval을 permission bypass가 아니라 evaluator-mediated approval gate로 고정한다.
3. Docker 중심 운영을 primary containment로 인정하되, `proc_exec` permission 판단 자체를 없애지 않는다.
4. `plan`, `default`, `accept_edits`, `auto`, `dont_ask`, `bypass_permissions` mode의 최종 의미를 구분한다.
5. LLM 또는 classifier 판단 실패, 낮은 확신, prompt injection 의심, scope 불일치가 자동 실행으로 이어지지 않게 한다.
6. future Rust 구현에서 permission snapshot, approval request, action digest, evaluator verdict, audit record, stale decision rejection 타입과 테스트를 도출할 수 있게 한다.

핵심 문장:

```text
Auto approval은 사용자의 권한을 대체하지 않는다. 사용자가 설정한 mode와 scope 안에서만, runtime이 실행 직전 action을 다시 평가해 묻지 않아도 되는지 결정한다.
```

---

## 제품 정의

Auto approval은 agent가 tool call을 만들었을 때 사용자의 추가 확인 없이 실행할 수 있는지 결정하는 runtime permission layer다.

Auto approval이 하는 일:

1. provider가 요청한 tool call을 `PermissionedAction`으로 정규화한다.
2. action kind, target, argument digest, session intent, permission mode, Docker containment state를 frozen snapshot으로 묶는다.
3. 정적 deny rule과 protected target rule을 먼저 적용한다.
4. 필요한 경우 별도 evaluator 또는 classifier가 action이 사용자 요청 범위 안에 있는지 평가한다.
5. verdict를 `allow`, `ask`, `deny` 중 하나로 내리고 audit record를 남긴다.
6. `allow`일 때만 tool runtime으로 action을 넘긴다.
7. `ask`일 때는 사용자에게 risk summary와 선택지를 보여 준다.
8. `deny`일 때는 denied outcome을 provider와 user-facing projection에 남긴다.

Auto approval이 하지 않는 일:

1. 도구가 스스로 permission을 결정하게 하지 않는다.
2. prompt나 skill instruction이 permission mode를 바꾸게 하지 않는다.
3. evaluator hint만으로 deny rule을 우회하지 않는다.
4. Docker 안이라는 이유만으로 모든 `exec`를 자동 승인하지 않는다.
5. secret read, external delivery, persistent automation, app install, self modification을 조용히 승인하지 않는다.
6. 사용자에게 보이지 않는 장기 권한 grant를 만들지 않는다.
7. 부분 구현된 normalization slice를 전체 auto approval 완성으로 주장하지 않는다.

---

## 상위 기준과의 관계

이 문서는 다음 기준을 전제로 한다.

1. `shacs-bot`은 self-hosted / personal-use assistant runtime이다. 기본 주체는 사용자가 직접 설치하고 운영하는 본인이다.
2. Docker container는 기본 운영 경계이자 primary containment다.
3. Docker containment는 host 손상 가능성을 낮추지만 permission 판단을 대체하지 않는다.
4. `MainOrchestrator` 또는 동등한 runtime policy layer가 permission 확정과 effect 실행의 유일한 권한자다.
5. LLM의 자기 점검은 참고 신호일 수 있지만 최종 허용 자체가 아니다.

교차 spec 관계:

| spec | Auto approval이 소비하는 것 | Auto approval이 소유하는 것 |
|---|---|---|
| 004 tool runtime | provider tool call, runtime executor, interrupt, skipped call, tool event 경계 | tool 실행 전 permission decision gate. tool 내부 구현은 소유하지 않음 |
| 007 main orchestrator policy | user intent, scope, turn ownership, orchestrator authority | permission mode와 evaluator verdict를 실행 결정으로 합성하는 계약 |
| 008 configuration profiles and runtime layout | config 저장, runtime path, user-local 설정 경계 | permission mode config shape와 migration 의미. secret backend는 소유하지 않음 |
| 010 host safety, permissions, and secrets | host safety, secret boundary, current local baseline, future permission primitive | formal permission mode와 auto approval decision table의 최종 계약 |
| 011 subagent runtime | inherited execution config, child tool registry restriction | child가 parent permission ceiling을 넓히지 못한다는 mode inheritance 계약 |
| 012 runtime services | channel worker, heartbeat, cron, local API background run boundary | background action에도 동일한 permission gate를 적용한다는 계약 |
| 013 user interfaces and session UX | approval surface, waiting state, projection rendering 의미 | auto approval decision과 user prompt의 상태 언어 |
| 014 observability diagnostics and inspection | diagnostics, redaction, trace evidence surface | permission audit record와 denied/auto-approved evidence 요건 |
| 017 app operating environment | app manifest permission declaration과 install/runtime 경계 | app 권한이 auto approval mode를 초과하지 못한다는 계약 |
| 018 evaluation automation and self-improvement | evaluator envelope, ledger, approval/checkpoint/automation language | action-level auto approval evaluator와 runtime consumption rule |
| 020 tool search and provider tool surface | provider-visible tool surface와 deferred bridge tool call | deferred tool도 실제 실행 전 같은 permission gate를 통과한다는 계약 |

이 문서는 010의 current local baseline 완료 선언을 바꾸지 않는다. 010이 future gap으로 둔 permission mode와 approval correlation을 최종 상태로 끌어내린 별도 owner contract다.

이 문서는 018을 대체하지 않는다. Evaluator와 ledger는 018의 언어를 소비하되, tool 실행 허용 여부는 이 문서와 orchestrator policy가 확정한다.

---

## 범위

이 문서는 다음을 정의한다.

1. Permission mode 최종 의미.
2. `PermissionedAction`과 action digest.
3. Auto approval evaluator 입력과 verdict.
4. 정적 rule, protected target, evaluator, approval cache의 적용 순서.
5. Docker primary containment 전제와 `exec` sandbox 비목표.
6. `proc_exec`, filesystem, network, secret, external delivery, automation, self modification capability 판단.
7. Subagent, app, MCP, deferred tool call의 permission ceiling.
8. 사용자에게 물어야 하는 경우와 자동으로 거절해야 하는 경우.
9. Audit, diagnostics, replay, contract test matrix.

이 문서는 다음을 정의하지 않는다.

1. Docker image build, compose 파일, container runtime 구현.
2. Shell command sandbox backend. Docker가 primary containment이므로 per-command sandbox 설계는 비목표다.
3. UI 화면 디자인.
4. Provider별 tool call wire format.
5. Secret vault backend.
6. 원격 운영 콘솔과 다중 사용자 권한 관리.
7. 구현 PRD. 이 문서는 owner spec이고 PRD를 만들지 않는다.

---

## 현재 구현 상태

현재 저장소는 Spec 022 일부만 구현했다. Full auto approval engine은 아직 완성된 상태가 아니다.

현재 구현으로 인정할 수 있는 것은 다음이다.

1. `PermissionMode`, `PermissionModeSource`, `SafetyCapability`, `AutoApprovalConfig`와 safe config normalization이 구현됐다.
2. `PermissionedAction`, `PermissionedActionOrigin`, action digest, argument digest, snapshot digest, redacted argument representation이 구현됐다.
3. Direct runtime tool call과 deferred bridge normalization이 구현됐다.
4. `ask_user`는 tool interrupt와 resume mechanism으로 남아 있으며 formal approval과 구분된다.

구현 증거는 다음 경로에 있다.

1. `crates/shacs-config/src/permissions.rs`
2. `crates/shacs-core/src/runtime/permission_action.rs`
3. `crates/shacs-core/tests/permission_action.rs`
4. `crates/shacs-core/tests/runtime.rs`

남은 open work는 다음이다.

1. Static protected target decision policy. 문서상 닫힌 것으로 보지 않는다.
2. Runtime policy decision table.
3. Auto evaluator.
4. Formal approval request, cache, correlation.
5. User-facing approval prompt.
6. Audit diagnostics.
7. Replay.
8. Full contract matrix.

---

## 핵심 용어

### PermissionMode

현재 session 또는 run에서 unmatched action을 어떻게 처리할지 정하는 runtime mode다. Mode는 user-local config, CLI flag, local API request, session command 중 명시된 source에서 온다. Prompt, skill, tool result는 mode source가 될 수 없다.

### PermissionedAction

실행 직전 tool call을 permission 판단 가능한 형태로 정규화한 envelope다.

필수 필드:

1. `action_id`.
2. `session_id` 또는 session key.
3. `turn_id`.
4. `tool_name`.
5. `capabilities`.
6. `target_refs`.
7. `argument_digest`.
8. `origin`, 예: user turn, subagent, cron wake, app task, local API.
9. `permission_mode_snapshot`.
10. `containment_snapshot`.
11. `intent_snapshot_ref`.

### SafetyCapability

Action이 요구하는 canonical capability다.

최소 capability set:

1. `fs_read`.
2. `fs_write`.
3. `proc_exec`.
4. `net_outbound`.
5. `secret_read`.
6. `external_delivery`.
7. `automation_schedule`.
8. `app_install`.
9. `runtime_config_write`.
10. `self_modification`.

### ApprovalDecision

Permissioned action에 대한 최종 decision이다.

Decision 종류:

1. `allow`.
2. `ask`.
3. `deny`.

`allow`는 실행 허용이다. `ask`는 사용자 입력이 필요하다는 뜻이다. `deny`는 실행하지 않고 denied outcome을 남긴다는 뜻이다.

### AutoApprovalEvaluator

Action이 사용자의 요청 범위와 active scope 안에 있는지 평가하는 별도 evaluator다. 같은 provider model을 쓸 수는 있지만, agent가 작성한 자연어 자기평가만으로 대체하면 안 된다.

---

## Permission mode 계약

최종 mode set은 다음을 포함한다.

| mode | 자동 실행 기본값 | 의미 |
|---|---|---|
| `plan` | read-only action만 | 조사와 계획. write, exec, delivery, schedule은 실행하지 않음 |
| `default` | low-risk read와 명시 allow rule만 | 민감 작업 기본값. side effect는 물어봄 |
| `accept_edits` | workspace 내부 일반 파일 edit/write | 코드 수정 반복 작업. exec와 외부 side effect는 별도 판단 |
| `auto` | evaluator가 allow한 action | 긴 작업. 사용자 prompt fatigue를 줄이되 deny-first와 evaluator를 통과해야 함 |
| `dont_ask` | 명시 allow rule만 | 비대화형 locked-down 실행. 물어볼 수 없으면 거절 |
| `bypass_permissions` | 대부분의 action | 격리된 Docker/dev container 전용 위험 모드. 명시적 opt-in으로만 가능 |

기본 mode는 `default`다.

`auto`는 시작 기본값이 될 수 있지만 user-local config 또는 explicit CLI/local API request에서만 가능하다. Workspace 안의 prompt, skill, app manifest, session memory가 `auto`를 켤 수 없다.

`bypass_permissions`는 다음 조건을 모두 만족할 때만 사용할 수 있다.

1. 사용자가 명시 flag 또는 user-local config로 켠다.
2. runtime이 Docker 또는 동등한 recognized containment 안에 있다고 확인한다.
3. runtime이 root 또는 host privileged mode로 실행 중이면 거절한다.
4. protected target circuit breaker는 여전히 남긴다.

`bypass_permissions`에서도 다음은 자동 허용되지 않는다.

1. container root 또는 mounted home root 전체 삭제.
2. auth store와 secret material의 raw export.
3. runtime binary 자기 교체.
4. host mount root escape가 감지된 path operation.

---

## Decision pipeline

모든 permissioned action은 아래 순서를 통과해야 한다.

1. Provider tool call을 `PermissionedAction`으로 정규화한다.
2. Tool name과 arguments schema를 검증한다.
3. Capability와 target refs를 계산한다.
4. Frozen snapshot을 만든다.
5. Static deny rules를 적용한다.
6. Protected target rules를 적용한다.
7. Permission mode baseline을 적용한다.
8. 명시 allow/ask rule을 적용한다.
9. 필요한 경우 auto approval evaluator를 호출한다.
10. Approval cache 또는 pending approval decision을 action digest와 대조한다.
11. 최종 `allow | ask | deny` decision을 만든다.
12. Audit record를 기록한다.
13. `allow`만 tool runtime으로 넘긴다.

순서 불변식:

1. `deny`는 항상 `allow`보다 우선한다.
2. Protected target은 evaluator가 `allow`해도 우회하지 않는다.
3. Permission mode가 action을 금지하면 evaluator hint는 실행으로 바뀌지 않는다.
4. Approval decision은 action digest, snapshot digest, request id가 맞을 때만 소비된다.
5. Stale, expired, mismatched decision은 `deny`로 접는다.
6. Evaluator failure는 `allow`가 아니라 `ask` 또는 `deny`다.

---

## Auto evaluator 계약

Evaluator input은 다음을 포함해야 한다.

1. 사용자 원 요청 요약.
2. 현재 turn의 explicit scope.
3. Action kind와 tool name.
4. Canonical capability set.
5. Target refs와 redacted arguments.
6. Diff 또는 command summary.
7. Docker containment snapshot.
8. Permission mode snapshot.
9. Known protected target 여부.
10. Prompt injection suspicion signals.
11. Prior approval refs.

Evaluator output은 다음을 포함해야 한다.

1. `verdict`: `allow_candidate`, `ask_user`, `deny_candidate`, `insufficient_context`.
2. `confidence`: bounded score 또는 enum.
3. `scope_match`: requested, adjacent, unrelated, hostile 중 하나.
4. `risk_summary`: 사용자에게 보여 줄 수 있는 짧은 설명.
5. `evidence_refs`: redacted evidence reference.
6. `expires_at` 또는 turn-scoped lifetime.

Evaluator는 다음 경우 반드시 `allow_candidate`를 내면 안 된다.

1. 사용자 요청과 action target이 직접 연결되지 않는다.
2. Action이 prompt-injected content의 지시에 의해 유도된 것으로 보인다.
3. External delivery 대상이 사용자 요청에 명시되지 않았다.
4. Secret read 또는 auth export가 포함된다.
5. Persistent automation이 새로 등록된다.
6. Runtime config, app registry, MCP exposure, skill activation 범위가 넓어진다.
7. Protected target write 또는 delete가 포함된다.
8. `proc_exec` command summary를 안전하게 분해할 수 없다.

Evaluator verdict는 최종 permission decision이 아니다. Runtime policy가 mode, static rule, protected target, approval cache를 합성해 최종 decision을 만든다.

---

## Docker containment와 exec sandbox

Docker는 이 프로젝트의 기본 운영 모델에서 primary containment다.

따라서 최종 스펙은 per-command shell sandbox를 필수 요구사항으로 두지 않는다. `bwrap`, seccomp profile, chroot 같은 exec sandbox backend는 선택적 hardening일 수 있지만 auto approval의 완료 조건이 아니다.

하지만 Docker containment는 다음을 대체하지 않는다.

1. `proc_exec` capability 판정.
2. Command intent 요약.
3. Dangerous command deny rule.
4. Workspace 또는 mounted volume target 확인.
5. Secret/env exposure 확인.
6. Network and external delivery permission.
7. Audit record.

Docker snapshot은 최소한 다음을 표현해야 한다.

1. `contained`: true 또는 false.
2. `container_runtime`: docker, podman, devcontainer, unknown 중 하나.
3. `root_user`: true 또는 false.
4. `privileged`: true 또는 false 또는 unknown.
5. `host_mounts`: redacted mount summary.
6. `network_mode`: none, bridge, host, unknown 중 하나.

`auto` mode에서 Docker snapshot이 unknown이면 `proc_exec`의 자동 승인 범위를 좁혀야 한다. `bypass_permissions`는 Docker snapshot이 contained로 확인되지 않으면 사용할 수 없다.

---

## Capability별 자동 승인 기준

### fs_read

`fs_read`는 workspace 내부 일반 파일, app bundle metadata, diagnostics redacted summary에 대해 자동 승인할 수 있다.

다음은 자동 승인하면 안 된다.

1. Secret file.
2. Auth store raw content.
3. Workspace 밖 host path.
4. Symlink escape target.

### fs_write

`accept_edits`와 `auto`는 workspace 내부 일반 source file write/edit을 자동 승인할 수 있다.

다음은 `auto`에서도 ask 또는 deny다.

1. `.git` 내부 write.
2. `.shacs-bot/auth.json`과 raw credential path.
3. Runtime config의 permission widening.
4. App registry install/enable/disable state.
5. Shell hook, startup script, package manager lifecycle script write.
6. Deleting large directory trees without explicit user request.

### proc_exec

`proc_exec`는 Docker 안에서도 action-level permission check를 통과해야 한다.

자동 승인 가능한 예:

1. 사용자가 요청한 빌드, 테스트, 포맷, lint 명령.
2. Workspace 내부 파일을 대상으로 하는 read-only inspection command.
3. 이미 정해진 verification command.

자동 승인하면 안 되는 예:

1. User request와 무관한 install, upgrade, global config 변경.
2. Secret, token, env dump.
3. Host mount 전체 삭제 또는 broad destructive command.
4. Network exfiltration이 포함된 command.
5. Daemon start, cron 등록, background long-running process.
6. Command text를 안전하게 요약할 수 없는 경우.

`exec` sandbox backend는 비목표지만, command deny pattern과 target classification은 필수다.

### net_outbound

Web fetch/search는 사용자 요청과 직접 관련된 public target이면 자동 승인할 수 있다.

Private, loopback, link-local, internal URL은 deny한다. Redirect 후 private target도 deny한다.

### secret_read

Secret raw value read는 auto approval 대상이 아니다.

Allowed behavior는 secret presence check, redacted key name, missing env report 같은 non-secret projection이다.

### external_delivery

Message send, email send, Slack/Discord/Telegram outbound, webhook call은 기본적으로 ask다.

자동 승인 가능한 경우는 사용자가 같은 turn에서 명시적으로 대상과 내용을 승인한 단발성 delivery에 한정한다.

### automation_schedule

Cron, heartbeat rule, persistent goal continuation, scheduled automation 등록은 기본적으로 ask다.

자동 승인 가능한 것은 이미 승인된 automation rule의 동일 scope 내 wake execution뿐이다.

### app_install

App install, enable, disable, uninstall은 auto approval 대상이 아니다. App Maker 또는 authoring flow가 만든 proposal도 approval/checkpoint/apply/verify를 통과해야 한다.

### runtime_config_write

Permission widening, tool exposure, MCP enabled tools, model/provider auth, runtime update config는 auto approval 대상이 아니다.

### self_modification

Runtime binary 교체, skill activation, system prompt 변경, self-improvement apply는 approval, checkpoint, verify, rollback contract를 통과해야 한다. Auto evaluator는 proposal을 만들 수 있지만 apply를 자동 승인하지 않는다.

---

## Subagent와 background runtime

Subagent는 parent permission mode와 capability ceiling을 상속한다.

불변식:

1. Child는 parent보다 넓은 permission mode를 가질 수 없다.
2. Parent가 `default`면 child가 `auto`로 승격할 수 없다.
3. Parent가 `auto`여도 child action은 별도 `PermissionedAction`으로 평가된다.
4. Child system prompt는 permission grant source가 아니다.
5. Child result는 parent approval decision을 새로 만들 수 없다.

Background runtime에도 같은 원칙을 적용한다.

1. Cron wake는 승인된 automation rule ref가 있어야 한다.
2. Local API background request는 request-level permission mode snapshot을 가져야 한다.
3. Channel inbound는 해당 channel/session scope를 벗어난 external delivery를 자동 승인하지 않는다.
4. Late result는 closed turn의 permission decision을 재사용하지 않는다.

---

## Approval cache와 사용자 decision

사용자가 한 번 승인한 decision은 action digest와 scope에 묶인다.

Approval request 필수 필드:

1. `approval_request_id`.
2. `action_digest`.
3. `snapshot_digest`.
4. `requested_scope`.
5. `risk_summary`.
6. `allowed_decisions`.
7. `expires_at`.

Approval decision 필수 필드:

1. `approval_request_id`.
2. `decision`: approved, denied, inspect_only.
3. `approved_scope`.
4. `actor`: local_user.
5. `decided_at`.

Decision consumption rules:

1. `approval_request_id`가 다르면 소비하지 않는다.
2. `action_digest`가 다르면 소비하지 않는다.
3. `snapshot_digest`가 다르면 소비하지 않는다.
4. `expires_at` 이후면 소비하지 않는다.
5. `inspect_only`는 approval로 해석하지 않는다.
6. Message acknowledgement는 approval로 해석하지 않는다.

---

## User-facing behavior

사용자가 보는 상태는 다음 언어를 사용한다.

1. `allowed_automatically`: 자동 승인됨.
2. `waiting_for_approval`: 사용자 확인 필요.
3. `denied_by_policy`: policy로 거절됨.
4. `denied_by_scope`: 요청 범위 밖이라 거절됨.
5. `denied_by_protected_target`: 보호 대상이라 거절됨.
6. `evaluator_unavailable`: evaluator 실패로 확인 필요.
7. `containment_unknown`: Docker containment 확인 불가로 확인 필요.

Auto-approved action도 사용자에게 숨기지 않는다. Progress, final summary, diagnostics는 어떤 action이 자동 승인됐고 왜 허용됐는지 redacted summary를 보여 줄 수 있어야 한다.

사용자 prompt는 짧아야 하지만 다음은 포함해야 한다.

1. Tool/action summary.
2. Target summary.
3. Risk summary.
4. Decision options.
5. Scope와 expiration.

---

## Failure behavior

다음은 자동 실행으로 이어지면 안 된다.

1. Evaluator timeout.
2. Evaluator output parse failure.
3. Low confidence.
4. Missing intent snapshot.
5. Missing containment snapshot for `proc_exec` in `auto` mode.
6. Unknown capability.
7. Unknown tool.
8. Stale approval.
9. Action digest mismatch.
10. Protected target uncertainty.

Fallback decision:

1. Interactive session이면 `ask`.
2. `dont_ask` 또는 non-interactive background run이면 `deny`.
3. `bypass_permissions`라도 circuit breaker target이면 `ask` 또는 `deny`.

Denied outcome은 provider에 tool error로만 숨기지 않고 diagnostics와 audit record에도 남긴다.

---

## Configuration 계약

최종 config는 user-local scope와 workspace scope를 구분해야 한다.

권장 shape:

```json
{
  "permissions": {
    "defaultMode": "default",
    "autoApproval": {
      "enabled": false,
      "requireDockerContainmentForExec": true,
      "allowWorkspaceEdits": true,
      "allowProcExecVerification": true,
      "protectedTargets": [".git", ".shacs-bot/auth.json"]
    }
  }
}
```

Config rules:

1. Workspace config는 `auto`를 제안할 수 있지만 user-local opt-in 없이 켤 수 없다.
2. Workspace config는 protected target을 줄일 수 없다.
3. User-local config는 자신을 더 제한할 수 있다.
4. `bypass_permissions`는 explicit runtime flag 또는 user-local opt-in이 필요하다.
5. Malformed config는 permission widening이 아니라 safe default로 돌아간다.

---

## Audit와 diagnostics

모든 permissioned action은 audit record를 남겨야 한다.

Audit record 필수 필드:

1. `action_id`.
2. `session_id`.
3. `turn_id`.
4. `tool_name`.
5. `capabilities`.
6. `target_summary`.
7. `argument_digest`.
8. `mode`.
9. `decision`.
10. `decision_reason`.
11. `evaluator_ref`.
12. `approval_ref`.
13. `containment_summary`.
14. `created_at`.

Audit record는 raw secret, full env, raw token, unredacted private URL credential을 저장하면 안 된다.

Diagnostics는 다음 질문에 답할 수 있어야 한다.

1. 왜 이 action이 자동 승인됐는가?
2. 왜 이 action이 사용자 확인으로 넘어갔는가?
3. 왜 이 action이 거절됐는가?
4. 어떤 mode와 snapshot이 적용됐는가?
5. Docker containment가 어떻게 판정됐는가?
6. Evaluator failure 또는 low confidence가 있었는가?

---

## Replay와 regression

Permission decision은 replay 가능해야 한다.

Replay input:

1. Frozen `PermissionedAction`.
2. Permission mode snapshot.
3. Containment snapshot.
4. Static rule version.
5. Evaluator version 또는 recorded evaluator output.
6. Approval decision ref.

Replay invariant:

1. 같은 snapshot과 same rule version이면 같은 final decision이 나와야 한다.
2. Rule이 더 엄격해진 replay는 old allow를 deny로 바꿀 수 있다.
3. Rule이 느슨해진 replay는 old deny를 자동 allow로 바꾸지 않는다.
4. Replay는 실제 tool을 실행하지 않는다.

---

## Contract test matrix

최종 구현은 최소한 아래 테스트 관점을 만족해야 한다.

| 테스트 | 기대 결과 |
|---|---|
| `default` mode에서 workspace write 요청 | ask |
| `accept_edits` mode에서 workspace 일반 파일 edit | allow |
| `accept_edits` mode에서 `cargo test` exec | ask 또는 evaluator 필요 |
| `auto` mode에서 사용자 요청과 일치하는 `cargo test` | allow |
| `auto` mode에서 unrelated install command | deny 또는 ask |
| `auto` mode에서 `.git/config` write | ask 또는 deny |
| `auto` mode에서 auth store raw read | deny |
| `auto` mode에서 external message send 대상 불명 | ask |
| `dont_ask` mode에서 unmatched write | deny |
| `bypass_permissions` without Docker containment | deny mode activation |
| Docker snapshot unknown인 `proc_exec` | ask 또는 deny |
| Evaluator timeout | ask 또는 deny, never allow |
| Approval action digest mismatch | deny |
| Expired approval | deny |
| Subagent attempts wider mode | deny |
| Deferred MCP tool call executes | same permission gate applied |
| Prompt-injected webpage asks to run command | deny 또는 ask, never silent allow |
| App install proposal apply without approval | deny |
| Runtime config permission widening | ask 또는 deny |
| Audit record includes raw token | test fails |

---

## 금지 패턴

다음 구현은 이 스펙을 위반한다.

1. Agent prompt에 “스스로 안전하면 진행하라”고 쓰고 permission engine 없이 실행한다.
2. Docker 안이라는 이유로 모든 `exec`를 자동 승인한다.
3. `allow_side_effect_tools`를 auto approval과 동일시한다.
4. `ask_user`를 formal approval engine으로 재사용하면서 action digest correlation을 생략한다.
5. Workspace config가 `auto`나 `bypass_permissions`를 조용히 켠다.
6. Evaluator failure를 allow로 처리한다.
7. Subagent에게 parent보다 넓은 tool surface나 mode를 준다.
8. App manifest permission declaration을 user approval로 해석한다.
9. External delivery acknowledgement를 approval로 해석한다.
10. Audit record에 raw secret을 저장한다.

---

## 완료 기준

Spec 022를 closed 상태로 전환하려면 다음이 모두 충족되어야 한다.

1. `PermissionMode`와 `PermissionedAction` 타입이 존재한다.
2. 모든 provider tool call과 deferred tool call이 실행 전 permission gate를 통과한다.
3. `auto` mode는 evaluator-mediated allow일 때만 묻지 않고 실행한다.
4. `proc_exec`는 Docker 안에서도 capability와 command summary 기반 permission check를 통과한다.
5. Docker containment snapshot이 permission decision에 들어간다.
6. `bypass_permissions`는 explicit opt-in과 containment 확인 없이는 활성화되지 않는다.
7. Approval request와 decision은 action digest와 snapshot digest로 correlation된다.
8. Stale, expired, mismatched approval은 실행으로 이어지지 않는다.
9. Subagent, cron, local API background, app task, MCP deferred call이 같은 permission ceiling을 따른다.
10. Audit와 diagnostics가 auto-approved, asked, denied action을 redacted evidence로 설명한다.
11. Contract test matrix의 필수 케이스가 통과한다.

## 결론

Auto approval은 이 프로젝트에서 유용한 최종 기능이다. 특히 Docker 중심 personal runtime에서는 사용자가 반복적으로 승인해야 하는 빌드, 테스트, 파일 수정 작업을 줄일 수 있다.

하지만 최종 계약은 “LLM이 한 번 더 생각했으니 그냥 진행”이 아니다. 올바른 계약은 frozen action snapshot, deny-first rule, Docker containment evidence, evaluator verdict, approval correlation, audit record를 가진 permission layer다.

Docker는 host 안전의 기본 격리 수단이다. 그래서 별도 exec sandbox를 완료 조건으로 두지 않는다. 대신 `proc_exec` permission check와 audit는 남긴다. 이 균형이 `shacs-bot`의 self-hosted / personal-use 성격에 맞는 auto approval의 최종 목표다.
