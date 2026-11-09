# 052. Docker 처음 실행 준비 구현 명세

Status: Planned

Consumes: 015·023 공식 image와 Compose(처음부터 일반 사용자로 실행, official-container 증거), 029 runtime 시작과 시작 출력, 003 provider 고르기(`ProviderRegistry`), 010 provider 인증 명령(`provider codex login`, `provider <id> import-key`)과 비밀 다루기, 030 credential 해석(자리 순서, 인증 저장소 읽기, fingerprint, 갱신), 031 config 읽기·쓰기와 credential 선언 저장, config migration marker, 048 `.env.json` 형식·`${이름}` 해석 순서·이름 규칙·충돌 규칙·바뀜 확인 쓰기·비밀 옮기기 marker

## 목적과 완료 경계

지금 공식 Docker 경로로 처음 띄우려면 사용자가 README·`docs/USAGE.md` "Docker Compose"의 여섯 단계를 순서대로 해야 한다. 쓰는 터미널마다 `SHACS_UID`·`SHACS_GID`를 내보내고, `~/.shacs-bot`을 미리 만들고, `onboard`를 돌리고, `config.json`에 provider와 channel 값을 적고, `runtime config-migrate --apply`로 비밀을 옮기고, gateway를 띄운다.

하나라도 빠지면 막히고, 막힌 까닭을 알려 주지 않는다. 2026-11-09 UID 1001인 Linux 서버에서 확인한 것:

- Compose(`user: "${SHACS_UID:-1000}:${SHACS_GID:-1000}"`)는 `SHACS_UID`가 없으면 컨테이너를 1000번으로 돌린다. data 폴더가 1001 소유라 `onboard`가 `CLI I/O failed: Permission denied (os error 13)`로 끝났다.
- data 폴더가 없으면 Docker가 mount하면서 `root:root`(755)로 만들고, 같은 오류가 난다.
- 컨테이너를 1001번으로 돌리면 data 폴더에는 쓰지만 image의 HOME(`/home/shacs`, 1000 소유, 755)에는 쓰지 못한다. `~/.cache`·`~/.config`를 만들지 못하고, npm 캐시 자리(`~/.npm`)도 HOME 아래다.
- config가 없어도 gateway(`run`)는 WebSocket만 켜고 뜬다. 외부 channel은 꺼짐으로 건너뛴다. gateway 출력은 provider 이야기를 하지 않는다.
- 공식 Compose는 provider 환경 변수(`ANTHROPIC_API_KEY` 등)를 host에서 정하지 않으면 빈 값으로 넘긴다. 이때 runtime은 같은 provider의 key가 인증 저장소(`provider anthropic import-key`)나 config(`apiKey`, `.env.json`으로 푸는 `${이름}` 포함)에 있어도 빈 환경 변수를 먼저 써서 key 없이 요청했다. 빈 값이 없으면 둘 다 key를 보냈다(네트워크 없는 컨테이너 안의 가짜 provider 서버로 확인). 소유 범위 9가 고친다.

코드로 확인한 것:

- `runtime inspect`의 readiness `provider_auth`는 config와 인증 저장소의 API key만 본다. Codex 로그인(OAuth)만 있으면 `blocked`로 보인다. 그래서 처음 설정에서 인증이 있는지 판단하는 데 쓸 수 없다.
- runtime은 config나 인증 저장소에 항목이 있는 provider만 후보로 삼는다. 환경 변수만 있는 provider는 고르지 않는다.
- `onboard`와 provider 로그인처럼 config를 쓰는 명령은 031·048 migration marker를 보지 않는다. marker를 보는 것은 `runtime config-migrate --apply`와 runtime 시작이다.

이 스펙은 처음 실행을 저장소의 스크립트 하나(`scripts/docker-setup.sh`)로 끝내게 한다. 스크립트가 data 폴더와 사용자 번호를 맞추고, image를 만들고, 처음 설정을 하고, ChatGPT 계정 로그인과 Discord 연결을 묻고, gateway를 띄운다.

공식 image는 지금처럼 처음부터 일반 사용자로 돈다(사용자 결정 2026-11-09). 컨테이너가 관리자로 켜져 사용자 번호를 바꾸고 data 폴더 주인을 고친 뒤 일반 사용자로 내려가는 방식(Hermes Agent `docker/stage2-hook.sh`)은 쓰지 않는다. 이 컨테이너에서는 어시스턴트가 명령 실행·파일 쓰기 도구를 쓴다. 켜질 때마다 관리자 권한으로 data 폴더를 고치면, 어시스턴트가 그 폴더에 심은 symlink가 그 작업을 다른 파일로 돌릴 수 있다. 또 provider 로그인과 channel 토큰은 어느 방식이든 사용자가 넣어야 한다. 이 스펙은 OpenClaw `scripts/docker/setup.sh`처럼 처음 설정 스크립트를 둔다. 다만 data 폴더 주인을 컨테이너 사용자(1000)로 바꾸지 않고, 컨테이너가 host 사용자 번호로 돌게 한다. 그래서 image의 HOME을 어느 번호로 돌아도 쓸 수 있게 한다.

완료되면 사용자는 다음을 경험한다.

- 저장소를 받고 `./scripts/docker-setup.sh`를 한 번 실행해 몇 가지 질문(ChatGPT 계정 로그인, Discord 토큰과 자기 사용자 ID)에 답하면 봇이 뜬다. UID가 1000이 아닌 컴퓨터에서도 같다. 다시 실행하면 이미 된 것은 묻지 않고, 넣은 값은 그대로다. API key로 쓰는 provider는 USAGE의 손 설정을 따른다.
- 스크립트 없이 Compose를 쓰다 data 폴더에 쓸 수 없으면, 그 폴더의 주인과 컨테이너가 도는 번호, 고치는 방법을 알리고 멈춘다.
- `provider status`가 runtime이 쓸 provider와, 그 인증이 runtime이 실제로 쓸 자리에 있는지 알린다. Codex 로그인도 인증으로 본다.
- Docker가 넘기는 빈 provider 환경 변수는 인증 저장소와 config(`.env.json` 포함)에 둔 key를 가리지 않는다. 저장할 때와 달라진 선언 때문에 건너뛰던 인증도 다시 로그인하거나 key를 다시 가져오면 다시 쓰인다.
- Discord 토큰은 CLI 명령 하나로 `.env.json`에 들어가고 `config.json`에는 `${이름}`만 남는다. Docker 밖에서도 같은 명령을 쓴다.
- provider 인증이 없는 gateway는 지금처럼 꺼지지 않고, 할 일을 시작 출력에 알린다.

이 스펙은 다른 스펙의 구현을 기다리지 않는다. 049가 공식 image에 브라우저를 더해도 이 스펙의 계약은 바뀌지 않는다.

## 소유 범위

1. **provider 인증 확인(`shacs-bot provider status`):**
   - provider는 runtime과 같은 코드로 고른다. 후보는 030 credential runtime이 모으는 것(config의 provider 항목과 인증 저장소의 항목)이고, 고르는 규칙은 003 `ProviderRegistry::match_provider`와 config의 `agents.defaults.provider`·`model`이다. config는 runtime처럼 `${이름}`을 풀어 읽는다. 그래서 config와 인증 저장소에 항목이 없고 환경 변수만 있는 provider는 runtime처럼 고르지 않는다.
   - 고른 provider의 credential은 runtime(030)과 같은 선언, 같은 자리 순서, 같은 자리 규칙으로 보고 runtime이 쓸 첫 자리를 찾는다.
     - 선언된 환경 변수: process 환경에 그 이름이 있으면 이 자리다. 소유 범위 9 뒤에는 값이 빈 글이 아닐 때만 이 자리다(PRD 000은 빈 값도 이 자리로 본 그때 runtime을 그대로 따랐다).
     - 인증 저장소: 선언이 허용하고, 항목 종류(API key, OAuth)가 선언과 맞고, 항목에 선언 fingerprint가 없거나 지금 선언과 같으면 이 자리다. PRD 000 때 provider 로그인과 `import-key`는 fingerprint를 적지 않는다(소유 범위 9 뒤에는 저장할 때의 선언 fingerprint를 적는다). fingerprint가 다르면 runtime처럼 이 자리를 건너뛴다.
     - credential command: 선언이 있으면 이 자리다.
     - config 값: `${이름}`을 푼 값이 비어 있지 않으면 이 자리다.
     - 요청마다 오는 runtime override는 보지 않는다.
   - 같은 코드: 지금 runtime 해석(`ProviderCredentialRuntime`)은 자리를 고르는 중에 credential command를 실행하고 OAuth를 갱신한다. PRD 000은 provider 고르기와 runtime이 쓸 첫 자리 찾기를, config·선언·process 환경·인증 저장소 내용만 읽고 아무것도 실행·갱신·기록하지 않는 함수로 떼어 낸다. runtime 해석은 그 함수가 고른 자리에서 지금처럼 값을 얻고(command 실행, OAuth 갱신, 상태 기록), `provider status`와 소유 범위 2는 그 함수만 부른다. PRD 000에서 runtime이 고르는 provider와 자리, 그 결과는 바뀌지 않는다. 그 뒤 PRD 040이 소유 범위 9의 두 가지(빈 환경 변수, 저장할 때의 fingerprint)만 바꾼다.
   - 판정: runtime이 쓸 자리의 값이 비어 있지 않으면 `ready`다. credential command 자리면 실행하지 않아 값을 모르므로 `ready`로 보되 확인하지 않았다고 적는다. 그 자리의 값이 비어 있으면 `missing`이고 어느 자리인지 적는다. 고를 provider가 없거나 쓸 자리가 없으면 `missing`이다. 인증 저장소 항목을 fingerprint가 달라 건너뛰어 쓸 자리가 없으면, 그 provider의 credential 선언(`credentialSource`, `apiKeyRef`)이 저장할 때와 달라졌다고 적고 선언을 그때로 되돌리라고 알린다. PRD 000에서는 다시 로그인하거나 `import-key`해도 남은 fingerprint가 지워지지 않아 그것을 할 일로 알리지 않는다. 소유 범위 9 뒤에는 다시 로그인하거나 가져오는 것도 할 일로 알린다. 만료된 OAuth 항목은 갱신 token이 있으면 `ready`로 보고 첫 호출에서 갱신한다고 적는다. 갱신 token이 없으면 runtime이 쓰지 못하므로 `missing`이고 다시 로그인하라고 알린다.
   - 값이 맞는지는 보지 않는다. 네트워크를 쓰지 않고, credential command를 실행하지 않고, OAuth를 갱신하지 않고, 아무 파일도 쓰지 않는다. 갱신이나 command가 실패하면 runtime이 첫 호출에서 지금처럼 인증이 필요하다는 오류를 낸다.
   - 출력 첫 줄은 `provider: ready <provider>` 또는 `provider: missing <provider 또는 none>`이다. 다음 줄은 쓸 자리나 까닭이고, `missing`이면 할 일(로그인 명령, key 가져오기, 고칠 설정)이다. 두 결과 모두 종료 코드 0이다. config, `.env.json`, 인증 저장소를 읽지 못하면 지금 CLI처럼 오류를 보이고 종료 코드 1이다.
   - 031·048 marker가 있어도 판정은 같다. 읽기만 하고, 끊긴 어느 단계에서도 config가 풀리기 때문이다(소유 범위 4의 `--status`와 같다). 다만 marker가 있으면 할 일로는 `runtime config-migrate --recover`만 알린다. 로그인과 `import-key`는 marker를 보지 않고 config와 인증 저장소를 쓰므로, 끊긴 migration 중에 알리면 `--recover`가 되돌리지 못하게 된다. 읽기만 하므로 gateway가 돌고 있을 때도 실행할 수 있다.
   - process 환경이 판정에 들어가므로 runtime과 같은 환경에서 실행해야 맞는다. Docker에서는 gateway와 같은 Compose 환경을 받는 `shacs-cli` 서비스로 실행한다.
   - 비밀 값은 보이지 않는다.
2. **provider 인증이 없는 gateway:**
   - gateway는 지금처럼 provider 인증이 없어도 뜨고 꺼지지 않는다.
   - 소유 범위 1의 확인이 `missing`이면 시작 출력에 그 사실과 할 일을 한 줄로 적는다. 공식 container 안(023 official-container 증거)이면 `docker compose run --rm shacs-cli …` 꼴, 밖이면 `shacs-bot …` 꼴이다.
   - 확인이 인증 저장소 같은 파일을 읽지 못해 실패하면 비밀 값 없는 경고 한 줄(`provider status`로 확인하라는 안내)을 적고 지금처럼 계속 뜬다. 지금 runtime은 credential을 첫 호출에서야 읽으므로, 그 파일을 읽지 못해도 시작은 막히지 않는다.
   - 그 상태에서 메시지가 오면 지금처럼 답한다. 이 스펙은 그 답을 바꾸지 않는다.
3. **migration 중의 `onboard`:** `onboard`와 `onboard --wizard`는 config를 읽거나 무엇을 쓰기 전에 031 config migration과 048 비밀 옮기기의 marker를 확인한다. 하나라도 있으면 아무 파일도 만들거나 바꾸지 않고, runtime 시작과 같은 오류(`MigrationInterrupted`)로 `runtime config-migrate --recover`를 알리며 실패한다. 끊긴 migration 중에 config를 고치면 `--recover`가 되돌리지 못하기 때문이다(048 PRD 010 범위 7). marker가 없으면 지금과 같다.
4. **Discord 연결 명령(`shacs-bot channels connect discord`):**
   - 토큰은 stdin으로만 받는다(`--token-stdin`). 명령줄 인자로 받지 않는다. stdin이 terminal이면 입력을 화면에 보이지 않게 하고, 입력이 끝나거나 오류나 신호(SIGINT, SIGTERM)로 끝나도 terminal의 입력 표시를 되돌린다. 이미 `Cargo.lock`에 있는 crate(`libc`)로 하고, 안 되면 멈추고 사용자에게 묻는다. 앞뒤 공백을 지운 토큰이 비면 실패한다.
   - 이름은 048 이름 규칙대로 `channels.discord.token`의 `SHACS_CHANNELS_DISCORD_TOKEN`이다.
   - 아래이면 아무것도 바꾸지 않고 멈추고, 고치는 방법을 알린다. `--replace`를 줘도 같다.
     - 031 config migration이나 048 비밀 옮기기의 marker가 있다(`runtime config-migrate --recover`). 그 사이 파일을 고치면 `--recover`가 되돌리지 못하기 때문이다.
     - `token` 칸이 비어 있지도 `${SHACS_CHANNELS_DISCORD_TOKEN}`도 아니다. 글자 값이면 `runtime config-migrate --apply`로 먼저 옮기라고, 다른 `${이름}`이면 손으로 고치라고 알린다.
     - `botToken`·`bot_token` 칸에 값이 있다. runtime은 이 칸을 `token`보다 먼저 읽는다.
     - 명령을 실행한 process 환경 변수에 `SHACS_CHANNELS_DISCORD_TOKEN`이 비어 있지 않은 다른 값으로 있다. 048 해석 순서에서 이 값이 이겨 `.env.json`을 바꿔도 쓰이지 않는다. Docker에서는 gateway와 같은 Compose 환경을 받는 `shacs-cli` 서비스로 실행해야 이 확인이 맞는다(048 `processEnvDiffers`와 같다).
   - `.env.json`에 그 이름이 다른 값으로 있으면 멈춘다. `--replace`를 주면 그 값만 바꾸고 config는 아래처럼 고친다.
   - 쓰는 것: `.env.json`의 그 이름(없을 때와 `--replace`일 때)과, config `channels.discord`의 `enabled: true`, `token: "${SHACS_CHANNELS_DISCORD_TOKEN}"`, 그리고 `--allow-from <id>`(여러 번 줄 수 있다)를 주면 그 목록으로 바꾼 `allowFrom`. `--allow-from`이 없으면 `allowFrom`은 그대로다. 다른 칸과 `.env.json`의 다른 항목은 그대로다. 바꿀 것이 없으면 아무 파일도 쓰지 않는다.
   - `.env.json`을 config보다 먼저 쓴다. 그래서 config가 이 이름을 가리키는데 값이 없는 상태(runtime이 config를 풀지 못해 시작하지 못한다)가 생기지 않는다. 둘 사이에서 끊기면 config는 그대로 읽힌다. 이미 연결돼 있었으면 새 토큰으로 연결되고, 처음 연결이었으면 연결되지 않은 채다. 같은 토큰으로 다시 실행하면 끝난다.
   - config는 `${이름}`을 풀지 않고 읽고 쓴다(`resolve_env: false`). 풀린 값을 config에 다시 쓰지 않는다.
   - 두 파일은 명령이 처음 읽은 byte와 지금이 같을 때만 바꾼다. 048 PRD 050의 `write_config_value_if_unchanged`로 쓴다(0600 임시 파일을 쓰고 동기화한 뒤 다시 읽어 같으면 원자적으로 교체). `.env.json`이 그 사이 바뀌었으면 아무 파일도 쓰지 않고, config가 바뀌었으면 config를 쓰지 않고 실패하며 다시 실행하라고 알린다. config만 쓰지 못한 상태는 위의 끊긴 상태와 같다.
   - 결과 `allowFrom`이 비어 있으면 봇이 볼 수 있는 누구나 봇을 쓴다고 출력에 적는다.
   - 출력은 바꾼 칸의 경로와 이름만 보인다. 토큰은 출력과 오류에 나오지 않는다.
   - `--status`는 아무것도 쓰지 않고 첫 줄에 `discord: connected` 또는 `discord: not connected`와 그 까닭을 알린다. 두 결과 모두 종료 코드 0이고, config나 `.env.json`을 읽지 못하면 오류와 종료 코드 1이다. marker는 보지 않는다. 읽기만 하고, 048은 끊긴 어느 단계에서도 config가 풀리게 하기 때문이다.
   - `connected`는 runtime이 Discord worker를 띄우는 조건과 같다. `enabled`가 `true`이고, runtime이 읽는 순서(`botToken`, `bot_token`, `token`)로 각 칸을 runtime처럼 풀고 앞뒤 공백을 지웠을 때 비어 있지 않은 값이 하나라도 있다. 칸의 `${이름}`이 어디에도 없으면 runtime은 config를 읽지 못해 시작하지 못하므로, 그 까닭으로 `not connected`다. config의 다른 칸은 풀지 않는다.
5. **처음 설정 스크립트(`scripts/docker-setup.sh`):**
   - 어느 폴더에서 불러도 스크립트가 든 저장소의 루트(스크립트 폴더의 부모)에서 그 `docker-compose.yml`로 돈다. `docker`와 Compose v2(`docker compose version`)가 없으면 그 사실을 알리고 멈춘다.
   - POSIX `sh`로 쓴다. bash 전용 문법과 `jq`·`python` 같은 host 도구를 쓰지 않아 macOS와 Linux에서 같게 돈다. 출력 글은 CLI처럼 영어다.
   - **실행 사용자:** root(UID 0)로 실행하면 아무것도 바꾸지 않고 멈춘다. `.env`에 0이 적혀 컨테이너가 관리자로 돌게 되기 때문이다.
   - **data 폴더:** Compose가 mount하는 `~/.shacs-bot`이 없으면 실행한 사용자로 만든다. 있는데 그 사용자가 쓸 수 없으면 아무것도 바꾸지 않고, 지금 주인과 고치는 명령(`sudo chown -R "$(id -u):$(id -g)" ~/.shacs-bot`)을 알리고 멈춘다. 스크립트는 `sudo`를 부르지 않는다.
   - **사용자 번호:** 저장소 루트의 `.env`(Compose가 자동으로 읽고 `.gitignore`에 있다)에 `SHACS_UID`·`SHACS_GID`를 실행한 사용자의 값으로 적는다. 다른 줄과 있던 `.env`의 권한은 그대로 둔다. 다른 값이 있으면 바꾼다고 알리고 바꾼다. 스크립트는 `.env`에 이 두 줄 말고는 쓰지 않는다. 새로 만드는 `.env`의 권한은 0600이다. `.env.example`대로 사용자가 provider key를 적는 파일이기 때문이다. 실행한 shell이 다른 `SHACS_UID`나 `SHACS_GID`를 내보내고 있으면(Compose는 그 값을 `.env`보다 먼저 쓴다) 스크립트의 Compose 명령에는 맞는 값을 주고, 그 변수를 지우라고 알린다.
   - **Compose 호출:** 컨테이너 명령은 모두 `docker compose run --rm -T shacs-cli …`로 pseudo-TTY 없이 부른다. 그래야 stdin으로 넘긴 값이 그대로 가고, 스크립트가 읽는 출력에 terminal 줄바꿈 글자가 섞이지 않는다.
   - **image와 처음 설정:** `docker compose build`, 그다음 `onboard`. `onboard`는 지금처럼 config가 있으면 있는 값을 바꾸지 않고 빠진 기본값만 더한다. 끊긴 migration이 있으면 소유 범위 3대로 멈추고, 스크립트도 멈춘다.
   - **provider:** 소유 범위 1의 `provider status`가 `ready`면 묻지 않는다. `missing`이면 그 줄을 보이고 고르게 한다.
     1. ChatGPT 계정(Codex): `provider codex login --headless`. 이 명령은 지금처럼 Codex를 기본 provider와 model로 고른다.
     2. 건너뛰기: 나중에 할 명령을 알린다.

     로그인 뒤 `provider status`를 다시 보고 아직 `missing`이면 그 줄을 알린다. API key provider는 USAGE의 손 설정(Compose `.env`의 key, `provider <id> import-key`)을 가리킨다.
   - **Discord:** 소유 범위 4의 `channels connect discord --status`가 `connected`면 묻지 않는다. 아니면 연결할지 묻는다. 연결하면 토큰(숨긴 입력)과 답할 사용자 ID를 받아, 토큰은 `--token-stdin`의 stdin으로, ID는 `--allow-from`으로 넘긴다. 사용자 ID를 비우면 봇이 볼 수 있는 누구나 봇을 쓰고, gateway가 명령 실행·파일 쓰기 도구를 켠 채 돈다는 것을 알리고 다시 확인을 받는다. 확인하지 않으면 ID를 다시 묻는다. 연결하지 않기로 하면 나중에 할 명령을 알린다. 다른 channel은 USAGE의 손 설정을 가리킨다.
   - **시작:** gateway가 돌고 있는지 Compose로 먼저 본 뒤 `docker compose up -d shacs-gateway`를 한다. image가 바뀌었으면 `up -d`가 컨테이너를 새로 만든다. 돌고 있었고 이번 실행에서 로그인이나 Discord 연결이 성공했으면 `docker compose restart shacs-gateway`로 다시 띄운다. gateway는 config(기본 provider·model, channel 설정)를 시작할 때 읽고 channel worker도 그때 띄우기 때문이다(credential 값은 지금처럼 첫 호출에서 읽는다). 그다음 로그 보기(`docker compose logs -f shacs-gateway`)와 상태 확인(`provider status`, `runtime inspect`) 명령을 알린다.
   - **실패:** 폴더, 사용자 번호, image, `onboard`, 두 확인 명령(`provider status`, `--status`), `up -d`, `restart`가 실패하면 그 출력을 보이고 그 뒤 명령 없이 0이 아닌 종료 코드로 멈춘다. 로그인과 Discord 연결이 실패하면 그 출력을 보이고 다음 단계로 간다(gateway는 소유 범위 2의 안내와 함께 뜬다).
   - **terminal이 아닐 때:** stdin이 terminal이 아니면 묻지 않는다. 폴더, 사용자 번호, image, `onboard`, 두 확인, 시작만 하고 남은 일과 그 명령을 알린다.
   - **비밀 값:** Discord 토큰은 숨긴 입력(`stty -echo`)으로 받아 shell 변수에서 `printf`(shell 내장 명령)로 `docker compose run -T`의 stdin에만 넘긴다. 출력, `.env`, 어느 process의 명령줄 인자나 환경 변수에도 남기지 않는다. 숨긴 입력 중에 끝나거나 신호를 받아도 terminal의 입력 표시를 되돌린다.
   - 다시 실행해도 된다. 이미 맞은 것은 확인만 하고 지나간다.
6. **컨테이너 HOME:** 공식 image는 HOME(`/home/shacs`)을 `/tmp`처럼 누구나 쓰고 자기 것만 지울 수 있게(1777) 둔다. 그래서 1000이 아닌 번호로 돌아도 도구가 HOME에 캐시와 설정을 만든다. 주인(1000), `USER shacs`, data 폴더 mount 자리는 그대로다. HOME의 data 폴더 밖 내용은 지금처럼 컨테이너를 다시 만들면 사라진다.
7. **알아듣기 쉬운 권한 오류:**
   - data 폴더에 쓰는 명령(`onboard`, `run`, `serve`, `runtime` 아래 쓰는 명령, provider 인증 명령, `channels connect`, `config-migrate` 등. 정확한 목록은 PRD 030이 정한다)은 시작할 때 data 폴더에 쓸 수 있는지 먼저 확인한다. data 폴더가 없으면 그것을 만들 부모 폴더를 본다.
   - 쓸 수 없으면 원시 I/O 오류 대신 폴더 경로, 그 주인의 UID:GID, 실행 중인 UID:GID, 고치는 방법을 알리고 실패로 끝난다. 폴더가 `root` 소유면 Docker가 없는 폴더를 만들면 그렇게 된다고 적는다.
   - 고치는 방법은 공식 container 안이면 `scripts/docker-setup.sh`와 `.env`의 `SHACS_UID`·`SHACS_GID`, 밖이면 `chown`이다.
   - 그 밖의 I/O 오류는 지금처럼 보인다.
8. **문서:** README와 `docs/USAGE.md`의 Docker Compose 설명을 스크립트 중심으로 바꾸고, 지금의 손 순서는 "스크립트 없이" 절로 남긴다. `SHACS_UID`·`SHACS_GID`와 `.env`의 뜻, 폴더를 미리 만들지 않으면 생기는 문제, `provider status`, migration 중의 `onboard`, `channels connect discord`를 적는다. Docker에서 provider API key를 넣는 방법(Compose `.env`의 provider 환경 변수, config의 provider 항목, `provider <id> import-key`)을 README와 USAGE의 Docker provider secret 문장에 맞게 적는다. 소유 범위 9 전에는 인증 저장소·`.env.json`에 둔 key가 빈 환경 변수에 가려진다고 적고(PRD 000), 소유 범위 9가 그 문장을 고친 동작으로 바꾼다(PRD 040). USAGE의 "Provider 설정이 없거나 … runtime은 시작하지 않으므로" 문장 중 provider 설정이 없을 때의 부분은 확인한 동작(소유 범위 2)에 맞게 고친다. 선택 도구의 인증이 빠졌을 때의 부분은 그 동작을 확인해 맞게 적는다.
9. **빈 provider 환경 변수와 다시 저장한 인증(사용자 결정 2026-11-09):**
   - runtime credential 해석(030)에서 선언된 환경 변수는 값이 빈 글이 아닐 때만 그 자리다. 빈 글이면 없는 것으로 보고 다음 자리(인증 저장소, credential command, config 값)로 간다. 048의 `${이름}` 풀기가 빈 process 값을 건너뛰는 것과 같은 뜻이다(공백만인 값은 둘 다 값으로 본다). 비어 있지 않은 값은 지금처럼 먼저 쓰인다.
   - 이 규칙은 소유 범위 1의 자리 찾기 함수 하나에 둔다. 그래서 runtime, `provider status`, gateway 안내가 같이 바뀐다.
   - 새 인증을 인증 저장소에 저장하는 provider 인증 명령(`provider codex login`, `provider codex import-token`, `provider copilot import-token`, `provider <id> import-key`)은 그 provider의 선언 fingerprint를 저장할 때의 지금 선언(runtime이 쓰는 같은 선언 함수로 정한 것)의 것으로 적는다. 남아 있던 다른 fingerprint도 이 값으로 바뀐다. 그래서 다시 로그인하거나 가져오면 새 인증이 쓰이고, 그 뒤 선언이 바뀌면 지금처럼 stale로 건너뛴다. 다른 provider의 fingerprint와 항목은 그대로다.
   - `provider status`는 fingerprint가 달라 건너뛴 인증에 대해 선언을 되돌리거나 다시 로그인·가져오라고 알린다.
   - 문서: README·USAGE·`.env.example`의 빈 환경 변수 경고를 고친 동작으로 바꾼다. 비어 있지 않은 provider 환경 변수는 여전히 저장한 key보다 먼저 쓰인다는 것과, 가져온 key도 이제 선언 fingerprint를 가진다는 것을 적는다.

## 기존 owner와의 경계

| 기존 owner | 052가 소비하는 것 | 052가 소유하지 않는 것 |
|---|---|---|
| 015·023 | 공식 image, Compose, 처음부터 일반 사용자로 실행, official-container 증거 | image 구성(052는 HOME 권한만 바꾼다), Compose 안전 속성(그대로 지킨다), containment 판정 |
| 029 | runtime 시작과 시작 출력 | runtime lifecycle, owner lease |
| 003 | provider 고르기(`ProviderRegistry`, `agents.defaults`) | 고르는 규칙 |
| 010 | provider 인증 명령, 비밀 다루기 | 인증 명령의 동작(아래 "앞선 스펙에서 바꾸는 것" 말고) |
| 030 | credential 해석(자리 순서, 인증 저장소 읽기, fingerprint, 갱신, 상태) | 해석 규칙과 인증 저장소 형식(아래 "앞선 스펙에서 바꾸는 것" 말고) |
| 031 | config 읽기·쓰기, credential 선언 저장, config migration marker | config schema, migration |
| 048 | `.env.json` 형식, `${이름}` 해석 순서, 이름 규칙, 충돌 규칙, 바뀜 확인 쓰기(`write_config_value_if_unchanged`), 비밀 옮기기 marker | 아래 "앞선 스펙에서 바꾸는 것" 말고 그 규칙 |
| 049 | 없음 | 브라우저 image 구성. 049는 브라우저 임시 폴더를 이미 HOME이 아닌 `/tmp`에 둔다. 052는 049를 기다리지 않는다 |

## 앞선 스펙에서 바꾸는 것

052는 아래 앞선 스펙의 계약을 바꾼다. 앞선 스펙 문서는 고치지 않고, 052가 닫힌 뒤에는 이 표의 "바뀐 뒤"가 지금 계약이다.

| 스펙 | 지금 계약(위치) | 바뀐 뒤 | PRD |
|---|---|---|---|
| 048 | SPEC 소유 범위 3의 마지막 항목과 PRD 010 범위 11: `onboard`와 `onboard --wizard`는 비밀 값을 받지 않는다. 사용자는 비밀 값을 `.env.json`에 직접 적고 config에 `${이름}`을 쓰거나, config에 적은 뒤 `runtime config-migrate`로 옮긴다. PRD 010 Non Scope: "비밀 값을 받아 `.env.json`에 쓰는 새 명령" | 세 번째 길을 더한다. `channels connect discord`가 Discord 토큰을 받아 048 이름 규칙·충돌 규칙과 바뀜 확인 쓰기(0600 임시 파일, 원자적 교체)로 `.env.json`에 쓰고 config에는 `${이름}`을 남긴다(소유 범위 4). 048 Non Scope의 새 명령은 이 명령 하나이고 Discord 토큰만 받는다. `onboard`와 wizard는 그대로 비밀 값을 받지 않는다 | 010 |
| 030 | SPEC 반드시 지킬 것 6과 PRD 003 범위 1·6: credential 해석은 runtime override, environment, local auth store, provider config의 precedence를 결정적으로 적용하고, environment·literal·command 값을 푼다. 빈 환경 변수에 대한 말은 없고, 구현은 선언된 환경 변수의 이름만 있으면 값이 비어도 environment 자리로 썼다 | environment 자리는 값이 빈 글이 아닐 때만이다(048 `${이름}` 풀기와 같은 뜻). 빈 글이면 없는 것으로 보고 local auth store, command, provider config 순서로 간다. 순서와 비어 있지 않은 값의 우선은 그대로다(소유 범위 9) | 040 |
| 030, 010 | 030 PRD 003 범위 4: source fingerprint와 stale detection. 구현은 OAuth 갱신 때만 fingerprint를 적는다. 010 SPEC "현재 구현" 5번의 Codex·Copilot token import·login 처리와 그 뒤 더해진 `provider <id> import-key`(`import_provider_api_key`)는 인증 저장소의 provider 항목만 바꾸고 fingerprint를 적거나 지우지 않는다. 그래서 갱신 뒤 선언을 바꾼 provider는 다시 로그인해도 인증이 건너뛰어진다 | 이 명령들이 새 인증을 저장할 때 그 provider의 fingerprint를 그때 선언의 것으로 적는다. 가져온 API key도 fingerprint를 가지게 되어, 그 뒤 선언이 바뀌면 다른 인증처럼 stale로 건너뛴다. 갱신 때 적고 다르면 건너뛰는 stale detection은 그대로다(소유 범위 9) | 040 |

아래는 바꾸지 않는다.

- 023의 Packaging containment layer 요구(runtime이 일반 사용자로 돈다, data mount를 좁힌다, Docker socket·`privileged`·nested daemon이 없다). 023 SPEC이 적은 `useradd --create-home --uid 1000`은 현 상태의 근거이고, 소유 범위 6의 HOME 권한은 그 사용자와 `USER shacs`를 바꾸지 않는다.
- `runtime inspect` readiness의 `provider_auth` 판정.
- 030 credential 해석의 나머지(자리 순서, 인증 저장소 규칙, OAuth 갱신, 상태 기록). PRD 000이 자리 찾기를 함수로 떼어 내도 runtime이 고르는 provider와 자리, 그 결과는 같다. 위 표의 두 줄만 PRD 040이 바꾸고, `provider status`는 같은 함수라 그 바뀐 동작을 그대로 보인다.
- 031·048 marker의 뜻과 `--recover`. 052는 `onboard`도 그 marker에서 멈추게 할 뿐이다(소유 범위 3).

## 반드시 지킬 것

- 공식 image는 처음부터 일반 사용자로 돈다. 스크립트는 root로 돌지 않고, `sudo`를 부르지 않으며, 관리자 권한으로 data 폴더 주인을 바꾸지 않는다.
- 052의 스크립트와 명령은 비밀 값을 명령줄 인자, 환경 변수, 출력, 로그, Compose `.env`, `config.json`에 쓰지 않는다. `.env.json`과 인증 저장소에만 쓴다. 사용자가 `.env.example`대로 Compose `.env`에 provider key를 적는 지금의 방식은 그대로 둔다.
- 스크립트는 `.env`의 사용자 번호 두 줄과 사용자가 고른 것 말고 사용자의 config, 인증, `.env`의 다른 줄을 바꾸지 않는다.
- 스크립트와 `channels connect`는 다시 실행해도 같은 결과다. `provider status`와 `channels connect discord --status`는 아무것도 쓰지 않는다.
- `provider status`와 runtime은 provider 고르기와 credential 자리 순서·규칙을 같은 코드(소유 범위 1에서 떼어 낸 함수)로 정한다. 따로 구현해 어긋나게 두지 않는다.
- 시작할 때의 쓰기 확인은 미리 알리는 것뿐이다. 확인을 지나도 실제 쓰기는 지금처럼 실패할 수 있고, 그때의 오류는 지금과 같다.
- Cargo 테스트는 가짜 `docker`, 임시 폴더, 임시 폴더에 복사한 저장소 꼴로 돈다. 사용자의 data 폴더와 실제 저장소의 `.env`를 쓰지 않는다.
- `Cargo.lock`에 새 crate를 더하지 않는다. 숨긴 입력과 terminal 시험은 `libc`(termios, pty)로 한다. 그것으로 안 되면 멈추고 사용자에게 묻는다.

## 수용 조건

1. provider 확인(Cargo):
   - 아래는 `ready`다. config가 Codex를 고르고 인증 저장소에 `provider codex login`이 만든 꼴(fingerprint 없음)의 Codex OAuth 항목만 있는 경우(만료됐고 갱신 token이 있는 항목 포함). 기본 config(provider `auto`, model `anthropic/…`)에 `import-key`가 만든 꼴(fingerprint 없음)의 Anthropic API key 항목만 있고 `ANTHROPIC_API_KEY`가 process 환경에 없는 경우. 인증 저장소 항목의 fingerprint가 지금 선언과 같은 경우. config에 `anthropic` 항목이 있고 `ANTHROPIC_API_KEY`가 비어 있지 않은 경우. config의 `apiKey`가 `.env.json`으로 풀리고 `ANTHROPIC_API_KEY`가 없는 경우. credential command가 선언된 경우(실행하지 않았다고 적는다).
   - 아래는 `missing`이고 까닭과 할 일을 보인다. 인증이 하나도 없는 경우. config와 인증 저장소에 항목 없이 `ANTHROPIC_API_KEY`만 있는 경우. config가 고른 provider가 아닌 다른 provider에만 인증이 있는 경우. 인증 저장소에 Anthropic key가 있지만 `ANTHROPIC_API_KEY`가 빈 값으로 있는 경우(빈 환경 변수라고 적는다. PRD 000 때 동작이고, 소유 범위 9 뒤에는 수용 조건 11대로 `ready`다). 인증 저장소 항목만 있고 그 fingerprint가 지금 선언과 다른 경우(선언을 되돌리라고 적는다. PRD 000에서는 로그인을 알리지 않고, 소유 범위 9 뒤에는 다시 로그인·가져오기도 알린다). 만료됐고 갱신 token이 없는 Codex OAuth 항목만 있는 경우(다시 로그인).
   - 같은 경우 표로 runtime의 provider·credential 해석(030)을 돌리면, `ready`인 경우 같은 provider와 같은 자리의 값을 쓰고(command와 만료된 OAuth는 시험용 command·갱신기의 값), `missing`인 경우 provider를 고르지 못하거나 해석이 실패하거나 빈 값을 쓴다.
   - 확인 전후 config, `.env.json`, 인증 저장소가 byte 단위로 같고, 네트워크, credential command, OAuth 갱신이 불리지 않는다. 031·048 marker가 있어도 첫 줄은 같고, 할 일은 `runtime config-migrate --recover`뿐이며 로그인·`import-key` 안내가 없다.
   - 두 결과 모두 종료 코드 0이고, 읽을 수 없는 config와 인증 저장소는 종료 코드 1이다. 출력에 비밀 값이 없다.
2. gateway 안내(Cargo):
   - `missing`인 config로 `run`의 시작 출력에 안내 한 줄이 있고 runtime이 계속 돈다.
   - `ready`면 그 줄이 없다.
   - 인증 저장소를 읽을 수 없으면 비밀 값 없는 경고 한 줄이 있고 runtime이 계속 돈다.
   - container 안과 밖의 명령 꼴이 다르다.
3. migration 중의 `onboard`(Cargo): 031 marker와 048 marker 각각이 있을 때 `onboard`와 `onboard --wizard`가 `MigrationInterrupted`와 `--recover` 안내로 실패하고, config, `.env.json`, 인증 저장소가 byte 단위로 같으며, data 폴더와 workspace에 새 파일이 생기지 않는다. marker가 없으면 지금과 같다.
4. Discord 연결(Cargo):
   - `--token-stdin`으로 넣은 토큰이 `.env.json`(0600)의 `SHACS_CHANNELS_DISCORD_TOKEN`에 있고, config에는 `enabled: true`, `${SHACS_CHANNELS_DISCORD_TOKEN}`, 준 `allowFrom`이 있으며, 다른 칸과 `.env.json`의 다른 항목은 그대로다. runtime config 읽기가 그 토큰을 풀고, `--status`가 `connected`다.
   - 같은 입력으로 다시 실행하면 파일이 바뀌지 않는다.
   - 출력과 오류에 토큰이 없다. 결과 `allowFrom`이 비면 경고가 있다.
   - 멈추는 경우(marker, 글자 토큰, 다른 `${이름}`, `botToken`·`bot_token` 값, 다른 값의 process 환경 변수)마다 `--replace`를 줘도 config와 `.env.json`이 byte 단위로 같다.
   - `.env.json`에 다른 값이 있으면 멈추고, `--replace`면 그 값만 바뀌며 config는 위처럼 연결된다.
   - `.env.json`을 쓴 뒤 config를 쓰기 전에 끊긴 상태에서 config는 지금처럼 읽히고 runtime이 시작할 수 있다. 처음 연결이었으면 Discord는 연결되지 않은 채다. 같은 토큰으로 다시 실행하면 `--replace` 없이 연결된다.
   - 명령이 읽은 뒤 `.env.json`이 바뀌면(시험 hook) 두 파일이 그 바뀐 상태 그대로이고, config가 바뀌면 config가 바뀐 상태 그대로이며, 둘 다 실패와 다시 실행하라는 안내로 끝난다.
   - terminal인 stdin에서 입력이 화면에 보이지 않고, 입력을 마친 뒤, 빈 토큰 오류 뒤, SIGINT 뒤, SIGTERM 뒤 terminal의 입력 표시가 되돌아온다(`libc` pty 시험).
   - `--status`가 아래와 같다. 연결된 경우는 `connected`. `enabled: false`, 빈 토큰, 어디에도 없는 `${이름}`은 `not connected`와 그 까닭. `${이름}`이 `.env.json`으로 풀리는 경우, process 환경 변수가 이기는 경우, `botToken`에 값이 있고 `token`이 빈 경우, `botToken`이 빈 값으로 풀리고 `token`에 값이 있는 경우는 `connected`. 모두 종료 코드 0이고 파일이 byte 단위로 같으며, marker가 있어도 같게 답한다. 읽을 수 없는 `.env.json`은 종료 코드 1이다.
5. 스크립트(Cargo, 가짜 `docker`, 임시 HOME):
   - 시험은 스크립트와 compose 파일을 임시 폴더에 복사한 저장소 꼴에서 돈다. 실제 저장소의 `.env`는 시험 전후 같다.
   - 없는 data 폴더를 실행한 사용자로 만들고, `.env`의 다른 줄을 지키며 `SHACS_UID`·`SHACS_GID`를 쓴다. 새 `.env`는 0600이고, 있던 `.env`의 권한은 그대로다.
   - shell이 다른 `SHACS_UID`와 `SHACS_GID`를 내보내도 가짜 `docker`가 받은 환경의 두 값이 맞고, 지우라는 안내가 있다.
   - root로 실행하면(시험은 가짜 `id`로 0을 돌려준다) 아무것도 바꾸지 않고 멈춘다. 쓸 수 없는 data 폴더도 같다.
   - 가짜 `docker`가 받은 명령이 `build` → `onboard` → `provider status` → (로그인) → `channels connect discord --status` → (연결) → gateway 확인 → `up -d shacs-gateway` 순서이고, 컨테이너 명령마다 `-T`가 있다.
   - gateway가 돌고 있었고 로그인이나 연결이 성공했을 때만 `restart`가 더해진다. 돌고 있었지만 아무것도 성공하지 않았거나 돌고 있지 않았으면 없다.
   - `provider status`가 `ready`이고 Discord가 `connected`면 묻지 않는다.
   - 질문은 `libc` pty로 만든 terminal에서 시험한다. 로그인 고르기, 건너뛰기, Discord 거절, 빈 사용자 ID를 확인하지 않아 다시 묻는 것, 빈 사용자 ID를 확인하는 것을 본다. 숨긴 입력 중 SIGINT를 받으면 terminal의 입력 표시가 되돌아온다.
   - 로그인이나 연결이 실패해도 `up -d`까지 간다. `build`, `onboard`, 확인 명령, `up -d`가 실패하면 그 뒤 명령 없이 0이 아닌 종료 코드로 멈춘다.
   - terminal이 아니면 묻지 않고, 두 확인은 돌리며, 남은 일을 알린다.
   - 넣은 토큰이 가짜 `docker`의 stdin에만 있고, 받은 명령줄 인자, 환경 변수, 스크립트 출력에 없다.
   - 두 번 실행해도 결과가 같다.
6. 실제 Docker(opt-in 스크립트, 사용자 승인 뒤). 사용자의 data 폴더와 실제 저장소의 `.env`는 쓰지 않는다.
   - 임시 폴더의 저장소 사본(`git worktree` 등)과 비어 있는 임시 data 폴더에서, UID가 1000이 아닌 사용자로 스크립트를 terminal 아닌 방식으로 돌리면 image가 만들어지고 gateway가 뜬다. image 이름 `shacs-bot:local`을 사본에서 다시 만든다는 것을 실행 전에 알린다.
   - data 폴더의 파일이 그 사용자 소유다. 컨테이너 안에서 HOME에 폴더를 만들 수 있다.
   - `runtime inspect`가 official-container 증거를 보이고, 시작 출력에 안내가 있다.
   - 이미 `shacs-gateway` 컨테이너가 있으면 아무것도 하지 않고 멈춘다. 끝나면 만든 컨테이너를 지운다.
7. image(Cargo): Dockerfile이 HOME을 1777로 두고, `USER shacs`와 `SHACS_RUNTIME_PACKAGE`는 그대로다.
8. 권한 오류(Cargo):
   - 쓸 수 없는 임시 data 폴더에서 소유 범위 7의 명령이 폴더 경로, 주인과 실행 UID:GID, 고치는 방법을 담은 글로 실패하고, 원시 `Permission denied (os error 13)`만 남기지 않는다. data 폴더가 없고 그 부모에 쓸 수 없을 때도 같다.
   - official-container 증거가 있을 때와 없을 때 고치는 방법이 다르다. 주인이 `root`인 경우의 글은 주인 번호를 넣어 만든 시험으로 확인한다.
   - 쓸 수 있는 폴더에서는 지금과 같이 돈다.
9. 문서가 소유 범위 8과 맞다(소유 범위 9의 문서 포함).
10. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다. 전체 workspace 테스트는 사용자 승인을 받아 실행한다.
11. 빈 환경 변수와 다시 저장한 인증(Cargo):
   - 수용 조건 1의 같은 경우 표에서, 인증 저장소의 Anthropic key와 빈 `ANTHROPIC_API_KEY`는 `ready`이고 runtime이 저장한 key를 쓴다. config `apiKey`(`.env.json`으로 푼 값)와 빈 환경 변수도 `ready`이고 runtime이 그 값을 쓴다. 비어 있지 않은 환경 변수는 지금처럼 저장한 key보다 먼저 쓰이고, 공백만인 값은 지금처럼 그 자리이며 `provider status`가 비었다고 알린다. 나머지 경우의 결과는 수용 조건 1과 같다.
   - binary에서 빈 `ANTHROPIC_API_KEY`와 `import-key`로 저장한 key가 있으면 `provider status`가 `ready`이고, 같은 상태로 띄운 gateway 시작 출력에 `provider: missing` 줄이 없다.
   - 네트워크 없는 로컬 가짜 Anthropic 서버로 보낸 runtime 요청에, 빈 `ANTHROPIC_API_KEY`가 있어도 저장한 key가 `x-api-key`로 실린다("확인한 것"의 재현).
   - `provider codex login`의 저장(시험 token), `provider codex import-token`, `provider copilot import-token`, `provider <id> import-key`가 그 provider의 fingerprint를 그때 선언의 것으로 적고, 다른 provider의 fingerprint와 항목은 그대로다. fingerprint가 다른 인증만 있던 provider를 다시 가져오면 `provider status`가 `ready`이고 runtime이 새 인증을 쓴다. 그 뒤 선언을 바꾸면 그 인증은 다시 stale로 건너뛴다.
   - fingerprint가 달라 건너뛴 인증의 `provider status` 안내에 선언 되돌리기와 다시 로그인·가져오기가 함께 있다.

위 조건은 052의 단독 종료 기준이다. 수용 조건 6은 사용자가 돌리지 않기로 결정할 수 있다. 그러면 그 PRD의 종료 확인에 결정과 이유, 확인하지 못한 것을 적고 scoped로 닫는다(048 수용 조건 10과 같다).

## 남은 위험과 한계

- 스크립트 없이 `docker compose up -d`만 하면, UID가 1000이 아닌 컴퓨터에서는 지금처럼 쓰기에 실패한다. 이제는 까닭과 고치는 방법을 알리고 멈출 뿐, 저절로 맞추지는 않는다. 설정 없이 바로 뜨는 방식(Hermes Agent)은 이 스펙의 범위가 아니다.
- Compose를 스크립트보다 먼저 돌려 Docker가 data 폴더를 `root`로 만들었으면, 사용자가 `sudo`로 주인을 고치거나 지워야 한다. 스크립트는 그 명령을 알릴 뿐 하지 않는다.
- 공식 Compose는 `ANTHROPIC_API_KEY`·`OPENAI_API_KEY`·`OPENROUTER_API_KEY`·`GROQ_API_KEY`·`DEEPSEEK_API_KEY`·`GEMINI_API_KEY`·`HF_TOKEN`을 host에서 정하지 않으면 빈 값으로 넘긴다. PRD 040 전에는 runtime이 그 빈 값을 먼저 써서, 인증 저장소나 config(`.env.json` 포함)의 key가 있어도 key 없이 요청한다(`provider status`는 `missing`과 그 까닭). PRD 040 뒤에는 빈 값이 저장한 key를 가리지 않는다. 다만 host에서 비어 있지 않은 값(공백만인 값 포함)을 내보내면 그 값이 저장한 key보다 먼저 쓰인다(030 순서 그대로). 스크립트의 provider 선택지는 그대로 ChatGPT 로그인뿐이고 API key provider는 USAGE의 손 설정을 따른다.
- PRD 040 뒤에는 빈 provider 환경 변수로 뒤 자리의 인증을 막을 수 없다. 그렇게 막던 실행(CI 등)은 인증 저장소의 key나 로그인, credential command, config 값(`.env.json`으로 푼 값 포함)을 쓰게 된다. `credentialSource`의 `localAuth: false`는 인증 저장소만 막는다. 모두 막으려면 그 provider의 저장한 인증, credential command, config `apiKey`(`${이름}` 포함)를 지운다.
- config와 인증 저장소에 항목이 없는 provider는 환경 변수가 있어도 runtime이 고르지 않는다(지금 그대로). `provider status`도 `missing`이다.
- `provider status`는 인증이 있는지만 본다. key가 틀렸거나, credential command가 값을 내지 못하거나, OAuth를 갱신하지 못하면 첫 메시지에서 지금처럼 오류가 난다.
- 인증 저장소의 fingerprint는 PRD 040 전에는 OAuth 갱신 때만 적히고, 뒤에는 로그인·가져오기 때도 적힌다. 적힌 뒤 사용자가 그 provider의 credential 선언을 바꾸면 runtime은 저장된 인증을 건너뛴다. PRD 040 전에 저장해 fingerprint가 없는 인증은 그대로 fingerprint가 없어(030 규칙대로 지금 선언의 것으로 본다), 다시 로그인하거나 가져올 때까지 선언이 바뀌어도 건너뛰지 않는다. PRD 040 전에는 다시 로그인하거나 `import-key`해도 남은 fingerprint가 지워지지 않아 계속 건너뛰고, `provider status`는 선언을 되돌리라고만 알린다. PRD 040 뒤에는 다시 로그인하거나 가져오면 그 provider의 fingerprint가 그때 선언의 것으로 바뀌어 새 인증이 쓰인다. 가져온 API key도 fingerprint를 가지므로, 가져온 뒤 `apiKeyRef`·`credentialSource`(환경 변수 이름 등)를 바꾸면 그 key는 건너뛰어지고 `provider status`가 선언을 되돌리거나 다시 가져오라고 알린다. 처음 실행에서는 생기지 않는다.
- `channels connect discord`의 바뀜 확인은 교체 직전의 다시 읽기까지만 본다. 그 뒤 교체까지의 짧은 틈에 다른 프로그램이 `.env.json`이나 config를 쓰면 그 쓰기는 덮일 수 있다(잠금이 없다. 048 PRD 050과 같다).
- `runtime inspect` readiness의 `provider_auth`는 지금처럼 API key만 보아 Codex 로그인을 `blocked`로 보인다. 052는 그 판정을 바꾸지 않는다.
- 스크립트의 provider 선택지는 ChatGPT 계정(Codex) 로그인뿐이다.
- provider 로그인처럼 config를 쓰는 다른 명령은 지금처럼 migration marker를 보지 않는다. 스크립트는 그보다 앞의 `onboard`에서 멈춘다.
- 컨테이너 HOME을 누구나 쓸 수 있게 둔다. 컨테이너 안의 process는 runtime과 그 자식뿐이고 모두 같은 번호라, 1000번으로 돌 때보다 넓어지는 쓰기는 없다. HOME 권한을 엄격히 보는 도구는 경고할 수 있다.
- Telegram 연결은 지금 runtime에 보낸 사람을 제한하는 설정(`allowFrom`)이 없다. 그래서 스크립트는 Discord만 연결한다. Telegram 제한은 이 스펙의 범위가 아니다.
- rootless Docker나 Podman은 사용자 번호가 다르게 이어진다. 그곳에서는 스크립트가 적는 번호가 맞지 않으므로 스크립트 없이 손 설정을 따르고, Podman `--userns=keep-id` 같은 방법을 문서로 안내한다.
- Docker Desktop(macOS, Windows)은 mount 폴더의 주인을 번역하므로 번호를 맞추지 않아도 대개 된다. 맞춰도 해가 없다. Windows는 WSL 안에서 스크립트를 돌린다.
- 게시된 image가 없어 스크립트가 직접 빌드한다. 처음에는 몇 분 걸리고 메모리를 쓴다.

## 실행 PRD와 독립 종료

PRD 문서는 구현을 시작할 때 쓴다. 각 PRD는 앞 번호의 닫힌 결과만 소비하고 번호 순서대로 구현한다. 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| 000 provider 확인과 gateway 안내 | 소유 범위 1, 2와 그 문서 | 없음 | 수용 조건 1, 2, 10 |
| 010 migration 중의 `onboard`와 Discord 연결 | 소유 범위 3, 4와 그 문서 | 없음 | 수용 조건 3, 4, 10 |
| 020 처음 설정 스크립트와 컨테이너 HOME | 소유 범위 5, 6과 Docker 설명 문서 | 000, 010 | 수용 조건 5, 6, 7, 10 |
| 030 권한 오류 | 소유 범위 7과 그 문서 | 020(고치는 방법에 스크립트를 적는다) | 수용 조건 8, 10 |
| 040 빈 provider 환경 변수와 다시 저장한 인증 | 소유 범위 9와 그 문서 | 000(자리 찾기 함수와 `provider status`) | 수용 조건 11, 10 |

수용 조건 9와 10은 각 PRD가 자기 변경에 대해 확인한다.
