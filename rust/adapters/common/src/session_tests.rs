//! Shared fixture-backed contract tests compiled in each summary-based adapter.
use std::sync::Arc;

use ccusage_core::{
    LoadedEntry, UsageEntry,
    cli::{AgentReportKind, SortOrder},
    parse_ts_timestamp, sort_summaries, summary_period,
};

fn entries() -> Vec<LoadedEntry> {
    let records: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../tests/fixtures/session_activity.json")).unwrap();
    records
        .iter()
        .map(|record| {
            let id = record["sessionId"].as_str().unwrap();
            let timestamp = record["timestamp"].as_str().unwrap();
            let data: UsageEntry = serde_json::from_value(serde_json::json!({
                "sessionId": id,
                "timestamp": timestamp,
                "message": {
                    "model": "test-model",
                    "usage": {"input_tokens": record["inputTokens"], "output_tokens": 0,
                        "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
                }
            }))
            .unwrap();
            LoadedEntry {
                data,
                timestamp: parse_ts_timestamp(timestamp).unwrap(),
                date: timestamp[..10].to_string(),
                project: Arc::from("test-project"),
                session_id: Arc::from(id),
                project_path: Arc::from("/test-project"),
                cost: record["inputTokens"].as_f64().unwrap() / 100.0,
                extra_total_tokens: 0,
                credits: None,
                message_count: Some(1),
                model: Some("test-model".to_string()),
                usage_limit_reset_time: None,
                missing_pricing_model: None,
            }
        })
        .collect()
}

#[test]
fn last_sessions_selects_recent_activity_before_order_and_totals() {
    let entries = entries();
    for order in [SortOrder::Asc, SortOrder::Desc] {
        let mut rows = super::summarize_entries(&entries, AgentReportKind::Session).unwrap();
        ccusage_adapter_common::limit_session_rows(
            &mut rows,
            &entries,
            AgentReportKind::Session,
            Some(2),
        );
        sort_summaries(&mut rows, &order, summary_period);
        let ids = rows
            .iter()
            .map(|row| row.session_id.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            if matches!(order, SortOrder::Asc) {
                vec!["a-resumed", "m-new"]
            } else {
                vec!["m-new", "a-resumed"]
            }
        );
        let report = super::report::report_from_rows(&rows, AgentReportKind::Session);
        assert_eq!(report["sessions"].as_array().unwrap().len(), 2);
        assert_eq!(report["totals"]["inputTokens"], 60);
        assert_eq!(report["totals"]["totalTokens"], 60);
        assert!((report["totals"]["totalCost"].as_f64().unwrap() - 0.6).abs() < 1e-9);
    }
}

#[test]
fn last_one_includes_all_usage_of_resumed_session_and_handles_large_or_empty_limits() {
    let entries = entries();
    let kind = AgentReportKind::Session;
    let mut rows = super::summarize_entries(&entries, kind).unwrap();
    ccusage_adapter_common::limit_session_rows(&mut rows, &entries, kind, Some(100));
    assert_eq!(rows.len(), 3);
    ccusage_adapter_common::limit_session_rows(&mut rows, &entries, kind, Some(1));
    assert_eq!(rows[0].session_id.as_deref(), Some("a-resumed"));
    assert_eq!(
        super::report::report_from_rows(&rows, kind)["totals"]["inputTokens"],
        40
    );
    rows.clear();
    ccusage_adapter_common::limit_session_rows(&mut rows, &entries, kind, Some(1));
    assert!(rows.is_empty());
}

#[test]
fn last_does_not_limit_period_rows_and_no_limit_preserves_sessions() {
    let entries = entries();
    for kind in [
        AgentReportKind::Daily,
        AgentReportKind::Weekly,
        AgentReportKind::Monthly,
        AgentReportKind::Session,
    ] {
        let mut rows = super::summarize_entries(&entries, kind).unwrap();
        let before = super::report::report_from_rows(&rows, kind);
        let last = if kind == AgentReportKind::Session {
            None
        } else {
            Some(1)
        };
        ccusage_adapter_common::limit_session_rows(&mut rows, &entries, kind, last);
        assert_eq!(super::report::report_from_rows(&rows, kind), before);
    }
}
