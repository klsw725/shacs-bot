# shacs-bot usage guide

This guide is for a person running `shacs-bot` locally for their own workspace. It does not assume a SaaS control plane, a fleet operator, or a separate administrator role.

For design contracts and invariants, see [docs/specs/README.md](specs/README.md). This page describes the Rust CLI surface implemented now.

## Build from source

From the repository root:

```sh
cargo build --manifest-path crates/shacs-cli/Cargo.toml --locked
```

The examples below use `shacs-bot` as if the binary is on `PATH`. From a source checkout, prefix commands with `cargo run --manifest-path crates/shacs-cli/Cargo.toml --` when needed:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- status
```

## Configuration

The current Rust CLI uses one JSON config file. By default it is loaded from:

```text
$HOME/.shacs-bot/config.json
```

Use `--config <path>` or `-c <path>` to load a specific config file. Runtime commands also accept `--workspace <path>` or `-w <path>` as a non-persistent workspace override.

Create or refresh the config and workspace templates:

```sh
shacs-bot onboard --workspace /tmp/ws
shacs-bot --config /tmp/shacs-config.json onboard --workspace /tmp/ws
```

`onboard` writes the JSON config, ensures runtime directories, and creates workspace template files such as `AGENTS.md`, `SOUL.md`, `USER.md`, `TOOLS.md`, `memory/MEMORY.md`, `memory/history.jsonl`, and `skills/` without overwriting existing workspace files. `onboard --wizard` is still deferred.

Inspect the current config and provider fields:

```sh
shacs-bot status
shacs-bot status --config /tmp/shacs-config.json
```

`status` prints plain text. It does not emit a JSON envelope and does not write config migrations back to disk.

Inspect local runtime/workspace state without reading secrets or session messages:

```sh
shacs-bot runtime inspect
shacs-bot runtime inspect --workspace /tmp/ws
```

`runtime inspect` reports the selected config, workspace, data directory, provider/model, configured provider flags, runtime capability summary, and session count/latest session metadata. It does not expose `auth.json` token values or raw session messages, and it does not start or display long-running cron/heartbeat workers.

List local session files for the resolved workspace:

```sh
shacs-bot session list
shacs-bot session list --workspace /tmp/ws
```

Inspect one session by key without printing raw message bodies:

```sh
shacs-bot session inspect --session cli:direct
shacs-bot session inspect --session cli:direct --workspace /tmp/ws
```

`session list` shows keys, timestamps, and file paths. `session inspect` shows key, path, timestamps, message count, `last_consolidated`, metadata key names, and recovery marker names such as `pending_user_turn` or `runtime_checkpoint`. It intentionally does not print stored prompt/assistant content or raw metadata values.

Create an empty session file, print filtered conversation history, or inspect local diagnostics:

```sh
shacs-bot session create --session cli:work
shacs-bot session history --session cli:work --max-messages 10
shacs-bot session history --session cli:work --json
shacs-bot session diagnostics --session cli:work
```

`session history` uses the same filtered replay view as the runtime: consolidated messages are skipped, orphan tool results are repaired, and the default text output truncates long visible user/assistant messages. Use `--json` when you need the filtered structured history, not the raw session file.

Export raw local session content only when explicitly needed:

```sh
shacs-bot session export --session cli:work --format json --yes
shacs-bot session export --session cli:work --format jsonl --yes
```

`session export` can include raw prompts, assistant messages, metadata values, and tool payloads. It therefore requires `--yes`/`-y` and should be treated as sensitive local data.

Clear or compact one local session file:

```sh
shacs-bot session clear --session cli:work --yes
shacs-bot session compact --session cli:work --keep-messages 8 --yes
```

`session clear` keeps the session metadata but removes all messages and resets `last_consolidated`. `session compact` rewrites the JSONL file to keep only a recent legal suffix; it is a destructive local trim, not provider-backed summarization.

Delete one local session file:

```sh
shacs-bot session delete --session cli:direct --yes
```

Session deletion removes the matching workspace `sessions/*.jsonl` file from disk and cannot be undone. The CLI requires `--yes`/`-y` as an explicit confirmation. If the session does not exist, the command reports `Deleted: no` and does not create a missing `sessions/` directory.

## Skills

List active local skill registry entries for the resolved workspace:

```sh
shacs-bot skills list
shacs-bot skills list --workspace /tmp/ws
```

Inspect one skill without loading the full prompt body:

```sh
shacs-bot skills show skill-creator
shacs-bot skills show clawhub --workspace /tmp/ws
```

`skills list` includes embedded built-in skills even before `onboard` materializes `builtin_skills/`. Workspace skills can shadow built-ins; use `skills list --all` to include inactive diagnostics such as shadowed, conflicted, or malformed entries. `skills show` prints source, status, body hash, requirements, install metadata, and diagnostics. ClawHub search/install/update commands remain a later skills slice.

## One-shot CLI agent

Send one message through the local `AgentLoop`:

```sh
shacs-bot ask "hello" --workspace /tmp/ws
```

The original nanobot-compatible direct form is also supported:

```sh
shacs-bot agent -m "hello" --workspace /tmp/ws
shacs-bot agent --message "hello" --session work --workspace /tmp/ws
```

`ask` and `agent -m/--message` use the same direct execution path. They load config, resolve the configured provider/model, create an `AgentLoop`, run one user turn, and print the assistant text to stdout.

Built-in slash commands are handled locally before a provider call:

- `/status`: report whether the current loop/session has an active task.
- `/new`: clear the current session and start fresh.
- `/stop`: request cancellation for any registered active task.
- `/restart`: acknowledge a local restart request; the Rust CLI does not replace the current process in-place.
- `/history [n]`: show recent visible user/assistant messages, default 10 and max 50.
- `/dream`: run the configured Dream memory consolidation once.
- `/dream-log [sha]`: show the latest memory commit diff or a selected commit diff.
- `/dream-restore [sha]`: list restorable memory versions or revert tracked memory files to the state before a selected commit.
- `/help`: show the slash-command list.

Exact commands remain exact: text such as `/status now` is treated as a normal user message rather than a `/status` command.

If an `ask` message begins with `-`, separate it from options with `--`, for example `shacs-bot ask -- "-starts-with-dash"`.

Supported direct-message options:

- `--config <path>` / `-c <path>`
- `--workspace <path>` / `-w <path>`
- `--session <id>` / `-s <id>`: defaults to `cli:direct`; values without `:` are stored as `cli:<id>`.
- `--temperature <number>`
- `--max-tokens <positive integer>`
- `--allow-side-effects`: opt in to write/edit/exec tools for this local CLI turn.
- `--markdown` / `--no-markdown`: accepted for nanobot CLI compatibility; the current Rust binary prints plain stdout text.

Running `shacs-bot agent` with no message does not start an interactive REPL yet. The interactive loop remains a later runtime/channel slice.

## Codex provider auth

Codex request/stream support is implemented under provider id `openai_codex`. Auth uses an OpenCode-style `auth.json` file next to `config.json`.

Start browser OAuth login:

```sh
shacs-bot provider codex login
```

For terminals where a browser cannot be opened automatically, print the URL and complete the localhost callback manually:

```sh
shacs-bot provider codex login --no-browser
```

For headless environments, use the device flow:

```sh
shacs-bot provider codex login --headless
```

Successful login stores `access`, `refresh`, `expires`, and optional `accountId` in `auth.json`, selects provider `openai_codex`, and selects model `gpt-5.4`. Runtime startup refreshes expired Codex access tokens when a refresh token is available and writes the refreshed session back to `auth.json`.

`gpt-5.4` is the conservative ChatGPT-account Codex default. Newer Codex model slugs such as `gpt-5.5` may require account rollout or entitlement; provider-qualified ids such as `openai/gpt-5.5` are normalized before sending requests to the ChatGPT Codex backend.

Import a token from stdin:

```sh
printf '%s' "$CODEX_TOKEN" | shacs-bot provider codex import-token --token-stdin
```

Or import from an environment variable:

```sh
shacs-bot provider codex import-token --token-env CODEX_TOKEN --account-id acct_123
```

`import-token` remains available as a fallback. It writes provider selection/config metadata to `config.json` but stores the bearer token only in `auth.json` next to that config file. The auth file uses a provider-keyed OAuth entry with fields such as `type`, `access`, optional `refresh`, optional `expires`, and optional `accountId`, and is written with secret-file permissions on Unix. Command output prints paths and status only; it does not print tokens.

By default, import selects `gpt-5.4` as the configured model. Use `--no-select` to store auth without changing the selected provider/model.

## Local OpenAI-compatible API

Start the local API server:

```sh
shacs-bot serve --bind 127.0.0.1:8900 --workspace /tmp/ws --timeout 120
```

For compatibility with earlier API-oriented command shapes, `api serve` is an alias for the same command:

```sh
shacs-bot api serve --bind 127.0.0.1:8900 --workspace /tmp/ws --timeout 120
```

The default bind address comes from the JSON config API section and defaults to `127.0.0.1:8900`. Use `--bind <host:port>` or `--host <ip> --port <port>` to override it for one run.

Non-loopback binds require an explicit opt-in because the local API is unauthenticated:

```sh
shacs-bot serve --bind 0.0.0.0:8900 --allow-remote --workspace /tmp/ws
```

API turns use read/search/web tools by default. Write/edit/exec/self-modifying tools require:

```sh
shacs-bot serve --allow-api-side-effects --workspace /tmp/ws
```

Implemented endpoints:

- `GET /health`
- `GET /v1/models`
- `POST /v1/chat/completions`

`POST /v1/chat/completions` accepts a single user message, optional `session_id`, optional `temperature`, optional `max_tokens`, JSON text or data-URL image content parts, multipart uploads, non-stream responses, and `stream=true` Server-Sent Events. Remote image URLs are rejected. Data URLs and uploaded files are persisted under the runtime media directory with a 10 MiB per-file limit.

Same-session API requests are serialized by session key. `--timeout` controls the HTTP wait timeout; if a timeout response is returned, the in-flight turn still owns that session lock until its blocking AgentLoop work exits.

## Channels

Inspect the local channel registry and configured channel plugin settings:

```sh
shacs-bot channels list
shacs-bot channels status --workspace /tmp/ws
```

`channels list` shows built-in channel descriptors, config-enabled state, capabilities, and worker boundary count. `channels status` summarizes configured channel plugins and runtime defaults such as progress/tool-hint delivery and send retry count. These commands are read-only diagnostics; use `run` to start runnable channel workers.

Start the selected channel runtime:

```sh
shacs-bot run --workspace /tmp/ws
shacs-bot run --websocket-host 127.0.0.1 --websocket-port 8765 --workspace /tmp/ws
```

`run` starts the WebSocket channel server when the `websocket` channel is enabled. Incoming JSON text or binary WebSocket frames are normalized through the channel contract, processed by the local `AgentLoop`, and returned as WebSocket server events. The WebSocket config is read from `channels.plugins.websocket` with `enabled`, `host`, `port`, and `path`; command-line `--websocket-host` and `--websocket-port` override the host and port for one run. Non-loopback WebSocket binds require `--allow-remote`.

`run` also starts selected external channel transports when their plugin config contains enough credentials. Missing credentials are reported as `skipped-missing-credentials` rather than failing the whole runtime, so you can enable WebSocket first and add external channels incrementally.

Minimal external channel config keys:

- `channels.plugins.telegram`: `enabled`, `botToken`/`bot_token`/`token`, optional `pollTimeoutSeconds`, `pollLimit`.
- `channels.plugins.discord`: `enabled`, `botToken`/`bot_token`/`token`, `channelIds` or `defaultChannelId`, optional `pollIntervalSeconds`.
- `channels.plugins.slack`: `enabled`, `botToken`/`bot_token`/`token`, `channelIds` or `defaultChannelId`, optional `pollIntervalSeconds`.
- `channels.plugins.email.smtp`: `host`, `port`, `from`, optional `username`, `password`, `security`, `timeoutSeconds`; `channels.plugins.email.imap`: `host`, `port`, `username`, `password`, optional `mailbox`, `markSeen` (defaults to true), `pollIntervalSeconds`, `timeoutSeconds`, `security`.
- `channels.plugins.whatsapp`: `enabled`, `bridgeUrl`, optional `bridgeToken`, `pollPath`, `sendPath`, `pollIntervalSeconds`, `groupPolicy`, and `allowlist.allowedSenders`.

External transports are intentionally minimal adapters: Telegram uses long polling, Discord/Slack poll configured channels over REST, Email uses SMTP/IMAP, and WhatsApp talks to a local bridge HTTP endpoint. They run in the same personal-use `shacs-bot run` process as the local AgentLoop. Email IMAP keeps an in-process UID cache to avoid repeating the same unseen message during a run; restart-time replay is still possible if the server leaves messages unseen.

## Docker Compose

The repository includes a multi-stage Dockerfile and `compose.yaml` for running the local HTTP API as a personal-use service:

```sh
docker compose up --build
```

The compose service runs `shacs-bot serve` in the container. Check the API after startup:

```sh
curl http://127.0.0.1:18080/health
```

Provider secrets should be supplied through your local config/environment workflow; do not bake secrets into the image.

## Reserved commands

The following command names are reserved but not implemented in the Rust CLI yet:

- `plugins`

TUI commands, provider OAuth flows beyond the implemented Codex login, ClawHub install/update wrappers, and gateway supervision remain future migration slices. If a command is not listed above as implemented, do not treat it as available.
