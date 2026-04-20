use crate::core::config::Config;
use crate::core::parser::SnapshotsArgs;
use crate::core::retention::{classify_snapshots, SnapshotCategory};
use crate::core::storage_backend::from_config;
use std::collections::HashSet;
use thiserror::Error;

/// Errors that can occur when listing snapshots.
#[derive(Debug, Error)]
pub enum SnapshotsError {
    #[error(transparent)]
    Config(#[from] crate::core::config::ConfigError),

    #[error(transparent)]
    Backend(#[from] crate::core::storage_backend::StorageBackendError),
}

/// Lists all available snapshots at the backup destination and prints them to stdout.
///
/// When a retention policy is configured the output is grouped by category
/// (Hourly / Daily / Weekly / Monthly / Yearly), with the newest snapshot
/// at the top of each group.  Any snapshot not claimed by any rule (e.g.
/// because the policy was tightened since the last backup) is shown in an
/// "Other" section at the bottom.
///
/// When no policy is configured the list is printed flat, oldest-first.
pub fn run(args: SnapshotsArgs) -> Result<(), SnapshotsError> {
    let config = Config::load(&args.config)?;
    let backend = from_config(&config);

    let snapshots = backend.list_snapshots()?;
    if snapshots.is_empty() {
        println!("no snapshots found");
        return Ok(());
    }

    if config.policy.is_empty() {
        snapshots.iter().for_each(|s| println!("{}", s));
        return Ok(());
    }

    print_grouped(&config.policy, &snapshots);
    Ok(())
}
/// Prints `snapshots` grouped by retention category to stdout.
///
/// Display order: Recent → Hourly → Daily → Weekly → Monthly → Yearly.
/// Snapshots not claimed by any rule are shown under "Other" at the bottom.
/// Entries within each section are sorted newest-first.
///
/// NOTE: the order array must stay in sync with [`crate::core::retention::print_snapshot_groups`].
fn print_grouped(policy: &crate::core::config::RetentionPolicy, snapshots: &[String]) {
    let categorized = classify_snapshots(policy, snapshots);

    let categorized_names: HashSet<&str> =
        categorized.iter().map(|(n, _)| n.as_str()).collect();

    // Snapshots not claimed by any rule (policy may have changed since last prune).
    // `snapshots` is oldest-first, so reverse for newest-first display.
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

    let mut first_section = true;

    for category in &order {
        let mut entries: Vec<&str> = categorized
            .iter()
            .filter(|(_, cat)| cat == category)
            .map(|(name, _)| name.as_str())
            .collect();

        if !entries.is_empty() {
            entries.sort_by(|a, b| b.cmp(a)); // newest-first
            if !first_section {
                println!();
            }
            println!("{}", category.label());
            println!("{}", sep);
            for s in entries {
                println!("{}", s);
            }
            first_section = false;
        }
    }

    if !other.is_empty() {
        if !first_section {
            println!();
        }
        println!("Other");
        println!("{}", sep);
        for s in other {
            println!("{}", s);
        }
    }
}

