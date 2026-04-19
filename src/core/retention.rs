use chrono::{Datelike, NaiveDateTime};
use std::collections::HashSet;

use crate::core::config::RetentionPolicy;

// ── Category ───────────────────────────────────────────────────────────────

/// The retention category assigned to a kept snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotCategory {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl SnapshotCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hourly => "Hourly",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Yearly => "Yearly",
        }
    }
}

// ── Parsing ────────────────────────────────────────────────────────────────

fn parse_snapshot_date(name: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(name, "%Y-%m-%d-%H%M%S").ok()
}

// ── Core bucketing step ───────────────────────────────────────────────────

/// Walk `pool` (newest-first), claim up to `max_buckets` distinct buckets,
/// push the claimed entries into `kept`, and return what remains unclaimed.
///
/// Buckets present in `skip` are already covered by a finer-grained rule;
/// snapshots that fall into those buckets are left in the pool so that a
/// coarser rule can evaluate them (or they are eventually pruned).
///
/// Among non-skipped buckets, only the newest snapshot per bucket is claimed.
fn apply_rule<K, F>(
    pool: Vec<(String, NaiveDateTime)>,
    max_buckets: u32,
    bucket_of: F,
    skip: &HashSet<K>,
    category: SnapshotCategory,
    kept: &mut Vec<(String, SnapshotCategory)>,
) -> Vec<(String, NaiveDateTime)>
where
    K: Eq + std::hash::Hash,
    F: Fn(&NaiveDateTime) -> K,
{
    let mut seen: HashSet<K> = HashSet::new();
    let mut remaining = Vec::new();

    for (name, dt) in pool {
        let bucket = bucket_of(&dt);
        if skip.contains(&bucket) {
            // Already represented by a finer rule — pass to coarser rules.
            remaining.push((name, dt));
        } else if (seen.len() as u32) < max_buckets && !seen.contains(&bucket) {
            seen.insert(bucket);
            kept.push((name, category.clone()));
        } else {
            remaining.push((name, dt));
        }
    }

    remaining
}

/// Collect the set of K-buckets occupied by every snapshot currently in `kept`.
///
/// Used to tell the next (coarser) rule which of its own buckets are already
/// represented, so it can skip them rather than double-counting.
fn covered<K, F>(kept: &[(String, SnapshotCategory)], bucket_of: F) -> HashSet<K>
where
    K: Eq + std::hash::Hash,
    F: Fn(&NaiveDateTime) -> K,
{
    kept.iter()
        .filter_map(|(name, _)| parse_snapshot_date(name))
        .map(|dt| bucket_of(&dt))
        .collect()
}

// ── Main algorithm ────────────────────────────────────────────────────────

/// Applies the retention policy using cascading non-overlapping rules.
///
/// Rules are applied finest-first (hourly → daily → weekly → monthly → yearly).
/// Before each rule runs, the buckets already represented by all finer rules
/// are computed and passed as a skip-set, so a coarser rule never claims a
/// bucket that a finer rule already covers.
///
/// Returns `(to_delete, categorized_keeps)`.
fn run_policy(
    policy: &RetentionPolicy,
    snapshots: &[String],
) -> (Vec<String>, Vec<(String, SnapshotCategory)>) {
    let mut pool: Vec<(String, NaiveDateTime)> = snapshots
        .iter()
        .filter_map(|s| parse_snapshot_date(s).map(|dt| (s.clone(), dt)))
        .collect();

    // Newest-first so each rule's representative is the newest in its bucket.
    pool.sort_by(|a, b| b.1.cmp(&a.1));

    let mut kept: Vec<(String, SnapshotCategory)> = Vec::new();

    // A value of 0 is treated as "disabled" (matches the config file comment
    // "0 = disabled").
    if let Some(n) = policy.keep_hourly.filter(|&n| n > 0) {
        // "Hourly" means the N most recent snapshots from the same calendar
        // day as the newest snapshot. No per-clock-hour deduplication is
        // applied: two backups taken within the same hour both count.
        // Snapshots from any earlier day bypass this rule entirely and fall
        // through to the daily/weekly/... rules.
        let today = pool.first().map(|(_, dt)| (dt.year(), dt.month(), dt.day()));
        if let Some(today) = today {
            let (todays, older): (Vec<_>, Vec<_>) = pool
                .into_iter()
                .partition(|(_, dt)| (dt.year(), dt.month(), dt.day()) == today);

            // `todays` is newest-first; claim the first N, pass the rest on.
            let mut leftover = Vec::new();
            let mut claimed = 0u32;
            for (name, dt) in todays {
                if claimed < n {
                    kept.push((name, SnapshotCategory::Hourly));
                    claimed += 1;
                } else {
                    leftover.push((name, dt));
                }
            }
            leftover.extend(older);
            leftover.sort_by(|a, b| b.1.cmp(&a.1));
            pool = leftover;
        }
    }

    if let Some(n) = policy.keep_daily.filter(|&n| n > 0) {
        let skip: HashSet<_> = covered(&kept, |dt| (dt.year(), dt.month(), dt.day()));
        pool = apply_rule(
            pool, n,
            |dt| (dt.year(), dt.month(), dt.day()),
            &skip, SnapshotCategory::Daily, &mut kept,
        );
    }

    if let Some(n) = policy.keep_weekly.filter(|&n| n > 0) {
        let skip: HashSet<_> = covered(&kept, |dt| {
            let w = dt.iso_week();
            (w.year(), w.week())
        });
        pool = apply_rule(
            pool, n,
            |dt| { let w = dt.iso_week(); (w.year(), w.week()) },
            &skip, SnapshotCategory::Weekly, &mut kept,
        );
    }

    if let Some(n) = policy.keep_monthly.filter(|&n| n > 0) {
        let skip: HashSet<_> = covered(&kept, |dt| (dt.year(), dt.month()));
        pool = apply_rule(
            pool, n,
            |dt| (dt.year(), dt.month()),
            &skip, SnapshotCategory::Monthly, &mut kept,
        );
    }

    if let Some(n) = policy.keep_yearly.filter(|&n| n > 0) {
        let skip: HashSet<_> = covered(&kept, |dt| dt.year());
        pool = apply_rule(
            pool, n,
            |dt| dt.year(),
            &skip, SnapshotCategory::Yearly, &mut kept,
        );
    }

    let to_delete = pool.into_iter().map(|(s, _)| s).collect();
    (to_delete, kept)
}

// ── Public API ────────────────────────────────────────────────────────────

/// Returns the names of snapshots that should be deleted according to the policy.
///
/// `snapshots` must be sorted oldest-first (the `YYYY-MM-DD-HHMMSS` format sorts
/// lexicographically, so a plain alphabetical sort is sufficient).
/// Returns an empty list when no policy rules are configured.
pub fn apply_policy(policy: &RetentionPolicy, snapshots: &[String]) -> Vec<String> {
    if policy.is_empty() {
        return vec![];
    }
    let (to_delete, _) = run_policy(policy, snapshots);
    to_delete
}

/// Returns each snapshot paired with its retention category, in display order
/// (Hourly first, then Daily, Weekly, Monthly, Yearly; newest-first within
/// each category).
///
/// Snapshots not claimed by any rule (i.e. those `apply_policy` would delete)
/// are not included in the result.
pub fn classify_snapshots(
    policy: &RetentionPolicy,
    snapshots: &[String],
) -> Vec<(String, SnapshotCategory)> {
    let (_, kept) = run_policy(policy, snapshots);
    kept
}

/// Print `snapshots` grouped by retention category.
///
/// When no policy rules are active the list is printed flat, oldest-first.
/// When a policy is active, snapshots are grouped under Hourly / Daily /
/// Weekly / Monthly / Yearly headings, newest-first within each group.
/// Any snapshot not claimed by any rule is shown under "Other" at the bottom.
pub fn print_snapshot_groups(policy: &RetentionPolicy, snapshots: &[String]) {
    if policy.is_empty() {
        snapshots.iter().for_each(|s| println!("{s}"));
        return;
    }

    let categorized = classify_snapshots(policy, snapshots);

    let categorized_names: HashSet<&str> =
        categorized.iter().map(|(n, _)| n.as_str()).collect();

    let other: Vec<&str> = snapshots
        .iter()
        .rev()
        .filter(|s| !categorized_names.contains(s.as_str()))
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
            .filter(|(_, cat)| cat == category)
            .map(|(n, _)| n.as_str())
            .collect();
        if entries.is_empty() {
            continue;
        }
        if !first { println!(); }
        println!("{}", category.label());
        println!("{sep}");
        for s in entries { println!("{s}"); }
        first = false;
    }

    if !other.is_empty() {
        if !first { println!(); }
        println!("Other");
        println!("{sep}");
        for s in other { println!("{s}"); }
    }
}
