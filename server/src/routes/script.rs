use axum::extract::{Path, State};

use crate::app_state::AppState;
use crate::error::AppError;

pub async fn get_script(
    State(state): State<AppState>,
    Path(script_path): Path<String>,
) -> Result<(axum::http::HeaderMap, String), AppError> {
    let script_token = script_path
        .strip_suffix(".js")
        .ok_or_else(|| AppError::invalid_request("script path must end with .js"))?;

    state
        .script
        .render_script_by_token(script_token)
        .await
}
