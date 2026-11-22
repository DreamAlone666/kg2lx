use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::{OwnedMutexGuard, watch};

use crate::error::AppError;

type Outcome<T> = Option<Result<T, AppError>>;
type InFlight<T> = HashMap<String, watch::Receiver<Outcome<T>>>;

pub struct SingleFlight<T> {
    in_flight: Arc<Mutex<InFlight<T>>>,
}

impl<T> Default for SingleFlight<T> {
    fn default() -> Self {
        Self {
            in_flight: Arc::default(),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> SingleFlight<T> {
    pub async fn run(
        &self,
        key: String,
        work: impl Future<Output = Result<T, AppError>> + Send + 'static,
    ) -> Result<T, AppError> {
        let mut receiver = {
            let mut entries = self.in_flight.lock().unwrap();
            if let Some(receiver) = entries.get(&key).filter(|rx| rx.has_changed().is_ok()) {
                receiver.clone()
            } else {
                let (sender, receiver) = watch::channel(None);
                entries.insert(key.clone(), receiver.clone());
                let in_flight = self.in_flight.clone();
                // Other callers can still use the result if the first HTTP request disconnects.
                tokio::spawn(async move {
                    let result = work.await;
                    let mut entries = in_flight.lock().unwrap();
                    sender.send_replace(Some(result));
                    entries.remove(&key);
                });
                receiver
            }
        };
        receiver
            .wait_for(Option::is_some)
            .await
            .map_err(|_| AppError::internal("account operation stopped"))?
            .as_ref()
            .expect("waited for a result")
            .clone()
    }
}

/// Serialize operations on one identity or QR session without keeping finished keys alive.
#[derive(Default)]
pub struct KeyedLocks {
    keys: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
}

impl KeyedLocks {
    pub async fn lock(&self, key: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut keys = self.keys.lock().unwrap();
            if let Some(lock) = keys.get(key).and_then(Weak::upgrade) {
                lock
            } else {
                keys.retain(|_, lock| lock.strong_count() > 0);
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                keys.insert(key.into(), Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }
}
