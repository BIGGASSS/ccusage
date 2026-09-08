use std::process::Command;

use ccusage_test_support::{Fixture, fs_fixture};
use serde_json::{Value, json};

#[test]
fn pi_weekly_json_aggregates_at_sunday_boundary() {
    let fixture = pi_fixture();
    let stdout = run_cli(&fixture, ["--json"]);
    let report: Value = serde_json::from_str(&stdout).expect("CLI should return JSON");
    let weeks = report["weekly"].as_array().expect("weekly rows");

    assert_eq!(weeks.len(), 2);
    assert_eq!(weeks[0]["week"], "2025-12-28");
    assert_eq!(weeks[0]["inputTokens"], 100);
    assert_eq!(weeks[0]["totalTokens"], 133);
    assert_eq!(weeks[1]["week"], "2026-01-04");
    assert_eq!(weeks[1]["inputTokens"], 500);
    assert_eq!(weeks[1]["outputTokens"], 50);
    assert_eq!(weeks[1]["cacheReadTokens"], 100);
    assert_eq!(weeks[1]["cacheCreationTokens"], 15);
    assert_eq!(weeks[1]["totalTokens"], 665);
    assert_eq!(weeks[1]["totalCost"], 1.25);
    assert_eq!(
        report["totals"],
        json!({
            "inputTokens": 600,
            "outputTokens": 60,
            "cacheReadTokens": 120,
            "cacheCreationTokens": 18,
            "totalTokens": 798,
            "totalCost": 1.5,
        })
    );
    insta::assert_snapshot!("focused_weekly_json", stdout);
}

#[test]
fn pi_weekly_json_sorts_and_filters_before_aggregation() {
    let fixture = pi_fixture();
    let descending: Value =
        serde_json::from_str(&run_cli(&fixture, ["--json", "--order", "desc"])).unwrap();
    assert_eq!(descending["weekly"][0]["week"], "2026-01-04");
    assert_eq!(descending["weekly"][1]["week"], "2025-12-28");

    let filtered: Value = serde_json::from_str(&run_cli(
        &fixture,
        ["--json", "--since", "20260104", "--until", "20260104"],
    ))
    .unwrap();
    assert_eq!(filtered["weekly"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["weekly"][0]["week"], "2026-01-04");
    assert_eq!(filtered["weekly"][0]["inputTokens"], 200);
    assert_eq!(filtered["totals"]["totalTokens"], 266);
    assert_eq!(filtered["totals"]["totalCost"], 0.5);
}

#[test]
fn snapshots_pi_focused_weekly_table_stdout() {
    let fixture = pi_fixture();
    insta::assert_snapshot!("focused_weekly_table", run_cli(&fixture, []));
}

fn pi_fixture() -> Fixture {
    // Saturday immediately before Sunday midnight, followed by Monday in the
    // same Sunday-based week. Separate files also exercise cross-session sums.
    fs_fixture!({
        "pi/sessions/project-a/session-a.jsonl": r#"{"type":"message","timestamp":"2026-01-03T23:59:59.999Z","message":{"role":"assistant","model":"gpt-5","usage":{"input":100,"output":10,"cacheRead":20,"cacheWrite":3,"cost":{"total":0.25}}}}
    {"type":"message","timestamp":"2026-01-04T00:00:00.000Z","message":{"role":"assistant","model":"gpt-5","usage":{"input":200,"output":20,"cacheRead":40,"cacheWrite":6,"cost":{"total":0.5}}}}"#,
        "pi/sessions/project-b/session-b.jsonl": r#"{"type":"message","timestamp":"2026-01-05T12:00:00.000Z","message":{"role":"assistant","model":"gpt-5-mini","usage":{"input":300,"output":30,"cacheRead":60,"cacheWrite":9,"cost":{"total":0.75}}}}"#,
    })
}

fn run_cli<const N: usize>(fixture: &Fixture, args: [&str; N]) -> String {
    run_report(fixture, &["pi", "weekly"], &args)
}

#[test]
fn session_last_limits_rows_not_days_and_totals_follow_selection() {
    let fixture = pi_fixture();
    for command in [&["pi", "session"][..], &["session"][..]] {
        let report: Value =
            serde_json::from_str(&run_report(&fixture, command, &["--json", "--last", "1"]))
                .unwrap();
        let rows = report
            .get("sessions")
            .or_else(|| report.get("session"))
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["inputTokens"], 300);
        assert_eq!(report["totals"]["totalTokens"], 399);
        assert_eq!(report["totals"]["totalCost"], 0.75);

        let filtered: Value = serde_json::from_str(&run_report(
            &fixture,
            command,
            &["--json", "--last", "1", "--until", "20260104"],
        ))
        .unwrap();
        assert_eq!(filtered["totals"]["totalTokens"], 399);
        assert_eq!(filtered["totals"]["inputTokens"], 300);
        let oversized: Value =
            serde_json::from_str(&run_report(&fixture, command, &["--json", "--last", "5"]))
                .unwrap();
        assert_eq!(oversized["totals"]["totalTokens"], 798);
    }
}

fn run_report(fixture: &Fixture, command: &[&str], args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ccusage"))
        .env_clear()
        .env("HOME", fixture.path("empty-home"))
        .env("USERPROFILE", fixture.path("empty-userprofile"))
        .env("XDG_CONFIG_HOME", fixture.path("empty-xdg-config"))
        .env("PI_AGENT_DIR", fixture.path("pi"))
        .env("LOG_LEVEL", "0")
        .env("NO_COLOR", "1")
        .env("COLUMNS", "120")
        .args(command)
        .args(args)
        .args([
            "--offline",
            "--no-color",
            "--timezone",
            "UTC",
            "--mode",
            "display",
        ])
        .output()
        .expect("ccusage CLI should run");
    assert!(
        output.status.success(),
        "ccusage report failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("ccusage CLI stdout should be UTF-8")
}
