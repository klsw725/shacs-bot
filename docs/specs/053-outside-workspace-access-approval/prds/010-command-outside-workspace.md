# PRD 010. 명령의 workspace 밖 경로

Status: Implemented (independent review approved 2026-11-16)

Depends on: [PRD 000](000-continue-after-refusal.md)의 닫힌 결과(거절 사건과 그 종류, 시작 하나 단위의 셈, 고칠 수 있는 실패와 안내). 010의 `exec` 안전 장치와 경로 찾기 함수, 022의 판정 순서·기억된 규칙·승인 correlation, 030의 process gate, 033의 durable automation 경로, 048의 판정 표와 다른 mode의 더하기를 소비한다. 뒤 번호 PRD에 의존하지 않는다.

## Goal

`restrictToWorkspace`가 꺼진 run에서 `exec`가 workspace 밖 경로를 적어도 tool이 거절하지 않는다. 그 호출은 판정이 "workspace 밖 접근"으로 보고, auto mode에서는 classifier가, 다른 대화형 mode에서는 사용자가 정한다. 사람이 없는 run에서는 거절한다(`bypass_permissions`의 cron이 아닌 run만 예외). cron·heartbeat·결과 관찰 run은 믿을 수 있는 출처 표시로 서로 구분되어 tool 판정과 process gate까지 닿는다. 이 표시는 권한을 주지 않는다.

## 지금 상태

- `crates/shacs-core/src/tools/shell/guard.rs:112` `workspace_guard_enabled`는 `restrict_to_workspace || sandbox.is_none()`이다. 공식 Docker에는 쓸 수 있는 sandbox가 없으므로 `restrictToWorkspace: false`여도 guard가 켜진다.
- 같은 파일 `:8` `resolve_working_dir`는 guard가 켜지면 workspace 밖 `working_dir`을 `working_dir is outside the configured workspace`로 거절한다. `:55` `guard_command`는 deny pattern, allow pattern, 내부 주소를 본 뒤 guard가 켜지면 `../`(`path traversal detected`)와, `extract_absolute_paths`(`:139`)·`expand_shell_path`(`:157`)·`canonicalize_existing_prefix`(`:170`)로 찾은 절대 경로·`~` 경로가 **그 명령의 working directory**와 media 폴더 밖이면(`:84-105`) `path outside working dir`로 거절한다.
- `crates/shacs-core/src/tools/shell.rs:169` `preflight_refusal`과 `tools/shell/execution.rs:49-50`의 실행이 같은 두 함수를 부른다. auto mode는 `runtime/auto_mode/action_materials.rs:30-40`에서 tool의 `preflight_refusal`을 부작용 없이 불러 판정 재료(`PermissionMaterials.preflight_refusal`)에 넣고, 판정 표 5번(`auto_decision.rs:51`)이 그 호출을 `ToolRefusal`로 거절한다. 다른 mode는 `before_mode_rules`(`auto_decision.rs:228`)가 그 호출에 048의 질문을 하지 않고 022 판정 뒤 tool이 거절한다.
- 판정 재료는 `runtime/tool_execution.rs:800`의 `action_materials`가 만든다. `context.permission_environment`가 없으면 재료가 없다(`action_materials.rs:27`). 같은 재료가 `ProcessGatePolicyMaterials.materials`(`runtime/process_gate.rs:127-132`)로 process gate에 넘어가고, gate는 `decide_permission`을 다시 돈다(`process_gate.rs:231-240`).
- 다른 mode 판정은 `permission_policy.rs:150-171` `decide_permission`이 `before_mode_rules` → `decide_without_user_rules` → `after_mode_rules` 순으로 한다. `decide_without_user_rules`는 static deny, `plan`·`bypass_permissions`의 이른 반환, 기억된 거절, static ask, 이번 승인, allow 규칙, 기억된 허용, mode baseline 순이다(`:218` 이후). 묻지 못하면 `ask_or_deny`(`:552`)가 거절로 바꾼다. `dont_ask`는 `can_ask`(`auto_decision.rs:307`)가 다룬다.
- 자동화 출처: `crates/shacs-cli/src/automation_producer.rs:81-104` `enqueue_cron`은 envelope의 `AutomationSourceEventKind::Cron { approved_automation_rule_ref: Some("cron-job:<id>") }`에 원래 cron 작업 id를 이미 싣는다(`event.job_id`는 event id다). heartbeat(`:50`, `:66`)와 하위 작업 결과(`:17`)도 같은 envelope 형식이다. 그러나 `runtime/automation_dispatch.rs:215-245` `normalized_request`는 `AutomationDispatchRequest`(`runtime/automation_adapter.rs:83-94`)에 출처를 담지 않고, `automation_worker.rs:327-363`은 generic `ProviderTurnOrigin { channel: "automation", .. }`로 `AgentLoop`를 부른다. `AgentLoop`는 `in_cron_context: false`(`agent_loop.rs:1913`)로 tool 문맥을 만들어, cron run도 tool 판정에서는 다른 자동화 run과 같다.
- `PermissionedActionOrigin::CronWake { job_id }`는 `tool_execution.rs:636`에서 `job_id: None`으로만 만들어진다. cron 권한 한도의 `approval_ref` 요구(`permission_ceiling.rs:151-159` `MissingApprovalRef`)는 `permission_ceiling_snapshot`의 출처가 `CronWake`일 때만 걸리며, 지금 그런 snapshot을 만드는 곳은 없다.
- PRD 000의 거절 종류는 `runtime/tool_refusal.rs:12-54`(`RefusalKind`, 결과 글로 알아보는 `from_text`)이고, 셈의 root id는 `agent_loop.rs:1067-1088`이 `user:<turn>:<pid>:<time>:<nonce>`로 만든다. 자동화 run도 지금은 같은 `user:` 꼴이다.
- 기존 시험: `crates/shacs-core/tests/tools.rs:1763` `exec_tool_restricts_working_dir_and_paths_to_workspace`(restrict 켬), `:1802` `exec_tool_native_unknown_without_backend_enforces_workspace_scope`(restrict 끔·sandbox 없음에서 거절을 기대), `tests/it/spec048_auto_mode/decision_table.rs`의 "exec naming a path outside the workspace without a sandbox" 줄(모든 칸 `Refuse`).
- `docs/USAGE.md:850`은 "현재는 sandbox가 없어도 guard가 켜지므로 설정을 끄는 것만으로는…"이라 적는다.

## Scope

1. **guard 조건:** `workspace_guard_enabled`는 `restrict_to_workspace`만 본다. sandbox 설정 유무는 guard를 켜거나 끄지 않는다. guard가 켜진 run의 동작(working directory 기준 비교, `working_dir` 검사, `../`, media 허용)은 지금 그대로다. deny pattern, allow pattern, 내부 주소 검사는 guard와 상관없이 지금 그대로다.
2. **workspace 밖 대상 찾기:** `ExecTool`에 부작용 없는 `outside_workspace_targets(&params) -> Vec<String>`을 둔다(`Tool` trait의 기본 구현은 빈 목록).
   - 허용 범위는 workspace(`PathContext.workspace`, 없으면 `ExecConfig.working_dir`)와 media 폴더다. 비교 기준은 명령의 working directory가 아니라 이 범위다.
   - 대상: `extract_absolute_paths`로 찾은 절대 경로와 `~` 경로를 `expand_shell_path`·`canonicalize_existing_prefix`로 풀어 허용 범위 밖인 것, 명령 글에 `../`나 `..\`가 있으면 `..` 표시 하나, `working_dir` 인자를 풀어 허용 범위 밖이면 그 경로. 중복을 빼고 정렬한다.
   - guard가 켜진 run(restrict 켬)에서는 빈 목록이다(그 run은 tool이 거절한다).
   - 경로 찾기 함수는 바꾸지 않는다. 찾은 경로를 거절로 바꾸는 부분(guard)과 판정 재료로 쓰는 부분을 나눈다.
3. **guard가 꺼진 run의 실행:** `resolve_working_dir`은 workspace 밖 `working_dir`도 풀어 쓴다(풀 수 없으면 지금처럼 `working_dir could not be resolved`). `guard_command`는 workspace 밖 경로를 거절하지 않는다. 판정이 허용한 호출만 실행에 닿는다.
4. **판정 재료:** `permission_evaluation_for_action`(`tool_execution.rs`)이 registry의 tool에서 workspace 밖 대상을 한 번 구하고, 그 run의 자동화 출처와 함께 `OutsideAccess { targets, automation_source }`(`runtime/outside_access.rs`)로 묶어 `PermissionPolicyInput.outside_access`와 `ProcessGatePolicyMaterials.outside_access`에 같은 값을 넣는다. 판정 재료(`PermissionMaterials`)가 없을 때도 이 값은 있다. process gate는 받은 값을 다시 계산하지 않고 그대로 `decide_permission`에 넘긴다. 그래서 tool 판정과 process gate가 같은 판정을 낸다. `PermissionPolicyInput`의 이 칸은 직렬화하지 않는다(`serde(skip)`). 저장된 값이 출처를 흉내 내지 못하게 하려는 것이다. 승인 질문에는 이 대상 경로가 나온다(`approval_why`).
5. **auto mode:** workspace 밖 대상이 있는 호출은 판정 표 14번(읽기 바로 허용)과 15번(workspace 쓰기 바로 허용)에 들지 않고 16번 classifier로 간다. 1–13번은 지금 그대로 먼저 걸린다.
6. **auto가 아닌 대화형 mode:** `after_mode_rules` 뒤에 한 줄을 더한다. 결과가 허용이고 workspace 밖 대상이 있으면 묻기로 바꾼다. 단 이유가 이번 호출 승인(`ApprovalAccepted`, 세션 승인 포함), 설정 allow 규칙(`UserRuleAllow`), 기억된 허용(`RememberedAllow`)이거나 mode가 `bypass_permissions`이면 바꾸지 않는다. 묻지 못하면(`dont_ask`) 거절이다(`can_ask`). 거절과 묻기, `plan`의 금지는 그대로다.
7. **사람이 없는 run:** 모든 mode에서(auto 포함), `permission_interactive`가 꺼진 run의 호출에 workspace 밖 대상이 있고 결과가 거절이 아니면, 아래가 아닌 한 거절한다(새 이유 `OutsideWorkspaceUnattended`).
   - cron이 아닌 run이고 mode가 `bypass_permissions`이다.
   - (PRD 030이 더한다) cron run이고 그 작업의 승인된 목록에 맞는다. 010에서는 cron run의 workspace 밖 접근이 `bypass_permissions`에서도 거절된다.
   - 설정 allow 규칙, 기억된 허용, 세션 승인, classifier 허용은 이 줄을 풀지 못한다.
   - 이 줄은 6번과 auto mode 판정이 끝난 뒤, `decide_permission`의 마지막에 한 자리에서 건다. 그래서 tool 판정과 process gate가 같다.
8. **자동화 출처:** 믿을 수 있는 run 출처 `AutomationRunSource`(`Cron { job_id, work_id }`, `Heartbeat { work_id }`, `SubagentResult { work_id }`, `Other { work_id }`)를 둔다.
   - `normalized_request`가 envelope의 `event.source`에서 만들어 `AutomationDispatchRequest.run_source`에 담는다. cron의 `job_id`는 `approved_automation_rule_ref`의 `cron-job:` 뒤 값이다. 값이 없거나 꼴이 다르면 `job_id: None`인 `Cron`이다(cron이라는 사실은 남는다).
   - envelope 형식은 바꾸지 않는다. 그래서 이미 저장된 자동화 작업도 그대로 읽히고 출처가 정해진다.
   - worker는 `execute_agent_automation`에서 그 turn의 `AgentLoopConfig.automation_source`에 넣는다. message metadata로 넘기지 않는다(채널 입력이 흉내 낼 수 없게 한다). `AgentLoop`는 그 값을 `ToolExecutionContext.automation_source`에 넣고, 판정 입력과 process gate 재료에 함께 싣는다(4번과 같은 자리). deferred bridge 호출은 부모 tool 문맥을 그대로 쓰므로 같은 출처를 지닌다.
   - `PermissionedActionOrigin`, action digest, 승인 correlation, `permission_ceiling_snapshot`은 바꾸지 않는다. 그래서 cron 권한 한도의 `approval_ref` 요구가 새로 걸리지 않고 지금의 cron run은 그대로 돈다. 이 출처는 권한을 주지 않으며, 7번의 거절 판단과 PRD 000의 셈 범위에만 쓰인다.
   - local API run은 자동화 경로를 지나지 않으므로 출처가 `None`이고, 7번에서는 cron이 아닌 run이다.
9. **거절 사건과 셈의 범위:** 7번의 거절은 PRD 000의 거절 사건이다.
   - `permission_block_message`가 이 이유에 `outside the workspace in an unattended run` 문구가 든 글을 만들고, `RefusalKind::from_text`가 그 문구를 새 종류 `UnattendedOutside`로 알아본다. 대상은 workspace 밖 대상 목록이다. 회복 가능한 결과에는 PRD 000의 안내가 붙는다.
   - 자동화 run의 root id는 출처로 만든다: `cron:<job_id 또는 unknown>:<work_id>`, `heartbeat:<work_id>`, `subagent-result:<work_id>`, `automation:<work_id>`. 한 번 울림·한 번 실행이 시작 하나다. 출처가 없으면 지금처럼 `user:` 꼴이다.
10. **문서:** `docs/USAGE.md`의 `exec` 안전 장치와 `restrictToWorkspace` 설명(`:740`, `:789`, `:844-850` 근처)을 이 동작으로 고치고, README의 권한 설명에 해당 문장이 있으면 함께 고친다. 앞선 스펙 문서는 고치지 않는다.

## 결정과 이유

- **찾기와 거절을 나눈다.** 경로를 찾는 함수는 그대로 두고, guard는 restrict가 켜진 run에서만 거절한다. 찾은 경로는 따로 판정 재료가 된다. 기존 거절 문구가 꺼진 run에서 `preflight_refusal`로 남으면 auto mode 표 5번이 계속 거절하기 때문이다.
- **working directory가 아니라 허용 범위와 비교한다.** working directory가 `workspace/sub`일 때 `workspace/file`을 밖으로 보던 지금 guard의 비교는 판정 재료에 맞지 않는다. restrict가 켜진 run은 지금 비교를 그대로 쓴다(SPEC 소유 범위 1).
- **`../`는 꼴만으로 밖으로 본다.** 셸 문법을 따라 정확히 풀지 않는다(새 명령 분석을 더하지 않는다). workspace 안에서 `cd ..`처럼 안전한 꼴도 묻거나 classifier로 간다. 지금은 아예 거절이므로 좁아지지 않는다.
- **재료를 `PermissionMaterials` 밖에 둔다.** 재료는 `permission_environment`가 없으면 만들어지지 않는다. workspace 밖 접근은 그때도 판정되어야 하므로, 판정 입력과 gate 재료에 같은 값을 따로 싣는다.
- **묻기 줄은 판정 끝에 둔다.** 022·048이 낸 거절과 묻기는 그대로 두고 허용만 묻기로 바꾼다. 사용자가 직접 준 근거(이번 승인, allow 규칙, 기억된 허용)는 존중한다(SPEC 소유 범위 3). `default`의 읽기 자동 허용과 `accept_edits`의 편집 자동 허용은 이 줄에서 묻기가 된다. 022 `fs_read` 기준("workspace 밖 host path는 자동 승인하지 않는다")과 같다.
- **사람이 없는 run의 거절도 판정 끝 한 자리에 둔다.** 그래야 auto mode의 classifier 허용, 기억된 허용이 이를 풀지 못하고, gate도 같은 판정을 낸다.
- **출처는 envelope을 바꾸지 않고 읽는다.** envelope에 이미 출처(`event.source`)와 원래 cron 작업 id(`cron-job:<id>`)가 들어 있다. 새 칸을 더하지 않고 dispatch 때 읽으면 이미 저장된 작업도 그대로 읽히고, serde 호환 문제가 생기지 않는다.
- **출처는 message metadata가 아니라 config로 넘긴다.** metadata는 채널 입력이 채울 수 있다. worker만 쓰는 `AgentLoopConfig.automation_source`로 넘기면 채널 메시지가 cron이나 heartbeat를 흉내 내지 못한다.
- **`PermissionedActionOrigin`과 권한 한도는 바꾸지 않는다.** `CronWake` snapshot을 만들면 `approval_ref`가 없어 지금의 cron run이 모두 거절된다(`permission_ceiling.rs:151-159`). 010의 출처는 권한을 주지 않으므로 한도에 닿을 필요가 없다. 승인 참조를 한도에 잇는 일은 PRD 030이 한다.
- **cron run은 010에서 `bypass_permissions`도 거절한다.** SPEC 소유 범위 3은 cron run의 workspace 밖 접근을 승인된 목록에 맞을 때만 허용한다. 목록은 PRD 030이 만들므로, 010이 닫힌 상태에서는 목록이 늘 비어 있어 거절이다. 목록 없는 지금의 cron은 지금도 guard에 막히므로 좁아지지 않는다(sandbox 없는 공식 Docker 기준).
- **local API는 출처가 없다.** local API run은 자동화 경로를 지나지 않고 `permission_interactive`가 꺼져 있으므로 7번에서 cron이 아닌 비대화형 run이다. 그 이상의 구분은 SPEC이 요구하지 않는다.

## Non Scope

- file tool의 workspace 밖 대상(PRD 020). `read_file`·`write_file`·목록·검색은 이 PRD에서 지금처럼 허용 범위 밖을 거절한다.
- cron 작업의 목록, 승인 기록, 취소, 명령 항목, 좁은 allow 규칙의 `ProcExecSummaryUnavailable` 예외(PRD 030).
- 하위 작업과 worktree workflow 자식의 `restrict_to_workspace`(011, 그대로 켜짐).
- deny pattern, allow pattern, 내부 주소 검사, process gate의 격리 증명, sandbox adapter.
- 명령 분석(048)과 경로 찾기 함수의 확장. 변수로 만든 경로는 지금처럼 찾지 못한다.
- 폴더 단위 기억. `exec`의 기억된 허용(`ExecPrefix`)은 지금 그대로다.

## Parent Requirement Mapping

- 053 소유 범위 1·2·3 중 `exec`, 판정과 process gate가 함께 쓰는 workspace 밖 대상 재료, auto가 아닌 mode의 묻기 줄, 비대화형 run 줄(cron 목록 없이 거절), 소유 범위 5의 믿을 수 있는 자동화 출처와 cron 작업 id 넘기기, 소유 범위 4의 새 거절 종류와 자동화 root, 소유 범위 6 중 자기 변경의 문서.
- 독립 종료 조건: 수용 조건 2, 3의 명령 항목, 4.7 중 명령과 출처 구분, 5, 6.
- 앞선 스펙에서 바꾸는 것: 010 SPEC의 workspace guard 조건, 048 PRD 000 판정 표 5·14·15번과 다른 mode의 판정(명령 부분), 033·022의 자동화 출처(010 줄). 앞선 스펙 문서는 고치지 않는다.

## Acceptance Criteria

1. guard: `restrictToWorkspace: false`, sandbox 없음에서 `… > /tmp/x.txt`, `cat ~/x`, `../`가 든 명령, workspace 밖 `working_dir`이 `preflight_refusal`과 실행 모두에서 거절되지 않는다. working directory가 workspace 하위 폴더일 때 workspace 안 다른 절대 경로와 media 폴더 경로는 workspace 밖 대상이 아니다. `restrictToWorkspace: true`면 지금처럼 거절된다(working directory 기준 비교 포함). `sandbox: "bwrap"`이 적혀 있어도 guard 판단이 sandbox 없을 때와 같다. deny pattern, allow pattern, 내부 주소 거절은 두 설정 모두에서 그대로다.
2. auto mode(048 판정 표 시험에 `restrictToWorkspace`가 꺼진 줄을 더한다): workspace 밖 경로를 적은 명령이 classifier로 가고, 허용이면 실행되며(process gate도 통과), 막음·판정 없음이면 실행되지 않는다. deny·ask 규칙, critical path, 048 조건을 지킨 allow 규칙·기억된 허용은 그 앞에서 걸린다. 기존 "exec naming a path outside the workspace without a sandbox" 줄은 restrict 켬·끔 두 줄로 나뉜다.
3. 다른 대화형 mode: `default`·`accept_edits`에서 workspace 밖 명령은 묻고(질문에 workspace 밖 대상이 적힌다), 승인한 그 호출만 실행된다. 기존 판정이 설정 allow 규칙이나 기억된 허용으로 이미 허용한 호출은 밖 경로 때문에 새로 묻지 않는다. 기존 판정이 허용하지 않은 호출은 그 판정 그대로다: `ProcExecSummaryUnavailable` 같은 정적 질문은 allow 규칙보다 먼저 남고(좁은 allow 규칙 예외는 PRD 030 몫), `dont_ask`의 기본 거절도 그대로다. `dont_ask`에서 allow 규칙·기억된 허용 없이 허용으로 끝난 workspace 밖 명령은 물을 수 없어 거절된다. `bypass_permissions`에서는 실행된다. `plan`은 명령을 지금처럼 막는다.
4. 비대화형 run: production 경로(cron이 울려 automation 작업을 넣고 worker가 `AgentLoop`를 부른다)의 cron run과 heartbeat run이 서로 다른 출처로 tool 판정에 닿는다. 기억된 허용과 설정 allow 규칙이 있어도 cron·heartbeat·local API run의 workspace 밖 명령은 거절되고, cron이 아닌 비대화형 run은 `bypass_permissions`에서 실행된다. cron run은 `bypass_permissions`에서도 거절된다. 이 거절은 PRD 000의 거절 사건(안내 포함, 같은 거절 세 번째에 멈춤)이고, 자동화 run의 셈은 그 run 하나로 끝난다. 출처 칸이 없던 이미 저장된 automation 작업도 읽혀 출처가 정해진다. 지금의 cron run(workspace 안 명령)은 권한 한도 때문에 새로 거절되지 않는다.
5. 2026-11-15 유튜브 흐름(skill 읽기, 자막 받기, `> /tmp/… ; wc -lc /tmp/…`, 요약)이 auto mode와 classifier 허용에서 승인 질문 없이 끝나고 `/tmp`에 파일이 생긴다(가짜 provider, 실제 실행 기록으로 확인). 같은 흐름에서 `restrictToWorkspace: true`면 `/tmp` 쓰기가 거절되고 안내를 받은 어시스턴트가 workspace 안에 써서 끝낸다.
6. fmt(Google 옵션), workspace clippy `-D warnings`, `shacs-core`·`shacs-cli` 패키지 테스트가 통과한다. SPEC "바뀌는 기존 시험" 중 010 몫(`tests/tools.rs`의 restrict 끔·sandbox 없음 시험, 048 판정 표의 밖 경로 줄)은 의도를 적고 기대를 바꾼다.

## 구현 메모 및 검증 근거

- `tools/shell/guard.rs`: guard는 `restrictToWorkspace`만 보고 켜진다(sandbox 이름은 보지 않는다). `command_outside_workspace_targets`가 workspace와 media 폴더를 허용 뿌리로 두고, 그 밖의 절대 경로·`~`·`..` 표시·workspace 밖 `working_dir`을 대상으로 낸다. `Tool::outside_workspace_targets`(기본은 빈 목록)를 `exec`가 구현한다.
- `runtime/outside_access.rs`: `OutsideAccess{targets, automation_source}`를 `decide_permission` 끝에서 한 번 적용한다. 거절은 그대로 두고, 비대화형 run은 cron이 아닌 run의 `bypass_permissions`만 통과시킨다(cron은 PRD 030의 목록 자리 `cron_list_allows`가 아직 늘 false). 대화형 run은 auto·bypass가 아니고 이번 호출 승인·설정 allow 규칙·기억된 허용으로 허용된 것도 아니면 묻기(`OutsideWorkspaceAccess`), 물을 수 없으면 거절이다. 같은 `OutsideAccess`를 process gate 재판정에도 넘긴다.
- `runtime/automation_run_source.rs`: 저장된 automation 작업에서 출처(cron·heartbeat·subagent 결과·그 밖)를 정한다. 출처 칸이 없던 작업도 읽힌다. worker가 `ProviderTurnInvocation.automation_source`로 `AgentLoopConfig`에 넣고, `ToolExecutionContext.automation_source`(직렬화 제외)로 판정에 닿는다. 메시지 metadata로는 들어오지 않는다.
- 판정 이유 `OutsideWorkspaceAccess`·`OutsideWorkspaceUnattended`를 더했다. 비대화형 거절 글은 `unattended_outside_block_text()` 하나이고, `RefusalKind::UnattendedOutside`가 그 글 전체가 같을 때만 PRD 000 거절로 센다.
- 독립 검토(REQUEST_CHANGES) 뒤 고친 것:
  - bridge 호출: 받아들이는 자리의 context에는 해석 중 받은 classifier 판정이 없어 `EvaluatorUnavailable`로 다시 계산되며 classifier 막힘으로 빠졌다. 결과 글이 비대화형 거절 글과 정확히 같으면 classifier 막힘으로 보지 않고 거절로 센다.
  - 거절 열쇠: `UnattendedOutside`는 경로를 다시 파싱하지 않고 판정이 쓴 `outside_workspace_targets`로 만든다(`ToolRefusal::with_targets`). workspace 안 인자를 바꿔도 셈이 이어지고, 다른 밖 대상은 따로 센다.
  - classifier 막힘 셈: classifier나 규칙이 허용했는데 비대화형 밖 접근으로만 거절된 호출은 `auto_mode_blocks`의 막힘·미실행으로 세지 않고, 진단 결과도 `no_verdict`가 아니다.
  - 승인 질문: 이유가 무엇이든 밖 대상이 있으면 `Why:`에 "this call reaches outside the workspace: …"가 붙는다(다른 이유가 있으면 그 뒤에 "Also, …").
  - 재검토 뒤: 질문에 적는 밖 대상은 `.env.json` 값을 가린 사본(`shown_outside_targets`)이다. 판정·process gate·거절 열쇠는 가리기 전 대상을 쓴다. 밖 경로에 비밀 값이 든 `default` 질문과 세션에 저장된 승인 대기 질문이 표시만 보이고 값은 보이지 않는 시험을 더했다(가리기를 끄면 둘 다 실패).
  - 수용 기준 3 문장을 실제 판정 순서(앞선 정적 질문과 `dont_ask` 기본 거절은 그대로, 좁은 allow 규칙 예외는 PRD 030)에 맞게 고쳤다.
- 시험(`tests/it/spec053_outside_exec.rs`): 실제 `exec` 실행으로 2026-11-15 흐름(skill 읽기 → `/tmp` 대신 시험 폴더 밖에 쓰고 `wc -lc` → 요약)이 질문 없이 끝나고 파일이 생기는 것, restrict 켬에서 거절·안내 뒤 workspace 안에 써서 끝나는 것, auto 비대화형 cron run의 bridge 호출이 classifier 허용을 받은 뒤에도 거절로 세어져 셋째에 멈추고 classifier 막힘 셈이 0인 것, 안쪽 인자만 바꾼 명령이 셈을 잇는 것, 다른 밖 대상이 따로 세어지는 것, `default` 질문에 밖 대상이 적히는 것을 본다. 세 고침을 하나씩 되돌리면 각각 이 시험 중 하나가 실패하는 것을 확인했다.
- 시험(`shacs-cli/src/automation_worker/outside_access_tests.rs`): production worker(`execute_agent_automation`) → `AgentLoop` → tool 판정을 지나, `bypass_permissions`에서 cron run의 밖 쓰기는 PRD 000 안내와 함께 거절되고 heartbeat run은 실행되며, cron run의 workspace 안 쓰기는 그대로 실행된다. worker의 출처 대입을 지우면 cron 시험이 실패한다. 시험 기계의 `/proc`에 기대는 격리 확인 때문에 `tests::CONFIRMED_CONTAINMENT`(시험 전용 thread-local)를 더했다.
- 남는 한계: cron run의 밖 접근 허락(목록)은 PRD 030 몫이라 지금은 모든 mode에서 거절된다. file tool의 밖 경로는 PRD 020 몫이다.
- 독립 검토: 첫 검토 REQUEST_CHANGES(위 6건), 재검토 REQUEST_CHANGES(질문의 밖 대상 비밀 값 노출 1건), 세 번째 검토 APPROVE(2026-11-16, 앞선 지적 모두 포함).
- 최종 검증 로그: `/tmp/spec053-prd010-approved-tests.log`(재검토 고침 뒤 최종 코드). 전체 workspace 테스트는 스펙 종료 때 실행한다.
- `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true`: exit 0.
- `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`: exit 0. 기존 의존성 `imap-proto v0.10.2`의 future-incompatibility 알림은 남아 있다.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-core -q`: exit 0, 79개 결과 묶음에서 2056 passed, 0 failed, 5 ignored.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q`: exit 0, 20개 결과 묶음에서 717 passed, 0 failed, 3 ignored.
- 처음 시작한 검증 두 번은 결과 없이 끝났다(백그라운드 프로세스가 실행 셀과 함께 끝났고, 한 번은 고침 전 코드여서 멈췄다). 통과로 기록하지 않는다.
