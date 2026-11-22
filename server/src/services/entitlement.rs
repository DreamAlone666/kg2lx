use std::sync::Arc;
use std::time::Instant;

use crate::domain::account::now_ts;
use crate::domain::event_log::{EntitlementAction, Event, EventContext, EventLog};
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::entitlement::EntitlementRepo;
use crate::repos::event_log::{self, EventLogRepo};
use crate::services::kugou_lite_client::KugouLiteClient;
use crate::services::singleflight::{KeyedLocks, SingleFlight};

pub struct EntitlementService {
    client: Arc<KugouLiteClient>,
    accounts: Arc<dyn AccountRepo>,
    snapshots: Arc<dyn EntitlementRepo>,
    logs: Arc<dyn EventLogRepo>,
    syncs: SingleFlight<i64>,
    admin_actions: Arc<KeyedLocks>,
}

impl EntitlementService {
    pub fn new(
        client: Arc<KugouLiteClient>,
        accounts: Arc<dyn AccountRepo>,
        snapshots: Arc<dyn EntitlementRepo>,
        logs: Arc<dyn EventLogRepo>,
        admin_actions: Arc<KeyedLocks>,
    ) -> Self {
        Self {
            client,
            accounts,
            snapshots,
            logs,
            syncs: SingleFlight::default(),
            admin_actions,
        }
    }

    pub async fn sync(
        self: &Arc<Self>,
        account_id: String,
        context: EventContext,
    ) -> Result<i64, AppError> {
        let service = self.clone();
        self.syncs
            .run(account_id.clone(), async move {
                let _action = service.admin_actions.lock(&account_id).await;
                let account = service
                    .accounts
                    .find_by_id(&account_id)
                    .await?
                    .ok_or_else(AppError::account_not_found)?;
                let start = Instant::now();
                let result = async {
                    match service
                        .client
                        .fetch_vip_status(&account.cookies, service.client.default_timeout())
                        .await
                    {
                        Ok(result) => {
                            service
                                .accounts
                                .merge_cookies(&account, &result.cookies)
                                .await?;
                            let synced_at = now_ts();
                            service
                                .snapshots
                                .record_sync(&account_id, Some((result.entitlements, synced_at)))
                                .await?;
                            Ok(synced_at)
                        }
                        Err(error) => {
                            service.snapshots.record_sync(&account_id, None).await?;
                            Err(error)
                        }
                    }
                }
                .await;
                event_log::record(
                    service.logs.as_ref(),
                    EventLog::new(
                        context,
                        Event::Entitlement {
                            action: EntitlementAction::Sync,
                        },
                        Some(start.elapsed().as_millis() as u64),
                        result.as_ref().err().map(|error| error.code),
                    ),
                )
                .await;
                result
            })
            .await
    }
}
