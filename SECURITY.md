# Security policy

Peil reads input that strangers control: every BLE advertisement and Wi-Fi frame a sensor picks up was sent by someone else, and some of it may be built to break the program reading it. Its recordings and verdicts also hold other people's device identifiers and the user's own movements. A flaw in either is a security problem, and reports are welcome.

## Supported versions

Peil is an early scaffold. Only the latest release is supported: fixes go to the `main` branch and into the next release.

## Reporting a vulnerability

Report it privately through GitHub: [open a security advisory](https://github.com/erickramer42/peil/security/advisories/new) (the repository's Security tab, then "Report a vulnerability"). Don't open a public issue, pull request or discussion for it.

Include:

- the commit you tested (`git rev-parse HEAD`) and your platform,
- what an attacker controls (a radio transmission, a sensor program's output, a file of events, a command-line argument),
- the smallest input that shows the problem, and what happens: a crash, a hang, memory use without bound, a wrong verdict, or data that should have stayed private.

**Keep reports free of real personal data.** Don't attach a real recording or real device identifiers, positions or timestamps, even redacted. Rebuild the input from synthetic values instead, the way Peil's own test fixtures are written: BLE addresses whose first three octets repeat one byte (`aa:aa:aa:00:00:01`), coordinates with two decimals at most, and timestamps such as `1800000000000000000`.

Reports are read as they arrive. There is no fixed response time, but you'll get an answer, and a confirmed problem gets a fix, an advisory and credit if you want it. Please keep the details private until the advisory is published.

## In scope

- **Hostile input:** anything a transmission, a sensor program's output, a file of events or a peer on the app's laptop link (its port on the USB tether) can do to Peil beyond being rejected: a panic, an integer overflow, a hang, an allocation without a size check, or a parser that accepts what it should refuse.
- **Wrong verdicts from crafted input:** input that makes a device the rules should flag go unflagged, or makes the same input give different verdicts.
- **Personal data leaving its place:** an error message, log line, export or verdict that exposes a device identifier, a position or a timestamp it shouldn't, such as an error that repeats a rejected value.
- **Dependencies and CI:** a vulnerable dependency Peil uses in a way that matters, or a weakness in its GitHub Actions workflow.

## Out of scope

- **Flaws in the programs Peil reads** (Kismet, rtl_433, gpsd): report those to their own projects. Peil runs them as separate programs and links none of their code.
- **Attacks that need the user's machine or account already compromised,** or a build of Peil changed by the attacker.
- **Requests for Peil to transmit, jam, deauthenticate, spoof, or decode the contents of anyone's communications.** Peil is defensive by design (see [Use and privacy](README.md#use-and-privacy)); those are out of scope as features, not only as reports.

## Testing

Test against your own hardware and synthetic events only. Don't capture, transmit to or interfere with other people's devices to research a report: Peil's tests use synthetic input for the same reason.
