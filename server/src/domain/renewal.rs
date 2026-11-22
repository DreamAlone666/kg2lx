use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};

pub const RENEWAL_THRESHOLD: i64 = 48 * 60 * 60;
pub const PROPAGATION_WINDOW: i64 = 120;
pub const SNAPSHOT_MAX_AGE: i64 = 24 * 60 * 60;
pub const WRITE_TIMEOUTS: [u64; 3] = [1, 5, 30];

// Leave room for the longest write to return before the undated upgrade endpoint rolls over.
pub const MIDNIGHT_MARGIN: i64 = 35;

// This is an explicit assumption about KuGou, independent of the deployment's timezone.
fn upstream_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

pub fn upstream_day(timestamp: i64) -> NaiveDate {
    DateTime::from_timestamp(timestamp, 0)
        .expect("wall clock is representable")
        .with_timezone(&upstream_offset())
        .date_naive()
}

pub fn next_upstream_day_start(day: NaiveDate) -> i64 {
    day.succ_opt()
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(upstream_offset())
        .single()
        .unwrap()
        .timestamp()
}

pub fn upstream_svip_timestamp(text: &str) -> Option<i64> {
    if let Ok(date) = DateTime::parse_from_rfc3339(text) {
        return Some(date.timestamp());
    }
    let local = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok()?;
    Some(
        upstream_offset()
            .from_local_datetime(&local)
            .single()?
            .timestamp(),
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SvipEvidence {
    #[default]
    Unknown,
    Inactive,
    ActiveUntil {
        end_at: i64,
    },
}

impl SvipEvidence {
    pub fn remaining(self, now: i64) -> Option<i64> {
        match self {
            Self::Unknown => None,
            Self::Inactive => Some(0),
            Self::ActiveUntil { end_at } => Some(end_at.saturating_sub(now).max(0)),
        }
    }

    pub fn estimated_end(self, now: i64, added_hours: u32) -> Option<i64> {
        self.remaining(now)?
            .checked_add(i64::from(added_hours) * 3600)?
            .checked_add(now)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenewalSettings {
    pub enabled: bool,
    pub rate_limit_until: Option<i64>,
    pub rate_limit_streak: u8,
    pub propagating_until: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantStep {
    Claim,
    Upgrade,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    #[default]
    Ready,
    Uncertain,
    Done,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct StepProgress {
    pub state: StepState,
    pub attempts: usize,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayPause {
    Verification,
    Gateway,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DailyProgress {
    pub claim: StepProgress,
    pub upgrade: StepProgress,
    pub record_reads: u8,
    pub snapshot_reads: u8,
    pub post_upgrade_read: bool,
    pub paused: Option<DayPause>,
    pub recovery_closed: bool,
    pub retry_at: Option<i64>,
    pub uncertain_until: Option<i64>,
    pub estimated_end_at: Option<i64>,
    pub last_outcome: Option<RenewalOutcome>,
}

impl DailyProgress {
    pub fn next_step(&self) -> Option<GrantStep> {
        if self.claim.state != StepState::Done {
            Some(GrantStep::Claim)
        } else if self.upgrade.state != StepState::Done {
            Some(GrantStep::Upgrade)
        } else {
            None
        }
    }

    pub fn step(&mut self, step: GrantStep) -> &mut StepProgress {
        match step {
            GrantStep::Claim => &mut self.claim,
            GrantStep::Upgrade => &mut self.upgrade,
        }
    }

    pub fn uncertain(&self) -> Option<GrantStep> {
        self.next_step().filter(|step| match step {
            GrantStep::Claim => self.claim.state == StepState::Uncertain,
            GrantStep::Upgrade => self.upgrade.state == StepState::Uncertain,
        })
    }

    pub fn observe_grant(&mut self, step: GrantStep) -> bool {
        self.claim.state = StepState::Done;
        if step == GrantStep::Upgrade && self.upgrade.state != StepState::Done {
            self.upgrade.state = StepState::Done;
            return true;
        }
        false
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenewalOutcome {
    Succeeded,
    AlreadyDone,
    Failed,
    Uncertain,
    Deferred,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenewalReason {
    Manual,
    T1LowBattery,
    T2ExpiredRefusal,
    ConfirmSnapshot,
    ReconcileMismatch,
    RecoverWrite,
    PostRefresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RenewalState {
    Ready,
    WaitingForReconciliation,
    VerificationPaused,
    UpstreamUnavailable,
    RateLimited,
    Propagating,
    AwaitingUpgrade,
    Completed,
    AttemptsExhausted,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenewalStatus {
    pub enabled: bool,
    pub upstream_day: NaiveDate,
    state: RenewalState,
    claim: StepState,
    upgrade: StepState,
    svip_confirmation_required: bool,
    paused_until: Option<i64>,
    retry_at: Option<i64>,
    rate_limit_until: Option<i64>,
    propagating_until: Option<i64>,
    estimated_end_at: Option<i64>,
    last_outcome: Option<RenewalOutcome>,
}

impl RenewalStatus {
    pub fn new(
        day: NaiveDate,
        now: i64,
        settings: RenewalSettings,
        daily: &DailyProgress,
        older_pending: bool,
        evidence: SvipEvidence,
    ) -> Self {
        let propagating_until = settings.propagating_until.filter(|until| *until > now);
        let rate_limit_until = settings.rate_limit_until.filter(|until| *until > now);
        let next = daily.next_step();
        let exhausted = match next {
            Some(GrantStep::Claim) => daily.claim.attempts >= WRITE_TIMEOUTS.len(),
            Some(GrantStep::Upgrade) => daily.upgrade.attempts >= WRITE_TIMEOUTS.len(),
            None => false,
        };
        let state = if let Some(pause) = daily.paused {
            match pause {
                DayPause::Verification => RenewalState::VerificationPaused,
                DayPause::Gateway => RenewalState::UpstreamUnavailable,
            }
        } else if rate_limit_until.is_some() {
            RenewalState::RateLimited
        } else if propagating_until.is_some() {
            RenewalState::Propagating
        } else if older_pending || daily.uncertain().is_some() {
            RenewalState::WaitingForReconciliation
        } else if next.is_none() {
            RenewalState::Completed
        } else if exhausted {
            RenewalState::AttemptsExhausted
        } else if daily.claim.state == StepState::Done {
            RenewalState::AwaitingUpgrade
        } else {
            RenewalState::Ready
        };
        Self {
            enabled: settings.enabled,
            upstream_day: day,
            state,
            claim: daily.claim.state,
            upgrade: daily.upgrade.state,
            svip_confirmation_required: evidence == SvipEvidence::Unknown,
            paused_until: daily.paused.map(|_| next_upstream_day_start(day)),
            retry_at: daily.retry_at.filter(|until| *until > now),
            rate_limit_until,
            propagating_until,
            estimated_end_at: daily.estimated_end_at,
            last_outcome: daily.last_outcome,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackSignal {
    Link,
    TargetRefused,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_day_and_svip_parser_are_independent_of_the_host_timezone() {
        let day = NaiveDate::from_ymd_opt(2035, 2, 1).unwrap();
        let midnight = upstream_svip_timestamp("2035-02-01 00:00:00").unwrap();
        assert_eq!(upstream_day(midnight), day);
        assert_eq!(upstream_day(midnight - 1), day.pred_opt().unwrap());
        assert_eq!(
            Some(midnight),
            upstream_svip_timestamp("2035-01-31T16:00:00Z")
        );
        assert_eq!(upstream_svip_timestamp("invalid"), None);
    }
}
