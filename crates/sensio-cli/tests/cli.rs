//! `peil judge` end to end on the core's synthetic drive.

use std::path::PathBuf;
use std::process::Command;

fn drive_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sensio-core/tests/fixtures/drive.jsonl")
}

#[test]
fn judge_prints_one_verdict_per_flagged_device() {
    let output = Command::new(env!("CARGO_BIN_EXE_peil"))
        .arg("judge")
        .arg(drive_fixture())
        .output()
        .expect("peil runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    let devices: Vec<String> = stdout
        .lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).expect("each line is JSON");
            v["device"].as_str().expect("a device").to_owned()
        })
        .collect();
    assert_eq!(devices, ["ble:aa:aa:aa:00:00:01", "tpms:0a1b2c3d"]);
}

#[test]
fn judge_reports_a_missing_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_peil"))
        .args(["judge", "no-such-file.jsonl"])
        .output()
        .expect("peil runs");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("opening no-such-file.jsonl"));
}
