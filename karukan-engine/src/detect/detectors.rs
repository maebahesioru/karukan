// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の
// DictionaryDetector.cs / EnglishDetector.cs / ProperNouns.cs / TypoDetector.cs の移植。

//! 各判定器。いずれも直接 Verdict を決めず、Contribution (加点) を出すだけ。

use super::{Contribution, WordList};

/// 日本語辞書 (ローマ字見出し) との前方一致。
/// 見出し語から綴りの揺れ (shi/si, tsu/tu, chi/ti, ん の n/nn …) を展開して登録する。
pub struct DictionaryDetector {
    pub words: WordList,
}

impl DictionaryDetector {
    pub fn new(canonical_words: &[String]) -> Self {
        let mut words = WordList::new();
        for word in canonical_words {
            for variant in spelling_variants(word) {
                words.add(&variant);
            }
        }
        Self { words }
    }

    pub fn is_prefix(&self, letters: &str) -> bool {
        self.words.has_prefix(letters)
    }

    pub fn evaluate(&self, letters: &str, output: &mut Vec<Contribution>) {
        if letters.chars().count() < 3 {
            return;
        }
        if self.words.contains_word(letters) {
            output.push(Contribution::new("Dictionary", 5, 0, "日本語辞書の語と一致"));
        } else if self.words.has_prefix(letters) {
            let score = if letters.chars().count() >= 4 { 4 } else { 3 };
            output.push(Contribution::new("Dictionary", score, 0, "日本語辞書の語の先頭と一致"));
        } else if let Some(particle) = starts_with_particle(letters) {
            let rest = &letters[particle.len()..];
            if rest.chars().count() >= 4 && self.words.has_prefix(rest) {
                output.push(Contribution::new(
                    "Dictionary",
                    4,
                    0,
                    format!("助詞「{particle}」+ 日本語辞書の語"),
                ));
            }
        }
    }
}

const PARTICLES: [&str; 10] = ["no", "ga", "wo", "ni", "de", "to", "ha", "mo", "wa", "he"];

/// 助詞 (の が を に で と は も わ へ) で始まっていれば、その助詞。
pub fn starts_with_particle(letters: &str) -> Option<&'static str> {
    PARTICLES.iter().find(|p| letters.starts_with(**p)).copied()
}

/// 明らかな英語・技術用語を検出する。
/// 辞書には「ローマ字としても読める英単語」と日本語由来の外来語 (sushi, kana …) を重点的に入れてある。
pub struct EnglishDetector {
    pub words: WordList,
}

impl EnglishDetector {
    pub fn new(words: impl IntoIterator<Item = String>) -> Self {
        let mut list = WordList::new();
        for word in words {
            list.add(&word);
        }
        Self { words: list }
    }

    pub fn is_prefix(&self, letters: &str) -> bool {
        letters.chars().count() >= 2 && self.words.has_prefix(letters)
    }

    pub fn evaluate(&self, letters: &str, output: &mut Vec<Contribution>) {
        if letters.chars().count() < 2 {
            return;
        }
        if self.words.contains_word(letters) {
            output.push(Contribution::new("English", 0, 4, "英語辞書の語と一致"));
        } else if self.words.has_prefix(letters) {
            output.push(Contribution::new("English", 0, 3, "英語辞書の語の先頭と一致"));
        }
    }
}

/// 英語の固有名詞 (propernouns.txt)。小文字で引き、正しい大文字小文字の形 (GitHub, iPhone) を返す。
pub struct ProperNouns {
    canonical: std::collections::HashMap<String, String>,
    words: WordList,
}

impl ProperNouns {
    pub fn load(user_directory: Option<&std::path::Path>) -> Self {
        let mut nouns = Self {
            canonical: std::collections::HashMap::new(),
            words: WordList::new(),
        };
        nouns.add_text(super::DictionarySource::read_embedded("propernouns.txt"));
        if let Some(dir) = user_directory {
            if let Ok(text) = std::fs::read_to_string(dir.join("propernouns.txt")) {
                nouns.add_text(&text);
            }
        }
        nouns
    }

    pub fn add_text(&mut self, text: &str) {
        for raw_line in text.lines() {
            let line = match raw_line.find('#') {
                Some(pos) => &raw_line[..pos],
                None => raw_line,
            };
            for word in line.split([' ', '\t', '\r', ',']).filter(|w| !w.is_empty()) {
                if !word.chars().all(|c| c.is_ascii_alphanumeric()) {
                    continue;
                }
                let lower = word.to_lowercase();
                self.canonical.entry(lower.clone()).or_insert_with(|| word.to_string());
                self.words.add(&lower);
            }
        }
    }

    /// 小文字の綴り (英語辞書に足す用)。
    pub fn lowercase_words(&self) -> impl Iterator<Item = &String> {
        self.canonical.keys()
    }

    pub fn contains(&self, lower: &str) -> bool {
        self.canonical.contains_key(lower)
    }

    pub fn has_prefix(&self, lower: &str) -> bool {
        self.words.has_prefix(lower)
    }

    /// 正しい大文字小文字の形。固有名詞でなければ None。
    pub fn canonical(&self, lower: &str) -> Option<&str> {
        self.canonical.get(lower).map(|s| s.as_str())
    }
}

/// 日本語辞書の語との編集距離で打ち間違いを拾う。補助的な加点のみ。
pub struct TypoDetector<'a> {
    japanese_words: &'a WordList,
}

impl<'a> TypoDetector<'a> {
    pub const MIN_LENGTH: usize = 5;

    pub fn new(japanese_words: &'a WordList) -> Self {
        Self { japanese_words }
    }

    pub fn evaluate(&self, letters: &str, output: &mut Vec<Contribution>) {
        let n = letters.chars().count();
        if n < Self::MIN_LENGTH || self.japanese_words.has_prefix(letters) {
            return;
        }
        let Some(first) = letters.chars().next() else { return };

        // 先頭文字の打ち間違いはまれなので、同じ先頭文字の語だけを比べる。
        let mut best: Option<&String> = None;
        'outer: for word in self.japanese_words.words_starting_with(first) {
            let wlen = word.chars().count();
            for length in (n - 1)..=(n + 1) {
                if length > wlen || length < 2 {
                    continue;
                }
                let prefix: String = word.chars().take(length).collect();
                if levenshtein(letters, &prefix, 1) <= 1 {
                    best = Some(word);
                    break 'outer;
                }
            }
        }
        if let Some(word) = best {
            output.push(Contribution::new("Typo", 3, 0, format!("「{word}」の打ち間違いに近い")));
        }
    }
}

/// 上限 max を超えたら打ち切る Levenshtein 距離。
pub fn levenshtein(a: &str, b: &str, max: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        current[0] = i;
        let mut row_min = current[0];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            current[j] = (previous[j] + 1).min(current[j - 1] + 1).min(previous[j - 1] + cost);
            row_min = row_min.min(current[j]);
        }
        if row_min > max {
            return max + 1;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// 見出し語から綴りの揺れ (ヘボン式 ⇄ 訓令式) を展開する。
///
/// TODO(Phase 2): Meltype の RomajiDetector.SpellingVariants と完全一致させる。
/// 現在は主要な対 (shi/si, chi/ti, tsu/tu, fu/hu, ji/zi, sha/sya …) のみ。
pub fn spelling_variants(word: &str) -> Vec<String> {
    const PAIRS: [(&str, &str); 12] = [
        ("sha", "sya"),
        ("shu", "syu"),
        ("sho", "syo"),
        ("she", "sye"),
        ("shi", "si"),
        ("cha", "tya"),
        ("chu", "tyu"),
        ("cho", "tyo"),
        ("che", "tye"),
        ("chi", "ti"),
        ("tsu", "tu"),
        ("fu", "hu"),
    ];
    let mut variants = vec![word.to_string()];
    for (a, b) in PAIRS {
        let mut next = Vec::new();
        for v in &variants {
            if v.contains(a) {
                next.push(v.replace(a, b));
            }
            if v.contains(b) {
                next.push(v.replace(b, a));
            }
        }
        variants.extend(next);
    }
    variants.sort();
    variants.dedup();
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_detector_matches_and_prefixes() {
        let words = vec!["konnichiwa".to_string(), "tasuku".to_string()];
        let det = DictionaryDetector::new(&words);
        let mut out = Vec::new();
        det.evaluate("konnichiwa", &mut out);
        assert!(out.iter().any(|c| c.japanese == 5));
        let mut out2 = Vec::new();
        det.evaluate("konn", &mut out2);
        assert!(out2.iter().any(|c| c.japanese >= 3));
    }

    #[test]
    fn spelling_variants_expand() {
        let v = spelling_variants("shi");
        assert!(v.contains(&"si".to_string()));
        let v2 = spelling_variants("konshi");
        assert!(v2.iter().any(|w| w == "konsi"));
    }

    #[test]
    fn english_detector_scores() {
        let det = EnglishDetector::new(vec!["github".to_string()]);
        let mut out = Vec::new();
        det.evaluate("github", &mut out);
        assert!(out.iter().any(|c| c.english == 4));
    }

    #[test]
    fn typo_detector_finds_close_word() {
        let words = vec!["konnichiwa".to_string()];
        let dict = DictionaryDetector::new(&words);
        let typo = TypoDetector::new(&dict.words);
        let mut out = Vec::new();
        typo.evaluate("konnichiwx", &mut out);
        assert!(out.iter().any(|c| c.source == "Typo"));
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("kitten", "sitting", 10), 3);
        assert_eq!(levenshtein("abc", "abc", 1), 0);
        assert_eq!(levenshtein("abc", "xyz", 1), 2); // max+1 で打ち切り
    }
}
