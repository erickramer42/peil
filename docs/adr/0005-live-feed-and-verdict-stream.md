# ADR-0005: The live feed and the verdict stream

Status: Accepted. Built: the feed and verdict-stream formats with the `radio` line and per-radio coverage, `peil live`, the `sensio`, `android` and `kismet` normalizers, and the incremental judge with its lateness window. Not yet built: an `end` for an interrupted stream.

## Context

ADR-0004 runs the core as a process inside the Android app, fed raw lines over stdin and answering with live verdicts on stdout, and keeps every raw line on the phone from the first session. It requires that the live verdict at a time t is the judge over the events that arrived by t, and that a replay reproduces every verdict the user saw. The pipe and the saved lines need a format, and the format becomes a saved one: files written from the first drive are read back by every later version of the core.

## Decision

1. **One format, `sensio.feed/1`, for the pipe and the saved raw lines.** The app writes the same lines to the core's stdin and to the session's file. A replay is the same command run over the file: `peil live <file>` gives, byte for byte, what the core wrote live. The format is versioned on its own, apart from the product's version (ADR-0003 section 3).

2. **Feed lines.** Each is a JSON object on one line, with a `type`:
   - `start`, the first line: `schema` (`sensio.feed/1`), the app's version, and the boot clock at the session's start.
   - `clock`: a boot-clock reading (`boot_ns`) with the UTC time it maps to (`utc_ns`) and its basis, `gnss` or `system`. Arrival times map to UTC through the latest `clock` line before them, so the mapping a live session used is the one a replay uses (ADR-0004 section 5).
   - `offset`: a receiver's measured clock offset to the phone and its uncertainty, for lines a laptop stamped (ADR-0004 section 6).
   - `line`: one source's output as received. It carries `seq` (rising, never repeated), `boot_ns` (arrival on the phone), `source`, `receiver`, `raw` (the received line's exact text, as a JSON string), and, for a line a laptop stamped, `remote_ns` (its time on that receiver's clock).
   - `judge`: a request for verdicts, with its `boot_ns`. The verdicts cover every `line` before it in the feed.
   - `dropped`: how many lines, per source, the core didn't get (item 5).
   - `slept`: time the phone slept, as lost coverage.

   Amended 2026-11-07: a `radio` line, written when a radio's state changes: its `boot_ns`, the `radio` (`wifi`, `ble`, `gnss`, `bt_classic`, or `link` for a laptop's link), its `receiver`, and its `state`: `on`, `filtered` (part of what it could receive, such as BLE's screen-off filters, ADR-0004), `off`, or `failed` (asked to receive and refused, such as a failed BLE scan or a refused Wi-Fi scan). A radio's time can't go back; a `radio` line that does is rejected. It records the reduced and lost coverage ADR-0004 asks for in the feed, so a replay shows it as the live session did.

3. **Event times.** A normalized event's `t_ns` is its line's arrival mapped to UTC: `remote_ns` through the receiver's latest `offset` when present, otherwise `boot_ns` through the latest `clock`. A source's own timestamps are used only for spacing within that source, never as the session clock (ADR-0002 section 3).

   Amended 2026-11-07: an event's time is at least the latest event time so far minus a lateness window of 10 minutes. An event that maps earlier, such as one from a receiver whose lines stalled or one after a `clock` line that moves time back by more than the window, takes the window's edge as its time, and coverage counts its line as `late` for its source. A `judge` line's time is held to the same edge. Every event then arrives within the window of its place in time order, so the core can fold older events into its state and keep only the window's events raw (section 4).

4. **Judging.** Each `judge` line runs the existing judge over every normalized event so far and answers with the full current verdict set, not a difference from the last one. A property test holds that the answer to every `judge` equals `judge()` over the events before it; an incremental judge may replace the re-run only by passing the same test. Nothing about the judge itself changes.

   Amended 2026-11-07: the core judges and describes incrementally. Events older than the lateness window (section 3) are folded, in the judge's order, into per-emitter state; each answer folds the window's events onto a copy of what they touch. Memory then grows with the distinct emitters, not the sightings, and an answer costs about the same late in a long drive as early in it. The property test holds that each answer equals `judge()` over the events before it, with their times held to the window as above.

5. **Every raw line is kept, including the ones the core didn't get.** When the core falls behind, the app buffers; when the buffer is full, it stops passing lines to the core but still writes them to the file, marked `"withheld": true`, and writes a `dropped` line with the count into both. The core never sees a withheld line live. `peil live <file>` skips withheld lines, so a replay is exactly what the core saw; `peil live --all <file>` judges them too, for the most complete verdicts a session's raw lines can give.

6. **The verdict stream, `sensio.verdicts/1`.** Each line is a JSON object with a `type`:
   - `start`: the schema, the core's version and `RULE_SET_VERSION`.
   - `verdicts`: the `judge` it answers (its position in the feed) and the full verdict set.
   - `rejected`: a line that couldn't be normalized, by its `seq`, with the reason and never its values; it is counted and skipped, and the stream goes on.
   - `coverage`: per source and receiver, the lines seen, rejected, withheld and dropped, and time asleep. Amended 2026-11-07: also the time it covers up to (the `judge` line's `boot_ns`, or at the end the feed's latest), each source's latest arrival overall and by event kind, and each radio's state, when it entered it, and its time `filtered` and lost (`off` or `failed`) so far.
   - `end`, the last line: why the stream ended (input closed, interrupted) and the totals.

   Amended 2026-11-07: an `emitters` line, the emitter picture at that moment (ADR-0006), comes before each `verdicts` line. Readers skip line types they don't know, so a new line type is an additive change.

7. **Hostile input.** Every field of a feed line is checked: known `type`s only, a `seq` that rises, times in range, `receiver` as in the event schema, and a line at most 1 MiB long. A bad feed line is `rejected` and counted; a torn last line is dropped and counted; a first line that isn't `start` with a known `schema` is fatal, with exit code 2. A source's raw text is parsed only by that source's normalizer, and one bad raw line rejects only that line.

8. **Normalizers and the first source.** Each source's normalizer lives in `sensio-sources` and turns a `raw` text into normalized events (`crates/sensio-core/src/event.rs`). A line with `source` `sensio` carries one normalized event as its raw text. It lets the live mode, its tests and the fake sources for the gates (synthetic drives fed through the feed) land before the Android and Kismet normalizers do.

9. **The command:** `peil live [FILE]` reads a feed from the file, or from stdin without one, and writes the verdict stream to stdout. `peil judge` stays as it is, for files of normalized events.

## Consequences

- Replay equals live by construction, so the 0.1.0 gate's "a drive judged again gives every verdict that was shown, at its time" is a byte comparison.
- Re-running the judge over every event on each `judge` costs time proportional to the session so far. The load budget (a stated events-a-second rate, measured on the phone and the Pi 3) says when an incremental judge is needed; until then the simple form stays. Amended 2026-11-07: a synthetic two-hour drive at the phone's measured rate held every event, about 68 MB, and each answer re-sorted them all, so the incremental judge of section 4 replaces the re-run. An event more than the window late is misdated to the window's edge rather than lost, and its count shows where.
- Escaping each raw line into a JSON string costs some size; the files compress well.
- Kismet's pcapng packet stream is binary and doesn't fit a text `raw`; it stays a separate raw file in the recording (ADR-0002 section 1), and the feed carries Kismet's JSON outputs.
- A change that breaks `sensio.feed/1` or `sensio.verdicts/1` is a new schema version that the core reads alongside the old one, never an edit of the old one: files from the first drive stay readable.
- Fake sources that stall, restart mid-stream, tear a line or never start become synthetic feeds, so ADR-0001's "absent, restarted or slow" is tested in CI through the same command the phone runs.
