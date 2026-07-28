use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, header};

use crate::app_state::AppState;
use crate::error::AppError;
use crate::services::runtime::RuntimeMusicUrlRequest;

fn verify_runtime(headers: &HeaderMap) -> Result<String, AppError> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return Err(AppError::admin_unauthorized());
    }
    Ok(token.to_string())
}

pub async fn music_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    let runtime_token = verify_runtime(&headers)?;

    match std::str::from_utf8(body.as_ref()) {
        Ok(s) => tracing::debug!(
            content_length = body.len(),
            content_type = ?headers.get(header::CONTENT_TYPE),
            raw_body = s,
            "captured runtime music-url request body",
        ),
        Err(e) => tracing::debug!(
            content_length = body.len(),
            content_type = ?headers.get(header::CONTENT_TYPE),
            raw_bytes = ?body.as_ref(),
            utf8_error = %e,
            "runtime music-url body is not valid UTF-8",
        ),
    }

    let req = parse_music_url_request(&body)?;
    let resp = state.runtime.fetch_music_url(&runtime_token, req).await?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

fn parse_music_url_request(body: &[u8]) -> Result<RuntimeMusicUrlRequest, AppError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| AppError::invalid_request("invalid json"))?;
    // Tolerate two request body shapes for backward compatibility with scripts
    // older than 0.1.1: a plain JSON object, or a JSON string wrapping that
    // object (inner quotes backslash-escaped). Scripts at 0.1.1+ pass the
    // object directly; older scripts passed a `JSON.stringify`'d string body,
    // which can arrive here as the wrapped shape. Normalize the latter before
    // deserializing. See `tests` for exact captured bytes.
    let value = match value {
        serde_json::Value::String(s) => serde_json::from_str(&s)
            .map_err(|_| AppError::invalid_request("invalid request body"))?,
        value => value,
    };
    let req: RuntimeMusicUrlRequest = serde_json::from_value(value)
        .map_err(|_| AppError::invalid_request("invalid request body"))?;

    if req.hash.is_empty() {
        return Err(AppError::invalid_request("hash is required"));
    }
    match req.quality.as_str() {
        "128k" | "320k" | "flac" | "flac24bit" => Ok(req),
        _ => Err(AppError::invalid_request("invalid quality")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // REAL WIRE CAPTURES, 2026-07-28. The backend must accept two body shapes:
    // a plain JSON object, and a JSON string wrapping that object. Both
    // fixtures are the SAME song (hash 1A3C..., quality flac) so the only
    // variable is shape; the 12B delta = 10 inner quotes each gaining one `\`
    // + 2 outer string quotes. Captured via the debug log in `music_url` with
    // `RUST_LOG=kg2lx_server::routes::runtime=debug`; the `len()` assertions
    // pin each fixture to the server-observed `content_length`, so any
    // transcription error fails the test. The string-wrapped fixture is the
    // shape scripts older than 0.1.1 can produce; 0.1.1+ scripts produce the
    // object shape.

    // Shape A: a plain JSON object (captured content_length=82).
    static OBJECT_BODY: &[u8] =
        br#"{"hash":"1A3CAACB1E361DAF217A2383E9C8E292","album_audio_id":null,"quality":"flac"}"#;

    // Shape B: a JSON string wrapping the same object text, inner quotes
    // backslash-escaped (captured content_length=94, same song as Shape A).
    static STRING_WRAPPED_BODY: &[u8] =
        b"\"{\\\"hash\\\":\\\"1A3CAACB1E361DAF217A2383E9C8E292\\\",\\\"album_audio_id\\\":null,\\\"quality\\\":\\\"flac\\\"}\"";

    #[test]
    fn parses_object_body() {
        let body = OBJECT_BODY;
        assert_eq!(body.len(), 82, "fixture must match captured content_length");
        let r = parse_music_url_request(body).expect("object body should parse");
        assert_eq!(r.hash, "1A3CAACB1E361DAF217A2383E9C8E292");
        assert!(r.album_audio_id.is_none());
        assert_eq!(r.quality, "flac");
    }

    #[test]
    fn parses_string_wrapped_body() {
        let body = STRING_WRAPPED_BODY;
        assert_eq!(body.len(), 94, "fixture must match captured content_length");
        let r = parse_music_url_request(body).expect("string-wrapped body should parse");
        assert_eq!(r.hash, "1A3CAACB1E361DAF217A2383E9C8E292");
        assert!(r.album_audio_id.is_none());
        assert_eq!(r.quality, "flac");
    }

    #[test]
    fn both_shapes_normalize_to_same_request() {
        let object = parse_music_url_request(OBJECT_BODY).expect("object shape parses");
        let wrapped =
            parse_music_url_request(STRING_WRAPPED_BODY).expect("string-wrapped shape parses");
        assert_eq!(
            object.hash, wrapped.hash,
            "same song must normalize to same hash regardless of shape"
        );
        assert_eq!(object.quality, wrapped.quality);
        assert_eq!(object.album_audio_id, wrapped.album_audio_id);
    }
}
