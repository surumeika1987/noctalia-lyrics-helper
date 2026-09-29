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
    let lyrics: Vec<&Lyric> = lyrics.iter().filter(|v| v.time <= length).collect();
    let lines = lyrics
        .iter()
        .enumerate()
        .map(|(i, lyric)| NoctaliaLyricsModelLine {
            time: lyric.time,
            duration: Some(match lyrics.get(i + 1) {
                Some(next_lyric) => next_lyric.time.saturating_sub(lyric.time),
                None => length.saturating_sub(lyric.time),
            }),
            text: if lyric.text.is_empty() {
                String::from("...")
            } else {
                lyric.text.clone()
            },
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
    adjust_ms: i64,
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

    let pos = mpris.position.saturating_add_signed(adjust_ms);

    // 現在位置以前の歌詞行のうち、最後に該当する行（アクティブな行）のインデックスを求める
    let mut active_index: i64 = -1;
    for (index, lyric) in lyrics.iter().enumerate() {
        if lyric.time <= pos {
            active_index = index as i64;
        } else {
            break;
        }
    }

    let current_index = usize::try_from(active_index).ok();
    let current = current_index
        .and_then(|index| lyrics.get(index))
        .map(|lyric| lyric.text.clone())
        .unwrap_or_default();
    let prev = current_index
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| lyrics.get(index))
        .map(|lyric| lyric.text.clone())
        .unwrap_or_default();
    let prev_prev = current_index
        .and_then(|index| index.checked_sub(2))
        .and_then(|index| lyrics.get(index))
        .map(|lyric| lyric.text.clone())
        .unwrap_or_default();
    let next = current_index
        .and_then(|index| index.checked_add(1))
        .and_then(|index| lyrics.get(index))
        .map(|lyric| lyric.text.clone())
        .unwrap_or_default();
    let next_next = current_index
        .and_then(|index| index.checked_add(2))
        .and_then(|index| lyrics.get(index))
        .map(|lyric| lyric.text.clone())
        .unwrap_or_default();

    NoctaliaLyricsState {
        prev_prev,
        prev,
        current,
        next,
        next_next,
        ..empty_state
    }
}

/// 状態を`current.json`としてキャッシュディレクトリへ書き出す。
pub fn write_current_state(state: &NoctaliaLyricsState) {
    let json = serde_json::to_string(state).unwrap();
    let noctalia_state_file = CACHE_DIR.join("current.json");
    std::fs::write(noctalia_state_file, json).unwrap();
}

#[cfg(test)]
mod tests {
    use super::{build_lyrics_model, build_state};
    use crate::lyrics::Lyric;
    use crate::mpris::{MPRISData, MPRISStatus};

    fn mpris(position: u64) -> MPRISData {
        MPRISData {
            status: MPRISStatus::Playing,
            position,
            title: "Title".to_string(),
            artist: "Artist".to_string(),
            length: 10_000,
        }
    }

    #[test]
    fn model_omits_lines_after_track_end() {
        let model = build_lyrics_model(
            &[
                Lyric {
                    time: 1_000,
                    text: "one".into(),
                },
                Lyric {
                    time: 12_000,
                    text: "two".into(),
                },
            ],
            10_000,
        );

        assert_eq!(model.lines.len(), 1);
        assert_eq!(model.lines[0].duration, Some(9_000));
    }

    #[test]
    fn state_exposes_surrounding_lines() {
        let lyrics = vec![
            Lyric {
                time: 1_000,
                text: "one".into(),
            },
            Lyric {
                time: 2_000,
                text: "two".into(),
            },
            Lyric {
                time: 3_000,
                text: "three".into(),
            },
        ];
        let state = build_state(&mpris(2_500), Some(&lyrics), 0);

        assert_eq!(state.prev, "one");
        assert_eq!(state.current, "two");
        assert_eq!(state.next, "three");
    }
}
