#![no_main]

use gitturtle_core::{ConflictBlockChoice, choose_conflict_block, text_conflict_blocks};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data)
        && let Ok(blocks) = text_conflict_blocks(source)
    {
        for block in &blocks {
            let _ = choose_conflict_block(source, block, ConflictBlockChoice::Current);
            let _ = choose_conflict_block(source, block, ConflictBlockChoice::Incoming);
            let _ = choose_conflict_block(source, block, ConflictBlockChoice::Both);
        }
    }
});
