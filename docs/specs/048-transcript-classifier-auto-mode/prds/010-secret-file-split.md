# PRD 010. 비밀 파일 분리

Status: Implemented, closure blocked

Depends on: [PRD 000](000-decision-order-and-transcript-classifier.md)의 닫힌 결과(비밀 파일 규칙, action 정규화, classifier 요청). 뒤 번호 PRD에 의존하지 않는다.

## Goal

`config.json`의 비밀 값을 같은 data 폴더의 비밀 파일 `.env.json`으로 옮기고 `config.json`에는 `${이름}`만 남긴다. 그러면 workspace를 data 폴더로 둔 설정에서도 auto mode가 `config.json`을 묻지 않고 읽어도 비밀 값이 나가지 않는다. `.env.json`, 그 임시 파일, migration 백업과 진행 표시는 PRD 000의 인증 저장소와 같은 비밀 파일 규칙을 받는다.

## Scope

1. **`.env.json` 형식:** active data 폴더(`ConfigContext.data_dir`, config 파일이 있는 폴더)의 `.env.json`이다. 이름을 글 값에 잇는 JSON 객체 하나다.
   - 이름은 `[A-Z_][A-Z0-9_]*`이고 값은 글이다(빈 글도 된다).
   - JSON이 아님, 객체가 아님, 이름이 형식에 맞지 않음, 값이 글이 아님, 같은 이름이 두 번 나옴, 파일을 읽을 수 없음은 읽기 실패다. 오류는 문제의 종류와 몇 번째 항목인지(JSON 문법 오류는 줄과 칸)만 적고, 파일의 이름과 값을 적지 않는다. 값을 담을 수 있는 serde 오류 글은 쓰지 않는다.
   - 파일이 없으면 빈 것으로 본다.
   - shacs-bot이 쓸 때는 인증 저장소처럼 소유자만 읽고 쓰는 임시 파일(0600, `..env.json.tmp-*`)에 쓰고 원자적으로 바꾼다(`write_secret_file`). 읽을 때 권한을 검사하거나 고치지 않는다. 사용자가 직접 만든 파일의 권한은 문서에 `chmod 600`으로 안내한다.
2. **`${이름}` 해석:** config를 `${이름}`까지 풀어 읽을 때(`resolve_env`를 켠 `load_config_with_env`, runtime 시작 경로의 `resolve_config_env_refs` 자리) 이름마다 아래 순서로 값을 정한다.
   1. process 환경 변수(호출자가 넘긴 `EnvSource`)에 비어 있지 않은 값이 있으면 그 값.
   2. `.env.json`에 그 이름이 있으면 그 값.
   3. process 환경 변수의 빈 값(지금과 같다).
   4. 모두 없으면 지금처럼 이름만 적은 오류로 config 읽기가 실패한다.
   - 빈 process 값을 건너뛰는 까닭: `docker-compose.yml`이 정하지 않은 provider 변수도 빈 값(`${OPENAI_API_KEY:-}`)으로 넘긴다.
   - 풀어 읽을 때마다 `.env.json`을 읽고 검사한다. 모양이 틀리면 그 읽기가 실패한다. 이 오류는 `ConfigError::Json`과 다른 오류라서 `load_config_with_env_or_default`가 기본 config로 바꾸지 않는다. `${이름}`을 풀지 않는 읽기(`resolve_env: false`: `status`, `onboard`, 설정을 고치는 명령, agent loop가 turn마다 사용자 규칙을 읽는 자리)는 `.env.json`을 읽지 않아 사용자 규칙이 그대로 쓰인다.
   - 값은 process 환경 변수로 내보내지 않는다. 명령 실행·skill·MCP가 받는 환경 변수는 지금처럼 config의 top-level `env`, `tools.exec.env`, MCP server `env`가 정하고, 그 값의 `${이름}`이 이 순서로 풀린다. 그래서 config가 가리키지 않은 `.env.json` 이름은 들어가지 않는다.
   - 풀린 값 안의 `${…}`는 다시 풀지 않는다(지금 `interpolate_env_with_source`와 같다).
   - provider의 `apiKeyRef`·`credentialSource.environment`와 provider 기본 환경 변수(`OPENROUTER_API_KEY` 등)는 지금처럼 process 환경 변수에서만 찾는다(030·031 그대로). `.env.json` 값을 provider key로 쓰려면 `apiKey`에 `${이름}`을 적는다.
3. **비밀 자리:** `runtime config-migrate`는 config의 JSON 값 전체를 훑어 아래를 비밀 자리로 본다.
   - 칸 이름이 022 redaction이 비밀로 보는 이름인 글 칸(`shacs_redaction::is_sensitive_key`: 대소문자를 무시하고 `-`를 `_`로 본 이름에 `api_key`·`apikey`·`token`·`bearer`·`authorization`·`password`·`passwd`·`secret`·`credential`·`private_key`·`cookie` 등이 든다). 배열 안의 글은 그 배열의 칸 이름을 쓴다.
   - top-level `env`, `tools.exec.env`, `tools.mcpServers.<server>.env`의 글 값 전부(칸 이름과 상관없다).
   - 지금 config에서 걸리는 칸: `providers.<이름>.apiKey`(`api_key`), `providers.<이름>.extraHeaders`·`extraBody`와 `tools.mcpServers.<server>.headers` 안의 그런 이름(`Authorization`, `X-Api-Key` 등), `tools.web.search.apiKey`, channel 설정의 `token`·`appToken`·`botToken`·`bridgeToken`과 snake 표기, email의 `password`·`smtpPassword`·`imapPassword`와 snake 표기, 세 env 목록의 값.
   - 빈 글과 글 전체가 `${…}` 하나인 칸은 건너뛴다.
   - `${`가 든 그 밖의 글(`Bearer ${TOKEN}`)은 옮기지 않고 `kept`(이유 `placeholder`)로 보인다. `.env.json` 값은 다시 풀리지 않아 옮기면 뜻이 바뀐다.
   - `"kind": "secret_ref"`인 객체(031 secret ref, `apiKeyRef`) 안은 보지 않는다. 그 안의 `staleness_token` 같은 칸은 비밀 값이 아니라 digest다.
   - 글이 아닌 값(`maxTokens` 같은 숫자)은 보지 않는다.
   - 칸 이름으로 찾으므로 이름이 비밀처럼 보이지 않는 칸 안의 비밀(`tools.web.proxy` 주소의 비밀번호, `apiBase`에 넣은 key)은 옮기지 않는다. dry-run이 옮길 칸을 모두 보인다.
4. **이름:** `SHACS_` 뒤에 칸의 경로를 붙인다. 경로의 칸 이름마다 소문자·숫자 뒤에 오는 대문자 앞에 `_`를 넣고(camelCase 나누기), 영문자·숫자가 아닌 글자를 `_`로 바꾸고, 대문자로 바꾼다. 배열 위치는 그 숫자다. 칸 사이는 `_`로 잇고, 이어진 `_`는 하나로 줄이고, 끝의 `_`는 뗀다. 칸의 값은 `${이름}`이 된다.
   - 예: `channels.discord.token` → `SHACS_CHANNELS_DISCORD_TOKEN`, `providers.openrouter.apiKey`와 `providers.openrouter.api_key` → `SHACS_PROVIDERS_OPENROUTER_API_KEY`, `tools.exec.env.GITHUB_TOKEN` → `SHACS_TOOLS_EXEC_ENV_GITHUB_TOKEN`, `tools.mcpServers.my-server.headers.X-Api-Key` → `SHACS_TOOLS_MCP_SERVERS_MY_SERVER_HEADERS_X_API_KEY`.
   - env 목록의 값도 key가 아니라 경로로 이름을 만든다(SPEC 정정). key를 그대로 쓰면, process 환경 변수가 먼저라서 같은 이름의 변수(Docker Compose가 넘기는 `OPENAI_API_KEY`, `PATH` 등)가 옮긴 값을 대신하게 되고, `env`와 `tools.exec.env`에 같은 key를 다른 값으로 둔 설정(지금 지원하며 `tools.exec.env`가 이긴다)에서 멈추고, 소문자 key(`http_proxy`)는 이름 형식에 맞지 않는다.
5. **멈춤:** 아래 중 하나라도 있으면 dry-run은 `conflicts`로 보이고 apply는 어떤 파일도 바꾸지 않고 실패한다. 둘 다 칸 경로와 이름만 보인다.
   - `sameNameInConfig`: 다른 비밀 자리가 같은 이름을 다른 값으로 만든다(`channels.discordToken`과 `channels.discord.token` 등). 그 이름을 만드는 칸을 모두 보인다.
   - `envFileDiffers`: `.env.json`에 그 이름이 다른 값으로 있다.
   - `processEnvDiffers`: 명령을 실행한 process 환경 변수에 그 이름이 비어 있지 않은 다른 값으로 있다. 범위 2의 순서로 그 값이 이겨 쓰는 값이 바뀌기 때문이다. runtime이 다른 환경(Docker)에서 돈다면 같은 환경에서 실행해야 이 확인이 맞는다(문서).
   - 값이 같으면 멈추지 않고 그 이름을 같이 쓴다. `.env.json`에 같은 값이 이미 있으면 그 항목은 바꾸지 않는다.
6. **적용:** 비밀 옮기기는 031 schema migration과 다른 진행 표시(marker)와 백업을 쓴다. 모두 data 폴더에 두고 031처럼 config 파일 이름(`<c>`, 기본 `config.json`)에서 만든다(`with_extension("json.…")`): marker `<c>.secrets-migration-in-progress`, 원래 config 백업 `<c>.secrets-migration-backup`, 원래 `.env.json` 백업 `<c>.secrets-migration-env-backup`. marker는 두 파일의 원래·결과 digest와 원래 `.env.json`이 있었는지만 담는다. marker를 지우는 것이 적용의 확정이다.
   1. 031 marker나 이 marker가 있으면 멈춘다(`MigrationInterrupted`). 031 apply도 이 marker가 있으면 멈춘다.
   2. 지금 파일을 읽어 범위 3–5로 계획한다. schema가 지원하는 판보다 새로우면 031처럼 실패한다. 멈춤이 있으면 아무 파일도 바꾸지 않고 실패한다.
   3. marker가 없는데 남은 이 migration의 백업(확정 뒤 지우기 전에 끊긴 것)을 지운다. config와 `.env.json`은 바꾸지 않는다. 그 뒤 옮길 것이 없으면 다른 파일을 쓰지 않고 끝난다(`noOp`). 이미 옮긴 config는 늘 `noOp`이므로 남은 백업은 이 단계에서 지워진다.
   4. marker를 쓴다.
   5. 원래 config 백업을 쓰고, 원래 `.env.json`이 있었으면 그 백업을 쓴다. 4·5의 쓰기가 실패하면 쓴 백업과 marker를 지우고 실패한다.
   6. `.env.json`이 계획 때와 같은지 확인하고 새 `.env.json`을 쓴다. 다르면(계획 뒤 다른 것이 고침) 데이터 파일은 아직 그대로이므로 백업과 marker를 지우고 실패한다.
   7. config가 계획 때와 같은지 확인하고 새 config를 쓴다. 다르면 `.env.json`을 원래 내용으로 되돌리고(원래 없었으면 지우고) 백업과 marker를 지운 뒤 실패한다. 되돌리기가 실패하면 marker를 남겨 `--recover`에 맡긴다.
   8. marker를 지운다(확정). 그 뒤 백업을 지운다.
   - 모든 파일은 0600 임시 파일에 쓰고 원자적으로 바꾼다(`write_secret_file`). 그래서 백업은 다 있거나 없다. 이 transaction은 쓰기와 지우기마다 data 폴더를 fsync해(unix) 순서가 기록되게 한다. 시험이 확인하는 것은 process가 끊기는 경우이고, 전원이 꺼지는 경우는 파일 시스템이 fsync를 지키는 만큼만 같다. 새 `.env.json`은 이름 순서로 정렬한 JSON이다.
   - `.env.json`을 config보다 먼저 쓰므로 어느 단계에서 끊겨도 config는 옮기기 전과 같은 값으로 풀린다. 4–6 뒤에는 config가 원래 값을 담고, 7 뒤에는 `${이름}`을 담고 `.env.json`이 그 값을 담는다.
   - 시험이 단계마다 끊거나 그 사이에 파일을 바꿀 수 있게, 단계마다 부르는 관찰자를 받는 공개 함수를 둔다(관찰자가 멈추라고 하면 그 자리에서 끝난다. 031의 `begin_config_migration_apply`와 같은 목적). 되돌리기도 같다.
7. **되돌리기:** `runtime config-migrate --recover`는 이 marker가 있으면 비밀 옮기기를 되돌리고, 없으면 지금처럼 031 migration을 복구한다. 031 `--recover`의 동작(결과 상태면 되돌리지 않고 끝냄)은 그대로다.
   1. marker를 읽는다. 읽을 수 없으면 아무것도 바꾸지 않고 실패한다.
   2. 두 파일의 지금 상태를 원래·결과·없음·모름으로 나눈다. 원래 없던 `.env.json`은 없으면 원래 상태다. 하나라도 모름(끊긴 뒤 사용자가 고침)이면 아무것도 바꾸지 않고 실패한다(031의 `Unknown`과 같다).
   3. 원래 상태가 아닌 파일마다 그 백업이 있고 marker의 원래 digest와 같아야 한다. 없거나 다르면 아무것도 바꾸지 않고 실패한다(`MigrationBackupMismatch`). 백업은 데이터를 쓰기 전에 다 쓰므로, 백업이 없거나 덜 쓰였으면 두 파일은 원래 상태라 이 확인에 걸리지 않는다.
   4. config를 원래 내용으로 되돌린 뒤 `.env.json`을 원래 내용으로 되돌린다(원래 없었으면 지운다). config를 먼저 되돌리므로 되돌리기가 끊겨도 config는 풀리고, `--recover`를 다시 하면 이어서 된다.
   5. marker를 지우고(확정) 남은 백업을 지운다. 그 뒤 apply를 다시 할 수 있다.
   - marker 없이 이 migration의 백업만 남아 있으면(apply나 되돌리기가 확정된 뒤 백업을 지우기 전에 끊김) 백업만 지우고 끝난다. 두 파일은 바꾸지 않고, 출력의 `removedBackups`에 지운 백업 이름이 나온다. 그래서 되돌리기가 marker를 지운 뒤 끊겨도 `--recover`를 다시 하면 끝난다.
   - 옮기기가 확정되면 marker가 없어 되돌릴 수 없다(SPEC 남은 위험).
8. **CLI:** `runtime config-migrate`의 JSON 출력은 지금 031 칸을 그대로 두고 `secrets` 칸을 더한다. `secrets`는 늘 `action`, `moved`(`field`, `name`), `kept`(`field`, `reason`), `conflicts`(`field`, `name`, `reason`), `restored`(파일 이름), `removedBackups`(지운 남은 백업 이름)를 담는다.
   - `--dry-run`: 031 migration을 적용한 모양(`migrate_config_value`의 결과)으로 계획해 `action: "dryRun"`을 보인다. 멈춤이 있어도 성공으로 끝난다.
   - `--apply`: 먼저 031 migration을 적용한 모양으로 계획해 멈춤이 있으면 031 migration도 하지 않고 실패한다. 없으면 031 migration을 적용하고, 그 결과 파일을 원래 상태로 삼아 범위 6을 처음부터 한다(다시 계획하고 다시 확인한다). 031 migration은 비밀 자리를 바꾸지 않으므로 두 계획은 같다. 다시 계획에서 멈추면 031 결과는 그대로 남고 실패한다.
   - `--recover`: 이 marker가 있으면 비밀 옮기기를, 없고 031 marker가 있으면 031 migration을, 둘 다 없고 이 migration의 백업만 남았으면 그 백업 정리(범위 7)를 한다. 비밀 쪽이면 `{"secrets": …}`(`action: "recovered"`, `restored`, `removedBackups`), 아니면 지금 031 출력이다.
   - `field`는 칸 이름을 `.`으로, 배열 위치를 `[N]`으로 잇는다. 값과 digest는 보이지 않는다(짧은 값은 digest로 알아낼 수 있다).
   - 도움말에 비밀 옮기기를 적는다. runtime 시작의 확인(`guard_config_migration_admission`)은 이 marker도 보고 멈춘다.
9. **비밀 파일:** PRD 000 범위 5의 비밀 파일에 data 폴더 바로 아래의 아래 파일과, 각 파일의 원자적 저장 임시 파일(`.<이름>.tmp-*`)을 더한다. `SecretFiles`가 config 파일 경로를 받아 이름을 만든다.
   - `.env.json`.
   - 031 migration 백업 `<c>.migration-backup`(옮기기 전 config의 비밀 값을 담을 수 있다).
   - 비밀 옮기기의 marker와 두 백업. marker는 `.env.json`의 digest를 담아 짧은 값을 알아낼 수 있게 하므로 넣는다. 031 marker는 config 파일의 digest만 담아 넣지 않는다.
   - 모든 mode에서 PRD 000과 같이 다룬다: 훑기 결과에서 빠지고, 지정한 읽기는 매번 한 번 승인·거절 질문이 되고, 쓰기는 거절되고, hard link와 symlink도 같다. PRD 000 범위 6의 글자로 찾기 이름에 `.env.json`, 백업과 marker 이름이 더해진다.
   - **판정용 인자:** action 정규화(`normalize_tool_candidate`)는 redaction한 인자를 두 벌 만든다. 밖에 보이는 인자(`redacted_arguments`, 지금 칸)는 022 redaction 뒤 범위 10의 값 가리기를 거치고, digest·대상(`target_refs`)·승인 질문·diagnostics·최근 거절·classifier 요청·기억된 규칙의 저장이 이것을 쓴다. 판정용 인자는 022 redaction 뒤 아래 경로 되살리기만 거치고 값 가리기를 하지 않는다. 판정 재료(비밀 파일, 사용자 규칙, 보호 경로, critical path, 명령 분석), 실행 전 거절 확인, 022 대상 분류(`classify_permission_action`), 명령 요약(`proc_exec_summary_for_action`), 기억된 규칙의 match가 이것을 쓴다. 그래서 값 가리기가 판정을 바꾸지 않는다(`.env.json` 값이 `.env.json`이나 workspace 경로여도 `cat .env.json`은 묻는다). 판정용 인자는 `PermissionedAction`의 `#[serde(skip)]` 칸으로 메모리에만 있고, 그 action의 같음 비교와 `Debug`는 이 칸을 보지 않는다. 두 벌이 다를 때만 채우고, 비어 있으면 `redacted_arguments`가 판정용 인자다(permission environment가 없거나 가리기·되살리기가 아무것도 바꾸지 않은 action, 직렬화했다 읽은 action). 022 대상 분류는 `action.target_refs` 대신 판정용 인자로 대상을 만든다.
   - **기억과 세션 승인:** 두 벌이 다른 action(값을 가렸거나 경로를 되살린 action)은 기억하지 않는다. 그 승인 질문은 한 번 승인·거절만 받고(`ApprovalExplanation.once_only`, PRD 000 범위 15), 기억된 규칙(세션·프로젝트)과 세션 승인 cache(`SessionApprovalCacheEntry`)를 만들지 않는다. 가린 글은 값이 바뀌어도 같아서, 가린 인자로 만든 규칙이나 cache는 다른 명령·경로에 맞을 수 있기 때문이다. 기억된 규칙과 세션 승인 cache의 match는 판정용 인자로 한다: `ExactAction`과 cache의 exact 항목은 판정용 인자로 만든 digest, `ExecPrefix`와 cache의 명령 pattern(`ExecCommandPattern`)은 판정용 명령, `WorkspacePath`·`WebOrigin`은 판정용 경로·주소로 본다. 기억할 수 있는 action은 두 벌이 같아 저장되는 digest·명령·경로가 판정용 인자의 것과 같다. 그래서 가리기 전에 저장한 규칙과 cache(경로 deny, subtree deny, exact deny, 명령 pattern)도 그대로 맞고, 새로 저장하는 규칙과 cache에는 값이 들어가지 않는다.
   - **보이는 글:** 판정 재료에서 나와 사람이나 provider에게 보이는 글(승인 질문 `Why:` 줄의 비밀 파일 이름·critical path 폴더·symlink 실제 경로, 실행 전 거절 글)은 보이기 전에 범위 10의 값 가리기를 거친다. 판정은 가리기 전 재료로 이미 끝났다. 실행 전 거절 글은 tool이 실행됐으면 낼 글과 같아 어시스턴트가 준 경로(되살린 경로)를 담을 수 있다. 비밀 파일 질문의 `Why:` 줄은 인증 저장소만이 아니므로 "secret file"이라고만 적는다.
   - permission environment가 없는 실행은 두 벌이 같다(지금과 같다).
   - **redaction이 지운 file tool 경로:** 022 redaction은 `.`·`/`·`~`로 시작하면서 `.env`·`auth`·`token` 등이 든 글을 통째로 `[REDACTED]`로 바꾼다. 그래서 지금은 file tool에 `.env.json`이나 비밀 파일의 절대 경로를 준 호출이 대상 분류 불가(PRD 000 범위 2의 2번)로 모든 mode에서 거절된다(PRD 000 남은 제한). 이 PRD는 판정용 인자에서, file tool(`read_file`, `list_dir`, `glob`, `grep`, `notebook_read`, `write_file`, `edit_file`, `notebook_edit`)의 `path`가 `[REDACTED]`가 됐고 원래 글이 가리키는 경로(workspace 기준으로 풀고 `..`를 정리한 것)가 비밀 파일이면 `path`를 그 경로로 둔다. 원래 글은 남기지 않는다. 그러면 대상 분류, 사용자 규칙, 비밀 파일 판정, 실행 전 거절 확인이 그 파일을 보고, 읽기는 한 번 승인·거절 질문, 쓰기는 거절, file tool의 범위 밖이면 지금처럼 거절이 된다. 밖에 보이는 인자는 지금처럼 `[REDACTED]`라 경로가 직렬화되지 않는다. 비밀 파일이 아닌 경로는 판정용 인자에서도 `[REDACTED]`다.
   - `notebook_read`는 이름으로 `fs_read`가 되는 tool(plugin이 그 이름을 쓸 수 있다)인데 PRD 000의 읽는 file tool 목록에 빠져, 비밀 파일과 경로 규칙을 보지 않고 auto mode 14번으로 허용될 수 있었다. 읽는 file tool 목록에 더한다(`path` 인자).
   - `exec`의 `command`가 통째로 `[REDACTED]`가 된 명령(`/bin/cat .env.json`처럼 `.`·`/`·`~`로 시작하면서 그런 낱말이 든 명령)은 명령 분석이 글을 볼 수 없어 지금처럼 모든 mode에서 묻지 않고 거절된다. 명령 글을 되살리면 그 안의 비밀이 남기 때문이다(남은 제한).
10. **`.env.json` 값 가리기:** `.env.json`의 8자 이상인 값을 `[secret <이름>]`으로 바꾼다. 022 redaction은 따로 그대로 돈다.
    - **action 정규화:** `normalize_tool_candidate`가 022 redaction과 남은 비밀 확인(`checked_redacted_arguments`) 뒤 밖에 보이는 인자(범위 9)의 글마다 바꾸고, 그 인자로 `argument_digest`, 대상과 그 digest, `action_digest`를 만든다. 그래서 승인 질문, diagnostics의 permission event, 최근 거절, classifier 기록과 요청의 action, 기억된 규칙의 matcher에 값이 들어가지 않는다. 인자 검사 오류 글도 바꾼다. 판정은 가리지 않은 판정용 인자로 한다(범위 9). 실제 tool 실행은 원래 인자를 쓴다. 값은 `PermissionedActionInput`이 실행 문맥의 permission environment에서 받는다(직렬화하지 않는다). subagent도 부모의 permission environment를 받아 같게 가린다. permission environment가 없는 실행(022 흐름)은 지금과 같다.
    - **승인 답:** 세션에 저장한 승인 질문의 실행 문맥에는 permission environment가 없다(직렬화하지 않는다). 승인·거절 답을 받아 action을 다시 정규화해 승인 질문과 맞추기 전에(`agent_loop.rs`의 승인 답 처리) 그 turn의 permission environment를 다시 붙인다. 그래서 같은 값이면 digest가 같아 승인한 호출이 실행된다. 질문과 답 사이에 `.env.json` 값이 바뀌어 digest가 달라지면 지금 `ActionMismatch`처럼 실행하지 않는다. `.env.json`을 읽지 못해 마지막에 읽은 값을 쓰는 동안에는 digest가 그대로다.
    - **classifier 요청:** instruction 파일 글, payload의 user 글과 앞선 tool call의 인자(세션 기록에서 가져오므로 따로 바꾼다)를 redaction 뒤, 자르기 전에 JSON 값 안의 글마다 바꾼다. JSON 글로 만든 뒤에 찾지 않으므로 `"`·`\`가 든 값도 찾고, 자르기 경계에 걸친 값도 가려진다. 고정 template, slot, skill 목록은 바꾸지 않는다(그 글을 가려 요청이 망가지지 않게).
    - 글을 한 번 훑으며 그 자리에서 맞는 가장 긴 값을 바꾸고, 바꾼 글은 다시 보지 않는다. 같은 값이 여러 이름에 있으면 이름 순서로 앞의 이름을 쓴다.
    - 8자보다 짧은 값은 흔한 글까지 가려 판정을 망칠 수 있어 가리지 않는다. 8자 이상의 흔한 글(`production` 같은 env 값)은 사용자 메시지에서도 가려진다(남은 제한).
    - 값은 agent loop가 turn마다 permission environment를 만들 때(`auto_mode_cli::permission_environment`) `.env.json`에서 읽는다. runtime 시작이 `${이름}`을 풀며 읽은 값도 같은 기억에 넣는다. 읽을 수 없으면 같은 process가 그 config 경로로 마지막에 읽은 값을 그대로 쓰고, 그 turn에는 classifier를 부르지 않는다(PRD 000 범위 2의 16번은 판정 없음이 된다). 사용자 규칙은 그대로 쓴다. permission environment와 값의 `Debug`는 이름만 보인다.
    - 도구 결과 검사 요청의 가리기는 그 요청을 만드는 PRD 040이 같은 값으로 한다.
11. **`onboard`:** 지금 `onboard`는 channel 기본 설정의 비밀 칸을 빈 글로만 만들고, `onboard --wizard`는 환경 변수 이름만 받아 `apiKeyRef`를 쓴다. 둘 다 비밀 값을 받는 단계가 없어 바꾸지 않고, SPEC의 "`onboard`는 새 비밀 값을 `.env.json`에 쓴다"를 이 사실로 고친다. 사용자는 비밀 값을 `.env.json`에 적고 config에 `${이름}`을 쓰거나, config에 적은 뒤 `runtime config-migrate --apply`로 옮긴다(문서).
12. **문서:** `docs/USAGE.md`의 `${이름}` 해석, config-migrate, 비밀 파일 설명, README의 config-migrate·Docker·권한 문단, `.env.example`.

## Non Scope

- provider credential의 환경 변수 참조(`apiKeyRef`, `credentialSource.environment`, 기본 이름)가 `.env.json`을 보게 하는 것.
- 비밀 값을 받아 `.env.json`에 쓰는 새 명령.
- 이름이 비밀처럼 보이지 않는 칸과 주소 안의 비밀 옮기기. process 환경 변수로 받은 값과 config에 남은 값의 정확한 가리기(022 redaction만 받는다).
- 통째로 redaction된 `exec` 명령을 묻게 하는 것.
- runtime layout(`runtime inspect`)에 `.env.json` 항목 더하기. 029 stored-data migration, TOML 전환.
- 막힘을 돌려주기(PRD 020), 하위 작업(PRD 030), 도구 결과 검사(PRD 040), 기본 mode와 설정 도구(PRD 050).

## Parent Requirement Mapping

- [048 SPEC](../SPEC.md)의 소유 범위 3 중 PRD 000이 소유하지 않은 나머지(`.env.json`, `${이름}` 해석 순서, 비밀 옮기기, `onboard`, 새 비밀 파일), "반드시 지킬 것"의 `.env.json` 값·옮기기 중단 항목 중 이 PRD가 만드는 표면(도구 결과 검사 요청은 PRD 040), 17 중 `.env.example`과 이 PRD 변경의 문서, 수용 조건 5와 이 PRD 변경에 대한 수용 조건 11을 소유한다.
- PRD 000의 남은 제한 중 "redaction이 지우는 절대 경로로 비밀 파일을 읽으면 묻지 않고 거절한다"를 file tool에 대해 없애고, PRD 000의 읽는 file tool 목록에 빠진 `notebook_read`를 더한다(범위 9). SPEC이 정한 "읽을 때마다 묻는다"에 맞추는 것이다. 판정 재료가 redaction한 인자를 쓰는 PRD 000의 방식은 판정용 인자로 그대로 잇는다.
- 이 계획에서 SPEC을 고친다.
  - `${이름}`은 비어 있지 않은 process 환경 변수를 먼저 본다(범위 2).
  - 비밀 자리의 칸 이름은 022 redaction의 비밀 key 판정을 따른다(`authorization`·`cookie` 등이 더해진다, 범위 3).
  - 이름은 모두 칸 경로에서 만든다. 멈춤에 config 안의 같은 이름과 process 환경 변수를 더한다(범위 4·5).
  - 비밀 파일에 비밀 옮기기의 marker를 적는다(범위 9).
  - `onboard`는 비밀 값을 받지 않는다는 사실로 고친다(범위 11).
  - `.env.json` 값을 가리는 자리와 8자 기준, 022 redaction과의 관계를 정확히 적는다(범위 10).

## Required Contract

1. `.env.json` 값은 process 환경 변수로 나가지 않는다. 명령 실행·skill·MCP의 환경 변수는 config의 env 목록이 정한다.
2. 비밀 옮기기가 어느 단계에서 끊겨도 config는 옮기기 전과 같은 값으로 풀린다. `--recover` 뒤 두 파일은 옮기기 전과 byte 단위로 같다(원래 없던 `.env.json`은 없다).
3. 멈춤이 있으면 어떤 파일도 만들거나 바꾸지 않는다.
4. 옮기기의 출력과 오류, `.env.json` 오류에는 값이 없다. `.env.json` 오류에는 이름도 없다. 새 코드는 값을 log와 diagnostics에 쓰지 않는다.
5. 새 비밀 파일은 모든 mode에서 PRD 000의 비밀 파일 규칙을 받고, 사용자 설정으로 줄일 수 없다. file tool로 비밀 파일을 지정해 읽으면 redaction이 경로를 지웠어도 묻는다.
6. 8자 이상인 `.env.json` 값은 action의 밖에 보이는 인자와 그 digest, 승인 질문, diagnostics, classifier 요청의 범위 10 자리에 나오지 않는다. 값 가리기는 판정을 바꾸지 않는다(판정과 기억된 규칙·세션 승인의 match는 판정용 인자로 한다). 값이 든 action은 기억되지 않는다. 값을 읽을 수 없는 turn에는 classifier를 부르지 않는다.
7. 031 migration의 동작과 출력 칸은 그대로다. `secrets` 칸과 새 marker 확인만 더한다.
8. `Cargo.lock`에 새 crate를 더하지 않는다.

## Acceptance Criteria

1. 해석 시험: 비어 있지 않은 process 값이 이기고, 빈 process 값이면 `.env.json`, `.env.json`에 없으면 빈 process 값, 모두 없으면 이름만 적은 실패다. runtime 시작 경로(`load_runtime_config_with_env`)도 `.env.json`으로 푼다. `resolve_env: false` 읽기는 모양이 틀린 `.env.json`에도 성공한다. config를 읽은 뒤 process 환경 변수에 `.env.json`의 이름이 없고, 풀린 config의 env 목록에는 config가 적은 key만 있다.
2. 형식 시험: JSON 아님, 객체 아님(배열, 글), 이름 형식 틀림, 값이 글이 아님(숫자, 객체), 같은 이름 두 번, 읽을 수 없음이 각각 실패하고, 오류 글에 파일의 이름과 값(canary)이 없다. `load_config_with_env_or_default`는 기본 config로 바꾸지 않는다.
3. 계획 시험: 범위 3의 각 칸(provider `apiKey`·`api_key`, header, web search, channel token 종류, email 비밀번호, 세 env 목록)이 옮겨지고 이름이 범위 4와 같다. 빈 글, `${…}` 하나, secret ref, 숫자는 건너뛰고 `Bearer ${X}`는 `kept`다. 같은 값의 두 칸은 이름 하나를 같이 쓴다. 세 가지 멈춤이 각각 `conflicts`가 되고, 빈 process 값은 멈춤이 아니다.
4. transaction 시험:
   - apply 뒤 config에 원래 값이 없고 `${이름}`만 있으며, `.env.json`이 그 값을 담고, 풀어 읽은 config가 옮기기 전과 같다. `.env.json`의 다른 항목은 그대로다. `.env.json`, 백업, marker의 권한이 0600이고, 끝난 뒤 백업과 marker가 없다. 다시 apply하면 `noOp`이다.
   - 멈춤이 있는 apply는 config, `.env.json`, 백업, marker를 만들거나 바꾸지 않고 031 migration도 하지 않는다.
   - apply를 marker, config 백업, `.env.json` 백업, `.env.json`, config, marker 지우기 뒤에서 각각 끊은 상태에서: config가 옮기기 전과 같은 값으로 풀리고, marker가 남은 상태면 apply와 runtime 시작이 멈추며, `--recover` 뒤 두 파일이 옮기기 전과 byte 단위로 같고(확정 뒤에 끊은 경우는 옮긴 상태 그대로이고, 남은 백업은 다음 apply가 `noOp`으로 끝나며 지운다), 다시 apply가 된다. 원래 `.env.json`이 없던 경우와 있던 경우를 모두 본다.
   - recover를 config 되돌리기, `.env.json` 되돌리기, marker 지우기 뒤에서 각각 끊은 뒤 다시 `--recover`하면 이어서 끝나고(marker 지우기 뒤면 남은 백업만 지운다) 두 파일이 옮기기 전과 같다. CLI로 두 번 실행해도 같다.
   - 끊긴 뒤 config나 `.env.json`을 고쳤으면 `--recover`가 아무것도 바꾸지 않고 실패한다. 필요한 백업이 marker와 다르면 실패한다.
   - 계획 뒤 `.env.json`이 바뀌면(6단계) apply가 실패하고 config는 원래 내용, `.env.json`은 바뀐 내용 그대로다. `.env.json`을 쓴 뒤 config가 바뀌면(7단계) apply가 실패하고 `.env.json`은 원래 내용으로 돌아가며 config는 바뀐 내용 그대로다. 둘 다 marker와 백업이 남지 않는다.
   - 031 migration이 필요한 config에서 apply가 031 migration과 비밀 옮기기를 차례로 하고, 비밀 옮기기의 원래 상태는 031 결과 파일이다.
5. CLI 시험: dry-run·apply·recover 출력이 범위 8의 모양이고, 출력과 실패 글에 canary 값이 없다. 멈춤이 있는 apply는 실패로 끝난다. 기존 031 CLI 시험과 `spec031_migration_transaction`·`spec031_migration_cas` 시험은 그대로 통과한다.
6. 비밀 파일 시험(`tests/it/spec048_auto_mode/`, workspace를 data 폴더로 둔 설정), 여섯 mode × 대화형 여부:
   - `read_file`과 `notebook_read`(가짜 tool)에 `.env.json`, `./.env.json`, 그 절대 경로, `..env.json.tmp-1`, 백업, marker를 준 호출과, `.env.json`을 가리키는 symlink·hard link의 `read_file`, `cat .env.json`, `cat config.json.migration-backup`, here-doc과 `python3 -c` 안의 `.env.json`이 한 번 승인·거절 질문이 되고(비대화형·`dont_ask`는 거절), 질문이 그 파일 이름을 보이며, 승인한 그 호출만 읽는다.
   - 같은 파일들에 대한 `write_file`·`edit_file`·`>`·`rm`은 거절된다.
   - `list_dir`·`glob`·`grep` 훑기에 나오지 않고 내용도 읽히지 않는다.
   - 되살린 경로는 판정에만 쓰이고, 그 호출의 `redacted_arguments`와 승인 질문 글에는 지금처럼 `[REDACTED]`가 있다.
   - data 폴더가 workspace 밖인 설정의 `read_file <data 폴더>/.env.json`은 지금처럼 묻지 않고 거절된다. `/bin/cat .env.json`처럼 통째로 redaction되는 `exec`도 묻지 않고 거절된다. 비밀 파일이 아닌 redaction 경로(`.ssh/config` 등)의 file tool 호출은 지금처럼 거절된다.
7. 가리기 시험:
   - `exec` 인자에 `.env.json` 값이 든 호출의 `redacted_arguments`, `argument_digest`, `action_digest`, 승인 질문 글, diagnostics의 permission event에 그 값이 없고 `[secret <이름>]`이 있다. 같은 호출을 값을 미리 바꾼 인자로 정규화한 digest와 같다. tool은 원래 값으로 실행된다.
   - 값이 user 메시지, 앞선 tool call 인자, 판정할 action 인자, instruction 파일에 든 run에서 가짜 provider가 받은 classifier 요청에 그 값이 없고 `[secret <이름>]`이 있다.
   - 정확히 8자인 값은 가려지고 7자 값은 그대로다. 겹치는 값은 긴 값이 먼저 가려지고 바꾼 글은 다시 보지 않는다. `"`가 든 값과 자르기 경계에 걸친 값도 가려진다.
   - 값 가리기가 판정을 바꾸지 않는다: `.env.json` 값이 `.env.json`, workspace 절대 경로, `[REDACTED]`인 설정에서 `cat .env.json`의 질문, workspace를 지우는 critical path 질문, 경로 deny 규칙, 비밀 파일 쓰기 거절이 값이 없는 설정과 같다.
   - 기억과 세션 승인: 값이 든 action의 질문은 한 번 승인·거절만 받고, `approve_session`·`approve_project` 같은 답은 실행하지 않고 다시 묻고, 기억된 규칙과 세션 승인 cache가 생기지 않는다. workspace 경로에 `.env.json` 값이 든 파일에 대해 가리기 전에 저장한 exact·경로·subtree deny와 세션 cache의 exact·명령 pattern 항목이 그대로 맞는다. `.env.json` 값을 바꾼 뒤 같은 가린 글이 되는 다른 명령에는 앞의 승인이 쓰이지 않는다. 저장한 `permissions.json`과 세션 기록의 cache에 값이 없다.
   - 보이는 글: 값이 든 critical path 폴더·symlink 실제 경로의 승인 질문 `Why:` 줄에 값이 없다. subagent run의 action도 같게 가려진다.
   - 승인 답: 값이 든 `exec`를 묻고 승인하면 실행되고, 질문과 답 사이에 `.env.json` 값을 바꾸면 실행되지 않으며, `.env.json`을 깨뜨린 뒤의 답(마지막에 읽은 값을 쓰는 permission environment)은 그 호출을 한 번 실행한다.
   - turn 중에 `.env.json`이 깨지면 다음 turn은 classifier를 부르지 않고(판정 없음), 앞서 읽은 값으로 계속 가리고, 사용자 규칙은 그대로 걸린다.
8. onboard 시험: 새 data 폴더에서 `onboard`한 config는 계획에 옮길 칸이 없고, `.env.json`을 만들지 않는다.
9. 문서 확인: `docs/USAGE.md`, README, `.env.example`이 범위 2–11의 사용자 동작(해석 순서, 옮기기와 되돌리기, 비밀 파일, 가리기의 한계, onboard 뒤 비밀 넣는 법)과 같다. `Cargo.lock`이 바뀌지 않는다(`git diff --exit-code crates/Cargo.lock`).
10. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다.

## Closure Evidence

1. `.env.json`, 해석, 비밀 옮기기와 되돌리기, CLI, 비밀 파일, 가리기, 문서의 구현과 시험.
2. 고친 기존 시험과 까닭.
3. 값이 없는 dry-run 출력 예 하나.
4. Cargo 검증 기록과 `Cargo.lock` 확인.

PRD 010은 PRD 020–050 없이 닫힌다.

## 구현 메모 및 검증 근거 (2026-11-05)

- 구현:
  - `shacs-config`: `env_file.rs`(`.env.json` 읽기와 검사, `ConfigEnv`), `secret_places.rs`(비밀 자리·이름·멈춤), `secrets_migration.rs`(marker를 먼저 쓰는 두 파일 transaction, 되돌리기, 시험용 관찰자, 쓰기마다 폴더 fsync), `lib.rs`(`resolve_config_with_env_file`, `ConfigError::{EnvFile, SecretsMigrationStopped, SecretsMigrationConflict}`), `config_migration.rs`(두 marker 확인, `config_migration_backup_path`).
  - `shacs-core`: `auto_mode/secret_file.rs`(새 비밀 파일과 임시 파일), `auto_mode/secret_values.rs`(가리기), `auto_mode/action_secrets.rs`(두 벌 인자와 경로 되살리기), `permission_action.rs`(`DecisionArguments`, `PermissionedActionInput.permission_environment`). 판정 쪽은 `permission_materials`·`action_materials`(실행 전 거절, 보이는 글 가리기)·`permission_rules`(대상 분류)·`tool_execution`(명령 요약, 세션 승인 match, 한 번만 받는 질문)·`permission_remembered`·`permission_pattern`이 판정용 인자를 쓴다. classifier 요청(`classifier_request`, `classifier_review`)은 값을 가리고, 값을 읽지 못한 turn에는 부르지 않는다. agent loop는 승인 답에 그 turn의 permission environment를 다시 붙이고, 값이 든 호출로 세션 승인 cache를 만들지 않는다. `notebook_read`를 읽는 file tool에 더했다. 비밀 파일 질문의 `Why:`는 "secret file"이라고만 적는다.
  - `shacs-cli`: `auto_mode_cli.rs`(config 경로마다 마지막에 읽은 값을 기억하고 `secret_values_current`를 정함), runtime 시작의 `.env.json` 해석과 기억 채우기, 두 marker를 보는 runtime 시작 확인, `runtime config-migrate`의 `secrets` 출력과 도움말.
  - 문서: `docs/USAGE.md`의 `${이름}` 해석, "`.env.json`과 비밀 옮기기", 비밀 파일, config-migrate, Docker 설명. README의 config-migrate·권한·Docker 문단. `.env.example`. PRD 000 남은 제한에 이 PRD가 고친 것을 적었다.
- 계획 밖에서 고친 것:
  - 세션 승인 cache의 policy safety snapshot 비교도 판정용 인자로 만든 snapshot으로 한다. snapshot id가 action digest를 담아, 가린 digest로 비교하면 가리기 전에 만든 cache가 같은 원래 호출에도 맞지 않았다.
  - critical path `Why:`는 글이 `/`로 시작하는지로 폴더와 확인할 수 없는 까닭을 가렸는데, 가린 폴더(`[secret …]`)도 폴더로 읽게 했다.
  - 구현 검토 1차: marker 없이 남은 백업만 있으면 recover가 백업만 지우게 했다(범위 7). 되돌리기가 marker를 지운 뒤 끊기면 다시 실행할 수 없었다.
- 시험:
  - `shacs-config`: 단위 시험 12개(`env_file_tests.rs`, `secret_places_tests.rs`), 통합 시험 13개(`tests/it/spec048_env_file.rs` 해석 순서·모양·읽을 수 없음·내보내지 않음, `tests/it/spec048_secrets_migration.rs` apply·멈춤·apply와 recover의 단계별 끊김·고친 파일과 백업 위조·계획 뒤 바뀐 파일·0600·031 다음 순서).
  - `shacs-core`: 단위 시험 `secret_values_tests.rs` 5개, `secret_file_tests.rs`의 새 파일 1개, `classifier_request_tests.rs`의 자르기 경계·앞선 호출·instruction 파일 가리기 1개. 통합 시험 `tests/it/spec048_auto_mode/` 16개: `env_secret_files.rs`(여섯 mode × 대화형 여부의 읽기 질문·쓰기 거절·훑기 숨김, 되살린 경로의 질문 글, 승인한 읽기, 범위 밖·통째로 redaction된 명령·비밀 파일이 아닌 redaction 경로), `env_masking.rs`(보이는 곳의 가리기와 digest, 원래 값으로 실행, 판정이 바뀌지 않음, 가리기 전 기억된 규칙, 기억 거절, 가리기 전 세션 승인과 값 바꾸기, subagent), `classifier.rs`(classifier 요청 가리기와 한 번만 받는 질문·세션 기록의 cache, 값이 바뀐 뒤의 답, `.env.json`이 깨진 뒤의 답, 값을 읽지 못한 turn).
  - `shacs-cli`: `tests/it/spec048_secrets_migration_cli.rs` 5개(출력 모양과 값 없음, 멈춤, runtime 시작 해석과 marker, onboard, marker를 지운 뒤 끊긴 되돌리기를 CLI로 다시 실행), `auto_mode_cli_tests.rs` 1개(값을 읽지 못한 turn의 기억과 규칙).
- 고친 기존 시험과 까닭: 모두 칸·인자 추가만이고 동작은 같다. `PermissionedAction.decision`(`Default::default()`), `PermissionedActionInput.permission_environment`(`None`), `PermissionEnvironmentInput`의 `env_file`·`env_file_current`, `AutoModeClassifier::new`와 `payload`·`system_message`의 가릴 값: `src/runtime/auto_mode/{auto_decision_tests,classifier_request_tests,permission_materials_tests}.rs`, `tests/{permission_action,permission_policy,plugin_runtime,spec030_classifier_baseline,tools}.rs`, `tests/it/{containment_permission_baseline,containment_process_gate,process_envelope,process_gate}.rs`, `tests/it/{remembered_permission_matcher_support,remembered_session_legacy_support,remembered_session_permission_support}/mod.rs`, `tests/it/spec048_auto_mode/{support,classifier}.rs`.
- 값이 없는 dry-run 출력 예: 로컬 `.omo/evidence/spec048/prd010/dry-run-sample.json`(추적하지 않음).
- Cargo 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings` 통과. `crates/Cargo.lock`은 바뀌지 않았다. `shacs-config`·`shacs-cli` package 시험 통과, `shacs-core` lib 454 passed, 통합 시험은 이 checkout의 `spec034_release_runner` 6개 중 5개(`InvalidConfig`, PRD 000 종료 기록과 같은 조건)를 빼고 통과했다(로컬 `.omo/evidence/spec048/prd010/package-tests.log`, 구현 검토 1차 뒤 다시 돌린 `package-tests-round2.log`).
- 짧은 경로 clone 확인(2026-11-05): PRD 010과 020을 담은 커밋 `efcfc00`을 `/private/tmp/shacs-s48/clone`에 받아 `spec034_release_runner`를 돌렸고 6개 모두 통과했다(`.omo/evidence/spec048/closure/spec034-runner-copy-prd020.log`). 이 checkout의 `InvalidConfig`는 경로 길이 때문이다. 전체 workspace 시험은 사용자가 미뤄 아직 돌지 않았다.
- Oracle 독립 검토: 계획은 1–4차 BLOCKED(file tool 비밀 경로 거절, 031 뒤 낡은 계획, marker 순서, digest의 값, 값을 읽지 못한 turn, SPEC 모순, 판정에 쓰인 가린 인자, 승인 답의 environment, 남은 백업 정리 순서, `notebook_read`, 기억된 규칙과 세션 승인 cache) 뒤 5차 **PASS**. 구현은 1차 BLOCKED(marker를 지운 뒤 끊긴 되돌리기, `.env.json`이 깨진 뒤의 승인 시험) 뒤 2차 **PASS**. 기록은 로컬 `.omo/evidence/spec048/plan-review.md`와 `implementation.md`.
- 남은 제한:
  - 통째로 redaction된 `exec` 명령은 비밀 파일을 읽더라도 묻지 않고 거절된다.
  - 8자보다 짧은 값과 모양을 바꾼 값은 가리지 못한다. 비밀 자리는 칸 이름으로 찾는다.
  - `processEnvDiffers`는 명령을 실행한 환경으로 본다. provider credential의 환경 변수 참조는 `.env.json`을 보지 않는다.
  - 전원이 꺼지는 경우의 순서는 파일 시스템이 fsync를 지키는 만큼만 같다.
