use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering::SeqCst};

use async_trait::async_trait;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::Notify;

use super::*;
use crate::domain::account::AccountStatus;
use crate::domain::cookie_store::CookieStore;
use crate::domain::entitlement::{EntitlementObservation, VipEntitlement};
use crate::domain::event_log::{ListGlobalEvents, LogView};
use crate::domain::renewal::upstream_svip_timestamp;
use crate::repos::account::AccountRepo;
use crate::repos::entitlement::EntitlementRepo;
use crate::repos::event_log::EventLogRepo;
use crate::repos::renewal::RenewalRepo;
use crate::repos::sqlite::Sqlite;
use crate::services::kugou_lite_client::entitlement::{EntitlementUpstream, UpstreamReply};

const ACCOUNT: &str = "acc_test";

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Snapshot,
    Record,
    Grant(GrantStep, NaiveDate, u64, String),
}

#[derive(Default)]
struct Pause {
    sent: Notify,
    release: Notify,
}

struct Grant {
    result: Result<GrantResult, UpstreamFailure>,
    returns_cookie: Option<&'static str>,
    advances_time: i64,
    pause: Option<Arc<Pause>>,
}

impl Grant {
    fn confirmed() -> Self {
        Self::result(Ok(GrantResult::Confirmed {
            added_hours: Some(24),
        }))
    }

    fn result(result: Result<GrantResult, UpstreamFailure>) -> Self {
        Self {
            result,
            returns_cookie: None,
            advances_time: 0,
            pause: None,
        }
    }
}

fn failure(kind: FailureKind, code: Option<i64>) -> UpstreamFailure {
    UpstreamFailure {
        kind,
        code,
        http_status: None,
        message: "synthetic response",
    }
}

struct Upstream {
    calls: Mutex<Vec<Call>>,
    snapshots: Mutex<VecDeque<Result<SvipEvidence, UpstreamFailure>>>,
    records: Mutex<VecDeque<Result<DailyRecord, UpstreamFailure>>>,
    writes: Mutex<VecDeque<Grant>>,
    now: Arc<AtomicI64>,
}

#[async_trait]
impl EntitlementUpstream for Upstream {
    async fn snapshot(
        &self,
        cookies: &CookieStore,
        _now: i64,
    ) -> UpstreamReply<EntitlementObservation> {
        self.calls.lock().unwrap().push(Call::Snapshot);
        let svip = self
            .snapshots
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected snapshot");
        UpstreamReply {
            result: svip.map(presentation),
            cookies: cookies.clone(),
        }
    }

    async fn grant(
        &self,
        cookies: &CookieStore,
        step: GrantStep,
        day: NaiveDate,
        timeout: Duration,
    ) -> UpstreamReply<GrantResult> {
        self.calls.lock().unwrap().push(Call::Grant(
            step,
            day,
            timeout.as_secs(),
            cookies.get("token").unwrap().into(),
        ));
        let result = self
            .writes
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected entitlement write");
        if let Some(pause) = result.pause {
            pause.sent.notify_one();
            pause.release.notified().await;
        }
        self.now.fetch_add(result.advances_time, SeqCst);
        let mut cookies = cookies.clone();
        if let Some(token) = result.returns_cookie {
            cookies.insert("token", token);
            cookies.insert("reply", "fixture");
        }
        UpstreamReply {
            result: result.result,
            cookies,
        }
    }

    async fn daily_record(&self, cookies: &CookieStore) -> UpstreamReply<DailyRecord> {
        self.calls.lock().unwrap().push(Call::Record);
        UpstreamReply {
            result: self
                .records
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected record lookup"),
            cookies: cookies.clone(),
        }
    }
}

fn presentation(svip: SvipEvidence) -> EntitlementObservation {
    let items = vec![VipEntitlement {
        product_type: "svip".into(),
        active: matches!(svip, SvipEvidence::ActiveUntil { .. }),
        begin_at: None,
        end_at: match svip {
            SvipEvidence::ActiveUntil { end_at } => Some(end_at),
            _ => None,
        },
    }];
    EntitlementObservation { items, svip }
}

struct Fixture {
    db: Arc<Sqlite>,
    service: Arc<EntitlementService>,
    upstream: Arc<Upstream>,
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let accounts = dir.path().join("accounts");
        std::fs::create_dir(&accounts).unwrap();
        std::fs::write(
            accounts.join(format!("{ACCOUNT}.json")),
            json!({
                "account_id": ACCOUNT, "provider": "kugou_lite", "upstream_userid": "fixture-user",
                "status": "active", "cookies": {"items": {"token": "original"}}, "created_at": 1, "updated_at": 1
            }).to_string(),
        ).unwrap();
        let upstream = Arc::new(Upstream {
            calls: Mutex::default(),
            snapshots: Mutex::default(),
            records: Mutex::default(),
            writes: Mutex::default(),
            now: Arc::new(AtomicI64::new(
                upstream_svip_timestamp("2035-02-01 12:00:00").unwrap(),
            )),
        });
        let (db, service) = Self::open(&dir, &upstream);
        Self {
            db,
            service,
            upstream,
            dir,
        }
    }

    fn open(dir: &TempDir, upstream: &Arc<Upstream>) -> (Arc<Sqlite>, Arc<EntitlementService>) {
        let db = Arc::new(Sqlite::open(dir.path(), 100).unwrap());
        let now = upstream.now.clone();
        let service = Arc::new(EntitlementService::new(
            upstream.clone(),
            db.clone(),
            db.clone(),
            db.clone(),
            db.clone(),
            Arc::default(),
            move || now.load(SeqCst),
        ));
        (db, service)
    }

    fn reopen(&mut self) {
        (self.db, self.service) = Self::open(&self.dir, &self.upstream);
    }

    fn now(&self) -> i64 {
        self.upstream.now.load(SeqCst)
    }

    fn advance(&self, seconds: i64) {
        self.upstream.now.fetch_add(seconds, SeqCst);
    }

    fn day(&self) -> NaiveDate {
        upstream_day(self.now())
    }

    fn writes(&self, results: impl IntoIterator<Item = Grant>) {
        self.upstream.writes.lock().unwrap().extend(results);
    }

    fn records(&self, entries: impl IntoIterator<Item = (NaiveDate, GrantStep)>) {
        self.upstream
            .records
            .lock()
            .unwrap()
            .push_back(Ok(DailyRecord {
                days: entries.into_iter().collect(),
                truncated: false,
            }));
    }

    fn snapshots(&self, evidence: impl IntoIterator<Item = SvipEvidence>) {
        self.upstream
            .snapshots
            .lock()
            .unwrap()
            .extend(evidence.into_iter().map(Ok));
    }

    fn take_calls(&self) -> Vec<Call> {
        std::mem::take(&mut *self.upstream.calls.lock().unwrap())
    }

    fn context(trigger: EventTrigger) -> EventContext {
        EventContext::new(trigger, Some(ACCOUNT.into()), None)
    }

    async fn cache(&self, svip: SvipEvidence) {
        self.db
            .record_sync(ACCOUNT, Some((presentation(svip), self.now())))
            .await
            .unwrap();
    }

    async fn manual(&self) -> Value {
        serde_json::to_value(self.service.renew_manually(ACCOUNT.into()).await.unwrap()).unwrap()
    }

    async fn auto(&self, signal: PlaybackSignal) {
        self.service
            .renew(ACCOUNT, Self::context(EventTrigger::Runtime), Some(signal))
            .await
            .unwrap();
    }

    async fn enable(&self) {
        self.service.set_auto_claim(ACCOUNT, true).await.unwrap();
    }

    async fn events(&self, view: LogView) -> Vec<serde_json::Value> {
        self.db
            .list_global(&ListGlobalEvents {
                source_id: None,
                account_id: Some(ACCOUNT.into()),
                correlation_id: None,
                before_seq: None,
                after_seq: None,
                limit: 100,
                view,
                event_type: Some(crate::domain::event_log::EventType::Entitlement),
            })
            .await
            .unwrap()
            .into_iter()
            .map(|event| serde_json::to_value(event).unwrap())
            .collect()
    }
}

fn write(step: GrantStep, day: NaiveDate, timeout: u64) -> Call {
    Call::Grant(step, day, timeout, "original".into())
}

#[tokio::test]
async fn a_confirmed_snapshot_leaves_room_to_reconcile_a_later_refusal() {
    let f = Fixture::new();
    f.enable().await;
    f.snapshots([SvipEvidence::ActiveUntil {
        end_at: f.now() + RENEWAL_THRESHOLD + 3600,
    }]);
    f.auto(PlaybackSignal::Link).await;
    assert_eq!(f.take_calls(), [Call::Snapshot]);

    f.snapshots([SvipEvidence::Inactive]);
    f.writes([Grant::confirmed(), Grant::confirmed()]);
    f.auto(PlaybackSignal::TargetRefused).await;
    assert_eq!(
        f.take_calls(),
        [
            Call::Snapshot,
            write(GrantStep::Claim, f.day(), WRITE_TIMEOUTS[0]),
            write(GrantStep::Upgrade, f.day(), WRITE_TIMEOUTS[0]),
        ]
    );
}

#[tokio::test]
async fn failed_post_upgrade_read_can_retry_after_the_backoff() {
    let f = Fixture::new();
    f.enable().await;
    f.writes([Grant::confirmed(), Grant::confirmed()]);
    assert_eq!(f.manual().await["state"], "propagating");
    f.take_calls();
    f.advance(PROPAGATION_WINDOW);
    f.upstream
        .snapshots
        .lock()
        .unwrap()
        .push_back(Err(failure(FailureKind::Unknown, None)));
    f.auto(PlaybackSignal::Link).await;
    assert_eq!(f.take_calls(), [Call::Snapshot]);
    assert!(
        !f.db
            .daily(ACCOUNT, f.day())
            .await
            .unwrap()
            .post_upgrade_read
    );

    f.advance(READ_RETRY_DELAY);
    f.snapshots([SvipEvidence::Inactive]);
    f.auto(PlaybackSignal::Link).await;
    assert_eq!(f.take_calls(), [Call::Snapshot]);
    assert!(
        f.db.daily(ACCOUNT, f.day())
            .await
            .unwrap()
            .post_upgrade_read
    );
}

#[tokio::test]
async fn reconciliation_respects_the_previous_day_wait_and_reports_exhaustion() {
    let f = Fixture::new();
    let yesterday = f.day().pred_opt().unwrap();
    let wait = f.now() + 90;
    let mut progress = DailyProgress {
        uncertain_until: Some(wait),
        ..Default::default()
    };
    progress.claim.state = StepState::Uncertain;
    f.db.save_daily(ACCOUNT, yesterday, &progress, None)
        .await
        .unwrap();
    f.records([]);
    let result = f.manual().await;
    assert_eq!(result["state"], "waiting_for_reconciliation");
    assert_eq!(result["retry_at"], wait);
    assert_eq!(f.take_calls(), [Call::Record]);

    f.advance(READ_RETRY_DELAY);
    f.manual().await;
    assert!(f.take_calls().is_empty());
    let progress = DailyProgress {
        record_reads: MAX_RECORD_READS,
        ..Default::default()
    };
    f.db.save_daily(ACCOUNT, f.day(), &progress, None)
        .await
        .unwrap();
    assert_eq!(f.manual().await["state"], "reconciliation_exhausted");
    assert!(f.take_calls().is_empty());
}

#[tokio::test]
async fn a_rejected_upgrade_resumes_without_claim_and_keeps_the_snapshot() {
    let f = Fixture::new();
    f.enable().await;
    let end_at = f.now() + 3600;
    f.cache(SvipEvidence::ActiveUntil { end_at }).await;
    f.writes([
        Grant::confirmed(),
        Grant::result(Err(failure(FailureKind::Rejected, Some(4321)))),
    ]);

    f.auto(PlaybackSignal::Link).await;
    let day = f.day();
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, day, 5),
            write(GrantStep::Upgrade, day, 5)
        ]
    );
    let state = f.manual().await;
    assert_eq!(state["state"], "awaiting_upgrade");
    assert_eq!(state["claim"], "done");
    assert_eq!(state["upgrade"], "ready");
    assert!(f.take_calls().is_empty());

    f.advance(REJECTED_RETRY_DELAY);
    f.writes([Grant::confirmed()]);
    let state = f.manual().await;
    assert_eq!(f.take_calls(), [write(GrantStep::Upgrade, day, 10)]);
    assert_eq!(state["state"], "propagating");
    assert_eq!(state["estimated_end_at"], end_at + 86_400);
    assert_eq!(state["propagating_until"], f.now() + PROPAGATION_WINDOW);
    let snapshot = f.db.entitlement_snapshot(ACCOUNT).await.unwrap();
    assert_eq!(snapshot.items.unwrap()[0].end_at, Some(end_at));

    f.auto(PlaybackSignal::TargetRefused).await;
    assert!(
        f.take_calls().is_empty(),
        "propagation bars both writes and contradiction reads"
    );
    f.advance(PROPAGATION_WINDOW);
    f.snapshots([SvipEvidence::ActiveUntil {
        end_at: end_at + 86_400,
    }]);
    f.auto(PlaybackSignal::TargetRefused).await;
    f.auto(PlaybackSignal::TargetRefused).await;
    assert_eq!(f.take_calls(), [Call::Snapshot]);
    assert_eq!(
        f.db.entitlement_snapshot(ACCOUNT)
            .await
            .unwrap()
            .items
            .unwrap()[0]
            .end_at,
        Some(end_at + 86_400)
    );
}

#[tokio::test]
async fn missing_records_wait_before_retrying_each_timeout_with_its_own_budget() {
    let f = Fixture::new();
    let timed_out = || Grant::result(Err(failure(FailureKind::Timeout, None)));
    f.writes([
        timed_out(),
        timed_out(),
        Grant::confirmed(),
        Grant::confirmed(),
    ]);

    let day = f.day();
    assert_eq!(f.manual().await["state"], "waiting_for_reconciliation");
    assert_eq!(f.take_calls(), [write(GrantStep::Claim, day, 5)]);

    f.records([]);
    assert_eq!(f.manual().await["state"], "waiting_for_reconciliation");
    assert_eq!(f.take_calls(), [Call::Record]);
    f.advance(PROPAGATION_WINDOW);
    f.manual().await;
    assert!(
        f.take_calls().is_empty(),
        "an early missing record is inconclusive"
    );

    f.advance(5);
    f.records([]);
    assert_eq!(f.manual().await["state"], "waiting_for_reconciliation");
    assert_eq!(
        f.take_calls(),
        [Call::Record, write(GrantStep::Claim, day, 10)]
    );

    f.advance(PROPAGATION_WINDOW + 10);
    f.records([]);
    assert_eq!(f.manual().await["state"], "propagating");
    assert_eq!(
        f.take_calls(),
        [
            Call::Record,
            write(GrantStep::Claim, day, 30),
            write(GrantStep::Upgrade, day, 5)
        ]
    );
}

#[tokio::test]
async fn process_interruption_preserves_uncertain_claim_and_uses_the_record_after_restart() {
    let mut f = Fixture::new();
    let pause = Arc::new(Pause::default());
    f.writes([
        Grant {
            pause: Some(pause.clone()),
            ..Grant::confirmed()
        },
        Grant::confirmed(),
    ]);
    let service = f.service.clone();
    let interrupted = tokio::spawn(async move {
        service
            .renew(ACCOUNT, Fixture::context(EventTrigger::Admin), None)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), pause.sent.notified())
        .await
        .unwrap();
    let day = f.day();
    let persisted = f.db.daily(ACCOUNT, day).await.unwrap();
    assert_eq!(persisted.uncertain(), Some(GrantStep::Claim));
    interrupted.abort();
    assert!(interrupted.await.unwrap_err().is_cancelled());

    f.reopen();
    f.records([(day, GrantStep::Claim)]);
    assert_eq!(f.manual().await["state"], "propagating");
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, day, 5),
            Call::Record,
            write(GrantStep::Upgrade, day, 5)
        ]
    );
}

#[tokio::test]
async fn record_errors_and_conflicts_preserve_unknown_upgrade_until_a_positive_observation() {
    let f = Fixture::new();
    f.writes([
        Grant::confirmed(),
        Grant::result(Err(failure(FailureKind::Timeout, None))),
    ]);
    assert_eq!(f.manual().await["upgrade"], "uncertain");
    f.take_calls();
    f.advance(200);
    f.upstream
        .records
        .lock()
        .unwrap()
        .push_back(Err(failure(FailureKind::Unknown, None)));
    assert_eq!(f.manual().await["upgrade"], "uncertain");

    f.advance(READ_RETRY_DELAY);
    f.records([]);
    assert_eq!(
        f.manual().await["upgrade"],
        "uncertain",
        "the record must agree with the known claim before any upgrade retry"
    );

    f.advance(READ_RETRY_DELAY);
    f.records([(f.day(), GrantStep::Upgrade)]);
    let state = f.manual().await;
    assert_eq!(state["state"], "propagating");
    assert!(state["estimated_end_at"].is_null());
    assert_eq!(f.take_calls(), [Call::Record, Call::Record, Call::Record]);
}

#[tokio::test]
async fn idempotent_codes_complete_only_their_own_step_and_are_successes_in_logs() {
    let f = Fixture::new();
    f.cache(SvipEvidence::ActiveUntil {
        end_at: f.now() + 3600,
    })
    .await;
    f.writes([
        Grant::result(Ok(GrantResult::AlreadyDone { code: 131001 })),
        Grant::result(Ok(GrantResult::AlreadyDone { code: 297002 })),
    ]);

    let day = f.day();
    let state = f.manual().await;
    assert_eq!(state["claim"], "done");
    assert_eq!(state["upgrade"], "done");
    assert!(state["estimated_end_at"].is_null());
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, day, 5),
            write(GrantStep::Upgrade, day, 5)
        ]
    );
    let events = f.events(LogView::All).await;
    for (event, (action, code)) in events.iter().zip([("upgrade", 297002), ("claim", 131001)]) {
        assert_eq!(event["ok"], true);
        assert_eq!(event["detail"]["action"], action);
        assert_eq!(event["detail"]["outcome"], "already_done");
        assert_eq!(event["detail"]["upstream_code"], code);
    }
    assert_eq!(events.len(), 2);
    assert!(f.events(LogView::Errors).await.is_empty());
}

#[tokio::test]
async fn verification_pauses_only_its_day_and_rate_limit_exponent_survives_midnight() {
    let mut f = Fixture::new();
    let day = f.day();
    let midnight = next_upstream_day_start(day);
    f.advance(midnight - f.now() - (MIDNIGHT_MARGIN + 5));
    f.writes([Grant::result(Err(failure(
        FailureKind::Verification,
        Some(20028),
    )))]);

    assert_eq!(f.manual().await["state"], "verification_paused");
    f.reopen();
    assert_eq!(f.manual().await["state"], "verification_paused");
    assert_eq!(f.take_calls(), [write(GrantStep::Claim, day, 5)]);
    let account = f.db.find_by_id(ACCOUNT).await.unwrap().unwrap();
    assert_eq!(account.status, AccountStatus::Active);
    assert_eq!(account.cookies.get("token"), Some("original"));

    f.advance(41);
    let day = f.day();
    let midnight = next_upstream_day_start(day);
    f.advance(midnight - f.now() - (MIDNIGHT_MARGIN + 5));
    f.writes([
        Grant::result(Err(failure(FailureKind::RateLimited, Some(429)))),
        Grant::result(Err(failure(FailureKind::RateLimited, Some(429)))),
        Grant::confirmed(),
        Grant::confirmed(),
    ]);

    let state = f.manual().await;
    assert_eq!(state["state"], "rate_limited");
    assert_eq!(state["rate_limit_until"], f.now() + 60);
    assert_eq!(f.take_calls(), [write(GrantStep::Claim, day, 5)]);
    f.advance(41);
    f.reopen();
    assert_eq!(f.manual().await["state"], "rate_limited");
    assert!(f.take_calls().is_empty());
    f.advance(19);
    assert_eq!(f.manual().await["rate_limit_until"], f.now() + 120);
    assert_eq!(f.take_calls(), [write(GrantStep::Claim, f.day(), 5)]);
    f.advance(119);
    assert_eq!(f.manual().await["state"], "rate_limited");
    assert!(f.take_calls().is_empty());
    f.advance(1);
    assert_eq!(f.manual().await["state"], "propagating");
    assert_eq!(f.db.settings(ACCOUNT).await.unwrap().rate_limit_streak, 0);
}

#[tokio::test]
async fn a_claim_close_to_midnight_never_sends_an_undated_upgrade() {
    let f = Fixture::new();
    let day = f.day();
    let midnight = next_upstream_day_start(day);
    f.advance(midnight - f.now() - MIDNIGHT_MARGIN - 1);
    f.writes([Grant {
        advances_time: 2,
        ..Grant::confirmed()
    }]);
    let state = f.manual().await;
    assert_eq!(state["state"], "awaiting_upgrade");
    assert_eq!(state["retry_at"], midnight);
    assert_eq!(f.take_calls(), [write(GrantStep::Claim, day, 5)]);
    f.advance(midnight - f.now());
    f.writes([Grant::confirmed(), Grant::confirmed()]);
    assert_eq!(f.manual().await["state"], "propagating");
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, f.day(), 5),
            write(GrantStep::Upgrade, f.day(), 5)
        ]
    );
    assert_eq!(
        f.db.daily(ACCOUNT, day).await.unwrap().upgrade.state,
        StepState::Ready
    );
}

#[tokio::test]
async fn a_response_crossing_midnight_is_reconciled_against_both_days() {
    let mut f = Fixture::new();
    let day = f.day();
    let midnight = next_upstream_day_start(day);
    f.advance(midnight - f.now() - 40);
    f.writes([
        Grant::confirmed(),
        Grant {
            advances_time: 41,
            ..Grant::confirmed()
        },
    ]);
    assert_eq!(f.manual().await["state"], "waiting_for_reconciliation");
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, day, 5),
            write(GrantStep::Upgrade, day, 5)
        ]
    );
    assert_eq!(
        f.db.daily(ACCOUNT, day).await.unwrap().uncertain(),
        Some(GrantStep::Upgrade)
    );

    f.reopen();
    f.records([(day, GrantStep::Claim), (f.day(), GrantStep::Upgrade)]);
    let state = f.manual().await;
    assert_eq!(state["upgrade"], "done");
    assert_eq!(
        state["state"], "propagating",
        "the detected new-day upgrade starts a propagation window"
    );
    assert_eq!(f.take_calls(), [Call::Record]);
    f.advance(PROPAGATION_WINDOW);
    f.records([(day, GrantStep::Claim), (f.day(), GrantStep::Upgrade)]);
    assert_eq!(f.manual().await["state"], "completed");
    assert!(f.db.daily(ACCOUNT, day).await.unwrap().recovery_closed);
    assert_eq!(f.take_calls(), [Call::Record]);
}

#[tokio::test]
async fn exact_threshold_and_free_playback_do_not_trigger_and_refusal_requires_expiry() {
    let f = Fixture::new();
    let end_at = f.now() + RENEWAL_THRESHOLD;
    f.cache(SvipEvidence::ActiveUntil { end_at }).await;
    f.auto(PlaybackSignal::Link).await;
    assert!(f.take_calls().is_empty(), "new settings default off");
    f.enable().await;
    f.auto(PlaybackSignal::Link).await;
    assert!(f.take_calls().is_empty(), "48 hours exactly is outside T1");
    f.snapshots([SvipEvidence::ActiveUntil { end_at }]);
    f.auto(PlaybackSignal::TargetRefused).await;
    f.auto(PlaybackSignal::TargetRefused).await;
    assert_eq!(
        f.take_calls(),
        [Call::Snapshot],
        "a contradiction is read once and cannot start a write while active"
    );

    f.cache(SvipEvidence::Inactive).await;
    f.auto(PlaybackSignal::Link).await;
    assert!(
        f.take_calls().is_empty(),
        "an expired account can play free music"
    );

    f.writes([Grant::confirmed(), Grant::confirmed()]);
    f.auto(PlaybackSignal::TargetRefused).await;
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, f.day(), 5),
            write(GrantStep::Upgrade, f.day(), 5)
        ]
    );
    let events = f.events(LogView::All).await;
    assert_eq!(events[0]["detail"]["reason"], "t2_expired_refusal");
}

#[tokio::test]
async fn concurrent_playback_and_manual_share_a_run_and_late_reply_cannot_overwrite_cookie_version()
{
    let f = Fixture::new();
    f.enable().await;
    f.cache(SvipEvidence::ActiveUntil {
        end_at: f.now() + 3600,
    })
    .await;
    let pause = Arc::new(Pause::default());
    f.writes([
        Grant {
            pause: Some(pause.clone()),
            returns_cookie: Some("stale"),
            ..Grant::confirmed()
        },
        Grant::confirmed(),
    ]);
    let context = Fixture::context(EventTrigger::Runtime);
    let correlation_id = context.correlation_id.clone();
    f.service.on_playback(context, PlaybackSignal::Link);
    tokio::time::timeout(Duration::from_secs(2), pause.sent.notified())
        .await
        .unwrap();
    for _ in 0..12 {
        f.service.on_playback(
            Fixture::context(EventTrigger::Runtime),
            PlaybackSignal::Link,
        );
    }
    let service = f.service.clone();
    let manual = tokio::spawn(async move { service.renew_manually(ACCOUNT.into()).await.unwrap() });

    let account = f.db.find_by_id(ACCOUNT).await.unwrap().unwrap();
    let mut fresh = account.clone();
    fresh.credential_version += 1;
    fresh.cookies.insert("token", "new-version");
    f.db.save_auth(&account, &fresh).await.unwrap();
    pause.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), manual)
        .await
        .unwrap()
        .unwrap();

    let current = f.db.find_by_id(ACCOUNT).await.unwrap().unwrap();
    assert_eq!(current.cookies.get("token"), Some("new-version"));
    assert_eq!(current.cookies.get("reply"), None);
    assert_eq!(current.status, AccountStatus::Active);
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, f.day(), 5),
            Call::Grant(GrantStep::Upgrade, f.day(), 5, "new-version".into()),
        ]
    );
    let events = f.events(LogView::All).await;
    assert_eq!(events.len(), 2);
    for event in events {
        assert_eq!(event["correlation_id"], correlation_id);
        assert_eq!(event["detail"]["reason"], "t1_low_battery");
    }
}

#[tokio::test]
async fn simultaneous_link_and_refusal_keep_the_refusal_evidence_for_t2() {
    let f = Fixture::new();
    f.enable().await;
    f.cache(SvipEvidence::Inactive).await;
    let pause = Arc::new(Pause::default());
    f.writes([
        Grant {
            pause: Some(pause.clone()),
            ..Grant::confirmed()
        },
        Grant::confirmed(),
    ]);
    let held = f.service.admin_actions.lock(ACCOUNT).await;
    f.service.on_playback(
        Fixture::context(EventTrigger::Runtime),
        PlaybackSignal::Link,
    );
    let refused = Fixture::context(EventTrigger::Runtime);
    let correlation_id = refused.correlation_id.clone();
    f.service
        .on_playback(refused, PlaybackSignal::TargetRefused);
    drop(held);

    tokio::time::timeout(Duration::from_secs(2), pause.sent.notified())
        .await
        .unwrap();
    pause.release.notify_one();
    assert_eq!(f.manual().await["state"], "propagating");
    assert_eq!(
        f.take_calls(),
        [
            write(GrantStep::Claim, f.day(), 5),
            write(GrantStep::Upgrade, f.day(), 5)
        ]
    );
    let events = f.events(LogView::All).await;
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|event| event["correlation_id"] == correlation_id)
    );
}

#[tokio::test]
async fn a_bare_gateway_failure_pauses_writes_and_preserves_uncertainty_across_days() {
    let f = Fixture::new();
    let day = f.day();
    f.writes([Grant::result(Err(UpstreamFailure {
        http_status: Some(502),
        ..failure(FailureKind::Gateway, None)
    }))]);
    let state = f.manual().await;
    assert_eq!(state["state"], "upstream_unavailable");
    assert_eq!(state["paused_until"], next_upstream_day_start(day));
    assert_eq!(state["claim"], "uncertain");
    f.manual().await;
    assert_eq!(f.take_calls(), [write(GrantStep::Claim, day, 5)]);

    f.advance(next_upstream_day_start(day) - f.now());
    f.records([(day, GrantStep::Claim)]);
    f.writes([Grant::confirmed(), Grant::confirmed()]);
    assert_eq!(f.manual().await["state"], "propagating");
    assert_eq!(
        f.take_calls(),
        [
            Call::Record,
            write(GrantStep::Claim, f.day(), 5),
            write(GrantStep::Upgrade, f.day(), 5)
        ]
    );
    assert_eq!(
        f.db.daily(ACCOUNT, day).await.unwrap().claim.state,
        StepState::Done
    );
}

#[tokio::test]
async fn a_rate_limited_snapshot_can_be_retried_within_its_read_budget() {
    let f = Fixture::new();
    f.enable().await;
    f.upstream
        .snapshots
        .lock()
        .unwrap()
        .push_back(Err(failure(FailureKind::RateLimited, Some(429))));
    f.snapshots([SvipEvidence::Inactive]);
    f.writes([Grant::confirmed(), Grant::confirmed()]);

    f.auto(PlaybackSignal::TargetRefused).await;
    assert_eq!(f.take_calls(), [Call::Snapshot]);
    f.advance(59);
    f.auto(PlaybackSignal::TargetRefused).await;
    assert!(f.take_calls().is_empty());
    f.advance(1);
    f.auto(PlaybackSignal::TargetRefused).await;
    assert_eq!(
        f.take_calls(),
        [
            Call::Snapshot,
            write(GrantStep::Claim, f.day(), 5),
            write(GrantStep::Upgrade, f.day(), 5)
        ]
    );
}
