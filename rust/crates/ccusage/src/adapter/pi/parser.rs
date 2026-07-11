use std::{fs, path::Path, sync::Arc};

use jiff::tz::TimeZone as JiffTimeZone;
use serde::Deserialize;

use super::super::jsonl;
use crate::{
    LoadedEntry, Pricing, PricingMap, Result, TimestampMs, TokenUsageRaw, UsageEntry, UsageMessage,
    apply_total_token_fallback, calculate_cost_for_usage, calculate_cost_from_pricing,
    cli::CostMode, fast::LinePrefilter, format_date_tz, format_rfc3339_millis,
    missing_pricing_model_for_usage,
};

/// A single parsed pi session record. Only the fields ccusage consumes are
/// declared; serde skips everything else.
#[derive(Debug, Deserialize)]
struct PiLine {
    #[serde(default, deserialize_with = "jsonl::non_empty_string")]
    r#type: Option<String>,
    #[serde(default, deserialize_with = "jsonl::non_empty_string")]
    timestamp: Option<String>,
    message: Option<PiMessage>,
}

/// The pi `message` block carried by assistant records.
#[derive(Debug, Deserialize)]
struct PiMessage {
    #[serde(default, deserialize_with = "jsonl::non_empty_string")]
    role: Option<String>,
    #[serde(default, deserialize_with = "jsonl::non_empty_string")]
    model: Option<String>,
    usage: Option<PiUsage>,
    #[serde(
        rename = "toolName",
        default,
        deserialize_with = "jsonl::non_empty_string"
    )]
    tool_name: Option<String>,
    #[serde(
        rename = "toolCallId",
        default,
        deserialize_with = "jsonl::non_empty_string"
    )]
    tool_call_id: Option<String>,
    #[serde(default, deserialize_with = "jsonl::lenient_object")]
    details: Option<PiSubagentDetails>,
}

#[derive(Debug, Default, Deserialize)]
struct PiSubagentDetails {
    #[serde(default, deserialize_with = "jsonl::lenient_vec")]
    results: Vec<PiSubagentResult>,
}

#[derive(Debug, Default, Deserialize)]
struct PiSubagentResult {
    #[serde(default, deserialize_with = "jsonl::non_empty_string")]
    model: Option<String>,
    #[serde(default, deserialize_with = "jsonl::lenient_object")]
    usage: Option<PiAggregateUsage>,
    #[serde(default, deserialize_with = "jsonl::lenient_vec")]
    messages: Vec<PiSubagentMessage>,
}

#[derive(Debug, Default, Deserialize)]
struct PiSubagentMessage {
    #[serde(default, deserialize_with = "jsonl::non_empty_string")]
    role: Option<String>,
    #[serde(default, deserialize_with = "jsonl::non_empty_string")]
    model: Option<String>,
    #[serde(default, deserialize_with = "jsonl::lenient_i64")]
    timestamp: Option<i64>,
    #[serde(default, deserialize_with = "jsonl::lenient_object")]
    usage: Option<PiUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct PiAggregateUsage {
    #[serde(default, deserialize_with = "jsonl::lenient_u64")]
    input: u64,
    #[serde(default, deserialize_with = "jsonl::lenient_u64")]
    output: u64,
    #[serde(rename = "cacheRead", default, deserialize_with = "jsonl::lenient_u64")]
    cache_read: u64,
    #[serde(
        rename = "cacheWrite",
        default,
        deserialize_with = "jsonl::lenient_u64"
    )]
    cache_write: u64,
    #[serde(
        rename = "totalTokens",
        default,
        deserialize_with = "jsonl::lenient_u64"
    )]
    total_tokens: u64,
    #[serde(default, deserialize_with = "jsonl::lenient_f64")]
    cost: Option<f64>,
}

/// Token counts and optional display cost carried by a pi assistant message.
#[derive(Debug, Default, Deserialize)]
struct PiUsage {
    #[serde(default, deserialize_with = "jsonl::lenient_u64")]
    input: u64,
    #[serde(default, deserialize_with = "jsonl::lenient_u64")]
    output: u64,
    #[serde(rename = "cacheRead", default, deserialize_with = "jsonl::lenient_u64")]
    cache_read: u64,
    #[serde(
        rename = "cacheWrite",
        default,
        deserialize_with = "jsonl::lenient_u64"
    )]
    cache_write: u64,
    #[serde(
        rename = "totalTokens",
        default,
        deserialize_with = "jsonl::lenient_u64"
    )]
    total_tokens: u64,
    // A non-object `cost` previously left display cost absent without dropping
    // the record, so deserialize it leniently instead of failing the line.
    #[serde(default, deserialize_with = "jsonl::lenient_object")]
    cost: Option<PiCost>,
}

/// Optional display cost block carried by a pi assistant message.
#[derive(Debug, Default, Deserialize)]
struct PiCost {
    #[serde(default, deserialize_with = "jsonl::lenient_f64")]
    total: Option<f64>,
}

pub(crate) fn read_session_file(
    path: &Path,
    tz: Option<&JiffTimeZone>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
) -> Result<Vec<LoadedEntry>> {
    read_session_file_with_context(path, tz, mode, pricing, PiStoreContext::Default)
}

pub(super) fn read_session_file_for_store(
    path: &Path,
    store_root: &Path,
    tz: Option<&JiffTimeZone>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
    store_name: &str,
) -> Result<Vec<LoadedEntry>> {
    read_session_file_with_context(
        path,
        tz,
        mode,
        pricing,
        PiStoreContext::Named {
            root: store_root,
            name: store_name,
        },
    )
}

#[derive(Clone, Copy)]
enum PiStoreContext<'a> {
    Default,
    Named { root: &'a Path, name: &'a str },
}

impl<'a> PiStoreContext<'a> {
    fn store_name(self) -> &'a str {
        match self {
            Self::Default => "pi",
            Self::Named { name, .. } => name,
        }
    }

    fn project(self, path: &Path) -> String {
        match self {
            Self::Default => extract_project(path),
            Self::Named { root, .. } => extract_project_for_store(path, root),
        }
    }

    fn cost(
        self,
        raw_model: Option<&str>,
        display_model: Option<&str>,
        usage: TokenUsageRaw,
        display_cost: Option<f64>,
        mode: CostMode,
        pricing: Option<&PricingMap>,
    ) -> f64 {
        match self {
            Self::Default => {
                calculate_cost_for_usage(display_model, usage, display_cost, mode, pricing)
            }
            Self::Named { .. } => {
                calculate_store_cost(raw_model, display_model, usage, display_cost, mode, pricing)
            }
        }
    }

    fn missing_pricing_model(
        self,
        raw_model: Option<&str>,
        display_model: Option<&str>,
        usage: TokenUsageRaw,
        display_cost: Option<f64>,
        mode: CostMode,
        pricing: Option<&PricingMap>,
    ) -> Option<String> {
        match self {
            Self::Default => {
                missing_pricing_model_for_usage(display_model, usage, display_cost, mode, pricing)
            }
            Self::Named { .. } => missing_store_pricing_model(
                raw_model,
                display_model,
                usage,
                display_cost,
                mode,
                pricing,
            ),
        }
    }
}

fn read_session_file_with_context(
    path: &Path,
    tz: Option<&JiffTimeZone>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
    context: PiStoreContext<'_>,
) -> Result<Vec<LoadedEntry>> {
    let content = fs::read(path)?;
    let project = context.project(path);
    let session_id = extract_session_id(path);
    // Usable pi lines carry token counts under a `usage` key nested in a
    // `message` object, so require both substrings before JSON parsing.
    let prefilter = LinePrefilter::all(&[br#""usage""#, br#""message""#]);
    let mut entries = Vec::new();

    for record in jsonl::records::<PiLine>(&content, Some(&prefilter)) {
        if record
            .r#type
            .as_deref()
            .is_some_and(|message_type| message_type != "message")
        {
            continue;
        }
        let Some(message) = record.message.as_ref() else {
            continue;
        };
        if message.role.as_deref() == Some("assistant") {
            let Some(record_timestamp_text) = record.timestamp.as_deref() else {
                continue;
            };
            let Some(record_timestamp) = crate::parse_ts_timestamp(record_timestamp_text) else {
                continue;
            };
            if let Some(usage) = message.usage.as_ref() {
                push_entry(
                    &mut entries,
                    &project,
                    &session_id,
                    record_timestamp_text.to_string(),
                    record_timestamp,
                    message.model.as_deref(),
                    usage,
                    usage.cost.as_ref().and_then(|cost| cost.total),
                    None,
                    tz,
                    mode,
                    pricing,
                    context,
                );
            }
            continue;
        }

        if message.role.as_deref() != Some("toolResult")
            || message.tool_name.as_deref() != Some("subagent")
        {
            continue;
        }
        let Some(details) = message.details.as_ref() else {
            continue;
        };
        let call_identity = message
            .tool_call_id
            .as_deref()
            .or(record.timestamp.as_deref())
            .unwrap_or_default();
        for (result_index, result) in details.results.iter().enumerate() {
            let mut usable_nested_messages = 0;
            for (message_index, nested) in result.messages.iter().enumerate() {
                if nested.role.as_deref() != Some("assistant") {
                    continue;
                }
                let (Some(timestamp_millis), Some(usage)) =
                    (nested.timestamp, nested.usage.as_ref())
                else {
                    continue;
                };
                if jiff::Timestamp::from_millisecond(timestamp_millis).is_err() {
                    continue;
                }
                let timestamp = TimestampMs::from_millis(timestamp_millis);
                let request_id = format!("subagent:{call_identity}:{result_index}:{message_index}");
                if push_entry(
                    &mut entries,
                    &project,
                    &session_id,
                    format_rfc3339_millis(timestamp),
                    timestamp,
                    nested
                        .model
                        .as_deref()
                        .or_else(|| result.model.as_deref().map(subagent_result_model)),
                    usage,
                    usage.cost.as_ref().and_then(|cost| cost.total),
                    Some(request_id),
                    tz,
                    mode,
                    pricing,
                    context,
                ) {
                    usable_nested_messages += 1;
                }
            }
            if usable_nested_messages > 0 {
                continue;
            }
            let Some(aggregate) = result.usage.as_ref() else {
                continue;
            };
            let Some(record_timestamp_text) = record.timestamp.as_deref() else {
                continue;
            };
            let Some(record_timestamp) = crate::parse_ts_timestamp(record_timestamp_text) else {
                continue;
            };
            let usage = PiUsage {
                input: aggregate.input,
                output: aggregate.output,
                cache_read: aggregate.cache_read,
                cache_write: aggregate.cache_write,
                total_tokens: aggregate.total_tokens,
                cost: None,
            };
            push_entry(
                &mut entries,
                &project,
                &session_id,
                record_timestamp_text.to_string(),
                record_timestamp,
                result.model.as_deref().map(subagent_result_model),
                &usage,
                aggregate.cost,
                Some(format!("subagent:{call_identity}:{result_index}:aggregate")),
                tz,
                mode,
                pricing,
                context,
            );
        }
    }
    Ok(entries)
}

fn subagent_result_model(model: &str) -> &str {
    model
        .rsplit_once('/')
        .map(|(_, model_name)| model_name)
        .filter(|model_name| !model_name.is_empty())
        .unwrap_or(model)
}

#[allow(clippy::too_many_arguments)]
fn push_entry(
    entries: &mut Vec<LoadedEntry>,
    project: &str,
    session_id: &str,
    timestamp_text: String,
    timestamp: TimestampMs,
    raw_model: Option<&str>,
    usage_value: &PiUsage,
    display_cost: Option<f64>,
    request_id: Option<String>,
    tz: Option<&JiffTimeZone>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
    context: PiStoreContext<'_>,
) -> bool {
    let usage = TokenUsageRaw {
        input_tokens: usage_value.input,
        output_tokens: usage_value.output,
        cache_creation_input_tokens: usage_value.cache_write,
        cache_read_input_tokens: usage_value.cache_read,
        speed: None,
        cache_creation: None,
    };
    let (usage, extra_total_tokens) =
        apply_total_token_fallback(usage, 0, usage_value.total_tokens);
    if crate::total_usage_tokens(usage) + extra_total_tokens == 0 {
        return false;
    }
    let model = raw_model.map(|model| format!("[{}] {model}", context.store_name()));
    let cost = context.cost(
        raw_model,
        model.as_deref(),
        usage,
        display_cost,
        mode,
        pricing,
    );
    let missing_pricing_model = context.missing_pricing_model(
        raw_model,
        model.as_deref(),
        usage,
        display_cost,
        mode,
        pricing,
    );
    let data = UsageEntry {
        session_id: Some(session_id.to_string()),
        timestamp: timestamp_text,
        version: None,
        message: UsageMessage {
            usage,
            model: model.clone(),
            id: None,
        },
        cost_usd: display_cost,
        request_id,
        is_api_error_message: None,
        is_sidechain: None,
    };
    entries.push(LoadedEntry {
        date: format_date_tz(timestamp, tz),
        timestamp,
        project: Arc::from(project),
        session_id: Arc::from(session_id),
        project_path: Arc::from(project),
        cost,
        extra_total_tokens,
        credits: None,
        message_count: None,
        model,
        data,
        usage_limit_reset_time: None,
        missing_pricing_model,
    });
    true
}

fn extract_session_id(path: &Path) -> String {
    let filename = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    filename
        .split_once('_')
        .map_or(filename, |(_, session)| session)
        .to_string()
}

fn extract_project(path: &Path) -> String {
    let mut previous_was_sessions = false;
    for component in path.components() {
        let segment = component.as_os_str().to_string_lossy();
        if previous_was_sessions {
            return segment.into_owned();
        }
        previous_was_sessions = segment == "sessions";
    }
    "unknown".to_string()
}

fn extract_project_for_store(path: &Path, store_root: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(store_root)
        && let Some(project) = relative.components().next()
    {
        return project.as_os_str().to_string_lossy().into_owned();
    }
    extract_project(path)
}

pub(super) fn entry_id(entry: &LoadedEntry) -> String {
    entry_id_for_store("pi", entry)
}

fn calculate_store_cost(
    raw_model: Option<&str>,
    display_model: Option<&str>,
    usage: TokenUsageRaw,
    display_cost: Option<f64>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
) -> f64 {
    match mode {
        CostMode::Display => display_cost.unwrap_or(0.0),
        CostMode::Auto => display_cost.unwrap_or_else(|| {
            calculate_store_cost_from_tokens(raw_model, display_model, usage, pricing)
        }),
        CostMode::Calculate => {
            calculate_store_cost_from_tokens(raw_model, display_model, usage, pricing)
        }
    }
}

fn calculate_store_cost_from_tokens(
    raw_model: Option<&str>,
    display_model: Option<&str>,
    usage: TokenUsageRaw,
    pricing: Option<&PricingMap>,
) -> f64 {
    let Some(pricing) = store_pricing(raw_model, display_model, pricing) else {
        return 0.0;
    };
    calculate_cost_from_pricing(usage, pricing)
}

fn missing_store_pricing_model(
    raw_model: Option<&str>,
    display_model: Option<&str>,
    usage: TokenUsageRaw,
    display_cost: Option<f64>,
    mode: CostMode,
    pricing: Option<&PricingMap>,
) -> Option<String> {
    if mode == CostMode::Display || (mode == CostMode::Auto && display_cost.is_some()) {
        return None;
    }
    if crate::total_usage_tokens(usage) == 0 {
        return None;
    }
    let raw_model = raw_model?;
    store_pricing(Some(raw_model), display_model, pricing)
        .is_none()
        .then(|| crate::model_aliases::resolve_model_name(raw_model).into_owned())
}

fn store_pricing(
    raw_model: Option<&str>,
    display_model: Option<&str>,
    pricing: Option<&PricingMap>,
) -> Option<Pricing> {
    let pricing = pricing?;
    display_model
        .and_then(|model| pricing.find_exact(model))
        .or_else(|| raw_model.and_then(|model| pricing.find(model)))
}

pub(super) fn entry_id_for_store(store_name: &str, entry: &LoadedEntry) -> String {
    [
        store_name,
        entry.project.as_ref(),
        entry.session_id.as_ref(),
        entry.data.timestamp.as_str(),
        entry.model.as_deref().unwrap_or_default(),
        entry.data.request_id.as_deref().unwrap_or_default(),
        &entry.data.message.usage.input_tokens.to_string(),
        &entry.data.message.usage.output_tokens.to_string(),
        &entry
            .data
            .message
            .usage
            .cache_creation_input_tokens
            .to_string(),
        &entry.data.message.usage.cache_read_input_tokens.to_string(),
        &entry.extra_total_tokens.to_string(),
        &entry.cost.to_string(),
    ]
    .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ccusage_test_support::fs_fixture;

    fn fixture_file(
        fixture: &ccusage_test_support::Fixture,
        name: &str,
        content: &str,
    ) -> std::path::PathBuf {
        let path = format!("sessions/project-a/{name}");
        let _ = fixture.write_file(&path, content);
        fixture.path(path)
    }

    #[test]
    fn includes_parallel_nested_subagent_messages_without_aggregate_double_counting() {
        let fixture = fs_fixture!({});
        let file = fixture_file(
            &fixture,
            "agent_session-a.jsonl",
            include_str!("fixtures/subagent-nested.jsonl"),
        );

        let entries = read_session_file(&file, None, CostMode::Display, None).unwrap();

        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.data.message.usage.input_tokens)
                .sum::<u64>(),
            410
        );
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.data.message.usage.output_tokens)
                .sum::<u64>(),
            60
        );
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.data.message.usage.cache_read_input_tokens)
                .sum::<u64>(),
            4_030
        );
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.data.message.usage.cache_creation_input_tokens)
                .sum::<u64>(),
            44
        );
        assert!((entries.iter().map(|entry| entry.cost).sum::<f64>() - 0.8).abs() < 1e-12);
    }

    #[test]
    fn keeps_parallel_subagent_messages_with_identical_usage_distinct() {
        let fixture = fs_fixture!({});
        let file = fixture_file(
            &fixture,
            "agent_session-a.jsonl",
            include_str!("fixtures/subagent-nested.jsonl"),
        );

        let entries = read_session_file(&file, None, CostMode::Display, None).unwrap();
        let identical = entries
            .iter()
            .filter(|entry| entry.data.message.usage.input_tokens == 100)
            .collect::<Vec<_>>();

        assert_eq!(identical.len(), 2);
        assert_ne!(entry_id(identical[0]), entry_id(identical[1]));
    }

    #[test]
    fn uses_nested_subagent_timestamps_for_date_grouping() {
        let fixture = fs_fixture!({});
        let file = fixture_file(
            &fixture,
            "agent_session-a.jsonl",
            include_str!("fixtures/subagent-nested.jsonl"),
        );

        let tz = crate::parse_tz(Some("America/Los_Angeles"));
        let entries = read_session_file(&file, tz.as_ref(), CostMode::Display, None).unwrap();
        let dates = entries
            .iter()
            .map(|entry| entry.date.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            dates,
            ["2026-01-01", "2026-01-02", "2026-01-03", "2026-01-02"]
        );
    }

    #[test]
    fn keeps_nested_subagent_usage_when_the_tool_result_timestamp_is_invalid() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"not-a-timestamp","message":{"role":"toolResult","toolName":"subagent","toolCallId":"call-a","details":{"results":[{"model":"provider/gpt-sub","usage":{"input":999,"output":999,"cost":9.99},"messages":[{"role":"assistant","timestamp":1767398400000,"model":"gpt-sub","usage":{"input":25,"output":5,"cost":{"total":0.2}}}]}]}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        let entries = read_session_file(&file, None, CostMode::Display, None).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].data.message.usage.input_tokens, 25);
        assert_eq!(entries[0].data.message.usage.output_tokens, 5);
        assert_eq!(entries[0].cost, 0.2);
        assert_eq!(entries[0].data.timestamp, "2026-01-03T00:00:00.000Z");
    }

    #[test]
    fn falls_back_to_subagent_aggregate_when_nested_messages_are_unusable() {
        let fixture = fs_fixture!({});
        let file = fixture_file(
            &fixture,
            "agent_session-a.jsonl",
            include_str!("fixtures/subagent-fallback.jsonl"),
        );

        let entries = read_session_file(&file, None, CostMode::Display, None).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].data.message.usage.input_tokens, 50);
        assert_eq!(entries[0].data.message.usage.output_tokens, 5);
        assert_eq!(entries[0].data.message.usage.cache_read_input_tokens, 500);
        assert_eq!(entries[0].data.message.usage.cache_creation_input_tokens, 2);
        assert_eq!(entries[0].cost, 0.4);
        assert_eq!(entries[0].model.as_deref(), Some("[pi] gpt-fallback"));
        assert_eq!(entries[0].data.timestamp, "2026-01-05T12:00:00.000Z");
    }

    #[test]
    fn applies_cost_modes_to_each_subagent_request() {
        let fixture = fs_fixture!({});
        let file = fixture_file(
            &fixture,
            "agent_session-a.jsonl",
            include_str!("fixtures/subagent-nested.jsonl"),
        );
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "[pi] gpt-parent": {
                    "input_cost_per_token": 0.001,
                    "output_cost_per_token": 0.002,
                    "cache_read_input_token_cost": 0.0001,
                    "cache_creation_input_token_cost": 0.0003
                },
                "[pi] gpt-sub": {
                    "input_cost_per_token": 0.001,
                    "output_cost_per_token": 0.002,
                    "cache_read_input_token_cost": 0.0001,
                    "cache_creation_input_token_cost": 0.0003
                }
            }"#,
        );

        let auto = read_session_file(&file, None, CostMode::Auto, Some(&pricing)).unwrap();
        let display = read_session_file(&file, None, CostMode::Display, Some(&pricing)).unwrap();
        let calculate =
            read_session_file(&file, None, CostMode::Calculate, Some(&pricing)).unwrap();

        assert!((auto.iter().map(|entry| entry.cost).sum::<f64>() - 0.8).abs() < 1e-12);
        assert!((display.iter().map(|entry| entry.cost).sum::<f64>() - 0.8).abs() < 1e-12);
        assert!((calculate.iter().map(|entry| entry.cost).sum::<f64>() - 0.9462).abs() < 1e-12);
    }

    #[test]
    fn prefixes_subagent_models_for_named_stores() {
        let fixture = fs_fixture!({});
        let file = fixture_file(
            &fixture,
            "agent_session-a.jsonl",
            include_str!("fixtures/subagent-nested.jsonl"),
        );

        let entries = read_session_file_for_store(
            &file,
            &fixture.path("sessions"),
            None,
            CostMode::Display,
            None,
            "omp",
        )
        .unwrap();

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[1].model.as_deref(), Some("[omp] gpt-sub"));
        assert_eq!(entries[2].model.as_deref(), Some("[omp] gpt-sub"));
        assert_eq!(entries[3].model.as_deref(), Some("[omp] gpt-sub"));
    }

    #[test]
    fn falls_back_to_total_tokens_when_pi_parts_are_missing() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"gpt-5","usage":{"totalTokens":333}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        let entries = read_session_file(&file, None, CostMode::Display, None).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].data.message.usage.output_tokens, 333);
        assert_eq!(entries[0].extra_total_tokens, 0);
    }

    #[test]
    fn sets_missing_pricing_model_when_model_not_in_pricing() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"unknown-model-xyz","usage":{"input":100,"output":200}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        // Use Calculate mode with an empty PricingMap so model won't be found
        let pricing = PricingMap::default();
        let entries = read_session_file(&file, None, CostMode::Calculate, Some(&pricing)).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].missing_pricing_model.as_deref(),
            Some("[pi] unknown-model-xyz")
        );
    }

    #[test]
    fn named_store_name_does_not_price_unknown_models() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"totally-unknown-model","usage":{"input":1000000,"output":1000000}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "o3": {
                    "input_cost_per_token": 0.000002,
                    "output_cost_per_token": 0.000008
                }
            }"#,
        );

        let entries = read_session_file_for_store(
            &file,
            &fixture.path("sessions"),
            None,
            CostMode::Calculate,
            Some(&pricing),
            "o3",
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cost, 0.0);
        assert_eq!(
            entries[0].missing_pricing_model.as_deref(),
            Some("totally-unknown-model")
        );
    }

    #[test]
    fn named_store_name_does_not_outmatch_real_model_pricing() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"o3","usage":{"input":1000,"output":2000}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "o3": {
                    "input_cost_per_token": 0.000002,
                    "output_cost_per_token": 0.000008
                },
                "deepseek-chat": {
                    "input_cost_per_token": 0.001,
                    "output_cost_per_token": 0.001
                }
            }"#,
        );

        let entries = read_session_file_for_store(
            &file,
            &fixture.path("sessions"),
            None,
            CostMode::Calculate,
            Some(&pricing),
            "deepseek-chat",
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cost, 0.018000000000000002);
        assert_eq!(entries[0].missing_pricing_model, None);
    }

    #[test]
    fn named_store_prefixed_pricing_override_wins_before_unprefixed_lookup() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"gpt-5.4","usage":{"input":1000,"output":2000}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");
        let mut pricing = PricingMap::default();
        pricing.load_json(
            r#"{
                "gpt-5.4": {
                    "input_cost_per_token": 0.001,
                    "output_cost_per_token": 0.001
                },
                "[omp] gpt-5.4": {
                    "input_cost_per_token": 0.000002,
                    "output_cost_per_token": 0.000008
                }
            }"#,
        );

        let entries = read_session_file_for_store(
            &file,
            &fixture.path("sessions"),
            None,
            CostMode::Calculate,
            Some(&pricing),
            "omp",
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cost, 0.018000000000000002);
        assert_eq!(entries[0].missing_pricing_model, None);
    }

    #[test]
    fn no_missing_pricing_model_in_display_mode() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"unknown-model-xyz","usage":{"input":100,"output":200}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        let pricing = PricingMap::default();
        let entries = read_session_file(&file, None, CostMode::Display, Some(&pricing)).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].missing_pricing_model, None);
    }

    #[test]
    fn no_missing_pricing_model_when_auto_mode_has_display_cost() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"unknown-model-xyz","usage":{"input":100,"output":200,"cost":{"total":0.05}}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        let pricing = PricingMap::default();
        let entries = read_session_file(&file, None, CostMode::Auto, Some(&pricing)).unwrap();

        assert_eq!(entries.len(), 1);
        // In Auto mode with a display cost present, no missing pricing warning
        assert_eq!(entries[0].missing_pricing_model, None);
    }

    #[test]
    fn keeps_record_when_cost_is_not_an_object() {
        // A non-object `cost` must not fail the whole line; the usage tokens
        // should still be counted with display cost treated as missing.
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"gpt-5","usage":{"input":100,"output":200,"cost":0}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        let entries = read_session_file(&file, None, CostMode::Display, None).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].data.message.usage.input_tokens, 100);
        assert_eq!(entries[0].data.message.usage.output_tokens, 200);
    }

    #[test]
    fn prefixes_named_store_models_with_store_name() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"gpt-5","usage":{"input":100,"output":200}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        let entries = read_session_file_for_store(
            &file,
            &fixture.path("sessions"),
            None,
            CostMode::Display,
            None,
            "omp",
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model.as_deref(), Some("[omp] gpt-5"));
        assert_eq!(
            entries[0].data.message.model.as_deref(),
            Some("[omp] gpt-5")
        );
    }

    #[test]
    fn includes_named_store_in_dedupe_identity() {
        let fixture = fs_fixture!({
            "sessions/project-a/agent_session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-02T00:00:00.000Z","message":{"role":"assistant","model":"gpt-5","usage":{"input":100,"output":200}}}"#,
        });
        let file = fixture.path("sessions/project-a/agent_session-a.jsonl");

        let pi = read_session_file(&file, None, CostMode::Display, None)
            .unwrap()
            .pop()
            .unwrap();
        let omp = read_session_file_for_store(
            &file,
            &fixture.path("sessions"),
            None,
            CostMode::Display,
            None,
            "omp",
        )
        .unwrap()
        .pop()
        .unwrap();

        assert_ne!(entry_id(&pi), entry_id_for_store("omp", &omp));
    }
}
