# packaging, process lifecycle, and upgrades 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/006-session-store/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`, `docs/specs/012-runtime-services/SPEC.md`, `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`, `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 바탕으로 `shacs-bot`의 self-hosted 설치, 프로세스 수명주기, 패키징, 업그레이드, 복구 계약을 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- 사용자가 직접 install, update, start, stop, recover를 수행할 수 있는 제품 수명주기 모델을 고정한다.
- 단일 사용자 self-hosted 환경에 맞는 process model과 런타임 자산 배치를 정의한다.
- version compatibility, schema migration, interrupted upgrade 처리 규칙을 정한다.
- 업그레이드와 재시작이 session correctness를 깨지 않도록 공식 절차와 금지 패턴을 명시한다.
- Rust 구현에서 bootstrap flow, runtime marker, migration runner, upgrade guard, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 배포 편의 메모가 아니다. 구현이 이 문서와 충돌하면 "일단 실행되니까 괜찮다"는 식으로 process lifecycle과 upgrade semantics를 흐리지 말고, 사용자가 스스로 설치하고 복구할 수 있는 계약부터 다시 점검해야 한다.

이 spec의 FullSpec 완료 기준은 바이너리 하나를 빌드해 띄우는 POC가 아니라, 이 문서가 정의한 install/update/start/stop/recover lifecycle, process ownership, version compatibility, migration, interrupted upgrade handling을 충족하는 **완전한 기능 구현과 검증**이다. 현재 구현은 self-hosted/personal-use 로컬 lifecycle baseline을 충족하지만, 모든 migration/upgrade 제품 표면을 완성한 상태는 아니다. OS package manager, remote rolling upgrade, web installer, fleet management, SaaS updater는 명시적 비범위다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `shacs-bot`은 self-hosted / personal-use 런타임이며 사용자가 직접 설치, 실행, 업데이트, 복구해야 한다.
- `MainOrchestrator`와 session store는 세션 정확성의 중심이다.
- runtime layout은 단순하고 예측 가능해야 하며, 장애 시 로컬 파일과 진단 출력만으로 복구 방향을 파악할 수 있어야 한다.
- 목표는 운영팀이 관리하는 SaaS control plane이 아니라, 한 사용자의 머신에서 장기간 안정적으로 동작하는 assistant runtime이다.

따라서 이 문서는 중앙 배포 파이프라인, 조직별 fleet management, remote rolling upgrade, 멀티노드 control plane을 다루지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- 공식 패키징 산출물과 설치 단위
- process model과 runtime ownership
- start, stop, restart, recover lifecycle
- version compatibility, data schema version, binary/runtime compatibility
- install, update, rollback, migration 절차
- interrupted upgrade와 partial migration 처리 규칙
- 구현 불변식, 정상 시퀀스, 실패 시퀀스, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 특정 OS 패키지 매니저 지원 범위
- systemd, launchd, Docker 같은 외부 supervisor 세부 설정
- 조직 단위 자동 배포 시스템
- 웹 기반 installer
- SaaS hosted update service

---

## 현재 구현 평가

현재 확인된 구현 증거는 self-hosted/local lifecycle baseline이다.

- Cargo 기반 build/install-equivalent 경로와 Dockerfile, Docker Compose 기반 로컬 실행 표면이 있다.
- 공식 `runtime start`, `runtime stop`, `runtime restart`가 구현되어 있고, `runtime start`는 lifecycle admission/ownership 획득 후 기존 channel runtime foreground 경로를 실행한다.
- `run`, `serve`, `runtime start`는 장기 실행 loop 진입 전에 ownership marker를 획득하고 heartbeat를 갱신하며, 정상 종료 시 자신이 소유한 marker만 정리한다.
- `runtime stop`과 `runtime restart`는 active ownership을 직접 삭제하지 않고 stop-request marker를 기록한다. active owner가 없거나 stale owner만 있으면 그 상태를 보고한다.
- `runtime inspect`는 binary version, data schema version, compatibility classification, ownership status, stop request marker, update marker, capabilities, sessions를 보고한다.
- `runtime update`는 source-install workflow에서 target version이 실행 중인 binary version과 일치해야 하며, update marker를 `in_progress`에서 `completed_cleanup`으로 남긴다.
- 현재 compatibility/admission 판단은 바이너리 상수인 `RUNTIME_DATA_SCHEMA_VERSION`과 runtime marker 상태에 근거한다. current schema에서는 실제 runtime data transform이 필요 없으므로 `migration_required=false`인 no-op completion을 남긴다. 이는 current schema의 증거이며, 저장된 데이터를 실제로 변환하는 migration framework가 검증됐다는 뜻은 아니다.
- migration-required/inspect-only/incompatible/partial marker 상태는 running/mutation admission을 차단한다.
- `runtime recover`는 partial migration과 active ownership을 차단하고, partial migration이 아닌 update marker와 stale ownership marker를 안전하게 정리한다.
- diagnostics/recovery evidence는 update marker와 recovery 결과를 설명하는 수준까지 확인된다.
- ownership active/stale classification, active ownership conflict, stop/restart request observation, stale ownership recovery, compatibility/admission guard가 단위 테스트로 확인된다.

따라서 현재 상태는 self-hosted/personal-use 로컬 lifecycle baseline 완료로 본다. 전체 FullSpec 중 실제 stored-data transform migration, 더 넓은 upgrade 제품 표면, 회귀 coverage 일부는 남아 있다. OS package manager, remote rolling upgrade, web installer, fleet management, SaaS updater는 이 spec의 현재 구현 범위 밖이다.

---

## 핵심 정의

### packaging unit

packaging unit은 사용자가 설치하고 교체하는 공식 배포 단위다. 초기 기준은 Rust 바이너리와 기본 번들 자산, 예: bundled skills, 기본 schema metadata, 문서 참조를 포함하는 로컬 설치 단위다.

`shacs-bot` 자체의 packaging unit과 `.shacsapp` app bundle은 다르다. 전자는 runtime binary와 core schema를 교체하고, 후자는 해당 runtime 위에서 실행될 capability bundle을 설치한다.

### runtime instance

runtime instance는 특정 버전의 `shacs-bot` 프로세스가 특정 runtime root를 사용해 실행 중인 실제 인스턴스다.

### process lifecycle

process lifecycle은 bootstrap, running, draining, stopped, crashed, recovering, upgrading 같은 프로세스 수준 상태 변화를 뜻한다.

### data compatibility

data compatibility는 특정 바이너리 버전이 기존 config schema, session store format, checkpoint format, diagnostics artifact를 읽고 쓸 수 있는 범위를 뜻한다.

### migration

migration은 runtime data나 config schema를 새 버전 의미론에 맞게 안전하게 전환하는 절차다.

### interrupted upgrade

interrupted upgrade는 바이너리 교체, migration, restart 도중 중단되어 이전 버전도 새 버전도 완전한 정상 상태를 보장하지 못하는 상황이다.

---

## 제품 수명주기의 기본 원칙

1. 사용자는 로컬에서 install, start, stop, update, recover를 수행할 수 있어야 한다.
2. 버전 교체는 세션 truth를 손상시키지 않는 절차여야 한다.
3. migration은 설명 가능해야 하며, 실패 시 상태를 숨기면 안 된다.
4. interrupted upgrade는 감지 가능해야 하고 recover flow가 있어야 한다.
5. 런타임은 여러 프로세스가 같은 runtime root를 무질서하게 공유하는 구조를 기본으로 두면 안 된다.
6. demo 편의 때문에 compatibility 규칙을 느슨하게 만들면 안 된다.

---

## 패키징 모델

### 공식 산출물

초기 구현은 최소한 아래 산출물을 기준으로 설명 가능해야 한다.

- 실행 바이너리 또는 Cargo 기반 설치 결과물
- 번들된 기본 리소스, 예: bundled skills, 기본 문서 링크, schema metadata
- 버전 식별 정보
- 현재 바이너리가 지원하는 data schema compatibility 정보

### 패키징 규칙

1. 실행 가능한 바이너리 버전과 data schema compatibility 범위가 함께 식별 가능해야 한다.
2. 패키징 산출물은 runtime data, session data, secrets를 덮어쓰는 방식이면 안 된다.
3. 업그레이드는 설치 산출물 교체와 runtime data migration을 구분해 설명 가능해야 한다.
4. runtime 업그레이드는 installed app registry와 app ledger를 임의 삭제하거나 silent migration하면 안 된다.

---

## process model

> 현재 구현 메모: `run`, `serve`, `runtime start`는 runtime root에 대한 active/stale ownership marker를 사용한다. `runtime stop/restart`는 active owner marker를 직접 삭제하지 않고 stop-request marker를 기록한다.

### 기본 모델

초기 구현의 기본 모델은 단일 사용자, 단일 runtime root, 단일 주 런타임 인스턴스를 기준으로 한다.

의미는 다음과 같다.

- 한 runtime root에 대해 동시에 여러 주 프로세스가 같은 세션 저장소를 쓰는 것을 기본 동작으로 허용하지 않는다.
- CLI 단발 명령은 짧게 붙었다 떨어질 수 있지만, 공식 long-lived runtime instance는 ownership을 명확히 가져야 한다.
- TUI와 local API는 같은 주 런타임에 붙는 클라이언트일 수 있다.

### process ownership 규칙

1. 주 runtime instance는 runtime root에 대한 ownership marker를 가질 수 있어야 한다.
2. ownership marker는 stale 여부를 판정할 수 있어야 한다.
3. stale marker는 recover 또는 clean restart를 통해 정리 가능해야 한다.
4. ownership marker는 truth source가 아니라 lifecycle 보호 장치다.

> 참고 메모: ownership/stale/interrupted-upgrade marker는 008의 runtime layout, 014의 diagnostics surface와 함께 해석되는 교차 계약이다.
> 이 문서는 marker의 역할을 정의하지만, 정확한 저장 위치와 cleanup lifecycle은 관련 문서 사이에 더 명시적으로 정리될 여지가 있다.

---

## start, stop, restart lifecycle

> 현재 구현 메모: 명시적 lifecycle 명령은 `runtime start`, `runtime stop`, `runtime restart` 형태로 제공된다. `run`과 `serve`도 같은 ownership/admission guard를 사용하는 기존 foreground 표면이다.

### start

start는 아래 단계를 따라야 한다.

1. binary version과 runtime root 확인
2. config discovery와 secrets discovery
3. runtime root 존재 여부와 쓰기 가능성 확인
4. ownership marker와 stale marker 검사
5. data compatibility와 migration 필요 여부 검사
6. interrupted upgrade marker 검사
7. session store 및 서비스 부트스트랩
8. runtime instance running 상태 진입

### stop

stop은 아래 원칙을 따라야 한다.

1. 새 입력 수용 중지
2. 가능한 경우 진행 중 effect를 draining 또는 취소 절차로 전환
3. session truth는 event와 checkpoint 기준으로 일관되게 남김
4. shutdown reason을 observability surface에 남김
5. ownership marker 정리 또는 stale-safe 종료 기록

### restart

restart는 stop 이후 새 start로 설명 가능해야 한다.

허용되지 않는 restart:

- 열린 턴과 pending effect를 메모리에서만 이어 붙이는 opaque 재시작
- compatibility 검사 없이 바로 실행

---

## install lifecycle

install은 사용자가 처음 로컬 환경에 `shacs-bot`을 배치하는 절차다.

### install 규칙

1. install은 실행 파일 배치와 초기 runtime layout 준비를 포함해야 한다.
2. install은 기존 user data를 암묵적으로 덮어쓰면 안 된다.
3. 첫 실행 전 최소한 버전 정보, runtime root, config 위치를 설명할 수 있어야 한다.
4. install 직후 기본 start와 inspect가 가능해야 한다.

### install 이후 기대 상태

- 빈 세션 저장소 또는 초기화된 runtime 디렉터리
- config/secrets 파일 미존재여도 시작 가능한 기본값, 단 provider 사용에 필요한 최소 설정은 별도 오류로 설명
- 현재 설치 버전 확인 가능

---

## update 및 upgrade lifecycle

update는 새 버전 산출물을 받아 설치하는 행위이고, upgrade는 그 결과로 runtime data 의미론까지 새 버전으로 전환하는 전체 절차다.

### update 절차 원칙

1. 현재 실행 중 인스턴스와 대상 버전을 식별해야 한다.
2. 호환성 검사를 먼저 수행해야 한다.
3. 필요한 migration이 있으면 upgrade plan을 명시해야 한다.
4. 진행 중 세션과 열린 턴에 대한 처리 정책을 먼저 결정해야 한다.

### upgrade 절차 원칙

1. 새 바이너리 배치 전 또는 직후 compatibility marker를 기록할 수 있어야 한다.
2. migration이 필요하면 시작 전용 단계에서 수행해야 한다.
3. migration 완료 전 running 상태로 진입하면 안 된다.
4. interrupted upgrade marker가 남으면 다음 start에서 이를 감지해야 한다.

---

## version compatibility 모델

### 최소 버전 축

버전 호환성은 최소한 아래 축으로 설명 가능해야 한다.

- binary version
- config schema version
- session store format version
- checkpoint format version
- diagnostics artifact format version, 필요 시

### compatibility 규칙

1. 같은 binary version이라도 runtime data format이 다르면 compatibility 검사가 필요하다.
2. 하위 호환 읽기 가능 여부와 쓰기 가능 여부를 구분해야 한다.
3. 새 버전이 예전 format을 읽을 수 있어도, 쓴 뒤 되돌릴 수 있는지는 별도 규칙이어야 한다.
4. compatibility 불일치가 감지되면 조용히 best-effort로 실행하면 안 된다.

### compatibility 결과 범주

- fully compatible
- compatible with migration required
- read-only inspect compatible only
- incompatible, start blocked

현재 구현의 compatibility/admission은 `RUNTIME_DATA_SCHEMA_VERSION` 같은 바이너리 상수와 ownership/update/migration marker 상태를 조합해 판단한다. current schema에서는 migration이 필요하지 않아 `migration_required=false`인 no-op completion만 검증되어 있으며, 저장된 runtime data를 새 schema로 실제 변환하는 경로는 아직 검증된 제품 표면이 아니다.

---

## migration 규칙

### migration 기본 원칙

1. migration은 명시적 단계여야 한다.
2. migration 대상과 결과 버전이 기록되어야 한다.
3. migration 도중 중단되면 partial state를 감지할 수 있어야 한다.
4. migration은 session truth를 손상시키는 in-place overwrite를 기본 전략으로 삼으면 안 된다.
5. migration 완료 전 성공 실행처럼 보이면 안 된다.

### migration 범주

- config schema migration
- session metadata migration
- checkpoint format migration
- diagnostics layout migration, 필요 시

### migration과 세션 correctness

1. 열린 턴이 있는 세션에 대해 migration이 필요하면, 먼저 recovery 또는 safe stop을 거쳐 안정 상태를 만든 뒤 진행해야 한다.
2. 미완료 턴 중간 상태를 새 포맷 성공 상태로 승격하면 안 된다.
3. migration 후 replay 결과가 이전 durable truth와 모순되면 안 된다.

---

## interrupted upgrade 처리 규칙

### interrupted upgrade가 발생할 수 있는 지점

- 새 바이너리 배치 직후, migration 전
- migration 도중
- migration 완료 후 marker 정리 전
- restart 도중

### 감지 규칙

다음 start에서 최소한 아래 사실을 감지 가능해야 한다.

- upgrade marker 존재
- partial migration marker 존재
- 이전 version, target version, phase, partial migration 여부
- recovery 또는 rollback 필요 여부

### 처리 규칙

1. interrupted upgrade가 감지되면 일반 running 상태로 바로 진입하면 안 된다.
2. recover 또는 rollback 절차가 먼저 제시되어야 한다.
3. partial migration 상태에서 session store를 정상 writable로 열면 안 된다.
4. inspect-only 모드가 가능하면 제공할 수 있지만, mutation은 막아야 한다.

---

## recover lifecycle

recover는 crash, stale ownership, interrupted upgrade, partial migration, replay mismatch 같은 문제를 안정 상태로 정리하는 절차다.

### recover 규칙

1. recover는 문제 종류를 식별해야 한다.
2. recover는 어떤 marker를 정리했고 어떤 것은 정리하지 못했는지 기록해야 한다.
3. recover 후 start가 가능해졌는지, inspect-only만 가능한지 명확히 구분해야 한다.
4. recover가 세션 truth를 추측으로 보정하면 안 된다.

---

## 사용자 수명주기 동작

사용자는 최소한 아래 작업을 로컬에서 수행할 수 있어야 한다.

- 설치 버전 확인
- runtime root 확인
- start
- stop
- 상태 inspect
- update 전 compatibility 검사
- update 실행
- interrupted upgrade 또는 stale runtime recover

이 흐름은 관리자 조직을 전제로 하지 않는다. 한 사용자가 자신의 머신에서 직접 수행할 수 있어야 한다.

---

## 결정표

### 1. start 허용 결정표

| 조건 | 결정 | 비고 |
| --- | --- | --- |
| compatibility OK, stale marker 없음 | start 허용 | running 진입 가능 |
| migration 필요 | migration 단계 우선 | running 진입 전 |
| read-only inspect compatible only | inspect-only로 제한 | mutation 차단 |
| incompatible data format | start 차단 | 명시적 오류 필요 |
| interrupted upgrade marker 존재 | start 차단 또는 recover-only | 안전 우선 |
| active ownership marker 유효 | 중복 start 거절 | 단일 주 인스턴스 원칙 |

### 2. upgrade 처리 결정표

| 조건 | 결정 | 비고 |
| --- | --- | --- |
| fully compatible | binary 교체 후 restart 가능 | 추가 migration 없음 |
| migration required | safe stop 후 migration 실행 | 완료 전 running 금지 |
| read-only inspect compatible only | inspect-only 제공 가능 | mutation 차단 |
| incompatible | upgrade 차단 | 명시적 오류 필요 |

### 3. interrupted upgrade 후 처리 결정표

| 감지 상태 | 결정 | 사용자 surface |
| --- | --- | --- |
| 바이너리 교체 후 marker만 존재 | recover or continue upgrade | upgrade recovery 안내 |
| partial migration 존재 | mutation 차단 | recover 또는 rollback |
| stale ownership만 존재 | stale recover 허용 | clean restart 가능 |
| replay mismatch 동반 | deep recovery required | inspect 우선 |

---

## 정상 시퀀스 예시

### 예시 1. 정상 업데이트

1. 사용자가 update를 실행한다.
2. 현재 버전과 대상 버전, compatibility 결과가 표시된다.
3. 런타임은 safe stop으로 들어간다.
4. 필요한 migration이 수행된다.
5. migration 완료 marker가 기록된다.
6. 새 바이너리로 restart가 수행된다.
7. start 검사를 통과하고 running 상태로 진입한다.
8. inspect surface에서 새 버전과 정상 상태가 보인다.

### 예시 2. stale ownership recover 후 재시작

1. 비정상 종료 후 ownership marker가 남아 있다.
2. 다음 start에서 active process heartbeat가 없다는 사실이 확인된다.
3. 사용자는 recover를 실행한다.
4. stale marker가 정리되고 crash evidence가 기록된다.
5. 이후 clean start가 가능해진다.

---

## 실패 시나리오

### 시나리오 1. migration 도중 중단됐는데도 바로 running으로 진입

- 잘못된 동작: partial migration 상태를 무시하고 새 버전이 세션 쓰기 시작
- 올바른 동작: interrupted upgrade marker를 감지하고 recover-only 또는 inspect-only로 제한

### 시나리오 2. 새 버전이 예전 데이터 포맷을 best-effort로 덮어씀

- 잘못된 동작: compatibility 불명확 상태에서 묵시적 자동 변환
- 올바른 동작: compatibility 결과를 명시하고 migration 또는 차단 선택

### 시나리오 3. 중복 주 프로세스가 같은 runtime root를 동시에 사용

- 잘못된 동작: ownership 없이 두 인스턴스가 session store에 접근
- 올바른 동작: active ownership marker 검사로 중복 start 거절

---

## 구현 불변식

1. install 산출물과 runtime data는 구분되어야 한다.
2. compatibility 검사는 running 진입 전에 수행되어야 한다.
3. migration 완료 전에는 정상 running 상태로 들어가면 안 된다.
4. interrupted upgrade는 감지 가능해야 한다.
5. partial migration 상태에서는 mutation이 차단되어야 한다.
6. stale ownership과 active ownership은 구분 가능해야 한다.
7. upgrade는 session truth를 추측으로 보정하면 안 된다.
8. recover는 무엇을 정리했는지 observability surface에 남겨야 한다.
9. read compatibility와 write compatibility는 구분되어야 한다.
10. 단일 runtime root에 대한 주 인스턴스 ownership은 명확해야 한다.

---

## 금지 패턴

### 1. 바이너리 교체와 데이터 의미 변경을 한 번에 숨김 처리

왜 금지인가:

- 사용자가 어떤 단계에서 실패했는지 설명할 수 없게 된다.

### 2. migration 중 in-place overwrite를 기본 전략으로 채택

왜 금지인가:

- interrupted upgrade 시 복구 가능성이 크게 떨어진다.

### 3. compatibility 불일치를 경고만 하고 계속 실행

왜 금지인가:

- demo는 통과해도 실제 데이터 손상 위험이 높다.

### 4. stale marker를 무조건 삭제하고 시작

왜 금지인가:

- 실제 active instance와 충돌할 수 있다.

### 5. upgrade 실패를 runtime failure와 동일하게 취급

왜 금지인가:

- recover 절차와 사용자 안내가 달라져야 하기 때문이다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 현재 구현 증거 또는 남은 구현 항목으로 답할 수 있어야 한다.

- `BinaryVersion`, `DataSchemaVersion`, `CompatibilityResult`, `MigrationPlan`, `UpgradeMarker`, `OwnershipMarker` 같은 타입 경계가 있는가?
- start bootstrap에서 compatibility 검사, stale marker 검사, interrupted upgrade 검사를 분리된 단계로 수행할 수 있는가?
- read-only inspect compatible only 상태를 타입이나 모드로 표현할 수 있는가?
- migration이 성공, 실패, partial 상태를 명시적으로 기록할 수 있는가?
- upgrade recover가 observability evidence를 남기도록 구현할 수 있는가?
- 단일 runtime root ownership을 검사하고 stale 여부를 판정할 수 있는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

Rust 구현은 최소한 다음 성격의 테스트를 검증해야 한다. 현재 coverage가 있는 항목은 구현 평가와 PRD evidence에 기록하고, 실제 data-transform migration이나 replay 검증처럼 아직 제품 표면이 아닌 항목은 남은 coverage로 둔다.

- compatibility OK일 때만 running으로 진입하는지 확인하는 테스트
- migration required 상태에서 migration 전 running 진입이 차단되는지 확인하는 테스트
- read-only inspect compatible only 상태에서 inspect-only admission으로 제한되는지 확인하는 테스트
- incompatible data format 상태에서 start가 차단되는지 확인하는 테스트
- interrupted upgrade marker가 있으면 recover-only 또는 inspect-only로 제한되는지 확인하는 테스트
- stale ownership marker와 active ownership marker가 구분되는지 확인하는 테스트
- partial migration 상태에서 mutation command가 거절되는지 확인하는 테스트
- update 후 replay 결과가 이전 durable truth와 모순되지 않는지 확인하는 테스트
- rollback 또는 recover 이후 다시 clean start가 가능한지 확인하는 테스트

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- OS 패키지 매니저별 install script
- 컨테이너 오케스트레이션
- 원격 fleet rollout
- 중앙 자동 업데이트 서비스
- 조직 운영자용 배포 대시보드

이 항목들은 필요 시 별도 문서로 다룰 수 있다. 단, 어떤 배포 방식도 이 문서가 고정한 self-hosted lifecycle, compatibility, interrupted upgrade safety 규칙을 약화하면 안 된다.

---

## 결론

`shacs-bot`의 패키징과 수명주기는 사용자가 자신의 머신에서 직접 install, start, stop, update, recover를 수행할 수 있는 구조여야 한다. 버전 호환성과 migration은 명시적이어야 하고, interrupted upgrade와 stale runtime 상태는 감지 가능해야 하며, 어떤 경우에도 세션 truth를 추측으로 이어 붙여 정상 상태처럼 위장하면 안 된다.

핵심은 배포를 화려하게 만드는 것이 아니라, 버전 교체와 프로세스 재시작이 세션 정확성과 복구 가능성을 깨지 않도록 계약으로 고정하는 데 있다.
