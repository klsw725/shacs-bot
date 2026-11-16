# 053. workspace 밖 접근 묻기 구현 명세

Status: Planned (PRD 000 구현·독립 검토 승인 2026-11-16, 커밋·전체 workspace 시험 전)

Consumes: 004 tool 실행과 runner의 tool 결과 처리(`fatal_tool_message`, `tool_outcome_for_message`, 한 dispatch의 호출 순서), 004·010 file tool이 다룰 수 있는 폴더(`PathContext`, `allowed_roots`: workspace, media 폴더, `extra_allowed_dirs`)와 symlink 검사, 010 `exec` 안전 장치(deny pattern, allow pattern, 내부·사설 주소, workspace guard, `resolve_working_dir`), 011 하위 작업과 workflow 자식의 `restrict_to_workspace`·`fail_on_tool_error`, 012 run의 대화형 여부와 cron 작업 저장·실행(`shacs-cron`의 `CronJob`, `cron` tool, 실행 출처 `CronWake { job_id }`), 013 승인 질문, 020 deferred bridge 호출, 022 permission mode·`fs_read` 자동 승인 기준(workspace 밖 host path와 symlink escape 대상은 자동 승인하지 않는다)·기억된 규칙과 matcher·승인 correlation, 022 PRD 005 runtime 경계의 상속 권한 한도(cron의 `approval_ref`), 030 process gate와 sandbox adapter, 031 config의 `tools.restrictToWorkspace`, 033 production 자동화 경로(cron 실행이 durable automation 작업을 넣고 automation worker가 `AgentLoop`를 부르는 것), 037 PRD 040 turn을 멈춘 도구 결과 가리기(`quiet_failure`), 048 판정 순서(PRD 000 판정 표와 다른 mode의 더하기)·사용자 규칙·비밀 파일 규칙·보호 경로·명령 분석·classifier·막힘 수

## 목적과 완료 경계

지금 어시스턴트가 workspace 밖 경로를 쓰면 tool이 그 자리에서 거절하고, 그 거절이 turn을 끝낸다.

- `exec`: 010 안전 장치가 `restrictToWorkspace`가 켜졌거나 **sandbox가 없을 때** workspace guard를 켠다(`crates/shacs-core/src/tools/shell/guard.rs`의 `workspace_guard_enabled`). 공식 Docker 경로에는 쓸 수 있는 sandbox가 없으므로(아래) `restrictToWorkspace: false`로 둔 사용자도 늘 guard 아래에 있다. guard는 명령 글에서 `../`를 찾거나, 절대 경로·`~` 경로가 그 명령의 working directory와 media 폴더 밖이면 승인과 상관없이 거절한다. `working_dir` 인자가 workspace 밖이어도 거절한다(`resolve_working_dir`).
- file tool: `read_file`·`write_file`·`edit_file`·목록·검색은 `restrictToWorkspace`와 상관없이 workspace, media 폴더, `extra_allowed_dirs` 밖을 다루지 못한다(`Path … is outside allowed directory …`, `crates/shacs-core/src/tools/filesystem.rs`). 본 대화의 production 등록은 `allowed_dir`를 늘 workspace로 둔다(`crates/shacs-cli/src/spec035_cli_media/tool_registry.rs`).
- runner는 tool 결과 글에 workspace 위반 문구(`blocked by safety guard`, `path outside working dir`, `outside allowed directory` 등, `is_workspace_violation`)가 있으면 그 결과를 fatal로 보고 run을 `tool_error`로 끝낸다(`crates/shacs-core/src/runtime/runner.rs`의 `fatal_tool_message`). 이미지·복합 요청이 아니면 거절 글이 그대로 어시스턴트의 마지막 답이 된다.
- 048 auto mode는 tool이 스스로 거절할 호출을 묻거나 classifier에 보내지 않고 거절한다(048 소유 범위 6, PRD 000 판정 표 5번). 그래서 auto mode에서도 이 거절은 그대로다.

2026-11-15 Discord(auto mode, `restrictToWorkspace: false`, `tools.exec.sandbox: ""`)에서 유튜브 요약 요청이 자막을 받아 온 뒤 `… > /tmp/u0RtU4u8Pw0.txt; wc -lc /tmp/u0RtU4u8Pw0.txt`에서 `Error executing command: Error: Command blocked by safety guard (path outside working dir)`로 막혔고, 이 글이 사용자에게 간 답 전부였다. 받아 둔 자막은 쓰이지 않았다.

sandbox를 켜서 guard를 끄는 길은 공식 Docker 경로에서 쓸 수 없다. 2026-11-15 같은 서버에서 확인했다.

- 지원하는 sandbox는 `bwrap` 하나다(`crates/shacs-core/src/tools/shell/execution.rs`). gateway image에는 `bwrap`이 없다.
- gateway container 안에서 `unshare -Ur true`가 `Operation not permitted`로 실패한다(기본 seccomp, `privileged` 없음). `bwrap`을 넣어도 준비 확인(`probe_policy`)이 실패한다. 이를 풀려면 container 격리를 낮춰야 한다(023이 막는 것).
- `sandbox: "bwrap"`이면 준비 확인이 실패해 기본 fallback(`SandboxRequired`)에서는 모든 명령이 거절되고, `TrustedNativeFallback`에서는 sandbox 없이 실행되면서 guard도 꺼진다(`crates/shacs-core/src/runtime/sandbox_adapter/execute.rs`의 `fallback_or_reject`). guard가 sandbox가 실제로 돌았는지가 아니라 설정에 이름이 있는지만 보기 때문이다.

Claude Code와 OpenCode는 workspace 밖 경로를 거절로 끝내지 않고 묻는다. Claude Code는 명령과 file tool이 working directory 밖을 읽거나 쓰면 승인 질문(`ask`)으로 보내고(`docs/refs/claude-code/src/tools/BashTool/pathValidation.ts`의 `checkPathConstraints`, `src/utils/permissions/filesystem.ts`), auto mode에서는 그 판단을 classifier가 맡는다. OpenCode는 bash 명령이 프로젝트 밖 폴더를 건드리면 `external_directory` 권한(기본 `ask`)으로 묻는다(`docs/refs/opencode/packages/opencode/src/tool/bash.ts`). 010 안전 장치를 가져온 nanobot은 workspace guard를 `restrict_to_workspace`일 때만 켜고, 거절을 고칠 수 있는 tool 오류로 돌려주며 우회하지 말라는 안내를 붙이고, 같은 대상의 거절이 거듭되면 더 강한 거절 글을 돌려준다(turn은 끝내지 않는다. `docs/refs/nanobot/nanobot/agent/runner.py`, `utils/runtime.py`의 `repeated_workspace_violation_error`).

이 스펙은 workspace 밖 접근을 Claude Code와 같게 묻는 쪽으로 바꾼다(사용자 결정 2026-11-15: 명령과 file tool 모두, `restrictToWorkspace: true`는 지금처럼 거절, data 폴더의 workspace 밖 부분을 따로 막지 않음).

완료되면 사용자는 다음을 경험한다.

- `restrictToWorkspace`를 켜지 않았으면, 어시스턴트가 명령이나 file tool로 workspace 밖을 읽거나 쓸 때 auto mode에서는 classifier가 판단하고, 다른 mode에서는 사용자에게 묻는다. 직접 적은 allow 규칙이나 기억된 허용이 있으면 그대로 쓰인다. 위 유튜브 흐름은 승인 질문 없이 `/tmp`에 쓰고 요약까지 끝난다(classifier가 허용할 때).
- `restrictToWorkspace: true`면 지금처럼 workspace 밖은 거절된다.
- tool이 거절해도 turn이 끝나지 않는다. 어시스턴트가 거절 이유와 "우회하지 말고 다른 방법을 찾거나 사용자에게 물어라"는 안내를 받고 이어서 일한다. 같은 거절이 한 요청 안에서 세 번째일 때만 멈춘다.
- 인증 파일과 `.env.json` 같은 비밀 파일, 보호 경로, 사용자 deny·ask 규칙, critical path 제거 질문은 그대로 걸린다. workspace 밖에 있어 지금은 거절되던 data 폴더의 비밀 파일 읽기도 이제 매번 묻는다.
- cron처럼 도중에 물을 수 없는 실행은 미리 허락받은 만큼만 한다. cron 작업을 만들 때 어시스턴트가 그 작업이 쓸 workspace 밖 경로와 실행할 명령을 목록으로 내고, 사용자가 그 자리에서 한 번에 승인하면 그 작업에만 묶인다. 실행 때는 목록 안만 허용되고 목록 밖은 거절된다. 사용자가 작업을 지우면 그 허락은 바로 사라진다(사용자 결정 2026-11-15, 2026-11-16).

이 스펙은 다른 스펙의 구현을 기다리지 않는다.

## 소유 범위

1. **workspace 밖 대상:**
   - 판정에 쓰는 범위(이하 "허용 범위")는 file tool이 지금 쓰는 범위와 같다: workspace, media 폴더, 그 run의 `extra_allowed_dirs`. `exec`도 같은 범위를 쓴다(`extra_allowed_dirs`는 `exec`에 지금 없으므로 workspace와 media 폴더다).
   - 대상이 workspace 밖이라는 것은 요청한 경로나 symlink를 따라간 경로가 허용 범위 밖이라는 뜻이다. `exec`의 대상은 010 guard가 명령 글에서 찾는 꼴(절대 경로, `~` 경로, `../`가 든 글)과 `working_dir` 인자다. 경로를 찾는 함수(`extract_absolute_paths`, `expand_shell_path`, `canonicalize_existing_prefix`)는 지금 것을 쓰고 경로를 찾는 새 명령 분석을 더하지 않는다. 찾은 경로를 읽기 전용으로 볼 수 있는지 가르는 일은 cron 목록 대조에만 쓰고 소유 범위 5가 정한다. 다만 판정은 그 명령의 working directory가 아니라 허용 범위와 비교한다. `../`가 든 명령은 그 꼴만으로 workspace 밖 대상으로 본다.
   - `restrictToWorkspace: true`인 run, 그리고 지금 그 값을 늘 켜는 run(011 하위 작업, worktree를 쓰는 workflow 자식)에서는 지금 guard와 file tool 검사를 그대로 쓴다(working directory 기준 비교와 `working_dir` 검사 포함). 거절 뒤 처리만 소유 범위 4를 따른다.
   - 그 밖의 run에서는 tool이 workspace 밖이라는 이유로 거절하지 않고, 판정이 그 호출을 workspace 밖 접근으로 본다(소유 범위 3). 이 사실은 판정과 process gate가 함께 쓰는 판정 재료(PRD 000의 tool 실행 전 확인과 같은 자리)로 넘겨, tool 실행 판정과 process gate가 같은 판정을 낸다.
2. **그대로 남는 tool 거절:** workspace 밖이라는 이유의 거절만 없앤다. 아래는 `restrictToWorkspace`와 상관없이 지금처럼 tool이 거절하고, 048 PRD 000 판정 표 5번(tool이 실행 전에 거절할 호출)으로 남는다.
   - `exec`: deny pattern(`rm -r`·`rm -f`·`dd if=` 등), 설정한 allow pattern에 맞지 않는 명령, 내부·사설 주소. sandbox 설정 유무는 더 이상 workspace guard를 켜거나 끄지 않는다.
   - file tool: 쓰는 경로에 이미 있는 symlink 구성 요소가 있는 쓰기, learned-method store 쓰기, 목록·검색이 허용 범위 밖으로 나가는 symlink를 따라가지 않는 것. 048의 symlink 쓰기 질문(PRD 000 판정 표 11번, 13번)은 이 검사들을 지난 호출에만 걸린다.
   - 030 process gate(격리 증명이 없으면 시작하지 않는다)와 sandbox mount. 승인은 실행 중인 sandbox의 쓰기 가능 mount를 넓히지 않는다.
3. **판정:** workspace 밖 접근은 아래 순서를 따른다. 여기 적지 않은 줄과 순서는 048 PRD 000 그대로다.
   - auto mode(048 PRD 000 판정 표):
     - 1–13번(정규화 오류, 비밀 파일 쓰기, deny 규칙과 기억된 거절, 5번의 남은 tool 거절, 이번 호출에 대한 승인, 비밀 파일 읽기, ask 규칙, critical path, symlink 쓰기 질문, allow 규칙과 기억된 허용)은 그대로 먼저 걸린다. 12번의 allow 규칙·기억된 허용은 지금 조건(넓은 규칙 아님, 보호 경로 쓰기 아님, `exec` 리디렉션 대상 조건)을 그대로 지켜야 쓰인다.
     - 14번(읽기 바로 허용)과 15번(workspace 쓰기 바로 허용)에 workspace 밖 대상이 있는 호출은 들지 않는다.
     - 나머지는 16번 classifier가 본다. 허용이면 실행하고, 막음·판정 없음은 048 PRD 020대로 실행하지 않고 이어간다.
   - auto가 아닌 mode(048 PRD 000 범위 3 "다른 mode의 판정"에 한 줄을 더한다):
     - 022 판정과 048이 더한 규칙 뒤에, 결과가 허용이고 workspace 밖 대상이 있으면 묻기로 바꾼다. 단, 이번 호출에 대한 승인, 설정 allow 규칙, 기억된 허용이 그 호출을 허용했으면 바꾸지 않는다. 022 `fs_read` 자동 승인 기준이 이미 "workspace 밖 host path와 symlink escape 대상은 자동 승인하지 않는다"고 정하므로, 이 줄은 그 기준을 `default`의 읽기 자동 허용과 `accept_edits`의 편집 자동 허용에도 똑같이 적용하는 것이다.
     - 거절과 묻기는 그대로다. `plan`이 쓰기와 명령 실행을 막는 것도 그대로다.
     - `bypass_permissions`에서는 바꾸지 않는다(실행한다).
     - `dont_ask`의 대화형 run에서는 이 줄이 낸 묻기만 거절이 된다. 설정 allow 규칙과 기억된 허용으로 허용된 호출은 그대로 실행된다.
   - 비대화형 run(`permission_interactive`가 꺼진 run: cron, heartbeat, local API, 위임 작업 관찰): mode와 상관없이 workspace 밖 대상은 아래일 때만 그 조건을 지난다. 나머지는 거절한다.
     - cron run(소유 범위 5의 믿을 수 있는 cron 실행 표시가 있는 run)은 `bypass_permissions`를 포함해 그 작업의 승인된 목록에 맞을 때만. 목록이 비었거나 맞지 않으면 `bypass_permissions`에서도 거절한다.
     - 그 밖의 비대화형 run은 `bypass_permissions`일 때만.
     - 설정 allow 규칙, 기억된 허용, classifier는 비대화형 run의 workspace 밖 접근을 허락하지 못한다.
     - 이 조건을 지나도 다른 판정(명령 실행 승인, process gate, 1–13번의 deny·비밀 파일·critical path 등)은 그대로 따로 걸린다. 비밀 파일 읽기와 critical path는 물을 수 없으니 거절된다.
     - 하위 작업과 worktree workflow 자식은 소유 범위 1대로 늘 workspace 밖을 거절한다. cron run이 시작한 하위 작업도 부모의 목록을 물려받지 않는다.
   - 질문은 기존 승인 질문을 쓰고, 대상이 workspace 밖이라는 것과 그 경로를 적는다. 기억 선택지는 022의 기억된 규칙을 그대로 쓴다. workspace 밖 file tool 대상은 지금 matcher가 workspace 기준이라 정확한 action(`ExactAction`)으로 기억된다. `exec`의 기억(`ExecPrefix`)은 지금 그대로이고 048의 allow 조건을 따른다(`crates/shacs-config/src/remembered_permissions/matcher.rs`, `crates/shacs-core/src/runtime/permission_remembered.rs`). 폴더 단위 기억(Claude Code의 "이 세션 동안 이 폴더 읽기 허용")은 053에 넣지 않는다. 048이 기억하지 못하게 한 질문(비밀 파일 읽기, critical path)과 auto mode의 넓은 규칙 정지는 그대로다.
   - 비밀 파일 규칙(048 소유 범위 3)은 workspace 밖에서도 그대로다. 지금은 file tool이 거절하던 data 폴더의 인증 저장소와 `.env.json` 읽기는 모든 mode에서 매번 묻고(묻지 못하면 거절), 쓰기는 거절한다. 목록·검색에서 비밀 파일을 빼는 것도 허용 범위 밖 폴더를 훑을 때 그대로다.
   - 보호 경로(048 소유 범위 4)의 쓰기는 workspace 밖에서도 auto mode에서 classifier가 보고, allow 규칙과 기억된 허용으로 허락되지 않는다.
   - 허용하거나 승인한 호출을 tool이 실행할 때 workspace 밖이라는 이유로 다시 거절하지 않는다. 판정과 실행 사이에 파일이 바뀌어 대상이 달라진 경우는 048 PRD 000의 기존 처리를 따른다.
4. **tool 거절 뒤 계속하기:**
   - 대상: tool이 돌려준 거절 결과(지금 `is_workspace_violation`이 찾는 문구가 든 결과: `exec` 안전 장치, `restrictToWorkspace`의 workspace guard, file tool의 범위 밖), auto mode가 판정 표 5번으로 미리 거절한 호출(`ToolRefusal`, `crates/shacs-core/src/runtime/tool_execution.rs`), 소유 범위 3의 비대화형 run 조건이 낸 workspace 밖 거절(cron 목록 불일치 포함)이다. PRD 000이 거절 사건을 알리는 자리를 만들고, 010과 030은 자기 거절 종류를 그 자리에 더한다. 048의 classifier 막힘·판정 없음은 대상이 아니다(048 PRD 020의 막힘 수가 맡는다).
   - 이 결과는 run을 멈추지 않는다. 실패한 tool 결과(`Failed { class: Recoverable }`)로 어시스턴트에게 돌아간다.
   - 결과 글에 거절 이유와 함께 안내를 붙인다: 이 거절은 정책 경계라서 다시 해도 같다, symlink·인코딩·다른 tool·`working_dir` 바꾸기로 우회하지 말라, workspace 안으로 할 수 있으면 그렇게 하고 아니면 사용자에게 할 수 없다고 말하고 어떻게 할지 물어라. `restrictToWorkspace`의 거절이면 사용자가 그 설정을 끌 수 있다는 것을, 내부 주소면 `tools.ssrfWhitelist`를 알린다. 정확한 글은 PRD 000이 정한다.
   - **반복 한도:**
     - 거절 사건은 위 대상의 tool 결과 하나다. 결과 하나는 한 번만 센다(같은 결과를 여러 번 분류해도 다시 세지 않는다). 그 결과가 고칠 수 있는 실패인지 run을 멈추는지는 셀 때 한 번 정하고 바꾸지 않는다.
     - 세는 범위는 시작 하나(root invocation)다: 사용자 요청 하나(사용자 메시지로 시작한 run과 그 run이 낸 승인 질문에 답해 이어지는 run), cron 한 번 울림, heartbeat 한 번, local API 요청 하나, 위임 작업 관찰 한 번. 그 시작을 이어가는 run은 셈을 이어받고, 새 시작은 0부터다. 같은 세션을 함께 쓰는 서로 다른 시작은 따로 센다. 정확한 저장 자리는 PRD 000이 정한다.
     - 같은 거절은 실제 tool(deferred bridge 호출이면 bridge가 부른 tool), 거절 종류(workspace 밖, deny pattern, allow pattern 불일치, 내부 주소, file tool 범위 밖 등), 대상이 같은 것이다. 대상은 경로가 있으면 정규화한 경로, 없으면(deny pattern, 내부 주소) 거절 종류만으로 본다. 정확한 정규화는 PRD 000이 정한다.
     - 같은 거절의 세 번째 사건에서 run을 `tool_error`로 멈춘다. 같은 dispatch에서 그 뒤에 있는 호출은 시작하지 않는다. 예외는 하나다: 동시에 실행하는 묶음(읽기 전용 tool만 함께 묶인다, `crates/shacs-core/src/tools/base.rs`)에서 실행 전 확인을 지난 호출이 실행 중에 처음 거절되면, 그 묶음의 다른 읽기 전용 호출은 이미 시작했을 수 있다. 실행 전에 알 수 있는 거절이 있는 묶음은 순서대로 실행하므로 이 예외에 들지 않는다. 쓰기와 명령은 이 예외로 시작되지 않는다.
     - 이때 사용자에게 가는 답과 기록은 지금과 같다: 이미지·복합 요청은 037 PRD 040의 가리기를 따르고, 그 밖의 요청은 지금처럼 그 거절 결과가 답이 된다.
   - `fail_on_tool_error` run(하위 작업 등)은 지금처럼 첫 tool 실패에서 멈춘다.
   - 048이 실행하지 않은 호출(classifier 막힘)의 처리는 그대로다(048 PRD 020).
5. **cron 작업의 미리 허락된 목록:**
   - **목록:** `cron` tool의 `add`에 그 작업이 쓸 권한 목록을 더한다. 항목은 두 가지다.
     - 경로 항목: 경로(파일, 또는 그 아래 전부를 뜻하는 폴더)와 종류(읽기, 쓰기. 쓰기는 읽기를 포함한다). workspace 밖 접근을 허락한다.
     - 명령 항목: 048 사용자 규칙의 명령 pattern(`exec(<pattern>)`과 같은 문법, 예: `python3 /home/shacs/.shacs-bot/workspace/builtin_skills/youtube-content/scripts/fetch_transcript.py *`). 그 명령의 실행을 허락한다. 048이 auto mode에서 쓰지 않는 넓은 규칙(048 소유 범위 2: `exec` 전체, `python3 *` 같은 셸·스크립트 실행기 wildcard, `bash -c *`, `sudo *`·`env *`, `npx *`·`npm run *` 등)에 해당하는 pattern은 받지 않는다.
     - 인자 이름과 형식은 PRD 030이 정한다. `cron` tool에 수정 동작은 더하지 않는다. 목록·일정·지시를 바꾸려면 작업을 지우고 다시 만든다(다시 만들 때 다시 묻는다).
   - **묻기:** 목록이 비어 있지 않은 `add`는 모든 mode에서(`bypass_permissions`와 `dont_ask` 포함) 대화형 run이면 사용자에게 묻는다. classifier, allow 규칙, 기억된 허용이 대신 승인하지 못하고, 질문은 한 번 승인과 한 번 거절만 받는다(048 critical path 질문과 같은 꼴). 비대화형 run에서는 거절한다. 질문에는 작업 이름, 일정, 지시, 목록의 모든 항목(경로 항목과 명령 항목)을 적는다. 거절하면 작업이 만들어지지 않는다. 목록이 없는 `add`는 지금 판정(048의 예약) 그대로다.
   - **목록이 허락하지 못하는 것:** 목록은 비밀 파일 접근, 보호 경로 쓰기, critical path 제거를 허락하지 못한다. 항목이 비밀 파일이나 보호 경로를 직접 가리키는 쓰기이면 등록을 거절한다. 그 밖의 폴더 항목은 그 아래에 비밀 파일이나 보호 경로가 있어도 받되, 실행 때 접근마다 그 규칙을 다시 건다. home이나 data 폴더를 읽기 항목으로 적는 것은 제거 요청이 아니므로 critical path로 거절하지 않는다.
   - **승인의 증거:** 실행 때 쓰는 목록은 사용자가 승인한 그 목록이어야 한다. 그래서 승인은 작업 id와 정규화한 목록의 digest에 묶인 승인 기록으로 남기고, 실행 때 기록과 맞는 목록만 쓴다. 저장한 작업(`runtime/cron/jobs.json`)이나 action journal의 목록 칸만으로는 승인이 아니다. 어시스턴트의 tool 호출(file tool, 명령)은 승인 기록을 만들거나 넓히지 못한다. 기록을 비밀 파일로 다룰지, runtime만 아는 키로 서명할지는 PRD 030이 정한다(비밀 파일로 다루면 048 비밀 파일 목록에 더한다). data 폴더의 다른 파일에 대한 접근은 지금 판정 그대로다.
   - **실행에 넘기기:** cron이 울리면 지금 durable automation 작업을 넣고 automation worker가 generic `"automation"` 출처로 `AgentLoop`를 부른다(033, `crates/shacs-cli/src/automation_producer.rs`, `automation_worker.rs`). 지금은 cron 작업 id가 tool 판정까지 오지 않는다(`CronWake { job_id: None }`이고, 보통 `AgentLoop` 문맥은 `in_cron_context`가 꺼져 있다).
     - PRD 010은 믿을 수 있는 자동화 출처 종류(cron, heartbeat, 결과 관찰 등)와 원래 cron 작업 id를 producer → worker → `AgentLoop` → tool 판정 재료 → deferred bridge → process gate까지 넘긴다. 이 표시는 아무 권한도 주지 않는다. 목록 없는 cron run의 workspace 밖 접근은 거절된다(소유 범위 3). PRD 020은 이 표시를 쓴다.
     - PRD 030은 그 위에 승인 기록의 판 참조와 그 수명을 더한다. 022 PRD 005의 cron `approval_ref` 요구와 맞추어, cron 권한 한도에 승인 참조가 실제로 닿는 것을 시험으로 확인한다(`crates/shacs-core/src/runtime/permission_ceiling.rs`는 `approval_ref` 없는 cron 한도를 거절한다).
     - heartbeat와 하위 작업 결과 관찰은 같은 automation 경로를 쓰므로, generic `"automation"` 출처만으로는 cron 목록을 쓰지 못한다.
   - **판의 수명:** 넣은 automation 작업은 넣을 때의 승인된 목록 판을 가리킨다. 승인 기록의 판은 일정 표(`jobs.json`)와 따로, 사라지지 않는 취소 상태를 가진다.
     - 사용자가 작업을 지우면, 지웠다고 알리기 전에 그 판을 쓸 수 없게 한다. 그 뒤로는 아직 시작하지 않은 실행도, 이미 도는 실행의 다음 tool 호출(deferred·buffer된 호출 포함)도 목록을 쓰지 못한다. 이미 허용되어 시작한 호출은 끝까지 간다(사용자 결정 2026-11-16).
     - 실제 tool 호출을 들일 때마다 그 판이 지금 유효한지 보고, 같은 결과를 그 호출의 process gate에 넘긴다. 일정 표의 scheduler와 tool registry가 서로 다른 `PersistentCronService`를 쓰므로(`crates/shacs-cli/src/lib.rs`, `spec035_cli_media/tool_registry.rs`), 취소 상태는 한 instance 안의 표시가 아니라 저장된 상태여야 한다.
     - 한 번만 도는 작업(`delete_after_run`)이 일정 표에서 자동으로 지워지는 것은 취소가 아니다. 이미 넣은 그 실행은 재시작을 지나도 그 판을 쓴다.
     - 승인 기록이 없거나, 맞지 않거나, 취소되었으면 목록은 아무것도 허락하지 않는다. 그 실행은 cron 실행 표시를 그대로 지닌다(그래서 workspace 밖 접근은 거절된다).
     - durable work의 기존 취소(dispatcher가 실행이 끝난 뒤 본다)는 이 취소를 대신하지 않는다.
   - **대조:** workspace 밖 대상이 목록에 맞아야 한다.
     - file tool: 요청한 경로와 symlink를 따라간 경로가 모두 같은 종류 이상의 항목 안이어야 한다.
     - `exec`: 소유 범위 1이 찾은 workspace 밖 경로가 모두 항목 안이어야 한다. 읽기 항목은 읽기 전용으로 확인한 접근에만 맞는다. 048 명령 분석의 `reads`에 들었다는 것만으로는 읽기 전용이 아니다(분석이 쓰기로 알아보지 못한 인자를 모두 `reads`에 넣는다, `crates/shacs-core/src/runtime/auto_mode/shell_paths.rs`). 쓰기로 알아본 경로, 무엇을 하는지 확인할 수 없는 경로, 읽기와 쓰기에 함께 나온 경로는 쓰기 항목이 있어야 한다. 읽기 전용으로 확인하는 기준(읽기 전용 명령의 이름 목록과 입력 리디렉션 등)은 PRD 030이 정하며, 경로를 찾는 방식은 넓히지 않는다. `../`가 든 명령은 목록으로 허락하지 않는다.
     - 맞지 않으면 거절하고, 그 거절은 소유 범위 4를 따른다.
   - **명령 항목의 효력:** 그 작업의 cron run 동안 명령 항목은 설정 allow 규칙과 같은 자리, 같은 조건으로 쓰인다(auto mode는 048 PRD 000 판정 표 12번, 다른 mode는 048 PRD 000 범위 3의 설정 allow 규칙 자리). 그래서 명령 항목은 명령 실행만 허락한다.
     - `&&`·`;`·pipe로 이은 명령은 나뉜 명령마다 명령 항목에 맞아야 한다. 분석할 수 없는 명령은 명령 항목으로 허용되지 않는다(048 그대로).
     - 명령이 쓰는 workspace 밖 경로는 명령 항목과 별개로 경로 항목에 맞아야 한다.
     - 리디렉션: 048 판정 표 12번의 조건 중 "출력 리디렉션과 `tee` 대상이 workspace 안이거나 `edit` allow 규칙에 맞고, 입력 리디렉션 대상이 workspace 안이거나 `read` allow 규칙에 맞는다"에서, 그 cron 실행 동안 승인된 쓰기 경로 항목은 `edit` allow 규칙을, 읽기·쓰기 경로 항목은 `read` allow 규칙을 대신할 수 있다. 요청한 경로와 canonical 경로 둘 다 맞아야 한다. 그 밖의 조건(glob 대상, 알 수 없는 working directory, `~`로 시작하는 출력 대상, 보호 경로 쓰기, `classifyAllShell`)은 그대로다. 이 대신하기는 리디렉션 확인에만 쓴다(`crates/shacs-core/src/runtime/auto_mode/permission_materials.rs`, `rule_match.rs`).
     - deny 규칙, ask 규칙, 비밀 파일 규칙, critical path, 보호 경로 쓰기, 022 static deny, 010 안전 장치의 남은 거절, 030 process gate는 명령 항목이 풀지 못한다.
   - **요약할 수 없는 명령과 좁은 allow 규칙:** 지금 auto가 아닌 mode에서는 022 static ask인 `ProcExecSummaryUnavailable`(요약할 수 있는 명령은 인자 없는 `pwd`와 몇 가지 `cargo` 검증 명령뿐이다, `crates/shacs-core/src/runtime/tool_execution.rs`)이 allow 규칙보다 먼저 걸리고, 비대화형 run에서는 승인 재사용까지 막는다(`crates/shacs-core/src/runtime/permission_policy.rs`의 `static_ask_blocks_approval`). 그래서 명령 항목에 맞는 스크립트도 그 앞에서 거절된다. 053은 이것을 아래처럼 바꾼다(사용자 결정 2026-11-16).
     - auto가 아닌 mode에서, 명령에 맞는 쓸 수 있는 좁은 `exec` allow 규칙(user-local config의 설정 allow 규칙, 또는 지금 cron 작업의 승인된 명령 항목)이 있으면 `ProcExecSummaryUnavailable`을 채운 것으로 본다. 조건은 048 명령 분석이 나뉜 명령을 모두 분석했고 모두 그 규칙들에 맞으며, 격리(`ContainmentUnknown`이 아님, 030 process gate)가 따로 실행을 허락하는 것이다.
     - 넓은 규칙(048 소유 범위 2의 목록)은 이 자리에서 쓰지 않는다. 기억된 허용(022 `ExecPrefix`)은 이 자리를 채우지 않는다.
     - 022 static deny, 격리 실패, mode의 금지(`plan`), 사용자 deny·ask 규칙, 비밀 파일 규칙, critical path, 010 안전 장치의 남은 거절은 이것으로 풀리지 않는다.
     - tool 실행 판정과 process gate는 같은 재료로 같은 판정을 낸다(`crates/shacs-core/src/runtime/process_gate.rs`는 static 판정과 `decide_permission`을 다시 돈다).
     - auto mode는 048 판정 표가 022 static ask를 쓰지 않으므로 바뀌지 않는다.
   - **목록이 허락하지 않는 것:** 웹, 메시지 보내기, 하위 작업, 예약 같은 다른 판정은 그대로다. 비대화형 run이라 그 질문은 거절된다.
   - 정확한 경로 비교(폴더 항목의 범위, `~` 풀기, 정규화)는 PRD 030이 정한다. 목록 칸이 없는 기존 작업과 journal 기록은 빈 목록으로 읽는다(저장 형식은 칸이 없어도 읽혀야 한다).
6. **표면과 문서:** 진행 표시와 diagnostics에 workspace 밖 접근 판정, 거절 뒤 계속하기, cron 목록으로 허용하거나 거절한 것을 보인다. 각 PRD는 자기가 바꾼 동작을 `docs/USAGE.md`(권한, `exec` 안전 장치, `restrictToWorkspace` 설명)와 README에 함께 적는다. `exec`와 file tool의 설명 글에 workspace 밖 접근이 판정을 거친다는 것을 적을지는 PRD가 정한다.

## 기존 owner와의 경계

| 기존 owner | 053이 소비하는 것 | 053이 소유하지 않는 것 |
|---|---|---|
| 004 | tool 실행, runner의 tool 결과 분류와 run 종료 판단, 한 dispatch의 호출 순서 | workspace 위반이 아닌 fatal 판단, tool 결과 형식 |
| 010 | `exec` 안전 장치와 file tool 경로 검사, 경로 찾기 함수 | deny pattern과 기본 목록, allow pattern, 내부 주소 검사, symlink 검사, redaction |
| 011 | 하위 작업의 `restrict_to_workspace`, `fail_on_tool_error` | 하위 작업 runtime과 mode 상속 |
| 012 | run의 대화형 여부, cron 작업 저장과 실행 | background 실행, cron 일정 계산과 전달 |
| 013 | 승인 질문과 선택지 | UI 구현 |
| 020 | deferred bridge 호출 | tool 목록 표면 |
| 022 | permission mode, `fs_read` 자동 승인 기준, 기억된 규칙과 matcher, 승인 correlation | 다른 판정 기준, matcher 형식 |
| 030 | process gate, sandbox adapter | 격리 증명, sandbox 실행과 fallback |
| 031 | `tools.restrictToWorkspace`, `tools.exec.sandbox` 설정 | config schema의 나머지 |
| 037 | PRD 040의 turn을 멈춘 도구 결과 가리기 | 방식 선택과 결과 판정, 가리기의 범위 |
| 048 | 판정 순서, 사용자 규칙, 비밀 파일 규칙, 보호 경로, classifier, 막힘 수 | 판정 표의 나머지 줄 |

## 앞선 스펙에서 바꾸는 것

053은 아래 앞선 스펙의 계약을 바꾼다. 앞선 스펙 문서는 고치지 않고, 053이 닫힌 뒤에는 이 표의 "바뀐 뒤"가 지금 계약이다.

| 스펙 | 지금 계약(위치) | 바뀐 뒤 | PRD |
|---|---|---|---|
| 004, 048 | runner가 tool 결과에 workspace 위반 문구가 있으면 run을 `tool_error`로 끝낸다(`fatal_tool_message`, `is_workspace_violation`). 048 PRD 020 범위 2의 "지금 runner는 … 그 결과를 실패로 보고 run을 `tool_error`로 끝내는데"와 수용 조건 3의 "tool이 스스로 거절하는 호출(`exec` 안전 장치)은 지금과 같게 다룬다. 같은 호출 id가 다음 dispatch에서 실제로 workspace 위반을 내면 지금처럼 run이 멈춘다" | 그 결과와 auto mode의 판정 표 5번 거절은 고칠 수 있는 실패로 어시스턴트에게 돌아가고 안내가 붙는다. 같은 요청 안에서 같은 거절의 세 번째 사건일 때만 run이 멈추고, 그 dispatch의 뒤 호출은 시작하지 않는다(소유 범위 4). `fail_on_tool_error` run과 048이 실행하지 않은 호출은 그대로다 | 000 |
| 037 | PRD 040: 이미지·복합 요청에서 실행 중이거나 승인 뒤 실행한 도구 결과가 turn을 멈추는 오류(작업 폴더 밖 경로 등)이면 답과 기록에 digest만 남긴다 | 가리기와 그 범위(이미지·복합 요청)는 그대로다. workspace 위반이 turn을 멈추는 것은 반복 한도에 닿았을 때뿐이다(소유 범위 4) | 000 |
| 010 | SPEC "현재 아키텍처 매핑 기준" 1–3과 "현재 구현 요약" 1·2: 경로 입력은 workspace boundary와 symlink escape 검사를 통과해야 하고, shell 실행은 workspace 내부 working directory를 적용해야 한다. 구현은 `restrictToWorkspace`가 켜졌거나 sandbox가 없으면 workspace guard를 켠다(`workspace_guard_enabled`) | workspace guard는 `restrictToWorkspace`가 켜졌을 때만 거절한다. 꺼져 있으면 workspace 밖 경로와 `working_dir`은 허용 범위와 비교해 판정으로 간다(소유 범위 1, 3). deny pattern, allow pattern, 내부 주소, 경로 찾기 함수는 그대로다 | 010 |
| 004, 010 | file tool은 `restrictToWorkspace`와 상관없이 허용 범위 밖을 다루지 못한다 | `restrictToWorkspace`가 켜진 run에서만 그렇다. 꺼진 run에서 허용 범위 밖 대상은 판정으로 간다(소유 범위 1, 3). symlink 구성 요소가 있는 쓰기 거절, learned-method store 거절, 목록·검색이 밖으로 나가는 symlink를 따라가지 않는 것은 그대로다(소유 범위 2) | 020 |
| 048 | SPEC 판정 순서 머리글, 소유 범위 3의 "tool이 실행 전에 거절할 호출이면 묻지 않고 지금처럼 거절된다", 소유 범위 5의 마지막 항목, 소유 범위 6 전체, "Claude Code와 다른 점"의 "workspace 밖 file tool 읽기·쓰기"·"명령의 안전 장치" 줄, "반드시 지킬 것"의 "file tool의 범위(허용 폴더), 010의 `exec` 안전 장치 … 바꾸지 않는다", "남은 위험과 한계"의 "sandbox가 없으면 workspace 밖 경로를 적은 명령을 승인과 상관없이 막는다". PRD 000 판정 표 5번(file tool의 범위 밖 대상과 `exec` 안전 장치가 막는 명령 → 거절), 14번(`fs_read`이고 5번에 걸리지 않으면 허용), 범위 3 "다른 mode의 판정"의 5번 | 5번은 소유 범위 2의 남은 거절과 `restrictToWorkspace`가 켜진 run의 workspace 밖 대상이다. 꺼진 run의 workspace 밖 대상은 14·15번에 들지 않고 16번 classifier가 본다. 다른 mode는 결과가 허용이면 묻기로 바꾸는 한 줄을 더한다(소유 범위 3). 1–13번과 비밀 파일 규칙은 그 앞에서 그대로 걸린다 | 010, 020 |
| 012, 048 | cron 작업은 일정·메시지·전달만 담는다(`CronJob`). cron 등록은 auto mode에서 048 판정 표 16번 classifier가 보고(048 SPEC 판정 순서 4단계의 "예약"), 다른 mode에서는 022 판정을 따른다 | cron `add`에 경로 항목과 명령 항목의 목록을 더하고, 목록이 있는 등록은 모든 mode의 대화형 run에서 사용자에게 묻는다. 승인 기록에 묶인 목록만 실행 때 쓴다. 실행 때 workspace 밖 접근은 경로 항목에 맞아야 그 조건을 지나고, 명령 항목은 그 작업의 run 동안 설정 allow 규칙처럼 쓰인다(소유 범위 5). 목록 없는 등록은 그대로다 | 030 |
| 048 | SPEC "반드시 지킬 것": `autoMode`와 `permissions.allow\|ask\|deny`는 user-local config에서만 읽고, prompt·skill·도구 결과·workspace config는 auto mode 설정의 근거가 되지 않는다. 소유 범위 2의 사용자 규칙 출처 | 사용자가 cron 등록 질문에서 승인한 명령 항목이 그 작업의 cron run에서만 쓰이는 allow 규칙의 출처로 더해진다. PRD 000 판정 표 12번의 리디렉션 대상 조건에서 승인된 경로 항목이 `edit`·`read` allow 규칙을 대신할 수 있다(그 cron run에서만, 리디렉션 확인에만). 어시스턴트가 낸 목록은 사용자 승인과 승인 기록이 있을 때만 효력이 있다. user-local config의 규칙과 다른 출처의 제한은 그대로다(소유 범위 5) | 030 |
| 022 | SPEC `proc_exec` 기준과 Auto evaluator 계약 8: 요약할 수 없는 `proc_exec`는 자동 승인하지 않는다. 구현은 auto가 아닌 mode에서 `ProcExecSummaryUnavailable` static ask를 allow 규칙보다 먼저 보고, 비대화형 run에서는 승인 재사용도 막는다(`static_ask_blocks_approval`) | auto가 아닌 mode에서 명령에 맞는 좁은 `exec` allow 규칙(설정 allow 규칙, 또는 지금 cron 작업의 승인된 명령 항목)이 있고, 048 분석이 나뉜 명령을 모두 분석해 모두 맞으며, 격리가 따로 허락하면 그 static ask를 채운 것으로 본다. 넓은 규칙과 기억된 허용은 채우지 못하고, static deny·격리 실패·mode 금지·deny·ask 규칙·비밀 파일·critical path·남은 tool 거절은 그대로다(소유 범위 5) | 030 |
| 033, 022 | cron 실행은 durable automation 작업을 넣고 automation worker가 generic `"automation"` 출처로 `AgentLoop`를 부른다. cron 작업 id는 tool 판정까지 오지 않는다(022 PRD 005의 `CronWake`는 `job_id`가 없다) | automation 작업이 믿을 수 있는 출처 종류와 원래 cron 작업 id를 싣고 `AgentLoop`·tool 판정 재료·deferred bridge·process gate까지 넘긴다. 이 표시는 권한을 주지 않는다(소유 범위 5) | 010 |
| 033, 022 | 승인 기록과 cron 실행을 잇는 계약이 없다 | automation 작업이 승인 기록의 판 참조를 싣고, 실제 tool 호출마다 그 판의 유효성(취소 포함)을 본다. cron 권한 한도에 `approval_ref`가 닿는다. heartbeat와 결과 관찰은 cron 목록을 쓰지 못한다(소유 범위 5) | 030 |
| 004, 022, 048 | 비대화형 run은 022·048 판정을 따르고, 설정 allow 규칙과 기억된 허용으로 허용된 호출은 묻지 않고 실행된다 | workspace 밖 대상은 cron 목록에 맞거나(cron run) `bypass_permissions`일 때(그 밖의 비대화형 run)만 그 조건을 지난다. allow 규칙, 기억된 허용, classifier는 허락하지 못한다(소유 범위 3) | 010, 020, 030 |

022 `fs_read` 자동 승인 기준("workspace 밖 host path", "symlink escape target"은 자동 승인하지 않는다)은 바꾸지 않는다. 053은 file tool이 그런 대상을 다룰 수 있게 되는 run에서 그 기준을 auto가 아닌 mode의 읽기·편집 자동 허용에 그대로 적용한다(소유 범위 3).

아래는 바꾸지 않는다.

- deny pattern과 그 기본 목록, allow pattern, 내부·사설 주소 검사, 030 process gate. 이 거절은 그대로이고 turn을 끝내지 않게만 바뀐다.
- 하위 작업과 worktree workflow 자식이 `restrict_to_workspace`를 늘 켜는 것(011), 하위 작업의 `fail_on_tool_error`.
- 048의 판정 표 나머지 줄, 사용자 규칙 문법, 비밀 파일 목록, 보호 경로 목록, classifier 입력과 template, 막힘 수.
- 022 기억된 규칙의 matcher 형식. workspace 밖 file tool 대상은 정확한 action(`ExactAction`)으로 기억되고, `exec`의 `ExecPrefix` 기억은 그대로다.
- 037 PRD 040의 가리기 범위(이미지·복합 요청).
- `tools.restrictToWorkspace`의 기본값(꺼짐)과 config 형식.
- sandbox adapter와 fallback 동작(030). 053은 sandbox 설정이 guard를 켜고 끄지 않게만 한다.

### 바뀌는 기존 시험

PRD는 아래 시험의 기대를 일부러 바꾸고, 바꾼 까닭을 구현 메모에 적는다.

- `crates/shacs-core/tests/tools.rs`의 `restrictToWorkspace`가 꺼지고 sandbox가 없을 때 workspace 밖 경로를 거절하는 시험(1801–1843번 줄 근처): 판정으로 넘기는 기대로 바꾼다. `restrictToWorkspace`가 켜진 시험(1762–1799번 줄 근처)은 그대로다(010).
- `crates/shacs-core/tests/it/spec048_auto_mode/decision_table.rs`의 "read outside the file tool range", "exec naming a path outside the workspace without a sandbox" 줄: `restrictToWorkspace`가 켜진 줄과 꺼진 줄로 나눈다(010, 020).
- `crates/shacs-core/tests/it/spec048_auto_mode/blocks.rs`에서 실제 workspace 위반이 run을 바로 멈추는 기대(319–374번 줄 근처): 반복 한도 기대로 바꾼다(000).
- `crates/shacs-core/tests/it/spec037_method_selection/composed_errors.rs`, `composed_output.rs`의 turn을 멈춘 출력 시험: 원문을 가리는 확인은 그대로 두고, 멈추는 계기를 반복 한도로 바꾼다(000). 같은 즉시 중단을 기대하던 `report_terminal.rs`, `calculation_terminal.rs`도 같다(000).
- `crates/shacs-core/tests/it/spec048_auto_mode/classifier.rs`의 격리 증명이 없는 deferred process 호출 시험: 판정 표 5번 거절이 소유 범위 4의 대상이 되어 결과에 안내가 붙는다. 실행되지 않음, 질문 없음, gate 문구로 시작함은 그대로 확인한다(000).

## Claude Code와 다른 점

| 항목 | Claude Code | 053 | 이유 |
|---|---|---|---|
| 밖을 막는 설정 | 경로 deny 규칙으로 막는다 | 그것에 더해 `restrictToWorkspace` 하나가 명령과 file tool의 workspace 밖 접근을 함께 막는다 | 이미 있는 설정을 쓴다 |
| 하위 작업 | 하위 작업도 같은 판정을 따른다 | 하위 작업과 worktree workflow 자식은 지금처럼 workspace 밖을 거절하고, 거절하면 멈춘다 | 011의 계약을 바꾸지 않는다 |
| 명령의 고정 거절 | 승인이나 classifier가 허용한 명령에 대해, 여기 남기는 것과 같은 조건 없는 정규식 거절은 없다 | 010의 deny pattern, allow pattern, 내부 주소 검사는 그대로 거절한다. 다만 turn을 끝내지 않는다 | 010 소유의 검사를 넓히지 않는다 |
| 같은 거절의 반복 | turn을 멈추지 않는다 | 같은 요청 안에서 같은 거절의 세 번째 사건이면 멈춘다. nanobot은 더 강한 거절 글만 돌려주고 멈추지 않는데, 053은 일부러 멈춘다 | 정책 경계를 거듭 두드리는 시도를 끊는다 |
| 명령의 밖 경로 찾기 | 명령을 셸 문법으로 읽어 경로 인자와 리디렉션을 본다 | 010 guard가 쓰는 글자 찾기(절대 경로, `~`, `../`) 그대로다 | 새 명령 분석을 더하지 않는다. 변수로 만든 경로처럼 글에 나오지 않는 꼴은 찾지 못한다(남은 위험) |
| 폴더 단위 기억 | 밖 폴더 읽기를 그 세션 동안 허용하는 답이 있다 | 없다. workspace 밖 file tool 대상은 정확한 action으로만 기억한다(`exec`는 지금 명령 앞부분 기억 그대로, cron 목록은 폴더 항목을 받는다) | 022 matcher가 workspace 기준이다. 넓히려면 matcher 계약을 바꿔야 한다 |
| 예약 작업의 권한 | 예약 실행은 그 자리의 권한 설정을 따른다 | cron 작업을 만들 때 workspace 밖 경로와 실행할 명령의 목록을 물어 승인 기록과 함께 그 작업에 묶고, 실행 때는 그 목록만 쓴다 | cron은 도중에 물을 수 없다. 사용자 결정 2026-11-15 |

## 반드시 지킬 것

- `restrictToWorkspace: true`인 run과 하위 작업·worktree workflow 자식에서는 허용 범위 밖 접근이 실행되지 않는다.
- `restrictToWorkspace`가 꺼진 대화형 run에서 workspace 밖 접근은 다음 중 하나 없이 실행되지 않는다: 이번 호출에 대한 사용자 승인, 048 조건을 지키는 설정 allow 규칙이나 기억된 허용, auto mode의 classifier 허용, `bypass_permissions`. `dont_ask`에서 이 중 아무것도 없으면 거절된다.
- 비대화형 run에서 workspace 밖 접근은 cron run이면 `bypass_permissions`를 포함해 사용자가 등록 때 승인한 그 작업의 목록에 맞을 때만, 그 밖의 비대화형 run은 `bypass_permissions`일 때만 그 조건을 지난다. 그 뒤의 다른 판정은 따로 걸린다.
- cron 목록(경로 항목과 명령 항목)은 사용자의 직접 승인으로만 효력이 생기고, 그 작업의 cron run에서만 쓰인다. 명령 항목은 넓은 규칙을 받지 않고, deny·ask 규칙, 비밀 파일 규칙, critical path, 보호 경로 쓰기, 022 static deny, 010 남은 거절, process gate를 풀지 못한다. 사용자가 작업을 지우면 아직 시작하지 않은 실행과 도는 실행의 다음 tool 호출부터 목록이 쓰이지 않는다. 저장 파일이나 journal을 고쳐 목록을 만들거나 넓혀도 승인 기록과 맞지 않으면 쓰이지 않는다.
- 비밀 파일 규칙, 사용자 deny·ask 규칙, critical path 제거 질문, 보호 경로 쓰기 판정, 소유 범위 2의 남은 tool 거절은 workspace 밖에서도 지금과 같은 힘으로 걸린다.
- 승인은 그 호출에만 쓰인다. tool 실행과 process gate는 판정과 같은 판정을 낸다(048 그대로).
- tool의 거절은 실행되지 않은 채 어시스턴트에게 돌아간다. 안내 글이 거절을 풀지 않는다. 반복 한도에 닿은 뒤 같은 dispatch의 호출은 시작되지 않는다(소유 범위 4의 읽기 전용 묶음 예외는 빼고).
- sandbox 설정의 유무가 workspace 밖 접근 판정을 바꾸지 않는다.
- Cargo 테스트는 가짜 provider로 classifier 응답을 정해 돈다. 시험은 고정한 대기 시간에 기대지 않는다.
- `Cargo.lock`에 새 crate를 더하지 않는다.

## 수용 조건

1. 거절 뒤 계속하기(가짜 provider의 실제 `AgentLoop` turn):
   - `restrictToWorkspace: true`에서 workspace 밖 경로를 적은 `exec`, deny pattern에 맞는 `exec`, 내부 주소를 적은 `exec`, 범위 밖 `read_file`, auto mode의 판정 표 5번 거절이 각각 run을 멈추지 않고 실패한 tool 결과와 안내로 돌아가고, 어시스턴트가 다음 호출로 일을 끝낸다.
   - 같은 요청 안에서 같은 거절의 세 번째 사건이면 run이 `tool_error`로 멈춘다. 한 dispatch에 같은 거절 셋과 그 뒤 쓰기가 있으면 쓰기는 시작되지 않는다. 다른 대상의 거절은 따로 센다. deferred bridge 호출은 bridge가 부른 tool로 센다. 승인 질문을 지나 이어진 run에서도 앞의 셈이 이어지고, 다음 사용자 요청에서 0이 된다.
   - 멈춘 뒤 이미지·복합 요청의 답과 기록은 037 PRD 040대로 가려지고(실행 중과 승인 뒤 실행 둘 다), 그 밖의 요청은 지금처럼 거절 결과가 답이 된다.
   - `fail_on_tool_error` run은 첫 거절에서 멈춘다. 048이 실행하지 않은 호출의 기존 시험은 그대로 통과한다.
2. `exec`:
   - `restrictToWorkspace: false`, sandbox 없음에서 `… > /tmp/x.txt`, `cat ~/x`, `../`가 든 명령, workspace 밖 `working_dir`이 tool에서 거절되지 않고 판정으로 간다. working directory가 workspace 하위 폴더일 때 workspace 안의 다른 절대 경로는 workspace 밖으로 보지 않는다. media 폴더 경로는 workspace 밖으로 보지 않는다.
   - `restrictToWorkspace: true`면 지금처럼 거절된다(working directory 기준 비교 포함).
   - `sandbox: "bwrap"`이 적혀 있어도(준비 확인이 실패해 fallback하는 경우 포함) guard 판단이 sandbox가 없을 때와 같다.
   - deny pattern, allow pattern, 내부 주소 거절은 두 설정 모두에서 그대로다. 판정이 허용한 workspace 밖 명령은 process gate도 통과한다.
3. 판정(048 판정 표 시험에 `restrictToWorkspace`가 꺼진 줄을 더한다):
   - auto mode에서 workspace 밖 읽기·쓰기(file tool), workspace 밖 경로를 적은 명령이 classifier로 가고, 허용이면 실행되고 막음·판정 없음이면 실행되지 않는다. deny·ask 규칙, critical path 제거, 048 조건을 지킨 allow 규칙·기억된 허용은 그 앞에서 걸린다.
   - `default`·`accept_edits`의 대화형 run에서 workspace 밖 읽기와 편집도 묻고, 승인한 그 호출만 실행된다. `plan`은 쓰기와 명령을 지금처럼 막고 workspace 밖 읽기는 묻는다. `dont_ask`에서는 그 질문이 거절이 되고, 기억된 허용이 있는 호출은 실행된다. `bypass_permissions`에서는 실행된다. 비대화형 run은 수용 조건 4를 따른다.
   - 질문에서 기억한 허용은 같은 action의 다음 접근을 묻지 않게 한다.
   - data 폴더의 인증 저장소와 `.env.json`을 file tool이나 이름을 쓴 명령으로 읽으면 모든 mode에서 매번 묻고 기억 선택지가 없으며, 쓰기는 거절된다. 허용 범위 밖 폴더의 목록·검색에도 비밀 파일이 나오지 않고, 밖으로 나가는 symlink를 따라가지 않는다. `config.json` 같은 보호 경로 쓰기는 auto mode에서 classifier가 보고 allow 규칙으로 허락되지 않는다.
   - symlink 구성 요소가 있는 쓰기와 learned-method store 쓰기는 `restrictToWorkspace`와 상관없이 지금처럼 거절된다.
4. cron(항목 번호는 PRD 소유를 가리키므로 바꾸지 않는다):
   - 4.1 등록: 목록이 있는 `add`는 auto mode, `bypass_permissions`, `dont_ask`를 포함한 모든 mode의 대화형 run에서 목록 전부를 적은 질문을 내고, classifier·allow 규칙·기억된 허용으로 넘어가지 않으며, 비대화형 run에서는 거절된다. 승인하면 목록이 그 작업에 저장되고 목록 보기에 나오며, 거절하면 작업이 만들어지지 않는다. 비밀 파일이나 보호 경로를 직접 가리키는 쓰기 항목, `python3 *` 같은 넓은 명령 pattern이 든 목록은 등록이 거절되고, home을 읽기 항목으로 적은 목록은 받는다.
   - 4.2 경로 항목: production 경로(cron이 울려 automation 작업을 넣고 worker가 `AgentLoop`를 부른다)에서, auto mode와 classifier 허용이고 앞선 거절이 없을 때 목록 안의 file tool 읽기·쓰기가 실행된다. 목록 밖, 읽기 항목만 있는 경로에 쓰기, 읽기 전용으로 확인할 수 없는 명령 경로(`mkdir <읽기 항목 안의 경로>` 같은), symlink를 따라가면 목록 밖인 대상, `../`가 든 명령은 어느 mode에서든(`bypass_permissions` 포함) 거절된다. 다른 mode의 경로 항목 안 접근은 그 mode의 지금 판정을 따른다. 같은 세션을 쓰는 두 cron 작업은 서로의 목록을 쓰지 못한다. heartbeat와 결과 관찰 run은 cron 목록을 쓰지 못한다.
   - 4.3 명령 항목: `default` mode의 production cron run에서 명령 항목에 맞는 `exec`(유튜브 자막 받기 스크립트)가 묻지 않고 실행된다. 다른 권한 근거(설정 allow 규칙, 기억된 허용)가 없는 `default` mode에서, 맞지 않는 명령, 이은 명령 중 하나라도 맞지 않는 명령, 분석할 수 없는 명령은 거절된다. 명령 항목에 맞아도 workspace 밖 리디렉션 대상이 쓰기 경로 항목에 없으면 거절된다. 명령 항목은 deny 규칙과 010 deny pattern에 맞는 명령을 허용하지 않는다. 명령 항목은 같은 세션의 대화형 turn이나 다른 cron 작업에 쓰이지 않는다. 명령 항목에 맞지 않는 것은 권한을 더하지 않을 뿐, 설정 allow 규칙·기억된 허용·mode의 지금 동작은 그대로 쓰인다.
   - 4.3a 요약할 수 없는 명령: 대화형 `default` mode에서 설정 allow 규칙 `exec(python3 <고정 스크립트 경로> *)`에 맞는 명령은 묻지 않고 실행되고, `python3 *` 같은 넓은 규칙이나 기억된 허용만 있는 명령은 지금처럼 묻는다. 격리를 확인할 수 없는 실행(`ContainmentUnknown`)은 규칙이 있어도 지금처럼 묻거나 거절된다. process gate가 tool 판정과 같은 판정을 낸다.
   - 4.4 승인 증거: 저장한 작업 파일이나 journal에서 목록을 늘리면, 늘린 부분은 승인 기록과 맞지 않아 쓰이지 않는다.
   - 4.5 수명(고정 대기 시간 없이, 명시적 동기화로 시험한다): 삭제 → 재시작 → 대기열의 실행은 목록을 쓰지 못한다. 서로 다른 `PersistentCronService` instance를 쓰는 상태에서 두 tool 호출 사이에 삭제하면 둘째 호출부터 목록을 쓰지 못한다. 한 번만 도는 작업의 자동 정리 → 재시작 → 실행은 목록을 쓴다.
   - 4.6 빈 목록과 이전 기록: 목록이 비었거나 맞지 않는 cron run은 `bypass_permissions`에서도 workspace 밖 접근이 거절된다. 목록 칸이 없는 기존 작업과 journal 기록, 승인 판 참조가 없는 기존 automation 작업은 읽히고 빈 목록이다.
   - 4.7 비대화형 run: 기억된 허용과 설정 allow 규칙이 있어도 cron·heartbeat·local API run의 workspace 밖 접근은 cron 목록이 아니면 거절되고(명령은 010, file tool은 020), cron이 아닌 비대화형 run은 `bypass_permissions`에서 그 조건을 지난다. production cron run과 heartbeat run이 그 출처 종류로 구분된다(010). cron run이 시작한 하위 작업은 workspace 밖을 거절한다.
5. 2026-11-15 유튜브 흐름(skill 읽기, 자막 받기, `> /tmp/… ; wc -lc /tmp/…`, 요약)이 auto mode와 classifier 허용에서 승인 질문 없이 끝나고 `/tmp`에 파일이 생긴다(가짜 provider, 실제 실행 기록으로 확인한다). 같은 흐름에서 `restrictToWorkspace: true`면 `/tmp` 쓰기가 거절되고 안내를 받은 어시스턴트가 workspace 안에 써서 끝낸다.
6. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다. 전체 workspace 테스트는 사용자 승인을 받아 실행한다.

위 조건은 053의 단독 종료 기준이다.

## 남은 위험과 한계

- `restrictToWorkspace`를 켜지 않은 auto mode 사용자는 workspace 밖 접근도 classifier 판단에 맡긴다. classifier는 틀릴 수 있다(048 남은 위험). 지금은 승인과 상관없이 막히던 workspace 밖 쓰기와 읽기가 classifier 허용으로 실행된다.
- 기본 배치(workspace가 data 폴더 아래 `workspace/`)에서 data 폴더의 workspace 밖 부분(`config.json`, `runtime/`, `cron/`, `logs/`, `channels/` 등)은 지금 file tool과 guard가 닿지 못했지만, 이제 판정을 거쳐 읽힐 수 있다. 비밀 파일은 매번 묻고 보호 경로 쓰기는 classifier가 보지만, log와 runtime 상태 읽기는 auto mode에서 classifier만 막는다. 이를 원하지 않는 사용자는 `restrictToWorkspace`를 켜거나 deny 규칙을 적어야 한다. 세션 기록은 기본 배치에서 workspace 안(`workspace/sessions/`)이라 지금도 묻지 않고 읽히며, 053이 바꾸지 않는다.
- 명령의 workspace 밖 경로는 010 guard의 글자 찾기로만 찾는다. 변수로 만든 경로, 스크립트 안에서 여는 파일처럼 명령 글에 나오지 않는 접근은 workspace 밖 접근으로 보이지 않는다. auto mode에서는 그 명령도 classifier가 보지만, 다른 mode에서는 022 판정만 따른다. 지금 guard도 찾지 못하므로 새로 생긴 틈은 아니다.
- 공식 Docker 경로에는 쓸 수 있는 sandbox가 없다. 명령은 shacs-bot과 같은 container·사용자 권한으로 돈다.
- 반복 한도에 닿은 뒤에도, 동시에 실행한 읽기 전용 묶음 안에서는 실행 중에 처음 생긴 거절보다 뒤에 있는 읽기 호출이 이미 시작했을 수 있다(소유 범위 4의 예외). 쓰기와 명령은 해당하지 않는다.
- 거절이 turn을 끝내지 않으므로 어시스턴트가 같은 일을 다른 꼴로 거듭 시도할 수 있다. 같은 거절 세 번에서 멈추지만, 대상이나 종류가 다른 시도는 따로 센다. auto mode의 classifier 막힘 수(048)는 tool 거절을 세지 않는다.
- 이미지·복합 요청이 아닌 요청이 반복 한도로 멈추면 지금처럼 거절 결과 글이 그대로 답이 된다.
- cron 목록은 등록 때 어시스턴트가 적은 것이다. 실제 실행에 필요한 경로를 빠뜨리면 그 접근은 거절되고 사용자가 작업을 고쳐야 한다. 넓은 폴더 항목(`~` 같은)을 승인하면 그 아래 전부를 묻지 않고 다루게 된다(비밀 파일과 보호 경로 쓰기는 빼고).
- cron 목록은 workspace 밖 경로와 명령 실행만 다룬다. cron에서 질문이 필요한 다른 일(웹, 메시지 보내기, 하위 작업 등)은 지금처럼 거절된다. 그런 권한까지 미리 물으려면 뒤 스펙이 필요하다.
- 좁은 `exec` allow 규칙이 요약할 수 없는 명령의 static ask를 채우므로, 사용자가 설정에 적은 좁은 규칙도 대화형 `default` mode에서 그 스크립트를 묻지 않고 실행하게 된다. 스크립트가 하는 일은 요약되지 않으며, 규칙을 적은 사용자의 판단에 맡겨진다.
- 명령 항목의 `*`는 인자를 넓게 받는다. 스크립트 경로를 고정해도 그 스크립트가 받는 인자나 스크립트 내용이 바뀌면 승인 때 생각한 것과 다른 일을 할 수 있다. workspace 안의 스크립트는 어시스턴트가 고칠 수 있으므로, 명령 항목이 가리키는 스크립트를 나중에 고치면 cron이 고친 내용을 실행한다(auto mode에서는 그 수정이 classifier를 거친다).
- cron 승인 기록은 어시스턴트의 tool 호출로 만들 수 없게 하지만, 파일 이름을 명령에 쓰지 않고 파일을 여는 프로그램은 막지 못한다(048 남은 위험과 같다). `bypass_permissions`에서 그런 프로그램이 승인 기록을 고칠 수 있는지는 PRD 030이 고른 방식(비밀 파일 또는 서명)에 달려 있고, 서명이면 키를 모르는 한 기록을 만들지 못한다.

## 실행 PRD와 독립 종료

PRD 문서는 구현을 시작할 때 쓴다. 각 PRD는 앞 번호의 닫힌 결과만 소비하고 번호 순서대로 구현한다. 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| 000 거절 뒤 계속하기 | 소유 범위 4(거절 사건과 그것을 알리는 자리, 셈의 범위, 승인 뒤 이어진 run과 bridge 호출 포함), 6 중 자기 변경과 그 문서 | 없음 | 수용 조건 1, 6 |
| 010 명령의 workspace 밖 경로 | 소유 범위 1·2·3 중 `exec`, 판정과 process gate가 함께 쓰는 workspace 밖 대상 재료, auto가 아닌 mode의 묻기 줄, 비대화형 run 줄(cron 목록 없이 거절), 소유 범위 5의 믿을 수 있는 자동화 출처와 cron 작업 id 넘기기, 6 중 자기 변경과 그 문서 | 000(guard가 켜진 run의 거절 뒤 처리) | 수용 조건 2, 3의 명령 항목, 4.7 중 명령과 출처 구분, 5, 6 |
| 020 file tool의 workspace 밖 대상 | 소유 범위 1·2·3 중 file tool, 6 중 자기 변경과 그 문서 | 000, 010(workspace 밖 대상 재료, 묻기 줄, 비대화형 거절) | 수용 조건 3의 나머지, 4.7 중 file tool, 6 |
| 030 cron 작업의 미리 허락된 목록 | 소유 범위 3의 비대화형 run 줄 중 cron 목록, 5(목록, 묻기, 승인 기록과 판 참조, 판의 수명과 취소, 읽기 전용 확인, 명령 항목, 리디렉션 대신하기), 6 중 자기 변경과 그 문서 | 000, 010(workspace 밖 대상 재료, 비대화형 거절, cron 출처와 작업 id), 020(file tool 판정) | 수용 조건 4.1–4.6(4.3a 포함), 6 |

수용 조건 6은 각 PRD가 자기 변경에 대해 확인한다.
