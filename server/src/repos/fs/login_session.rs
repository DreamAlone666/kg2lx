use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::login_session::LoginSession;
use crate::error::AppError;
use crate::repos::login_session::LoginSessionRepo;

pub struct FsLoginSessionRepo {
    dir: PathBuf,
}

impl FsLoginSessionRepo {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("login_sessions"),
        }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", id))
    }
}

#[async_trait]
impl LoginSessionRepo for FsLoginSessionRepo {
    async fn find_by_id(&self, session_id: &str) -> Result<Option<LoginSession>, AppError> {
        let p = self.path(session_id);
        if !p.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&p).await?;
        let session: LoginSession = serde_json::from_str(&data)
            .map_err(|e| AppError::internal(format!("parse login session: {}", e)))?;
        Ok(Some(session))
    }

    async fn save(&self, session: &LoginSession) -> Result<(), AppError> {
        fs::create_dir_all(&self.dir).await?;
        let data = serde_json::to_string_pretty(session)
            .map_err(|e| AppError::internal(format!("serialize login session: {}", e)))?;
        fs::write(self.path(&session.session_id), data).await?;
        Ok(())
    }
}
