# PRD 020. 결정 마감 알림

Status: Planned

Depends on: [PRD 010](010-change-failure-recovery-and-gap-notification.md)의 042 알림 대상과 전달 경로, [036](../../036-entrusted-work-continuity-and-change-notification/SPEC.md) 완료 사실. [PRD 000](000-web-page-observation-and-condition-decision.md)의 최신 관측 사실이 있으면 함께 보여주되 없어도 동작한다. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

사용자가 정한 결정 마감이 다가오면 **변화가 없어도 한 번 알린다.** 마감을 정하거나 바꿔도 관측의 비교 기준은 그대로다.

## Scope

1. **마감 revision:** 결정 마감 시각과 알림 시점(기본값: 마감 24시간 전)을 관측 revision과 분리된 `deadline_revision`으로 저장하고 CAS한다. 사용자가 입력한 시간대 표기와 절대 시각을 함께 보존한다.
2. **검증:** 마감 자체가 지났으면 거절한다. 마감은 미래지만 알림 시점이 이미 지났다면 "확인 직후 즉시 알림"을 명시적으로 선택했을 때만 받는다.
3. **알림:** 029 durable work로 알림 시점에 한 번 wake한다. 위임 목표, 마감, 남은 시간, 조건, 마지막 성공 확인 시각과 최신 값(있다면)을 결정적으로 렌더링해 PRD 010의 알림 대상과 전달 경로로 보낸다.
4. **취소와 재개:** 마감·알림 시점 변경, pause/done/clear, 동의 철회 뒤에는 이전 예약을 취소하고 알리지 않는다. resume 때 알림 시점이 아직 미래면 다시 예약하고, 이미 지났으면 소급해 보내지 않는다. 마감이 지난 뒤에도 goal이 active이면 관측은 계속하고 마감 알림을 더 만들지 않는다.
5. CLI/API로 마감 설정·조회·삭제를 제공한다.

## Non Scope

- 마감 자동 추정, 반복 리마인더, 마감 이후의 자동 중단이나 자동 행동.
- 웹 관측과 실패 알림([000](000-web-page-observation-and-condition-decision.md), [010](010-change-failure-recovery-and-gap-notification.md))의 의미 변경, 대화형 요약([030](030-conversational-entrustment-summary-and-confirmation.md)).

## Parent Requirement Mapping

- [042 SPEC](../SPEC.md)의 소유 범위 7과 수용 조건 6을 소유한다.

## Required Contract

1. 마감 알림은 변화 판정과 독립적이며 `notify_candidate`를 만들거나 비교 기준을 바꾸지 않는다.
2. `deadline_revision` 하나에 알림은 한 번이다. 재시작·중복 wake에도 다시 보내지 않는다.
3. 마감 알림 의도는 `deadline_revision`과 PRD 010의 알림 설정 revision에 함께 결속된다. enqueue와 소비 직전에 036과 같은 owner·goal·동의 확인과 두 revision 확인을 모두 거치며, 알림 대상이 바뀌었으면 이전 대상으로 보내지 않는다.

## Acceptance Criteria

1. 실제 runtime, 로컬 SMTP, 시험용 시계로 변화가 없는 위임의 알림 시점에 마감 알림 한 건을 받는다.
2. 마감을 바꾸면 이전 시점에는 알림이 없고 새 시점에 한 건을 받는다. 마감 변경 전후의 관측 판정 기준이 이어진다.
3. 알림 시점 전 pause는 알림을 만들지 않는다. 알림 시점이 지난 뒤 resume하면 알림이 없고, 지나기 전 resume하면 그 시점에 한 건을 받는다. done/clear 뒤에는 알림이 없다.
4. 알림 시점 직전·직후의 재시작과 중복 wake에서 알림이 한 건만 남는다. 지난 마감은 거절되고, 남은 시간이 기본 알림 간격보다 짧으면 즉시 알림을 명시적으로 선택했을 때만 설정된다.

## Closure Evidence

1. 실제 runtime·로컬 SMTP·시험용 시계의 transcript와 수신 메시지.
2. 시간대·경계 시각·취소·재개·중복 억제·재시작·비교 기준 유지 테스트.
3. 사용자 문서에 마감 설정 방법과 비보장 범위를 기록.

PRD 020은 PRD 010의 알림 경로만 소비해 마감 알림 경험을 닫는다. 대화형 확인의 구현을 기다리지 않는다.
