# 030. trusted agent runtime and operational controls 아키텍처 명세

Status: Complete (Scoped)

Origin specs: 004, 005, 007, 010, 011, 022, 023, 025

## 문서 목적

이 문서는 self-hosted 사용자가 직접 선택한 workspace, instruction, skill, extension을 신뢰하고 에이전트가 사용자 OS 권한으로 실행하는 runtime 계약을 정의한다.

030은 중앙 permission engine이나 보안 sandbox를 제품의 기본 전제로 삼지 않는다. 대신 실행 직전 extension hook, 경로별 timeout·abort·cleanup, credential lifecycle, 선택적 sandbox adapter, resource provenance와 사용자 가시 disclosure를 조합한다.

핵심 문장:

```text
Shacs는 신뢰된 로컬 agent runtime이다. 안전 경계는 숨은 보장이 아니라 실행 전 veto, 운영 제어, 선택적 격리, 정확한 disclosure로 구성한다.
```

## 신뢰 모델

1. 사용자는 workspace, local instruction, skill, extension, package source를 직접 선택하고 검토하는 기본 주체다. Executable resource activation은 명시 config 또는 trusted workspace assertion을 요구한다.
2. Model-generated Python, shell command, project command, activated executable skill과 extension은 별도 sandbox가 활성화되지 않으면 현재 사용자 OS 권한으로 실행될 수 있다.
3. Workspace instruction과 skill content는 model behavior를 바꾸는 trusted input이다. 이 문서는 이를 비권한 데이터로 재분류하지 않는다.
4. Extension은 trusted process-local code다. Tool, command, context, provider request, resource discovery를 변경할 수 있다.
5. Daemon, worker, kernel, child session 분리는 lifecycle·recovery 경계다. Security sandbox, UID 분리, filesystem·network containment proof가 아니다.
6. Sandbox는 adapter별 선택 기능이다. 활성 범위와 fallback 상태를 사용자에게 표시해야 하며 전체 runtime 보장으로 광고하지 않는다.

## 소유하는 범위

030은 다음을 소유한다.

1. Spec 025에서 닫힌 `tool:before` hook의 실행 직전 veto와 block reason 전달 계약을 승계하고 trusted in-process extension에 확장하는 계약.
2. UI confirm/select와 headless confirmation fallback의 최소 동작.
3. Bash, generic exec, package operation, kernel, daemon worker, MCP adapter의 경로별 lifecycle control과 명시적 비통합 상태.
4. Timeout, abort, process-tree cleanup, startup readiness, output bounding 같은 operational control.
5. Credential source resolution, local auth persistence, refresh serialization, status projection과 source fingerprint.
6. Optional sandbox adapter의 활성 범위, 설정, disabled/unsupported/failed 상태와 fallback disclosure.
7. Skill, extension, prompt, context, package의 discovery·precedence·load diagnostics와 trusted-code disclosure.
8. Session, log, trace, shell output이 중앙 secret redaction을 보장하지 않는다는 데이터 disclosure.
9. CLI, TUI, local API, diagnostics가 trusted mode, active hooks, sandbox mode, credential status, resource diagnostics를 같은 의미로 표시하도록 Spec 035에 owner fact를 제공하는 계약.

## 제거된 이전 계약

다음은 더 이상 Spec 030의 목표 또는 closure requirement가 아니다.

1. 통합 `PolicySafetySnapshot` 또는 action/snapshot digest correlation.
2. 중앙 permission mode, capability ceiling, protected-target static deny precedence.
3. Approval request ID, scope, expiry, consumed state, exact retry, remembered permission rule.
4. 모든 process launch를 통과하는 universal process envelope과 공통 redacted receipt.
5. Parent-child containment inheritance 또는 equal-or-narrower permission proof.
6. Typed `SecretRef`, raw value 비지속성, 전 표면 redaction provenance.
7. Permission classifier routing·token·latency·cost accounting.
8. Skill content를 permission non-grant input으로 강제하는 trust provenance gate.
9. Verified entrypoint와 dependency preparation의 digest-bound authorization.

이 제거는 기존 Rust primitive의 즉시 삭제를 요구하지 않는다. 기존 permission, redaction, containment 타입은 다른 owner가 소비하거나 호환성 때문에 남을 수 있지만, Spec 030 closure를 막는 필수 계약으로 취급하지 않는다.

## Implementation PRDs

1. `000-trusted-runtime-profile-and-boundary.md`
2. `001-pre-tool-hooks-and-user-confirmation.md`
3. `002-path-specific-process-lifecycle-controls.md`
4. `003-auth-source-resolution-and-credential-lifecycle.md`
5. `004-optional-sandbox-adapters.md`
6. `005-resource-loading-trust-and-data-disclosure.md`
7. `006-sequential-integration-and-spec030-closure.md`

## Owner map

| 계약 | Spec 030 PRD owner | 외부 owner boundary |
|---|---|---|
| Trusted runtime profile과 disclosure | PRD 000 | Spec 035가 사용자 surface에 투영 |
| Pre-tool hook과 confirmation | PRD 001 | Tool execution 자체는 Spec 004 계열 현재 구현 소비 |
| Process lifecycle controls | PRD 002 | AppSupervisor는 032, physical layout은 031 |
| Credential lifecycle | PRD 003 | Config/profile persistence는 031 |
| Optional sandbox adapter | PRD 004 | 물리 sandbox implementation은 adapter 또는 외부 runtime |
| Resource trust와 data disclosure | PRD 005 | App/skill lifecycle은 032, projection은 035 |
| 순차 통합과 closure | PRD 006 | 031·032·035 evidence 소비 |

## Invariants

1. Core tool runner는 extension `tool:before` hook을 실제 tool 실행 전에 호출한다.
2. 어느 handler든 block을 반환하면 해당 tool call은 실행하지 않고 reason을 tool failure와 사용자 surface에 전달한다.
3. Confirmation은 호출 시점의 ephemeral decision이다. Durable approval, remembered allow, replay authorization으로 표현하지 않는다.
4. Headless 환경에서 interactive confirmation을 요구하는 hook은 자동 allow하지 않는다.
5. Process lifecycle control은 adapter별로 정의한다. 공통 gate가 존재한다고 주장하지 않는다.
6. Timeout과 abort는 가능한 경로에서 descendant cleanup을 시도하지만 kernel isolation이나 side-effect rollback을 보장하지 않는다.
7. Native bash, Python kernel, extension code와 package operation은 선택적 sandbox가 없으면 현재 사용자 권한과 환경을 사용할 수 있다.
8. Sandbox status는 `active`, `disabled`, `unsupported`, `failed`를 구분한다. `active`만 해당 adapter 범위의 sandbox evidence다.
9. `trusted_native_fallback` profile에서는 sandbox 초기화 실패 후 경고와 함께 native fallback이 가능하다. `sandbox_required` profile에서는 active가 아닌 sandbox 상태를 fail closed한다.
10. Credential status projection은 raw credential을 표시하지 않는다. Auth backend 자체는 local raw credential을 저장할 수 있다.
11. Session JSONL, log, trace, tool output과 extension data에는 사용자가 입력하거나 실행이 반환한 민감 정보가 남을 수 있다. 중앙 secret-safe projection을 보장하지 않는다.
12. Skill과 extension의 path, source, precedence, digest, parse·collision 상태는 provenance와 diagnostics다. 실행 권한 증명이 아니다.
13. Builtin·명시 configured resource와 trusted workspace의 auto-discovered resource만 활성 후보다. 활성화된 Python skill과 extension은 trusted code로 취급한다.
14. Untrusted source는 활성화하지 않거나 별도 sandbox 또는 별도 OS account에서 실행한다.
15. Daemon token, worker generation, kernel protocol, session directory는 lifecycle·ownership evidence이며 OS-level containment evidence가 아니다.

## Must Have

1. `tool:before` hook은 tool name, call id, validated input을 받고 block reason을 반환할 수 있어야 한다.
2. UI surface는 confirm/select/notify를 제공하고 headless fallback을 명시해야 한다.
3. Bash와 generic exec는 cwd validation, timeout 또는 abort, bounded output, terminal outcome을 제공해야 한다.
4. Daemon과 worker는 stale generation fencing, startup readiness, crash/restart cleanup을 제공해야 한다.
5. 각 process adapter는 실제 제공하는 timeout, env, cwd, cleanup 범위를 문서화해야 한다.
6. Credential resolution은 runtime override, environment, local auth store, provider config의 precedence를 결정적으로 적용해야 한다.
7. Local auth persistence는 파일 권한, atomic write 또는 lock, refresh serialization, status-only inspection을 제공해야 한다.
8. Optional sandbox는 적용 adapter, filesystem/network 정책, unsupported platform과 failure fallback을 표시해야 한다.
9. Resource loader는 source path, precedence, collision, parse error를 진단하고 어떤 resource가 선택되었는지 표시해야 한다.
10. Python skill과 extension 실행 전 trusted-code disclosure를 사용자 문서와 inspect surface에 제공해야 한다.
11. Session·log·trace export surface는 raw content 가능성과 외부 전송 여부를 명시해야 한다.
12. CLI/TUI/API diagnostics는 trusted runtime status, hook denial, process control, sandbox mode, credential status, resource load issue를 owner fact에서 투영해야 한다.

## Must Not Have

1. Daemon, worker, kernel, child session을 security sandbox로 광고하지 않는다.
2. Optional bash sandbox를 Python, MCP, package manager, extension, 모든 child process의 universal containment로 확대 해석하지 않는다.
3. Confirmation boolean을 durable approval receipt나 replay authorization이라고 부르지 않는다.
4. Tool call id를 action digest 또는 immutable safety snapshot으로 설명하지 않는다.
5. Auth fingerprint를 typed secret reference, redaction proof, exfiltration prevention으로 설명하지 않는다.
6. Timeout·kill을 transaction rollback 또는 side-effect prevention으로 설명하지 않는다.
7. Resource hash와 release checksum만으로 executable-resource activation evidence가 완성된다고 설명하지 않는다.
8. Parse·collision diagnostics를 malicious content detection으로 설명하지 않는다.
9. Session, log, trace가 secret-safe하다고 기본 주장하지 않는다.
10. 조직 RBAC, fleet policy, 관리자 승인 console, 중앙 vault를 기본 제품 흐름으로 도입하지 않는다.

## Acceptance Criteria

1. 실제 tool call이 hook block 시 실행되지 않고 call id와 reason이 model·사용자 surface에 전달된다.
2. UI confirmation allow/deny와 headless deny가 실제 tool surface에서 검증된다.
3. Bash와 generic exec의 success, timeout, abort, descendant cleanup, invalid cwd가 실제 process QA로 검증된다.
4. Package, kernel, daemon, MCP 경로가 공통 process gate를 거친다고 주장하지 않고 각 adapter의 실제 control matrix가 문서·diagnostics에 존재한다.
5. Auth source precedence, local persistence permission, refresh lock, stale fingerprint, status-only inspect가 검증된다.
6. Session·log·trace에 raw content가 남을 수 있다는 disclosure와 trace opt-in 상태가 사용자 surface에서 확인된다.
7. Sandbox active 상태에서 해당 adapter 정책이 적용되고 disabled, unsupported, initialization failure에서는 `trusted_native_fallback` 경고 또는 `sandbox_required` 실행 거부가 나타난다.
8. Skill·extension discovery가 source, precedence, collision, parse diagnostics를 제공하며 trusted-code disclosure가 함께 표시된다.
9. Local project resource가 builtin/package resource보다 우선할 수 있는 precedence가 테스트와 inspect output에 나타난다.
10. 035 projection은 owner fact가 없을 때 안전을 추론하지 않고 `unknown` 또는 `unavailable`로 표시한다.
11. 문서와 release evidence는 제거된 이전 계약을 현재 또는 future Spec 030 guarantee로 다시 주장하지 않는다.

## External dependency gates

1. Spec 035는 trusted runtime status와 diagnostics projection parity를 소유한다.
2. Spec 032는 app/executable-resource install, activation lifecycle, supervisor lifecycle을 소유하되 030의 trusted-code disclosure를 소비한다.
3. Spec 031은 config/profile/auth locator persistence와 runtime layout을 소유하되 030의 raw credential lifecycle을 재정의하지 않는다.

## Closure Evidence

Spec 030은 PRD 000부터 005의 실제 runtime evidence와 PRD 006의 통합 gate를 통과해 `Complete (Scoped)`로 닫혔다. Closure는 보안 sandbox나 secret prevention을 증명하는 절차가 아니다. Trusted runtime의 실제 동작, 선택적 제어 범위, fallback과 비보장을 사용자가 정확히 이해할 수 있음을 증명하는 절차다.

현재 구현은 `Spec030RuntimeProjection`, live trusted runtime owner facts, validated deterministic `tool:before`, process-local JS/TS trusted hook host, controlled child, production credential resolution과 cancellation, adapter-scoped sandbox, trusted resource inspection, CLI/TUI/API surface와 source-bound `spec030-release-runner`를 제공한다. Workspace Cargo gates, active owner surface QA, tamper-resistant evidence validation과 Linux `SHACS_REQUIRE_BWRAP=1` bwrap lifecycle lane을 closure evidence로 사용한다.
