use std::time::{Duration, Instant};

/// Tiny single-value TTL cache (one entry per kind).
pub(super) struct MutexCache<T> {
    value: std::sync::Mutex<Option<(T, Instant)>>,
}

impl<T: Clone> MutexCache<T> {
    pub(super) fn new() -> Self {
        Self {
            value: std::sync::Mutex::new(None),
        }
    }

    pub(super) fn get(&self, ttl: Duration) -> Option<T> {
        let guard = self.value.lock().ok()?;
        let (value, at) = guard.as_ref()?;
        (at.elapsed() < ttl).then(|| value.clone())
    }

    pub(super) fn set(&self, value: T) {
        if let Ok(mut guard) = self.value.lock() {
            *guard = Some((value, Instant::now()));
        }
    }

    pub(super) fn invalidate(&self) {
        if let Ok(mut guard) = self.value.lock() {
            *guard = None;
        }
    }
}
