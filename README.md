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

Spec 031 config migration은 기존 JSON을 유지하며 별도 dry-run/apply/recover 표면을 제공합니다. 기존 `runtime migrate`는 029 stored-data family용이고, 아래 명령은 config/profile/auth-source declaration transform만 수행합니다. TOML 전환은 하지 않습니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime config-migrate --dry-run --config /tmp/shacs-config.json
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime config-migrate --apply --config /tmp/shacs-config.json
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime config-migrate --recover --config /tmp/shacs-config.json
```

Persisted execution snapshot과 activation record는 JSON diagnostics로 inspect할 수 있습니다. 두 표면은 현재 authorization이나 live config/auth truth가 아니며 replay는 resource discovery, credential resolution, dependency preparation, entrypoint execution을 수행하지 않습니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime snapshot inspect /tmp/execution-snapshot.json
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime activation inspect activation:skill:formatter:v1 --store /tmp/activations.json --owner workspace:sha256:owner
```

Plugin과 hook manifest 상태는 management surface로 확인합니다. `plugin.json`과 `plugin.toml` manifest discovery/config gate를 지원하며, `plugins`/`hooks` inspect 계열 명령은 plugin command, hook callback, MCP server, process를 실행하지 않습니다. `enable`/`disable`은 config만 수정하고 다음 session/reload에 적용된다고 보고합니다. Agent runtime에서 enabled plugin의 typed hook entrypoint는 redacted diagnostics로 dispatch될 수 있고, 현재 behavior-affecting 소비 범위는 `tool:before`의 block-only 결과를 도구 실행 직전 normalized tool error로 반환하는 것뿐입니다. 이 block은 permission approval/allow/grant를 만들 수 없습니다. Enabled plugin의 command-backed tool은 기존 tool registry/executor 경계로 등록되고, plugin MCP declaration은 production MCP startup 경로에만 투영되며, plugin-provided skill은 read-only skill root로 agent context에 포함됩니다. Enabled plugin command는 builtin `CommandId`를 확장하지 않는 별도 plugin command router/dispatcher 경계에서만 실행됩니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- plugins list --workspace /tmp/shacs-ws
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- plugins doctor --workspace /tmp/shacs-ws
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- hooks list --workspace /tmp/shacs-ws
```

공식 로컬 runtime lifecycle 진입점은 `runtime start/stop/restart`입니다. `runtime start`는 channel runtime foreground 경로를 실행하며, strict v1 local owner lease를 획득한 뒤 API/WebSocket/external channel processor/component supervision 상태를 기록합니다. `runtime stop`과 `runtime restart`는 실행 중인 로컬 owner generation에 연결된 durable request와 stop-request marker를 기록합니다. `runtime restart`는 안전 종료 의도만 남기며, process를 자동으로 reexec하거나 외부 process manager를 대신하지 않습니다. 다음 start는 사용자가 다시 실행하거나 Docker Compose 같은 외부 supervisor가 수행해야 합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime start --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime stop --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime restart --workspace /tmp/shacs-ws
```

`runtime inspect`는 event/checkpoint recovery, durable work queue, durable child recovery, durable diagnostics evidence, owner lease, supervision-state, channel restart hint 상태를 분리해 표시합니다. Pending/retry/cancellation request는 restart 뒤 복원되고, stale work lease나 legacy stale owner lease는 writable start를 막습니다. 과거 v1 runtime이 기록한 `automation.owner_request` suppress 성공 이벤트 중 lease가 누락된 정확한 legacy 형태는 읽기 호환으로 terminal 상태를 복원하지만, 새 work 상태 전이는 append 전에 검증해 같은 손상 이벤트가 다시 저장되지 않게 합니다. 현재 lock-aware owner는 lifetime process lock과 갱신되는 fenced heartbeat를 함께 사용합니다. 비정상 종료 뒤 heartbeat가 만료되면 다음 `run`/`serve`가 prior owner lifecycle event를 먼저 기록하고 새 generation으로 인계하며, fresh heartbeat가 있는 owner는 PID namespace가 달라도 인계하지 않습니다. Lock protocol이 없는 legacy marker와 일반 수동 복구는 `runtime recover`로 정리합니다. Running 중 중단된 child는 성공으로 추정하지 않고 `recovery_needed`로 표시하며, `runtime recover`는 해당 child에 cancellation request와 terminal cancelled fact를 순서대로 기록합니다. 일반 stale work lease는 pending으로 requeue하고, 이미 cancellation request가 있는 stale lease는 `Cancelled`로 확정합니다. `owner_lost`와 `failed_shutdown`은 supervision shutdown evidence로 남으며, inspect/recover 출력에서 queue/work/child/event/checkpoint/component outcome을 확인해야 합니다. Durable diagnostics evidence는 event sequence를 설명하는 redacted trace/log 보조 자료이며 event truth, replay 입력, writable admission 기준이 아닙니다. Channel restart projection은 worker metadata의 cursor/delivery/dedupe hint와 pending durable inbound safe ref만 보여주며 session truth나 exactly-once delivery가 아닙니다. Durable work queue는 policy/session truth, channel delivery truth, child result truth가 아니며 exactly-once 실행을 보장하지 않습니다. 이 runtime lifecycle은 fleet/admin 제어면, 자동 reexec 또는 범용 process-manager를 제공하지 않습니다. 현재 local safety bound는 inline payload 16 KiB, work payload 1 MiB, open work 1,024개, retry 5회, terminal work/child projection 각 512개, runtime request projection 32개이며, payload store와 event log가 각각 512 MiB에 도달하면 새 enqueue를 받지 않습니다.

Stored-data migration은 `runtime start`에서 자동 실행되지 않습니다. `runtime inspect`는 migration plan 요약을 표시하고, 변환이 필요하거나 partial ledger가 있으면 writable runtime admission을 차단합니다. 먼저 dry-run plan을 확인한 뒤 명시적으로 apply/resume하세요. 출력은 family, version, action, opaque digest/ref만 보여주며 raw payload나 secret을 출력하지 않습니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime migrate --dry-run --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime migrate --apply --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime migrate --resume --workspace /tmp/shacs-ws
```

문제 상황을 확인해야 할 때는 로컬 runtime diagnostics를 bundle로 저장할 수 있습니다. diagnostics 출력과 bundle은 secret, token 같은 민감한 값을 가리며, durable diagnostics evidence와 supervision projection도 raw provider/tool/channel payload, absolute host path, process handle, raw owner id, raw child identity 없이 같은 redacted projection으로 포함합니다. Native host에서 Docker/Compose 같은 인식 가능한 containment evidence가 없으면 containment는 unknown으로 보고됩니다:

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

Codex OAuth 로그인을 이미지 생성 provider에도 사용할 수 있습니다. 로그인 후 `config.json`에서 `tools.imageGeneration`을 활성화하면 `image_generate`가 Codex의 `gpt-image-2` backend를 호출하고 생성 파일을 local media artifact로 저장합니다:

```json
{
  "tools": {
    "imageGeneration": {
      "enable": true,
      "provider": "openai_codex",
      "model": "gpt-image-2"
    }
  }
}
```

이 경로는 Codex 계정의 image-generation entitlement가 필요하며 공개 OpenAI Images API가 아닌 Codex backend 계약을 사용합니다. 자세한 인증 절차와 제한은 [`docs/USAGE.md`](docs/USAGE.md)를 참고하세요.

Spec 033 local goal owner는 CLI와 local API에서 같은 typed snapshot을 사용합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- goal set "finish local verification" --workspace /tmp/shacs-ws --session cli:direct
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- goal status --workspace /tmp/shacs-ws --session cli:direct
curl http://127.0.0.1:8900/v1/sessions/cli%3Adirect/goal-snapshot
```

사용자 소유 local artifact에 대한 CAS self-improvement는 `improve propose|inspect|apply|verify|candidate|rollback`으로 실행합니다. Target은 `--root` 아래 상대 경로만 허용하며 apply/rollback은 current hook, confirmation, process, sandbox, credential gate를 다시 확인합니다.

Recorded-only trajectory와 release evidence는 다음 binary로 생성합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- trajectory record --workspace /tmp/shacs-ws --store /tmp/shacs-trajectories --trajectory-id local-run-1 --instruction "record local no-provider automation"
cargo run --manifest-path crates/Cargo.toml -p shacs-core --bin spec033-release-runner -- --run-id local-release-1 --repo-root . --evidence-root /tmp/shacs-spec033-release --trajectory-root /tmp/shacs-trajectories --trajectory-id local-run-1 --data-dir /tmp/shacs-spec033-data-v6 --mode current-worktree
```

Permission approval prompt는 한 번 승인/거절, session remembered 승인/거절, project remembered 승인/거절의 여섯 가지 선택을 지원합니다. Project remembered rule은 config data directory의 `permissions.json`에 현재 canonical workspace bucket별로 저장되며, `exec` arity prefix, workspace path exact/subtree, `web_fetch` origin, MCP tool name, exact action matcher 중 구현된 요약만 재사용합니다. 현재 rule은 CLI에서 read/revoke할 수 있고, slash/API/TUI projection도 같은 redacted rule summary를 소비합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- permissions list --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- permissions inspect <rule-id-prefix> --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- permissions revoke <rule-id-prefix> --workspace /tmp/shacs-ws
```

Malformed `permissions.json`은 raw content를 출력하지 않고 fail closed됩니다. Remembered allow는 protected target, static deny, permission ceiling, containment precondition을 우회하지 않으며, 이 기능은 완전한 sandboxing/redaction이나 prompt/tool/repo content 기반 permission grant를 보장하지 않습니다.

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
curl http://127.0.0.1:8900/v1/readiness
curl http://127.0.0.1:8900/v1/diagnostics
```

Spec 034 generated media와 video analyzer 상태는 기존 runtime/API/TUI/channel 표면에서 확인합니다. 별도 image editor CLI는 없습니다. `image_generate`의 Codex final output과 검증된 edit/mask output은 local media root에 commit된 뒤 relative artifact ref로 노출되며, provider별 미지원 edit/mask/variation은 `unsupported`가 될 수 있습니다. `runtime inspect`는 canonical media projection이 없을 때 성공을 합성하지 않고 unavailable로 표시합니다. API adapter에 projection이 configured된 경우 media diagnostics를 반환하고, 없으면 404입니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime inspect --workspace /tmp/shacs-ws
curl --fail-with-body http://127.0.0.1:8900/v1/media/diagnostics
```

Video analyzer는 runtime에 주입된 경우에만 bounded metadata/transcript/scene/keyframe evidence를 만들며, analyzer missing, unsupported codec, extraction failure, truncation과 timeout/cancellation을 성공으로 표시하지 않습니다. Built-in ffmpeg, full codec understanding, CDN/gallery/editor, all-provider parity, arbitrary URL intake, permanent remote reference, universal containment와 complete redaction은 제공하거나 보장하지 않습니다. Spec034의 정확한 22/22 mapping, [review remediation PASS](.omo/evidence/spec034/remediation/PASS.json), [Todo 14 QA baseline](.omo/evidence/spec034/task-14-final-qa-candidate2/PASS.json)은 구현과 회귀 baseline입니다. 최신 source-bound release 상태는 [canonical final manifest](.omo/evidence/spec034/task-15-closure/final-committed/manifest.json)가 현재 committed source와 최종 5개 리뷰를 결합해 판정하며, manifest가 없거나 검증에 실패하면 final seal이 아닙니다. 상세 범위와 판정 계약은 [`Spec 034 CLOSURE`](docs/specs/034-generated-media-and-rich-file-context-expansion/CLOSURE.md)를 참고하세요.

`spec034-release-runner` 성공 stdout은 runner가 반환한 committed publication digest인 lowercase `sha256:<64 hex>` 한 줄뿐이며 stderr는 비어 있습니다. 실패 시 stdout은 비어 있고 stderr는 `spec034 release runner failed: `로 시작하며 non-zero로 종료합니다.

검증된 Rust tool closure의 영속 캐시는 Cargo build target과 분리됩니다. Darwin 기본 위치는 `~/Library/Caches/shacs-bot/spec034-release`이고, 테스트나 격리 실행은 절대 경로 `SHACS_SPEC034_RELEASE_CACHE_ROOT`로 별도 사용자 소유 cache root를 지정할 수 있습니다. Cache root는 Cargo target 내부, symlink leaf, repository 소유자와 다른 디렉터리를 거부합니다. 캐시는 검증된 tool binary와 digest manifest만 보관하며 credential, environment secret, 실행별 source copy는 보관하지 않습니다.

이 digest의 structural audit는 artifact 구조와 expected manifest/run ID/digest 결합을 검사하지만 외부 실행 attestation 자체는 아닙니다. `success-fixture` 성공은 runner mechanics만 검증하며 Spec034 closure가 아니고, dirty current-worktree 실행은 provenance를 기록할 뿐 final closure가 아닙니다. Darwin APFS vnode ledger와 suspended-process CDHash 검증에서 감지된 실행 중 tool/runtime tamper는 fresh execution attestation과 publication을 fail closed합니다. Cleanup은 root와 nested pathname identity를 destructive unlink 직전에 다시 확인하며 감지된 교체나 event 불일치는 publication을 차단합니다. 다만 지속적으로 악의적인 same-UID process가 identity 확인과 unlink syscall 사이에 pathname을 다시 교체할 수 있으므로 unrelated replacement의 원자적 보존과 isolation root의 보장된 회수는 제공하지 않습니다. Public-API polling 사이의 double-fork/setsid 후 reparent 원자적 추적, 승인되지 않은 descendant의 zero-instruction 실행 방지, universal sandbox/process containment도 보장하지 않습니다.

Spec 035 implemented baseline에서 기존 QA 기록이 있는 표면은 TUI, `agent` REPL, secret-ref-only onboard wizard, readiness API/diagnostics, delivery hint projection, release runner artifact입니다. 이는 현재 Spec 035 closure PASS 또는 planned Tasks/reconnect parity 완료를 뜻하지 않습니다. TUI는 live runtime projection을 읽어 session, approval, degraded readiness, stop/restart/recover action을 표시합니다. Fresh workspace에서는 먼저 workspace template과 session store를 만들고, 표시할 session을 생성한 뒤 `--once` 또는 interactive TUI를 실행하세요:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- onboard --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- session create --session cli:direct --workspace /tmp/shacs-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-tui -- --workspace /tmp/shacs-ws --session cli:direct --once
cargo run --manifest-path crates/Cargo.toml -p shacs-tui -- --workspace /tmp/shacs-ws --session cli:direct
```

Message 없이 `agent`를 실행하면 같은 command router를 쓰는 REPL이 시작됩니다. 일반 입력은 session turn으로 처리되고 `/status`, `/stop`, `/restart`는 priority command 의미를 보존합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- agent --workspace /tmp/shacs-ws
```

Spec 031 release runner는 generated machine-readable `manifest.json`, `coverage-matrix.json`, `results.json`, `failure-triage.json`, `reproducibility-observations.json`과 human-readable `summary.md`를 씁니다. Current-worktree mode는 실제 Cargo exit status와 transcript만으로 PASS를 만들고 dirty 상태를 `observations/dirty-worktree.json`에 별도 기록하되 failure triage나 의미 verdict에 포함하지 않습니다. Specs 029/030/032/033/034/035는 031이 요구하는 exact adapter fact만 검사하며 source spec 전체 Complete 상태를 요구하지 않습니다. Missing/unknown fact나 실패 command는 계속 blocked입니다. `success-fixture`는 runner 자체의 passing fixture일 뿐 semantic Spec031 closure 증거가 아닙니다:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --run-id spec031-current --evidence-root /tmp/spec031-current --repo-root "$(git rev-parse --show-toplevel)" --mode current-worktree
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --run-id spec031-success-fixture --evidence-root /tmp/spec031-success-fixture --repo-root "$(git rev-parse --show-toplevel)" --mode success-fixture
```

Delivery와 readiness projection은 보수적인 hint입니다. Remote ACK/read receipt, replay 방지, exactly-once delivery를 보장하지 않습니다. SSE final delivery는 현재 pending 또는 unknown으로 남을 수 있고, 외부 owner fact가 없으면 성공으로 합성하지 않습니다. Approval durable request는 owner terminal event가 기록되기 전까지 Requested로 표시됩니다.

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

Compose는 host의 `~/.shacs-bot`을 container의 `/home/shacs/.shacs-bot`에 mount합니다. 기본 `docker compose up`은 단일 owner인 `shacs-gateway`만 시작하며, `shacs-api`는 명시적으로 서비스 이름을 지정할 때 시작되는 opt-in profile입니다. Provider secret은 image에 넣지 말고 `onboard` 후 생성된 config/auth workflow 또는 `.env.example`을 참고한 shell environment로 제공하세요. 기본 UID/GID는 nanobot과 같은 `1000:1000`이고, 위 예시처럼 `SHACS_UID`/`SHACS_GID`를 지정하면 host user 소유권에 맞춰 실행합니다. `bwrap`는 공식 image/package에 포함되어 자동 설정된 경우가 아니라면 optional hardening입니다. 로컬 OpenAI 호환 API만 띄우려면 gateway를 중지한 뒤 provider 설정 후 `docker compose up -d shacs-api`와 `curl http://127.0.0.1:8900/health`를 사용하세요.

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

Channel config는 원본 nanobot과 같은 `channels.<name>` 형태를 우선합니다. 예를 들어 `channels.sendMemoryHints`는 status에서 memory hint 설정으로 표시되고, `channels.sendMaxRetries`는 `ChannelManager` dispatch/enqueue와 실제 transport send의 총 시도 횟수로 적용됩니다. 값은 최소 1회, 최대 10회로 제한됩니다. WebSocket과 외부 transport outbound는 `ChannelManager` dispatch 정책을 통과하며, 외부 transport inbound/outbound는 runtime `MessageBus` 경계를 사용합니다. 외부 transport inbound는 remote ACK/cursor 갱신 전에 durable work queue에 저장되며 restart 후 다시 dispatch될 수 있습니다. Runtime은 Nanobot처럼 같은 session key의 turn을 process-local로 직렬화하고, durable queue에서 dispatch된 follow-up을 현재 turn 뒤에 이어서 처리합니다. Built-in slash command는 command router에서 priority/exact/prefix 경계로 분류되며, `/status`, `/stop`, `/restart` priority command만 active session turn 중에도 먼저 처리됩니다. 일반 turn과 exact/prefix command는 같은 process-local session turn lock을 공유합니다. `channels.sendProgress`가 enabled이면 WebSocket channel은 provider text deltas를 coalesce한 `delta` event와 `stream_end` event를 보낸 뒤 최종 `message` event를 유지합니다. Telegram/Discord/Slack external transport는 provider progress delta를 보내지 않고 최종 assistant answer만 새 message로 전송하며 기존 message를 edit/update하지 않습니다. WebSocket event delivery는 bounded queue로 socket writer에 넘겨져 느린 client에 대해 backpressure를 적용합니다. Telegram topic, Slack thread, Discord thread, Email subject/reply context는 outbound reply metadata로 이어집니다. Telegram offset, Discord REST last message id, Discord Gateway resume state, Email IMAP seen UID + UIDVALIDITY hint, outbound delivery pending/sent/failed/processed status는 runtime metadata JSON으로 best-effort 보존됩니다. 새 metadata는 typed restart envelope도 함께 기록하지만 기존 key와 호환됩니다. Inspect/status projection은 delivery를 `pending`, `sent_hint`, `failed_hint`, `unknown`, `dedupe_candidate`로 보수적으로 표시하며 raw content를 출력하지 않습니다. Channel metadata는 remote read/ACK, durable queue truth, replay 방지, exactly-once delivery 보장이 아닙니다. SSE final delivery는 pending 또는 unknown으로 남을 수 있습니다. Email은 `consentGranted: true`와 inbound `allowFrom`/`allowedSenders`가 있어야 IMAP polling을 시작하며, `smtpUsername`/`smtpPassword`, `imapUsername`/`imapPassword`, `fromAddress` alias를 함께 받습니다. IMAP polling은 현재 TLS만 지원하고, inbound Email은 기본적으로 `Authentication-Results`의 `spf=pass`/`dkim=pass`를 확인합니다.

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
