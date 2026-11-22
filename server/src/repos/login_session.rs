use async_trait::async_trait;

use crate::domain::account::ProviderAccount;
use crate::domain::login_session::LoginSession;
use crate::domain::source::Source;
use crate::error::AppError;

#[async_trait]
pub trait LoginSessionRepo: Send + Sync {
    async fn find_by_id(&self, session_id: &str) -> Result<Option<LoginSession>, AppError>;
    async fn save(&self, session: &LoginSession) -> Result<(), AppError>;
    async fn bind(
        &self,
        session: &LoginSession,
        account: &ProviderAccount,
        new_source: Option<&Source>,
    ) -> Result<(), AppError>;
}
