//! CourtListener REST ingestor (ul2.1): audio + built-in STT transcripts.
//!
//! See `docs/research/corpus/court-audio-transcript-sources.md` §1, §7.
//! Endpoint: `GET https://www.courtlistener.com/api/rest/v4/audio/` with
//! `Authorization: Token <token>`. Uses **cursor pagination** via the `next`
//! field (the endpoint rejects `?limit=`).
//!
//! The token comes from the `COURTLISTENER_API_TOKEN` env var or the
//! gitignored repo-root `.env` file — never from a literal in source.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use invariant_ppt::assert_invariant;
use serde::Deserialize;

use crate::dotenv;
use crate::invariants::TOKEN_NEVER_HARDCODED;

/// Env var holding the CourtListener API token.
pub const TOKEN_ENV: &str = "COURTLISTENER_API_TOKEN";
/// The `.env.example` placeholder value — a loaded token must not equal it.
pub const PLACEHOLDER_TOKEN: &str = "---";
/// Base REST URL for the v4 audio endpoint.
pub const AUDIO_API_URL: &str = "https://www.courtlistener.com/api/rest/v4/audio/";

/// One CourtListener audio object (subset of the v4 fields we consume).
#[derive(Debug, Clone, Deserialize)]
pub struct AudioObject {
    pub id: u64,
    #[serde(default)]
    pub local_path_mp3: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub duration: Option<f32>,
    #[serde(default)]
    pub stt_status: Option<i32>,
    #[serde(default)]
    pub stt_transcript: Option<String>,
    #[serde(default)]
    pub date_created: Option<String>,
    /// Docket/case metadata (case name, court, argued date).
    #[serde(default)]
    pub docket: Option<serde_json::Value>,
}

impl AudioObject {
    /// The transcript is usable when STT ran and produced non-empty text.
    pub fn has_transcript(&self) -> bool {
        self.stt_transcript
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
    }
}

/// One cursor-paginated page of results.
///
/// Note: CourtListener echoes `count` back as a URL string when queried with
/// `count=on`, so it is kept as raw JSON rather than a number.
#[derive(Debug, Clone, Deserialize)]
pub struct AudioPage {
    #[serde(default)]
    pub count: serde_json::Value,
    #[serde(default)]
    pub next: Option<String>,
    pub results: Vec<AudioObject>,
}

/// Reference transcript text from a transcript-manifest JSON value.
///
/// Reads the CourtListener STT field. Eval/reference material only: scoring
/// harnesses use it as ground truth, never as live-scoring input. The raw
/// field stays inside ingestion so the no-model guard keeps passing.
pub fn reference_transcript_text(v: &serde_json::Value) -> Option<&str> {
    v.get("stt_transcript")?.as_str()
}

/// Load the CourtListener token from env/`.env`, enforcing
/// `token_never_hardcoded` (no literals, no placeholder value).
pub fn api_token() -> Result<String, String> {
    let token = dotenv::get_var(TOKEN_ENV).ok_or_else(|| {
        format!(
            "{} not set (env or repo-root .env). Copy .env.example -> .env and fill it in.",
            TOKEN_ENV
        )
    })?;
    assert_invariant(
        !token.is_empty() && token != PLACEHOLDER_TOKEN,
        TOKEN_NEVER_HARDCODED,
        Some("courtlistener auth"),
    );
    Ok(token)
}

/// Build the `Authorization` header value: `Token <token>`.
pub fn auth_header() -> Result<String, String> {
    Ok(format!("Token {}", api_token()?))
}

/// Blocking CourtListener REST client.
pub struct CourtListenerClient {
    http: reqwest::blocking::Client,
    token: String,
}

impl CourtListenerClient {
    /// Construct from the env/`.env` token.
    pub fn from_env() -> Result<Self, String> {
        let token = api_token()?;
        let http = reqwest::blocking::Client::builder()
            .user_agent("hearsay-iv-corpus/0.1 (research prototype)")
            .build()
            .map_err(|e| format!("reqwest client: {e}"))?;
        Ok(Self { http, token })
    }

    fn get_page(&self, url: &str) -> Result<AudioPage, String> {
        let resp = self
            .http
            .get(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Token {}", self.token),
            )
            .send()
            .map_err(|e| format!("GET {url}: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("GET {url}: HTTP {status}"));
        }
        resp.json::<AudioPage>()
            .map_err(|e| format!("GET {url}: JSON parse: {e}"))
    }

    /// Fetch audio objects for a court via cursor pagination, walking `next`
    /// up to `max_pages` pages (pass `usize::MAX` for all).
    pub fn list_audio(&self, court: &str, max_pages: usize) -> Result<Vec<AudioObject>, String> {
        let mut all = Vec::new();
        let mut url = format!("{AUDIO_API_URL}?docket__court={court}&format=json");
        for _ in 0..max_pages {
            let page = self.get_page(&url)?;
            all.extend(page.results);
            match page.next {
                Some(next) if !next.is_empty() => url = next,
                _ => break,
            }
        }
        Ok(all)
    }
    /// Download raw bytes from an audio URL with the API token auth.
    /// Used by the corpus-fetch bin for `local_path_mp3` / `download_url`.
    pub fn download_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .http
            .get(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Token {}", self.token),
            )
            .send()
            .map_err(|e| format!("GET {url}: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("GET {url}: HTTP {status}"));
        }
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("GET {url}: read body: {e}"))
    }
}

/// A single SCOTUS audio object with its transcript, saved to `data/corpus/`.
///
/// (Reference-only for now: writes the transcript + metadata manifest; audio
/// download is a follow-up beyond ul2.1.)
pub fn save_object_manifest(obj: &AudioObject, out: &Path) -> Result<(), String> {
    let manifest = serde_json::json!({
        "id": obj.id,
        "local_path_mp3": obj.local_path_mp3,
        "download_url": obj.download_url,
        "duration": obj.duration,
        "stt_status": obj.stt_status,
        "has_transcript": obj.has_transcript(),
        "date_created": obj.date_created,
        "docket": obj.docket,
    });
    std::fs::create_dir_all(out).map_err(|e| format!("mkdir {}: {e}", out.display()))?;
    let file = out.join(format!("audio_{}.json", obj.id));
    let mut f = File::create(&file).map_err(|e| format!("create {}: {e}", file.display()))?;
    f.write_all(
        serde_json::to_string_pretty(&manifest)
            .unwrap_or_default()
            .as_bytes(),
    )
    .map_err(|e| format!("write {}: {e}", file.display()))?;
    Ok(())
}
