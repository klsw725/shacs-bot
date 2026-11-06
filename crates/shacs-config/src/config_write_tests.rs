use super::*;
use serde_json::json;
use std::path::PathBuf;

fn entries(folder: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).map_or(0, |metadata| metadata.permissions().mode() & 0o777)
}

#[test]
fn an_atomic_write_replaces_the_file_owner_only_and_leaves_no_temporary_file() {
    // Given
    let folder = tempfile::tempdir().expect("temp folder");
    let path = folder.path().join("config.json");
    fs::write(&path, "{}").expect("config");

    // When
    write_config_value_atomically(&path, &json!({"permissions": {}})).expect("written");

    // Then
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        "{\n  \"permissions\": {}\n}\n"
    );
    assert_eq!(entries(folder.path()), std::slice::from_ref(&path));
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn a_guarded_write_refuses_a_file_changed_created_or_removed_meanwhile() {
    let cases: [(Option<&str>, Option<&str>, bool); 5] = [
        (Some("{}"), Some("{\"changed\": true}"), false),
        (Some("{}"), None, false),
        (None, Some("{}"), false),
        (Some("{}"), Some("{}"), true),
        (None, None, true),
    ];
    for (first, meanwhile, writes) in cases {
        // Given
        let folder = tempfile::tempdir().expect("temp folder");
        let path = folder.path().join("config.json");
        if let Some(text) = first {
            fs::write(&path, text).expect("config");
        }
        let expected = first.map(str::as_bytes);

        // When
        let result =
            write_if_unchanged_with_hook(&path, expected, &json!({"new": 1}), || match meanwhile {
                Some(text) => fs::write(&path, text).expect("changed"),
                None => {
                    let _ = fs::remove_file(&path);
                }
            });

        // Then
        let label = format!("{first:?} then {meanwhile:?}");
        let now = fs::read_to_string(&path).ok();
        if writes {
            assert!(result.is_ok(), "{label}: {result:?}");
            assert_eq!(now.as_deref(), Some("{\n  \"new\": 1\n}\n"), "{label}");
            #[cfg(unix)]
            assert_eq!(mode(&path), 0o600, "{label}");
        } else {
            assert!(
                matches!(result, Err(ConfigWriteError::Changed)),
                "{label}: {result:?}"
            );
            assert_eq!(now.as_deref(), meanwhile, "{label}");
        }
        let left = entries(folder.path());
        assert!(
            left.len() <= 1,
            "{label}: a temporary file was left: {left:?}"
        );
    }
}
