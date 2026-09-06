use std::net::Ipv6Addr;

pub(super) fn web_uri_syntax_slash(text: &str, position: usize) -> bool {
    if text.as_bytes().get(position) != Some(&b'/') {
        return false;
    }
    let separator = if position > 0
        && text.as_bytes()[position - 1] == b':'
        && text[position..].starts_with("//")
    {
        position - 1
    } else if position > 1
        && text.as_bytes()[position - 1] == b'/'
        && text.as_bytes()[position - 2] == b':'
    {
        position - 2
    } else {
        return false;
    };
    let scheme_start = text[..separator]
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            (!scheme_character(character)).then_some(index + character.len_utf8())
        })
        .unwrap_or(0);
    let scheme = &text[scheme_start..separator];
    ["http", "https", "ws", "wss"]
        .iter()
        .any(|expected| scheme.eq_ignore_ascii_case(expected))
        && valid_authority(&text[separator + 3..])
}

fn valid_authority(text: &str) -> bool {
    let end = text
        .char_indices()
        .find_map(|(index, character)| {
            (matches!(character, '/' | '?' | '#') || uri_terminator(character)).then_some(index)
        })
        .unwrap_or(text.len());
    let authority = &text[..end];
    let host_port = match authority.rsplit_once('@') {
        Some((userinfo, host_port)) if valid_userinfo(userinfo) => host_port,
        Some(_) => return false,
        None => authority,
    };
    if let Some(bracketed) = host_port.strip_prefix('[') {
        let Some((host, port)) = bracketed.split_once(']') else {
            return false;
        };
        return host.parse::<Ipv6Addr>().is_ok() && valid_port_suffix(port);
    }
    let (host, port) = host_port.rsplit_once(':').map_or((host_port, None), |(host, port)| {
        (host, Some(port))
    });
    valid_host(host) && port.map_or(true, valid_port)
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
                && label.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
                && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_userinfo(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if bytes.get(index + 1).map_or(true, |value| !value.is_ascii_hexdigit())
                || bytes.get(index + 2).map_or(true, |value| !value.is_ascii_hexdigit())
            {
                return false;
            }
            index += 3;
        } else if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':')
        {
            index += 1;
        } else {
            return false;
        }
    }
    !bytes.is_empty()
}

fn valid_port_suffix(value: &str) -> bool {
    value.is_empty() || value.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u16>().is_ok()
}

fn scheme_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
}

fn uri_terminator(character: char) -> bool {
    character.is_whitespace()
        || character.is_control()
        || matches!(character, '(' | ')' | '{' | '}' | '"' | '\'' | '`' | '<' | '>' | '|')
}
