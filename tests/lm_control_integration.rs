use deterministic_ai_kernel::lm_control::parse_gb;

#[test]
fn parses_plain_gb_values() {
    assert_eq!(parse_gb("8 GB"), Some(8.0));
    assert_eq!(parse_gb("12GB"), Some(12.0));
}

#[test]
fn parses_decimal_gb_values() {
    assert_eq!(parse_gb("7.5 GB"), Some(7.5));
    assert_eq!(parse_gb("10.25GB"), Some(10.25));
}

#[test]
fn rejects_non_gb_values() {
    assert_eq!(parse_gb("8192 MB"), None);
    assert_eq!(parse_gb("unknown"), None);
    assert_eq!(parse_gb(""), None);
}
