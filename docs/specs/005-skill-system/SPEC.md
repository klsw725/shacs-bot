# skill system 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`의 스킬 시스템 방향을 현재 Rust 구현 기준으로 구체화한 하위 아키텍처 명세다.

목표는 다음과 같다.

- `shacs-bot`에서 스킬이 무엇이고 무엇이 아닌지 고정한다.
- 스킬 탐색, 파싱, 레지스트리 구성, 런타임 주입 경계를 구현 기준으로 정리한다.
- 스킬이 메인 오케스트레이터를 보조하되 권한을 넘겨받지 않도록 불변식과 금지 패턴을 명시한다.
- 현재 `shacs-skills`, `ContextBuilder`, CLI inspect 표면이 어떤 계약을 만족하는지 문서화한다.

이 문서는 방향 제안이 아니라 구현 기준이다. 구현이 이 문서와 충돌하면, 코드를 우선 밀어붙이지 말고 문서 판단부터 다시 갱신해야 한다.

이 spec의 완료 기준은 스킬 파일을 읽어보는 수준의 POC가 아니라, 이 문서가 정의한 탐색 규약, precedence, 파싱/진단 상태, 레지스트리 동작, read-only 주입 경계를 현재 코드 표면과 테스트 증거로 충족하는 상태다.

---

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 Spec 005는 current architecture 기준으로 완료로 닫는다. 완료의 의미는 formal per-turn registry snapshot, replay/effect provenance snapshot, app bundle lifecycle ownership, remote marketplace, executable plugin code를 구현했다는 뜻이 아니다.

완료의 의미는 현재 코드의 `shacs-skills` registry/discovery, `SkillSourceKind`, `SkillRegistryStatus`, `SkillDescriptor`, `discover_skill_registry`, `ContextBuilder` skill injection, CLI `skills list`/`skills show` inspect 표면이 스킬 시스템의 현재 경계로 문서화됐고, 기존 테스트 증거가 그 범위 안에서 유지된다는 뜻이다.

### 이미 반영된 것

- `shacs-skills`는 virtual builtin, materialized builtin, configured user skills, workspace skill root, plugin root 후보를 탐색해 레지스트리를 만든다.
- `SkillSourceKind`는 `VirtualBuiltin`, `MaterializedBuiltin`, `UserGlobal`, `WorkspaceLegacy`, `WorkspaceLocal`, `PluginProvided`를 구분한다.
- `SkillRegistryStatus`는 `Active`, `Shadowed`, `Conflicted`, `Malformed`를 구분한다.
- `SkillDescriptor`는 `name`, `description`, `source_kind`, `source_path`, `body_hash`, `requirements`, `install_metadata`를 inspect 가능한 메타데이터로 제공한다.
- `ContextBuilder::build_system_prompt`는 `# Active Skills`와 `# Available Skills`를 구성하고, 선택된 스킬 Markdown은 `load_skills_for_context`를 통해 read-only context로 들어간다.
- CLI `skills list`와 `skills show`는 source, status, path, description, `body_hash`, requirements, install metadata, diagnostics를 확인하는 inspect 표면이다. 이것은 replay provenance나 effect provenance 표면이 아니다.

### 후속 비목표 / 별도 owner로 넘길 것

- formal per-turn registry snapshot과 replay/effect provenance snapshot은 현재 완료 조건이 아니다.
- app bundle 설치, 제거, 업데이트 lifecycle ownership은 Spec 005가 아니라 `017-app-operating-environment/`와 `015-packaging-process-lifecycle-and-upgrades/`가 소유한다.
- 원격 marketplace, 서명된 배포 채널, 실행 가능한 plugin code는 이 문서의 현재 구현 범위가 아니다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태를 변경할 수 있는 유일한 구성요소다.
- session kernel은 한 턴의 문맥 구성과 외부 실행 조정을 책임진다.
- `Command`, `Event`, `Effect` 경계는 유지되어야 한다.
- 스킬은 `OpenHarness`식의 단순한 설치 및 로딩 UX를 참고하되, `shacs-bot`에서는 어디까지나 오케스트레이터에 종속된 read-only 지식 팩이다.

따라서 이 문서에서 말하는 스킬은 플러그인 코드 실행 단위가 아니다. 스킬은 Markdown 파일로 저장된 지식 묶음이며, 오케스트레이터가 문맥 구성 단계에서 필요할 때만 읽어 들이는 보조 입력이다.

---

## 핵심 정의

### 스킬

스킬은 특정 작업 맥락에 맞는 절차, 제약, 스타일, 주의사항을 담은 Markdown 기반 지식 팩이다.

스킬은 다음 성격을 가진다.

- 파일 시스템에 저장된다.
- 기본 단위는 `SKILL.md` 한 파일이다.
- 읽기 전용이다.
- 세션 문맥을 보강할 수는 있지만 세션 상태를 직접 수정하지 못한다.
- 툴 권한, permission mode, 상태 전이 권한을 얻지 못한다.

### 스킬 발견

스킬 발견은 미리 정해진 디렉터리 규약 아래에서 후보 `SKILL.md` 파일을 찾고, 이를 레지스트리 후보로 수집하는 과정이다.

### 스킬 파싱

스킬 파싱은 `SKILL.md` 텍스트에서 최소 메타데이터와 본문을 안정적으로 추출해 레지스트리에 올릴 수 있는 형태로 정규화하는 과정이다.

### 스킬 레지스트리

스킬 레지스트리는 현재 세션 또는 워크스페이스 맥락에서 사용할 수 있는 스킬의 카탈로그다. 레지스트리는 스킬의 실제 본문을 영구 소유하는 저장소가 아니라, 탐색 결과와 선택 규칙을 담은 읽기 전용 인덱스다.

### 스킬 주입

스킬 주입은 오케스트레이터가 `context_building` 단계에서 특정 스킬의 본문 일부 또는 전체를 모델 입력 문맥에 포함시키는 행위다.

이 주입은 문맥 구성의 일부일 뿐이며, `Command` 실행이나 `Effect` 승인과 동일하지 않다.

---

## 초기 설계 목표

스킬 시스템은 아래 목표를 만족해야 한다.

1. 사용자가 파일을 놓는 것만으로 스킬을 추가할 수 있어야 한다.
2. 어떤 스킬이 선택되었는지와 왜 선택되었는지를 설명할 수 있어야 한다.
3. 같은 이름의 스킬이 여러 위치에 있을 때 선택 규칙이 예측 가능해야 한다.
4. 잘못된 스킬 파일이 전체 세션 실행을 망가뜨리지 않아야 한다.
5. 스킬 본문은 문맥 보강만 해야 하고 상태 권한은 얻지 못해야 한다.

---

## 초기 설계 비목표

이 문서는 다음을 정의하지 않는다.

- 원격 스킬 마켓플레이스
- 스킬 서명, 배포 채널, 패키지 저장소 프로토콜
- 실행 가능한 스킬 코드 샌드박스
- app 생명주기 전체 명세
- 고급 스킬 추천 랭킹 시스템

초기 범위에서 필요한 것은 복잡한 유통 체계가 아니라, 로컬 단일 사용자 환경에서 예측 가능한 탐색과 안전한 주입이다.

---

## 파일 시스템 발견 규약

현재 구현은 `discover_skill_registry`가 아래 source에서 스킬 후보를 찾는다.

```text
virtual builtins
<workspace>/builtin_skills/<skill-name>/SKILL.md
<configured user data>/skills/<skill-name>/SKILL.md
<workspace>/.nanobot/skills/<skill-name>/SKILL.md
<workspace>/.shacs-bot/skills/<skill-name>/SKILL.md
<workspace>/skills/<skill-name>/SKILL.md
<plugin_roots>/<root>/<skill-name>/SKILL.md
```

설명:

- virtual builtins는 파일 시스템에 materialize되지 않아도 레지스트리에 나타나는 기본 스킬이다.
- `<workspace>/builtin_skills/`는 materialized builtin 스킬 위치다.
- `<configured user data>/skills/`는 설정된 사용자 데이터 스킬 위치다.
- `<workspace>/.nanobot/skills/`는 legacy workspace 스킬 위치다.
- `<workspace>/.shacs-bot/skills/`와 `<workspace>/skills/`는 현재 workspace 스킬 위치다.
- `plugin_roots`는 외부에서 넘겨준 Markdown skill root이며 `PluginProvided`로 표시된다.

여기서 plugin 제공 스킬은 파일 시스템에 놓인 Markdown 팩일 뿐이다. 실행 가능한 plugin code가 아니며, 스킬을 통해 오케스트레이터 권한을 우회해서는 안 된다.

### 발견 단위

- 디렉터리 이름이 곧 기본 `skill-name` 후보다.
- 유효한 스킬 파일 이름은 정확히 `SKILL.md` 하나다.
- 같은 디렉터리에 부가 문서가 있어도 현재 구현은 `SKILL.md`만 공식 입력으로 본다.

### 경로 우선순위

동일한 `skill-name`이 여러 위치에서 발견되면 아래 우선순위를 적용한다.

```text
VirtualBuiltin < MaterializedBuiltin < UserGlobal < WorkspaceLegacy < WorkspaceLocal < PluginProvided
```

같은 이름의 후보가 여러 source에서 발견되면 더 높은 우선순위 후보가 활성 후보가 되고, 낮은 우선순위 후보는 `Shadowed`로 남는다.

### 동률 처리

같은 우선순위 계층 안에서 동일한 `skill-name`이 둘 이상 발견되면 자동 병합하지 않는다. 이 경우는 충돌이다.

예:

- 두 개의 서로 다른 plugin root가 각각 `review/SKILL.md`를 제공하는 경우
- workspace 로컬 경로 안에 중복 마운트로 같은 이름이 두 번 나타나는 경우

현재 구현의 기본 정책:

- 해당 `skill-name`을 `conflicted` 상태로 표시한다.
- 레지스트리는 충돌 진단 정보를 남긴다.
- 충돌이 해소되기 전까지 그 이름의 스킬은 자동 주입 대상에서 제외한다.

자동 병합을 하지 않는 이유는, Markdown 스킬의 지시사항을 문자열 수준에서 합치면 상충하는 지침이 조용히 섞일 수 있기 때문이다.

---

## Markdown 형식 기대치

현재 구현은 사람 친화적이면서도 Rust 파서가 단순하게 다룰 수 있는 최소 규약을 따른다.

### 필수 조건

- 파일은 UTF-8 텍스트여야 한다.
- 파일명은 `SKILL.md`여야 한다.
- `shacs-skills` registry에서 쓰는 이름은 비어 있지 않은 frontmatter `name`이 있으면 그 값을 쓰고, 없으면 디렉터리 또는 fallback 이름을 진단과 함께 쓴다.
- 첫 번째 Markdown H1 제목은 현재 registry의 `SkillDescriptor.name` 결정 규칙이 아니다.

### 권장 구조

아래 구조를 권장한다.

```markdown
# skill-name

짧은 한 줄 설명.

## Use When

- 어떤 상황에서 이 스킬을 쓰는지

## Instructions

- 모델이 따라야 할 핵심 절차

## Constraints

- 하면 안 되는 것
```

### 메타데이터

현재 구현은 optional YAML frontmatter를 허용할 수 있다. 단, frontmatter가 없어도 스킬은 유효할 수 있어야 한다.

허용 예시:

```markdown
---
name: rust-review
description: Rust 코드 리뷰 체크리스트
---

# Rust review
```

현재 registry 파서는 다음 메타데이터를 inspect 가능한 descriptor로 정리한다.

- `name`
- `description`
- `requirements`
- `install_metadata`

그 외 키는 현재 registry와 inspect 표면의 계약으로 주장하지 않는다.

---

## 파싱 가정과 정규화 규칙

파서는 관대한 입력 수용보다 예측 가능한 정규화를 우선한다.

### 최소 파싱 단계

1. 파일을 UTF-8로 읽는다.
2. 선택적 frontmatter를 분리한다.
3. registry 이름을 결정한다.
4. 본문 hash와 inspect 메타데이터를 정리한다.
5. 정규화된 `SkillDescriptor`를 만든다.

### registry 이름 결정 순서

1. 비어 있지 않은 frontmatter `name`
2. 디렉터리 또는 fallback 이름 `<skill-name>`과 진단

빈 frontmatter `name`은 그 자체로 malformed가 아니다. 현재 registry는 이를 이름 누락으로 진단하고 디렉터리 또는 fallback 이름을 사용한다. 첫 번째 H1 제목 텍스트는 현재 registry 이름 결정에 쓰지 않는다.

### 정규화 결과에 포함되어야 할 최소 필드

- `name`
- `description`
- `source_kind`, `VirtualBuiltin`, `MaterializedBuiltin`, `UserGlobal`, `WorkspaceLegacy`, `WorkspaceLocal`, `PluginProvided`
- `source_path`
- `body_hash`
- `requirements`
- `install_metadata`
- registry status, `Active`, `Shadowed`, `Conflicted`, `Malformed`

스킬 본문 Markdown은 descriptor의 권한 메타데이터가 아니라, 선택된 스킬을 context에 넣을 때 읽히는 read-only 입력이다.

`ContextBuilder`의 context용 `SkillDocument`는 registry `SkillDescriptor`를 그대로 복제한 모델이 아니다. context `SkillDocument.name`은 경로에서 얻고, description, requirements, always, disabled, availability 같은 값은 frontmatter metadata에서 읽는다.

### malformed 판정 기준

아래 중 하나라도 만족하면 malformed로 본다.

- UTF-8로 읽을 수 없다.
- frontmatter가 열렸지만 닫히지 않아 본문 경계를 정할 수 없다.
- skill root가 디렉터리 형태의 `SKILL.md` 규약을 만족하지 못한다.

malformed 스킬은 레지스트리에 진단 정보와 함께 남길 수는 있지만, 사용 가능 상태로 승격하면 안 된다.

---

## 레지스트리 동작 규칙

레지스트리는 스킬의 선택 가능성, 출처, 충돌 상태를 설명하는 읽기 전용 인덱스여야 한다.

### 레지스트리의 책임

- 발견된 스킬 후보 수집
- 우선순위 적용
- 충돌 및 malformed 상태 기록
- 목록 조회용 메타데이터 제공
- 특정 스킬 로드 요청 시 canonical source 반환

### 레지스트리가 하지 말아야 할 일

- 스킬 내용을 임의로 합성해서 새 스킬을 만들기
- 스킬 본문을 수정해서 저장하기
- 스킬 선택만으로 세션 정책을 변경하기
- 스킬 내용만 믿고 직접 tool 실행 계획을 승인하기

### shadowed 스킬

더 낮은 우선순위의 같은 이름 스킬은 `shadowed` 상태로 남길 수 있다.

예:

- virtual builtin `rust-review`
- `<workspace>/.shacs-bot/skills/rust-review/SKILL.md`

이 경우 `WorkspaceLocal` 스킬이 활성 후보가 되고, virtual builtin 스킬은 shadowed로 남는다. shadowed 항목은 디버깅과 설명 가능성을 위해 목록에 보이게 할 수 있지만, 기본 주입 대상은 아니다.

### 레지스트리 재구성 시점

현재 구현은 호출 시점의 discovery 결과로 레지스트리를 만든다.

- CLI `skills list` 또는 `skills show`에서 inspect용 레지스트리를 만든다.
- `ContextBuilder`가 context를 구성할 때 active와 available skill 정보를 만든다.
- 선택된 스킬 본문은 `load_skills_for_context`에서 read-only Markdown context로 로드한다.

이 문서는 formal per-turn immutable registry snapshot이나 replay/effect provenance snapshot이 현재 구현돼 있다고 주장하지 않는다. 현재 완료 범위는 discovery 결과, status, source, `body_hash`, diagnostics를 inspect 가능하게 만들고, context 구성 시 읽은 Markdown을 권한 없는 입력으로 주입하는 데 있다.

### CLI inspect 표면

CLI `skills list`와 `skills show`는 레지스트리의 현재 내용을 사람이 확인하는 표면이다.

- `skills list`는 스킬 이름, 상태, source를 목록화한다.
- `skills show`는 status, source, `body_hash`, path, description, requirements, install metadata, diagnostics를 보여준다.
- 이 표면은 현재 registry 상태를 설명하기 위한 것이며, formal replay provenance나 effect provenance의 저장소가 아니다.

---

## 런타임 주입 경계

스킬은 session kernel의 `context_building` 단계에서만 문맥으로 주입될 수 있다.

### 허용되는 주입 경계

- 세션 시작 전 기본 스킬 선택 상태를 참고해 주입 후보를 정한다.
- 사용자 요청, 명시적 선택, 오케스트레이터 정책에 따라 특정 스킬을 로드한다.
- 로드된 스킬 Markdown 본문을 모델 입력용 참조 문맥으로 포함한다.

### 허용되지 않는 주입 경계

- 스킬 로더가 `SessionState`를 직접 수정
- 스킬 파서가 `Command`를 생성
- 스킬 본문이 `Effect` 승인 권한을 얻음
- 스킬이 permission mode를 자동 승격
- 스킬이 tool 결과를 공식 기록으로 commit

### 주입의 의미

스킬 주입은 다음 중 하나로만 해석되어야 한다.

- 모델에게 참고 지침을 제공한다.
- 오케스트레이터가 문맥 설명력을 높인다.

스킬 주입은 다음으로 해석되면 안 된다.

- 독립 실행 계획의 승인
- 안전 정책 우회
- 새 상태 전이 타입의 생성

즉, 스킬은 텍스트다. 권한자가 아니다.

---

## 정상 로드 및 주입 시퀀스 예시

아래는 하나의 정상적인 로드 및 주입 시퀀스다.

```text
1) 사용자가 workspace에서 "Rust 에러를 정리해줘" 같은 요청을 보낸다.
2) MainOrchestrator는 새 턴을 accepted로 연다.
3) context_building 단계에서 skill selector가 `rust-review` 스킬 필요성을 판단한다.
4) SkillRegistry는 같은 이름 후보를 조회한다.
5) virtual builtin, materialized builtin, user, workspace, plugin 후보 중 우선순위와 충돌 상태에 따라 활성 후보가 선택된다.
6) SkillLoader는 활성 후보의 `SKILL.md`를 읽고 파싱한다.
7) `ContextBuilder::build_system_prompt`는 `# Active Skills`와 `# Available Skills`를 구성한다.
8) `load_skills_for_context`는 선택된 Markdown 본문을 모델 입력용 read-only context로 포함한다.
9) 모델이 응답이나 tool call 제안을 반환한다.
10) 이후 tool 실행 승인 여부는 여전히 오케스트레이터 정책이 결정한다.
```

이 시퀀스에서 중요한 점은 8단계와 10단계다. 스킬은 문맥에 들어갈 수 있지만, tool 실행 권한을 직접 얻지 못한다.

---

## malformed 또는 conflicting 스킬 예시

### 예시 1. malformed 스킬

```text
<configured user data>/skills/release-check/SKILL.md

---
name: release-check
description: 릴리스 전 체크리스트

# Release check
```

문제점:

- frontmatter가 닫히지 않았다.
- 닫히지 않은 frontmatter 때문에 본문 경계를 정할 수 없다.

처리 규칙:

- 파서는 이 파일을 malformed로 표시한다.
- 레지스트리는 진단 메시지를 남긴다.
- 이 스킬은 로드 가능 목록의 활성 항목이 되면 안 된다.
- 다른 정상 소스에서 같은 `release-check` 스킬이 있다면, 정상 항목이 우선 고려될 수 있다.

닫힌 frontmatter에서 `name` 값만 비어 있는 경우는 이 예시와 다르다. 현재 registry는 그 경우를 malformed로 보지 않고 fallback 이름과 진단을 남긴다.

### 예시 2. conflicting 스킬

```text
<plugin_roots>/git-helper/review/SKILL.md
<plugin_roots>/security-helper/review/SKILL.md
```

문제점:

- 둘 다 `PluginProvided` 계층이다.
- 둘 다 같은 `review` 이름을 제공한다.
- 같은 우선순위 안에서 자동 병합 기준이 없다.

처리 규칙:

- 레지스트리는 `review`를 conflicted 상태로 표시한다.
- 자동 선택과 자동 주입에서 제외한다.
- 사용자 또는 상위 설정이 명시적으로 경로를 좁히기 전까지 활성 스킬로 승격하지 않는다.

---

## 구현 불변식

아래 불변식은 현재 Rust 구현과 후속 owner 작업 모두에서 유지해야 한다.

1. 스킬은 read-only Markdown 자원이다.
2. 스킬 발견과 파싱은 `SessionState`를 직접 변경하지 않는다.
3. 스킬 주입은 `context_building` 단계에서만 일어난다.
4. 스킬은 `Command`, `Event`, `Effect`의 생산 권한을 갖지 않는다.
5. 스킬 내용은 permission mode나 tool 실행 허용 여부를 직접 바꾸지 못한다.
6. 동일한 `skill-name`의 최종 활성 후보는 우선순위 규칙 또는 명시적 충돌 상태로 설명 가능해야 한다.
7. malformed 스킬은 진단 가능해야 하지만 활성 로드 대상이 되면 안 된다.
8. context에 들어간 스킬은 source, status, `body_hash`, diagnostics로 설명 가능해야 한다.
9. 레지스트리는 shadowed, malformed, conflicted 상태를 구분 가능해야 한다.
10. 스킬 본문 주입 이후에도 최종 상태 전이와 외부 실행 승인 권한은 오케스트레이터에 남아 있어야 한다.

---

## Rust 구현 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에 모두 "예"라고 답할 수 있어야 한다.

- `SkillSourceKind`, `SkillDescriptor`, `SkillRegistryEntry` 같은 타입 경계가 분리되어 있는가?
- 탐색 결과와 활성 선택 결과가 구분되어 있는가?
- malformed, shadowed, conflicted를 enum 수준에서 구별할 수 있는가?
- context 구성과 CLI inspect에서 source, status, `body_hash`, diagnostics를 확인할 수 있는가?
- 스킬 주입 코드가 tool permission 판단 코드와 분리되어 있는가?
- 스킬 로더가 텍스트만 반환하고 상태 변경을 하지 않는가?

이 질문 중 하나라도 "아니오"라면, 구현이 스킬을 단순 문맥 팩이 아니라 숨은 권한자로 만들고 있을 가능성이 높다.

---

## 테스트 관점에서 꼭 검증할 시나리오

현재 Rust 구현은 최소한 아래 시나리오를 테스트로 가져갈 수 있어야 한다.

- virtual builtin, materialized builtin, user, workspace 후보가 있을 때 우선순위대로 하나만 활성 선택되는가
- 같은 우선순위 계층의 동일 이름 스킬이 충돌로 표시되는가
- frontmatter가 깨진 스킬이 malformed로 분류되고 활성 로드되지 않는가
- 스킬 본문이 문맥에는 포함되지만 permission 판단을 바꾸지 않는가
- CLI `skills list`와 `skills show`가 source, status, `body_hash`, diagnostics를 노출하는가
- shadowed 스킬이 설명 가능 메타데이터로는 남고 활성 주입 대상에서는 제외되는가

---

## 금지 패턴

다음 패턴은 구현에서 명시적으로 금지한다.

### 1. 스킬을 실행 권한자로 취급

금지 예:

- 스킬 본문에 "이 경우 바로 shell tool을 실행하라"가 적혀 있다는 이유만으로 오케스트레이터 승인 없이 effect를 발행
- 스킬 로더가 위험 작업을 자동 허용 플래그로 변환

왜 금지인가:

- 스킬은 참고 문맥이지 정책 권한자가 아니다.
- 메인 오케스트레이터 단일 권한 원칙이 깨진다.

### 2. 여러 스킬 본문 자동 병합

금지 예:

- 같은 이름 스킬 여러 개를 단순 문자열 concat으로 합쳐 사용
- 충돌한 `PluginProvided` 스킬을 임의 순서로 병합해 하나의 지침처럼 주입

왜 금지인가:

- 상충 지시가 조용히 섞인다.
- 왜 그런 문맥이 만들어졌는지 설명하기 어려워진다.

### 3. 스킬 파싱 실패를 조용히 무시하고 정상처럼 취급

금지 예:

- frontmatter 오류가 나도 빈 본문으로 등록
- 제목이 없으면 아무 이름이나 생성해서 활성화

왜 금지인가:

- 사용자 입장에서 어떤 스킬이 실제로 로드되었는지 알 수 없어진다.
- 디버깅 가능성과 재현성이 떨어진다.

### 4. 스킬이 상태 patch를 제공하는 구조

금지 예:

- 스킬 파일 안에 세션 상태 JSON diff를 넣고 자동 적용
- 스킬 선택만으로 session policy를 영속 변경

왜 금지인가:

- Markdown 지식 팩이라는 경계를 벗어난다.
- `Command`, `Event`, `Effect` 구조와 충돌한다.

### 5. 한 context 구성 안에서 서로 다른 discovery 결과 섞기

금지 예:

- 한 번의 `context_building` 입력을 만들면서 서로 다른 재스캔 결과를 섞어 다른 버전의 스킬 문맥을 사용

왜 금지인가:

- 같은 입력이 실행 시점에 따라 다른 문맥을 얻게 된다.
- 설명 가능성과 테스트 가능성이 떨어진다.

---

## 명시적 비범위

위 설계 목표와 비목표는 방향성과 우선순위를 설명하는 요약이다. 아래 명시적 비범위는 이 문서가 실제로 정의하지 않는 계약 경계를 최종적으로 고정한다.

이 문서는 다음을 정의하지 않는다.

- 스킬 추천 UX 세부 화면
- 스킬 설치용 네트워크 프로토콜
- 서명 검증, trust policy, 원격 배포 메타데이터
- app 시스템 전체 수명주기와 바이너리 로딩 계약, 이 계약은 `017-app-operating-environment/`와 `015-packaging-process-lifecycle-and-upgrades/`가 소유한다.
- 다중 파일 스킬 팩 포맷

이 항목들은 필요가 생기면 별도 문서에서 다룬다. 단, 어떤 확장도 "스킬은 read-only Markdown 지식 팩이며 최종 권한은 오케스트레이터에 남는다"는 원칙을 뒤집어서는 안 된다.

> 참고 메모: 현재 스킬 탐색 규약은 `plugin_roots`를 `PluginProvided` source로 받을 수 있지만, app bundle 자체의 설치/업데이트/제거 lifecycle은 `017-app-operating-environment/`와 `015-packaging-process-lifecycle-and-upgrades/`가 소유한다.
> 따라서 이 문서는 skill discovery와 injection 경계만 다루고, app ownership 계약이나 실행 가능한 plugin code 계약은 정의하지 않는다.

---

## 결론

`shacs-bot`의 스킬 시스템은 OpenHarness식의 단순한 설치 및 로딩 경험을 참고하되, 강한 메인 오케스트레이터 구조 아래에서만 동작해야 한다.

- 스킬은 파일 시스템에서 발견되는 `SKILL.md` 기반 read-only 지식 팩이다.
- 레지스트리는 우선순위, shadowing, malformed, conflict를 설명 가능하게 관리해야 한다.
- 스킬 주입은 문맥 보강일 뿐이며 상태 전이, 권한 승인, 외부 실행 확정은 모두 오케스트레이터가 담당한다.

핵심은 스킬을 많이 붙이는 것이 아니라, 어떤 스킬이 어디서 왔고 왜 선택되었으며 어떤 경계 안에서만 영향력을 가지는지를 끝까지 설명 가능하게 유지하는 것이다.
