# Retired Spec 032 app environment test source

`app_environment_lifecycle_projection_test.rs` preserves, byte for byte, lines 145–161 of `crates/shacs-app/tests/app_environment.rs` at commit `1e8d141`: the `enable_disable_lifecycle_projection_does_not_create_process_truth` test. [Spec 043](../../specs/043-app-capability-retirement/SPEC.md) removed the `shacs-app` crate and this test.

Spec031 release runner results with schema `spec031.release_runner.v3` or `spec031.release_runner.v4` cite that test as the `spec032` external owner fact. When those results are read later, the runner checks the cited test name in this snapshot instead of the live test file, and it never runs the deleted test again. `spec031.release_runner.v2` results cited it too, but [Spec 045](../../specs/045-release-runner-scope-reduction/SPEC.md) removed the v2 reader; the runner rejects them with `UnsupportedSchema`, and they are checked only through [the v2 result record](../spec031-v2-results/README.md). New `spec031.release_runner.v5` runs have no `spec032` owner. The snapshot digest is pinned in `crates/shacs-projection/src/spec031/release_runner/external_audit_facts.rs`; `SHA256SUMS` records the same digest.

This file is not compiled, is not an active test, and does not certify any historical execution. It only lets recorded results be read with the composition they were produced under.
