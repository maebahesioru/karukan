// SPDX-License-Identifier: GPL-3.0-or-later
// このモジュールは Meltype (Copyright (C) 2026 Yukishiro, GPL-3.0-or-later) の
// 判定ロジック (src/Meltype.Core/Detection/) を Rust へ移植したものです。

//! 日英混在入力の自動判定。
//!
//! 「きょうは github に push した」のようなローマ字入力を、日本語区間と英語区間に
//! 分割するための判定器群。Meltype の設計に倣い、単一の判定器が直接 JP/EN を
//! 決めることはなく、各判定器がスコア (Contribution) を加点し、最終的に
//! スコアエンジンが閾値で決める。
//!
//! - 日本語辞書 (japanese.txt) との一致 → JP 加点
//! - 英語辞書 (english.txt) との一致 → EN 加点
//! - ローマ字として成立しない綴り (th, l, v, 子音連続 …) → RomajiDetector が EN 寄りに
//! - 打ち間違い (Levenshtein) → JP 加点 (補助のみ)

mod composition;
mod detectors;
mod kana_detector;
mod memory;
mod romaji_detector;
mod score;
mod word_checker;
mod word_list;

pub use composition::{CompositionDetector, CompositionSegment, CompositionUnit};
pub use detectors::{starts_with_particle, DictionaryDetector, EnglishDetector, ProperNouns, TypoDetector};
pub use kana_detector::KanaDetector;
pub use memory::{Entry, LanguageMemory, Learned};
pub use romaji_detector::{RomajiAnalysis, RomajiDetector, RomajiToken};
pub use score::{DetectSettings, DetectionInput, DetectionResult, ScoreEngine};
pub use word_checker::{BuiltInWordChecker, WordChecker};
pub use word_list::{DictionarySource, WordList};

/// 判定結果の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// まだ判断材料が足りない。保留を続ける。
    Undecided,
    Japanese,
    English,
    /// 判断できない。何もしない。
    Unknown,
}

/// 各判定器の加点。1 つの判定器が直接 Japanese/English を決めることはない。
#[derive(Debug, Clone)]
pub struct Contribution {
    /// 判定器の名前 ("Dictionary", "English", "Typo" …)
    pub source: &'static str,
    /// 日本語スコアへの加点
    pub japanese: i32,
    /// 英語スコアへの加点
    pub english: i32,
    /// 理由 (ログ用)
    pub reason: String,
}

impl Contribution {
    pub fn new(source: &'static str, japanese: i32, english: i32, reason: impl Into<String>) -> Self {
        Self {
            source,
            japanese,
            english,
            reason: reason.into(),
        }
    }
}

/// 判定の強さ (Meltype の DetectionLevel 相当)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionLevel {
    /// 積極的: 少しでも英語らしければ英字にする。
    Aggressive,
    /// 標準: 辞書・文脈をバランスよく使う。短い語 (no, to) は前後が英語のときだけ英字。
    Balanced,
    /// 慎重: 確信度が高いときだけ英字にする。
    Conservative,
    /// 手動: 自動では切り替えず、Shift で打った大文字始まりの語だけを英語にする。
    Manual,
}
