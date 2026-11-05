//! Where text that ends (or starts) in an ellipsis is cut: between extended
//! grapheme clusters, never inside one ([design](../../../DESIGN.md#one-responsive-order)).
//! A letter keeps its combining marks, and an emoji with modifiers or a ZWJ
//! sequence and a regional-indicator pair (a flag) stay whole on one side of
//! the cut: cut between characters, "é" written as "e" and U+0301 lost its
//! accent, "👩🏽‍💻" its laptop, and a flag read as two letters. The bound a
//! cut keeps within, in bytes or characters, still holds: a cut keeps the
//! longest prefix of whole clusters within it. Only where one cluster runs
//! past the bound, so no whole cluster fits within it, as a letter with
//! fifty thousand combining marks in a report does, is the cut made at a
//! character boundary instead, keeping the bound's text rather than an
//! ellipsis alone.
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation as _};

/// The last extended grapheme-cluster boundary of `text` at or before byte
/// `at`: the end of its longest prefix of whole clusters within `at` bytes,
/// or, where the first cluster runs past `at`, the last character boundary
/// there. It walks back from `at` over the cluster `at` falls in and,
/// inside a run of regional indicators, over the whole run before `at`, to
/// count its pairs: never more than `at` bytes, and for most text a few,
/// rather than segmenting the whole prefix from its start, since an
/// accessible text cut from a long report is cut in every frame drawing it.
pub(crate) fn floor_cluster(text: &str, at: usize) -> usize {
    if at >= text.len() {
        return text.len();
    }
    let at = text.floor_char_boundary(at);
    // The cursor's chunk is the whole text, so it never asks for context.
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    match cursor.is_boundary(text, 0) {
        Ok(true) => at,
        _ => cursor
            .prev_boundary(text, 0)
            .ok()
            .flatten()
            .filter(|&boundary| boundary > 0)
            .unwrap_or(at),
    }
}

/// The first extended grapheme-cluster boundary of `text` at or after byte
/// `at`: where its longest suffix of whole clusters that starts at or after
/// `at` begins, or, where the last cluster starts before `at`, the first
/// character boundary there.
pub(crate) fn ceil_cluster(text: &str, at: usize) -> usize {
    let at = text.ceil_char_boundary(at.min(text.len()));
    if at == text.len() {
        return at;
    }
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    match cursor.is_boundary(text, 0) {
        Ok(true) => at,
        _ => cursor
            .next_boundary(text, 0)
            .ok()
            .flatten()
            .filter(|&boundary| boundary < text.len())
            .unwrap_or(at),
    }
}

/// The byte end of `text`'s longest prefix of whole clusters within its
/// first `characters` characters, or `None` where `text` has no more
/// characters than that and is kept whole.
pub(crate) fn within_characters(text: &str, characters: usize) -> Option<usize> {
    text.char_indices()
        .nth(characters)
        .map(|(end, _)| floor_cluster(text, end))
}

/// `text` less the whole clusters it ends in whose every character
/// `trimmed` accepts, such as the spaces and a clause's punctuation an
/// ellipsis stands for. A cluster that merely ends in such a character, as
/// a prepended mark and the space after it do, stays whole.
pub(crate) fn trim_end_clusters(text: &str, trimmed: impl Fn(char) -> bool) -> &str {
    let mut end = text.len();
    for (start, cluster) in text.grapheme_indices(true).rev() {
        if !cluster.chars().all(&trimmed) {
            break;
        }
        end = start;
    }
    &text[..end]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Clusters of several characters that a cut between characters split:
    /// a letter with two combining marks, a ZWJ emoji sequence with a skin
    /// tone, and a flag (two regional indicators).
    pub(crate) const CLUSTERS: [(&str, &str); 3] = [
        ("a combining mark", "e\u{301}\u{323}"),
        (
            "a ZWJ emoji sequence",
            "\u{1f469}\u{1f3fd}\u{200d}\u{1f4bb}",
        ),
        ("a flag", "\u{1f1ef}\u{1f1f5}"),
    ];

    /// Asserts that `kept` is a prefix of `text` that ends between whole
    /// clusters, for `case`.
    #[track_caller]
    pub(crate) fn assert_whole_prefix(text: &str, kept: &str, case: &str) {
        assert!(
            text.starts_with(kept),
            "{case}: {kept:?} is not a prefix of {text:?}"
        );
        assert!(
            kept.len() == text.len()
                || text
                    .grapheme_indices(true)
                    .any(|(start, _)| start == kept.len()),
            "{case}: {kept:?} ends inside a cluster of {text:?}"
        );
    }

    /// Asserts that `kept` is a suffix of `text` that starts between whole
    /// clusters, for `case`.
    #[track_caller]
    pub(crate) fn assert_whole_suffix(text: &str, kept: &str, case: &str) {
        assert!(
            text.ends_with(kept),
            "{case}: {kept:?} is not a suffix of {text:?}"
        );
        let start = text.len() - kept.len();
        assert!(
            text.grapheme_indices(true).any(|(at, _)| at == start) || start == text.len(),
            "{case}: {kept:?} starts inside a cluster of {text:?}"
        );
    }

    /// A bound that falls inside a cluster, at each of its inner bytes,
    /// moves to the cluster's start for a prefix and to its end for a suffix,
    /// and a bound between clusters stays where it is. In a run of flags the
    /// regional indicators pair from the run's start, so a bound inside the
    /// second flag moves to the end of the first.
    #[test]
    fn a_bound_inside_a_cluster_moves_to_its_edge() {
        let flags = "\u{1f1ef}\u{1f1f5}\u{1f1eb}\u{1f1f7}z";
        for inside in 9..16 {
            assert_eq!(
                floor_cluster(flags, inside),
                8,
                "byte {inside} of two flags"
            );
            assert_eq!(
                ceil_cluster(flags, inside),
                16,
                "byte {inside} of two flags"
            );
        }
        for (case, cluster) in CLUSTERS {
            let text = format!("ab{cluster}cd");
            for inside in 3..2 + cluster.len() {
                assert_eq!(floor_cluster(&text, inside), 2, "{case} at byte {inside}");
                assert_eq!(
                    ceil_cluster(&text, inside),
                    2 + cluster.len(),
                    "{case} at byte {inside}"
                );
            }
            for between in [0, 1, 2, 2 + cluster.len(), text.len()] {
                assert_eq!(floor_cluster(&text, between), between, "{case}");
                assert_eq!(ceil_cluster(&text, between), between, "{case}");
            }
            assert_eq!(floor_cluster(&text, text.len() + 5), text.len());
        }
    }

    /// A count of characters that ends inside a cluster keeps the clusters
    /// before it; a text no longer than the count is kept whole.
    #[test]
    fn a_count_of_characters_keeps_whole_clusters() {
        for (case, cluster) in CLUSTERS {
            let text = format!("ab{cluster}cd");
            for characters in 3..2 + cluster.chars().count() {
                let end = within_characters(&text, characters).expect("a cut");
                assert_eq!(end, 2, "{case}: {characters} characters");
                assert_whole_prefix(&text, &text[..end], case);
            }
            assert_eq!(within_characters(&text, text.chars().count()), None);
        }
    }

    /// One cluster longer than the bound, a letter with fifty thousand
    /// combining marks, is cut at a character boundary rather than left out
    /// whole, which left an accessible error, a prefix or a suffix nothing
    /// but the ellipsis; the bound still holds.
    #[test]
    fn a_cluster_longer_than_the_bound_is_cut_between_characters() {
        let marks = format!("e{}", "\u{301}".repeat(50_000));
        let text = format!("{marks}tail");
        for at in [1, 100, 101, 16 * 1024] {
            assert_eq!(
                floor_cluster(&text, at),
                text.floor_char_boundary(at),
                "at {at}"
            );
            assert!(floor_cluster(&text, at) > 0, "at {at}");
        }
        assert_eq!(
            within_characters(&text, 64),
            Some(text.floor_char_boundary(1 + 63 * 2))
        );
        let text = format!("head{marks}");
        for at in [5, 100, text.len() - 2] {
            assert_eq!(
                ceil_cluster(&text, at),
                text.ceil_char_boundary(at),
                "at {at}"
            );
            assert!(ceil_cluster(&text, at) < text.len(), "at {at}");
        }
        // Where a whole cluster fits, the cut is still between clusters.
        let text = format!("ab{marks}");
        assert_eq!(floor_cluster(&text, 100), 2);
        assert_eq!(ceil_cluster(&format!("{marks}cd"), 100), marks.len());
    }

    /// Trailing clusters made only of trimmed characters go; a cluster that
    /// ends in one but holds others, as an Arabic number sign (a prepended
    /// mark) and the full stop it joins do, stays whole.
    #[test]
    fn trimming_takes_whole_clusters() {
        let clause = |c: char| c.is_whitespace() || ".,;:".contains(c);
        assert_eq!(
            trim_end_clusters("Not decoded, \r\n", clause),
            "Not decoded"
        );
        assert_eq!(trim_end_clusters("e\u{301}.", clause), "e\u{301}");
        assert_eq!(
            trim_end_clusters("number \u{600}.", clause),
            "number \u{600}."
        );
        assert_eq!(trim_end_clusters("., ", clause), "");
    }
}
