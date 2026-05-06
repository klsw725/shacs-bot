pub fn abbreviate_path(path: &str, max_len: usize) -> String {
    if path.is_empty() || max_len == 0 {
        return path.to_owned();
    }
    if path.starts_with("http://") || path.starts_with("https://") {
        return abbreviate_url(path, max_len);
    }
    let mut normalized = path.replace('\\', "/");
    if let Some(home) = home_dir_string() {
        if normalized == home {
            normalized = "~".to_owned();
        } else if let Some(rest) = normalized.strip_prefix(&format!("{home}/")) {
            normalized = format!("~/{rest}");
        }
    }
    if normalized.chars().count() <= max_len {
        return normalized;
    }
    let trimmed = normalized.trim_end_matches('/');
    let parts = trimmed.split('/').collect::<Vec<_>>();
    if parts.len() <= 1 {
        return truncate_with_ellipsis(&normalized, max_len);
    }
    let basename = parts.last().copied().unwrap_or_default();
    let Some(mut budget) = max_len.checked_sub(basename.chars().count() + 3) else {
        return format!(
            "…/{}",
            truncate_component(basename, max_len.saturating_sub(2))
        );
    };
    let mut kept = Vec::new();
    for segment in parts[..parts.len() - 1].iter().rev() {
        let needed = segment.chars().count() + 1;
        if needed <= budget {
            kept.push(*segment);
            budget -= needed;
        } else {
            break;
        }
    }
    kept.reverse();
    if kept.is_empty() {
        format!("…/{basename}")
    } else {
        format!("…/{}/{basename}", kept.join("/"))
    }
}

fn abbreviate_url(url: &str, max_len: usize) -> String {
    if url.chars().count() <= max_len {
        return url.to_owned();
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (domain, path_with_query) = rest.split_once('/').unwrap_or((rest, ""));
    let path_part = path_with_query.split(['?', '#']).next().unwrap_or_default();
    let basename = path_part
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if basename.is_empty() {
        return truncate_with_ellipsis(url, max_len);
    }
    let Some(mut budget) =
        max_len.checked_sub(domain.chars().count() + basename.chars().count() + 4)
    else {
        let trunc = max_len.saturating_sub(domain.chars().count() + 5);
        return format!("{domain}/…/{}", truncate_component(basename, trunc));
    };
    let segments = path_part
        .trim_end_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    let mut kept = Vec::new();
    for segment in segments[..segments.len().saturating_sub(1)].iter().rev() {
        let needed = segment.chars().count() + 1;
        if needed <= budget {
            kept.push(*segment);
            budget -= needed;
        } else {
            break;
        }
    }
    kept.reverse();
    if kept.is_empty() {
        format!("{domain}/…/{basename}")
    } else {
        format!("{domain}/…/{}/{basename}", kept.join("/"))
    }
}

fn truncate_with_ellipsis(value: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    if value.chars().count() <= max_len {
        return value.to_owned();
    }
    let take = max_len.saturating_sub(1);
    format!("{}…", value.chars().take(take).collect::<String>())
}

fn truncate_component(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect()
}

fn home_dir_string() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| home.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviates_paths_urls_and_windows_separators() {
        assert_eq!(abbreviate_path("short.txt", 40), "short.txt");
        assert_eq!(
            abbreviate_path("/very/long/path/to/file.txt", 18),
            "…/to/file.txt"
        );
        assert_eq!(
            abbreviate_path("C:\\very\\long\\file.txt", 18),
            "…/long/file.txt"
        );
        assert_eq!(
            abbreviate_path("https://example.com/api/v1/resource.json?x=1", 32),
            "example.com/…/v1/resource.json"
        );
        assert!(abbreviate_path("abcdef", 3).ends_with('…'));
    }
}
