//! Noctalia歌詞プラグインへ送信する状態（`current.json` / push-json用モデル）の構築と送信。
//!
//! 元の`daemon`関数内では「歌詞が見つかった場合」と「見つからなかった場合」で
//! ほぼ同一のフィールド（status/title/artist/art_path）を持つ状態構築が重複していたため、
//! `build_state`関数に一本化した。

use std::process::Stdio;

use serde::Serialize;
use tokio::process::Command;

use crate::cache::CACHE_DIR;
use crate::lyrics::Lyric;
use crate::mpris::{MPRISData, MPRISStatus};

const MICROSECONDS_PER_MILLISECOND: u64 = 1_000;

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

// Noctaliaプラグインへpush-stateで送る楽曲情報
#[derive(Debug, Serialize)]
pub struct NoctaliaLyricsModelTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub status: String,
    pub position: u64,
    pub duration: u64,
    #[serde(rename = "playerInstance")]
    pub player_instance: String,
    #[serde(rename = "trackId")]
    pub track_id: String,
    #[serde(rename = "mediaUrl")]
    pub media_url: String,
}

/// Noctaliaプラグインへpush-stateで送る歌詞モデルの1行
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

/// Noctaliaプラグインへpush-stateで送るモデル全体
#[derive(Debug, Serialize)]
pub struct NoctaliaLyricsModelRoot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<NoctaliaLyricsModelTrack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<NoctaliaLyricsModelLine>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
}

/// MPRISの再生状態をNoctalia側で使う文字列表現に変換する
fn status_str(status: MPRISStatus) -> String {
    match status {
        MPRISStatus::Paused => "Paused".to_string(),
        MPRISStatus::Playing => "Playing".to_string(),
    }
}

/// Noctaliaのトラック状態と再生フラグを返す。
fn playback_state(status: MPRISStatus) -> (&'static str, bool) {
    match status {
        MPRISStatus::Playing => ("playing", true),
        MPRISStatus::Paused => ("paused", false),
    }
}

/// ミリ秒をNoctalia APIで使うマイクロ秒へ変換する。
fn to_microseconds(milliseconds: u64) -> u64 {
    milliseconds.saturating_mul(MICROSECONDS_PER_MILLISECOND)
}

/// MPRIS情報と歌詞からNoctaliaプラグインへ送信するモデルを構築する。
pub fn build_lyrics_model(lyrics: &[Lyric], mpris: &MPRISData) -> NoctaliaLyricsModelRoot {
    let (status, playing) = playback_state(mpris.status);

    NoctaliaLyricsModelRoot {
        track: Some(NoctaliaLyricsModelTrack {
            title: mpris.title.clone(),
            artist: mpris.artist.clone(),
            album: String::new(),
            status: status.to_string(),
            position: to_microseconds(mpris.position),
            duration: to_microseconds(mpris.length),
            player_instance: mpris.player.clone(),
            track_id: String::new(),
            media_url: String::new(),
        }),
        lines: Some(build_lyrics_lines(lyrics, mpris.length)),
        position: Some(to_microseconds(mpris.position)),
        playing: Some(playing),
        cover: Some(String::new()),
    }
}

/// 歌詞行のリストから、Noctaliaプラグインへpushする歌詞モデルを構築する。
/// 各行の`duration`は次の行の開始時刻との差分（最終行のみ曲の長さとの差分）とする。
/// MPRISの再生時間より長い歌詞データは該当部分を無視する
fn build_lyrics_lines(lyrics: &[Lyric], length: u64) -> Vec<NoctaliaLyricsModelLine> {
    let lyrics: Vec<&Lyric> = lyrics.iter().filter(|v| v.time <= length).collect();
    lyrics
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
        .collect()
}

/// 歌詞モデル全体をNoctaliaプラグインへ`push-state`で送信する。
pub async fn push_state(model: &NoctaliaLyricsModelRoot) {
    let json = serde_json::to_string(model).unwrap();

    tracing::debug!(
        "noctalia msg plugin h465855hgg/lyrics:service all push-state '{}'",
        json
    );

    let _ = Command::new("noctalia")
        .args([
            "msg",
            "plugin",
            "h465855hgg/lyrics:service",
            "all",
            "push-state",
            json.as_str(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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
            player: "exsample".to_string(),
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
            &mpris(0),
        );

        let lines = model.lines.as_ref().unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].duration, Some(9_000));
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
