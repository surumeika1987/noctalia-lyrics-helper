//! 歌詞のローカルキャッシュ（`~/.cache/noctalia/lyrics/`）に関するユーティリティ。
//!
//! 元コードでは「キャッシュJSONへの書き込み＋db.cvsへの追記」処理が
//! `get_lyrics_lrclib` / `fallback` / `get_lyrics_lrclib_id` の3箇所に
//! ほぼ同一のまま重複していたため、`save_lyrics` として1箇所にまとめた。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

use md5::{Digest, Md5};

use crate::lyrics::Lyric;

/// 歌詞キャッシュの保存先ディレクトリ
pub static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".cache/noctalia/lyrics")
});

pub fn init_cache_dir() {
    if !CACHE_DIR.is_dir() {
        if CACHE_DIR.is_file() {
            tracing::error!("CACHE_DIR is file! This must folder!");
            panic!("CACHE_DIR is file! This must folder!");
        }
        fs::create_dir_all(CACHE_DIR.to_path_buf()).unwrap();
    }

    if !db_path().is_file() {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(db_path())
            .unwrap();

        // db.cvsへの追記失敗は元コードと同様に無視する（副次的な記録用ファイルのため）
        let _ = writeln!(file, "hash,id,song_key");
    }
}

/// アーティスト名と曲名から、曲を一意に識別する文字列キーを生成する
pub fn song_key(artist: &str, title: &str) -> String {
    format!("{} - {}", artist, title)
}

/// 曲キーをMD5でハッシュ化する（キャッシュファイル名およびDB記録に使用）
pub fn song_key_hash(song_key: &str) -> String {
    hex::encode(Md5::digest(song_key.as_bytes()))
}

/// 歌詞キャッシュJSONファイルのパスを返す
fn lyric_cache_path(song_key_hash: &str) -> PathBuf {
    CACHE_DIR.join(format!("{}.json", song_key_hash))
}

/// 取得済み楽曲の一覧を記録するDBファイル（CSV形式）のパスを返す
fn db_path() -> PathBuf {
    CACHE_DIR.join("db.cvs")
}

/// 取得した歌詞をキャッシュJSONへ書き込み、DBファイルに追記する。
///
/// （元コードで3箇所に重複していた「キャッシュ保存＋DB追記」を集約したもの）
pub fn save_lyrics(song_key_hash: &str, id: u64, song_key: &str, lyrics: &[Lyric]) {
    let cache_lyric_file_path = lyric_cache_path(song_key_hash);

    tracing::info!("Save file to {}", cache_lyric_file_path.to_string_lossy());
    let json = serde_json::to_string(lyrics).unwrap();
    std::fs::write(&cache_lyric_file_path, json).unwrap();

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(db_path())
        .unwrap();

    // db.cvsへの追記失敗は元コードと同様に無視する（副次的な記録用ファイルのため）
    let _ = writeln!(file, "{},{},{}", song_key_hash, id, song_key);
}

/// ローカルキャッシュに保存済みの歌詞を読み込む（存在しなければ`None`）
pub fn load_lyrics(song_key_hash: &str) -> Option<Vec<Lyric>> {
    let cache_lyric_file_path = lyric_cache_path(song_key_hash);
    if cache_lyric_file_path.is_file() {
        let json = std::fs::read_to_string(&cache_lyric_file_path).unwrap();
        return Some(serde_json::from_str(&json).unwrap());
    }
    None
}
