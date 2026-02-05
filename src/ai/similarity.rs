//! Text similarity for memory and skill deduplication
//!
//! Uses hybrid tokenization (Latin words + CJK character bigrams) with
//! Jaccard similarity to detect near-duplicate memories and skills.

use std::collections::HashSet;

/// Similarity threshold for memory dedup (title 0.6 + content 0.4 weighted)
pub const MEMORY_SIMILARITY_THRESHOLD: f64 = 0.65;

/// Similarity threshold for skill dedup (name 0.6 + description 0.4 weighted)
pub const SKILL_SIMILARITY_THRESHOLD: f64 = 0.70;

/// Check if a character is in CJK Unicode ranges
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Unified Ideographs Extension A
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
    )
}

/// Simple English suffix stemming for better matching
/// Strips common suffixes: -ing, -ed, -s, -es, -tion, -ly
fn simple_stem(word: &str) -> String {
    let w = word;
    if w.len() > 5 && w.ends_with("ting") {
        // "reviewing" → "review" (strip "ing"), but "ting" stays
        return w[..w.len() - 3].to_string();
    }
    if w.len() > 4 && w.ends_with("ing") {
        return w[..w.len() - 3].to_string();
    }
    if w.len() > 3 && w.ends_with("ed") {
        return w[..w.len() - 2].to_string();
    }
    if w.len() > 3 && w.ends_with("es") {
        return w[..w.len() - 2].to_string();
    }
    if w.len() > 3 && w.ends_with('s') {
        return w[..w.len() - 1].to_string();
    }
    if w.len() > 4 && w.ends_with("ly") {
        return w[..w.len() - 2].to_string();
    }
    w.to_string()
}

/// Hybrid tokenizer: Latin words + CJK character bigrams
///
/// - Latin/alphanumeric: split into lowercase word tokens (min 2 chars), with simple stemming
/// - CJK characters: sliding window of 2 characters (bigrams)
/// - Mixed text: combines both approaches
///
/// Example: "UTF-8边界崩溃" → ["utf", "边界", "界崩", "崩溃"]
pub fn tokenize(text: &str) -> Vec<String> {
    let text = text.to_lowercase();
    let mut tokens = Vec::new();
    let mut word_buf = String::new();
    let mut cjk_chars: Vec<char> = Vec::new();

    let flush_word = |buf: &mut String, tokens: &mut Vec<String>| {
        if buf.len() >= 2 {
            tokens.push(simple_stem(buf));
        }
        buf.clear();
    };

    let flush_cjk = |chars: &mut Vec<char>, tokens: &mut Vec<String>| {
        // Generate bigrams from collected CJK characters
        if chars.len() == 1 {
            tokens.push(chars[0].to_string());
        } else {
            for window in chars.windows(2) {
                tokens.push(window.iter().collect());
            }
        }
        chars.clear();
    };

    for c in text.chars() {
        if is_cjk(c) {
            flush_word(&mut word_buf, &mut tokens);
            cjk_chars.push(c);
        } else if c.is_alphanumeric() {
            if !cjk_chars.is_empty() {
                flush_cjk(&mut cjk_chars, &mut tokens);
            }
            word_buf.push(c);
        } else {
            // Separator (space, punctuation, etc.)
            flush_word(&mut word_buf, &mut tokens);
            if !cjk_chars.is_empty() {
                flush_cjk(&mut cjk_chars, &mut tokens);
            }
        }
    }

    // Flush remaining
    flush_word(&mut word_buf, &mut tokens);
    if !cjk_chars.is_empty() {
        flush_cjk(&mut cjk_chars, &mut tokens);
    }

    tokens
}

/// Jaccard similarity: |intersection| / |union| of token sets
pub fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let set_a: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

/// Combined similarity with weighted title (0.6) and content (0.4)
pub fn combined_similarity(
    title_a: &str,
    content_a: &str,
    title_b: &str,
    content_b: &str,
) -> f64 {
    let title_tokens_a = tokenize(title_a);
    let title_tokens_b = tokenize(title_b);
    let content_tokens_a = tokenize(content_a);
    let content_tokens_b = tokenize(content_b);

    let title_sim = jaccard_similarity(&title_tokens_a, &title_tokens_b);
    let content_sim = jaccard_similarity(&content_tokens_a, &content_tokens_b);

    0.6 * title_sim + 0.4 * content_sim
}

/// Check if a new memory is similar to an existing one
pub fn is_similar_memory(
    new_title: &str,
    new_content: &str,
    existing_title: &str,
    existing_content: &str,
    threshold: f64,
) -> bool {
    combined_similarity(new_title, new_content, existing_title, existing_content) >= threshold
}

/// Check if a new skill is similar to an existing one
/// Uses 0.3 name + 0.7 description weighting (names are short, descriptions carry more signal)
pub fn is_similar_skill(
    new_name: &str,
    new_desc: &str,
    existing_name: &str,
    existing_desc: &str,
    threshold: f64,
) -> bool {
    let name_tokens_a = tokenize(new_name);
    let name_tokens_b = tokenize(existing_name);
    let desc_tokens_a = tokenize(new_desc);
    let desc_tokens_b = tokenize(existing_desc);

    let name_sim = jaccard_similarity(&name_tokens_a, &name_tokens_b);
    let desc_sim = jaccard_similarity(&desc_tokens_a, &desc_tokens_b);

    (0.3 * name_sim + 0.7 * desc_sim) >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_latin() {
        let tokens = tokenize("UTF-8 boundary panic in Rust");
        // "in" dropped (< 2 chars), words are stemmed
        assert_eq!(tokens, vec!["utf", "boundary", "panic", "in", "rust"]);
    }

    #[test]
    fn test_stemming() {
        assert_eq!(simple_stem("reviewing"), "review");
        assert_eq!(simple_stem("reviews"), "review");
        assert_eq!(simple_stem("requests"), "request");
        assert_eq!(simple_stem("changes"), "chang");
        assert_eq!(simple_stem("configured"), "configur");
    }

    #[test]
    fn test_tokenize_cjk() {
        let tokens = tokenize("数据库连接池");
        // Bigrams: 数据, 据库, 库连, 连接, 接池
        assert_eq!(tokens, vec!["数据", "据库", "库连", "连接", "接池"]);
    }

    #[test]
    fn test_tokenize_mixed() {
        let tokens = tokenize("UTF-8边界崩溃");
        assert_eq!(tokens, vec!["utf", "边界", "界崩", "崩溃"]);
    }

    #[test]
    fn test_tokenize_single_cjk_char() {
        let tokens = tokenize("是");
        assert_eq!(tokens, vec!["是"]);
    }

    #[test]
    fn test_jaccard_identical() {
        let a = tokenize("hello world");
        let b = tokenize("hello world");
        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_jaccard_completely_different() {
        let a = tokenize("hello world");
        let b = tokenize("foo bar baz");
        assert!((jaccard_similarity(&a, &b)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_latin_near_duplicate() {
        let sim = combined_similarity(
            "UTF-8 boundary panic in Rust",
            "String slicing by byte index panics when the index falls inside a multi-byte UTF-8 character",
            "UTF-8 boundary causes panic in Rust string slicing",
            "String slicing by byte index panics when index falls inside multi-byte UTF-8 character boundary",
        );
        assert!(sim >= MEMORY_SIMILARITY_THRESHOLD, "similarity {sim} should be >= {MEMORY_SIMILARITY_THRESHOLD}");
    }

    #[test]
    fn test_cjk_near_duplicate() {
        let sim = combined_similarity(
            "数据库连接池配置",
            "配置数据库的连接池参数和超时设置",
            "配置数据库连接池",
            "设置数据库连接池的参数和超时配置",
        );
        assert!(sim >= MEMORY_SIMILARITY_THRESHOLD, "CJK similarity {sim} should be >= {MEMORY_SIMILARITY_THRESHOLD}");
    }

    #[test]
    fn test_completely_different_below_threshold() {
        let sim = combined_similarity(
            "UTF-8 boundary panic in Rust",
            "String slicing panics on multi-byte characters",
            "Database connection pool configuration",
            "Configure the maximum number of database connections",
        );
        assert!(sim < MEMORY_SIMILARITY_THRESHOLD, "similarity {sim} should be < {MEMORY_SIMILARITY_THRESHOLD}");
    }

    #[test]
    fn test_skill_near_duplicate() {
        let sim = combined_similarity(
            "reviewing-pull-requests",
            "Reviews code changes in pull requests for quality and correctness",
            "review-pull-requests",
            "Review pull request code changes for quality and correctness",
        );
        assert!(sim >= SKILL_SIMILARITY_THRESHOLD, "skill similarity {sim} should be >= {SKILL_SIMILARITY_THRESHOLD}");
    }

    #[test]
    fn test_empty_strings() {
        assert!((jaccard_similarity(&[], &[]) - 1.0).abs() < f64::EPSILON);
        assert!((jaccard_similarity(&tokenize("hello"), &[]) - 0.0).abs() < f64::EPSILON);
    }
}
