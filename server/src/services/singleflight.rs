use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::{OwnedMutexGuard, watch};

use crate::error::AppError;

type Outcome<T> = Option<Result<T, AppError>>;
type InFlight<T> = HashMap<String, watch::Receiver<Outcome<T>>>;

struct FlightGuard<T> {
    entries: Arc<Mutex<InFlight<T>>>,
    key: String,
    receiver: watch::Receiver<Outcome<T>>,
}

impl<T> Drop for FlightGuard<T> {
    fn drop(&mut self) {
        let mut entries = self.entries.lock().unwrap();
        if entries
            .get(&self.key)
            .is_some_and(|entry| entry.same_channel(&self.receiver))
        {
            entries.remove(&self.key);
        }
    }
}

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
        self.start(key, work)
            .wait_for(Option::is_some)
            .await
            .map_err(|_| AppError::internal("account operation stopped"))?
            .as_ref()
            .expect("waited for a result")
            .clone()
    }

    pub fn start(
        &self,
        key: String,
        work: impl Future<Output = Result<T, AppError>> + Send + 'static,
    ) -> watch::Receiver<Outcome<T>> {
        {
            let mut entries = self.in_flight.lock().unwrap();
            if let Some(receiver) = entries.get(&key).filter(|rx| rx.has_changed().is_ok()) {
                receiver.clone()
            } else {
                let (sender, receiver) = watch::channel(None);
                entries.insert(key.clone(), receiver.clone());
                let guard = FlightGuard {
                    entries: self.in_flight.clone(),
                    key,
                    receiver: receiver.clone(),
                };
                // Other callers can still use the result if the first HTTP request disconnects.
                tokio::spawn(async move {
                    let result = work.await;
                    sender.send_replace(Some(result));
                    drop(guard);
                });
                receiver
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn removes_the_entry_after_a_panicking_operation() {
        let flights = SingleFlight::<()>::default();
        assert!(
            flights
                .run("account".into(), async { panic!("fixture panic") })
                .await
                .is_err()
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !flights.in_flight.lock().unwrap().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        flights
            .run("account".into(), async { Ok(()) })
            .await
            .unwrap();
    }
}
