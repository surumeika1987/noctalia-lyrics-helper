//! Noctalia歌詞プラグインへ送信する状態（`current.json` / push-json用モデル）の構築と送信。
//!
//! 元の`daemon`関数内では「歌詞が見つかった場合」と「見つからなかった場合」で
//! ほぼ同一のフィールド（status/title/artist/art_path）を持つ状態構築が重複していたため、
//! `build_state`関数に一本化した。

use serde::Serialize;
use tokio::process::Command;

use crate::cache::CACHE_DIR;
use crate::lyrics::Lyric;
use crate::mpris::{MPRISData, MPRISStatus};

/// `current.json`として書き出す、現在表示用の歌詞状態
#[derive(Debug, Serialize)]
pub struct NoctaliaLyricsState {
    pub status: String,
    pub title: String,
    pub artist: String,
    pub prev_prev: String,
    pub prev: String,
    pub current: String,
    pub next: String,
    pub next_next: String,
    pub art_path: String,
}

/// Noctaliaプラグインへpush-jsonで送る歌詞モデルの1行
#[derive(Debug, Serialize)]
pub struct NoctaliaLyricsModelLine {
    pub time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub romanization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chars: Option<Vec<u64>>,
}

/// Noctaliaプラグインへpush-jsonで送る歌詞モデル全体
#[derive(Debug, Serialize)]
pub struct NoctaliaLyricsModelRoot {
    pub lines: Vec<NoctaliaLyricsModelLine>,
}

/// MPRISの再生状態をNoctalia側で使う文字列表現に変換する
fn status_str(status: MPRISStatus) -> String {
    match status {
        MPRISStatus::Paused => "Paused".to_string(),
        MPRISStatus::Playing => "Playing".to_string(),
    }
}

/// 歌詞行のリストから、Noctaliaプラグインへpushする歌詞モデルを構築する。
/// 各行の`duration`は次の行の開始時刻との差分（最終行のみ曲の長さとの差分）とする。
/// MPRISの再生時間より長い歌詞データは該当部分を無視する
pub fn build_lyrics_model(lyrics: &[Lyric], length: u64) -> NoctaliaLyricsModelRoot {
    let lines: Vec<NoctaliaLyricsModelLine> = lyrics
        .iter()
        .filter(|v| v.time <= length)
        .enumerate()
        .map(|(i, v)| NoctaliaLyricsModelLine {
            time: v.time,
            duration: Some(match lyrics.get(i + 1) {
                Some(next_lyric) => next_lyric.time - v.time,
                None => length - v.time,
            }),
            text: v.text.clone(),
            translation: None,
            romanization: None,
            chars: None,
        })
        .collect();

    NoctaliaLyricsModelRoot { lines }
}

pub async fn clear_lyrics() {
    tracing::debug!("noctalia msg plugin h465855hgg/lyrics:service all clear");

    let _ = Command::new("noctalia")
        .args(["msg", "plugin", "h465855hgg/lyrics:service", "all", "clear"])
        .status()
        .await;
}

/// 歌詞モデル全体をNoctaliaプラグインへ`push-json`で送信する。
pub async fn push_lyrics_model(model: &NoctaliaLyricsModelRoot) {
    let json = serde_json::to_string(model).unwrap();

    tracing::debug!(
        "noctalia msg plugin h465855hgg/lyrics:service all push-json '{}'",
        json
    );

    let _ = Command::new("noctalia")
        .args([
            "msg",
            "plugin",
            "h465855hgg/lyrics:service",
            "all",
            "push-json",
            json.as_str(),
        ])
        .status()
        .await;
}

/// 現在の再生位置に対応する前後の歌詞テキストを含む状態を構築する。
/// `lyrics`が`None`の場合（歌詞が見つかっていない場合）は、各歌詞欄を空文字列にした状態を返す。
pub fn build_state(
    mpris: &MPRISData,
    lyrics: Option<&[Lyric]>,
    adjust_ms: u64,
) -> NoctaliaLyricsState {
    let empty_state = NoctaliaLyricsState {
        status: status_str(mpris.status),
        title: mpris.title.clone(),
        artist: mpris.artist.clone(),
        prev_prev: String::new(),
        prev: String::new(),
        current: String::new(),
        next: String::new(),
        next_next: String::new(),
        art_path: String::new(),
    };

    let Some(lyrics) = lyrics else {
        return empty_state;
    };

    let default_lyric = Lyric::default();
    let pos = mpris.position + adjust_ms;

    // 現在位置以前の歌詞行のうち、最後に該当する行（アクティブな行）のインデックスを求める
    let mut active_index: i64 = -1;
    for (index, lyric) in lyrics.iter().enumerate() {
        if lyric.time <= pos {
            active_index = index as i64;
        } else {
            break;
        }
    }

    NoctaliaLyricsState {
        prev_prev: if 2 <= active_index {
            lyrics
                .get((active_index - 2) as usize)
                .unwrap_or(&default_lyric)
                .text
                .clone()
        } else {
            String::new()
        },
        prev: if 1 <= active_index {
            lyrics
                .get((active_index - 1) as usize)
                .unwrap_or(&default_lyric)
                .text
                .clone()
        } else {
            String::new()
        },
        current: if 0 <= active_index {
            lyrics
                .get(active_index as usize)
                .unwrap_or(&default_lyric)
                .text
                .clone()
        } else {
            String::new()
        },
        next: lyrics
            .get((active_index + 1) as usize)
            .unwrap_or(&default_lyric)
            .text
            .clone(),
        next_next: lyrics
            .get((active_index + 2) as usize)
            .unwrap_or(&default_lyric)
            .text
            .clone(),
        ..empty_state
    }
}

/// 状態を`current.json`としてキャッシュディレクトリへ書き出す。
pub fn write_current_state(state: &NoctaliaLyricsState) {
    let json = serde_json::to_string(state).unwrap();
    let noctalia_state_file = CACHE_DIR.join("current.json");
    std::fs::write(noctalia_state_file, json).unwrap();
}
