# shacs-bot

`shacs-bot`은 `nanobot`의 Rust porting이며, 개인 사용/자체 호스팅 운영을 기본으로 하는 Rust 에이전트 런타임입니다. 로컬 CLI, OpenAI 호환 HTTP API, 세션/런타임 유틸리티, provider adapter, tool, skill, template, 그리고 선택된 channel worker를 제공합니다.

- 원본 nanobot 저장소: <https://github.com/HKUDS/nanobot>

## 빠른 시작

현재 저장소의 Rust workspace는 `crates/Cargo.toml`입니다. 저장소 루트에서 workspace manifest와 package를 명시해 명령을 실행하세요.

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- --help
```

설정 파일과 workspace template을 생성하거나 갱신합니다. `onboard`는 built-in channel별 기본 config stub도 생성하며, 기존 channel 값과 secret/env placeholder는 보존하고 누락된 기본 key만 병합합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- onboard --workspace /tmp/shacs-ws
```

설정과 runtime 상태를 확인합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- status
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime inspect --workspace /tmp/shacs-ws
```

Plugin과 hook manifest 상태는 descriptor-only management surface로 확인합니다. `plugin.json`과 `plugin.toml` manifest를 discovery/config gate까지 지원하지만, 이는 plugin foundation이며 full live plugin execution은 아직 아닙니다. 이 명령들은 plugin command, hook callback, MCP server, process를 실행하지 않으며, `enable`/`disable`은 config만 수정하고 다음 session/reload에 적용된다고 보고합니다. Enabled plugin의 typed hook entrypoint는 agent runtime에서 diagnostics-only로 dispatch될 수 있지만, hook output은 command/tool/provider behavior를 바꾸지 않습니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- plugins list --workspace /tmp/shacs-ws
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- plugins doctor --workspace /tmp/shacs-ws
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- hooks list --workspace /tmp/shacs-ws
```

공식 로컬 runtime lifecycle 진입점은 `runtime start/stop/restart`입니다. `runtime start`는 channel runtime foreground 경로를 실행하며, `runtime stop`과 `runtime restart`는 실행 중인 로컬 owner가 관찰할 stop-request marker를 기록합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime start --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime stop --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime restart --workspace /tmp/shacs-ws
```

문제 상황을 확인해야 할 때는 로컬 runtime diagnostics를 bundle로 저장할 수 있습니다. diagnostics 출력과 bundle은 secret, token 같은 민감한 값을 가리며, runtime containment summary/digest를 함께 보고합니다. Native host에서 Docker/Compose 같은 인식 가능한 containment evidence가 없으면 containment는 unknown으로 보고됩니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime diagnostics --bundle /tmp/shacs-diagnostics.zip --workspace /tmp/shacs-ws
```

소스 checkout/Cargo 기반 update는 binary rebuild/replacement와 runtime marker 기록을 분리합니다. 새 binary를 준비한 뒤 local runtime upgrade evidence를 남기거나 중단 marker를 정리하려면 다음 명령을 사용합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- --version
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime update --target-version <current-shacs-bot-version> --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime recover --workspace /tmp/shacs-ws
```

로컬 agent turn을 한 번 실행합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- ask "hello" --workspace /tmp/shacs-ws
```

선택된 channel runtime worker를 시작합니다. 새 lifecycle workflow에서는 `runtime start`를 우선 사용하고, `run`은 같은 foreground channel runtime의 기존 호환 진입점입니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- run --workspace /tmp/shacs-ws
```

로컬 OpenAI 호환 HTTP API를 시작합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- serve --workspace /tmp/shacs-ws
```

실행 중인 로컬 API에서도 diagnostics를 확인할 수 있습니다. 응답은 민감한 값을 가린 형태입니다:

```sh
curl http://127.0.0.1:8900/v1/diagnostics
```

Docker Compose로 초기 설정과 장기 실행 서비스를 다룹니다. 이 경로가 현재 primary zero-setup containment path이며, 기본 Compose 설정은 Docker socket mount, privileged mode, host network를 사용하지 않습니다:

```sh
export SHACS_UID=$(id -u)
export SHACS_GID=$(id -g)
mkdir -p ~/.shacs-bot
docker compose run --rm shacs-cli onboard
vim ~/.shacs-bot/config.json
docker compose up -d shacs-gateway
```

공식 Compose containment smoke gate는 실제 Docker/Compose runtime에서 `runtime inspect`의 official-container evidence와 기본 Compose 안전 속성을 확인합니다. 이 검증은 opt-in이며 임시 host data directory를 사용하므로 사용자의 실제 `~/.shacs-bot`을 건드리지 않습니다:

```sh
./docs/scripts/spec023-compose-smoke.sh
```

Compose는 host의 `~/.shacs-bot`을 container의 `/home/shacs/.shacs-bot`에 mount합니다. Provider secret은 image에 넣지 말고 `onboard` 후 생성된 config/auth workflow 또는 `.env.example`을 참고한 shell environment로 제공하세요. 기본 UID/GID는 nanobot과 같은 `1000:1000`이고, 위 예시처럼 `SHACS_UID`/`SHACS_GID`를 지정하면 host user 소유권에 맞춰 실행합니다. `bwrap`는 공식 image/package에 포함되어 자동 설정된 경우가 아니라면 optional hardening입니다. 로컬 OpenAI 호환 API만 띄우려면 provider 설정 후 `docker compose up -d shacs-api`와 `curl http://127.0.0.1:8900/health`를 사용하세요.

스킬의 `requires.env` 확인이나 `exec`/subagent 실행에 필요한 환경 변수는 `config.json`의 top-level `env`에 둘 수 있습니다. `tools.exec.env`도 계속 지원하며 같은 key가 있으면 더 구체적인 `tools.exec.env` 값이 우선합니다. MCP 서버별 환경 변수는 기존처럼 `tools.mcpServers.<name>.env`에 별도로 둡니다. MCP `tools.mcpServers.<name>.enabledTools`는 기본값이 빈 배열인 default-deny opt-in입니다. MCP tools/resources/prompts를 노출하려면 `*`, raw capability name, 또는 `mcp_<server>_<kind>_<name>` 형태의 wrapped capability name을 명시하세요. 빈 문자열은 `requires.env`를 만족하지 않습니다. Secret 값을 넣은 config 파일은 커밋하거나 공유하지 마세요.

CLI binary를 빌드해서 실행합니다:

```sh
cargo build --manifest-path crates/Cargo.toml -p shacs-cli
./crates/target/debug/shacs-bot --help
```

## 채널

`shacs-bot run`은 WebSocket channel이 enabled 상태이면 WebSocket server를 시작하고, 외부 channel config에 필요한 인증 정보와 현재 구현된 transport 설정이 있으면 해당 transport도 함께 시작합니다. 외부 인증 정보가 없으면 전체 runtime을 실패시키지 않고 `skipped-missing-credentials`로 보고합니다. Slack은 inbound/event 수신에 Socket Mode를 사용하므로 `appToken`/`app_token`과 `botToken`/`bot_token`/`token`이 모두 필요합니다.

현재 실행 가능한 상태로 연결된 channel transport는 다음과 같습니다:

- WebSocket server
- Telegram long polling
- Discord Gateway
- Slack Socket Mode inbound + Web API outbound
- Email SMTP/IMAP
- WhatsApp bridge WebSocket

Channel 설정과 worker 상태를 확인합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- channels list
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- channels status --workspace /tmp/shacs-ws
```

Channel config는 원본 nanobot과 같은 `channels.<name>` 형태를 우선합니다. 예를 들어 `channels.sendMemoryHints`는 status에서 memory hint 설정으로 표시되고, `channels.sendMaxRetries`는 `ChannelManager` dispatch/enqueue와 실제 transport send의 총 시도 횟수로 적용됩니다. 값은 최소 1회, 최대 10회로 제한됩니다. WebSocket과 외부 transport outbound는 `ChannelManager` dispatch 정책을 통과하며, 외부 transport inbound/outbound는 runtime `MessageBus` 경계를 사용합니다. 외부 runtime은 Nanobot처럼 같은 session key의 turn을 직렬화하고 진행 중 들어온 같은 session 메시지를 in-memory pending follow-up queue에 보관해 현재 turn 뒤에 이어서 처리합니다. 이 pending follow-up queue는 process 재시작 후 복구되는 durable queue가 아닙니다. Built-in slash command는 command router에서 priority/exact/prefix 경계로 분류되며, `/status`, `/stop`, `/restart` priority command만 active session turn 중에도 먼저 처리됩니다. 일반 turn과 exact/prefix command는 같은 process-local session turn lock을 공유합니다. `channels.sendProgress`가 enabled이면 WebSocket channel은 provider text deltas를 coalesce한 `delta` event와 `stream_end` event를 보낸 뒤 최종 `message` event를 유지합니다. Telegram/Discord/Slack external transport는 provider progress delta를 보내지 않고 최종 assistant answer만 새 message로 전송하며 기존 message를 edit/update하지 않습니다. WebSocket event delivery는 bounded queue로 socket writer에 넘겨져 느린 client에 대해 backpressure를 적용합니다. Telegram topic, Slack thread, Discord thread, Email subject/reply context는 outbound reply metadata로 이어집니다. Telegram offset, Discord REST last message id, Discord Gateway resume state, Email IMAP seen UID + UIDVALIDITY hint, outbound delivery pending/sent/failed/processed status는 runtime metadata JSON으로 best-effort 보존됩니다. 이 metadata는 durable queue나 exactly-once delivery 보장이 아닙니다. Email은 `consentGranted: true`와 inbound `allowFrom`/`allowedSenders`가 있어야 IMAP polling을 시작하며, `smtpUsername`/`smtpPassword`, `imapUsername`/`imapPassword`, `fromAddress` alias를 함께 받습니다. IMAP polling은 현재 TLS만 지원하고, inbound Email은 기본적으로 `Authentication-Results`의 `spf=pass`/`dkim=pass`를 확인합니다.

## 검증

Cargo 명령은 `crates/Cargo.toml` workspace manifest를 명시해서 실행합니다:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --workspace
cargo build --manifest-path crates/Cargo.toml -p shacs-cli
```

Channel crate를 수정한 경우:

```sh
cargo fmt --manifest-path crates/Cargo.toml -p shacs-channels -- --check
cargo clippy --manifest-path crates/Cargo.toml -p shacs-channels --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml -p shacs-channels
cargo build --manifest-path crates/Cargo.toml -p shacs-channels
```

## 추가 문서

- 사용자 가이드: [`docs/USAGE.md`](docs/USAGE.md)
- 스펙 인덱스: [`docs/specs/README.md`](docs/specs/README.md)
