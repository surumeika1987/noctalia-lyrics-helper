mod lrclib;
use std::{
    collections::HashMap,
    env::{self, args},
    fs::OpenOptions,
    io::Write,
    process::exit,
    time::Duration,
};

use anyhow::Result;
use lrclib::{LRCLIBAPI, LRCLIBError, LRCLIBResponse};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Error;
use tokio::process::Command;
use tokio::time::sleep;

use std::sync::LazyLock;

use crate::MPRISStatus::{Paused, Playing};

static CACHE_DIR: LazyLock<std::path::PathBuf> = LazyLock::new(|| {
    dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".cache/noctalia/lyrics")
});

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct Lyric {
    time: u64,
    text: String,
}

#[derive(Copy, Clone, Debug)]
enum MPRISStatus {
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
struct MPRISData {
    status: MPRISStatus,
    position: u64,
    title: String,
    artist: String,
    length: u64,
    art_url: String,
}

#[derive(Debug, Serialize)]
struct NoctaliaLyricState {
    status: String,
    title: String,
    artist: String,
    prev_prev: String,
    prev: String,
    current: String,
    next: String,
    next_next: String,
    art_path: String,
}

#[derive(Debug, Serialize)]
struct NoctaliaLyricModel {
    lines: Vec<Lyric>,
}

fn parse_lrc(lrc_text: Vec<&str>) -> Vec<Lyric> {
    let mut parsed: Vec<Lyric> = Vec::new();

    for line in lrc_text {
        let parts: Vec<&str> = line.split("]").collect();
        let time_part = parts.get(0).unwrap().replace("[", "");
        let text = line
            .strip_prefix(&format!("[{}]", time_part))
            .unwrap_or(line)
            .trim()
            .to_string();
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
        let secounds = (*min_sec_parts.get(1).unwrap()).parse::<u64>().unwrap();
        let time = ((minutes * 60) + secounds) * 1000 + hund;
        parsed.push(Lyric {
            text: text.to_string(),
            time,
        });
    }

    parsed
}

async fn get_player_status() -> Result<MPRISData> {
    let output = Command::new("playerctl")
        .args([
            "-p",
            "YoutubeMusic",
            "metadata",
            "--format",
            "{\"status\": \"{{status}}\",\"position\":{{position}},\"title\":\"{{title}}\",\"artist\":\"{{artist}}\",\"length\":{{mpris:length}},\"art_url\":\"{{mpris:artUrl}}\"}",
        ])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)?;
    let status = if json["status"] == "Playing" {
        Playing
    } else {
        Paused
    };
    let mpris = MPRISData {
        status,
        position: json["position"].as_u64().unwrap() / 1000,
        title: json["title"].as_str().unwrap().to_string(),
        artist: json["artist"].as_str().unwrap().to_string(),
        length: json["length"].as_u64().unwrap() / 1000,
        art_url: json["art_url"].as_str().unwrap().to_string(),
    };
    Ok(mpris)
}

fn get_youtube_id(url: Option<&str>) -> Option<&str> {
    if url?.starts_with("https://music.youtube.com/watch?v=") {
        url?.split_once("=").map(|(_, s)| s)
    } else {
        return None;
    }
}

fn get_lyrics_local(title: &str, artist: &str) -> Option<Vec<Lyric>> {
    let song_key = format!("{} - {}", artist, title);
    let song_key_hash = hex::encode(Md5::digest(song_key.as_bytes()));

    let cache_lyric_file_path = CACHE_DIR.join(format!("{}.json", song_key_hash));
    if cache_lyric_file_path.is_file() {
        let json = std::fs::read_to_string(&cache_lyric_file_path).unwrap();
        return Some(serde_json::from_str(&json).unwrap());
    }
    None
}

async fn get_lyrics_lrclib(
    title: String,
    artist: String,
    length: Option<u64>,
) -> Option<Vec<Lyric>> {
    let song_key = format!("{} - {}", artist, title);
    let song_key_hash = hex::encode(Md5::digest(song_key.as_bytes()));

    let cache_lyric_file_path = CACHE_DIR.join(format!("{}.json", song_key_hash));
    let cache_lyric_db_file_path = CACHE_DIR.join("db.cvs");

    let response = LRCLIBAPI::get_lyrics_with_a_tracks_signature(
        title.as_str(),
        artist.as_str(),
        None,
        if let Some(len) = length {
            Some((len / 1000) as u16)
        } else {
            None
        },
    )
    .await;

    match response {
        Ok(res) => {
            if let Some(synd_lrc_text) = res.syncd_lyrics {
                let lyrics = parse_lrc(
                    synd_lrc_text
                        .iter()
                        .map(|v| v.as_str())
                        .collect::<Vec<&str>>(),
                );

                tracing::info!("Save file to {}", cache_lyric_file_path.to_string_lossy());
                let json = serde_json::to_string(&lyrics).unwrap();
                std::fs::write(cache_lyric_file_path, json).unwrap();
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(cache_lyric_db_file_path)
                    .unwrap();

                writeln!(file, "{},{}", song_key_hash, song_key);

                return Some(lyrics);
            }
            None
        }
        Err(err) => None,
    }
}

async fn get_lyrics_lrclib_id(title: String, artist: String, id: u64) -> Option<Vec<Lyric>> {
    let song_key = format!("{} - {}", artist, title);
    let song_key_hash = hex::encode(Md5::digest(song_key.as_bytes()));

    let cache_lyric_file_path = CACHE_DIR.join(format!("{}.json", song_key_hash));
    let cache_lyric_db_file_path = CACHE_DIR.join("db.cvs");

    let response = LRCLIBAPI::get_lyrics_by_lrclibs_id(id).await;

    match response {
        Ok(res) => {
            if let Some(synd_lrc_text) = res.syncd_lyrics {
                let lyrics = parse_lrc(
                    synd_lrc_text
                        .iter()
                        .map(|v| v.as_str())
                        .collect::<Vec<&str>>(),
                );

                tracing::info!("Save file to {}", cache_lyric_file_path.to_string_lossy());
                let json = serde_json::to_string(&lyrics).unwrap();
                std::fs::write(cache_lyric_file_path, json).unwrap();
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(cache_lyric_db_file_path)
                    .unwrap();

                writeln!(file, "{},{}", song_key_hash, song_key);

                return Some(lyrics);
            }
            None
        }
        Err(err) => None,
    }
}

async fn daemon(adjust_ms: u64) -> Result<()> {
    let mut lyrics_dict: HashMap<String, Option<Vec<Lyric>>> = HashMap::new();
    let mut prev_song_key = String::new();

    while true {
        let mpris = get_player_status().await?;
        let song_key = format!("{} - {}", mpris.artist, mpris.title);

        let lyrics: Option<Vec<Lyric>> = match lyrics_dict.get(&song_key) {
            None => {
                if let Some(lyrics) = get_lyrics_local(mpris.title.as_str(), mpris.artist.as_str())
                {
                    lyrics_dict.insert(song_key.clone(), Some(lyrics.clone()));
                    Some(lyrics)
                } else {
                    tracing::info!("Spawn get_lyrics_lrclib");
                    tokio::spawn(get_lyrics_lrclib(
                        mpris.title.clone(),
                        mpris.artist.clone(),
                        Some(mpris.length),
                    ));
                    lyrics_dict.insert(song_key.clone(), None);
                    None
                }
            }
            Some(None) => {
                if let Some(lyrics) = get_lyrics_local(mpris.title.as_str(), mpris.artist.as_str())
                {
                    lyrics_dict.insert(song_key.clone(), Some(lyrics.clone()));
                    Some(lyrics)
                } else {
                    None
                }
            }
            Some(Some(lyrics)) => Some(lyrics.clone()),
        };

        if let Some(lyrics) = lyrics {
            if prev_song_key != song_key {
                let modified_lyrics: Vec<Lyric> = lyrics
                    .iter()
                    .map(|v| Lyric {
                        time: v.time,
                        text: if v.text.trim().len() == 0 {
                            String::from("...")
                        } else {
                            v.text.clone()
                        },
                    })
                    .collect();
                // For Noctalia lyric plugin
                let mut lyric_model: NoctaliaLyricModel = NoctaliaLyricModel {
                    lines: modified_lyrics,
                };

                let json = serde_json::to_string(&lyric_model).unwrap();

                tracing::debug!(
                    "noctalia msg plugin h465855hgg/lyrics:service all push-json '{}'",
                    json
                );

                Command::new("noctalia")
                    .args([
                        "msg",
                        "plugin",
                        "h465855hgg/lyrics:service",
                        "all",
                        "push-json",
                        json.as_str(),
                    ])
                    .spawn();
            }

            let mut active_index: i64 = -1;
            let pos = mpris.position + adjust_ms;

            for (index, lyric) in lyrics.iter().enumerate() {
                if lyric.time <= pos {
                    active_index = index as i64
                } else {
                    break;
                }
            }

            let default_lyric = Lyric::default();

            let state = NoctaliaLyricState {
                status: match mpris.status {
                    Paused => "Paused".to_string(),
                    Playing => "Playing".to_string(),
                },
                title: mpris.title.clone(),
                artist: mpris.artist.clone(),
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
                art_path: String::new(),
            };
            let json = serde_json::to_string(&state).unwrap();

            let noctalia_state_file = CACHE_DIR.join("current.json");
            std::fs::write(noctalia_state_file, json).unwrap();

            prev_song_key = song_key.clone();
        } else {
            let state = NoctaliaLyricState {
                status: match mpris.status {
                    Paused => "Paused".to_string(),
                    Playing => "Playing".to_string(),
                },
                title: mpris.title.clone(),
                artist: mpris.artist.clone(),
                prev_prev: String::new(),
                prev: String::new(),
                current: String::new(),
                next: String::new(),
                next_next: String::new(),
                art_path: String::new(),
            };

            let json = serde_json::to_string(&state).unwrap();

            let noctalia_state_file = CACHE_DIR.join("current.json");
            std::fs::write(noctalia_state_file, json).unwrap();
        }

        match mpris.status {
            Paused => sleep(Duration::from_millis(1000)).await,
            Playing => sleep(Duration::from_millis(100)).await,
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        tracing::error!("Need args daemon or get <LIBLRC_ID>");
        return Ok(());
    }

    match args.get(1).unwrap().as_str() {
        "daemon" => daemon(1000).await,
        "get" => {
            if args.len() == 2 {
                tracing::error!("Need args for get. get <LIBLRC_ID>");
                return Ok(());
            }
            let id: u64 = args.get(2).unwrap().parse()?;
            let mpris = get_player_status().await?;
            let song_key = format!("{} - {}", mpris.artist, mpris.title);

            tracing::info!("Get {} as {}", song_key, id);

            match get_lyrics_lrclib_id(mpris.title.clone(), mpris.artist.clone(), id).await {
                Some(_) => tracing::info!("Success."),
                None => tracing::info!("Can't get lyrics."),
            }

            Ok(())
        }
        _ => Ok(()),
    }
}
