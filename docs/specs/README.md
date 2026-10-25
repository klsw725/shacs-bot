# shacs-bot spec set overview

이 디렉터리는 `shacs-bot`의 구현 기준이 되는 아키텍처 명세(`SPEC.md`)와 하위 실행 문서(`prds/*.md`)를 모아 둔 곳이다.

사용자용 설치·설정·실행 문서는 [`../USAGE.md`](../USAGE.md)를 먼저 볼 것.

각 numbered spec은 하나의 owner 경계를 가진다. `SPEC.md`는 권한, 상태, 범위, 불변식, 금지 패턴 같은 상위 계약을 고정하고, `prds/*.md`는 그 계약을 실제 구현 웨이브와 검증 기준으로 내린다.

## 현재 제품 범위 요약

스펙 세트 전체는 사용자가 직접 운영하는 self-hosted / personal-use assistant runtime을 기본으로 한다. [제품 의도](../../intent.md)의 중심은 사용자가 맡긴 일을 이어받고 의미 있는 변화에 근거 있는 도움을 먼저 가져오는 경험이다. 하네스, 도구와 채널은 이를 위한 수단이다. 설치형 AI app은 [043](043-app-capability-retirement/SPEC.md)에서 퇴역했다. 공식 인터페이스 표면은 CLI, TUI, local API이며, 외부 채널은 Slack, Discord, Telegram, Email, WhatsApp bridge를 지원 대상으로 본다. 제품 방향은 기존 구현의 완료 선언이 아니다.

## 상태 체계와 2026-07-20 migration

- `001`부터 `031`은 실제 Rust 구현과 테스트가 존재하는 현재 범위로 완료 처리했다. `029`와 `031`은 각 owner 범위를 `Complete (Scoped)`로 닫았고, 최종 full workspace gate/manual review evidence는 별도 record로 남긴다.
- `Complete (Scoped)`는 해당 owner가 정의한 현재 구현 범위가 닫혔음을 뜻한다.
- Source-bound release seal은 재현 가능한 명령으로 생성하는 로컬 `.omo` evidence이며 source로 추적하지 않는다. 해당 manifest가 현재 checkout에 없거나 검증되지 않으면 그 checkout에는 local final seal이 없는 것으로 판정한다.
- `Complete (Baseline)`은 self-hosted/local 최소 기준이 닫혔음을 뜻한다.
- `Open (implemented, closure blocked)` 또는 PRD의 `Implemented, closure blocked`는 구현과 제한된 검증 기록이 존재하지만 release closure 조건이 아직 통과하지 않았음을 뜻한다. 과거 QA 기록만으로 현재 소스의 QA나 전체 승인까지 주장하지 않는다.
- `Planned`는 accepted contract이지만 구현·surface evidence가 아직 없는 범위다. 기존 implemented evidence로 완료 처리하지 않는다.
- `Planned (Decision only)`는 삭제나 변경을 미리 승인한 상태가 아니다. 사용 경로·자료·대체 검증의 근거로 유지/축소/제거를 결정할 범위이며, 결정이 내려져도 실제 코드 변경은 별도다. 결정문과 필요한 증거가 갖춰지면 `Complete (Decision)`으로 닫을 수 있고, 이는 기능 구현 완료를 뜻하지 않는다.
- `Planned revision (implemented baseline)`은 이전 계약의 구현 evidence는 존재하지만, 새로 확장된 계약은 아직 검증되지 않았다는 뜻이다.
- 기존 문서의 미구현 accepted work는 후속 owner spec으로 이관했다. `028`부터 `031`은 scoped implementation을 완료했고, `032`부터 `034`는 아래 표에 완료 범위를 기록한다. 미완료였던 `035` owner 문서는 현재 스펙 세트에서 제거했으며, 구현된 기능 자체와 보존된 실행 증거의 상태를 완료로 재분류하지 않는다. 새 제품 경험은 역사적 `035` 번호를 재사용하지 않고 `036`부터 독립적인 계약으로 정의한다.
- 기존 문서 본문의 future 문구는 역사와 설계 근거로 남기되 `035`로 향하던 이관 링크는 제거했다. 당시 원문은 [비활성 보관 자료](../archive/spec035/README.md)에 있으며 현재 스펙 세트의 owner가 아니다.
- SaaS/admin/fleet, 멀티유저 조직 운영, remote marketplace, complete kernel isolation 같은 명시적 비목표는 신규 backlog로 승격하지 않았다.

## 현재 numbered owner specs

| Spec | Owner scope | Origin / consumes |
|---|---|---|
| `032-app-maker-runtime-and-extension-lifecycle` | `Complete (Scoped)`: foreground AppSupervisor/process lifecycle, current-user authoring/apply/install, app-level activation provenance and blockers. 이 범위는 [043](043-app-capability-retirement/SPEC.md)에서 퇴역했고 문서는 역사 기록으로 남음 | 005, 017, 021, 025 |
| `033-evaluation-automation-live-integration` | `Complete (Scoped)`: local goal/evaluator, durable automation routing, CAS self-improvement, recorded-only replay, domain projection input과 source-bound release evidence 구현; QA/goal/code/security/docs final PASS 및 final source-bound release execution PASS | 009, 012, 013, 014, 016, 018, 022 |
| `034-generated-media-and-rich-file-context-expansion` | `Complete (Scoped)`: Codex image event, edit/mask/variation 계약, streaming/remote-output 정책, generated artifact, bounded video analyzer와 media-domain evidence; [remediation PASS](../../.omo/evidence/spec034/remediation/PASS.json)와 [Todo 14 QA baseline](../../.omo/evidence/spec034/task-14-final-qa-candidate2/PASS.json)에서 22/22 검증. 최신 source-bound release 상태는 [local canonical final manifest](../../.omo/evidence/spec034/task-15-closure/final-committed/manifest.json)가 현재 committed source와 최종 5개 리뷰를 결합해 판정하며 이전 candidate receipt로 대체하지 않음. video analyzer·분석 기록 표면과 이에 근거한 9개 요구사항은 [044](044-video-analyzer-retirement/SPEC.md)에서 퇴역했고 runner는 남은 13개만 현재 coverage로 보고 | 004, 019, 027 |
| [`036-entrusted-work-continuity-and-change-notification`](036-entrusted-work-continuity-and-change-notification/SPEC.md) | `Complete (Scoped)`: 명시적 위임·조건 수정·goal-bound 관측·조건별 변화 통지와 사용자 결정의 로컬 Email 수직 흐름. 네 PRD 및 통합 Oracle PASS; 전체 출시 seal 아님 | 029, 030, 031, 033 |
| [`037-task-specific-assistant-methods`](037-task-specific-assistant-methods/SPEC.md) | `Complete (Scoped)`: 한 요청의 수행 방식 선택·사용자 변경, 계산·보고서·이미지의 결과 확인, 복합 요청의 부분별 판정과 답 우선 표면. 다섯 PRD 및 통합 Oracle PASS([종료 증거](037-task-specific-assistant-methods/SPEC.md#종료-증거-2026-10-23)); 전체 출시 seal 아님. 024 실행 엔진 재소유 없음 | 004, 005, 024, 030, 034 |
| [`038-evidence-backed-method-learning`](038-evidence-backed-method-learning/SPEC.md) | `Complete (Scoped)`: 한 대화의 근거에서 비활성 방법 후보를 만들고, 사용자 확인이나 서로 다른 두 번째 작업으로 확인하며, 맞는 요청에 버전과 함께 재사용하고 수정·사용 중지·이전 버전 복원을 한다. 세 PRD 및 스펙 차원 Oracle PASS([종료 증거](038-evidence-backed-method-learning/SPEC.md#종료-증거-2026-10-24)); 전체 출시 seal 아님. 저장 파일은 인증된 기록이 아님 | 005, 018, 024, 030, 033 |
| [`039-app-capability-disposition`](039-app-capability-disposition/SPEC.md) | `Complete (Decision)`: app authoring/lifecycle을 **제거**로 판정([판정](039-app-capability-disposition/SPEC.md#판정-결과-2026-10-24)), 독립 검토 2차 PASS. 어시스턴트 경험과 연결이 없고 실제 설치가 없으며, 격리 실제 표면에서 복구 경로가 성립하지 않았다. 사용자 자료는 보존하고 `apps`는 읽기 전용 퇴역 안내로 바꾸는 구현은 [043](043-app-capability-retirement/SPEC.md)이 했다 | 021, 030, 032 |
| [`040-video-analyzer-capability-disposition`](040-video-analyzer-capability-disposition/SPEC.md) | `Complete (Decision)`: 선택적 video analyzer를 **제거**로 판정([판정](040-video-analyzer-capability-disposition/SPEC.md#판정-결과-2026-10-24)), 독립 검토 3차 PASS. production 주입 경로가 없고(실제 사용은 미확인), 시험 주입에서 표면이 모델 입력과 어긋났으며, 제품이 쓰지 않는 분석 기록 파일 하나의 손상이 `runtime inspect`·진단 묶음과 외부 채널 답장을 막았다. 영상 첨부 저장과 `unsupported` note는 남기고 analyzer·분석 기록 표면을 빼는 구현은 [044](044-video-analyzer-retirement/SPEC.md)가 했다 | 027, 030, 031, 034 |
| [`041-release-evidence-tooling-disposition`](041-release-evidence-tooling-disposition/SPEC.md) | `Planned (Decision only)`: Spec031 release runner의 소비자·증거/호환 계약을 조사해 유지·축소·제거 판정 | 016, 031, 033, 034 |
| [`042-conversational-web-watch-and-trustworthy-silence`](042-conversational-web-watch-and-trustworthy-silence/SPEC.md) | `Planned`: 대화로 맡긴 웹페이지 한 곳의 관찰, 사용자와 함께 정한 조건의 변화와 확인 실패·회복·공백·결정 마감 알림. 036 완료 범위 재판정 없음 | 010, 029, 030, 031, 033, 036 |
| [`043-app-capability-retirement`](043-app-capability-retirement/SPEC.md) | `Complete (Scoped)`: 039 판정대로 `apps`를 읽기 전용 퇴역 안내로 바꾸고, readiness·Tasks·agent 실행 기록에서 앱 의존을 빼고, `shacs-app`과 앱 전용 코드·테스트를 지웠으며 spec030·spec031(v5)·spec033 runner를 맞췄다. 세 PRD와 스펙 차원 Oracle PASS([종료 증거](043-app-capability-retirement/SPEC.md#종료-증거-2026-10-25)); 전체 출시 seal 아님. 남은 앱 자료는 지우거나 고쳐 쓰지 않음 | 030, 031, 039 |
| [`044-video-analyzer-retirement`](044-video-analyzer-retirement/SPEC.md) | `Complete (Scoped)`: 040 판정대로 분석 기록을 읽던 inspect·diagnostics·API·WebSocket·TUI·채널을 기록이 없을 때의 결과로 고정하고, 영상 첨부는 고정 `unsupported` 안내로 남기며, analyzer·분석 기록·replay 코드와 테스트를 지우고 034·Spec031 runner를 맞췄다. 세 PRD와 스펙 차원 Oracle PASS([종료 증거](044-video-analyzer-retirement/SPEC.md#종료-증거-2026-10-25)); 전체 출시 seal 아님. 남은 첨부·기록은 지우거나 고쳐 쓰지 않음 | 027, 031, 034, 040 |

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
30. `030-trusted-agent-runtime-and-operational-controls`
31. `031-configuration-runtime-layout-and-execution-snapshots`
32. `032-app-maker-runtime-and-extension-lifecycle`
33. `033-evaluation-automation-live-integration`
34. `034-generated-media-and-rich-file-context-expansion`
35. 이전 `035` owner는 [비활성 보관 자료](../archive/spec035/README.md)로 이동했으며 새 구현 owner가 아니다.
36. `036-entrusted-work-continuity-and-change-notification`
37. `037-task-specific-assistant-methods`
38. `038-evidence-backed-method-learning`
39. `039-app-capability-disposition`
40. `040-video-analyzer-capability-disposition`
41. `041-release-evidence-tooling-disposition`
42. `042-conversational-web-watch-and-trustworthy-silence`
43. `043-app-capability-retirement`
44. `044-video-analyzer-retirement`

## 운영 원칙

- 새 요구사항은 먼저 owner spec에 반영하고, 필요하면 해당 PRD에 실행 범위와 검증 기준을 추가한다.
- 교차 문서 개념은 한 문서가 의미를 소유하고, 다른 문서는 그것을 소비하는 구조를 유지한다.
- 번호가 낮은 스펙의 기존 계약은 소비할 수 있지만, 각 스펙의 수용 조건과 완료 판정은 번호가 높은 스펙의 구현·완료에 의존하지 않는다. 독립적으로 닫을 수 없는 범위는 현재 스펙 안에 포함하거나 별도의 완결된 계약으로 다시 나눈다.
- 구현이 문서와 충돌하면 코드를 밀어붙이기보다 문서 계약부터 다시 점검한다.
- `001`부터 `027`의 완료 범위를 확장해야 하는 요구는 기존 문서를 다시 열지 않고 `028` 이후 owner spec에서 다룬다.
