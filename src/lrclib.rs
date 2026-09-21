use serde::Deserialize;
use thiserror::Error;

const LRCLIB_URL: &str = "https://lrclib.net";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

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
            let dur_s = dur.to_string();
            query_params.push(("duration", dur_s));
        }

        let query_string: String = query_params
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<String>>()
            .join("&");

        let url = format!("{}/api/get?{}", LRCLIB_URL, query_string);
        tracing::info!("Get: {}", url);

        let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
        let response = client.get(url).send().await?;
        let status = response.status();

        if status == reqwest::StatusCode::NOT_FOUND {
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

        let json: serde_json::Value = response.json().await?;
        let lyrics = LRCLIBResponse {
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
        };

        return Ok(lyrics);
    }

    pub async fn get_lyrics_by_lrclibs_id(id: u64) -> Result<LRCLIBResponse, LRCLIBError> {
        let url = format!("{}/api/get/{}", LRCLIB_URL, id);
        tracing::info!("Get: {}", url);

        let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
        let response = client.get(url).send().await?;
        let status = response.status();

        if status == reqwest::StatusCode::NOT_FOUND {
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

        let json: serde_json::Value = response.json().await?;
        let lyrics = LRCLIBResponse {
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
        };

        return Ok(lyrics);
    }
}
