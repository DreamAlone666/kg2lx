use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::runtime_log::{ListRuntimeLogsQuery, RuntimeLog, RuntimeLogView};
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

    async fn list_by_source(&self, query: &ListRuntimeLogsQuery) -> Result<Vec<RuntimeLog>, AppError> {
        let mut entries = match fs::read_dir(&self.dir).await {
            Ok(rd) => {
                let mut v = Vec::new();
                let mut stream = rd;
                while let Some(entry) = stream.next_entry().await? {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("runtime-") && name.ends_with(".jsonl") {
                        v.push(entry.path());
                    }
                }
                v
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };

        entries.sort_by(|a, b| b.cmp(a));

        let mut items: Vec<RuntimeLog> = Vec::new();

        for path in &entries {
            let content = match fs::read_to_string(path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();

            for line in lines.iter().rev() {
                let log: RuntimeLog = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if log.source_id != query.source_id {
                    continue;
                }

                if query.view == RuntimeLogView::Errors && log.ok {
                    continue;
                }

                items.push(log);

                if items.len() >= query.limit {
                    return Ok(items);
                }
            }
        }

        Ok(items)
    }
}
