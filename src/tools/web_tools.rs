/// URL validation shared by fetch_url/open_url (audit finding M5).
///
/// Only http/https are allowed. Loopback, private, link-local (includes the
/// 169.254.169.254 cloud metadata endpoint), and unspecified addresses are
/// rejected to block naive SSRF. Note: DNS-rebinding is NOT fully mitigated
/// by literal-host checks — full mitigation requires resolving and pinning
/// the address before connecting; this is documented as a known limitation
/// of the TUI tool layer.
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
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            let blocked = match ip {
                std::net::IpAddr::V4(v4) => {
                    v4.is_private()
                        || v4.is_loopback()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                        || v4.is_broadcast()
                        || v4.is_documentation()
                }
                std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
            };
            if blocked {
                return Err(format!("host '{}' is not allowed", host));
            }
        }
    }

    Ok(parsed)
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
