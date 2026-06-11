# shacs-bot spec set overview

이 디렉터리는 `shacs-bot`의 구현 기준이 되는 아키텍처 명세(`SPEC.md`)와 하위 실행 문서(`prds/*.md`)를 모아 둔 곳이다.

사용자용 설치·설정·실행 문서는 [`../USAGE.md`](../USAGE.md)를 먼저 볼 것.

각 numbered spec은 하나의 owner 경계를 가진다. `SPEC.md`는 권한, 상태, 범위, 불변식, 금지 패턴 같은 상위 계약을 고정하고, `prds/*.md`는 그 계약을 실제 구현 웨이브와 검증 기준으로 내린다.

## 현재 제품 범위 요약

스펙 세트 전체는 아래 제품 범위를 전제로 읽어야 한다.

- 기본 성격은 self-hosted / personal-use assistant runtime이다.
- 장기 도착점은 사용자가 설치한 AI app을 실행, 관찰, 복구하는 개인용 AI Operating System이다. 이 최종 상위 계약은 `017-app-operating-environment/`가 소유한다.
- App Maker / app authoring 범위는 사용자가 app 초안, 제안, 설치 handoff를 만드는 경험이며 `021-app-maker-and-app-authoring/`가 소유한다.
- 공식 인터페이스 표면은 CLI, TUI, local API다.
- provider/auth family는 **OpenAI-compatible**, **Anthropic auth**, **Codex auth(OpenAI auth style)** 세 종류만 지원 대상으로 본다.
- 외부 채널은 **Slack**, **Discord**, **Telegram**, **Email**, **WhatsApp bridge** 다섯 가지면 충분하다고 본다.

이 범위는 특히 아래 문서에서 owner 또는 보조 경계로 다룬다.

- provider/auth 범위: 주 owner는 `008-configuration-profiles-and-runtime-layout/`, 실행 계약 보조 경계는 `003-provider-runtime/`
- channel 범위: `012-runtime-services/`, 보조 경계는 `013-user-interfaces-and-session-ux/`. one-shot mailbox connector, 초기 장기 실행 assistant channel worker, follow-up runtime waves는 `012-runtime-services/prds/000-service-reentry-and-dedup.md`, `012-runtime-services/prds/001-channel-worker-runtime.md`, `012-runtime-services/prds/002-channel-runtime-follow-up-waves.md`에서 분리해 다룬다.
- evaluation/automation/self-improvement 범위: `018-evaluation-automation-and-self-improvement/`가 goal evaluator, capability evaluator, task outcome evaluator, scheduled automation, 자기 개선, checkpoint/rollback, replay, diagnostics의 통합 최종 계약을 소유한다.
- 018 구현 상태: `018-evaluation-automation-and-self-improvement/prds/000-014`는 Rust contract/runtime helper 기준으로 구현되었고, `crates/shacs-utils`와 `crates/shacs-core`의 포맷, clippy, 관련 테스트 및 QA/목표/코드/보안/문서 재리뷰를 통과해 closed 상태다.
- image generation / generated media 범위: `019-image-generation-and-generated-media/`가 provider image generation capability, `image_generate` tool, generated image artifact 저장 계약을 소유한다.
- tool search / provider tool surface 범위: `020-tool-search-and-provider-tool-surface/`가 provider-visible tool schema progressive disclosure, deferred MCP catalog, bridge tool scope 계약을 소유한다.
- zero-setup sandbox execution 범위: `023-zero-setup-sandbox-execution/`이 사용자가 별도 host sandbox runtime을 설치하지 않아도 동작해야 하는 공식 packaging/runtime containment 계약을 소유한다.
- dynamic workflows / harness orchestration 범위: `024-dynamic-workflows-and-harness-orchestration/`가 작업별 하네스, workflow-backed subagent graph, verifier, worktree isolation, budget/resume 계약을 소유한다.
- user-extensible hooks / plugins 범위: `025-user-extensible-hooks-and-plugins/`가 사용자가 opt-in하는 plugin manifest, hook event, plugin-provided tool/skill/command, extension diagnostics 계약을 소유한다.
- context files / inline references 범위: `026-context-files-and-inline-references/`가 workspace context file discovery와 user message inline `@` reference resolution 계약을 소유한다.

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

## 운영 원칙

- 새 요구사항은 먼저 owner spec에 반영하고, 필요하면 해당 PRD에 실행 범위와 검증 기준을 추가한다.
- 교차 문서 개념은 한 문서가 의미를 소유하고, 다른 문서는 그것을 소비하는 구조를 유지한다.
- 구현이 문서와 충돌하면 코드를 밀어붙이기보다 문서 계약부터 다시 점검한다.
