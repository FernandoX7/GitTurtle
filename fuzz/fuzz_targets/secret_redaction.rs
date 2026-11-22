#![no_main]

use gitturtle_core::{redact_diagnostic, strip_bidi_controls};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    const SENTINEL: &str = "gtsec_fuzz_sentinel_token";
    let text = String::from_utf8_lossy(data);
    let combined = format!(
        "https://user:{SENTINEL}@host.example/repo.git?token={SENTINEL} {text}"
    );
    let redacted = redact_diagnostic(&combined);
    let stripped = strip_bidi_controls(&redacted);
    assert!(!stripped.contains(SENTINEL));
    assert!(
        !stripped
            .chars()
            .any(|ch| matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
    );
});
