use async_trait::async_trait;

use crate::domain::entitlement::EntitlementObservation;
#[cfg(test)]
use crate::domain::entitlement::EntitlementSnapshot;
use crate::error::AppError;

#[async_trait]
pub trait EntitlementRepo: Send + Sync {
    #[cfg(test)]
    async fn entitlement_snapshot(&self, account_id: &str)
    -> Result<EntitlementSnapshot, AppError>;

    async fn record_sync(
        &self,
        account_id: &str,
        snapshot: Option<(EntitlementObservation, i64)>,
    ) -> Result<(), AppError>;
}
