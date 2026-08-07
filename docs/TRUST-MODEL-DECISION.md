# Trusted local agent runtime 전환 결정

Date: 2026-08-07

Status: Accepted

## 결정

Shacs의 기본 실행 모델을 중앙 permission-safety 계약 중심에서 Prime Agent와 같은 trusted local agent runtime으로 전환한다.

사용자는 workspace, instruction, skill, extension, package source를 직접 선택하고 신뢰하는 주체다. 활성 resource와 model-generated Python·shell·project command는 선택적 sandbox가 적용되지 않으면 현재 사용자 OS 권한으로 실행될 수 있다.

## 채택하는 것

1. 기존 Spec 025 `tool:before` hook을 실행 직전 veto primitive로 승계한다.
2. Durable approval 대신 현재 호출에만 적용되는 ephemeral confirmation을 사용한다.
3. 공통 process envelope 대신 adapter별 timeout, abort, cleanup, readiness를 사용한다.
4. Local auth store, environment, literal, command-backed credential source를 허용한다.
5. Sandbox는 adapter별 선택 기능으로 두고 active/fallback 범위를 표시한다.
6. Markdown skill, Python skill, in-process extension을 서로 다른 resource kind로 표시하되 활성화 뒤에는 trusted input/code로 취급한다.
7. Session, log, trace, tool output에 raw content가 남을 수 있음을 사용자에게 공개한다.

## 제거하는 목표

- Unified policy/safety snapshot
- 중앙 permission mode와 capability ceiling
- Durable approval correlation, remembered rule, exact retry contract
- Universal process envelope과 containment inheritance proof
- Typed SecretRef와 전 표면 redaction provenance
- Permission classifier accounting
- Skill content의 non-grant authorization gate

기존 구현 타입과 닫힌 spec evidence는 호환성 또는 해당 owner의 현재 baseline으로 남을 수 있다. 그러나 030의 future owner나 closure blocker로 승계하지 않는다.

## Activation 결정

1. Builtin과 명시적으로 configured resource는 활성 후보가 된다.
2. Project-local auto-discovered resource는 workspace가 trusted로 확인된 경우에만 활성화한다.
3. 발견 사실만으로 untrusted workspace의 executable resource를 실행하지 않는다.
4. 활성화된 Python skill과 extension은 process-local trusted code다.

## Sandbox fallback 결정

1. `trusted_native_fallback`이 기본 profile이다. Sandbox disabled, unsupported, initialization failure 시 경고와 diagnostics를 남기고 native 실행을 계속할 수 있다.
2. 사용자가 `sandbox_required`를 선택하면 sandbox가 active가 아닌 모든 상태에서 해당 adapter 실행을 fail closed한다.
3. 기존 Spec 023의 official Docker/Compose fail-closed lane은 닫힌 범위로 유지한다. 새 fallback은 그 lane을 약화시키지 않고 030이 소유하는 native/optional adapter에만 적용한다.

## 영향

- `docs/SYSTEM-FOUNDATION.md`는 trusted resource와 operational control을 상위 방향으로 사용한다.
- Spec 030은 `trusted agent runtime and operational controls`로 대체한다.
- Specs 005와 025의 닫힌 Markdown·command-backed baseline은 유지하고, executable resource와 in-process extension의 open work를 030으로 넘긴다.
- Spec 032는 install·activation·inspect·revoke lifecycle을 소유하지만 permission provenance를 만들지 않는다.
- Spec 035는 config/auth locator schema와 migration을 소유하고, raw auth store lifecycle은 030이 소유한다.
