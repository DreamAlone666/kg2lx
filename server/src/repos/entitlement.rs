use async_trait::async_trait;

use crate::domain::entitlement::{EntitlementSnapshot, VipEntitlement};
use crate::error::AppError;

#[async_trait]
pub trait EntitlementRepo: Send + Sync {
    async fn entitlement_snapshot(&self, account_id: &str)
    -> Result<EntitlementSnapshot, AppError>;

    async fn record_sync(
        &self,
        account_id: &str,
        snapshot: Option<(Vec<VipEntitlement>, i64)>,
    ) -> Result<(), AppError>;
}
