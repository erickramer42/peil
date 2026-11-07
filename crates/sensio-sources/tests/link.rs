//! The laptop's side of the link: the Kismet poller against a fake Kismet,
//! and a link session against a fake phone, both on loopback.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use sensio_sources::kismet_poll::{PollError, Poller};
use sensio_sources::link::{self, Config, LinkError, LinkLine};
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

fn config(phone: SocketAddr, code: &str) -> Config {
    Config {
        phone,
        code: code.to_owned(),
        receiver: "thinkpad".to_owned(),
        app: "peil-test".to_owned(),
        poll_every: Duration::from_millis(50),
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
        send(&mut stream, &LinkLine::Welcome);
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
    let ended = link::run_session(
        &config(address, "123456"),
        &mut poller,
        clock,
        &stop,
        &mut |text| log.push(text.to_owned()),
    );
    let got = phone.join().expect("the phone");

    assert!(matches!(ended, Err(LinkError::Closed(_))), "{ended:?}");
    assert_eq!(
        got[0],
        LinkLine::Hello {
            schema: link::SCHEMA.to_owned(),
            code: "123456".to_owned(),
            receiver: "thinkpad".to_owned(),
            app: "peil-test".to_owned(),
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
        &config(address, "000000"),
        &mut poller,
        clock,
        &AtomicBool::new(false),
        &mut |_| {},
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
    assert_eq!(read(r#"{"type":"welcome"}"#), LinkLine::Welcome);
    assert_eq!(
        read(r#"{"type":"refused","reason":"wrong code"}"#),
        LinkLine::Refused {
            reason: "wrong code".to_owned()
        }
    );
    assert_eq!(read(r#"{"type":"ping","id":3}"#), LinkLine::Ping { id: 3 });
}
