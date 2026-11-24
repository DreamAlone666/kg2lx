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
use crate::domain::cookie_store::CookieStore;
use crate::domain::now_ts;
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
const ACCOUNT_ROUTE: &str = "/api/v1/admin/accounts/acc_fixture";
const LOG_ROUTE: &str = "/api/v1/admin/logs";
const RENEWAL_ROUTE: &str = "/api/v1/admin/accounts/acc_fixture/entitlements/renewal";
const CLAIM_ROUTE: &str = "/api/v1/admin/accounts/acc_fixture/entitlements/renewal/claim";
const MUSIC_URL_ROUTE: &str = "/api/v1/runtime/music-url";
const SCRIPT_UPDATE_ROUTE: &str = "/api/v1/runtime/script-update";

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
    device_requests: AtomicUsize,
    qr_checks: AtomicUsize,
    qr_mode: AtomicUsize,
    claim_mode: AtomicUsize,
    vip_mode: AtomicUsize,
    song_mode: AtomicUsize,
}

impl Upstream {
    async fn serve(State(this): State<Arc<Self>>, request: Request) -> Response {
        assert_eq!(request.headers().get("x-apicache-bypass").unwrap(), "1");
        match request.uri().path() {
            "/login/qr/key" => Json(json!({"data":{"key":"qr-test"}})).into_response(),
            "/login/qr/create" => {
                Json(json!({"data":{"url":"https://example.test/qr"}})).into_response()
            }
            "/login/qr/check" => {
                this.qr_checks.fetch_add(1, SeqCst);
                if this.qr_mode.load(SeqCst) == 1 {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"status":0,"errcode":20028})),
                    )
                        .into_response();
                }
                Json(json!({"data":{"status":4,"userid":42,"token":"qr-session"}})).into_response()
            }
            "/register/dev" => {
                this.device_requests.fetch_add(1, SeqCst);
                if this.login_mode.load(SeqCst) == 5 {
                    return StatusCode::TOO_MANY_REQUESTS.into_response();
                }
                Json(json!({"status":1,"cookie":"dfid=fixture-device"})).into_response()
            }
            "/login/token" => {
                let number = this.login.on_request().await;
                let userid = request
                    .headers()
                    .get("cookie")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .split(';')
                    .find_map(|part| part.trim().strip_prefix("userid="))
                    .unwrap();
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
                    3 => Json(json!({"status":1,"cookie":"token=wrong-account; userid=99; dfid="})).into_response(),
                    4 => Json(json!({"status":1,"cookie":format!("userid={userid}")})).into_response(),
                    5 => StatusCode::TOO_MANY_REQUESTS.into_response(),
                    6 => (StatusCode::BAD_GATEWAY, Json(json!({"status":0,"msg":"dfid registration invalid"}))).into_response(),
                    7 => (StatusCode::BAD_GATEWAY, Json(json!({"status":0,"msg":"token expired"}))).into_response(),
                    _ => Json(json!({"status":1,"cookie":format!("token=auth-{number}; userid={userid}")}))
                        .into_response(),
                }
            }
            "/youth/union/vip" => {
                let number = this.vip.on_request().await;
                match this.vip_mode.load(SeqCst) {
                    1 => (StatusCode::BAD_GATEWAY, Json(json!({"status":0}))).into_response(),
                    2 => Json(json!({"status":0,"data":{"busi_vip":[]}})).into_response(),
                    3 if number == 1 => Json(
                        json!({"status":1,"data":{"busi_vip":[{"product_type":"svip","is_vip":1}]},
                        "cookie":"token=late-vip; vip_extra=seen"}),
                    )
                    .into_response(),
                    4 => Json(json!({"status": 1, "data": {"busi_vip": [{
                        "product_type": "svip",
                        "is_vip": 1,
                        "vip_begin_time": "2035-02-01 00:00:00",
                        "vip_end_time": "2035-02-03 00:00:00"
                    }]}}))
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
                    7 if number == 1 => (StatusCode::UNAUTHORIZED, Json(json!({"status":0,"code":301}))).into_response(),
                    4 | 7 if request.uri().query().is_some_and(|query| query.contains("quality=flac")) =>
                        Json(json!({"status":2,"data":{},"priv_status":0})).into_response(),
                    4 | 7 => Json(json!({"status":1,"url":"https://example.test/fallback.mp3"})).into_response(),
                    5 => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status":1,"url":"https://example.test/invalid.mp3"}))).into_response(),
                    6 => StatusCode::TOO_MANY_REQUESTS.into_response(),
                    8 => {
                        let args: std::collections::HashMap<_, _> =
                            url::form_urlencoded::parse(request.uri().query().unwrap().as_bytes()).into_owned().collect();
                        assert_eq!(args["hash"], "sample & # hash");
                        assert_eq!(args["album_audio_id"], "album + track");
                        assert_eq!(args["quality"], "flac");
                        Json(json!({"status":1,"url":"https://example.test/song.flac"})).into_response()
                    }
                    10 if number == 1 => Json(json!({"status":2,"priv_status":0})).into_response(),
                    9 | 10 => Json(json!({"status":3,"priv_status":0,"fail_process":[]})).into_response(),

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
                if this.claim_mode.load(SeqCst) == 1 {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"status":0,"error_code":4321})),
                    )
                        .into_response();
                }
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"status":0,"error_code":131001,"cookie":"fixture_grant=merged"})),
                )
                    .into_response()
            }
            "/youth/day/vip/upgrade" => {
                assert_eq!(request.method(), Method::POST);
                let cookie = request.headers().get("cookie").unwrap().to_str().unwrap();
                assert!(cookie.contains("fixture_grant=merged"));
                this.upgrade.on_request().await;
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"status":0,"error_code":297002})),
                )
                    .into_response()
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
        Self::start_with_accounts(legacy_time, false).await
    }

    async fn start_with_accounts(legacy_time: Option<i64>, second_account: bool) -> Self {
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
            if second_account {
                write(
                    "accounts/acc_other.json",
                    json!({
                        "account_id":"acc_other","provider":"kugou_lite","upstream_userid":"43","status":"active",
                        "last_refresh_at":refreshed_at,"created_at":1,"updated_at":1,
                        "cookies":{"items":{"userid":"43","dfid":"fixture-device","token":"other-token"}}
                    }),
                );
            }
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
            .post(format!("{}{MUSIC_URL_ROUTE}", self.server.base))
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

    async fn wait_for_post_bind(&self, account_id: &str) -> Vec<Value> {
        let route = format!("{LOG_ROUTE}?account_id={account_id}");
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let (_, reply) = self.admin(Method::GET, &route).await;
                let rows = reply["items"].as_array().unwrap();
                if rows
                    .iter()
                    .any(|event| event["detail"]["reason"] == "post_bind")
                {
                    return rows.clone();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the background sync should record a post_bind event")
    }

    async fn script_update(&self, token: &str) -> (StatusCode, Value) {
        let response = self
            .http
            .post(format!("{}{SCRIPT_UPDATE_ROUTE}", self.server.base))
            .bearer_auth(token)
            .json(&json!({"version": "0.1.2"}))
            .send()
            .await
            .unwrap();
        (response.status(), response.json().await.unwrap())
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
async fn runtime_routes_require_their_own_bearer_token() {
    let app = Fixture::start(None).await;
    for path in [MUSIC_URL_ROUTE, SCRIPT_UPDATE_ROUTE] {
        for authorization in [
            None,
            Some("Bearer"),
            Some("Bearer  "),
            Some("Basic example"),
        ] {
            let mut request = app.http.post(format!("{}{path}", app.server.base));
            if let Some(header) = authorization {
                request = request.header("Authorization", header);
            }
            let response = request.body("invalid-json").send().await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["code"], "RUNTIME_UNAUTHORIZED");
            assert!(body.get("retryable").is_none());
        }
    }
}

#[tokio::test]
async fn whole_song_refusal_stops_fallback_and_does_not_request_vip() {
    for (mode, song_calls) in [(9, 1), (10, 2)] {
        let app = Fixture::start(Some(now_ts())).await;
        app.upstream.song_mode.store(mode, SeqCst);
        assert_eq!(
            app.http
                .patch(format!("{}{DETAIL_ROUTE}", app.server.base))
                .bearer_auth(ADMIN_TOKEN)
                .json(&json!({"quality_fallback": true}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(app.admin(Method::POST, SYNC_ROUTE).await.0, StatusCode::OK);
        assert_eq!(
            app.http
                .patch(format!("{}{RENEWAL_ROUTE}", app.server.base))
                .bearer_auth(ADMIN_TOKEN)
                .json(&json!({"enabled": true}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let (status, body) = app.song("rt_fixture", old_three_fields()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            json!({"code":"UPSTREAM_SONG_FORBIDDEN","message":"song playback forbidden by upstream"})
        );
        let (_, logs) = app
            .admin(Method::GET, &format!("{LOG_ROUTE}?event_type=music_url"))
            .await;
        let events = logs["items"].as_array().unwrap();
        assert_eq!(events.len(), song_calls);
        assert_eq!(events[0]["error_code"], "UPSTREAM_SONG_FORBIDDEN");
        assert_eq!(events[0]["detail"]["upstream_status"], 3);
        assert_eq!(
            events[0]["detail"]["quality_fallback_index"],
            song_calls - 1
        );
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(app.upstream.song.requests.load(SeqCst), song_calls);
        assert_eq!(app.upstream.vip.requests.load(SeqCst), 1);
        assert_eq!(app.upstream.claim.requests.load(SeqCst), 0);
        assert_eq!(app.upstream.upgrade.requests.load(SeqCst), 0);
    }
}

#[tokio::test]
async fn rotating_a_source_revokes_both_old_tokens_and_keeps_the_source_settings() {
    use crate::repos::source::SourceRepo;
    let app = Fixture::start(Some(now_ts())).await;
    let before = SourceRepo::find_by_id(app.state.storage.as_ref(), SOURCE)
        .await
        .unwrap()
        .unwrap();
    let route = format!("{DETAIL_ROUTE}/rotate-tokens");
    let (status, reply) = app.admin(Method::POST, &route).await;
    assert_eq!(status, StatusCode::OK);
    let after = SourceRepo::find_by_id(app.state.storage.as_ref(), SOURCE)
        .await
        .unwrap()
        .unwrap();
    let new_script = url::Url::parse(reply["script_url"].as_str().unwrap()).unwrap();
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["source_id"], SOURCE);
    assert_eq!(reply["updated_at"], after.updated_at);
    assert_eq!(
        reply["runtime_token_preview"],
        after.runtime_token_preview()
    );
    assert!(reply.get("runtime_token").is_none());
    assert_eq!(new_script.host_str(), Some("example.test"));
    assert_eq!(new_script.path(), format!("/s/{}.js", after.script_token));
    assert!(uuid::Uuid::parse_str(after.script_token.strip_prefix("st_").unwrap()).is_ok());
    assert!(uuid::Uuid::parse_str(after.runtime_token.strip_prefix("rt_").unwrap()).is_ok());
    assert_ne!(before.script_token, after.script_token);
    assert_ne!(before.runtime_token, after.runtime_token);
    assert_eq!(after.account_id, before.account_id);
    assert_eq!(after.name, before.name);
    assert_eq!(after.quality_fallback, before.quality_fallback);
    assert!(after.updated_at > before.updated_at);

    let (status, missing) = app.admin(Method::GET, "/s/st_fixture.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["code"], "SOURCE_NOT_FOUND");
    for (status, error) in [
        app.song("rt_fixture", old_three_fields()).await,
        app.script_update("rt_fixture").await,
    ] {
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error["code"], "SOURCE_NOT_FOUND");
    }
    let script = app
        .http
        .get(format!("{}{}", app.server.base, new_script.path()))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(script.contains(&format!("const TOKEN = \"{}\"", after.runtime_token)));
    assert!(!script.contains(&before.runtime_token));
    assert_eq!(
        app.song(&after.runtime_token, old_three_fields()).await.0,
        StatusCode::OK
    );
    let (status, update) = app.script_update(&after.runtime_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(update["update"], true);
    assert!(
        update["log"]
            .as_str()
            .unwrap()
            .contains(new_script.as_str())
    );
    assert_eq!(
        app.song("rt_shared", old_three_fields()).await.0,
        StatusCode::OK
    );
    let (status, missing) = app
        .admin(Method::POST, "/api/v1/admin/sources/missing/rotate-tokens")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["code"], "SOURCE_NOT_FOUND");
}

#[tokio::test]
async fn deleting_an_account_revokes_all_its_sources_and_keeps_its_history() {
    let app = Fixture::start_with_accounts(Some(now_ts()), true).await;
    assert_eq!(
        app.song("rt_fixture", old_three_fields()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        app.song("rt_shared", old_three_fields()).await.0,
        StatusCode::OK
    );
    let (status, reply) = app.admin(Method::DELETE, ACCOUNT_ROUTE).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply, json!({"ok": true, "account_id": ACCOUNT}));
    let (status, list) = app.admin(Method::GET, "/api/v1/admin/accounts").await;
    assert_eq!(status, StatusCode::OK);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["account_id"], "acc_other");
    for method in [Method::GET, Method::DELETE] {
        let (status, error) = app.admin(method, ACCOUNT_ROUTE).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error["code"], "ACCOUNT_NOT_FOUND");
    }
    for (script_path, token) in [
        ("/s/st_fixture.js", "rt_fixture"),
        ("/s/st_shared.js", "rt_shared"),
    ] {
        for (status, error) in [
            app.admin(Method::GET, script_path).await,
            app.song(token, old_three_fields()).await,
            app.script_update(token).await,
        ] {
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(error["code"], "SOURCE_NOT_FOUND");
        }
    }
    let (_, logs) = app
        .admin(Method::GET, &format!("{LOG_ROUTE}?account_id={ACCOUNT}"))
        .await;
    let rows = logs["items"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for (row, source) in rows.iter().zip(["src_shared", SOURCE]) {
        assert_eq!(row["account_id"], ACCOUNT);
        assert_eq!(row["source_id"], source);
        assert_eq!(row["ok"], true);
    }
}

#[tokio::test]
async fn deletion_waits_for_an_account_sync_and_a_concurrent_rebind() {
    for rebind in [false, true] {
        let app = Arc::new(Fixture::start(Some(now_ts())).await);
        let upstream = if rebind {
            &app.upstream.login
        } else {
            &app.upstream.vip
        };
        upstream.pause_next();
        let inflight = {
            let app = app.clone();
            tokio::spawn(async move {
                if rebind {
                    assert_eq!(app.bind().await["account"]["account_id"], ACCOUNT);
                } else {
                    assert_eq!(app.admin(Method::POST, SYNC_ROUTE).await.0, StatusCode::OK);
                }
            })
        };
        upstream.wait_until_entered().await;
        let mut deletion = {
            let app = app.clone();
            tokio::spawn(async move { app.admin(Method::DELETE, ACCOUNT_ROUTE).await })
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut deletion)
                .await
                .is_err(),
            "deletion must wait for the account operation"
        );
        upstream.release.notify_one();
        inflight.await.unwrap();
        assert_eq!(deletion.await.unwrap().0, StatusCode::OK);
        assert_eq!(
            app.admin(Method::GET, ACCOUNT_ROUTE).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            app.song("rt_fixture", old_three_fields()).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn refreshes_two_accounts_and_rejects_wrong_identity_or_missing_token() {
    let app = Fixture::start_with_accounts(Some(now_ts()), true).await;
    let db = &app.state.storage;
    for (index, (account_id, userid)) in [(ACCOUNT, "42"), ("acc_other", "43")]
        .into_iter()
        .enumerate()
    {
        let route = format!("/api/v1/admin/accounts/{account_id}/refresh-token");
        assert_eq!(app.admin(Method::POST, &route).await.0, StatusCode::OK);
        let account = db.find_by_id(account_id).await.unwrap().unwrap();
        assert_eq!(account.cookies.get("userid"), Some(userid));
        assert_eq!(
            account.cookies.get("token").unwrap(),
            format!("auth-{}", index + 1)
        );
    }
    let previous = db.find_by_id(ACCOUNT).await.unwrap().unwrap();
    for mode in [3, 4, 6] {
        app.upstream.login_mode.store(mode, SeqCst);
        assert_eq!(
            app.admin(Method::POST, AUTH_ROUTE).await.0,
            StatusCode::BAD_GATEWAY
        );
        let current = db.find_by_id(ACCOUNT).await.unwrap().unwrap();
        assert_eq!(current.cookies, previous.cookies);
        assert_eq!(current.credential_version, previous.credential_version);
        assert_eq!(current.status, previous.status);
    }
    assert_eq!(app.upstream.device_requests.load(SeqCst), 0);
    app.upstream.login_mode.store(5, SeqCst);
    let (status, error) = app.admin(Method::POST, AUTH_ROUTE).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error["code"], "UPSTREAM_RATE_LIMITED");
    let client = KugouLiteClient::new(&app.state.config);
    let registration = client
        .ensure_dfid(&CookieStore::new(), Duration::from_secs(2))
        .await;
    assert_eq!(
        registration.err().unwrap().error.code.as_str(),
        "UPSTREAM_RATE_LIMITED"
    );
    app.upstream.login_mode.store(7, SeqCst);
    assert_eq!(
        app.admin(Method::POST, AUTH_ROUTE).await.0,
        StatusCode::BAD_GATEWAY
    );
    let count = app.upstream.login.requests.load(SeqCst);
    let (status, error) = app.song("rt_fixture", old_three_fields()).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(error["code"], "UPSTREAM_LOGIN_FAILED");
    assert_eq!(app.upstream.login.requests.load(SeqCst), count);
    assert_eq!(app.upstream.song.requests.load(SeqCst), 0);
}

#[tokio::test]
async fn a_structured_rejection_neither_pauses_nor_makes_the_claim_uncertain() {
    let app = Fixture::start(Some(now_ts())).await;
    app.upstream.claim_mode.store(1, SeqCst);
    let (status, response) = app.admin(Method::POST, CLAIM_ROUTE).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["state"], "ready");
    assert_eq!(response["claim"], "ready");
    assert_eq!(response["last_outcome"], "failed");
    assert!(response["paused_until"].is_null());
    let (_, logs) = app.admin(Method::GET, LOG_ROUTE).await;
    assert_eq!(logs["items"][0]["detail"]["upstream_code"], 4321);
    assert_eq!(logs["items"][0]["detail"]["outcome"], "failed");
}

#[tokio::test]
async fn a_failed_qr_session_does_not_poll_upstream_again() {
    let app = Fixture::start(None).await;
    app.upstream.qr_mode.store(1, SeqCst);
    let path = "/api/v1/admin/providers/kugou-lite/login/qr";
    let (status, started) = app.admin(Method::POST, path).await;
    assert_eq!(status, StatusCode::OK);
    let key = started["session_id"].as_str().unwrap();
    for _ in 0..2 {
        let (status, error) = app.admin(Method::GET, &format!("{path}/{key}")).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(error["code"], "UPSTREAM_LOGIN_FAILED");
        assert!(error["message"].as_str().unwrap().contains("verification"));
    }
    assert_eq!(app.upstream.qr_checks.load(SeqCst), 1);
}

#[tokio::test]
async fn playback_fallback_survives_refresh_and_errors_keep_their_boundary() {
    let app = Fixture::start(Some(now_ts())).await;
    let response = app
        .http
        .patch(format!("{}{DETAIL_ROUTE}", app.server.base))
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({"quality_fallback":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    app.upstream.song_mode.store(7, SeqCst);
    assert_eq!(
        app.song("rt_fixture", old_three_fields()).await,
        (
            StatusCode::OK,
            json!({"url":"https://example.test/fallback.mp3"})
        )
    );
    assert_eq!(app.upstream.login.requests.load(SeqCst), 1);

    app.upstream.song_mode.store(5, SeqCst);
    let (status, error) = app.song("rt_fixture", old_three_fields()).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(error["code"], "UPSTREAM_REQUEST_FAILED");
    assert_eq!(error["message"], "internal error");

    app.upstream.song_mode.store(6, SeqCst);
    let (status, error) = app.song("rt_fixture", old_three_fields()).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error["retryable"], true);
    let (_, logs) = app.admin(Method::GET, LOG_ROUTE).await;
    assert_eq!(logs["items"][0]["detail"]["status_code"], 429);

    app.upstream.song_mode.store(8, SeqCst);
    let (status, _) = app
        .song(
            "rt_fixture",
            json!({
                "hash":"sample & # hash","album_audio_id":"album + track","quality":"flac"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn router_authenticates_before_parsing_and_preserves_static_http_contract() {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("web");
    std::fs::create_dir_all(web.join("_app/immutable")).unwrap();
    std::fs::write(web.join("index.html"), "<title>fixture</title>").unwrap();
    std::fs::write(web.join("robots.txt"), "User-agent: *").unwrap();
    std::fs::write(web.join("_app/version.json"), "{}").unwrap();
    std::fs::write(web.join("_app/immutable/test.js"), "export {}").unwrap();
    let state = AppState::new(Config {
        listen_addr: "127.0.0.1:0".into(),
        public_base_url: "http://example.test".into(),
        admin_token: ADMIN_TOKEN.into(),
        kugou_api_base_url: "http://127.0.0.1:1".into(),
        data_dir: dir.path().join("data"),
        upstream_timeout_ms: 1000,
        account_refresh_ttl: 100,
        web_dist_dir: Some(web),
        event_log_limit: 100,
    })
    .unwrap();
    let server = HttpServer::start(crate::build_app(state.clone())).await;
    let http = Client::new();

    let response = http
        .patch(format!("{}{DETAIL_ROUTE}", server.base))
        .header("content-type", "application/json")
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.json::<Value>().await.unwrap()["code"],
        "ADMIN_UNAUTHORIZED"
    );
    for (method, path, expected) in [
        (Method::POST, "/api/unknown", StatusCode::NOT_FOUND),
        (
            Method::GET,
            "/_app/immutable/missing.js",
            StatusCode::NOT_FOUND,
        ),
        (Method::GET, "/missing.css", StatusCode::NOT_FOUND),
        (Method::POST, "/accounts", StatusCode::METHOD_NOT_ALLOWED),
        (Method::GET, "/api/v1/admin/sources", StatusCode::NOT_FOUND),
        (Method::GET, DETAIL_ROUTE, StatusCode::METHOD_NOT_ALLOWED),
    ] {
        let response = http
            .request(method, format!("{}{path}", server.base))
            .bearer_auth(ADMIN_TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{path}");
    }
    for (path, cache) in [
        ("/accounts", "no-cache"),
        ("/_app/version.json", "no-cache"),
        (
            "/_app/immutable/test.js",
            "public, max-age=31536000, immutable",
        ),
    ] {
        let response = http
            .head(format!("{}{path}", server.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], cache);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert!(response.bytes().await.unwrap().is_empty());
    }
    let robots = http
        .get(format!("{}/robots.txt", server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(
        robots.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    let health = format!("{}/healthz", server.base);
    assert_eq!(
        http.get(&health).send().await.unwrap().status(),
        StatusCode::OK
    );
    state.storage.shutdown().await.unwrap();
    assert_eq!(
        http.get(&health).send().await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn post_bind_sync_returns_bound_before_fetching_and_records_the_same_event_chain() {
    let app = Fixture::start(None).await;
    app.upstream.vip_mode.store(4, SeqCst);
    app.upstream.vip.pause_next();
    let bound = tokio::time::timeout(Duration::from_secs(2), app.bind())
        .await
        .expect("binding must return while the upstream snapshot is blocked");
    let account_id = bound["account"]["account_id"].as_str().unwrap();
    let path = format!("/api/v1/admin/accounts/{account_id}");
    app.upstream.vip.wait_until_entered().await;
    let (_, before) = app.admin(Method::GET, &path).await;
    assert!(before.get("entitlements").is_none());
    app.upstream.vip.release.notify_one();

    let events = app.wait_for_post_bind(account_id).await;
    assert_eq!(events.len(), 3);
    for (event, kind) in events
        .iter()
        .zip(["entitlement", "account_login", "token_refresh"])
    {
        assert_eq!(event["event_type"], kind);
        assert_eq!(event["trigger"], "qr");
        assert_eq!(event["correlation_id"], events[0]["correlation_id"]);
        assert_eq!(event["ok"], true);
    }
    assert_eq!(events[0]["detail"]["action"], "sync");
    assert_eq!(events[0]["detail"]["reason"], "post_bind");
    let (_, after) = app.admin(Method::GET, &path).await;
    assert_eq!(after["entitlements"][0]["product_type"], "svip");
    assert_eq!(after["entitlements"][0]["active"], true);
    assert_eq!(after["entitlements_synced_at"], entitlement_time());
    assert_eq!(after["renewal"]["enabled"], false);
    let session = bound["session_id"].as_str().unwrap();
    let (status, repeated) = app
        .admin(
            Method::GET,
            &format!("/api/v1/admin/providers/kugou-lite/login/qr/{session}"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated, bound);
    assert_eq!(app.upstream.vip.requests.load(SeqCst), 1);
}

#[tokio::test]
async fn binds_and_refreshes_during_vip_outage_and_exposes_four_event_categories() {
    use crate::repos::source::SourceRepo;
    let app = Fixture::start(None).await;
    app.upstream.vip_mode.store(1, SeqCst);
    let bound = app.bind().await;
    let source_id = bound["source"]["source_id"].as_str().unwrap();
    let account_id = bound["account"]["account_id"].as_str().unwrap();
    let db = Sqlite::open(&app.state.config.data_dir, app.state.config.event_log_limit).unwrap();
    let source = SourceRepo::find_by_id(&db, source_id)
        .await
        .unwrap()
        .unwrap();

    let account = format!("/api/v1/admin/accounts/{account_id}");
    let events = app.wait_for_post_bind(account_id).await;
    assert_eq!(app.upstream.vip.requests.load(SeqCst), 1);
    assert_eq!(events[0]["detail"]["reason"], "post_bind");
    assert_eq!(events[0]["correlation_id"], events[1]["correlation_id"]);
    assert_eq!(events[0]["error_code"], "UPSTREAM_VIP_CHECK_FAILED");
    let (_, response) = app.admin(Method::GET, &account).await;
    assert_eq!(response["status"], "active");
    assert_eq!(response["renewal"]["enabled"], false);
    assert_eq!(response["entitlements_sync_failed"], true);
    assert!(response.get("entitlements").is_none());

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
    let (_, response) = app.admin(Method::GET, &account).await;
    assert_eq!(response["entitlements"], json!([]));
    assert_eq!(response["entitlements_sync_failed"], false);
    assert!(response["entitlements_synced_at"].is_number());

    let (_, logs) = app
        .admin(Method::GET, &format!("{LOG_ROUTE}?account_id={account_id}"))
        .await;
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
    assert_eq!(rows[3]["correlation_id"], rows[4]["correlation_id"]);
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
            &format!("{LOG_ROUTE}?account_id={account_id}&view=errors&event_type=entitlement"),
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
        assert_eq!(rows.len(), 3);
        assert!(rows[..2].iter().all(|row| row["event_type"] == "music_url"));
        assert_eq!(rows[2]["event_type"], "token_refresh");
        assert!(
            rows[..2]
                .iter()
                .any(|row| row["correlation_id"] == rows[2]["correlation_id"])
        );
        assert_eq!(rows[2]["ok"], mode == 0);
        if mode == 1 {
            assert_eq!(rows[0]["detail"]["upstream_status"], Value::Null);
            assert_eq!(rows[0]["latency_ms"], Value::Null);
            let (_, auth) = app.admin(Method::GET, ACCOUNT_ROUTE).await;
            assert_eq!(auth["status"], "login_failed");
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
    app.wait_for_post_bind(ACCOUNT).await;
    assert_eq!(app.token().await, "auth-2");

    let (_, logs) = app
        .admin(Method::GET, &format!("{LOG_ROUTE}?account_id={ACCOUNT}"))
        .await;
    let rows = logs["items"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|row| row["event_type"] == "entitlement" && row["source_id"].is_null())
    );
    assert!(
        rows.iter()
            .any(|row| row["event_type"] == "token_refresh" && row["source_id"].is_null())
    );
    let (_, shared) = app
        .admin(Method::GET, &format!("{LOG_ROUTE}?source_id=src_shared"))
        .await;
    assert!(shared["items"].as_array().unwrap().is_empty());
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
    let (_, detail) = app.admin(Method::GET, ACCOUNT_ROUTE).await;
    assert_eq!(detail["status"], "active");
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
    assert!(script.contains("@version 0.1.3"));
    assert!(script.contains("rt_fixture"));

    for (version, expect_update) in [
        ("0.1.0", true),
        ("0.1.1", true),
        ("0.1.2", true),
        ("0.1.3", false),
    ] {
        let reply: Value = app
            .http
            .post(format!("{}/api/v1/runtime/script-update", app.server.base))
            .bearer_auth("rt_fixture")
            .json(&if version == "0.1.0" {
                json!(json!({"version":version}).to_string())
            } else {
                json!({"version":version})
            })
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
        EntitlementAction, Event, EventContext, EventLog, EventTrigger, ListGlobalEvents, LogView,
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
    let query = ListGlobalEvents {
        source_id: None,
        account_id: Some(ACCOUNT.into()),
        correlation_id: None,
        before_seq: None,
        after_seq: None,
        limit: 20,
        view: LogView::All,
        event_type: None,
    };
    let rows = db.list_global(&query).await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.log.log_id.as_str())
            .collect::<Vec<_>>(),
        ["4", "3", "2"]
    );
    assert!(rows.iter().all(|row| row.log.latency_ms.is_none()));

    let reopened = Sqlite::open(dir.path(), 1).unwrap();
    let rows = reopened.list_global(&query).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].log.log_id, "4");
}

#[tokio::test]
async fn account_refresh_and_entitlement_sync_are_independent() {
    let app = Fixture::start(Some(now_ts())).await;
    app.upstream.vip_mode.store(3, SeqCst);
    assert_eq!(app.admin(Method::POST, SYNC_ROUTE).await.0, StatusCode::OK);
    let (_, before) = app.admin(Method::GET, ACCOUNT_ROUTE).await;
    assert_eq!(before["entitlements"][0]["product_type"], "svip");

    app.upstream.vip_mode.store(1, SeqCst);
    let (status, receipt) = app.admin(Method::POST, AUTH_ROUTE).await;
    assert_eq!(status, StatusCode::OK);
    assert!(receipt.get("source_id").is_none());
    assert_eq!(app.upstream.vip.requests.load(SeqCst), 1);
    assert_eq!(
        app.admin(Method::POST, SYNC_ROUTE).await.0,
        StatusCode::BAD_GATEWAY
    );
    let (_, after) = app.admin(Method::GET, ACCOUNT_ROUTE).await;
    assert_eq!(after["status"], "active");
    assert_eq!(after["entitlements"], before["entitlements"]);
    assert_eq!(
        after["entitlements_synced_at"],
        before["entitlements_synced_at"]
    );
    assert_eq!(after["entitlements_sync_failed"], true);
}

#[tokio::test]
async fn renewal_api_keeps_quality_fallback_and_logs_and_uses_account_scope() {
    let app = Fixture::start(Some(now_ts())).await;
    let (_, detail) = app.admin(Method::GET, ACCOUNT_ROUTE).await;
    assert_eq!(detail["renewal"]["enabled"], false);
    assert!(detail.get("entitlements").is_none());

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
    let (_, accounts) = app.admin(Method::GET, "/api/v1/admin/accounts").await;
    assert_eq!(accounts["items"][0]["renewal"]["enabled"], true);

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

    let (_, detail) = app.admin(Method::GET, ACCOUNT_ROUTE).await;
    assert_eq!(detail["entitlements"], json!([]));
    assert_eq!(detail["renewal"], state);
    let (_, new) = app.admin(Method::GET, LOG_ROUTE).await;
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
        .route("/youth/month/vip/record", get(|request: Request| async move {
            assert_eq!(request.headers().get("x-apicache-bypass").unwrap(), "1");
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

#[tokio::test]
async fn accounts_api_aggregates_zero_one_and_multiple_sources_and_handles_missing_account() {
    let data = tempfile::tempdir().unwrap();
    let write = |relative: &str, json: Value| {
        let path = data.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json.to_string()).unwrap();
    };
    for (id, userid, created_at, display_name) in [
        ("acc_zero", "100", 10_i64, Value::Null),
        ("acc_one", "200", 20_i64, json!("Single Source User")),
        ("acc_two", "300", 30_i64, json!("Multi Source User")),
    ] {
        write(
            &format!("accounts/{id}.json"),
            json!({
                "account_id": id,
                "provider": "kugou_lite",
                "upstream_userid": userid,
                "display_name": display_name,
                "status": "active",
                "last_refresh_at": 1000,
                "created_at": created_at,
                "updated_at": created_at,
                "cookies": {"items": {"userid": userid, "token": "t"}},
                "entitlements": [{"product_type": "svip", "active": true, "begin_at": 100, "end_at": 900}]
            }),
        );
    }
    for (source_id, account_id, name, script_token, runtime_token, created_at, fallback) in [
        (
            "src_one",
            "acc_one",
            "One",
            "st_one",
            "rt_one_123456",
            40_i64,
            false,
        ),
        (
            "src_two_a",
            "acc_two",
            "Two A",
            "st_two_a",
            "rt_two_a_1234",
            50_i64,
            false,
        ),
        (
            "src_two_b",
            "acc_two",
            "Two B",
            "st_two_b",
            "rt_two_b_5678",
            60_i64,
            true,
        ),
    ] {
        write(
            &format!("sources/{source_id}.json"),
            json!({
                "source_id": source_id,
                "account_id": account_id,
                "provider": "kugou_lite",
                "name": name,
                "script_token": script_token,
                "runtime_token": runtime_token,
                "created_at": created_at,
                "updated_at": created_at + 1,
                "quality_fallback": fallback,
            }),
        );
    }

    let state = AppState::with_entitlement_clock(
        Config {
            listen_addr: "127.0.0.1:0".into(),
            public_base_url: "http://example.test".into(),
            admin_token: ADMIN_TOKEN.into(),
            kugou_api_base_url: "http://127.0.0.1:1".into(),
            data_dir: data.path().to_owned(),
            upstream_timeout_ms: 1_000,
            account_refresh_ttl: 100,
            event_log_limit: crate::config::DEFAULT_EVENT_LOG_LIMIT,
            web_dist_dir: None,
        },
        entitlement_time,
    )
    .unwrap();
    let server = HttpServer::start(crate::build_app(state)).await;
    let http = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // Auth guard
    assert_eq!(
        http.get(format!("{}/api/v1/admin/accounts", server.base))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        http.get(format!("{}/api/v1/admin/accounts/acc_zero", server.base))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    // 404 ACCOUNT_NOT_FOUND
    let missing = http
        .get(format!("{}/api/v1/admin/accounts/acc_missing", server.base))
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        missing.json::<Value>().await.unwrap()["code"],
        "ACCOUNT_NOT_FOUND"
    );

    // List accounts: ordered by created_at ASC, account_id ASC with 0, 1, 2 sources
    let list: Value = http
        .get(format!("{}/api/v1/admin/accounts", server.base))
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["account_id"], "acc_zero");
    assert_eq!(items[0]["sources"].as_array().unwrap().len(), 0);
    assert_eq!(items[1]["account_id"], "acc_one");
    assert_eq!(items[1]["sources"].as_array().unwrap().len(), 1);
    assert_eq!(items[2]["account_id"], "acc_two");
    assert_eq!(items[2]["sources"].as_array().unwrap().len(), 2);

    // Single account detail matches exact wire contract
    let detail: Value = http
        .get(format!("{}/api/v1/admin/accounts/acc_two", server.base))
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["account_id"], "acc_two");
    assert_eq!(detail["provider"], "kugou_lite");
    assert_eq!(detail["userid"], "300");
    assert!(detail.get("display_name").is_none());
    assert_eq!(detail["status"], "active");
    assert_eq!(detail["last_refresh_at"], 1000);
    assert!(detail["last_error"].is_null());
    assert!(detail["last_error_code"].is_null());
    assert_eq!(detail["entitlements"][0]["product_type"], "svip");
    assert_eq!(detail["entitlements_sync_failed"], false);
    assert_eq!(detail["renewal"]["enabled"], false);
    let sources = detail["sources"].as_array().unwrap();
    assert_eq!(sources[0]["source_id"], "src_two_a");
    assert_eq!(sources[0]["default_name"], "酷狗概念版 300");
    assert_eq!(sources[0]["name_advisory_chars"], 24);
    assert_eq!(
        sources[0]["script_url"],
        "http://example.test/s/st_two_a.js"
    );
    assert_eq!(sources[0]["runtime_token_preview"], "rt_two_a...");
    assert_eq!(sources[0]["quality_fallback"], false);
    assert_eq!(sources[0]["created_at"], 50);
    assert_eq!(sources[0]["updated_at"], 51);
    assert_eq!(sources[1]["source_id"], "src_two_b");
    assert_eq!(sources[1]["quality_fallback"], true);
}

#[tokio::test]
async fn global_logs_api_supports_unowned_events_filters_pagination_and_correlation_chain() {
    use crate::domain::event_log::{
        EntitlementAction, Event, EventContext, EventLog, EventTrigger, LoginOutcome,
        MusicUrlEvent, MusicUrlStage, RefreshReason,
    };
    use crate::error::ErrorCode;
    use crate::repos::event_log::EventLogRepo;

    let app = Fixture::start(Some(now_ts())).await;

    // Auth guard
    assert_eq!(
        app.http
            .get(format!("{}/api/v1/admin/logs", app.server.base))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    // Parameter validation -> 400 INVALID_REQUEST
    for bad_query in [
        "limit=0",
        "limit=101",
        "limit=abc",
        "view=unknown",
        "before_seq=0",
        "before_seq=-3",
        "before_seq=xyz",
        "after_seq=0",
        "after_seq=-3",
        "after_seq=abc",
        "before_seq=10&after_seq=5",
        "event_type=invalid_kind",
        "correlation_id=req_test&after_seq=1",
        "correlation_id=req_test&before_seq=10",
    ] {
        let (status, err) = app
            .admin(Method::GET, &format!("/api/v1/admin/logs?{bad_query}"))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "query {bad_query}");
        assert_eq!(err["code"], "INVALID_REQUEST", "query {bad_query}");
    }

    let db = Sqlite::open(&app.state.config.data_dir, app.state.config.event_log_limit).unwrap();
    let mk = |account: Option<&str>,
              source: Option<&str>,
              corr: &str,
              trigger: EventTrigger,
              event: Event,
              err: Option<ErrorCode>| {
        let mut ctx = EventContext::new(
            trigger,
            account.map(str::to_owned),
            source.map(str::to_owned),
        );
        ctx.correlation_id = corr.into();
        EventLog::new(ctx, event, Some(12), err)
    };

    let logs_to_insert = [
        mk(
            None,
            None,
            "corr_qr_expired",
            EventTrigger::Qr,
            Event::AccountLogin {
                outcome: LoginOutcome::Expired,
            },
            Some(ErrorCode::UpstreamLoginFailed),
        ),
        mk(
            None,
            None,
            "corr_qr_failed",
            EventTrigger::Qr,
            Event::AccountLogin {
                outcome: LoginOutcome::Failed,
            },
            Some(ErrorCode::UpstreamLoginFailed),
        ),
        mk(
            Some(ACCOUNT),
            None,
            "corr_chain",
            EventTrigger::Admin,
            Event::TokenRefresh {
                reason: RefreshReason::Manual,
                upstream_endpoint: "login/token".into(),
                human_verification: false,
            },
            None,
        ),
        mk(
            Some(ACCOUNT),
            Some(SOURCE),
            "corr_chain",
            EventTrigger::Runtime,
            Event::MusicUrl(Box::new(MusicUrlEvent {
                stage: MusicUrlStage::FetchMusicUrl,
                track_title: Some("Track".into()),
                artist_name: None,
                album_name: None,
                request_hash: "hash-1".into(),
                album_audio_id: None,
                requested_quality: "flac".into(),
                attempted_quality: Some("flac".into()),
                quality_fallback_index: 0,
                retry_count: 0,
                reported_types: None,
                upstream_endpoint: Some("song/url".into()),
                status_code: Some(200),
                human_verification: false,
                upstream_quality: Some("flac".into()),
                upstream_status: Some(1),
                upstream_priv_status: Some(0),
                upstream_fail_process: Some(vec![]),
                upstream_ext_name: Some("flac".into()),
                upstream_file_size: Some(1234),
                upstream_bit_rate: Some(0),
                upstream_time_length: Some(10),
            })),
            None,
        ),
        mk(
            Some(ACCOUNT),
            None,
            "corr_chain",
            EventTrigger::Admin,
            Event::Entitlement {
                action: EntitlementAction::Claim,
                observation: None,
            },
            Some(ErrorCode::UpstreamRequestFailed),
        ),
        mk(
            Some("acc_other"),
            None,
            "corr_sync",
            EventTrigger::Admin,
            Event::Entitlement {
                action: EntitlementAction::Sync,
                observation: None,
            },
            None,
        ),
    ];
    for entry in &logs_to_insert {
        db.append(entry).await.unwrap();
    }

    // Default /logs includes renewal + unowned events + empty strings treated as None + default limit=30
    let (status, all) = app
        .admin(
            Method::GET,
            "/api/v1/admin/logs?account_id=&source_id=&correlation_id=",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all["limit"], 30);
    assert_eq!(all["view"], "all");
    assert!(all["account_id"].is_null());
    assert!(all["source_id"].is_null());
    assert!(all["correlation_id"].is_null());
    assert_eq!(all["has_more"], false);
    assert!(all["next_before_seq"].is_null());
    let items = all["items"].as_array().unwrap();
    assert_eq!(items.len(), 6);
    assert!(
        items
            .iter()
            .all(|i| i["created_at_ms"].as_i64().unwrap() >= 1_000_000_000_000)
    );
    assert!(items.iter().any(|i| i["account_id"].is_null()
        && i["source_id"].is_null()
        && i["detail"]["outcome"] == "expired"));
    assert!(items.iter().any(|i| i["detail"]["action"] == "claim"));

    // Filter by account_id includes both source-bound and account-level events
    let (_, by_acc) = app
        .admin(
            Method::GET,
            &format!("/api/v1/admin/logs?account_id={ACCOUNT}"),
        )
        .await;
    assert_eq!(by_acc["items"].as_array().unwrap().len(), 3);

    // Filter by source_id
    let (_, by_src) = app
        .admin(
            Method::GET,
            &format!("/api/v1/admin/logs?source_id={SOURCE}"),
        )
        .await;
    assert_eq!(by_src["items"].as_array().unwrap().len(), 1);
    assert_eq!(by_src["items"][0]["event_type"], "music_url");

    // Filter by event_type
    let (_, by_type) = app
        .admin(Method::GET, "/api/v1/admin/logs?event_type=account_login")
        .await;
    assert_eq!(by_type["items"].as_array().unwrap().len(), 2);

    // Filter by view=errors
    let (_, errors) = app
        .admin(Method::GET, "/api/v1/admin/logs?view=errors")
        .await;
    let error_items = errors["items"].as_array().unwrap();
    assert_eq!(error_items.len(), 3);
    assert!(error_items.iter().all(|i| i["ok"] == false));

    // Cursor pagination with limit=2 walks all 6 rows without duplicates or gaps
    let mut collected_seqs: Vec<i64> = Vec::new();
    let mut before_seq: Option<i64> = None;
    for page_idx in 0..3 {
        let url = match before_seq {
            Some(seq) => format!("/api/v1/admin/logs?limit=2&before_seq={seq}"),
            None => "/api/v1/admin/logs?limit=2".into(),
        };
        let (_, page) = app.admin(Method::GET, &url).await;
        let page_items = page["items"].as_array().unwrap();
        assert_eq!(page_items.len(), 2);
        for item in page_items {
            collected_seqs.push(item["seq"].as_i64().unwrap());
        }
        if page_idx < 2 {
            assert_eq!(page["has_more"], true);
            let cursor = page["next_before_seq"].as_i64().unwrap();
            assert_eq!(cursor, page_items[1]["seq"].as_i64().unwrap());
            before_seq = Some(cursor);
        } else {
            assert_eq!(page["has_more"], false);
            assert!(page["next_before_seq"].is_null());
        }
    }
    let expected_seqs: Vec<i64> = items.iter().map(|i| i["seq"].as_i64().unwrap()).collect();
    assert_eq!(collected_seqs, expected_seqs);

    // Correlation chain returns events in seq ASC order and next_before_seq is always null
    let (_, chain) = app
        .admin(Method::GET, "/api/v1/admin/logs?correlation_id=corr_chain")
        .await;
    let chain_items = chain["items"].as_array().unwrap();
    assert_eq!(chain_items.len(), 3);
    assert_eq!(chain["has_more"], false);
    assert!(chain["next_before_seq"].is_null());
    assert_eq!(chain_items[0]["event_type"], "token_refresh");
    assert_eq!(chain_items[1]["event_type"], "music_url");
    assert_eq!(chain_items[2]["event_type"], "entitlement");
    assert!(
        chain_items
            .windows(2)
            .all(|w| w[0]["seq"].as_i64().unwrap() < w[1]["seq"].as_i64().unwrap())
    );

    let (_, chain_truncated) = app
        .admin(
            Method::GET,
            "/api/v1/admin/logs?correlation_id=corr_chain&limit=2",
        )
        .await;
    assert_eq!(chain_truncated["items"].as_array().unwrap().len(), 2);
    assert_eq!(chain_truncated["has_more"], true);
    assert!(chain_truncated["next_before_seq"].is_null());

    // Incremental polling with after_seq
    let max_seq = expected_seqs[0];
    let (status, empty_after) = app
        .admin(
            Method::GET,
            &format!("/api/v1/admin/logs?after_seq={max_seq}"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty_after["items"].as_array().unwrap().len(), 0);
    assert_eq!(empty_after["has_more"], false);
    assert!(empty_after["next_before_seq"].is_null());

    // Pivot at expected_seqs[3] (older) so 3 newer rows exist (expected_seqs[0..3])
    let pivot_seq = expected_seqs[3];
    let (status, after_limited) = app
        .admin(
            Method::GET,
            &format!("/api/v1/admin/logs?after_seq={pivot_seq}&limit=2"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let after_limited_items = after_limited["items"].as_array().unwrap();
    assert_eq!(after_limited_items.len(), 2);
    assert_eq!(after_limited["has_more"], true);
    assert!(after_limited["next_before_seq"].is_null());
    assert!(
        after_limited_items
            .iter()
            .all(|item| item["seq"].as_i64().unwrap() > pivot_seq)
    );
    assert_eq!(
        after_limited_items
            .iter()
            .map(|item| item["seq"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![expected_seqs[0], expected_seqs[1]]
    );

    let (status, after_all) = app
        .admin(
            Method::GET,
            &format!("/api/v1/admin/logs?after_seq={pivot_seq}&limit=10"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let after_all_items = after_all["items"].as_array().unwrap();
    assert_eq!(
        after_all_items
            .iter()
            .map(|item| item["seq"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![expected_seqs[0], expected_seqs[1], expected_seqs[2]]
    );
    assert_eq!(after_all["has_more"], false);
    assert!(after_all["next_before_seq"].is_null());
}
