//! The classifier's trust boundary and rules: built-in defaults and the
//! user's `permissions.autoMode` lists (Spec 048 PRD 000 scope 13).

use serde::Serialize;

pub const DEFAULTS_MARKER: &str = "$defaults";

const DEFAULT_ENVIRONMENT: &[&str] = &[
    "[Workspace] {workspace} and everything under it. It is the user's own working folder.",
    "[Current Conversation] {conversation}, where the latest user message came from. Sending results there reaches the user.",
    "[Outside] Everything else is outside the trust boundary: other folders, websites, hosts, APIs, accounts, repositories, chats, channels, and people, unless a user message in this session names them.",
];
const DEFAULT_HARD_DENY: &[&str] = &[
    "[Credential Exfiltration] Sending, uploading, posting, or writing outside the workspace any credential, token, API key, password, cookie, or private key, or the contents of auth.json, the auth/ folder, or .env.json, whatever the user asked.",
];
const DEFAULT_SOFT_DENY: &[&str] = &[
    "[Irreversible Deletion] Deleting, emptying, or overwriting files, folders, data, or history the user did not name; recursive or mass deletion (rm -r, find -delete, git clean); force-pushing; dropping databases.",
    "[Data Exfiltration] Sending workspace files, memory, conversation content, or personal data outside the trust boundary, through requests that carry data, uploads, pastes, gists, emails, or messages to other chats, unless the user asked to send that content to that destination.",
    "[Security Weakening] Changing the assistant's own permission or auto mode settings, config.json, permissions.json, instruction files, skill, workflow, agent, or plugin definitions, session records, or the data folder's runtime state; turning off logs or diagnostics; adding persistence the user did not ask for, such as scheduled jobs, startup scripts, shell profiles, or SSH keys.",
    "[Untrusted Code] Downloading and running code from outside the trust boundary, such as curl ... | sh, running scripts from a repository the assistant just fetched, or installing a package that neither the user, the skill in use, nor a manifest in the workspace names.",
    "[Credential Search] Searching environment variables, config files, keychains, or other files for credentials or tokens, or using a credential the user did not provide for this task.",
    "[Unrequested Account Action] Acting through an account or an authenticated API (posting, buying, booking, changing settings, deleting) when the user did not ask for that action.",
    "[Affecting Others] Messaging people, chats, or channels other than the current conversation, or changing or deleting shared resources the assistant did not create in this session.",
];
const DEFAULT_ALLOW: &[&str] = &[
    "[Current Conversation] Sending results, summaries, or generated files to the current conversation.",
    "[Public Reading] Web searches and fetching public web pages for the user's request, when nothing but the search terms or the page address leaves the machine.",
    "[Skill Setup] Installing a package into the user's own environment when a skill in use for this request needs it, for example pip install youtube-transcript-api for youtube-content.",
    "[Own Files] Deleting or overwriting files the assistant itself created earlier in this session.",
    "[User Login] Running a login or authorization flow the user started.",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoModeSlots {
    pub environment: Vec<String>,
    pub hard_deny: Vec<String>,
    pub soft_deny: Vec<String>,
    pub allow: Vec<String>,
}

/// The user's lists; `None` keeps the default list.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotOverrides<'a> {
    pub environment: Option<&'a [String]>,
    pub hard_deny: Option<&'a [String]>,
    pub soft_deny: Option<&'a [String]>,
    pub allow: Option<&'a [String]>,
}

impl Default for AutoModeSlots {
    fn default() -> Self {
        Self::resolve(SlotOverrides::default())
    }
}

impl AutoModeSlots {
    pub fn resolve(overrides: SlotOverrides<'_>) -> Self {
        Self {
            environment: splice(overrides.environment, DEFAULT_ENVIRONMENT),
            hard_deny: splice(overrides.hard_deny, DEFAULT_HARD_DENY),
            soft_deny: splice(overrides.soft_deny, DEFAULT_SOFT_DENY),
            allow: splice(overrides.allow, DEFAULT_ALLOW),
        }
    }

    /// Fills `{workspace}` and `{conversation}` for one classifier request.
    pub fn filled(&self, workspace: &str, conversation: &str) -> Self {
        let fill = |entries: &[String]| {
            entries
                .iter()
                .map(|entry| {
                    entry
                        .replace("{workspace}", workspace)
                        .replace("{conversation}", conversation)
                })
                .collect()
        };
        Self {
            environment: fill(&self.environment),
            hard_deny: fill(&self.hard_deny),
            soft_deny: fill(&self.soft_deny),
            allow: fill(&self.allow),
        }
    }

    /// Keeps the entries whose `[Label]` starts with `prefix`, ignoring case.
    pub fn with_label_prefix(&self, prefix: &str) -> Self {
        let prefix = prefix.to_lowercase();
        let keep = |entries: &[String]| {
            entries
                .iter()
                .filter(|entry| {
                    entry
                        .strip_prefix('[')
                        .is_some_and(|label| label.to_lowercase().starts_with(&prefix))
                })
                .cloned()
                .collect()
        };
        Self {
            environment: keep(&self.environment),
            hard_deny: keep(&self.hard_deny),
            soft_deny: keep(&self.soft_deny),
            allow: keep(&self.allow),
        }
    }
}

fn splice(user: Option<&[String]>, defaults: &[&str]) -> Vec<String> {
    let Some(user) = user else {
        return defaults.iter().map(|entry| (*entry).to_owned()).collect();
    };
    user.iter()
        .flat_map(|entry| {
            if entry == DEFAULTS_MARKER {
                defaults.iter().map(|entry| (*entry).to_owned()).collect()
            } else {
                vec![entry.clone()]
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "classifier_slots_tests.rs"]
mod tests;
