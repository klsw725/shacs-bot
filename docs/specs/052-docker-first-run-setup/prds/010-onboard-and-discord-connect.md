# PRD 010. migration 중의 onboard와 Discord 연결

Status: Complete (Scoped)

Depends on: 없음. 031·048의 닫힌 계약만 쓴다. PRD 000과 서로 기대지 않고, 뒤 번호 PRD에 의존하지 않는다.

## Goal

끊긴 config migration 중에는 `onboard`가 아무것도 바꾸지 않고 멈추게 하고, Discord 토큰을 명령 하나로 `.env.json`에 넣고 config에는 `${이름}`만 남기는 `shacs-bot channels connect discord`와 그 상태를 읽기만 하는 `--status`를 만든다.

## Scope

1. **migration 중의 `onboard`:** `onboard()`의 맨 앞, `--wizard` 갈래보다 먼저 config 경로(`--config` 또는 기본 경로)로 `guard_config_migration_admission`을 부른다. 031 marker나 048 marker가 있으면 config를 읽거나 쓰기 전, 폴더·template·wizard 진행 표시를 만들기 전에 `MigrationInterrupted`로 실패한다.
   - `ConfigError::MigrationInterrupted`의 글을 `config migration is interrupted; run \`runtime config-migrate --recover\` first (marker <경로>)`로 바꾼다. 앞부분은 그대로라 이 글을 보는 시험(`spec031_runtime_layout_admission`, `spec048_secrets_migration_cli`)이 그대로 통과한다. runtime 시작과 `runtime config-migrate --apply`도 같은 글을 보인다.
2. **048 이름 함수:** `shacs-config`의 비밀 이름 규칙(`secret_places`의 칸 경로 → `SHACS_…`)을 `secret_env_name(keys: &[&str])`로 공개한다. `channels connect discord`는 `secret_env_name(&["channels", "discord", "token"])`(`SHACS_CHANNELS_DISCORD_TOKEN`)을 쓴다.
3. **`channels connect discord`:** `shacs-bot channels connect discord --token-stdin [--allow-from <id>]… [--replace] [--config <path>]`. `--token-stdin`이 없으면 인자 오류다. `--allow-from`의 값이 앞뒤 공백을 빼고 비면 인자 오류다. 순서:
   1. 031·048 marker가 있으면 멈춘다(범위 1과 같은 `MigrationInterrupted`).
   2. config 파일의 byte를 읽고(없으면 없음) JSON으로 읽는다(`${이름}`을 풀지 않고 migration도 하지 않는다. 지금 설정을 고치는 명령과 같다). JSON이 아니거나 최상위·`channels`·`channels.discord`가 객체가 아니면 실패한다.
   3. 아래면 멈추고 고치는 방법을 알린다. `--replace`와 상관없다.
      - `channels.discord.token`: 없음, `null`, 앞뒤 공백을 빼면 빈 글, 정확히 `${SHACS_CHANNELS_DISCORD_TOKEN}`이면 계속한다(`channels`·`channels.discord`가 없어도 계속한다). 그 밖에는 멈춘다. 정확히 다른 `${이름}` 하나면 그 칸을 손으로 고치라고, `${`가 섞인 글이나 글이 아닌 값(숫자, 객체 등)이면 손으로 고치라고, 그 밖의 글(글자 토큰)이면 `runtime config-migrate --apply`로 먼저 옮기라고 알린다.
      - `channels.discord.botToken`이나 `bot_token`이 글이고 앞뒤 공백을 뺀 값이 비지 않았다(runtime은 `botToken`, `bot_token`, `token` 순서로 처음 비지 않은 글을 쓴다).
   4. 토큰을 읽는다(범위 4). 앞뒤 공백을 빼고 비면 실패한다.
   5. 명령을 실행한 process 환경 변수 `SHACS_CHANNELS_DISCORD_TOKEN`이 비지 않았고 토큰과 다르면 멈춘다(048 `processEnvDiffers`와 같다. 그 값이 `.env.json`보다 먼저 쓰인다).
   6. `.env.json`의 byte를 읽고(없으면 없음) 048 형식으로 읽는다. 모양이 틀리면 그 문제만 적은 오류로 실패한다. 그 이름이 없으면 더하고, 같은 값이면 그대로 두고, 다른 값이면 `--replace`가 없을 때 멈추고 있을 때 그 값만 바꾼다.
   7. config의 `channels.discord`(없으면 만든다)에 `enabled: true`, `token: "${SHACS_CHANNELS_DISCORD_TOKEN}"`를 두고, `--allow-from`이 있으면 `allowFrom`을 그 목록(준 순서, 앞뒤 공백 뺌)으로 바꾼다. 다른 칸은 그대로다.
   8. 바뀐 파일만 쓴다. 이 명령 안의 두 파일 쓰기 함수가 순서를 맡는다.
      1. `.env.json`이 바뀌면 `write_config_value_if_unchanged(.env.json, 6의 byte, 새 값)`로 쓴다(0600 임시 파일, 동기화, 다시 읽어 같을 때만 교체). 새 `.env.json`은 이름 순서의 JSON 객체(048 쓰기와 같은 모양)다. 바뀌지 않으면 쓰지 않는다.
      2. config를 쓰기 전에 `.env.json`의 지금 byte가 기대한 byte(1에서 쓴 byte, 쓰지 않았으면 6의 byte)와 같은지 다시 본다. 다르면 config를 쓰지 않고 실패한다. 그 사이 다른 것이 그 이름을 지우면 config만 `${이름}`을 가리켜 runtime이 시작하지 못하기 때문이다.
      3. config가 바뀌면 `write_config_value_if_unchanged(config, 2의 byte, 새 값)`로 쓴다.
      - 실패는 단계마다 다른 글로 알리고 종료 코드 1이다. 모두 다시 실행하라고 덧붙인다.
        - 1의 `.env.json` 바뀜(`ConfigWriteError::Changed`): "`.env.json` changed while this command ran; nothing was written."
        - 2의 다시 보기에서 `.env.json`이 다름: 1에서 썼으면 "`.env.json` changed after the token was written to it; config was not changed.", 쓰지 않았으면 "`.env.json` changed while this command ran; nothing was written."
        - 3의 config 바뀜: 1에서 썼으면 "`config.json` changed while this command ran; `.env.json` holds the token but config was not changed.", 쓰지 않았으면 "`config.json` changed while this command ran; nothing was written."
      - 시험이 각 비교 직전에 파일을 바꿀 수 있게, 두 파일 쓰기 함수는 단계(`.env.json` 쓰기 전, config 쓰기 전)마다 부르는 관찰자를 받는다. 실행 경로는 아무것도 하지 않는 관찰자를 준다.
   9. 출력: `discord: connected`, 쓴 파일마다 그 파일과 칸 경로·이름(`updated: .env.json SHACS_CHANNELS_DISCORD_TOKEN`, `updated: config channels.discord.enabled, channels.discord.token`), 쓴 것이 없으면 `unchanged`. 결과 config에서 runtime이 읽는 보낸 사람 목록(`allowFrom`, `allowedSenders`, `allow_from`, `allowed_senders` 중 처음 있는 칸, `allowed_sender_config`)이 비면 `warning: … anyone the bot can see can use it`. 토큰은 출력과 오류에 나오지 않는다.
4. **숨긴 입력(`--token-stdin`):** stdin이 terminal이 아니면 끝까지 읽는다. terminal이면 `libc` termios로 입력 표시(ECHO)를 끈 뒤 stderr에 prompt를 적고(끈 뒤에 적어 prompt를 본 입력이 표시되지 않게) 한 줄을 읽는다.
   - guard 하나가 맡는다. 원래 termios는 guard가 가진 `Box`에 두어 주소가 바뀌지 않고, handler는 그 주소를 `AtomicPtr`로 읽는다.
   - 켜는 순서: SIGINT·SIGTERM을 막고(`pthread_sigmask(SIG_BLOCK)`) 그때 돌려받은 이전 mask를 기억한다 → `tcgetattr`로 원래 설정을 `Box`에 담고 그 주소를 `AtomicPtr`에 둔다 → 두 신호의 이전 `sigaction`을 기억하고 handler를 둔다 → ECHO를 끈다 → prompt를 쓰고 flush → 기억한 이전 mask로 되돌린다(`SIG_SETMASK`. 원래 막혀 있던 신호는 계속 막힌다). 중간에 실패하면 거기까지 한 것을 아래 순서로 되돌리고 오류로 끝난다.
   - 끄는 순서(guard의 `Drop`, 정상·오류 모두): 두 신호를 막고 그때의 mask를 기억한다 → 원래 termios로 되돌린다 → 이전 `sigaction`으로 되돌린다 → `AtomicPtr`를 비운다 → 기억한 mask로 되돌린다(그 사이 온 신호는 이전 동작으로 처리되고, 그때 terminal은 이미 되돌려져 있다). `Box`는 그 뒤에 놓인다.
   - handler는 `AtomicPtr`가 비어 있지 않으면 그 설정을 `tcsetattr`로 되돌리고, 그 신호를 기본 동작으로 바꾼 뒤 다시 보낸다(`tcsetattr`, `signal`, `raise`는 async-signal-safe). 그래서 process는 그 신호로 끝난다.
   - 이 명령은 그 사이 다른 thread를 띄우지 않는다. unsafe 블록마다 `// SAFETY:`를 적는다. 새 crate를 더하지 않는다.
5. **`channels connect discord --status`:** 아무것도 쓰지 않고 marker를 보지 않는다. 다른 인자(`--token-stdin`, `--allow-from`, `--replace`)와 같이 쓰면 인자 오류다.
   - config를 runtime처럼 읽되 풀지 않는다(`load_config_with_env`, `resolve_env: false`, `write_back_migrations: false`). `.env.json`과 process 환경으로 `config_env`를 만든다(모양이 틀린 `.env.json`이면 종료 코드 1).
   - `channels.discord`의 `botToken`·`bot_token`·`token` 중 글인 칸만 runtime과 같은 `interpolate_env_with_source`로 푼다. 하나라도 풀지 못하면(그 이름이 어디에도 없음) runtime이 config를 읽지 못해 시작하지 못하므로 `not connected`이고 그 칸과 이름을 까닭으로 적는다. config의 다른 칸은 풀지 않는다.
   - `connected`는 Discord 칸만 보아 runtime이 Discord worker를 띄우는 조건을 채운다는 뜻이다. runtime과 같은 함수로 정한다: `channel_enabled_from_plugins(…, "discord", false)`이고, 세 칸을 푼 plugin 값으로 `discord_runtime_config`가 값을 낸다. config의 다른 칸은 풀지 않으므로, 다른 칸의 `${이름}` 때문에 runtime이 시작하지 못하는 경우는 여기서 알 수 없다(남은 제한).
   - 출력: 첫 줄 `discord: connected` 또는 `discord: not connected`. 둘째 줄은 `connected`면 쓰는 칸(`token: channels.discord.<칸>`), 아니면 `reason: <까닭>`(`enabled`가 `true`가 아님, 토큰 없음, 풀지 못한 이름). 두 결과 모두 종료 코드 0, config·`.env.json`을 읽지 못하면 종료 코드 1. 값은 보이지 않는다.
6. **CLI 표면:** `channels` 도움말과 `parse_channels`에 `connect discord`를 더한다(다른 channel은 `unsupported channel` 인자 오류).
7. **문서:** `docs/USAGE.md`의 채널 절에 `channels connect discord`(인자, 멈추는 경우, 쓰는 것과 순서, `--status`, Docker에서 `docker compose run --rm shacs-cli`로 숨긴 입력을 쓰거나 `-T`와 pipe로 넣는 법), `.env.json` 절의 "`onboard`와 `onboard --wizard`는 비밀 값을 받지 않습니다" 줄에 Discord 토큰은 이 명령으로 넣을 수 있다는 것, config-migrate 절에 marker가 있으면 `onboard`도 멈춘다는 것. README 채널 절의 명령 목록에 한 줄. README와 USAGE의 Docker 손 설정 순서(지금 `vim`·`config-migrate --apply`만 있는 곳)에 `docker compose run --rm shacs-cli channels connect discord --token-stdin --allow-from <id>`와, 끊긴 migration이 있으면 `onboard`가 멈추고 `runtime config-migrate --recover`를 안내한다는 것을 더한다. 스크립트 중심으로 다시 쓰는 것은 PRD 020이다.

## Non Scope

- 다른 channel의 연결 명령, Telegram 보낸 사람 제한, 토큰이 맞는지 Discord에 물어보기.
- 처음 설정 스크립트와 컨테이너 HOME(PRD 020), 권한 오류(PRD 030).
- 048 비밀 옮기기와 `--recover`의 동작, `.env.json` 형식과 해석 순서.
- config 쓰기의 잠금. 다시 읽기와 교체 사이의 틈은 남은 제한이다(048 PRD 050과 같다).

## Parent Requirement Mapping

- [052 SPEC](../SPEC.md)의 소유 범위 3, 4와 소유 범위 8 중 그 문서(migration 중의 `onboard`, `channels connect discord`), "반드시 지킬 것"의 비밀 값·다시 실행·읽기만 하는 `--status`·새 crate 항목, 수용 조건 3, 4와 이 PRD 변경에 대한 수용 조건 9, 10을 소유한다. "앞선 스펙에서 바꾸는 것"의 048 행(비밀 값을 받아 `.env.json`에 쓰는 새 명령)을 이 PRD가 구현한다.

## Required Contract

1. marker가 있으면 `onboard`·`onboard --wizard`·`channels connect discord`는 아무 파일도 만들거나 바꾸지 않는다.
2. 멈추는 경우(범위 3의 1·3·5, 6의 `--replace` 없는 다른 값)에는 config와 `.env.json`이 byte 단위로 그대로다.
3. 토큰은 명령줄 인자, 환경 변수, 출력, 오류, log, config에 나오지 않는다. `.env.json`에만 쓴다.
4. `.env.json`을 config보다 먼저 쓰고, 두 파일 모두 처음 읽은 byte와 같을 때만 바꾼다.
5. 같은 입력으로 다시 실행하면 파일이 바뀌지 않는다. `--status`는 아무것도 쓰지 않는다.
6. 숨긴 입력은 어떻게 끝나도(정상, 오류, SIGINT, SIGTERM) terminal의 입력 표시를 되돌린다.
7. `Cargo.lock`에 새 crate를 더하지 않는다.

## Acceptance Criteria

모든 binary 시험은 `env_clear()` 뒤 `HOME`·`PATH`와 그 경우에 정한 변수만 넣은 환경으로 돌리고, 새 시험 module은 `tests/it/main.rs`에 등록한다(새 시험 실행 파일 없음).

1. migration 중의 `onboard`(`shacs-cli` `tests/it/`, binary): 031 marker와 048 marker 각각이 있을 때 `onboard`와 `onboard --wizard`가 종료 코드 1이고 오류에 `config migration is interrupted`와 `runtime config-migrate --recover`가 있으며, config·`.env.json`·`auth.json`이 byte 단위로 같고 data 폴더와 workspace의 파일 목록이 같다. marker가 없으면 `onboard`가 지금처럼 끝난다(기존 onboard 시험이 그대로 통과).
2. 연결(binary, stdin은 pipe):
   - `--token-stdin --allow-from 111 --allow-from 222`: `.env.json`(0600)에 `SHACS_CHANNELS_DISCORD_TOKEN`이 토큰으로 있고 다른 항목은 그대로다. config의 `channels.discord`에 `enabled: true`, `token: "${SHACS_CHANNELS_DISCORD_TOKEN}"`, `allowFrom: ["111", "222"]`가 있고 다른 칸(`channelIds` 등)과 config의 다른 부분은 그대로다. runtime config 읽기(`load_runtime_config_with_env`)가 그 토큰을 풀고 `discord_runtime_config`가 값을 내며, `--status`가 `connected`다.
   - 같은 입력으로 다시 실행하면 두 파일이 byte 단위로 같고 출력이 `unchanged`다.
   - stdout·stderr 어디에도 토큰이 없다. `--allow-from` 없이 연결해 결과 보낸 사람 목록이 비면 `warning:`이 있다.
   - config 파일이 없는 data 폴더에서 연결하면 `channels.discord`만 담은 config와 `.env.json`이 생기고 runtime config 읽기가 그 토큰을 푼다.
3. 멈춤(binary): marker 두 가지, 글자 토큰, 다른 `${이름}`, `botToken` 값, `bot_token` 값, 다른 값의 process 환경 변수 각각에서 `--replace`를 줘도 종료 코드 1이고 config와 `.env.json`이 byte 단위로 같으며 오류에 토큰이 없다. `.env.json`에 다른 값이 있으면 멈추고, `--replace`면 그 값만 바뀌고 config는 위처럼 연결된다.
4. 끊긴 상태(binary): `.env.json`에만 그 이름이 있고 config는 그대로인 상태(`.env.json`을 쓴 뒤 config를 쓰기 전에 끊긴 상태)에서 config는 runtime config 읽기로 읽히고, 처음 연결이었으면 `--status`가 `not connected`다. 같은 토큰으로 다시 실행하면 `--replace` 없이 연결된다.
5. 바뀜 확인(`shacs-cli` 단위 시험, 두 파일 쓰기 함수의 관찰자): `.env.json` 쓰기 직전에 `.env.json`이 바뀌면 두 파일이 그 바뀐 상태 그대로이고 오류가 `.env.json`을 말한다. config 쓰기 직전에 config가 바뀌면 `.env.json`만 새 값이고 config는 바뀐 상태 그대로이며 오류가 config를 말한다. `.env.json`을 쓰지 않는 경우(이미 같은 값)에도 config 쓰기 직전에 `.env.json`에서 그 이름이 지워지면 config를 쓰지 않고 오류가 `.env.json`을 말한다. 모두 다시 실행하라는 안내다.
6. 숨긴 입력:
   - `shacs-cli` `tests/it/`, `libc::openpty`로 만든 terminal을 stdin으로 binary를 띄운다. prompt가 보인 뒤 쓴 토큰이 terminal(master 쪽)에 표시되지 않고 연결이 끝난다. 입력을 마친 뒤, 빈 토큰 오류 뒤, SIGINT 뒤, SIGTERM 뒤 slave의 termios에 ECHO가 다시 켜져 있다. 신호 경우는 process가 그 신호로 끝난다.
   - `shacs-cli` 단위 시험: pty slave에 guard를 켜면 SIGINT·SIGTERM의 동작이 handler이고 ECHO가 꺼지며, guard를 놓으면 두 신호의 동작과 signal mask가 켜기 전 것과 같고(켜기 전에 막아 둔 다른 신호는 여전히 막혀 있다) ECHO가 다시 켜진다. process 전체의 신호 동작을 바꾸므로, 이 시험은 같은 시험 실행 파일을 `--exact <이름> --test-threads=1`과 표시 환경 변수로 다시 띄운 child process에서 돈다(`generated_media_release/tools_test.rs`와 같은 방식). 부모 쪽은 child의 성공만 본다.
   - 인자 해석과 token 칸 판정은 표 하나의 단위 시험으로 본다(`--status`와 다른 인자, `--token-stdin` 없음, 빈 `--allow-from`, token 칸의 계속·멈춤 갈래).
7. `--status`(binary): 연결된 경우 `connected`. `enabled: false`, 빈 토큰, 어디에도 없는 `${이름}`은 `not connected`와 그 까닭. `${이름}`이 `.env.json`으로 풀리는 경우, process 환경 변수가 이기는 경우, `botToken`에 값이 있고 `token`이 빈 경우, `botToken`이 빈 값으로 풀리고 `token`에 값이 있는 경우는 `connected`. 모두 종료 코드 0이고 파일이 byte 단위로 같으며, marker가 있어도 같게 답한다. 모양이 틀린 `.env.json`은 종료 코드 1이다.
8. 문서가 범위 7과 같다.
9. fmt(Google 옵션), workspace clippy `-D warnings`, 변경한 package(`shacs-config`, `shacs-cli`) 시험이 통과한다. `Cargo.lock`이 바뀌지 않는다.

## Closure Evidence

1. `onboard` 멈춤, 이름 함수, 연결·`--status`, 숨긴 입력, 문서의 구현과 시험.
2. 고친 기존 시험이 있으면 그 까닭.
3. 값이 없는 출력 예(연결, `unchanged`, 멈춤, `--status`).
4. Cargo 검증 기록과 `Cargo.lock` 확인.

PRD 010은 PRD 000, 020, 030 없이 닫힌다.

## 남은 제한

- 바뀜 확인은 교체 직전의 다시 읽기까지만 본다. 그 뒤 교체까지의 짧은 틈에 다른 프로그램이 쓰면 그 쓰기는 덮일 수 있다(잠금 없음).
- `--status`의 `connected`는 Discord 칸만 본다. config의 다른 칸의 `${이름}`을 풀지 못해 runtime이 시작하지 못하는 경우는 여기서 알 수 없고 runtime 시작 오류로 드러난다.
- process 환경 변수 확인은 명령을 실행한 환경으로 한다. Docker에서는 `shacs-cli` 서비스로 실행해야 gateway와 같은 Compose 환경을 받는다.
- 숨긴 입력의 신호 처리는 SIGINT·SIGTERM만 다룬다. SIGKILL처럼 잡을 수 없는 신호로 끝나면 terminal의 입력 표시가 꺼진 채 남을 수 있다(`stty echo`로 되돌린다).

## 구현 메모 및 검증 근거 (2026-11-09)

실행 기록은 로컬 `.omo/evidence/spec052/`(추적하지 않음)의 `plan-review.md`와 `prd010/`에 있다.

- 구현:
  - `shacs-config`: `secret_places.rs`의 `secret_env_name(keys)`(048 이름 규칙을 그대로 씀)를 공개. `ConfigError::MigrationInterrupted`의 글을 `config migration is interrupted; run \`runtime config-migrate --recover\` first (marker …)`로 바꿈(runtime 시작과 `config-migrate --apply`도 같은 글).
  - `shacs-cli` `lib.rs`: `onboard()`가 wizard 갈래보다 먼저 `guard_config_migration_admission`을 부름. `ChannelsCommand::Connect`, `channels connect` 해석, 도움말.
  - `shacs-cli` `hidden_input.rs`(새 파일): 범위 4의 `EchoOff` guard(신호를 막은 채 켜고 끄기, 이전 mask를 `SIG_SETMASK`로 되돌림, `Box`의 termios와 `AtomicPtr`, handler의 `tcsetattr`·`signal`·`raise`), terminal이 아니면 끝까지 읽기.
  - `shacs-cli` `discord_connect_cli.rs`(새 파일): 인자 해석, token 칸 판정(빈 글 판정만 앞뒤 공백을 빼고, `${이름}` 비교는 원래 글로), 멈춤, `.env.json` 같음·다름·`--replace`, config 고치기, 관찰자를 받는 두 파일 쓰기와 세 가지 실패 글, 출력, `--status`.
  - 문서: USAGE "Discord 연결" 절, `.env.json` 절의 onboard 줄, config-migrate 절의 marker 문장, Docker 손 설정 순서. README의 Docker 순서와 채널 절.
- 시험:
  - `shacs-cli` 단위 시험: `discord_connect_cli_tests.rs` 4개(인자·token 칸 표, `.env.json` 바뀜, config 바뀜, 같은 값인 `.env.json`에서 이름이 지워짐), `hidden_input_tests.rs` 2개(자기 실행 파일을 다시 띄운 child에서 신호 동작·mask·ECHO 되돌리기).
  - `shacs-cli` 통합 시험 `tests/it/spec052_discord_connect.rs` 9개(binary를 `env_clear`한 환경으로): onboard 멈춤(031·048 marker × 보통·wizard), 연결과 다시 실행·0600·다른 칸 유지·runtime 읽기, config 없는 연결과 경고, 멈춤 8가지(`--replace` 포함), 다른 저장 값과 `--replace`, 끊긴 상태에서 다시 연결, `--status` 7가지와 marker·모양이 틀린 `.env.json`, pty 숨긴 입력, 빈 토큰·SIGINT·SIGTERM 뒤 ECHO.
  - 고친 기존 시험: `lib.rs`의 `parser_handles_channels_list_and_status_options`가 보던 오류 글을 새 글("channels requires `list`, `status` or `connect`")로 바꿈. 하위 명령이 늘었기 때문이다. `MigrationInterrupted` 글을 보는 두 시험은 앞부분을 그대로 두어 고치지 않았다.
- 값이 없는 출력 예(로컬 실행):

  ```text
  discord: connected
  updated: .env.json SHACS_CHANNELS_DISCORD_TOKEN
  updated: config channels.discord.enabled, channels.discord.token, channels.discord.allowFrom
  ```

  같은 입력으로 다시 실행하면 `discord: connected` / `unchanged`, 다른 저장 값이면 `runtime error: discord connect stopped: .env.json has SHACS_CHANNELS_DISCORD_TOKEN with another value; add --replace to replace it`, `--status`는 `discord: connected` / `token: channels.discord.token`.
- Cargo 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings` 통과. 마지막 소스로 `shacs-config`+`shacs-cli` 653 passed / 0 failed / 3 ignored(로컬 `prd010/config-cli-tests-final.log`). `crates/Cargo.lock`은 바뀌지 않았다.
- Oracle 독립 검토: 계획 3차 **PASS**, 구현 2차 **PASS**(1차: 앞뒤 공백이 붙은 `${SHACS_CHANNELS_DISCORD_TOKEN}`을 받아들임, USAGE의 "만 남깁니다" 문장).
## 종료 확인 (2026-11-09)

- 전체 workspace 시험: 사용자 승인(2026-11-09)을 받아 PRD 000과 PRD 010을 담은 작업 트리에서 `cargo test --manifest-path crates/Cargo.toml --workspace --locked --no-fail-fast`를 돌렸다. suite 211개, 3,727 passed / 0 failed / 8 ignored, exit 0이다(로컬 `.omo/evidence/spec052/closure/workspace-tests.log`).
- 수용 조건 1–9를 위 시험과 검증으로 확인했고, 계획 3차·구현 2차 독립 검토가 PASS다. PRD 010은 `Complete (Scoped)`로 닫는다. 전체 출시 seal이 아니다.
- 남은 제한은 위 "남은 제한"과 같다.
