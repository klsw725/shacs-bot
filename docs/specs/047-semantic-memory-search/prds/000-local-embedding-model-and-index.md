# PRD 000. local 임베딩 모델과 색인

Status: Complete (Scoped)

Depends on: 없음. [046](../../046-sqlite-durable-event-store/SPEC.md)이 정한 `rusqlite` 판과 SQLite 설정을 따를 뿐 046의 구현·완료를 기다리지 않는다. 뒤 번호 PRD에 의존하지 않는다.

## Goal

기억 파일에서 다시 만들 수 있는 검색 색인을 만들고, 그 벡터를 shacs-bot process 안의 local 모델로 계산한다. 모델 파일은 사용자가 한 번 설치한다.

## Scope

1. **의존성:** `shacs-core`에 아래를 정확한 판으로 더한다. Rust 1.88로 빌드되지 않으면 이 PRD를 멈추고 사용자에게 묻는다. Rust 판 변경은 AGENTS.md가 정한 세 곳(`rust-version`·`Cargo.lock`·`Dockerfile`)을 함께 바꾸는 별도 결정이다.
   - `candle-core`·`candle-nn`·`candle-transformers` `=0.11.1`(기본 feature, CPU만)
   - `tokenizers = { version = "=0.23.2", default-features = false, features = ["fancy-regex"] }`(C 라이브러리 `onig`, C++ `esaxx_fast`, download를 쓰지 않는다)
   - `rusqlite = { version = "=0.40.2", features = ["bundled"] }`(046과 같은 판)
   - Linux arm64에서는 `gemm-f16`이 FP16 명령을 요구한다(SPEC의 "반드시 지킬 것"). Dockerfile의 builder 단계는 `ARG TARGETARCH`를 받아, 그 값이 `arm64`일 때만 그 `RUN`의 cargo 실행에 `RUSTFLAGS="-C target-feature=+fp16"`를 준다. 전역 `ENV`, 저장소의 `.cargo/config`, 다른 대상의 빌드 설정은 바꾸지 않는다.
2. **모델 manifest:** 아래 값을 코드에 고정한다. 값은 2026-10-31에 Hugging Face API(LFS sha256)와 고정 revision의 파일을 직접 받아 확인했고, 구현 때 다시 받아 대조한다.

   | 항목 | `dragonkue/multilingual-e5-small-ko-v2` | `nlpai-lab/KURE-v1` |
   |---|---|---|
   | revision | `fcfc26bf355882620c48df58be112275bd756f50` | `8b418a58414668e75532ed045c22d9ca018ae2b2` |
   | 구조 | `BertModel`, hidden 384, 12층 | `XLMRobertaModel`, hidden 1024, 24층 |
   | 차원 | 384 | 1024 |
   | 입력 접두어 | 질문 `query: `, 항목 `passage: ` | 없음 |
   | pooling·정규화 | 평균 pooling, L2 정규화 | CLS pooling, L2 정규화 |
   | 최대 입력 | 512 token | 512 token에서 자른다(모델 한도 8192) |

   | 파일 | e5-small-ko-v2 크기 / sha256 | KURE-v1 크기 / sha256 |
   |---|---|---|
   | `config.json` | 628 / `d99cc57db7529a5b6921f38d899dafad5bb829e21078900c8ba2c4ae9d141acb` | 807 / `852d42e020c7f989c2acaf30fc683b7f768e8c6d1ab17166e835442162bd825d` |
   | `tokenizer.json` | 17,082,800 / `cd98e5698b201ba914efb8c18b6709fa8735ab71dcad8d2b431e52e8bf68d932` | 17,083,053 / `fb3c3b93c46fd5a8634e262e1b7de7da11a18b527aa2282b312952b692781dfd` |
   | `model.safetensors` | 470,637,416 / `9794f247caf80caf54b5a04d391c932ba6260cf20bd16fba3892ce2d8f5784ec` | 2,271,064,456 / `c18156e80caf8ff45eb84a24a853130c3bca03087ccb41b051f86e7556bae02c` |
   | `1_Pooling/config.json` | 296 / `a19c83805e1ce4174f3fbfec4ac8d3b8dbae0c958f8fd51b80937eb33e0c5335` | 297 / `13e69897522ee8255104483ed9f219465d1be3936654a54a318758738052789e` |
   | `modules.json` | 349 / `84e40c8e006c9b1d6c122e02cba9b02458120b5fb0c87b746c41e0207cf642cf` | 349 / `84e40c8e006c9b1d6c122e02cba9b02458120b5fb0c87b746c41e0207cf642cf` |
   | `sentence_bert_config.json` | 53 / `ec8e29d6dcb61b611b7d3fdd2982c4524e6ad985959fa7194eacfb655a8d0d51` | 54 / `eb9b44b13c0f52a3b3685c3b1cbdea1ba8b04bea123b98f61610048940776eb1` |

3. **모델 실행:** candle의 `bert`·`xlm_roberta` 구현으로 두 모델을 CPU에서 돌린다. tokenizer는 각 모델의 `tokenizer.json`을 쓰고, 접두어·pooling·정규화·최대 입력은 manifest를 따른다. 모델은 처음 쓸 때 올리고 그 process 안에서 다시 쓴다.
4. **모델 설치:** `shacs-bot memory model install [--model <id>]`가 `https://huggingface.co/<id>/resolve/<revision>/<file>`에서 manifest의 파일을 받는다. download는 010 network guard를 거친다. 파일은 `<data_dir>/runtime/memory-search/models/<org>--<name>@<revision>/`(0700) 안에 0600 임시 파일로 받고, 크기와 sha256이 맞을 때만 이름을 바꿔 둔다. 중간에 끊기거나 확인에 실패해도 이미 확인된 파일은 그대로 두고, 확인되지 않은 파일이 최종 이름으로 남지 않는다. 다시 실행하면 확인되지 않은 임시 파일을 지우고 다시 받는다. 사용자가 같은 파일을 직접 넣으면 같은 확인을 거친다. 경로의 어느 부분(상위 폴더 포함)이든 symlink이면 쓰지 않는다.
5. **색인 파일:** `<data_dir>/runtime/memory-search/index/<canonical workspace 경로의 sha256>.sqlite3`(폴더 0700, DB·`-wal`·`-shm` 0600, 상위 폴더와 부속 파일을 포함해 symlink 거절, 046의 SQLite 설정). 항목 테이블은 key, 출처, 항목 text, 검색용 text(항목 text를 Unicode NFC로 맞추고 Unicode 소문자로 바꾼 것), text digest, 검색용 text로 만든 FTS5(trigram) 색인, 벡터(f32 BLOB, 없으면 NULL)를 담는다. 메타 테이블은 색인 schema 판(1), 모델 id·revision·차원, 나누기 규칙 판(1), canonical workspace 경로 digest, 다시 만들 때마다 1씩 느는 색인 세대 번호를 담는다. 메타의 어느 값이든 지금과 다르면(다른 workspace의 색인을 복사해 온 경우 포함) 색인을 비우고 새 세대로 다시 만든다.
6. **나누기 규칙(판 1):** `history.jsonl`은 `MemoryStore`와 같은 방식으로 읽고, 읽지 못한 줄은 건너뛰며 그 개수를 상태에 남긴다. 각 항목의 content와 `MEMORY.md`를 줄 단위로 보고, `- `로 시작하는 줄은 한 항목, 그 밖의 빈 줄로 나뉜 줄 묶음은 한 문단 항목으로 만든다. 빈 줄과 `#` 제목 줄은 버린다.
7. **key와 출처:** history 항목의 key는 `h:{cursor}:{순번}:{text digest 앞 16자}`이고 출처는 cursor와 timestamp다. `MEMORY.md` 항목의 key는 `m:{text digest 앞 16자}:{같은 text의 순번}`이고 출처는 지금 파일의 줄 범위다. 출처는 key에 넣지 않고 맞추기마다 지금 값으로 덮어쓴다.
8. **원본과 맞추기:** 색인을 쓰기 전마다 두 기억 파일을 한 번씩 읽어 지금의 항목 목록(snapshot)을 만든다.
   - 한 SQLite transaction 안에서 목록에 없는 key의 행을 지우고, 있는 key는 text와 출처를 snapshot 값으로 덮어쓰고, 새 key는 글자 색인과 함께 더한다. 검색은 이 transaction이 commit된 색인으로만 한다.
   - 맞추기가 실패하면 이전 색인을 읽지 않고, 방금 읽은 snapshot과 `index_unavailable`을 담은 실패 값을 돌려준다(그 snapshot으로 하는 글자 검색은 PRD 010이 정한다). 원본을 읽지 못했으면 원본 읽기 실패를 돌려준다.
   - 벡터는 맞추기 transaction 뒤에 따로 계산하며, 한 번에 256개까지 계산하고 남은 개수를 상태에 남긴다. 계산을 시작할 때의 색인 세대와 행의 text digest를 기억해 두고, 저장할 때 한 transaction 안에서 둘 다 그대로인 행에만 넣는다. 그 사이 다른 process가 색인을 다시 만들었으면 계산한 벡터를 버린다. 계산한 벡터에 유한하지 않은 값이 있거나 크기가 0이거나 차원이 다르면 저장하지 않고 실패 개수로 센다.
9. **`shacs-bot memory index`:** 위 맞추기를 벡터까지 끝까지 실행하고 상태(항목 수, 남은 벡터 수, 건너뛴 줄 수, 모델)를 출력한다.
10. **가짜 임베딩:** 테스트용으로 text를 결정적으로 벡터로 바꾸는 구현과, 불러오기·계산 실패를 일으키는 구현을 둔다. production 경로에서는 쓰지 않는다.
11. **034 release runner의 의존성 트리 한도:** `crates/shacs-core/src/runtime/generated_media_release/source_descriptor.rs`의 트리 한도를 트리 종류별로 나눈다.
    - 의존성 트리(`TreeKind::Cache`: `cargo vendor` 결과, tool 캐시, runtime library)는 2 GiB, 항목 131,072개.
    - 작업 트리·git 트리(`Worktree`·`LiveWorktree`·`Git`·`ControlledGit`)는 지금처럼 512 MiB, 항목 32,768개.
    - 바이트 한도를 넘으면 항목 수 한도처럼 `InvalidEvidence`로 거절한다. 지금은 `DigestMismatch`로 보고되어, 읽는 동안 바뀐 파일과 구별되지 않는다. 읽는 동안 바뀐 파일은 계속 `DigestMismatch`다.
    - 한도는 자원 사용만 묶는다. 각 트리의 확인은 그대로다. `cargo vendor` 결과는 `Cargo.lock`의 checksum으로 파일마다 확인하고, tool 캐시는 manifest와 source closure digest로, runtime library는 경로 seal과 복사본 digest로 확인한다.

## Non Scope

- `memory_search` tool, 검색 규칙, skill 안내, `memory search` CLI, config, 평가와 사용자 문서([PRD 010](010-memory-search-tool-and-evaluation.md)).
- 다른 모델, GPU·Metal 가속, 외부 임베딩 서버, sqlite-vec.

## Parent Requirement Mapping

- [047 SPEC](../SPEC.md)의 소유 범위 1–4·8, `memory model install`·`memory index`, 수용 조건 3·4·7, 이 PRD 변경에 대한 수용 조건 6을 소유한다.

## Required Contract

1. 색인은 원본에서 다시 만들 수 있다. 색인을 지우고 다시 만든 결과는 같다.
2. 맞추기 뒤 색인의 항목과 출처는 그 맞추기가 읽은 원본과 같다. 원본에 없는 항목은 남지 않는다.
3. 맞추기에 실패하면 이전 색인을 읽지 않고, snapshot과 실패 이유를 돌려준다.
4. 한 색인 안의 벡터는 모두 그 세대의 모델·revision·차원으로 만든 유효한 벡터다.
5. 검색과 색인은 네트워크를 쓰지 않는다. 네트워크는 모델 설치 명령에서만 쓴다.
6. 크기나 sha256이 manifest와 다른 모델 파일은 쓰지 않고 이유를 알린다.

## Acceptance Criteria

1. 가짜 임베딩으로 색인을 만들고 지운 뒤 다시 만들면 key·text·출처·벡터가 같다.
2. history 압축(`compact_history`)으로 사라진 항목, 손으로 고친 history 줄, Dream이 지우거나 바꾼 `MEMORY.md` 항목이 다음 맞추기 뒤 색인에 없다. timestamp만 고친 history 항목과 줄 위치만 옮겨진 `MEMORY.md` 항목은 출처가 새 값으로 바뀐다. 새 항목만 벡터가 계산된다.
3. 맞추기 중 SQLite 실패를 일으키면 이전 색인의 행을 읽지 않고, 방금 읽은 snapshot과 `index_unavailable`이 담긴 실패 값이 나온다.
4. 벡터 상한을 넘는 항목이 있으면 남은 개수가 상태에 나오고, `memory index`를 실행하면 0이 된다. 유효하지 않은 벡터를 내는 가짜 임베딩에서는 그 항목의 벡터가 저장되지 않고 실패 개수로 센다. 벡터를 계산하는 사이 다른 연결이 모델 메타를 바꿔 색인을 새 세대로 다시 만들면, 이전 모델로 계산한 벡터는 버려진다.
5. NFD로 적힌 원본 항목과 대소문자가 섞인 원본 항목의 검색용 text가 NFC 소문자로 저장된다.
6. 메타의 색인 schema 판, 모델, revision, workspace digest 중 하나를 바꾸거나 다른 workspace의 색인 파일을 복사해 오면 다음 맞추기에서 색인이 새 세대로 다시 만들어진다. 한 data directory를 쓰는 두 workspace는 서로 다른 색인 파일을 쓴다.
7. 모델 설치를 중간에 끊거나 sha256이 틀린 파일을 받게 하면, 이미 확인된 파일은 그대로이고 확인되지 않은 파일은 최종 이름으로 남지 않는다. 크기나 sha256이 틀린 모델 파일은 거절된다.
8. 모델 파일, 색인 파일, 색인의 `-wal`·`-shm`, 그 상위 폴더 중 하나를 symlink로 바꾸면 거절된다. Unix에서 폴더 0700, 파일(`-wal`·`-shm`·임시 파일 포함) 0600이다. 설치 명령이 아닌 경로에서는 네트워크 호출이 없다.
9. 실제 기본 모델로 만든 벡터가 참조 구현(Hugging Face sentence-transformers, 같은 revision)의 벡터와 cosine 0.99 이상으로 같음을 512 token 아래의 한국어 문장 10개로 한 번 확인해 기록한다.
10. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다.
11. Dockerfile의 builder 단계가 linux/arm64에서 Rust 1.88 `--locked`로 빌드된다(위 flag로). macOS에서 flag 없이 빌드된다.
12. Spec 034 release runner 시험(`cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test spec034_release_runner`)이 047 변경을 포함한 작업 트리의 사본(저장소 밖 짧은 경로)에서 통과한다. 단위 시험은 세 가지를 보인다.
    - 트리 종류별 한도 값.
    - 바이트·항목이 한도와 같으면 받고, 하나라도 넘으면 `InvalidEvidence`로 거절한다.
    - 읽는 동안 바뀐 파일은 `DigestMismatch`다.

## Closure Evidence

1. 모델 실행·설치·색인·맞추기 구현과 테스트.
2. 다시 받아 대조한 manifest 값, 기본 모델의 벡터 대조 기록, Rust 1.88 빌드(Docker linux/arm64)와 macOS 빌드, `Cargo.lock` 변경 내용.
3. Cargo 검증 기록.
4. 034 runner 실행 기록(047 변경을 포함한 사본과 뺀 사본)과 의존성 트리 크기 측정.

PRD 000은 검색 tool 없이 닫힌다. 뒤 번호 PRD의 완료를 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-11-02)

- 구현:
  - `crates/shacs-core/src/memory_search/`: manifest, 경로·권한, 확인한 모델 파일, 설치기, `CandleEmbedder`, `LazyEmbedder`와 loader, 가짜 임베딩, 나누기 규칙, 색인.
  - CLI `memory model install`·`memory index`(`crates/shacs-cli/src/memory_cli.rs`).
  - Dockerfile arm64 flag.
  - 034 runner의 트리 종류별 한도(`generated_media_release/source_descriptor.rs`).
- 시험:
  - Spec 047 시험 37개(색인 21, 모델 파일 14, CLI 2)와 runner 단위 시험 3개.
  - 변이 시험 55개를 모두 잡았다. 시험으로 만들 수 없는 방어 분기 두 곳(권한을 바꿀 수 없는 모델 파일, CLI의 다시 맞추기 실패 보고)은 기록에 따로 적었다.
- 실제 모델:
  - 기본 모델을 release binary로 설치하고 한국어 문장 10개로 `memory index`를 실행했다. Hugging Face sentence-transformers(같은 revision)와 비교한 cosine 최저값은 0.99999999999807이다.
  - KURE-v1과 질문 벡터도 같은 수준으로 일치했다.
  - 불러오는 동안 최대 RSS는 기본 모델 1,311 MiB, KURE-v1 3,215 MiB였다. 이 값은 PRD 010 사용자 문서에 적는다.
- manifest: 두 모델 12개 파일의 크기와 sha256을 Hugging Face와 다시 대조했고, 모두 일치했다.
- 실행 기록은 로컬 `.omo/evidence/spec047/prd000/`(추적하지 않음)에 있고, `implementation.md`에 정리했다.

## 종료 확인 (2026-11-02)

- 범위: 소유 범위 1–4·8, 설치·색인 CLI, 가짜 임베딩. `memory_search` tool, config, skill 안내, 사용자 문서는 [PRD 010](010-memory-search-tool-and-evaluation.md)이 맡는다.
- 최종 검증(최종 소스):
  - Google 옵션을 붙인 `cargo fmt --all -- --check`와 workspace clippy `--all-targets -D warnings`가 통과했다.
  - `cargo clean` 뒤 macOS `cargo build --locked -p shacs-cli`(debug·release, flag 없음)가 통과했다.
  - Docker `rust:1.88-bookworm` linux/arm64 builder 단계의 `cargo build -p shacs-cli --release --locked`(arm64 flag)가 통과했다.
- 전체 workspace 테스트: 사용자 승인을 받아 `--no-fail-fast`로 실행했다(`closure/workspace-tests-final.log`). suite 184개에서 3,341 passed / 0 failed / 7 ignored이고, 테스트가 띄운 자식 process 실행 26번도 모두 통과했다.
- 034 runner: 사용자 승인을 받아 `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test spec034_release_runner`를 실행했다.
  - 이 checkout에서는 046 때와 같이 6개 중 5개가 `InvalidConfig`로 실패한다(`closure/spec034-runner-checkout.log`, `closure/spec034-checkout-conditions.txt`). 추적하지 않는 로컬 symlink, runner 한도를 넘는 `docs/refs/`, 없는 로컬 fixture 때문이며, 이 변경과는 관계없다.
  - 짧은 경로의 사본(`closure/spec034-runner-copy.sh`)에서는 범위 11 전에 6개 중 5개가 `DigestMismatch`로 실패했다(`closure/spec034-runner-copy-before-fix*.log`). 같은 상태에서 047 변경만 뺀 사본은 6개가 모두 통과했다(`closure/spec034-runner-without-047.log`).
  - 원인은 runner가 vendor 트리를 훑다가 512 MiB 한도를 넘은 것이다(`closure/spec034-runner-diagnosis-bytes.log`). 진단용 출력은 임시 사본에만 넣었다.
  - 범위 11 뒤에는 새 사본에서 6 passed / 0 failed, exit 0이다(574 s, `closure/spec034-runner-copy.log`).
- Oracle 독립 검증:
  - 범위 1–10의 구현은 7차에 **PASS**를 받았다.
  - 범위 11의 계획 수정은 2차에, 그 구현은 1차에 **PASS**를 받았다(첫 구현 검토는 결과를 받기 전에 세션이 사라져 새 세션으로 다시 받았다).
  - 각 차수의 지적과 조치는 `implementation.md`에 있다.
- 남은 제한:
  - 설치 본문에는 시간 제한이 없다.
  - 경로를 확인한 뒤 같은 사용자가 그 경로를 바꾸는 경쟁은 이 저장소의 다른 곳과 같이 받아들인다.
  - 034 runner의 의존성 트리가 새 한도(2 GiB, 131,072개)마저 넘으면 한도를 다시 정해야 한다.
