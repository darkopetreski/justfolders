/// Retention policy simulator.
///
/// Edit the constants below, then run:
///
///   cargo run --example simulate
///
/// Delete this file when you are done experimenting.

use chrono::{Duration, NaiveDate};
use justfolders::core::config::RetentionPolicy;
use justfolders::core::retention::{apply_policy, classify_snapshots, SnapshotCategory};
use std::collections::HashSet;
use std::fs;
use tempfile::TempDir;

// ── Configuration — edit these ────────────────────────────────────────────

const START: &str = "2024-04-15";
const END:   &str = "2026-04-19";

const KEEP_HOURLY:  Option<u32> = None;
const KEEP_DAILY:   Option<u32> = Some(1);
const KEEP_WEEKLY:  Option<u32> = Some(4);
const KEEP_MONTHLY: Option<u32> = Some(12);
const KEEP_YEARLY:  Option<u32> = Some(2);

// ─────────────────────────────────────────────────────────────────────────

fn main() {
    let start = NaiveDate::parse_from_str(START, "%Y-%m-%d")
        .unwrap_or_else(|_| panic!("invalid START date: {START}"));
    let end = NaiveDate::parse_from_str(END, "%Y-%m-%d")
        .unwrap_or_else(|_| panic!("invalid END date: {END}"));

    assert!(start <= end, "--start must be before or equal to --end");

    let policy = RetentionPolicy {
        keep_hourly:  KEEP_HOURLY,
        keep_daily:   KEEP_DAILY,
        keep_weekly:  KEEP_WEEKLY,
        keep_monthly: KEEP_MONTHLY,
        keep_yearly:  KEEP_YEARLY,
        ..Default::default()
    };

    assert!(!policy.is_empty(), "configure at least one KEEP_* constant");

    let dest = TempDir::new().expect("create temp dir");

    // ── Header ────────────────────────────────────────────────────────────
    let days = (end - start).num_days() + 1;
    println!();
    println!(
        "Simulating {} day{} from {} to {}",
        days,
        if days == 1 { "" } else { "s" },
        start,
        end
    );
    print!("Policy: keep");
    if let Some(n) = KEEP_HOURLY  { print!("  hourly={n}");  }
    if let Some(n) = KEEP_DAILY   { print!("  daily={n}");   }
    if let Some(n) = KEEP_WEEKLY  { print!("  weekly={n}");  }
    if let Some(n) = KEEP_MONTHLY { print!("  monthly={n}"); }
    if let Some(n) = KEEP_YEARLY  { print!("  yearly={n}");  }
    println!();
    println!();
    println!("{}", "─".repeat(62));

    // ── Day-by-day loop ───────────────────────────────────────────────────
    let mut day = start;
    while day <= end {
        let name = day.format("%Y-%m-%d-000000").to_string();
        fs::create_dir(dest.path().join(&name)).expect("create snapshot dir");

        let snapshots = list_snapshots(&dest);
        let to_delete = apply_policy(&policy, &snapshots);
        let kept = snapshots.len() - to_delete.len();

        for s in &to_delete {
            fs::remove_dir_all(dest.path().join(s)).expect("delete snapshot");
        }

        if to_delete.is_empty() {
            println!("  {}  +1  →  {} kept", day, kept);
        } else {
            println!(
                "  {}  +1  pruned {}  →  {} kept",
                day,
                to_delete.len(),
                kept
            );
            for s in &to_delete {
                println!("              ✗  {}", s);
            }
        }

        day += Duration::days(1);
    }

    println!("{}", "─".repeat(62));

    // ── Final grouped view ────────────────────────────────────────────────
    let final_snapshots = list_snapshots(&dest);
    println!();
    print_grouped(&policy, &final_snapshots);
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn list_snapshots(dest: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dest.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            n.len() == 17
                && n.chars().nth(4)  == Some('-')
                && n.chars().nth(7)  == Some('-')
                && n.chars().nth(10) == Some('-')
        })
        .collect();
    names.sort();
    names
}

fn print_grouped(policy: &RetentionPolicy, snapshots: &[String]) {
    let categorized = classify_snapshots(policy, snapshots);
    let cat_names: HashSet<&str> = categorized.iter().map(|(n, _)| n.as_str()).collect();

    let other: Vec<&str> = snapshots
        .iter()
        .rev()
        .filter(|s| !cat_names.contains(s.as_str()))
        .map(String::as_str)
        .collect();

    let sep = "─".repeat(22);
    let order = [
        SnapshotCategory::Hourly,
        SnapshotCategory::Daily,
        SnapshotCategory::Weekly,
        SnapshotCategory::Monthly,
        SnapshotCategory::Yearly,
    ];

    let mut first = true;
    for category in &order {
        let entries: Vec<&str> = categorized
            .iter()
            .filter(|(_, c)| c == category)
            .map(|(n, _)| n.as_str())
            .collect();
        if entries.is_empty() {
            continue;
        }
        if !first { println!(); }
        println!("{}", category.label());
        println!("{}", sep);
        for s in entries {
            println!("  {}", s);
        }
        first = false;
    }

    if !other.is_empty() {
        if !first { println!(); }
        println!("Other (outside policy)");
        println!("{}", sep);
        for s in other {
            println!("  {}", s);
        }
    }

    println!();
    println!(
        "Total: {} snapshot{}",
        snapshots.len(),
        if snapshots.len() == 1 { "" } else { "s" }
    );
    println!();
}
