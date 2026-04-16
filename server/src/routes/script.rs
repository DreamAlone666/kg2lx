use axum::extract::{Path, State};

use crate::app_state::AppState;
use crate::error::AppError;

pub async fn get_script(
    State(state): State<AppState>,
    Path(script_token): Path<String>,
) -> Result<(axum::http::HeaderMap, String), AppError> {
    state
        .script
        .render_script_by_token(&script_token)
        .await
}
