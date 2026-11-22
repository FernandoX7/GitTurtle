//! Zero-allocation ASCII-fast-path case-insensitive substring matching with
//! full Unicode `str::to_lowercase()` equivalence.

/// Above this needle length, switch ASCII case-insensitive search from naive
/// scanning to linear-time KMP so adversarial long queries cannot go quadratic.
const KMP_THRESHOLD: usize = 32;

/// Precomputed literal case-insensitive substring matcher.
///
/// Mirrors `haystack.to_lowercase().contains(&needle.to_lowercase())` while
/// avoiding per-item heap allocations on the common ASCII and non-sigma
/// Unicode paths.
#[derive(Clone, Debug)]
pub(crate) struct CaseInsensitiveMatcher {
    needle_lower: String,
    needle_is_ascii: bool,
    needle_has_unicode_ascii_fold: bool,
    kmp_lps: Vec<usize>,
}

impl CaseInsensitiveMatcher {
    pub(crate) fn new(query: &str) -> Self {
        let needle_lower = query.to_lowercase();
        let needle_is_ascii = needle_lower.is_ascii();
        // Only U+0130 ('İ' -> "i\u{307}") and U+212A ('K' -> "k") are non-ASCII
        // characters whose Unicode lowercase starts with or contains ASCII 'i'/'k'.
        let needle_has_unicode_ascii_fold = needle_is_ascii
            && needle_lower
                .as_bytes()
                .iter()
                .any(|&b| b == b'i' || b == b'k');
        let kmp_lps = if needle_is_ascii && needle_lower.len() > KMP_THRESHOLD {
            build_kmp_lps(needle_lower.as_bytes())
        } else {
            Vec::new()
        };
        Self {
            needle_lower,
            needle_is_ascii,
            needle_has_unicode_ascii_fold,
            kmp_lps,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.needle_lower.is_empty()
    }

    /// Matches against raw bytes (valid UTF-8 or arbitrary OS path bytes,
    /// equivalent to `String::from_utf8_lossy(raw).to_lowercase().contains(...)`).
    pub(crate) fn matches_bytes(&self, raw: &[u8], scratch: &mut String) -> bool {
        if self.needle_lower.is_empty() {
            return true;
        }
        if self.needle_is_ascii {
            let needle = self.needle_lower.as_bytes();
            let matched = if self.kmp_lps.is_empty() {
                ascii_contains_ignore_case(raw, needle)
            } else {
                ascii_contains_ignore_case_kmp(raw, needle, &self.kmp_lps)
            };
            if matched {
                return true;
            }
            if !self.needle_has_unicode_ascii_fold
                || raw.is_ascii()
                || !contains_special_ascii_fold_utf8(raw)
            {
                return false;
            }
        } else if raw.is_ascii() {
            return false;
        }
        match std::str::from_utf8(raw) {
            Ok(text) => self.matches_unicode_str(text, scratch),
            Err(_) => {
                let lossy = String::from_utf8_lossy(raw);
                self.matches_unicode_str(&lossy, scratch)
            }
        }
    }

    /// Matches against a valid UTF-8 `&str` without allocating on the common path.
    #[cfg(test)]
    pub(crate) fn matches_str(&self, text: &str, scratch: &mut String) -> bool {
        self.matches_bytes(text.as_bytes(), scratch)
    }

    fn matches_unicode_str(&self, text: &str, scratch: &mut String) -> bool {
        // Greek capital sigma ('Σ') has context-sensitive final-sigma lowering
        // in `str::to_lowercase()`; delegate only when present so the fast
        // per-char lowercase into `scratch` matches `str::to_lowercase()` exactly.
        if text.contains('\u{03A3}') {
            return text.to_lowercase().contains(&self.needle_lower);
        }
        scratch.clear();
        for ch in text.chars() {
            for lower in ch.to_lowercase() {
                scratch.push(lower);
            }
        }
        scratch.contains(&self.needle_lower)
    }
}

/// Checks whether `haystack` contains `needle_lower` (guaranteed non-empty ASCII
/// lowercase bytes) ignoring ASCII case. Safe on arbitrary UTF-8 because UTF-8
/// continuation and lead bytes are always `>= 0x80`.
fn ascii_contains_ignore_case(haystack: &[u8], needle_lower: &[u8]) -> bool {
    let Some((&first, rest)) = needle_lower.split_first() else {
        return true;
    };
    let n = needle_lower.len();
    if haystack.len() < n {
        return false;
    }
    let last_start = haystack.len() - n;
    if first.is_ascii_lowercase() {
        for i in 0..=last_start {
            if (haystack[i] | 0x20) == first && haystack[i + 1..i + n].eq_ignore_ascii_case(rest) {
                return true;
            }
        }
    } else {
        for i in 0..=last_start {
            if haystack[i] == first && haystack[i + 1..i + n].eq_ignore_ascii_case(rest) {
                return true;
            }
        }
    }
    false
}

fn build_kmp_lps(needle_lower: &[u8]) -> Vec<usize> {
    let mut lps = vec![0usize; needle_lower.len()];
    let mut len = 0usize;
    let mut i = 1usize;
    while i < needle_lower.len() {
        if needle_lower[i] == needle_lower[len] {
            len += 1;
            lps[i] = len;
            i += 1;
        } else if len != 0 {
            len = lps[len - 1];
        } else {
            lps[i] = 0;
            i += 1;
        }
    }
    lps
}

fn ascii_contains_ignore_case_kmp(haystack: &[u8], needle_lower: &[u8], lps: &[usize]) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.len() < needle_lower.len() {
        return false;
    }
    let mut j = 0usize;
    for &byte in haystack {
        let lower = byte.to_ascii_lowercase();
        while j > 0 && lower != needle_lower[j] {
            j = lps[j - 1];
        }
        if lower == needle_lower[j] {
            j += 1;
            if j == needle_lower.len() {
                return true;
            }
        }
    }
    false
}

/// True if `raw` contains the UTF-8 encoding of U+0130 (`İ`: `C4 B0`) or
/// U+212A (`K`: `E2 84 AA`), the only non-ASCII scalars whose `to_lowercase()`
/// contains ASCII `'i'` or `'k'`.
fn contains_special_ascii_fold_utf8(raw: &[u8]) -> bool {
    raw.windows(2).any(|w| w == [0xC4, 0xB0]) || raw.windows(3).any(|w| w == [0xE2, 0x84, 0xAA])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ascii_unicode_special_cases_and_non_utf8() {
        let mut scratch = String::new();
        // ASCII fast path
        let m = CaseInsensitiveMatcher::new("Hello");
        assert!(m.matches_str("say HELLO world", &mut scratch));
        assert!(!m.matches_str("say world", &mut scratch));
        assert!(m.matches_bytes(b"prefix-\xff-HeLlO-suffix", &mut scratch));

        // Long ASCII needle exercises linear-time KMP path (> KMP_THRESHOLD bytes)
        let long_needle = format!("{}B", "a".repeat(KMP_THRESHOLD + 8));
        let long_matcher = CaseInsensitiveMatcher::new(&long_needle);
        assert!(!long_matcher.kmp_lps.is_empty());
        let matching_haystack = format!("{}b", "A".repeat(KMP_THRESHOLD + 64));
        let non_matching_haystack = "A".repeat(KMP_THRESHOLD + 64);
        assert!(long_matcher.matches_str(&matching_haystack, &mut scratch));
        assert!(!long_matcher.matches_str(&non_matching_haystack, &mut scratch));

        // Kelvin sign U+212A ('K') folds to ASCII 'k' in both directions
        let kelvin = CaseInsensitiveMatcher::new("Kelvin");
        assert!(kelvin.matches_str("kelvin scale", &mut scratch));
        let ascii_k = CaseInsensitiveMatcher::new("kelvin");
        assert!(ascii_k.matches_str("Kelvin scale", &mut scratch));

        // Turkish dotted capital I (U+0130 'İ' -> "i\u{307}")
        let dotted = CaseInsensitiveMatcher::new("i");
        assert!(dotted.matches_str("İstanbul", &mut scratch));

        // Non-ASCII accented characters
        let cafe = CaseInsensitiveMatcher::new("CAFÉ");
        assert!(cafe.matches_str("café au lait", &mut scratch));
        assert!(!cafe.matches_str("cafe au lait", &mut scratch));

        // Greek final sigma context sensitivity ('ΟΔΥΣΣΕΥΣ' -> 'οδυσσευς')
        let final_sigma = CaseInsensitiveMatcher::new("ς");
        assert!(final_sigma.matches_str("ΟΔΥΣΣΕΥΣ", &mut scratch));
        assert!(!final_sigma.matches_str("ΣΑ", &mut scratch));

        // Empty query always matches
        let empty = CaseInsensitiveMatcher::new("");
        assert!(empty.is_empty());
        assert!(empty.matches_bytes(b"", &mut scratch));
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 128,
            failure_persistence: None,
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x5A17_F01D_0001),
            ..proptest::prelude::ProptestConfig::default()
        })]

        #[test]
        fn proptest_matcher_equals_to_lowercase_contains_for_unicode_and_bytes(
            query in "[a-zA-Z0-9 _./\\-éÉüÜİKΣσςß]{0,12}",
            haystack in "[a-zA-Z0-9 _./\\-éÉüÜİKΣσςß]{0,64}",
        ) {
            let matcher = CaseInsensitiveMatcher::new(&query);
            let mut scratch = String::new();
            let actual = matcher.matches_str(&haystack, &mut scratch);
            let expected = haystack.to_lowercase().contains(&query.to_lowercase());
            proptest::prop_assert_eq!(actual, expected, "query={:?} haystack={:?}", query, haystack);
        }
    }
}
