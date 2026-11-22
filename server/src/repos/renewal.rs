use async_trait::async_trait;
use chrono::NaiveDate;

use crate::domain::renewal::{DailyProgress, RenewalSettings, RenewalStatus, SvipEvidence};
use crate::error::AppError;

#[async_trait]
pub trait RenewalRepo: Send + Sync {
    async fn settings(&self, account_id: &str) -> Result<RenewalSettings, AppError>;
    async fn status(&self, account_id: &str, now: i64) -> Result<RenewalStatus, AppError>;
    async fn set_enabled(&self, account_id: &str, enabled: bool) -> Result<(), AppError>;
    async fn save_rate_limit(
        &self,
        account_id: &str,
        until: Option<i64>,
        streak: u8,
    ) -> Result<(), AppError>;
    async fn daily(&self, account_id: &str, day: NaiveDate) -> Result<DailyProgress, AppError>;
    async fn save_daily(
        &self,
        account_id: &str,
        day: NaiveDate,
        progress: &DailyProgress,
        propagating_until: Option<i64>,
    ) -> Result<(), AppError>;
    async fn pending_older_days(
        &self,
        account_id: &str,
        today: NaiveDate,
    ) -> Result<Vec<(NaiveDate, DailyProgress)>, AppError>;
    async fn svip_evidence(&self, account_id: &str, now: i64) -> Result<SvipEvidence, AppError>;
}
