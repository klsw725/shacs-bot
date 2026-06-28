# AGENTS.md

> 코드를 읽으면 알 수 있는 것은 여기 적지 않는다.
> 여기에는 지뢰만 있다.

## 응답 언어

- 사용자에게는 항상 한국어로 답변할 것. 코드, 커밋 메시지, 식별자, 설정 키 이름은 기존 프로젝트 관례를 따른다.

## 제품 관점

- 이 시스템은 OpenClaw처럼 사용자가 직접 설치, 설정, 운영하는 self-hosted/personal-use 성격을 기본으로 본다.
- 별도의 운영자/관리자 조직을 기본 가정하지 말고, 기본 주체를 사용자 본인으로 간주할 것.
- 문서, 설명, 구현 제안에서 관리자 전용 워크플로우나 조직 운영 관점을 불필요하게 도입하지 말 것.

## 툴링

- **Rust는 `cargo`로만 다룬다**. 빌드/실행/테스트/체크를 다른 언어용 도구로 우회하지 말 것. 빠른 검증은 `cargo check`, 빌드는 `cargo build`, 배포용 빌드는 `cargo build --release`, 테스트는 `cargo test`를 기본으로 본다.
- **포맷/린트는 Cargo 하위 명령 기준**. 포맷은 `cargo fmt`, 린트는 `cargo clippy`를 사용한다. 검증 단계에서는 가능하면 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`까지 확인할 것.
- **Rust toolchain component를 전제로 생각하지 말 것**. `rustfmt`, `clippy`가 없다면 `rustup component add rustfmt clippy`가 먼저다.
- **레포 루트 추측 실행 금지**. Rust workspace는 `crates/Cargo.toml`이므로 저장소 루트에서 `cargo --manifest-path crates/Cargo.toml ...` 형태로 명시 실행하라.
- **`crates/`는 Rust workspace root**다. 하위 `shacs-*` 디렉터리는 workspace member crate이며, 루트 저장소에는 Cargo workspace manifest를 두지 않는다.

## 프로젝트 구조 지뢰

- **Rust crate가 하나면 workspace를 서두르지 말 것**. 루트 workspace는 여러 crate를 함께 묶어야 할 때만 도입하라. 단일 바이너리 단계에서 과한 workspace 설계는 피한다.
- **workspace를 도입하면 루트 규칙이 생긴다**. 공용 `Cargo.lock`, 공용 `target/`, 루트 기준 명령 적용 범위를 함께 고려해야 한다. 필요하면 `default-members`까지 명시할 것.
- **경로 의존성(path dependency) 추가 시 workspace 편입 범위를 확인할 것**. crate 위치를 대충 잡으면 나중에 의도치 않게 workspace 범위가 꼬일 수 있다.
- **현재 저장소 구조 메모**: Rust workspace root는 `crates/Cargo.toml`이다. 주 binary package는 `shacs-cli`, core runtime package는 `shacs-core`이며, Rust 검증/빌드 명령은 `--manifest-path crates/Cargo.toml`에 `--workspace` 또는 `-p <package>`를 붙여 실행하라.

## 산출물/재현성 지뢰

- **`Cargo.lock`은 프로그램이라면 버전 관리에 포함하는 쪽을 기본으로 본다**. manifest를 바꾸지 않았는데 lockfile만 덮어쓰지 말 것. 수동 편집도 금지.
- **빌드 산출물은 Cargo 기본 위치를 따른다**. workspace target directory는 `crates/target/`이다. `target/`을 소스처럼 다루지 말고, 필요 이상으로 다른 언어 산출물과 섞지 말 것.
- **재현성이 필요하면 toolchain을 명시할 것**. 최소 지원 버전은 `Cargo.toml`의 `rust-version`, 저장소 차원 고정이 필요하면 `rust-toolchain.toml`을 사용한다.
- **자동화/CI성 명령은 가능하면 `--locked`를 우선 고려**. 의도치 않은 lockfile 갱신을 막는다.

## 위반하면 일관성 깨지는 것

- **`unwrap()` / `expect()` 남발 금지**. 초기 프로토타입이 아니라면 실패 맥락을 드러내는 에러 전파나 메시지를 우선하라.
- **커밋할 코드에 `dbg!`, `todo!`, `unimplemented!` 남기지 말 것**. 디버깅 흔적과 미완성 표식을 런타임 경로에 남기지 않는다.
- **`#[allow(...)]`로 경고를 눌러서 끝내지 말 것**. 특히 clippy/rustc 경고를 회피용 attribute로 덮지 말고 원인을 먼저 해결하라.

## 검증 기준

- **Rust 변경의 기본 검증 순서**는 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check` → `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings` → `cargo test --manifest-path crates/Cargo.toml --workspace`다.
- **검증 범위를 명시**. 전체 workspace면 `--workspace`, 특정 crate면 `-p <package>`를 붙여 어느 crate를 검증하는지 분명히 하라.
- **사용자가 요청하지 않은 언어/서브프로젝트까지 한꺼번에 건드리지 말 것**. Rust를 건드렸다고 해서 다른 툴체인까지 리팩토링하지 않는다.

## 코딩 행동 지침

> 속도보다 신중함에 비중을 둡니다. 사소한 작업은 상황에 맞게 판단하십시오.

### 1. 코딩 전에 생각하라

**가정하지 말 것. 혼란을 숨기지 말 것. 트레이드오프를 드러낼 것.**

- 자신의 가정을 명시적으로 밝혀라. 확실하지 않다면 질문하라.
- 해석이 여러 가지라면 모두 제시하라 — 임의로 하나를 선택하지 마라.
- 더 단순한 접근 방식이 있다면 반드시 언급하라. 필요하면 합리적으로 반박하라.
- 무엇인가 불분명하다면 멈춰라. 무엇이 헷갈리는지 명확히 말하고 질문하라.

### 2. 단순함 우선

**문제를 해결하는 최소한의 코드만 작성하라. 추측성 구현 금지.**

- 요청되지 않은 기능은 추가하지 마라.
- 단일 용도의 코드를 위해 불필요한 추상화를 만들지 마라.
- 요청되지 않은 "유연성"이나 "설정 가능성"을 추가하지 마라.
- 발생할 수 없는 시나리오를 위한 에러 처리를 만들지 마라.
- 200줄을 썼는데 50줄로 가능하다면 다시 작성하라.

> "시니어 엔지니어가 이 코드를 과도하게 복잡하다고 말할까?" — 그렇다면 단순화하라.

### 3. 외과적 변경

**정말 필요한 부분만 수정하라. 자신의 변경으로 생긴 것만 정리하라.**

- 주변 코드, 주석, 포맷을 "개선"하지 마라.
- 고장 나지 않은 것을 리팩토링하지 마라.
- 본인이 선호하는 스타일이 있더라도 기존 스타일에 맞춰라.
- 관련 없는 데드 코드가 보이면 언급만 하고 삭제하지 마라.
- 본인의 변경으로 사용되지 않게 된 import/변수/함수는 제거하라.
- 기존에 존재하던 데드 코드는 요청받지 않았다면 제거하지 마라.

> 모든 변경된 라인은 사용자의 요청과 직접적으로 연결되어야 한다.

### 4. 목표 중심 실행

**성공 기준을 정의하고, 검증될 때까지 반복하라.**

작업을 검증 가능한 목표로 변환하라:
- "유효성 검증 추가" → "잘못된 입력에 대한 테스트를 작성하고 통과시키기"
- "버그 수정" → "버그를 재현하는 테스트를 작성하고 통과시키기"
- "X 리팩토링" → "리팩토링 전후 테스트가 모두 통과하는지 확인하기"

## 작업 기록

- 작업 완료 후, 자신의 변경이 사용자 문서/README/스펙/PRD/CLI 사용법/설정 가이드에 영향을 주면 관련 문서를 같은 작업 안에서 함께 업데이트할 것.
- 문서 업데이트가 불필요하다고 판단한 경우에도 왜 불필요한지 한 번은 점검하고 넘어갈 것.
