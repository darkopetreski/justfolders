//! Integration tests for the `checksum` config flag.
//!
//! Covers the three pieces wired up in step 6:
//!   1. Config parser — `[default] checksum` is read, defaults to `true`
//!      when absent, and rejects garbage.
//!   2. Init generator — `--no-checksum` flips the emitted line and the
//!      resulting file parses back to the same boolean (round-trip).
//!   3. Backup step gating — `do_backup` skips the checksum work when
//!      `checksum = false`, leaving the snapshot without a `sha256sum`
//!      file (so `verify` can report it honestly).

use justfolders::commands::backup::run as backup_run;
use justfolders::commands::init::run as init_run;
use justfolders::commands::verify::{run as verify_run, VerifyError};
use justfolders::core::config::{
    Config, ConfigGeneratorParams, RetentionPolicy, generate_config_content,
};
use justfolders::core::parser::{BackupArgs, InitArgs, VerifyArgs, DEFAULT_RSYNC_FLAGS};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ── Parser tests ────────────────────────────────────────────────────────────

/// Writes `body` to a fresh `.ini` file inside `tmp` and returns its path.
/// Saves a few lines of boilerplate in each test.
fn write_config(tmp: &TempDir, body: &str) -> std::path::PathBuf {
    let path = tmp.path().join("backup.ini");
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn checksum_defaults_to_true_when_absent() {
    let tmp = TempDir::new().unwrap();
    let cfg_path = write_config(
        &tmp,
        "[default]\nsrc = /tmp/src\ndest = /tmp/dest\n",
    );

    let cfg = Config::load(&cfg_path).unwrap();
    assert!(
        cfg.checksum,
        "missing checksum key must default to true (backwards-compat for old configs)"
    );
}

#[test]
fn checksum_explicit_false_is_respected() {
    let tmp = TempDir::new().unwrap();
    let cfg_path = write_config(
        &tmp,
        "[default]\nsrc = /tmp/src\ndest = /tmp/dest\nchecksum = false\n",
    );

    let cfg = Config::load(&cfg_path).unwrap();
    assert!(!cfg.checksum);
}

#[test]
fn checksum_explicit_true_is_respected() {
    let tmp = TempDir::new().unwrap();
    let cfg_path = write_config(
        &tmp,
        "[default]\nsrc = /tmp/src\ndest = /tmp/dest\nchecksum = true\n",
    );

    let cfg = Config::load(&cfg_path).unwrap();
    assert!(cfg.checksum);
}

#[test]
fn checksum_rejects_non_boolean_values() {
    let tmp = TempDir::new().unwrap();
    let cfg_path = write_config(
        &tmp,
        "[default]\nsrc = /tmp/src\ndest = /tmp/dest\nchecksum = sometimes\n",
    );

    let err = Config::load(&cfg_path).expect_err("non-boolean must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("checksum"),
        "error should mention the offending field; got: {msg}"
    );
}

// ── Generator tests ─────────────────────────────────────────────────────────

fn base_params() -> ConfigGeneratorParams {
    ConfigGeneratorParams {
        src: "/tmp/src".parse().unwrap(),
        dest: "/tmp/dest".parse().unwrap(),
        port: None,
        id_rsa: None,
        disable_logs: false,
        log_dir: None,
        exclude_from: None,
        rsync_flags: DEFAULT_RSYNC_FLAGS.to_string(),
        bwlimit: None,
        policy: RetentionPolicy::default(),
        checksum: true,
    }
}

/// Returns every uncommented `key = value` setting line that begins with `key`.
/// The generator includes prose that *mentions* `checksum = false` inside the
/// comment block explaining the option, so the assertions below have to
/// distinguish actual settings from documentation.
fn active_settings_for<'a>(content: &'a str, key: &'a str) -> Vec<&'a str> {
    let prefix = format!("{key} = ");
    content
        .lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#') && line.starts_with(&prefix))
        .collect()
}

#[test]
fn generator_emits_checksum_true_by_default() {
    let content = generate_config_content(&base_params());
    let settings = active_settings_for(&content, "checksum");
    assert_eq!(
        settings,
        vec!["checksum = true"],
        "generated config should activate checksum exactly once by default"
    );
}

#[test]
fn generator_emits_checksum_false_when_disabled() {
    let mut params = base_params();
    params.checksum = false;
    let content = generate_config_content(&params);
    let settings = active_settings_for(&content, "checksum");
    assert_eq!(settings, vec!["checksum = false"]);
}

#[test]
fn generator_output_round_trips_through_the_parser() {
    // Both directions: render → parse must yield the original value.
    for value in [true, false] {
        let tmp = TempDir::new().unwrap();
        let mut params = base_params();
        params.checksum = value;
        let content = generate_config_content(&params);
        let path = tmp.path().join("backup.ini");
        fs::write(&path, &content).unwrap();

        let parsed = Config::load(&path).unwrap();
        assert_eq!(
            parsed.checksum, value,
            "round-trip failed for checksum = {value}\n--- generated ---\n{content}"
        );
    }
}

// ── Init command tests ──────────────────────────────────────────────────────

fn init_args(config_path: &Path, no_checksum: bool) -> InitArgs {
    InitArgs {
        config: config_path.to_path_buf(),
        src: "/tmp/src".parse().unwrap(),
        dest: "/tmp/dest".parse().unwrap(),
        ssh_port: None,
        id_rsa: None,
        rsync_flags: DEFAULT_RSYNC_FLAGS.to_string(),
        disable_logs: false,
        log_dir: None,
        exclude_from: None,
        bwlimit: None,
        keep_hourly: None,
        keep_daily: None,
        keep_weekly: None,
        keep_monthly: None,
        keep_yearly: None,
        keep_within_days: None,
        no_checksum,
        dry_run: false,
    }
}

#[test]
fn init_without_no_checksum_flag_writes_checksum_true() {
    let tmp = TempDir::new().unwrap();
    let cfg_path = tmp.path().join("backup.ini");
    init_run(init_args(&cfg_path, false)).unwrap();

    let written = fs::read_to_string(&cfg_path).unwrap();
    assert!(written.contains("checksum = true"));

    // And the parser agrees, of course.
    assert!(Config::load(&cfg_path).unwrap().checksum);
}

#[test]
fn init_with_no_checksum_flag_writes_checksum_false() {
    let tmp = TempDir::new().unwrap();
    let cfg_path = tmp.path().join("backup.ini");
    init_run(init_args(&cfg_path, true)).unwrap();

    let written = fs::read_to_string(&cfg_path).unwrap();
    assert!(written.contains("checksum = false"));

    assert!(!Config::load(&cfg_path).unwrap().checksum);
}

// ── End-to-end backup gating ────────────────────────────────────────────────

fn have_binary(name: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Writes a self-contained local-to-local config and a tiny source tree.
/// Returns (config_path, src_dir TempDir, dest_dir TempDir) — TempDirs are
/// returned so the caller keeps them alive for the duration of the test.
fn local_to_local_config(
    tmp: &TempDir,
    checksum_enabled: bool,
) -> (std::path::PathBuf, TempDir, TempDir) {
    let src_dir = TempDir::new().unwrap();
    let dest_dir = TempDir::new().unwrap();
    fs::write(src_dir.path().join("payload.txt"), b"abc").unwrap();

    // `rsync_flags` matters here: with no flags rsync skips directories and
    // never produces the `.in_progress` tree that `do_backup` expects to
    // promote to a snapshot.  Using the same defaults that `init` writes
    // keeps the test honest end-to-end.
    let cfg = format!(
        "[default]\nsrc = {}\ndest = {}\ndisable_logs = true\nrsync_flags = {}\nchecksum = {}\n",
        src_dir.path().display(),
        dest_dir.path().display(),
        DEFAULT_RSYNC_FLAGS,
        checksum_enabled,
    );
    let cfg_path = tmp.path().join("backup.ini");
    fs::write(&cfg_path, cfg).unwrap();
    (cfg_path, src_dir, dest_dir)
}

#[test]
fn backup_with_checksum_true_writes_sha256sum_file() {
    if !have_binary("rsync") || !have_binary("sha256sum") || !have_binary("find") {
        eprintln!("skipping: rsync/sha256sum/find not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let (cfg_path, _src, dest) = local_to_local_config(&tmp, true);
    backup_run(BackupArgs { config: cfg_path }).unwrap();

    // Exactly one snapshot directory should exist.
    let snap = find_only_snapshot(dest.path());
    assert!(
        snap.join("sha256sum").exists(),
        "checksum = true must write a per-snapshot sha256sum file"
    );
    assert!(
        dest.path().join("checksums.db").exists(),
        "checksum = true must also create the global cache"
    );
}

#[test]
fn backup_with_checksum_false_does_not_write_sha256sum() {
    if !have_binary("rsync") {
        eprintln!("skipping: rsync not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let (cfg_path, _src, dest) = local_to_local_config(&tmp, false);
    backup_run(BackupArgs { config: cfg_path }).unwrap();

    let snap = find_only_snapshot(dest.path());
    assert!(
        !snap.join("sha256sum").exists(),
        "checksum = false must skip the per-snapshot sha256sum file"
    );
    assert!(
        !dest.path().join("checksums.db").exists(),
        "checksum = false must also skip the global cache"
    );
}

#[test]
fn verify_after_checksum_false_returns_no_checksum_file_error() {
    if !have_binary("rsync") {
        eprintln!("skipping: rsync not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let (cfg_path, _src, _dest) = local_to_local_config(&tmp, false);
    backup_run(BackupArgs {
        config: cfg_path.clone(),
    })
    .unwrap();

    // verify must refuse to lie: there's no sha256sum, so it reports it.
    let err = verify_run(VerifyArgs {
        config: cfg_path,
        snapshot: "latest".to_string(),
    })
    .expect_err("verify must error when the snapshot has no sha256sum");

    assert!(matches!(err, VerifyError::NoChecksumFile(_)));
}

/// Returns the single snapshot directory inside `dest`.  Panics if more
/// than one snapshot is present — these helpers run on a freshly created
/// destination so there should be exactly one.
fn find_only_snapshot(dest: &Path) -> std::path::PathBuf {
    let snaps: Vec<_> = fs::read_dir(dest)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| {
                    // YYYY-MM-DD-HHMMSS — same shape the snapshot directories use.
                    name.len() == 17 && name.chars().filter(|&c| c == '-').count() == 3
                })
        })
        .collect();
    assert_eq!(snaps.len(), 1, "expected exactly one snapshot, got {snaps:?}");
    snaps.into_iter().next().unwrap()
}
