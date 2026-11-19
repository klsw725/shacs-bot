# PRD 010. 선언한 값을 exec에 넘기기

Status: Implemented (independent review approved 2026-11-18)

Consumes: 010 `ExecTool` 환경·명령 검사, 011 subagent 실행 설정, 012·033 agent 자동화 경로, 024 workflow 자식, 030 controlled child와 sandbox, 048 permission environment·`SecretValues`·classifier, 049 browser 빈 환경, 053 명령·파일 판정.

Depends on: [PRD 000](000-declarations-and-missing-status.md)의 닫힌 결과(선언 정규화, 활성 원본 선택, 값 우선순위와 missing 상태). 020의 사용자 문서 완성을 기다리지 않는다.

## Goal

config env에 적지 않은 선언 키가 실제 exec 자식에 들어간다. 본 대화와 그 자식, cron·heartbeat의 agent 실행이 같은 규칙을 쓰고, workflow의 별도 자식 설정 생성에서도 환경 자료를 잃지 않는다. 값은 실행을 위한 메모리에만 두고, 기존 classifier 가리기에는 실제 전달값을 포함한다.

## 지금 상태

- startup은 `crates/shacs-cli/src/lib.rs:3155-3162`에서 `resolve_config_with_env_file`의 결과를 받아 auto mode에 기억하고 끝난다. `.env.json` 키가 exec 설정에 자동으로 들어가지는 않는다.
- 명시 env 합성은 `crates/shacs-cli/src/lib.rs:22052-22056`, main 등록은 `spec035_cli_media/tool_registry.rs:108-129`, adapter 보관은 `lib.rs:19816`이다.
- 일반 spawn은 `lib.rs:20912-20943`에서 자식 설정을 capture하고 `:21108-21134`가 exec 설정을 복사한다. child context와 registry는 `crates/shacs-core/src/runtime/subagent.rs:909-911`, `:1535-1543`에서 그 map을 쓴다.
- cron 등은 `crates/shacs-cli/src/automation_worker.rs:329-367`에서 같은 adapter의 `run_agent_loop_turn_with_origin_control`을 부른다. heartbeat는 `lib.rs:21137-21139`에서 agent 자동화에 넣는다.
- workflow는 `crates/shacs-core/src/runtime/agent_loop.rs:3236-3271`에서 별도로 자식 설정을 만들고 exec 환경을 복사하지 않는다. `runtime/workflow.rs:530-546`은 그 설정을 복사해 workspace와 도구 scope를 바꾼다. main workflow 생성은 `enable_exec = false`이며 이 PRD가 켜지 않는다.
- `crates/shacs-core/src/tools/shell/guard.rs:31-52`가 환경을 만들고 `execution.rs:67-76`이 `path_append`를 더한다. `process.rs:29-35`는 부모 환경 상속을 끄고 이 map을 controlled child에 넣는다.
- sandbox는 `runtime/sandbox_adapter/execute.rs:13-47`, `:75-100`에서 실행·준비 확인에 같은 command 환경을 쓴다. 이 공유 adapter 자체에 모든 process용 skill env를 추가하면 browser 등 범위를 넘길 수 있다.
- `auto_mode_cli.rs:20-50`, `:66-96`은 매 turn `.env.json`을 다시 읽는다. `SecretValues::new`(`runtime/auto_mode/secret_values.rs:37-50`)는 파일 값만 받고 8자 이상만 가린다. process 우선값과 명시 env로 덮인 값은 이 파일 값과 다를 수 있다.

## Scope

1. **시작 때 값의 원천 보관:**
   - `${이름}`을 푼 그 startup의 `EnvFile`과 process 환경의 읽기 결과를 adapter의 직렬화하지 않는 자료로 보관한다. config 파일을 다른 자리에서 다시 읽어 별도의 env 기준을 만들지 않는다. 값 원천의 `Debug`는 이름만 보인다.
   - `Config.env`나 `tools.exec.env`에 자동 전달값을 삽입하지 않는다. 원본 config와 `ExecutionSnapshot`의 설정 자료를 오염시키지 않기 위해서다. `configured_exec_env`는 계속 명시 설정만 뜻한다.
   - file 부재는 정상 빈 파일이고 missing 키만 생긴다. 시작 때 파일이 malformed·unreadable이면 048의 config load 오류 그대로 시작을 멈춘다. 스킬에만 선언된 키가 없는 것은 시작 오류가 아니다.
   - `.env.json`, process 값, 명시 env 변경은 runtime 재시작 뒤 반영한다. 기존 auto mode의 turn별 파일 재읽기는 가리기용으로 계속 별도로 유지한다.
2. **실행 시작의 선택:**
   - main 사용자 turn, 채널·local API turn, cron 한 번, heartbeat 한 번을 시작할 때 000의 활성 목록을 한 번 고르고 보관한 원천에서 이름을 해석한다.
   - 그 실행의 스킬 요약, 실제 읽을 원본, missing 표시, exec 전달이 같은 선택을 쓴다. 실행 중 skill 파일 변경으로 새 키를 더하지 않는다. 다음 시작에는 다시 선택한다. 승인 답으로 이어지는 실행은 기존 승인 correlation을 유지하며, 값이 든 자료는 저장하지 않고 runtime의 현재 시작 원천으로 다시 구성한다.
   - 선언한 이름 가운데 실제 비어 있지 않은 최종값과 상태를 메모리 자료로 나눈다. 자식은 부모의 선택을 그대로 받고 child workspace/worktree에서 키 목록을 넓히지 않는다.
   - 자료는 `AgentLoopConfig`·`ToolExecutionContext`와 자식 실행 설정의 기존 메모리 전달 경로로 이어진다. 직렬화하는 구조에 붙일 때는 직렬화에서 제외한다. `SpawnEnvelope`, workflow checkpoint, durable event, session metadata에 값 map을 넣지 않는다.
3. **exec 환경 합성:**
   1. 현재의 `HOME`, `LANG`, `TERM`, `PATH`.
   2. 현재의 `allowed_env_keys`에 지정된 process 값.
   3. 이번 실행의 스킬 자동 전달값(예약 네 이름 제외).
   4. 명시 top-level `env`.
   5. 명시 `tools.exec.env`.
   6. 기존 `path_append`.

   - 뒤 항목이 앞 항목을 덮는다. 명시 설정의 빈 값도 덮고 missing 상태를 만든다. 선언값 부재를 빈 문자열로 삽입하지 않는다.
   - 합성은 자식을 시작할 때 `exec`가 받은 실행 문맥 안에서 한다. 공유 `ToolRegistry`의 `ExecConfig`나 process 전역을 turn마다 바꾸지 않는다. 동시 세션이 서로의 키 목록을 쓰지 않게 한다.
   - `allowed_env_keys`와 명시 env를 사용하는 기존 API 호출자는 skill 환경이 없는 경우 지금 동작 그대로다. 이 PRD 때문에 provider 인증이나 MCP env의 의미를 바꾸지 않는다.
   - 값은 shell 인자에 직접 치환하지 않고 자식 환경으로만 넣는다. `$NOTION_API_KEY`는 기존 shell이 해석한다. 명령 판정·process gate를 통과하기 전에 자식을 시작하지 않는다.
4. **전달 경로:**

   | 경로 | 054에서 이어야 할 것 | 그대로 남는 제한 |
   |---|---|---|
   | main·채널·local API | production registry의 exec가 이번 turn의 자료를 받음 | 도구 enable, permission mode, process gate |
   | 일반 subagent | `subagent_execution_config` → `SubagentExecutionConfig` → child context·registry·tool context | `restrict_to_workspace`, `fail_on_tool_error`, 도구 scope |
   | cron·heartbeat agent 실행 | automation worker가 시작한 turn에서 같은 resolver를 쓰고 spawn 자식까지 전달 | 비대화형 승인·053 cron 목록 |
   | workflow | `workflow_subagent_execution_config`의 별도 생성과 workflow의 config 복사를 지나도 부모 자료 유지 | 기존 exec off·read-only scope, worktree 경계. 전달을 이유로 도구를 켜지 않음 |
   | sandbox/native | `ExecTool`이 만든 최종 환경 → controlled child → wrapped/native/fallback·준비 확인 | `inherit_env = false`, sandbox 실패 정책 |

   - exec가 허용된 workflow 입력은 같은 환경을 쓰되 현재 main workflow가 exec를 켜지 않는 사실을 완료 보고에 적는다. “workflow에서 실제 shell을 실행했다”와 “exec가 꺼진 경로에서 설정을 잃지 않았다”를 구분한다.
   - agent 없는 automation script, plugin command, MCP, credential command, 브라우저·viewer에는 이 자료를 적용하지 않는다. 공통 process spawn 함수에 묵시적으로 덧붙이지 않는다.
5. **실제로 전달하는 값 가리기:**
   - 048 `SecretValues`의 입력을 현재 turn의 파일 값과 이번 실행의 선언 키 최종 전달값의 합으로 만든다. `.env.json` 대신 process 값이 이겼거나 명시 env가 덮었어도 실제 child 값이 가리기 대상이다.
   - 파일을 재시작 전에 바꿔 이름 하나에 옛 값·새 값이 생기면 둘 다 남긴다. 이름을 key로 하는 map 하나에 덮어 옛 전달값을 잃지 않는다. 같은 값이 여러 이름에 있으면 기존 이름순 대표 하나, 길이가 긴 값 우선, 치환한 글 재해석 없음, 8자 기준을 유지한다.
   - 기존 048 적용 자리만 확장한다: 밖에 보이는 action 인자와 digest·승인·permission 진단, classifier가 보는 instruction/user/tool-call 글, 결과 검사 요청. 판정용 재료와 실행 인자는 지금처럼 가리기 전 값을 쓴다.
   - 자식 permission environment도 같은 집합을 이어받는다. 승인 답의 환경 재부착 경로에서도 전달값과 가리기값이 어긋나지 않는다.
   - 파일 재읽기 실패 때 기존 마지막 값 기억과 `env_file_current = false`, classifier 중지를 유지한다. 시작 때 보관한 자동 전달값이 있다는 이유로 이 실패를 정상으로 바꾸지 않는다.
   - 새 전달 자료의 오류·`Debug`·진단은 이름만 남긴다. 실제 exec stdout/stderr, 일반 assistant provider 입력·transcript·로그를 모두 가리는 기능은 추가하지 않는다. 기존 redaction과 결과 검사는 유지하되 전 표면 유출 방지라고 주장하지 않는다.
6. **기능 문서:** `docs/USAGE.md`의 “runtime이 `.env.json` 값을 환경에 내보내지 않는다”는 일반 설명을 좁힌다. runtime 자체의 process 환경은 그대로이고 선언 키만 exec 자식에 들어간다는 구분을 적는다. 기존 exec env 순서와 재시작 필요, raw output 한계도 이 PRD의 실행 변경과 함께 맞춘다. 000의 “자동 전달 미구현” 경계는 실제 실행 시험이 통과한 뒤 지운다. 다섯 스킬의 설정 절차와 정리된 사용자 절은 020 몫이다.

## 결정과 이유

- 실행 문맥으로 전달한다. registry를 세션마다 고치거나 `std::env::set_var`를 쓰면 동시에 도는 작업과 browser 등 다른 자식에 값이 섞인다.
- 시작 때 파일을 재사용한다. config env는 시작 때 풀었는데 자동 전달만 다른 시점의 값을 쓰는 혼합을 피한다. 즉시 키 회전은 새 기능으로 넣지 않는다.
- `SecretValues`에는 실제 전달값을 더한다. 파일 값만 가리면 process가 이긴 키와 명시 env가 이긴 키는 classifier에 그대로 갈 수 있다.
- workflow는 exec 활성화를 넓히지 않는다. 환경 전달 버그를 고치는 것과 workflow의 실행 권한을 바꾸는 것은 별개다.

## Non Scope

- 새 env 설정 키, 전역 process 환경 변경, 파일 자동 저장, provider·MCP·browser·plugin env 확대.
- 키 hot reload·실행 중 회수·역사적 모든 키 값 저장, 일반 transcript/log 전면 redaction.
- 000의 선언 문법 확대, 020의 다섯 스킬 설정 절차, 새로운 자동화 mode나 workflow exec enable.

## Parent Requirement Mapping

- 054 소유 범위 3·4·6과 7 중 자기 기능 문서를 소유한다.
- 앞선 계약 변경은 010, 011·024, 048의 exec 전달·값 가리기 행이다. 049의 빈 환경과 053의 승인 규칙은 유지한다.
- 독립 종료는 실제 자식 실행, 경로별 상속, 값 가리기 및 자기 문서의 일치다. 다섯 SKILL.md가 아직 옛 안내여도 그것을 010 완료로 주장하지 않고 020에 남긴다.

## 반드시 지킬 것

- 원천에는 여러 키가 있어도 새로 전달하는 map에는 활성 선언 키만 있다.
- main·child·cron의 값 선택과 missing 표시는 같고, 다른 실행의 선택이 섞이지 않는다.
- 원본 config·실행 snapshot·durable 저장 자료는 값 map의 저장소가 아니다.
- browser/viewer는 지금처럼 빈 환경과 자기 허용 목록으로 시작한다.
- 키 보유 여부는 명령 승인·sandbox 준비 여부를 대신하지 않는다.

## Acceptance Criteria

1. config의 env map을 비운 채 임시 `.env.json`에 가짜 `NOTION_API_KEY`와 선언하지 않은 canary를 둔다. production main turn에서 스킬 본문을 먼저 읽지 않고 exec로 키 존재·예상값 일치를 확인하면 성공하고, 선언하지 않은 canary는 없다. 값 자체는 출력하지 않고 성공 표시만 낸다. top-level/exec env가 있으면 그 값이 이기는 대표 충돌 하나를 같은 시험에 더한다.
2. 실제 일반 subagent exec와 cron·heartbeat의 production agent 자동화 경로가 같은 canary 존재를 확인한다. cron은 workspace 안 명령에 이미 유효한 승인 조건을 사용한다. 권한을 새로 우회하지 않는다. 다른 세션·workspace의 활성 스킬은 그 실행의 환경에 섞이지 않는다.
3. workflow의 별도 설정 생성에서 선언 자료·명시 env가 유지된다. 실제 live workflow에서 exec가 꺼진 scope는 계속 실행되지 않는다. 기존 worktree workflow 시험(`crates/shacs-core/tests/runtime_workflow.rs:832-887`, 지금은 `write_file`만 쓰고 `enable_exec`는 꺼져 있다)을 확장해 시험 입력에서만 exec와 필요한 허용 조건을 켜고, 자식이 같은 값을 확인한다. native와 sandbox/fallback은 같은 최종 map을 받고 부모 전체 환경을 상속하지 않는다.
4. 선언된 이름만 process 우선·빈 process 파일 fallback으로 전달된다. 명시 빈 값, reserved 네 이름, 기존 `allowed_env_keys`와 `path_append`의 동작이 보존된다. 파일 변경·삭제 후 같은 runtime은 시작 때 값을 쓰고, 새 runtime은 새 값 또는 missing을 쓴다.
5. actual exec output에 의도적으로 넣은 **가짜** 8자 이상 canary가 결과 검사 classifier 요청에서 가려진다. process 우선값과 명시 env 우선값도 가려지고, 새 상태·diagnostics·직렬화 snapshot/envelope/checkpoint에 원문이 없다. 일반 tool 결과의 원문까지 없어졌다는 assert는 추가하지 않는다. 파일 재읽기 실패 시 classifier를 부르지 않는 기존 시험을 유지한다.
6. browser/viewer의 실제 child-command 환경에 canary가 없다. 기본 exec allowlist·환경 상속 시험은 유지된다. 값 전달과 무관한 browser 로그인/화면 캡처를 새로 시험하지 않는다.
7. fmt(Google 옵션) → workspace clippy `-D warnings` → 변경한 `shacs-core`·`shacs-cli`와 필요시 `shacs-config` package의 `cargo test -q`가 통과한다. 전체 workspace 테스트는 별도 승인 없이는 실행하지 않는다.

## 최소 시험과 구현 위치

- 000이 값 선택을 소유하므로 그 조합 표를 복제하지 않는다. `crates/shacs-core/tests/tools.rs:1708`의 `exec_tool_injects_configured_env_after_allowed_process_env`에 자동 전달층·명시 override 확인을 더한다.
- CLI 기존 `adapter_wires_exec_env_to_context_and_subagents`(`src/lib.rs:28235-28320` 부근)의 배선 확인을 확장하고, production main·자식·자동화의 실제 canary 실행은 `crates/shacs-cli/tests/it/`에 작은 통합 모듈 하나로 둔다. 가짜 provider와 임시 data/workspace를 쓰고 `tests/it/main.rs`에 등록한다. 등록 표를 흉내 낸 수동 `ExecConfig`만으로 production 연결 완료를 주장하지 않는다.
- workflow는 기존 worktree workflow 시험의 입력에서만 exec를 켜고 canary 확인을 얹는다. 새 shell 권한이나 시험 전용 공개 API를 만들지 않는다. actual main의 exec off도 기존 scope assertion을 유지해 확인한다.
- 가리기는 `crates/shacs-core/tests/it/spec048_auto_mode/env_masking.rs`와 `result_checks.rs`의 기존 canary 시험을 확장한다. 핵심 추가 회귀는 process/명시 값이 파일 값과 다를 때다. 모든 permission mode를 새로 조합해 반복하지 않는다.
- sandbox와 browser의 기존 child-command 환경 시험에 canary 부재/전달 확인을 붙인다. 공식 Docker에 bwrap이 없다는 사실을 sandbox 성공으로 기록하지 않는다. Docker 수동 확인을 했다면 별도의 가짜 키·임시 data를 쓰고 실행 여부를 기록하며 실제 서비스 키는 요구하지 않는다.

## 구현 메모

- `shacs-config::RuntimeEnvironment`는 시작 시 process 읽기 결과와 config 보간에 사용한 같은 `EnvFile`을 보관한다. `EnvSource::snapshot`을 통해 환경 전체를 한 번 읽고, 기존 config 보간기를 그 읽기 결과로 실행한다. 자료는 직렬화하지 않는 `ConfigBundle`을 거쳐 adapter로 이동한다. `Config.env`와 `tools.exec.env`에는 자동 전달값을 넣지 않는다. 직접 만든 bundle은 adapter 생성 때 같은 초기화를 거친다.
- `ContextBuilder::select_skills`는 PRD 000 registry와 `resolve_skill_credentials`를 사용해 실행 시작의 본문·상태·값을 함께 고정한다. 기존 우선순위 표를 다시 구현하지 않는다. 자식의 workspace가 바뀌어도 선택된 본문과 선언을 유지한다. 다음 실행은 선언을 다시 선택하지만 값은 같은 시작 원천을 사용한다.
- `SkillEnvironment`는 이름만 출력하는, 직렬화하지 않는 공유 메모리 자료다. `AgentLoopConfig` → `ToolExecutionContext` → `ToolCallExecutionContext`로 전달하며 저장되는 문맥에는 `serde(skip)`을 적용한다. 일반 자식과 workflow의 별도 설정도 이를 이어받는다. 승인 재개 경로는 저장된 승인 정보를 유지하고 환경만 현재 runtime 원천으로 다시 연결한다.
- `ExecTool::build_env`의 기존 allowlist 뒤와 명시 env 앞에 실행 문맥의 값을 더한다. process gate와 controlled child의 `inherit_env = false`, path append와 sandbox 정책은 바꾸지 않는다. 공유 registry나 process 전역 환경을 변경하지 않는다.
- `SecretValues::extend`는 기존 파일 값과 실제 전달값을 이름으로 덮어쓰지 않고 함께 보존한다. 동일 값의 이름순 대표, 긴 값 우선, 한 번 치환, 8자 기준을 유지한다. 자식도 같은 permission environment를 받는다. 파일 재읽기 실패의 classifier 중지 조건은 그대로다.
- main workflow는 계속 exec가 꺼져 있다. 실제 exec 시험은 기존 worktree workflow 시험 입력에서만 허용한다. sandbox의 wrapped 경로는 가짜 bwrap 실행 파일로 환경 전달을 확인하며 실제 Linux sandbox 격리 성공을 뜻하지 않는다.
- 사용자 지시에 따라 `docs/USAGE.md`, 다섯 `SKILL.md`, PRD 000 및 이전 스펙은 수정하지 않는다. 이 문서의 Scope 6이 요구한 사용법 갱신은 이번 작업에서 제외하며 PRD 020에 남긴다. 값이 없는 상태 안내의 “자동 전달 미구현” 문구만 실행 구현에 맞춰 제거한다. 일반 stdout/stderr·provider 입력·transcript·로그 전면 가리기는 추가하지 않는다.

인수 조건별 시험:

| 조건 | 시험 |
|---|---|
| 1 production main, 스킬 사전 읽기 없음, 선언하지 않은 키 제외 | `production_main_and_spawn_use_startup_values_without_reading_skills` |
| 2 일반 자식, 다른 workspace 선택 분리, cron·heartbeat | 위 production 시험, `production_cron_and_heartbeat_exec_receive_declared_startup_values` |
| 3 workflow 설정 보존·기존 exec off, 허용된 worktree exec | `automatic_workflows_share_a_durable_store_without_reporting_children_to_the_conversation`, `isolated_worktree_live_workflow_writes_only_child_tree_and_hands_off_diff` |
| 3 native·fallback·wrapped·준비 확인의 환경 | `native_fallback_reports_unsupported_without_claiming_applied_adapter`, `active_fact_requires_probe_and_wrapped_execution` |
| 4 process 우선·빈 process fallback·명시 override·명시 빈 값·예약 이름 | `production_exec_uses_process_fallback_and_explicit_winners`, 기존 `credential_resolution_obeys_precedence_and_excludes_reserved_names` |
| 4 allowlist·path append, 실행별 선언 고정과 재시작 수명 | `exec_tool_injects_configured_env_after_allowed_process_env`, `selected_skills_freeze_bodies_and_values_until_the_next_run`, production main 시험 |
| 5 실제 exec 결과 검사, process/명시 전달값, 원문 tool output 유지 | `env_values_are_masked_in_the_check_only` |
| 5 같은 이름의 옛 값·새 값, 중복 대표, 공개 action과 저장 문맥 | `a_value_under_two_names_keeps_the_first_name`, `values_are_masked_in_what_is_shown_and_the_tool_gets_the_raw_value`, production main의 session metadata 검사 |
| 5 파일 재읽기 실패 시 기존 classifier 중지 | 기존 `a_turn_without_current_env_values_does_not_call_the_classifier`, `an_answer_after_env_json_broke_runs_the_call_with_the_last_read_values`, `a_subagent_without_current_values_has_no_classifier_and_gets_the_note` |
| 6 browser/viewer 제외 | `spec049_browser_viewer`의 기존 child-command 환경 검사에 `NOTION_API_KEY` canary를 추가 |

시험의 허용 조건은 기존 권한 체계를 사용한다. production main은 사용자 allow가 설정된 auto 모드이며 공개 embedded-runtime API로 테스트용 격리 확인 입력을 준다. cron·heartbeat는 테스트의 격리 확인 입력과 정확한 명령 allow를 사용한다. cron에는 실제 저장한 job·grant가 있다. workflow 시험도 격리 확인 입력과 정확한 명령 allow를 제공한다. 실제 Docker 실행이나 host가 격리되었다는 증거로 해석하지 않는다. 처음의 production 시험은 격리 확인 입력이 없어 `StaticAskRequired` 및 process gate `Denied`로 실패했다. 구현의 권한 검사를 바꾸지 않고 시험의 기존 허용 조건을 바로잡았다.

## 검증 기록(2026-11-18)

- Google 옵션을 붙인 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true` 통과(exit 0).
- 요청한 `cargo clippy --manifest-path crates/Cargo.toml -p shacs-skills -p shacs-core -p shacs-cli -p shacs-config --all-targets --no-deps -- -D warnings`는 기존 `shacs-core/src/runtime/memory.rs:1078`의 `needless_range_loop`로 exit 101. 해당 파일은 수정하지 않았다.
- 위 경고 때문에 중단된 나머지 package를 `cargo clippy --manifest-path crates/Cargo.toml -p shacs-skills -p shacs-cli -p shacs-config --all-targets --no-deps -- -D warnings`로 확인했고 통과했다(exit 0).

종료까지 확인한 시험:

| 명령 | 결과 |
|---|---|
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec054_exec_environment::` | 2 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --lib -q` | 기존 478 passed. 새 자동화 시험 1개는 fixture의 격리 backend 이름 때문에 실패해 exit 101; 아래 한 시험만 고쳐 다시 실행 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --lib -q production_cron_and_heartbeat_exec_receive_declared_startup_values` | 1 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q runtime::context::tests::` | 6 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q runtime::agent_loop::tests::` | 25 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q runtime::auto_mode::secret_values::tests::` | 5 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q tools::shell::execution::tests::` | 1 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q spec048_auto_mode::` | 99 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime --test runtime_agent --test runtime_loop --test runtime_workflow --test tools --test spec030_sandbox_adapter -q` | `runtime` 39, `runtime_agent` 65, `runtime_loop` 198, `runtime_workflow` 기존 39 passed. 확장한 workflow 시험은 입력의 `proc_exec` 금지 상한 때문에 실패해 exit 101. 이후 실행 파일은 이 명령에서 실행하지 않음 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime_workflow -q isolated_worktree_live_workflow_writes_only_child_tree_and_hands_off_diff` | 시험 입력에서 exec 금지를 해제하고 process gate의 필수 permission ceiling을 명시한 후 1 passed, exit 0. 기존 workflow 39개와 합쳐 40개 검증 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test tools --test spec030_sandbox_adapter -q` | sandbox 7 + tools 86 passed, exit 0. 실제 Linux bwrap 필수 실행 lane은 설정하지 않음 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-skills -q` | 단위 24 + 통합 2 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-config --lib -q` | 59 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec049_browser_registry::` | 29 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec049_browser_viewer::` | 4 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec031_activation_production_builder::` | 1 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec052_discord_connect::` | 13 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test spec030_javascript_tool_before_host --test spec030_mcp_startup_facts --test spec030_provider_credential_construction --test spec030_provider_credential_invocation --test spec030_trusted_handler_registry --test spec030_user_data_plugin -q` | 1 + 1 + 2 + 3 + 1 + 2 passed, exit 0 |

초기 `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec054`의 PRD 000 readiness 시험 1개도 통과했다. 같은 실행에서 새 exec 시험 2개는 위에서 설명한 승인 fixture 문제로 실패했으며, 수정 후 첫 행의 명령으로 두 시험 모두 통과했다. 이미 통과한 시험은 같은 코드에 대해 반복 실행하지 않는다.

마지막 workflow fixture 수정 뒤에도 fmt 검사 → 지정한 네 package clippy → 해당 workflow 시험 순서를 지켰다. 각각 exit 0 → 기존 `memory.rs:1078` 경고의 exit 101 → 1 passed/exit 0이다. 최종 변경의 관련 시험은 합계 1,195개가 통과했으며 위의 초기 실패 두 명령 자체를 성공으로 기록하지 않는다. `git diff --check`도 통과했다. 실제 서비스 키, 외부 API, 전체 workspace 테스트, Docker, build/clean, 저장소 커밋은 실행하지 않았다.

## 변경 파일 범위

- 시작 원천: `crates/shacs-config/src/{lib.rs,env_file.rs,runtime_environment.rs}`.
- 선택된 값과 안내: `crates/shacs-skills/src/{lib.rs,credential_resolution.rs,skill_environment.rs}`. PRD 000의 선언 파서와 우선순위 표는 재작성하지 않았다.
- 실행별 선택·자식·승인 재개: `crates/shacs-core/src/runtime/{context.rs,agent_loop.rs,subagent.rs,tool_execution.rs,tool_execution_provider.rs}`.
- 실제 exec와 마스킹: `crates/shacs-core/src/tools/{base.rs,shell/guard.rs,shell/execution.rs}`, `crates/shacs-core/src/runtime/auto_mode/{permission_environment.rs,secret_values.rs,secret_values_tests.rs}`.
- production adapter: `crates/shacs-cli/src/lib.rs`. `src/auto_mode_setup.rs`는 새 bundle 필드의 기본값만 추가했다.
- 주요 실행 시험: `crates/shacs-cli/tests/it/{main.rs,spec054_exec_environment.rs,spec054_exec_environment_support.rs,spec049_browser_viewer.rs}`, `crates/shacs-cli/src/automation_worker/outside_access_tests.rs`, `crates/shacs-core/tests/{tools.rs,runtime_workflow.rs,spec030_sandbox_adapter.rs}`, `crates/shacs-core/tests/it/spec048_auto_mode/{env_masking.rs,result_checks.rs}`.
- 기존 생성자·테스트 호환: core의 `tests/{runtime.rs,runtime_loop.rs}`; CLI의 `src/{discord_connect_cli_tests.rs,memory_cli_tests.rs,run_permission_mode_tests.rs,surface_approval_worker/fixture/support.rs}`, `tests/it/{spec031_activation_production_builder/support.rs,spec049_browser_registry.rs,spec052_discord_connect.rs}`, `tests/{spec030_javascript_tool_before_host/support.rs,spec030_mcp_startup_facts.rs,spec030_provider_credential_construction.rs,spec030_provider_credential_invocation.rs,spec030_trusted_handler_registry.rs,spec030_user_data_plugin.rs}`.
- 문서는 이 PRD만 수정했다. 시작 시 이미 있던 PRD 000 미커밋 변경, manifest/lockfile 변경 및 스펙 목록 변경은 보존했다.

남은 것은 독립 리뷰와 PRD 020의 사용자 안내다. 이번 구현에 필요한 미결정 사항은 없다.
