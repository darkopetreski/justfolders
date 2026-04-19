/// Retention policy simulation tests.
///
/// Each test defines a start date, an end date, and a policy.
/// The simulation creates one snapshot directory per day (or per specified
/// date list), applies the retention policy after each backup exactly as the
/// real backup command does, then prints the final grouped snapshot list.
///
/// Run with `cargo test --test retention -- --nocapture` to see the output.
use chrono::{Duration, NaiveDate};
use justfolders::core::config::RetentionPolicy;
use justfolders::core::retention::{apply_policy, classify_snapshots, SnapshotCategory};
use std::fs;
use tempfile::TempDir;

// ── Helpers ────────────────────────────────────────────────────────────────

/// List all snapshot directories inside `dest`, sorted oldest-first.
fn list_snapshots(dest: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dest.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| {
            name.len() == 17
                && name.chars().nth(4) == Some('-')
                && name.chars().nth(7) == Some('-')
                && name.chars().nth(10) == Some('-')
        })
        .collect();
    names.sort();
    names
}

/// Create a snapshot directory for `date` and immediately apply the policy,
/// deleting whatever the policy says to remove.
fn run_backup(dest: &TempDir, date: NaiveDate, policy: &RetentionPolicy) {
    let name = date.format("%Y-%m-%d-000000").to_string();
    fs::create_dir(dest.path().join(&name)).expect("create snapshot dir");

    let snapshots = list_snapshots(dest);
    for s in apply_policy(policy, &snapshots) {
        fs::remove_dir_all(dest.path().join(&s)).expect("delete snapshot");
    }
}

/// Run a backup for every day from `start` through `end` (inclusive).
fn simulate_daily(
    dest: &TempDir,
    start: NaiveDate,
    end: NaiveDate,
    policy: &RetentionPolicy,
) {
    let mut day = start;
    while day <= end {
        run_backup(dest, day, policy);
        day += Duration::days(1);
    }
}

/// Run a backup only on the explicitly listed dates (already sorted oldest-first).
#[allow(dead_code)]
fn simulate_dates(dest: &TempDir, dates: &[NaiveDate], policy: &RetentionPolicy) {
    for &day in dates {
        run_backup(dest, day, policy);
    }
}

// ── Display ────────────────────────────────────────────────────────────────

fn print_grouped(label: &str, policy: &RetentionPolicy, snapshots: &[String]) {
    let categorized = classify_snapshots(policy, snapshots);

    let categorized_names: std::collections::HashSet<&str> =
        categorized.iter().map(|(n, _)| n.as_str()).collect();

    let other: Vec<&str> = snapshots
        .iter()
        .rev()
        .filter(|s| !categorized_names.contains(s.as_str()))
        .map(String::as_str)
        .collect();

    let sep = "─".repeat(22);

    println!("┌─ {} ", label);
    println!();

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
            .filter(|(_, cat)| cat == category)
            .map(|(n, _)| n.as_str())
            .collect();
        if !entries.is_empty() {
            if !first {
                println!();
            }
            println!("{}", category.label());
            println!("{}", sep);
            for s in entries {
                println!("{}", s);
            }
            first = false;
        }
    }

    if !other.is_empty() {
        if !first {
            println!();
        }
        println!("Other (outside policy)");
        println!("{}", sep);
        for s in other {
            println!("{}", s);
        }
    }

    println!();
}

// ── Tests ──────────────────────────────────────────────────────────────────

/// Simulate the exact irregular backup schedule from the design discussion:
///   Apr15, Apr16, Apr20, Apr21, Apr28, May5, May6, May7
/// Policy: keep_daily=2, keep_weekly=2
///
/// After each backup the current state is printed so you can follow the
/// evolution step by step.
#[test]
fn irregular_schedule_from_discussion() {
    let dest = TempDir::new().unwrap();
    let policy = RetentionPolicy {
        keep_daily: Some(2),
        keep_weekly: Some(2),
        ..Default::default()
    };

    let dates: Vec<NaiveDate> = [
        "2026-04-15",
        "2026-04-16",
        "2026-04-20",
        "2026-04-21",
        "2026-04-28",
        "2026-05-05",
        "2026-05-06",
        "2026-05-07",
    ]
    .iter()
    .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap())
    .collect();

    println!();
    println!("=== Irregular schedule: keep_daily=2, keep_weekly=2 ===");
    println!();

    for &day in &dates {
        run_backup(&dest, day, &policy);
        let snapshots = list_snapshots(&dest);
        println!(
            "  after {:10}  →  {:?}",
            day.format("%Y-%m-%d"),
            snapshots
        );
    }

    let final_snapshots = list_snapshots(&dest);
    println!();
    print_grouped("Final state", &policy, &final_snapshots);

    // The final state must have exactly 2 daily + 2 weekly snapshots
    // (our cascading algorithm guarantees non-overlapping categories).
    let categorized = classify_snapshots(&policy, &final_snapshots);
    let daily = categorized
        .iter()
        .filter(|(_, c)| *c == SnapshotCategory::Daily)
        .count();
    let weekly = categorized
        .iter()
        .filter(|(_, c)| *c == SnapshotCategory::Weekly)
        .count();
    assert_eq!(daily, 2, "expected 2 daily snapshots");
    assert_eq!(weekly, 2, "expected 2 weekly snapshots");
    assert_eq!(final_snapshots.len(), 4, "expected 4 snapshots total (no overlap)");
}

/// Simulate a continuous daily backup for ~7 weeks.
/// Policy: keep_daily=7, keep_weekly=4, keep_monthly=3
///
/// Useful for seeing how the three tiers interact over a longer horizon.
#[test]
fn daily_for_7_weeks_three_tier_policy() {
    let dest = TempDir::new().unwrap();
    let policy = RetentionPolicy {
        keep_daily: Some(7),
        keep_weekly: Some(4),
        keep_monthly: Some(3),
        ..Default::default()
    };

    let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 4, 19).unwrap();

    simulate_daily(&dest, start, end, &policy);

    let final_snapshots = list_snapshots(&dest);
    println!();
    print_grouped(
        &format!(
            "Daily {start} → {end}  |  keep_daily=7, keep_weekly=4, keep_monthly=3"
        ),
        &policy,
        &final_snapshots,
    );

    let categorized = classify_snapshots(&policy, &final_snapshots);
    let daily = categorized
        .iter()
        .filter(|(_, c)| *c == SnapshotCategory::Daily)
        .count();
    let weekly = categorized
        .iter()
        .filter(|(_, c)| *c == SnapshotCategory::Weekly)
        .count();
    let monthly = categorized
        .iter()
        .filter(|(_, c)| *c == SnapshotCategory::Monthly)
        .count();

    assert_eq!(daily, 7, "expected 7 daily snapshots");
    assert_eq!(weekly, 4, "expected 4 weekly snapshots");
    assert!(monthly >= 1, "expected at least 1 monthly snapshot");
    assert_eq!(
        final_snapshots.len(),
        daily + weekly + monthly,
        "no overlap between tiers"
    );
}

/// Simulate a full year of daily backups.
/// Policy: keep_daily=7, keep_weekly=4, keep_monthly=6, keep_yearly=2
#[test]
fn full_year_four_tier_policy() {
    let dest = TempDir::new().unwrap();
    let policy = RetentionPolicy {
        keep_daily: Some(7),
        keep_weekly: Some(4),
        keep_monthly: Some(6),
        keep_yearly: Some(2),
        ..Default::default()
    };

    let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 4, 19).unwrap();

    simulate_daily(&dest, start, end, &policy);

    let final_snapshots = list_snapshots(&dest);
    println!();
    print_grouped(
        "Daily 2025-01-01 → 2026-04-19  |  keep_daily=7, keep_weekly=4, keep_monthly=6, keep_yearly=2",
        &policy,
        &final_snapshots,
    );

    let categorized = classify_snapshots(&policy, &final_snapshots);
    let daily = categorized.iter().filter(|(_, c)| *c == SnapshotCategory::Daily).count();
    let weekly = categorized.iter().filter(|(_, c)| *c == SnapshotCategory::Weekly).count();
    let monthly = categorized.iter().filter(|(_, c)| *c == SnapshotCategory::Monthly).count();
    let yearly = categorized.iter().filter(|(_, c)| *c == SnapshotCategory::Yearly).count();

    assert_eq!(daily, 7);
    assert_eq!(weekly, 4);
    assert_eq!(monthly, 6);
    assert!(yearly >= 1);
    assert_eq!(
        final_snapshots.len(),
        daily + weekly + monthly + yearly,
        "no overlap between tiers"
    );
}
