# PRD 000. service reentry and dedup

## 목표

이 문서는 `docs/specs/012-runtime-services/SPEC.md`의 하위 실행 문서다. queue, scheduler, mailbox, hooks, background worker를 제품 기능이 아니라 재진입 보조 서비스로 다루며, dedupe, retry, wake, failure-safe reentry 구현 계획을 고정한다.

이번 PRD의 목표는 어떤 서비스도 세션 truth를 직접 건드리지 못하게 하면서, 중복 전달, 재시작, 지연 전달 상황에서도 오케스트레이터가 안정적으로 같은 결론을 내리게 만드는 것이다.

## SPEC 입력

- 주관 spec: `docs/specs/012-runtime-services/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/011-subagent-runtime/SPEC.md`
- 교차 의존:
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 002는 service command envelope와 correlation 규약의 기반이 된다.
- 006은 replay 가능한 truth source를 제공하므로 서비스 메타데이터가 truth를 대체하면 안 된다.
- 007은 wake 이후 resume, ignore, retry, reject 결정을 가진다.
- 011은 subagent completion과 background completion이 같은 reentry 규약 아래 돌아와야 함을 요구한다.
- 015는 service restart와 interrupted lifecycle에서 stale wake가 정상 가능성임을 제공한다.

## 범위

- queue, scheduler, mailbox, hooks, background worker의 emit 범위 구현
- service-owned metadata와 session truth 경계 구현
- dedupe key와 idempotent reentry 처리 구현
- wake command와 resume 판단 입력 구현
- service restart 이후 duplicate delivery, stale wake 처리 구현
- 관측과 inspect를 위한 correlation metadata 연결
- mailbox adapter 범위를 Slack, Discord, Telegram, Email 네 채널로 제한
- Email mailbox adapter는 이미 추출된 메시지 필드 정규화까지만 포함하고 IMAP/SMTP/MIME/network/provider-specific API는 제외

## 범위 제외

- 특정 벤더 큐나 cron 선택
- Slack, Discord, Telegram, Email 바깥의 추가 채널 adapter
- 관리자 inbox UI
- 멀티노드 스케줄러 합의
- 외부 조직용 webhook 운영 시스템

## 현재 구현 상태

### 이미 반영된 것

- queue, scheduler, mailbox, hooks, background worker service command envelope와 dedupe 경계가 core service 모델에 구현돼 있다.
- service reentry는 fact-only command로 처리되며 duplicate delivery, stale wake, metadata loss 이후 current turn 보호, non-mailbox dedupe marker replay가 검증된다.
- Slack, Email adapter는 network-free normalizer 또는 strict approval parser 범위로 구현돼 있고, Telegram과 Discord는 CLI one-shot polling connector를 통해 같은 mailbox 경계로 라우팅된다.
- accepted service/mailbox events는 `service_correlation_id`를 observability projection으로 보존한다.

### 아직 남은 것

- 실제 Slack/Discord Gateway, hosted webhook, Telegram webhook, IMAP/SMTP polling 같은 장기 실행 provider-specific network loop는 이 PRD 범위 밖이다. Discord REST one-shot polling CLI는 현재 포함 범위다. 장기 실행 assistant channel worker와 unified runtime supervisor 설계는 `docs/specs/012-runtime-services/prds/001-channel-worker-runtime.md`에서 별도 확장 범위로 다룬다.
- service metadata와 session truth 경계는 유지되지만, 장기 운영용 metadata storage 고도화는 아직 별도 확장 범위다.

### 로컬 근거

- `crates/shacs-core/src/core/service.rs`
- `crates/shacs-core/tests/runtime_services.rs`
- `crates/shacs-core/tests/mailbox_adapter.rs`
- `crates/shacs-runtime-adapters/src/slack.rs`
- `crates/shacs-runtime-adapters/src/discord.rs`
- `crates/shacs-runtime-adapters/src/telegram.rs`
- `crates/shacs-runtime-adapters/src/email.rs`
- `crates/shacs-runtime-adapters/src/subagent.rs`

## TDD 계획

1. 서비스별 dedupe key 생성과 envelope validation 단위 테스트를 만든다.
2. 같은 key가 두 번 들어와도 turn이 다시 열리지 않는 idempotency 테스트를 추가한다.
3. scheduler, mailbox, worker가 wake command를 보내고 오케스트레이터가 resume 여부를 판단하는 통합 테스트를 추가한다.
4. service restart 뒤 duplicate delivery, stale wake, already-closed turn 재진입 테스트를 추가한다.
5. emit 금지 command를 서비스가 만들 수 없도록 타입 또는 검증 테스트를 추가한다.

## 구현 웨이브

### Wave 1. Service envelope와 emit 범위 고정

- 서비스별 command envelope 스키마와 공통 correlation 필드를 정의한다.
- queue, scheduler, mailbox, hooks, background worker가 emit 가능한 command 집합을 타입으로 제한한다.
- 세션 상태 직접 변경 command와 privileged command는 생성 단계에서 막는다.

### Wave 2. Dedupe와 metadata 경계 구현

- 서비스별 dedupe key 계산기를 구현한다.
- service-owned metadata 저장소와 session truth 저장소를 분리한다.
- 오케스트레이터 재진입 시 processed marker 검사와 idempotent 처리 경로를 구현한다.

### Wave 3. Wake and resume 경로 연결

- wake command를 단순 "확인 필요" 신호로 제한한다.
- 오케스트레이터가 replay, open turn, pending effect, stale 여부를 보고 resume 또는 ignore를 결정하게 만든다.
- mailbox approval response와 background completion도 같은 재진입 규약으로 묶는다.

### Wave 4. 재시작, 중복 전달, 관측 가능성 회귀 검증

- 서비스 재시작 후 이전 delivery 재전송을 허용하되 truth가 변하지 않게 만든다.
- late service signal과 stale wake가 inspect, diagnostics, trace에서 구분되도록 연결한다.
- duplicate delivery, missed fire, cancelled work, stale background completion 테스트를 묶는다.

## Verification Evidence

- 단위 테스트: `crates/shacs-core/tests/runtime_services.rs`의 `service_dedupe_markers_are_stable_for_non_mailbox_services`, `queue_payload_session_mismatch_is_ignored`, `queue_dedupe_key_mismatch_is_ignored`, `queue_service_kind_mismatch_is_ignored`, `hook_payload_and_dedupe_mismatches_are_ignored`, `background_worker_payload_and_dedupe_mismatches_are_ignored`, `scheduler_wake_target_session_mismatch_is_ignored`, `scheduler_dedupe_key_mismatch_is_ignored`가 dedupe key, envelope validation, metadata boundary를 검증한다.
- 단위 테스트: `queue_work_ready_is_fact_only_and_does_not_open_turn`, `queued_work_cancelled_is_fact_only_and_does_not_increment_policy_retry`, `queue_delivery_retry_attempt_does_not_increment_turn_policy_retry`가 queue delivery fact-only command acceptance, dedupe, retry-attempt isolation을 검증한다.
- 단위 테스트: `crates/shacs-core/tests/mailbox_adapter.rs`와 runtime adapter unit tests가 Email extracted-field normalizer, strict approval parser, mailbox ingress/approval envelope 변환을 검증한다.
- 통합 테스트: `crates/shacs-core/tests/runtime_services.rs`의 `scheduler_missed_and_cancelled_wakes_are_fact_only`, `mailbox_message_rejected_is_fact_only_without_context_append`, `mailbox_approval_response_resolves_pending_approval_without_context_append`, `hook_observation_is_fact_only_and_deduped`, `hook_failure_is_fact_only_and_deduped`, `background_job_completion_is_fact_only_for_active_turn`, `background_worker_terminal_failures_are_fact_only_for_active_turn`, `background_worker_wake_request_is_fact_only_without_opening_turn`이 scheduler/mailbox/worker reentry, queue delivery retry, hooks observation/failure flow, background worker fact-only envelope를 검증한다.
- 통합 테스트: accepted service/mailbox events preserve `service_correlation_id` for observability projection
- 내구성 테스트: `replay_restores_non_mailbox_service_dedupe_markers`, `duplicate_scheduler_fire_is_idempotent`, `duplicate_queue_delivery_attempt_is_idempotent`, `duplicate_mailbox_message_does_not_append_twice`, `duplicate_mailbox_approval_response_is_idempotent`, `stale_scheduler_session_wake_after_metadata_loss_does_not_abort_current_turn`, `crates/shacs-core/tests/mailbox_adapter.rs`의 `surface_channel_ingress_persists_dedupe_across_resume`, `surface_channel_approval_response_persists_dedupe_across_resume_without_context_append`가 duplicate delivery, stale wake, restart after pending service signal, service metadata loss 이후 session-level stale wake가 current turn을 abort하지 않는 경계를 검증한다.
- 안전성 테스트: privileged command bypass 불가, closed turn reopen 방지
- Spec016 matrix 증거: `crates/shacs-contracts/src/verification.rs`가 Spec012 `Unit`, `Integration`, `DurabilityRecovery`를 `CoverageLevel::FullSpec` / `CoverageStatus::Verified`로 선언하고, `crates/shacs-core/tests/verification_matrix.rs`의 `spec012_full_spec_evidence_covers_required_families`가 이를 검증한다.

## Open Risks

- 서비스 메타데이터와 truth 저장소의 경계가 흐리면 replay correctness가 깨질 수 있다.
- dedupe key 설계가 약하면 다른 이벤트를 중복으로 잘못 묶을 수 있다.
- restart 직후 stale wake 폭주가 있으면 diagnostics는 많아지지만 실제 상태는 안 바뀌는 상황을 잘 설명해야 한다.
- 참고 메모: service reentry의 dedupe/stale 판단은 007의 ingress arbitration과 shared correlation 계약에 의존하므로, 서비스 레벨에서 우선순위를 독자 정의하면 안 된다.

## 종료 기준

- 모든 runtime service 결과가 command envelope로만 재진입한다.
- 서비스 메타데이터가 없어도 session truth replay가 가능하다.
- duplicate delivery와 stale wake가 truth를 재적용하거나 닫힌 턴을 되살리지 않는다.
- emit 금지 command를 서비스가 만들 수 없거나 즉시 거절된다.
- 012와 016이 요구하는 단위, 통합, 내구성 검증 증거가 확보된다.
