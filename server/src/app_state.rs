use std::sync::Arc;

use crate::config::Config;
use crate::domain::account::now_ts;
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::event_log::EventLogRepo;
use crate::repos::source::SourceRepo;
use crate::repos::sqlite::Sqlite;
use crate::services::auth::AccountAuth;
use crate::services::entitlement::EntitlementService;
use crate::services::kugou_lite_client::KugouLiteClient;
use crate::services::login::LoginService;
use crate::services::runtime::RuntimeService;
use crate::services::script::ScriptService;
use crate::services::source::SourceService;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub auth: Arc<AccountAuth>,
    pub entitlement: Arc<EntitlementService>,
    pub login: Arc<LoginService>,
    pub source: Arc<SourceService>,
    pub runtime: Arc<RuntimeService>,
    pub script: Arc<ScriptService>,
}

impl AppState {
    pub fn new(config: Config) -> Result<Self, AppError> {
        Self::with_entitlement_clock(config, now_ts)
    }

    pub(crate) fn with_entitlement_clock(
        config: Config,
        now: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Result<Self, AppError> {
        let storage = Arc::new(Sqlite::open(&config.data_dir, config.event_log_limit)?);
        let config = Arc::new(config);
        let accounts: Arc<dyn AccountRepo> = storage.clone();
        let sources: Arc<dyn SourceRepo> = storage.clone();
        let logs: Arc<dyn EventLogRepo> = storage.clone();
        let client = Arc::new(KugouLiteClient::new(&config));
        let auth = Arc::new(AccountAuth::new(
            &config,
            client.clone(),
            accounts.clone(),
            logs.clone(),
        ));
        let entitlement = Arc::new(EntitlementService::new(
            client.clone(),
            accounts.clone(),
            storage.clone(),
            storage.clone(),
            logs.clone(),
            auth.admin_actions.clone(),
            now,
        ));
        let login = Arc::new(LoginService::new(
            config.clone(),
            client.clone(),
            auth.clone(),
            logs.clone(),
            storage.clone(),
            accounts.clone(),
            sources.clone(),
        ));
        let source = Arc::new(SourceService::new(
            config.clone(),
            storage,
            accounts.clone(),
            sources.clone(),
            logs.clone(),
        ));
        let runtime = Arc::new(RuntimeService::new(
            auth.clone(),
            client,
            sources.clone(),
            accounts.clone(),
            logs,
            Some(entitlement.clone()),
        ));
        let script = Arc::new(ScriptService::new(config.clone(), sources, accounts));
        Ok(Self {
            config,
            auth,
            entitlement,
            login,
            source,
            runtime,
            script,
        })
    }
}
