# Retired Spec031 release runner v2 results

The Spec031 release runner wrote `spec031.release_runner.v2` results during the retired Spec 035 work. [Spec 041](../../specs/041-release-evidence-tooling-disposition/SPEC.md) decided to stop reading them in the current runner, and [Spec 045](../../specs/045-release-runner-scope-reduction/SPEC.md) removed that reader after this record was in place. The current runner rejects every v2 result with `UnsupportedSchema`. This folder keeps a separate way to check the ten canonical v2 results and to reproduce the verdicts they had.

These records are not an active spec, a release approval, or proof that a historical execution happened. A record proves only that a copy of a result has the same bytes as the recorded result. The meaning of a result comes from the pinned validator below and the source it ran against. Each result is read as a result of its own schema (146 rows, six owners). It is not reclassified into the current 65 rows and four owners, and it is not current PASS evidence.

## The ten results

The results are kept only in the untracked local folder `.omo/worktree-archives/spec035-20260928/evidence/spec035/`. If that folder is lost, these records cannot recreate the results. The 49 test temp outputs under `closure/f1-r01-owned-tmp/` are not covered, as in the Spec 035 handoff. They are kept. The validator rejected them with `UnsupportedSchema` before the v2 reader was removed and still does.

All ten results have schema `spec031.release_runner.v2`. Only the two G7 results contain `spec035-execution-binding.json`.

| Record | Result | Files | Coverage rows (pass/blocked) | Owners | Commands | Recorded summary | Pinned validator |
|---|---|---|---|---|---|---|---|
| `closure_f1-r01-current.sha256` | `closure/f1-r01-current` | 20 | 146 (5/141) | 6 | 0 | `BLOCKED` | `UnsupportedSchema` |
| `closure_f1-r01-current-final.sha256` | `closure/f1-r01-current-final` | 20 | 146 (5/141) | 6 | 0 | `BLOCKED` | `UnsupportedSchema` |
| `closure_f1-runner-current.sha256` | `closure/f1-runner-current` | 20 | 146 (5/141) | 6 | 0 | `BLOCKED` | `UnsupportedSchema` |
| `closure_f3-runner-current.sha256` | `closure/f3-runner-current` | 20 | 146 (5/141) | 6 | 0 | `BLOCKED` | `UnsupportedSchema` |
| `closure_f3-runner-fixture.sha256` | `closure/f3-runner-fixture` | 131 | 146 (146/0) | 6 | 17 | `PASS` | `UnsupportedSchema` |
| `closure_final-compiled-runner-current.sha256` | `closure/final-compiled-runner-current` | 20 | 146 (5/141) | 6 | 0 | `BLOCKED` | `UnsupportedSchema` |
| `closure_final-compiled-runner-fixture.sha256` | `closure/final-compiled-runner-fixture` | 131 | 146 (146/0) | 6 | 17 | `PASS` | `UnsupportedSchema` |
| `closure_finalization_g7-integrated_retained-originals.sha256` | `closure/finalization/g7-integrated/retained-originals` | 51 | 146 (140/6) | 6 | 17 | `pending-final-audit` | valid |
| `closure_finalization_g7-integrated_runner-output.sha256` | `closure/finalization/g7-integrated/runner-output` | 101 | 146 (140/6) | 6 | 17 | `pending-final-audit` | valid |
| `workspace-no-fail-fast-20260917_observed-current.sha256` | `workspace-no-fail-fast-20260917/observed-current` | 20 | 146 (5/141) | 6 | 0 | `BLOCKED` | `UnsupportedSchema` |

"valid" means the pinned validator accepted the result as a G7-bound v2 result in its `pending-final-audit` state, with the six Spec035 postrun rows still blocked. `UnsupportedSchema` means the validator does not read a v2 result without an execution binding; the recorded summaries above are what the runner wrote at the time.

## Checking bytes

Each record lists every file of one result in the `shasum -a 256` format, with paths relative to the result folder. `shasum -c` alone does not report extra files, so compare the file list too and make sure there are no symlinks:

```sh
results=<repo>/.omo/worktree-archives/spec035-20260928/evidence/spec035
records=<repo>/docs/archive/spec031-v2-results
cd "$results/<result>"
shasum -a 256 -c --quiet "$records/<record>"
find . -type f | sed 's#^\./##' | LC_ALL=C sort | diff - <(cut -c67- "$records/<record>" | LC_ALL=C sort)
find . -type l
```

All three commands must succeed without output.

## Reproducing the verdicts

The pinned commit is `0f0279973a12af65b9616fc41163748b367fc937` on `main`. It contains the Spec 045 PRD 000 count check and the whole v2 reader. Do not rewrite `main` history past it (no rebase or squash), or this commit will no longer be reachable.

1. Extract the pinned source: `mkdir -p <work>/source && git -C <repo> archive 0f0279973a12af65b9616fc41163748b367fc937 | tar -x -C <work>/source`.
2. Copy a result to an absolute path and rewrite only `evidence_root` in the copy's `manifest.json` to that path. The validator compares the manifest on disk with its input.

   ```sh
   cp -R "$results/<result>" <work>/copy
   python3 -I -c 'import json, sys; p = sys.argv[1] + "/manifest.json"; d = json.load(open(p)); d["evidence_root"] = sys.argv[1]; open(p, "w").write(json.dumps(d, indent=2))' <work>/copy
   ```

3. Put the harness below in `<work>/harness` with its path dependency pointing at `<work>/source/crates/shacs-projection`, then run it with the extracted source as the repository root: `cargo run --manifest-path <work>/harness/Cargo.toml -- <work>/source <work>/copy`. It prints the folder, `OK` or `ERR`, and the verdict.

`Cargo.toml`:

```toml
[package]
name = "spec041-harness"
version = "0.1.0"
edition = "2021"

[dependencies]
shacs-projection = { path = "<work>/source/crates/shacs-projection" }
serde_json = "1"

[workspace]
```

`src/main.rs`:

```rust
//! Spec041 harness: validate recorded Spec031 runner outputs with the current validator.
//! Usage: spec041-harness <repo-root> <run-dir>...
use shacs_projection::{validate_spec031_release_artifacts_with_repo_root, Spec031ReleaseRunArtifacts};
use std::path::{Path, PathBuf};

fn main() {
    let mut args = std::env::args().skip(1);
    let repo = PathBuf::from(args.next().unwrap_or_default());
    for dir in args {
        let outcome = std::fs::read_to_string(Path::new(&dir).join("manifest.json"))
            .map_err(|error| format!("read manifest: {error}"))
            .and_then(|text| {
                serde_json::from_str::<Spec031ReleaseRunArtifacts>(&text)
                    .map_err(|error| format!("parse manifest: {error}"))
            })
            .and_then(|mut artifacts| {
                let schema = artifacts.schema.clone();
                artifacts.evidence_root = dir.clone();
                validate_spec031_release_artifacts_with_repo_root(&artifacts, &repo)
                    .map(|()| format!("{schema}: valid"))
                    .map_err(|error| format!("{schema}: {error:?}"))
            });
        match outcome {
            Ok(text) => println!("{dir}\tOK\t{text}"),
            Err(text) => println!("{dir}\tERR\t{text}"),
        }
    }
}
```

The harness is the one Spec 041 used. It is not compiled with the workspace.
