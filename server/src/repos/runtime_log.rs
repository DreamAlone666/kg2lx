use async_trait::async_trait;

use crate::domain::runtime_log::RuntimeLog;
use crate::error::AppError;

#[async_trait]
pub trait RuntimeLogRepo: Send + Sync {
    async fn append(&self, log: &RuntimeLog) -> Result<(), AppError>;
}
