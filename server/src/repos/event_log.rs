use async_trait::async_trait;

use crate::domain::event_log::{EventLog, ListEvents, ListGlobalEvents, StoredEventLog};
use crate::error::AppError;

#[async_trait]
pub trait EventLogRepo: Send + Sync {
    async fn append(&self, event: &EventLog) -> Result<(), AppError>;
    async fn list(&self, query: &ListEvents) -> Result<Vec<EventLog>, AppError>;
    async fn list_global(&self, query: &ListGlobalEvents) -> Result<Vec<StoredEventLog>, AppError>;
}

pub async fn record(repo: &dyn EventLogRepo, event: EventLog) {
    if let Err(error) = repo.append(&event).await {
        tracing::warn!(event_type = event.event.event_type().as_str(), %error, "could not record event");
    }
}
