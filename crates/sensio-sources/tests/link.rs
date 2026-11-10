//! The laptop's side of the link: the Kismet poller against a fake Kismet,
//! and a link session against a fake phone, both on loopback.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use sensio_sources::kismet_poll::{PollError, Poller};
use sensio_sources::link::{
    self, Auth, Config, LinkError, LinkLine, OwnAddress, OwnRadio, Pairing,
};
use serde_json::{Value, json};

const NOW_NS: i64 = 1_800_000_000_000_000_000;

fn clock() -> i64 {
    NOW_NS
}

fn record(mac: &str, last_time: i64) -> Value {
    json!({
        "kismet.device.base.key": format!("key-{mac}"),
        "kismet.device.base.macaddr": mac,
        "kismet.device.base.phyname": "Bluetooth",
        "kismet.device.base.type": "BTLE",
        "kismet.device.base.last_time": last_time,
        "kismet.device.base.signal": {
            "kismet.common.signal.type": "dbm",
            "kismet.common.signal.last_signal": -70,
            "kismet.common.signal.signal_rrd": {"kismet.common.rrd.minute_vec": [0, 0]}
        }
    })
}

/// A fake Kismet answering each request, one per connection, with the next
/// of `answers` (a status and a body), and sending each request's head to
/// the returned receiver.
fn fake_kismet(answers: Vec<(u16, String)>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let (heads, received) = mpsc::channel();
    thread::spawn(move || {
        for (status, body) in answers {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let _ = heads.send(head);
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, received)
}

#[test]
fn the_poller_asks_for_recent_devices_with_the_key_and_hands_each_on_once_per_sighting() {
    let first = json!([
        record("aa:aa:aa:00:00:01", 100),
        record("bb:bb:bb:00:00:01", 100)
    ]);
    let second = json!([
        record("aa:aa:aa:00:00:01", 100),
        record("bb:bb:bb:00:00:01", 101)
    ]);
    let (base, heads) = fake_kismet(vec![(200, first.to_string()), (200, second.to_string())]);
    let mut poller = Poller::new(&base, Some("example-key".to_owned()), 10);

    let lines = poller.poll().expect("the first poll");
    let head = heads.recv().expect("a request");
    assert!(
        head.starts_with("GET /devices/last-time/-10/devices.json "),
        "{head}"
    );
    assert!(head.contains("KISMET=example-key"), "no API key: {head}");
    assert_eq!(lines.len(), 2);
    let pruned: Value = serde_json::from_str(&lines[0]).expect("JSON");
    assert!(
        pruned["kismet.device.base.signal"]
            .get("kismet.common.signal.signal_rrd")
            .is_none()
    );

    let lines = poller.poll().expect("the second poll");
    assert_eq!(
        lines.len(),
        1,
        "only the device seen again is handed on: {lines:?}"
    );
    assert!(lines[0].contains("bb:bb:bb:00:00:01"));
}

#[test]
fn the_poller_says_why_kismet_gave_nothing() {
    let (base, _heads) = fake_kismet(vec![(401, String::new()), (200, "{\"not\": 1}".to_owned())]);
    let mut poller = Poller::new(&base, None, 10);
    let unauthorized = poller.poll().expect_err("a 401");
    assert_eq!(unauthorized, PollError::Status(401));
    assert!(unauthorized.to_string().contains("PEIL_KISMET_API_KEY"));
    assert!(matches!(poller.poll(), Err(PollError::NotDevices(_))));

    let closed = TcpListener::bind("127.0.0.1:0").expect("bind");
    let base = format!("http://{}", closed.local_addr().expect("address"));
    drop(closed);
    let mut poller = Poller::new(&base, None, 10);
    assert!(matches!(poller.poll(), Err(PollError::Unreachable(_))));
}

/// A pairing's secret, as the app would hand it over.
const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

fn config(phone: SocketAddr, auth: Auth) -> Config {
    Config {
        phone,
        auth,
        receiver: "laptop".to_owned(),
        app: "peil-test".to_owned(),
        own: vec![own_wifi()],
        poll_every: Duration::from_millis(50),
    }
}

fn own_wifi() -> OwnAddress {
    OwnAddress {
        radio: OwnRadio::Wifi,
        address: "aa:bb:cc:dd:ee:ff".to_owned(),
    }
}

fn send(stream: &mut TcpStream, line: &LinkLine) {
    let mut text = serde_json::to_string(line).expect("JSON");
    text.push('\n');
    stream.write_all(text.as_bytes()).expect("write");
}

fn receive(reader: &mut BufReader<TcpStream>) -> LinkLine {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read");
    serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}: {line}"))
}

#[test]
fn a_session_says_hello_answers_pings_and_sends_kismets_records() {
    let devices = json!([record("aa:aa:aa:00:00:01", 100)]).to_string();
    let (base, _heads) = fake_kismet(vec![(200, devices); 50]);
    let phone = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = phone.local_addr().expect("address");

    let phone = thread::spawn(move || {
        let (mut stream, _) = phone.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let hello = receive(&mut reader);
        send(
            &mut stream,
            &LinkLine::Welcome {
                secret: Some(SECRET.to_owned()),
            },
        );
        send(&mut stream, &LinkLine::Ping { id: 7 });
        let mut got = vec![hello];
        while !(got.iter().any(|l| matches!(l, LinkLine::Pong { .. }))
            && got.iter().any(|l| matches!(l, LinkLine::Line { .. })))
        {
            got.push(receive(&mut reader));
        }
        got
    });

    let mut poller = Poller::new(&base, None, 10);
    let stop = AtomicBool::new(false);
    let mut log = Vec::new();
    let mut pairings = Vec::new();
    let ended = link::run_session(
        &config(address, Auth::Code("123456".to_owned())),
        &mut poller,
        clock,
        &stop,
        &mut |text| log.push(text.to_owned()),
        &mut |pairing| pairings.push(pairing),
    );
    let got = phone.join().expect("the phone");
    assert_eq!(
        pairings,
        [Pairing {
            receiver: "laptop".to_owned(),
            secret: SECRET.to_owned()
        }],
        "the code's welcome is a pairing to keep"
    );

    assert!(matches!(ended, Err(LinkError::Closed(_))), "{ended:?}");
    assert_eq!(
        got[0],
        LinkLine::Hello {
            schema: link::SCHEMA.to_owned(),
            code: Some("123456".to_owned()),
            receiver: "laptop".to_owned(),
            app: "peil-test".to_owned(),
            own: vec![own_wifi()],
        }
    );
    assert!(got.contains(&LinkLine::Pong {
        id: 7,
        remote_ns: NOW_NS
    }));
    assert!(got.contains(&LinkLine::Status {
        text: "Kismet is answering".to_owned()
    }));
    let Some(LinkLine::Line {
        source,
        remote_ns,
        raw,
    }) = got.iter().find(|l| matches!(l, LinkLine::Line { .. }))
    else {
        unreachable!("the loop waited for a line");
    };
    assert_eq!((source.as_str(), *remote_ns), ("kismet", NOW_NS));
    assert!(
        raw.contains("aa:aa:aa:00:00:01") && !raw.contains("rrd"),
        "{raw}"
    );
    assert!(
        log.iter().any(|l| l.starts_with("linked to the phone")),
        "{log:?}"
    );
}

#[test]
fn a_refused_hello_ends_the_session_with_the_reason() {
    let phone = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = phone.local_addr().expect("address");
    let phone = thread::spawn(move || {
        let (mut stream, _) = phone.accept().expect("accept");
        let mut hello = [0u8; 1];
        let _ = stream.read(&mut hello);
        send(
            &mut stream,
            &LinkLine::Refused {
                reason: "wrong code".to_owned(),
            },
        );
    });
    let mut poller = Poller::new("http://127.0.0.1:9", None, 10);
    let ended = link::run_session(
        &config(address, Auth::Code("000000".to_owned())),
        &mut poller,
        clock,
        &AtomicBool::new(false),
        &mut |_| {},
        &mut |_| panic!("a refusal pairs nothing"),
    );
    phone.join().expect("the phone");
    match ended {
        Err(LinkError::Refused(reason)) => assert_eq!(reason, "wrong code"),
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn the_phone_is_the_gateway_of_the_usb_tethers_default_route() {
    let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
                 wlan0\t00000000\t0100000A\t0003\t0\t0\t600\t00000000\n\
                 usb0\t002AA8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\n\
                 usb0\t00000000\t812AA8C0\t0003\t0\t0\t100\t00000000\n";
    assert_eq!(
        link::tether_gateway(table),
        Some(Ipv4Addr::new(192, 168, 42, 129))
    );
    assert_eq!(link::tether_gateway(&table.replace("usb0", "eth0")), None);
}

#[test]
fn the_lines_the_app_writes_are_the_ones_the_laptop_reads() {
    // As the app's LinkProtocol writes them.
    let read = |text: &str| serde_json::from_str::<LinkLine>(text).expect(text);
    assert_eq!(
        read(r#"{"type":"welcome"}"#),
        LinkLine::Welcome { secret: None }
    );
    assert_eq!(
        read(r#"{"type":"welcome","secret":"0a0b"}"#),
        LinkLine::Welcome {
            secret: Some("0a0b".to_owned())
        }
    );
    assert_eq!(
        read(r#"{"type":"challenge","nonce":"aa"}"#),
        LinkLine::Challenge {
            nonce: "aa".to_owned()
        }
    );
    assert_eq!(
        read(r#"{"type":"refused","reason":"wrong code"}"#),
        LinkLine::Refused {
            reason: "wrong code".to_owned()
        }
    );
    assert_eq!(read(r#"{"type":"ping","id":3}"#), LinkLine::Ping { id: 3 });
}

#[test]
fn the_hello_carries_own_addresses_as_the_app_reads_them_and_none_when_empty() {
    // As the app's LinkProtocolTest reads them.
    let hello = |own| LinkLine::Hello {
        schema: link::SCHEMA.to_owned(),
        code: None,
        receiver: "laptop".to_owned(),
        app: "sensio-test".to_owned(),
        own,
    };
    let bluetooth = OwnAddress {
        radio: OwnRadio::Bluetooth,
        address: "aa:aa:aa:00:00:01".to_owned(),
    };
    assert_eq!(
        serde_json::to_string(&hello(vec![own_wifi(), bluetooth])).expect("JSON"),
        r#"{"type":"hello","schema":"sensio.link/1","receiver":"laptop","app":"sensio-test","own":[{"radio":"wifi","address":"aa:bb:cc:dd:ee:ff"},{"radio":"bluetooth","address":"aa:aa:aa:00:00:01"}]}"#
    );
    let none = serde_json::to_string(&hello(Vec::new())).expect("JSON");
    assert!(!none.contains("own"), "{none}");
    assert_eq!(
        serde_json::from_str::<LinkLine>(&none).expect("JSON"),
        hello(Vec::new())
    );
}

#[test]
fn own_addresses_are_six_hex_octets_and_say_which_radio() {
    assert_eq!(link::parse_own("wifi:AA:BB:CC:DD:EE:FF"), Ok(own_wifi()));
    assert_eq!(
        link::parse_own("bluetooth:aa:aa:aa:00:00:01").map(|o| o.radio),
        Ok(OwnRadio::Bluetooth)
    );
    for bad in [
        "aa:bb:cc:dd:ee:ff",
        "ble:aa:bb:cc:dd:ee:ff",
        "wifi:aa:bb:cc:dd:ee",
        "wifi:aa:bb:cc:dd:ee:fg",
        "wifi:aa:bb:cc:dd:ee:+f",
        "wifi:aabb.ccdd.eeff",
        "wifi:00:00:00:00:00:00",
    ] {
        let reason = link::parse_own(bad).expect_err(bad);
        assert!(
            !reason.contains("aa"),
            "the reason quotes the text: {reason}"
        );
    }
}

#[test]
fn sysfs_gives_the_wireless_interfaces_and_bluetooth_controllers() {
    let sys = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("own-sysfs");
    let _ = std::fs::remove_dir_all(&sys);
    let write = |path: &str, text: &str| {
        let path = sys.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    };
    write("class/net/wlan0/address", "AA:BB:CC:DD:EE:FF\n");
    write("class/net/wlan0/phy80211/name", "phy0\n");
    write("class/net/wlan0mon/address", "aa:bb:cc:dd:ee:ff\n");
    write("class/net/wlan0mon/wireless/x", "");
    write("class/net/eth0/address", "bb:bb:bb:00:00:01\n");
    write("class/net/lo/address", "00:00:00:00:00:00\n");
    write("class/net/lo/wireless/x", "");
    write("class/bluetooth/hci0/address", "aa:aa:aa:00:00:01\n");
    if cfg!(unix) {
        // A connection under its controller; Windows can't name a directory so.
        write("class/bluetooth/hci0:11/address", "cc:cc:cc:00:00:01\n");
    }
    write("class/bluetooth/hci1/name", "no address shown\n");

    assert_eq!(
        link::own_from_sysfs(&sys),
        [
            own_wifi(),
            OwnAddress {
                radio: OwnRadio::Bluetooth,
                address: "aa:aa:aa:00:00:01".to_owned(),
            },
        ],
        "the wired card, the zero address and a connection are left out; the monitor interface repeats wlan0"
    );
    assert_eq!(link::own_from_sysfs(&sys.join("absent")), []);
}

#[test]
fn hmac_sha256_matches_rfc_4231() {
    let hex = |bytes: [u8; 32]| {
        bytes.iter().fold(String::new(), |mut out, b| {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
            out
        })
    };
    // Test case 1, and test case 6 (a key longer than the block).
    assert_eq!(
        hex(link::hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hex(link::hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn the_proof_is_the_one_the_app_checks() {
    // The same vector is in the app's LinkProtocolTest.
    let pairing = Pairing {
        receiver: "laptop".to_owned(),
        secret: SECRET.to_owned(),
    };
    assert_eq!(
        link::proof(&pairing, &"aa".repeat(16)).as_deref(),
        Some("d2fba96fe4aa315748030adf529ace1cdc3ecec2f9cf6be2cc8f46fc700a1dae")
    );
    let short = Pairing {
        secret: "0a0b".to_owned(),
        ..pairing
    };
    assert_eq!(link::proof(&short, "aa"), None, "a secret too short to use");
}

#[test]
fn a_secret_with_a_sign_in_it_gives_no_proof() {
    // Found by the link_line fuzz target: a sign parses as part of a hex byte.
    let pairing = Pairing {
        receiver: "laptop".to_owned(),
        secret: format!("+a{}", "0a".repeat(15)),
    };
    assert_eq!(link::proof(&pairing, "aa"), None);
}

#[test]
fn a_paired_laptop_answers_the_challenge_and_sends_no_code() {
    let phone = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = phone.local_addr().expect("address");
    let phone = thread::spawn(move || {
        let (mut stream, _) = phone.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let hello = receive(&mut reader);
        send(
            &mut stream,
            &LinkLine::Challenge {
                nonce: "aa".repeat(16),
            },
        );
        let proof = receive(&mut reader);
        send(&mut stream, &LinkLine::Welcome { secret: None });
        (hello, proof)
    });
    let pairing = Pairing {
        receiver: "laptop".to_owned(),
        secret: SECRET.to_owned(),
    };
    let mut poller = Poller::new("http://127.0.0.1:9", None, 10);
    let mut log = Vec::new();
    let ended = link::run_session(
        &config(address, Auth::Paired(pairing)),
        &mut poller,
        clock,
        &AtomicBool::new(false),
        &mut |text| log.push(text.to_owned()),
        &mut |_| panic!("proving a pairing makes no new one"),
    );
    let (hello, proof) = phone.join().expect("the phone");
    assert!(matches!(ended, Err(LinkError::Closed(_))), "{ended:?}");
    assert_eq!(
        hello,
        LinkLine::Hello {
            schema: link::SCHEMA.to_owned(),
            code: None,
            receiver: "laptop".to_owned(),
            app: "peil-test".to_owned(),
            own: vec![own_wifi()],
        },
        "the pairing's receiver name, and no code"
    );
    assert_eq!(
        proof,
        LinkLine::Proof {
            mac: "d2fba96fe4aa315748030adf529ace1cdc3ecec2f9cf6be2cc8f46fc700a1dae".to_owned()
        }
    );
    assert!(
        log.iter().any(|l| l.starts_with("linked to the phone")),
        "{log:?}"
    );
}
