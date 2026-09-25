//! 歌詞データの型定義、LRC形式のパース、LRCLIBからの取得ロジック。
//!
//! `get_lyrics_lrclib` / `fallback` / `get_lyrics_lrclib_id` の3関数で
//! 重複していた「同期歌詞をパースしてキャッシュへ保存する」処理は
//! `parse_and_cache` に、`fallback`内の候補選択ロジックは
//! `select_best_match` にそれぞれ切り出した。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::cache;
use crate::lrclib::{LRCLIBAPI, LRCLIBError, LRCLIBResponse};

/// パース済みの1行分の歌詞（時刻[ms]とテキスト）
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Lyric {
    pub time: u64,
    pub text: String,
}

/// 1行をタイムスタンプ付き歌詞としてパースする。
/// メタデータ行（`[ar:...]` `[ti:...]` など）、空行、壊れた行は`None`を返してスキップ対象とする。
///
/// 正規表現は使わず、`split_once`/`strip_prefix`/`parse`のみで判定する。
/// タイムスタンプ行は`[`の直後が必ず数字、メタデータ行は英字始まりという
/// LRC形式の性質を利用して区別している。
fn parse_lrc_line(line: &str) -> Option<Lyric> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let rest = line.strip_prefix('[')?;
    let (time_part, after_bracket) = rest.split_once(']')?;

    // タイムスタンプは必ず数字始まり。[ar:...]等のメタデータ行は英字始まりなのでここで弾く
    if !time_part.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }

    let time = parse_lrc_timestamp(time_part)?;
    Some(Lyric {
        time,
        text: after_bracket.trim().to_string(),
    })
}

/// "分:秒" または "分:秒.百分の一秒" 形式のタイムスタンプをミリ秒へ変換する。
/// 形式が不正な場合は`None`を返す（呼び出し元でスキップされる）。
fn parse_lrc_timestamp(time_part: &str) -> Option<u64> {
    let (min_sec, hund) = match time_part.split_once('.') {
        Some((min_sec, hund_str)) => {
            let hund: u64 = hund_str.parse().ok()?;
            // 元コードと同じ仕様: 小数部が2桁(百分の一秒)ならミリ秒換算(*10)、それ以外はそのまま扱う
            let hund = if hund_str.len() == 2 { hund * 10 } else { hund };
            (min_sec, hund)
        }
        None => (time_part, 0),
    };

    let (minutes_str, seconds_str) = min_sec.split_once(':')?;
    let minutes: u64 = minutes_str.parse().ok()?;
    let seconds: u64 = seconds_str.parse().ok()?;

    Some((minutes * 60 + seconds) * 1000 + hund)
}

/// LRC形式（例: `[01:23.45]テキスト`）の行群を、時刻付き歌詞のリストへ変換する。
/// メタデータ行や不正な行は自動的に読み飛ばされる。
/// また、時刻が直前に採用した行より前（逆行）になっている行も除外する
/// （LRCの記述ミスや行の並び替えミスによる、歌詞の時系列逆転を防ぐため）。
pub fn parse_lrc(lrc_text: Vec<&str>) -> Vec<Lyric> {
    let mut last_time: Option<u64> = None;

    lrc_text
        .into_iter()
        .filter_map(parse_lrc_line)
        .filter(|lyric| {
            let is_in_order = last_time.map_or(true, |prev| lyric.time >= prev);
            if is_in_order {
                last_time = Some(lyric.time);
            }
            is_in_order
        })
        .collect()
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
    let Some(synd_lrc_text) = res.syncd_lyrics.as_ref() else {
        tracing::info!("Can't find syncd lyrics.");
        return None;
    };
    let lyrics = parse_lrc(
        synd_lrc_text
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<&str>>(),
    );
    cache::save_lyrics(song_key_hash, res.id, song_key, &lyrics);
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

    let mut retry = 0;

    loop {
        let response = LRCLIBAPI::get_lyrics_with_a_tracks_signature(
            title.clone(),
            artist.clone(),
            None,
            length.map(|len| (len / 1000) as u16),
        )
        .await;

        match response {
            Ok(res) => match parse_and_cache(&res, &hash, &key) {
                Some(lyrics) => return Some(lyrics),
                None => return search_lyrics_lrclib(title, artist, length).await,
            },
            Err(err) => {
                match err {
                    LRCLIBError::NotFound => {
                        return search_lyrics_lrclib(title, artist, length).await;
                    }
                    LRCLIBError::TooManyRequest => return None,
                    LRCLIBError::Overload => {
                        // 5回失敗で終了
                        if 5 < retry {
                            tracing::info!("Retry limit reached.");
                            return None;
                        }
                        tracing::info!("Server Overloaded. Rtrying...");
                        // 少し待って再取得
                        sleep(Duration::from_millis(1000)).await;
                        retry += 1;
                    }
                    LRCLIBError::Request(req_err) => {
                        if req_err.is_timeout() {
                            // 5回失敗で終了
                            if 5 < retry {
                                tracing::info!("Retry limit reached.");
                                return None;
                            }
                            tracing::info!("Connection timeout. Rtrying...");
                            // 少し待って再取得
                            sleep(Duration::from_millis(1000)).await;
                            retry += 1;
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                };
            }
        }
    }
}

/// LRCLIBの検索APIで曲名の候補を探し
/// 指定された長さに最も近い同期歌詞があるものを採用する
pub async fn search_lyrics_lrclib(
    title: String,
    artist: String,
    length: Option<u64>,
) -> Option<Vec<Lyric>> {
    let key = cache::song_key(&artist, &title);
    let hash = cache::song_key_hash(&key);

    tracing::info!("Fallback to search");

    let mut retry = 0;

    loop {
        let responses = LRCLIBAPI::search_lyrics(title.clone()).await;

        match responses {
            Ok(ress) => {
                let filtered_ress: Vec<LRCLIBResponse> = ress
                    .into_iter()
                    .filter(|v| v.syncd_lyrics != None)
                    .collect();
                if filtered_ress.len() == 0 {
                    tracing::info!("Can't find syncd lyrics.");
                    return None;
                }
                let select_lyrics_index = select_best_match(&filtered_ress, length)?;
                let res = filtered_ress.get(select_lyrics_index).unwrap();
                return parse_and_cache(res, &hash, &key);
            }
            Err(err) => {
                match err {
                    LRCLIBError::TooManyRequest => return None,
                    LRCLIBError::Overload => {
                        // 5回失敗で終了
                        if 5 < retry {
                            tracing::info!("Retry limit reached.");
                            return None;
                        }
                        tracing::info!("Server Overloaded. Rtrying...");
                        // 少し待って再取得
                        sleep(Duration::from_millis(1000)).await;
                        retry += 1;
                    }
                    LRCLIBError::Request(req_err) => {
                        if req_err.is_timeout() {
                            // 5回失敗で終了
                            if 5 < retry {
                                tracing::info!("Retry limit reached.");
                                return None;
                            }
                            tracing::info!("Connection timeout. Rtrying...");
                            // 少し待って再取得
                            sleep(Duration::from_millis(1000)).await;
                            retry += 1;
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                };
            }
        }
    }
}

/// 検索結果の中から採用する候補のインデックスを選ぶ。
/// 長さが指定されていればその長さに最も近い候補、指定が無ければ先頭（存在すれば）を選ぶ。
fn select_best_match(ress: &[LRCLIBResponse], length: Option<u64>) -> Option<usize> {
    if let Some(length) = length {
        let mut select_lyrics_index: Option<usize> = None;
        let mut diff_time: u64 = u64::MAX;
        for (index, res) in ress.iter().enumerate() {
            // duration*1000とlengthの差はどちらが大きいか分からないため、
            // 符号なし整数のまま安全に絶対差を取れるabs_diffを使う
            // （元コードは `(a - b) as i64.abs()` で、a<bのとき減算がオーバーフローしてパニックしていた）
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
