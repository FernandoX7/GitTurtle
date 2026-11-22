//! Property-based tests for untrusted Git parsers, conflict-block editing,
//! and secret/bidi redaction in `gitturtle-core`.

use gitturtle_core::{
    ConflictBlockChoice, choose_conflict_block, redact_diagnostic, strip_bidi_controls,
    text_conflict_blocks,
};
use proptest::prelude::*;

fn bounded_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        failure_persistence: None,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0x5A17_C0DE_0001),
        ..ProptestConfig::default()
    }
}

fn has_bidi_control(s: &str) -> bool {
    s.chars()
        .any(|ch| matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
}

proptest! {
    #![proptest_config(bounded_config())]

    #[test]
    fn conflict_block_parser_and_chooser_never_panic(source in ".{0,512}") {
        if let Ok(blocks) = text_conflict_blocks(&source) {
            for block in &blocks {
                for choice in [
                    ConflictBlockChoice::Current,
                    ConflictBlockChoice::Incoming,
                    ConflictBlockChoice::Both,
                ] {
                    let _ = choose_conflict_block(&source, block, choice);
                }
            }
        }
    }

    #[test]
    fn valid_two_way_conflict_block_round_trips_exact_sides(
        prefix in "[a-z0-9 ]{0,40}",
        ours in "[a-z0-9 ]{0,40}",
        theirs in "[a-z0-9 ]{0,40}",
        suffix in "[a-z0-9 ]{0,40}",
    ) {
        let source = format!(
            "{prefix}\n<<<<<<< HEAD\n{ours}\n=======\n{theirs}\n>>>>>>> topic\n{suffix}\n"
        );
        let blocks = text_conflict_blocks(&source).expect("well-formed 2-way conflict");
        prop_assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        let keep_ours = choose_conflict_block(&source, block, ConflictBlockChoice::Current)
            .expect("choose ours");
        prop_assert_eq!(keep_ours, format!("{prefix}\n{ours}\n{suffix}\n"));
        let keep_theirs = choose_conflict_block(&source, block, ConflictBlockChoice::Incoming)
            .expect("choose theirs");
        prop_assert_eq!(keep_theirs, format!("{prefix}\n{theirs}\n{suffix}\n"));
        let keep_both = choose_conflict_block(&source, block, ConflictBlockChoice::Both)
            .expect("choose both");
        prop_assert_eq!(keep_both, format!("{prefix}\n{ours}\n{theirs}\n{suffix}\n"));
    }

    #[test]
    fn redaction_and_bidi_stripping_never_leak_url_credentials_or_bidi_controls(
        user in "[a-z]{3,10}",
        secret_body in "[A-Za-z0-9]{8,14}",
        host in "[a-z]{3,10}\\.example",
        bidi_noise in "[a-z \u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}]{0,24}",
    ) {
        let secret = format!("gtsec_{secret_body}");
        let raw = format!(
            "fatal: unable to access 'https://{user}:{secret}@{host}/repo.git?token={secret}' {bidi_noise}"
        );
        let redacted = redact_diagnostic(&raw);
        let stripped = strip_bidi_controls(&redacted);
        prop_assert!(!stripped.contains(&secret), "secret leaked in {stripped:?}");
        prop_assert!(!has_bidi_control(&stripped), "bidi control survived in {stripped:?}");
    }
}
