//! LRCLIB (https://lrclib.net) の非公式APIクライアント。
//!
//! 元コードでは「HTTPクライアント生成→リクエスト送信→ステータスコード判定→
//! JSONパース」と「JSON→LRCLIBResponseへのマッピング」が3つのAPI呼び出し関数に
//! それぞれ重複していたため、共通ヘルパー関数（`request_json` / `response_from_json`）
//! に集約した。外部から見える公開API（関数シグネチャ・挙動）は変更していない。

use serde::Deserialize;
use thiserror::Error;

/// LRCLIB APIのベースURL
const LRCLIB_URL: &str = "https://lrclib.net";
/// リクエストヘッダに載せるUser-Agent（パッケージ名+バージョン）
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

/// LRCLIB APIが返す歌詞情報
#[derive(Debug, Deserialize)]
pub struct LRCLIBResponse {
    pub id: u64,
    pub name: String,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration: u64,
    pub instrumental: bool,
    pub plain_lyrics: Vec<String>,
    pub syncd_lyrics: Option<Vec<String>>,
    pub lyricsfile: String,
}

/// LRCLIB API呼び出しで発生しうるエラー
#[derive(Debug, Error)]
pub enum LRCLIBError {
    #[error("Lyrics not found.")]
    NotFound(),
    #[error("Too many request to LRCLIB.")]
    TooManyRequest(),
    #[error("Somethin wrong. code: {0}")]
    ResponseNotOK(reqwest::StatusCode),
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Default, Debug)]
pub struct LRCLIBAPI {}

impl LRCLIBAPI {
    /// 曲名・アーティスト名（任意でアルバム名・長さ）をもとにLRCLIBへ歌詞を問い合わせる。
    ///
    /// 対象が見つからない場合は`LRCLIBError::NotFound`を返す。
    pub async fn get_lyrics_with_a_tracks_signature(
        track_name: &str,
        artist_name: &str,
        album_name: Option<&str>,
        duration: Option<u16>,
    ) -> Result<LRCLIBResponse, LRCLIBError> {
        let mut query_params: Vec<(&str, String)> = vec![
            ("track_name", track_name.into()),
            ("artist_name", artist_name.into()),
        ];

        if let Some(album) = album_name {
            query_params.push(("album_name", album.into()));
        }

        if let Some(dur) = duration {
            query_params.push(("duration", dur.to_string()));
        }

        let url = format!(
            "{}/api/get?{}",
            LRCLIB_URL,
            build_query_string(query_params)
        );

        // このエンドポイントは404を「歌詞が見つからない」として扱う
        let json = request_json(&url, true).await?;
        Ok(response_from_json(&json))
    }

    /// LRCLIBの楽曲ID指定で歌詞を取得する。
    pub async fn get_lyrics_by_lrclibs_id(id: u64) -> Result<LRCLIBResponse, LRCLIBError> {
        let url = format!("{}/api/get/{}", LRCLIB_URL, id);

        // このエンドポイントも404を「歌詞が見つからない」として扱う
        let json = request_json(&url, true).await?;
        Ok(response_from_json(&json))
    }

    /// クエリ文字列でLRCLIBを検索し、候補となる歌詞情報の一覧を取得する。
    pub async fn search_lyrics(query: String) -> Result<Vec<LRCLIBResponse>, LRCLIBError> {
        let mut query_params: Vec<(&str, String)> = vec![("q", query)];
        let url = format!(
            "{}/api/search?{}",
            LRCLIB_URL,
            build_query_string(query_params),
        );

        // 検索APIには「404=見つからない」の特別扱いは無い（元コードと同様）
        let json = request_json(&url, false).await?;

        let mut response_array: Vec<LRCLIBResponse> = Vec::new();
        if let Some(array) = json.as_array() {
            for lyrics_raw in array {
                response_array.push(response_from_json(lyrics_raw));
            }
        }

        Ok(response_array)
    }
}

/// クエリパラメータの配列を `key=value&key=value...` 形式に組み立てる
fn build_query_string(query_params: Vec<(&str, String)>) -> String {
    query_params
        .into_iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v.as_str())))
        .collect::<Vec<String>>()
        .join("&")
}

/// GETリクエストを送信し、ステータスコードを確認したうえでレスポンスをJSONとして返す。
///
/// `treat_404_as_not_found` が`true`のエンドポイントのみ、404を
/// `LRCLIBError::NotFound` として扱う（元の各関数の挙動差をそのまま維持するため）。
async fn request_json(
    url: &str,
    treat_404_as_not_found: bool,
) -> Result<serde_json::Value, LRCLIBError> {
    tracing::info!("Get: {}", url);

    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let response = client.get(url).send().await?;
    let status = response.status();

    if treat_404_as_not_found && status == reqwest::StatusCode::NOT_FOUND {
        tracing::info!("Lyrics not found.");
        return Err(LRCLIBError::NotFound());
    }

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        tracing::info!("Too Many Request.");
        return Err(LRCLIBError::TooManyRequest());
    }

    if status != reqwest::StatusCode::OK {
        tracing::info!("Response Not OK. code: {}", status.as_str());
        return Err(LRCLIBError::ResponseNotOK(status));
    }

    Ok(response.json().await?)
}

/// `serde_json::Value` を `LRCLIBResponse` へマッピングする。
///
/// 元コードでは3つのAPI呼び出し（署名検索・ID指定取得・検索）それぞれで
/// 全く同じマッピング処理が重複していたため、ここに集約した。
fn response_from_json(json: &serde_json::Value) -> LRCLIBResponse {
    LRCLIBResponse {
        id: json["id"].as_u64().unwrap(),
        name: json["name"].as_str().unwrap().to_string(),
        track_name: json["trackName"].as_str().unwrap().to_string(),
        artist_name: json["artistName"].as_str().unwrap().to_string(),
        album_name: json["albumName"].as_str().unwrap().to_string(),
        duration: json["duration"].as_f64().unwrap() as u64,
        instrumental: json["instrumental"].as_bool().unwrap(),
        plain_lyrics: json["plainLyrics"]
            .as_str()
            .map(|lyrics| lyrics.lines().map(str::to_string).collect())
            .unwrap_or_default(),
        syncd_lyrics: json["syncedLyrics"]
            .as_str()
            .map(|lyrics| lyrics.lines().map(str::to_string).collect()),
        lyricsfile: json["lyricsfile"].as_str().unwrap().to_string(),
    }
}
