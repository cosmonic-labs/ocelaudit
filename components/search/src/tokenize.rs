use unicode_normalization::UnicodeNormalization;

/// Normalize a single string: NFKC → strip combining marks (accent fold) →
/// lowercase ASCII. Returns the cleaned string.
pub fn normalize(input: &str) -> String {
    input
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// Tokenize a normalized string: split on anything that isn't an ASCII
/// alphanumeric (after normalize, accents are gone). Filter empty tokens.
pub fn tokenize(input: &str) -> Vec<String> {
    let normalized = normalize(input);
    normalized
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// Trigrams over a normalized string with whitespace replaced by `_` and
/// `^`/`$` anchors. e.g. "abc def" → "^ab", "abc", "bc_", "c_d", "_de",
/// "def", "ef$".
pub fn trigrams(input: &str) -> Vec<String> {
    let normalized = normalize(input);
    let collapsed: String = normalized
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if collapsed.is_empty() {
        return Vec::new();
    }
    let padded = format!("^{}$", collapsed);
    let chars: Vec<char> = padded.chars().collect();
    if chars.len() < 3 {
        return Vec::new();
    }
    chars
        .windows(3)
        .map(|w| w.iter().collect::<String>())
        .collect()
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // Extended
        | 0x1DC0..=0x1DFF // Supplement
        | 0x20D0..=0x20FF // for Symbols
        | 0xFE20..=0xFE2F // Half Marks
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases() {
        assert_eq!(normalize("ABC"), "abc");
    }

    #[test]
    fn strips_diacritics() {
        assert_eq!(normalize("Citroën"), "citroen");
        assert_eq!(normalize("Pyongyang"), "pyongyang");
        assert_eq!(normalize("Müller"), "muller");
    }

    #[test]
    fn nfkc_folds_compatibility() {
        // Full-width digits → ASCII digits via NFKC step in normalize.
        assert_eq!(normalize("０１２"), "012");
    }

    #[test]
    fn tokenize_drops_punctuation() {
        let toks = tokenize("Acme Corp., Inc. (Pyongyang)");
        assert_eq!(toks, vec!["acme", "corp", "inc", "pyongyang"]);
    }

    #[test]
    fn tokenize_handles_aliases() {
        assert_eq!(tokenize(""), Vec::<String>::new());
        assert_eq!(tokenize("   "), Vec::<String>::new());
    }

    #[test]
    fn trigrams_anchored() {
        let g = trigrams("abc");
        assert_eq!(g, vec!["^ab", "abc", "bc$"]);
    }

    #[test]
    fn trigrams_collapse_whitespace_to_underscore() {
        let g = trigrams("ab c");
        assert_eq!(g, vec!["^ab", "ab_", "b_c", "_c$"]);
    }
}
