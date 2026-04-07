use std::process::Command;

#[test]
fn rsync_get_flags_prints_and_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_justfolders"))
        .arg("--rsync-get-flags")
        .output()
        .expect("binary should start");

    assert!(output.status.success(), "expected zero exit status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--numeric-ids"));
    assert!(stdout.contains("--delete"));
}
