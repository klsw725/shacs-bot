//! `exec(<pattern>)` matching with Claude Code wildcard rules and the broad
//! rules auto mode suspends (Spec 048 PRD 000 scope 4).

use regex::Regex;

const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "ksh",
    "python",
    "python2",
    "python3",
    "pypy",
    "node",
    "deno",
    "bun",
    "ruby",
    "perl",
    "php",
    "lua",
    "Rscript",
    "osascript",
    "pwsh",
    "powershell",
];
const WRAPPERS: &[&str] = &[
    "env", "xargs", "sudo", "su", "doas", "nohup", "timeout", "nice", "time", "stdbuf", "watch",
    "setsid", "ionice", "flock", "eval", "exec", "command", "builtin", "noglob", "ssh",
];
const RUNNERS: &[&str] = &[
    "npx",
    "pnpx",
    "bunx",
    "uvx",
    "npm run",
    "npm exec",
    "npm x",
    "pnpm run",
    "pnpm exec",
    "pnpm dlx",
    "yarn run",
    "yarn exec",
    "yarn dlx",
    "bun run",
    "bun x",
    "deno run",
    "deno task",
    "uv run",
    "pipx run",
    "poetry run",
    "cargo run",
    "go run",
    "make",
    "just",
];
const EXEC_WRAPPERS: &[&str] = &["watch", "setsid", "ionice", "flock"];
const FIND_ACTIONS: &[&str] = &[
    "-exec",
    "-execdir",
    "-ok",
    "-okdir",
    "-delete",
    "-fprint",
    "-fprint0",
    "-fprintf",
    "-fls",
    "-files0-from",
];

#[derive(Debug, Clone)]
pub(crate) struct ExecPattern {
    text: String,
    matcher: Matcher,
}

#[derive(Debug, Clone)]
enum Matcher {
    Exact,
    /// `<prefix> *` as the only wildcard: the prefix alone or followed by a space.
    WordPrefix(String),
    Wildcard(Regex),
}

impl PartialEq for ExecPattern {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for ExecPattern {}

impl ExecPattern {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = match collapsed.strip_suffix(":*") {
            Some(prefix) => format!("{} *", prefix.trim_end()),
            None => collapsed,
        };
        if text.is_empty() {
            return None;
        }
        let wildcards = text.matches('*').count();
        let matcher = if wildcards == 0 {
            Matcher::Exact
        } else if wildcards == 1 && text.ends_with(" *") {
            Matcher::WordPrefix(text[..text.len() - 2].to_owned())
        } else {
            let expression = text
                .split('*')
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join(".*");
            Matcher::Wildcard(Regex::new(&format!("^(?s:{expression})$")).ok()?)
        };
        Some(Self { text, matcher })
    }

    pub(crate) fn has_wildcard(&self) -> bool {
        !matches!(self.matcher, Matcher::Exact)
    }

    pub(crate) fn matches(&self, command: &str) -> bool {
        match &self.matcher {
            Matcher::Exact => command == self.text,
            Matcher::WordPrefix(prefix) => {
                command == prefix
                    || command
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| rest.starts_with(' '))
            }
            Matcher::Wildcard(regex) => regex.is_match(command),
        }
    }

    /// Finds the pattern inside text that could not be split into
    /// subcommands, starting and ending at word boundaries.
    pub(crate) fn appears_in(&self, text: &str) -> bool {
        let body = self
            .text
            .trim_end_matches(" *")
            .split('*')
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join(".*");
        Regex::new(&format!(r"(?s)(^|[^\w-]){body}($|[^\w-])"))
            .is_ok_and(|regex| regex.is_match(text))
    }

    /// A wildcard allow rule cannot approve exec wrappers or `find` actions
    /// that run, delete or write.
    pub(crate) fn allow_excludes(&self, words: &[&str]) -> bool {
        if !self.has_wildcard() {
            return false;
        }
        match words.first() {
            Some(first) if EXEC_WRAPPERS.contains(first) => true,
            Some(&"find") => words.iter().any(|word| FIND_ACTIONS.contains(word)),
            _ => false,
        }
    }

    /// The pattern lets any program or code run (Spec 048 PRD 000 scope 4).
    pub(crate) fn is_broad(&self) -> bool {
        let Some(star) = self.text.find('*') else {
            return false;
        };
        let prefix = &self.text[..star];
        if prefix.trim().is_empty() {
            return true;
        }
        let words = prefix.split_whitespace().collect::<Vec<_>>();
        if !prefix.ends_with(' ') && words.len() == 1 {
            let partial = words[0];
            return INTERPRETERS
                .iter()
                .chain(WRAPPERS)
                .chain(RUNNERS)
                .any(|name| {
                    name.split(' ')
                        .next()
                        .is_some_and(|first| first.starts_with(partial))
                });
        }
        let options_only = |rest: &[&str]| rest.iter().all(|word| word.starts_with('-'));
        if WRAPPERS.contains(&words[0])
            || (INTERPRETERS.contains(&words[0]) && options_only(&words[1..]))
        {
            return true;
        }
        RUNNERS.iter().any(|runner| {
            let runner_words = runner.split(' ').collect::<Vec<_>>();
            words.len() >= runner_words.len()
                && words[..runner_words.len()] == runner_words[..]
                && options_only(&words[runner_words.len()..])
        })
    }

    /// An allow rule with a `*` right after a single word, as in
    /// `git * main`, lets every subcommand of that program through.
    pub(crate) fn wildcard_before_subcommand(&self) -> bool {
        let Some(star) = self.text.find('*') else {
            return false;
        };
        let prefix = self.text[..star].trim_end();
        prefix.split_whitespace().count() == 1 && self.text.len() > star + 1
    }
}

#[cfg(test)]
#[path = "exec_pattern_tests.rs"]
mod tests;
