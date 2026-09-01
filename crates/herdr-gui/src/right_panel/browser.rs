//! [INPUT]: std, gpui, gpui_component, theme
//! [OUTPUT]: Provides AddressTarget, resolve_address, display_url, search_url
//! [POS]: The right-panel Browser preview module of crates/herdr-gui

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressTarget {
    Url(String),
    Search(String),
}

/// Safari-style omnibox resolution: explicit schemes pass through, host-like
/// text gets a scheme guessed for it, anything else becomes a web search.
pub fn resolve_address(raw: &str) -> Option<AddressTarget> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let has_scheme = trimmed.split_once(':').is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && (rest.starts_with("//") || matches!(scheme, "about" | "data" | "mailto" | "file"))
    });
    if has_scheme {
        return Some(AddressTarget::Url(trimmed.to_owned()));
    }

    if trimmed.contains(char::is_whitespace) {
        return Some(AddressTarget::Search(trimmed.to_owned()));
    }

    let authority = trimmed.split(['/', '?', '#']).next().unwrap_or(trimmed);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, true)
        }
        Some(_) => return Some(AddressTarget::Search(trimmed.to_owned())),
        None => (authority, false),
    };
    let is_ip = !host.is_empty()
        && host.chars().all(|c| c.is_ascii_digit() || c == '.')
        && host.split('.').count() == 4;
    let is_local = host.eq_ignore_ascii_case("localhost") || is_ip;
    let host_like = is_local
        || (host.contains('.')
            && !host.starts_with('.')
            && !host.ends_with('.')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')));

    if !host_like {
        return Some(AddressTarget::Search(trimmed.to_owned()));
    }

    // Dev servers rarely speak TLS; the public web rarely speaks anything else.
    let scheme = if is_local || (port && host.eq_ignore_ascii_case("localhost")) {
        "http"
    } else {
        "https"
    };
    Some(AddressTarget::Url(format!("{scheme}://{trimmed}")))
}

pub fn search_url(query: &str) -> String {
    let mut encoded = String::with_capacity(query.len() * 3);
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("https://www.google.com/search?q={encoded}")
}

pub fn display_url(url: &str) -> &str {
    url.strip_prefix("https://").unwrap_or(url)
}

pub fn is_secure_url(url: &str) -> bool {
    url.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_resolve_correctly() {
        assert_eq!(
            resolve_address("https://example.com"),
            Some(AddressTarget::Url("https://example.com".into()))
        );
        assert_eq!(
            resolve_address("localhost:3000"),
            Some(AddressTarget::Url("http://localhost:3000".into()))
        );
        assert_eq!(
            resolve_address("127.0.0.1:8080/api"),
            Some(AddressTarget::Url("http://127.0.0.1:8080/api".into()))
        );
        assert_eq!(
            resolve_address("example.com/docs"),
            Some(AddressTarget::Url("https://example.com/docs".into()))
        );
        assert_eq!(
            resolve_address("rust borrow checker"),
            Some(AddressTarget::Search("rust borrow checker".into()))
        );
        assert_eq!(resolve_address("   "), None);
    }

    #[test]
    fn search_urls_encode_queries() {
        assert_eq!(
            search_url("rust async"),
            "https://www.google.com/search?q=rust+async"
        );
        assert_eq!(
            search_url("a&b=c"),
            "https://www.google.com/search?q=a%26b%3Dc"
        );
    }

    #[test]
    fn display_and_secure_url_checks() {
        assert_eq!(display_url("https://example.com"), "example.com");
        assert_eq!(
            display_url("http://localhost:3000"),
            "http://localhost:3000"
        );
        assert!(is_secure_url("https://github.com"));
        assert!(!is_secure_url("http://localhost:5173"));
    }
}
