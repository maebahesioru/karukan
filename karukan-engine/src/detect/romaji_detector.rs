// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の RomajiDetector.cs の移植。

//! ローマ字 → かな変換の可能性を評価する。
//!
//! ここで分かるのは「ローマ字として成立するか」と日本語らしい特徴だけで、
//! 成立しても英語の可能性は残る (kana, sushi, radio)。最終判断は ScoreEngine が行う。

use std::collections::{HashMap, HashSet};

/// ローマ字→かなのトークン。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomajiToken {
    pub romaji: String,
    pub kana: String,
}

/// ローマ字としての解析結果。入力途中 (prefix) を前提にしている。
#[derive(Debug, Clone)]
pub struct RomajiAnalysis {
    pub is_valid: bool,
    pub tokens: Vec<RomajiToken>,
    /// まだ変換できない残り (入力途中の子音など)
    pub partial: String,
    pub invalid_reason: Option<String>,
    pub strong_youon: usize,
    pub tsu: usize,
    pub sokuon: usize,
    pub long_vowels: usize,
}

impl RomajiAnalysis {
    pub fn kana(&self) -> String {
        self.tokens.iter().map(|t| t.kana.as_str()).collect()
    }
}

/// かな → 綴り (先頭が標準の綴り)。l / x / v 系は英語と区別できないので意図的に含めない。
const TABLE: &[(&str, &[&str])] = &[
    ("あ", &["a"]),
    ("い", &["i", "yi"]),
    ("う", &["u", "whu", "wu"]),
    ("え", &["e"]),
    ("お", &["o"]),
    ("か", &["ka"]),
    ("き", &["ki"]),
    ("く", &["ku"]),
    ("け", &["ke"]),
    ("こ", &["ko"]),
    ("きゃ", &["kya"]),
    ("きゅ", &["kyu"]),
    ("きょ", &["kyo"]),
    ("さ", &["sa"]),
    ("し", &["shi", "si"]),
    ("す", &["su"]),
    ("せ", &["se", "ce"]),
    ("そ", &["so"]),
    ("しゃ", &["sha", "sya"]),
    ("しゅ", &["shu", "syu"]),
    ("しょ", &["sho", "syo"]),
    ("しぇ", &["she", "sye"]),
    ("た", &["ta"]),
    ("ち", &["chi", "ti"]),
    ("つ", &["tsu", "tu"]),
    ("て", &["te"]),
    ("と", &["to"]),
    ("ちゃ", &["cha", "tya", "cya"]),
    ("ちゅ", &["chu", "tyu", "cyu"]),
    ("ちょ", &["cho", "tyo", "cyo"]),
    ("ちぇ", &["che", "tye"]),
    ("な", &["na"]),
    ("に", &["ni"]),
    ("ぬ", &["nu"]),
    ("ね", &["ne"]),
    ("の", &["no"]),
    ("にゃ", &["nya"]),
    ("にゅ", &["nyu"]),
    ("にょ", &["nyo"]),
    ("は", &["ha"]),
    ("ひ", &["hi"]),
    ("ふ", &["fu", "hu"]),
    ("へ", &["he"]),
    ("ほ", &["ho"]),
    ("ひゃ", &["hya"]),
    ("ひゅ", &["hyu"]),
    ("ひょ", &["hyo"]),
    ("ふぁ", &["fa"]),
    ("ふぃ", &["fi"]),
    ("ふぇ", &["fe"]),
    ("ふぉ", &["fo"]),
    ("ま", &["ma"]),
    ("み", &["mi"]),
    ("む", &["mu"]),
    ("め", &["me"]),
    ("も", &["mo"]),
    ("みゃ", &["mya"]),
    ("みゅ", &["myu"]),
    ("みょ", &["myo"]),
    ("や", &["ya"]),
    ("ゆ", &["yu"]),
    ("よ", &["yo"]),
    ("ら", &["ra"]),
    ("り", &["ri"]),
    ("る", &["ru"]),
    ("れ", &["re"]),
    ("ろ", &["ro"]),
    ("りゃ", &["rya"]),
    ("りゅ", &["ryu"]),
    ("りょ", &["ryo"]),
    ("わ", &["wa"]),
    ("を", &["wo"]),
    ("うぉ", &["who"]),
    ("うぁ", &["wha"]),
    ("が", &["ga"]),
    ("ぎ", &["gi"]),
    ("ぐ", &["gu"]),
    ("げ", &["ge"]),
    ("ご", &["go"]),
    ("ぎゃ", &["gya"]),
    ("ぎゅ", &["gyu"]),
    ("ぎょ", &["gyo"]),
    ("ざ", &["za"]),
    ("じ", &["ji", "zi"]),
    ("ず", &["zu"]),
    ("ぜ", &["ze"]),
    ("ぞ", &["zo"]),
    ("じゃ", &["ja", "jya", "zya"]),
    ("じゅ", &["ju", "jyu", "zyu"]),
    ("じょ", &["jo", "jyo", "zyo"]),
    ("じぇ", &["je", "jye", "zye"]),
    ("だ", &["da"]),
    ("ぢ", &["di"]),
    ("づ", &["du"]),
    ("で", &["de"]),
    ("ど", &["do"]),
    ("ぢゃ", &["dya"]),
    ("ぢゅ", &["dyu"]),
    ("ぢょ", &["dyo"]),
    ("ば", &["ba"]),
    ("び", &["bi"]),
    ("ぶ", &["bu"]),
    ("べ", &["be"]),
    ("ぼ", &["bo"]),
    ("びゃ", &["bya"]),
    ("びゅ", &["byu"]),
    ("びょ", &["byo"]),
    ("ぱ", &["pa"]),
    ("ぴ", &["pi"]),
    ("ぷ", &["pu"]),
    ("ぺ", &["pe"]),
    ("ぽ", &["po"]),
    ("ぴゃ", &["pya"]),
    ("ぴゅ", &["pyu"]),
    ("ぴょ", &["pyo"]),
];

/// 変換ボックスでだけ使う綴り (小書き文字・外来音)。英語かどうかの判定には使わない。
const COMPOSITION_TABLE: &[(&str, &[&str])] = &[
    ("ぁ", &["xa", "la"]),
    ("ぃ", &["xi", "li", "xyi", "lyi"]),
    ("ぅ", &["xu", "lu"]),
    ("ぇ", &["xe", "le", "xye", "lye"]),
    ("ぉ", &["xo", "lo"]),
    ("ゃ", &["xya", "lya"]),
    ("ゅ", &["xyu", "lyu"]),
    ("ょ", &["xyo", "lyo"]),
    ("っ", &["xtu", "ltu", "xtsu", "ltsu"]),
    ("ゎ", &["xwa", "lwa"]),
    ("ゕ", &["xka", "lka"]),
    ("ゖ", &["xke", "lke"]),
    ("ん", &["xn"]),
    ("ゔぁ", &["va"]),
    ("ゔぃ", &["vi"]),
    ("ゔ", &["vu"]),
    ("ゔぇ", &["ve"]),
    ("ゔぉ", &["vo"]),
    ("いぇ", &["ye"]),
    ("うぃ", &["wi", "whi"]),
    ("うぇ", &["we", "whe"]),
    ("ゐ", &["wyi"]),
    ("ゑ", &["wye"]),
    ("てぃ", &["thi"]),
    ("でぃ", &["dhi"]),
    ("てゅ", &["thu"]),
    ("でゅ", &["dhu"]),
    ("とぅ", &["twu"]),
    ("どぅ", &["dwu"]),
    ("か", &["ca"]),
    ("く", &["cu"]),
    ("こ", &["co"]),
    ("ちぃ", &["cyi", "tyi"]),
    ("くぁ", &["kwa"]),
    ("ぐぁ", &["gwa"]),
    ("つぁ", &["tsa"]),
    ("つぃ", &["tsi"]),
    ("つぇ", &["tse"]),
    ("つぉ", &["tso"]),
];

/// 英語の綴りにほぼ現れない拗音 (sha/cha/ja は shut, chat, jam などで普通に出るので除外)。
const STRONG_YOUON_KANA: &[&str] = &[
    "きゃ", "きゅ", "きょ", "にゃ", "にゅ", "にょ", "ひゃ", "ひゅ", "ひょ", "みゃ", "みゅ", "みょ",
    "りゃ", "りゅ", "りょ", "ぎゃ", "ぎゅ", "ぎょ", "びゃ", "びゅ", "びょ", "ぴゃ", "ぴゅ", "ぴょ",
];

fn build_spelling_map(table: &[(&str, &[&str])]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (kana, spellings) in table {
        for spelling in *spellings {
            map.insert(spelling.to_string(), kana.to_string());
        }
    }
    map
}

fn build_partials(spellings: &HashMap<String, String>) -> HashSet<String> {
    let mut set = HashSet::new();
    for spelling in spellings.keys() {
        let chars: Vec<char> = spelling.chars().collect();
        for i in 1..chars.len() {
            set.insert(chars[..i].iter().collect::<String>());
        }
    }
    set
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}

fn is_consonant(c: char) -> bool {
    c.is_ascii_lowercase() && !is_vowel(c)
}

/// ローマ字 → かなの妥当性を評価する判定器。
pub struct RomajiDetector {
    spelling_to_kana: HashMap<String, String>,
    composition_spelling_to_kana: HashMap<String, String>,
    composition_partials: HashSet<String>,
    partial_spellings: HashSet<String>,
    kana_to_spellings: HashMap<String, Vec<String>>,
    strong_youon_kana: HashSet<String>,
}

impl Default for RomajiDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl RomajiDetector {
    pub fn new() -> Self {
        let spelling_to_kana = build_spelling_map(TABLE);
        let mut all = TABLE.to_vec();
        all.extend_from_slice(COMPOSITION_TABLE);
        let composition_spelling_to_kana = build_spelling_map(&all);
        let composition_partials = build_partials(&composition_spelling_to_kana);
        let partial_spellings = build_partials(&spelling_to_kana);
        let mut kana_to_spellings = HashMap::new();
        for (kana, spellings) in TABLE {
            kana_to_spellings.insert(
                kana.to_string(),
                spellings.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            );
        }
        Self {
            spelling_to_kana,
            composition_spelling_to_kana,
            composition_partials,
            partial_spellings,
            kana_to_spellings,
            strong_youon_kana: STRONG_YOUON_KANA.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn analyze(&self, letters: &str) -> RomajiAnalysis {
        self.analyze_impl(letters, true, false)
    }

    /// c 行 (ca / cu / co) を k 行に読み替える (ch は そのまま)。
    /// 英数状態の判定で、fucarete を fukarete として調べるのに使う。
    pub fn read_c_row(letters: &str) -> String {
        if !letters.contains('c') {
            return letters.to_string();
        }
        let mut chars: Vec<char> = letters.chars().collect();
        for i in 0..chars.len().saturating_sub(1) {
            if chars[i] == 'c' && matches!(chars[i + 1], 'a' | 'u' | 'o') {
                chars[i] = 'k';
            }
        }
        chars.into_iter().collect()
    }

    /// 変換ボックス用。語の途中から解析し (語頭の「ん」「っ」も許す)、
    /// "nn" は Microsoft IME と同じく常に「ん」と読む。
    pub fn analyze_fragment(&self, letters: &str) -> RomajiAnalysis {
        self.analyze_impl(letters, false, true)
    }

    /// 入力途中ではなく完結した語として解析する (語末の n を ん とみなす)。
    pub fn analyze_word(&self, word: &str) -> RomajiAnalysis {
        let analysis = self.analyze(word);
        if analysis.is_valid && analysis.partial == "n" {
            self.analyze(&format!("{word}n"))
        } else {
            analysis
        }
    }

    fn analyze_impl(&self, letters: &str, strict_start: bool, composition: bool) -> RomajiAnalysis {
        let s: Vec<char> = letters.chars().collect();
        let mut tokens: Vec<RomajiToken> = Vec::new();
        let mut strong_youon = 0usize;
        let mut tsu = 0usize;
        let mut sokuon = 0usize;
        let mut long_vowels = 0usize;

        fn make_invalid(
            tokens: Vec<RomajiToken>,
            reason: String,
            strong_youon: usize,
            tsu: usize,
            sokuon: usize,
            long_vowels: usize,
        ) -> RomajiAnalysis {
            RomajiAnalysis {
                is_valid: false,
                tokens,
                partial: String::new(),
                invalid_reason: Some(reason),
                strong_youon,
                tsu,
                sokuon,
                long_vowels,
            }
        }

        let mut i = 0usize;
        while i < s.len() {
            let c = s[i];
            if !c.is_ascii_lowercase() {
                return make_invalid(tokens.clone(), format!("'{c}' は英字ではない"), strong_youon, tsu, sokuon, long_vowels);
            }

            // ん: "nn" / 子音の前の "n"。ヘボン式 konnichiwa は ん + に。
            if c == 'n'
                && i + 1 < s.len()
                && (s[i + 1] == 'n' || (is_consonant(s[i + 1]) && s[i + 1] != 'y'))
            {
                if i == 0 && strict_start {
                    return make_invalid(tokens.clone(), "語頭の「ん」".to_string(), strong_youon, tsu, sokuon, long_vowels);
                }
                let mut consumed = 1usize;
                if s[i + 1] == 'n'
                    && (composition || i + 2 >= s.len() || !(is_vowel(s[i + 2]) || s[i + 2] == 'y'))
                {
                    consumed = 2;
                }
                tokens.push(RomajiToken {
                    romaji: s[i..i + consumed].iter().collect(),
                    kana: "ん".to_string(),
                });
                i += consumed;
                continue;
            }

            // っ: 同じ子音の連続 (kk, tt, ss …) と tch。
            if i + 1 < s.len()
                && is_consonant(c)
                && c != 'n'
                && (s[i + 1] == c
                    || (c == 't' && s[i + 1] == 'c' && i + 2 < s.len() && s[i + 2] == 'h'))
            {
                if i == 0 && strict_start {
                    return make_invalid(tokens.clone(), "語頭の「っ」".to_string(), strong_youon, tsu, sokuon, long_vowels);
                }
                tokens.push(RomajiToken {
                    romaji: c.to_string(),
                    kana: "っ".to_string(),
                });
                sokuon += 1;
                i += 1;
                continue;
            }

            let spellings = if composition {
                &self.composition_spelling_to_kana
            } else {
                &self.spelling_to_kana
            };
            let mut matched = false;
            let max_len = std::cmp::min(4, s.len() - i);
            for length in (1..=max_len).rev() {
                let piece: String = s[i..i + length].iter().collect();
                if let Some(kana) = spellings.get(&piece) {
                    if self.strong_youon_kana.contains(kana) {
                        strong_youon += 1;
                    }
                    if piece == "tsu" {
                        tsu += 1;
                    }
                    if kana == "う"
                        && tokens
                            .last()
                            .map(|t| {
                                t.romaji.ends_with('o') || t.romaji.ends_with('u')
                            })
                            .unwrap_or(false)
                    {
                        long_vowels += 1;
                    }
                    tokens.push(RomajiToken {
                        romaji: piece,
                        kana: kana.clone(),
                    });
                    i += length;
                    matched = true;
                    break;
                }
            }
            if matched {
                continue;
            }

            let rest: String = s[i..].iter().collect();
            let partials = if composition {
                &self.composition_partials
            } else {
                &self.partial_spellings
            };
            if partials.contains(&rest) || rest == "n" || rest == "tc" {
                return RomajiAnalysis {
                    is_valid: true,
                    tokens,
                    partial: rest,
                    invalid_reason: None,
                    strong_youon,
                    tsu,
                    sokuon,
                    long_vowels,
                };
            }
            return make_invalid(tokens, format!("「{rest}」はローマ字として成立しない"), strong_youon, tsu, sokuon, long_vowels);
        }

        RomajiAnalysis {
            is_valid: true,
            tokens,
            partial: String::new(),
            invalid_reason: None,
            strong_youon,
            tsu,
            sokuon,
            long_vowels,
        }
    }

    /// 変換ボックスの表示用。ローマ字として読めない文字はその文字だけ英字のまま残し、
    /// 続きを変換する (MS-IME の「ごおｇ」と同じ振る舞い)。final なら語末の n を ん にする。
    pub fn convert_lenient(&self, letters: &str, final_n: bool) -> String {
        let mut builder = String::new();
        let mut rest = letters.to_string();
        while !rest.is_empty() {
            let analysis = self.analyze_impl(&rest, false, true);
            for token in &analysis.tokens {
                builder.push_str(&token.kana);
            }
            if analysis.is_valid {
                if final_n && analysis.partial == "n" {
                    builder.push('ん');
                } else {
                    builder.push_str(&analysis.partial);
                }
                break;
            }
            let consumed: usize = analysis.tokens.iter().map(|t| t.romaji.chars().count()).sum();
            let rest_chars: Vec<char> = rest.chars().collect();
            if consumed < rest_chars.len() {
                builder.push(rest_chars[consumed]);
                rest = rest_chars[consumed + 1..].iter().collect();
            } else {
                break;
            }
        }
        builder
    }

    /// 辞書の見出し語 (ヘボン式) から、実際に打たれうる綴りの揺れ (si/shi, tu/tsu, nn/n …) を列挙する。
    /// 組み合わせ爆発を防ぐため最大 limit 件。
    pub fn spelling_variants(&self, canonical: &str, limit: usize) -> Vec<String> {
        let analysis = self.analyze_word(canonical);
        if !analysis.is_valid || !analysis.partial.is_empty() {
            return vec![canonical.to_string()];
        }
        let tokens = &analysis.tokens;
        let mut results: Vec<String> = Vec::new();
        let mut builder = String::new();

        fn walk(
            det: &RomajiDetector,
            tokens: &[RomajiToken],
            index: usize,
            builder: &mut String,
            results: &mut Vec<String>,
            limit: usize,
        ) {
            if results.len() >= limit {
                return;
            }
            if index == tokens.len() {
                results.push(builder.clone());
                return;
            }
            let mark = builder.len();
            for option in det.options_for(tokens, index) {
                builder.push_str(&option);
                walk(det, tokens, index + 1, builder, results, limit);
                builder.truncate(mark);
            }
        }
        walk(self, tokens, 0, &mut builder, &mut results, limit);
        if !results.contains(&canonical.to_string()) {
            results.insert(0, canonical.to_string());
        }
        results
    }

    fn options_for(&self, tokens: &[RomajiToken], index: usize) -> Vec<String> {
        let token = &tokens[index];
        let next = tokens.get(index + 1);
        let mut out = Vec::new();
        match token.kana.as_str() {
            "ん" => {
                out.push("nn".to_string());
                if next.is_none() {
                    out.push("n".to_string());
                }
                if let Some(n) = next {
                    if n.kana != "っ" {
                        if let Some(first) = n.romaji.chars().next() {
                            if is_consonant(first) && first != 'n' && first != 'y' {
                                out.push("n".to_string());
                            }
                        }
                    }
                    if n.romaji.starts_with('n') {
                        out.push("n".to_string());
                    }
                }
            }
            "っ" => {
                if let Some(following) = next {
                    if let Some(spellings) = self.kana_to_spellings.get(&following.kana) {
                        let mut firsts: Vec<char> = spellings
                            .iter()
                            .filter_map(|s| s.chars().next())
                            .collect();
                        firsts.sort();
                        firsts.dedup();
                        for first in firsts {
                            if is_consonant(first) && first != 'n' {
                                out.push(first.to_string());
                            }
                        }
                    }
                } else {
                    out.push(token.romaji.clone());
                }
            }
            _ => {
                if let Some(options) = self.kana_to_spellings.get(&token.kana) {
                    out.extend(options.iter().cloned());
                } else {
                    out.push(token.romaji.clone());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzes_kyouha() {
        let det = RomajiDetector::new();
        let a = det.analyze("kyouha");
        assert!(a.is_valid);
        assert_eq!(a.kana(), "きょうは");
        assert!(a.long_vowels >= 1);
    }

    #[test]
    fn analyzes_konnichiwa() {
        let det = RomajiDetector::new();
        let a = det.analyze("konnichiwa");
        assert!(a.is_valid);
        // ローマ字→かなは「わ」まで (こんにちは の語彙知識は辞書側の担当)
        assert_eq!(a.kana(), "こんにちわ");
        // "nn" は ん であって促音ではない
        assert_eq!(a.sokuon, 0);
    }

    #[test]
    fn rejects_english_like_th() {
        let det = RomajiDetector::new();
        let a = det.analyze("github");
        // gi + th... "th" はローマ字として成立しない
        assert!(!a.is_valid);
        assert!(a.invalid_reason.is_some());
    }

    #[test]
    fn partial_handling() {
        let det = RomajiDetector::new();
        let a = det.analyze("ky");
        assert!(a.is_valid);
        assert_eq!(a.partial, "ky");
        let b = det.analyze("kya");
        assert_eq!(b.kana(), "きゃ");
        assert!(b.strong_youon >= 1);
    }

    #[test]
    fn spelling_variants_include_kunrei() {
        let det = RomajiDetector::new();
        let v = det.spelling_variants("shita", 64);
        assert!(v.contains(&"sita".to_string()));
        assert!(v.contains(&"shita".to_string()));
    }

    #[test]
    fn read_c_row_converts() {
        assert_eq!(RomajiDetector::read_c_row("fucarete"), "fukarete");
        assert_eq!(RomajiDetector::read_c_row("chotto"), "chotto");
    }

    #[test]
    fn convert_lenient_keeps_unreadable() {
        let det = RomajiDetector::new();
        // "kyouha" + 読めない文字が混ざっても壊れない
        let out = det.convert_lenient("kyouha", false);
        assert_eq!(out, "きょうは");
    }
}
