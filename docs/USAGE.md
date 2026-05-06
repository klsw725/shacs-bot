# shacs-bot 사용 가이드

이 문서는 사용자가 자신의 workspace에서 `shacs-bot`을 로컬로 실행하는 경우를 기준으로 합니다. SaaS control plane, fleet operator, 별도 관리자 조직을 전제로 하지 않습니다.

설계 계약과 invariant는 [docs/specs/README.md](specs/README.md)를 참고하세요. 이 문서는 현재 Rust CLI에서 실제 구현된 사용자-facing 표면을 설명합니다.

## 소스에서 빌드

저장소 루트에서 실행합니다:

```sh
cargo build --manifest-path crates/shacs-cli/Cargo.toml --locked
```

아래 예시는 `shacs-bot` binary가 `PATH`에 있다고 가정합니다. 소스 checkout에서 바로 실행할 때는 필요하면 명령 앞에 `cargo run --manifest-path crates/shacs-cli/Cargo.toml --`를 붙이세요:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- status
```

## 설정

현재 Rust CLI는 하나의 JSON config 파일을 사용합니다. 기본 경로는 다음과 같습니다:

```text
$HOME/.shacs-bot/config.json
```

특정 config 파일을 사용하려면 `--config <path>` 또는 `-c <path>`를 지정합니다. Runtime 명령은 config를 저장하지 않는 일회성 workspace override로 `--workspace <path>` 또는 `-w <path>`도 받습니다.

Config와 workspace template을 생성하거나 갱신합니다:

```sh
shacs-bot onboard --workspace /tmp/ws
shacs-bot --config /tmp/shacs-config.json onboard --workspace /tmp/ws
```

`onboard`는 JSON config를 쓰고, runtime directory를 준비하며, `AGENTS.md`, `SOUL.md`, `USER.md`, `TOOLS.md`, `memory/MEMORY.md`, `memory/history.jsonl`, `skills/` 같은 workspace template 파일을 만듭니다. 이미 존재하는 workspace 파일은 덮어쓰지 않습니다. `onboard --wizard`는 아직 보류된 기능입니다.

현재 config와 provider field를 확인합니다:

```sh
shacs-bot status
shacs-bot status --config /tmp/shacs-config.json
```

`status`는 plain text를 출력합니다. JSON envelope를 출력하지 않고, config migration을 disk에 다시 쓰지도 않습니다.

Secret이나 session message 본문을 읽지 않고 로컬 runtime/workspace 상태를 확인합니다:

```sh
shacs-bot runtime inspect
shacs-bot runtime inspect --workspace /tmp/ws
```

`runtime inspect`는 선택된 config, workspace, data directory, provider/model, provider 설정 여부, runtime capability 요약, session 개수와 최신 session metadata를 보고합니다. `auth.json` token 값이나 raw session message는 노출하지 않으며, 장기 실행 cron/heartbeat worker를 시작하거나 실행 중인 것처럼 표시하지 않습니다.

해결된 workspace의 로컬 session 파일 목록을 봅니다:

```sh
shacs-bot session list
shacs-bot session list --workspace /tmp/ws
```

Raw message 본문을 출력하지 않고 session 하나를 확인합니다:

```sh
shacs-bot session inspect --session cli:direct
shacs-bot session inspect --session cli:direct --workspace /tmp/ws
```

`session list`는 key, timestamp, file path를 보여줍니다. `session inspect`는 key, path, timestamp, message count, `last_consolidated`, metadata key 이름, `pending_user_turn` 또는 `runtime_checkpoint` 같은 recovery marker 이름만 보여줍니다. 저장된 prompt/assistant content나 raw metadata value는 출력하지 않습니다.

빈 session 파일을 만들거나, 필터링된 conversation history를 출력하거나, 로컬 diagnostics를 확인합니다:

```sh
shacs-bot session create --session cli:work
shacs-bot session history --session cli:work --max-messages 10
shacs-bot session history --session cli:work --json
shacs-bot session diagnostics --session cli:work
```

`session history`는 runtime과 같은 filtered replay view를 사용합니다. Consolidated message는 건너뛰고, orphan tool result는 복구하며, 기본 text 출력에서는 긴 user/assistant message를 잘라 보여줍니다. Raw session 파일이 아니라 필터링된 구조화 history가 필요할 때 `--json`을 사용하세요.

Raw 로컬 session content는 명시적으로 필요할 때만 export하세요:

```sh
shacs-bot session export --session cli:work --format json --yes
shacs-bot session export --session cli:work --format jsonl --yes
```

`session export`는 raw prompt, assistant message, metadata value, tool payload를 포함할 수 있습니다. 따라서 `--yes`/`-y` 확인이 필요하며 민감한 로컬 데이터로 취급해야 합니다.

로컬 session 파일 하나를 비우거나 compact합니다:

```sh
shacs-bot session clear --session cli:work --yes
shacs-bot session compact --session cli:work --keep-messages 8 --yes
```

`session clear`는 session metadata를 유지하면서 모든 message를 제거하고 `last_consolidated`를 reset합니다. `session compact`는 최근 legal suffix만 남기도록 JSONL 파일을 다시 씁니다. Provider 기반 요약이 아니라 로컬 destructive trim입니다.

로컬 session 파일 하나를 삭제합니다:

```sh
shacs-bot session delete --session cli:direct --yes
```

Session 삭제는 workspace의 `sessions/*.jsonl` 파일 하나를 disk에서 제거하며 되돌릴 수 없습니다. CLI는 명시적 확인으로 `--yes`/`-y`를 요구합니다. Session이 없으면 `Deleted: no`로 보고하고, 없는 `sessions/` directory를 만들지 않습니다.

## 스킬

해결된 workspace 기준으로 활성 로컬 skill registry entry를 나열합니다:

```sh
shacs-bot skills list
shacs-bot skills list --workspace /tmp/ws
```

Skill prompt 본문 전체를 로드하지 않고 하나의 skill을 확인합니다:

```sh
shacs-bot skills show skill-creator
shacs-bot skills show clawhub --workspace /tmp/ws
```

`skills list`는 `onboard`가 `builtin_skills/`를 materialize하기 전에도 embedded built-in skill을 포함합니다. Workspace skill은 built-in skill을 shadow할 수 있습니다. Shadowed, conflicted, malformed 같은 비활성 diagnostic까지 보려면 `skills list --all`을 사용하세요. `skills show`는 source, status, body hash, requirements, install metadata, diagnostics를 출력합니다. ClawHub search/install/update 명령은 이후 slice로 남아 있습니다.

## 일회성 CLI 에이전트

로컬 `AgentLoop`에 메시지 하나를 보냅니다:

```sh
shacs-bot ask "hello" --workspace /tmp/ws
```

기존 nanobot 호환 direct 형식도 지원합니다:

```sh
shacs-bot agent -m "hello" --workspace /tmp/ws
shacs-bot agent --message "hello" --session work --workspace /tmp/ws
```

`ask`와 `agent -m/--message`는 같은 direct execution path를 사용합니다. Config를 로드하고, 설정된 provider/model을 resolve하고, `AgentLoop`를 만든 뒤 user turn 하나를 실행하고 assistant text를 stdout에 출력합니다.

Provider 호출 전에 built-in slash command는 로컬에서 처리됩니다:

- `/status`: 현재 loop/session에 active task가 있는지 보고합니다.
- `/new`: 현재 session을 비우고 새로 시작합니다.
- `/stop`: 등록된 active task에 cancellation을 요청합니다.
- `/restart`: 로컬 restart 요청을 acknowledge합니다. Rust CLI는 현재 process를 in-place로 교체하지 않습니다.
- `/history [n]`: 최근 visible user/assistant message를 보여줍니다. 기본값은 10, 최대값은 50입니다.
- `/dream`: 설정된 Dream memory consolidation을 한 번 실행합니다.
- `/dream-log [sha]`: 최신 memory commit diff 또는 선택한 commit diff를 보여줍니다.
- `/dream-restore [sha]`: 복원 가능한 memory version 목록을 보여주거나 선택한 commit 이전 상태로 tracked memory file을 되돌립니다.
- `/help`: slash command 목록을 보여줍니다.

정확히 일치하는 command만 command로 처리됩니다. 예를 들어 `/status now`는 `/status` command가 아니라 일반 user message로 처리됩니다.

`ask` message가 `-`로 시작하면 option과 구분하기 위해 `--`를 사용하세요. 예: `shacs-bot ask -- "-starts-with-dash"`.

지원되는 direct-message option:

- `--config <path>` / `-c <path>`
- `--workspace <path>` / `-w <path>`
- `--session <id>` / `-s <id>`: 기본값은 `cli:direct`입니다. `:`가 없는 값은 `cli:<id>`로 저장됩니다.
- `--temperature <number>`
- `--max-tokens <positive integer>`
- `--allow-side-effects`: 이 로컬 CLI turn에서 write/edit/exec tool을 명시적으로 허용합니다.
- `--markdown` / `--no-markdown`: nanobot CLI 호환을 위해 받지만, 현재 Rust binary는 plain stdout text를 출력합니다.

Message 없이 `shacs-bot agent`만 실행해도 아직 interactive REPL은 시작하지 않습니다. Interactive loop는 이후 runtime/channel slice로 남아 있습니다.

## Codex provider 인증

Codex request/stream 지원은 provider id `openai_codex` 아래에 구현되어 있습니다. 인증은 `config.json` 옆의 OpenCode-style `auth.json` 파일을 사용합니다.

브라우저 OAuth login을 시작합니다:

```sh
shacs-bot provider codex login
```

브라우저를 자동으로 열 수 없는 terminal에서는 URL을 출력하고 localhost callback을 수동으로 완료합니다:

```sh
shacs-bot provider codex login --no-browser
```

Headless 환경에서는 device flow를 사용합니다:

```sh
shacs-bot provider codex login --headless
```

Login에 성공하면 `auth.json`에 `access`, `refresh`, `expires`, optional `accountId`를 저장하고, provider를 `openai_codex`, model을 `gpt-5.4`로 선택합니다. Runtime startup은 refresh token이 있으면 만료된 Codex access token을 갱신하고, 갱신된 session을 다시 `auth.json`에 씁니다.

`gpt-5.4`는 ChatGPT account Codex의 보수적인 기본값입니다. `gpt-5.5` 같은 더 새로운 Codex model slug는 계정 rollout 또는 entitlement가 필요할 수 있습니다. `openai/gpt-5.5` 같은 provider-qualified id는 ChatGPT Codex backend로 보내기 전에 정규화됩니다.

stdin에서 token을 import합니다:

```sh
printf '%s' "$CODEX_TOKEN" | shacs-bot provider codex import-token --token-stdin
```

환경 변수에서 token을 import합니다:

```sh
shacs-bot provider codex import-token --token-env CODEX_TOKEN --account-id acct_123
```

`import-token`은 fallback으로 유지됩니다. Provider 선택/config metadata는 `config.json`에 쓰지만 bearer token은 config 옆 `auth.json`에만 저장합니다. Auth file은 provider-keyed OAuth entry를 사용하며 `type`, `access`, optional `refresh`, optional `expires`, optional `accountId` 같은 field를 가집니다. Unix에서는 secret-file permission으로 작성됩니다. Command output은 path와 status만 출력하며 token은 출력하지 않습니다.

기본적으로 import는 configured model을 `gpt-5.4`로 선택합니다. Auth만 저장하고 provider/model 선택을 바꾸지 않으려면 `--no-select`를 사용하세요.

## 로컬 OpenAI 호환 API

로컬 API server를 시작합니다:

```sh
shacs-bot serve --bind 127.0.0.1:8900 --workspace /tmp/ws --timeout 120
```

이전 API 중심 command shape와의 호환을 위해 `api serve`는 같은 command의 alias입니다:

```sh
shacs-bot api serve --bind 127.0.0.1:8900 --workspace /tmp/ws --timeout 120
```

기본 bind address는 JSON config의 API section에서 오며 기본값은 `127.0.0.1:8900`입니다. 한 번의 실행에서만 바꾸려면 `--bind <host:port>` 또는 `--host <ip> --port <port>`를 사용하세요.

로컬 API는 인증이 없으므로 non-loopback bind에는 명시적 opt-in이 필요합니다:

```sh
shacs-bot serve --bind 0.0.0.0:8900 --allow-remote --workspace /tmp/ws
```

API turn은 기본적으로 read/search/web tool을 사용합니다. Write/edit/exec/self-modifying tool은 다음 option이 필요합니다:

```sh
shacs-bot serve --allow-api-side-effects --workspace /tmp/ws
```

구현된 endpoint:

- `GET /health`
- `GET /v1/models`
- `POST /v1/chat/completions`

`POST /v1/chat/completions`는 단일 user message, optional `session_id`, optional `temperature`, optional `max_tokens`, JSON text 또는 data-URL image content part, multipart upload, non-stream response, `stream=true` Server-Sent Events를 받습니다. Remote image URL은 거부합니다. Data URL과 uploaded file은 runtime media directory 아래에 저장되며, 파일당 10 MiB 제한이 있습니다.

같은 session key의 API request는 직렬화됩니다. `--timeout`은 HTTP wait timeout을 제어합니다. Timeout response가 반환되어도 in-flight turn은 blocking `AgentLoop` 작업이 끝날 때까지 해당 session lock을 계속 소유합니다.

## 채널

로컬 channel registry와 configured channel plugin 설정을 확인합니다:

```sh
shacs-bot channels list
shacs-bot channels status --workspace /tmp/ws
```

`channels list`는 built-in channel descriptor, config-enabled 상태, capability, worker boundary 개수를 보여줍니다. `channels status`는 configured channel plugin과 progress/tool-hint delivery, send retry count 같은 runtime default를 요약합니다. 이 명령들은 read-only diagnostics입니다. Runnable channel worker를 시작하려면 `run`을 사용하세요.

선택된 channel runtime을 시작합니다:

```sh
shacs-bot run --workspace /tmp/ws
shacs-bot run --websocket-host 127.0.0.1 --websocket-port 8765 --workspace /tmp/ws
```

`run`은 `websocket` channel이 enabled이면 WebSocket channel server를 시작합니다. 들어오는 JSON text 또는 binary WebSocket frame은 channel contract를 통해 normalize되고, 로컬 `AgentLoop`에서 처리된 뒤 WebSocket server event로 반환됩니다. WebSocket config는 `channels.plugins.websocket`의 `enabled`, `host`, `port`, `path`에서 읽습니다. Command-line `--websocket-host`와 `--websocket-port`는 한 번의 실행에서 host/port를 override합니다. Non-loopback WebSocket bind에는 `--allow-remote`가 필요합니다.

`run`은 plugin config에 충분한 인증 정보가 있으면 선택된 외부 channel transport도 시작합니다. 인증 정보가 없으면 전체 runtime을 실패시키지 않고 `skipped-missing-credentials`로 보고하므로, WebSocket부터 켜고 외부 channel을 점진적으로 추가할 수 있습니다.

최소 외부 channel config key:

- `channels.plugins.telegram`: `enabled`, `botToken`/`bot_token`/`token`, optional `pollTimeoutSeconds`, `pollLimit`.
- `channels.plugins.discord`: `enabled`, `botToken`/`bot_token`/`token`, `channelIds` 또는 `defaultChannelId`, optional `pollIntervalSeconds`.
- `channels.plugins.slack`: `enabled`, `botToken`/`bot_token`/`token`, `channelIds` 또는 `defaultChannelId`, optional `pollIntervalSeconds`.
- `channels.plugins.email.smtp`: `host`, `port`, `from`, optional `username`, `password`, `security`, `timeoutSeconds`; `channels.plugins.email.imap`: `host`, `port`, `username`, `password`, optional `mailbox`, `markSeen`(기본 true), `pollIntervalSeconds`, `timeoutSeconds`, `security`.
- `channels.plugins.whatsapp`: `enabled`, `bridgeUrl`, optional `bridgeToken`, `pollPath`, `sendPath`, `pollIntervalSeconds`, `groupPolicy`, `allowlist.allowedSenders`.

외부 transport는 의도적으로 최소 adapter입니다. Telegram은 long polling, Discord/Slack은 configured channel을 REST로 polling, Email은 SMTP/IMAP, WhatsApp은 로컬 bridge HTTP endpoint와 통신합니다. 모두 로컬 `AgentLoop`와 같은 personal-use `shacs-bot run` process 안에서 실행됩니다. Email IMAP은 한 번의 실행 중 같은 unseen message 반복 처리를 피하기 위해 in-process UID cache를 유지합니다. Server가 message를 unseen 상태로 남겨두면 restart 후 replay 가능성은 여전히 있습니다.

## Docker Compose

저장소에는 개인 사용 서비스로 로컬 HTTP API와 channel runtime을 실행하기 위한 multi-stage Dockerfile과 `docker-compose.yaml`이 포함되어 있습니다. Docker 동작은 upstream nanobot deployment와 같이 먼저 host config directory를 초기화하고, host의 `~/.shacs-bot`을 non-root container user의 `/home/shacs/.shacs-bot`에 mount하는 방식을 따릅니다:

```sh
export SHACS_UID=$(id -u)
export SHACS_GID=$(id -g)
mkdir -p ~/.shacs-bot
docker compose run --rm shacs-cli onboard   # first-time setup
vim ~/.shacs-bot/config.json                # add API keys or provider config
docker compose up -d shacs-gateway          # start channel runtime
```

`shacs-gateway`는 container 안에서 `shacs-bot run --websocket-host 0.0.0.0 --allow-remote`를 실행하고 host loopback의 WebSocket port `8765`에만 publish합니다. Provider 설정이 없으면 runtime은 `provider not found: auto`로 시작하지 않으므로, 먼저 `config.json` 또는 `auth.json` workflow로 provider를 설정하세요.

로컬 OpenAI 호환 API를 시작하려면 같은 config/workspace를 사용해 별도 API service를 띄웁니다:

```sh
docker compose up -d shacs-api
curl http://127.0.0.1:8900/health
```

CLI command를 container에서 한 번 실행하려면 다음처럼 `shacs-cli` service를 사용합니다:

```sh
docker compose run --rm shacs-cli ask "Hello!"
docker compose run --rm shacs-cli status
```

Channel runtime logs를 확인하거나 서비스를 내리려면 다음 명령을 사용합니다:

```sh
docker compose up -d shacs-gateway
docker compose logs -f shacs-gateway
docker compose down
```

Provider secret은 로컬 config/environment workflow로 제공하세요. Image 안에 secret을 bake하지 마세요. 기본 container UID/GID는 nanobot과 같은 `1000:1000`이고, 위 예시처럼 `SHACS_UID`/`SHACS_GID`를 지정하면 host user 소유권에 맞춰 실행합니다. Permission denied가 계속 나면 host에서 `sudo chown -R 1000:1000 ~/.shacs-bot`로 ownership을 맞추거나 Podman의 `--userns=keep-id` 같은 실행 user 전략을 사용하세요.

## 예약된 명령

다음 command name은 예약되어 있지만 현재 Rust CLI에는 아직 구현되어 있지 않습니다:

- `plugins`

TUI command, 구현된 Codex login 외의 provider OAuth flow, ClawHub install/update wrapper, gateway supervision은 이후 migration slice로 남아 있습니다. 위에서 구현된 것으로 명시하지 않은 command는 사용할 수 있는 기능으로 취급하지 마세요.
