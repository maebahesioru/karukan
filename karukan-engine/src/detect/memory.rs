// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の LanguageMemory.cs の移植。

//! 英語とも日本語とも読める語 (api, sushi …) を、ユーザーが自分で英字 / かなに直したときに覚えておく。
//! 次からその語は、自動の判定より覚えた方を優先する (api と打って F10 で英字にして確定 → 次から api は英字)。
//! languages.json に「打った英字 → 英語か」だけを保存する。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 学習エントリ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// 英語として覚えたか
    pub english: bool,
    /// 最後に使った時刻 (unix 秒)
    pub used: i64,
    /// 同じ向きに直した回数 (前の版で保存したものは 0 = 1 回)
    #[serde(default)]
    pub count: u32,
    /// F10 / F6 などで、はっきり英字 / かなに直したか
    #[serde(default)]
    pub explicit: bool,
}

/// 学習した語 (一覧表示用)。
#[derive(Debug, Clone)]
pub struct Learned {
    pub word: String,
    pub english: bool,
    pub count: u32,
    pub used: i64,
    /// 今効いているか (2 回目を待っている語は false)
    pub active: bool,
}

/// ユーザー修正の学習データ。
pub struct LanguageMemory {
    path: Option<PathBuf>,
    entries: HashMap<String, Entry>,
    /// 覚えている内容が変わるたびに増える (判定の結果を使い回してよいかを見るのに使う)
    version: u32,
    /// ローマ字としてよく使う日本語になる語か (kyouha = 今日は、sushi = すし)。
    pub is_common_japanese: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
    /// ローマ字として読み切れる語か (go = ご)。
    pub is_readable_romaji: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
}

const MAX_ENTRIES: usize = 3000;

impl LanguageMemory {
    pub fn new(path: Option<&Path>) -> Self {
        let mut memory = Self {
            path: path.map(|p| p.to_path_buf()),
            entries: HashMap::new(),
            version: 0,
            is_common_japanese: None,
            is_readable_romaji: None,
        };
        if let Some(p) = &memory.path {
            if p.exists() {
                if let Ok(text) = std::fs::read_to_string(p) {
                    if let Ok(loaded) = serde_json::from_str::<HashMap<String, Entry>>(&text) {
                        memory.entries = loaded;
                    }
                }
            }
        }
        memory
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// 英語として覚えるのに 2 回要る語か。
    fn needs_twice(&self, word: &str, entry: &Entry) -> bool {
        if !entry.english || entry.count.max(1) >= 2 {
            return false;
        }
        let common = self
            .is_common_japanese
            .as_ref()
            .map(|f| f(word))
            .unwrap_or(false);
        let short_readable = !entry.explicit
            && word.chars().count() <= 2
            && self
                .is_readable_romaji
                .as_ref()
                .map(|f| f(word))
                .unwrap_or(false);
        common || short_readable
    }

    /// 覚えている語なら英語か (true) 日本語か (false)。覚えていなければ None。word は小文字の英字。
    pub fn get(&self, word: &str) -> Option<bool> {
        let entry = self.entries.get(word)?;
        if self.needs_twice(word, entry) {
            return None;
        }
        Some(entry.english)
    }

    /// ユーザーが英字 / かなに直した語を覚える (2 文字以上の英字だけ)。
    /// explicit = F10 / F6 / Tab ではっきり直したとき。
    pub fn remember(&mut self, word: &str, english: bool, explicit: bool) {
        let word = word.to_lowercase();
        if word.chars().count() < 2 || !word.chars().all(|c| c.is_ascii_lowercase()) {
            return;
        }
        self.version += 1;
        let now = chrono::Utc::now().timestamp();
        match self.entries.get_mut(&word) {
            Some(old) if old.english == english => {
                old.used = now;
                old.count = old.count.max(1) + 1;
                old.explicit |= explicit;
            }
            _ => {
                self.entries.insert(
                    word,
                    Entry {
                        english,
                        used: now,
                        count: 1,
                        explicit,
                    },
                );
            }
        }
        if self.entries.len() > MAX_ENTRIES {
            let mut by_used: Vec<(String, i64)> =
                self.entries.iter().map(|(k, v)| (k.clone(), v.used)).collect();
            by_used.sort_by_key(|(_, used)| *used);
            let remove_count = self.entries.len() - MAX_ENTRIES * 9 / 10;
            for (key, _) in by_used.into_iter().take(remove_count) {
                self.entries.remove(&key);
            }
        }
        self.save();
    }

    /// 学習した語の一覧 (新しく使ったものから)。
    pub fn learned(&self) -> Vec<Learned> {
        let mut list: Vec<Learned> = self
            .entries
            .iter()
            .map(|(word, entry)| Learned {
                word: word.clone(),
                english: entry.english,
                count: entry.count.max(1),
                used: entry.used,
                active: !self.needs_twice(word, entry),
            })
            .collect();
        list.sort_by_key(|l| std::cmp::Reverse(l.used));
        list
    }

    /// 学習した語を忘れる。
    pub fn remove(&mut self, words: &[String]) {
        let mut removed = false;
        for word in words {
            removed |= self.entries.remove(word).is_some();
        }
        if removed {
            self.version += 1;
            self.save();
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.version += 1;
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(&self.entries) {
            let temp = path.with_extension("json.tmp");
            if std::fs::write(&temp, json).is_ok() {
                let _ = std::fs::rename(&temp, path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_and_recalls() {
        let mut mem = LanguageMemory::new(None);
        assert_eq!(mem.get("api"), None);
        mem.remember("api", true, true);
        assert_eq!(mem.get("api"), Some(true));
        mem.remember("api", true, true);
        assert_eq!(mem.get("api"), Some(true));
    }

    #[test]
    fn needs_twice_for_common_japanese() {
        let mut mem = LanguageMemory::new(None);
        mem.is_common_japanese = Some(Box::new(|w| w == "kyouha"));
        mem.remember("kyouha", true, false);
        // 1回目 = まだ効かない
        assert_eq!(mem.get("kyouha"), None);
        mem.remember("kyouha", true, false);
        // 2回目 = 効く
        assert_eq!(mem.get("kyouha"), Some(true));
    }

    #[test]
    fn ignores_short_or_non_ascii() {
        let mut mem = LanguageMemory::new(None);
        mem.remember("a", true, true);
        mem.remember("日本語", true, true);
        assert_eq!(mem.count(), 0);
    }
}
