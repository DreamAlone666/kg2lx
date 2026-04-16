use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::source::Source;
use crate::error::AppError;
use crate::repos::source::SourceRepo;

pub struct FsSourceRepo {
    dir: PathBuf,
}

impl FsSourceRepo {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("sources"),
        }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", id))
    }
}

#[async_trait]
impl SourceRepo for FsSourceRepo {
    async fn find_by_id(&self, source_id: &str) -> Result<Option<Source>, AppError> {
        let p = self.path(source_id);
        if !p.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&p).await?;
        let source: Source = serde_json::from_str(&data)
            .map_err(|e| AppError::internal(format!("parse source: {}", e)))?;
        Ok(Some(source))
    }

    async fn find_by_script_token(&self, script_token: &str) -> Result<Option<Source>, AppError> {
        self.find_by_index("script_token", script_token).await
    }

    async fn find_by_runtime_token(
        &self,
        runtime_token: &str,
    ) -> Result<Option<Source>, AppError> {
        self.find_by_index("runtime_token", runtime_token).await
    }

    async fn find_by_account_id(&self, account_id: &str) -> Result<Option<Source>, AppError> {
        self.find_by_index("account_id", account_id).await
    }

    async fn upsert(&self, source: &Source) -> Result<(), AppError> {
        fs::create_dir_all(&self.dir).await?;
        let data = serde_json::to_string_pretty(source)
            .map_err(|e| AppError::internal(format!("serialize source: {}", e)))?;
        fs::write(self.path(&source.source_id), data).await?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<Source>, AppError> {
        if !self.dir.exists() {
            return Ok(vec![]);
        }
        let mut result = Vec::new();
        let mut entries = fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let data = fs::read_to_string(&path).await?;
            let source: Source = match serde_json::from_str(&data) {
                Ok(s) => s,
                Err(_) => continue,
            };
            result.push(source);
        }
        Ok(result)
    }
}

impl FsSourceRepo {
    async fn find_by_index(&self, field: &str, value: &str) -> Result<Option<Source>, AppError> {
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
            let source: Source = match serde_json::from_str(&data) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let matches = match field {
                "script_token" => source.script_token == value,
                "runtime_token" => source.runtime_token == value,
                "account_id" => source.account_id == value,
                _ => false,
            };
            if matches {
                return Ok(Some(source));
            }
        }
        Ok(None)
    }
}
