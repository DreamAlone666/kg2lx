use async_trait::async_trait;

use crate::domain::account::ProviderAccount;
use crate::domain::cookie_store::CookieStore;
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
    /// Persist the authentication outcome if the credentials used are still current.
    async fn save_auth(
        &self,
        previous: &ProviderAccount,
        updated: &ProviderAccount,
    ) -> Result<ProviderAccount, AppError>;

    /// Apply response changes to keys that still match the request, and return the current account.
    async fn merge_cookies(
        &self,
        request: &ProviderAccount,
        response: &CookieStore,
    ) -> Result<ProviderAccount, AppError>;
}
