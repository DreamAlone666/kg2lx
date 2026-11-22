use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::NaiveDate;

use super::{EntitlementService, action_observation};
use crate::domain::account::{AccountStatus, ProviderAccount};
use crate::domain::event_log::{EntitlementAction, EventContext, EventTrigger};
use crate::domain::renewal::{
    DailyProgress, DayPause, GrantStep, MIDNIGHT_MARGIN, PROPAGATION_WINDOW, PlaybackSignal,
    RENEWAL_THRESHOLD, RenewalOutcome, RenewalReason, RenewalSettings, RenewalStatus, StepState,
    SvipEvidence, WRITE_TIMEOUTS, next_upstream_day_start, upstream_day,
};
use crate::error::{AppError, ErrorCode};
use crate::services::kugou_lite_client::entitlement::{
    DailyRecord, FailureKind, GrantResult, RecordEntry, UpstreamFailure,
};

const MAX_RECORD_READS: u8 = 8;
const MAX_SNAPSHOT_READ_ATTEMPTS: u8 = 2;
const READ_RETRY_DELAY: i64 = 30;
const REJECTED_RETRY_DELAY: i64 = 15;

struct RenewalRun {
    account: ProviderAccount,
    day: NaiveDate,
    progress: DailyProgress,
    settings: RenewalSettings,
    context: EventContext,
}

impl RenewalRun {
    fn account_id(&self) -> &str {
        &self.account.account_id
    }

    fn can_continue(&self, now: i64) -> bool {
        upstream_day(now) == self.day
            && self.account.status != AccountStatus::Disabled
            && self.progress.paused.is_none()
            && self.progress.retry_at.is_none_or(|until| until <= now)
            && self
                .settings
                .rate_limit_until
                .is_none_or(|until| until <= now)
            && self
                .settings
                .propagating_until
                .is_none_or(|until| until <= now)
    }
}

impl EntitlementService {
    pub fn on_playback(self: &Arc<Self>, context: EventContext, signal: PlaybackSignal) {
        let Some(account_id) = context.account_id.clone() else {
            return;
        };
        let service = self.clone();
        // A link and a refusal can qualify for different triggers. Keep both signals if
        // they arrive together, then use the same account lock and durable progress.
        let key = format!("{account_id}/{signal:?}");
        self.automatic_runs.start(key, async move {
            let result = service.renew(&account_id, context, Some(signal)).await;
            if let Err(error) = &result {
                tracing::warn!(%error, "could not process automatic entitlement renewal");
            }
            result
        });
    }

    pub async fn renew_manually(
        self: &Arc<Self>,
        account_id: String,
    ) -> Result<RenewalStatus, AppError> {
        let service = self.clone();
        let context = EventContext::new(EventTrigger::Admin, Some(account_id.clone()), None);
        let key = account_id.clone();
        self.manual_runs
            .run(key.clone(), async move {
                service.renew(&key, context, None).await
            })
            .await?;
        self.renewal_status(&account_id).await
    }

    pub async fn set_auto_claim(
        &self,
        account_id: &str,
        enabled: bool,
    ) -> Result<RenewalStatus, AppError> {
        let _guard = self.admin_actions.lock(account_id).await;
        self.account(account_id).await?;
        self.renewals.set_enabled(account_id, enabled).await?;
        self.renewal_status(account_id).await
    }

    pub async fn renewal_status(&self, account_id: &str) -> Result<RenewalStatus, AppError> {
        self.renewals.status(account_id, (self.now)()).await
    }

    async fn renew(
        &self,
        account_id: &str,
        context: EventContext,
        signal: Option<PlaybackSignal>,
    ) -> Result<(), AppError> {
        if signal.is_some() && !self.renewals.settings(account_id).await?.enabled {
            return Ok(());
        }
        let _guard = self.admin_actions.lock(account_id).await;
        let settings = self.renewals.settings(account_id).await?;
        if signal.is_some() && !settings.enabled {
            return Ok(());
        }
        let account = self.account(account_id).await?;
        if account.status == AccountStatus::Disabled {
            return if signal.is_some() {
                Ok(())
            } else {
                Err(AppError::account_disabled())
            };
        }

        let now = (self.now)();
        let day = upstream_day(now);
        let mut run = RenewalRun {
            account,
            day,
            settings,
            context,
            progress: self.renewals.daily(account_id, day).await?,
        };
        if !run.can_continue(now) {
            return Ok(());
        }
        let older = self.renewals.pending_older_days(account_id, day).await?;
        if (run.progress.uncertain().is_some() || !older.is_empty())
            && !self.reconcile_record(&mut run, older).await?
        {
            return Ok(());
        }
        let now = (self.now)();
        if !run.can_continue(now) {
            return Ok(());
        }

        let evidence = self.renewals.svip_evidence(account_id, now).await?;
        let (reason, evidence) = match signal {
            None => (RenewalReason::Manual, evidence),
            Some(signal) => {
                let Some(decision) = self.automatic_decision(&mut run, signal, evidence).await?
                else {
                    return Ok(());
                };
                decision
            }
        };
        while let Some(step) = run.progress.next_step() {
            if !self.grant_step(&mut run, step, reason, evidence).await? {
                break;
            }
        }
        Ok(())
    }

    async fn automatic_decision(
        &self,
        run: &mut RenewalRun,
        signal: PlaybackSignal,
        mut evidence: SvipEvidence,
    ) -> Result<Option<(RenewalReason, SvipEvidence)>, AppError> {
        if run.progress.next_step().is_none() {
            // When the cache has settled, give the confirmed snapshot one chance to catch up.
            if evidence == SvipEvidence::Unknown && !run.progress.post_upgrade_read {
                run.progress.post_upgrade_read = true;
                self.save_run(run, None).await?;
                self.confirm_automatic_snapshot(run, RenewalReason::ConfirmSnapshot)
                    .await?;
            }
            return Ok(None);
        }
        if run.progress.claim.state == StepState::Done {
            return Ok(Some((RenewalReason::RecoverWrite, evidence)));
        }

        let remaining = evidence.remaining((self.now)());
        let reason = match (signal, remaining) {
            (_, None) => Some(RenewalReason::ConfirmSnapshot),
            (PlaybackSignal::TargetRefused, Some(remaining)) if remaining > 0 => {
                Some(RenewalReason::ReconcileMismatch)
            }
            _ => None,
        };
        if let Some(reason) = reason {
            if run.progress.snapshot_reads >= MAX_SNAPSHOT_READ_ATTEMPTS {
                return Ok(None);
            }
            run.progress.snapshot_reads += 1;
            self.save_run(run, None).await?;
            evidence = self.confirm_automatic_snapshot(run, reason).await?;
        }
        let trigger = match (signal, evidence.remaining((self.now)())) {
            (PlaybackSignal::Link, Some(remaining))
                if (1..RENEWAL_THRESHOLD).contains(&remaining) =>
            {
                Some(RenewalReason::T1LowBattery)
            }
            (PlaybackSignal::TargetRefused, Some(0)) => Some(RenewalReason::T2ExpiredRefusal),
            _ => None,
        };
        Ok(trigger.map(|reason| (reason, evidence)))
    }

    async fn confirm_automatic_snapshot(
        &self,
        run: &mut RenewalRun,
        reason: RenewalReason,
    ) -> Result<SvipEvidence, AppError> {
        match self
            .read_snapshot(&mut run.account, &run.context, reason)
            .await?
        {
            Ok((_, evidence)) => {
                run.progress.snapshot_reads = MAX_SNAPSHOT_READ_ATTEMPTS;
                self.save_run(run, None).await?;
                Ok(evidence)
            }
            Err(failure) => {
                let now = (self.now)();
                self.apply_backoff(run, failure, now).await?;
                run.progress.retry_at = Some(now + READ_RETRY_DELAY);
                self.save_run(run, None).await?;
                Ok(SvipEvidence::Unknown)
            }
        }
    }

    async fn reconcile_record(
        &self,
        run: &mut RenewalRun,
        older: Vec<(NaiveDate, DailyProgress)>,
    ) -> Result<bool, AppError> {
        if run.progress.record_reads >= MAX_RECORD_READS {
            return Ok(false);
        }
        run.progress.record_reads += 1;
        self.save_run(run, None).await?;
        let started = Instant::now();
        let response = self.upstream.daily_record(&run.account.cookies).await;
        let now = (self.now)();
        let result = self.use_reply(&mut run.account, response).await?;
        let failure = result.as_ref().err().copied();
        let outcome = match result {
            Ok(record) if upstream_day(now) == run.day => {
                self.apply_record(run, older, record, now).await?
            }
            Ok(_) => RenewalOutcome::Deferred,
            Err(failure) => {
                self.apply_backoff(run, failure, now).await?;
                run.progress.retry_at = Some(now + READ_RETRY_DELAY);
                run.progress.last_outcome = Some(RenewalOutcome::Deferred);
                self.save_run(run, None).await?;
                RenewalOutcome::Deferred
            }
        };
        let mut observation =
            action_observation(RenewalReason::RecoverWrite, run.day, outcome, failure);
        observation.retry_at = run.progress.retry_at;
        observation.propagating_until = run.settings.propagating_until.filter(|until| *until > now);
        self.log_action(
            &run.context,
            EntitlementAction::ReconcileRecord,
            started,
            observation,
            failure.map(entitlement_error),
        )
        .await;
        Ok(outcome != RenewalOutcome::Deferred)
    }

    async fn apply_record(
        &self,
        run: &mut RenewalRun,
        older: Vec<(NaiveDate, DailyProgress)>,
        record: DailyRecord,
        now: i64,
    ) -> Result<RenewalOutcome, AppError> {
        if record
            .days
            .last_key_value()
            .is_some_and(|(day, _)| *day > run.day)
        {
            // Future-dated rows make the assumed day boundary unsuitable for this record.
            run.progress.retry_at = Some(now + READ_RETRY_DELAY);
            run.progress.last_outcome = Some(RenewalOutcome::Deferred);
            self.save_run(run, None).await?;
            return Ok(RenewalOutcome::Deferred);
        }
        let current_entry = record.entry(run.day);
        let today_covered = current_entry != RecordEntry::Uncovered;
        let pending = run.progress.uncertain();
        let confirmed = observe_record(&mut run.progress, &current_entry);
        let mut may_continue = today_covered;
        let mut propagating_until = confirmed.then_some(now + PROPAGATION_WINDOW);
        if let Some(step) = run.progress.uncertain() {
            let missing_step = matches!(
                (step, &current_entry),
                (GrantStep::Claim, RecordEntry::Absent)
                    | (GrantStep::Upgrade, RecordEntry::Found(GrantStep::Claim))
            );
            if missing_step
                && run
                    .progress
                    .uncertain_until
                    .is_some_and(|until| until <= now)
            {
                run.progress.step(step).state = StepState::Ready;
            } else {
                may_continue = false;
            }
        }
        if run.progress.uncertain().is_none() {
            run.progress.uncertain_until = None;
        }

        // Save today's evidence before closing yesterday. A restart between these writes still
        // sees either the evidence or the older unresolved request.
        self.save_run(run, propagating_until).await?;
        for (day, mut progress) in older {
            let entry = record.entry(day);
            let upgraded = observe_record(&mut progress, &entry);
            let may_close = today_covered
                && progress.uncertain_until.is_some_and(|until| until <= now)
                && (entry != RecordEntry::Uncovered || day.succ_opt() < Some(run.day));
            if progress.uncertain().is_none() || may_close {
                progress.recovery_closed = true;
                let propagate = upgraded.then_some(now + PROPAGATION_WINDOW);
                self.renewals
                    .save_daily(run.account_id(), day, &progress, propagate)
                    .await?;
                propagating_until = propagating_until.or(propagate);
            } else {
                may_continue = false;
            }
        }
        if let Some(until) = propagating_until {
            run.settings.propagating_until = Some(until);
        }
        let outcome = if !may_continue {
            let safe_after = run.progress.uncertain_until.unwrap_or_default();
            run.progress.retry_at = Some((now + READ_RETRY_DELAY).max(safe_after));
            RenewalOutcome::Deferred
        } else if confirmed || (pending.is_some() && run.progress.next_step() != pending) {
            run.progress.retry_at = None;
            RenewalOutcome::AlreadyDone
        } else {
            run.progress.retry_at = None;
            RenewalOutcome::Succeeded
        };
        run.progress.last_outcome = Some(outcome);
        self.save_run(run, None).await?;
        Ok(outcome)
    }

    async fn grant_step(
        &self,
        run: &mut RenewalRun,
        step: GrantStep,
        reason: RenewalReason,
        evidence: SvipEvidence,
    ) -> Result<bool, AppError> {
        let now = (self.now)();
        if !run.can_continue(now) || run.progress.uncertain().is_some() {
            return Ok(false);
        }
        let Some(&timeout) = WRITE_TIMEOUTS.get(run.progress.step(step).attempts) else {
            return Ok(false);
        };
        let midnight = next_upstream_day_start(run.day);
        if now >= midnight - MIDNIGHT_MARGIN {
            run.progress.retry_at = Some(midnight);
            self.save_run(run, None).await?;
            return Ok(false);
        }
        let progress = run.progress.step(step);
        progress.state = StepState::Uncertain;
        progress.attempts += 1;
        run.progress.uncertain_until = Some(now + timeout as i64 + PROPAGATION_WINDOW);
        run.progress.retry_at = None;
        self.save_run(run, None).await?;

        // SQLite may have waited for a writer. Recheck the day immediately before sending.
        let sent_at = (self.now)();
        if upstream_day(sent_at) != run.day || sent_at >= midnight - MIDNIGHT_MARGIN {
            restore_unsent(&mut run.progress, step);
            self.save_run(run, None).await?;
            return Ok(false);
        }
        let started = Instant::now();
        let reply = self
            .upstream
            .grant(
                &run.account.cookies,
                step,
                run.day,
                Duration::from_secs(timeout),
            )
            .await;
        let received_at = (self.now)();
        let result = self.use_reply(&mut run.account, reply).await?;
        let failure = result.as_ref().err().copied();
        let mut propagating_until = None;
        let mut already_code = None;
        let outcome = match result {
            Ok(result) if upstream_day(received_at) == run.day => {
                run.progress.observe_grant(step);
                run.progress.uncertain_until = None;
                if step == GrantStep::Upgrade {
                    propagating_until = Some(received_at + PROPAGATION_WINDOW);
                }
                match result {
                    GrantResult::Confirmed { added_hours } => {
                        if step == GrantStep::Upgrade {
                            run.progress.estimated_end_at = added_hours
                                .and_then(|hours| evidence.estimated_end(received_at, hours));
                        }
                        RenewalOutcome::Succeeded
                    }
                    GrantResult::AlreadyDone { code } => {
                        already_code = Some(code);
                        RenewalOutcome::AlreadyDone
                    }
                }
            }
            Ok(_) => RenewalOutcome::Uncertain,
            Err(failure) => {
                self.apply_backoff(run, failure, received_at).await?;
                if failure.write_may_have_applied() {
                    RenewalOutcome::Uncertain
                } else if matches!(
                    failure.kind,
                    FailureKind::RateLimited | FailureKind::Verification
                ) {
                    restore_unsent(&mut run.progress, step);
                    RenewalOutcome::Deferred
                } else {
                    run.progress.step(step).state = StepState::Ready;
                    run.progress.uncertain_until = None;
                    run.progress.retry_at = Some(received_at + REJECTED_RETRY_DELAY);
                    RenewalOutcome::Failed
                }
            }
        };
        if run.progress.uncertain().is_some() {
            let wait_until = sent_at + timeout as i64 + PROPAGATION_WINDOW;
            run.progress.uncertain_until = Some(wait_until.max(received_at + PROPAGATION_WINDOW));
        }
        run.progress.last_outcome = Some(outcome);
        self.save_run(run, propagating_until).await?;
        if failure.is_none()
            && matches!(
                outcome,
                RenewalOutcome::Succeeded | RenewalOutcome::AlreadyDone
            )
            && run.settings.rate_limit_streak != 0
        {
            self.renewals
                .save_rate_limit(run.account_id(), None, 0)
                .await?;
            run.settings.rate_limit_streak = 0;
            run.settings.rate_limit_until = None;
        }
        let mut observation = action_observation(reason, run.day, outcome, failure);
        observation.upstream_code = observation.upstream_code.or(already_code);
        observation.retry_at = run
            .settings
            .rate_limit_until
            .filter(|until| *until > received_at)
            .or(run.progress.retry_at);
        observation.propagating_until = propagating_until;
        observation.estimated_end_at = run.progress.estimated_end_at;
        self.log_action(
            &run.context,
            match step {
                GrantStep::Claim => EntitlementAction::Claim,
                GrantStep::Upgrade => EntitlementAction::Upgrade,
            },
            started,
            observation,
            failure
                .map(entitlement_error)
                .or((outcome == RenewalOutcome::Uncertain)
                    .then_some(ErrorCode::UpstreamRequestFailed)),
        )
        .await;
        Ok(run.progress.uncertain().is_none() && failure.is_none())
    }

    async fn apply_backoff(
        &self,
        run: &mut RenewalRun,
        failure: UpstreamFailure,
        now: i64,
    ) -> Result<(), AppError> {
        match failure.kind {
            FailureKind::RateLimited => {
                let streak = run.settings.rate_limit_streak.saturating_add(1);
                let delay = (60_i64 << streak.saturating_sub(1).min(6)).min(3600);
                self.renewals
                    .save_rate_limit(run.account_id(), Some(now + delay), streak)
                    .await?;
                run.settings.rate_limit_until = Some(now + delay);
                run.settings.rate_limit_streak = streak;
            }
            FailureKind::Verification | FailureKind::Gateway => {
                let pause = if failure.kind == FailureKind::Verification {
                    DayPause::Verification
                } else {
                    DayPause::Gateway
                };
                let day = upstream_day(now);
                if day == run.day {
                    run.progress.paused = Some(pause);
                } else {
                    let mut progress = self.renewals.daily(run.account_id(), day).await?;
                    progress.paused = Some(pause);
                    self.renewals
                        .save_daily(run.account_id(), day, &progress, None)
                        .await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn save_run(
        &self,
        run: &RenewalRun,
        propagating_until: Option<i64>,
    ) -> Result<(), AppError> {
        self.renewals
            .save_daily(run.account_id(), run.day, &run.progress, propagating_until)
            .await
    }
}

fn observe_record(progress: &mut DailyProgress, entry: &RecordEntry) -> bool {
    match entry {
        RecordEntry::Found(step) => progress.observe_grant(*step),
        RecordEntry::Absent | RecordEntry::Uncovered => false,
    }
}

fn restore_unsent(progress: &mut DailyProgress, step: GrantStep) {
    let step = progress.step(step);
    step.attempts -= 1;
    step.state = StepState::Ready;
    progress.uncertain_until = None;
}

fn entitlement_error(failure: UpstreamFailure) -> ErrorCode {
    if failure.kind == FailureKind::RateLimited {
        ErrorCode::UpstreamRateLimited
    } else {
        ErrorCode::UpstreamRequestFailed
    }
}

#[cfg(test)]
mod tests;
