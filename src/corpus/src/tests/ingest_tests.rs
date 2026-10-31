//! ul2.1 gate: `cd src/corpus && cargo test ingest`
//!
//! One live test (network) + one offline test (auth header from env/.env,
//! never a hardcoded literal).

use crate::courtlistener::{CourtListenerClient, PLACEHOLDER_TOKEN, api_token, auth_header};

/// OFFLINE: the auth header is built from the env/.env token — never a
/// literal in source, never the placeholder.
#[test]
fn ingest_auth_header_from_env_never_hardcoded() {
    let token = match api_token() {
        Ok(token) => token,
        Err(e) => {
            eprintln!("SKIP: {e}");
            return;
        }
    };
    assert!(!token.is_empty());
    assert_ne!(
        token, PLACEHOLDER_TOKEN,
        "loaded token must not be the .env.example placeholder"
    );
    // The token must not appear as a literal anywhere in this crate's source.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    fn read_rs_hay(dir: &std::path::Path, out: &mut String) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                read_rs_hay(&path, out)?;
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push_str(&std::fs::read_to_string(&path)?);
            }
        }
        Ok(())
    }
    let mut hay = String::new();
    read_rs_hay(&manifest_dir.join("src"), &mut hay).expect("walk crate source");
    assert!(
        !hay.contains(&token),
        "token must never be hardcoded in crate source"
    );
    let header = auth_header().expect("header built");
    assert!(
        header.starts_with("Token "),
        "header format: 'Token <token>'"
    );
    assert_eq!(header, format!("Token {token}"));
}

/// LIVE: fetch >=1 SCOTUS audio object via `docket__court=scotus` and assert
/// the built-in STT transcript and enhanced MP3 are present.
#[test]
fn ingest_fetches_scotus_audio_with_stt_transcript() {
    let client = match CourtListenerClient::from_env() {
        Ok(client) => client,
        Err(e) => {
            eprintln!("SKIP: live CourtListener test needs a token: {e}");
            return;
        }
    };
    let objects = client
        .list_audio("scotus", 1)
        .expect("GET /api/rest/v4/audio/?docket__court=scotus");
    assert!(!objects.is_empty(), "at least one SCOTUS audio object");
    for obj in objects.iter().take(3) {
        assert!(
            obj.has_transcript(),
            "id {}: stt_transcript non-empty",
            obj.id
        );
        assert!(
            obj.local_path_mp3.as_deref().is_some_and(|p| !p.is_empty()),
            "id {}: local_path_mp3 present",
            obj.id
        );
        assert!(
            obj.duration.is_some_and(|d| d > 0.0),
            "id {}: duration positive",
            obj.id
        );
    }
}
