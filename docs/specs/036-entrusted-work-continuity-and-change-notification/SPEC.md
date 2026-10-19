# 036. 맡긴 일의 지속 관찰과 변화 통지 아키텍처 명세

Status: Complete (Scoped)

Consumes: 029 durable runtime, 030 trusted execution, 031 execution snapshot, 033 goal and automation

## 목적과 완료 경계

이 스펙은 [`intent.md`](../../../intent.md)의 첫 경험을 **사용자가 맡긴 일의 등록 → 조건 수정 → 주기 확인 → 이전 관측과 비교 → 의미 있는 변화 통지 또는 침묵 → 중단**까지 한 사용자 흐름으로 완결한다. 기존 goal 명령, scheduler, durable queue, evaluator와 channel sender가 존재한다는 사실만으로 이 흐름을 완료 처리하지 않는다. 여행·쇼핑 같은 개별 도메인 기능이나 전문 하네스가 없어도 이 스펙은 단독으로 종료 가능해야 한다.

현재 goal은 session metadata에 저장되고 CLI/API에서 set/pause/resume/done/clear를 지원한다. Cron·heartbeat는 자동화 work를 만들지만 production producer의 `goal_id: None`과 `active_goal: true`는 실제 위임 증거가 아니다. Cron의 source별 `Notify` 정책도 이전 관측과 달라졌다는 판정이 아니다. 현재 연결점과 근거는 [`PRODUCT-PATH-MAP.md`](../../PRODUCT-PATH-MAP.md)를 따른다.

## 소유 범위

1. **위임 의사와 작업 정체성:** 일회성 질문과 지속 위임을 구별한다. 명시적 요청 또는 제안에 대한 사용자 동의만 관찰을 시작한다. 불분명하면 질문하고, 동의 전에는 반복 wake를 등록하지 않는다. 한 작업은 안정적인 식별자, 사용자 소유 범위, 조건·이전 결정·남은 질문, 연결된 session/알림 대상을 가진다. 다른 session/channel을 같은 사용자로 자동 병합하지 않고, 연결이 필요하면 사용자가 명시적으로 선택한 대상만 결합한다.
2. **생명주기와 관찰 기준:** active/paused/blocked/done/cleared의 유일한 상태 권한자는 기존 033 goal owner다. 036은 사용자 동의, goal binding, 조건 버전 및 해당 goal 전이 ID의 참조만 보존하고 독립적인 작업 lifecycle state를 만들지 않는다. 사용자가 조건을 고치면 다음 관찰은 새 버전의 조건을 사용하고 이전 관측과의 비교 기준이 무엇인지 표시한다. 마지막 관측, 관측 시각, 다음 확인 시각 및 변화 판단에 사용한 근거를 재시작 뒤 복원한다. 과거 관측 내용이 부족하면 성공이나 무변화로 합성하지 않는다.
3. **기존 실행 경계에 결합:** pause/resume/done/clear 요청은 033 goal transition으로 처리하고 036에는 그 전이 ID만 연결한다. 033 goal owner 사실을 확인한 뒤 029 scheduler/durable work로 wake를 남긴다. enqueue 전과 실행 직전 모두 작업의 실제 활성 상태·조건 버전·위임 근거를 확인한다. 중단/종료 또는 오래된 wake는 실행·통지되지 않으며, wake 자체나 `active_goal` 선언만으로 사용자 승인을 만들지 않는다. 030/031의 현재 hook, confirmation, process, sandbox, credential, snapshot 경계를 그대로 소비한다.
4. **변화와 통지:** 첫 관찰은 비교 기준으로 기록한다. 다음 관찰은 같은 작업의 조건 버전과 비교 가능한 이전 관측을 사용한다. 동일 결과나 선택에 영향 없는 변화는 Suppress, 사용자 조건·선택지·결정 시점을 바꾸는 변화는 이전값·새값·이유·근거를 갖춘 Notify로 보낸다. 검증 불가능한 결과는 `unknown`/`blocked`로 표시하고 중요 변화인 양 알리지 않는다. 정기 요약은 사용자가 별도로 요청한 경우에만 보낸다.
5. **결과와 사용자 결정:** 기존 durable route와 channel outbound를 사용한다. 동일 관측 사건의 중복 통지는 억제하되 exactly-once 송신이나 원격 수신 ACK는 주장하지 않는다. Job 성공·통지 enqueue·실제 전달 관측을 분리한다. 예약·구매·발송 등 확정적인 외부 행동은 관찰 결과의 통지와 별개로 사용자 판단을 요구한다.

## 기존 owner와의 경계

| 기존 owner | 036이 소비하는 것 | 036이 소유하지 않는 것 |
|---|---|---|
| [029](../029-durable-runtime-recovery-and-data-migration/SPEC.md) | durable event/work, wake/retry/cancel, recovery, delivery hint | 새 queue·scheduler·lease·migration 및 exactly-once 전달 계약 |
| [030](../030-trusted-agent-runtime-and-operational-controls/SPEC.md) | hook veto, 필요한 현재 호출 confirmation, 실행·credential·sandbox 상태 | tool permission 정책, 사용자 인증, 전면 격리·redaction 보장 |
| [031](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) | config/auth locator와 실행 당시 snapshot provenance | snapshot을 현재 작업 조건이나 실행 허가의 진실로 승격하는 일 |
| [033](../033-evaluation-automation-live-integration/SPEC.md) | 유일한 goal lifecycle authority와 전이 ID, automation lifecycle, advisory evaluator, Notify/Suppress route | 기존 goal/automation/evaluator의 재구현 및 완료 범위 재판정 |

이 스펙이 새로 소유하는 것은 **위임된 일의 조건·관측·변화·통지 판단의 의미와 기존 owner 간 연결**이다. 별도 범용 task DB를 선행 조건으로 요구하지 않고 저장 방식은 정확한 owner/replay/복구 테스트로 결정한다. 지원되지 않는 app/channel/local API background terminal producer를 033에서 가져와 성공으로 표시하지 않는다.

## 반드시 지킬 것

- 사용자가 거부하거나 멈춘 일에 대한 예약 wake가 늦게 도착해도 새 관찰과 통지를 만들지 않는다. 같은 조건 변경 직전·직후의 관측을 비교할 때 구버전이 신버전 판단을 덮지 않는다.
- 관찰 결과와 변화 근거는 raw credential이나 무제한 외부 payload를 사용자 표면에 노출하지 않는다. 제한된 근거 표시가 데이터 전체의 안전이나 정확성 증명은 아니다.
- Source 종류가 Cron이라는 이유만으로 알리지 않고, 동일 결과 반복을 매번 새 사건으로 포장하지 않는다. 불확실한 전달 상태는 성공으로 합성하지 않는다.
- 자동 예약·구매·발송, 사용자 요청 없는 관심사 발굴, 전체 채널의 완전한 parity, 모든 외부 사이트의 변경 감지는 이 스펙의 완료 조건이 아니다.

## 수용 조건

1. 격리된 사용자 workspace에서 같은 문장을 일회성 질문으로 처리할 때는 반복 관찰이 생기지 않고, 명시적 위임 또는 제안에 대한 동의 후에만 작업이 생성된다. CLI/API와 한 실제 대화 진입점에서 이 구분을 확인한다.
2. 작업 조건·마지막 관측·다음 확인은 process restart 뒤에도 같은 식별자로 조회된다. 사용자의 조건 변경과 pause/resume/done/clear가 다음 wake의 사용 조건을 바꾸며, 중단 뒤 도착한 stale work도 실행·통지를 만들지 않는다.
3. 사용자 소유의 제어 가능한 자료에서 첫 관측 A, 다시 A, 선택에 영향 없는 변화, 의미 있는 변화 B를 차례로 만든다. 기준 생성과 무변화는 침묵하고, B만 이전/새 값·조건·근거가 포함된 메시지를 한 번 enqueue한다. 정기 요약은 opt-in일 때만 발생한다.
4. Notify가 기존 durable inbound → session turn → outbound 경계를 통해 올바른 대상으로 전달되는지 실제 runtime 표면에서 확인한다. 전달 실패·미확인 수신은 job 성공과 별도로 표시한다.
5. 예약이나 발송 요청은 위임된 관찰의 연장이더라도 사용자 판단 전에는 수행되지 않는다. 프로세스 재시작, 중복 wake, 조건 변경 경합에서도 이를 검증한다.

위 조건은 이 스펙의 **단독 종료 기준**이다. 작업별 전문 방식이나 방법 학습을 요구하지 않고 기존 도구·일반 실행 경로만으로 검증할 수 있어야 한다. 테스트 green, 기록된 계획, 035의 옛 QA만으로 완료 처리하지 않는다.

## 실행 PRD와 독립 종료

각 PRD는 이전 번호의 이미 닫힌 결과만 소비할 수 있다. 자신이 소유한 사용자 표면과 증거가 있으면 뒤 PRD가 없어도 완료할 수 있으며, 상태는 구현 전까지 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 위임·조건 수명주기](prds/000-entrusted-work-consent-and-lifecycle.md) | 명시 동의, 안정 작업 ID, 조건 변경·종료와 재시작 복원 | 없음 | 대화·CLI/API에서 등록·조회·수정·종료 |
| [010 예약 확인·관측 기록](prds/010-goal-bound-wake-and-observation-history.md) | 실제 goal-bound wake, 중단/구버전 차단, 마지막 관측과 다음 확인 | 000 | 사용자 소유 자료를 확인하고 알림 없이 재시작 뒤 조회 |
| [020 변화 판정](prds/020-observation-comparison-and-significance.md) | 기준 관측, 무변화/무관 변화/중요 변화/불확실성 판정과 요약 선호 | 000, 010 | 비교 이유와 제한된 근거를 조회, outbound 없음 |
| [030 통지·사용자 결정](prds/030-change-notification-and-user-decision.md) | 변화 결정의 durable 전달, 중복 억제, 전달 관측, 외부 행동 전 질문 | 000, 010, 020 | 한 실제 표면에서 침묵/통지·실패·사용자 판단을 종단 확인 |

소유 범위 1과 수용 조건 1은 000, 소유 범위 2의 조건/전이는 000·관측/다음 확인은 010이 소유한다. 소유 범위 3과 수용 조건 2의 stale wake/관측 복원은 010, 소유 범위 4와 수용 조건 3의 비교 판단은 020, 소유 범위 5와 수용 조건 3의 실제 한 번 통지 및 조건 4–5는 030이 소유한다. 조건 변경 경합은 000의 저장 사실, 010의 실행 gate, 030의 발신 직전 확인에서 각각 자신의 경계를 검증한다.

036의 최종 완료는 네 PRD의 각자 종료 증거와 실제 end-to-end 사용자 흐름을 함께 확인할 때만 가능하다. PRD 030의 실제 표면 검증은 이 통합 증거를 포함하며 별도의 뒤 번호 closure PRD에 의존하지 않는다.

## 종료 증거 (2026-10-19)

PRD [000](prds/000-entrusted-work-consent-and-lifecycle.md), [010](prds/010-goal-bound-wake-and-observation-history.md), [020](prds/020-observation-comparison-and-significance.md), [030](prds/030-change-notification-and-user-decision.md)이 각각 독립 Oracle **PASS**를 받았고, 마지막 Oracle 검토는 이 스펙의 수용 조건 1–5 전체도 **PASS**로 판정했다. 이는 현재 self-hosted 지원 범위의 scoped 완료이며 전체 release seal이 아니다.

- CLI/API와 대화 경로에서 명시적 위임·제안 수락과 일반 질문의 비승격, 조건 CAS·연결 대상·033 goal 전이 ID의 재시작 복원을 검증했다(조건 1–2).
- 격리된 실제 runtime과 로컬 SMTP 수신기에서 사용자 소유 파일의 `120 → 120 → 110 → 90`을 관찰했다. 기준·반복·무관 변화는 침묵하고, 상한 100을 넘는 상태에서 충족 상태로 바뀐 90만 이전/새 값·조건·양쪽 digest·한계가 담긴 메시지 한 건으로 전달됐다. 중복 wake와 재시작은 같은 통지를 다시 만들지 않았고 summary는 명시적 opt-in에서만 별도 유형으로 처리했다(조건 2–3).
- 029 durable inbound → session turn → MessageBus/ChannelManager → 기존 Email SMTP 경로를 실행했다. SMTP 550 실패, pending, sent hint는 관측 성공 및 enqueue와 별개로 남았다. API enqueue 후 pause한 오래된 알림은 channel runtime 재시작 시 outbound/assistant history 없이 억제됐다(조건 2·4).
- 외부 확정 행동 제안은 별도의 사용자 결정을 요구했다. 실제 AgentLoop 대화에서 무응답·거절·위조 ID는 fake side effect 0회, 정확한 수락 뒤에도 도구별 permission 전 0회, 별도 승인 후에만 1회였다. 조건/상태 경합과 프로세스 재시작도 보류된 제안을 닫는다(조건 5).

`cargo fmt --manifest-path crates/Cargo.toml --all -- --check`, workspace 전체 target clippy, 관련 core·CLI·API 회귀와 실제 runtime SMTP 3개가 통과했다. 전체 workspace 테스트는 장시간 제한과 기존 Spec034 fixture 누락 때문에 green을 주장하지 않는다. 현재 지원은 사용자 소유의 bounded 로컬 정수 JSON과 선택된 owner Email session 한 곳이다. 원격 ACK·exactly-once·이미 송신된 메시지의 회수·범용 외부 행동 분류·모든 채널 parity는 제공하지 않는다. 실제 명령과 제한은 [사용 가이드](../../USAGE.md#명시적으로-맡긴-작업)를 따른다.
