use shacs_config::{
    RememberedPermissionEffect, RememberedPermissionFileStore, RememberedPermissionMatcher,
    RememberedPermissionRemoveByPrefixOutcome, RememberedPermissionRule,
    RememberedPermissionStoreErrorKind,
};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::thread;

#[path = "remembered_permission_store_support/mod.rs"]
mod support;

use support::{exec_rule, StoreFixture};

#[test]
fn remembered_permission_store_loads_missing_file_as_empty(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = RememberedPermissionFileStore::for_context(&fixture.context);

    let loaded = store.load()?;

    assert!(loaded.project(&fixture.workspace_id).is_none());
    assert_eq!(store.path(), fixture.data_dir.join("permissions.json"));
    assert!(!store.path().exists());
    Ok(())
}

fn assert_no_temp_files(data_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with(".permissions.json.tmp-"),
            "leftover temp file: {name}"
        );
    }
    Ok(())
}

#[test]
fn remembered_permission_store_mutation_writes_0600_and_reloads_under_lock(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = RememberedPermissionFileStore::for_context(&fixture.context);
    let first_rule = exec_rule("cargo", 1);
    let second_rule = exec_rule("test", 2);

    store.mutate(|permissions| {
        permissions.upsert_rule(fixture.workspace_id.clone(), first_rule.clone());
        Ok(())
    })?;
    let stale = store.load()?;
    store.mutate(|permissions| {
        assert_eq!(permissions, &stale);
        permissions.upsert_rule(fixture.workspace_id.clone(), second_rule.clone());
        Ok(())
    })?;

    let reloaded = store.load()?;
    let rules = reloaded
        .project(&fixture.workspace_id)
        .ok_or("missing rules")?;
    assert_eq!(rules.len(), 2);
    let removed_id = first_rule.id().clone();
    store.mutate(|permissions| {
        assert!(permissions.remove_rule(&fixture.workspace_id, &removed_id));
        Ok(())
    })?;
    assert_eq!(
        store
            .load()?
            .project(&fixture.workspace_id)
            .ok_or("missing rules")?
            .len(),
        1
    );
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(store.path())?.permissions().mode() & 0o777,
        0o600
    );
    assert_no_temp_files(&fixture.data_dir)?;
    Ok(())
}

#[test]
fn remembered_permission_store_removes_rule_by_prefix_only_when_unique(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = RememberedPermissionFileStore::for_context(&fixture.context);
    let first_rule = exec_rule("cargo", 1);
    let second_rule = exec_rule("test", 2);

    store.mutate(|permissions| {
        permissions.upsert_rule(fixture.workspace_id.clone(), first_rule.clone());
        permissions.upsert_rule(fixture.workspace_id.clone(), second_rule.clone());
        Ok(())
    })?;
    let prefix = &first_rule.id().as_str()[..16];
    let outcome = store.remove_rule_by_prefix(&fixture.workspace_id, prefix)?;

    assert_eq!(
        outcome,
        RememberedPermissionRemoveByPrefixOutcome::Removed(first_rule)
    );
    assert_eq!(
        store
            .load()?
            .project(&fixture.workspace_id)
            .ok_or("missing rules")?,
        &[second_rule]
    );
    Ok(())
}

#[test]
fn remembered_permission_store_preserves_rules_for_missing_and_ambiguous_prefix(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = RememberedPermissionFileStore::for_context(&fixture.context);
    let mut rules = Vec::new();
    for index in 0..64 {
        let rule = exec_rule(&format!("cmd-{index}"), 100 + index);
        store.mutate(|permissions| {
            permissions.upsert_rule(fixture.workspace_id.clone(), rule.clone());
            Ok(())
        })?;
        rules.push(rule);
        if shared_prefix(&rules).is_some() {
            break;
        }
    }
    let ambiguous_prefix = shared_prefix(&rules).ok_or("missing shared prefix")?;

    let before = fs::read(store.path())?;
    let missing = store.remove_rule_by_prefix(&fixture.workspace_id, "missing")?;
    let ambiguous = store.remove_rule_by_prefix(&fixture.workspace_id, &ambiguous_prefix)?;

    assert_eq!(missing, RememberedPermissionRemoveByPrefixOutcome::Missing);
    assert_eq!(
        ambiguous,
        RememberedPermissionRemoveByPrefixOutcome::Ambiguous
    );
    assert_eq!(
        store
            .load()?
            .project(&fixture.workspace_id)
            .ok_or("missing rules")?,
        rules.as_slice()
    );
    assert_eq!(fs::read(store.path())?, before);
    Ok(())
}

#[test]
fn remembered_permission_store_concurrent_non_conflicting_mutations_preserve_both(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = Arc::new(RememberedPermissionFileStore::for_context(&fixture.context));
    let workspace_id = Arc::new(fixture.workspace_id.clone());
    let barrier = Arc::new(Barrier::new(2));

    let handles: Vec<_> = [exec_rule("cargo", 10), exec_rule("test", 11)]
        .into_iter()
        .map(|rule| {
            let store = Arc::clone(&store);
            let workspace_id = Arc::clone(&workspace_id);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.mutate(|permissions| {
                    permissions.upsert_rule((*workspace_id).clone(), rule);
                    Ok(())
                })
            })
        })
        .collect();

    for handle in handles {
        handle.join().map_err(|_| "thread panicked")??;
    }
    let reloaded = store.load()?;
    assert_eq!(
        reloaded
            .project(&fixture.workspace_id)
            .ok_or("missing rules")?
            .len(),
        2
    );
    Ok(())
}

#[test]
fn remembered_permission_store_replaces_same_matcher_and_enforces_project_cap(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StoreFixture::new()?;
    let store = RememberedPermissionFileStore::for_context(&fixture.context);

    store.mutate(|permissions| {
        permissions.upsert_rule(fixture.workspace_id.clone(), exec_rule("cargo", 1));
        permissions.upsert_rule(
            fixture.workspace_id.clone(),
            RememberedPermissionRule::new(
                RememberedPermissionEffect::Deny,
                RememberedPermissionMatcher::ExecPrefix {
                    tokens: vec!["cargo".to_owned()],
                },
                2,
            ),
        );
        Ok(())
    })?;
    let reloaded = store.load()?;
    let rules = reloaded
        .project(&fixture.workspace_id)
        .ok_or("missing rules")?;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].effect(), RememberedPermissionEffect::Deny);

    let error = store
        .mutate(|permissions| {
            for index in 0..257 {
                permissions.upsert_rule(
                    fixture.workspace_id.clone(),
                    exec_rule(&format!("cmd{index}"), index),
                );
            }
            Ok(())
        })
        .expect_err("project cap must fail closed");
    assert_eq!(
        error.kind(),
        RememberedPermissionStoreErrorKind::ProjectRuleLimitExceeded
    );
    assert_eq!(
        store
            .load()?
            .project(&fixture.workspace_id)
            .ok_or("missing rules")?
            .len(),
        1
    );
    assert_no_temp_files(&fixture.data_dir)?;
    Ok(())
}

fn shared_prefix(rules: &[RememberedPermissionRule]) -> Option<String> {
    for (left_index, left) in rules.iter().enumerate() {
        for right in rules.iter().skip(left_index + 1) {
            let prefix = left
                .id()
                .as_str()
                .chars()
                .zip(right.id().as_str().chars())
                .take_while(|(left, right)| left == right)
                .map(|(left, _right)| left)
                .collect::<String>();
            if !prefix.is_empty() {
                return Some(prefix);
            }
        }
    }
    None
}
