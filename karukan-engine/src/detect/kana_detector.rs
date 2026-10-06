// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の KanaDetector.cs の移植。

//! JIS かな入力の判定。
//!
//! IME が閉じている状態で打たれた仮想キー列を JIS かな配列として読み替え、
//! 日本語としての妥当性 (辞書との前方一致) を評価する。
//! ローマ字入力かかな入力かを厳密に識別する必要はなく、日本語らしさの加点だけを返す。

use super::romaji_detector::RomajiDetector;
use super::Contribution;
use std::collections::HashSet;

/// JIS かな配列 (Shift なし)。仮想キーコードは日本語 106/109 キーボードのもの。
fn jis_kana(vk: u32) -> Option<char> {
    Some(match vk {
        0x31 => 'ぬ', 0x32 => 'ふ', 0x33 => 'あ', 0x34 => 'う', 0x35 => 'え', 0x36 => 'お',
        0x37 => 'や', 0x38 => 'ゆ', 0x39 => 'よ', 0x30 => 'わ', 0xBD => 'ほ', 0xDE => 'へ', 0xDC => 'ー',
        0x51 => 'た', 0x57 => 'て', 0x45 => 'い', 0x52 => 'す', 0x54 => 'か', 0x59 => 'ん',
        0x55 => 'な', 0x49 => 'に', 0x4F => 'ら', 0x50 => 'せ', 0xC0 => '゛', 0xDB => '゜',
        0x41 => 'ち', 0x53 => 'と', 0x44 => 'し', 0x46 => 'は', 0x47 => 'き', 0x48 => 'く',
        0x4A => 'ま', 0x4B => 'の', 0x4C => 'り', 0xBB => 'れ', 0xBA => 'け', 0xDD => 'む',
        0x5A => 'つ', 0x58 => 'さ', 0x43 => 'そ', 0x56 => 'ひ', 0x42 => 'こ', 0x4E => 'み',
        0x4D => 'も', 0xBC => 'ね', 0xBE => 'る', 0xBF => 'め', 0xE2 => 'ろ',
        _ => return None,
    })
}

/// Shift を押したときに別の文字になるキー (小書き文字・を・句読点・かぎかっこ)。
fn jis_kana_shift(vk: u32) -> Option<char> {
    Some(match vk {
        0x33 => 'ぁ', 0x34 => 'ぅ', 0x35 => 'ぇ', 0x36 => 'ぉ', 0x37 => 'ゃ', 0x38 => 'ゅ', 0x39 => 'ょ', 0x30 => 'を',
        0x45 => 'ぃ', 0x5A => 'っ', 0xDB => '「', 0xDD => '」', 0xBC => '、', 0xBE => '。', 0xBF => '・',
        _ => return None,
    })
}

const DAKUTEN: &str = "かがきぎくぐけげこごさざしじすずせぜそぞただちぢつづてでとどはばひびふぶへべほぼうゔ";
const HANDAKUTEN: &str = "はぱひぴふぷへぺほぽ";

/// JIS かな入力の判定器。
pub struct KanaDetector {
    words: HashSet<String>,
    prefixes: HashSet<String>,
}

impl KanaDetector {
    pub fn new(canonical_romaji_words: &[String], romaji: &RomajiDetector) -> Self {
        let mut words = HashSet::new();
        let mut prefixes = HashSet::new();
        for word in canonical_romaji_words {
            let analysis = romaji.analyze_word(word);
            if !analysis.is_valid || !analysis.partial.is_empty() {
                continue;
            }
            let kana = analysis.kana();
            if !words.insert(kana.clone()) {
                continue;
            }
            let chars: Vec<char> = kana.chars().collect();
            for i in 1..=chars.len() {
                prefixes.insert(chars[..i].iter().collect::<String>());
            }
        }
        Self { words, prefixes }
    }

    pub fn is_kana_key(vk: u32) -> bool {
        jis_kana(vk).is_some()
    }

    /// JIS かな配列でそのキーが入力するかな (濁点・半濁点のキーは ゛ ゜)。かなのキーでなければ None。
    pub fn kana_for_key(vk: u32, shift: bool) -> Option<char> {
        if shift {
            jis_kana_shift(vk).or_else(|| jis_kana(vk))
        } else {
            jis_kana(vk)
        }
    }

    /// 濁点・半濁点を直前のかなに付けた文字 (か + ゛ → が)。付けられなければ None。
    pub fn combine(previous: char, mark: char) -> Option<char> {
        let table = match mark {
            '゛' => DAKUTEN,
            '゜' => HANDAKUTEN,
            _ => return None,
        };
        let chars: Vec<char> = table.chars().collect();
        let index = chars.iter().position(|&c| c == previous)?;
        if index % 2 == 0 {
            chars.get(index + 1).copied()
        } else {
            None
        }
    }

    /// 辞書の日本語の語か、その先頭と一致するかな (2 文字以上)。
    pub fn is_japanese_word_or_prefix(&self, kana: &str) -> bool {
        kana.chars().count() >= 2 && (self.words.contains(kana) || self.prefixes.contains(kana))
    }

    /// 仮想キー列をかな文字列にする。濁点・半濁点キーは直前の文字に合成する。
    pub fn to_kana(keys: &[u32]) -> Option<String> {
        let mut chars: Vec<char> = Vec::with_capacity(keys.len());
        for &vk in keys {
            let kana = jis_kana(vk)?;
            if kana == '゛' || kana == '゜' {
                let table = if kana == '゛' { DAKUTEN } else { HANDAKUTEN };
                let table_chars: Vec<char> = table.chars().collect();
                let last = *chars.last()?;
                let index = table_chars.iter().position(|&c| c == last)?;
                if index % 2 != 0 {
                    return None;
                }
                *chars.last_mut()? = *table_chars.get(index + 1)?;
                continue;
            }
            chars.push(kana);
        }
        Some(chars.into_iter().collect())
    }

    /// かなとして妥当なら true を返し、加点を output に積む。
    pub fn evaluate(&self, keys: &[u32], output: &mut Vec<Contribution>) -> bool {
        let Some(kana) = Self::to_kana(keys) else {
            output.push(Contribution::new("Kana", 0, 0, "かな配列として読めない"));
            return false;
        };
        if kana.is_empty() {
            return true;
        }
        let first = kana.chars().next().unwrap_or(' ');
        if first == 'ん' || first == 'ー' {
            output.push(Contribution::new("Kana", 0, 0, format!("「{kana}」は語頭として不自然")));
            return false;
        }

        let len = kana.chars().count();
        if len >= 2 && self.words.contains(&kana) {
            output.push(Contribution::new("Kana", 5, 0, format!("かな入力「{kana}」が辞書の語と一致")));
        } else if len >= 3 && self.prefixes.contains(&kana) {
            output.push(Contribution::new("Kana", 4, 0, format!("かな入力「{kana}」が辞書の語の先頭と一致")));
        } else if len == 2 && self.prefixes.contains(&kana) {
            output.push(Contribution::new("Kana", 2, 0, format!("かな入力「{kana}」")));
        } else if len >= 3 {
            // 辞書に無い 3 文字以上のかな列は日本語としての根拠が弱い。
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_detector() -> KanaDetector {
        let romaji = RomajiDetector::new();
        let words: Vec<String> = vec![
            "konnichiwa".to_string(),
            "arigatou".to_string(),
            "kyou".to_string(),
        ];
        KanaDetector::new(&words, &romaji)
    }

    #[test]
    fn to_kana_handles_dakuten() {
        // か + ゛ = が (0x54 = か, 0xC0 = ゛)
        let kana = KanaDetector::to_kana(&[0x54, 0xC0]).unwrap();
        assert_eq!(kana, "が");
    }

    #[test]
    fn evaluates_dictionary_word() {
        let det = make_detector();
        let mut out = Vec::new();
        // こんにちわ = こ(0x42) ん(0x59) に(0x49) ち(0x41) わ(0x30)
        // (かな辞書はローマ字辞書の解析結果から作るため「わ」で登録される)
        let keys = [0x42, 0x59, 0x49, 0x41, 0x30];
        assert!(det.evaluate(&keys, &mut out));
        assert!(out.iter().any(|c| c.japanese == 5));
    }

    #[test]
    fn rejects_nonsense() {
        let det = make_detector();
        let mut out = Vec::new();
        // ぬ ふ ぬ ふ ... は辞書に無い 3+ 文字列
        let keys = [0x31, 0x32, 0x31];
        assert!(!det.evaluate(&keys, &mut out));
    }
}
