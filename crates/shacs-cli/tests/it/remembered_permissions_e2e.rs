#[path = "remembered_permissions_e2e/support.rs"]
mod support;

use std::fs;
use support::{
    assert_success, remembered_rule_id, shacs_bot, stderr_text, stdout_text, text_response,
    workspace_arg, write_config, write_file_response,
};

#[test]
fn remembered_permissions_e2e_project_allow_reuse_revoke_and_fail_closed(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: a real shacs-bot binary configured with a temp workspace and fake provider.
    let root = tempfile::tempdir()?;
    let root_path = root.path().canonicalize()?;
    let workspace = root_path.join("workspace");
    fs::create_dir_all(&workspace)?;
    let allowed_path = workspace.join("allowed.txt");
    let protected_store_path = root_path.join("data").join("permissions.json");
    let fake_responses_path = root_path.join("fake-provider-responses.json");
    fs::write(
        &fake_responses_path,
        serde_json::to_vec(&vec![
            write_file_response(&allowed_path, "approved-once"),
            text_response("project approval executed"),
            write_file_response(&allowed_path, "reused-in-new-session"),
            text_response("project approval reused"),
            write_file_response(&allowed_path, "revoked-should-not-run"),
            write_file_response(&protected_store_path, "sk-protected-sentinel"),
            write_file_response(&allowed_path, "malformed-should-not-run"),
            text_response("malformed store blocked"),
        ])?,
    )?;
    let config_path = write_config(&root_path, &workspace)?;

    // When: the user approves a project rule and a new session repeats the same action.
    let first_prompt = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "ask",
            "first write",
            "--workspace",
            workspace_arg(&workspace).as_str(),
            "--session",
            "remembered-e2e-a",
            "--allow-side-effects",
        ])
        .output()?;
    assert_success(&first_prompt, "first prompt")?;
    let first_stdout = stdout_text(&first_prompt);
    assert!(
        first_stdout.contains("Permission approval required"),
        "{first_stdout}"
    );
    assert!(!allowed_path.exists());

    let approved = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "ask",
            "approve_project",
            "--workspace",
            workspace_arg(&workspace).as_str(),
            "--session",
            "remembered-e2e-a",
            "--allow-side-effects",
        ])
        .output()?;
    assert_success(&approved, "project approval")?;
    assert_eq!(fs::read_to_string(&allowed_path)?, "approved-once");

    let reused = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "ask",
            "repeat write",
            "--workspace",
            workspace_arg(&workspace).as_str(),
            "--session",
            "remembered-e2e-b",
            "--allow-side-effects",
        ])
        .output()?;
    assert_success(&reused, "new session reuse")?;
    let reused_stdout = stdout_text(&reused);
    assert!(
        reused_stdout.contains("project approval reused"),
        "{reused_stdout}"
    );
    assert!(
        !reused_stdout.contains("Permission approval required"),
        "{reused_stdout}"
    );
    assert_eq!(fs::read_to_string(&allowed_path)?, "reused-in-new-session");

    // Then: CLI revoke removes the project rule and the next matching action prompts again.
    let rule_id = remembered_rule_id(&config_path, &workspace)?;
    let revoked = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "permissions",
            "revoke",
            rule_id.get(..16).unwrap_or(&rule_id),
            "--workspace",
            workspace_arg(&workspace).as_str(),
        ])
        .output()?;
    assert_success(&revoked, "revoke")?;
    let revoked_stdout = stdout_text(&revoked);
    assert!(
        revoked_stdout.contains("Revoked remembered permission"),
        "{revoked_stdout}"
    );

    let after_revoke = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "ask",
            "repeat after revoke",
            "--workspace",
            workspace_arg(&workspace).as_str(),
            "--session",
            "remembered-e2e-c",
            "--allow-side-effects",
        ])
        .output()?;
    assert_success(&after_revoke, "after revoke")?;
    let after_revoke_stdout = stdout_text(&after_revoke);
    assert!(
        after_revoke_stdout.contains("Permission approval required"),
        "{after_revoke_stdout}"
    );
    assert_eq!(fs::read_to_string(&allowed_path)?, "reused-in-new-session");

    // Then: protected store targets and malformed stores fail closed without executing the tool.
    let protected = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "ask",
            "modify permission store",
            "--workspace",
            workspace_arg(&workspace).as_str(),
            "--session",
            "remembered-e2e-protected",
            "--allow-side-effects",
        ])
        .output()?;
    assert_success(&protected, "protected target")?;
    assert!(!stdout_text(&protected).contains("sk-protected-sentinel"));
    let store_before_malformed = fs::read_to_string(&protected_store_path)?;
    assert!(!store_before_malformed.contains("sk-protected-sentinel"));

    fs::write(
        &protected_store_path,
        r#"{"schemaVersion":1,"rawArguments":"sk-malformed-sentinel","projects":"#,
    )?;
    let malformed_list = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "permissions",
            "list",
            "--workspace",
            workspace_arg(&workspace).as_str(),
        ])
        .output()?;
    assert!(!malformed_list.status.success());
    assert!(!stderr_text(&malformed_list).contains("sk-malformed-sentinel"));
    let malformed = shacs_bot(&config_path, &fake_responses_path)
        .args([
            "ask",
            "malformed store write",
            "--workspace",
            workspace_arg(&workspace).as_str(),
            "--session",
            "remembered-e2e-malformed",
            "--allow-side-effects",
        ])
        .output()?;
    assert_success(&malformed, "malformed store")?;
    let malformed_stdout = stdout_text(&malformed);
    assert!(
        malformed_stdout.contains("malformed store blocked"),
        "{malformed_stdout}"
    );
    assert!(!malformed_stdout.contains("sk-malformed-sentinel"));
    assert_eq!(fs::read_to_string(&allowed_path)?, "reused-in-new-session");
    Ok(())
}
