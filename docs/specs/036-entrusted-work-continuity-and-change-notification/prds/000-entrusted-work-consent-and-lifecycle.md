# PRD 000. 명시적 위임과 작업 조건의 수명주기

Status: Complete (Scoped)

## Goal

사용자가 지속적으로 맡긴 일만 안정적인 작업 식별자로 보존하고, 조건·결정·중단 상태를 직접 조회·수정할 수 있게 한다. **주기적 확인이나 통지가 없어도 이 PRD만으로 동작하고 완료 판정할 수 있어야 한다.**

## Scope

1. 일회성 질문, 명시적 지속 위임, 어시스턴트의 제안·사용자 동의/거절, 애매한 요청의 확인을 구분하는 실제 대화 진입점.
2. 사용자 소유 작업 ID, 연결한 session/알림 대상, 목표·조건·이전 결정·남은 질문과 수정 버전·변경 시각, 033 goal ID·전이 ID 참조. 임의 session/channel 자동 병합이나 별도의 작업 lifecycle state 생성 금지.
3. 현재 [033](../../033-evaluation-automation-live-integration/SPEC.md) goal이 active/paused/blocked/done/cleared의 유일한 authority다. 위임 근거와 작업 조건은 goal 설명 텍스트에서 추정하지 않고, 상태 표시·전이는 033 owner 사실을 소비한다.
4. 사용자에게 조회·조건 변경·pause/resume/done/clear를 제공하는 CLI/API와 한 실제 대화 표면. 수명주기 명령은 033 goal transition을 요청한 뒤 해당 전이 ID를 연결하며 재시작 뒤 같은 작업 ID/조건/전이 이력을 읽는다.

## Non Scope

- 관측값 저장, 반복 wake 실행, 변화 판정, 사용자 선제 통지. 이 PRD는 뒤 PRD가 구현돼야 완료되는 절차를 포함하지 않는다.
- 새로운 범용 task DB, 033의 goal lifecycle 재구현, 사용자 인증 또는 전체 채널 identity 병합.
- 위임을 tool permission grant 또는 예약·구매·발송 승인으로 해석하는 동작.

## Parent Requirement Mapping

- [036 SPEC](../SPEC.md)의 소유 범위 1 및 2 중 위임·조건·생명주기·버전 사실을 소유한다.
- 부모 불변식의 거부/중단·조건 경합 사실을 저장 경계에서 소유한다. 실제 예약 work 차단은 이후 wake 경계에서 별도로 검증하되 이 PRD의 종료에 필요하지 않다.
- 부모 수용 조건 1 전체와 2의 작업 ID·조건 변경·전이의 재시작 복원 부분을 소유한다.

## Required Contract

1. 지속 위임은 명시적 요청 또는 확인된 제안으로만 만든다. 일회성/모호한 입력 뒤에는 저장된 활성 위임이 없어야 한다. 거절·철회한 제안이 재시작 뒤 활성 작업으로 변해서는 안 된다.
2. 작업 조건의 버전과 연결 session/알림 대상은 사용자 소유 경계에서 변경한다. 수정 전후 내용과 현재 유효 버전을 구분하고 오래된 입력이나 동시 수정이 최신 결정을 덮지 않게 한다.
3. 033 goal이 유일한 lifecycle 상태 owner이고, 036 작업 자료는 consent·조건 버전·goal binding과 확정된 전이 ID만 저장한다. pause/resume/done/clear는 033 transition을 호출하고 성공한 ID를 참조한다. 바인딩이 stale이거나 owner 사실과 충돌하면 새 상태를 합성하지 않고 차단한다.
4. 기존 저장된 goal/session은 사전 사용자 위임 증거로 소급 변환하지 않는다. 읽기 호환과 새로운 위임의 opt-in을 구분한다.

## Acceptance Criteria

1. 격리된 workspace에서 같은 주제의 일회성 질문, 명시적 위임, 모호한 요청, 제안 후 거절/동의를 실제 대화 경로와 CLI/API에서 각각 확인한다. 동의 전에는 활성 작업이 없고, 동의 뒤에는 같은 작업 ID·현재 조건이 조회된다.
2. 조건·예산·이전 결정 수정과 pause/resume/done/clear를 수행하고 재시작한다. 동일 작업 ID와 현재 조건·버전, 033 owner의 정확한 전이 ID/상태가 함께 반환되며, 종료 작업은 새 활성 작업처럼 보이지 않는다. 036 자료에 별도 상태를 기록하지 않는다.
3. 다른 session 또는 channel의 우연히 같은 문장을 전송해도 자동으로 작업이 병합되지 않는다. 명시적으로 연결한 대상만 이 작업을 조회/수정할 수 있다. 오래된 조건 버전으로의 수정은 최신값을 덮지 않는다.

## Closure Evidence

1. 일회성/위임/제안 거절·동의의 실제 대화·CLI/API 출력과 저장 상태의 대조 기록.
2. 재시작 전후 작업 ID·조건·버전·전이의 round-trip 및 stale/동시 수정 거절 테스트.
3. 033 goal owner 소비와 과거 goal data 읽기 호환성 테스트. 활성 위임으로 소급 승격하지 않은 근거.

PRD 000은 **작업을 맡기고 나중에 고치거나 끝내는 사용자 경험**만으로 종료된다. 예약 확인이나 알림의 구현·검증을 기다리지 않는다.

## 종료 확인 (2026-10-19)

- Core의 동의·조건 revision·연결 대상·033 goal 전이와 과거 goal 호환, CLI/API와 실제 대화 명령·제안 동의/거절을 구현했다. 관측 예약과 통지는 포함하지 않는다.
- `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test spec036_entrusted_work --test spec036_entrusted_work_compat --test entrusted_work_conversation -p shacs-cli --test spec036_entrusted_work_cli -p shacs-api --test spec036_entrusted_work_api -p shacs-command --test router`: 32개 통과. 기존 033 goal 회귀 10개와 fmt, workspace clippy, clean workspace build도 통과했다.
- 격리된 workspace에서 CLI의 동의 없는 생성 거절, 명시적 생성·연결 대상 조회, 조건·대상 수정과 오래된 revision 거절을 확인했다. 실제 API는 미연결 대상 403, pause 전이 ID와 재시작 뒤 같은 작업 ID·상태 조회 200을 반환했다. 대화 REPL에서 `/work create/read/edit` 후 저장 조건을 확인했고 제안 수락·거절/다른 발신자 경계는 `entrusted_work_conversation` 통합 테스트로 확인했다.
- Oracle 독립 검증: **PASS**. PRD 000의 명시적 동의, 033 유일 lifecycle authority, 수정 경합, session/channel 격리, 재시작 복원에 차단 사유가 없다고 판정했다. 후속 PRD 010의 관측·wake나 020/030의 판정·통지는 이 PASS에 포함되지 않는다.
