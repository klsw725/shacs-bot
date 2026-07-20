# observability, diagnostics, and inspection 아키텍처 명세

Status: Complete (Baseline)

Implemented scope: 현재 구현은 local status, runtime inspect, runtime diagnostics bundle, session diagnostics projection, runtime marker projection, shared redaction, and progress evidence helper를 self-hosted local diagnostics baseline으로 지원한다.

Open work moved to: [029 durable runtime recovery and data migration](../029-durable-runtime-recovery-and-data-migration/SPEC.md), [031 ui projection, diagnostics, and release evidence parity](../031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md), [033 evaluation automation live integration](../033-evaluation-automation-live-integration/SPEC.md)

Not carried forward: remote observability SaaS, organization or admin dashboard, time-series metrics platform, SOC or audit portal, multi-tenant APM, distributed trace aggregation, long-term performance analytics product strategy는 self-hosted local baseline 밖에 둔다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/006-session-store/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`, `docs/specs/012-runtime-services/SPEC.md`, `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`를 바탕으로 `shacs-bot`의 관측 가능성, 진단, inspection surface를 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- 로그, trace, inspect surface가 어떤 사실을 남기고 어떤 사실을 남기지 말아야 하는지 정의한다.
- crash, recovery, interrupted execution의 증거 체계를 고정한다.
- redaction, privacy, secret handling 규칙이 관측 계층에도 동일하게 적용되도록 명시한다.
- self-hosted 사용자가 로컬 환경에서 장애를 재현하고 원인을 좁힐 수 있는 operability baseline을 정한다.
- future Rust 구현에서 event observer, trace envelope, diagnostics bundle, inspect reader, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 로그를 많이 찍자는 권고문이 아니다. 구현이 이 문서와 충돌하면 편의상 raw payload를 남기거나, 반대로 너무 적게 남겨 복구 근거를 잃지 말고, 공식 증거 구조부터 다시 점검해야 한다.

이 spec의 완료 기준은 debug print를 몇 군데 넣는 POC가 아니라, 이 문서가 정의한 observability boundary, diagnostics contract, recovery evidence, redaction rule, operability baseline을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 모든 공식 상태 전이의 중심이며, 관측 가능성도 그 사실을 기준으로 구성되어야 한다.
- session store와 event log는 공식 truth를 보존한다.
- host safety와 secret redaction 규칙은 로그와 진단 출력에도 동일하게 적용되어야 한다.
- 목표는 self-hosted / personal-use 환경에서 사용자가 스스로 원인을 파악하고 복구할 수 있는 수준의 운영 가능성이다.

따라서 이 문서는 중앙 수집형 SaaS 텔레메트리, 조직 단위 감사 콘솔, 멀티테넌트 APM, 원격 SOC 운영 흐름, 분산 trace aggregation, 장기 성능 분석 제품 전략을 다루지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- event, log, trace, inspect surface의 역할 구분
- crash 및 recovery 증거 구조
- diagnostics bundle과 redaction 규칙
- 기본 operability baseline과 최소 관측 항목
- 진단 조회 흐름과 failure triage 원칙
- 구현 불변식, 정상 시퀀스, 실패 시퀀스, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 특정 log library, tracing backend, metrics backend 선택
- 원격 observability SaaS 연동
- 시계열 DB 구축
- 관리자 또는 조직 운영용 대시보드 제품 설계
- SOC 또는 조직별 보안 감사 포털
- 멀티테넌트 APM과 분산 trace aggregation
- 장기 성능 분석 제품 전략

---

## 현재 구현 평가

이 문서는 self-hosted/local diagnostics baseline 구현 근거를 갖춘 상태다. 현재 코드는 로컬 상태 조회, formal diagnostics/redaction model, runtime marker 기반 projection, redacted diagnostics bundle, CLI/API diagnostics snapshot을 갖추고 있다. 단, 아래 명시적 비범위와 future gap은 FullSpec 완료로 보지 않는다.

### 현재 코드에서 확인되는 기반

- CLI는 `crates/shacs-cli/src/lib.rs`에서 `status`, `runtime inspect`, `runtime diagnostics`, `runtime update`, `runtime recover`, session diagnostics 조회 surface를 제공한다.
- session 계층은 `crates/shacs-session/src/lib.rs`의 `SessionUxDetail`, `SessionUxDiagnostics`로 세션 요약, checkpoint, recovery marker, redacted diagnostics metadata 값을 projection한다.
- API는 `crates/shacs-api/src/lib.rs`에서 `/health`, `/v1/diagnostics`, 로컬 session diagnostics query를 제공한다. `/health`는 기본 alive check이며 readiness나 dependency degraded 상태까지 표현하지 않는다.
- provider/tool 진행 상황은 callback plumbing과 `crates/shacs-utils/src/progress_events.rs`의 `ToolProgressEvent`, payload helper로 전달된다.
- runtime은 `crates/shacs-core/src/runtime/runner.rs`, `crates/shacs-core/src/runtime/agent_loop.rs`, `crates/shacs-core/tests/runtime_loop.rs`, `crates/shacs-core/tests/runtime_agent.rs`에서 checkpoint와 `pending_user_turn` 성격의 marker를 검증한다.
- shared redaction은 `crates/shacs-redaction/src/lib.rs`에서 diagnostics snapshot과 bundle serialization 전에 재귀 적용된다. 기존 scoped redaction은 `crates/shacs-core/src/tools/self_tool.rs`, `crates/shacs-core/tests/tools.rs`, `crates/shacs-config/src/lib.rs`, `crates/shacs-cron/src/lib.rs`, session diagnostics metadata 경로에서 유지된다.

### 부분 구현 또는 future gap

- `OperationalLogRecord`, `TraceRecord`, `DiagnosticsRecord`, `CrashEvidence`, `RecoveryEvidence` 같은 local JSON evidence model은 `crates/shacs-utils/src/diagnostics.rs`에 있다.
- diagnostics bundle과 artifact generator는 `runtime diagnostics --bundle <path>` 로컬 zip artifact로 구현되어 있다.
- durable trace/log store, event replay 기반 inspection, provider/tool/subagent progress는 현재 snapshot/evidence field 수준이며, 장기 저장 제품 또는 분산 수집 계층은 아니다.
- readiness와 dependency health, degraded health state, event stream reconnect/backpressure/dropped-event accounting은 별도 future 범위다.
- Web UI diagnostics/status surface와 multi-session observability ordering/isolation도 현재 구현 완료로 분류하지 않는다.

### 제품 관점의 명시적 비범위

- remote observability SaaS
- organization/admin dashboard
- time-series metrics platform
- SOC/audit portal
- multi-tenant APM
- distributed trace aggregation
- long-term performance analytics product strategy

---

## 핵심 정의

### observability

observability는 세션과 런타임이 왜 현재 상태에 도달했는지 설명할 수 있게 만드는 공식 증거 체계다. 단순히 로그가 많은 상태를 뜻하지 않는다.

### event

event는 오케스트레이터가 확정한 공식 상태 전이 사실이다. replay와 resume의 진실 원천이다.

### operational log

operational log는 공식 상태 전이 그 자체는 아니지만, 특정 실행 시점의 관찰 사실과 진단 힌트를 남기는 기록이다. 예:

- process bootstrap 결과
- config discovery 경로와 선택 결과
- effect dispatch 시작과 종료
- crash detector가 본 마지막 heartbeat

### trace

trace는 한 턴 또는 한 effect chain의 인과 관계를 따라갈 수 있게 하는 상관관계 기록이다. trace는 event보다 자세할 수 있지만, 공식 truth를 대체하지 않는다.

### inspect surface

inspect surface는 사용자가 현재 또는 최근 상태를 읽기 전용으로 조회할 수 있게 만드는 진단 창구다. CLI, TUI, local API 모두 같은 의미를 공유해야 한다.

### diagnostics bundle

diagnostics bundle은 특정 오류나 recover 필요 상태를 분석하기 위해 수집한 구조화된 진단 묶음이다. bundle은 redaction을 거친 뒤에만 사용자에게 노출되거나 파일로 저장될 수 있다.

### diagnostics artifact

diagnostics artifact는 너무 크거나 민감해서 본문에 직접 담지 않고 별도 참조로 남기는 진단 산출물이다. diagnostics artifact는 008에서 정의한 runtime-managed artifact 루트 아래에만 저장할 수 있으며, 저장 전 redaction 규칙을 통과해야 한다.

### crash evidence

crash evidence는 프로세스 중단, 강제 종료, interrupted upgrade, replay mismatch 같은 비정상 상태의 존재와 경위를 설명할 수 있게 하는 최소 증거 집합이다.

---

## 관측 가능성의 기본 원칙

1. 공식 상태 전이는 event가 설명하고, log와 trace는 그 주변 증거를 보강해야 한다.
2. trace와 diagnostics는 충분히 자세해야 하지만, secret과 민감 출력은 redaction 없이 남기면 안 된다.
3. crash와 recovery는 숨길 대상이 아니라 설명 가능한 사실이어야 한다.
4. inspect surface는 사용자가 현재 무엇이 진행 중이고 어디서 멈췄는지 알 수 있게 해야 한다.
5. 관측 데이터는 복구를 돕는 증거여야지, 새로운 진실 원천이 되면 안 된다.
6. self-hosted 환경에서 별도 중앙 시스템 없이도 문제를 좁힐 수 있어야 한다.

---

## event, log, trace, inspect의 역할 구분

### 1. event

event가 담당하는 것:

- 공식 상태 전이
- replay 가능 사실
- completed, aborted, approval requested, recovery completed 같은 확정 사실

event가 담당하지 않는 것:

- raw stdout/stderr 전체 저장
- 상세 stack trace의 무제한 보존
- 고빈도 heartbeat 샘플

### 2. operational log

log가 담당하는 것:

- 부트스트랩, 종료, signal, config discovery, effect dispatch, I/O 오류 같은 운영 사실
- event만으로는 부족한 원인 추적 보조 정보
- 사용자가 최근 실행 흐름을 시간순으로 볼 수 있는 기록

log가 담당하지 않는 것:

- 공식 세션 상태 확정
- secret 원문 저장
- replay 입력 대체

### 3. trace

trace가 담당하는 것:

- `session_id`, `turn_id`, `effect_id`, `approval_request_id`, `child_task_id` 같은 상관관계 연결
- 한 요청이 어떤 외부 호출과 재진입을 거쳤는지 설명
- late result와 stale result 판정 근거 제공

trace가 담당하지 않는 것:

- 세션 기록 자체 대체
- transport별 임시 UI 상태 저장

### 4. inspect surface

inspect가 담당하는 것:

- 현재 열린 턴, 마지막 완료 턴, pending effect, 최근 오류, recovery 필요 여부 조회
- 사용자가 다음 행동을 결정할 수 있는 수준의 요약 제공

inspect가 담당하지 않는 것:

- 원시 내부 객체 노출
- redaction 전 diagnostics 전체 공개

---

## 최소 observability 데이터 모델

### trace correlation 필수 필드

최소한 아래 correlation 필드는 event, log, trace 중 적절한 위치에 일관되게 연결 가능해야 한다.

- `session_id`
- `turn_id`, 있으면
- `effect_id`, 있으면
- `event_id`, 있으면
- `approval_request_id`, 있으면
- `child_task_id`, 있으면
- `app_id`, `app_process_id`, 있으면
- `device_id`, `port_id`, 있으면
- `service_correlation_id`, 있으면
- 시각 정보, 예: `occurred_at`, `recorded_at`

### diagnostics record 최소 필드

- 분류, 예: config, provider, tool, store, recovery, upgrade
- severity, 예: info, warning, error, critical
- 요약 메시지
- 상관관계 id 집합
- redaction 상태
- 사용자 노출 가능 여부
- 후속 action 힌트, 예: inspect, recover, retry, report

---

## inspect surface 명세

inspect surface는 013에서 정의한 interface surface가 읽는 공통 진단 모델이다.

> 참고 메모: 이 문서는 inspect의 진단 의미론을 소유하는 기준점으로 읽되, 실제 UI projection 이름과 transport surface는 013에서 소비하는 구조를 전제로 한다.
> 따라서 `SessionFocusProjection`, `ErrorProjection`, `RecoveryProjection`과 inspect contract의 정확한 매핑은 교차 문서 관점에서 더 명시될 여지가 있다.

### inspect가 최소한 제공해야 하는 정보

- 현재 lifecycle state
- 열린 턴 존재 여부와 phase
- 마지막 durably committed event sequence
- 최근 N개의 공식 event 요약
- 현재 pending approval과 pending effect 요약
- 현재 app process, device status, 최근 task receipt 요약
- 마지막 abort reason 또는 recovery required reason
- 마지막 recover 시각과 결과
- 관련 diagnostics record 요약

### inspect의 읽기 원칙

1. inspect는 진실 원천을 수정하지 않는다.
2. inspect는 session store와 trace/log index를 읽을 수 있어도, 없는 사실을 추론으로 합성하면 안 된다.
3. inspect 출력은 기본적으로 redacted form이어야 한다.
4. 사용자가 deeper diagnostics를 요청할 때만 더 자세한 bundle을 제공할 수 있다.

---

## crash 및 recovery evidence

### crash evidence의 최소 구성

아래 사실은 비정상 종료 뒤 재현 가능하게 남아야 한다.

- 마지막 프로세스 시작 시각과 종료 시각 또는 종료 미확정 사실
- 마지막 heartbeat 또는 진행 흔적
- 마지막 durably committed event sequence
- 열린 턴 존재 여부
- 중단 시점의 pending effect 요약
- interrupted upgrade marker 존재 여부, 있으면 `from_version`, `target_version`, `phase`, `partial_migration`
- recovery 미완료 marker 존재 여부

### recovery evidence의 최소 구성

- recovery 시작 시각
- recovery 대상 `session_id`
- recovery가 정리한 열린 턴 또는 interrupted marker 요약
- recovery 결과, 예: stabilized, aborted-open-turn, blocked
- recovery 이후 새 stable sequence 또는 stable state 요약

### crash와 recovery의 해석 원칙

1. crash evidence는 성공 상태를 대신 증명하면 안 된다.
2. recovery evidence는 어떤 durable 사실을 기준으로 정리했는지 설명해야 한다.
3. 메모리에만 있던 중간 출력은 crash 후 공식 성공 증거가 될 수 없다.

---

## diagnostics bundle 규칙

diagnostics bundle은 사용자가 `inspect --diagnostics` 같은 흐름으로 요청할 수 있는 구조화 묶음이다.

### bundle에 포함할 수 있는 것

- 관련 session summary
- 최근 event 요약
- 관련 trace span 요약
- 관련 operational log excerpt
- redacted error context
- config snapshot 출처 정보, 단 secret 값 제외
- 복구 또는 재현을 위한 권장 다음 action
- redacted diagnostics artifact reference, 필요 시

### bundle에 포함하면 안 되는 것

- secret 원문
- redaction 전 provider payload 전체
- redaction 전 tool stdout/stderr 전체
- 사용자의 워크스페이스 밖 민감 경로 전체 노출
- OS 사용자 인증 정보
- runtime-managed artifact 규약을 따르지 않는 임의 경로 참조

### bundle 생성 규칙

1. bundle은 항상 redaction pass를 거쳐야 한다.
2. bundle은 공식 event를 수정하거나 재정렬하면 안 된다.
3. bundle은 없는 상관관계를 추정해서 확정 서술하면 안 된다.
4. bundle 생성 실패 자체도 diagnostics record로 남겨야 한다.

---

## redaction 규칙

관측 계층은 010의 secret handling 규칙을 그대로 따라야 한다.

### redaction 대상

- API key, token, cookie, private key, credential
- secret reference가 resolve된 실제 값
- 민감 path, 예: SSH key 위치, 사용자 홈의 민감 파일 경로
- provider/tool payload 안의 민감 문자열
- 환경 변수 값

### redaction 적용 지점

- operational log 기록 전
- trace payload 저장 전
- inspect surface 렌더링 전
- diagnostics bundle 생성 전
- crash report 파일 생성 전

### redaction 규칙

1. secret value는 가능하면 애초에 구조화 필드에 들어오지 않게 한다.
2. unavoidable하게 지나간 값은 기록 전에 치환해야 한다.
3. redaction 실패 시 원문 기록 대신 기록 거절 또는 안전한 대체 요약을 남겨야 한다.
4. redaction 여부와 redaction failure 사실은 진단 가능해야 한다.

---

## operability baseline

`shacs-bot`은 별도 관리자 조직 없이 사용자가 직접 다룰 수 있어야 하므로, 최소한 아래 baseline을 만족해야 한다.

### baseline 1. 로컬에서 현재 상태를 설명할 수 있어야 한다

사용자는 최소한 아래 질문에 답할 수 있어야 한다.

- 지금 어떤 세션이 active인가
- 어떤 세션이 recovery를 필요로 하는가
- 현재 열린 턴은 무엇을 기다리는가
- 마지막 실패는 provider, tool, store, upgrade 중 어디서 발생했는가

### baseline 2. crash 이후 복구 판단 근거를 볼 수 있어야 한다

사용자는 아래 질문에 답할 수 있어야 한다.

- 마지막 durable 상태는 어디까지인가
- 열린 턴이 있었는가
- interrupted upgrade가 있었는가
- late result가 관찰되었는가

### baseline 3. 지원 요청 없이도 기본 triage가 가능해야 한다

사용자는 아래 수준의 self-triage가 가능해야 한다.

- config 오류인지
- provider/network 오류인지
- tool failure인지
- session store/replay 손상인지
- recover로 해결 가능한지

### baseline 4. 진단 출력이 안전해야 한다

진단을 위해 로그를 남기더라도 secret 유출이 기본 동작이어서는 안 된다.

---

## 결정표

### 1. diagnostics 기록 결정표

| 상황 | diagnostics 기록 | 비고 |
| --- | --- | --- |
| 공식 상태 전이 발생 | event 필수, log/trace 선택적 보강 | truth는 event |
| provider/tool 실패 | log + diagnostics record 필수 | correlation 포함 |
| recovery 필요 상태 감지 | diagnostics record + inspect surface 반영 | 사용자 action 가능 |
| secret redaction 실패 | 원문 기록 금지, redaction failure 기록 | 안전 우선 |

### 2. inspect 출력 깊이 결정표

| 요청 수준 | 제공 정보 | 제한 |
| --- | --- | --- |
| 기본 inspect | 상태 요약, 최근 event 요약, pending effect | redacted only |
| 상세 inspect | trace 요약, diagnostics summary | redacted only |
| diagnostics bundle | 관련 log excerpt, trace chain, recovery evidence | redaction 필수 |

### 3. crash evidence 해석 결정표

| 조건 | 해석 | 사용자 surface |
| --- | --- | --- |
| 열린 턴 + 정상 종료 증거 없음 | interrupted session | recover 권장 |
| interrupted upgrade marker 존재 | upgrade recovery required | recover 또는 rollback 안내 |
| event/checkpoint mismatch | storage inconsistency | recover blocked 또는 deep inspect |
| late result 관찰만 존재 | stale external result | inspect 가능, 세션 truth 불변 |

---

## 정상 시퀀스 예시

### 예시 1. provider timeout 후 diagnostics 생성

1. provider effect가 timeout으로 종료된다.
2. 오케스트레이터는 공식 실패 판단을 하고 관련 event를 남긴다.
3. operational log에 timeout 사실과 correlation id가 남는다.
4. diagnostics record가 생성된다.
5. inspect surface는 최근 오류를 `provider_timeout` 성격으로 보여준다.
6. 사용자는 recover가 아니라 retry 가능한 runtime failure로 이해할 수 있다.

### 예시 2. crash 후 recover 전 inspect

1. 프로세스가 비정상 종료된다.
2. 다음 부트스트랩에서 crash evidence가 감지된다.
3. inspect surface는 마지막 durable sequence, 열린 턴 잔재, pending effect 요약을 보여준다.
4. 사용자는 왜 recover가 필요한지 이해한 뒤 `RecoverSession` command를 실행한다.

---

## 실패 시나리오

### 시나리오 1. raw provider payload를 로그에 그대로 저장

- 잘못된 동작: secret과 민감 문맥이 log file에 그대로 남음
- 올바른 동작: 저장 전 redaction, 또는 안전한 요약만 저장

### 시나리오 2. trace를 truth처럼 사용

- 잘못된 동작: event 없이 trace만 보고 턴 완료로 판단
- 올바른 동작: trace는 인과 추적 보조이며 공식 완료는 event로만 확정

### 시나리오 3. crash 후 아무 증거 없이 자동 복귀

- 잘못된 동작: interrupted execution을 감춘 채 세션을 정상 active로 표시
- 올바른 동작: crash evidence와 recovery-required reason을 남기고 사용자가 inspect/recover 가능하게 함

---

## 구현 불변식

1. 공식 상태 전이의 truth source는 event다.
2. operational log와 trace는 event를 보강할 수 있어도 대체하면 안 된다.
3. 모든 diagnostics record는 correlation 정보를 가질 수 있어야 한다.
4. inspect surface는 읽기 전용이어야 한다.
5. crash와 recovery는 관찰 가능한 증거를 남겨야 한다.
6. secret 원문은 log, trace, inspect, diagnostics bundle 기본 출력에 남으면 안 된다.
7. redaction failure는 조용히 무시되면 안 되고 별도 진단 사실로 남아야 한다.
8. late result 관찰은 가능하지만 공식 세션 truth를 뒤집으면 안 된다.
9. interrupted upgrade 상태는 일반 runtime failure와 구분해 표시되어야 한다.
10. self-hosted 사용자가 중앙 서비스 없이도 기본 triage를 수행할 수 있어야 한다.

---

## 금지 패턴

### 1. 로그 과다 저장으로 truth와 diagnostics 경계를 흐림

왜 금지인가:

- replay와 triage가 뒤섞여 무엇이 공식 상태인지 불명확해진다.

### 2. redaction을 나중 문제로 미룸

왜 금지인가:

- self-hosted라도 secret 유출 위험이 바로 발생한다.

### 3. crash를 성공처럼 숨김

왜 금지인가:

- 사용자가 interrupted execution을 정상 완료로 오해한다.

### 4. inspect에서 raw 내부 구조체 노출

왜 금지인가:

- 안정된 계약 없이 민감 정보와 구현 세부가 새어 나온다.

### 5. diagnostics bundle에 원문 payload 덤프

왜 금지인가:

- 진단 편의 때문에 safety boundary를 무너뜨리게 된다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- `OperationalLogRecord`, `TraceRecord`, `DiagnosticsRecord`, `CrashEvidence`, `RecoveryEvidence` 같은 경계가 분리되는가?
- trace correlation id들이 session, turn, effect, approval, child task를 일관되게 잇는가?
- inspect reader가 event truth와 diagnostics 보조 데이터를 혼동하지 않게 구현되는가?
- redaction pass가 log, trace, bundle 생성 전에 공통 계층으로 적용되는가?
- crash evidence와 recovery evidence를 파일 또는 저장 구조로 남길 수 있는가?
- self-contained diagnostics bundle을 로컬에서 생성할 수 있는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

Rust 구현은 최소한 다음 성격의 테스트를 만들 수 있어야 한다.

- provider/tool failure 시 diagnostics record와 correlation이 생성되는지 확인하는 테스트
- inspect surface가 최근 abort reason과 pending effect 요약을 보여주는지 확인하는 테스트
- secret 원문이 log, trace, diagnostics bundle에 남지 않는지 확인하는 redaction 테스트
- crash 이후 recovery-required evidence가 생성되는지 확인하는 테스트
- late result가 trace/log에는 관찰되더라도 공식 completed event를 덮어쓰지 않는지 확인하는 테스트
- interrupted upgrade marker가 일반 runtime failure와 구분되어 surface에 나타나는지 확인하는 테스트
- diagnostics bundle 생성 실패가 별도 diagnostics record로 남는지 확인하는 테스트

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 중앙 observability 수집 서버
- remote observability SaaS
- organization/admin dashboard
- 관리자 감사 포털 또는 SOC/audit portal
- 조직 정책 기반 retention 규칙
- time-series metrics platform
- multi-tenant APM
- 분산 노드 간 trace aggregation
- 장기 성능 분석용 metrics 제품 전략

이 항목들은 이후 필요 시 별도 문서로 다룰 수 있다. 단, 어떤 확장도 이 문서가 정의한 redaction, truth ownership, self-hosted operability baseline을 약화하면 안 된다.

---

## 결론

`shacs-bot`의 observability는 event, log, trace, inspect가 각자 다른 역할을 가지되 하나의 인과 구조로 연결되는 형태여야 한다. crash와 recovery는 증거를 남겨야 하고, diagnostics는 self-hosted 사용자가 스스로 triage할 수 있을 정도로 충분히 자세해야 하며, 동시에 secret과 민감 정보는 redaction 없이 남아서는 안 된다.

핵심은 많이 남기는 것이 아니라, 무엇이 공식 사실이고 무엇이 진단 보조인지 구분한 채, 실패와 복구를 설명 가능하게 만드는 것이다.
