# PRD 010. 변화·확인 실패·회복·공백 알림

Status: Planned

Depends on: [PRD 000](000-web-page-observation-and-condition-decision.md)과 [036](../../036-entrusted-work-continuity-and-change-notification/SPEC.md) 완료 사실. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

웹 관측의 결과를 사용자에게 알리되, 알림이 없다면 **기준 시간을 넘는 확인 공백 없이 확인했고 의미 있는 변화도 없었다**는 뜻이 되게 한다. 조건 결과가 바뀐 변화, 기준 시간을 넘은 확인 실패와 그 회복, 어시스턴트가 꺼져 있던 공백을 각각 한 번씩 알린다.

## Scope

1. **042 알림 설정:** 위임마다 단일 `email:<address>` 알림 대상과 실패 알림 기준 시간을 042 알림 설정 revision으로 저장하고 CLI/API로 설정·조회한다. 이 revision은 관측 revision과 따로이며 바꿔도 비교 기준이 다시 잡히지 않는다. 036의 `linked_targets`를 발송 대상으로 재해석하지 않고 036 작업의 owner-Email 규칙을 바꾸지 않는다. Email 설정이 없거나 지원하지 않는 대상은 거절한다.
2. **변화 알림:** PRD 000의 `notify_candidate`만 036의 durable route와 Email 경로로 보낸다. 이전/새 값, 조건과 충족 여부, query·fragment를 뺀 URL, 두 관측의 읽은 시각, 근거 digest, 확인 한계, 추적 ID를 결정적으로 렌더링한다.
3. **확인 실패 알림:** PRD 000의 실패 구간이 `runtime_gap`을 뺀 길이로 기준 시간 이상 이어지면 구간마다 한 번, 실패 분류·구간 시작 시각·마지막 성공 시각·다음 확인 시각을 담아 알린다. 기준 시간의 기본값은 확인 주기의 2배와 6시간 중 긴 시간이며, 확인 주기의 2배 이상 7일 이하에서 바꿀 수 있다. 기준 시간 전에 닫힌 구간은 알림도 회복 알림도 만들지 않는다. 알림 설정 revision이 바뀌어도 열린 실패 구간은 이어지며 새 기준 시간으로 다시 판단하고, 이미 보낸 실패 알림은 다시 보내지 않는다.
4. **회복 알림:** 실패 알림을 보낸 구간이 성공으로 닫히면 한 번 알린다. 그 성공이 `notify_candidate`이면 회복 사실을 담은 변화 알림 한 건만 보낸다.
5. **공백 알림:** PRD 000이 기록한 활성 위임의 `runtime_gap`이 기준 시간 이상이면 공백 기간을 담아 한 번 알린다. 공백은 실패 구간의 길이에 넣지 않으므로 같은 공백 때문에 실패 알림이 따라오지 않는다. pause/done/clear 상태의 위임에는 공백 알림이 없다.
6. **중복과 오래된 알림:** 알림 종류와 사건별로 중복 enqueue를 막는다. enqueue 직전과 소비 직전에 036과 같은 owner·goal·동의·관측 revision과 042 알림 설정 revision을 다시 확인해 pause/done/clear나 설정 변경 뒤의 오래된 알림을 억제한다. 확인 성공·판정·enqueue·전달 사실을 분리해 조회한다.

## Non Scope

- 새 channel transport와 Email 이외 대상으로의 자동 발송, 결정 마감 알림([020](020-decision-deadline-reminder.md)), 대화형 요약([030](030-conversational-entrustment-summary-and-confirmation.md)).
- 모델이 만든 알림 문구나 중요도 재평가, 036 정기 요약 opt-in의 의미 변경.
- 원격 수신 ACK, exactly-once 전달, 이미 보낸 메시지의 회수.

## Parent Requirement Mapping

- [042 SPEC](../SPEC.md)의 소유 범위 5·6과 수용 조건 4·5를 소유한다.

## Required Contract

1. 변화 알림은 저장된 판정만 소비하며 중요도를 다시 판단하지 않는다. 확인 실패 중에는 변화 알림을 만들지 않는다.
2. 실패·회복·공백 알림은 PRD 000의 상태 사실(마지막 성공 시각, 실패 구간, `runtime_gap`, 다음 확인)만으로 결정적으로 만든다.
3. 같은 사건은 재시작·중복 wake·retry에도 한 번만 enqueue한다. 소비를 claim한 뒤 crash하면 재발송 대신 `unknown`을 보존한다.
4. 알림 payload는 제한된 값·근거만 담고 응답 본문, URL query·fragment, credential을 담지 않는다.

## Acceptance Criteria

1. 실제 runtime, `127.0.0.1/32`만 허용한 시험용 profile의 로컬 HTTP 서버, 로컬 SMTP 수신기, 시험용 시계로 A → A → 무관 변화 → B를 실행해 B의 변화 알림만 한 번 받는다.
2. 기준 시간을 넘긴 실패 구간에서 실패 알림 한 건, 회복 뒤 회복 알림 한 건을 받는다. 실패 중의 값 변화는 알림을 만들지 않고, 기준 시간 전에 회복한 구간은 아무 알림도 만들지 않는다. 회복 확인이 조건 결과 변화이면 회복 사실이 담긴 변화 알림 한 건만 받는다.
3. runtime을 기준 시간 넘게 멈췄다가 다시 시작하면 공백 알림 한 건을 받고 바로 확인이 이루어진다. 공백 뒤 확인이 실패해도 실패 알림이 바로 따라오지 않는다. 기준 시간 안의 재시작과 pause 상태의 위임은 공백 알림을 만들지 않는다.
4. 같은 사건의 중복 wake·재시작, enqueue 뒤 pause·설정 변경 경합에서 중복되거나 오래된 알림이 나가지 않는다. SMTP 실패는 확인 성공과 별개로 기록된다.

## Closure Evidence

1. 실제 runtime·로컬 HTTP 서버·로컬 SMTP·시험용 시계의 transcript와 수신 메시지, 알림 사실 조회 결과.
2. 기준 시간 경계, 회복과 변화 알림의 합침, 공백과 실패 구간의 구분, 중복 억제, 오래된 알림 억제 테스트.
3. 사용자 문서에 알림 종류·기준 시간·비보장 범위를 기록.

PRD 010은 PRD 000의 닫힌 관측·상태 사실만 소비한다. 알림 조건을 모두 충족하면 마감 알림이나 대화형 확인 없이 종료한다.
