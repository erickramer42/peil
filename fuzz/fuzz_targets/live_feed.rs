//! The live judge over any bytes as a feed, through `run` and the real
//! normalizers. Beyond not panicking: a feed it reads gives a stream that
//! starts with `start` and ends with `end`, answers as many `judge` lines as
//! the end says, rejects each line at most once in order, and is the same
//! on a second run.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_core::feed::VerdictLine;
use sensio_core::{Fatal, Live, PersistenceRule, RunError, run};
use sensio_sources::Sources;

fn live(data: &[u8], all: bool) -> Result<Vec<VerdictLine>, Fatal> {
    let mut out = Vec::new();
    let engine = Live::new(Sources, PersistenceRule::default(), all);
    match run(engine, data, |line| {
        out.push(line.clone());
        Ok(())
    }) {
        Ok(()) => Ok(out),
        Err(RunError::Fatal(fatal)) => Err(fatal),
        Err(err) => panic!("a slice can't fail to read: {err}"),
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&flags, feed)) = data.split_first() else {
        return;
    };
    let all = flags & 1 == 1;
    let lines = match live(feed, all) {
        Ok(lines) => lines,
        Err(Fatal::NotAFeed { line, .. }) => {
            assert!(line >= 1);
            return;
        }
        Err(Fatal::Empty) => return,
    };

    assert!(matches!(lines.first(), Some(VerdictLine::Start { .. })));
    let Some(VerdictLine::End {
        feed_lines, judged, ..
    }) = lines.last()
    else {
        panic!("no end line");
    };
    let answers = lines
        .iter()
        .filter(|l| matches!(l, VerdictLine::Verdicts { .. }))
        .count();
    assert_eq!(answers, *judged);
    let mut last_rejected = 0;
    for line in &lines {
        if let VerdictLine::Rejected { feed_line, .. } = line {
            assert!(
                *feed_line > last_rejected && feed_line <= feed_lines,
                "{line:?}"
            );
            last_rejected = *feed_line;
        }
        serde_json::to_string(line).expect("a verdict-stream line serializes");
    }
    assert_eq!(live(feed, all).ok(), Some(lines), "a second run differs");
});
