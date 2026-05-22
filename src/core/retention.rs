//! Snapshot retention policy engine.
//!
//! # Overview
//!
//! This module decides which snapshots to keep and which to delete after each
//! backup run.  The policy is expressed as a set of rules in the `[policy]`
//! section of the config file:
//!
//! ```ini
//! keep_within_days = 7   # keep ALL snapshots from the last 7 calendar days
//! keep_hourly      = 4   # keep the 4 most-recent snapshots from today
//! keep_daily       = 7   # keep one representative per day for 7 days
//! keep_weekly      = 4   # keep one representative per ISO week for 4 weeks
//! keep_monthly     = 12  # keep one representative per month for 12 months
//! keep_yearly      = 5   # keep one representative per year for 5 years
//! ```
//!
//! Rules that are absent or set to `0` are disabled.  Any combination is valid;
//! you do not need to set all of them.
//!
//! # Algorithm: cascading non-overlapping rules
//!
//! Count-based rules (hourly, daily, weekly, monthly, yearly) are applied in
//! order from finest to coarsest granularity.  A shared "pool" of unclaimed
//! snapshots is threaded through each rule: every rule claims its snapshots
//! from the pool, and the remainder is passed to the next rule.
//!
//! Before each rule runs, the set of time-buckets already occupied by ALL
//! previously-kept snapshots is computed (the "skip set").  The current rule
//! skips any snapshot whose bucket is already in the skip set and only claims
//! snapshots from buckets that have not yet been covered.
//!
//! Example — why the skip set matters:
//!
//! ```text
//! Snapshots: Mon-09:00, Mon-14:00, Mon-20:00, Wed-10:00
//! Policy:    keep_daily=2, keep_weekly=2
//!
//! WITHOUT skip set (naïve union):
//!   Daily claims  Mon-20:00 (Mon), Wed-10:00 (Wed)           → 2 days ✓
//!   Weekly claims Mon-20:00 (W1),  Wed-10:00 (W1)            → both same week!
//!   Result: weekly appears to give W1 twice, never shows W2.
//!
//! WITH skip set (cascading):
//!   Daily  claims Mon-20:00 (Mon), Wed-10:00 (Wed)
//!   skip_weeks = {W_Mon, W_Wed} = {W1, W1} = {W1}
//!   Weekly skips W1, claims the oldest snapshot from W2 (previous week)
//!   Result: 2 distinct daily + 1 truly older weekly ✓
//! ```
//!
//! This is the exact bug present in restic's implementation (see
//! https://forum.restic.net/t/retention-policy/4953 and
//! https://github.com/restic/restic/issues/3624).  The skip-set approach fixes it.
//!
//! # Rule: keep_within_days (post-process, not pre-pass)
//!
//! `keep_within_days = N` guarantees that **every** snapshot from the last N
//! calendar days is kept, with no count limit.
//!
//! ## Why it runs as a post-process, not a pre-pass
//!
//! An earlier implementation ran `keep_within_days` as a pre-pass — claiming
//! all within-window snapshots before the count-based rules.  This caused a
//! subtle regression:
//!
//! ```text
//! Snapshots:       Apr-20 (×9), Apr-19, Apr-17, Apr-10
//! Policy:          within=2d, daily=3, weekly=3
//! keep_within_days window: Apr-19, Apr-20
//!
//! PRE-PASS (broken):
//!   WithinDays claims Apr-20 (×9) and Apr-19 → pool = [Apr-17, Apr-10]
//!   Daily claims Apr-17 and Apr-10 (only 2 left in pool)
//!   covered_weeks = {W17(Apr-20), W16(Apr-19), W16(Apr-17), W15(Apr-10)}
//!   Weekly skips W17, W16, W15 → needs W14, which doesn't exist → Weekly empty ✗
//!
//! POST-PROCESS (correct):
//!   Daily claims Apr-20-newest (W17), Apr-19 (W16), Apr-17 (W16)  [pool untouched]
//!   covered_weeks = {W17, W16}
//!   Weekly claims Apr-10 (W15) ✓
//!   Then WithinDays post-process:
//!     → overrides Apr-20-newest and Apr-19 from "Daily" to "WithinDays"
//!     → rescues the 8 other Apr-20 snapshots (unclaimed) as "WithinDays"
//!   Result: Recent shows newest snapshot at top ✓  Weekly still shows Apr-10 ✓
//! ```
//!
//! The post-process does two things:
//! 1. **Override** – any snapshot already kept by a count-based rule whose date
//!    falls within the window has its category changed to `WithinDays`.  The
//!    snapshot remains kept; only the display label changes to "Recent".
//! 2. **Rescue** – any snapshot still in the "to-delete" pool whose date falls
//!    within the window is moved into `kept` as `WithinDays`.  It is never deleted.
//!
//! # Rule: keep_hourly (semantics)
//!
//! `keep_hourly = N` keeps the **N most recent snapshots from today's calendar
//! date**.  It does NOT mean "one snapshot per distinct clock-hour".
//!
//! ## Why not per-clock-hour
//!
//! An earlier version bucketed by `(year, month, day, hour)` and kept one
//! snapshot per distinct hour.  If two backups ran at 23:19 and 23:20 they
//! shared the same `hour=23` bucket and counted as one — so `keep_hourly=2`
//! only kept 1 snapshot even after 2 runs.  Switching to a simple "take the N
//! newest from today" solves this naturally.
//!
//! ## Why "today" is anchored to Local::now(), not pool.first()
//!
//! An earlier version used `pool.first()` (the newest snapshot in the pool) as
//! the definition of "today".  If the `snapshots` command is run a day after
//! the last backup, `pool.first()` is yesterday's snapshot and the hourly rule
//! would try to label that as "today" — incorrectly.  Using `Local::now()` ties
//! "today" to the actual wall-clock date when the policy is evaluated.  As a
//! side effect, when `keep_within_days` runs as a post-process and overrides
//! today's snapshots to "Recent", the hourly rule finds an empty "today" bucket
//! in the pool and claims nothing — which is correct, since those snapshots are
//! already covered.

use chrono::{Datelike, Duration, Local, NaiveDateTime};
use std::collections::HashSet;

use crate::core::config::RetentionPolicy;

// ── Category ───────────────────────────────────────────────────────────────

/// The retention category assigned to a kept snapshot.
///
/// Categories are ordered from finest to coarsest granularity for display.
/// `WithinDays` is shown first because it represents the most recently dated
/// snapshots (everything within the `keep_within_days` window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotCategory {
    /// Kept by the `keep_within_days` rule.  This category is assigned by a
    /// post-process step that runs *after* all count-based rules (see module
    /// doc).  It either overrides the category of a snapshot already claimed by
    /// a count-based rule, or rescues an unclaimed snapshot that would otherwise
    /// be deleted.
    WithinDays,
    /// Kept by `keep_hourly`: the N most recent snapshots from today.
    Hourly,
    /// Kept by `keep_daily`: one representative per calendar day.
    Daily,
    /// Kept by `keep_weekly`: one representative per ISO week.
    Weekly,
    /// Kept by `keep_monthly`: one representative per calendar month.
    Monthly,
    /// Kept by `keep_yearly`: one representative per calendar year.
    Yearly,
}

impl SnapshotCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::WithinDays => "Recent",
            Self::Hourly     => "Hourly",
            Self::Daily      => "Daily",
            Self::Weekly     => "Weekly",
            Self::Monthly    => "Monthly",
            Self::Yearly     => "Yearly",
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
/// ## Parameters
///
/// - `pool`       – snapshots not yet claimed, sorted newest-first.
/// - `max_buckets`– how many distinct buckets this rule may claim.
/// - `bucket_of`  – maps a timestamp to the rule's bucket key (e.g. ISO week).
/// - `skip`       – buckets already covered by finer-grained rules; snapshots
///   whose bucket is in `skip` are passed through to the pool returned to
///   the caller so a coarser rule can evaluate them.
/// - `category`   – the `SnapshotCategory` label to attach to claimed entries.
/// - `kept`       – accumulator: claimed entries are appended here.
///
/// ## Behaviour
///
/// For each snapshot in `pool` (newest-first):
///
/// * If the snapshot's bucket is in `skip` → leave it in the pool unchanged.
///   This is critical: a snapshot representing a week already covered by a
///   daily rule must remain in the pool so the weekly rule can see it, confirm
///   it's already covered, and look further back for an uncovered week.
///
/// * If the bucket is new and the quota is not full → claim it.
///
/// * Otherwise → discard (leave in remaining; will be deleted unless a coarser
///   rule or the `keep_within_days` post-process rescues it).
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
/// Called before each count-based rule runs.  The result becomes the `skip` set
/// passed to `apply_rule`, ensuring that a coarser rule never claims a bucket
/// that a finer rule (or the `keep_within_days` post-process override) has
/// already covered.
///
/// Because `kept` includes ALL previously kept snapshots regardless of their
/// category (WithinDays, Hourly, Daily, …), this also prevents, for example,
/// Weekly from re-selecting a week that is already fully represented by a Daily
/// snapshot that was subsequently re-labelled as "Recent".
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

/// Core retention engine.  See the module-level documentation for the full
/// explanation of the algorithm and its design decisions.
///
/// Execution order:
///
/// 1. Sort all snapshots newest-first into `pool`.
/// 2. **Hourly** – take the N most recent snapshots from today (`Local::now()`).
/// 3. **Daily** – one representative per calendar day, skipping days already
///    covered by hourly.
/// 4. **Weekly** – one representative per ISO week, skipping weeks already
///    covered by hourly/daily.
/// 5. **Monthly** – one representative per calendar month, skipping months
///    already covered by hourly/daily/weekly.
/// 6. **Yearly** – one representative per year, skipping years already covered
///    by all finer rules.
/// 7. **WithinDays post-process** – override the category of any
///    within-window snapshot already in `kept`, and rescue any unclaimed
///    within-window snapshot from the deletion pool.
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

    // Newest-first: ensures each rule claims the most recent representative
    // within each bucket, which is the intuitive expectation.
    pool.sort_by(|a, b| b.1.cmp(&a.1));

    let mut kept: Vec<(String, SnapshotCategory)> = Vec::new();

    // ── Hourly ──────────────────────────────────────────────────────────────
    //
    // Keeps the N most recent snapshots from today.  "Today" is the real
    // current date from Local::now(), NOT the date of the newest snapshot in
    // the pool.
    //
    // PITFALL: do not switch back to using pool.first() for "today".  If the
    // policy is evaluated a day after the last backup (e.g. via the `snapshots`
    // command), pool.first() would return yesterday's snapshot and the rule
    // would incorrectly label it as "today's hourly".
    //
    // PITFALL: do not bucket by clock-hour ((year, month, day, hour)).  Two
    // backups at 23:19 and 23:20 share hour=23 and would count as one, causing
    // `keep_hourly=2` to keep only 1 snapshot even after 2 runs.  The "take
    // the N newest from today" approach counts each run independently.
    if let Some(n) = policy.keep_hourly.filter(|&n| n > 0) {
        let today = Local::now().date_naive();
        let today_tuple = (today.year(), today.month(), today.day());
        let (todays, older): (Vec<_>, Vec<_>) = pool
            .into_iter()
            .partition(|(_, dt)| (dt.year(), dt.month(), dt.day()) == today_tuple);

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

    // ── Daily ───────────────────────────────────────────────────────────────
    //
    // skip_days = all (year, month, day) tuples already in `kept`.
    // This prevents Daily from re-claiming a day already covered by Hourly.
    if let Some(n) = policy.keep_daily.filter(|&n| n > 0) {
        let skip: HashSet<_> = covered(&kept, |dt| (dt.year(), dt.month(), dt.day()));
        pool = apply_rule(
            pool, n,
            |dt| (dt.year(), dt.month(), dt.day()),
            &skip, SnapshotCategory::Daily, &mut kept,
        );
    }

    // ── Weekly ──────────────────────────────────────────────────────────────
    //
    // skip_weeks = ISO weeks of all snapshots in `kept` (Hourly + Daily).
    //
    // IMPORTANT: the skip set is computed from ALL of `kept`, not just Daily.
    // This means if Hourly has a snapshot from this week, Weekly will skip
    // this week and look further back — correct behaviour.
    //
    // IMPORTANT: snapshots that are in a skipped week are NOT consumed by
    // apply_rule; they are passed back in the remaining pool so that Monthly
    // and Yearly can evaluate them.  This is the `skip.contains(&bucket)` →
    // `remaining.push(...)` branch inside apply_rule.
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

    // ── Monthly ─────────────────────────────────────────────────────────────
    if let Some(n) = policy.keep_monthly.filter(|&n| n > 0) {
        let skip: HashSet<_> = covered(&kept, |dt| (dt.year(), dt.month()));
        pool = apply_rule(
            pool, n,
            |dt| (dt.year(), dt.month()),
            &skip, SnapshotCategory::Monthly, &mut kept,
        );
    }

    // ── Yearly ──────────────────────────────────────────────────────────────
    if let Some(n) = policy.keep_yearly.filter(|&n| n > 0) {
        let skip: HashSet<_> = covered(&kept, |dt| dt.year());
        pool = apply_rule(
            pool, n,
            |dt| dt.year(),
            &skip, SnapshotCategory::Yearly, &mut kept,
        );
    }

    // ── WithinDays post-process ─────────────────────────────────────────────
    //
    // MUST run after all count-based rules so that those rules operate on the
    // full snapshot pool and cascade correctly.
    //
    // PITFALL: do NOT run this as a pre-pass (before count-based rules).  A
    // pre-pass reduces the pool available to Daily/Weekly/etc.  With limited
    // snapshot history this causes the weekly rule to find no uncovered weeks:
    //
    //   Pre-pass removes Apr-19 from pool.  Daily claims Apr-17 and Apr-10.
    //   covered_weeks = {W16(Apr-17), W15(Apr-10)}.  Weekly needs W14 → nothing.
    //
    //   Post-process leaves pool intact.  Daily claims Apr-20, Apr-19, Apr-17.
    //   covered_weeks = {W17, W16}.  Weekly claims Apr-10 (W15) ✓.
    //   Then post-process overrides Apr-20 and Apr-19 to "Recent".
    //
    // Step 1: override — re-label any snapshot already in `kept` whose date
    // falls in the window.  It stays kept; only the display category changes.
    //
    // Step 2: rescue — move any snapshot still in `pool` (would be deleted)
    // whose date falls in the window into `kept` as WithinDays.
    if let Some(n) = policy.keep_within_days.filter(|&n| n > 0) {
        let today = Local::now().date_naive();
        let cutoff = today - Duration::days(n as i64);

        for (name, cat) in &mut kept {
            if let Some(dt) = parse_snapshot_date(name)
                && dt.date() >= cutoff
            {
                *cat = SnapshotCategory::WithinDays;
            }
        }

        pool.retain(|(name, dt)| {
            if dt.date() >= cutoff {
                kept.push((name.clone(), SnapshotCategory::WithinDays));
                false // remove from deletion pool
            } else {
                true
            }
        });
    }

    let to_delete = pool.into_iter().map(|(s, _)| s).collect();
    (to_delete, kept)
}

// ── Public API ────────────────────────────────────────────────────────────

/// Returns the names of snapshots that should be deleted according to the policy.
///
/// Snapshot names must follow the `YYYY-MM-DD-HHMMSS` format used by
/// justfolders; entries that cannot be parsed are silently ignored and will not
/// appear in the delete list.
///
/// Returns an empty list when no policy rules are configured.
pub fn apply_policy(policy: &RetentionPolicy, snapshots: &[String]) -> Vec<String> {
    if policy.is_empty() {
        return vec![];
    }
    let (to_delete, _) = run_policy(policy, snapshots);
    to_delete
}

/// Returns each kept snapshot paired with its retention category.
///
/// Snapshots that `apply_policy` would delete are not included.
/// The order within each category is not guaranteed here; callers that need
/// newest-first ordering (such as `print_snapshot_groups`) must sort themselves.
pub fn classify_snapshots(
    policy: &RetentionPolicy,
    snapshots: &[String],
) -> Vec<(String, SnapshotCategory)> {
    let (_, kept) = run_policy(policy, snapshots);
    kept
}

/// Builds a compact one-line summary of the active policy rules.
///
/// Example: `Policy: within=7d  hourly=2  daily=3  weekly=2  monthly=12  yearly=2`
///
/// Fields that are `None` or `0` (disabled) are omitted so the line stays
/// short regardless of how many rules are configured.
fn format_policy_summary(policy: &RetentionPolicy) -> String {
    let mut parts: Vec<String> = Vec::new();

    let fields: &[(&str, Option<u32>, &str)] = &[
        ("within",  policy.keep_within_days, "d"),
        ("hourly",  policy.keep_hourly,      ""),
        ("daily",   policy.keep_daily,       ""),
        ("weekly",  policy.keep_weekly,      ""),
        ("monthly", policy.keep_monthly,     ""),
        ("yearly",  policy.keep_yearly,      ""),
    ];

    for (name, value, suffix) in fields {
        if let Some(n) = value.filter(|&n| n > 0) {
            parts.push(format!("{}={}{}", name, n, suffix));
        }
    }

    format!("Policy: {}", parts.join("  "))
}

/// Print `snapshots` grouped by retention category.
///
/// When no policy rules are active the list is printed flat (oldest-first).
/// When a policy is active:
///   - The active policy is printed as a one-line header.
///   - Snapshots are grouped under Recent / Hourly / Daily / Weekly /
///     Monthly / Yearly headings, newest-first within each group.
///   - Snapshots not claimed by any rule (i.e. those that `apply_policy` would
///     delete) are shown under "Other" at the bottom.
///
/// Entries are sorted newest-first within each section explicitly, because
/// `classify_snapshots` does not guarantee insertion order (the WithinDays
/// post-process appends rescued snapshots after count-based entries, which
/// would otherwise produce a non-monotonic ordering inside the Recent group).
pub fn print_snapshot_groups(policy: &RetentionPolicy, snapshots: &[String]) {
    if policy.is_empty() {
        snapshots.iter().for_each(|s| println!("{s}"));
        return;
    }

    println!("{}", format_policy_summary(policy));
    println!();

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
        SnapshotCategory::WithinDays,
        SnapshotCategory::Hourly,
        SnapshotCategory::Daily,
        SnapshotCategory::Weekly,
        SnapshotCategory::Monthly,
        SnapshotCategory::Yearly,
    ];

    let mut first = true;
    for category in &order {
        let mut entries: Vec<&str> = categorized
            .iter()
            .filter(|(_, cat)| cat == category)
            .map(|(n, _)| n.as_str())
            .collect();
        if entries.is_empty() {
            continue;
        }
        // Sort newest-first.  Snapshot names are YYYY-MM-DD-HHMMSS, which
        // sorts correctly as plain strings (reverse lexicographic = newest-first).
        entries.sort_by(|a, b| b.cmp(a));
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
