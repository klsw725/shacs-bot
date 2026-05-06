# shacs-bot

`shacs-bot`은 `nanobot`의 Rust porting이며, 개인 사용/자체 호스팅 운영을 기본으로 하는 Rust 에이전트 런타임입니다. 로컬 CLI, OpenAI 호환 HTTP API, 세션/런타임 유틸리티, provider adapter, tool, skill, template, 그리고 선택된 channel worker를 제공합니다.

- 원본 nanobot 저장소: <https://github.com/HKUDS/nanobot>

## 빠른 시작

현재 저장소는 crate별 Cargo manifest를 기준으로 실행합니다. 저장소 루트에서 `--manifest-path crates/shacs-cli/Cargo.toml`을 붙여 명령을 실행하세요.

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- --help
```

설정 파일과 workspace template을 생성하거나 갱신합니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- onboard --workspace /tmp/shacs-ws
```

설정과 runtime 상태를 확인합니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- status
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- runtime inspect --workspace /tmp/shacs-ws
```

로컬 agent turn을 한 번 실행합니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- ask "hello" --workspace /tmp/shacs-ws
```

선택된 channel runtime worker를 시작합니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- run --workspace /tmp/shacs-ws
```

로컬 OpenAI 호환 HTTP API를 시작합니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- serve --workspace /tmp/shacs-ws
```

CLI binary를 빌드해서 실행합니다:

```sh
cargo build --manifest-path crates/shacs-cli/Cargo.toml
./crates/shacs-cli/target/debug/shacs-bot --help
```

## 채널

`shacs-bot run`은 WebSocket channel이 enabled 상태이면 WebSocket server를 시작하고, 외부 channel plugin config에 필요한 인증 정보가 있으면 해당 transport도 함께 시작합니다. 외부 인증 정보가 없으면 전체 runtime을 실패시키지 않고 `skipped-missing-credentials`로 보고합니다.

현재 실행 가능한 상태로 연결된 channel transport는 다음과 같습니다:

- WebSocket server
- Telegram long polling
- Discord REST polling
- Slack REST polling
- Email SMTP/IMAP
- WhatsApp HTTP bridge

Channel 설정과 worker 상태를 확인합니다:

```sh
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- channels list
cargo run --manifest-path crates/shacs-cli/Cargo.toml -- channels status --workspace /tmp/shacs-ws
```

## 검증

Cargo 명령은 manifest path를 명시해서 실행합니다:

```sh
cargo fmt --manifest-path crates/shacs-cli/Cargo.toml -- --check
cargo clippy --manifest-path crates/shacs-cli/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/shacs-cli/Cargo.toml
cargo build --manifest-path crates/shacs-cli/Cargo.toml
```

Channel crate를 수정한 경우:

```sh
cargo fmt --manifest-path crates/shacs-channels/Cargo.toml -- --check
cargo clippy --manifest-path crates/shacs-channels/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/shacs-channels/Cargo.toml
cargo build --manifest-path crates/shacs-channels/Cargo.toml
```

## 추가 문서

- 사용자 가이드: [`docs/USAGE.md`](docs/USAGE.md)
- 스펙 인덱스: [`docs/specs/README.md`](docs/specs/README.md)
