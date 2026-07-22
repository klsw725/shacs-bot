use crate::runtime::{PermissionedAction, SessionApprovalReuseMatch};
use serde_json::Value;

pub(crate) fn session_approval_reuse_match(
    action: &PermissionedAction,
) -> SessionApprovalReuseMatch {
    exec_command_pattern(action)
        .map(|pattern| SessionApprovalReuseMatch::ExecCommandPattern { pattern })
        .unwrap_or_default()
}

pub(crate) fn session_approval_reuse_matches(
    reuse_match: &SessionApprovalReuseMatch,
    approved_action_digest: &str,
    action: &PermissionedAction,
) -> bool {
    match reuse_match {
        SessionApprovalReuseMatch::ExactAction => approved_action_digest == action.action_digest,
        SessionApprovalReuseMatch::ExecCommandPattern { pattern } => {
            exec_command(action).is_some_and(|command| command_matches_pattern(command, pattern))
        }
    }
}

pub(crate) fn session_approval_reuse_pattern(action: &PermissionedAction) -> Option<String> {
    exec_command_pattern(action)
}

pub(crate) fn same_session_approval_grant(
    existing_match: &SessionApprovalReuseMatch,
    existing_action_digest: &str,
    new_match: &SessionApprovalReuseMatch,
    new_action_digest: &str,
) -> bool {
    match (existing_match, new_match) {
        (SessionApprovalReuseMatch::ExactAction, SessionApprovalReuseMatch::ExactAction) => {
            existing_action_digest == new_action_digest
        }
        (
            SessionApprovalReuseMatch::ExecCommandPattern { pattern: existing },
            SessionApprovalReuseMatch::ExecCommandPattern { pattern: new },
        ) => existing == new,
        (
            SessionApprovalReuseMatch::ExactAction,
            SessionApprovalReuseMatch::ExecCommandPattern { .. },
        )
        | (
            SessionApprovalReuseMatch::ExecCommandPattern { .. },
            SessionApprovalReuseMatch::ExactAction,
        ) => false,
    }
}

fn exec_command_pattern(action: &PermissionedAction) -> Option<String> {
    let command = exec_command(action)?;
    command_pattern(command)
}

fn command_pattern(command: &str) -> Option<String> {
    let tokens = reusable_command_tokens(command)?;
    let prefix_len = command_prefix_len(&tokens).min(tokens.len());
    Some(format!("{} *", tokens[..prefix_len].join(" ")))
}

fn exec_command(action: &PermissionedAction) -> Option<&str> {
    (action.tool_name == "exec")
        .then(|| action.redacted_arguments.get("command"))
        .flatten()
        .and_then(Value::as_str)
}

fn reusable_command_tokens(command: &str) -> Option<Vec<&str>> {
    if command.trim().is_empty()
        || command.chars().any(|character| {
            matches!(
                character,
                '\n' | '\r' | ';' | '&' | '|' | '<' | '>' | '`' | '$' | '(' | ')' | '\'' | '"'
            )
        })
    {
        return None;
    }
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    (!tokens.is_empty()).then_some(tokens)
}

fn command_prefix_len(tokens: &[&str]) -> usize {
    if matches!(
        tokens,
        ["bun", "run" | "x", ..]
            | ["cargo", "add" | "run", ..]
            | ["consul", "kv", ..]
            | ["deno", "task", ..]
            | [
                "docker",
                "builder" | "compose" | "container" | "image" | "network" | "volume",
                ..
            ]
            | ["eksctl", "create", ..]
            | ["git", "config" | "remote" | "stash", ..]
            | ["ip", "addr" | "link" | "netns" | "route", ..]
            | ["kind", "create", ..]
            | ["kubectl", "kustomize" | "rollout", ..]
            | ["mc", "admin", ..]
            | ["npm", "exec" | "init" | "run" | "view", ..]
            | ["openssl", "req" | "x509", ..]
            | ["pnpm", "dlx" | "exec" | "run", ..]
            | ["podman", "container" | "image", ..]
            | ["pulumi", "stack", ..]
            | ["terraform", "workspace", ..]
            | ["vault", "auth" | "kv", ..]
            | ["yarn", "dlx" | "run", ..]
    ) || matches!(
        tokens.first(),
        Some(&("aws" | "az" | "doctl" | "gcloud" | "gh" | "sfdx"))
    ) {
        return 3;
    }
    if matches!(
        tokens.first(),
        Some(
            &("bazel"
                | "brew"
                | "bun"
                | "cargo"
                | "cdk"
                | "cf"
                | "cmake"
                | "composer"
                | "consul"
                | "crictl"
                | "deno"
                | "docker"
                | "eksctl"
                | "firebase"
                | "flyctl"
                | "git"
                | "go"
                | "gradle"
                | "helm"
                | "heroku"
                | "hugo"
                | "ip"
                | "kind"
                | "kubectl"
                | "kustomize"
                | "make"
                | "mc"
                | "minikube"
                | "mongosh"
                | "mysql"
                | "mvn"
                | "ng"
                | "npm"
                | "nvm"
                | "nx"
                | "openssl"
                | "pip"
                | "pipenv"
                | "pnpm"
                | "podman"
                | "poetry"
                | "psql"
                | "pulumi"
                | "pyenv"
                | "python"
                | "rake"
                | "rbenv"
                | "redis-cli"
                | "rustup"
                | "serverless"
                | "skaffold"
                | "sls"
                | "sst"
                | "swift"
                | "systemctl"
                | "terraform"
                | "tmux"
                | "turbo"
                | "ufw"
                | "vault"
                | "vercel"
                | "volta"
                | "wp"
                | "yarn")
        )
    ) {
        return 2;
    }
    1
}

fn command_matches_pattern(command: &str, pattern: &str) -> bool {
    let Some(prefix) = pattern.strip_suffix(" *") else {
        return false;
    };
    if reusable_command_tokens(command).is_none() {
        return false;
    }
    let command = command.trim().replace('\\', "/");
    let prefix = prefix.replace('\\', "/");
    if cfg!(windows) {
        let command = command.to_lowercase();
        let prefix = prefix.to_lowercase();
        command == prefix || command.starts_with(&format!("{prefix} "))
    } else {
        command == prefix || command.starts_with(&format!("{prefix} "))
    }
}

#[cfg(test)]
mod tests {
    use super::{command_matches_pattern, command_pattern, same_session_approval_grant};
    use crate::runtime::SessionApprovalReuseMatch;

    #[test]
    fn command_pattern_uses_open_code_arity_prefix() {
        assert_eq!(
            command_pattern("cargo test --workspace").as_deref(),
            Some("cargo test *")
        );
        assert_eq!(
            command_pattern("npm run dev -- --host").as_deref(),
            Some("npm run dev *")
        );
        assert_eq!(
            command_pattern("python script.py --verbose").as_deref(),
            Some("python script.py *")
        );
    }

    #[test]
    fn command_pattern_falls_back_to_first_token() {
        assert_eq!(
            command_pattern("custom-tool deploy --force").as_deref(),
            Some("custom-tool *")
        );
    }

    #[test]
    fn command_pattern_requires_token_boundary_and_simple_shell_command() {
        assert!(command_matches_pattern("cargo test", "cargo test *"));
        assert!(command_matches_pattern(
            "cargo test --workspace",
            "cargo test *"
        ));
        assert!(!command_matches_pattern("cargo testevil", "cargo test *"));
        assert!(!command_matches_pattern(
            "cargo test && rm -rf .",
            "cargo test *"
        ));
    }

    #[test]
    fn exact_session_approval_grants_are_distinguished_by_action_digest() {
        assert!(same_session_approval_grant(
            &SessionApprovalReuseMatch::ExactAction,
            "digest-a",
            &SessionApprovalReuseMatch::ExactAction,
            "digest-a"
        ));
        assert!(!same_session_approval_grant(
            &SessionApprovalReuseMatch::ExactAction,
            "digest-a",
            &SessionApprovalReuseMatch::ExactAction,
            "digest-b"
        ));
    }
}
