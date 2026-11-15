use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// Process-wide repository snapshot with single-flight background refreshes.
pub struct RepositoryCache {
    snapshot: MutexCache<String>,
    refreshing: AtomicBool,
}

impl RepositoryCache {
    pub fn new() -> Self {
        Self {
            snapshot: MutexCache::new(),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn get(&self, ttl: Duration) -> Option<String> {
        self.snapshot.get(ttl)
    }

    pub fn set(&self, repos_json: String) {
        self.snapshot.set(repos_json);
    }

    pub fn begin_refresh(&self) -> bool {
        !self.refreshing.swap(true, Ordering::AcqRel)
    }

    pub fn finish_refresh(&self) {
        self.refreshing.store(false, Ordering::Release);
    }
}

impl Default for RepositoryCache {
    fn default() -> Self {
        Self::new()
    }
}

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
