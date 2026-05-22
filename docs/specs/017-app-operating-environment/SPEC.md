# AI app operating environment 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`와 numbered spec set 전체를 바탕으로 `shacs-bot`의 장기 제품 도착점을 **개인용 AI Operating System**으로 고정한다.

목표는 다음과 같다.

- `shacs-bot`이 단순한 플러그인 호스트가 아니라 설치 가능한 AI app을 실행하는 개인용 런타임이라는 최종상을 정의한다.
- skill, tool, MCP, channel, secret, permission, runtime service, UI projection을 하나의 app bundle / process model로 묶는 상위 owner 경계를 만든다.
- macOS 유사성을 UI 모방이 아니라 app bundle, launch, permission, process, settings, ledger의 제품 의미론으로 번역한다.
- future Rust 구현에서 app manifest, app registry, app supervisor, permission grants, secret binding, task ledger 타입과 테스트를 도출할 수 있게 한다.

이 문서는 방향 메모가 아니라 상위 제품 계약이다. 하위 spec이 app, plugin, installable program, app store, extension lifecycle을 다룰 때는 이 문서의 용어와 경계를 소비해야 한다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `shacs-bot`은 self-hosted / personal-use assistant runtime이며, 기본 주체는 사용자 본인이다.
- `MainOrchestrator`는 세션 상태와 정책 확정을 담당하는 유일한 권한자다.
- skill은 read-only 지식 팩이고, tool/MCP/channel/runtime service는 오케스트레이터의 정책 아래에서만 효과를 낸다.
- host safety, permission, secret handling은 편의 기능이 아니라 턴마다 강제되는 공식 정책이다.

따라서 이 문서는 멀티테넌트 SaaS app platform, 조직 app catalog, 중앙 관리자 승인, fleet plugin rollout, remote marketplace 운영을 다루지 않는다.

이 문서의 핵심은 `shacs-bot`을 “플러그인을 꽂는 봇”이 아니라, 사용자가 자기 머신에서 설치한 AI app들을 안전하게 실행하고 관찰하고 복구하는 **개인용 AI 컴퓨터의 userland**로 보는 것이다.

---

## 현재 구현 상태

현재 구현은 local app manifest, registry, process projection, task ledger receipt baseline을 완료한 상태다. 이 완료 범위는 개인 사용자가 config data dir의 local `apps/<app-id>.shacsapp/` bundle을 등록하고 관찰하는 첫 기준이며, 전체 AI Operating System 완성을 뜻하지 않는다.

구현 evidence는 다음 repo-relative 경로에 남아 있다.

- `crates/shacs-core/src/app.rs`: manifest validation, registry store, lifecycle state, process snapshot, task ledger entry redaction과 persistence의 core 타입과 동작
- `crates/shacs-core/tests/app_environment.rs`: manifest, registry, process projection, ledger baseline 회귀 테스트
- `crates/shacs-cli/src/lib.rs`: `apps install/list/inspect/show/enable/disable/uninstall` command surface와 parser evidence

이 baseline에서도 app install은 실제 app process 실행, supervisor start, MCP server 내부 구현, secret vault backend 구현, permission grant 확정을 만들지 않는다. `AppProcessSnapshot`은 session truth owner가 아니라 기존 session/runtime 상태에서 읽는 projection이다.

비목표도 그대로 유지한다. remote marketplace, Rust dynamic plugin ABI, arbitrary in-process third-party code loading, SaaS/admin/fleet workflow, OS별 visual UI design은 구현 완료로 주장하지 않는다. 실행 가능한 evidence는 존재하지 않는 경로가 아니라 `cargo test`와 해당 Cargo command로 확인해야 한다.

---

## reference 채택 원칙

`docs/refs`는 구현 복제 대상이 아니라 제품 의미론을 뽑아내는 참고 집합이다.

| reference | 가져올 것 | 그대로 가져오지 않을 것 |
|---|---|---|
| `nanobot` | 작은 중심 루프, message bus를 통한 외부 자극 정규화, 채널/cron/heartbeat를 코어 밖 서비스로 두는 경계 | Python entry point plugin 구조, 채널 수 확장 경쟁, 설정 파일에 secret을 직접 섞는 관성 |
| `opencode` | session/project/worktree identity, permission request/reply를 공식 상태로 다루는 방식, MCP status와 tool registry의 상태화 | npm package plugin을 런타임에서 임의 import하는 구조, SaaS 공유/원격 control plane 성격 |
| `claude-code` | built-in plugin과 bundled skill의 구분, user enable/disable 상태, skill/MCP/hooks를 하나의 사용자 활성화 단위로 묶는 관점 | raw 소스 구조, marketplace plugin ID 체계, skill frontmatter의 모든 필드 |
| `oh-my-opencode` | 사용자는 intent를 주고 시스템은 계획, 위임, 검증, 복구를 끝까지 수행하는 낮은 인지 부하의 작업 경험 | 특정 agent 이름, 외부 서비스 운영 방식, 인간 개입을 무조건 실패로 보는 절대화 |

따라서 `shacs-bot`의 app은 “임의 코드를 꽂는 plugin”이 아니라, 사용자가 설치하고 권한을 부여한 capability bundle이다. app은 사용자 intent를 처리할 수 있는 제품 단위지만, 상태 확정 권한은 끝까지 `MainOrchestrator`에 남는다.

---

## 최종 제품 정의

최종 상태의 `shacs-bot`은 다음 경험을 제공해야 한다.

```text
사용자는 AI 작업을 수행하기 위해 저수준 MCP 서버, skill 파일, config fragment를 직접 조합하지 않는다.
사용자는 app을 설치하고, app에 권한과 secret을 부여하고, app이 생성한 process/task를 관찰하고, 결과물을 회수한다.
```

macOS에 비유할 수는 있지만 macOS UI를 복제하지 않는다. 비유의 목적은 다음 개념 경계를 고정하는 것이다.

| macOS 개념 | shacs-bot 최종 개념 |
|---|---|
| `.app` bundle | `.shacsapp` app bundle |
| `Info.plist` | app manifest |
| LaunchServices | app registry / intent routing |
| launchd | app supervisor / process supervisor |
| Keychain | secret vault |
| TCC permission | capability grants / approval policy |
| Finder | context finder |
| Spotlight | intent bar |
| Activity Monitor | process monitor |
| System Settings | runtime settings |
| Notification Center | approval center |
| Time Machine | task ledger / replayable receipts |

이 표의 오른쪽 이름은 독립 UI 제품명을 새로 만드는 것이 아니라 기존 owner spec의 projection 이름으로 소비해야 한다. `context finder`와 `intent bar`는 013의 interface surface와 query/command 경계, `process monitor`와 `approval center`는 `AppProcessProjection` 및 `ApprovalProjection`, `runtime settings`는 008의 config/profile projection, `task ledger`는 014의 inspect/diagnostics evidence와 연결된다.

---

## 핵심 정의

### app

app은 사용자가 설치, 활성화, 비활성화, 제거할 수 있는 capability bundle이다.

app은 단일 prompt나 단일 tool이 아니다. app은 다음 리소스를 하나의 제품 단위로 묶는다.

- manifest
- skills
- MCP server 또는 외부 process entry
- tool exposure metadata
- optional slash command / intent route
- optional channel surface
- env / secret schema
- permission declaration
- lifecycle policy
- test / diagnostic metadata

### app bundle

app bundle은 파일 시스템에 놓이는 설치 단위다. 현재 구현된 설치 표면은 config data dir의 `apps/<app-id>.shacsapp/`에 이미 놓인 local bundle을 registry에 등록한다.

`shacs-bot`은 구현체와 CLI/runtime 이름이고, config data dir의 기본 위치는 `~/.shacs-bot/`이다. 따라서 `.shacsapp`은 workspace 루트에 흩어지는 독립 디렉터리가 아니라 data dir의 `apps/` 아래에 등록되는 app bundle 확장자다.

bundle 내부 기본 형태는 아래를 따른다.

```text
<data-dir>/apps/<app-id>.shacsapp/
  manifest.json
  skills/
  devices/
    mcp/
  tools/
  commands/
  services/
  assets/
  README.md
```

초기 구현은 반드시 이 전체 트리를 요구할 필요는 없지만, manifest는 app bundle의 유일한 진실 원천이어야 한다.

### app manifest

app manifest는 app의 identity, entry, required resources, secret schema, permission request를 선언하는 문서다.

예시:

```json
{
  "id": "notion",
  "name": "Notion",
  "version": "0.1.0",
  "entry": {
    "kind": "mcp",
    "command": "npx",
    "args": ["-y", "@notionhq/notion-mcp-server"]
  },
  "skills": ["skills/SKILL.md"],
  "secrets": {
    "NOTION_TOKEN_KEY": {
      "required": true
    }
  },
  "permissions": {
    "network": ["api.notion.com"],
    "tools": ["mcp"],
    "filesystem": ["workspace:read"]
  }
}
```

### app registry

app registry는 설치된 app의 identity, enabled state, manifest digest, install path, grant reference, lifecycle state를 보관하는 인덱스다.

app registry는 app process의 실시간 실행 로그, session truth, permission grant의 최종 원천을 소유하지 않는다. grant의 의미와 평가는 010의 host safety layer가 소유하고, registry는 app별로 어떤 grant reference를 연결할 수 있는지 인덱싱한다. 실행 기록은 task ledger와 observability projection의 책임이다.

### intent route

intent route는 사용자의 자연어 요청, slash command, 외부 channel/message, scheduled wake가 어떤 app capability 후보로 매핑되는지 설명하는 선언형 경로다.

intent route는 추천 또는 라우팅 힌트일 뿐이다. route가 존재한다는 이유만으로 app process 생성, permission 승인, tool 실행이 자동 확정되면 안 된다.

### app process

app process는 app 또는 intent가 실행되어 생긴 작업 인스턴스다.

app process는 OS process와 같은 완전한 커널 process가 아니라, `shacs-bot`의 공식 실행 단위다. 최소한 다음을 가져야 한다.

- process id 또는 session id
- app id
- originating intent
- workspace / context scope
- active permission grants
- secret handles used
- tool/MCP calls in flight
- status: running, waiting approval, completed, failed, cancelled, restartable
- artifact references

### device

device는 app이 외부 능력을 제공하기 위해 붙이는 MCP server, local service, remote adapter 같은 실행 경계다.

MCP server는 AI Operating System에서 “드라이버”에 해당한다. app은 device를 통해 tool을 노출할 수 있지만, device가 permission을 직접 확정하거나 세션 상태를 바꾸면 안 된다.

device 상태는 최소한 `disabled`, `ready`, `starting`, `needs_auth`, `failed`를 구분할 수 있어야 한다. `needs_auth`와 `failed`는 app failure를 숨기는 것이 아니라 사용자가 해결할 수 있는 process 상태로 projection되어야 한다.

### port

port는 실제 effect가 흘러가는 tool 경계다. file read/write, shell exec, network fetch, MCP tool call, restart request는 모두 port로 추적되어야 한다.

### secret vault

secret vault는 app이 요청한 secret 이름과 실제 값을 연결하는 경계다.

app과 agent는 secret value를 소유하지 않는다. app은 secret handle 또는 env key 이름을 요구하고, runtime은 승인된 process 실행 환경에만 값을 주입한다.

### task ledger

task ledger는 app process가 수행한 중요한 단계, tool call, permission decision, secret use, artifact write, restart/recover event를 나중에 설명 가능한 receipt로 남기는 개념적 저장소다. 기본 파일 저장 루트 이름은 008의 `runtime/app-ledger/`다.

ledger는 raw secret value, provider raw hidden reasoning, 필요 이상의 file contents를 저장하면 안 된다.

task ledger는 대화 로그의 다른 이름이 아니다. 사용자가 나중에 “어떤 app이, 어떤 device와 port를 통해, 어떤 grant 아래에서, 어떤 artifact를 만들었는지”를 읽을 수 있게 하는 실행 영수증이다.

---

## 사용자 경험 계약

최종 제품에서 사용자는 아래 흐름을 기대할 수 있어야 한다.

```text
install app
-> manifest 검증
-> 요구 secret/permission 표시
-> 사용자 승인 또는 보류
-> registry 등록
-> skill/context 노출
-> device/MCP 준비
-> intent에서 사용 가능
```

예시 명령 표면:

```sh
shacs-bot apps install ~/.shacs-bot/apps/notion.shacsapp --workspace /tmp/ws
shacs-bot apps list
shacs-bot apps inspect notion
shacs-bot apps enable notion
shacs-bot apps disable notion
shacs-bot apps uninstall notion
```

현재 `install`은 data-dir-local `apps/<app-id>.shacsapp/` bundle을 검증한 뒤 registry에 등록하는 의미다. 다른 위치에서 bundle을 복사하거나 remote catalog에서 이름만으로 받아오는 동작은 초기 계약이 아니다.

CLI 명령은 공식 의미론의 한 projection일 뿐이다. TUI/local API는 같은 app registry, grant reference, process status, ledger를 표시해야 한다.

intent 실행 예시:

```text
사용자: 어제 회의 내용을 Notion에 정리해줘.

Intent routing:
  -> Notion app + summarization capability 선택
  -> 필요한 secret과 권한 확인
  -> app process 생성
  -> MCP device 준비
  -> tool call 실행 전 approval 필요 여부 평가
  -> 결과 artifact와 receipt 저장
```

사용자에게는 이 작업이 단순 채팅 로그가 아니라 process card 또는 task projection으로 보여야 한다.

```text
Task: 어제 회의 Notion 정리
Status: waiting for approval
Apps: Notion, Memory
Secrets: NOTION_TOKEN_KEY 사용
Tools: notion_search, notion_create_page
Artifacts: meeting-summary.md
Receipts: notion page created
```

---

## app lifecycle 계약

### install

install은 app bundle을 검증하고 registry에 등록하는 행위다.

install 단계에서 허용되는 일:

- manifest parse / validate
- app id, version, digest 계산
- static resource 위치 기록
- required secret / permission summary 표시
- optional bundled skill 발견

install 단계에서 금지되는 일:

- app process 자동 실행
- secret value를 manifest에 기록
- permission을 묵시적으로 영구 승인
- remote code를 임의 실행

### enable

enable은 app을 intent routing, skill discovery, device preparation 후보로 만드는 행위다.

enable은 실행이 아니다. enabled app은 아직 process를 갖지 않을 수 있다.

### open / start

open 또는 start는 app process를 만든다.

start 단계에서는 해당 process에 필요한 scope를 계산하고, approval policy를 평가하고, device/MCP process를 준비한다.

### disable

disable은 새 process 생성을 막고, 가능한 경우 관련 device를 정리한다.

진행 중 process는 안전하게 완료하거나 취소 가능한 상태로 전환해야 한다. disable이 세션 truth를 직접 삭제하면 안 된다.

### uninstall

uninstall은 app registry entry와 app bundle을 제거한다.

uninstall은 ledger, receipts, historical session references를 무조건 삭제하면 안 된다. 삭제 정책은 별도 explicit cleanup command로 분리한다.

---

## permission과 secret 계약

app 권한은 반드시 선언형이어야 한다.

나쁜 권한:

```text
allow network
allow filesystem
```

좋은 권한:

```text
network: api.notion.com
filesystem: workspace:read
secret: NOTION_TOKEN_KEY
tool: notion_create_page
duration: this-task
```

규칙:

1. app manifest의 permission declaration은 요청일 뿐 승인 결과가 아니다.
2. 최종 승인 여부는 host safety permission layer가 판단한다.
3. approval은 app-wide, task-local, one-shot으로 구분 가능해야 한다.
4. secret value는 app manifest, task ledger, 일반 log, provider prompt에 raw로 저장되면 안 된다.
5. secret은 process 실행 환경 또는 MCP device 환경에 필요한 순간에만 주입한다.
6. permission denial은 app failure가 아니라 사용자 통제 결과로 기록되어야 한다.
7. app-wide grant는 최소 권한이어야 하며, write/exec/network/secret 조합은 가능하면 task-local 또는 one-shot으로 좁혀야 한다.

---

## 기존 spec과의 owner 경계

이 문서는 모든 세부 동작을 직접 소유하지 않는다.

- skill 파일 탐색, precedence, 파싱, 주입 규칙은 `005-skill-system/`이 소유한다.
- tool registry와 tool execution envelope는 `004-tool-runtime/`이 소유한다.
- permission, secret, host safety는 `010-host-safety-permissions-and-secrets/`가 소유한다.
- app process를 깨우거나 외부 메시지를 재진입시키는 service는 `012-runtime-services/`가 소유한다.
- process card, approval center, app list, settings projection은 `013-user-interfaces-and-session-ux/`가 소유한다.
- install/update/recover/restart의 host process lifecycle은 `015-packaging-process-lifecycle-and-upgrades/`가 소유한다.
- diagnostics, receipts, task ledger projection은 `014-observability-diagnostics-and-inspection/`와 연결된다.

이 문서가 소유하는 것은 위 요소를 **설치 가능한 AI app operating environment**로 묶는 상위 개념과 lifecycle 계약이다.

---

## 초기 비목표

초기 AI app operating environment는 다음을 목표로 하지 않는다.

- Rust dynamic library plugin ABI
- arbitrary in-process third-party code loading
- 중앙 App Store 운영
- remote package를 install 과정에서 즉시 실행하는 plugin loader
- 조직 관리자 승인 workflow
- 멀티유저 app entitlement 시스템
- 원격 fleet 배포
- macOS UI의 시각적 복제

특히 Rust dynamic plugin은 기본 경로가 아니다. 초기 app ABI는 manifest + skill + MCP/process boundary여야 한다. 이는 crash isolation, 언어 독립성, self-hosted 운영 단순성을 우선하기 때문이다.

---

## 최종 불변식

1. app은 오케스트레이터 권한을 우회하지 않는다.
2. app install은 app 실행이 아니다.
3. app manifest는 permission 요청을 선언하지만 approval을 확정하지 않는다.
4. skill은 app에 포함될 수 있지만 실행 권한을 갖지 않는다.
5. MCP/device는 app capability를 제공하지만 session truth를 직접 수정하지 않는다.
6. secret value는 app bundle, manifest, 일반 log, ledger에 raw로 저장되지 않는다.
7. 진행 중 process는 adapter/app reload와 독립적으로 끝까지 일관된 runtime snapshot을 사용해야 한다.
8. app disable/uninstall은 historical ledger와 session references를 임의로 파괴하지 않는다.
9. UI는 app/process/permission state를 왜곡 없이 projection해야 하며, 별도 진실 원천이 되면 안 된다.
10. 사용자는 언제나 “어떤 app이, 어떤 권한으로, 어떤 tool과 secret을 사용해, 어떤 결과를 만들었는지”를 나중에 설명받을 수 있어야 한다.

---

## Rust 구현 체크포인트

초기 구현은 아래 타입과 모듈 경계를 직접 도출할 수 있어야 한다.

```text
AppManifest
AppId
AppBundlePath
AppRegistry
AppRegistryEntry
AppLifecycleState
AppEntryKind
AppPermissionRequest
AppSecretRequest
AppProcessId
AppProcessSnapshot
AppSupervisor
AppDeviceSpec
TaskLedgerEntry
```

검증 관점:

- manifest parse/validate 실패가 전체 runtime start를 깨뜨리지 않는지
- app id collision과 digest mismatch를 진단하는지
- install이 process를 자동 실행하지 않는지
- enable/disable이 skill discovery와 tool/MCP exposure에 반영되는지
- secret required app이 missing secret일 때 unavailable로 표시되는지
- permission denial이 session corruption 없이 receipt로 남는지
- app process가 active turn 중 reload되어도 기존 snapshot으로 완료되는지
- uninstall 후 historical ledger/session reference가 설명 가능하게 남는지

---

## 명시적 비범위

위 초기 비목표는 app operating environment가 다루지 않는 제품 방향을 요약한다. 구현 문서로 내려갈 때도 아래 항목은 이 문서의 owner 범위 밖이다.

- marketplace protocol 세부 설계
- OS별 GUI shell 또는 desktop shell 구현
- app별 구체 MCP server 내부 구현
- secret vault의 저장 backend 세부 구현
- 원격 조직 정책 배포와 fleet lifecycle

단, 이 비범위가 app manifest, registry, process, grant, ledger의 상위 의미론을 약화해서는 안 된다.

---

## 결론

`shacs-bot`의 장기 도착점은 저수준 tool과 MCP 서버를 사용자가 직접 조립하는 봇이 아니라, 사용자가 설치한 AI app을 안전하게 실행하고 관찰하고 복구하는 개인용 AI Operating System이다.

핵심은 app을 권한자로 키우는 것이 아니라, app bundle이라는 제품 단위 아래에서 skill, device, tool, service, secret, permission, ledger를 설명 가능하게 묶고 최종 상태 전이와 승인은 `MainOrchestrator`에 남기는 것이다.
