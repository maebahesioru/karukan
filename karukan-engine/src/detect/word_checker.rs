// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の BuiltInWordChecker.cs / IWordChecker.cs の移植。

//! 普通の英単語として正しい綴りかを調べるもの。
//!
//! Meltype では Windows のスペルチェッカー (ISpellChecker) を使い、無い環境
//! (Mac/Linux/テスト) では同梱の SCOWL 由来リスト (english-words.txt) で代用する。
//! この移植ではまず同梱リスト版のみ提供する (OS スペルチェッカー連携は将来課題)。

use super::DictionarySource;
use std::collections::HashSet;
use std::sync::OnceLock;

/// 英単語チェッカー。
pub trait WordChecker: Send + Sync {
    /// 使えるか (使えなければ常に false を返す)。
    fn is_available(&self) -> bool;

    /// 小文字の英単語 (a-z だけ) が、英語として正しい綴りか。
    fn is_word(&self, lower: &str) -> bool;

    /// よくある打ち間違い (teh → the) なら、自動修正の綴り。無ければ None。
    fn auto_correction(&self, _lower: &str) -> Option<String> {
        None
    }
}

/// 同梱のよく使う英単語の一覧 (english-words.txt、SCOWL から作成) で英単語かを調べる。
/// 打ち間違いの自動修正は無い。
#[derive(Clone)]
pub struct BuiltInWordChecker {
    words: HashSet<String>,
}

impl BuiltInWordChecker {
    /// 同梱リストを読んだ共有インスタンス (最初に使うときに読む)。
    pub fn shared() -> &'static BuiltInWordChecker {
        static SHARED: OnceLock<BuiltInWordChecker> = OnceLock::new();
        SHARED.get_or_init(|| Self::from_text(DictionarySource::read_embedded("english-words.txt")))
    }

    pub fn from_text(text: &str) -> Self {
        let words = text
            .lines()
            .map(|l| l.trim())
            .filter(|w| !w.is_empty() && !w.starts_with('#'))
            .map(|w| w.to_string())
            .collect();
        Self { words }
    }
}

impl WordChecker for BuiltInWordChecker {
    fn is_available(&self) -> bool {
        !self.words.is_empty()
    }

    fn is_word(&self, lower: &str) -> bool {
        self.words.contains(lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_checker_loads_and_checks() {
        let checker = BuiltInWordChecker::shared();
        assert!(checker.is_available());
        assert!(checker.is_word("hello"));
        assert!(checker.is_word("computer"));
        // github は一般語リストに無い (propernouns.txt 側の担当)
        assert!(!checker.is_word("github"));
        assert!(!checker.is_word("zxcvbnmasdfgh"));
    }
}
