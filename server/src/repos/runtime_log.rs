use async_trait::async_trait;

use crate::domain::runtime_log::{ListRuntimeLogsQuery, RuntimeLog};
use crate::error::AppError;

#[async_trait]
pub trait RuntimeLogRepo: Send + Sync {
    async fn append(&self, log: &RuntimeLog) -> Result<(), AppError>;
    async fn list_by_source(
        &self,
        query: &ListRuntimeLogsQuery,
    ) -> Result<Vec<RuntimeLog>, AppError>;
}
