//! `web_fetch(domain:<host>)` patterns (Spec 048 PRD 000 scope 4): `*.x`
//! covers subdomains at any depth but not `x`, a lone `*` covers every host,
//! and any other `*` stays inside one label.

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostPattern {
    text: String,
}

impl HostPattern {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim().trim_end_matches('.').to_ascii_lowercase();
        (!text.is_empty() && !text.contains(['/', ':', ' '])).then_some(Self { text })
    }

    pub(crate) fn matches(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if self.text == "*" {
            return true;
        }
        let host_labels = host.split('.').collect::<Vec<_>>();
        if let Some(rest) = self.text.strip_prefix("*.") {
            let rest_labels = rest.split('.').collect::<Vec<_>>();
            return host_labels.len() > rest_labels.len()
                && labels_match(
                    &rest_labels,
                    &host_labels[host_labels.len() - rest_labels.len()..],
                );
        }
        let pattern_labels = self.text.split('.').collect::<Vec<_>>();
        pattern_labels.len() == host_labels.len() && labels_match(&pattern_labels, &host_labels)
    }
}

fn labels_match(patterns: &[&str], labels: &[&str]) -> bool {
    patterns.iter().zip(labels).all(|(pattern, label)| {
        if !pattern.contains('*') {
            return pattern == label;
        }
        let expression = pattern
            .split('*')
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join("[^.]*");
        Regex::new(&format!("^{expression}$")).is_ok_and(|regex| regex.is_match(label))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_patterns_follow_the_claude_code_rules() {
        let cases = [
            ("example.com", "EXAMPLE.com.", true),
            ("example.com", "api.example.com", false),
            ("*.example.com", "a.b.example.com", true),
            ("*.example.com", "example.com", false),
            ("*", "anything.test", true),
            ("example.*", "example.org", true),
            ("example.*", "example.evil.com", false),
        ];
        for (rule, host, expected) in cases {
            let pattern = HostPattern::parse(rule).expect("test host pattern should parse");
            assert_eq!(pattern.matches(host), expected, "{rule} vs {host}");
        }
    }
}
