//! 自動判定 (Meltype 移植) の統合テスト。
//!
//! 確定 (Enter) の瞬間に生キー列を判定し、英語と判定されたら英字のまま
//! コミットする。日本語と判定されたら従来どおりかな/漢字で確定する。

use super::*;

/// 生キー列を打って Enter で確定し、Commit されたテキストを返す。
fn commit_of(keys: &str) -> Option<String> {
    let mut engine = InputMethodEngine::new();
    for ch in keys.chars() {
        engine.process_key(&press(ch));
    }
    let result = engine.process_key(&press_key(Keysym::RETURN));
    result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    })
}

#[test]
fn github_commits_as_ascii() {
    // github は英語辞書にあるので英字のまま確定
    assert_eq!(commit_of("github").as_deref(), Some("github"));
}

#[test]
fn sushi_commits_as_ascii() {
    // sushi は「ローマ字としても読める英単語」の代表例
    assert_eq!(commit_of("sushi").as_deref(), Some("sushi"));
}

#[test]
fn push_commits_as_ascii() {
    assert_eq!(commit_of("push").as_deref(), Some("push"));
}

#[test]
fn konnichiwa_still_commits_kana() {
    // 日本語は従来どおりかなで確定。karukan のローマ字変換は "nn" を即 ん に
    // 確定するため konnichiwa は こんいちわ になる (ベースの挙動、本パッチとは無関係)。
    assert_eq!(commit_of("konnichiwa").as_deref(), Some("こんいちわ"));
}

#[test]
fn arigatou_still_commits_kana() {
    // きれいな日本語例: arigatou → ありがとう
    assert_eq!(commit_of("arigatou").as_deref(), Some("ありがとう"));
}

#[test]
fn kyouha_still_commits_kana() {
    assert_eq!(commit_of("kyouha").as_deref(), Some("きょうは"));
}

#[test]
fn edited_input_falls_back_to_kana() {
    // 編集 (backspace) が入ると判定を諦めて従来動作 (かな) に戻る
    let mut engine = InputMethodEngine::new();
    for ch in "githubb".chars() {
        engine.process_key(&press(ch));
    }
    engine.process_key(&press_key(Keysym::BACKSPACE));
    let result = engine.process_key(&press_key(Keysym::RETURN));
    let commit = result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    });
    // github にはならない (判定無効化) — かな読みのまま
    assert_ne!(commit.as_deref(), Some("github"));
}
