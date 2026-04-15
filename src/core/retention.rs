use crate::core::config::RetentionPolicy;

/// Determines which snapshots should be deleted based on the retention policy.
///
/// Returns the names of snapshots that fall outside the policy and should be removed.
/// `snapshots` must be sorted oldest-first (the format `YYYY-MM-DD-HHMMSS` sorts
/// lexicographically, so a plain alphabetical sort is sufficient).
///
/// # TODO
/// Implement the retention logic. The policy fields to honour:
/// - `keep_hourly`  / `keep_within_hourly`
/// - `keep_daily`   / `keep_within_daily`
/// - `keep_weekly`  / `keep_within_weekly`
/// - `keep_monthly` / `keep_within_monthly`
/// - `keep_yearly`  / `keep_within_yearly`
///
/// Algorithm sketch:
/// 1. Parse each snapshot name into a `DateTime`.
/// 2. Walk the list newest-first, keeping one snapshot per bucket
///    (hour / day / week / month / year) up to the configured count.
/// 3. Any snapshot not selected by any rule is added to the delete list.
pub fn apply_policy(policy: &RetentionPolicy, snapshots: &[String]) -> Vec<String> {
    // TODO: implement retention logic (see doc comment above).
    let _ = policy;
    let _ = snapshots;
    vec![]
}
