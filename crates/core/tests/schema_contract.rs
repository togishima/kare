//! Guards the `Report` JSON output contract against unintended breakage.

use kare_core::analysis::{HealthStatus, Report, SCHEMA_VERSION};
use kare_core::db::RunSummary;

fn sample_report() -> Report {
    Report {
        schema_version: SCHEMA_VERSION,
        score: 100,
        status: HealthStatus::Healthy,
        run: RunSummary {
            id: 1,
            run_at: "2026-07-01T00:00:00Z".to_string(),
            git_ref: None,
            total_time_sec: 1.0,
            total_tests: 1,
            total_failures: 0,
        },
        slow: Vec::new(),
        flaky: Vec::new(),
        regression: Vec::new(),
        cost: None,
        insufficient_history: None,
    }
}

#[test]
fn report_json_contract_has_expected_shape() {
    assert_eq!(SCHEMA_VERSION, 1);

    let report = sample_report();
    let value = serde_json::to_value(&report).expect("serialize report to JSON");

    assert_eq!(value["schema_version"], serde_json::json!(1));

    let obj = value.as_object().expect("report serializes as an object");
    for key in ["score", "status", "slow", "flaky", "regression", "cost"] {
        assert!(obj.contains_key(key), "missing top-level key: {key}");
    }
}
