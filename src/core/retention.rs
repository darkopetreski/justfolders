use chrono::{Datelike, NaiveDateTime, Timelike};
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
/// Only the newest snapshot in each bucket is claimed; older snapshots in the
/// same bucket stay in the pool and may be picked up by a coarser rule.
fn apply_rule<K, F>(
    pool: Vec<(String, NaiveDateTime)>,
    max_buckets: u32,
    bucket_of: F,
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
        if (seen.len() as u32) < max_buckets && !seen.contains(&bucket) {
            seen.insert(bucket);
            kept.push((name, category.clone()));
        } else {
            remaining.push((name, dt));
        }
    }

    remaining
}

// ── Main algorithm ────────────────────────────────────────────────────────

/// Applies the retention policy using cascading non-overlapping rules.
///
/// Rules are applied finest-first (hourly → daily → weekly → monthly → yearly).
/// Each rule claims from the pool of snapshots not yet claimed by a finer rule,
/// keeping the newest snapshot per calendar bucket up to the configured count.
///
/// Returns `(to_delete, categorized_keeps)`.
fn run_policy(
    policy: &RetentionPolicy,
    snapshots: &[String],
) -> (Vec<String>, Vec<(String, SnapshotCategory)>) {
    // Parse snapshot names; silently skip any that cannot be parsed.
    let mut pool: Vec<(String, NaiveDateTime)> = snapshots
        .iter()
        .filter_map(|s| parse_snapshot_date(s).map(|dt| (s.clone(), dt)))
        .collect();

    // Newest-first: each rule's representative is the newest in its bucket.
    pool.sort_by(|a, b| b.1.cmp(&a.1));

    let mut kept: Vec<(String, SnapshotCategory)> = Vec::new();

    // A value of 0 is treated as "disabled" (equivalent to None), matching the
    // config file comment "0 = disabled".
    if let Some(n) = policy.keep_hourly.filter(|&n| n > 0) {
        pool = apply_rule(
            pool,
            n,
            |dt| (dt.year(), dt.month(), dt.day(), dt.hour()),
            SnapshotCategory::Hourly,
            &mut kept,
        );
    }

    if let Some(n) = policy.keep_daily.filter(|&n| n > 0) {
        pool = apply_rule(
            pool,
            n,
            |dt| (dt.year(), dt.month(), dt.day()),
            SnapshotCategory::Daily,
            &mut kept,
        );
    }

    if let Some(n) = policy.keep_weekly.filter(|&n| n > 0) {
        pool = apply_rule(
            pool,
            n,
            |dt| {
                let w = dt.iso_week();
                (w.year(), w.week())
            },
            SnapshotCategory::Weekly,
            &mut kept,
        );
    }

    if let Some(n) = policy.keep_monthly.filter(|&n| n > 0) {
        pool = apply_rule(
            pool,
            n,
            |dt| (dt.year(), dt.month()),
            SnapshotCategory::Monthly,
            &mut kept,
        );
    }

    if let Some(n) = policy.keep_yearly.filter(|&n| n > 0) {
        pool = apply_rule(
            pool,
            n,
            |dt| dt.year(),
            SnapshotCategory::Yearly,
            &mut kept,
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
