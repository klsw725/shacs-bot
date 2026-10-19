# PRD 030. 변화 통지와 사용자 결정의 실제 표면

Status: Complete (Scoped)

Depends on: [PRD 000](000-entrusted-work-consent-and-lifecycle.md), [PRD 010](010-goal-bound-wake-and-observation-history.md), [PRD 020](020-observation-comparison-and-significance.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

확인된 변화 판단을 사용자에게 **조건·이전/새 값·이유·근거가 있는 짧은 메시지**로 전달하고, 외부에 확정적인 행동은 사용자 판단 전까지 실행하지 않는다. 이 PRD가 기존 경로에서 위임→확인→침묵/통지→중단까지 실제 표면으로 연결해 [036 SPEC](../SPEC.md)을 독립 종료할 마지막 사용자 경험을 소유한다.

## Scope

1. PRD 020의 Notify 후보만 현재 유효한 위임·조건 버전·선택된 session/통지 대상으로 연결한다. 첫 기준 관측, Suppress, blocked/unknown, 철회된 작업은 보내지 않는다. 정기 요약은 PRD 020에서 기록한 사용자의 명시적 선호가 있을 때만 독립 유형으로 보낸다.
2. 기존 033 outcome route와 029 durable inbound → session turn → MessageBus/ChannelManager outbound를 사용한다. 현재 고정된 “Report the completed automation result” instruction만으로 변화의 내용이 채워졌다고 주장하지 않고, 실제 결과의 제한된 요약·근거와 추적 ID를 사용자 메시지에 연결한다.
3. 작업 실행 성공, Notify 결정, durable enqueue, transport의 보수적인 sent/failure/unknown 관측을 구별한다. 동일 작업·조건 버전·관측 사건의 중복 enqueue를 막고, 실패 후 재시도는 기존 owner fact에 따라 처리한다. 원격 수신 ACK나 exactly-once 전달은 보장하지 않는다.
4. 예약·구매·발송처럼 사용자 대신 확정적인 외부 행동을 제안할 때는 확인 질문으로 멈춘다. 이 질문은 도구별 permission approval을 대체하거나 성공을 암시하지 않는다. 사용자 거부/중단/조건 변경이 진행 중인 보고 또는 후속 행동보다 우선한다.

## Non Scope

- 새로운 channel transport, 범용 remote notification service, 모든 채널의 parity 또는 별도 task store.
- 의미 변화 판정 알고리즘, 시간 scheduler, goal 상태 재구현. 앞 PRD의 사실을 소비하며 같은 의미를 다시 소유하지 않는다.
- 사용자가 맡기지 않은 관심사 탐색, 자동 예약·구매·발송, 외부 플랫폼 read receipt 및 정확히 한 번 전달 보장.

## Parent Requirement Mapping

- [036 SPEC](../SPEC.md)의 소유 범위 5와 범위 4의 판정 결과를 durable Notify/Suppress 및 사용자 표면으로 연결하는 부분을 소유한다.
- 부모 불변식의 source-only 알림 금지·중복 사건 억제·보수적 delivery/민감 payload 경계를 사용자 표면에서 검증한다.
- 부모 수용 조건 3의 B 한 번 통지·opt-in 정기 요약, 4의 실제 outbound·실패 구분, 5의 사용자 결정·경합/재시작을 소유한다. 부모 조건 1–2는 앞 PRD의 종료 증거를 사용하되 이 PRD의 실제 통합 시나리오에서 재확인한다.

## Required Contract

1. Notify 전과 durable inbound 소비 전 작업 상태·조건 버전을 재확인한다. 통지하려던 사이 pause/done/clear나 조건 변경이 발생하면 오래된 결과를 현재 사실처럼 보내지 않는다.
2. 외부 전송이 실패하거나 단지 queue에 들어간 경우를 사용자 수신 성공으로 표시하지 않는다. 동일 사건을 두 번 처리해도 새 통지 의도를 매번 만들지 않으며, 다른 실제 변화는 별도 사건으로 기록한다.
3. 실제 전달 payload에는 안전한 이전/새 값 요약, 중요한 이유, 근거 ref와 확인되지 않은 범위를 포함한다. Raw credential·전체 provider/tool 출력이나 외부 payload를 인용만으로 무제한 재출력하지 않는다.
4. 사용자 판단을 요청하는 동작은 기본 조사·계산·초안 작업과 분리한다. 확인 응답이 없거나 거절이면 해당 확정 행동을 실행하지 않는다. 기존 030 hook/confirmation/credential/sandbox 경계는 그대로 적용한다.

## Acceptance Criteria

1. 격리된 workspace와 제어 가능한 입력으로 위임을 등록하고 재시작 뒤 A→A→선택 무관 차이→의미 변화 B를 실행한다. A/동일/무관 차이는 사용자 메시지 없이 관찰 가능하며, B만 이전/새 값·조건·근거를 담아 올바른 대상에게 한 번 전달된다.
2. 동일 사건 중복 wake·retry와 사용자 pause/done/clear 또는 조건 변경 경합을 각각 만들고, 늦은 통지와 외부 행동이 발생하지 않음을 확인한다. 새로운 B와 이전 B의 중복 처리 결과는 구별된다.
3. user-visible delivery의 pending/failed/unknown/sent hint를 job 결과와 분리한다. 실제 runtime의 CLI 또는 API 진입, 하나의 지원 channel/로컬 전달 표면에서 durable inbound → session turn → outbound 흐름을 검증하고 송신 실패를 성공으로 바꾸지 않는다.
4. 사용자가 정기 요약을 요청한 경우와 아닌 경우, 예약·발송 제안에 응답하지 않거나 거절/승인한 경우를 확인한다. 사용자 승인 이전에는 외부 확정 행동이 일어나지 않고 승인 후에도 기존 실행 gate를 거친다.

## Closure Evidence

1. 위임 생성·조건 수정·재시작·무변화 침묵·변화 통지·중단을 연결한 실제 표면 transcript와 당시 owner/관측/route/전달 사실.
2. 동일 관측 dedupe, stale work, 조건 경합, channel 실패, 필수 사용자 판단과 tool gate의 분리 테스트.
3. 029/030/031/033 owner 사실을 읽는 통합 경로와 raw payload 제한/unknown delivery 비보장에 대한 사용자 문서 확인.

이 PRD는 앞 PRD에서 이미 닫힌 위임·관측·판정 결과만 소비한다. 자체 Notify/침묵·실제 전달·사용자 결정 조건을 모두 충족하면 뒤 PRD 없이 종료한다. 과거 스펙의 QA 기록이나 테스트 green만으로 현재 사용자 흐름 완료를 추정하지 않는다.

## 알림 route와 전달

아래는 알림·전달 구현 경계다. 외부 확정 행동의 사용자 결정과 독립 Oracle PASS는 뒤의 종료 근거에 구분해 기록한다.

- PRD020의 저장된 판정만 소비한다. 기본 baseline/suppress/unknown/blocked는 메시지를 만들지 않으며, 명시적 opt-in은 별도 `regular_summary` 유형으로 처리한다. 변화 알림은 이전/새 수치, 상한 조건, 이유, evidence digest, 시각, 한계, correlation ID를 결정적으로 렌더링한다. 기존 033의 일반 provider instruction을 호출하거나 중요도를 재평가하지 않는다.
- 첫 지원 대상은 owner session `email:<address>`이다. 연결 대상들을 자동 발송 대상으로 확장하지 않는다. 다른 owner 표면은 unsupported로 기록한다. 기존 `DurableWorkDispatcher::enqueue_inbound` → AgentLoop session turn → MessageBus/ChannelManager → Email SMTP worker를 사용하고 새 queue/scheduler/goal lifecycle은 만들지 않는다.
- enqueue와 소비 직전에 owner·033 goal·동의·조건 revision·goal transition을 session mutation lock 안에서 확인한다. 합성 입력은 일반 provider turn/중간 입력 주입/ask_user/approval/workflow resume 경로와 분리하며 유효·stale 알림 모두 대화 history에 합성 user/assistant 메시지를 추가하지 않는다. 유효 소비의 accepted/completed session-turn fact는 기존 durable event에 남는다.
- correlation과 단계별 사실은 같은 029 event log의 `entrusted.notification_fact`로 보존한다. 전체 log의 기존 enqueue 사실을 확인하므로 terminal projection/history 보존 수와 중복 억제를 혼동하지 않는다. 관측 성공, 판정, enqueue, consumption, pending/failed/unknown/sent_hint를 따로 조회한다. 소비를 claim한 뒤 crash하면 재발송 대신 unknown/누락 가능성을 보존한다. remote ACK/read receipt나 exactly-once를 주장하지 않는다.
- 실제 runtime에서 surface approval worker와 external processor가 같은 관측 tick을 경쟁해 이미 leased/terminal인 work를 재lease하는 문제가 드러났다. 기존 session lock 안에서 현재 queue 상태를 다시 확인하고 cancel도 같은 lock 안에서 처리하도록 보정했다. 경쟁 소비 회귀는 수정 전 실패를 확인한 뒤 통과했다.

재현 명령은 `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli --test spec036_notification_runtime -- --ignored --nocapture`다. 실제 compiled binary의 CLI 위임/설정, production worker, 로컬 SMTP 수신기, 재시작을 사용하며 각 실행의 격리 directory를 `QA_ROOT`로 출력한다. snapshot JSON, 실제 `.eml`, runtime log 및 notification event transcript를 그 directory에 보존한다. 기본 unit/integration 검증은 `spec036_notification`과 기존 `spec036_observation`, `durable_dispatch`, `spec033_automation_dispatch`를 사용한다. 외부 provider endpoint는 사용하지 않는 loopback 주소로 지정해 provider로 알림을 만드는 경로를 성공 증거로 바꾸지 않는다.

실제 확인한 범위는 A/A/무관/B 단일 SMTP 메시지, SMTP 550 실패와 관측 성공의 분리, 같은 data directory 재시작 후 중복 방지, malformed source의 unknown과 pause 취소의 blocked 침묵, API worker에서 pending enqueue 후 pause하고 channel runtime을 시작했을 때 outbound/history 없는 stale 억제다. 별도 실제 SMTP 시나리오에서 summary opt-in의 baseline/suppress가 change alert 아닌 `regular_summary` 두 건으로 전달됐다. pause/done/clear/조건 변경의 enqueue·소비 경계, owner mismatch, 승인 질문 보존, unknown recovery는 core 회귀로 별도 검증한다.

## 외부 행동의 사용자 결정

이 단위는 Required Contract 4와 Acceptance Criteria 4의 사용자 결정 부분을 알림·전달과 구분해 소유한다.

- 기존 `ask_user` interruption에 `external_action` 선언을 추가했다. `reserve/purchase/send`, 현재 work ID, 제한된 안전 요약과 opaque reference를 받고, 같은 provider batch의 다른 도구는 실행하지 않는다. 전용 예약·구매·발송 도구는 만들지 않았다.
- Runtime이 부여한 고유 proposal ID와 원래 effective/source session·channel·chat·sender, work·consent·condition revision·goal ID/transition을 typed session metadata에 결속한다. `/action accept <id>`만 한 번 provider를 재개하고, 거절·무응답·잘못된 요청자/ID·일반 `yes`·`/work` 동의는 실행하지 않는다. 수락은 permission grant/cache가 아니다.
- 현재 session lock 안에서 이미 읽은 owner 사실을 검증하므로 잠금을 재획득하지 않는다. 후속 tool permission을 기다리는 동안에도 lineage를 보존하여 변경·pause/resume/done/clear·restart 뒤 승인을 실행으로 바꾸지 않는다. Epoch는 프로세스별이므로 운영 경로의 턴별 AgentLoop 재생성은 허용하지만 새 프로세스는 새 제안을 요구한다. 결정 binding 전 crash checkpoint도 일반 질문 재개로 전환하지 않는다.
- `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test spec036_action_decision -- --nocapture`는 실제 공개 AgentLoop `process_message` 경로, mock provider와 실행 카운터가 있는 좁은 fake `message` 도구를 사용한다. 자유 텍스트 재개와 질문 앞에 함께 반환된 도구의 경계는 수정 전 2건 실패를 확인했다. 수락 후 별도 permission prompt에서 실행 0회, 이후 도구별 `approve`에서 1회를 transcript로 출력한다. 새 subprocess로 pending/수락 후 restart 거절도 실행한다. 실제 외부 구매·예약이나 transport 송신 테스트로 해석하지 않는다.
- 사용자 명령, 동일 실행 프로세스 제한, 기존 permission/hook/confirmation 경계와 비보장 범위는 `docs/USAGE.md`에 기록한다. 기존 알림 구현과 PRD000/010/020의 완료 상태는 이 단위가 재소유하지 않는다.

## 독립 종료 확인 (2026-10-19)

- 사용자 소유 자료의 `120 → 120 → 110 → 90`에서 첫 세 관측은 메시지 없이 남고, 상한 100의 충족 여부가 바뀐 90만 이전/새 수치·조건·양쪽 digest·한계·tracking ID가 담긴 로컬 SMTP 메시지 한 건으로 전달됐다. 실제 재시작·중복 처리에서 추가 통지는 없었고 별도 변화 130은 SMTP 550 실패로 기록돼 관측 성공과 전달 실패가 분리됐다. API enqueue 후 pause한 pending 통지는 channel runtime 재시작에도 outbound와 assistant history 없이 억제됐다. 정기 요약은 명시적 opt-in에서만 `regular_summary`로 전달됐다.
- 대화에서 무응답·일반 `yes`·위조 ID·거절은 fake 발송 0회, 정확한 `/action accept <id>` 뒤에도 별도 도구 permission 전까지 0회, 도구별 `approve` 뒤에만 1회였다. 이것은 실제 원격 발송이 아니라 사용자 결정과 기존 도구 권한을 분리한 검증이다. 조건·goal 경합 및 프로세스 재시작은 보류된 결정을 닫는다.
- `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test spec036_action_decision --test spec036_notification --test spec036_observation`의 action 16개·notification 13개·observation 33개와, `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli --test spec036_notification_runtime -- --ignored --nocapture`의 실제 runtime/SMTP 3개가 통과했다. Workspace fmt/clippy와 변경 파일 진단, `git diff --check`도 통과했다. 전체 workspace 테스트는 장시간 제한과 기존 Spec034 fixture `.omo/evidence/spec034/task-12-integration.json` 부재 때문에 green을 주장하지 않는다.
- Oracle 독립 검토는 PRD 030의 수용 조건 1–4를 **PASS**로 판정했다. SMTP `sent_hint`는 원격 mailbox 수신 ACK가 아니며 claim 후 crash의 누락/unknown, 기존 transport retry의 중복 가능성, 이미 outbound로 넘긴 메시지의 회수 불가를 명시적으로 유지한다.
