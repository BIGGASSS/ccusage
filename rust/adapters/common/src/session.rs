//! Session-count selection, separate from the report's presentation ordering.
use std::collections::HashMap;

use ccusage_core::{
    LoadedEntry, TimestampMs, UsageSummary, cli::AgentReportKind, parse_ts_timestamp,
};

/// Keep the most recently active sessions. Call before the existing presentation
/// sort and before computing totals. Some adapters do not expose lastActivity in
/// their summaries, so derive recency from the underlying entries in that case.
pub fn limit_session_rows(
    rows: &mut Vec<UsageSummary>,
    entries: &[LoadedEntry],
    kind: AgentReportKind,
    last: Option<u32>,
) {
    if kind != AgentReportKind::Session || last.is_none() {
        return;
    }
    let mut activity = HashMap::<&str, TimestampMs>::new();
    for entry in entries {
        activity
            .entry(entry.session_id.as_ref())
            .and_modify(|timestamp| *timestamp = (*timestamp).max(entry.timestamp))
            .or_insert(entry.timestamp);
    }
    limit_recent(rows, last, |row| {
        let id = row.session_id.as_deref().unwrap_or_default();
        (
            row.last_activity
                .as_deref()
                .and_then(parse_ts_timestamp)
                .or_else(|| activity.get(id).copied()),
            id.to_owned(),
        )
    });
}

/// Select newest first, breaking timestamp ties by identifier for deterministic
/// membership. Missing timestamps are older than known timestamps. The caller
/// applies its usual --order after selection.
pub fn limit_recent<T>(
    rows: &mut Vec<T>,
    last: Option<u32>,
    key: impl Fn(&T) -> (Option<TimestampMs>, String),
) {
    let Some(last) = last else {
        return;
    };
    rows.sort_by_cached_key(|row| {
        let (timestamp, id) = key(row);
        (std::cmp::Reverse(timestamp), id)
    });
    rows.truncate(last as usize);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_by_activity_not_identifier_and_breaks_ties_deterministically() {
        let mut rows = vec![
            ("z-old", Some(1)),
            ("b-new", Some(3)),
            ("a-new", Some(3)),
            ("missing", None),
        ];
        limit_recent(&mut rows, Some(2), |(id, ts)| {
            (ts.map(TimestampMs::from_millis), id.to_string())
        });
        assert_eq!(rows, [("a-new", Some(3)), ("b-new", Some(3))]);
    }

    #[test]
    fn unlimited_oversized_zero_and_empty_limits() {
        let original = vec![("b", Some(1)), ("a", Some(2))];
        let key =
            |row: &(&str, Option<i64>)| (row.1.map(TimestampMs::from_millis), row.0.to_string());
        let mut rows = original.clone();
        limit_recent(&mut rows, None, key);
        assert_eq!(rows, original);
        limit_recent(&mut rows, Some(10), key);
        assert_eq!(rows.len(), 2);
        limit_recent(&mut rows, Some(0), key);
        assert!(rows.is_empty());
        limit_recent(&mut rows, Some(1), key);
        assert!(rows.is_empty());
    }
}
