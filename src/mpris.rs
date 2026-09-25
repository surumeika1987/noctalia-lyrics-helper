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

/// メタデータの各フィールドを区切るのに使う文字。
/// タイトルやアーティスト名に出現する可能性が実質無い制御文字(Unit Separator, U+001F)を使うことで、
/// JSON文字列を手動組み立てしてパースする方式（"や\を含むタイトルで壊れる）を避けている。
const FIELD_SEPARATOR: &str = "\u{1f}";

/// 現在アクティブなプレイヤーの再生状態を取得する。
/// 複数のプレイヤーが動作している場合はYoutubeMusicを優先し、無ければ先頭のプレイヤーを使う。
/// プレイヤーが1つも見つからない場合は`Ok(None)`を返す。
pub async fn get_player_status(priority_player: &str) -> Result<Option<MPRISData>> {
    let output = Command::new("playerctl")
        .args(["-l"])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let players: Vec<&str> = stdout.trim().lines().collect();
    let mut player: Option<String> = None;

    // "priority_player"を優先
    if players.contains(&priority_player) {
        player = Some(priority_player.to_string());
    } else if 0 < players.len() {
        player = Some(players.get(0).unwrap().to_string());
    }

    if player == None {
        return Ok(None);
    }

    // タイトルやアーティスト名に " や \ が含まれていても壊れないよう、
    // JSON形式ではなく制御文字区切りでフィールドをそのまま取り出す
    let format = [
        "{{status}}",
        "{{position}}",
        "{{title}}",
        "{{artist}}",
        "{{mpris:length}}",
        "{{mpris:artUrl}}",
    ]
    .join(FIELD_SEPARATOR);

    let output = Command::new("playerctl")
        .args([
            "-p",
            player.unwrap().as_str(),
            "metadata",
            "--format",
            &format,
        ])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fields = stdout.trim_end_matches('\n').split(FIELD_SEPARATOR);

    let status = if fields.next().unwrap_or_default() == "Playing" {
        MPRISStatus::Playing
    } else {
        MPRISStatus::Paused
    };
    let position: u64 = fields.next().unwrap_or_default().parse().unwrap_or(0);
    let title = fields.next().unwrap_or_default().to_string();
    let artist = fields.next().unwrap_or_default().to_string();
    let length: u64 = fields.next().unwrap_or_default().parse().unwrap_or(0);
    let art_url = fields.next().unwrap_or_default().to_string();

    let mpris = MPRISData {
        status,
        position: position / 1000,
        title,
        artist,
        length: length / 1000,
        art_url,
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
