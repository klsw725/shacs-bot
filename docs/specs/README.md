# shacs-bot spec set overview

이 디렉터리는 `shacs-bot`의 구현 기준이 되는 아키텍처 명세(`SPEC.md`)와 하위 실행 문서(`prds/*.md`)를 모아 둔 곳이다.

사용자용 설치·설정·실행 문서는 [`../USAGE.md`](../USAGE.md)를 먼저 볼 것.

각 numbered spec은 하나의 owner 경계를 가진다. `SPEC.md`는 권한, 상태, 범위, 불변식, 금지 패턴 같은 상위 계약을 고정하고, `prds/*.md`는 그 계약을 실제 구현 웨이브와 검증 기준으로 내린다.

## 현재 제품 범위 요약

스펙 세트 전체는 self-hosted / personal-use assistant runtime을 기본으로 하며, 사용자가 설치한 AI app을 실행·관찰·복구하는 개인용 AI Operating System을 장기 제품 방향으로 둔다. 공식 인터페이스 표면은 CLI, TUI, local API다. 외부 채널은 Slack, Discord, Telegram, Email, WhatsApp bridge를 지원 대상으로 본다.

## 상태 체계와 2026-07-20 migration

- `001`부터 `029`는 실제 Rust 구현과 테스트가 존재하는 현재 범위로 완료 처리했다. `029`는 Wave 1-8 baseline을 `Complete (Scoped)`로 닫았고, 최종 full workspace gate/manual review evidence는 별도 record로 남긴다.
- `Complete (Scoped)`는 해당 owner가 정의한 현재 구현 범위가 닫혔음을 뜻한다.
- `Complete (Baseline)`은 self-hosted/local 최소 기준이 닫혔음을 뜻한다.
- 기존 문서의 미구현 accepted work는 `029`부터 `035`의 owner spec으로 이관했다. `028`과 `029`는 scoped implementation을 완료했고, 남은 open owner table은 `030`부터 `035`를 따른다.
- 기존 문서 본문의 future 문구는 역사와 설계 근거로 보존하지만, 구현 owner는 각 문서 상단의 `Open work moved to:` ledger와 새 spec을 따른다.
- SaaS/admin/fleet, 멀티유저 조직 운영, remote marketplace, complete kernel isolation 같은 명시적 비목표는 신규 backlog로 승격하지 않았다.

## 현재 Open owner specs

| Spec | Owner scope | Origin specs |
|---|---|---|
| `030-policy-permission-redaction-and-containment-model` | formal policy/safety snapshot, capability, approval, redaction, containment | 004, 007, 010, 011, 022, 023 |
| `031-ui-projection-diagnostics-and-release-evidence-parity` | shared projection, TUI/REPL/wizard, diagnostics parity, release runner | 001, 011, 012, 013, 014, 016, 021, 023, 025, 026, 027 |
| `032-app-maker-runtime-and-extension-lifecycle` | app supervisor, process lifecycle, App Maker apply/install, extension provenance | 005, 017, 021, 025 |
| `033-evaluation-automation-live-integration` | evaluator, automation, self-improvement, replay의 live product integration | 009, 012, 013, 014, 016, 018, 022 |
| `034-generated-media-and-rich-file-context-expansion` | Codex image, edit/streaming/remote output, video projection/analyzer | 004, 019, 027 |
| `035-configuration-runtime-layout-and-execution-snapshots` | config migration, profiles, runtime layout, execution snapshots, context wiring | 008, 009, 010, 015, 026 |

## 읽는 순서 권장

처음 읽는다면 `docs/SYSTEM-FOUNDATION.md`를 먼저 보고, 그 다음 numbered spec을 순서대로 읽는 것이 가장 안전하다.

0. `../SYSTEM-FOUNDATION.md`
1. `001-session-kernel`
2. `002-command-event-effect`
3. `003-provider-runtime`
4. `004-tool-runtime`
5. `005-skill-system`
6. `006-session-store`
7. `007-main-orchestrator-policy`
8. `008-configuration-profiles-and-runtime-layout`
9. `009-context-assembly-and-compaction-input`
10. `010-host-safety-permissions-and-secrets`
11. `011-subagent-runtime`
12. `012-runtime-services`
13. `013-user-interfaces-and-session-ux`
14. `014-observability-diagnostics-and-inspection`
15. `015-packaging-process-lifecycle-and-upgrades`
16. `016-verification-matrix-and-release-gates`
17. `017-app-operating-environment`
18. `018-evaluation-automation-and-self-improvement`
19. `019-image-generation-and-generated-media`
20. `020-tool-search-and-provider-tool-surface`
21. `021-app-maker-and-app-authoring`
22. `022-auto-approval-permissions`
23. `023-zero-setup-sandbox-execution`
24. `024-dynamic-workflows-and-harness-orchestration`
25. `025-user-extensible-hooks-and-plugins`
26. `026-context-files-and-inline-references`
27. `027-channel-attachment-intake-and-file-context`
28. `028-formal-execution-reentry-and-outcome-contracts`
29. `029-durable-runtime-recovery-and-data-migration`
30. `030-policy-permission-redaction-and-containment-model`
31. `031-ui-projection-diagnostics-and-release-evidence-parity`
32. `032-app-maker-runtime-and-extension-lifecycle`
33. `033-evaluation-automation-live-integration`
34. `034-generated-media-and-rich-file-context-expansion`
35. `035-configuration-runtime-layout-and-execution-snapshots`

## 운영 원칙

- 새 요구사항은 먼저 owner spec에 반영하고, 필요하면 해당 PRD에 실행 범위와 검증 기준을 추가한다.
- 교차 문서 개념은 한 문서가 의미를 소유하고, 다른 문서는 그것을 소비하는 구조를 유지한다.
- 구현이 문서와 충돌하면 코드를 밀어붙이기보다 문서 계약부터 다시 점검한다.
- `001`부터 `027`의 완료 범위를 확장해야 하는 요구는 기존 문서를 다시 열지 않고 `028` 이후 owner spec에서 다룬다.
