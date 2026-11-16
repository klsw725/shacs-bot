# PRD 030. cron 작업의 미리 허락된 목록

Status: Implemented (independent review approved 2026-11-17)

Depends on: [PRD 000](000-continue-after-refusal.md)(거절 사건, 안내, 셈), [PRD 010](010-command-outside-workspace.md)(workspace 밖 대상 재료 `OutsideAccess`, 비대화형 거절, 믿을 수 있는 자동화 출처 `AutomationRunSource::Cron { job_id, .. }`과 그 전달, `cron_list_allows` 자리), [PRD 020](020-file-tools-outside-workspace.md)(file tool의 workspace 밖 대상)의 닫힌 결과. 012의 cron 작업 저장·실행(`shacs-cron`), 033의 automation producer·worker, 022의 static 규칙(`ProcExecSummaryUnavailable`)과 권한 한도(`CronWake` `approval_ref`), 048의 사용자 규칙·명령 분석·넓은 규칙·리디렉션 조건·비밀 파일·보호 경로를 소비한다.

## Goal

사용자가 cron 작업을 만들 때 그 작업이 사람 없이 돌 때 쓸 권한 목록(workspace 밖 경로 항목과 명령 항목)을 함께 승인할 수 있다. 승인한 목록은 그 작업의 cron 실행에서만, 승인 기록이 유효한 동안만 쓰인다. 작업을 지우면 그 즉시 목록은 효력을 잃는다.

## 지금 상태

- `crates/shacs-core/src/tools/cron.rs` `CronTool`은 `add`(`message`, `every_seconds`·`cron_expr`·`at`, `tz`, `deliver`, `name`), `list`, `remove`를 받는다. 권한 목록 칸은 없다. `add`는 `CronService::add_job`(`crates/shacs-cron/src/lib.rs:280`)을 부른다.
- `PersistentCronService`(`shacs-cron/src/lib.rs:460`)는 작업 파일과 같은 폴더의 `action.jsonl`(action journal)을 쓴다. 그런데 gateway scheduler(`crates/shacs-cli/src/lib.rs` `start_automation_cron_runtime`)는 `<data>/runtime/cron/jobs.json`을, `cron` tool의 production 등록(`spec035_cli_media/tool_registry.rs:70`)은 `bundle.context.runtime_subdir("cron")` 곧 `<data>/cron/jobs.json`을 연다. 그래서 지금은 대화에서 만든 작업을 scheduler가 보지 못한다(2026-11-17 서버 확인: `~/.shacs-bot/cron/`은 비었고 `runtime/cron/jobs.json`만 있다). `remove_job`(`:823`)은 사용자 삭제(cron tool, CLI) 경로이고, 한 번만 도는 작업(`delete_after_run`)의 자동 정리는 실행 뒤 다른 자리(`:1134` 근처)에서 한다. scheduler와 tool registry는 서로 다른 instance를 쓴다(`crates/shacs-cli/src/lib.rs`, `spec035_cli_media/tool_registry.rs`).
- cron이 울리면 `automation_producer.rs:81` `enqueue_cron`이 envelope에 `approved_automation_rule_ref: Some("cron-job:<id>")`를 싣고, PRD 010의 `normalized_request`가 이를 `AutomationRunSource::Cron { job_id, work_id }`로 읽어 worker → `AgentLoop` → tool 판정으로 넘긴다.
- PRD 010의 `runtime/outside_access.rs` `cron_list_allows`는 늘 `false`다. 그래서 cron run의 workspace 밖 접근은 모든 mode에서 거절된다.
- 022 static 규칙: 요약할 수 없는 `exec`는 `ProcExecSummaryUnavailable` static ask(`permission_rules.rs:296-318`)이고, auto가 아닌 mode에서 allow 규칙보다 먼저 묻거나(대화형) 거절한다(비대화형, `permission_policy.rs` `static_ask_blocks_approval`).
- 022 권한 한도: `evaluate_inherited_ceiling`(`permission_ceiling.rs:139`)은 출처가 `CronWake { approval_ref: None }`인 한도를 `MissingApprovalRef`로 거절한다. 지금 그런 한도를 만드는 곳은 없다(cron run도 `UserTurn` 한도).
- 048: 사용자 규칙은 `PermissionEnvironment.user_rules`, 판정 재료는 `permission_materials.rs`가 만든다. `exec` 리디렉션 대상 확인은 `target_allowed`(`:371`)가 workspace 안이거나 `read`·`edit` allow 규칙에 맞는지 본다. 넓은 `exec` 규칙은 `exec_pattern.rs:166` `is_broad`. 명령 분석은 `shell_command::analyze`, 경로는 `shell_paths::subcommand_paths`(`reads`, `writes`). 비밀 파일 판정은 `secret_file.rs`(data 폴더 기준, `auth/`, 브라우저 profile, 최상위 비밀 파일 이름).

## Scope

1. **목록 인자:** `cron` tool의 `add`에 `permissions` 배열을 더한다. 항목은 둘 중 하나다.
   - 경로 항목 `{"path": "<절대 경로 또는 ~ 경로>", "access": "read" | "write"}`. 파일이면 그 파일, 폴더면 그 아래 전부다. `write`는 읽기를 포함한다. 상대 경로와 `..`가 든 경로는 받지 않는다.
   - 명령 항목 `{"command": "<048 exec pattern>"}`. `exec(<pattern>)`의 괄호 안과 같은 문법이다. 048 `is_broad`인 pattern은 받지 않는다.
   - 정규화: 경로는 `~`를 풀고 lexical 정규화한다. 항목을 정렬하고 중복을 뺀다. digest는 정규화한 목록의 SHA-256이다.
   - `CronJob`에 `permissions` 칸을 더한다(`serde(default)`, 없으면 빈 목록). `list`는 각 작업의 목록을 보인다. 수정 동작은 더하지 않는다.
2. **등록 판정(묻기):** 목록이 비어 있지 않은 `cron add`는 판정 끝(PRD 010의 `outside_access::decide`와 같은 자리)에서 아래로 바꾼다.
   - 대화형 run: 거절(deny 규칙, static deny, 기억된 거절, `plan`의 금지)은 그대로 둔다. 그 밖은 이번 호출 승인(`ApprovalAccepted`)이 아니면 새 이유 `CronPermissionList`로 묻는다. auto mode classifier, allow 규칙, 기억된 허용, `bypass_permissions`, `dont_ask`의 거절은 이 질문을 대신하지 않는다. 질문은 한 번 승인·한 번 거절만 받는다(once only).
   - 비대화형 run: 거절한다.
   - 질문의 `Why:`에 작업 이름, 일정, 지시, 목록의 모든 항목을 적는다(`.env.json` 값은 가린다).
   - 등록 거절: 경로 항목이 비밀 파일을 가리키거나(읽기·쓰기 모두), 보호 경로를 가리키는 쓰기이면 판정에서 `CronPermissionListRefused`로 거절한다. 판정 재료(`permission_environment`)가 없으면 목록이 있는 `add`는 거절한다. 형식이 틀린 항목과 넓은 명령 pattern은 tool의 `validate_params`가 거절한다. home·data 폴더를 읽기 항목으로 적는 것은 받는다.
3. **승인 기록:** 승인된 `add`가 실행되면 `CronTool`이 작업을 만든 뒤 `runtime/cron/approvals.json`에 기록 `{job_id, digest, items, approved_at_ms, revoked_at_ms}`를 더한다. 목록이 있는 `add`는 2번 판정상 이번 호출 승인 없이는 실행되지 않으므로, 기록은 사용자 승인으로만 생긴다.
   - 기록 파일은 048 비밀 파일 목록에 더한다. 그래서 file tool·명령의 읽기는 매번 묻고, 쓰기는 모든 mode에서 거절되며, 목록·검색에 나오지 않는다. 서명은 쓰지 않는다(결정과 이유 참고).
   - 쓰기는 같은 폴더의 임시 파일에 쓴 뒤 이름을 바꾼다.
4. **판 참조:** `enqueue_cron`은 그 작업의 유효한(취소되지 않은) 기록이 있으면 `approved_automation_rule_ref`를 `cron-job:<id>#<digest>`로 싣는다. 없으면 지금처럼 `cron-job:<id>`다. `normalized_request`는 `#` 뒤를 `AutomationRunSource::Cron.grant_digest`(`serde(default)`)로 읽는다. 판 참조가 없는 기존 automation 작업은 그대로 읽히고 목록이 없다. 저장한 작업이나 journal의 `permissions` 칸은 실행에 쓰이지 않는다(목록은 기록에서만 읽는다).
5. **판의 수명:** 기록은 일정 표와 따로 저장된다.
   - 저장 파일 하나: scheduler와 `cron` tool이 같은 작업 파일 `<data>/runtime/cron/jobs.json`(`cron_store_path`)을 쓰게 한다. 기록 파일은 그 옆 `<data>/runtime/cron/approvals.json`이다. 지금 `<data>/cron/jobs.json`에만 있는 작업은 옮기지 않는다(서버에는 없다).
   - 사용자 삭제(`PersistentCronService::remove_job`)는 삭제를 알리기 전에 그 작업의 기록에 `revoked_at_ms`를 적는다. 기록을 저장하지 못하면 삭제도 하지 않고 `RemoveJobResult::RevokeFailed`를 돌려준다.
   - `delete_after_run`의 자동 정리는 기록을 건드리지 않는다.
   - tool 호출을 들일 때마다(`permission_evaluation_for_action`) 기록 파일을 다시 읽어 `job_id`·`digest`가 맞고 취소되지 않은 기록이 있는지 본다. 그 결과(목록 또는 없음)를 `OutsideAccess.cron_grant`에 담아 tool 판정과 process gate가 같은 값을 쓴다. 기록 파일 자리는 `AgentLoopConfig.cron_grant_store`(CLI가 `runtime/cron/approvals.json`으로 채운다)로 넘기고 `ToolExecutionContext`에 직렬화하지 않고 싣는다.
   - 기록이 없거나 맞지 않거나 취소되었으면 목록은 아무것도 허락하지 않고, run은 cron 표시를 그대로 지닌다.
6. **경로 대조(`cron_list_allows`):** 비대화형 cron run의 workspace 밖 대상은 모두 유효한 목록에 맞아야 한다.
   - file tool: 요청 경로와 따라간 경로가 각각 허용 범위(workspace, media) 안이거나 같은 종류 이상의 경로 항목 안이어야 한다. 읽기 전용 tool은 읽기, 그 밖은 쓰기가 필요하다. 이를 위해 PRD 020의 `outside_target`이 요청 경로도 허용 범위 밖이고 따라간 경로와 다르면 둘 다 낸다(실행 때 허락된 밖 대상 확인은 따라간 경로로 하므로 바뀌지 않는다).
   - `exec`: `..` 표시가 있으면 허락하지 않는다. 각 대상은 읽기 전용으로 확인되면 읽기 항목, 아니면 쓰기 항목 안이어야 한다. 읽기 전용 확인: 048 명령 분석이 되고, 그 대상이 나오는 자리가 모두 읽기 전용 명령(`cat`, `head`, `tail`, `wc`, `ls`, `stat`, `file`, `grep`, `rg`, `du`, `diff`, `cmp`, `md5sum`, `sha1sum`, `sha256sum`, `realpath`, `readlink`)의 인자이거나 입력 리디렉션이고, 쓰기 자리(출력 리디렉션, `tee`, 쓰기 명령의 인자)에 나오지 않는 것이다. 분석에서 찾지 못한 대상, `working_dir`, 그 밖의 대상은 쓰기 항목이 필요하다.
   - 맞지 않으면 PRD 010의 `OutsideWorkspaceUnattended` 거절이다(PRD 000 거절 사건). 다른 판정(비밀 파일, 보호 경로, critical path, deny·ask 규칙, mode)은 그대로 따로 걸린다.
7. **명령 항목:** 유효한 목록이 있는 cron run의 판정에서, 명령 항목을 `exec(<pattern>)` 설정 allow 규칙으로 더한 판정 환경을 쓴다(`PermissionEnvironment::with_cron_grant`). 그래서 auto mode는 048 판정 표 12번, 다른 mode는 설정 allow 규칙 자리에서 같은 조건(나뉜 명령 모두 맞기, 분석할 수 없으면 안 됨, deny·ask 규칙과 비밀 파일·critical path·보호 경로는 앞에서 걸림)으로 쓰인다.
   - 리디렉션: 같은 환경에서 `target_allowed`가 경로 항목도 본다. 쓰기 항목은 출력 리디렉션·`tee` 대상을, 읽기·쓰기 항목은 입력 리디렉션 대상을 받는다. 요청 경로와 canonical 경로가 모두 항목 안이어야 한다. 이 대신하기는 리디렉션 확인에만 쓴다.
   - 명령 항목은 workspace 밖 경로를 허락하지 않는다(6번이 따로 본다).
8. **좁은 allow 규칙과 요약할 수 없는 명령:** auto가 아닌 mode에서 static 판정이 `ProcExecSummaryUnavailable`이고, 나뉜 명령 모두가 넓지 않은 `exec` allow 규칙(설정 allow 규칙 또는 7번의 명령 항목)에 맞고(판정 재료 `narrow_exec_allow`, 048 allow 대조를 넓은 규칙을 빼고 한 번 더 한 결과), action의 격리 snapshot이 격리됨(`contained: true`)이면, static ask를 채운 것으로 보고 allow 규칙 자리로 간다. 넓은 규칙, 기억된 허용(`ExecPrefix`)은 채우지 않는다. static 규칙은 요약을 격리보다 먼저 보므로 요약을 건너뛸 때 격리 조건을 따로 둔다. 격리가 확인되지 않으면 지금처럼 묻거나 거절한다. static deny, `plan`, deny·ask 규칙, 비밀 파일, critical path, 010 안전 장치의 남은 거절은 그대로다. tool 판정과 process gate가 같은 재료로 같은 판정을 낸다. auto mode는 바뀌지 않는다.
9. **권한 한도의 승인 참조:** 유효한 판 참조가 있는 cron run(`grant_digest`가 있고 기록이 유효)의 tool 문맥은 권한 한도 출처를 `CronWake { job_id, approval_ref: Some("cron-grant:<job_id>:<digest>") }`로 만든다. 판 참조가 없는 cron run은 지금처럼 `UserTurn` 한도다(PRD 010 그대로).
10. **문서:** `docs/USAGE.md`의 cron·권한 설명과 README에 목록, 질문, 기록, 삭제, 명령 항목, 좁은 allow 규칙 예외를 적는다.

## 결정과 이유

- **목록은 기록에서만 읽는다.** 일정 표와 journal은 어시스턴트나 다른 프로그램이 고칠 수 있는 평범한 data 파일이다. 실행 때 쓰는 목록을 승인 기록에만 두면, 일정 표의 목록 칸을 늘려도 쓰이지 않는다(SPEC 수용 조건 4.4).
- **기록은 비밀 파일로 막고 서명하지 않는다.** 서명은 키를 어딘가 저장해야 하고, 그 키 파일도 결국 비밀 파일로 막아야 한다. 048 비밀 파일 규칙은 file tool과 이름을 쓴 명령의 쓰기를 모든 mode에서 거절하므로 같은 보호를 바로 얻는다. 남는 위험(이름을 쓰지 않고 파일을 여는 프로그램)은 SPEC "남은 위험과 한계"에 이미 적혀 있다.
- **등록 질문은 판정 끝 한 자리에서 만든다.** 그래야 auto mode classifier, allow 규칙, 기억된 허용, `bypass_permissions`가 지나가지 못하고, 질문이 once only로 나온다.
- **판 참조는 envelope의 기존 문자열에 싣는다.** envelope 형식을 바꾸지 않으므로 이미 저장된 작업이 그대로 읽힌다.
- **취소는 삭제 전에 저장한다.** scheduler와 tool registry가 다른 instance이므로 메모리 표시로는 다른 instance의 다음 tool 호출을 막지 못한다. 매 호출이 기록 파일을 다시 읽으므로 삭제 뒤 둘째 호출부터 목록이 쓰이지 않는다.
- **읽기 전용 확인은 좁은 명령 이름 목록으로 한다.** 048 분석의 `reads`는 쓰기로 알아보지 못한 인자를 모두 담으므로 읽기 전용의 근거가 될 수 없다. 이름 목록은 쓰기 옵션이 없는 명령만 담는다(`sort -o`, `sed -i`, `find -delete`처럼 쓰기 옵션이 있는 명령은 넣지 않는다).
- **권한 한도는 판 참조가 있을 때만 `CronWake`로 만든다.** 판 참조 없는 cron run까지 `CronWake`로 바꾸면 `approval_ref`가 없어 지금의 cron run이 모두 거절된다.

## Non Scope

- 웹, 메시지 보내기, 하위 작업, 예약 같은 다른 권한의 미리 허락.
- 작업 수정 동작, 폴더 단위 기억, 목록 항목의 glob.
- heartbeat·결과 관찰 run의 목록(cron 목록을 쓰지 못한다).
- 서명된 승인 기록.

## Parent Requirement Mapping

- 053 소유 범위 3의 비대화형 run 줄 중 cron 목록, 소유 범위 5 전체(목록, 묻기, 승인 기록과 판 참조, 판의 수명과 취소, 읽기 전용 확인, 명령 항목, 리디렉션 대신하기, 좁은 allow 규칙 예외), 6 중 자기 변경과 그 문서.
- 독립 종료 조건: 수용 조건 4.1–4.6(4.3a 포함), 6.
- 앞선 스펙에서 바꾸는 것: 012 cron tool과 작업 저장 형식(목록 칸), 033 cron tool의 작업 파일 자리(scheduler와 같은 `<data>/runtime/cron/jobs.json`으로), 022 static ask의 `ProcExecSummaryUnavailable` 처리(좁은 allow 규칙), 022 권한 한도(판 참조가 있는 cron run), 033 automation 출처(판 참조), 048 비밀 파일 목록과 리디렉션 조건(cron 실행 동안). 앞선 스펙 문서는 고치지 않는다.

## Acceptance Criteria

1. 등록(4.1): 목록이 있는 `add`는 auto(classifier 허용), `bypass_permissions`, `dont_ask`, `default`의 대화형 run에서 목록 전부를 적은 once only 질문을 내고, 설정 allow 규칙·기억된 허용이 있어도 묻는다. 비대화형 run에서는 거절된다. 승인하면 작업과 기록이 생기고 `list`에 목록이 보이며, 거절하면 작업이 없다. 비밀 파일을 가리키는 항목, 보호 경로 쓰기 항목, `python3 *` 같은 넓은 명령 pattern은 등록이 거절되고, home 읽기 항목은 받는다.
2. 경로 항목(4.2): production worker의 auto mode cron run에서 classifier 허용일 때 목록 안 file tool 읽기·쓰기가 실행된다. 목록 밖, 읽기 항목 안의 쓰기, `mkdir <읽기 항목 안의 경로>`, symlink를 따라가면 목록 밖인 대상, `../`가 든 명령은 `bypass_permissions`에서도 거절된다. 다른 작업의 목록, heartbeat run은 목록을 쓰지 못한다.
3. 명령 항목(4.3): `default` mode production cron run에서 명령 항목에 맞는 스크립트 `exec`가 묻지 않고 실행된다. 맞지 않는 명령, 이은 명령 중 하나가 맞지 않는 명령, 분석할 수 없는 명령은 거절된다. workspace 밖 리디렉션 대상이 쓰기 항목에 없으면 거절된다. deny 규칙과 010 deny pattern에 맞는 명령은 명령 항목으로 허용되지 않는다. 같은 세션의 대화형 turn은 명령 항목을 쓰지 못한다.
4. 요약할 수 없는 명령(4.3a): 대화형 `default` mode에서 설정 allow 규칙 `exec(python3 <고정 경로> *)`에 맞는 명령은 묻지 않고 실행되고, `python3 *`나 기억된 허용만 있으면 지금처럼 묻는다. `ContainmentUnknown`은 규칙이 있어도 묻거나 거절된다. process gate가 같은 판정을 낸다.
5. 승인 증거(4.4): `jobs.json`이나 journal의 목록을 늘려도 늘린 항목은 쓰이지 않는다.
6. 수명(4.5): 삭제 → 다시 연 서비스 → 대기열의 실행은 목록을 쓰지 못한다. 다른 `PersistentCronService` instance로 두 tool 호출 사이에 삭제하면 둘째 호출부터 목록을 쓰지 못한다. `delete_after_run` 자동 정리 뒤의 실행은 목록을 쓴다.
7. 빈 목록과 이전 기록(4.6): 목록이 없거나 맞지 않는 cron run은 `bypass_permissions`에서도 workspace 밖 접근이 거절된다. 목록 칸 없는 기존 작업·journal, 판 참조 없는 기존 automation 작업이 읽히고 빈 목록이다.
8. 권한 한도: 판 참조가 있는 cron run의 tool 판정이 `CronWake` 한도와 `approval_ref`로 통과하고, `approval_ref`가 없는 `CronWake` 한도는 지금처럼 거절된다.
9. fmt(Google 옵션), workspace clippy `-D warnings`, 변경한 package(`shacs-cron`, `shacs-core`, `shacs-cli`) 테스트가 통과한다.

## 구현 메모 및 검증 근거

- `shacs-cron/src/grants.rs`(새): `CronPermission`(경로 항목·명령 항목), `CronAccess`, `CronGrant`, `normalize_permissions`(`~` 풀기, lexical 정규화, 상대 경로·`..` 거절, 정렬·중복 제거), `permissions_digest`(SHA-256), `record_grant`·`revoke_grants`·`valid_grant`·`current_grant`(파일을 매번 읽고, 임시 파일에 쓴 뒤 이름 바꾸기). `CronJob.permissions`·`AddCronJob.permissions`(빈 목록이면 저장하지 않고, 없으면 빈 목록으로 읽음). `CronService::grants_path`, `PersistentCronService::remove_job`은 삭제 전에 취소를 저장하고 실패하면 `RemoveJobResult::RevokeFailed`.
- `tools/cron.rs`: `add`의 `permissions` 인자와 `validate_params`(형식, 정규화, 넓은 명령 pattern), 실행 뒤 승인 기록 남기기(실패하면 작업을 지우고 오류), `list`의 `Permissions:` 줄, `remove`의 `RevokeFailed` 글.
- `runtime/cron_grant.rs`(새): `current_grant`(출처의 `job_id`·`grant_digest`로 기록을 지금 읽음), `covers`(file tool은 tool 종류로 read·write, `exec`는 `..` 거절과 읽기 전용 확인), `read_only_target`(048 명령 분석의 쓰기 자리에 없고, 읽기 전용 명령 이름의 인자나 입력 리디렉션으로만 나옴), `cron_command_problem`, `add_request`(비밀 파일·보호 경로 쓰기 거절과 질문 글), `ceiling_for_source`.
- `tool_execution.rs` `permission_evaluation_for_action`: 유효한 기록이 있으면 `PermissionEnvironment::with_cron_grant`(명령 항목을 `exec(...)` allow 규칙으로, 경로 항목을 리디렉션 확인용 `cron_paths`로)를 쓴 문맥으로 판정한다. `OutsideAccess.cron_list_covers`·`cron_list_request`를 채워 tool 판정과 process gate가 같은 값을 쓴다. 질문 글은 `shown_cron_list`(`.env.json` 값 가림).
- `outside_access.rs`: `cron_list_allows`가 `cron_list_covers`를 읽는다. `decide_cron_list`가 판정 끝에서 목록 있는 `add`를 묻기(`CronPermissionList`, once only)·거절(`CronPermissionListRefused`)로 바꾼다. `dont_ask`의 기본 거절만 묻기로 바꾸고 다른 거절은 그대로다.
- `permission_materials.rs`: `narrow_exec_allow`(넓은 규칙을 뺀 allow 대조가 모든 나뉜 명령에 맞음), `target_allowed`의 `cron_path_covers`(요청 경로와 실제 경로 모두). `permission_policy.rs` `narrow_allow_fills_summary`가 auto가 아닌 mode의 `ProcExecSummaryUnavailable` static ask를 격리된 실행에서만 채운다.
- `automation_run_source.rs`: `Cron.grant_digest`, `cron-job:<id>#<digest>` 읽기. CLI `automation_producer::cron_rule_ref`가 유효한 기록의 digest를 싣는다. `AgentLoopConfig.cron_grant_store`(CLI가 `<data>/runtime/cron/approvals.json`), 하위 작업 문맥은 `None`. `AgentLoop`가 `ceiling_for_source`로 판 참조 cron run의 한도를 `CronWake { approval_ref }`로 만든다.
- 048 비밀 파일: `<data>/runtime/cron/approvals.json`(과 임시 파일)을 더했다(`secret_file.rs`).
- 저장 파일 하나: 구현 중 `cron` tool(`<data>/cron/jobs.json`)과 scheduler(`<data>/runtime/cron/jobs.json`)가 다른 파일을 쓰는 것을 찾았다(2026-11-17 서버에서도 `cron/`은 비어 있고 `runtime/cron/jobs.json`만 있음). `cron_store_path` 하나로 맞췄다.
- 시험: `shacs-cron` grants 단위 시험(정규화와 digest, 기록·취소·digest 대조, 다른 instance의 사용자 삭제는 취소하고 한 번만 도는 작업의 자동 정리는 취소하지 않음, 목록 칸 없는 저장 형식 읽기), `tests/it/spec053_cron_list.rs`(모든 대화형 mode의 once only 질문과 목록 내용, 비대화형 등록 거절, 승인·거절 결과와 기록·`list`, 비밀 파일·보호 경로 쓰기·넓은 pattern·상대 경로 거절, 경로 항목의 종류별 사용과 목록 밖·읽기 항목 안 쓰기·symlink·`mkdir`·`../` 거절(auto, bypass), 다른 작업·heartbeat·늘린 목록, 다른 instance 삭제 뒤 다음 호출 거절, `default` cron run의 명령 항목 실행·쓰기 항목 리디렉션과 맞지 않는 명령·이은 명령·분석 불가·목록 밖 리디렉션·deny pattern 거절, 대화 turn은 쓰지 못함, 좁은 설정 allow 규칙의 요약 불가 예외와 넓은 규칙·격리 없음, auto mode 리디렉션 대신하기, 기록 파일의 비밀 파일 처리), `cron_grant` 한도 단위 시험, CLI worker 시험(producer의 판 참조 → worker → 목록 안 쓰기 실행, 다른 instance 삭제 뒤 거절). 등록 묻기·취소 저장·좁은 규칙 예외·리디렉션 대신하기·읽기 전용 확인을 하나씩 끄면 각각 시험이 실패한다.
- 독립 검토(REQUEST_CHANGES) 뒤 고친 것:
  - 기록 파일의 hard link: `secret_file.rs` hard link 검사가 `runtime/cron/approvals.json`과 임시 파일의 파일 identity도 본다.
  - workspace 밖 `working_dir`: 명령의 상대 경로가 그 폴더에서 보이지 않게 풀리므로, 그 폴더는 읽기 전용으로 보지 않고 늘 쓰기 항목을 요구한다.
  - 동시 저장: `record_grant`·`revoke_grants`가 `approvals.json.lock`에 OS 파일 잠금(`File::lock`)을 잡고 읽기부터 저장까지 한다. 다른 instance·process 사이에도 걸린다.
  - 판정 뒤 취소: 판정이 기록에 기댔으면 `ToolCallExecutionContext`에 (기록 파일, 작업 id, digest)를 싣고, tool이 실제로 시작하기 직전(단일 실행과 묶음 실행 모두, deferred 포함)에 기록을 다시 읽어 유효하지 않으면 실행하지 않고 비대화형 밖 접근 거절 글을 돌려준다. process gate는 tool이 시작할 때만 닿으므로 같은 결과를 따른다.
  - 요청 경로: `outside_target`이 요청 경로도 허용 범위 밖이고 따라간 경로와 다르면 둘 다 내어, 목록 대조가 둘 다 본다.
  - `bypass_permissions` 등록: 판정이 `ModeBaselineAllow`로 끝나도 이번 호출 승인(`PermissionMaterials.this_call_approved`)이 있으면 등록이 완료된다.
  - 권한 한도: `ceiling_for_source`가 기록 파일을 읽어 유효한 기록일 때만 `CronWake { approval_ref }`를 만든다. 없는 기록·다른 digest·취소된 기록은 지금 한도 그대로다.
  - 시험: bypass 승인 뒤 등록, `working_dir` 쓰기 요구, 목록 밖 요청 경로, 기록 파일 hard link, 저장한 작업 파일의 목록을 늘려도 쓰이지 않음(실제 `jobs.json` 수정), 같은 dispatch에서 앞 묶음 실행 중 취소된 뒤 쓰기가 시작되지 않음, 동시 등록·취소에서 취소가 사라지지 않음(잠금을 빼면 5번 모두 실패), 한도의 유효 기록 조건, CLI `enqueue_cron` → 대기열 → `process_due_automation` → worker → `AgentLoop` 실제 경로에서 목록 사용과 삭제 뒤 거절(producer가 판 참조를 빼면 실패). 고침 다섯 개를 하나씩 끄면 각각 시험이 실패한다.
  - CLI 경로 시험은 `bypass_permissions`와 MCP꼴 writer를 쓴다. auto mode의 classifier 허용과 `default` mode 명령 항목 실행은 core 판정 시험(`spec053_cron_list.rs`)이 같은 판정 코드로 확인한다. CLI의 실제 `exec`는 격리 증명이 있는 공식 container에서만 시작되어 시험 기계에서 돌릴 수 없다.
- 재검토(REQUEST_CHANGES) 뒤: 작업 폴더를 `..`를 먼저 정리해 구해, `working_dir: link/..`처럼 링크를 따라 밖으로 가는 폴더를 workspace로 보았다. 이제 실행처럼 적힌 경로(`join_word_as_written`)를 canonicalize해 작업 폴더로 쓰고, 명령 분석도 그 폴더를 기준으로 한다. 시험 `a_working_folder_reached_through_a_link_and_dotdot_still_needs_a_write_entry`는 고침을 되돌리면 실패한다.
- 독립 검토: 첫 검토 REQUEST_CHANGES(위 8건), 재검토 REQUEST_CHANGES(`working_dir`의 `link/..`), 세 번째 검토 APPROVE(2026-11-17, 앞선 지적 모두 포함).
- 최종 검증 로그: `/tmp/spec053-prd030-approved-tests.log`(최종 코드).
- `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true`: exit 0.
- `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`: exit 0. 구현 중 시험 코드의 `mut`·복잡한 type 지적을 고쳤다.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-core -q`: exit 0, 79개 결과 묶음에서 2089 passed, 0 failed, 5 ignored.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q`: exit 0, 20개 결과 묶음에서 720 passed, 0 failed, 3 ignored.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-cron -q`: exit 0, 2개 결과 묶음에서 16 passed, 0 failed, 0 ignored.
- 앞선 검증 두 번은 검토 지적으로 코드를 고치게 되어 끝나기 전에 멈췄다. 통과로 기록하지 않는다.
