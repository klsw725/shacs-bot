# PRD 050. Discord 설정 질문

Status: Implemented (전체 workspace 시험·종료 확인 전)

Depends on: [PRD 010](010-onboard-and-discord-connect.md)(`channels connect discord`와 `--status`)과 [PRD 020](020-setup-script-and-container-home.md)(설정 스크립트의 Discord 단계)의 닫힌 결과. 012 runtime이 Discord 칸을 읽는 법(`discord_runtime_config`, inbound 거르기)을 그대로 쓴다. 뒤 번호 PRD에 의존하지 않는다.

## Goal

설정 스크립트가 Discord를 연결할 때 봇과 말할 수 있는 사람(`allowFrom`), 봇이 읽는 채널(`allowChannels`), 서버 채널에서 답하는 때(`groupPolicy`)를 각 설정이 하는 일을 설명하며 하나씩 묻는다. 이미 연결돼 있으면 지금 설정을 보여 주고, 토큰을 다시 넣지 않고 바꾸거나 Discord를 끄고(`enabled`) 다시 켤 수 있게 한다. `channels connect discord`는 같은 일을 하는 옵션을 받는다(SPEC 소유 범위 10).

## 지금 동작(2026-11-09 확인)

- `channels connect discord`는 `--token-stdin`이 있어야 하고 `enabled`, `token`, (준 경우) `allowFrom`만 쓴다. `--status`는 두 줄(`discord: connected`와 `token:` 또는 `discord: not connected`와 `reason:`)이다. 경고는 결과 보낸 사람 목록이 빌 때만 나온다(`*`는 보지 않는다).
- 설정 스크립트는 연결되지 않았을 때만 토큰과 사용자 ID 하나를 묻는다. 연결돼 있으면 Discord에 대해 아무것도 묻지 않는다. restart는 로그인이나 연결이 성공했고 gateway가 돌고 있었을 때다.
- runtime(012): `allowFrom`(먼저 있는 별칭 `allowFrom`·`allowedSenders`·`allow_from`·`allowed_senders`)이 비거나 `*`면 누구나. 채널 목록은 `channelIds`·`allowedChannelIds`·`allowChannels`·`channel_ids`·`allowed_channel_ids`·`allow_channels` 중 먼저 있는 칸에 `defaultChannelId`·`default_channel_id`·`channelId`·`channel_id`를 더한 것이고, 비거나 `*`면 모든 채널. `allowChannels`·`allow_channels` 칸이 없고 목록에 채널이 있으면 REST로 돌아보며 읽는다(`RestPolling`). 이때는 목록의 값마다 그 채널을 그대로 돌아보므로 `*`도 채널 ID 하나로 쓰이고(모든 채널이 아니다), 봇 메시지는 버리지만 `groupPolicy`는 적용하지 않는다. Gateway에서는 목록에 없는 채널(부모 채널 포함 비교, DM 포함)과 봇·자기 메시지를 버리고, 서버 채널(`guild_id`)에서 `groupPolicy`가 `open`이 아니면 멘션한 메시지만 받는다. DM도 사람·채널 거르기를 받는다. `streaming`은 읽기만 하고 쓰지 않고(보내기는 다 쓴 답만), `replyInThread`는 읽지 않는다.

## Scope

1. **옵션 해석(`discord_connect_cli::parse`):** `DiscordConnectAction`을 세 갈래로 바꾼다: `Connect { settings, existing }`(토큰 있음), `Settings(settings)`(토큰 없음), `Status`. `DiscordSettings`는 바꿀 설정만 담는다(없으면 그대로).
   - `users: Option<Users>`: `--allow-from <id>`(여러 번) → `Only(목록)`, `--allow-anyone` → `Anyone`. 둘을 같이 주면 인자 오류.
   - `channels: Option<Channels>`: `--allow-channel <id>`(여러 번) → `Only(목록)`, `--all-channels` → `All`. 둘을 같이 주면 인자 오류.
   - `group_policy: Option<GroupPolicy>`: `--group-policy mention|open`. 다른 값, 값 없음, 서로 다른 값으로 두 번 주면 인자 오류.
   - `enabled: Option<bool>`: `--enable` → `true`, `--disable` → `false`. 둘을 같이 주면 인자 오류. `--disable`과 `--token-stdin`을 같이 주면 인자 오류.
   - ID 목록: 앞뒤 공백을 빼고 비면 인자 오류(지금 `--allow-from`과 같은 글), 준 순서, 같은 값은 처음 한 번. 그 밖의 글자는 거르지 않는다.
   - `--token-stdin`이 있으면 `Connect`. 없고 설정 옵션이 하나라도 있으면 `Settings`. 둘 다 없고 `--status`도 없으면 인자 오류 `channels connect discord requires --token-stdin, a setting option or --status`. `--replace`는 `--token-stdin` 없이 주면 인자 오류. `--status`는 다른 어떤 옵션과도 같이 주면 인자 오류(지금 글의 옵션 목록을 넓힌다).
2. **설정을 config에 적용(`connected_config`를 넓힌 하나의 함수):** `channels.discord` 객체(없으면 만든다)에 아래 칸을 이 순서로 비교해 다르면 넣고, 바뀐 칸 경로를 그 순서로 돌려준다.
   - `Connect`: `enabled: true`, `token: "${SHACS_CHANNELS_DISCORD_TOKEN}"`(지금과 같다), 그다음 설정 칸.
   - `Settings`: `enabled`(준 경우만), 그다음 설정 칸.
   - 설정 칸: `allowFrom`(`Only` → 목록, `Anyone` → `[]`), `allowChannels`(`Only` → 목록, `All` → `[]`), `groupPolicy`(`"mention"`·`"open"`).
   - 다른 칸과 다른 별칭 칸은 건드리지 않는다.
3. **멈추는 경우(두 갈래 모두, 아무 파일도 쓰지 않음, `discord connect stopped: …`):**
   - 지금 PRD 010의 멈춤(marker, 토큰 칸, `botToken`·`bot_token`, process 환경의 다른 토큰, `.env.json`의 다른 값)은 `Connect`에 그대로다. `Settings`는 marker(`MigrationInterrupted`)와 아래만 본다.
   - **먼저 읽히는 채널 칸:** `channels`를 주었고, 지금 `channels.discord`에 `channelIds`나 `allowedChannelIds` 키가 있거나 `defaultChannelId`·`default_channel_id`·`channelId`·`channel_id` 중 하나가 앞뒤 공백을 빼고 비지 않은 글이면 멈추고 그 칸 이름과 손으로 지우라는 말을 알린다.
   - **REST로 읽는 설정의 `groupPolicy`:** `group_policy`를 주었고, 적용한 뒤의 `channels.discord`를 runtime의 채널 판정(`discord_channel_config`)에 넣으면 REST polling(`allowChannels`·`allow_channels` 키 없음, 목록에 채널 있음)이면 멈추고, `groupPolicy`가 쓰이지 않는다는 것과 `--allow-channel`로 채널을 다시 정하거나 예전 칸을 `allowChannels`로 옮기라는 말을 알린다.
4. **토큰 없는 설정 바꾸기(`Settings`):** 순서: marker → config byte 읽기(없으면 토큰이 없으므로 멈춤) → JSON·객체 확인(PRD 010과 같은 오류) → 토큰이 있는지(범위 5의 판정에서 `enabled`를 뺀 것: `botToken`·`bot_token`·`token` 중 글인 칸을 `.env.json`과 process 환경으로 풀 수 있고, 푼 plugin 값으로 `discord_runtime_config`가 값을 냄) → 없으면 멈추고 `discord is not connected yet; connect it with --token-stdin` → 범위 2·3 → 바뀐 칸이 없으면 `unchanged` → 있으면 관찰자(`WriteStep::BeforeConfig`)를 부른 뒤 `write_config_value_if_unchanged(config, 처음 byte, 새 값)`. 바뀌었으면 `config.json changed while this command ran; nothing was written. Run it again.`
   - 새 토큰을 묻거나 stdin에서 읽지 않는다. 있는 토큰 칸은 토큰이 있는지 정하려고 메모리에서만 풀고(`--status`와 같다) 출력하지 않는다. `.env.json`은 읽기만 하며 쓰지 않는다.
   - 출력: 첫 줄 `discord: settings updated`, 그다음 `updated: config <칸>, …` 또는 `unchanged`, 결과 사람 목록이 비거나 `*`가 있으면 경고 줄(범위 5).
   - PRD 030의 확인 목록(`data_folder_access::write_target`)에 `Settings`를 더한다. 이 쓰임도 data 폴더의 config를 쓰기 때문이다(지금은 `Connect`만 확인한다).
5. **`--status`:** 지금의 판정 순서(풀지 못한 토큰 칸 → `enabled` → 토큰)와 첫 두 줄은 그대로다. 토큰 칸이 모두 풀리고 푼 plugin 값으로 `discord_runtime_config`가 값을 내면(연결됨, 또는 `enabled`가 `true`가 아니라서만 연결되지 않음) `token: channels.discord.<칸>`(연결되지 않았을 때는 `reason:` 다음)과 아래 세 줄을 더한다. 값은 그 runtime 값에서 온다.
   - `users: anyone`(목록이 비거나 `*` 있음) 또는 `users: <id>, <id>`
   - `channels: all`(`AllVisible`, 또는 Gateway에서 목록에 `*`) 또는 `channels: <id>, <id>`(REST polling이면 `*`를 포함해 적힌 목록 그대로)
   - `server channels: when mentioned`(`Mention`), `server channels: every message`(`Open`), REST polling이면 `server channels: every message (channelIds polling does not apply groupPolicy)`
   - 줄 순서는 첫 줄, (연결되지 않았으면) `reason:`, `token:`, `users:`, `channels:`, `server channels:`다. 토큰이 없거나 풀지 못한 토큰 칸이 있으면 지금처럼 첫 줄과 `reason:` 두 줄뿐이다.
   - 경고(`Connect`와 `Settings`의 출력): runtime이 읽는 결과 보낸 사람 목록(`allowed_sender_config`, 별칭 포함)이 비거나 `*`가 있으면 `warning: the Discord sender list is empty or has *; anyone the bot can see can use it`. 지금 글(`channels.discord has no allowFrom list`)은 `*`일 때와 다른 별칭 칸일 때 맞지 않아 바꾼다(뒷부분은 같다).
6. **설정 스크립트(`scripts/docker-setup.sh`):** `--status` 출력으로 상태를 정한다: 첫 줄 `discord: connected` → 연결됨, 아니고 `token: `으로 시작하는 줄이 있으면 → 꺼짐, 아니면 → 토큰 없음. terminal이 아니면 묻지 않는다(토큰 없음 → 지금처럼 출력과 연결 할 일, 꺼짐 → 출력과 `Turn Discord on: docker compose run --rm shacs-cli channels connect discord --enable` 할 일, 연결됨 → 아무것도 하지 않음).
   - **질문 글(정확한 영어):**
     - 세 질문 앞: `The bot answers only the people, channels and messages you choose here.`
     - 사람: 설명 두 줄 `Who can talk to the bot: the bot answers only the Discord users listed here.` / `  To copy a user ID, turn on Developer Mode (User Settings > Advanced), then right-click the user and choose Copy User ID.`, 질문 `Discord user IDs, separated by spaces (empty allows anyone): `. 비우면 지금의 경고 줄과 `Continue without a user ID? [y/N]: `(확인하지 않으면 사람을 다시 묻는다).
     - 채널: 설명 `Which channels the bot reads: empty means every channel the bot can see, plus direct messages.` / `  With channel IDs (right-click a channel > Copy Channel ID) it reads only those channels and their threads, and a direct message only if you list its channel ID.`, 질문 `Discord channel IDs, separated by spaces (empty for all): `.
     - 답하는 때: `When the bot answers in server channels (this choice does not apply to direct messages):` / `  1) Only when someone mentions it (@the bot)` / `  2) Every message in the channels it reads`, 질문 `Choose 1 or 2 [1]: `. 1이나 빈 답 → `mention`, 2 → `open`, 그 밖 → 다시 묻는다.
     - 위험한 조합(사람 비움 + 2): `With no user IDs and every message, anyone in those channels can make the bot run commands and change files without mentioning it.`, `Answer every message anyway? [y/N]: `. 확인하지 않으면 답하는 때를 다시 묻는다.
     - ID 질문: 답에 숫자·공백·쉼표 밖의 글자가 있으면 `Use only the digits of each ID, separated by spaces.`를 보이고 다시 묻는다. 공백·쉼표로 나눈 각 숫자가 옵션 하나(`--allow-from <id>`, `--allow-channel <id>`)가 된다. 빈 사람(확인 뒤) → `--allow-anyone`, 빈 채널 → `--all-channels`, 답하는 때 → `--group-policy mention|open`.
   - **토큰 없음:** `Connect a Discord bot now? [y/N]: `(지금과 같다) → 토큰(숨긴 입력, 지금과 같다) → 세 질문 → `printf '%s\n' "$token" | cli channels connect discord --token-stdin <옵션들>`.
   - **연결됨:** `Discord is connected:`와 `--status`의 `users:`·`channels:`·`server channels:` 줄, `  1) Keep these settings` / `  2) Change them` / `  3) Turn Discord off (the token stays saved)`, `Choose 1, 2 or 3 [1]: `. 1(빈 답)은 아무 명령도 부르지 않는다. 2는 세 질문 뒤 `cli channels connect discord <옵션들>`. 3은 `cli channels connect discord --disable`. 그 밖의 답은 다시 묻는다.
   - **꺼짐:** `Discord is set up but turned off:`와 세 설정 줄, `Turn Discord on? [y/N]: `. 켜면 `Change these settings too? [y/N]: ` → 바꾸면 세 질문 → `cli channels connect discord --enable [옵션들]`. 켜지 않으면 아무 명령도 부르지 않고 켜는 명령을 할 일로 알린다.
   - 질문 중 입력이 끝나면(EOF) Discord 단계를 그만두고(명령 없음) 다음으로 간다. 토큰은 지금처럼 다룬다(숨긴 입력, 변수 비우기, stdin으로만).
   - 옵션은 숫자 ID와 고정된 옵션 이름만으로 만든 글을 따옴표 없이 펼쳐 넘긴다(POSIX `sh`라 배열이 없다). ID를 나눌 때는 `set -f`로 pathname 확장을 끄고 `IFS`를 공백·쉼표로 둔 뒤 되돌린다.
   - **restart:** Discord 명령(연결·설정 바꾸기·켜기·끄기)이 종료 코드 0이고 그 출력에 `unchanged` 줄이 없으면 `discord_changed`. gateway가 돌고 있었고 로그인이나 `discord_changed`면 restart한다. 명령이 실패하면 그 출력과 `The Discord change did not finish.`를 보이고 다음으로 간다(할 일 줄은 상태대로).
7. **문서:** USAGE "Discord 연결" 절(새 옵션과 충돌, 토큰 없는 설정 바꾸기와 켜고 끄기, `--status`의 설정 줄, 멈추는 다른 채널 칸과 REST polling의 `groupPolicy`), "설정 스크립트로 처음 실행"의 6번(세 질문, 연결됨·꺼짐의 메뉴), channel 설정 줄(`channels.discord`: 각 칸의 뜻, `groupPolicy`는 Gateway에서만, REST polling의 `*`, `streaming`·`replyInThread`는 지금 효과 없음). README는 채널 절의 `channels connect discord` 줄과 Docker 설정 스크립트 문단에 각각 한 문장.

## 앞선 PRD에서 바꾸는 것(052 안)

| PRD | 지금 | 바뀐 뒤 |
|---|---|---|
| 010 | `--token-stdin` 없이는 인자 오류. `--status`는 두 줄. 경고는 보낸 사람 목록이 빌 때 | 설정 옵션과 토큰 없는 설정 바꾸기(`discord: settings updated`), 켜고 끄기. `--status`는 토큰이 있으면 `token:`과 설정 세 줄을 더한다. 경고는 목록이 비거나 `*`가 있을 때 |
| 020 | 토큰 없을 때 토큰과 사용자 ID 하나만 묻고, 연결돼 있으면 묻지 않음. restart는 연결 성공 때 | 세 설정 질문, 연결됨·꺼짐 메뉴. restart는 Discord 명령이 성공하고 `unchanged`가 아닐 때 |
| 030 | data 폴더 확인 목록의 Discord는 토큰으로 연결할 때(`Connect`)만 | 토큰 없는 설정 바꾸기(`Settings`)도 확인한다 |

PRD 010·020·030 문서는 닫힌 기록이라 고치지 않는다.

## Non Scope

- `streaming`·`replyInThread`를 동작하게 만들기, REST polling에 `groupPolicy` 더하기, 다른 channel의 설정 질문.
- 예전 별칭 칸을 대신 옮기거나 지우기(멈추고 알린다).

## Parent Requirement Mapping

- [052 SPEC](../SPEC.md)의 소유 범위 10과 그 문서, 수용 조건 12와 이 PRD 변경에 대한 수용 조건 9, 10을 소유한다.

## Required Contract

1. 주지 않은 설정과 다른 칸은 바뀌지 않는다. 멈추는 경우와 인자 오류에는 config와 `.env.json`이 byte 단위로 그대로다.
2. 토큰 없는 설정 바꾸기는 새 토큰을 묻거나 stdin에서 읽지 않고(저장된 토큰 칸은 메모리에서만 풀어 있는지 정하고 출력하지 않는다) `.env.json`을 쓰지 않으며, config를 처음 읽은 byte와 같을 때만 쓴다.
3. 쓴 값은 runtime이 같은 뜻으로 읽는다(먼저 읽히는 다른 칸이 있거나 `groupPolicy`가 쓰이지 않는 설정이면 쓰지 않는다).
4. 토큰은 지금처럼 stdin으로만 가고 출력·명령줄·환경·로그에 없다. 사용자·채널 ID는 비밀 값이 아니다.
5. 같은 입력으로 다시 실행하면 파일이 바뀌지 않고 restart도 없다.
6. `Cargo.lock`에 새 crate가 없다. 스크립트는 POSIX `sh`와 지금 쓰는 도구만 쓴다.

## Acceptance Criteria

시험은 `Given`/`When`/`Then` 주석을 쓴다.

- 단위 시험(`shacs-cli` `discord_connect_cli_tests.rs`):
  1. 해석 표: 각 옵션의 결과(`Connect`·`Settings`·`Status`, 목록의 공백 빼기·중복 한 번, 같은 `--group-policy` 값 두 번은 받음), 충돌(`--allow-from`+`--allow-anyone`, `--allow-channel`+`--all-channels`, `--enable`+`--disable`, `--disable`+`--token-stdin`, 다른 `--group-policy` 두 번, 잘못된 값), 옵션 없는 호출, 토큰 없는 `--replace`, `--status`와 다른 옵션.
  2. 적용 표: `Connect`·`Settings` 각각의 바뀐 칸 목록과 결과 객체(되돌리기 포함), 같은 값이면 빈 목록, 다른 칸과 별칭 칸 그대로. 결과 config로 runtime의 `discord_runtime_config`와 worker 조건(`channel_enabled_from_plugins`)이 준 값을 낸다.
  3. 먼저 읽히는 칸 표(`Connect`와 `Settings` 각각, 함수 호출로 config와 `.env.json`이 byte 단위로 같은지 봄): `channelIds`·`allowedChannelIds`(빈 배열이어도 키가 있으면) → 채널을 바꿀 때 멈춤, 비지 않은 `defaultChannelId`·`default_channel_id`·`channelId`·`channel_id` → 멈춤, 빈 글 → 멈추지 않음, `channel_ids`·`allowed_channel_ids`·`allow_channels`만 → 멈추지 않고 runtime이 쓴 `allowChannels`를 읽음, 채널을 바꾸지 않으면 멈추지 않음. `Connect`가 멈추면 토큰도 읽지 않는다(멈춤 판정이 토큰 읽기보다 먼저다).
  4. REST polling 표(`Connect`와 `Settings` 각각, 두 파일이 byte 단위로 같은지와 `Connect`가 토큰 읽기 함수를 부르지 않는지 봄): `channel_ids`에 채널이 있고 `allowChannels`·`allow_channels`가 없을 때 `--group-policy`만 → 멈춤, `--allow-channel`과 같이 → 바꿈(Gateway), `--allow-from`만 → 바꿈. `channelIds`에 채널이 있으면 `--group-policy`만 → 멈춤. `defaultChannelId`나 `default_channel_id`만 있어도(목록에 더해져 REST polling) `--group-policy`만 → 멈춤.
  5. 토큰 없는 바꾸기의 관찰자: config 쓰기 직전에 config를 바꾸면 쓰지 않고 다시 실행하라는 오류이고 config는 바뀐 그대로, `.env.json`은 byte 단위로 같다.
  - 토큰 없는 바꾸기의 토큰 판정 표(함수 호출): `botToken`이나 `bot_token`에만 토큰이 있으면 바꾼다. 앞 칸(`botToken`)은 풀지 못하고 뒤 칸(`token`)은 풀리면 멈춘다. 꺼짐이고 토큰 있음이면 바꾼다. 멈출 때마다 두 파일이 byte 단위로 같다.
  - 경고 표: 결과 보낸 사람 목록이 `[]`, `["*"]`, 다른 별칭 칸(`allowedSenders: ["*"]`)일 때 경고 줄이 정확히 범위 5의 글이고, ID 목록이면 경고가 없다.
  6. `--status` 줄 표(`.env.json`과 환경을 넣은 함수 호출, 줄 전체를 순서대로 비교): 연결됨(누구나·목록·`*`, 모든 채널·Gateway의 `*`·목록, `mention`·`open`), `botToken`·`bot_token`에 토큰이 있을 때의 `token:` 줄, 꺼짐이고 토큰 있음(설정 줄 있음), 꺼짐이고 토큰 없음(두 줄), 켜짐이고 토큰 없음(두 줄), 앞 칸(`botToken`)은 풀지 못하고 뒤 칸(`token`)은 풀리는 경우(풀지 못한 이름의 두 줄, 설정 줄 없음), REST polling(`channels:`에 `*`를 포함한 적힌 목록, `server channels:`의 polling 글).
  - PRD 030의 목록 표(`data_folder_access_tests.rs`)에 `channels connect discord --allow-anyone`(확인함)을 더한다.
- binary 시험(`shacs-cli` `tests/it/spec052_discord_connect.rs`, PRD 010 시험에 더함, `env_clear`):
  7. 토큰으로 연결하며 세 설정 옵션을 주면 config에 그 값이 있고 `--status`의 세 줄이 같은 값을 보인다. 같은 입력으로 다시 실행하면 `unchanged`이고 두 파일이 byte 단위로 같다. 토큰은 출력에 없다.
  8. 토큰 없는 바꾸기: 연결된 상태에서 `--allow-anyone --all-channels --group-policy open`이 그 칸만 바꾸고(첫 줄 `discord: settings updated`, 경고 줄), `.env.json`이 byte 단위로 같다. `--disable` 뒤 `--status`가 연결되지 않음과 설정 줄을 보이고, `--enable`로 다시 연결된다. 토큰이 없는 data 폴더(빈 폴더, 그리고 `token`이 빈 config)에서는 멈추고 파일이 그대로다. 031·048 marker가 있으면 멈추고 파일이 그대로다.
  9. 인자 오류(대표 셋)는 종료 코드 1이고 파일이 그대로다.
  - PRD 030 binary 시험(`tests/it/spec052_data_folder_access.rs`)의 확인 명령 목록에 `channels connect discord --allow-anyone`을 더한다(쓸 수 없는 data 폴더에서 같은 안내로 멈춤).
- 스크립트 시험(`shacs-cli` `tests/it/spec052_setup_script.rs`, PRD 020 시험을 고치고 더함): 가짜 `docker`가 `--status` 출력(`FAKE_DISCORD`)과 Discord 명령의 출력(`FAKE_DISCORD_OUTPUT`, 기본은 바뀐 칸 줄)·종료 코드를 정하고, 토큰 없는 Discord 명령도 기록한다.
  10. 토큰 없음: 토큰 다음에 세 설명과 질문이 순서대로 나온다. 여러 ID(공백·쉼표)는 `--allow-from`·`--allow-channel` 여러 개가 되고, 빈 채널은 `--all-channels`, 빈 사람은 확인 뒤 `--allow-anyone`, 1·2는 `--group-policy mention|open`이다. `12ab`는 다시 묻고 그 사이 명령이 없다. 빈 사람 + 2에서 확인하지 않으면 답하는 때를 다시 묻고, 1을 고르면 `mention`으로 연결한다. 토큰은 Discord 명령의 stdin에만 있다.
  11. 연결됨: 설정 줄이 보이고, 1(빈 답)은 Discord 명령 없음·restart 없음, 2는 토큰을 묻지 않고 설정 옵션만 넘김, 3은 `--disable`.
  12. 꺼짐: 켜지 않으면 명령 없음과 켜기 할 일, 켜고 바꾸지 않으면 `--enable`만, 켜고 바꾸면 `--enable`과 설정 옵션.
  13. restart: gateway가 돌고 있었을 때 Discord 명령이 성공하고 출력에 `unchanged`가 없으면 restart, `unchanged`면 없음, 실패면 없음. 연결됨에서 1이면 없음.
  14. EOF: 세 질문과 메뉴 각각에서 입력이 끝나면 Discord 명령이 없고 스크립트가 0으로 끝난다.
  15. terminal이 아니면 연결됨·꺼짐·토큰 없음 모두 묻지 않고, 꺼짐이면 켜기 할 일 줄이 있다.
  - 고치는 기존 시험(까닭은 구현 메모에 적는다):
    - `spec052_setup_script.rs`의 질문 표: 사용자 ID 질문 글이 바뀌고 토큰 뒤에 채널·답하는 때 질문이 더해지므로 질문 상수와 답 순서, restart 경우(이제 Discord 명령 출력에 `unchanged`가 없어야 함)를 고친다.
    - `a_ready_provider_and_a_connected_bot_ask_nothing`: 연결된 Discord는 이제 메뉴를 묻는다. 이름과 내용을 "provider 질문은 없고 Discord 메뉴만 묻고 빈 답이면 명령이 없다"로 바꾼다.
    - `spec052_discord_connect.rs`·`discord_connect_cli_tests.rs`: `--status` 연결됨 출력 전체나 해석 오류 글·경고 글을 그대로 비교하는 곳이 있으면 새 꼴로 고친다.
    - `spec052_data_folder_access.rs`·`data_folder_access_tests.rs`: 확인 명령 목록에 한 줄을 더한다(위).
- 16. 문서가 범위 7과 같다.
- 17. 검증은 순서대로: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings`, `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q`, 스크립트 `sh -n`·`dash -n`·`bash --posix -n`. `Cargo.lock`이 바뀌지 않는다. 전체 workspace 시험은 사용자 승인 없이 돌리지 않는다.

## Closure Evidence

1. 바뀐 파일과 새 시험, 고친 기존 시험과 까닭.
2. 값이 없는 출력 예(`--status`의 설정 줄, 토큰 없는 바꾸기).
3. Cargo 검증 기록과 `Cargo.lock` 확인.

## 남은 제한

- `streaming`과 `replyInThread`는 지금 효과가 없다(SPEC 남은 위험).
- 예전 별칭 채널 칸(`channelIds` 등)이 있으면 채널과 (REST polling이면) 답하는 때를 바꾸지 못하고, 사용자가 손으로 칸을 지우거나 옮겨야 한다.
- 스크립트는 ID가 숫자인지만 본다. 그 ID가 실제 사용자·채널인지는 Discord에 묻지 않는다.
- config 쓰기는 교체 직전의 다시 읽기까지만 본다(PRD 010과 같다).

## 구현 메모 및 검증 근거 (2026-11-09)

실행 기록은 로컬 `.omo/evidence/spec052/`(추적하지 않음)의 `plan-review.md`와 `prd050/`에 있다.

- 구현:
  - `shacs-cli` `discord_connect_cli.rs`:
    - 범위 1의 세 갈래(`Connect { settings, existing }`·`Settings(settings)`·`Status`)와 `DiscordSettings`(`Users`·`Channels`·`GroupPolicy`·`enabled`), 옵션 충돌.
    - `connected_config`를 `discord_config`로 넓혔다. 토큰 이름을 주면 `enabled`·`token`, 아니면 준 `enabled`만 넣고, 그다음 `allowFrom`·`allowChannels`·`groupPolicy`를 넣는다.
    - `check_settings`(먼저 읽히는 채널 칸, REST polling의 `groupPolicy`)는 `Connect`에서 토큰 칸 확인 뒤·토큰 읽기 전에, `Settings`에서 토큰 판정 뒤에 부른다.
    - `change_settings`(범위 4).
    - `--status`와 토큰 판정이 같이 쓰는 `discord_reading`(풀지 못한 토큰 칸, `enabled`, 푼 plugin 값의 runtime 값, 토큰 칸 이름)과 `settings_lines`.
    - 경고는 runtime이 읽는 보낸 사람 목록(`allowed_sender_config`)으로 정하고 글을 바꿨다.
  - `shacs-cli` `data_folder_access.rs`: Discord는 `Status`가 아닌 갈래를 모두 확인한다(`Settings`를 더함).
  - `scripts/docker-setup.sh`:
    - `ask_ids`(숫자·공백·쉼표 밖의 글자가 있으면 다시 물음), `id_options`(`IFS=' ,'`와 `set -f`로 나누고, `IFS`와 그 전의 `-f` 상태를 되돌림).
    - `ask_discord_settings`(세 설명과 질문, 위험한 조합 확인).
    - `finish_discord_change`(출력을 보이고, 성공이고 `unchanged` 줄이 없으면 `discord_changed`).
    - `connect_discord`·`change_discord_settings`·`turn_discord_on`.
    - `--status` 출력으로 연결됨(메뉴)·꺼짐(켜기)·토큰 없음(연결)을 정한다. restart는 로그인했거나 `discord_changed`일 때다. 할 일에 `Turn Discord on:` 줄을 더했다.
  - 문서(범위 7):
    - USAGE "Discord 연결" 절(설정 옵션, 토큰 없는 바꾸기, 멈춤, `--status` 설정 줄, Docker에서 설정만 바꾼 뒤 restart), channel 설정 줄, "설정 스크립트로 처음 실행" 6·7번.
    - README 채널 절과 Docker 문단, SPEC 052 개정과 `docs/specs/README.md` 행.
- 새 시험:
  - `discord_connect_cli_tests.rs`(단위, 13개 중 9개가 새것, 수용 조건 1–6):
    - 해석 표 `setting_options_parse_into_one_action`
    - 적용 표와 runtime 읽기 `settings_apply_only_their_fields_and_the_runtime_reads_them`: 행마다 `Settings`와 `Connect`(토큰 이름을 줌, `--disable` 행은 `Settings`만) 둘 다. 바뀐 칸 목록, 그 칸만 되돌리면 처음 객체와 같음, runtime이 뒤에 읽는 별칭 칸(`allowedSenders`·`allow_from`·`allowed_senders`·`channel_ids`·`allowed_channel_ids`·`allow_channels`·`group_policy`)과 `streaming`·`replyInThread`가 그대로, 결과의 runtime 값과 worker 조건.
    - 빈 config에서 연결 `connecting_without_a_config_adds_the_token_name_and_turns_discord_on`(바뀐 칸 목록과 결과 객체 전체)
    - 먼저 읽히는 칸 표 `a_channel_field_the_runtime_reads_first_stops_both_actions`(멈추지 않는 별칭 칸은 두 갈래 모두 값이 그대로)
    - REST polling 표 `a_group_policy_stops_where_the_runtime_polls_channels`(바꾸는 경우 별칭 칸 그대로)
    - 토큰 없는 바꾸기의 관찰자 `a_config_changed_before_the_settings_write_is_left_alone`
    - 토큰 판정 표 `settings_change_only_where_a_token_is_saved`: `.env.json`에 없고 process 환경에만 있는 토큰은 바꾸고, 어디서도 풀리지 않으면 멈춤. 바꿀 때도 `.env.json`이 byte 단위로 같음.
    - 경고 표 `the_warning_follows_the_sender_list_the_runtime_reads`
    - `--status` 줄 표 `status_lines_follow_what_the_runtime_reads`: 토큰이 `.env.json`에 없고 환경에만 있는 경우(`token`, 다른 이름의 `botToken`, 꺼짐)도 줄 전체를 비교.
  - `tests/it/spec052_discord_connect.rs`(binary, 13개 중 4개가 새것):
    - 수용 조건 7: `connecting_with_settings_writes_them_and_status_shows_them`
    - 수용 조건 8: `settings_change_without_the_token_and_turn_discord_off_and_on`, `settings_without_a_saved_token_or_during_a_migration_change_nothing`(빈 폴더, 빈 `token`, 031 marker, 048 marker. marker는 그 경로를 알리는지도 봄)
    - 수용 조건 9: `conflicting_setting_options_change_nothing`
  - `tests/it/spec052_setup_script.rs`(스크립트, 13개 중 4개가 새것):
    - `the_discord_questions_explain_each_setting_and_pass_its_options`: 수용 조건 10과 14의 연결·채널·답하는 때·확인 질문 EOF
    - `a_connected_bot_keeps_changes_or_turns_off_its_settings`: 수용 조건 11·13과 메뉴·바꾸는 중 EOF
    - `a_turned_off_bot_is_turned_on_when_asked`: 수용 조건 12와 켜기·같이 바꾸기 질문 EOF
    - `discord_states_without_a_terminal_ask_nothing`: 수용 조건 15
- 고친 기존 시험과 까닭:
  - `spec052_setup_script.rs`:
    - 질문 표 `the_questions_sign_in_connect_and_restart_as_answered`: 사용자 ID 뒤에 채널·답하는 때 질문이 더해졌다. 그래서 답에 `CHANNELS`·`REPLY`를 넣고, 기대 명령에 `--all-channels --group-policy mention`을 더했다(빈 사람을 확인한 경우는 `--allow-anyone`도).
    - `a_ready_provider_and_a_connected_bot_ask_nothing` → `a_ready_provider_asks_nothing_and_a_connected_bot_offers_its_settings`: 연결된 Discord가 이제 메뉴를 묻기 때문이다. gateway가 돌고 있을 때 provider 질문은 없고, 설정 줄과 메뉴를 보이며, 빈 답이면 Discord 명령도 restart도 없음을 본다.
    - 가짜 `docker`: Discord 명령의 출력(`FAKE_DISCORD_OUTPUT`, 기본은 바뀐 칸 줄)을 내고 토큰 없는 Discord 명령도 받는다. restart가 이제 출력의 `unchanged`를 보기 때문이다.
    - `CONNECTED` 상수: 실제 `--status`처럼 설정 세 줄을 갖는다.
  - `discord_connect_cli_tests.rs` `arguments_and_token_fields_follow_the_plan`: `Connect`의 꼴이 `allow_from`에서 `settings`로 바뀌어 기대 값을 고쳤다. `Given`/`When`/`Then`으로 나눴고, 도움 함수 `connect_with_change`도 새 꼴로 바꿨다.
  - `data_folder_access_tests.rs`·`spec052_data_folder_access.rs`: 확인 명령 목록에 `channels connect discord --allow-anyone`을 더했다(binary `CHECKED` 29 → 30).
  - `spec052_discord_connect.rs`의 기존 시험은 고치지 않았다(module 설명만). 경고 확인은 `warning:`만 보고, `--status` 확인은 출력의 앞부분(그대로인 첫 두 줄)만 비교한다.
- 값이 없는 출력 예(debug binary, `env -i`와 임시 HOME, 시험 토큰을 stdin으로 넣은 연결 뒤):

  ```text
  $ shacs-bot channels connect discord --status
  discord: connected
  token: channels.discord.token
  users: 111
  channels: 222
  server channels: when mentioned
  $ shacs-bot channels connect discord --allow-anyone --all-channels --group-policy open
  discord: settings updated
  updated: config channels.discord.allowFrom, channels.discord.allowChannels, channels.discord.groupPolicy
  warning: the Discord sender list is empty or has *; anyone the bot can see can use it
  $ shacs-bot channels connect discord --disable
  discord: settings updated
  updated: config channels.discord.enabled
  warning: the Discord sender list is empty or has *; anyone the bot can see can use it
  $ shacs-bot channels connect discord --status
  discord: not connected
  reason: channels.discord.enabled is not true
  token: channels.discord.token
  users: anyone
  channels: all
  server channels: every message
  ```

  같은 설정 옵션으로 다시 실행하면 둘째 줄이 `unchanged`다. 끝난 config의 `channels.discord`에는 `allowChannels: []`, `allowFrom: []`, `enabled`, `groupPolicy`, `token: "${SHACS_CHANNELS_DISCORD_TOKEN}"`만 있다. 시험 토큰은 `.env.json`에만 있었다.
- 검증:
  - Google 옵션을 붙인 `cargo fmt --all -- --check`(처음에는 새 시험 몇 곳의 줄 바꿈이 달라 fmt로 고침)와 workspace `cargo clippy --all-targets -- -D warnings`가 통과했다.
  - 마지막 소스로 `shacs-cli` 575 passed / 0 failed / 3 ignored(`prd050/cli-tests.log`, PRD 040과 같은 셈법).
  - 적용 표가 별칭 칸을 지키는지: `discord_config`에 `allow_from`을 지우는 줄을 잠시 넣으면 그 시험이 실패하는 것을 보고 되돌렸다(소스는 그 전과 byte 단위로 같음).
- Oracle 독립 검토:
  - 계획: 4차 **PASS**. 1차는 REST polling의 `*`, `Settings`의 PRD 030 확인, DM 글, 멈춤 표 두 갈래, `--status` 표, README, 경고 글. 2차는 토큰 판정 별칭, `*`·별칭 경고, `defaultChannelId`의 REST polling, 같은 `--group-policy` 두 번. 3차는 "토큰을 읽지 않는다"의 뜻.
  - 구현: 3차 **PASS**.
    - 1차 지적: `Settings`의 048 marker 시험, 연결·켜기 질문의 EOF 시험, 적용 표의 별칭 칸·`Connect` 갈래, `--status` 표의 환경 토큰, `id_options`의 `-f` 되돌리기, USAGE의 snake_case 채널 칸, BDD 주석.
    - 2차 지적: USAGE의 멈춤 조건을 연결할 때와 설정만 바꿀 때로 나눔.
  - 스크립트 `sh -n`·`dash -n`·`bash --posix -n`(macOS bash 3.2.57)이 통과했다.
  - `crates/Cargo.lock`과 `Cargo.toml`은 바뀌지 않았다.
