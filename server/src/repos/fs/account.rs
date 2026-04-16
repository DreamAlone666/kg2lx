use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::account::ProviderAccount;
use crate::domain::provider::ProviderKind;
use crate::error::AppError;
use crate::repos::account::AccountRepo;

pub struct FsAccountRepo {
    dir: PathBuf,
}

impl FsAccountRepo {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("accounts"),
        }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", id))
    }
}

#[async_trait]
impl AccountRepo for FsAccountRepo {
    async fn find_by_id(&self, account_id: &str) -> Result<Option<ProviderAccount>, AppError> {
        let p = self.path(account_id);
        if !p.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&p).await?;
        let account: ProviderAccount = serde_json::from_str(&data)
            .map_err(|e| AppError::internal(format!("parse account: {}", e)))?;
        Ok(Some(account))
    }

    async fn find_by_provider_userid(
        &self,
        provider: ProviderKind,
        upstream_userid: &str,
    ) -> Result<Option<ProviderAccount>, AppError> {
        let _ = provider;
        if !self.dir.exists() {
            return Ok(None);
        }
        let mut entries = fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let data = fs::read_to_string(&path).await?;
            let account: ProviderAccount = match serde_json::from_str(&data) {
                Ok(a) => a,
                Err(_) => continue,
            };
            if account.upstream_userid == upstream_userid {
                return Ok(Some(account));
            }
        }
        Ok(None)
    }

    async fn upsert(&self, account: &ProviderAccount) -> Result<(), AppError> {
        fs::create_dir_all(&self.dir).await?;
        let data = serde_json::to_string_pretty(account)
            .map_err(|e| AppError::internal(format!("serialize account: {}", e)))?;
        fs::write(self.path(&account.account_id), data).await?;
        Ok(())
    }
}
