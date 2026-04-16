use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::runtime_log::RuntimeLog;
use crate::error::AppError;
use crate::repos::runtime_log::RuntimeLogRepo;

pub struct FsRuntimeLogRepo {
    dir: PathBuf,
}

impl FsRuntimeLogRepo {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("logs"),
        }
    }
}

#[async_trait]
impl RuntimeLogRepo for FsRuntimeLogRepo {
    async fn append(&self, log: &RuntimeLog) -> Result<(), AppError> {
        fs::create_dir_all(&self.dir).await?;
        let now = chrono::Local::now();
        let filename = format!("runtime-{}.jsonl", now.format("%Y-%m-%d"));
        let path = self.dir.join(filename);
        let line = serde_json::to_string(log)
            .map_err(|e| AppError::internal(format!("serialize runtime log: {}", e)))?;
        let line = format!("{}\n", line);
        use tokio::io::AsyncWriteExt;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        f.write_all(line.as_bytes()).await?;
        Ok(())
    }
}
