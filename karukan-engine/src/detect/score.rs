// SPDX-License-Identifier: GPL-3.0-or-later
// Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の ScoreEngine.cs の移植。

//! 各 Detector の結果を統合して Japanese / English / Unknown (と保留継続の Undecided) を返す。
//!
//! 日本語と判定するのは
//!   JapaneseScore >= 閾値  かつ  JapaneseScore - EnglishScore >= 閾値
//! のときだけ。英語の根拠があるときは、日本語スコアが「多少高い」程度では切り替えない。

use super::kana_detector::KanaDetector;
use super::romaji_detector::RomajiDetector;
use super::{Contribution, DetectionLevel, DictionaryDetector, EnglishDetector, ProperNouns, TypoDetector, Verdict};

/// 判定への入力。
#[derive(Debug, Clone)]
pub struct DetectionInput {
    /// 打鍵を英小文字に直したもの (英字以外のキーは '#')
    pub letters: String,
    /// 仮想キーコード列 (かな入力判定用)
    pub keys: Vec<u32>,
    /// 確定操作 (Enter/space 等) が行われたか
    pub is_final: bool,
}

/// 判定の設定。
#[derive(Debug, Clone)]
pub struct DetectSettings {
    /// 日本語判定の閾値 (最低 2)
    pub japanese_threshold: i32,
    /// ローマ字入力を扱うか
    pub use_romaji: bool,
    /// かな入力を扱うか
    pub use_kana: bool,
    /// 打ち間違い検出を使うか
    pub typo_enabled: bool,
    /// 保留を続ける最大打鍵数
    pub max_pending_keys: usize,
}

impl Default for DetectSettings {
    fn default() -> Self {
        Self {
            japanese_threshold: 5,
            use_romaji: true,
            use_kana: true,
            typo_enabled: true,
            max_pending_keys: 12,
        }
    }
}

/// 判定結果。
#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub verdict: Verdict,
    pub text: String,
    pub japanese_score: i32,
    pub english_score: i32,
    pub contributions: Vec<Contribution>,
    pub summary: String,
}

/// 判定器一式を組み立てて評価するエンジン。
pub struct ScoreEngine {
    romaji: RomajiDetector,
    kana: KanaDetector,
    english: EnglishDetector,
    dictionary: DictionaryDetector,
    typo: TypoDetector,
    settings: DetectSettings,
    level: DetectionLevel,
}

impl ScoreEngine {
    /// 組み込み辞書 (+ ユーザー辞書) から一式を組み立てる。
    pub fn create_default(user_directory: Option<&std::path::Path>) -> Self {
        let romaji = RomajiDetector::new();
        let japanese_words = super::DictionarySource::load("japanese.txt", user_directory);
        let dictionary = DictionaryDetector::new(&japanese_words);
        let proper = ProperNouns::load(user_directory);
        let english_words: Vec<String> = super::DictionarySource::load("english.txt", user_directory)
            .into_iter()
            .chain(proper.lowercase_words().cloned())
            .collect();
        let english = EnglishDetector::new(english_words);
        let kana = KanaDetector::new(&japanese_words, &romaji);
        let typo = TypoDetector::new(&dictionary.words);
        Self {
            romaji,
            kana,
            english,
            dictionary,
            typo,
            settings: DetectSettings::default(),
            level: DetectionLevel::Balanced,
        }
    }

    pub fn settings(&self) -> &DetectSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: DetectSettings) {
        self.settings = settings;
    }

    pub fn set_level(&mut self, level: DetectionLevel) {
        self.level = level;
    }

    pub fn romaji(&self) -> &RomajiDetector {
        &self.romaji
    }

    pub fn dictionary(&self) -> &DictionaryDetector {
        &self.dictionary
    }

    pub fn evaluate(&self, input: &DetectionInput) -> DetectionResult {
        let settings = &self.settings;
        let threshold = settings.japanese_threshold.max(2);
        // c 行 (ca / cu / co = か く こ) は k に読み替えてローマ字・日本語の辞書で調べる。
        // 英語の判定は打ったまま。
        let letters = RomajiDetector::read_c_row(&input.letters);
        let original = input.letters.clone();
        let mut contributions: Vec<Contribution> = Vec::new();

        if letters.is_empty() {
            let verdict = if input.is_final { Verdict::Unknown } else { Verdict::Undecided };
            return Self::result(verdict, &letters, contributions, "入力なし");
        }

        let mut romaji_valid = false;
        let mut japanese_dictionary_prefix = false;
        if settings.use_romaji {
            let analysis = self.romaji.analyze(&letters);
            romaji_valid = analysis.is_valid;
            if !analysis.is_valid {
                contributions.push(Contribution::new(
                    "Romaji",
                    0,
                    if settings.use_kana { 0 } else { 5 },
                    analysis.invalid_reason.clone().unwrap_or_else(|| "ローマ字として成立しない".to_string()),
                ));
            } else {
                if analysis.strong_youon > 0 {
                    contributions.push(Contribution::new("Romaji", 3, 0, "日本語特有の拗音 (kya/ryo …)"));
                }
                if analysis.tsu > 0 {
                    contributions.push(Contribution::new("Romaji", 3, 0, "tsu"));
                }
                if analysis.long_vowels > 0 {
                    contributions.push(Contribution::new("Romaji", 1, 0, "長音 (ou/uu)"));
                }
                if analysis.sokuon > 0 {
                    contributions.push(Contribution::new("Romaji", 1, 0, "促音"));
                }

                self.dictionary.evaluate(&letters, &mut contributions);
                japanese_dictionary_prefix = self.dictionary.is_prefix(&letters);

                if settings.typo_enabled {
                    self.typo.evaluate(&letters, &mut contributions);
                }

                // 5 文字以上が最後までローマ字として読めて、英単語 (の先頭) にも当たらないなら日本語寄り。
                let len = letters.chars().count();
                if len >= 5 && analysis.partial.is_empty() && !self.english.is_prefix(&original) {
                    contributions.push(Contribution::new(
                        "Romaji",
                        if len >= 6 { 4 } else { 3 },
                        0,
                        "5 文字以上がすべてローマ字として成立し、英単語にも当たらない",
                    ));
                }
            }
        }

        let mut kana_plausible = false;
        if settings.use_kana {
            kana_plausible = self.kana.evaluate(&input.keys, &mut contributions);
        }

        self.english.evaluate(&original, &mut contributions);

        let japanese: i32 = contributions.iter().map(|c| c.japanese).sum();
        let english: i32 = contributions.iter().map(|c| c.english).sum();

        // どの入力方式としても日本語になり得ないなら、この時点で英語と確定して保留をやめる。
        if !romaji_valid && !kana_plausible {
            return Self::result(Verdict::English, &letters, contributions, "日本語の入力として成立しない");
        }

        if japanese >= threshold && japanese - english >= threshold {
            return Self::result(Verdict::Japanese, &letters, contributions, "日本語スコアが閾値を超えた");
        }

        // 英語の語 (の先頭) と一致し、日本語の語の途中でもない → これ以上待っても日本語にはならない。
        // 助詞で始まり、助詞の後ろがまだ 3 文字以下の語は、助詞 + 次の語の打ちかけかもしれないので待つ。
        let len = letters.chars().count();
        let particle_then_more = romaji_valid
            && !input.is_final
            && super::starts_with_particle(&letters).is_some_and(|particle| {
                let rest = len - particle.chars().count();
                rest > 0 && rest < 4
            });
        if english >= 3
            && japanese < threshold
            && !japanese_dictionary_prefix
            && !particle_then_more
            && !(settings.use_kana && kana_plausible)
            && len >= 2
        {
            return Self::result(Verdict::English, &letters, contributions, "英語の語と一致");
        }

        if input.is_final || len >= settings.max_pending_keys {
            return Self::result(Verdict::Unknown, &letters, contributions, "判断できない");
        }
        Self::result(Verdict::Undecided, &letters, contributions, "判定材料を収集中")
    }

    fn result(verdict: Verdict, text: &str, contributions: Vec<Contribution>, summary: &str) -> DetectionResult {
        let japanese_score = contributions.iter().map(|c| c.japanese).sum();
        let english_score = contributions.iter().map(|c| c.english).sum();
        DetectionResult {
            verdict,
            text: text.to_string(),
            japanese_score,
            english_score,
            contributions,
            summary: summary.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> ScoreEngine {
        ScoreEngine::create_default(None)
    }

    #[test]
    fn japanese_input_detected() {
        let e = engine();
        let r = e.evaluate(&DetectionInput {
            letters: "konnichiwa".to_string(),
            keys: vec![0x4B, 0x4F, 0x4E, 0x4E, 0x49, 0x43, 0x48, 0x49, 0x57, 0x41],
            is_final: false,
        });
        assert_eq!(r.verdict, Verdict::Japanese);
        assert!(r.japanese_score >= 5);
    }

    #[test]
    fn english_word_detected() {
        let e = engine();
        let r = e.evaluate(&DetectionInput {
            letters: "github".to_string(),
            keys: vec![0x47, 0x49, 0x54, 0x48, 0x55, 0x42], // G I T H U B
            is_final: false,
        });
        assert_eq!(r.verdict, Verdict::English);
    }

    #[test]
    fn ambiguous_stays_undecided() {
        let e = engine();
        let r = e.evaluate(&DetectionInput {
            letters: "ka".to_string(),
            keys: vec![0x4B, 0x41],
            is_final: false,
        });
        assert_eq!(r.verdict, Verdict::Undecided);
    }

    #[test]
    fn sushi_is_english_by_dictionary() {
        let e = engine();
        let r = e.evaluate(&DetectionInput {
            letters: "sushi".to_string(),
            keys: vec![0x53, 0x55, 0x53, 0x48, 0x49], // S U S H I
            is_final: false,
        });
        // english.txt に sushi が入っている (日本語由来の外来語)
        assert_eq!(r.verdict, Verdict::English);
    }

    #[test]
    fn kyouha_is_japanese() {
        let e = engine();
        let r = e.evaluate(&DetectionInput {
            letters: "kyouha".to_string(),
            keys: vec![0x4B, 0x59, 0x4F, 0x55, 0x48, 0x41],
            is_final: false,
        });
        assert_eq!(r.verdict, Verdict::Japanese);
    }
}
