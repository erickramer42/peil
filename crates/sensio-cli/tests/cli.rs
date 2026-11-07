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

#[test]
fn judge_rejects_a_place_separation_of_zero() {
    for value in ["0", "-5", "NaN"] {
        let output = Command::new(env!("CARGO_BIN_EXE_peil"))
            .arg("judge")
            .arg(drive_fixture())
            .arg(format!("--place-separation-m={value}"))
            .output()
            .expect("peil runs");
        assert!(!output.status.success(), "{value}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("invalid rule parameters: place_separation_m must be"),
            "{value}: {stderr}"
        );
        assert!(
            output.stdout.is_empty(),
            "{value}: no verdicts on a bad rule"
        );
    }
}

#[test]
fn judge_reports_a_bad_line_without_quoting_its_value() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bad-value.jsonl");
    std::fs::write(
        &path,
        "{\"t_ns\":1,\"source\":\"kismet\",\"kind\":\"ble_advert\",\"addr\":\"aa:aa:aa:00:00:01\",\"rssi_dbm\":\"zz:zz:zz:00:00:09\"}\n",
    )
    .expect("the test file is written");
    let output = Command::new(env!("CARGO_BIN_EXE_peil"))
        .arg("judge")
        .arg(&path)
        .output()
        .expect("peil runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line 1, column "), "{stderr}");
    assert!(
        stderr.contains("(value left out), expected i16"),
        "{stderr}"
    );
    assert!(!stderr.contains("zz:zz:zz"), "{stderr}");
}

fn feed_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sensio-core/tests/fixtures/drive.feed")
}

fn peil(args: &[&std::ffi::OsStr], stdin: Option<&[u8]>) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_peil"))
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("peil runs");
    let mut input = child.stdin.take().expect("a stdin");
    std::io::Write::write_all(&mut input, stdin.unwrap_or_default()).expect("stdin is written");
    drop(input);
    child.wait_with_output().expect("peil exits")
}

/// The devices in the stream's last answer, and the types of its lines.
fn last_answer(stdout: &[u8]) -> (Vec<String>, Vec<String>) {
    let lines: Vec<serde_json::Value> = std::str::from_utf8(stdout)
        .expect("UTF-8 output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();
    let types = lines
        .iter()
        .map(|v| v["type"].as_str().expect("a type").to_owned())
        .collect();
    let answer = lines
        .iter()
        .rev()
        .find(|v| v["type"] == "verdicts")
        .expect("an answer");
    let devices = answer["verdicts"]
        .as_array()
        .expect("verdicts")
        .iter()
        .map(|v| {
            format!(
                "{} x{}",
                v["device"].as_str().expect("a device"),
                v["sightings"]
            )
        })
        .collect();
    (devices, types)
}

#[test]
fn live_replays_a_saved_feed() {
    let output = peil(&["live".as_ref(), feed_fixture().as_os_str()], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (devices, types) = last_answer(&output.stdout);
    assert_eq!(devices, ["ble:aa:aa:aa:00:00:01 x3", "tpms:0a1b2c3d x3"]);
    assert_eq!(types.first().map(String::as_str), Some("start"));
    assert_eq!(types.last().map(String::as_str), Some("end"));
    assert_eq!(types.iter().filter(|t| *t == "verdicts").count(), 3);
}

#[test]
fn live_with_all_judges_withheld_lines_too() {
    let output = peil(
        &[
            "live".as_ref(),
            "--all".as_ref(),
            feed_fixture().as_os_str(),
        ],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        last_answer(&output.stdout).0,
        ["ble:aa:aa:aa:00:00:01 x4", "tpms:0a1b2c3d x3"]
    );
}

#[test]
fn live_reads_stdin_as_it_reads_a_file() {
    let feed = std::fs::read(feed_fixture()).expect("the fixture reads");
    let from_stdin = peil(&["live".as_ref()], Some(&feed));
    let from_file = peil(&["live".as_ref(), feed_fixture().as_os_str()], None);
    assert!(from_stdin.status.success());
    assert_eq!(from_stdin.stdout, from_file.stdout);
}

#[test]
fn live_gives_the_same_bytes_twice() {
    let fixture = feed_fixture();
    let args = ["live".as_ref(), fixture.as_os_str()];
    assert_eq!(peil(&args, None).stdout, peil(&args, None).stdout);
}

#[test]
fn live_takes_the_rule_flags() {
    let fixture = feed_fixture();
    let args = [
        "live".as_ref(),
        "--all".as_ref(),
        "--min-places=4".as_ref(),
        fixture.as_os_str(),
    ];
    let output = peil(&args, None);
    assert!(output.status.success());
    assert_eq!(last_answer(&output.stdout).0, ["ble:aa:aa:aa:00:00:01 x4"]);
}

#[test]
fn live_exits_2_on_input_that_isnt_a_feed() {
    for input in [&b"{\"t_ns\":1}\n"[..], b""] {
        let output = peil(&["live".as_ref()], Some(input));
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.starts_with("Error: stdin: "), "{stderr}");
    }
}

#[test]
fn live_reports_a_missing_file() {
    let output = peil(&["live".as_ref(), "no-such-file.feed".as_ref()], None);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("opening no-such-file.feed"));
}

#[test]
fn describe_prints_the_recent_emitters_loud_first() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../sensio-core/tests/fixtures/drive_wifi.jsonl");
    let output = peil(&["describe".as_ref(), fixture.as_os_str()], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let devices: Vec<String> = String::from_utf8(output.stdout)
        .expect("UTF-8 output")
        .lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).expect("each line is JSON");
            format!(
                "{} {}",
                v["device"].as_str().expect("a device"),
                v["band"].as_str().expect("a band")
            )
        })
        .collect();
    assert_eq!(
        devices,
        [
            "wifi:ee:ee:ee:00:00:01 strong",
            "wifi:22:22:22:00:00:04 medium"
        ]
    );
}

#[test]
fn live_reports_a_bad_vendor_registry_with_its_line() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bad-vendors.csv");
    std::fs::write(&path, "MA-L,XYZ,Example\n").expect("the test file is written");
    let output = peil(
        &[
            "live".as_ref(),
            "--vendors".as_ref(),
            path.as_os_str(),
            feed_fixture().as_os_str(),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("line 1: not an MA-L, MA-M or MA-S assignment"),
        "{stderr}"
    );
}

#[test]
fn link_stops_when_the_phone_refuses_the_code() {
    use std::io::{BufRead, Write};
    let pairing = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("refused-pairing.json");
    let _ = std::fs::remove_file(&pairing);
    let phone = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = phone.local_addr().expect("address").to_string();
    let phone = std::thread::spawn(move || {
        let (stream, _) = phone.accept().expect("accept");
        let mut hello = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut hello)
            .expect("a hello");
        (&stream)
            .write_all(b"{\"type\":\"refused\",\"reason\":\"wrong code\"}\n")
            .expect("write");
        hello
    });
    let output = peil(
        &[
            "link".as_ref(),
            "--code".as_ref(),
            "000000".as_ref(),
            "--phone".as_ref(),
            address.as_ref(),
            "--pairing".as_ref(),
            pairing.as_ref(),
        ],
        None,
    );
    assert!(!pairing.exists(), "a refusal keeps no pairing");
    let hello: serde_json::Value =
        serde_json::from_str(&phone.join().expect("the phone")).expect("JSON");
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["schema"], "sensio.link/1");
    assert_eq!(hello["code"], "000000");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("refused the link: wrong code"));
}

#[test]
fn link_rejects_a_receiver_name_the_events_cant_carry() {
    let output = peil(
        &[
            "link".as_ref(),
            "--code".as_ref(),
            "1".as_ref(),
            "--phone".as_ref(),
            "127.0.0.1".as_ref(),
            "--receiver".as_ref(),
            "My Laptop".as_ref(),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--receiver must be"));
}

#[test]
fn link_pairs_with_the_code_once_then_proves_the_pairing() {
    use std::io::{BufRead, Write};
    let pairing = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("link-pairing.json");
    let _ = std::fs::remove_file(&pairing);
    let secret = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    let phone = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = phone.local_addr().expect("address").to_string();
    let hello = |stream: &std::net::TcpStream| {
        let mut line = String::new();
        std::io::BufReader::new(stream)
            .read_line(&mut line)
            .expect("a line");
        serde_json::from_str::<serde_json::Value>(&line).expect("JSON")
    };
    let phone = std::thread::spawn(move || {
        // The code pairs; the phone hands over the secret, then hangs up.
        let (first, _) = phone.accept().expect("accept");
        let pairing_hello = hello(&first);
        (&first)
            .write_all(format!("{{\"type\":\"welcome\",\"secret\":\"{secret}\"}}\n").as_bytes())
            .expect("write");
        drop(first);
        // The reconnect proves the pairing instead; this phone has forgotten it.
        let (second, _) = phone.accept().expect("accept");
        let proving_hello = hello(&second);
        (&second)
            .write_all(b"{\"type\":\"refused\",\"reason\":\"not paired\"}\n")
            .expect("write");
        (pairing_hello, proving_hello)
    });
    let run = |args: &[&str]| {
        let mut all: Vec<&std::ffi::OsStr> =
            vec!["link".as_ref(), "--phone".as_ref(), address.as_ref()];
        all.extend(["--pairing".as_ref(), pairing.as_os_str()]);
        all.extend(args.iter().map(std::ffi::OsStr::new));
        peil(&all, None)
    };
    let output = run(&[
        "--code",
        "123456",
        "--receiver",
        "thinkpad",
        "--kismet",
        "http://127.0.0.1:9",
    ]);
    let (pairing_hello, proving_hello) = phone.join().expect("the phone");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(pairing_hello["code"], "123456");
    assert!(proving_hello.get("code").is_none(), "{proving_hello}");
    assert_eq!(proving_hello["receiver"], "thinkpad");
    let kept: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&pairing).expect("the pairing is kept"))
            .expect("JSON");
    assert_eq!(
        kept,
        serde_json::json!({"receiver": "thinkpad", "secret": secret})
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("paired with the phone"), "{stderr}");
    assert!(stderr.contains("pair again with --code"), "{stderr}");

    // With the pairing kept, a different name is refused before connecting.
    let output = run(&["--receiver", "other"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("paired as thinkpad"));
}

#[test]
fn link_without_a_code_or_a_pairing_says_how_to_pair() {
    let pairing = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-pairing.json");
    let _ = std::fs::remove_file(&pairing);
    let output = peil(
        &[
            "link".as_ref(),
            "--phone".as_ref(),
            "127.0.0.1".as_ref(),
            "--pairing".as_ref(),
            pairing.as_os_str(),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not paired with the phone yet"));
}
