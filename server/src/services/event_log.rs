use std::sync::Arc;

use crate::domain::event_log::{EventType, ListGlobalEvents, LogView, StoredEventLog};
use crate::error::AppError;
use crate::repos::event_log::EventLogRepo;

#[derive(serde::Serialize)]
pub struct GlobalLogsResponse {
    pub view: LogView,
    pub event_type: Option<EventType>,
    pub account_id: Option<String>,
    pub source_id: Option<String>,
    pub correlation_id: Option<String>,
    pub limit: usize,
    pub has_more: bool,
    pub next_before_seq: Option<i64>,
    pub items: Vec<StoredEventLog>,
}

pub struct EventLogService {
    repo: Arc<dyn EventLogRepo>,
}

impl EventLogService {
    pub fn new(repo: Arc<dyn EventLogRepo>) -> Self {
        Self { repo }
    }

    pub async fn list(&self, input: ListGlobalEvents) -> Result<GlobalLogsResponse, AppError> {
        let query = ListGlobalEvents {
            limit: input.limit + 1,
            ..input.clone()
        };
        let mut items = self.repo.list_global(&query).await?;
        let has_more = items.len() > input.limit;
        items.truncate(input.limit);
        let next_before_seq =
            if has_more && input.correlation_id.is_none() && input.after_seq.is_none() {
                items.last().map(|item| item.seq)
            } else {
                None
            };
        Ok(GlobalLogsResponse {
            view: input.view,
            event_type: input.event_type,
            account_id: input.account_id,
            source_id: input.source_id,
            correlation_id: input.correlation_id,
            limit: input.limit,
            has_more,
            next_before_seq,
            items,
        })
    }
}
