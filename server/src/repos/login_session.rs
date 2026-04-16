use async_trait::async_trait;

use crate::domain::login_session::LoginSession;
use crate::error::AppError;

#[async_trait]
pub trait LoginSessionRepo: Send + Sync {
    async fn find_by_id(&self, session_id: &str) -> Result<Option<LoginSession>, AppError>;
    async fn save(&self, session: &LoginSession) -> Result<(), AppError>;
}
