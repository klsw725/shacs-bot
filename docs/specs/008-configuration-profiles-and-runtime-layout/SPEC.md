# configuration profiles and runtime layout 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/005-skill-system/SPEC.md`, `docs/specs/006-session-store/SPEC.md`를 바탕으로 `shacs-bot`의 설정 탐색 규약, profile 모델, runtime 디렉터리 레이아웃을 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- 설정 파일과 런타임 데이터가 어디에 있어야 하는지 예측 가능한 규약으로 고정한다.
- config discovery, precedence, profile selection, secrets 분리 규칙을 정의한다.
- 저장 포맷 버전과 migration 원칙을 정한다.
- self-hosted / personal-use 환경에 맞는 기본 경로와 기본 동작을 고정한다.

이 문서는 단순 예시 문서가 아니라 구현 계약이다. 구현이 이 문서와 충돌하면 편의상 경로를 늘리거나 암묵 동작을 추가하지 말고 문서 판단부터 다시 확인해야 한다.

이 spec의 완료 기준은 config 파일 하나를 읽어보는 POC가 아니라, 이 문서가 정의한 discovery 순서, precedence, runtime layout, profile semantics, secrets 분리, migration/version 규칙을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `shacs-bot`은 self-hosted / personal-use 성격의 Rust 기반 assistant runtime이다.
- 사용자는 직접 설치, 설정, 실행, 복구를 해야 한다.
- `MainOrchestrator`는 정책 권한자이며, 설정은 그 정책의 입력일 뿐이다.
- 디렉터리 구조는 단순하고 예측 가능해야 하며, 장애 시 원인 파악과 복구가 쉬워야 한다.

따라서 이 문서의 runtime layout은 클러스터 배포, 멀티노드 shared state, 조직 단위 admin console, 중앙 SaaS control plane을 전제로 하지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- 설정 탐색 위치와 우선순위
- global / workspace / runtime override의 precedence 규칙
- provider profile, permission profile, runtime profile의 의미
- config와 secrets의 분리 원칙
- runtime data 디렉터리 구조
- 버전 필드와 migration 규칙
- 로컬 단일 사용자 기본값

이 문서는 다음을 정의하지 않는다.

- GUI 설정 편집기
- 원격 config sync 서비스
- 클라우드 secret manager 연동
- 멀티유저 권한 정책
- 개별 provider 벤더의 모든 옵션 목록

---

## 핵심 정의

### config

config는 `shacs-bot`의 실행 정책과 환경 연결점을 정의하는 정적 또는 반정적 설정 집합이다. 예:

- 기본 provider profile 이름
- permission mode 기본값
- runtime data root 경로
- timeout과 token budget의 기본값
- skill discovery 루트
- app bundle 설치 루트와 app registry 위치

config는 세션 이력이나 실행 결과 자체가 아니다.

### secret

secret은 provider API key, 토큰, 인증 자격증명처럼 민감해서 일반 설정 파일과 분리해야 하는 값이다.

### profile

profile은 특정 상황에서 함께 선택되는 설정 묶음이다. 이 문서에서 profile은 크게 세 종류를 다룬다.

- provider profile
- permission profile
- runtime profile

profile은 세션 상태가 아니라 설정 입력이다. 다만 오케스트레이터가 어떤 profile을 선택했는지는 effect snapshot이나 세션 메타데이터에 반영될 수 있다.

### runtime layout

runtime layout은 실행 중 데이터, 로그, 세션 저장소, checkpoint, 캐시, PID 또는 lock 파일 같은 런타임 자원이 놓이는 디렉터리 구조다.

### discovery

discovery는 어떤 config 파일과 secrets 파일을 읽을지 찾는 절차다.

### precedence

precedence는 같은 설정 키가 여러 위치에서 주어졌을 때 어떤 값이 최종 유효한지 정하는 우선순위다.

### config schema version

config schema version은 특정 설정 파일 포맷이 어떤 구조와 의미 규약을 따르는지 나타내는 버전 표식이다.

---

## 초기 설계 목표

이 설정 시스템은 아래 목표를 만족해야 한다.

1. 사용자가 파일 위치를 쉽게 예측할 수 있어야 한다.
2. 로컬 단일 사용자 환경에서 설치 직후 최소 설정으로 시작할 수 있어야 한다.
3. global 설정과 workspace 설정이 명확히 구분되어야 한다.
4. secrets는 일반 config와 분리되어 accidental commit 가능성을 낮춰야 한다.
5. precedence 규칙이 단순하고 설명 가능해야 한다.
6. config schema 변경 시 migration 가능성이 있어야 한다.

---

## 초기 설계 비목표

이 문서는 다음을 목표로 하지 않는다.

- 중앙 서버에서 여러 사용자 설정을 배포하는 구조
- 조직 정책과 개인 정책의 계층형 상속 체계
- 웹 기반 admin-console
- marketplace형 profile 배포
- 자동 원격 secrets 회전 시스템

초기 범위에서 중요한 것은 복잡한 플랫폼화가 아니라, self-hosted 사용자가 로컬 파일만으로 설정 상태를 이해하고 복구할 수 있는 것이다.

---

## 기본 디렉터리 규약

초기 구현은 아래 경로를 기준 규약으로 삼는다.

```text
~/.shacs/
  config.toml
  secrets.toml
  skills/
  apps/
  app-registry.toml
  runtime/
    artifacts/
    sessions/
    checkpoints/
    app-ledger/
    logs/
    cache/
    tmp/

<workspace>/.shacs/
  config.toml
  secrets.toml
  skills/
  apps/
  app-registry.toml
  runtime/
    artifacts/
    sessions/
    checkpoints/
    app-ledger/
    logs/
    cache/
    tmp/
```

### 경로 의미

- `~/.shacs/config.toml`: 사용자 전역 기본 설정
- `~/.shacs/secrets.toml`: 사용자 전역 secret 저장 위치
- `~/.shacs/apps/`: 사용자 전역 app bundle 설치 루트
- `~/.shacs/app-registry.toml`: 사용자 전역 app registry와 grant reference metadata의 저장 위치
- `~/.shacs/runtime/`: 전역 런타임 데이터 루트
- `<workspace>/.shacs/config.toml`: 워크스페이스 오버라이드 설정
- `<workspace>/.shacs/secrets.toml`: 워크스페이스 한정 secret, 필요 시만 사용
- `<workspace>/.shacs/apps/`: 워크스페이스 전용 app bundle 설치 루트
- `<workspace>/.shacs/app-registry.toml`: 워크스페이스 app registry와 grant reference metadata의 저장 위치
- `<workspace>/.shacs/runtime/`: 워크스페이스 한정 세션/로그 저장 루트

`.shacs`는 `shacs-bot` 구현체가 사용하는 filesystem namespace다. app bundle 확장자인 `.shacsapp`은 이 namespace 아래의 `apps/` 루트에 설치될 때 공식 설치 단위가 된다.

`runtime/artifacts/`는 runtime-managed artifact의 공식 저장 위치다. tool runtime의 `binary_ref` / `artifact_list`, diagnostics bundle이 외부 파일을 참조해야 할 때는 기본적으로 이 루트 아래의 안정된 참조만 사용해야 한다. workspace 원본 파일 경로나 executor 내부 임시 객체는 artifact 참조를 가장해 직접 노출하면 안 된다.

`runtime/app-ledger/`는 app process receipt, permission decision 요약, artifact reference, recover/restart evidence 같은 app 실행 영수증을 저장하는 기본 루트다. raw secret value나 provider hidden reasoning은 이 루트에도 저장하면 안 된다.

> 참고 메모: runtime-managed artifact 계약은 004의 tool/runtime 결과 참조, 014의 diagnostics artifact, 015의 upgrade/recover 흐름과 함께 해석된다.
> 이 문서는 공식 저장 루트를 정의하지만, 보존/정리/orphan 처리와 upgrade/recover 시 유효성 같은 lifecycle 규칙은 교차 문서 차원에서 더 정리될 여지가 있다.

초기 구현은 XDG, AppData, 복수 운영체제별 특수 규약을 강하게 추상화하기보다 위 규칙을 명시적으로 우선 고정해도 된다. 중요한 것은 경로가 적고 예측 가능해야 한다는 점이다.

---

## config discovery 규칙

### discovery 순서

하나의 워크스페이스 컨텍스트에서 설정을 구성할 때 기본 탐색 순서는 아래와 같다.

1. built-in defaults
2. user-global config, `~/.shacs/config.toml`
3. workspace-local config, `<workspace>/.shacs/config.toml`
4. explicit runtime override, 예: CLI flag 또는 process bootstrap override

secrets는 별도 채널로 탐색한다.

1. user-global secrets, `~/.shacs/secrets.toml`
2. workspace-local secrets, `<workspace>/.shacs/secrets.toml`
3. explicit secret override, 예: 환경 변수 또는 실행기 주입 값

### discovery 원칙

1. 없는 파일은 조용히 무시할 수 있어야 한다.
2. malformed config는 진단 가능해야 하지만 다른 유효 계층까지 무효화하면 안 된다.
3. 한 번의 프로세스 부트스트랩 또는 한 번의 명시적 reload에서는 일관된 config snapshot을 사용해야 한다.
4. 턴 도중 discovery 결과가 흔들리면 안 된다.

### workspace 경계

workspace-local config는 현재 실행 기준 workspace가 명확할 때만 적용한다. workspace가 없는 단독 실행이라면 user-global config만 사용하면 된다.

---

## precedence 규칙

같은 설정 키가 여러 계층에서 존재할 때 아래 precedence를 적용한다.

```text
built-in defaults < user-global < workspace-local < explicit runtime override
```

secrets도 같은 방향을 따른다.

```text
user-global secrets < workspace-local secrets < explicit secret override
```

### precedence 해석 원칙

- 더 높은 계층은 더 낮은 계층을 덮어쓴다.
- 덮어쓰기는 키 단위로 해석한다.
- profile 단위 덮어쓰기에서 부분 병합을 허용하더라도, 어떤 필드가 어디서 왔는지 설명 가능해야 한다.
- 같은 계층 안에서 같은 키가 중복 정의되면 malformed 또는 duplicate 정의로 처리해야 한다.

### 금지되는 precedence 해석

- 파일 읽기 순서에 따라 우연히 마지막 값이 이기는 구조
- workspace config와 user-global config를 무작위 deep merge해 출처를 잃는 구조
- explicit runtime override보다 config 파일 값이 우선하는 구조

---

## config와 secrets 분리 규칙

### 기본 원칙

민감 정보는 `config.toml`에 직접 두지 않고 `secrets.toml` 또는 명시적 secret override 경로에 둔다.

### config에 두어야 하는 것

- 기본 provider profile 이름
- timeout 기본값
- permission mode 기본값
- runtime 루트 경로
- skill discovery 관련 경로
- profile 이름과 비민감 옵션

### secrets에 두어야 하는 것

- provider API key
- access token
- bearer token
- local integration credential

### config에 두면 안 되는 것

- plaintext API key
- 장기 인증 토큰
- 세션 이력 데이터
- checkpoint payload

### 참조 방식

provider profile은 secret 값을 직접 내장하기보다 secret key name 또는 secret reference를 가리킬 수 있어야 한다.

예:

```toml
[providers.default]
model = "gpt-5"
api_key_ref = "openai.default"
```

그리고 실제 값은 `secrets.toml`에 둔다.

```toml
[provider_secrets.openai]
default = "..."
```

중요한 점은 provider runtime으로 내려가는 시점에만 secret이 해석되어야 하며, 일반 디버그 출력이나 provider input snapshot에는 원문 secret이 들어가면 안 된다는 것이다.

---

## profile 모델

### 1. provider profile

provider profile은 모델 호출에 필요한 비민감 기본 설정 묶음이다.

현재 제품 범위에서 provider/auth family는 아래 셋만 있으면 충분하다.

- OpenAI-compatible provider
- Anthropic auth provider
- Codex auth provider, 단 transport/auth shape는 OpenAI auth 방식에 맞춘다.

구현 참고 기준은 OpenCode의 provider/auth 구현이다. 즉 custom provider shape는 OpenAI-compatible 계열을 우선 기준으로 보고, Anthropic와 Codex는 그 위에 필요한 auth 차이만 얹는 방향을 기본으로 삼는다.

최소 필드 예시는 아래 정도면 충분하다.

- `provider_kind`
- `model_id`
- `api_base` optional
- `api_key_ref` optional
- `timeout_ms`
- `max_output_tokens`
- `temperature` optional
- `tool_calling_enabled`

### 2. permission profile

permission profile은 capability 허용 범위의 기본 묶음이다.

최소 의미는 아래를 포함할 수 있어야 한다.

- `mode`, 예: `default`, `auto`, `plan`
- 허용 capability 범위
- 허용 path root 또는 path policy
- network 허용 scope, 예: `host:api.example.test`
- secret 허용 scope, 예: `openai.default` 같은 secret reference 이름

### 3. runtime profile

runtime profile은 세션 저장, 로그, cache, compaction, tracing 같은 로컬 런타임 동작의 기본값 묶음이다.

최소 의미는 아래를 포함할 수 있어야 한다.

- runtime data root
- logs root
- session store backend kind optional
- compaction threshold
- default checkpoint cadence
- trace verbosity optional

### profile 선택 규칙

profile 자체는 config에 정의되지만, 이번 턴에 어떤 profile이 실제 사용되는지는 오케스트레이터 selection policy가 결정한다. 즉 config는 후보를 정의하고, 실제 채택은 오케스트레이터가 한다.

---

## runtime layout 명세

runtime data는 최소한 아래 범주로 나뉘어야 한다.

```text
runtime/
  artifacts/     # runtime-managed artifact 참조 루트
  sessions/      # session store, event log, metadata
  checkpoints/   # checkpoint 파일 또는 checkpoint backing store
  app-ledger/    # task ledger entry와 app process receipt 저장 루트
  logs/          # 실행 로그, trace, diagnostics
  cache/         # 재생성 가능한 캐시
  tmp/           # 프로세스 생존 범위 임시 파일
```

### 각 디렉터리의 책임

- `artifacts/`: tool/runtime/diagnostics가 참조하는 runtime-managed artifact 루트
- `sessions/`: 공식 session metadata와 event log가 위치하는 루트
- `checkpoints/`: session replay 가속용 checkpoint 저장 위치
- `app-ledger/`: 017의 task ledger entry와 app process receipt를 파일로 저장하는 기본 루트
- `logs/`: 사람이 읽을 수 있는 진단 로그와 tracing 산출물
- `cache/`: 지워져도 재구성 가능한 비공식 캐시
- `tmp/`: 프로세스 종료나 정리 후 사라져도 되는 임시 산출물

### 절대 섞으면 안 되는 것

- `cache/`에 공식 session state 저장
- `tmp/`에 checkpoint 저장
- `app-ledger/`에 raw secret value 또는 provider hidden reasoning 저장
- `logs/`에 secret 원문 기록
- `sessions/`와 `checkpoints/`를 UI 캐시와 혼합 저장

공식 복구 진실 원천과 재생성 가능한 캐시는 반드시 분리되어야 한다.

---

## 로컬 self-hosted 기본값

초기 기본값은 단순성과 설명 가능성을 우선한다.

### 기본 동작

- user-global config가 없어도 built-in defaults로 읽기 전용 또는 최소 동작이 가능해야 한다.
- provider 사용 전에는 필요한 secret이 없으면 명확한 진단으로 실패해야 한다.
- `--workspace-root <path>`가 제공되면 workspace-local config를 후보로 본다.
- runtime data root는 명시되지 않으면 `--workspace-root` 생략 시 `~/.shacs/runtime`, `--workspace-root <path>` 제공 시 `<workspace>/.shacs/runtime`을 택한다.

### 기본 가정

- 단일 사용자
- 단일 머신 또는 단일 파일 시스템
- 로컬 디스크 기반 session store
- 운영자 팀이 아닌 사용자 본인 관리

이 가정을 깨는 enterprise형 기능은 초기 명세에 넣지 않는다.

---

## 버전과 migration 규칙

### config schema version

각 config 파일은 schema version을 명시하거나, 없을 경우 초기 버전으로 해석할 수 있어야 한다. 중요한 것은 parser가 자신이 읽는 구조의 버전을 알고 있어야 한다는 점이다.

예시:

```toml
schema_version = 1
```

### migration 기본 원칙

1. migration은 명시적이어야 한다.
2. 알 수 없는 미래 버전은 조용히 읽지 말고 명확히 거절해야 한다.
3. 이전 버전에서 자동 보정이 가능해도, 의미가 바뀌는 경우는 진단을 남겨야 한다.
4. secret 포맷 migration은 config migration과 분리 가능해야 한다.

### migration 범위

다음은 migration 대상이 될 수 있다.

- 키 이름 변경
- profile 필드 구조 변경
- runtime root 하위 디렉터리 명칭 변경
- 기본값 의미 변경

### migration이 해서는 안 되는 일

- 기존 session store를 조용히 덮어쓰기
- secret 값을 로그에 노출
- 사용자의 workspace-local 설정을 user-global로 승격
- schema version이 맞지 않는 파일을 억지로 읽고 모호한 상태로 진행

---

## discovery 및 적용 시퀀스 예시

### 예시 1. workspace-root 실행의 정상 config 조립

```text
1) 사용자가 `--workspace-root <path>`를 지정해 shacs-bot을 실행한다.
2) 런타임은 built-in defaults를 로드한다.
3) `~/.shacs/config.toml`이 있으면 user-global 설정을 읽는다.
4) `<workspace>/.shacs/config.toml`이 있으면 workspace-local 설정을 읽는다.
5) explicit runtime override가 있으면 가장 높은 precedence로 적용한다.
6) secrets는 별도 경로에서 같은 방식으로 조립한다.
7) 결과적으로 하나의 immutable config snapshot이 만들어진다.
8) 이 snapshot을 바탕으로 provider registry, skill roots, runtime roots를 초기화한다.
```

### 예시 2. secret 미존재로 인한 provider 사용 실패

```text
1) provider profile이 `api_key_ref = "openai.default"`를 가진다.
2) discovery 결과 해당 secret reference를 어떤 secret source에서도 찾지 못한다.
3) config 로드는 계속 가능하지만, provider profile은 incomplete diagnostic을 가진다.
4) 실제 model invocation selection 시 이 profile을 선택하려 하면 오케스트레이터는 명확한 구성 오류로 턴을 중단한다.
```

핵심은 config 파서와 runtime selection이 역할을 나누는 것이다. 읽는 단계와 실제 사용하는 단계는 구분되어야 한다.

---

## 실패 시나리오

### 시나리오 1. malformed workspace config

- `<workspace>/.shacs/config.toml` 문법이 깨짐
- 처리 규칙: user-global과 built-in defaults까지 무효화하지 말고, workspace 계층만 진단과 함께 제외하거나 전체 부트스트랩을 명시적으로 실패시켜야 한다
- 금지: 깨진 값을 partial parse해서 조용히 적용

### 시나리오 2. runtime root를 cache 아래로 잘못 설정

- 사용자가 `runtime_root = "<workspace>/.shacs/runtime/cache"` 같은 값을 넣음
- 처리 규칙: 공식 state 루트와 cache 루트가 뒤섞이는 설정은 진단과 함께 거절해야 한다

### 시나리오 3. 미래 schema version

- `schema_version = 999`
- 처리 규칙: 현재 실행기가 지원하지 않는 미래 버전이면 명확히 거절해야 한다
- 금지: 임의 fallback으로 읽기

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 타입, validation, 테스트로 강제 대상이다.

1. config discovery precedence는 결정적이어야 한다.
2. 한 번 부트스트랩된 config snapshot은 명시적 reload 전까지 흔들리면 안 된다.
3. secrets는 일반 config와 분리된 source로 다뤄져야 한다.
4. 공식 session state는 `sessions/` 또는 `checkpoints/` 경계 밖의 캐시에 저장되면 안 된다.
5. malformed config는 진단 가능해야 하며 조용히 정상 값처럼 취급되면 안 된다.
6. 지원하지 않는 schema version은 명확히 실패해야 한다.
7. profile 선택은 config 정의와 오케스트레이터 selection을 구분해야 한다.
8. secret 원문은 로그, trace, provider input snapshot에 평문으로 남으면 안 된다.
9. workspace-local config는 workspace 컨텍스트에서만 적용되어야 한다.
10. self-hosted 기본값은 외부 제어 평면 없이도 로컬 단독 실행 가능해야 한다.

---

## 금지 패턴

### 1. config와 runtime state 혼합

금지 예:

- `config.toml` 안에 last session id, checkpoint path, open turn 상태를 저장

왜 금지인가:

- 설정과 실행 상태의 책임 경계가 무너진다.
- 복구와 디버깅이 어려워진다.

### 2. secret 평문을 일반 설정에 저장

금지 예:

- provider API key를 `config.toml`에 직접 둠
- 디버그 로그에 secret 값을 그대로 출력

왜 금지인가:

- accidental commit과 노출 위험이 커진다.
- config 공유와 secret 관리가 분리되지 않는다.

### 3. precedence를 암묵 merge에 맡김

금지 예:

- TOML 파서가 읽은 순서대로 마지막 값이 이기게 방치
- profile object를 깊은 병합하면서 출처 추적 불가

왜 금지인가:

- 어떤 설정이 실제로 적용됐는지 설명할 수 없어진다.
- 재현성이 떨어진다.

### 4. runtime root를 비공식 캐시 위치에 둠

금지 예:

- `tmp/` 아래를 공식 session store 루트로 사용
- cache 디렉터리를 checkpoint 저장소로 사용

왜 금지인가:

- 공식 복구 데이터와 일시 데이터가 섞인다.
- crash recovery 계약이 약해진다.

### 5. 미래 schema version을 추측해서 읽기

금지 예:

- 모르는 버전인데 알 만한 키만 대충 읽고 계속 실행

왜 금지인가:

- 의미가 바뀐 필드를 잘못 해석할 수 있다.
- 사용자가 현재 실행기의 지원 범위를 알 수 없게 된다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- `ConfigSnapshot`, `SecretsSnapshot`, `ProviderProfile`, `PermissionProfile`, `RuntimeProfile`이 분리된 타입인가?
- discovery 단계와 precedence 적용 단계가 분리되어 있는가?
- config source별 origin 정보를 보존할 수 있는가?
- secret reference 해석이 provider invocation 직전까지 지연될 수 있는가?
- runtime layout validation이 `sessions/`, `checkpoints/`, `cache/`, `tmp/` 충돌을 잡을 수 있는가?
- schema version validator와 migration entrypoint가 있는가?
- workspace-local config 적용 여부가 현재 workspace 컨텍스트와 연결되어 있는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져갈 수 있어야 한다.

- built-in, user-global, workspace-local, explicit override precedence가 문서대로 적용되는가
- user-global secret보다 workspace-local secret이 같은 reference를 덮어쓸 수 있는가
- malformed workspace config가 조용히 partial 적용되지 않는가
- unsupported schema version이 명확히 실패하는가
- provider profile이 missing secret reference를 가질 때 진단 가능 상태가 되는가
- runtime root와 cache root가 뒤섞이면 validation이 실패하는가
- workspace가 없는 실행에서 workspace-local config를 잘못 읽지 않는가
- secret 값이 trace/debug snapshot에 노출되지 않는가

---

## 명시적 비범위

위 초기 설계 목표와 초기 설계 비목표는 이 설정 문서의 방향성과 우선순위를 설명하는 요약이다. 아래 명시적 비범위는 이 문서가 실제로 정의하지 않는 계약 경계를 최종적으로 고정한다.

이 문서는 다음을 정의하지 않는다.

- 위 세 종류를 넘는 추가 provider/auth family
- GUI 설정 편집 UX
- 원격 정책 배포
- 조직 단위 RBAC
- cloud secret manager와의 공식 통합 계약
- 다중 머신 shared runtime layout

이 항목들은 필요가 생기면 별도 문서에서 다룬다. 단, 어떤 확장도 "설정은 예측 가능해야 하고, secrets는 분리되어야 하며, 공식 런타임 데이터는 복구 가능한 디렉터리 경계 안에 있어야 한다"는 원칙을 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 configuration profiles and runtime layout은 단순 파일 위치 규약이 아니다. 이것은 사용자가 무엇을 어디에 두어야 하는지, 어떤 값이 실제로 적용되는지, 무엇이 공식 복구 데이터이고 무엇이 단순 캐시인지를 고정하는 운영 계약이다.

핵심은 네 가지다.

- discovery와 precedence는 단순하고 결정적이어야 한다.
- profile은 config 후보 집합이며 실제 채택은 오케스트레이터가 한다.
- secrets는 일반 설정과 분리되어야 한다.
- runtime layout은 self-hosted 단일 사용자 복구성을 중심으로 나뉘어야 한다.

이 구조가 지켜져야 `shacs-bot`은 설치와 운영이 단순하고, 문제 발생 시 원인을 찾기 쉽고, 설정과 상태를 혼동하지 않는 로컬 assistant runtime으로 유지될 수 있다.
