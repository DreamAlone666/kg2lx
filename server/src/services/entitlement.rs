mod renewal;

use std::sync::Arc;
use std::time::Instant;

use chrono::NaiveDate;

use crate::domain::account::ProviderAccount;
use crate::domain::event_log::{
    EntitlementAction, EntitlementEvent, Event, EventContext, EventLog,
};
use crate::domain::renewal::{RenewalOutcome, RenewalReason, SvipEvidence, upstream_day};
use crate::error::{AppError, ErrorCode};
use crate::repos::account::AccountRepo;
use crate::repos::entitlement::EntitlementRepo;
use crate::repos::event_log::{self, EventLogRepo};
use crate::repos::renewal::RenewalRepo;
use crate::services::kugou_lite_client::entitlement::{
    EntitlementUpstream, FailureKind, UpstreamFailure, UpstreamReply,
};
use crate::services::singleflight::{KeyedLocks, SingleFlight};

pub struct EntitlementService {
    upstream: Arc<dyn EntitlementUpstream>,
    accounts: Arc<dyn AccountRepo>,
    snapshots: Arc<dyn EntitlementRepo>,
    renewals: Arc<dyn RenewalRepo>,
    logs: Arc<dyn EventLogRepo>,
    now: Box<dyn Fn() -> i64 + Send + Sync>,
    syncs: SingleFlight<i64>,
    automatic_runs: SingleFlight<()>,
    manual_runs: SingleFlight<()>,
    admin_actions: Arc<KeyedLocks>,
}

impl EntitlementService {
    pub fn new(
        upstream: Arc<dyn EntitlementUpstream>,
        accounts: Arc<dyn AccountRepo>,
        snapshots: Arc<dyn EntitlementRepo>,
        renewals: Arc<dyn RenewalRepo>,
        logs: Arc<dyn EventLogRepo>,
        admin_actions: Arc<KeyedLocks>,
        now: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            upstream,
            accounts,
            snapshots,
            renewals,
            logs,
            now: Box::new(now),
            syncs: SingleFlight::default(),
            automatic_runs: SingleFlight::default(),
            manual_runs: SingleFlight::default(),
            admin_actions,
        }
    }

    pub async fn sync(
        self: &Arc<Self>,
        account_id: String,
        context: EventContext,
        reason: RenewalReason,
    ) -> Result<i64, AppError> {
        let service = self.clone();
        self.syncs
            .run(account_id.clone(), async move {
                let _action = service.admin_actions.lock(&account_id).await;
                let mut account = service.account(&account_id).await?;
                service
                    .read_snapshot(&mut account, &context, reason)
                    .await?
                    .map(|(synced_at, _)| synced_at)
                    .map_err(|failure| AppError::upstream_vip_check_failed(failure.message))
            })
            .await
    }

    async fn account(&self, account_id: &str) -> Result<ProviderAccount, AppError> {
        self.accounts
            .find_by_id(account_id)
            .await?
            .ok_or_else(AppError::account_not_found)
    }

    async fn use_reply<T>(
        &self,
        account: &mut ProviderAccount,
        reply: UpstreamReply<T>,
    ) -> Result<Result<T, UpstreamFailure>, AppError> {
        *account = self.accounts.merge_cookies(account, &reply.cookies).await?;
        Ok(reply.result)
    }

    async fn read_snapshot(
        &self,
        account: &mut ProviderAccount,
        context: &EventContext,
        reason: RenewalReason,
    ) -> Result<Result<(i64, SvipEvidence), UpstreamFailure>, AppError> {
        let started = Instant::now();
        let requested_at = (self.now)();
        let reply = self.upstream.snapshot(&account.cookies, requested_at).await;
        let observed_at = (self.now)();
        let result = match self.use_reply(account, reply).await? {
            Ok(snapshot) => {
                let svip = snapshot.svip;
                self.snapshots
                    .record_sync(&account.account_id, Some((snapshot, observed_at)))
                    .await?;
                Ok((observed_at, svip))
            }
            Err(failure) => {
                self.snapshots
                    .record_sync(&account.account_id, None)
                    .await?;
                Err(failure)
            }
        };
        let failure = result.as_ref().err().copied();
        self.log_action(
            context,
            EntitlementAction::Sync,
            started,
            action_observation(
                reason,
                upstream_day(requested_at),
                if failure.is_some() {
                    RenewalOutcome::Failed
                } else {
                    RenewalOutcome::Succeeded
                },
                failure,
            ),
            failure.map(|_| ErrorCode::UpstreamVipCheckFailed),
        )
        .await;
        Ok(result)
    }

    async fn log_action(
        &self,
        context: &EventContext,
        action: EntitlementAction,
        started: Instant,
        observation: EntitlementEvent,
        error: Option<ErrorCode>,
    ) {
        event_log::record(
            self.logs.as_ref(),
            EventLog::new(
                context.clone(),
                Event::Entitlement {
                    action,
                    observation: Some(observation),
                },
                Some(started.elapsed().as_millis() as u64),
                error,
            ),
        )
        .await;
    }
}

fn action_observation(
    reason: RenewalReason,
    upstream_day: NaiveDate,
    outcome: RenewalOutcome,
    failure: Option<UpstreamFailure>,
) -> EntitlementEvent {
    EntitlementEvent {
        reason,
        upstream_day,
        outcome,
        upstream_code: failure.and_then(|failure| failure.code),
        http_status: failure.and_then(|failure| failure.http_status),
        human_verification: failure
            .is_some_and(|failure| failure.kind == FailureKind::Verification),
        retry_at: None,
        propagating_until: None,
        estimated_end_at: None,
    }
}
