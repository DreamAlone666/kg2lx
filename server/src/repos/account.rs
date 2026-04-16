use async_trait::async_trait;

use crate::domain::account::ProviderAccount;
use crate::domain::provider::ProviderKind;
use crate::error::AppError;

#[async_trait]
pub trait AccountRepo: Send + Sync {
    async fn find_by_id(&self, account_id: &str) -> Result<Option<ProviderAccount>, AppError>;
    async fn find_by_provider_userid(
        &self,
        provider: ProviderKind,
        upstream_userid: &str,
    ) -> Result<Option<ProviderAccount>, AppError>;
    async fn upsert(&self, account: &ProviderAccount) -> Result<(), AppError>;
}
