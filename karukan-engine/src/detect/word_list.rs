// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の WordList.cs / DictionarySource の移植。

//! 単語集合とその全 prefix 集合。
//!
//! 判定は入力フックのスレッドで走るため、O(1) で引けるように prefix を
//! 事前展開しておく (Meltype の設計と同じ)。

use std::collections::{HashMap, HashSet};

/// 単語集合 + 全 prefix + 先頭文字インデックス。
#[derive(Debug, Default)]
pub struct WordList {
    words: HashSet<String>,
    prefixes: HashSet<String>,
    by_first: HashMap<char, Vec<String>>,
}

impl WordList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, word: &str) {
        if word.is_empty() || !self.words.insert(word.to_string()) {
            return;
        }
        let chars: Vec<char> = word.chars().collect();
        for i in 1..=chars.len() {
            self.prefixes.insert(chars[..i].iter().collect());
        }
        if let Some(first) = chars.first() {
            self.by_first.entry(*first).or_default().push(word.to_string());
        }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn contains_word(&self, text: &str) -> bool {
        self.words.contains(text)
    }

    /// text で始まる単語があるか (text 自体が単語の場合も true)。
    pub fn has_prefix(&self, text: &str) -> bool {
        self.prefixes.contains(text)
    }

    /// 指定の先頭文字を持つ単語の一覧 (TypoDetector の計算量削減用)。
    pub fn words_starting_with(&self, first: char) -> &[String] {
        self.by_first.get(&first).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn words(&self) -> impl Iterator<Item = &String> {
        self.words.iter()
    }
}

/// 空白・改行・タブ・カンマ区切り。# 以降はコメント。
/// 英小文字 (a-z) 以外を含む語は無視する (Meltype と同じ)。
pub fn parse_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let line = match raw_line.find('#') {
            Some(pos) => &raw_line[..pos],
            None => raw_line,
        };
        for token in line.split([' ', '\t', '\r', ',']).filter(|t| !t.is_empty()) {
            let word = token.trim().to_lowercase();
            if !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase()) {
                out.push(word);
            }
        }
    }
    out
}

/// 組み込み辞書 (src/detect/data/*.txt) とユーザー辞書を読む。
pub struct DictionarySource;

impl DictionarySource {
    /// 組み込み辞書を読み込む。
    pub fn read_embedded(name: &str) -> &'static str {
        match name {
            "english.txt" => include_str!("data/english.txt"),
            "japanese.txt" => include_str!("data/japanese.txt"),
            "propernouns.txt" => include_str!("data/propernouns.txt"),
            "english-words.txt" => include_str!("data/english-words.txt"),
            _ => "",
        }
    }

    /// 組み込み辞書 + ユーザー辞書 (あれば) の語を列挙する。
    pub fn load(name: &str, user_directory: Option<&std::path::Path>) -> Vec<String> {
        let mut out = parse_words(Self::read_embedded(name));
        if let Some(dir) = user_directory {
            let user_file = dir.join(name);
            if let Ok(text) = std::fs::read_to_string(&user_file) {
                out.extend(parse_words(&text));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordlist_prefix_works() {
        let mut wl = WordList::new();
        wl.add("github");
        wl.add("git");
        assert!(wl.contains_word("github"));
        assert!(wl.contains_word("git"));
        assert!(!wl.contains_word("githu"));
        assert!(wl.has_prefix("githu"));
        assert!(wl.has_prefix("github"));
        assert!(!wl.has_prefix("githubb"));
        assert_eq!(wl.words_starting_with('g').len(), 2);
    }

    #[test]
    fn parse_words_strips_comments_and_keeps_ascii() {
        let words = parse_words("github gitlab # コメント\nreact, vue\n日本語NG gmail123\n");
        assert_eq!(words, vec!["github", "gitlab", "react", "vue"]);
    }
}
