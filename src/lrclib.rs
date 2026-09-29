//! LRCLIB (https://lrclib.net) の非公式APIクライアント。
//!
//! 元コードでは「HTTPクライアント生成→リクエスト送信→ステータスコード判定→
//! JSONパース」と「JSON→LRCLIBResponseへのマッピング」が3つのAPI呼び出し関数に
//! それぞれ重複していたため、共通ヘルパー関数（`request_json` / `response_from_json`）
//! に集約した。外部から見える公開API（関数シグネチャ・挙動）は変更していない。

use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// LRCLIB APIのベースURL
const LRCLIB_URL: &str = "https://lrclib.net";
/// リクエストヘッダに載せるUser-Agent（パッケージ名+バージョン+リポジトリURL）
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);
/// LRCLIBへの接続のタイムアウト
const TIMEOUT_SEC: u64 = 5;

/// LRCLIB APIが返す歌詞情報
#[derive(Debug, Deserialize)]
pub struct LRCLIBResponse {
    pub id: u64,
    #[serde(deserialize_with = "deserialize_duration")]
    pub duration: u64,
    #[serde(rename = "syncedLyrics", deserialize_with = "deserialize_lyrics")]
    pub syncd_lyrics: Option<Vec<String>>,
}

/// LRCLIB API呼び出しで発生しうるエラー
#[derive(Debug, Error)]
pub enum LRCLIBError {
    #[error("Lyrics not found.")]
    NotFound,
    #[error("Too many request to LRCLIB.")]
    TooManyRequest,
    #[error("LRCLIB Server Overload.")]
    Overload,
    #[error("Somethin wrong. code: {0}")]
    ResponseNotOK(reqwest::StatusCode),
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Clone, Default, Debug)]
#[allow(clippy::upper_case_acronyms)]
pub struct LRCLIBAPI;

impl LRCLIBAPI {
    /// 曲名・アーティスト名（任意でアルバム名・長さ）をもとにLRCLIBへ歌詞を問い合わせる。
    ///
    /// 対象が見つからない場合は`LRCLIBError::NotFound`を返す。
    pub async fn get_lyrics_with_a_tracks_signature(
        track_name: String,
        artist_name: String,
        album_name: Option<String>,
        duration: Option<u16>,
    ) -> Result<LRCLIBResponse, LRCLIBError> {
        let mut query_params: Vec<(&str, String)> =
            vec![("track_name", track_name), ("artist_name", artist_name)];

        if let Some(album) = album_name {
            query_params.push(("album_name", album));
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
        request_json(&url, true).await
    }

    /// LRCLIBの楽曲ID指定で歌詞を取得する。
    pub async fn get_lyrics_by_lrclibs_id(id: u64) -> Result<LRCLIBResponse, LRCLIBError> {
        let url = format!("{}/api/get/{}", LRCLIB_URL, id);

        // このエンドポイントも404を「歌詞が見つからない」として扱う
        request_json(&url, true).await
    }

    /// クエリ文字列でLRCLIBを検索し、候補となる歌詞情報の一覧を取得する。
    pub async fn search_lyrics(query: String) -> Result<Vec<LRCLIBResponse>, LRCLIBError> {
        let query_params: Vec<(&str, String)> = vec![("q", query)];
        let url = format!(
            "{}/api/search?{}",
            LRCLIB_URL,
            build_query_string(query_params),
        );

        // 検索APIには「404=見つからない」の特別扱いは無い（元コードと同様）
        request_json(&url, false).await
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
async fn request_json<T: DeserializeOwned>(
    url: &str,
    treat_404_as_not_found: bool,
) -> Result<T, LRCLIBError> {
    tracing::debug!("USER_AGENT: {}", USER_AGENT);
    tracing::info!("Get: {}", url);

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(TIMEOUT_SEC))
        .build()?;
    let response = client.get(url).send().await?;
    let status = response.status();

    if treat_404_as_not_found && status == reqwest::StatusCode::NOT_FOUND {
        tracing::info!("Lyrics not found.");
        return Err(LRCLIBError::NotFound);
    }

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        tracing::info!("Too Many Request.");
        return Err(LRCLIBError::TooManyRequest);
    }

    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        tracing::info!("Server Overloaded.");
        return Err(LRCLIBError::Overload);
    }

    if status != reqwest::StatusCode::OK {
        tracing::info!("Response Not OK. code: {}", status.as_str());
        return Err(LRCLIBError::ResponseNotOK(status));
    }

    Ok(response.json().await?)
}

/// 改行区切りの歌詞文字列を行ごとの配列へ変換する。
fn deserialize_lyrics<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
        .map(|lyrics| lyrics.map(|text| text.lines().map(str::to_owned).collect()))
}

/// APIが小数として返すことがある曲の長さを、秒単位の整数へ正規化する。
fn deserialize_duration<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    f64::deserialize(deserializer).map(|duration| duration as u64)
}
