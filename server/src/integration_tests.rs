use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::Notify;

use crate::app_state::AppState;
use crate::config::Config;
use crate::domain::account::now_ts;
use crate::domain::cookie_store::CookieStore;
use crate::domain::renewal::GrantStep;
use crate::domain::renewal::{upstream_day, upstream_svip_timestamp};
use crate::repos::account::AccountRepo;
use crate::repos::sqlite::Sqlite;
use crate::services::kugou_lite_client::KugouLiteClient;
use crate::services::kugou_lite_client::entitlement::{
    EntitlementUpstream, GrantResult, RecordEntry,
};

const ADMIN_TOKEN: &str = "test-admin-token";
const ACCOUNT: &str = "acc_fixture";
const SOURCE: &str = "src_fixture";
const AUTH_ROUTE: &str = "/api/v1/admin/accounts/acc_fixture/refresh-token";
const SYNC_ROUTE: &str = "/api/v1/admin/accounts/acc_fixture/entitlements/sync";
const DETAIL_ROUTE: &str = "/api/v1/admin/sources/src_fixture";
const LOG_ROUTE: &str = "/api/v1/admin/sources/src_fixture/logs";
const RENEWAL_ROUTE: &str = "/api/v1/admin/accounts/acc_fixture/entitlements/renewal";
const CLAIM_ROUTE: &str = "/api/v1/admin/accounts/acc_fixture/entitlements/renewal/claim";

fn entitlement_time() -> i64 {
    upstream_svip_timestamp("2035-02-01 12:00:00").unwrap()
}

struct HttpServer {
    base: String,
    task: tokio::task::JoinHandle<()>,
}

impl HttpServer {
    async fn start(router: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { base, task }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
struct Pause {
    requests: AtomicUsize,
    pause_at: AtomicUsize,
    entered: Notify,
    release: Notify,
}

impl Pause {
    async fn on_request(&self) -> usize {
        let number = self.requests.fetch_add(1, SeqCst) + 1;
        if self.pause_at.load(SeqCst) == number {
            self.entered.notify_one();
            self.release.notified().await;
        }
        number
    }

    fn pause_next(&self) {
        self.pause_at.store(self.requests.load(SeqCst) + 1, SeqCst);
    }

    async fn wait_until_entered(&self) {
        tokio::time::timeout(Duration::from_secs(3), self.entered.notified())
            .await
            .unwrap();
    }
}

#[derive(Default)]
struct Upstream {
    login: Pause,
    song: Pause,
    vip: Pause,
    claim: Pause,
    upgrade: Pause,
    login_mode: AtomicUsize,
    vip_mode: AtomicUsize,
    song_mode: AtomicUsize,
}

impl Upstream {
    async fn serve(State(this): State<Arc<Self>>, request: Request) -> Response {
        match request.uri().path() {
            "/login/qr/key" => Json(json!({"data":{"key":"qr-test"}})).into_response(),
            "/login/qr/create" => {
                Json(json!({"data":{"url":"https://example.test/qr"}})).into_response()
            }
            "/login/qr/check" => {
                Json(json!({"data":{"status":4,"userid":42,"token":"qr-session"}})).into_response()
            }
            "/register/dev" => {
                Json(json!({"status":1,"cookie":"dfid=fixture-device"})).into_response()
            }
            "/login/token" => {
                let number = this.login.on_request().await;
                match this.login_mode.load(SeqCst) {
                    1 => (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"status":0,"code":301})),
                    )
                        .into_response(),
                    2 => (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"status":0,"errcode":20028,"error":"本次请求需要验证"})),
                    )
                        .into_response(),
                    _ => Json(json!({"status":1,"cookie":format!("token=auth-{number}")}))
                        .into_response(),
                }
            }
            "/youth/union/vip" => {
                this.vip.on_request().await;
                match this.vip_mode.load(SeqCst) {
                    1 => (StatusCode::BAD_GATEWAY, Json(json!({"status":0}))).into_response(),
                    2 => Json(json!({"status":0,"data":{"busi_vip":[]}})).into_response(),
                    3 => Json(
                        json!({"status":1,"data":{"busi_vip":[{"product_type":"svip","is_vip":1}]},
                        "cookie":"token=late-vip; vip_extra=seen"}),
                    )
                    .into_response(),
                    _ => Json(json!({"status":1,"data":{"busi_vip":[]}})).into_response(),
                }
            }
            "/song/url" => {
                let number = this.song.on_request().await;
                match this.song_mode.load(SeqCst) {
                    1 => (StatusCode::BAD_GATEWAY, Json(json!({"status":0,"errcode":20028,"error":"本次请求需要验证","priv_status":0}))).into_response(),
                    2 if number == 1 => (StatusCode::UNAUTHORIZED, Json(json!({"status":0,"code":301}))).into_response(),
                    3 => Json(json!({"status":1,"url":"https://example.test/song.flac", "cookie":"token=late-song",
                        "priv_status":0,"fail_process":[],"extName":"flac","bitRate":0,"fileSize":80000,"timeLength":8})).into_response(),
                    4 if request.uri().query().is_some_and(|query| query.contains("quality=flac")) =>
                        Json(json!({"status":2,"data":{},"priv_status":0})).into_response(),
                    4 => Json(json!({"status":1,"url":"https://example.test/fallback.mp3"})).into_response(),
                    _ => Json(json!({"status":1,"url":"https://example.test/song.flac",
                        "priv_status":0,"fail_process":[],"extName":"flac","bitRate":0,"fileSize":80000,"timeLength":8})).into_response(),
                }
            }
            "/youth/day/vip" => {
                assert_eq!(request.method(), Method::POST);
                assert_eq!(
                    request.uri().query().unwrap(),
                    format!("receive_day={}", upstream_day(entitlement_time()))
                );
                this.claim.on_request().await;
                Json(json!({"status":0,"error_code":131001,"cookie":"fixture_grant=merged"}))
                    .into_response()
            }
            "/youth/day/vip/upgrade" => {
                assert_eq!(request.method(), Method::POST);
                let cookie = request.headers().get("cookie").unwrap().to_str().unwrap();
                assert!(cookie.contains("fixture_grant=merged"));
                this.upgrade.on_request().await;
                Json(json!({"status":0,"error_code":297002})).into_response()
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

struct Fixture {
    server: HttpServer,
    upstream: Arc<Upstream>,
    state: AppState,
    http: Client,
    _upstream_server: HttpServer,
    _data: TempDir,
}

impl Fixture {
    async fn start(legacy_time: Option<i64>) -> Self {
        let upstream = Arc::new(Upstream::default());
        let upstream_server = HttpServer::start(
            Router::new()
                .fallback(Upstream::serve)
                .with_state(upstream.clone()),
        )
        .await;
        let data = tempfile::tempdir().unwrap();
        if let Some(refreshed_at) = legacy_time {
            let write = |relative: &str, json: Value| {
                let path = data.path().join(relative);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, json.to_string()).unwrap();
            };
            write(
                "accounts/acc_fixture.json",
                json!({
                    "account_id":ACCOUNT,"provider":"kugou_lite","upstream_userid":"42","status":"active",
                    "last_refresh_at":refreshed_at,"created_at":1,"updated_at":1,
                    "cookies":{"items":{"userid":"42","dfid":"fixture-device","token":"legacy-token"}}
                }),
            );
            for (id, script, token) in [
                (SOURCE, "st_fixture", "rt_fixture"),
                ("src_shared", "st_shared", "rt_shared"),
            ] {
                write(
                    &format!("sources/{id}.json"),
                    json!({
                        "account_id":ACCOUNT,"source_id":id,"provider":"kugou_lite","name":"Fixture source",
                        "script_token":script,"runtime_token":token,"created_at":1,"updated_at":1,
                    }),
                );
            }
        }
        let state = AppState::with_entitlement_clock(
            Config {
                listen_addr: "127.0.0.1:0".into(),
                public_base_url: "http://example.test".into(),
                admin_token: ADMIN_TOKEN.into(),
                kugou_api_base_url: upstream_server.base.clone(),
                data_dir: data.path().to_owned(),
                upstream_timeout_ms: 4_000,
                account_refresh_ttl: 100,
                event_log_limit: crate::config::DEFAULT_EVENT_LOG_LIMIT,
                web_dist_dir: None,
            },
            entitlement_time,
        )
        .unwrap();
        let server = HttpServer::start(crate::build_app(state.clone())).await;
        let http = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        Self {
            server,
            upstream,
            state,
            http,
            _upstream_server: upstream_server,
            _data: data,
        }
    }

    async fn admin(&self, method: Method, path: &str) -> (StatusCode, Value) {
        let response = self
            .http
            .request(method, format!("{}{path}", self.server.base))
            .bearer_auth(ADMIN_TOKEN)
            .send()
            .await
            .unwrap();
        let status = response.status();
        (status, response.json().await.unwrap())
    }

    async fn song(&self, token: &str, shape: Value) -> (StatusCode, Value) {
        let response = self
            .http
            .post(format!("{}/api/v1/runtime/music-url", self.server.base))
            .bearer_auth(token)
            .json(&shape)
            .send()
            .await
            .unwrap();
        let status = response.status();
        (status, response.json().await.unwrap())
    }

    async fn bind(&self) -> Value {
        let root = "/api/v1/admin/providers/kugou-lite/login/qr";
        let (status, started) = self.admin(Method::POST, root).await;
        assert_eq!(status, StatusCode::OK);
        let session = started["session_id"].as_str().unwrap();
        let (status, bound) = self.admin(Method::GET, &format!("{root}/{session}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bound["status"], "bound");
        bound
    }

    async fn token(&self) -> String {
        let db = Sqlite::open(
            &self.state.config.data_dir,
            self.state.config.event_log_limit,
        )
        .unwrap();
        db.find_by_id(ACCOUNT)
            .await
            .unwrap()
            .unwrap()
            .cookies
            .get("token")
            .unwrap()
            .to_owned()
    }
}

fn old_three_fields() -> Value {
    json!({"hash":"TEST-HASH","album_audio_id":null,"quality":"flac"})
}

#[tokio::test]
async fn binds_and_refreshes_during_vip_outage_and_exposes_four_event_categories() {
    let app = Fixture::start(None).await;
    app.upstream.vip_mode.store(1, SeqCst);
    let bound = app.bind().await;
    let source_id = bound["source"]["source_id"].as_str().unwrap();
    let account_id = bound["account"]["account_id"].as_str().unwrap();
    let db = Sqlite::open(&app.state.config.data_dir, app.state.config.event_log_limit).unwrap();
    use crate::repos::source::SourceRepo;
    let source = SourceRepo::find_by_id(&db, source_id)
        .await
        .unwrap()
        .unwrap();

    let account = format!("/api/v1/admin/accounts/{account_id}");
    let detail = format!("/api/v1/admin/sources/{source_id}");
    assert_eq!(app.upstream.vip.requests.load(SeqCst), 0);
    let (status, _) = app
        .admin(Method::POST, &format!("{account}/entitlements/sync"))
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let (_, response) = app.admin(Method::GET, &detail).await;
    assert_eq!(response["account"]["status"], "active");
    assert_eq!(response["account"]["renewal"]["enabled"], false);
    assert_eq!(response["account"]["entitlements_sync_failed"], true);
    assert!(response["account"].get("entitlements").is_none());

    assert_eq!(
        app.admin(Method::POST, &format!("{account}/refresh-token"))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        app.song(&source.runtime_token, old_three_fields()).await.0,
        StatusCode::OK
    );
    assert_eq!(app.upstream.vip.requests.load(SeqCst), 1);
    assert_eq!(app.upstream.login.requests.load(SeqCst), 2);
    app.upstream.vip_mode.store(0, SeqCst);
    assert_eq!(
        app.admin(Method::POST, &format!("{account}/entitlements/sync"))
            .await
            .0,
        StatusCode::OK
    );
    let (_, response) = app.admin(Method::GET, &detail).await;
    assert_eq!(response["account"]["entitlements"], json!([]));
    assert_eq!(response["account"]["entitlements_sync_failed"], false);
    assert!(response["account"]["entitlements_synced_at"].is_number());

    let (_, logs) = app.admin(Method::GET, &format!("{detail}/logs")).await;
    let rows = logs["items"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let kinds: Vec<_> = rows
        .iter()
        .map(|e| e["event_type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "entitlement",
            "music_url",
            "token_refresh",
            "entitlement",
            "account_login",
            "token_refresh"
        ]
    );
    assert_eq!(rows[4]["correlation_id"], rows[5]["correlation_id"]);
    assert!(
        rows[2]["source_id"].is_null(),
        "admin actions are account-level"
    );
    assert!(rows[2]["detail"].get("request_hash").is_none());
    assert!(rows[4]["detail"].get("attempted_quality").is_none());
    assert_eq!(rows[1]["detail"]["upstream_bit_rate"], 0);
    assert_eq!(rows[1]["detail"]["upstream_priv_status"], 0);
    assert_eq!(rows[1]["detail"]["upstream_fail_process"], json!([]));

    let (_, filtered) = app
        .admin(
            Method::GET,
            &format!("{detail}/logs?view=errors&event_type=entitlement"),
        )
        .await;
    let failed = filtered["items"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["error_code"], "UPSTREAM_VIP_CHECK_FAILED");
    assert_eq!(failed[0]["ok"], false);
}

#[tokio::test]
async fn runtime_and_manual_refreshes_share_success_and_failure() {
    for mode in [0, 1] {
        let app = Arc::new(Fixture::start(Some(1)).await);
        app.upstream.login_mode.store(mode, SeqCst);
        app.upstream.login.pause_next();

        let first = {
            let app = app.clone();
            tokio::spawn(async move { app.song("rt_fixture", old_three_fields()).await.0 })
        };
        app.upstream.login.wait_until_entered().await;

        let parallel = {
            let app = app.clone();
            tokio::spawn(async move { app.song("rt_shared", old_three_fields()).await.0 })
        };
        let manual = {
            let app = app.clone();
            tokio::spawn(async move { app.admin(Method::POST, AUTH_ROUTE).await.0 })
        };
        // All three HTTP requests overlap the paused upstream refresh.
        tokio::time::sleep(Duration::from_millis(70)).await;
        app.upstream.login.release.notify_one();

        let expected = if mode == 0 {
            StatusCode::OK
        } else {
            StatusCode::BAD_GATEWAY
        };
        for pending in [first, parallel, manual] {
            assert_eq!(pending.await.unwrap(), expected);
        }
        assert_eq!(app.upstream.login.requests.load(SeqCst), 1);
        assert_eq!(app.upstream.vip.requests.load(SeqCst), 0);

        let (_, logs) = app.admin(Method::GET, LOG_ROUTE).await;
        let rows = logs["items"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["event_type"], "music_url");
        assert_eq!(rows[1]["event_type"], "token_refresh");
        assert_eq!(rows[0]["correlation_id"], rows[1]["correlation_id"]);
        assert_eq!(rows[1]["ok"], mode == 0);
        if mode == 1 {
            assert_eq!(rows[0]["detail"]["upstream_status"], Value::Null);
            assert_eq!(rows[0]["latency_ms"], Value::Null);
            let (_, auth) = app.admin(Method::GET, DETAIL_ROUTE).await;
            assert_eq!(auth["account"]["status"], "login_failed");
        }
    }
}

#[tokio::test]
async fn late_music_and_vip_responses_cannot_overwrite_renewed_credentials() {
    let app = Arc::new(Fixture::start(Some(now_ts())).await);
    app.upstream.song_mode.store(3, SeqCst);
    app.upstream.song.pause_next();
    let song = {
        let app = app.clone();
        tokio::spawn(async move { app.song("rt_fixture", old_three_fields()).await.0 })
    };
    app.upstream.song.wait_until_entered().await;
    assert_eq!(app.admin(Method::POST, AUTH_ROUTE).await.0, StatusCode::OK);
    app.upstream.song.release.notify_one();
    assert_eq!(song.await.unwrap(), StatusCode::OK);
    assert_eq!(app.token().await, "auth-1");

    app.upstream.vip_mode.store(3, SeqCst);
    app.upstream.vip.pause_next();
    let sync = {
        let app = app.clone();
        tokio::spawn(async move { app.admin(Method::POST, SYNC_ROUTE).await.0 })
    };
    app.upstream.vip.wait_until_entered().await;
    let rebound = app.bind().await;
    assert_eq!(rebound["account"]["account_id"], ACCOUNT);
    let rebound_source = rebound["source"]["source_id"].as_str().unwrap();
    assert!([SOURCE, "src_shared"].contains(&rebound_source));
    app.upstream.vip.release.notify_one();
    assert_eq!(sync.await.unwrap(), StatusCode::OK);
    assert_eq!(app.token().await, "auth-2");

    let (_, shared_logs) = app
        .admin(Method::GET, "/api/v1/admin/sources/src_shared/logs")
        .await;
    let shared = shared_logs["items"].as_array().unwrap();
    assert!(shared.iter().any(|row| row["event_type"] == "entitlement"));
    assert!(
        shared
            .iter()
            .any(|row| row["event_type"] == "token_refresh")
    );
    assert!(shared.iter().all(|row| row["event_type"] != "music_url"));
}

#[tokio::test]
async fn verification_is_distinct_from_auth_and_refresh_can_retry_a_real_rejection() {
    let app = Fixture::start(Some(now_ts())).await;
    app.upstream.song_mode.store(1, SeqCst);
    let (status, error) = app.song("rt_fixture", old_three_fields()).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(error["code"], "UPSTREAM_REQUEST_FAILED");
    assert_eq!(app.upstream.login.requests.load(SeqCst), 0);

    app.upstream.login_mode.store(2, SeqCst);
    assert_eq!(
        app.admin(Method::POST, AUTH_ROUTE).await.0,
        StatusCode::BAD_GATEWAY
    );
    let (_, detail) = app.admin(Method::GET, DETAIL_ROUTE).await;
    assert_eq!(detail["account"]["status"], "active");
    let (_, logs) = app.admin(Method::GET, LOG_ROUTE).await;
    assert_eq!(logs["items"][0]["detail"]["human_verification"], true);
    assert_eq!(logs["items"][1]["detail"]["human_verification"], true);
    assert_eq!(logs["items"][1]["detail"]["upstream_status"], 0);
    assert_eq!(app.token().await, "legacy-token");

    let app = Fixture::start(Some(now_ts())).await;
    app.upstream.song_mode.store(2, SeqCst);
    assert_eq!(
        app.song("rt_fixture", old_three_fields()).await.0,
        StatusCode::OK
    );
    let (_, logs) = app.admin(Method::GET, LOG_ROUTE).await;
    let entries = logs["items"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["detail"]["retry_count"], 1);
    assert_eq!(entries[1]["detail"]["reason"], "rejected");
    assert_eq!(entries[2]["detail"]["upstream_status"], 0);
    assert!(
        entries
            .iter()
            .all(|entry| entry["correlation_id"] == entries[1]["correlation_id"])
    );
    assert_eq!(app.upstream.login.requests.load(SeqCst), 1);
}

#[tokio::test]
async fn legacy_script_urls_and_request_shapes_survive_the_import() {
    let app = Fixture::start(Some(now_ts())).await;
    let script = app
        .http
        .get(format!("{}/s/st_fixture.js", app.server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(script.status(), StatusCode::OK);
    let script = script.text().await.unwrap();
    assert!(script.contains("@version 0.1.2"));
    assert!(script.contains("rt_fixture"));

    for (version, expect_update) in [("0.1.0", true), ("0.1.1", true), ("0.1.2", false)] {
        let reply: Value = app
            .http
            .post(format!("{}/api/v1/runtime/script-update", app.server.base))
            .bearer_auth("rt_fixture")
            .json(&json!({"version":version}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(reply["update"], expect_update);
    }
    let new = json!({"hash":"TEST-HASH","quality":"flac","track_title":"Demo","types":{"flac":{"size":80000}}});
    for shape in [
        new,
        json!(old_three_fields().to_string()),
        old_three_fields(),
    ] {
        let (status, reply) = app.song("rt_fixture", shape).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(reply["url"], "https://example.test/song.flac");
    }
    let (status, error) = app.song("rt_wrong", old_three_fields()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["code"], "SOURCE_NOT_FOUND");
    assert!(error["message"].is_string());
    assert!(error.get("retryable").is_none());
}

#[tokio::test]
async fn upstream_timeout_belongs_to_the_request() {
    let app = Fixture::start(Some(now_ts())).await;
    app.upstream.login.pause_next();
    let client = KugouLiteClient::new(&app.state.config);
    let mut cookies = CookieStore::new();
    cookies.insert("token", "fixture");
    let result = client
        .refresh_login(&cookies, Duration::from_millis(75))
        .await;
    assert!(
        result.is_err(),
        "this individual request has a shorter configured deadline"
    );
    assert_eq!(app.upstream.login.requests.load(SeqCst), 1);
}

#[tokio::test]
async fn event_retention_caps_rows_and_reopens_without_reimporting() {
    use crate::domain::event_log::{
        EntitlementAction, Event, EventContext, EventLog, EventTrigger, ListEvents, LogView,
    };
    use crate::repos::event_log::EventLogRepo;

    let dir = tempfile::tempdir().unwrap();
    let db = Sqlite::open(dir.path(), 3).unwrap();
    for index in 0..5 {
        let mut event = EventLog::new(
            EventContext::new(EventTrigger::Admin, Some(ACCOUNT.into()), None),
            Event::Entitlement {
                action: EntitlementAction::Sync,
                observation: None,
            },
            None,
            None,
        );
        event.log_id = index.to_string();
        db.append(&event).await.unwrap();
    }
    let query = ListEvents {
        source_id: SOURCE.into(),
        account_id: ACCOUNT.into(),
        limit: 20,
        view: LogView::All,
        event_type: None,
        include_renewal: false,
    };
    let rows = db.list(&query).await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.log_id.as_str())
            .collect::<Vec<_>>(),
        ["4", "3", "2"]
    );
    assert!(rows.iter().all(|row| row.latency_ms.is_none()));

    let reopened = Sqlite::open(dir.path(), 1).unwrap();
    let rows = reopened.list(&query).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].log_id, "4");
}

#[tokio::test]
async fn legacy_refresh_replies_before_the_separate_sync_and_preserves_a_known_snapshot_on_error() {
    let app = Fixture::start(Some(now_ts())).await;
    app.upstream.vip_mode.store(3, SeqCst);
    assert_eq!(app.admin(Method::POST, SYNC_ROUTE).await.0, StatusCode::OK);
    let (_, before) = app.admin(Method::GET, DETAIL_ROUTE).await;
    assert_eq!(before["account"]["entitlements"][0]["product_type"], "svip");

    app.upstream.vip_mode.store(1, SeqCst);
    app.upstream.vip.pause_next();
    let legacy_path = format!("{DETAIL_ROUTE}/refresh");
    let (status, receipt) = tokio::time::timeout(
        Duration::from_secs(2),
        app.admin(Method::POST, &legacy_path),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["source_id"], SOURCE);
    assert_eq!(receipt["ok"], true);
    assert!(receipt["updated_at"].is_number());
    app.upstream.vip.wait_until_entered().await;
    app.upstream.vip.release.notify_one();

    let list_path = format!("{LOG_ROUTE}?view=errors&event_type=entitlement");
    let failed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (_, logs) = app.admin(Method::GET, &list_path).await;
            if let Some(event) = logs["items"].as_array().unwrap().first() {
                break event.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (_, token_logs) = app
        .admin(
            Method::GET,
            &format!("{LOG_ROUTE}?event_type=token_refresh"),
        )
        .await;
    assert_eq!(
        failed["correlation_id"],
        token_logs["items"][0]["correlation_id"]
    );
    let (_, after) = app.admin(Method::GET, DETAIL_ROUTE).await;
    assert_eq!(after["account"]["status"], "active");
    assert_eq!(
        after["account"]["entitlements"],
        before["account"]["entitlements"]
    );
    assert_eq!(
        after["account"]["entitlements_synced_at"],
        before["account"]["entitlements_synced_at"]
    );
    assert_eq!(after["account"]["entitlements_sync_failed"], true);
}

#[tokio::test]
async fn renewal_api_keeps_legacy_fallback_and_logs_and_uses_account_scope() {
    let app = Fixture::start(Some(now_ts())).await;
    let (_, detail) = app.admin(Method::GET, DETAIL_ROUTE).await;
    assert_eq!(detail["account"]["renewal"]["enabled"], false);
    assert!(detail["account"].get("entitlements").is_none());

    let change = |enabled| {
        app.http
            .patch(format!("{}{RENEWAL_ROUTE}", app.server.base))
            .json(&json!({"enabled":enabled}))
    };
    assert_eq!(
        change(true).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let enabled = change(true).bearer_auth(ADMIN_TOKEN).send().await.unwrap();
    assert_eq!(enabled.status(), StatusCode::OK);
    assert_eq!(enabled.json::<Value>().await.unwrap()["enabled"], true);
    let (_, sources) = app.admin(Method::GET, "/api/v1/admin/sources").await;
    for source in sources["items"].as_array().unwrap() {
        assert_eq!(source["renewal"]["enabled"], true);
    }

    let patched = app
        .http
        .patch(format!("{}{DETAIL_ROUTE}", app.server.base))
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({"quality_fallback":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(patched.status(), StatusCode::OK);
    app.upstream.song_mode.store(4, SeqCst);
    app.upstream.vip.pause_next();

    // The mock's entitlement read is blocked, so the original playback could not finish if
    // it awaited the renewal job. The lower-tier URL is the existing fallback response.
    let reply = tokio::time::timeout(
        Duration::from_secs(2),
        app.song("rt_fixture", old_three_fields()),
    )
    .await
    .unwrap();
    assert_eq!(
        reply,
        (
            StatusCode::OK,
            json!({"url":"https://example.test/fallback.mp3"})
        )
    );
    app.upstream.vip.wait_until_entered().await;
    assert_eq!(app.upstream.claim.requests.load(SeqCst), 0);
    app.upstream.vip.release.notify_one();
    let (status, state) = app.admin(Method::POST, CLAIM_ROUTE).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["state"], "propagating");
    assert!(state["estimated_end_at"].is_null());
    assert_eq!(app.upstream.claim.requests.load(SeqCst), 1);
    assert_eq!(app.upstream.upgrade.requests.load(SeqCst), 1);
    assert_eq!(app.upstream.login.requests.load(SeqCst), 0);

    let (_, detail) = app.admin(Method::GET, DETAIL_ROUTE).await;
    assert_eq!(detail["account"]["entitlements"], json!([]));
    assert_eq!(detail["account"]["renewal"], state);
    let (_, old) = app
        .admin(Method::GET, &format!("{LOG_ROUTE}?event_type=entitlement"))
        .await;
    assert_eq!(old["items"].as_array().unwrap().len(), 1);
    assert_eq!(old["items"][0]["detail"]["action"], "sync");

    let (_, new) = app
        .admin(Method::GET, &format!("{LOG_ROUTE}?include_renewal=true"))
        .await;
    let logs = new["items"].as_array().unwrap();
    let original = logs
        .iter()
        .find(|item| item["detail"]["upstream_status"] == 2)
        .unwrap();
    for (event, (action, code)) in logs
        .iter()
        .take(2)
        .zip([("upgrade", 297002), ("claim", 131001)])
    {
        assert_eq!(event["detail"]["action"], action);
        assert_eq!(event["detail"]["reason"], "t2_expired_refusal");
        assert_eq!(event["detail"]["upstream_code"], code);
        assert_eq!(event["ok"], true);
        assert_eq!(event["correlation_id"], original["correlation_id"]);
    }
    let disabled = change(false).bearer_auth(ADMIN_TOKEN).send().await.unwrap();
    assert_eq!(disabled.json::<Value>().await.unwrap()["enabled"], false);
    assert_eq!(
        app.admin(Method::POST, CLAIM_ROUTE).await.1["upgrade"],
        "done"
    );
    assert_eq!(app.upstream.claim.requests.load(SeqCst), 1);
}

#[tokio::test]
async fn entitlement_adapter_uses_a_dedicated_write_deadline_and_does_not_follow_redirects() {
    use axum::routing::{get, post};

    let replayed = Arc::new(AtomicUsize::new(0));
    let counter = replayed.clone();
    let upstream = HttpServer::start(Router::new()
        .route("/youth/day/vip", post(|| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Json(json!({"status":1,"data":{"recharge_hours":24}}))
        }))
        .route("/youth/day/vip/upgrade", post(|| async {
            (StatusCode::TEMPORARY_REDIRECT, [("location", "/write-sentinel")])
        }))
        .route("/write-sentinel", post(move || async move {
            counter.fetch_add(1, SeqCst);
            StatusCode::OK
        }))
        .route("/youth/month/vip/record", get(|| async {
            Json(json!({"status":1,"data":{"list":[{"receive_day":"2035-02-01","vip_type":"svip"}]}}))
        }))).await;
    let data = tempfile::tempdir().unwrap();
    let mut config = Config {
        listen_addr: "127.0.0.1:0".into(),
        public_base_url: "http://example.test".into(),
        admin_token: ADMIN_TOKEN.into(),
        kugou_api_base_url: upstream.base.clone(),
        data_dir: data.path().to_owned(),
        upstream_timeout_ms: 10,
        account_refresh_ttl: 100,
        event_log_limit: 100,
        web_dist_dir: None,
    };
    let client = KugouLiteClient::new(&config);
    let day = upstream_day(entitlement_time());
    let cookies = CookieStore::new();
    assert_eq!(
        client
            .grant(&cookies, GrantStep::Claim, day, Duration::from_secs(1))
            .await
            .result
            .unwrap(),
        GrantResult::Confirmed {
            added_hours: Some(24)
        },
    );
    let failure = client
        .grant(&cookies, GrantStep::Upgrade, day, Duration::from_secs(1))
        .await
        .result
        .unwrap_err();
    assert!(failure.write_may_have_applied());
    assert_eq!(failure.http_status, Some(307));
    assert_eq!(replayed.load(SeqCst), 0);

    config.upstream_timeout_ms = 2_000;
    let client = KugouLiteClient::new(&config);
    let record = client.daily_record(&cookies).await.result.unwrap();
    assert_eq!(record.entry(day), RecordEntry::Found(GrantStep::Upgrade));
}
