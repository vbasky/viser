use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::mpsc;
use tracing::debug;

/// Non-blocking progress sender. Logs when updates are dropped due to a full channel.
pub struct ProgressSender<T> {
    tx: Option<mpsc::Sender<T>>,
    dropped: AtomicI64,
}

impl<T> ProgressSender<T> {
    pub fn new(tx: Option<mpsc::Sender<T>>) -> Self {
        Self { tx, dropped: AtomicI64::new(0) }
    }

    /// Attempts to send a progress update. Non-blocking.
    pub fn send(&self, value: T) {
        let Some(ref tx) = self.tx else { return };
        match tx.try_send(value) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                let count = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if count == 1 || count % 100 == 0 {
                    debug!("progress update dropped (channel full), total_dropped={count}");
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }

    pub fn dropped(&self) -> i64 {
        self.dropped.load(Ordering::Relaxed)
    }
}
