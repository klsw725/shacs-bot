# shacs-bot usage guide

This guide is for a person running `shacs-bot` locally for their own workspace. It does not assume a SaaS control plane, a fleet operator, or a separate administrator role.

For design contracts and invariants, see [docs/specs/README.md](specs/README.md). This page focuses on commands and file formats that are implemented now.

## Build and release gate

From the repository root:

```sh
cargo build --workspace --locked
scripts/release-gate
```

`scripts/release-gate` runs the local Cargo-only shipping checks: format, check, clippy, required release-gate representative tests, release candidate smoke, spec coverage matrix, and workspace tests. Full-spec readiness is still an explicit matrix decision; passing the script alone is not a substitute for the matrix evidence rules.

The examples below use `shacs-bot` as if the binary is on `PATH`. From a source checkout, prefix commands with `cargo run -p shacs-cli --` when needed:

```sh
cargo run -p shacs-cli -- session list --workspace-root /tmp/ws
```

## Docker Compose usage

The repository includes a multi-stage Dockerfile and `compose.yaml` for running the Local HTTP API as a single personal-use service.

Build and start the API:

```sh
docker compose up --build
```

The compose service runs this command inside the container:

```sh
shacs-bot api serve --bind 0.0.0.0:18080 --workspace-root /workspace
```

The host port is intentionally loopback-only:

```text
127.0.0.1:18080 -> container 18080
```

Persistent data lives in the `shacs-workspace` named volume under `/workspace/.shacs/`:

- `/workspace/.shacs/config.toml`
- `/workspace/.shacs/secrets.toml`
- `/workspace/.shacs/runtime`
- `/workspace/.shacs/skills/<skill-name>/SKILL.md`

Do not bake provider secrets into the image. Put them in the mounted workspace volume. For example, import an externally obtained Codex token into the compose volume before starting the API:

```sh
docker compose build
printf '%s' "$CODEX_TOKEN" | docker compose run --rm -T shacs-bot \
  provider codex import-token --token-stdin --workspace-root /workspace
docker compose up
```

The API process reads configuration at startup. If you import or edit provider settings while the API container is already running, restart it so the process reloads the volume-backed files:

```sh
docker compose restart shacs-bot
```

For OpenAI-compatible or Anthropic profiles, create `/workspace/.shacs/config.toml` and `/workspace/.shacs/secrets.toml` in the volume using the formats below. If you prefer editing files from the host, replace the named volume in `compose.yaml` with a bind mount such as `./.shacs-docker:/workspace/.shacs`.

Check the API after startup:

```sh
curl http://127.0.0.1:18080/api/v1/sessions
```

## Configuration files

Config and secrets are separate files. Higher-precedence layers override lower-precedence layers with the same profile or secret name.

Config layers:

1. built-in defaults
2. user-global config: `$HOME/.shacs/config.toml`
3. workspace-local config: `<workspace>/.shacs/config.toml`
4. explicit config override, when a caller supplies one

Secrets layers:

1. user-global secrets: `$HOME/.shacs/secrets.toml`
2. workspace-local secrets: `<workspace>/.shacs/secrets.toml`
3. explicit secret override, when a caller supplies one

The explicit override layers are internal caller inputs, not current CLI flags. For normal local use, place files in the user-global or workspace-local paths above.

When `--workspace-root` is omitted, CLI/TUI/API commands use the user-global config, secrets, and runtime under `$HOME/.shacs`. Workspace-local layers are used only when `--workspace-root <path>` is provided. Keep provider API keys in `secrets.toml`; putting provider secrets in `config.toml` is malformed.

### Example `config.toml`

```toml
schema_version = 1

default_provider_profile = "openai"
default_permission_profile = "local-default"
default_runtime_profile = "local-runtime"

[selection]
provider = "openai"
permission = "local-default"
runtime = "local-runtime"

[provider_profiles.openai]
provider_kind = "openai_compatible"
model_id = "gpt-5"
api_base = "https://api.openai.com/v1/chat/completions"
api_key_ref = "openai.default"
timeout_ms = 45000
max_output_tokens = 2048
tool_calling_enabled = true

[provider_profiles.anthropic]
provider_kind = "anthropic_auth"
model_id = "claude-test"
api_base = "https://api.anthropic.com/v1/messages"
api_key_ref = "anthropic.default"
timeout_ms = 30000
max_output_tokens = 2048
tool_calling_enabled = true

[provider_profiles.codex]
provider_kind = "codex_auth"
model_id = "codex-test"
api_base = "https://chatgpt.com/backend-api/codex/responses"
api_key_ref = "codex.default"
timeout_ms = 30000
max_output_tokens = 2048
tool_calling_enabled = true

[permission_profiles.local-default]
mode = "default"
allowed_capabilities = ["fs_read", "proc_exec"]
allowed_paths = ["/Users/you/project"]
allowed_network_scopes = ["host:api.example.test"]
allowed_secret_scopes = ["openai.default"]
requires_confirmation = true

[runtime_profiles.local-runtime]
root = "/Users/you/project/.shacs/runtime"
working_directory = "/Users/you/project"
```

Supported `provider_kind` values:

- OpenAI-compatible: `OpenAiCompatible`, `openai_compatible`, `openai-compatible`
- Anthropic auth: `AnthropicAuth`, `anthropic_auth`, `anthropic-auth`
- Codex auth: `CodexAuth`, `codex_auth`, `codex-auth`

Supported permission modes:

- `Default` or `default`
- `Auto` or `auto`
- `Plan` or `plan`

Supported capability strings:

- `fs_read`
- `fs_write`
- `proc_exec`
- `shell_exec` as a `proc_exec` alias
- `net_outbound`
- `secret_read`

`net_outbound` and `secret_read` also require explicit scopes in the selected permission profile. Network scopes are descriptive allowlist strings such as `host:api.example.test`; secret scopes are secret reference names such as `openai.default`. Empty scope lists deny scoped network or secret access even when the capability is listed.

When `tool_calling_enabled = true`, configured provider submit flows can execute the built-in filesystem `read` tool through the local read-only executor. Shell execution and filesystem writes are not part of this default runtime path.

Do not add unsupported provider keys such as `temperature`, `top_p`, custom headers, or bearer token fields. The parser rejects unknown fields.

### Example `secrets.toml`

```toml
[provider_secrets.openai]
default = "OPENAI_API_KEY_VALUE"

[provider_secrets.anthropic]
default = "ANTHROPIC_API_KEY_VALUE"

[provider_secrets.codex]
default = "CODEX_AUTH_SESSION_OR_IMPORTED_TOKEN"

[provider_secrets.telegram]
default = "TELEGRAM_BOT_TOKEN_VALUE"

[provider_secrets.discord]
default = "DISCORD_BOT_TOKEN_VALUE"

[secret_refs]
"custom.literal.ref" = "CUSTOM_SECRET_VALUE"
```

`api_key_ref = "openai.default"` resolves the secret key named `openai.default`. A `[provider_secrets.openai] default = "..."` entry is internally exposed as that same `openai.default` secret reference.

### Codex login

`provider codex login` is the Codex/ChatGPT browser OAuth entrypoint. It opens or prints an OpenAI auth URL, receives the localhost redirect on `http://localhost:1455/auth/callback`, stores the resulting refreshable session bundle in the local secrets file, and writes a matching `codex_auth` provider profile in `config.toml`. This matches the user-facing behavior expected from OpenCode/Nanobot-style Codex login while keeping tokens out of stdout/stderr and `config.toml`.

Default browser login:

```sh
shacs-bot provider codex login
```

Workspace-local browser login:

```sh
shacs-bot provider codex login --workspace-root /tmp/ws
```

If the browser cannot be opened automatically, copy the URL printed on stderr:

```sh
shacs-bot provider codex login --no-browser --workspace-root /tmp/ws
```

Headless environments should use a device-code style fallback once available. The flag exists, but device-code login is not implemented yet:

```sh
shacs-bot provider codex login --headless --workspace-root /tmp/ws
```

If a token was obtained outside `shacs-bot`, use the explicit import fallback instead of the default login flow. Importing a token never prints the token to stdout/stderr.

Workspace-local import:

```sh
printf '%s' "$CODEX_TOKEN" | shacs-bot provider codex import-token \
  --token-stdin \
  --workspace-root /tmp/ws
```

User-global import:

```sh
shacs-bot provider codex import-token --token-env CODEX_TOKEN
```

Both browser login and the import fallback write `[provider_profiles.codex]` with `provider_kind = "codex_auth"`, select that provider profile, and keep sensitive material in the corresponding `secrets.toml`. Browser login stores a provider auth session JSON string under the selected secret ref. The import fallback stores the imported bearer token under that same ref. Provider calls accept both forms: a session bundle contributes its `access_token` and optional `account_id`, while a raw imported token is used directly as the bearer token. If the stored OAuth session is expired and has a `refresh_token`, provider calls refresh it before adapter construction and atomically write the refreshed session, including rotated refresh tokens, back to the same `secrets.toml` entry before using the new access token. If refresh is impossible, the call fails before model network access with a re-login message. Success JSON reports paths and secret references only; it never includes token values.

Sessions keep the provider profile snapshot from their creation time. After Codex login or import-token, create a new session before chatting with Codex; sessions created before the login keep their previous provider snapshot and are rejected with an explicit conflict message instead of silently mixing the old model snapshot with the new Codex credentials.

## CLI usage

All CLI success output uses this JSON envelope:

```json
{
  "diagnostics": [],
  "data": {}
}
```

Errors are emitted as:

```json
{
  "diagnostics": [],
  "error": {
    "kind": "usage | runtime | not_found | conflict",
    "message": "..."
  }
}
```

Create and list sessions:

```sh
shacs-bot session create --workspace-root /tmp/ws
shacs-bot session list --workspace-root /tmp/ws
shacs-bot session delete --session-id session-1 --workspace-root /tmp/ws
```

Session deletion removes the stored session metadata, event log, and checkpoint files. It is not reversible.

Inspect runtime/package metadata without a session:

```sh
shacs-bot runtime inspect --workspace-root /tmp/ws
```

`runtime inspect` reports the binary version, runtime root, observed/current data format versions, start admission, lifecycle blockers, and `upgrade_marker` details such as `from_version`, `target_version`, `phase`, and `partial_migration`.

Manage the local runtime ownership marker:

```sh
shacs-bot runtime start --workspace-root /tmp/ws
shacs-bot runtime stop --workspace-root /tmp/ws
shacs-bot runtime update --target-version 0.2.0 --workspace-root /tmp/ws
shacs-bot runtime recover --workspace-root /tmp/ws
shacs-bot runtime restart --workspace-root /tmp/ws
```

The current lifecycle commands manage `<workspace>/.shacs/runtime/ownership.marker.json` and `<workspace>/.shacs/runtime/upgrade.marker.json`. `runtime start` writes the ownership marker after bootstrap/admission checks and rejects a second active owner. `runtime update --target-version ...` records the local update lifecycle as an upgrade marker, performs the current no-op migration completion path, and leaves a `completed_cleanup` marker that `runtime inspect` can report. It is blocked by active ownership, stale ownership, partial migration, interrupted upgrade, and incompatible data states. `runtime recover` clears only stale ownership markers and leaves active ownership, partial migration, and interrupted upgrade markers blocked for inspection. `runtime stop` clears the ownership marker, including stale markers. `runtime restart` clears the ownership marker and then runs the same start admission path again. These commands do not start a background supervisor process or replace OS package manager tooling.

Inspect a session:

```sh
shacs-bot session inspect \
  --session-id session-1 \
  --query summary \
  --workspace-root /tmp/ws
```

For a one-shot CLI request without manually managing a session id, use `ask`:

```sh
shacs-bot ask "hello" --workspace-root /tmp/ws
```

`ask` creates a new session, submits the text, waits for the turn, and returns a JSON envelope containing the generated `session_id`, `data.kind`, and the same projection data used by `session wait`. If an approval is required and stdin is interactive, `ask` prompts for `approve-once`, `deny`, or `cancel-turn` and then continues waiting. In non-interactive execution, or when the prompt receives EOF, it does not auto-approve; it returns the approval projection so callers can decide explicitly. `--timeout-ms` limits the wait-poll loop after submit/approval responses; provider and tool calls use their configured runtime timeouts.

Supported inspect queries:

- `summary`
- `focus`
- `approval`
- `progress`
- `error`
- `diagnostics`
- `recovery`

Resume or submit input:

```sh
shacs-bot session resume --session-id session-1 --workspace-root /tmp/ws

shacs-bot session submit \
  --session-id session-1 \
  --text "hello" \
  --workspace-root /tmp/ws
```

Wait for an active turn:

```sh
shacs-bot session wait \
  --session-id session-1 \
  --interval-ms 1000 \
  --timeout-ms 30000 \
  --workspace-root /tmp/ws
```

`wait` returns `data.kind` as one of `approval`, `recovery`, `completed`, `aborted`, or `timed_out`.

Recover or cancel:

```sh
shacs-bot session recover --session-id session-1 --workspace-root /tmp/ws

shacs-bot session cancel-turn \
  --session-id session-1 \
  --turn-id turn-1 \
  --workspace-root /tmp/ws
```

Respond to an approval request:

```sh
shacs-bot session respond-approval \
  --session-id session-1 \
  --turn-id turn-1 \
  --approval-request-id tool-call-1 \
  --response approve-once \
  --workspace-root /tmp/ws
```

Supported CLI approval responses are `approve-once`, `deny`, and `cancel-turn`.

The runtime also has a provider-neutral mailbox approval response path for
external channel bridges. That path requires explicit `session_id`, `turn_id`,
`approval_request_id`, and response values, dedupes by channel source/message id,
and routes the decision through the same `RespondToApproval` core boundary. It
does not treat mailbox text as assistant output and does not imply that an
approved tool effect has completed.

`telegram-poll` accepts only this exact whole-message approval command:

```text
approval <turn_id> <approval_request_id> <approve-once|deny|cancel-turn>
```

The bridge never infers the latest approval and never parses natural language.
Messages that exactly match the command are routed to the provider-neutral
mailbox approval response path and are not appended to mailbox context. Text that
does not start with the reserved `approval` command remains normal mailbox
context. Malformed messages beginning with `approval` are counted as parse
failures and are not used as approval responses or mailbox context.

Poll Telegram once for text messages:

```sh
shacs-bot channel telegram-poll \
  --session-id session-1 \
  --token-ref telegram.default \
  --ack-text "Received and recorded." \
  --workspace-root /tmp/ws
```

`telegram-poll` calls Telegram Bot API `getUpdates` once with `allowed_updates = ["message"]`, routes exact approval commands to the mailbox approval response path, converts other text messages into mailbox channel ingress, and returns `received_messages`, `skipped_updates`, `context_messages_routed`, `approval_response_parsed`, `approval_response_routed`, `approval_response_accepted`, `approval_response_ignored`, `approval_response_parse_failed`, `ack_requested`, `ack_attempted`, `ack_succeeded`, `ack_failed`, `next_offset`, `effective_offset`, and cursor metadata. By default, the command stores `next_offset` under the selected runtime root at `.shacs/runtime/telegram-cursors/<session>/<token-ref>.json` after all messages are routed. Use `--cursor-file /path/to/cursor.json` to override that location, or `--offset 123456790` to override the stored cursor for one run. Missing cursor files are treated as first runs; malformed or mismatched cursor files are usage errors. `approve-once` only resolves the pending approval decision and queues the approved tool effect; it does not mean the tool completed or the assistant answered. `--ack-text` optionally sends the same static Telegram `sendMessage` acknowledgement after a message is accepted by mailbox context ingress or approval response routing; it is only a transport-level receipt, not an assistant answer, approval result, or processing-complete signal. Acknowledgement failures are reported in `ack_failed` and do not change session truth or cursor persistence. The command reads the bot token only through `secrets.toml`; it does not implement webhooks, public HTTPS endpoints, or non-text Telegram payloads.

Poll one Discord channel once for messages, following the same mailbox approval semantics:

```sh
shacs-bot channel discord-poll \
  --session-id session-1 \
  --token-ref discord.default \
  --channel-id 123456789012345678 \
  --bot-user-id 999999999999999999 \
  --allow-from YOUR_DISCORD_USER_ID \
  --allow-channel 123456789012345678 \
  --ack-text "Received and recorded." \
  --workspace-root /tmp/ws
```

`discord-poll` calls Discord REST `GET /channels/{channel.id}/messages` once with `Authorization: Bot <token>`, routes exact approval commands to the mailbox approval response path, converts other accepted messages into mailbox channel ingress, and optionally sends a static acknowledgement with Discord `POST /channels/{channel.id}/messages`. Acknowledgements use `allowed_mentions.parse = []` and `replied_user = false` to avoid accidental pings. By default, the command stores the highest seen Discord snowflake under `.shacs/runtime/discord-cursors/<session>/<token-ref>/<channel-id>.json`; use `--cursor-file` to override that path or `--after <message-id>` to override the stored cursor for one run. `--allow-from` may be repeated and defaults to `*`; `--allow-channel` may be repeated and defaults to the polled channel set only by `--channel-id`. In guild channels, `--group-policy mention` is the default and accepts messages only when `--bot-user-id` is supplied and the bot is mentioned; leading `<@bot>` or `<@!bot>` is stripped only for approval-command parsing, so `<@bot> approval <turn_id> <approval_request_id> approve-once` routes as an approval response instead of mailbox context. Other bot-authored messages are skipped by default to avoid bot-to-bot loops. Use `--group-policy open` for channel allowlist-only behavior. This command is a one-shot connector for debug, backfill, or external schedulers; it does not wait for assistant turns or push assistant final replies.

Run a foreground Discord assistant worker when you want Discord to behave like a long-running assistant surface:

```sh
shacs-bot channel discord-worker \
  --session-id session-1 \
  --token-ref discord.default \
  --channel-id 123456789012345678 \
  --bot-user-id 999999999999999999 \
  --allow-from YOUR_DISCORD_USER_ID \
  --allow-channel 123456789012345678 \
  --workspace-root /tmp/ws
```

`discord-worker` uses the same Discord REST polling adapter initially, but keeps running until stopped or until `--max-polls <n>` is reached. Normal accepted messages open an assistant turn through the same `SubmitUserInput` path as CLI/TUI, the worker waits for completion or approval using the same wait semantics as `ask` and `session wait`, and completed assistant output is sent back to Discord as a safe reply. If a turn requests approval, the worker sends an explicit strict-command prompt; reply with the exact command it prints to continue the turn. With the default `--group-policy mention` and `--bot-user-id`, that command starts with `<@bot_user_id> approval ...`; with `--group-policy open`, it starts with `approval ...`. If the session already has an open turn when a new normal message arrives, the worker sends `--busy-text` and does not advance past that message. If the worker times out waiting for a submitted turn or cannot send the approval/final Discord reply, it keeps that Discord message pending in memory and retries on a later poll instead of advancing the cursor. `discord-worker` is still a foreground local process, not a hosted Discord Gateway service; Gateway, slash commands, interaction webhooks, attachment download, streaming edits, durable pending-message recovery after process exit, and unified `shacs-bot run` supervision remain future work.

## TUI usage

Start the terminal UI:

```sh
shacs-bot tui --workspace-root /tmp/ws
shacs-bot tui --workspace-root /tmp/ws --session-id session-1
```

Key bindings:

- `q`: quit
- `j`/`k` or arrow keys: move between sessions
- `r`: refresh
- `n`: create a new session
- `D`: delete selected session; press `D` again to confirm because deletion is not reversible
- `S`: resume selected session
- `s`: enter compose mode
- compose mode: `Enter` submits, `Esc` cancels, `Backspace` erases
- `i`: toggle inspect mode
- inspect mode: `h`/`l` or left/right arrows change inspect query
- `R`: recover selected session when recovery is required
- `C`: cancel selected open turn
- approval query in inspect mode: `a` approve once, `d` deny, `c` cancel turn

The TUI uses the same session projections and command semantics as the CLI and local API. If process lifecycle blockers are present, it hides mutating controls and keeps the view in inspect/recovery-oriented mode.

## Local API usage

Start the local API server:

```sh
shacs-bot api serve --bind 127.0.0.1:18080 --workspace-root /tmp/ws
```

The default bind address is `127.0.0.1:18080`. Add `--once` to serve one request and exit.

HTTP success responses use the same `diagnostics` + `data` envelope as the CLI. Error responses use HTTP status codes:

- `400` for `usage`
- `404` for `not_found`
- `409` for `conflict`
- `500` for `runtime`

### Session routes

List sessions:

```http
GET /api/v1/sessions HTTP/1.1
Host: localhost
```

Create a session:

```http
POST /api/v1/sessions HTTP/1.1
Host: localhost
Content-Type: application/json

{}
```

The create body may be empty or `{}`. Unknown fields such as `label` are rejected.

Delete a session:

```http
DELETE /api/v1/sessions/session-1 HTTP/1.1
Host: localhost
```

Deletion removes the session metadata, event log, and checkpoint files and returns `404` when the session does not exist.

Inspect a session:

```http
GET /api/v1/sessions/session-1/inspect?query=summary HTTP/1.1
Host: localhost
```

Supported `query` values are `summary`, `focus`, `approval`, `progress`, `error`, `diagnostics`, and `recovery`.

Resume:

```http
POST /api/v1/sessions/session-1/resume HTTP/1.1
Host: localhost
Content-Length: 2

{}
```

Submit input:

```http
POST /api/v1/sessions/session-1/submit HTTP/1.1
Host: localhost
Content-Type: application/json

{"text":"hello"}
```

Record an external channel message:

```http
POST /api/v1/sessions/session-1/channel-events HTTP/1.1
Host: localhost
Content-Type: application/json

{
  "channel": "telegram",
  "source_id": "chat-1",
  "external_message_id": "message-1",
  "summary": "message summary",
  "observed_at": 1
}
```

Supported `channel` values are `slack`, `discord`, `telegram`, and `email`. This route normalizes the message into a mailbox service command; it does not create sessions, append conversation history directly, or implement provider-specific networking.
If `observed_at` is omitted, the server uses the current local timestamp in milliseconds. `service_correlation_id` may be supplied for adapter-side tracing and is optional.

For Slack, the runtime adapter provides a network-free normalizer for already-received Events API `event_callback` message JSON. It accepts only human `message` events without subtypes or bot fields, maps `team_id:channel` to `source_id`, `event.ts` to `external_message_id`, and `event_id` to `service_correlation_id`. A bridge script can feed that normalized output to this route; Slack webhook hosting, Socket Mode, OAuth installation, and outbound assistant messages are outside this local API route.

For Discord, the runtime adapter provides both a network-free normalizer for already-received message JSON and the `discord-poll` CLI connector described above. The Local API route itself still accepts only normalized provider-neutral mailbox events; it does not host a Discord Gateway, interaction webhook, slash command endpoint, or outbound assistant-message delivery service. The normalizer accepts default human messages with `id`, `channel_id`, `author.id`, non-blank `content`, and `timestamp`; bot, system, webhook, non-default, and incomplete messages are skipped by the raw JSON normalizer. The polling connector adds nanobot-style allowlists, mention gating, reply-message acceptance, bot-authored message filtering, self-loop filtering by `--bot-user-id`, Discord token redaction, mention-stripped approval parsing, and safe acknowledgement replies.

For Email, the runtime adapter provides a network-free normalizer for already-extracted message fields. It requires `source_id`, `external_message_id`, `from`, and plain-text `text`, accepts optional `subject` and `service_correlation_id`, and keeps `observed_at` as the bridge-supplied timestamp. `source_id` is the local bridge's mailbox/account/folder identity, while `external_message_id` is the bridge's stable source-local message identity. IMAP polling, SMTP sending, MIME parsing, OAuth, provider-specific mail APIs, webhook hosting, and outbound assistant messages are outside this local API route.

Record an external channel approval response:

```http
POST /api/v1/sessions/session-1/channel-approval-responses HTTP/1.1
Host: localhost
Content-Type: application/json

{
  "channel": "telegram",
  "source_id": "chat-1",
  "external_message_id": "message-approval-1",
  "turn_id": "turn-1",
  "approval_request_id": "tool-call-1",
  "response": "approve-once",
  "observed_at": 1
}
```

This route uses the provider-neutral mailbox approval response path. It requires explicit `turn_id` and `approval_request_id`, dedupes by channel source/message id, and never appends approval response text to mailbox context. Stale or duplicate responses return the current progress surface without implying tool completion or assistant output.
Slack, Discord, and Email approval text uses the same exact whole-message command as Telegram: `approval <turn_id> <approval_request_id> <approve-once|deny|cancel-turn>`. Malformed messages beginning with `approval` should be treated as parse failures by the bridge and not posted as mailbox context.

Wait:

```http
POST /api/v1/sessions/session-1/wait HTTP/1.1
Host: localhost
Content-Type: application/json

{"interval_ms":1000,"timeout_ms":30000}
```

Recover:

```http
POST /api/v1/sessions/session-1/recover HTTP/1.1
Host: localhost
Content-Length: 2

{}
```

Cancel an open turn:

```http
POST /api/v1/sessions/session-1/cancel-turn HTTP/1.1
Host: localhost
Content-Type: application/json

{"turn_id":"turn-1"}
```

Respond to approval:

```http
POST /api/v1/sessions/session-1/approval/respond HTTP/1.1
Host: localhost
Content-Type: application/json

{
  "turn_id": "turn-1",
  "approval_request_id": "tool-call-1",
  "response": "approve-once"
}
```

API approval responses accept both CLI-style values (`approve-once`, `deny`, `cancel-turn`) and JSON enum values advertised by approval projections (`ApproveOnce`, `Deny`, `CancelTurn`).

## Approval flow

An approval request means a pending effect needs an explicit response. It does not mean the turn has completed.

1. Inspect approval state with CLI, TUI, or API.
2. Read `approval_request_id`, `turn_id`, `capability`, `target_summary`, and `reason`.
3. Respond with approve once, deny, or cancel turn.
4. Continue watching progress or inspect recovery/error state if the turn does not complete.

Responding with a stale `approval_request_id` returns a conflict.

## Recovery and lifecycle blockers

Use the recovery projection to inspect recovery and process lifecycle state:

```sh
shacs-bot session inspect \
  --session-id session-1 \
  --query recovery \
  --workspace-root /tmp/ws
```

Recovery data includes:

- `recovery_required`
- `recovery_reason`
- `interrupted_turn_id`
- `last_committed_sequence`
- `process_blockers`
- `upgrade_marker`
- `available_actions`

`recover` clears an interrupted open-turn recovery state by applying the durable session recovery command. It is not a guarantee that partially executed external work can be resumed automatically.

Mutating operations such as create, submit, cancel, and approval response are blocked when the runtime is in unsafe lifecycle states. Common messages include:

- `runtime mutation blocked by active ownership`
- `runtime mutation blocked until recovery is inspected or completed`
- `runtime mutation blocked by partial migration; inspect only`

List, inspect, resume, and wait remain available for diagnosis. `recover` remains available for interrupted session recovery, but partial migration inspect-only mode blocks it until the upgrade state is resolved.

## Troubleshooting

- `missing selected provider api key`: the selected provider profile has an `api_key_ref`, but the selected secrets layers do not contain that secret.
- malformed config or secrets: remove unsupported keys; config/secrets parsing rejects unknown fields.
- unsupported schema version: use `schema_version = 1` or omit the key.
- open turn conflict: inspect `progress` or use `wait`; cancel or recover if needed.
- stale approval or turn id: refresh/inspect again and use the latest `turn_id` and `approval_request_id`.
