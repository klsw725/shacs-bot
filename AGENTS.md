# AGENTS.md

> 코드를 읽으면 알 수 있는 것은 여기 적지 않는다.
> 여기에는 지뢰만 있다.

## 응답 언어

- 사용자에게는 항상 한국어로 답변할 것. 코드, 커밋 메시지, 식별자, 설정 키 이름은 기존 프로젝트 관례를 따른다.
- 사용자에게 하는 설명과 보고는 비개발자에게 설명하듯 쉽게 쓸 것. 개발 용어는 피하거나 쉬운 말로 풀어 쓰고, 필요하면 일상적인 비유를 든다. 스펙·PRD·코드처럼 저장소에 남기는 문서의 문체에는 적용하지 않는다.

## 제품 관점

- 이 시스템은 OpenClaw처럼 사용자가 직접 설치, 설정, 운영하는 self-hosted/personal-use 성격을 기본으로 본다.
- 별도의 운영자/관리자 조직을 기본 가정하지 말고, 기본 주체를 사용자 본인으로 간주할 것.
- 문서, 설명, 구현 제안에서 관리자 전용 워크플로우나 조직 운영 관점을 불필요하게 도입하지 말 것.

## 툴링

- **Rust는 `cargo`로만 다룬다**. 빌드/실행/테스트/체크를 다른 언어용 도구로 우회하지 말 것. 빠른 검증은 `cargo check`, 빌드는 `cargo build`, 배포용 빌드는 `cargo build --release`, 테스트는 `cargo test`를 기본으로 본다.
- **빌드 전에는 기존 빌드 데이터를 정리한다**. 빌드 산출물이 과도하게 누적되지 않도록 `cargo clean --manifest-path crates/Cargo.toml`을 먼저 실행한 뒤 `cargo build --manifest-path crates/Cargo.toml`을 실행한다. `cargo check`/`clippy`/`test` 앞에서는 `cargo clean`을 실행하지 않는다. 매번 처음부터 다시 컴파일하게 된다.
- **포맷/린트는 Cargo 하위 명령 기준**. 포맷은 `cargo fmt -- --config imports_granularity=Module,format_macro_matchers=true`, 린트는 `cargo clippy`를 사용한다. 검증 단계에서는 가능하면 같은 `--config`를 붙인 `cargo fmt -- --check`와 `cargo clippy --all-targets -- -D warnings`까지 확인할 것. `--config`를 붙이는 이유는 아래 'Google Rust 스타일'에 있다.
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
- **`rust-version`은 21개 crate가 같은 값(지금 `1.88`)을 쓴다**. `Cargo.lock`의 의존성이 요구하는 가장 높은 `rust-version`과 `Dockerfile`의 `rust:<버전>` 이미지에 맞춘 값이니, 바꿀 때는 셋을 함께 바꾼다. clippy는 이 값을 기준으로 더 새 표준 라이브러리 기능을 막고(`incompatible_msrv`) 그 버전에서 쓸 수 있는 문법을 제안한다.
- **자동화/CI성 명령은 가능하면 `--locked`를 우선 고려**. 의도치 않은 lockfile 갱신을 막는다.

## 위반하면 일관성 깨지는 것

- **`unwrap()` / `expect()` 남발 금지**. 초기 프로토타입이 아니라면 실패 맥락을 드러내는 에러 전파나 메시지를 우선하라.
- **커밋할 코드에 `dbg!`, `todo!`, `unimplemented!` 남기지 말 것**. 디버깅 흔적과 미완성 표식을 런타임 경로에 남기지 않는다.
- **`#[allow(...)]`로 경고를 눌러서 끝내지 말 것**. 특히 clippy/rustc 경고를 회피용 attribute로 덮지 말고 원인을 먼저 해결하라. 원인을 없앨 수 없는 검토된 예외만 아래 'Google Rust 스타일'에 적은 방식으로 표시한다.

## Google Rust 스타일

- **새로 쓰거나 고치는 Rust 코드는 [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html)를 따른다**. 주변 기존 코드가 다르게 쓰여 있어도 새 줄은 이 규칙을 따르되, 손대지 않는 기존 코드를 이 규칙만을 이유로 고치지 않는다. 이 저장소는 Google 분류상 application code라 API 설계 규칙은 강한 권장이고, `[chrome]`·`[Fuchsia]` 표시 규칙(작은 crate, 평평한 파일 배치, `async` 금지 등)은 적용하지 않는다.
- **rustfmt의 Google 옵션 두 개는 명령줄로만 적용된다**. `imports_granularity = "Module"`과 `format_macro_matchers = true`는 nightly 전용이라 안정판 rustfmt는 `crates/rustfmt.toml`에 적어도 경고만 내고 무시한다. 그래서 `cargo fmt`에는 항상 `-- --config imports_granularity=Module,format_macro_matchers=true`를 붙인다. 옵션 없이 돌린 `--check`도 통과하므로 위반을 잡지 못한다.
- **`cargo fmt`는 `mod` 선언을 따라가며 파일을 찾는다**. 모듈 선언을 `macro_rules!` 안에 두면 그 아래 파일 전체가 포맷과 `--check`에서 조용히 빠진다(Spec034 모듈 97개 파일이 그랬다). 모듈 선언을 매크로로 감싸지 말 것.
- **lint 수준은 `crates/Cargo.toml`의 `[workspace.lints.clippy]`, 금지 메서드와 테스트 `unwrap()` 허용은 `crates/clippy.toml`에 있다**. 새 member crate에는 `[lints]` 아래 `workspace = true`를 넣어야 이 검사가 적용된다.
- **검토된 예외만 `#[expect(<lint>, reason = "...")]`로 표시한다**. `#[allow]`와 달리 예외가 필요 없어지면 경고가 나서 남지 않는다. 지금 예외는 serde `serialize_with` 함수가 받아야 하는 `&Option<T>` 인자(`clippy::ref_option`)와 아래 `catch_unwind` 경계뿐이다. 새 예외가 필요하면 먼저 사용자에게 묻는다.
- **poisoned `Mutex`의 값을 꺼내 쓰지 않는다**. `PoisonError::into_inner`는 clippy가 막는다. 잠금 실패는 이렇게 다룬다.
  - 콜백이나 trait 구현처럼 호출자가 넘긴 코드는 잠금을 쥔 채 부르지 않는다. 그 panic이 잠금을 망가뜨리면 뒤의 호출이 모두 멈춘다(그래서 message bus matcher와 MCP connector `close`는 잠금 밖에서 돈다). 잠금 밖으로 뺄 수 없으면(durable event visitor, cron executor) 그 잠금의 실패를 `Result` 오류로 돌려준다.
  - 그 밖에도 호출자가 처리할 수 있으면 오류로 돌려준다.
  - 잠금 실패를 빈 목록·`None`·`NotFound` 같은 정상 결과로 바꾸지 않는다. 재시작 뒤 다시 읽지 않는 진단용 기록만 그 쓰기를 건너뛸 수 있다. 커서·체크포인트처럼 나중에 읽는 값을 건너뛰거나 빈 값으로 대신하면 지난 메시지를 다시 처리하게 된다.
  - 반환형에 오류를 담을 수 없으면 `.expect("<무엇> lock poisoned")`로 멈춘다.
  - `Drop`과 `Drop`이 부르는 함수에서는 멈추지 말고 `if let Ok(..) = mutex.lock()`으로 건너뛴다. unwinding 중 두 번째 panic은 프로세스를 바로 abort한다.
- **`catch_unwind`를 새로 쓰지 말고 검토된 경계를 호출한다**. hook·callback panic 격리는 `crates/shacs-core/src/runtime/runner.rs`의 `isolate_panic`(Spec 003/025/030), facade hook 격리와 external turn panic 복구는 `crates/shacs-cli/src/lib.rs`의 `isolate_panic`(docs/USAGE.md, Spec 029)이다. 나머지 하나는 Spec034 release runner의 최종 검증이다. `McpRuntime::close`도 core의 `isolate_panic`으로 서버마다 `McpConnector::close`를 감싸, 하나가 panic해도 나머지를 닫은 뒤 그 panic을 다시 던진다(이미 unwinding 중이면 던지지 않는다).
- **도구가 잡지 못하는 규칙**:
  - 하위 모듈이 있는 파일에는 `mod` 선언과 re-export만 두고 구현은 leaf 모듈에 둔다. leaf 모듈 파일 이름은 안의 주된 타입 이름을 따른다.
  - 타입과 trait은 `use`로 가져와 짧은 이름으로 쓰고, 다른 모듈의 자유 함수는 `module::function()`으로 부른다. 파일 전체에서 쓰지 않는 trait은 쓰는 함수 안에서 import한다.
  - 인자는 감싼 타입이 아니라 데이터를 빌려 받는다(`&str`, `&[T]`, `Option<&T>`). application code에서는 `impl Into<T>` 인자를 쓰지 않고, 뜻이 드러나지 않는 `bool` 인자는 enum으로 바꾼다.
  - `Rc`/`Arc`, 빌림 대신 쓰는 `.clone()`은 소유 구조가 지나치게 복잡해질 때만 쓴다.
  - `.into()`·`.parse()`·긴 iterator chain의 결과에는 변수 타입을 적는다. `for` 머리에 combinator chain을 만들지 말고 변수에 담아 쓴다.
  - 값이 있는지 확인한 뒤 `unwrap()`하지 말고 `if let`/`match`로 받는다. panic이 필요하면 이유를 적은 `expect()`를 쓰고, `assert!`에는 왜 성립해야 하는지 메시지를 붙인다.
  - 문자열을 셋 이상 잇거나 치환할 때는 `format!`/`write!`을 쓴다. 프로젝트 밖 사람이 모를 약어를 쓰지 않는다.

## 검증 기준

- **Rust 변경의 마무리 검증 순서**는 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true` → `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings` → `cargo test --manifest-path crates/Cargo.toml -p <변경한 package> -q`다. 수정할 때마다 돌리지 말고 작업 끝에 실행하며, 변경한 package가 여러 개면 `-p`를 반복한다.
- **전체 테스트(`cargo test --manifest-path crates/Cargo.toml --workspace -q`)는 사용자 승인 없이 돌리지 않는다**. 다른 crate가 쓰는 공개 동작을 바꿨으면 실제 사용처를 찾아 그 package의 관련 테스트만 돌린다. 영향 범위를 좁히기 어렵거나 Rust 코드 변경을 커밋하기 전이면 전체 테스트를 돌릴지 먼저 묻는다. `clippy --workspace --all-targets`는 다른 crate의 타입 오류는 잡지만 동작이 바뀐 것은 잡지 못한다.
- **검증 범위를 명시**. 전체 workspace면 `--workspace`, 특정 crate면 `-p <package>`를 붙여 어느 crate를 검증하는지 분명히 하라.
- **사용자가 요청하지 않은 언어/서브프로젝트까지 한꺼번에 건드리지 말 것**. Rust를 건드렸다고 해서 다른 툴체인까지 리팩토링하지 않는다.

## 테스트 간소화

- **작업 중 반복 확인은 변경한 package의 `cargo check`와 관련 테스트만**. 컴파일 확인은 `cargo check --manifest-path crates/Cargo.toml -p <package>`로 충분하다. 테스트는 `--lib`(단위 테스트), `--test it`(합쳐진 통합 테스트) 또는 `--test <파일명>`(따로 남은 통합 테스트 파일)으로 대상을 좁히고 이름 필터를 붙인다. 예: `cargo test --manifest-path crates/Cargo.toml -p <package> --test it -q <모듈 이름>::`. 이름 필터만 쓰면 그 package의 테스트 실행 파일이 전부 빌드·실행된다.
- **`cargo test`에는 항상 `-q`를 붙인다**. 통과한 테스트를 한 줄씩 출력하지 않아 결과가 짧아진다.
- **검증 대상(코드·설정·fixture)이 바뀌지 않았으면 테스트를 다시 돌리지 않는다**. 통과를 재확인하려고 재실행하지 말 것. 고친 뒤에는 실패했던 테스트와 영향받는 테스트만 다시 돌린다.
- **자신의 변경과 무관한 기존 실패는 쫓지 말 것**. 고치지 말고 실패한 테스트 이름만 보고한다.
- **문서(`.md`)만 바꿨으면 Cargo 검증을 생략한다**.
- **새 테스트는 바뀐 동작에만, 최소 개수로**. 버그 수정은 재현 테스트 1개, 새 기능은 핵심 성공 경로와 꼭 필요한 실패 경로 정도면 충분하다. 입력 조합을 전부 나열하지 말고, 기존 테스트에 assert를 추가해 확인할 수 있으면 새 테스트를 만들지 않는다.
- **가치 없는 테스트 금지**. 단순 getter/생성자, 속성 없는 단순 derive(serde 왕복 등), 타입 시스템이 이미 보장하는 것, 기존 테스트와 같은 경로를 다시 확인하는 테스트는 쓰지 않는다. 저장·공개 직렬화 형식(`rename`, `default` 등)을 지키는 테스트는 예외다.
- **통합 테스트 실행 파일을 새로 늘리지 말 것**. `tests/` 바로 아래 `.rs` 파일은 하나하나가 별도 실행 파일로 컴파일·링크되어 전체 테스트가 느려진다. 새 통합 테스트는 `tests/it/` 아래에 두고 `tests/it/main.rs`에 모듈로 등록한다. `tests/it/`이 없는 crate는 기존 테스트 파일에 추가한다.
- **`tests/` 바로 아래에 남은 통합 테스트 파일을 이유 확인 없이 `tests/it/`으로 옮기지 말 것**. 출시 점검 도구(release runner)·다른 코드·예제·문서가 이름이나 경로로 직접 부르거나, 환경 변수 변경·자기 재실행처럼 별도 프로세스가 필요해서 남긴 파일이다.

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
- "X 리팩토링" → "리팩토링 전후 관련 테스트가 통과하는지 확인하기"

## 작업 기록

- 작업 완료 후, 자신의 변경이 사용자 문서/README/스펙/PRD/CLI 사용법/설정 가이드에 영향을 주면 관련 문서를 같은 작업 안에서 함께 업데이트할 것.
- 문서 업데이트가 불필요하다고 판단한 경우에도 왜 불필요한지 한 번은 점검하고 넘어갈 것.

## 스펙 작성 원칙

- **각 스펙은 독립적으로 완결되어야 한다**. 다른 스펙이 완료되어야만 해당 스펙을 완료로 처리할 수 있다면, 그 의존 범위는 현재 스펙에 포함하지 말고 별도의 후속 스펙으로 분리할 것.
