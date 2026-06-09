# verification matrix and release gates 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`와 numbered spec set 전체를 바탕으로 `shacs-bot`의 검증 전략, 테스트 매트릭스, release gate, 완료 기준을 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- 각 spec 범주가 어떤 테스트 계층으로 검증되어야 하는지 매핑한다.
- release blocker와 non-blocker를 구분한다.
- "동작하는 데모"와 "완전한 구현"을 구분하는 객관 기준을 정한다.
- self-hosted assistant runtime으로서 shipping 가능한 상태의 객관적 gate를 고정한다.
- future Rust 구현에서 test layout, verification jobs, release checklist, 실패 triage 규칙을 직접 도출할 수 있게 한다.

이 문서는 막연한 품질 선언문이 아니다. 구현이 이 문서와 충돌하면 테스트가 좀 있으니 됐다는 식의 POC 기준으로 마감하지 말고, 어떤 계약이 증명되었고 어떤 것은 아직 release blocker인지부터 다시 점검해야 한다.

이 spec의 완료 기준은 CI 하나가 녹색인 상태가 아니라, 이 문서가 정의한 verification family, spec coverage matrix, release blocker, completion gate를 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- 모든 상태 전이는 강한 `MainOrchestrator` 권한 모델 아래에서 설명 가능해야 한다.
- 각 spec은 POC가 아니라 완전한 기능 구현과 필요한 검증 완료를 완료 기준으로 가진다.
- 목표는 self-hosted / personal-use 환경에서 실제로 설치, 사용, 복구 가능한 assistant runtime이다.
- release 판단은 데모 효과나 일부 happy path가 아니라, spec contract 준수 여부로 내려야 한다.

따라서 이 문서는 투자자 데모, mock-only UI walkthrough, flaky showcase script, 수동 "한 번 해보니 됨" 수준의 승인 기준을 채택하지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- verification family 분류
- spec별 검증 매트릭스
- 필수 release gate
- blocker와 waiver 규칙
- demo behavior와 full implementation의 구분 기준
- 구현 불변식, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 특정 CI 서비스 선택
- 브랜치 전략
- 사람 조직의 승인 라인
- 마케팅용 출시 계획
- 외부 벤더 인증 절차

---

## 핵심 정의

### verification family

verification family는 같은 목적을 가진 검증 종류 집합이다. 예:

- 타입/정적 검증
- 단위 테스트
- 통합 테스트
- 복구 및 내구성 테스트
- UX contract 테스트
- 패키징/업그레이드 테스트

### release gate

release gate는 특정 버전을 shipping 가능하다고 선언하기 전에 반드시 통과해야 하는 객관 기준이다.

### blocker

blocker는 release를 막아야 하는 실패 또는 미충족 상태다. blocker는 단순 경고가 아니다.

### waiver

waiver는 특정 미충족 항목을 일시적으로 허용하는 예외 기록이다. `shacs-bot`의 기본 원칙은 blocker를 waiver로 습관적으로 덮지 않는 것이다.

### full implementation

full implementation은 happy path 데모가 아니라, spec이 정의한 경계, 실패 처리, recovery, observability, verification까지 구현되고 객관적으로 증명된 상태다.

### demo behavior

demo behavior는 제한된 경로에서만 동작하거나, 실패/복구/경계 조건이 검증되지 않았거나, 수동 개입 없이는 반복 가능하지 않은 상태다.

---

## 검증의 기본 원칙

1. 각 spec은 해당 범위의 정상 경로와 실패 경로가 모두 검증되어야 한다.
2. event truth, recovery semantics, approval boundary, redaction, upgrade safety 같은 교차 절단 관심사는 별도 검증 계층으로 다뤄야 한다.
3. 테스트 수가 아니라 계약 커버리지가 중요하다.
4. flaky 검증은 통과로 간주하면 안 된다.
5. 수동 확인만으로 release gate를 대체하면 안 된다.
6. "나중에 고칠 예정"은 blocker 해소가 아니다.
7. shipping release readiness와 full-spec readiness는 별도 판정이다. minimum-slice release gate 통과는 구현된 제품 범위의 release candidate 증거이며, 모든 spec의 full implementation 완료를 뜻하지 않는다.

---

## verification family

### 1. 정적 검증

목적:

- 타입 경계와 컴파일 가능성 보장
- 명백한 경고와 dead contract 조기 발견

초기 Rust 기준:

- 현재 저장소의 crate manifest 기준 `cargo fmt --manifest-path crates/shacs-cli/Cargo.toml -- --check`
- 현재 저장소의 crate manifest 기준 `cargo clippy --manifest-path crates/shacs-cli/Cargo.toml --all-targets -- -D warnings`
- `cargo test`와 별개인 빌드/타입 단계는 `cargo --manifest-path crates/shacs-core/Cargo.toml ...`처럼 실제 crate manifest를 명시 실행

### 2. 단위 테스트

목적:

- 상태 전이 함수, policy evaluator, redaction, parser, projection builder 같은 순수 또는 준순수 로직 검증

### 3. 통합 테스트

목적:

- command, event, effect, store, interface adapter가 함께 동작할 때 계약 준수 확인

### 4. 내구성 및 복구 테스트

목적:

- crash, late result, replay, checkpoint, interrupted upgrade, stale marker 상황 검증

### 5. UX contract 테스트

목적:

- CLI, TUI, local API가 같은 의미의 projection과 command contract를 따르는지 확인

### 6. 패키징 및 업그레이드 테스트

목적:

- 설치, start/stop, compatibility, migration, interrupted upgrade recovery 검증
- 공식 Docker/Compose containment evidence는 opt-in smoke command `./docs/scripts/spec023-compose-smoke.sh`로 검증

### 7. 안전성 및 redaction 테스트

목적:

- secret, permission, approval, path boundary, diagnostics redaction 검증

### 8. release candidate smoke test

목적:

- 실제 릴리스 후보 산출물이 self-hosted 사용자 기준 최소 사용 흐름을 반복 가능하게 수행하는지 확인

---

## spec별 verification matrix

현재 저장소의 Rust 검증 명령은 실제 crate manifest를 가리키는 `cargo --manifest-path crates/shacs-core/Cargo.toml ...` 형태로 명시 실행할 수 있어야 한다. 루트 `Cargo.toml` workspace가 추가되기 전까지 루트 workspace 범위 명령은 release evidence locator로 쓰지 않는다.

### 001. session kernel

- 단위 테스트: phase 전이, open turn invariant
- 통합 테스트: 입력부터 completed/aborted까지 흐름
- 내구성 테스트: late result, resume after crash

### 002. command, event, effect

- 단위 테스트: envelope validation, correlation parsing
- 통합 테스트: command to event/effect chain
- 정적 검증: 타입 분리와 enum completeness

### 003. provider runtime

- 통합 테스트: provider success, timeout, malformed result
- 안전성 테스트: snapshot boundary, raw payload 제한
- 내구성 테스트: retry와 stale provider result

### 004. tool runtime

- 통합 테스트: tool dispatch, correlation, failure path
- 안전성 테스트: capability boundary, path handling
- 내구성 테스트: tool timeout, late tool result

### 005. skill system

- 단위 테스트: discovery, precedence, parser
- 통합 테스트: on-demand injection
- 안전성 테스트: skill이 권한을 직접 확장하지 않음

### 006. session store

- 단위 테스트: append, checkpoint metadata
- 통합 테스트: replay, deterministic resume
- 내구성 테스트: crash after partial progress, checkpoint fallback

### 007. main orchestrator policy

- 단위 테스트: approval, retry, abort, late result decision table
- 통합 테스트: policy snapshot propagation
- 안전성 테스트: 바깥 실행기가 policy owner가 되지 않음

### 008. configuration, profiles, runtime layout

- 단위 테스트: discovery, precedence, malformed config handling
- 통합 테스트: bootstrap config snapshot
- 패키징 테스트: runtime root layout consistency

### 009. context assembly and compaction input

- 단위 테스트: context selection, compaction input filtering
- 통합 테스트: long-session compaction path
- 안전성 테스트: secret과 미확정 값 제외

### 010. host safety, permissions, and secrets

- 단위 테스트: redaction, MCP default-deny registration, config default-deny
- 통합 테스트: guard-denied execution, boundary enforcement
- 안전성 테스트: secret leakage prevention, oversized tool result redaction evidence, default-deny MCP tools/resources/prompts evidence

### 011. subagent runtime

- 단위 테스트: child lifecycle, stale result classification
- 통합 테스트: spawn, merge, cancel, timeout
- 내구성 테스트: parent turn closed before child result

### 012. runtime services

- 단위 테스트: dedupe key, wake envelope validation
- 통합 테스트: scheduler/mailbox/worker reentry
- 내구성 테스트: duplicate delivery, stale wake

### 013. user interfaces and session UX

- UX contract 테스트: CLI, TUI, local API 의미 일치
- 통합 테스트: create/resume/cancel/recover flow
- 안전성 테스트: inspect redaction, approval correlation

### 014. observability, diagnostics, inspection

- 단위 테스트: diagnostics classification, redaction pass
- 통합 테스트: inspect projection, bundle generation
- 내구성 테스트: crash evidence, recovery evidence, late result observability

### 015. packaging, process lifecycle, and upgrades

- 패키징 테스트: install/start/stop/update 경로
- 내구성 테스트: interrupted upgrade, stale ownership
- 통합 테스트: compatibility 검사와 migration gate

### 016. verification matrix and release gates

- 단위 테스트: blocker/waiver decision table, release readiness classification, demo-vs-full implementation classification
- 통합 테스트: release gate runner, evidence locator mapping, spec coverage reporting
- 메타 테스트: missing spec coverage detection, flaky/manual-only gate rejection

### 017. app operating environment

- 단위 테스트: manifest validation, app id collision, lifecycle state, permission/secret declaration parsing
- 통합 테스트: app registry install/list/inspect flow, process projection, task ledger receipt persistence
- 안전성 테스트: install does not execute app, secret value redaction, permission grant boundary preservation
- 내구성 테스트: reload during active process, uninstall with historical ledger/session references preserved

### 018. evaluation, automation, and self-improvement

- 단위 테스트: verdict mapping, approval correlation, stale/expired rejection, recursion guard, outcome classification
- 통합 테스트: goal lifecycle, scheduled job wake, subagent/app task outcome, provider fallback, UI projection
- 복구 테스트: checkpoint create/restore, failed rollback diagnostics, replay without destructive tool execution
- 안전성 테스트: trajectory/diagnostics/ledger redaction, silent self-improvement mutation rejection

### 019. image generation and generated media

- 단위 테스트: provider capability resolution, OpenAI request/response parsing, option validation, artifact metadata creation
- 통합 테스트: `image_generate` registration gate, generated media write, provider failure and media write failure paths
- 안전성 테스트: side-effect gate, auth absence before provider call, media subtree boundary, raw base64/prompt diagnostics redaction
- 회귀 테스트: Codex/provider expansion fixtures, partial-only stream failure, expiring URL persistence policy

### 020. tool search and provider tool surface

- 단위 테스트: config mode, threshold activation, visible/deferred split, catalog ranking, bridge argument parsing
- 통합 테스트: runner provider request assembly, bridge describe/call roundtrip, underlying tool execution mapping
- 안전성 테스트: core tools never defer, MCP default-deny preservation, subagent out-of-scope denial, bridge recursion rejection
- 관측 테스트: activation summary, deferred count, bridge-to-underlying tool event mapping, replay/ledger mapping evidence

---

## 공통 release gate

릴리스 후보는 최소한 아래 gate를 모두 통과해야 한다.

### Gate 1. 정적 검증 통과

- 포맷 체크 통과
- clippy 경고 0
- 빌드 가능

### Gate 2. 핵심 계약 테스트 통과

- session kernel, policy, session store, tool/provider integration 핵심 테스트 통과

### Gate 3. recovery 및 durability 테스트 통과

- crash/replay/recover 시나리오 통과
- late result가 truth를 뒤집지 않음

### Gate 4. safety 및 redaction 테스트 통과

- permission boundary 통과
- secret leakage 없음
- diagnostics/inspect redaction 통과
- oversized tool result persistence redaction 통과
- MCP capability registration이 명시적 enable 전까지 default-deny임

### Gate 5. interface contract 테스트 통과

- CLI, TUI, local API가 같은 command/projection 의미 공유
- approval, cancel, recover UX contract 통과

### Gate 6. packaging 및 upgrade 테스트 통과

- Cargo 기반 install-equivalent 산출물에서 start/stop/update/recover 경로 통과
- interrupted upgrade 방어 확인

### Gate 7. release candidate smoke test 통과

- fresh workspace 기준 release candidate 실행
- 세션 생성
- 기본 입력 처리
- guard-denied 작업 surface
- inspect
- recover, 필요 시 시뮬레이션

---

## blocker 규칙

다음은 기본적으로 release blocker다.

- 세션 truth 손상 가능성
- deterministic resume 실패
- approval boundary 우회 가능성
- secret 또는 민감 정보 유출
- interrupted upgrade 후 손상 상태에서 mutation 가능
- crash 후 recovery evidence 부재
- transport 간 의미 불일치
- flaky하거나 비결정적인 핵심 테스트
- 문서에 명시된 완료 기준 미충족

### blocker가 아닌 것의 예시

- cosmetics 수준의 출력 정렬 문제, 단 의미 왜곡이 없는 경우
- non-default experimental surface의 미세 UX 다듬기
- 추후 확장 기능 비범위 항목

---

## waiver 규칙

waiver는 예외적이어야 한다.

### waiver 최소 요건

- 어떤 spec 계약을 아직 못 지켰는지 명시
- 왜 blocker를 즉시 해결하지 못하는지 명시
- 사용자가 어떤 위험을 감수하는지 명시
- 임시 완화책과 제거 기한 명시

### waiver 금지 대상

- secret leakage
- session truth corruption 가능성
- deterministic resume 실패
- approval 우회
- interrupted upgrade safety 붕괴

위 항목은 waiver로 shipping하면 안 된다.

---

## full implementation과 demo behavior의 구분 기준

### full implementation으로 보려면 최소한 아래가 필요하다

1. 정상 경로와 실패 경로가 모두 자동 검증된다.
2. recovery와 interrupted state 처리 규칙이 구현되어 있다.
3. observability와 inspect로 원인 파악이 가능하다.
4. interface surface가 같은 의미를 공유한다.
5. 패키징과 업그레이드 경로가 검증되어 있다.
6. release gate가 반복 가능하게 통과된다.

### demo behavior의 전형적 징후

- happy path만 통과하고 cancel, timeout, crash, late result가 미검증
- 수동 로그 확인으로만 성공 판정
- install/update/recover 경로 미구현 또는 미검증
- TUI는 되는데 CLI/API contract가 다름
- secret redaction이 best-effort 수준
- interrupted upgrade를 고려하지 않음

위 징후가 남아 있으면 release 준비 완료가 아니다.

---

## 결정표

### 1. release readiness 결정표

| 조건 | 결정 | 비고 |
| --- | --- | --- |
| 모든 gate 통과, blocker 0 | release 가능 | 정상 릴리스 |
| gate 일부 실패, blocker 존재 | release 불가 | 수정 우선 |
| gate 통과했으나 non-blocker known issue 존재 | release 가능 | 명시적 기록 |
| waiver만으로 blocker를 덮으려 함 | release 불가 | waiver 금지 대상 검토 |

### 2. demo vs full implementation 결정표

| 상태 | 해석 | 결과 |
| --- | --- | --- |
| happy path만 수동 확인 | demo behavior | 미완료 |
| 실패/복구 포함 자동 검증 | full implementation 후보 | 추가 gate 확인 |
| 업그레이드/복구 미검증 | demo behavior | release 차단 |
| safety/redaction 미검증 | demo behavior | release 차단 |

---

## 구현 불변식

1. 각 spec은 적어도 하나 이상의 자동 검증 경로를 가져야 한다.
2. 핵심 truth, safety, recovery 계약은 수동 확인만으로 통과 처리하면 안 된다.
3. release gate는 반복 가능해야 한다.
4. flaky test는 통과로 간주하면 안 된다.
5. interface contract 불일치는 blocker다.
6. upgrade/recover 미검증 상태는 full implementation이 아니다.
7. secret leakage 가능성은 blocker다.
8. deterministic resume 실패는 blocker다.
9. 문서상 비범위를 근거로 필수 실패 경로 검증을 생략하면 안 된다.
10. 데모 성공은 release readiness의 충분조건이 아니다.

---

## 금지 패턴

### 1. 테스트 수치만으로 완료 선언

왜 금지인가:

- 어떤 계약이 빠졌는지 숨기게 된다.

### 2. 수동 시연으로 release gate 대체

왜 금지인가:

- 반복 가능성과 회귀 방지가 없다.

### 3. blocker를 known issue 문구로 축소

왜 금지인가:

- 실제 사용자 데이터와 복구 가능성에 영향을 준다.

### 4. transport 하나만 검증하고 전체 UX 완료로 간주

왜 금지인가:

- 013의 interface contract를 깨뜨린다.

### 5. interrupted upgrade와 recover를 나중으로 미룸

왜 금지인가:

- self-hosted 수명주기에서 핵심 실패 경로를 비워 둔다.

---

## Rust 구현으로 이어질 체크포인트

구체 테스트 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- spec별로 어떤 verification family가 적용되는지 코드 저장소에서 추적 가능한가?
- release gate를 자동 실행 가능한 명령 집합으로 표현할 수 있는가?
- crash/recover, approval/redaction, upgrade/compatibility 같은 교차 절단 시나리오에 대한 테스트 harness가 있는가?
- interface contract 테스트가 transport 의미 일치를 검증하는가?
- blocker와 waiver를 구조화된 release checklist로 기록할 수 있는가?
- release candidate smoke test가 설치 산출물 기준으로 반복 실행 가능한가?

---

## 테스트 관점에서 꼭 검증할 시나리오

Rust 구현은 최소한 다음 성격의 검증 체계를 만들 수 있어야 한다.

- spec별 핵심 계약을 family별로 추적하는 테스트 매트릭스 검증
- release gate 명령 집합이 실패 시 명확히 어느 gate가 막혔는지 보여주는 검증
- deterministic resume, approval boundary, redaction, interrupted upgrade가 blocker로 작동하는지 확인하는 메타 테스트
- demo behavior 징후를 자동으로 감지하는 smoke/regression 검증, 예: recover 미구현 시 gate 실패
- Cargo 기반 release candidate 실행 산출물로 end-to-end 흐름을 반복 실행하는 테스트

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 사람 조직의 승인 절차
- 제품 출시 마케팅 일정
- 외부 감사 인증 프로세스
- SaaS 운영 SLA
- 성능 벤치마크 제품 목표 수치, 단 blocker 성격 오류는 별도

이 항목들은 필요 시 별도 문서에서 다룰 수 있다. 단, 어떤 릴리스 절차도 이 문서가 고정한 blocker와 full implementation 기준을 약화하면 안 된다.

---

## 결론

`shacs-bot`의 release readiness는 일부 기능이 돌아가는 데모가 아니라, numbered spec set 전체의 계약이 family별 검증과 release gate로 증명되었는지에 따라 판단되어야 한다. 특히 truth correctness, recovery, approval, redaction, packaging, upgrade safety, app/process/permission ledger는 모두 blocker 수준으로 다뤄야 하며, 이 항목들이 비어 있으면 완성도가 아니라 미완성이다.

핵심은 "보여줄 수 있다"가 아니라 "반복 가능하게 증명할 수 있다"를 릴리스 기준으로 삼는 데 있다.
