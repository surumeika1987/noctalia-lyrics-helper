//! MPRIS（`playerctl`コマンド経由）でのメディアプレイヤー状態取得。

use anyhow::Result;
use tokio::process::Command;

/// 再生状態
#[derive(Copy, Clone, Debug)]
pub enum MPRISStatus {
    Playing,
    Paused,
}

/// `playerctl`から取得したプレイヤーの状態
#[derive(Clone, Debug)]
pub struct MPRISData {
    pub status: MPRISStatus,
    pub position: u64,
    pub title: String,
    pub artist: String,
    pub length: u64,
    pub art_url: String,
}

/// 現在アクティブなプレイヤーの再生状態を取得する。
/// 複数のプレイヤーが動作している場合はYoutubeMusicを優先し、無ければ先頭のプレイヤーを使う。
/// プレイヤーが1つも見つからない場合は`Ok(None)`を返す。
pub async fn get_player_status() -> Result<Option<MPRISData>> {
    let output = Command::new("playerctl")
        .args(["-l"])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let players: Vec<&str> = stdout.trim().lines().collect();
    let mut player: Option<String> = None;

    // YoutubeMusicを優先
    if players.contains(&"YoutubeMusic") {
        player = Some(String::from("YoutubeMusic"));
    } else if 0 < players.len() {
        player = Some(players.get(0).unwrap().to_string());
    }

    if player == None {
        return Ok(None);
    }

    let output = Command::new("playerctl")
        .args([
            "-p",
            player.unwrap().as_str(),
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
        MPRISStatus::Playing
    } else {
        MPRISStatus::Paused
    };
    let mpris = MPRISData {
        status,
        position: json["position"].as_u64().unwrap() / 1000,
        title: json["title"].as_str().unwrap().to_string(),
        artist: json["artist"].as_str().unwrap().to_string(),
        length: json["length"].as_u64().unwrap() / 1000,
        art_url: json["art_url"].as_str().unwrap().to_string(),
    };
    Ok(Some(mpris))
}

/// YouTube MusicのURLから動画IDを抽出する。
/// 現状どこからも呼ばれていないが、元コードにあった関数のため機能はそのまま保持している。
#[allow(dead_code)]
pub fn get_youtube_id(url: Option<&str>) -> Option<&str> {
    if url?.starts_with("https://music.youtube.com/watch?v=") {
        url?.split_once("=").map(|(_, s)| s)
    } else {
        None
    }
}
