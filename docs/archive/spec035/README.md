# Retired Spec 035 source snapshots

These files preserve the original bytes of the removed, incomplete Spec 035 documents for historical integrity checks. They are not active specs, implementation requirements, or release approval.

`current/` contains the documents removed from `docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity/`. Historical execution evidence must still be interpreted against the exact source snapshot it used; this archive does not retroactively certify that execution.

`g7/` contains the distinct SPEC and CLOSURE bytes bound to the earlier G7 integrity check. The two snapshots have different digests; neither may be substituted for the other. `SHA256SUMS` in each directory records the expected bytes.

[Spec 045](../../specs/045-release-runner-scope-reduction/SPEC.md) removed the Spec031 runner's v2 reader, including `crates/shacs-projection/src/spec031/release_runner/spec035_catalog.rs` and `spec035_execution.rs`, which both archived `CLOSURE.md` files link to. Open them at the pinned commit, for example `git show 0f0279973a12af65b9616fc41163748b367fc937:crates/shacs-projection/src/spec031/release_runner/spec035_catalog.rs`. The v2 runner results themselves are checked through [the v2 result record](../spec031-v2-results/README.md).
