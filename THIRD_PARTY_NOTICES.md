# Third-party notices

## Programs Peil works with

Peil reads the output of these programs across a process boundary. It doesn't link, include or ship their code (ADR-0001); a user installs them separately, under their own licenses.

| Program | Role | License |
|---|---|---|
| [Kismet](https://www.kismetwireless.net/) | Wi-Fi and BLE capture, device records, packet stream | GPL-2.0 |
| [rtl_433](https://github.com/merbanan/rtl_433) | Sub-GHz decoding (tire-pressure sensors and others) | GPL-2.0 |
| [gpsd](https://gpsd.io/) | GNSS position and time | BSD-2-Clause |

If Peil is ever distributed together with any of them, that distribution meets their licenses for those programs and lists them here.

## The Android app

The APK ships the Kotlin standard library (`org.jetbrains.kotlin:kotlin-stdlib`) and the JetBrains annotations it depends on (`org.jetbrains:annotations`), both Apache-2.0, which every Kotlin program ships; everything else it uses is Android's own platform API ([ADR-0004](docs/adr/0004-android-app.md) section 8). The app's About page shows Peil's licenses, MIT and Apache-2.0, from the APK itself, and the Apache License's text covers both libraries; neither library's jar carries a NOTICE file. The About page also shows the notices of the Rust crates linked into the core (below). Build and test tools (Gradle, the Android Gradle plugin, JUnit) never ship in the APK.

## Rust crates

Peil's binaries are built from crates on crates.io, each under its own license. Every crate they ship offers MIT and is used under it (`unicode-ident` also needs Unicode-3.0). `Cargo.lock` pins the exact versions; `cargo metadata` lists each crate's license. CI's `deny` job checks every crate against the allowlist in `deny.toml`, which admits no GPL, LGPL or AGPL license (ADR-0001 section 2). Each binary carries the MIT notices of the crates linked into it, which `packaging/crate_licenses.py` writes from `Cargo.lock` for the binary's target: a Linux release's tarball as `THIRD_PARTY_LICENSES.txt`, and the APK for the core inside it, which its About page shows. Crates used only to build (procedural macros, build scripts' dependencies) aren't linked and aren't listed.
