use std::sync::Arc;

use crate::config::Config;
use crate::repos::account::AccountRepo;
use crate::repos::login_session::LoginSessionRepo;
use crate::repos::runtime_log::RuntimeLogRepo;
use crate::repos::source::SourceRepo;
use crate::services::kugou_lite_client::KugouLiteClient;
use crate::services::login::LoginService;
use crate::services::runtime::RuntimeService;
use crate::services::script::ScriptService;
use crate::services::source::SourceService;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub login: Arc<LoginService>,
    pub source: Arc<SourceService>,
    pub runtime: Arc<RuntimeService>,
    pub script: Arc<ScriptService>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let config = Arc::new(config);
        let data_dir = std::path::Path::new(&config.data_dir);

        let account_repo: Arc<dyn AccountRepo> =
            Arc::new(crate::repos::fs::account::FsAccountRepo::new(data_dir));
        let source_repo: Arc<dyn SourceRepo> =
            Arc::new(crate::repos::fs::source::FsSourceRepo::new(data_dir));
        let session_repo: Arc<dyn LoginSessionRepo> = Arc::new(
            crate::repos::fs::login_session::FsLoginSessionRepo::new(data_dir),
        );
        let log_repo: Arc<dyn RuntimeLogRepo> = Arc::new(
            crate::repos::fs::runtime_log::FsRuntimeLogRepo::new(data_dir),
        );

        let client = Arc::new(KugouLiteClient::new(&config));

        let login_service = Arc::new(LoginService::new(
            config.clone(),
            client.clone(),
            session_repo,
            account_repo.clone(),
            source_repo.clone(),
        ));

        let source_service = Arc::new(SourceService::new(
            config.clone(),
            client.clone(),
            account_repo.clone(),
            source_repo.clone(),
        ));

        let runtime_service = Arc::new(RuntimeService::new(
            config.clone(),
            client.clone(),
            source_repo.clone(),
            account_repo,
            log_repo,
        ));

        let script_service = Arc::new(ScriptService::new(config.clone(), source_repo));

        Self {
            config,
            login: login_service,
            source: source_service,
            runtime: runtime_service,
            script: script_service,
        }
    }
}
