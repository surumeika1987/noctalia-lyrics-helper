//! 歌詞データの型定義、LRC形式のパース、LRCLIBからの取得ロジック。
//!
//! `get_lyrics_lrclib` / `fallback` / `get_lyrics_lrclib_id` の3関数で
//! 重複していた「同期歌詞をパースしてキャッシュへ保存する」処理は
//! `parse_and_cache` に、`fallback`内の候補選択ロジックは
//! `select_best_match` にそれぞれ切り出した。

use serde::{Deserialize, Serialize};

use crate::cache;
use crate::lrclib::{LRCLIBAPI, LRCLIBResponse};

/// パース済みの1行分の歌詞（時刻[ms]とテキスト）
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Lyric {
    pub time: u64,
    pub text: String,
}

/// LRC形式（例: `[01:23.45]テキスト`）の行群を、時刻付き歌詞のリストへ変換する。
pub fn parse_lrc(lrc_text: Vec<&str>) -> Vec<Lyric> {
    let mut parsed: Vec<Lyric> = Vec::new();

    for line in lrc_text {
        let parts: Vec<&str> = line.split("]").collect();
        let time_part = parts.get(0).unwrap().replace("[", "");
        let text = line
            .strip_prefix(&format!("[{}]", time_part))
            .unwrap_or(line)
            .trim()
            .to_string();

        // "分:秒" または "分:秒.百分の一秒" の形式から時刻を分離する
        let (min_sec, hund) = if time_part.contains(".") {
            let time_parts: Vec<&str> = time_part.split(".").collect();
            let hund_str = *time_parts.get(1).unwrap();
            let hund: u64 = if hund_str.len() == 2 {
                hund_str.parse::<u64>().unwrap() * 10
            } else {
                hund_str.parse::<u64>().unwrap()
            };
            (*time_parts.get(0).unwrap(), hund)
        } else {
            (time_part.as_str(), 0)
        };

        let min_sec_parts: Vec<&str> = min_sec.split(":").collect();
        let minutes = (*min_sec_parts.get(0).unwrap()).parse::<u64>().unwrap();
        let seconds = (*min_sec_parts.get(1).unwrap()).parse::<u64>().unwrap();
        let time = ((minutes * 60) + seconds) * 1000 + hund;

        parsed.push(Lyric { text, time });
    }

    parsed
}

/// LRCLIBのレスポンスから同期歌詞（syncedLyrics）が取れればパースしてキャッシュへ保存する。
/// 同期歌詞が存在しない場合は`None`を返す。
///
/// （元コードで3箇所に重複していた「パース→保存→返却」処理を集約したもの）
fn parse_and_cache(
    res: &LRCLIBResponse,
    song_key_hash: &str,
    song_key: &str,
) -> Option<Vec<Lyric>> {
    let synd_lrc_text = res.syncd_lyrics.as_ref()?;
    let lyrics = parse_lrc(
        synd_lrc_text
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<&str>>(),
    );
    cache::save_lyrics(song_key_hash, song_key, &lyrics);
    Some(lyrics)
}

/// ローカルキャッシュから歌詞を読み込む。
pub fn get_lyrics_local(title: &str, artist: &str) -> Option<Vec<Lyric>> {
    let key = cache::song_key(artist, title);
    let hash = cache::song_key_hash(&key);
    cache::load_lyrics(&hash)
}

/// 曲名・アーティスト名（・長さ）でLRCLIBへ問い合わせ、歌詞を取得する。
/// 該当が無い、または同期歌詞が無い場合は検索によるフォールバックを行う。
pub async fn get_lyrics_lrclib(
    title: String,
    artist: String,
    length: Option<u64>,
) -> Option<Vec<Lyric>> {
    let key = cache::song_key(&artist, &title);
    let hash = cache::song_key_hash(&key);

    let response = LRCLIBAPI::get_lyrics_with_a_tracks_signature(
        title.as_str(),
        artist.as_str(),
        None,
        length.map(|len| (len / 1000) as u16),
    )
    .await;

    match response {
        Ok(res) => match parse_and_cache(&res, &hash, &key) {
            Some(lyrics) => Some(lyrics),
            None => fallback(title, artist, length).await,
        },
        Err(_) => fallback(title, artist, length).await,
    }
}

/// LRCLIBの検索APIで曲名の候補を探し、指定された長さに最も近いものを採用するフォールバック処理。
pub async fn fallback(title: String, artist: String, length: Option<u64>) -> Option<Vec<Lyric>> {
    let key = cache::song_key(&artist, &title);
    let hash = cache::song_key_hash(&key);

    tracing::info!("Fallback to search");
    let responses = LRCLIBAPI::search_lyrics(title).await;

    match responses {
        Ok(ress) => {
            let select_lyrics_index = select_best_match(&ress, length)?;
            let res = ress.get(select_lyrics_index).unwrap();
            parse_and_cache(res, &hash, &key)
        }
        Err(_) => None,
    }
}

/// 検索結果の中から採用する候補のインデックスを選ぶ。
/// 長さが指定されていればその長さに最も近い候補、指定が無ければ先頭（存在すれば）を選ぶ。
fn select_best_match(ress: &[LRCLIBResponse], length: Option<u64>) -> Option<usize> {
    if let Some(length) = length {
        let mut select_lyrics_index: Option<usize> = None;
        let mut diff_time: u64 = u64::MAX;
        for (index, res) in ress.iter().enumerate() {
            let diff: u64 = (res.duration * 1000).abs_diff(length);
            if diff < diff_time {
                select_lyrics_index = Some(index);
                diff_time = diff;
            }
        }
        select_lyrics_index
    } else if !ress.is_empty() {
        Some(0)
    } else {
        None
    }
}

/// LRCLIBの楽曲IDを直接指定して歌詞を取得する。
pub async fn get_lyrics_lrclib_id(title: String, artist: String, id: u64) -> Option<Vec<Lyric>> {
    let key = cache::song_key(&artist, &title);
    let hash = cache::song_key_hash(&key);

    let response = LRCLIBAPI::get_lyrics_by_lrclibs_id(id).await;

    match response {
        Ok(res) => parse_and_cache(&res, &hash, &key),
        Err(_) => None,
    }
}
