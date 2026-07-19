use deterministic_ai_kernel::metrics::METRICS;

#[test]
fn test_metrics_record_correctly() {
    METRICS.reset();
    assert_eq!(METRICS.count("test_metric"), 0);
    assert_eq!(METRICS.last_ms("test_metric"), 0);
    assert_eq!(METRICS.avg_ms("test_metric"), 0.0);

    METRICS.record("test_metric", 10);
    assert_eq!(METRICS.count("test_metric"), 1);
    assert_eq!(METRICS.last_ms("test_metric"), 10);
    assert_eq!(METRICS.avg_ms("test_metric"), 10.0);

    METRICS.record("test_metric", 20);
    assert_eq!(METRICS.count("test_metric"), 2);
    assert_eq!(METRICS.last_ms("test_metric"), 20);
    assert_eq!(METRICS.avg_ms("test_metric"), 15.0);
}

#[test]
fn test_metrics_json_serialization() {
    METRICS.reset();
    METRICS.record("test_metric_json", 50);

    let val = METRICS.snapshot();
    assert!(val.is_object());
    let obj = val.as_object().unwrap();
    assert!(obj.contains_key("test_metric_json"));

    let entry = obj.get("test_metric_json").unwrap();
    assert_eq!(entry["total_ms"], 50);
    assert_eq!(entry["count"], 1);
    assert_eq!(entry["avg_ms"], 50.0);
    assert_eq!(entry["last_ms"], 50);
}

#[test]
fn test_metrics_reset_works() {
    METRICS.reset();
    METRICS.record("to_reset", 100);
    assert_eq!(METRICS.count("to_reset"), 1);

    METRICS.reset();
    assert_eq!(METRICS.count("to_reset"), 0);
    assert_eq!(METRICS.last_ms("to_reset"), 0);
}
