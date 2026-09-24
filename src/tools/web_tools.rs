/// URL validation shared by fetch_url/open_url (audit finding M5, M-3).
///
/// Only http/https are allowed. Loopback, private, link-local (includes the
/// 169.254.169.254 cloud metadata endpoint), unique-local IPv6, IPv4-mapped
/// IPv6, and unspecified addresses are rejected to block naive SSRF.
///
/// Note: DNS-rebinding is NOT fully mitigated by literal-host checks — full
/// mitigation requires resolving and pinning the address before connecting;
/// this is documented as a known limitation of the TUI tool layer.
fn validate_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid url: {e}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "url scheme '{}' not allowed (only http/https)",
                other
            ))
        }
    }

    if let Some(host) = parsed.host_str() {
        let host_lower = host.to_ascii_lowercase();
        if host_lower == "localhost"
            || host_lower.ends_with(".local")
            || host_lower.ends_with(".internal")
        {
            return Err(format!("host '{}' is not allowed", host));
        }

        // host_str() keeps the brackets around an IPv6 literal ("[::1]"), so
        // parsing it as an IpAddr fails. The previous code parsed `host`
        // directly inside `if let Ok(ip) = ...`, which meant every IPv6
        // literal silently skipped the address checks. Strip the brackets so
        // the blocklist actually runs.
        let literal = host_lower.trim_matches(|c| c == '[' || c == ']');
        if let Ok(ip) = literal.parse::<std::net::IpAddr>() {
            let blocked = match ip {
                std::net::IpAddr::V4(v4) => is_blocked_v4(&v4),
                std::net::IpAddr::V6(v6) => is_blocked_v6(&v6),
            };
            if blocked {
                return Err(format!("host '{}' is not allowed", host));
            }
        }
    }

    Ok(parsed)
}

fn is_blocked_v4(v4: &std::net::Ipv4Addr) -> bool {
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_documentation()
}

fn is_blocked_v6(v6: &std::net::Ipv6Addr) -> bool {
    let segments = v6.segments();
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        // fc00::/7 — unique local address
        || (segments[0] & 0xfe00) == 0xfc00
        // fe80::/10 — link local
        || (segments[0] & 0xffc0) == 0xfe80
        // ::ffff:0:0/96 and the deprecated ::a.b.c.d form must be judged by
        // the embedded IPv4, otherwise they bypass the v4 blocklist.
        || v6
            .to_ipv4()
            .map(|embedded| is_blocked_v4(&embedded))
            .unwrap_or(false)
}

/// Truncate at a UTF-8 char boundary (the previous byte-index slice panicked
/// on multi-byte content).
fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub async fn fetch_url(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("missing 'url'")?;
    let parsed = validate_url(url)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("client error: {e}"))?;

    let resp = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let body = resp.text().await.map_err(|e| format!("body error: {e}"))?;
    let truncated = body.len() > 50_000;
    let content = truncate_utf8(&body, 50_000);

    Ok(serde_json::json!({
        "url": url,
        "status": status,
        "content_type": content_type,
        "content": content,
        "truncated": truncated,
        "bytes": body.len(),
    }))
}

pub async fn open_url(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("missing 'url'")?;
    // Only http/https may be handed to the OS opener (file:// and friends
    // are rejected before any process is spawned).
    let parsed = validate_url(url)?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(parsed.as_str())
            .spawn()
            .map_err(|e| format!("open error: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(parsed.as_str())
            .spawn()
            .map_err(|e| format!("open error: {e}"))?;
    }

    Ok(serde_json::json!({
        "url": url,
        "opened": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::validate_url;

    fn rejected(url: &str) -> String {
        validate_url(url)
            .err()
            .unwrap_or_else(|| panic!("{url} must be rejected, but validate_url accepted it"))
    }

    /// The M-3 bypass: host_str() returns "[::1]" with brackets, so the old
    /// `host.parse::<IpAddr>()` failed and the whole address blocklist was
    /// skipped for every IPv6 literal.
    #[test]
    fn rejects_bracketed_ipv6_loopback() {
        assert!(rejected("http://[::1]:8080/").contains("not allowed"));
    }

    #[test]
    fn rejects_ipv4_mapped_ipv6() {
        assert!(rejected("http://[::ffff:127.0.0.1]/").contains("not allowed"));
    }

    #[test]
    fn rejects_ipv6_unique_local_and_link_local() {
        assert!(rejected("http://[fd12:3456::1]/").contains("not allowed"));
        assert!(rejected("http://[fc00::1]/").contains("not allowed"));
        assert!(rejected("http://[fe80::1]/").contains("not allowed"));
    }

    #[test]
    fn rejects_ipv6_unspecified_and_multicast() {
        assert!(rejected("http://[::]/").contains("not allowed"));
        assert!(rejected("http://[ff02::1]/").contains("not allowed"));
    }

    #[test]
    fn rejects_cloud_metadata_and_private_ipv4() {
        assert!(rejected("http://169.254.169.254/latest/meta-data/").contains("not allowed"));
        assert!(rejected("http://127.0.0.1:1234/v1").contains("not allowed"));
        assert!(rejected("http://10.0.0.5/").contains("not allowed"));
        assert!(rejected("http://192.168.1.1/").contains("not allowed"));
        assert!(rejected("http://0.0.0.0/").contains("not allowed"));
    }

    #[test]
    fn rejects_localhost_and_internal_names() {
        assert!(rejected("http://localhost:8080/").contains("not allowed"));
        assert!(rejected("http://printer.local/").contains("not allowed"));
        assert!(rejected("http://db.internal/").contains("not allowed"));
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(rejected("file:///etc/passwd").contains("scheme"));
        assert!(rejected("ftp://example.com/x").contains("scheme"));
    }

    /// Documents what the WHATWG URL parser does with the non-dotted decimal
    /// IPv4 form. If it normalizes to 127.0.0.1 the request is blocked; if it
    /// keeps it as an opaque host, this is a residual gap and the test says so.
    #[test]
    fn decimal_ipv4_form_is_blocked_or_explicitly_flagged() {
        match validate_url("http://2130706433/") {
            Err(_) => {}
            Ok(parsed) => panic!(
                "RESIDUAL GAP: decimal IPv4 was accepted and parsed to host {:?} — \
                 the literal blocklist cannot see it; resolution+pinning is required",
                parsed.host_str()
            ),
        }
    }

    #[test]
    fn allows_public_hosts() {
        assert!(validate_url("https://example.com/docs").is_ok());
        assert!(validate_url("https://93.184.216.34/").is_ok());
        assert!(validate_url("https://[2606:2800:220:1:248:1893:25c8:1946]/").is_ok());
    }
}
