# shacs-bot 사용 가이드

이 문서는 사용자가 자신의 workspace에서 `shacs-bot`을 로컬로 실행하는 경우를 기준으로 합니다. SaaS control plane, fleet operator, 별도 관리자 조직을 전제로 하지 않습니다.

설계 계약과 invariant는 [docs/specs/README.md](specs/README.md)를 참고하세요. 이 문서는 현재 Rust CLI에서 실제 구현된 사용자-facing 표면을 설명합니다.

## 소스에서 빌드

저장소 루트에서 실행합니다:

```sh
cargo build --manifest-path crates/Cargo.toml -p shacs-cli --locked
```

아래 예시는 `shacs-bot` binary가 `PATH`에 있다고 가정합니다. 소스 checkout에서 바로 실행할 때는 필요하면 명령 앞에 `cargo run --manifest-path crates/Cargo.toml -p shacs-cli --`를 붙이세요:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- status
```

## 설정

현재 Rust CLI는 하나의 JSON config 파일을 사용합니다. 기본 경로는 다음과 같습니다:

```text
$HOME/.shacs-bot/config.json
```

특정 config 파일을 사용하려면 `--config <path>` 또는 `-c <path>`를 지정합니다. Runtime 명령은 config를 저장하지 않는 일회성 workspace override로 `--workspace <path>` 또는 `-w <path>`도 받습니다.

각 config 파일의 parent directory가 해당 instance의 data directory입니다. 예를 들어 `/tmp/a/config.json`와 `/tmp/b/config.json`를 따로 쓰면 `auth.json`, `media/`, `cron/`, `logs/`, `channels/worker-metadata/`, `skills/`도 각각 `/tmp/a/`, `/tmp/b/` 아래로 분리됩니다. workspace는 config 값 또는 `--workspace` override로 별도 지정할 수 있지만, runtime metadata는 config별 data directory를 기준으로 유지됩니다.

Config와 workspace template을 생성하거나 갱신합니다:

```sh
shacs-bot onboard --workspace /tmp/ws
shacs-bot --config /tmp/shacs-config.json onboard --workspace /tmp/ws
```

`onboard`는 JSON config를 쓰고, runtime directory를 준비하며, `AGENTS.md`, `SOUL.md`, `USER.md`, `TOOLS.md`, `memory/MEMORY.md`, `memory/history.jsonl`, `skills/` 같은 workspace template 파일을 만듭니다. 또한 active built-in skill을 `builtin_skills/` 아래에 materialize하지만, reference-only deferred built-in skill은 복사하지 않습니다. 이미 존재하는 workspace 파일은 덮어쓰지 않습니다. `onboard --wizard`는 아직 보류된 기능입니다.

Config 문자열 값은 load 시 `${ENV_NAME}` 형태의 environment variable reference를 해석합니다. 예를 들어 provider key는 config에 `"apiKey": "${OPENROUTER_API_KEY}"`로 남겨두고 실행 환경에서 값을 제공할 수 있으며, migration write-back은 실제 secret 값을 config 파일에 저장하지 않습니다. 참조한 environment variable이 없으면 config load가 실패합니다.

스킬의 `requires.env` 확인이나 `exec`/subagent 실행에 필요한 환경 변수는 top-level `env`에 직접 둘 수 있습니다. `tools.exec.env`도 계속 지원하며 같은 key가 있으면 더 구체적인 `tools.exec.env` 값이 우선합니다. 이 값은 exec 실행 환경과 subagent exec 실행 환경에 주입되고, MCP 서버별 환경 변수는 기존처럼 `tools.mcpServers.<name>.env`에 따로 둡니다. MCP `tools.mcpServers.<name>.enabledTools`는 기본값이 빈 배열인 default-deny opt-in입니다. MCP tools/resources/prompts를 노출하려면 `*`, raw capability name, 또는 `mcp_<server>_<kind>_<name>` 형태의 wrapped capability name을 명시하세요. 빈 문자열은 `requires.env`를 만족하지 않습니다. Secret 값을 넣은 config 파일은 커밋하거나 공유하지 마세요.

`permissions.mode`는 provider tool 실행 경로가 소비하는 permission policy gate 설정입니다. Rust runtime은 `default`, `plan`, `accept_edits`, `auto`, `dont_ask`, `bypass_permissions` 값을 파싱해 safe fallback/diagnostics를 만들고, provider tool call과 deferred bridge call을 실행 직전 permission mode snapshot과 policy decision에 통과시킵니다. `allow`만 실제 tool 실행으로 이어지며, `ask`와 `deny`는 실행하지 않은 permission outcome으로 provider-visible tool result에 남습니다. `ask_user`는 여전히 별도 user interruption tool이며 formal approval decision으로 해석되지 않습니다. `auto`는 user-local opt-in이나 명시적 실행 source 없이 workspace config만으로 활성화되지 않고, `bypass_permissions`는 격리 precondition이 충족된 명시적 opt-in일 때만 normalized snapshot에서 유지됩니다.

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
shacs-bot runtime diagnostics --bundle /tmp/shacs-diagnostics.zip --workspace /tmp/ws
```

`runtime inspect`는 선택된 config, workspace, data directory, provider/model, provider 설정 여부, binary version, data schema compatibility classification, ownership status, stop request marker, update marker, runtime capability 요약, containment contained/backend/snapshot digest, session 개수와 최신 session metadata를 보고합니다. `runtime diagnostics` bundle에는 containment summary/digest가 redacted diagnostics field로 포함됩니다. Native host에서 Docker/Compose 같은 인식 가능한 containment evidence가 없으면 containment는 unknown으로 보고되며, sandboxed라고 주장하지 않습니다. `bwrap`는 공식 image/package에 포함되어 자동 설정된 경우가 아니라면 optional hardening입니다. `auth.json` token 값이나 raw session message는 노출하지 않으며, 장기 실행 cron/heartbeat worker를 시작하거나 실행 중인 것처럼 표시하지 않습니다.

Workspace context file과 inline `@` reference가 어떻게 해석되는지 dry-run으로 확인합니다:

```sh
shacs-bot context files list --workspace /tmp/ws
shacs-bot context files inspect --workspace /tmp/ws
shacs-bot context refs parse "read @src/lib.rs and @diff"
shacs-bot context refs resolve --workspace /tmp/ws --message "read @src/lib.rs"
```

`context files list/inspect`는 `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `.shacs.md`, `.shacs-bot.md` 같은 workspace context file 후보의 path, ordering, included/skipped/truncated/denied status, digest, byte/token estimate만 보여주며 raw file content는 출력하지 않습니다. `context refs parse`는 source를 읽지 않고 message 안의 token span, kind, normalized target, parse diagnostic만 표시합니다. 지원되는 reference syntax는 `@path`, `@folder/`, `@diff`, `@staged`, `@git:<rev>`, `@git:<rev>:<path>`, `@url:https://...`, `@https://...`입니다. `context refs resolve`는 read-only resolver, permission/redaction safety gate, shared context budget handoff를 통과한 status를 보여줍니다. 파일/URL body와 provider context block content는 diagnostics에 저장하지 않고 digest와 redacted summary만 사용합니다.

Context limits는 bounded file bytes, folder entry limit, URL byte limit, provider handoff의 shared context budget으로 적용됩니다. Protected target(`.env`, SSH/private key path 등)은 content를 읽기 전에 denied evidence로 처리되고, URL reference는 명시적으로 network가 enabled된 resolve 경로가 아니면 skipped diagnostic이 됩니다. External URL content는 `external_untrusted` trust label로 표시되며 prompt-injection 방어상 instruction이 아니라 data로 취급됩니다. Secret-like content는 diagnostics와 provider handoff 전에 redaction pass를 통과하고, replay는 live URL fetch나 mutable git state 재실행 대신 recorded digest/excerpt/evidence를 사용합니다.

## Plugins and hooks

User-local plugin manifests can be inspected through a descriptor-only management surface. This is the implemented plugin foundation, not the full live plugin execution system:

```sh
shacs-bot plugins list
shacs-bot plugins inspect <name>
shacs-bot plugins doctor
shacs-bot plugins enable <name>
shacs-bot plugins disable <name>
shacs-bot hooks list
shacs-bot hooks inspect <plugin-or-hook>
```

`plugins list`, `plugins inspect`, `plugins doctor`, `hooks list`, and `hooks inspect` read config and discovered manifests only. They do not run plugin commands, dispatch hook callbacks, register provider-visible tools, start MCP servers, or execute plugin processes. `plugins enable` and `plugins disable` mutate only `plugins.enabled`/`plugins.disabled` in the selected config file and report next-session/reload semantics; they do not mutate the running session prompt or toolset.

During agent turns, enabled plugin hook entrypoints may run through the runtime hook adapter for diagnostics-only dispatch. This initial live hook slice records redacted output/error/timeout evidence and does not apply hook output to tool calls, model content, permissions, provider-visible tools, commands, skills, or MCP servers.

Currently supported manifests are `plugin.json` and `plugin.toml` files under the config data directory's `plugins/<name>/` root or the workspace-local `.shacs-bot/plugins/<name>/` root. Workspace-local plugins still require an explicit trusted workspace gate before they can become enabled. Output shows secret reference names and presence metadata only, never raw secret values.

공식 로컬 lifecycle 명령으로 foreground channel runtime을 시작하거나 실행 중인 owner에게 종료/재시작을 요청합니다:

```sh
shacs-bot runtime start --workspace /tmp/ws
shacs-bot runtime stop --workspace /tmp/ws
shacs-bot runtime restart --workspace /tmp/ws
```

`runtime start`는 기존 `run`과 같은 channel runtime foreground 경로를 lifecycle admission과 ownership marker 획득 이후 실행합니다. `runtime stop`과 `runtime restart`는 data directory 아래 `runtime/stop-request.json`을 기록할 뿐 active ownership marker를 직접 삭제하거나 숨은 daemon을 시작하지 않습니다. 실행 중인 owner는 stop/restart request를 관찰하면 장기 실행 loop를 종료하고 정상 종료 시 자신이 소유한 `runtime/ownership-marker.json`만 정리합니다. active owner가 없으면 no-op 상태를 보고하고, stale owner만 있으면 `runtime recover`로 정리하라고 보고합니다.

소스 checkout/Cargo 기반 설치에서 새 binary를 빌드하거나 교체한 뒤 runtime upgrade evidence를 기록합니다. 현재 Rust 구현은 session data transform migration이 없는 no-op migration 경로만 수행하며, 실제 binary 교체나 `git pull`/`cargo install`은 사용자가 별도로 수행합니다:

```sh
shacs-bot --version
shacs-bot runtime update --target-version <current-shacs-bot-version>
shacs-bot runtime inspect
```

`runtime update`는 config 옆 data directory 아래의 `runtime/update-marker.json`에 current/target version, schema version, migration 필요 여부, 완료 phase를 atomic temp-file + rename 방식으로 기록합니다. 소스 checkout 기반 workflow에서는 사용자가 이미 새 binary를 실행 중이라는 증거로 `--target-version`이 현재 `shacs-bot --version`과 일치해야 합니다. 기존 `in_progress` 또는 `partial_migration` marker가 있으면 새 update와 일반 runtime mutation을 막고 먼저 inspect/recover를 요구합니다.

중단된 update marker나 완료된 local update marker를 정리합니다:

```sh
shacs-bot runtime recover
shacs-bot runtime recover --workspace /tmp/ws
```

`runtime recover`는 marker가 없으면 no-op으로 보고하고, partial migration marker와 active ownership은 세션 truth나 실행 중인 owner를 추측으로 고치지 않기 위해 차단합니다. 완료/중단 update marker 또는 stale ownership marker를 지운 뒤에는 다시 `runtime inspect`로 marker가 없는 상태를 확인할 수 있습니다.

Rust programmatic facade(`ShacsBot`/`Nanobot`)는 lifecycle hook과 별도로 observability hook을 제공합니다. Observability hook은 provider stream event와 tool start/finish progress payload를 in-process callback으로 그대로 전달하므로, tool arguments나 provider delta에 민감한 내용이 포함될 수 있습니다. Hook 구현자는 필요한 경우 직접 redaction 후 저장/로그 처리해야 하며, hook panic은 runtime을 중단하지 않고 redacted event kind만 stderr에 남깁니다.

일반 runtime turn은 skill 사용 알림과 subagent 시작 알림을 기본으로 보냅니다. Always-on skill이 실제 system prompt에 적재되는 새 session에서는 활성 skill 알림을 한 번 보내고, 선택 skill의 `SKILL.md`를 성공적으로 참고하면 해당 skill 사용 알림을 보냅니다. Background subagent가 성공적으로 시작되면 시작 메시지를 별도 runtime 알림으로 보냅니다. 단순히 사용 가능한 skill 목록에 나타나는 catalog 정보는 사용 알림으로 보내지 않고, 일반 도구/MCP 호출은 별도 사용 알림으로 보내지 않습니다.

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

`skills list`는 `onboard`가 `builtin_skills/`를 materialize하기 전에도 embedded active built-in skill을 포함합니다. Deferred built-in skill은 reference-only source로 보관되며 `onboard`, `skills list`, `skills show`에는 나오지 않습니다. Workspace skill은 built-in skill을 shadow할 수 있습니다. Shadowed, conflicted, malformed 같은 비활성 diagnostic까지 보려면 `skills list --all`을 사용하세요. `skills show`는 source, status, body hash, requirements, install metadata, diagnostics를 출력합니다. ClawHub search/install/update 명령은 이후 slice로 남아 있습니다.

## 앱

현재 Rust CLI의 app 표면은 사용자가 local `.shacsapp` bundle을 registry에 등록하고 상태를 관찰하는 baseline입니다. `apps init`은 설치 전 authoring draft만 생성하며 install, enable, start를 수행하지 않습니다. Bundle은 config data dir의 `apps/<app-id>.shacsapp/` 경로에 있어야 하며, 기본 config 기준으로는 `~/.shacs-bot/apps/<app-id>.shacsapp/`입니다. Manifest의 `id`와 directory 이름은 일치해야 합니다.

```sh
shacs-bot apps init demo.app --workspace /tmp/ws
shacs-bot apps install ~/.shacs-bot/apps/demo.app.shacsapp --workspace /tmp/ws
shacs-bot apps list --workspace /tmp/ws
shacs-bot apps inspect demo.app --workspace /tmp/ws
shacs-bot apps enable demo.app --workspace /tmp/ws
shacs-bot apps disable demo.app --workspace /tmp/ws
shacs-bot apps uninstall demo.app --workspace /tmp/ws
```

`apps init <app-id>`은 config data dir 아래 `authoring/apps/draft-<app-id>/`에 `draft.json`, `scaffold-plan.json`, `candidates/manifest.json`, `candidates/README.md`를 만듭니다. 이 명령은 app registry를 변경하지 않고, MCP/process/package/network 실행, secret read, grant 생성, active skill 주입을 하지 않습니다. 같은 내용의 draft가 이미 있으면 idempotent하게 기존 draft summary를 보여주며, 다른 내용이면 덮어쓰지 않고 conflict로 멈춥니다.

`apps install`은 `--bundle <path>` 또는 positional bundle path를 받습니다. 상대 경로도 canonicalize 후 config data dir의 `apps/<app-id>.shacsapp/`로 해석되면 허용됩니다. Install은 manifest와 선언 resource/skill/entry file을 읽어 digest와 summary를 registry에 저장하지만, app process를 자동 실행하지 않고 permission grant나 secret 주입을 승인하지도 않습니다. Registry의 grant reference는 permission/secret request를 나중에 연결하기 위한 placeholder이며 승인 상태가 아닙니다.

`apps list`는 app id, version, lifecycle state, digest를 요약합니다. `apps inspect`/`apps show`는 bundle path, permission/secret request 개수, process snapshot 개수, unavailable reason, grant reference를 표시합니다. `apps enable`과 `apps disable`은 registry lifecycle state만 바꾸며 실행 중인 process를 시작하거나 중지하지 않습니다. `apps uninstall`은 registry entry와 config data dir 안의 해당 local bundle directory를 제거하며, persisted registry path가 data-dir/id convention과 맞지 않으면 임의 경로를 삭제하지 않습니다.

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

Command router는 priority, exact, prefix 경계를 분리합니다. `/status`, `/stop`, `/restart`는 priority command라서 같은 session의 active turn이 있어도 provider 호출 전에 처리됩니다. `/new`, `/help`, `/dream` 같은 exact command와 `/history 25`, `/dream-log <sha>`, `/dream-restore <sha>` 같은 prefix command는 일반 user turn과 같은 process-local session turn lock을 공유합니다. Prefix로 등록되지 않은 command는 정확히 일치할 때만 command로 처리됩니다. 예를 들어 `/status now`는 `/status` command가 아니라 일반 user message로 처리됩니다.

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

Codex `import-token`은 fallback으로 유지됩니다. Provider 선택/config metadata는 `config.json`에 쓰지만 bearer token은 config 옆 `auth.json`에만 저장합니다. Auth file은 provider-keyed OAuth entry를 사용하며 `type`, `access`, optional `refresh`, optional `expires`, optional `accountId` 같은 field를 가집니다. Unix에서는 secret-file permission으로 작성됩니다. Command output은 path와 status만 출력하며 token은 출력하지 않습니다. 기본적으로 import는 configured model을 `gpt-5.4`로 선택합니다. Auth만 저장하고 provider/model 선택을 바꾸지 않으려면 `--no-select`를 사용하세요.

Generic provider API key는 아래처럼 `import-key`로 가져옵니다:

```sh
shacs-bot provider import-key --provider openrouter --token-env OPENROUTER_API_KEY
```

이 경로도 secret은 `auth.json`에만 저장하고 `config.json`에는 provider 설정만 남깁니다.

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

`POST /v1/chat/completions`는 단일 user message, optional `session_id`, optional `temperature`, optional `max_tokens`, JSON text 또는 data-URL image content part, multipart upload, non-stream response, `stream=true` Server-Sent Events를 받습니다. Remote image URL은 거부합니다. Data URL과 uploaded file은 runtime media directory의 `attachments/api/` subtree 아래에 저장되며, 파일당 10 MiB 제한이 있습니다. 저장된 attachment는 provider/model이 native image input을 지원한다고 확인되는 경우 image block으로 라우팅되고, text/PDF/Office 계열은 가능한 경우 text note와 추출 텍스트로 라우팅됩니다. Provider나 model이 image input을 지원하지 않는 경우 image attachment는 raw 경로를 노출하지 않는 unsupported note로 전달됩니다. Audio attachment는 지원되는 analyzer가 runtime에 주입된 경우 bounded transcript 또는 summary text artifact로 라우팅되고, analyzer가 없거나 지원되지 않으면 내용을 들은 것처럼 처리하지 않고 unsupported 또는 extraction_failed note로 남습니다. Video attachment도 같은 capability-based 방식입니다. Runtime에 video analyzer가 주입된 경우에만 byte/duration cap 이후 bounded metadata, subtitle excerpt, scene/keyframe summary, PRD 003 audio analyzer를 재사용한 audio-track transcript/summary 후보를 만들고, analyzer가 없으면 deferred가 아니라 `video analyzer is not configured` unsupported note로 남깁니다. 기본 ffmpeg, built-in codec parser, native outbound video delivery는 제공하지 않습니다.

같은 session key의 API request는 CLI/channel runtime과 같은 process-local `SessionTurnLock`으로 직렬화됩니다. `--timeout`은 HTTP wait timeout을 제어합니다. Timeout response가 반환되어도 in-flight turn은 blocking `AgentLoop` 작업이 끝날 때까지 해당 session lock을 계속 소유합니다.

## 채널

로컬 channel registry와 configured channel plugin 설정을 확인합니다:

```sh
shacs-bot channels list
shacs-bot channels status --workspace /tmp/ws
```

`channels list`는 built-in channel descriptor, config-enabled 상태, capability, worker boundary 개수를 보여줍니다. `channels status`는 configured channel plugin과 `channels.sendMemoryHints`, send retry count 같은 runtime default를 요약합니다. `sendMaxRetries`는 `ChannelManager`가 channel adapter로 메시지를 넘기는 dispatch/enqueue와 실제 transport send의 총 시도 횟수이며, 값은 최소 1회, 최대 10회로 제한됩니다. 이 명령들은 read-only diagnostics입니다. Runnable channel worker를 시작하려면 `run`을 사용하세요.

선택된 channel runtime을 시작합니다:

```sh
shacs-bot run --workspace /tmp/ws
shacs-bot run --websocket-host 127.0.0.1 --websocket-port 8765 --workspace /tmp/ws
```

`run`은 `websocket` channel이 enabled이면 WebSocket channel server를 시작합니다. 들어오는 JSON text 또는 binary WebSocket frame은 channel contract를 통해 normalize되고, 로컬 `AgentLoop`에서 처리된 뒤 WebSocket channel adapter와 `ChannelManager` dispatch 정책을 거쳐 WebSocket server event로 반환됩니다. Provider text delta는 너무 잘게 나가지 않도록 coalesce되어 `delta` event로 전달되고, turn 종료 시 `stream_end` event가 뒤따릅니다. 최종 assistant answer는 backward compatibility를 위해 기존 `message` event로도 유지됩니다. WebSocket server는 adapter event를 bounded queue로 socket writer에 넘기므로 느린 client에는 backpressure가 적용되고, client disconnect 시 해당 connection의 event delivery를 중단합니다. WebSocket config는 `channels.websocket`의 `enabled`, `host`, `port`, `path`에서 읽습니다. Command-line `--websocket-host`와 `--websocket-port`는 한 번의 실행에서 host/port를 override합니다. Non-loopback WebSocket bind에는 `--allow-remote`가 필요합니다. `run --verbose`와 `web --verbose`는 input/response/tool/usage preview만 stderr에 찍고 raw prompt나 full payload는 출력하지 않습니다.

`onboard`는 built-in channel별 기본 config stub을 `channels.<name>`에 생성하고, 기존 channel config가 있으면 사용자 값과 secret/env placeholder를 덮어쓰지 않은 채 누락된 기본 key만 병합합니다. `run`은 plugin config에 충분한 인증 정보와 현재 구현된 transport에 필요한 설정이 있으면 선택된 외부 channel transport도 시작합니다. 인증 정보가 없으면 전체 runtime을 실패시키지 않고 `skipped-missing-credentials`로 보고하므로, WebSocket부터 켜고 외부 channel을 점진적으로 추가할 수 있습니다. Slack은 Socket Mode worker를 사용하므로 `appToken`/`app_token`과 `botToken`/`bot_token`/`token`이 모두 필요합니다. Discord는 Gateway worker를 사용하므로 `allowChannels: []`가 원본처럼 봇이 볼 수 있는 모든 채널을 의미합니다. `allowChannels: ["*"]`도 모든 채널 허용으로 처리합니다.

최소 외부 channel config key:

- `channels.telegram`: `enabled`, `botToken`/`bot_token`/`token`, optional `pollTimeoutSeconds`, `pollLimit`.
- `channels.discord`: `enabled`, `botToken`/`bot_token`/`token`, optional `allowFrom`, `allowChannels`, `groupPolicy`(`mention`/`open`), `streaming`(기본 true). `allowChannels`가 비어 있거나 `["*"]`이면 봇이 볼 수 있는 모든 채널을 허용합니다. Gateway worker는 현재 일반 메시지/DM 응답과 best-effort Gateway resume metadata를 다루며, approval routing과 restart auto-replay는 후속 확장 범위입니다.
- `channels.slack`: `enabled`, `appToken`/`app_token`, `botToken`/`bot_token`/`token`, optional `channelIds`/`allowedChannelIds`/`allowChannels` 또는 `defaultChannelId`.
- `channels.email`: `enabled`, `consentGranted: true`, inbound 허용 목록 `allowFrom`/`allowedSenders`가 필요합니다. `channels.email.smtp`: `host`/`smtpHost`, `port`, `from`/`fromAddress`, optional `username`/`smtpUsername`, `password`/`smtpPassword`, `security`, `timeoutSeconds`; `channels.email.imap`: `host`/`imapHost`, `port`, `username`/`imapUsername`, `password`/`imapPassword`, optional `mailbox`, `markSeen`(기본 true), `pollIntervalSeconds`, `timeoutSeconds`, `security`. 현재 IMAP polling은 TLS(`security: "tls"`)만 시작하며, inbound Email은 `Authentication-Results` header의 `spf=pass`/`dkim=pass`를 기본 확인합니다(`verifySpf`/`verifyDkim`로 비활성화 가능).
- `channels.whatsapp`: `enabled`, `bridgeUrl`(WebSocket URL; 기존 `http://`/`https://` 값은 같은 host/path의 `ws://`/`wss://`로 호환 변환), optional `bridgeToken`, `groupPolicy`, `allowlist.allowedSenders`.

외부 transport는 의도적으로 최소 adapter입니다. Telegram은 long polling, Discord는 Gateway worker(필요 시 configured channel REST polling mode), Slack은 Socket Mode inbound와 Web API outbound, Email은 SMTP/IMAP, WhatsApp은 로컬 bridge WebSocket과 통신합니다. 외부 transport inbound/outbound는 runtime `MessageBus`와 `ChannelManager` dispatch 경계를 통과하며, worker lifecycle은 channel adapter의 `start`/`stop`이 관리합니다. 외부 agent processor는 같은 session key의 turn을 동시에 실행하지 않고, 진행 중 같은 session에서 들어온 메시지를 in-memory pending follow-up queue에 보관한 뒤 현재 turn이 끝나면 이어 처리합니다. 이 queue는 process-local이며 durable pending-message queue, cancellation persistence, restart replay가 아닙니다. 서로 다른 session은 transport processor 안에서 병렬 turn으로 처리될 수 있습니다. Telegram/Discord/Slack은 provider text delta를 채널에 보내지 않고 최종 assistant answer만 새 message로 전송하며 기존 message를 edit/update하지 않습니다. Platform message 길이 제한에 가까워지면 문단/줄/공백 경계에서 후속 message로 나눠 전송합니다. Background subagent가 시작되면 같은 channel/thread에 시작 알림을 보내고, Discord는 agent turn이 실행되는 동안 그리고 그 turn이 시작한 background subagent가 같은 session에서 계속 실행되는 동안 같은 channel/thread에 typing indicator를 best-effort로 반복 전송합니다. Slack Web API outbound에는 bot typing indicator 전송 API가 없어 fake “typing...” 메시지는 보내지 않습니다. Email/WhatsApp은 final-only transport로 유지됩니다. Telegram topic, Slack thread, Discord thread, Email subject/reply context는 outbound reply metadata로 이어집니다. 모두 로컬 `AgentLoop`와 같은 personal-use `shacs-bot run` process 안에서 실행됩니다. Telegram offset, Discord REST last message id, Discord Gateway resume state, Email IMAP seen UID + UIDVALIDITY hint, outbound delivery pending/sent/failed/processed status는 runtime metadata JSON에 best-effort로 저장됩니다. 이 metadata는 duplicate/replay를 줄이기 위한 cursor/dedupe/diagnostic hint이며 durable inbound/outbound queue, transaction, restart auto-replay, exactly-once delivery 보장은 아닙니다. Server가 message를 unseen 상태로 남겨두거나 crash timing이 겹치면 재시작 후 중복 또는 유실 가능성은 여전히 있습니다.

## Docker Compose

저장소에는 개인 사용 서비스로 로컬 HTTP API와 channel runtime을 실행하기 위한 multi-stage Dockerfile과 `docker-compose.yml`이 포함되어 있습니다. Docker/Compose는 현재 zero-setup containment의 primary path입니다. 사용자가 host에 gVisor, Firecracker, Kata, bubblewrap 같은 별도 sandbox runtime을 직접 설치하는 것을 기본 경로로 요구하지 않습니다. Docker 동작은 upstream nanobot deployment와 같이 먼저 host config directory를 초기화하고, host의 `~/.shacs-bot`을 non-root container user의 `/home/shacs/.shacs-bot`에 mount하는 방식을 따릅니다:

```sh
export SHACS_UID=$(id -u)
export SHACS_GID=$(id -g)
mkdir -p ~/.shacs-bot
docker compose run --rm shacs-cli onboard   # first-time setup
vim ~/.shacs-bot/config.json                # add API keys or provider config
docker compose up -d shacs-gateway          # start channel runtime
```

`shacs-gateway`는 container 안에서 `shacs-bot run --websocket-host 0.0.0.0 --allow-remote`를 실행하고 host loopback의 WebSocket port `8765`에만 publish합니다. 이 Compose path는 Docker socket mount, `privileged: true`, host network를 기본값으로 쓰지 않습니다. `run --verbose`를 붙이면 preview-only runtime logs가 stderr에 남고, Compose에서는 `docker compose logs -f shacs-gateway`로 확인할 수 있습니다. Provider 설정이 없으면 runtime은 `provider not found: auto`로 시작하지 않으므로, 먼저 `config.json` 또는 `auth.json` workflow로 provider를 설정하세요.

Spec023의 공식 Compose smoke gate는 opt-in으로 실행합니다. 이 명령은 임시 Compose service와 data directory로 `runtime inspect`의 official-container runtime evidence를 확인하고, 별도로 기본 `docker-compose.yml`이 Docker socket, `privileged: true`, host network를 쓰지 않는지도 검사합니다:

```sh
./docs/scripts/spec023-compose-smoke.sh
```

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

Channel runtime preview logs를 확인하거나 서비스를 내리려면 다음 명령을 사용합니다:

```sh
docker compose up -d shacs-gateway
docker compose logs -f shacs-gateway
docker compose down
```

Provider secret은 로컬 config/environment workflow로 제공하세요. Image 안에 secret을 bake하지 마세요. 기본 container UID/GID는 nanobot과 같은 `1000:1000`이고, 위 예시처럼 `SHACS_UID`/`SHACS_GID`를 지정하면 host user 소유권에 맞춰 실행합니다. Docker containment는 permission mode나 side-effect gate를 없애는 근거가 아니며, unsafe privileged evidence가 보이면 permissive permission mode는 safe fallback으로 내려갑니다. Permission denied가 계속 나면 host에서 `sudo chown -R 1000:1000 ~/.shacs-bot`로 ownership을 맞추거나 Podman의 `--userns=keep-id` 같은 실행 user 전략을 사용하세요.

## 아직 남은 명령 범위

`plugins`와 `hooks`는 위의 plugin/hook 섹션에 설명된 descriptor-only 관리 명령으로 구현되어 있습니다. TUI command, 구현된 Codex login 외의 provider OAuth flow, ClawHub install/update wrapper, gateway supervision은 이후 migration slice로 남아 있습니다. 위에서 구현된 것으로 명시하지 않은 command는 사용할 수 있는 기능으로 취급하지 마세요.
