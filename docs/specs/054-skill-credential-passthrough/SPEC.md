# 054. 스킬이 선언한 키를 명령에 자동으로 넘기기 구현 명세

Status: Open (implemented, closure blocked)

Consumes: 005 Markdown 스킬 발견·우선순위·descriptor·context 주입·`skills list|show`, 008 config의 top-level `env`·`tools.exec.env`, 010 `exec` 환경 허용 목록과 명령 실행 검사, 011 하위 작업의 실행 설정, 012·033 cron·heartbeat의 agent 실행, 024 workflow 자식, 025 enabled plugin의 skill root, 030 trusted local resource와 선택적 sandbox, 048 `.env.json` 형식·`${이름}` 해석·`SecretValues`와 classifier 입력 가리기, 049 Chromium·화면 보기 자식의 빈 환경, 052 Docker data 폴더 mount, 053 workspace 밖 접근 판정.

## 목적과 완료 경계

사용자는 data 폴더의 `.env.json`에 `{"NOTION_API_KEY": "ntn_..."}`를 적었는데, Notion 스킬을 읽고 실행한 명령에서는 그 키를 쓸 수 없다. 지금 `.env.json`은 config의 `${이름}`을 푸는 데만 쓰이고, `exec`는 별도의 환경을 만들기 때문이다. 스킬 문서가 말하는 `~/.shacs-bot/.env` 자동 전달도 구현되어 있지 않다.

이 스펙은 사용자가 고른 Hermes 방식으로 바꾼다(사용자 결정 2026-11-18). **활성 스킬이 선언한 이름에 값이 있으면 모든 `exec`에 자동으로 넘긴다.** 사용자는 `config.json`에 같은 이름을 다시 적지 않는다. 그 세션에서 스킬 본문을 먼저 읽었는지는 조건이 아니다. 활성 스킬 전체를 대상으로 하는 범위는 사용자가 별도로 답하지 않은 권장 기본안이며, 아래의 나머지 기본 결정과 함께 확인할 수 있게 적는다.

완료되면 사용자는 다음을 경험한다.

- data 폴더 `.env.json`에 키를 넣고 runtime을 다시 시작하면 본 대화, 하위 작업, cron·heartbeat가 실행하는 `exec`에서 같은 키를 쓴다. 실행 도구가 허용된 workflow 자식도 같다.
- 값은 비어 있지 않은 process 환경 변수, `.env.json` 순으로 찾는다. 사용자가 적은 top-level `env`와 `tools.exec.env`가 자동 전달보다 우선하고, 두 설정끼리는 `tools.exec.env`가 이긴다.
- 키가 없어도 스킬은 목록에서 사라지거나 로드 오류가 되지 않는다. `skills list`, `skills show <이름>`, 어시스턴트의 스킬 목록에 없는 **이름만** 보인다.
- 어시스턴트는 사용자가 실제 data 폴더의 `.env.json`을 직접 고치도록 안내한다. Docker에서는 host에서 mount한 폴더와 container 안의 폴더를 구분한다. 채팅에 비밀 값을 붙여 달라고 하지 않는다.
- Chromium·Xvfb·x11vnc·websockify, MCP, provider, plugin command 등 다른 실행 종류에 키를 덧붙이지 않는다.

이 스펙은 현재 구현된 경로만 소비하고 다른 스펙의 남은 구현·종료를 기다리지 않는다. 아래 세 PRD의 구현과 수용 조건으로 단독 종료한다. 이 문서 세트 작성은 기능 구현 완료를 뜻하지 않는다.

## 지금 상태와 근거

줄 번호는 2026-11-18 작업 트리 기준이다.

| 자리 | 확인한 동작 |
|---|---|
| `crates/shacs-config/src/env_file.rs:13-28`, `:81-123` | `.env.json`은 config 파일 옆에 있다. 이름은 `[A-Z_][A-Z0-9_]*`, 값은 문자열이며 중복 이름은 오류다. 파일이 없으면 빈 목록이다 |
| `crates/shacs-config/src/lib.rs:1303-1342`, `crates/shacs-cli/src/lib.rs:3155-3162` | config 보간을 풀며 `EnvFile`을 받고, 시작 때 읽은 파일은 `remember_env_file`에 기억한다 |
| `crates/shacs-core/src/tools/shell/guard.rs:31-52`, `tools/shell/process.rs:15-35` | `HOME`, `LANG`, `TERM`, `PATH` → `allowed_env_keys`의 process 값 → `ExecConfig.env` 순이다. 자식은 `inherit_env = false`로 시작한다 |
| `crates/shacs-cli/src/lib.rs:22052-22056`, `spec035_cli_media/tool_registry.rs:108-129` | `configured_exec_env`는 top-level `env` 뒤에 `tools.exec.env`를 덮고 본 대화의 `ExecTool`에 넣는다 |
| `crates/shacs-cli/src/lib.rs:19816`, `:21108-21134`, `crates/shacs-core/src/runtime/subagent.rs:909-911`, `:1535-1543` | adapter의 `exec_env`를 하위 작업의 명령과 스킬 context에 복사한다 |
| `crates/shacs-cli/src/automation_worker.rs:329-367`, `crates/shacs-cli/src/lib.rs:21137-21139` | cron 등 agent 자동화는 같은 adapter의 agent turn으로 들어오고 heartbeat도 자동화에 넣는다 |
| `crates/shacs-core/src/runtime/agent_loop.rs:3236-3271`, `runtime/workflow.rs:530-546` | workflow는 별도로 `SubagentExecutionConfig`를 만들며 지금 이 자리에는 exec 환경 복사가 없다. 기본 `enable_exec = false`이고 worktree가 생겼다고 자동으로 exec를 켜지는 않는다 |
| `crates/shacs-core/src/runtime/sandbox_adapter/execute.rs:13-47`, `:75-100` | native·wrapped 실행과 bwrap 준비 확인은 전달받은 command 환경을 사용한다 |
| `crates/shacs-skills/src/lib.rs:736-782`, `:796-820` | frontmatter를 줄별로 평평하게 읽는다. `prerequisites` 아래 `env_vars`의 부모를 잃고, Hermes 항목의 `name`도 최상위 이름과 충돌할 수 있다. `requires.env`와 nanobot·openclaw 표기는 requirements 글로만 추출한다 |
| `crates/shacs-core/src/runtime/context.rs:215-294`, `:574-628` | context는 registry와 따로 파일을 고르고 `requires.env`를 process·설정 값으로 검사한다. 누락은 `unavailable`이 되고 `always` 주입에서도 빠진다(`:297-313`). CLI registry의 표시 전용 처리와 다르다 |
| `crates/shacs-cli/src/lib.rs:7887-8001`, `:8825-8892` | 목록·inspect의 실제 명령은 `skills list`와 `skills show`다. 지금 누락 키는 계산하지 않는다 |
| `crates/shacs-cli/src/auto_mode_cli.rs:20-50`, `:66-96`, `crates/shacs-core/src/runtime/auto_mode/secret_values.rs:9-50`, `result_check.rs:69-93` | permission environment는 turn마다 파일을 다시 읽고, 실패하면 마지막 값으로 가리되 classifier를 쓰지 않는다. 8자 이상인 파일 값만 정확히 가린다. 결과 검사 요청은 redaction 뒤 값을 가린다 |
| `docs/USAGE.md:97`, `:111`, `docker-compose.yml:8` | 일반 설명은 파일 값을 환경에 내보내지 않는다고 한다. 화면 보기 자식도 빈 환경으로 시작한다. 기본 data mount는 host `~/.shacs-bot` → container `/home/shacs/.shacs-bot`이다 |

키를 선언한 내장 원본은 `crates/shacs-skills/airtable/SKILL.md:8-9`(`AIRTABLE_API_KEY`), `gif-search/SKILL.md:8-9`(`TENOR_API_KEY`), `linear/SKILL.md:8-9`(`LINEAR_API_KEY`), `notion/SKILL.md:8-9`(`NOTION_API_KEY`), `teams-meeting-pipeline/SKILL.md:7-8`(Graph의 세 이름)이다. Teams는 `deferred_builtins.txt:18`에 있고 본문 앞에도 `disabled: true`가 있어 기본 활성 대상이 아니다. Linear의 현재 안내는 `.env`라는 파일명을 직접 쓰지 않고 `config/env`라고만 한다(`:26`). 다섯 파일 모두 실제 `.env.json` 안내로 통일하는 일은 PRD 020이 소유한다.

참고한 외부 계약은 Hermes 공식 Skills 문서의 “Secure Setup on Load” 절이다(`https://hermes-agent.nousresearch.com/docs/user-guide/features/skills`, 2026-11-18 확인). `required_environment_variables`의 `name`·`prompt`·`help` 선언, 선언 키의 terminal·execute_code 전달, 메시지 채널에서 비밀을 받지 않는 원칙을 소비한다. Hermes의 로컬 CLI 비밀 입력 단계와 setup 명령까지 복제하지 않는다.

## 소유 범위

1. **키 선언:** `prerequisites.env_vars`, Hermes `required_environment_variables`, 기존 `requires.env`, `metadata.nanobot.requires.env`, `metadata.openclaw.requires.env`를 같은 이름 목록으로 정규화한다. 중첩 구조와 dotted 표기, 기존 inline JSON metadata를 구분해 읽는다. 본문에 적힌 `$NAME`, 부모 없는 `env_vars`, `prompt`·`help`의 글에서는 이름을 추측하지 않는다. 정확한 입력 형식·오류는 PRD 000이 정한다.
2. **활성 스킬:**
   - registry가 정한 `Active` 후보 중 config의 `agents.defaults.disabledSkills`와 frontmatter `disabled: true`로 꺼지지 않은 것이 대상이다. `Shadowed`, `Conflicted`, `Malformed` 후보는 제외한다. 같은 이름의 키를 모든 사본에서 합치지 않는다.
   - 005의 우선순위(`VirtualBuiltin < MaterializedBuiltin < UserGlobal < WorkspaceLegacy < WorkspaceLocal < PluginProvided`)를 그대로 쓰고, context와 키 전달도 같은 선택 결과를 소비하게 한다. disabled인 최종 후보를 빼면서 낮은 후보를 자동으로 되살리지 않는다.
   - builtin과 data 폴더 사용자 스킬, 현재 workspace의 Markdown 스킬을 포함한다. workspace는 사용자가 선택해 신뢰하는 현재 실행 범위로 취급한다. 기존 Markdown 활성화에 없는 별도 승인 장치를 만들지 않는다. 실행 중 새 파일을 발견했다는 이유로 executable plugin을 활성화하지 않는다.
   - plugin은 현재 `enabled_plugin_skill_roots`가 고른 enabled plugin의 skill root만 포함한다(`crates/shacs-cli/src/lib.rs:22383-22399`). disabled plugin에서 읽어 오지 않는다.
   - deferred builtin은 virtual·materialized 모두 제외한다. 같은 이름으로 사용자가 별도로 만든 workspace/user 스킬은 일반 우선순위와 disabled 조건을 지난 경우 포함한다. 이는 기존 deferred override 시험(`crates/shacs-skills/src/lib.rs:1319-1344`)과 같다. Teams를 활성화하거나 지원 기능을 더하지 않는다.
   - 명령 프로그램(`requires.bins`) 부재와 키 부재는 활성 여부와 구분한다. 키가 없다는 이유만으로 로드·목록·`always` 주입을 막지 않는다. 프로그램 부재의 기존 동작은 유지한다.
3. **값과 우선순위:**
   - 선언 이름마다 비어 있지 않은 process 값 → `.env.json` 값 순이다. 둘 다 없거나 결과가 빈 문자열이면 전달하지 않고 missing으로 본다. 공백은 임의로 trim하지 않는다. `${…}`를 다시 풀거나 셸로 실행하지 않는다.
   - 최종 자식 환경은 기본 네 이름 → 기존 `allowed_env_keys` → 스킬 자동 전달 → top-level `env` → `tools.exec.env` 순이다. `path_append`는 그 뒤 기존 방식으로 붙인다. 명시 설정의 빈 값도 이기며, 그 키의 상태는 missing이다. 이때 `.env.json`에 넣으라는 안내만 하지 않고 명시 설정이 빈 값으로 덮고 있다는 이름 기준 이유를 보인다.
   - 자동 전달에서만 `HOME`, `PATH`, `LANG`, `TERM`을 예약 이름으로 제외한다. 기존 환경 기초를 스킬 선언이 바꾸지 않게 한다. 예약 이름은 missing 안내 대신 제외 이유를 보이고, 사용자가 직접 적는 기존 env 설정의 뜻은 바꾸지 않는다.
   - provider 이름(`OPENAI_API_KEY` 등)을 따로 막지 않는다. 활성 스킬이 **정확히 선언한** 이름만 포함하므로 사용자 자신의 API 호출 스킬도 쓸 수 있다. 선언하지 않은 provider·channel·viewer 이름과 그 밖의 process·파일 값은 새로 전달하지 않는다. 이미 `allowed_env_keys`나 명시 env로 전달하던 이름은 그대로다.
4. **수명과 실행 경로:**
   - 값의 원천은 runtime 시작 때 config를 푸는 데 쓴 process/`.env.json`의 읽기 결과다. 같은 시작에서 다시 읽어 다른 값으로 조합하지 않는다. 자동 전달용 파일 값·process 값·명시 env를 바꾸려면 runtime을 재시작한다. config 없이 스킬 키만 추가한 경우도 같다.
   - 각 본 대화 turn·cron 한 번·heartbeat 한 번의 시작에서 활성 스킬을 선택하고, 시작 때 읽은 값에서 선언 키만 뽑아 그 실행의 환경 자료를 만든다. 그 실행 중 선언 변경은 적용하지 않으며 다음 시작에서 다시 고른다. 자식은 부모가 고른 이름과 값을 이어받고 worktree에서 다른 키를 더 발견해 넓히지 않는다.
   - 본 대화·채널·local API의 `exec`, 일반 하위 작업, cron·heartbeat의 agent 실행, 실행 도구가 허용된 workflow 자식에 적용한다. exec가 꺼지거나 도구 목록에서 빠진 자식을 새로 켜지 않는다. agent 없이 실행하는 자동화 script와 plugin/MCP 프로세스는 대상이 아니다.
   - 값은 config, workflow checkpoint, spawn envelope, `ExecutionSnapshot`, journal에 저장하지 않는 메모리 자료다. 부모의 process 환경에 `set_var`로 넣지 않는다. 공유 registry의 환경을 turn마다 덮어 다른 세션과 섞지도 않는다. 기존 실행 문맥에서 `exec`에만 넘긴다.
5. **누락 키 안내:** CLI `skills list`·`skills show`와 모델의 스킬 요약·선택된 스킬 안내가 같은 정규화된 이름과 상태를 쓴다. `required`, `missing`, 자동 전달에서 제외한 예약 이름, 비활성 이유를 값 없이 보인다. CLI 조회는 그 명령을 시작한 현재 파일·process를 읽으므로 실행 중 gateway의 옛 값을 검증한 것처럼 말하지 않고 재시작 필요를 적는다. 어시스턴트에게는 `.env.json`의 실제 data 경로와 직접 편집·재시작 안내를 준다. 비밀을 채팅·질문 tool·승인 질문으로 받거나 자동으로 파일을 읽어 확인하라고 하지 않는다.
6. **비밀 값과 관측:**
   - 시작 때 보관한 값 전체를 prompt·descriptor에 넣지 않는다. 전달 자료의 `Debug`, 새 오류·상태·진단은 이름만 보인다. 값, 접두부, 길이, hash를 상태로 내지 않는다.
   - `SecretValues`는 기존 turn별 `.env.json` 값에 **이번 실행에서 선언 키에 실제로 전달하는 최종 값**을 더한다. process 값과 명시 env가 이긴 값, 재시작 전까지 남는 시작 때 값도 포함한다. 같은 이름이 다른 두 값을 가지면 가리기 목록에서는 둘 다 남긴다. 8자 기준·긴 값 우선·한 번 치환은 048 그대로다.
   - 추가한 값도 기존 048의 자리(action의 밖에 보이는 인자, 승인·permission 진단, classifier transcript와 결과 검사 요청)에서 가린다. 파일 재읽기 실패 시 기존 마지막 값 가리기와 classifier 중지는 유지한다. exec의 시작 때 값이 남아 있더라도 classifier를 다시 켜는 근거가 되지 않는다.
   - `exec`가 출력한 원문을 일반 assistant provider·세션 기록·로그 전부에서 없애는 새 보장은 만들지 않는다. shell이 비밀을 출력하거나 변환·인코딩하면 원문이 남거나 밖으로 나갈 수 있다. 030의 trusted-local 모델과 `docs/TRUST-MODEL-DECISION.md:17-21`의 raw content 고지를 유지하고, 사용법에도 키를 출력하지 말라고 적는다. 048의 한정된 가리기를 전 표면 비밀 보호라고 설명하지 않는다.
7. **사용자 문서와 내장 안내:** PRD 020이 `docs/USAGE.md`에 스킬 키 절을 만들고 다섯 SKILL.md의 설정 안내를 고친다. `.env.json`은 문자열 JSON 객체이며 `KEY=value`·`export` 형식이 아니다. `.env`를 새로 읽거나 자동 이전하지 않는다. 기존 사용자가 고친 materialized 스킬을 덮어쓰지 않는다.

## 결정과 이유

- **전체 활성 스킬의 선언을 쓴다.** 본문을 읽은 기록으로 권한을 만드는 별도 상태가 없어도 하위 작업과 cron이 같은 환경을 쓸 수 있다. 대신 무관한 `exec`도 선언 키를 받을 수 있다. 사용자가 고른 trusted-local 실행 모델의 비용으로 공개한다.
- **기존 env requirements도 포함한다.** 이미 필요한 환경 이름이라는 뜻이고, 같은 키를 새 필드로 다시 적게 할 이유가 없다. 기존 표시 전용 계약의 변경은 아래에 기록한다.
- **registry와 context의 선택을 맞춘다.** 지금 둘의 이름·우선순위가 달라 표시된 스킬과 키를 내준 스킬이 달라질 수 있다. 원본 선택은 005 registry에 한 번 맡기고 disabled·누락 상태를 따로 얹는다.
- **키 값은 재시작 때 바뀐다.** 현재 `configured_exec_env`의 수명과 Docker 운영 방법을 유지한다. 키 회전·실행 도중 파일 변경을 새 감시 장치로 처리하지 않는다. 스킬 선언은 실행 시작마다 선택하되 값은 시작 snapshot에서 찾는다.
- **예약 이름은 네 개로 한정한다.** 이는 자식 환경의 기초 설정 보호이며 악성 스킬 격리가 아니다. `LD_PRELOAD` 같은 모든 실행 제어 변수를 막는 별도 정책이나 provider 이름 목록은 만들지 않는다. 신뢰한 스킬은 이미 shell을 실행할 수 있다.
- **파일 접근 승인과 키 전달은 다르다.** 키를 선언했다고 `.env.json` 읽기 승인이 생기는 것은 아니다. runtime이 알고 있는 값을 자식 환경에 넣는 좁은 경로만 더한다. 048·053의 비밀 파일 접근 규칙은 그대로다.

위 기본 결정(활성 범위, 기존 표기 포함, 선택 일치, 예약 이름, 재시작 수명, 제한된 값 가리기 확장)은 사용자에게 별도로 확답받은 사실로 기록하지 않는다. 구현 기본안으로 명시한 것이다.

## 기존 owner와의 경계

| Owner | 소비하는 것 | 054가 맡지 않는 것 |
|---|---|---|
| 005·025·030 | 스킬 발견·우선순위, enabled plugin root, 신뢰한 로컬 자료 | plugin 활성화·설치·격리 정책, executable skill 실행 방식 |
| 008·010·048 | env 설정, `.env.json` 검사와 config 보간 | 새 비밀 파일·키 입력 명령, provider 인증 해석, migration |
| 011·012·024·033 | 자식·workflow·agent 자동화의 실행 경로 | exec 권한 확대, 예약 승인, agent 없는 script 자동화 |
| 049·050 | 빈 환경으로 browser/viewer 시작 | 브라우저 env 전달, 로그인·viewer 비밀번호 규칙 변경 |
| 052·053 | Docker data mount, 명령·비밀 파일 접근 판정 | 설치 스크립트·Compose 수정, 파일 접근 허용 확대 |

## 앞선 스펙에서 바꾸는 것

앞선 문서는 닫힐 때의 기록으로 남기고 고치지 않는다. 새 계약은 이 절과 해당 PRD가 소유한다.

| 앞선 스펙 | 지금 계약(문서와 위치) | 바뀐 뒤 계약 | PRD |
|---|---|---|---|
| 005 | [SPEC](../005-skill-system/SPEC.md) inspect metadata(`:38-40`, `:270-276`): requirements는 읽기 전용 표시 자료이고 context는 별도 이름·availability 모델을 쓴다 | env 선언은 이름만 담은 실행 입력도 된다. context의 원본 선택·이름은 registry와 같게 하고, disabled 여부와 missing 키는 별도 상태로 표시한다. 키 누락만으로 로드·`always` 주입을 막지 않는다 | 000 |
| 010 | [SPEC](../010-host-safety-permissions-and-secrets/SPEC.md) 현재 구현 요약 2(`:56`), 현재 아키텍처 매핑 기준 3(`:74`): exec에 environment allowlist를 적용한다. 구현 `guard.rs:31-52`는 기본 네 이름·allowlist·명시 env만 합친다 | 기존 순서에 활성 스킬의 선언값을 allowlist 뒤·명시 env 앞에 더한다. 환경 전체 상속과 명령 승인 생략은 하지 않는다 | 010 |
| 011·024 | [011 SPEC](../011-subagent-runtime/SPEC.md) inherited context 절(`:156` 이하)과 시험 목록 `adapter_wires_exec_env_to_context_and_subagents`(`:239`), [024 SPEC](../024-dynamic-workflows-and-harness-orchestration/SPEC.md)의 workflow-local tool scope(`:87`). 현재 명시 env는 일반 자식에 복사되지만 workflow 설정 생성(`agent_loop.rs:3236-3271`)에는 없다 | 부모가 선택한 스킬 키와 명시 env의 최종 결과를 일반·workflow 자식에 같은 메모리 경로로 전달한다. exec 도구가 없는 자식은 계속 실행할 수 없다. envelope/checkpoint에 값은 넣지 않는다 | 010 |
| 048 | [PRD 010](../048-transcript-classifier-auto-mode/prds/010-secret-file-split.md) 범위 2(`:18-27`), 반드시 지킬 것 1(`:117`), 수용 조건 1(`:128`): config가 가리키지 않은 파일 이름은 명령·skill·MCP 환경에 들어가지 않는다. 사용자 설명은 `docs/USAGE.md:97` | 활성 스킬이 선언한 이름은 config 항목 없이 exec 자식에 들어간다. runtime 자체의 process 환경, MCP·provider·browser 자식은 바꾸지 않는다 | 010, 020 |
| 048 | 같은 PRD 범위 10(`:83-90`)과 Non Scope(`:98`): 정확한 값 가리기는 파일의 8자 이상 값에 한정하고 process·config 값은 대상이 아니다. [PRD 040](../048-transcript-classifier-auto-mode/prds/040-tool-result-check.md)은 같은 값을 결과 검사에 쓴다 | 선언 키의 실제 최종 전달값을 기존 가리기 집합에 더한다. process·명시 env 우선값과 시작 때 값을 포함한다. 적용 표면·8자 기준·raw output의 한계는 그대로다 | 010 |

049의 Chromium·viewer 빈 환경, 048·053의 비밀 파일 규칙, 052의 data mount와 재시작 운영, 030의 raw content 고지는 바꾸지 않는다.

## 반드시 지킬 것

- 새로 넘기는 이름은 활성 스킬의 유효한 선언에서만 온다. shadowed·disabled·deferred builtin의 이름, `.env.json` 전체, 부모 process 전체를 합치지 않는다.
- 명시 env는 자동 전달보다 우선한다. 빈 명시 값도 존중한다. 없는 키는 빈 문자열을 만들어 전달하지 않는다.
- 활성·누락 판정과 실행이 같은 선택 결과를 사용한다. 스킬을 읽기 전에도 전달하고, 다른 세션의 선택값이 섞이지 않는다.
- 키 누락, 선언 오류, reserved 이름 제외는 스킬 상태에 값 없이 드러나며 새 비밀 입력 질문을 만들지 않는다.
- 값이 든 메모리 자료를 직렬화하거나 새 진단·로그에 출력하지 않는다. `SecretValues`는 실제 자식에 간 값과 같은 값을 안다.
- 명령 실행 판정, `restrictToWorkspace`, cron 목록, sandbox fallback은 그대로다. 키가 있다는 사실은 실행 허가가 아니다.
- `.env.json` 파일 읽기·쓰기 승인 규칙과 browser/viewer 빈 환경은 그대로다.

## 수용 조건

1. 선언·상태: 다섯 내장 원본의 키와 Hermes 목록을 정확히 읽고, 중첩 항목의 `name`이 스킬 이름을 덮지 않는다. 기존 env requirements도 같은 결과다. 잘못된 이름·형식과 예약 이름은 값 없는 진단이 되고 자동 전달에서 제외된다. 누락 스킬이 목록·본문 로드·`always` 주입에 남는다.
2. 활성 범위: 서로 다른 키를 선언한 같은 이름의 두 스킬에서 선택된 원본의 키만 전달한다. disabled·conflicted·malformed·disabled plugin·deferred builtin은 전달하지 않는다. enabled plugin·사용자 스킬·workspace 스킬은 포함하며, Teams 원본은 계속 비활성이다.
3. 실행: config env 항목 없이 `.env.json`에만 둔 가짜 Notion 키가 production 등록 경로의 실제 exec 자식에서 확인된다. 스킬 본문을 읽지 않은 경우도 같고 일반 자식과 cron·heartbeat agent 실행에서도 같다. workflow의 전달값을 확인하되 꺼진 exec를 켜지 않는다.
4. 우선순위·수명: process 비어 있지 않음/빈 값, 파일 값/부재, top-level/exec env 충돌과 빈 명시 값, 예약 네 이름을 최소 표로 확인한다. 파일 키 변경은 재시작 뒤 반영된다. 한 실행 안에서 선언 변경으로 값이 늘지 않으며 다음 실행에는 새 선택이 반영된다.
5. 관측: CLI 상태·스킬 요약·새 오류·진단에 가짜 키 원문이 없고, 새 환경 자료는 config·실행 snapshot·spawn envelope·workflow checkpoint에 직렬화되지 않는다. process 값과 명시 env 우선값이 classifier 입력·결과 검사 요청에서 기존 8자 기준으로 가려진다. 일반 exec 출력이 세션 기록에 남는 경우까지 원문 부재를 요구하지 않는다. 짧은 값과 일반 exec 출력의 한계를 전 표면 보호라고 주장하지 않는다.
6. 사용자 표면: PRD 020의 JSON 예와 Docker 경로를 따라 config 변경 없이 키를 쓸 수 있다. 다섯 SKILL.md에 shacs-bot이 `.env`를 읽는다는 안내가 남지 않는다. Notion CLI 별칭은 같은 exec 안에서만 설정하고, Teams가 지원·활성화됐다고 안내하지 않는다.
7. Rust 구현 PRD는 fmt(Google 옵션) → workspace clippy `-D warnings` → 변경 package의 `cargo test -q`를 따른다. 전체 workspace 테스트는 사용자 승인 뒤에만 한다. PRD 020이 `.md`만 고치면 Cargo 검증은 생략한다. 실제 서비스 키·외부 API 호출은 수용 시험에 필요하지 않다.

## Non Scope

- 세션에서 스킬을 읽은 뒤에만 전달하기. loaded-state와 자식·cron의 상속 규칙을 별도로 만들지 않는다.
- Prime 방식의 process 환경 전체 상속, OpenClaw 방식의 per-skill config env/apiKey 설정, 새 비밀 입력 UI.
- 선언하지 않은 키 전달, shell 본문에서 이름 추측, `.env` 지원·자동 migration, 전체 YAML 규격 지원.
- `.env.json` hot reload, 실행 중 키 회수, 이미 시작한 자식의 환경 바꾸기.
- 출력·transcript·log 전부의 비밀 제거, 변환된 비밀 탐지, 악성 스킬 격리. provider 인증 저장소의 값 꺼내기.
- Python kernel, MCP, browser, plugin, agent 없는 automation script로 전달 확대. Teams runtime 지원.

## 남은 위험과 한계

- 어떤 활성 스킬이 키를 선언하면 그 실행의 다른 exec도 읽을 수 있다. 선택한 스킬·workspace·plugin을 신뢰하는 사용자 모델이며 스킬별 비밀 격리가 아니다. 스킬 변경으로 다음 실행의 선언 집합이 넓어질 수 있다.
- 키를 바꾸거나 지워도 재시작 전까지 시작 때 값이 쓰인다. 별도 `skills show`는 현재 파일을 읽으므로 장기 실행 gateway와 다를 수 있다.
- env 값은 자식 프로그램, 자식이 띄운 프로그램과 셸 시작 파일에 보인다. 셸 시작 파일이 값을 다시 바꾸는 것까지 막지 않는다. 일반 tool output·세션 기록의 원문 및 8자 미만·인코딩된 값은 비밀 가리기의 보장 밖이다.
- `sync_builtin_skills`는 이미 있는 파일을 덮지 않는다(`crates/shacs-skills/src/lib.rs:187-219`). image를 갱신해도 오래된 materialized 스킬 안내가 남을 수 있어 PRD 020이 수동 갱신 방법을 설명한다.

## 실행 PRD와 독립 종료

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 선언과 누락 키 상태](prds/000-declarations-and-missing-status.md) | 선언 정규화, 활성 원본 선택 일치, 값 없는 상태·안내, 값 선택 규칙 | 없음 | 수용 조건 1·2와 4의 선택·상태, 5의 목록, 7 |
| [010 exec에 값 전달](prds/010-exec-environment-passthrough.md) | 실행별 메모리 환경, 본 대화·자식·자동화·workflow·sandbox 연결, 실제 전달값 가리기 | 000 | 수용 조건 3·4·5의 실행·가리기, 7 |
| [020 사용법과 내장 스킬 안내](prds/020-user-and-skill-guidance.md) | USAGE 스킬 키 절, 다섯 SKILL.md의 실제 설정 경로·예·제한 | 000, 010 | 수용 조건 6, 7 |

앞 번호 PRD의 닫힌 결과만 소비하며 뒤 번호 PRD를 기다리는 수용 조건은 두지 않는다. 000의 상태에는 자동 전달이 아직 구현되지 않았다는 경계를 적고, 010은 실행 문서의 기능 설명을 자기 작업에서 맞춘다. 020은 사용자의 설정 절차와 다섯 원본 안내를 완성한다.

## 구현 기록 (2026-11-18)

- PRD 000·010·020을 구현했고 각 PRD가 독립 검토 APPROVE를 받았다. 증거는 각 PRD 문서에 있다.
- PRD 020 검토 전에 PRD 000이 남긴 CLI 안내 `Automatic exec forwarding is not implemented yet.`를 `Values of keys declared by active skills are passed to exec commands.`로 바꿨다(`crates/shacs-cli/src/skill_readiness.rs:43`).
- fmt(Google 옵션) 통과. `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`는 이 변경과 무관한 기존 두 위반(`crates/shacs-projection/src/spec030/release_runner/cleanup.rs:95` `needless_bool`, `crates/shacs-core/src/runtime/memory.rs:1078` `needless_range_loop`)에서 멈춘다. 두 lint만 명령줄에서 뺀 같은 workspace clippy는 통과했고, 변경 package를 `--no-deps`로 따로 본 clippy에서 두 lint는 위 기존 위치에만 나온다.
- 변경 package 시험: `cargo test --manifest-path crates/Cargo.toml -p <package> -q`가 `shacs-config`·`shacs-skills`·`shacs-cli`에서 통과했다. `shacs-core`는 첫 실행에서 `tests/it`의 `spec034_remote_output` 두 시험(실제 DNS 조회, 이 변경과 무관한 파일)이 실패해 멈췄고, `--no-fail-fast`로 다시 돌린 전체 실행은 exit 0으로 통과했다.
- 남은 종료 조건: 전체 workspace 시험은 사용자 승인 뒤에 실행한다. 실제 Docker·서버 확인과 실제 서비스 키 호출은 하지 않았다.

