# Third-party notices

## Programs Peil works with

Peil reads the output of these programs across a process boundary. It doesn't link, include or ship their code (ADR-0001); a user installs them separately, under their own licenses.

| Program | Role | License |
|---|---|---|
| [Kismet](https://www.kismetwireless.net/) | Wi-Fi and BLE capture, device records, packet stream | GPL-2.0 |
| [rtl_433](https://github.com/merbanan/rtl_433) | Sub-GHz decoding (tire-pressure sensors and others) | GPL-2.0 |
| [gpsd](https://gpsd.io/) | GNSS position and time | BSD-2-Clause |

If Peil is ever distributed together with any of them, that distribution meets their licenses for those programs and lists them here.

## Rust crates

Peil's binaries are built from crates on crates.io, each under its own license (most under MIT or Apache-2.0). `Cargo.lock` pins the exact versions; `cargo metadata` lists each crate's license. A generated list joins this file with the first binary release.
