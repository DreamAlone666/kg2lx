use async_trait::async_trait;

use crate::domain::source::Source;
use crate::error::AppError;

#[async_trait]
pub trait SourceRepo: Send + Sync {
    async fn find_by_id(&self, source_id: &str) -> Result<Option<Source>, AppError>;
    async fn find_by_script_token(&self, script_token: &str) -> Result<Option<Source>, AppError>;
    async fn find_by_runtime_token(&self, runtime_token: &str) -> Result<Option<Source>, AppError>;
    async fn find_by_account_id(&self, account_id: &str) -> Result<Option<Source>, AppError>;
    async fn update_settings(
        &self,
        source_id: &str,
        name: Option<String>,
        quality_fallback: Option<bool>,
        updated_at: i64,
    ) -> Result<Source, AppError>;
    async fn list_all(&self) -> Result<Vec<Source>, AppError>;
}
