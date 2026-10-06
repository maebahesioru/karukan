// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の
// src/Meltype.Core/Composition/CompositionDetector.cs / CompositionText.cs (型定義) の移植。

//! 変換ボックスの中身の区間分け (日本語区間 / 英語区間)。
//!
//! 日本語入力中に google と打って「ごおｇぇ」になるのを防ぐのが目的。英語かどうかは
//! 区間ごとに判定するので、「きょうは google」なら google の部分だけが英字になる。
//! 区間の区切りはかな 1 音の境目だけ。未確定のうちは何度でも表示を作り直せるので、
//! ここでの判定は IME 自動切替より積極的でよいが、既定は日本語で、英語と判断できる
//! 根拠があるときだけ英字にする。
//!
//! TODO(将来): Meltype の UserModel による評価 (`_user?.Evaluate`) は、参照した
//! CompositionDetector.cs の版には存在しないため未移植 (導入されたらここへ足す)。

use super::detectors::starts_with_particle;
use super::kana_detector::KanaDetector;
use super::romaji_detector::RomajiDetector;
use super::word_list::parse_words;
use super::{
    BuiltInWordChecker, DictionaryDetector, DictionarySource, EnglishDetector, LanguageMemory,
    ProperNouns, TypoDetector, WordChecker, WordList,
};
use super::DetectionLevel;
use std::sync::OnceLock;

/// 変換ボックス内の 1 単位。ローマ字 1 音 (きょ, っ, ん …)・ローマ字として読めなかった
/// 英字 1 文字・記号 1 文字のいずれか。`raw` は実際に打った文字 (英語として表示するときに使う)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionUnit {
    pub kana: String,
    pub raw: String,
}

/// 表示上のひとまとまり。英語と判定した区間は英字のまま、それ以外は日本語 (かな/漢字)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionSegment {
    pub is_english: bool,
    pub kana: String,
    pub raw: String,
}

/// ドメインの . の後ろを英字のままにするトップレベルドメイン (tetr.io、Wakatte.TV)。
pub const DOMAIN_SUFFIXES: [&str; 26] = [
    "ai", "app", "au", "biz", "ca", "cn", "co", "com", "de", "dev", "edu", "eu", "fr", "gg",
    "gov", "info", "in", "io", "jp", "kr", "me", "net", "org", "uk", "us", "xyz",
];

/// - を付けて使う英語の接頭辞 (e-mail、re-do、co-op、x-ray)。
/// 1 文字の母音 (o-bun = オーブン) は日本語の長音とまぎらわしいので e と x だけ。
pub const HYPHEN_PREFIXES: [&str; 15] = [
    "e", "x", "re", "co", "ex", "non", "anti", "semi", "multi", "pre", "sub", "post", "mid",
    "self", "well",
];

/// 接頭辞の規則では拾えない、- の入ったよく使う英単語 (後ろが 2 文字以下など)。
pub const HYPHENATED_WORDS: [&str; 29] = [
    "co-op", "re-do", "x-ray", "t-shirt", "wi-fi", "hi-fi", "e-book", "e-sports", "k-pop",
    "j-pop", "j-rock", "j-core", "p-hub", "talk-admin", "r-18", "sub-6", "gpt-6.7", "a-z",
    "u-turn", "check-in", "log-in", "sign-in", "add-on", "plug-in", "built-in", "follow-up",
    "set-up", "pop-up", "drop-down",
];

/// 日本語のローマ字の途中を英語の接頭辞と誤認しないよう、日本語に続けて拾うのは明示した英数字表記だけ。
pub const NUMERIC_HYPHENATED_WORDS: [&str; 3] = ["r-18", "sub-6", "gpt-6.7"];

/// 語尾の助詞 (grokga = grok + が の判定に使う)。
const TRAILING_PARTICLES: [&str; 12] = [
    "kara", "made", "yori", "ga", "wo", "ni", "de", "no", "to", "mo", "ha", "wa",
];

/// する の活用 (して・した・します …)。
const SURU_FORMS: [&str; 13] = [
    "する", "すれ", "した", "して", "しま", "しな", "しよ", "しと", "しちゃ", "しろ", "され",
    "させ", "せず",
];

/// 同梱の english-readable.txt (ローマ字として読めても英語にする英単語)。
fn readable_english() -> &'static WordList {
    static READABLE_ENGLISH: OnceLock<WordList> = OnceLock::new();
    READABLE_ENGLISH.get_or_init(|| {
        let mut list = WordList::new();
        for word in parse_words(include_str!("data/english-readable.txt")) {
            list.add(&word);
        }
        list
    })
}

/// 前の文脈の点数: true = +1、false = -1、分からなければ 0。
fn score(english: Option<bool>) -> i32 {
    match english {
        Some(true) => 1,
        Some(false) => -1,
        None => 0,
    }
}

/// 区間の前が英語か: 先頭なら入力欄の確定済みの文字、途中なら直前の区間
/// (英語区間の直後なら英語、それ以外は日本語)。
fn preceded_by_english(
    segments: &[CompositionSegment],
    japanese_start: usize,
    start: usize,
    preceding_english: Option<bool>,
) -> Option<bool> {
    if start == 0 {
        preceding_english
    } else {
        Some(segments.last().map_or(false, |s| s.is_english) && japanese_start == start)
    }
}

/// 前の文脈の点数: 英文の続きなら +2、英語なら +1、日本語なら -1、分からなければ 0。
fn before_score(
    segments: &[CompositionSegment],
    japanese_start: usize,
    start: usize,
    preceding_english: Option<bool>,
    english_sentence: bool,
) -> i32 {
    if start == 0 && english_sentence {
        2
    } else {
        score(preceded_by_english(segments, japanese_start, start, preceding_english))
    }
}

/// 単位 [start, end) の打った文字 (raw) をつなげたもの。
fn raw(units: &[CompositionUnit], start: usize, end: usize) -> String {
    let mut out = String::new();
    for unit in &units[start..end] {
        out.push_str(&unit.raw);
    }
    out
}

/// 単位 [start, end) のかなをつなげたもの。
fn kana(units: &[CompositionUnit], start: usize, end: usize) -> String {
    let mut out = String::new();
    for unit in &units[start..end] {
        out.push_str(&unit.kana);
    }
    out
}

/// 日本語区間の表示形式 (かな + 入力途中の子音)。
fn japanese(units: &[CompositionUnit], start: usize, end: usize, pending: &str) -> CompositionSegment {
    CompositionSegment {
        is_english: false,
        kana: kana(units, start, end),
        raw: raw(units, start, end) + pending,
    }
}

/// する の活用 (して・した・します …) だけでできたかなか。
fn is_suru_form(kana_text: &str) -> bool {
    SURU_FORMS.iter().any(|form| kana_text.starts_with(form))
        && kana_text.chars().all(|c| ('ぁ'..='ゖ').contains(&c) || c == 'ー')
}

/// 英文の中では半角のままにする記号。[ ] は日本語の入力では「」なので含めない
/// (英単語の後ろでも「」: bot「Thinking」)。
fn is_ascii_symbol(unit: &CompositionUnit) -> bool {
    let mut chars = unit.raw.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => {
            ('!'..='~').contains(&c) && !c.is_ascii_alphanumeric() && !matches!(c, '[' | ']')
        }
        _ => false,
    }
}

/// 区間 [.., end) が、ん の後の っ (1 文字の子音) で終わるか (meeting|ga の g = っ)。
/// 区間だけを見るとこの子音は読めない (英単語の最後の子音) ので、読めない英字を含む区間と同じに扱う。
fn ends_with_lone_sokuon(units: &[CompositionUnit], end: usize) -> bool {
    end >= 2
        && end < units.len()
        && units[end - 1].kana == "っ"
        && units[end - 1].raw.chars().count() == 1
        && units[end - 2].kana == "ん"
}

/// かなのすぐ後ろに続く w の単位 (きた|w|w): 笑いとして w のまま残したもの。
/// ローマ字として読めなかった英字 (zoom の m) とは違うので、英単語の根拠にしない。
fn is_laughter(units: &[CompositionUnit], k: usize) -> bool {
    let mut i = k as isize;
    while i >= 0 {
        let unit = &units[i as usize];
        if !(matches!(unit.raw.as_str(), "w" | "W") && unit.kana == unit.raw) {
            break;
        }
        i -= 1;
    }
    if !(i < k as isize && i >= 0) {
        return false;
    }
    let previous = &units[i as usize];
    if !previous.kana.chars().next().map_or(false, |c| ('ぁ'..='ヺ').contains(&c)) {
        return false;
    }
    units[k + 1..]
        .iter()
        .take_while(|unit| {
            !unit.raw.is_empty() && unit.raw.chars().next().map_or(false, |c| c.is_ascii_alphabetic())
        })
        .all(|unit| matches!(unit.raw.as_str(), "w" | "W"))
}

/// 単位 [start, end) に、ローマ字として読めなかった英字 (かなにならなかった 1 文字) があるか。
fn has_unreadable(units: &[CompositionUnit], start: usize, end: usize) -> bool {
    for k in start..end {
        let unit = &units[k];
        if unit.raw.chars().count() == 1
            && unit.kana == unit.raw
            && unit.raw.chars().next().map_or(false, |c| c.is_ascii_alphabetic())
            && !is_laughter(units, k)
        {
            return true;
        }
    }
    false
}

/// start から始まるユーザー名の終わり。無ければ -1。
/// @ の後ろ (Discord・X のメンション @kuraido) と、_ の入った語 (upah_setu、cafely_latte) は、
/// ローマ字として読めても英字のまま。英字・数字・_ が続く所までがユーザー名
/// (@ の後ろは、メールアドレスのドメインの . - も含める)。
fn user_name_end(units: &[CompositionUnit], start: usize, pending: &str) -> isize {
    fn is_name_unit(unit: &CompositionUnit) -> bool {
        !unit.raw.is_empty() && unit.raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
    let n = units.len();
    if !is_name_unit(&units[start]) || (start > 0 && is_name_unit(&units[start - 1])) {
        return -1;
    }
    let mention = start > 0 && units[start - 1].raw == "@";
    let mut end = start;
    while end < n && is_name_unit(&units[end]) {
        end += 1;
    }
    // メールアドレスのドメイン (taro@gmail.com) の . - も続けて英字に。
    while mention && end + 1 < n && (units[end].raw == "." || units[end].raw == "-") && is_name_unit(&units[end + 1]) {
        end += 1;
        while end < n && is_name_unit(&units[end]) {
            end += 1;
        }
    }
    let name = raw(units, start, end) + if end == n { pending } else { "" };
    if !name.chars().any(|c| c.is_ascii_alphabetic()) {
        return -1;
    }
    if mention || name.contains('_') {
        end as isize
    } else {
        -1
    }
}

/// 変換ボックスの中身のうち、どこを英字のまま見せるかを決める判定器。
pub struct CompositionDetector {
    romaji: RomajiDetector,
    japanese: DictionaryDetector,
    english: EnglishDetector,
    typo: TypoDetector,
    proper: ProperNouns,
    kana: Option<KanaDetector>,
    /// 普通の英単語の判定に使うスペルチェッカー。None なら同梱の辞書だけ。
    spell_checker: Option<Box<dyn WordChecker>>,
    /// ユーザーが英字 / かなに直して覚えた語 (自動の判定より優先する)。
    memory: Option<LanguageMemory>,
    /// 英字の並びが、ローマ字としてよく使う日本語になるか (kyouha = 今日は、tomato = とまと)。無ければ使わない。
    is_common_japanese: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
}

impl CompositionDetector {
    /// 判定器一式を組み立てる。proper を省略したい場合は `ProperNouns::load(None)` を渡す。
    pub fn new(
        romaji: RomajiDetector,
        japanese: DictionaryDetector,
        english: EnglishDetector,
        typo: TypoDetector,
        proper: ProperNouns,
        kana: Option<KanaDetector>,
    ) -> Self {
        Self {
            romaji,
            japanese,
            english,
            typo,
            proper,
            kana,
            spell_checker: None,
            memory: None,
            is_common_japanese: None,
        }
    }

    /// 組み込み辞書 (+ ユーザー辞書) から一式を組み立てる。
    pub fn create_default(user_directory: Option<&std::path::Path>) -> Self {
        let romaji = RomajiDetector::new();
        let japanese_words = DictionarySource::load("japanese.txt", user_directory);
        let japanese = DictionaryDetector::new(&japanese_words);
        let proper = ProperNouns::load(user_directory);
        let english = EnglishDetector::new(
            DictionarySource::load("english.txt", user_directory)
                .into_iter()
                .chain(proper.lowercase_words().cloned()),
        );
        let typo = TypoDetector::new(&japanese.words);
        let kana = KanaDetector::new(&japanese_words, &romaji);
        Self::new(romaji, japanese, english, typo, proper, Some(kana))
    }

    pub fn romaji(&self) -> &RomajiDetector {
        &self.romaji
    }

    pub fn proper_nouns(&self) -> &ProperNouns {
        &self.proper
    }

    /// 普通の英単語の判定に使うスペルチェッカーを設定する。
    pub fn set_spell_checker(&mut self, checker: Box<dyn WordChecker>) {
        self.spell_checker = Some(checker);
    }

    /// 同梱の英単語リスト (english-words.txt) をスペルチェッカーとして使う。
    pub fn set_builtin_spell_checker(&mut self) {
        self.spell_checker = Some(Box::new(BuiltInWordChecker::from_text(
            DictionarySource::read_embedded("english-words.txt"),
        )));
    }

    /// ユーザーが英字 / かなに直して覚えた語を設定する。
    pub fn set_memory(&mut self, memory: LanguageMemory) {
        self.memory = Some(memory);
    }

    /// 英字の並びが、ローマ字としてよく使う日本語になるかを判定する関数を設定する。
    pub fn set_is_common_japanese(&mut self, is_common_japanese: Box<dyn Fn(&str) -> bool + Send + Sync>) {
        self.is_common_japanese = Some(is_common_japanese);
    }

    /// 単位列 (+ 入力途中の子音) を英語区間と日本語区間に分ける。
    /// 先頭から見て、ある単位から始まる最長の「英語と言える」区間があればそこを英語にする。
    ///
    /// - `preceding_english`: 入力欄のキャレットの前の確定済みの文字が英語なら true、日本語なら false、分からなければ None。
    /// - `following_english`: キャレットの後ろの文字が英語なら true、日本語なら false、分からなければ None。
    /// - `level`: 判定の強さ。手動 (Manual) では Shift で打った大文字始まりの語だけを英語にする。
    /// - `english_sentence`: キャレットの前が英文 (空白で区切った英単語が 2 語以上続いて空白で終わる: "I want ")。
    ///   日本語の文の中の英単語 (GitHub の) より強い英語の根拠として扱う。
    /// - `final_`: 打ち終わった (Space・Enter)。末尾の区間でも、英単語の打ちかけ (amaz) は英語の根拠にしない。
    #[allow(clippy::too_many_arguments)]
    pub fn segment(
        &self,
        units: &[CompositionUnit],
        pending: &str,
        preceding_english: Option<bool>,
        following_english: Option<bool>,
        level: DetectionLevel,
        english_sentence: bool,
        kana_input: bool,
        final_: bool,
    ) -> Vec<CompositionSegment> {
        let segments = self.find_spans(
            units,
            pending,
            preceding_english,
            following_english,
            level,
            english_sentence && preceding_english == Some(true),
            kana_input,
            final_,
        );
        if kana_input {
            return segments;
        }
        // 辞書にない英単語 (stackoverflow など) を最初から打っているなら全体を英語にする。
        // 途中の区間 (… flow) だけを英語にすると「sたcこvえrflow」のようになってしまう。
        // ただし先頭が辞書の英単語として区切れている (github に push) ならその区切りを使う。
        let whole = raw(units, 0, units.len()) + pending;
        if let Some(split) = self.unknown_word_then_japanese(units, pending, &segments, level, &whole) {
            return split;
        }
        let first_is_english = segments.first().map_or(false, |s| s.is_english);
        let memory_allows = match &self.memory {
            Some(memory) => memory.get(&whole.to_lowercase()) != Some(false),
            None => true,
        };
        if level != DetectionLevel::Manual && !first_is_english && memory_allows && self.is_unknown_english_word(&whole) {
            return vec![CompositionSegment {
                is_english: true,
                kana: String::new(),
                raw: whole,
            }];
        }
        segments
    }

    /// 知らない英字の語 + 助詞で始まる日本語 (grokga、grokniyoruto) を最初から打っているなら、
    /// 語は英字・後ろは日本語 (grokが)。語は、ローマ字の打ちかけとしても読めない (gr) もの。
    /// 後ろは最後までローマ字として読めるもの。
    fn unknown_word_then_japanese(
        &self,
        units: &[CompositionUnit],
        pending: &str,
        segments: &[CompositionSegment],
        level: DetectionLevel,
        whole: &str,
    ) -> Option<Vec<CompositionSegment>> {
        if level == DetectionLevel::Manual
            || segments.first().map_or(false, |s| s.is_english)
            || !whole.chars().all(|c| c.is_ascii_alphabetic())
            || self.is_known_english_word(whole)
        {
            return None;
        }
        for k in 1..units.len() {
            let stem = raw(units, 0, k).to_lowercase();
            if stem.chars().count() < 3 {
                continue;
            }
            let stem_fragment = self.romaji.analyze_fragment(&stem);
            if stem_fragment.is_valid || (self.is_known_english_word(&stem) && stem.chars().count() >= 5) {
                continue;
            }
            // 語の頭から読めない (gr): 日本語の後ろの英単語 (kyouha|google) ではない。
            // 読めない 1 文字の後ろが最後まで読める (s|dake) なら、英字 1 文字 + 日本語。
            let stem_analysis = self.romaji.analyze(&stem);
            let readable: usize = stem_analysis.tokens.iter().map(|t| t.romaji.chars().count()).sum();
            if readable >= 2 {
                continue;
            }
            let after_unreadable: String = stem.chars().skip(readable + 1).collect();
            let after_analysis = self.romaji.analyze(&after_unreadable);
            if after_analysis.is_valid && after_analysis.partial.is_empty() {
                continue;
            }
            let rest = (raw(units, k, units.len()) + pending).to_lowercase();
            if !TRAILING_PARTICLES.iter().any(|particle| rest.starts_with(particle)) {
                continue;
            }
            if stem.chars().last() == rest.chars().next() {
                continue;
            }
            let rest_analysis = self.romaji.analyze(&rest);
            if !(rest_analysis.is_valid && (rest_analysis.partial.is_empty() || rest_analysis.partial == "n")) {
                continue;
            }
            return Some(vec![
                CompositionSegment {
                    is_english: true,
                    kana: String::new(),
                    raw: raw(units, 0, k),
                },
                japanese(units, k, units.len(), pending),
            ]);
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn find_spans(
        &self,
        units: &[CompositionUnit],
        pending: &str,
        preceding_english: Option<bool>,
        following_english: Option<bool>,
        level: DetectionLevel,
        english_sentence: bool,
        kana_input: bool,
        final_: bool,
    ) -> Vec<CompositionSegment> {
        let n = units.len();
        let mut segments: Vec<CompositionSegment> = Vec::new();
        let mut japanese_start = 0usize;
        let mut i = 0usize;
        while i < n {
            let mut found: isize = -1;
            // ドメインの . の後ろは、短い国別・用途別トップレベルドメインでも英字のままにする
            // (tetr.io、Wakatte.TV)。
            if !kana_input
                && level != DetectionLevel::Manual
                && i > 0
                && units[i - 1].raw == "."
                && preceded_by_english(&segments, japanese_start, i, preceding_english) == Some(true)
            {
                let mut j = n;
                while j > i {
                    let mut domain_label = raw(units, i, j);
                    if j == n {
                        domain_label.push_str(pending);
                    }
                    let domain_label = domain_label.to_lowercase();
                    if DOMAIN_SUFFIXES.contains(&domain_label.as_str())
                        || (!final_ && DOMAIN_SUFFIXES.iter().any(|tld| tld.starts_with(&domain_label)))
                    {
                        found = j as isize;
                        break;
                    }
                    j -= 1;
                }
            }
            // 英文の中の記号 (, . ! ? -) は読点・句点にせず半角のまま。日本語の文の中の英単語の後
            // (今日はgoogle、) は日本語の記号。(かな入力では 、。 も かなのキーなので対象外)
            if !kana_input
                && is_ascii_symbol(&units[i])
                && preceded_by_english(&segments, japanese_start, i, preceding_english) == Some(true)
                && segments
                    .iter()
                    .all(|s| s.is_english || !s.raw.chars().any(|c| c.is_ascii_alphabetic()))
            {
                found = i as isize + 1;
            }
            if !kana_input && found < 0 {
                found = user_name_end(units, i, pending);
            }
            if !kana_input && found < 0 && level != DetectionLevel::Manual {
                found = self.capitalized_word_end(units, i, pending, final_);
            }
            if !kana_input && found < 0 && level != DetectionLevel::Manual {
                found = self.hyphenated_word_end(units, i, pending);
            }
            // 英語の語 + 数字のすぐ後ろの英単語 (part1026|beta、win11|pro) は、ローマ字として読めても英語 (ベタ にしない)。
            // 助詞で始まるなら日本語 (PS5|wokaitai)。
            if !kana_input && found < 0 && level != DetectionLevel::Manual {
                let suffix = self.alphanumeric_suffix_end(units, i, pending, &segments, japanese_start);
                if suffix > 0 {
                    found = suffix;
                }
            }
            let mut j = n;
            while j > i && found < 0 {
                // 区間の後ろ: 末尾まで打っているならキャレットの後ろの文字、途中なら続きの日本語。
                // 後ろが記号だけ (let's go! の !) なら、語はそこで打ち終わっている: Enter で確定するときと
                // 同じく末尾の語として見る (記号を日本語の続きとみなして、英文の中の go・no を ご・の にしていた)。
                // (入力が 1 語 + 記号だけのとき。途中の区間 (BE|kana|?) の後ろの記号は、今までどおり日本語の続きとみなす)
                let symbols_after =
                    i == 0 && j < n && pending.is_empty() && (j..n).all(|k| is_ascii_symbol(&units[k]));
                let after = if j == n || symbols_after { following_english } else { Some(false) };
                // 英単語のすぐ後ろの する の活用 (push + site = して、commit + sita = した) は、英単語 (site) でも日本語
                // (末尾だと pushsite 全体が英字になっていた)。
                if !kana_input
                    && preceded_by_english(&segments, japanese_start, i, preceding_english) == Some(true)
                    && is_suru_form(&kana(units, i, j))
                {
                    j -= 1;
                    continue;
                }
                let english = if kana_input {
                    self.is_english_span_kana(
                        &raw(units, i, j),
                        &kana(units, i, j),
                        j == n,
                        before_score(&segments, japanese_start, i, preceding_english, english_sentence),
                        after,
                        level,
                        final_,
                    )
                } else {
                    let mut span = raw(units, i, j);
                    if j == n {
                        span.push_str(pending);
                    }
                    let mut next = None;
                    if j < n {
                        let mut next_text = units[j].raw.clone();
                        if j + 1 == n {
                            next_text.push_str(pending);
                        }
                        next = Some(next_text);
                    }
                    self.is_english_span(
                        &span,
                        j == n,
                        before_score(&segments, japanese_start, i, preceding_english, english_sentence),
                        after,
                        i == 0,
                        level,
                        final_,
                        has_unreadable(units, i, j) || ends_with_lone_sokuon(units, j),
                        next.as_deref(),
                        symbols_after,
                    )
                };
                if english {
                    found = j as isize;
                    break;
                }
                j -= 1;
            }
            // 途中で終わる英語の区間 (te + al… の teal) より、少し後ろから末尾まで続く長い英単語 (alcoholic)
            // があれば、そちらを取る (sometealcoholic → 染めて + alcoholic。teal を取ると残りの coholic が
            // ローマ字になってしまう)。
            if found > i as isize && (found as usize) < n && !kana_input && !is_ascii_symbol(&units[i]) {
                // 後ろに日本語が続いてもよい (motte|school|he → mottes を取ると chool が ちょおl になる。持って + school + へ)。
                let found_length = raw(units, i, found as usize).chars().count();
                let mut k = i + 1;
                while k < found as usize && found >= 0 {
                    let mut e = n;
                    while e > found as usize {
                        let mut word = raw(units, k, e);
                        if e == n {
                            word.push_str(pending);
                        }
                        if word.chars().count() >= found_length && self.is_long_english_word(&word) {
                            found = -1;
                            break;
                        }
                        e -= 1;
                    }
                    k += 1;
                }
            }
            if found < 0 {
                i += 1;
                continue;
            }
            let found = found as usize;
            if i > japanese_start {
                segments.push(japanese(units, japanese_start, i, ""));
            }
            let mut raw_text = raw(units, i, found);
            if found == n {
                raw_text.push_str(pending);
            }
            segments.push(CompositionSegment {
                is_english: true,
                kana: String::new(),
                raw: raw_text,
            });
            i = found;
            japanese_start = found;
            if found == n {
                return segments;
            }
        }

        // 打ちかけの 1 文字だけ (how r u の r): 1 文字の語の決まり (前が英語なら r・u は英字) で見る。
        if n == 0 && !kana_input && pending.chars().count() == 1 {
            let first = pending.chars().next().unwrap_or('\0');
            if first.is_ascii_lowercase()
                && self.is_english_span(
                    pending,
                    true,
                    before_score(&segments, japanese_start, 0, preceding_english, english_sentence),
                    following_english,
                    true,
                    level,
                    final_,
                    false,
                    None,
                    false,
                )
            {
                return vec![CompositionSegment {
                    is_english: true,
                    kana: String::new(),
                    raw: pending.to_string(),
                }];
            }
        }
        // Shift を押して打った入力途中の子音 (W, K) は大文字のまま英字で見せる (かなの読み途中として小文字にしない)。
        if !pending.is_empty() && pending.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
            if japanese_start < n {
                segments.push(japanese(units, japanese_start, n, ""));
            }
            segments.push(CompositionSegment {
                is_english: true,
                kana: String::new(),
                raw: pending.to_string(),
            });
            return segments;
        }
        if japanese_start < n || !pending.is_empty() || segments.is_empty() {
            segments.push(japanese(units, japanese_start, n, pending));
        }
        segments
    }

    /// 英語とも日本語とも読める語か (i, sushi, make, repo): 英単語で、ローマ字としても最後まで読める
    /// (母音か ん で終わる)。確定した後で前後の文脈と食い違ったら、確定し直す対象になる。
    pub fn is_ambiguous_word(&self, raw: &str) -> bool {
        if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_alphabetic()) || raw.chars().any(|c| c.is_ascii_uppercase()) {
            return false;
        }
        let lower = raw.to_lowercase();
        if !self.english.words.contains_word(&lower) || self.proper.contains(&lower) {
            return false;
        }
        let analysis = self.romaji.analyze(&lower);
        analysis.is_valid && (analysis.partial.is_empty() || analysis.partial == "n")
    }

    /// 確実に英語の語か (want, google, Tokyo): ローマ字として読めない・子音で終わる英単語・固有名詞・
    /// 大文字で始まる。前後の文脈にかかわらず英語なので、直前に確定した語を確定し直す根拠にできる。
    pub fn is_definitely_english(&self, raw: &str) -> bool {
        if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        if raw.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
            return true;
        }
        let lower = raw.to_lowercase();
        if self.proper.contains(&lower) {
            return true;
        }
        let analysis = self.romaji.analyze(&lower);
        if !analysis.is_valid {
            return self.english.words.contains_word(&lower)
                || self.english.is_prefix(&lower)
                || lower.chars().count() >= 4;
        }
        // 確定するときに呼ぶので、語は打ち終わっている (it が itai の打ちかけかは気にしない)。
        let word = self.english.words.contains_word(&lower) || self.is_spell_word(&lower);
        word && !analysis.partial.is_empty() && analysis.partial != "n"
    }

    /// Space を押した時点で、かなにならない子音が残る英単語か (my, by, meeting)。日本語として変換しても
    /// 子音が残るだけなので、英語として確定して空白を入れる。
    pub fn is_english_at_word_end(&self, raw: &str, level: DetectionLevel) -> bool {
        if level == DetectionLevel::Manual || raw.chars().count() < 2 || !raw.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let lower = raw.to_lowercase();
        let analysis = self.romaji.analyze(&lower);
        if !analysis.is_valid || analysis.partial.is_empty() || analysis.partial == "n" || analysis.partial == "nn" {
            return false;
        }
        self.english.words.contains_word(&lower) || self.is_spell_word(&lower)
    }

    /// スペルチェッカーが正しいと言う英単語か、よくある打ち間違い (teh、recieve) か。
    fn is_spell_word(&self, lower: &str) -> bool {
        self.spell_checker.as_ref().map_or(false, |checker| {
            checker.is_word(lower) || checker.auto_correction(lower).is_some()
        })
    }

    /// よくある英語の打ち間違いなら正しい綴り (teh → the)。大文字で始まる語は大文字で始める。
    pub fn english_auto_correction(&self, word: &str) -> Option<String> {
        if word.chars().count() < 2 || !word.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let right = self.spell_checker.as_ref()?.auto_correction(&word.to_lowercase())?;
        if word.chars().all(|c| c.is_ascii_uppercase()) && word.chars().count() > 1 {
            return Some(right.to_uppercase());
        }
        if word.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
            let mut chars = right.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
            return Some(first + chars.as_str());
        }
        Some(right)
    }

    /// 同梱の英単語の辞書・固有名詞にある語か、ユーザーが英字に直して覚えた語か (ok、github)。
    /// スペルチェッカーは使わない。
    pub fn is_listed_english_word(&self, lower: &str) -> bool {
        lower.chars().count() >= 2
            && self.memory.as_ref().and_then(|m| m.get(lower)).unwrap_or(
                self.english.words.contains_word(lower) || self.proper.contains(lower),
            )
    }

    /// 知っている英単語か (同梱の辞書・固有名詞・ユーザーが英字に直して覚えた語・4 文字以上ならスペルチェッカー)。
    /// python + no の n のように、英単語の最後の n と次の音がくっつくのを防ぐのに使う。
    pub fn is_known_english_word(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        if lower.chars().count() < 3 || !lower.chars().all(|c| c.is_ascii_lowercase()) {
            return false;
        }
        if let Some(learned) = self.memory.as_ref().and_then(|m| m.get(&lower)) {
            return learned;
        }
        self.english.words.contains_word(&lower)
            || self.proper.contains(&lower)
            || (lower.chars().count() >= 4 && self.is_spell_word(&lower))
    }

    /// - を付けて使う英語の接頭辞か (e、re、co …)。英数状態で、- の後を見てから英語か決めるのに使う。
    pub fn is_hyphen_prefix(lower: &str) -> bool {
        HYPHEN_PREFIXES.contains(&lower)
    }

    /// - の入った英単語か (e-mail、co-op、re-do、x-ray)。
    pub fn is_hyphenated_english_word(&self, lower: &str) -> bool {
        if HYPHENATED_WORDS.contains(&lower) {
            return true;
        }
        let Some(dash) = lower.find('-') else {
            return false;
        };
        if dash == 0 || lower[dash + 1..].contains('-') {
            return false;
        }
        let rest = &lower[dash + 1..];
        HYPHEN_PREFIXES.contains(&&lower[..dash])
            && rest.chars().count() >= 3
            && rest.chars().all(|c| c.is_ascii_lowercase())
            && self.is_known_english_word(rest)
    }

    /// 大文字で始まる語 (Shift を押して打った) の後ろに日本語が続いているなら、その語の終わり (単位の位置)。
    /// 無ければ -1。大文字で始まる区間は末尾まで英語になるので、そのままでは
    /// AutoIMEnotesuto → 全部英字 になってしまう。語は、知っている英単語 (Github) か、大文字で終わる語
    /// (AutoIME, OK, NHK)。後ろは 3 文字以上の小文字で、ローマ字として読めるもの。
    fn capitalized_word_end(&self, units: &[CompositionUnit], start: usize, pending: &str, final_: bool) -> isize {
        // 見るのは、次の記号・数字・空白まで (長い文の OCR|woshi, … では、後ろの , までの woshi を見る)
        let mut n = start;
        while n < units.len() && units[n].raw.chars().all(|c| c.is_ascii_alphabetic()) && !units[n].raw.is_empty() {
            n += 1;
        }
        let pending = if n < units.len() { "" } else { pending };
        let whole = raw(units, start, n) + pending;
        // 2 文字目が大文字の語 (iPad、iPhone、eSports) も、知っている語なら同じように区切る (iPad|deii → iPad でいい)
        let whole_chars: Vec<char> = whole.chars().collect();
        let lower_start = whole_chars.len() >= 2 && whole_chars[0].is_ascii_lowercase() && whole_chars[1].is_ascii_uppercase();
        if whole_chars.is_empty() || !(whole_chars[0].is_ascii_uppercase() || lower_start) {
            return -1;
        }
        // 全体が英単語・固有名詞 (Tokyo, Github) なら区切らない。
        if self.is_known_capitalized_word(&whole) {
            return -1;
        }
        let mut k = n as isize - 1;
        while k > start as isize {
            let ku = k as usize;
            let head = raw(units, start, ku);
            if !head.chars().all(|c| c.is_ascii_alphabetic()) {
                k -= 1;
                continue;
            }
            if lower_start && !(head.chars().count() >= 3 && self.is_known_capitalized_word(&head)) {
                k -= 1;
                continue;
            }
            // 後ろは小文字のローマ字 (長音の - を含んでもよい: TSyu-za- の yu-za-)。
            let rest = raw(units, ku, n) + pending;
            // 後ろが助詞 1 つだけ (OCR|wo、English|ga) なら 2 文字でもよい
            let rest_is_particle = starts_with_particle(&rest).map_or(false, |p| p == rest);
            let rest_chars: Vec<char> = rest.chars().collect();
            if (rest_chars.len() < 3 && !rest_is_particle)
                || !rest.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                || !rest_chars.first().map_or(false, |c| c.is_ascii_lowercase())
            {
                k -= 1;
                continue;
            }
            // 知っている英単語・略語 (English、OCR) の後ろが助詞で始まるなら、その後ろに英単語が続いても
            // (English|wo|happy) 区切る
            let head_len = head.chars().count();
            let known_head = (head_len >= 2 && head.chars().last().map_or(false, |c| c.is_ascii_uppercase()))
                || (head_len >= 3 && self.is_known_capitalized_word(&head))
                || (head_len >= 4 && self.is_spell_word(&head.to_lowercase()));
            if known_head && starts_with_particle(&rest).is_some() {
                return k;
            }
            let analysis = self.romaji.analyze_fragment(&rest.replace('-', ""));
            if !analysis.is_valid || (final_ && !analysis.partial.is_empty() && analysis.partial != "n") {
                k -= 1;
                continue;
            }
            // 大文字で終わる略語 (OCR)、知っている語 (Tokyo)、スペルチェッカーの 4 文字以上の語 (English)
            if known_head {
                return k;
            }
            // 大文字 1 文字 + 助詞で始まるローマ字 (A|nisiyouka → Aにしようか、B|noan → Bの案)。
            // 名前 (Tanaka、Hanako) を区切らないよう、後ろが助詞で始まるときだけ。
            if head_len == 1
                && head.chars().next().map_or(false, |c| c.is_ascii_uppercase())
                && starts_with_particle(&rest.replace('-', "")).is_some()
            {
                return k;
            }
            k -= 1;
        }
        // 後ろに大文字で始まる語が続く (Japanese|to|English…) と、後ろが小文字だけにならず上では区切れない。
        // 次の大文字の前までを見て、知っている語 + 助詞で始まるローマ字 (Japanese|to) なら区切る。
        let mut camel = start + 1;
        while camel < n && !units[camel].raw.chars().any(|c| c.is_ascii_uppercase()) {
            camel += 1;
        }
        let mut k = camel as isize - 1;
        while camel < n && k > start as isize {
            let ku = k as usize;
            let head = raw(units, start, ku);
            let rest = raw(units, ku, camel);
            let rest_ok = rest.chars().all(|c| c.is_ascii_lowercase())
                && starts_with_particle(&rest).is_some()
                && self.romaji.analyze_fragment(&rest).is_valid;
            if rest_ok {
                let head_len = head.chars().count();
                if (head_len >= 3 && self.is_known_capitalized_word(&head))
                    || (head_len >= 4 && self.is_spell_word(&head.to_lowercase()))
                {
                    return k;
                }
            }
            k -= 1;
        }
        -1
    }

    /// start から始まる - の入った英単語 (e-mail、co-op) の終わり。無ければ -1。
    /// 後ろに日本語が続いてもよい (e-mail|de) ので、- の後ろは長い方から英単語になる所を探す。
    fn hyphenated_word_end(&self, units: &[CompositionUnit], start: usize, pending: &str) -> isize {
        let n = units.len();
        // 日本語を打っている最中のローマ字列では、明示した英数字表記だけを英語として切り出す。
        let after_japanese_romaji = start > 0
            && !units[start - 1].raw.is_empty()
            && units[start - 1].raw.chars().all(|c| c.is_ascii_alphabetic())
            && units[start - 1].kana != units[start - 1].raw;
        if start > 0
            && !units[start - 1].raw.is_empty()
            && units[start - 1].raw.chars().all(|c| c.is_ascii_alphabetic())
            && !after_japanese_romaji
        {
            return -1;
        }
        let mut dash = start;
        while dash < n && !units[dash].raw.is_empty() && units[dash].raw.chars().all(|c| c.is_ascii_alphabetic()) {
            dash += 1;
        }
        if dash == start || dash >= n || units[dash].raw != "-" {
            return -1;
        }
        let mut end = n;
        while end > dash + 1 {
            let mut word = raw(units, start, end);
            if end == n {
                word.push_str(pending);
            }
            let word = word.to_lowercase();
            let hit = if after_japanese_romaji {
                NUMERIC_HYPHENATED_WORDS.contains(&word.as_str())
            } else {
                self.is_hyphenated_english_word(&word)
            };
            if hit {
                return end as isize;
            }
            end -= 1;
        }
        -1
    }

    /// 英語の短縮形 (don't, it's, I'm, you're, we'll, can't)。' の前が英単語か n't の形。
    fn is_contraction(&self, span: &str) -> bool {
        let Some(apostrophe) = span.find('\'') else {
            return false;
        };
        if apostrophe == 0 || Some(apostrophe) != span.rfind('\'') {
            return false;
        }
        let stem = span[..apostrophe].to_lowercase();
        let suffix = span[apostrophe + 1..].to_lowercase();
        // 語の最後の g を ' にした書き方 (swingin' = swinging、rockin'): stem + g が英単語なら英語
        if suffix.is_empty()
            && stem.chars().count() >= 3
            && stem.ends_with("in")
            && stem.chars().all(|c| c.is_ascii_lowercase())
        {
            return self.is_known_english_word(&(stem + "g"));
        }
        if !stem.chars().all(|c| c.is_ascii_lowercase())
            || !matches!(suffix.as_str(), "t" | "s" | "re" | "ve" | "ll" | "d" | "m")
        {
            return false;
        }
        if suffix == "t" {
            return stem.chars().count() >= 2 && stem.ends_with('n');
        }
        stem == "i" || self.english.words.contains_word(&stem) || self.proper.contains(&stem)
    }

    /// ローマ字として読めない、5 文字以上の英単語 (alcoholic, pressure)。
    fn is_long_english_word(&self, raw: &str) -> bool {
        if raw.chars().count() < 5 || !raw.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let lower = raw.to_lowercase();
        if self.romaji.analyze(&lower).is_valid {
            return false;
        }
        self.english.words.contains_word(&lower) || self.proper.contains(&lower) || self.is_spell_word(&lower)
    }

    /// 知らない英字の語 + 助詞 (grokga = grok + が) か。語の部分は 3 文字以上でローマ字として読めないもの、
    /// 全体は辞書に無いもの。語の最後の子音と助詞の頭が同じ (lot + to = ろっと) なら っ の綴りなので除く。
    fn ends_with_particle_after_unknown_word(&self, lower: &str) -> bool {
        if self.english.words.contains_word(lower)
            || self.proper.contains(lower)
            || self.memory.as_ref().and_then(|m| m.get(lower)) == Some(true)
            || self.is_spell_word(lower)
        {
            return false;
        }
        for particle in TRAILING_PARTICLES {
            if !lower.ends_with(particle) {
                continue;
            }
            let stem = &lower[..lower.len() - particle.len()];
            let particle_first = particle.chars().next().unwrap_or('\0');
            if stem.chars().count() < 3 || stem.chars().last() == Some(particle_first) {
                return false;
            }
            return !self.romaji.analyze(stem).is_valid;
        }
        false
    }

    /// 最初の 3 文字以内でローマ字として読めなくなる、4 文字以上の語 (日本語の打ち間違いでもないもの)。
    fn is_unknown_english_word(&self, raw: &str) -> bool {
        if raw.chars().count() < 4 || !raw.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let lower = raw.to_lowercase();
        let analysis = self.romaji.analyze(&lower);
        if analysis.is_valid {
            return false;
        }
        // 小書き文字などの綴り (kaxnji, ulo) まで含めれば読めるなら、日本語を打っている。
        if self.romaji.analyze_fragment(&lower).is_valid {
            return false;
        }
        let first_invalid: usize = analysis.tokens.iter().map(|t| t.romaji.chars().count()).sum();
        if first_invalid >= 3 {
            return false;
        }
        // 読めない文字の後ろが普通のローマ字なら、英字 1 文字 + 日本語 (sだけが, Xがわかる) を打っている。
        let rest: String = lower.chars().skip(first_invalid + 1).collect();
        if rest.chars().count() >= 3 && self.romaji.analyze_fragment(&rest).is_valid {
            return false;
        }
        let mut typo = Vec::new();
        self.typo.evaluate(&lower, &mut typo);
        typo.is_empty()
    }

    fn is_known_capitalized_word(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        self.memory.as_ref().and_then(|m| m.get(&lower)) == Some(true)
            || self.english.words.contains_word(&lower)
            || self.proper.contains(&lower)
    }

    /// 英語の区間 + 数字のすぐ後ろ (part1026|beta) から始まる英字の並びが英単語なら、その終わり。違えば -1。
    /// segments・japanese_start は FindSpans の途中の状態 (数字が、直前の英語の区間のすぐ後ろにあるかを見る)。
    fn alphanumeric_suffix_end(
        &self,
        units: &[CompositionUnit],
        start: usize,
        pending: &str,
        segments: &[CompositionSegment],
        japanese_start: usize,
    ) -> isize {
        fn is_digit(unit: &CompositionUnit) -> bool {
            let mut chars = unit.raw.chars();
            matches!((chars.next(), chars.next()), (Some(d), None) if d.is_ascii_digit())
        }
        if start == 0 || !is_digit(&units[start - 1]) {
            return -1;
        }
        if units[start].raw.is_empty() || !units[start].raw.chars().next().map_or(false, |c| c.is_ascii_alphabetic()) {
            return -1;
        }
        let mut k = start as isize - 1;
        while k >= 0 && is_digit(&units[k as usize]) {
            k -= 1;
        }
        if k < 0 || segments.is_empty() || !segments.last().map_or(false, |s| s.is_english) || japanese_start != (k + 1) as usize {
            return -1;
        }
        let mut end = start;
        while end < units.len() && !units[end].raw.is_empty() && units[end].raw.chars().all(|c| c.is_ascii_alphabetic()) {
            end += 1;
        }
        let mut word = raw(units, start, end);
        if end == units.len() {
            word.push_str(pending);
        }
        if starts_with_particle(&word.to_lowercase()).is_some() || !self.is_known_english_word(&word) {
            return -1;
        }
        end as isize
    }

    /// かな入力 (JIS) の区間が英語か。打ったキーの英字 (Raw) が英単語で、かなとしては日本語の語に
    /// ならないなら英語。かなとしても日本語の語 (の先頭) になるなら、ローマ字入力の
    /// 「英語とも日本語とも読める語」と同じく前後の文脈で決める。
    #[allow(clippy::too_many_arguments)]
    fn is_english_span_kana(
        &self,
        span: &str,
        kana_text: &str,
        at_end: bool,
        before: i32,
        after: Option<bool>,
        level: DetectionLevel,
        final_: bool,
    ) -> bool {
        if span.is_empty() || !span.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let lower = span.to_lowercase();
        let len = lower.chars().count();
        let in_dictionary = self.english.words.contains_word(&lower) || (len >= 4 && self.proper.contains(&lower));
        // キー列が偶然スペルチェッカーの語になることがあるので、スペルチェッカーの語は 4 文字以上だけ。
        let word = in_dictionary || (len >= 4 && self.is_spell_word(&lower));
        let prefix =
            at_end && !final_ && level == DetectionLevel::Aggressive && len >= 4 && self.english.is_prefix(&lower);

        // Shift を押して打った大文字で始まる語は英語 (手動でも)。
        if span.chars().next().map_or(false, |c| c.is_ascii_uppercase()) && (word || prefix || at_end) {
            return true;
        }
        if level == DetectionLevel::Manual {
            return false;
        }
        if let Some(learned) = self.memory.as_ref().and_then(|m| m.get(&lower)) {
            return learned;
        }
        if !(word || prefix) || len < 2 {
            return false;
        }

        let japanese = self.kana.as_ref().map_or(false, |k| k.is_japanese_word_or_prefix(kana_text));
        let context = (if after == Some(false) { before.min(1) } else { before }) + score(after);
        if context >= (if level == DetectionLevel::Conservative { 2 } else { 1 }) {
            return true;
        }
        if japanese || context < 0 {
            return false;
        }
        let minimum = match level {
            DetectionLevel::Aggressive => 2,
            DetectionLevel::Conservative => 4,
            _ => 3,
        };
        len >= minimum
    }

    /// 区間が英語か。
    ///
    /// - `final_`: 打ち終わった (Space・Enter)。末尾の区間でも、英単語の打ちかけ (amaz) は英語の根拠にしない。
    /// - `unreadable`: 区間にローマ字として読めなかった英字がある (zoom + de の m、bug + wo の g)。
    /// - `ends_word`: 区間の後ろが記号だけ (let's go! の go)。語はそこで打ち終わっているので、短い語も末尾の語と同じく見る。
    #[allow(clippy::too_many_arguments)]
    fn is_english_span(
        &self,
        span: &str,
        at_end: bool,
        before: i32,
        after: Option<bool>,
        start_of_input: bool,
        level: DetectionLevel,
        final_: bool,
        unreadable: bool,
        next: Option<&str>,
        ends_word: bool,
    ) -> bool {
        // まだ続きを打つかもしれない末尾の区間 (打ちかけの英単語を英語と見てよい)。
        let growing = at_end && !final_;
        if self.is_contraction(span) {
            return level != DetectionLevel::Manual
                || span.chars().next().map_or(false, |c| c.is_ascii_uppercase());
        }
        if span.is_empty() || !span.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let lower = span.to_lowercase();
        let lower_chars: Vec<char> = lower.chars().collect();
        // 小文字で始まって途中に大文字がある区間 (iPC、meteSNS) は、固有名詞の書き方 (iPhone・eBay・macOS)
        // でなければ 1 語ではない: 小文字の部分は前の日本語の続き (atarashi|i|PC → あたらしいPC、
        // motome|te|SNS → 求めてSNS)。
        if span.chars().next().map_or(false, |c| c.is_ascii_lowercase())
            && span.chars().skip(1).any(|c| c.is_ascii_uppercase())
            && self.proper.canonical(&lower) != Some(span)
        {
            return false;
        }
        let last = *lower_chars.last().unwrap_or(&'\0');
        let next_first = next.and_then(|n| n.chars().next());
        // 短い区間の最後の子音が、次の音と合わせて っ になる (u|lo|t + ti = ぉっち) なら、英単語 (lot) ではなく
        // 日本語の途中。
        if lower_chars.len() <= 4
            && next_first.map_or(false, |c| c.eq_ignore_ascii_case(&last))
            && !matches!(last, 'a' | 'i' | 'u' | 'e' | 'o' | 'n')
        {
            let head: String = lower_chars[..lower_chars.len() - 1].iter().collect();
            let head_analysis = self.romaji.analyze_fragment(&head);
            if head_analysis.is_valid && head_analysis.partial.is_empty() {
                return false;
            }
        }
        // 数字のすぐ前の、ローマ字として読めない子音で始まる短い英字 (kaibunsho|rta|2026、ps5): 略語。
        // 日本語の打ちかけではない。
        if next_first.map_or(false, |c| c.is_ascii_digit())
            && (2..=6).contains(&span.chars().count())
            && span.chars().all(|c| c.is_ascii_alphabetic())
        {
            let first_two: String = lower_chars.iter().take(2).collect();
            if !self.romaji.analyze_fragment(&first_two).is_valid {
                return true;
            }
        }
        // 日本語のすぐ後ろで、助詞 + 英単語 (dochira|mo|user の mouser) は、スペルチェッカーが 1 語と言っても
        // 助詞 + 英単語 (同梱の英語の辞書の語は除く)。後ろの英単語の区間は、この後で別に見る。
        if before < 0 {
            if let Some(leading) = starts_with_particle(&lower) {
                let rest = &lower[leading.len()..];
                if lower.len() - leading.len() >= 3
                    && !self.english.words.contains_word(&lower)
                    && self.is_known_english_word(rest)
                {
                    return false;
                }
            }
        }

        let in_dictionary = self.english.words.contains_word(&lower);
        // 知らない英字の語 + 助詞 (grok|ga、figma|de) は、助詞までを 1 語にしない。語の部分だけの区間は
        // この後で別に見る。
        if self.ends_with_particle_after_unknown_word(&lower) {
            return false;
        }
        let conservative = level == DetectionLevel::Conservative;
        // 小書き文字の綴り (mala = まぁ, xtu = っ) で最後まで読める語は、日本語をわざわざ打っている。
        // 同梱の辞書の英単語以外は日本語。(6 文字以上のスペルチェッカーの英単語は除く: chocolate の la = ぁ でも英語)
        let analysis = self.romaji.analyze(&lower);
        let fragment = self.romaji.analyze_fragment(&lower);
        let small_kana_spelling = !analysis.is_valid
            && fragment.is_valid
            && fragment.partial.is_empty()
            && !(lower.chars().count() >= 6 && self.is_spell_word(&lower));
        let spell_word = !in_dictionary && !small_kana_spelling && self.is_spell_word(&lower);
        let exact = in_dictionary || (spell_word && !analysis.is_valid);
        let prefix = growing
            && lower.chars().count() >= 4
            && !conservative
            && !small_kana_spelling
            && self.english.is_prefix(&lower);

        // 大文字で始まる語 (Shift を押して打った) は固有名詞や英文。1 文字 (I) でも、末尾まで打っている
        // 途中でも英語。手動でもこれだけは英語にする (Shift を押したのはユーザーの明示的な指定)。
        // 大文字で始まる語の後ろに記号が続く (Ah! Ooh, Wow.) なら、その語で終わっている:
        // スペルチェッカーの語でも英語。
        if span.chars().next().map_or(false, |c| c.is_ascii_uppercase())
            && (exact || prefix || at_end || (spell_word && next_first.map_or(false, |c| !c.is_ascii_alphanumeric())))
        {
            return true;
        }
        if level == DetectionLevel::Manual {
            return false;
        }
        // ユーザーが英字 / かなに直して覚えた語。ただし短くてローマ字として読める語 (go、no) は、日本語の
        // すぐ後ろ (nihon|go) では使わない (一度 go を英字で確定しただけで、日本語 が にほんgo になっていた)。
        if let Some(learned) = self.memory.as_ref().and_then(|m| m.get(&lower)) {
            if !(learned && before < 0 && lower.chars().count() <= 3 && fragment.is_valid && fragment.partial.is_empty()) {
                return learned;
            }
        }
        // 5 文字以上の英単語で、ローマ字としても読めるもの:
        // - c 行の綴り (camera、coffee、class) は英語。日本語を打つときは k を使う (カメラ は kamera)。
        // - ローマ字として読むと ぢ・づ になる綴り (radio = らぢお、studio、audio) で、ふつうの日本語の語に
        //   ならないなら英語。(sake・tokyo・suzuki のような日本由来の語は、ふつうのかなになるので、ここでは
        //   英語にしない。) 日本語の語にもなるもの (tomato = とまと、piano = ぴあの、anime) は、
        //   今までどおり前後の文脈で決める。
        // 日本語の辞書の語 (suzuki) と、慎重なときは使わない。続きと合わせて日本語の語になる
        // (sense + i = せんせい) ときも使わない。
        if lower.chars().count() >= 5
            && !conservative
            && (in_dictionary || self.is_spell_word(&lower))
            && !self.japanese.is_prefix(&lower)
            && !self.proper.contains(&lower)
            && fragment.is_valid
        {
            let suppressed = next.is_some_and(|n| {
                !n.is_empty()
                    && self.is_common_japanese.as_ref().is_some_and(|common| {
                        !common(&lower) && common(&(lower.clone() + &n.to_lowercase()))
                    })
            });
            if !suppressed {
                if lower.contains('c') && !lower.contains("ch") {
                    return true;
                }
                let fragment_kana = fragment.kana();
                if (fragment_kana.contains('ぢ') || fragment_kana.contains('づ'))
                    && self.is_common_japanese.as_ref().map_or(true, |f| !f(&lower))
                {
                    return true;
                }
            }
        }
        // ローマ字として最後まで読めても、日本語の語にならない英単語 (feature = ふぇあつれ、remote = れもて。
        // dictionaries/english-readable.txt、#12)。日本語の語の始まりにもならない語だけを入れているので、
        // 後ろに日本語が続いても (feature|wo) 英語。
        if lower.chars().count() >= 4 && readable_english().contains_word(&lower) {
            return true;
        }
        // c 行の綴りで読める語 (care = かれ、can = かん) が日本語の途中にあるなら、日本語を打っている
        // (fucarete → ふかれて、shoucanshi → しょうかんし)。入力全体がその語だけのときは英語。
        if !(start_of_input && at_end) {
            if lower.contains('c') {
                let c_row = self.romaji.analyze(&RomajiDetector::read_c_row(&lower));
                if c_row.is_valid && (c_row.partial.is_empty() || c_row.partial == "n") {
                    return false;
                }
            }
            // v 行 (va = ゔぁ): 辞書の英単語 (video) でなければ日本語 (vanpaia → ゔぁんぱいあ → ヴァンパイア)。
            if lower.contains('v')
                && !lower.contains('l')
                && !lower.contains('x')
                && !in_dictionary
                && !self.proper.contains(&lower)
                && fragment.is_valid
                && (fragment.partial.is_empty() || fragment.partial == "n")
            {
                return false;
            }
        }
        // 1 文字は英文の中の a / i だけ。1 文字は英文の中の a / i と、チャットの略し方の u (you)・r (are) だけ。
        // 前が英語の語なら英字 (A fool a fool a, for u / how r u)。語として独立している (後ろが空白・記号・終わり)
        // ときだけ。後ろにかなが続く (HH|i|reta = HH いれた) なら日本語の 1 音。
        if span.chars().count() < 2 {
            return matches!(lower.as_str(), "a" | "i" | "u" | "r")
                && before >= 1
                && !next.is_some_and(|n| n.chars().next().map_or(false, |c| c.is_ascii_alphabetic()));
        }

        // 英語の固有名詞 (amazon, adobe, netflix) は、ローマ字として読めても英語。日本語の語と同じ綴りなら除く。
        // 短い名前 (ben, tom) の偶然の一致 (にほんごの|ben|きょう) を避けるため 4 文字以上。文の途中の区間なら
        // 5 文字以上 (きょうは|amazon|で) か、ローマ字として読めないもの。末尾の 4 文字の語は、日本語のすぐ
        // 後ろでなければ (ある程度は の teido|ha を tei|doha = Doha にしない)。慎重なら、ローマ字として
        // 読めないものだけ。
        // ただし、ローマ字として最後まで読める固有名詞 (korea = これあ) の後ろに、助詞でない日本語が続くなら、
        // 日本語の語の途中 (korea|reka → これあれか。Korea|reka にしない)。助詞が続くなら固有名詞 (korea|de → Koreaで)。
        if lower.chars().count() >= 4
            && self.proper.contains(&lower)
            && next.is_some_and(|n| n.chars().next().map_or(false, |c| c.is_ascii_alphabetic()))
            && analysis.is_valid
            && analysis.partial.is_empty()
        {
            let n = next.unwrap_or("");
            if starts_with_particle(&n.to_lowercase()).is_none() {
                return false;
            }
        }
        if lower.chars().count() >= 4
            && self.proper.contains(&lower)
            && !self.japanese.words.contains_word(&lower)
        {
            let ok = if conservative {
                !analysis.is_valid
            } else {
                (at_end && before >= 0) || lower.chars().count() >= 5 || !analysis.is_valid
            };
            if ok {
                return true;
            }
        }
        if growing
            && lower.chars().count() >= 4
            && !conservative
            && !small_kana_spelling
            && self.proper.has_prefix(&lower)
            && !self.japanese.is_prefix(&lower)
        {
            return true;
        }

        // 英単語で、ローマ字として読めない英字を含む (zoom + でかいぎ → m が読めない)。日本語の文の途中でも英語。
        // 日本語の後ろで、助詞 + 読めない英字 (の + ts: jissainotsyu) は、英単語 (not) ではなく 助詞 + 英字。
        // 助詞の後ろがローマ字の打ちかけ (ts = つ) のときだけ。ローマ字にならない (he + lp: help) なら英単語。
        if unreadable && before < 0 {
            if let Some(particle) = starts_with_particle(&lower) {
                if lower.len() - particle.len() <= 2 {
                    let rest = &lower[particle.len()..];
                    if self.romaji.analyze_fragment(rest).is_valid {
                        return false;
                    }
                }
            }
        }
        // 同梱の辞書の英単語で、ローマ字としては促音 (っ) を使わないと読めない語 (issue = いっすえ, apple) は英語。
        // 日本語の語 (の先頭) なら除く。
        if in_dictionary
            && lower.chars().count() >= 4
            && analysis.is_valid
            && analysis.sokuon > 0
            && !self.japanese.is_prefix(&lower)
        {
            return true;
        }
        // 2 文字でも、同梱の辞書の語で読めない英字がある (ok + notasuku の k) なら英語。
        if unreadable && (exact || spell_word) && (lower.chars().count() >= 3 || in_dictionary) {
            return true;
        }

        // 英語とも日本語とも読める語 (sushi, repo, make) は前後の両方で決める。前が英語なら +1・日本語なら -1、
        // 後ろも同じように数え、合計が必要な点数に届けば英語 (どちらも分からない・食い違うときは日本語)。
        // 入力の先頭で末尾まで打っている途中なら英単語の先頭でも数える (英文の続きを打っている途中を英字で見せる)。
        // 変換ボックス内の英語区間の続きは、偶然の一致 (google + de) を避けるため 3 文字以上の英単語そのものだけ。
        // 助詞などと同じ 2 文字の語 (no, to, ga) は、前後の両方が英語のときだけ (標準)。
        let ambiguous = match level {
            // 積極的でも、助詞と同じ形の 2 文字 (ni, ga) は英単語の先頭というだけでは英語にしない。
            DetectionLevel::Aggressive => {
                if start_of_input && at_end {
                    exact || spell_word || (!final_ && lower.chars().count() >= 3 && self.english.is_prefix(&lower))
                } else {
                    exact || spell_word
                }
            }
            DetectionLevel::Conservative => (exact || spell_word) && (lower.chars().count() >= 3 || before >= 2),
            _ => {
                if start_of_input && ends_word {
                    exact || spell_word
                } else if start_of_input && at_end {
                    exact
                        || spell_word
                        || lower.chars().count() == 1
                        || (!final_ && !small_kana_spelling && self.english.is_prefix(&lower))
                } else {
                    (exact || spell_word) && lower.chars().count() >= 3
                }
            }
        };
        let mut needed = match level {
            DetectionLevel::Aggressive => 1,
            DetectionLevel::Conservative => 2,
            // 助詞と同じ形の 2 文字の語 (no, to, ga) は両側が必要。子音で終わる語 (is, at, my) は
            // 日本語の語にならないので片側でよい。
            _ => {
                if lower.chars().count() <= 2 && analysis.is_valid && (analysis.partial.is_empty() || analysis.partial == "n") {
                    2
                } else {
                    1
                }
            }
        };
        // スペルチェッカーだけが知っている、最後までローマ字として読める語 (shite, kore) は、前の英単語 1 つ
        // (push) では足りない (pushshite → pushして)。英文の続き (+2) か、前後の両方が英語のときだけ。
        if spell_word && !in_dictionary && analysis.is_valid && analysis.partial.is_empty() {
            needed = needed.max(2);
        }
        // 前が英文でも、後ろが日本語なら英文の強さは数えない (I love |sushi| が好き → 食い違うので日本語)。
        let context = if after == Some(false) { before.min(1) } else { before };
        if ambiguous && context + score(after) >= needed {
            return true;
        }
        if !exact && !prefix && !spell_word {
            return false;
        }

        if !analysis.is_valid {
            if !exact && !prefix {
                return false;
            }
            if !exact && small_kana_spelling {
                return false;
            }
            // 日本語のすぐ後ろの 2 文字の語で、変換ボックスでは読める綴り (こ + we = こうぇ、wi = うぃ) は日本語。
            if small_kana_spelling && lower.chars().count() <= 2 && before < 0 {
                return false;
            }
            // ローマ字として読めない英単語。途中の区間は 3 文字以上だけ (短い語の偶然の一致を避ける)。
            return (at_end && (exact || !conservative)) || span.chars().count() >= 3;
        }
        // ローマ字として読めても、末尾が子音の英単語 (git, zoom, about) で、日本語の語の途中でもないなら英語。
        // スペルチェッカーだけが知っている語 (meeting, my) は、前が日本語でないときだけ。
        // 打ち終わっていれば、日本語の語の打ちかけ (it → itai) かどうかは気にしなくてよい。
        let not_japanese_prefix = final_ || !self.japanese.is_prefix(&lower);
        if spell_word
            && at_end
            && before >= 0
            && !analysis.partial.is_empty()
            && analysis.partial != "n"
            && not_japanese_prefix
        {
            return true;
        }
        at_end
            && exact
            && (!conservative || lower.chars().count() >= 3)
            && !analysis.partial.is_empty()
            && analysis.partial != "n"
            && not_japanese_prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(kana: &str, raw: &str) -> CompositionUnit {
        CompositionUnit {
            kana: kana.to_string(),
            raw: raw.to_string(),
        }
    }

    fn detector() -> &'static CompositionDetector {
        static DETECTOR: OnceLock<CompositionDetector> = OnceLock::new();
        DETECTOR.get_or_init(|| CompositionDetector::create_default(None))
    }

    fn segment_balanced(det: &CompositionDetector, units: &[CompositionUnit], pending: &str) -> Vec<CompositionSegment> {
        det.segment(units, pending, None, None, DetectionLevel::Balanced, false, false, false)
    }

    /// 表示 (英語区間は英字、日本語区間はかな) を作る。
    fn render(segments: &[CompositionSegment]) -> String {
        segments
            .iter()
            .map(|s| if s.is_english { s.raw.clone() } else { s.kana.clone() })
            .collect()
    }

    /// 「きょうは github に push した」の区間分け。英語区間だけ英字になる。
    #[test]
    fn segments_kyouha_github_ni_push_shita() {
        let det = detector();
        // 打鍵の流れから作られる単位 (きょ う は | ぎ てゅ b | に | ぷ s h | し た)。
        let units = vec![
            unit("きょ", "kyo"),
            unit("う", "u"),
            unit("は", "ha"),
            unit("ぎ", "gi"),
            unit("てゅ", "thu"),
            unit("b", "b"),
            unit("に", "ni"),
            unit("ぷ", "pu"),
            unit("s", "s"),
            unit("h", "h"),
            unit("し", "shi"),
            unit("た", "ta"),
        ];
        let segments = segment_balanced(det, &units, "");
        // 日本語区間はかな、英語区間は打った英字で表示される。
        let expected = [
            ("きょうは", false),
            ("github", true),
            ("に", false),
            ("push", true),
            ("した", false),
        ];
        assert_eq!(segments.len(), expected.len(), "{segments:?}");
        for (segment, (text, is_english)) in segments.iter().zip(expected) {
            assert_eq!(segment.is_english, is_english, "{segments:?}");
            let display = if segment.is_english { segment.raw.clone() } else { segment.kana.clone() };
            assert_eq!(display, text, "{segments:?}");
        }
        assert_eq!(render(&segments), "きょうはgithubにpushした");
    }

    /// ドメインの . の後ろ (tetr.io) は短いトップレベルドメインでも英字のまま。
    #[test]
    fn segments_domain_tetr_io() {
        let det = detector();
        let units = vec![
            unit("て", "te"),
            unit("t", "t"),
            unit("r", "r"),
            unit("。", "."),
            unit("い", "i"),
            unit("お", "o"),
        ];
        let segments = segment_balanced(det, &units, "");
        assert!(segments.iter().all(|s| s.is_english), "{segments:?}");
        assert_eq!(render(&segments), "tetr.io");
    }

    /// @ の後ろのメンション (@name) は英字のまま。
    #[test]
    fn segments_mention_username() {
        let det = detector();
        let units = vec![unit("@", "@"), unit("な", "na"), unit("め", "me")];
        let segments = segment_balanced(det, &units, "");
        assert_eq!(segments.len(), 2, "{segments:?}");
        assert!(!segments[0].is_english);
        assert_eq!(segments[0].raw, "@");
        assert!(segments[1].is_english);
        assert_eq!(segments[1].raw, "name");
    }

    /// _ の入ったユーザー名 (upah_setu) はローマ字として読めても英字のまま。
    #[test]
    fn segments_underscore_username() {
        let det = detector();
        let units = vec![
            unit("u", "u"),
            unit("ぱ", "pa"),
            unit("h", "h"),
            unit("＿", "_"),
            unit("せ", "se"),
            unit("つ", "tu"),
        ];
        let segments = segment_balanced(det, &units, "");
        assert_eq!(segments.len(), 1, "{segments:?}");
        assert!(segments[0].is_english);
        assert_eq!(segments[0].raw, "upah_setu");
    }

    /// - の入った英単語 (e-mail) は英語。
    #[test]
    fn segments_hyphenated_email() {
        let det = detector();
        let units = vec![
            unit("e", "e"),
            unit("ー", "-"),
            unit("ま", "ma"),
            unit("i", "i"),
            unit("l", "l"),
        ];
        let segments = segment_balanced(det, &units, "");
        assert_eq!(segments.len(), 1, "{segments:?}");
        assert!(segments[0].is_english);
        assert_eq!(segments[0].raw, "e-mail");
    }

    /// 大文字で始まる語 (GitHub) は英語。入力途中の子音 (pending) も含めて英字。
    #[test]
    fn segments_capitalized_github() {
        let det = detector();
        let units = vec![unit("ぎ", "Gi"), unit("てゅ", "tHu")];
        let segments = det.segment(&units, "b", None, None, DetectionLevel::Balanced, false, false, false);
        assert_eq!(segments.len(), 1, "{segments:?}");
        assert!(segments[0].is_english);
        assert_eq!(segments[0].raw, "GitHub");
    }

    /// 英語とも日本語とも読める 2 文字 (go) は、前後の両方が英語のときだけ英語。
    #[test]
    fn segments_ambiguous_go_uses_context() {
        let det = detector();
        let units = vec![unit("ご", "go")];
        let alone = segment_balanced(det, &units, "");
        assert!(!alone[0].is_english, "{alone:?}");
        let both = det.segment(&units, "", Some(true), Some(true), DetectionLevel::Balanced, false, false, false);
        assert!(both[0].is_english, "{both:?}");
        let front_only = det.segment(&units, "", Some(true), Some(false), DetectionLevel::Balanced, false, false, false);
        assert!(!front_only[0].is_english, "{front_only:?}");
    }

    /// 助詞と同じ形の no も、前後が英語なら英語。
    #[test]
    fn segments_ambiguous_no_uses_context() {
        let det = detector();
        let units = vec![unit("の", "no")];
        let both = det.segment(&units, "", Some(true), Some(true), DetectionLevel::Balanced, false, false, false);
        assert!(both[0].is_english, "{both:?}");
        let alone = segment_balanced(det, &units, "");
        assert!(!alone[0].is_english, "{alone:?}");
    }

    /// かな入力でも、打ったキーの英字 (github) が英単語なら英語。
    #[test]
    fn segments_kana_input_english_word() {
        let det = detector();
        let units = vec![
            unit("き", "g"),
            unit("く", "i"),
            unit("た", "t"),
            unit("く", "h"),
            unit("な", "u"),
            unit("こ", "b"),
        ];
        let segments = det.segment(&units, "", None, None, DetectionLevel::Balanced, false, true, false);
        assert_eq!(segments.len(), 1, "{segments:?}");
        assert!(segments[0].is_english);
        assert_eq!(segments[0].raw, "github");
    }

    /// 公開ヘルパー (確定し直しの判定に使うもの)。
    #[test]
    fn public_helpers() {
        let det = detector();
        assert!(det.is_ambiguous_word("sushi"));
        assert!(!det.is_ambiguous_word("Sushi"));
        assert!(!det.is_definitely_english("sushi"));
        assert!(det.is_definitely_english("want"));
        assert!(det.is_definitely_english("Tokyo"));
        assert!(!det.is_definitely_english("kore"));
        assert!(det.is_english_at_word_end("git", DetectionLevel::Balanced));
        assert!(!det.is_english_at_word_end("kore", DetectionLevel::Balanced));
        assert!(det.is_known_english_word("github"));
        assert!(det.is_listed_english_word("github"));
        assert!(!det.is_known_english_word("kyouha"));
        assert!(CompositionDetector::is_hyphen_prefix("e"));
        assert!(!CompositionDetector::is_hyphen_prefix("o"));
        assert!(det.is_hyphenated_english_word("e-mail"));
        assert!(det.is_hyphenated_english_word("co-op"));
        assert!(!det.is_hyphenated_english_word("ra-men"));
    }
}
