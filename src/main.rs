//! Noctalia歌詞プラグイン用ヘルパーツール。
//! MPRIS経由で再生中の曲を検出し、ローカルキャッシュまたはLRCLIBから歌詞を取得して
//! Noctaliaプラグインへ送信する。

mod cache;
mod lrclib;
mod lyrics;
mod mpris;
mod noctalia;

use std::{collections::HashMap, env, time::Duration};

use anyhow::Result;
use tokio::time::sleep;

use lyrics::Lyric;
use mpris::{MPRISData, MPRISStatus};

/// 曲ごとの歌詞取得状況（`lyrics_dict`）を確認・更新しながら、現時点で表示可能な歌詞を返す。
///
/// - 未処理の曲: ローカルキャッシュを確認し、無ければLRCLIBへの取得をバックグラウンドで開始する
/// - 取得中（結果待ち）の曲: ローカルキャッシュへ改めて現れていないかだけ確認する（再取得はしない）
/// - 取得済みの曲: キャッシュ済みの歌詞をそのまま返す
///
/// （元コードで`None`ケースと`Some(None)`ケースに重複していたローカルキャッシュ確認処理を集約）
fn resolve_lyrics(
    lyrics_dict: &mut HashMap<String, Option<Vec<Lyric>>>,
    song_key: &str,
    mpris: &MPRISData,
) -> Option<Vec<Lyric>> {
    let already_queried = lyrics_dict.contains_key(song_key);

    if let Some(Some(lyrics)) = lyrics_dict.get(song_key) {
        return Some(lyrics.clone());
    }

    if let Some(lyrics) = lyrics::get_lyrics_local(&mpris.title, &mpris.artist) {
        lyrics_dict.insert(song_key.to_string(), Some(lyrics.clone()));
        return Some(lyrics);
    }

    // まだこの曲を一度も問い合わせていない場合のみ、LRCLIBへの取得を非同期で開始する
    if !already_queried {
        tracing::info!("Spawn get_lyrics_lrclib");
        tokio::spawn(lyrics::get_lyrics_lrclib(
            mpris.title.clone(),
            mpris.artist.clone(),
            Some(mpris.length),
        ));
        lyrics_dict.insert(song_key.to_string(), None);
    }

    None
}

/// メインループ。定期的にMPRISの再生状態を取得し、歌詞を解決してNoctaliaプラグインへ反映する。
async fn daemon(adjust_ms: u64) -> Result<()> {
    let mut lyrics_dict: HashMap<String, Option<Vec<Lyric>>> = HashMap::new();
    let mut prev_song_key = String::new();

    loop {
        let mpris = mpris::get_player_status().await?;
        let Some(mpris) = mpris else {
            // プレイヤーが見つからない場合は少し待って再試行する
            sleep(Duration::from_millis(1000)).await;
            continue;
        };

        if mpris.title.trim().is_empty() {
            // タイトルがない場合は少し待って再試行する
            sleep(Duration::from_millis(1000)).await;
        }

        let song_key = cache::song_key(&mpris.artist, &mpris.title);
        let lyrics = resolve_lyrics(&mut lyrics_dict, &song_key, &mpris);

        if let Some(lyrics) = &lyrics {
            // 曲が切り替わった直後だけ、歌詞モデル全体をNoctaliaプラグインへ再送信する
            if prev_song_key != song_key {
                let model = noctalia::build_lyrics_model(lyrics, mpris.length);
                noctalia::clear_lyrics().await;
                noctalia::push_lyrics_model(&model).await;
            }
            prev_song_key = song_key.clone();
        }

        let state = noctalia::build_state(&mpris, lyrics.as_deref(), adjust_ms);
        noctalia::write_current_state(&state);

        match mpris.status {
            MPRISStatus::Paused => sleep(Duration::from_millis(1000)).await,
            MPRISStatus::Playing => sleep(Duration::from_millis(100)).await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        tracing::error!("Need args daemon or get <LRCLIB_ID>");
        return Ok(());
    }

    match args.get(1).unwrap().as_str() {
        "daemon" => daemon(1000).await,
        "get" => {
            if args.len() == 2 {
                tracing::error!("Need args for get. get <LRCLIB_ID>");
                return Ok(());
            }
            let id: u64 = args.get(2).unwrap().parse()?;
            let mpris = mpris::get_player_status().await?;
            if let Some(mpris) = mpris {
                let song_key = cache::song_key(&mpris.artist, &mpris.title);

                tracing::info!("Get {} as {}", song_key, id);

                match lyrics::get_lyrics_lrclib_id(mpris.title.clone(), mpris.artist.clone(), id)
                    .await
                {
                    Some(_) => tracing::info!("Success."),
                    None => tracing::info!("Can't get lyrics."),
                }
                Ok(())
            } else {
                tracing::error!("Player not found. Please play music before run.");
                Ok(())
            }
        }
        _ => Ok(()),
    }
}
