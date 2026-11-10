//! The FAA registry snapshot's reader (`aircraft_registry.rs`): any bytes as
//! a snapshot, its first blocks read and addresses from the input looked up.
//! Nothing panics, and a block's rows are in rising address order within
//! 24 bits.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_core::AircraftRegistry;

/// Each block can inflate to 64 KiB, so a run reads only the first few.
const MAX_BLOCKS: usize = 8;

fuzz_target!(|data: &[u8]| {
    let Ok(registry) = AircraftRegistry::new(data) else {
        return;
    };
    for i in 0..registry.blocks().min(MAX_BLOCKS) {
        if let Ok(rows) = registry.block(i) {
            assert!(
                rows.windows(2).all(|w| w[0].0 < w[1].0),
                "block {i}'s rows don't rise"
            );
            assert!(
                rows.iter().all(|(a, _)| *a <= 0xff_ffff),
                "block {i} past 24 bits"
            );
            if let Some((address, _)) = rows.first() {
                let _ = registry.lookup(&format!("{address:06x}"));
            }
        }
    }
    for chunk in data.chunks(3).take(MAX_BLOCKS) {
        let address = chunk.iter().fold(0xa0_u32, |a, b| (a << 8) | u32::from(*b)) & 0xaf_ffff;
        let _ = registry.lookup(&format!("{address:06x}"));
    }
});
