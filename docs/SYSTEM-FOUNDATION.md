# shacs-bot 시스템 기반 문서

## 문서 목적

이 문서는 `shacs-bot`의 시스템 기반과 상위 방향을 고정하기 위한 기준 문서다.

이 문서는 `docs/specs/` 이하 numbered `SPEC.md`와 `prds/*.md`가 따르는 최상위 기준 문서다.

이 프로젝트는 아직 코드보다 의사결정이 더 중요한 단계에 있다. 따라서 이 문서는 기능 목록을 나열하는 문서가 아니라, 앞으로 상세 설계 문서, crate 설계, PRD, ADR, CLI 사용성 문서, 스킬 시스템 문서가 어떤 시스템 기반과 상위 방향을 따라야 하는지 판단하는 기준점 역할을 한다.

이 문서의 목표는 다음과 같다.

- 프로젝트의 성격을 분명히 정의한다.
- 현재 시점에서 이미 합의된 설계 원칙을 고정한다.
- 레퍼런스 프로젝트에서 가져올 것과 가져오지 않을 것을 구분한다.
- 이후 세부 설계를 쪼갤 수 있도록 상위 구조를 제공한다.

---

## 프로젝트 한 줄 정의

`shacs-bot`은 **사용자 본인이 직접 설치하고 운영하는 Rust 기반 개인형 AI Operating System**이다.

이 프로젝트는 일반적인 채팅봇보다 **로컬 작업 수행**, **지속 세션**, **툴 사용**, **개인 워크스페이스 기반 동작**, **명시적인 오케스트레이션**에 더 가깝다. 방향성은 “거대한 멀티테넌트 SaaS 에이전트 플랫폼”이 아니라, **한 명의 사용자를 위해 장기간 안정적으로 동작하는 self-hosted assistant runtime**에서 출발해, 사용자가 설치한 AI app을 실행하고 관찰하고 복구하는 개인용 AI 컴퓨터의 userland로 수렴하는 것이다.

---

## 제품 관점

### 기본 전제

- 기본 주체는 조직 운영자나 관리자 팀이 아니라 **사용자 본인**이다.
- 사용자는 직접 설치, 설정, 업데이트, 실행, 복구를 할 수 있어야 한다.
- 운영 환경은 서버 클러스터가 아니라 **개인 개발 환경, 개인 서버, 홈랩, 단일 워크스테이션**을 우선 가정한다.

### 이름 규약

`shacs-bot`은 현재 구현체, CLI, runtime process 이름이다. 현재 사용자 데이터 루트도 구현체 이름에 맞춰 `.shacs-bot/`을 사용한다.

따라서 기본 사용자 데이터 루트는 `~/.shacs-bot/`이고, 설치 가능한 AI app bundle은 config data dir의 `apps/<app-id>.shacsapp/` 아래에 놓는다. 기본 config 기준으로는 `~/.shacs-bot/apps/<app-id>.shacsapp/`이다. `.shacsapp`은 독립 최상위 workspace 이름이 아니라 data dir의 `apps/` 아래에서 의미를 갖는 app bundle 디렉터리 확장자다.

### 지향점

- 사용자의 명시적 요청을 받아 실제 작업을 수행할 수 있어야 한다.
- 단발성 질답보다 **세션 기반 작업 흐름**을 잘 다뤄야 한다.
- 외부 툴과 모델을 연결하더라도 내부 상태 변경 규칙은 일관되어야 한다.
- 초기부터 확장성보다 **디버깅 가능성, 예측 가능성, 재현 가능성**을 우선한다.

### 일부러 피할 것

- 관리자 전용 운영 콘솔을 전제로 한 설계
- 지나치게 분산된 런타임 구조
- 초기 단계부터 과한 멀티에이전트 운영 복잡도 도입
- 사용자가 요청하지 않은 범용 플랫폼화

### 현재 제품 범위 요약

현재 범위에서 제품 표면과 외부 연동은 의도적으로 좁게 잡는다.

- provider/auth family는 **OpenAI-compatible**, **Anthropic auth**, **Codex auth(OpenAI auth style)** 세 종류만 지원 대상으로 본다.
- 외부 채널은 **Slack**, **Discord**, **Telegram**, **Email**, **WhatsApp bridge** 다섯 가지면 충분하다고 본다.
- 공식 인터페이스 표면은 CLI, TUI, local API를 기준으로 잡고, 외부 채널은 별도 mailbox/runtime service 경계에서 다룬다.

즉 초기 단계의 목표는 많은 벤더와 채널을 넓게 덮는 플랫폼이 아니라, 실제로 필요한 소수의 provider/auth 조합과 채널만 안정적으로 설명 가능하게 붙이는 것이다.

---

## 현재 설계 판단의 핵심

이 프로젝트는 **강한 메인 오케스트레이터가 모든 상태 전이를 관장하는 구조**로 간다.

이는 다음 판단에 기반한다.

- `nanobot` 계열의 장점: 작은 중심 루프, 읽기 쉬운 코어, 단순한 재진입 모델
- `opencode` 계열의 장점: 세션 중심 오케스트레이션, 프로젝트/워크트리 경계, 실행 전 사용자 interaction의 공식 상태화
- `claude-code` / `OpenHarness` / `openclaw` 계열의 장점: 주변 서비스 분리, 스케줄링/메일박스/태스크, skill/MCP/app bundle을 하나의 사용자 활성화 단위로 묶는 경계 설정
- `oh-my-opencode` 계열의 장점: 사용자는 intent를 주고 시스템은 계획, 위임, 검증, 복구를 끝까지 수행하는 낮은 인지 부하의 작업 경험

`shacs-bot`은 이 참고점들을 그대로 복제하지 않는다. 대신 다음처럼 조합한다.

- **코어는 `nanobot + opencode` 성격으로 가져간다.**
- **주변 서비스와 app bundle 경계는 `claude-code`, `OpenHarness`, `openclaw`에서 아이디어만 가져온다.**
- **검증 가능한 위임 경험은 `oh-my-opencode`에서 방향만 가져오되, 최종 상태와 dispatch ordering은 이 프로젝트의 오케스트레이터 계약으로 재해석한다.**
- **최종 상태 변경 권한은 메인 오케스트레이터에 남긴다.**

---

## 아키텍처 원칙

### 1. 메인 오케스트레이터 단일 권한 원칙

`MainOrchestrator`는 세션 상태를 변경할 수 있는 유일한 구성요소다.

바깥 시스템은 상태를 직접 수정하지 않는다. 대신 다음 중 하나만 수행한다.

- `Command`를 오케스트레이터에 전달
- 오케스트레이터가 방출한 `Event`를 소비
- 오케스트레이터가 요청한 `Effect`를 실행

이 원칙의 목적은 다음과 같다.

- 상태 전이 추적 가능성 확보
- race condition 감소
- 세션 재현성과 복구 용이성 확보
- “왜 이렇게 되었는지” 설명 가능한 구조 확보

### 2. 세션 커널 우선 원칙

시스템의 중심은 “한 세션에서 한 턴이 어떻게 실행되는가”다.

즉 코어는 다음 책임을 우선 가진다.

- 사용자 입력 수용
- 문맥 구성
- 모델 호출
- tool roundtrip
- 결과 반영
- retry / abort / compact / resume 판단

이것은 작은 OS 비유로 보면 **session kernel**에 가깝다.

### 3. 바깥 시스템은 서비스화하되 권한은 제한한다

다음 기능은 중요하지만 코어에 넣지 않는다.

- queue
- cron / scheduler
- mailbox / inbox
- hooks
- 외부 채널 연동
- background worker
- local API 진입점

이들은 독립 서브시스템으로 존재할 수 있지만, **정책 권한자**는 아니다.

### 4. 단순한 설치/운영 우선

이 프로젝트는 self-hosted/personal-use 도구이므로, 운영 경험은 다음을 지향한다.

- 로컬 설치가 단순해야 한다.
- 구성 파일 위치가 예측 가능해야 한다.
- 장애 시 상태 복구와 원인 파악이 쉬워야 한다.
- 설정/스킬/세션 데이터의 디렉터리 구조가 단순해야 한다.

### 5. Rust 우선, Cargo 우선

이 프로젝트의 공식 런타임과 검증 흐름은 Rust와 Cargo 기준으로만 정의한다.

- 빌드/실행/테스트/체크는 Cargo 기반으로 수행한다.
- Rust 바깥 도구는 보조 수단일 뿐, 핵심 런타임을 대체하지 않는다.
- 기본 선호는 단일 core crate 유지다. 현재 저장소는 구조 정리를 위해 workspace를 사용하지만, 실제 구현 중심은 `crates/shacs-core` 단일 core crate에 둔다.

---

## 시스템 개요

```text
Interfaces
  - CLI
  - TUI
  - Local HTTP API
  - Scheduler / Mailbox adapters
        |
        v
MainOrchestrator
  - session kernel
  - turn loop
  - trusted runtime / hook ordering
  - tool roundtrip
  - subagent spawn / rejoin
        |
        +--> SessionStore / EventLog / Checkpoint
        |
        +--> Effect Dispatcher
                 |
                 +--> LLM Provider
                 +--> Tool Runtime
                 +--> Queue / Scheduler / Mailbox workers
                 +--> External channel adapters
```

이 구조의 핵심은, 오케스트레이터가 직접 모든 I/O를 수행하는 것이 아니라 **판단과 상태 전이만 책임지고**, 실제 외부 실행은 effect 기반으로 바깥에 위임한다는 점이다.

---

## 코어에 둘 것과 밖으로 뺄 것

### 코어 책임

- `SessionState`의 진실 원천 유지
- 한 턴의 실행 흐름 관리
- provider/tool 호출 순서 결정
- trusted runtime profile과 hook dispatch ordering 결정
- retry / timeout / abort / compact 판단
- subagent 생성 및 결과 재진입 규칙
- event log append

### 코어 밖 책임

- 실제 LLM API 호출
- 실제 tool 실행
- cron 트리거
- mailbox 수신/송신
- 외부 채널 연결
- background queue 소비
- hook 실행기
- UI transport

### 경계 규칙

어떤 기능을 코어에 넣을지 애매할 때는 다음 질문으로 판단한다.

> 이 기능이 없어도 single-session correctness가 유지되는가?

- **유지된다** → 코어 밖으로 뺀다.
- **유지되지 않는다** → 코어에 남긴다.

---

## 스킬과 executable resource 방향

2026-08-07의 [`Trusted local agent runtime 전환 결정`](TRUST-MODEL-DECISION.md)에 따라 skill/resource model은 read-only Markdown baseline과 trusted executable resource를 함께 지원한다.

### 채택할 원칙

- Markdown skill은 필요할 때 로드되는 **on-demand context unit**이다.
- Python skill과 in-process extension은 활성화 뒤 현재 사용자 권한으로 실행되는 **trusted code**다.
- Builtin·명시 configured resource는 활성 후보이며, project-local auto-discovered executable resource는 trusted workspace에서만 활성화한다.
- Resource source, precedence, collision, parse/load 상태를 inspect할 수 있어야 한다.
- Resource hash와 discovery status는 identity/provenance evidence이며 authorization 또는 sandbox proof가 아니다.

### 초기 디렉터리 규약

```text
~/.shacs-bot/skills/<skill-name>/SKILL.md
<workspace>/.shacs-bot/skills/<skill-name>/SKILL.md
<data-dir>/apps/<app-id>.shacsapp/skills/<skill-name>/SKILL.md
bundled-skills/<skill-name>/SKILL.md
```

### Resource 로딩 우선순위

```text
explicit > project-configured > trusted-project-auto > user-configured > user-auto > package > builtin
```

### 범위

- Markdown skill 목록·본문·context injection
- Python skill package install/import
- TypeScript/JavaScript extension import와 host API
- Package·project·user·builtin source precedence와 diagnostics
- Trusted-code disclosure와 inspect/disable/revoke lifecycle

원격 marketplace, 조직 governance, signed public distribution은 기본 제품 범위가 아니다.

---

## 서브에이전트 방향

서브에이전트는 별도 권한자가 아니다.

서브에이전트는 메인 오케스트레이터가 만든 **작업 단위**이며, 결과는 다시 메인 세션으로 재진입해야 한다. 즉 “독립된 지배자”가 아니라 “오케스트레이터가 통제하는 하위 실행자”로 취급한다.

초기 설계 원칙은 다음과 같다.

- 서브에이전트는 상태를 직접 commit하지 않는다.
- 서브에이전트의 결과는 `Event` 또는 synthetic `Command`로 재주입된다.
- 병렬성은 허용하되, 최종 병합 판단은 오케스트레이터가 한다.

---

## Trusted runtime과 operational control 방향

Shacs는 사용자가 직접 설치·운영하는 trusted local agent runtime을 기본으로 한다.

### 기본 원칙

- Model-generated Python, shell, project command는 기본적으로 현재 사용자 OS 권한으로 실행될 수 있다.
- 기존 `tool:before` hook은 tool 실행 직전 block-only veto를 제공한다.
- User confirmation은 현재 호출에만 적용하며 durable approval이나 permission grant로 표현하지 않는다.
- Bash, exec, package, kernel, daemon, MCP는 경로별 timeout·abort·cleanup을 제공하며 universal process gate를 주장하지 않는다.
- Daemon, worker, kernel, child session은 lifecycle isolation이며 security sandbox가 아니다.
- Sandbox는 adapter별 선택 기능이고 active/disabled/unsupported/failed 상태를 표시한다.
- Credential source와 raw session/log/trace 가능성을 사용자에게 공개한다.

### Runtime profile

- `trusted_native_fallback`: sandbox가 unavailable이면 경고 뒤 native 실행 가능.
- `sandbox_required`: sandbox가 active가 아니면 해당 adapter 실행 거부.

기존 permission mode, approval, redaction, containment primitive는 호환성 또는 닫힌 owner baseline으로 남을 수 있지만 이 foundation의 future 통합 모델은 아니다.

---

## 상태 저장과 복구 방향

이 프로젝트는 장기 세션과 재개 가능성을 중요하게 본다.

따라서 다음은 초기에 고려해야 한다.

- event log
- session checkpoint
- resume 가능한 session identity
- compact 이후에도 유지되는 핵심 작업 상태

단, 초기에 완전한 분산 복구 시스템을 만들 필요는 없다. 우선순위는 **로컬 단일 사용자 환경에서 신뢰할 수 있는 재개**다.

---

## 모듈 및 crate 방향 초안

초기에는 단일 crate가 기본이다. 다만 설계상 책임은 미리 분리해서 생각한다.

### 논리 모듈 초안

- `core`
  - `Command`, `Event`, `Effect`, `SessionState`, `TurnState`, trusted-runtime decision state, `MainOrchestrator`
- `session_store`
  - event log, checkpoint, resume metadata
- `provider`
  - LLM provider abstraction
- `tool_runtime`
  - tool registry, execution boundary, pre-tool hook and operational-control bridge
- `skills`
  - skill loader, registry, parser
- `runtime_services`
  - queue, scheduler, mailbox, background worker adapters
- `interfaces`
  - CLI, TUI, API transport

### 파일/모듈 분리 원칙

- 오케스트레이터는 정책 엔진이어야지 만능 I/O 객체가 되어서는 안 된다.
- I/O 구현은 trait 뒤로 밀어낸다.
- 단순한 책임 분리만 먼저 하고, 추상화는 실제 중복이 생긴 뒤에만 도입한다.

---

## 단계별 구현 계획

### Phase 0. 방향성 고정

- 이 문서를 기준 문서로 채택
- 이후 세부 문서는 이 문서를 상위 기준으로 참조

### Phase 1. 최소 세션 커널

- `MainOrchestrator`
- `SessionState`
- 기본 event log
- 한 턴의 입력 → 모델 호출 → 응답 완료 루프

### Phase 2. tool roundtrip

- tool registry
- 기본 read/write/shell/search 계열 툴
- pre-tool hook, 필요한 ephemeral confirmation, adapter별 process control
- tool result 재주입

### Phase 3. 세션 지속성

- resume
- compact를 고려한 상태 모델
- checkpoint

### Phase 4. skill 시스템

- `SKILL.md` 기반 로더
- bundled/user/workspace skill registry
- skill 조회 CLI 및 runtime injection

### Phase 5. 주변 서비스

- queue
- scheduler
- mailbox
- hooks

### Phase 6. 서브에이전트

- child task spawn
- synthetic command 재진입
- 제한된 병렬 처리

### Phase 7. 인터페이스 확장

- TUI
- local API
- 외부 채널 연동

이 순서는 “가장 먼저 시스템의 진실 원천을 안정화하고, 그 뒤에 주변 기능을 얹는다”는 의도다.

---

## 의도적으로 아직 결정하지 않은 것

다음 항목은 지금 단계에서 확정하지 않는다.

- 멀티유저 지원 여부
- 원격 협업 중심 기능
- 완전한 plugin marketplace
- 복잡한 분산 queue/worker topology
- production-grade daemon supervisor 구조
- 고급 멀티에이전트 팀 운영 UX
- 웹 우선 제품으로 갈지 CLI/TUI 우선으로 갈지의 최종 비중

이 항목들은 필요가 생기면 별도 문서에서 결정한다. 초기 방향 문서에 섣불리 포함하지 않는다.

---

## 레퍼런스 채택 원칙

`docs/refs`는 영감의 출처이지, `shacs-bot`의 정답이 아니다.

### 채택할 것

- 작은 중심 루프의 단순함
- 세션 오케스트레이션 응집
- tool roundtrip 패턴
- Markdown 기반 skill 로딩
- queue/scheduler/mailbox의 외부 서비스화
- 외부 자극을 command로 정규화하는 서비스 사고방식

### 채택하지 않을 것

- 초기부터 과한 분산 런타임
- 과도한 멀티에이전트 계층
- 구현보다 앞서는 복잡한 확장 포인트
- 관리자 조직 전제를 가진 운영 모델

---

## 세부 문서로 이어질 후속 주제

이 문서 다음으로 이어지는 후속 spec 세트는 아래 순서를 기준으로 본다.

1. `docs/specs/001-session-kernel/SPEC.md`
2. `docs/specs/002-command-event-effect/SPEC.md`
3. `docs/specs/003-provider-runtime/SPEC.md`
4. `docs/specs/004-tool-runtime/SPEC.md`
5. `docs/specs/005-skill-system/SPEC.md`
6. `docs/specs/006-session-store/SPEC.md`
7. `docs/specs/007-main-orchestrator-policy/SPEC.md`
8. `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
9. `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
10. `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
11. `docs/specs/011-subagent-runtime/SPEC.md`
12. `docs/specs/012-runtime-services/SPEC.md`
13. `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
14. `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
15. `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
16. `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`
17. `docs/specs/017-app-operating-environment/SPEC.md`
18. `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
19. `docs/specs/019-image-generation-and-generated-media/SPEC.md`
20. `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`
21. `docs/specs/021-app-maker-and-app-authoring/SPEC.md`
22. `docs/specs/022-auto-approval-permissions/SPEC.md`
23. `docs/specs/023-zero-setup-sandbox-execution/SPEC.md`
24. `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`
25. `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`
26. `docs/specs/026-context-files-and-inline-references/SPEC.md`
27. `docs/specs/027-channel-attachment-intake-and-file-context/SPEC.md`
28. `docs/specs/028-formal-execution-reentry-and-outcome-contracts/SPEC.md`
29. `docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md`
30. `docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md`
31. `docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md`
32. `docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md`
33. `docs/specs/033-evaluation-automation-live-integration/SPEC.md`
34. `docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md`
35. `docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md`

`001`부터 `027`은 2026-07-20 기준 실제 구현 범위로 완료 처리되었다. 각 문서에서 분리된 accepted open work는 `028`부터 `035`가 소유하며, 명시적 비목표와 미결정 아이디어는 자동으로 backlog에 포함하지 않는다.

`docs/product/cli-experience.md` 같은 제품 문서는 인터페이스 메모와 사용성 설명에 중요하지만, 위 architecture spec 세트를 대체하지 않는다.

각 문서는 반드시 이 문서의 방향성과 충돌하지 않아야 한다. 2026-08-07 trust model 변경은 [`TRUST-MODEL-DECISION.md`](TRUST-MODEL-DECISION.md)에 기록했으며, 이후 충돌은 이 결정과 현재 foundation을 기준으로 정리한다.

---

## 문서 사용 규칙

- 이 문서는 현재 기준의 **시스템 foundation 문서**다.
- 구현 세부사항이 필요하면 하위 문서로 분리한다.
- 확정된 사실과 제안된 구조를 혼동하지 않는다.
- 각 spec의 완료 기준은 POC나 프로토타입 제출이 아니라, 해당 spec 범위를 충족하는 **완전한 기능 구현과 필요한 검증의 완료**다.
- 각 spec은 필요 시 자신의 하위 실행 문서로 `docs/specs/NNN-spec-name/prds/NNN-prd-name.md` 구조를 가진 **spec-local PRD**를 둘 수 있다.
- `SPEC.md`는 규범 계약 문서이고, `prds/*.md`는 dependency cut, implementation wave, TDD 순서, exit criteria를 정의하는 실행 문서다. PRD는 SPEC를 대체하지 않는다.
- PRD 번호는 부모 spec 폴더 안에서만 유일하면 되며, zero-padded numbering을 사용한다. 기본 간격은 `000`, `010`, `020`처럼 두어 나중 삽입 여지를 남긴다.
- 구현은 spec 하나만 고립적으로 끝나는 것이 아니라 여러 spec 계약을 함께 소비할 수 있으므로, cross-spec 의존성과 wave는 상위 roadmap이 아니라 각 spec 하위 PRD에서 명시적으로 드러내야 한다.
- 구현 중 이 문서와 다른 방향이 더 낫다고 판단되면, 코드부터 바꾸지 말고 먼저 문서상 의사결정을 갱신한다.

---

## 현재 결론

`shacs-bot`은 다음 성격의 프로젝트로 진행한다.

- Rust 기반
- self-hosted / personal-use 중심
- 메인 오케스트레이터가 상태 전이를 단일 통제
- session kernel 중심 구조
- queue/scheduler/mailbox/hooks는 주변 서비스
- Markdown skill과 trusted executable resource 기반 확장
- 작은 코어를 먼저 안정화하고, 그 위에 기능을 단계적으로 얹는 방식

이 방향을 기준으로 이후 상세 설계를 진행한다.
