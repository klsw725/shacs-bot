# 047. 뜻으로 찾는 기억 검색 구현 명세

Status: Complete (Scoped)

Consumes: 004·020 tool runtime과 tool surface, 005 skill system, 009 context assembly, 010 host safety(network guard), 012 Dream lifecycle, 018 `MemoryStore` evidence retrieval, 031 config schema와 runtime layout, 034 release runner(의존성 트리 한도는 소유 범위 8로 바꾼다), 046의 `rusqlite` 판과 SQLite 설정

## 목적과 완료 경계

지금 기억은 두 층이다. Dream이 정리한 `memory/MEMORY.md`는 매 turn 통째로 context에 들어가고, `memory/history.jsonl`은 context에 넣지 않고 agent가 `grep` tool로 찾는다([`memory` skill](../../../crates/shacs-skills/memory/SKILL.md)). grep은 같은 글자가 있어야 찾으므로 조사가 붙은 한국어("회의록을"), 줄임말("포폴"), 다른 표현으로 적힌 옛 기억을 놓친다.

이 스펙은 기억 파일에서 다시 만들 수 있는 검색 색인과, 글자와 뜻을 함께 쓰는 `memory_search` tool을 더한다. 뜻 검색에 쓰는 임베딩은 shacs-bot process 안에서 local 모델로 계산한다. 별도 서버(Ollama 등)나 클라우드 API를 쓰지 않는다.

완료되면 사용자는 다음을 경험한다.

- 다른 표현으로 물어도 어시스턴트가 관련 기억을 찾아, 어느 기록의 어느 시점인지와 함께 쓴다.
- 모델 파일을 한 번 설치하면 인터넷 없이 동작하고, 기억 내용은 컴퓨터 밖으로 나가지 않는다.
- 기억 파일에서 지워지거나 바뀐 내용은 검색 결과에도 남지 않는다.
- 모델 파일이 없거나 불러오지 못해도 대화는 멈추지 않는다. 검색은 글자 기준 결과와 그 이유를 함께 알리고, 색인이 덜 만들어졌으면 그 사실도 알린다.

이 스펙은 046이 정한 `rusqlite` 판과 SQLite 설정을 따를 뿐, 다른 스펙의 구현을 기다리지 않는다.

## 소유 범위

1. **local 임베딩 모델:** candle(Hugging Face의 Rust ML 라이브러리)로 모델을 process 안에서 CPU로 돌린다. 지원 모델은 둘이며, 각 모델의 revision·파일·크기·sha256·구조·차원·입력 접두어·pooling·정규화·최대 입력을 저장소에 고정한 manifest로 둔다(값은 PRD 000).
   - 기본: `dragonkue/multilingual-e5-small-ko-v2`. multilingual-e5-small의 한국어 추가 학습판, Apache-2.0, safetensors 약 470MB, 384차원.
   - 선택: `nlpai-lab/KURE-v1`. bge-m3 기반 한국어 검색 모델, MIT, safetensors 약 2.3GB, 1024차원.
2. **모델 설치:** `shacs-bot memory model install`이 manifest의 파일을 받아 크기와 sha256을 확인한 뒤 data directory에 원자적으로 둔다. 확인에 실패한 파일은 쓰지 않는다. 사용자가 같은 파일을 직접 넣어도 같은 확인을 거친다. 대화 turn 중에는 모델을 내려받지 않는다.
3. **검색 색인:** data directory 아래, canonical workspace 경로의 SHA-256으로 이름 붙인 SQLite 파일. 원본이 아니며 지워도 기억 파일에서 다시 만든다. `history.jsonl` 항목과 `MEMORY.md`를 정해진 규칙으로 나눈 항목마다 안정된 key, 출처(파일·cursor·timestamp 또는 줄 범위), 글자 색인(FTS5 trigram), 벡터와 그것을 만든 모델·revision·차원을 저장한다.
4. **원본과 맞추기:** 검색할 때마다 두 기억 파일을 읽어 지금의 항목 목록을 만들고, 한 transaction 안에서 없어진 항목은 지우고, 남은 항목의 출처는 지금 값으로 고치고, 새 항목은 더한다. 맞추기에 실패하면 이전 색인을 쓰지 않는다. 글자 색인은 매번 끝까지 맞추고, 벡터는 한 번에 계산할 개수에 상한을 두어 남은 개수를 결과에 알린다. 색인 schema·모델·revision·차원·나누기 규칙·workspace가 바뀌면 전체를 새 세대로 다시 만들고, 다른 모델이나 이전 세대의 벡터를 섞지 않는다.
5. **검색:** 글자 검색과 벡터 검색(모든 벡터와 cosine 비교)의 순위를 RRF로 합쳐 상위 결과를 출처와 함께 돌려준다. 낱말 나누기, 순위, 동점 처리, 잘못된 질문 벡터의 처리는 PRD 010이 정한 결정적 규칙을 따른다. sqlite-vec 같은 확장은 쓰지 않는다.
6. **표면:** `memory_search` tool, `memory` skill 안내(먼저 `memory_search`, 정확한 글자·시각이 필요하면 `grep`), `shacs-bot memory model install`·`memory index`·`memory search`, config(`agents.defaults.memorySearch`의 사용 여부와 모델 선택), 사용자 문서.
7. **평가:** 한국어 fixture 기억과 질문(조사, 줄임말, 다른 표현, 두 글자 단어, 날짜)으로 grep 기준선과 이 검색을 비교한다.
8. **034 release runner의 의존성 트리 한도:** 이 스펙이 더하는 의존성 때문에 [034](../034-generated-media-and-rich-file-context-expansion/SPEC.md) release runner가 내려받아 확인하는 의존성 트리(`cargo vendor` 결과)가 runner의 트리 한도(512 MiB, 항목 32,768개)를 넘는다. 2026-11-02에 runner는 vendor 트리를 훑다가 512 MiB에서 멈췄다. 같은 날 registry source로 어림잡은 크기는 047 전 503.8 MiB·약 29,500개, 047 뒤 528.1 MiB·약 32,700개다. runner의 의존성 트리(vendor 결과, tool 캐시, runtime library)에는 작업 트리와 따로 한도를 둔다(값은 PRD 000). 작업 트리·git 트리 한도와 runner의 다른 검사는 바꾸지 않는다.

## 기존 owner와의 경계

| 기존 owner | 047이 소비하는 것 | 047이 소유하지 않는 것 |
|---|---|---|
| 004·020 | tool 등록과 실행 경계, tool 결과 한도 | 다른 tool의 동작 |
| 005 | built-in skill 형식과 `memory` skill | 다른 skill의 내용 |
| 009 | context assembly | `MEMORY.md`를 context에 넣는 방식 |
| 012·018 | Dream과 `MemoryStore`가 쓰는 기억 파일과 history 읽기 | Dream의 정리 규칙, 기억 파일 형식, history 압축 규칙 |
| 010 | 모델 설치 download의 network guard | permission 규칙 |
| 031 | config schema와 data directory 아래 runtime layout | 다른 config와 layout |
| [046](../046-sqlite-durable-event-store/SPEC.md) | `rusqlite` 판과 SQLite 설정 | durable event store |
| [034](../034-generated-media-and-rich-file-context-expansion/SPEC.md) | release runner와 그 트리 복사·확인. 그중 의존성 트리 한도 값만 소유 범위 8로 바꾼다 | runner의 다른 검사, 작업 트리·git 트리 한도, 034의 완료 판정 |

## 반드시 지킬 것

- 기억 원본은 `MEMORY.md`와 `history.jsonl`이다. 색인은 원본을 고치지 않으며, 원본에 없는 항목을 검색 결과로 내지 않는다.
- 임베딩 계산 때문에 기억 내용이 컴퓨터 밖으로 나가지 않는다. 네트워크는 사용자가 실행한 모델 설치에서만 쓴다.
- 임베딩 실패가 turn을 실패시키지 않는다. 뜻 검색을 못 썼거나 색인이 덜 만들어졌으면 결과에 그 사실을 적는다.
- 색인·모델 폴더는 0700, 그 안의 파일(SQLite `-wal`·`-shm`, 내려받는 중인 파일 포함)은 0600으로 만들고, symlink를 따라가지 않는다. 검색 결과에는 기억 파일에 있는 내용만 담는다.
- Cargo 테스트는 실제 모델 없이 결정적인 가짜 임베딩으로 돈다.
- 새 C/C++ 빌드 도구나 빌드 중 download를 요구하지 않는다. Rust 1.88에서 빌드되지 않으면 멈추고 사용자에게 묻는다.
- Linux arm64 빌드에서 candle의 행렬 곱셈 의존성(`gemm-f16`)은 CPU의 FP16 명령(`fullfp16`)을 요구하며, 이 flag 없이는 compile되지 않는다(2026-10-31 확인). 사용자 결정(2026-11-01)에 따라 Dockerfile의 arm64 빌드에서만 `RUSTFLAGS="-C target-feature=+fp16"`를 켜고, 다른 대상(macOS, Linux x86_64)과 저장소의 Cargo 설정은 바꾸지 않는다. macOS(aarch64-apple-darwin)는 flag 없이 빌드된다(2026-11-01 확인).

## 수용 조건

1. 기본 모델로 색인을 끝까지 만든 뒤 잰 한국어 fixture 질문의 상위 3개 적중률이 grep 기준선보다 낮지 않고, 다른 표현·줄임말 질문에서는 더 높다. 측정 결과를 기록으로 남긴다.
2. 모델이 꺼져 있거나, 파일이 없거나, 확인에 실패하거나, 불러오기·계산에 실패하면 tool은 결정적인 글자 검색 결과와 이유를 돌려주고 turn은 정상으로 끝난다. 벡터가 덜 계산됐으면 남은 개수를 알린다. 원본 맞추기에 실패하면 이전 색인 대신 방금 읽은 원본으로 글자 검색한 결과와 그 이유를 돌려준다.
3. history 압축이나 Dream이 지우거나 바꾼 항목은 다음 검색부터 결과에 나오지 않고, 위치만 바뀐 항목의 출처는 새 값으로 나온다. 맞추기에 실패한 검색은 이전 색인을 쓰지 않는다. 모델이나 revision을 바꾸면 색인을 다시 만들고, 다른 모델의 벡터가 섞이지 않는다. 색인을 지우면 다시 만들어져 같은 결과를 낸다.
4. 모델 설치 말고는 검색·색인이 네트워크를 쓰지 않는다. 색인·모델 파일의 권한과 symlink 거절이 지켜진다.
5. 기본 모델을 올린 process의 메모리 증가량과 질문 하나의 검색 시간을 기록한다.
6. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다. 전체 workspace 테스트는 사용자 승인을 받아 실행한다.
7. Spec 034 release runner 시험이 047 변경을 포함한 작업 트리에서 통과한다(저장소 밖 짧은 경로의 사본에서 실행). 의존성 트리와 작업 트리가 각자의 한도를 넘으면 한도 초과로 거절된다.

위 조건은 047의 단독 종료 기준이다.

## 남은 위험과 한계

- 기본 모델의 한국어 검색 점수는 공개 비교가 부족하다. 부족하면 KURE-v1로 바꿀 수 있지만 더 무겁고 느리며, 바꾸면 색인을 다시 만든다.
- 평가는 허구의 작은 fixture(항목 59개, 질문 35개)로 했으므로, 실제 기억에서의 적중률을 보장하지 않는다. 그 fixture에서도 다른 표현 질문 7개 중 3개는 상위 3개에 들지 못했다.
- "다음 주에 뭐 있었지"처럼 시점으로 찾는 질문은 임베딩이 잘 풀지 못한다. 날짜 거르기는 다루지 않는다.
- 모든 벡터를 비교하는 방식과 검색마다 원본을 다시 읽는 방식은 개인 규모(수만 항목, 기억 파일 수 MB)를 전제로 한다.
- 모델은 첫 뜻 검색 때 process에 올라가 그 process가 끝날 때까지 메모리를 쓴다(2026-11-02 측정, macOS aarch64 release).
  - 기본 모델: 불러오는 동안 최대 RSS 1,311 MiB, 올린 뒤 증가 약 1,286 MiB.
  - KURE-v1: 불러오는 동안 최대 RSS 3,215 MiB.
- 빌드 시간과 binary 크기가 늘어난다. candle은 `shacs-core`를 쓰는 모든 crate의 빌드에 들어간다.
- arm64 Docker image는 FP16 산술 확장(`FEAT_FP16`, Linux `/proc/cpuinfo`의 `fphp`·`asimdhp`)을 실제로 갖춘 CPU에서만 돈다(예: Apple Silicon, AWS Graviton2 이후). 이 확장은 ARMv8.2-A에서도 선택 기능이므로 판 번호만으로는 정해지지 않으며, Raspberry Pi 4 같은 ARMv8.0 CPU에서는 쓸 수 없다. Docker 밖에서 Linux arm64로 직접 빌드할 때도 같은 flag가 필요하다(Spec 031 release runner는 환경을 넘겨주므로 그 flag로 실행하면 된다). Spec 034 release runner는 내부 Cargo 실행의 환경을 비우고 Cargo 설정을 거절하므로 Linux arm64에서는 지원하지 않는다. 두 내용을 사용자 문서에 적는다.
- 세션 대화 원문, 웹 페이지, 이미지는 색인하지 않는다.
- 034 release runner는 의존성 트리를 통째로 내려받아 확인하므로, 의존성이 늘면 runner의 실행 시간과 디스크 사용도 늘어난다. 의존성 트리가 새 한도마저 넘으면 한도를 다시 정해야 한다.

## 실행 PRD와 독립 종료

각 PRD는 앞 번호의 닫힌 결과만 소비하며, 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 local 임베딩 모델과 색인](prds/000-local-embedding-model-and-index.md) | 소유 범위 1–4·8, 설치·색인 CLI, 가짜 임베딩 | 없음 | 수용 조건 3·4·6·7 |
| [010 검색 tool과 평가](prds/010-memory-search-tool-and-evaluation.md) | 소유 범위 5–7(설치·색인 CLI 제외), 사용자 문서 | 000 | 수용 조건 1·2·5·6 |

범위 1–4·8과 수용 조건 3·4·7은 000, 범위 5–7과 수용 조건 1·2·5는 010이 소유한다. 수용 조건 6은 각 PRD가 자기 변경에 대해 확인한다. 010은 000의 색인과 모델 실행 위에서만 구현할 수 있으므로 번호 순서대로 구현한다.

## 종료 증거 (2026-11-02)

PRD [000](prds/000-local-embedding-model-and-index.md)과 [010](prds/010-memory-search-tool-and-evaluation.md)이 각각 독립 Oracle **PASS**를 받았다. 000은 범위 1–10이 구현 7차, 범위 11이 구현 1차에 받았고, 010은 구현 2차에 받았다. 수용 조건 1–7과 이 종료 기록도 스펙 차원에서 지적 없이 **PASS**로 판정됐다. 이는 현재 self-hosted 지원 범위의 scoped 완료이며 전체 release seal이 아니다. 실행 기록은 로컬 `.omo/evidence/spec047/`(추적하지 않음)에 있다.

- 조건 1: 기본 모델로 색인을 끝까지 만든 뒤 허구의 한국어 fixture 질문 35개를 쟀다(010).
  - 상위 3개 적중률은 이 검색 31/35(88.6%), grep 기준선 19/35(54.3%)다.
  - 다른 표현은 4/7 대 2/7, 줄임말은 6/7 대 1/7이다. 기준선보다 낮은 유형은 없다.
  - 측정 전에 평가 자료의 sha256을 기록했다.
- 조건 2(010): 아래 각 경우에 tool이 같은 글자 검색 결과와 이유를 돌려주고, 실제 `AgentLoop` turn이 정상으로 끝난다.
  - 뜻 검색을 못 쓰는 경우: 꺼짐, 모델 없음, 확인 실패, 불러오기 실패, 벡터 계산 실패, 질문 임베딩 실패, 잘못된 질문 벡터.
  - 덜 계산된 색인: 남은 벡터 수를 알린다.
  - 맞추기 실패: 방금 읽은 원본으로 글자 검색하고, 모델을 올리지 않는다.
  - 맞추기 뒤 색인 실패나 망가진 meta: 원본을 다시 읽어 `index_unavailable`로 보고한다.
- 조건 3: history 압축과 직접 고친 항목은 다음 맞추기에서 지워지거나 출처가 고쳐진다. 맞추기 실패는 이전 색인을 쓰지 않는다. 모델·revision을 바꾸면 새 세대로 다시 만들고, 지운 색인은 같은 결과로 다시 만들어진다(000).
- 조건 4: 네트워크는 `memory model install`의 다운로드에서만 쓰고, 그 요청과 redirect는 010 network guard를 거친다. 폴더는 0700, 파일은 0600이고, symlink와 FIFO는 거절한다(000).
- 조건 5: 기본 모델을 올린 process의 RSS는 약 1,286 MiB 늘었다. 색인이 끝난 상태에서 질문 하나의 검색은 10회 중앙값 25.6 ms였다(010).
- 조건 6: fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 두 PRD에서 통과했다. 사용자 승인을 받아 두 PRD의 종료 때 전체 workspace 테스트를 실행했다.
  - 000: suite 184개, 3,341 passed / 0 failed / 7 ignored.
  - 010: 걸러진 시험이 없는 결과 줄 188개, 3,384 passed / 0 failed / 8 ignored. 자식 process 22개도 통과했다.
  - Docker `rust:1.88-bookworm` linux/arm64 builder의 `--locked` release 빌드가 두 PRD의 마지막 소스에서 모두 되었다.
- 조건 7: 사용자 승인을 받아 짧은 경로의 사본에서 034 runner 테스트 6개가 모두 통과했다. 의존성 트리와 작업 트리는 각자의 한도로 거절된다(000). 010은 의존성을 더하지 않았다.

[남은 위험과 한계](#남은-위험과-한계)는 그대로이고, 각 PRD의 종료 확인에 적은 제한도 남는다.
