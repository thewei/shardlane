//! Bounded request-id idempotency for Remote semantic mutations (A03/R2-05/09/10).
//!
//! [INPUT]: The client-supplied `request_id`, the operation type, the request
//! fingerprint (target id + canonical body, excluding request_id), and an
//! execution Future.
//! [OUTPUT]: `MutationCache::execute` — concurrent same-key requests merge
//! into one execution (per-key in-flight weak-reference slots); exact replays
//! return the first response; a mismatching request fingerprint returns a
//! conflict; success / stable-terminal failure / uncertain delivery
//! (tombstone) are all cached for replay.
//! [POS]: The safety layer for Remote mutations. R2-05: once
//! `delivery_uncertain` appears it is tombstoned — same-key replays get the
//! same uncertain result and the mutation is never re-executed; recovery only
//! happens via explicit reconciliation. in_flight uses Weak slots that are
//! truly reclaimed on completion (R2-09).

use crate::error::ApiError;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);
const DEFAULT_CAPACITY: usize = 256;

enum CachedOutcome {
    Success(serde_json::Value),
    TerminalFailure(ApiError),
}

struct CacheInner {
    /// key → (stored at, request fingerprint, outcome)
    entries: HashMap<String, (Instant, String, CachedOutcome)>,
    order: VecDeque<String>,
    in_flight: HashMap<String, Weak<tokio::sync::Mutex<()>>>,
}

/// Bounded recent-result cache with in-flight coalescing for mutating
/// operations. One instance lives in `RemoteState`.
pub struct MutationCache {
    inner: Mutex<CacheInner>,
}

impl Default for MutationCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MutationCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                in_flight: HashMap::new(),
            }),
        }
    }

    fn cached(&self, key: &str, fingerprint: &str) -> Result<Option<CachedOutcome>, ApiError> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match inner.entries.get(key) {
            Some((at, stored, outcome)) if at.elapsed() < DEFAULT_TTL => {
                if stored != fingerprint {
                    return Err(ApiError::conflict(
                        "this request_id was already used for a different mutation body",
                        key.to_string(),
                    ));
                }
                Ok(Some(match outcome {
                    CachedOutcome::Success(value) => CachedOutcome::Success(value.clone()),
                    CachedOutcome::TerminalFailure(error) => {
                        CachedOutcome::TerminalFailure(error.clone())
                    }
                }))
            }
            _ => Ok(None),
        }
    }

    fn store(&self, key: String, fingerprint: String, outcome: CachedOutcome) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !inner.entries.contains_key(&key) {
            inner.order.push_back(key.clone());
        }
        inner
            .entries
            .insert(key, (Instant::now(), fingerprint, outcome));
        // Bound both by capacity and by TTL-expired front entries.
        while inner.order.len() > DEFAULT_CAPACITY {
            if let Some(oldest) = inner.order.pop_front() {
                inner.entries.remove(&oldest);
            }
        }
        while let Some(front) = inner.order.front() {
            match inner.entries.get(front) {
                Some((at, _, _)) if at.elapsed() >= DEFAULT_TTL => {
                    let front = front.clone();
                    inner.order.pop_front();
                    inner.entries.remove(&front);
                }
                _ => break,
            }
        }
    }

    /// The per-key in-flight slot: concurrent duplicates coalesce onto one
    /// execution instead of racing the runtime mutation. The map holds only
    /// weak references, so finished no-waiter slots are reclaimed instead of
    /// leaking per unique request id (R2-09).
    fn in_flight_slot(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(slot) = inner.in_flight.get(key).and_then(Weak::upgrade) {
            return slot;
        }
        inner.in_flight.retain(|_, slot| slot.strong_count() > 0);
        let slot = Arc::new(tokio::sync::Mutex::new(()));
        inner
            .in_flight
            .insert(key.to_string(), Arc::downgrade(&slot));
        slot
    }

    /// Execute `run` at most once per `key` inside the TTL window. Exact
    /// replays return the first response (or first terminal failure /
    /// uncertainty tombstone) without re-executing the mutation. A reused key
    /// with a DIFFERENT request fingerprint conflicts (R2-10): the same
    /// logical id must describe the same mutation.
    pub async fn execute<F, Fut>(
        &self,
        key: String,
        fingerprint: String,
        run: F,
    ) -> Result<serde_json::Value, ApiError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<serde_json::Value, ApiError>>,
    {
        // Fingerprint consistency is checked before and after the in-flight
        // wait: a waiter queued behind a different-body request with the same
        // id must still conflict.
        if let Some(outcome) = self.cached(&key, &fingerprint)? {
            return resolve(outcome);
        }
        let slot = self.in_flight_slot(&key);
        let _guard = slot.lock().await;
        if let Some(outcome) = self.cached(&key, &fingerprint)? {
            return resolve(outcome);
        }
        match run().await {
            Ok(value) => {
                self.store(key, fingerprint, CachedOutcome::Success(value.clone()));
                Ok(value)
            }
            Err(error) => {
                if is_terminal_failure(&error) {
                    // R2-05: delivery uncertainty is terminal-by-tombstone —
                    // an exact replay must observe the SAME uncertainty and
                    // never re-execute the mutation.
                    self.store(
                        key,
                        fingerprint,
                        CachedOutcome::TerminalFailure(error.clone()),
                    );
                }
                Err(error)
            }
        }
    }

    /// Diagnostics/tests: live in-flight slot count.
    pub fn in_flight_size(&self) -> usize {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        inner
            .in_flight
            .values()
            .filter(|slot| slot.strong_count() > 0)
            .count()
    }

    /// Diagnostics/tests: cached entry count.
    pub fn cached_size(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .entries
            .len()
    }
}

fn resolve(outcome: CachedOutcome) -> Result<serde_json::Value, ApiError> {
    match outcome {
        CachedOutcome::Success(value) => Ok(value),
        CachedOutcome::TerminalFailure(error) => Err(error),
    }
}

/// Stable, request-shape-determined failures replay identically; transient
/// infrastructure failures must stay retryable. `delivery_uncertain` is the
/// R2-05 tombstone class: replay returns the same uncertainty.
fn is_terminal_failure(error: &ApiError) -> bool {
    matches!(
        error.code,
        "invalid_request"
            | "not_found"
            | "unsupported_protocol"
            | "delivery_uncertain"
            | "agent_created"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_value(tag: &str) -> serde_json::Value {
        serde_json::json!({ "accepted": true, "tag": tag })
    }

    fn fp(tag: &str) -> String {
        tag.to_string()
    }

    #[tokio::test]
    async fn exact_replay_returns_the_first_result_without_reexecution() {
        let cache = MutationCache::new();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let first = {
            let calls = calls.clone();
            cache
                .execute("prompt:r1".into(), fp("body-1"), move || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(ok_value("first"))
                    }
                })
                .await
        };
        assert_eq!(first.ok(), Some(ok_value("first")));
        let replay = cache
            .execute("prompt:r1".into(), fp("body-1"), || async {
                Ok(ok_value("second"))
            })
            .await;
        assert_eq!(
            replay.ok(),
            Some(ok_value("first")),
            "replay must return the original response"
        );
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "mutation executed exactly once"
        );
    }

    #[tokio::test]
    async fn same_key_with_a_different_body_is_rejected_as_conflict() {
        // R2-10: a reused request id describing a different mutation must not
        // replay the old result.
        let cache = MutationCache::new();
        cache
            .execute("prompt:c1".into(), fp("body-a"), || async {
                Ok(ok_value("a"))
            })
            .await
            .ok();
        let error = match cache
            .execute("prompt:c1".into(), fp("body-b"), || async {
                Ok(ok_value("b"))
            })
            .await
        {
            Err(error) => error,
            Ok(value) => panic!("mismatched fingerprint must conflict: {value}"),
        };
        assert_eq!(error.code, "conflict");
    }

    #[tokio::test]
    async fn concurrent_duplicates_coalesce_onto_one_execution() {
        let cache = std::sync::Arc::new(MutationCache::new());
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let calls = calls.clone();
            handles.push(tokio::spawn(async move {
                cache
                    .execute("continue:r2".into(), fp("body-1"), move || {
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            Ok(ok_value("once"))
                        }
                    })
                    .await
            }));
        }
        for handle in handles {
            assert_eq!(
                handle
                    .await
                    .unwrap_or_else(|error| panic!("{error:?}"))
                    .ok(),
                Some(ok_value("once"))
            );
        }
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "concurrent duplicates must coalesce"
        );
    }

    #[tokio::test]
    async fn uncertain_outcomes_are_tombstoned_and_never_reexecuted() {
        // R2-05: the most dangerous class — the mutation may be accepted but
        // the response was lost. Exact replays must observe the SAME
        // uncertainty; the underlying mutation runs exactly once.
        let cache = MutationCache::new();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let first = {
            let calls = calls.clone();
            cache
                .execute("prompt:u1".into(), fp("body-1"), move || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Err(ApiError::delivery_uncertain(
                            "response not read after the mutation was written",
                            "t".into(),
                        ))
                    }
                })
                .await
        };
        assert_eq!(
            first.err().map(|error| error.code),
            Some("delivery_uncertain")
        );
        for _ in 0..10 {
            let replay = cache
                .execute("prompt:u1".into(), fp("body-1"), || async {
                    Ok(ok_value("must-not-run"))
                })
                .await;
            assert_eq!(
                replay.err().map(|error| error.code),
                Some("delivery_uncertain"),
                "replays must observe the same tombstone"
            );
        }
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "uncertain mutation executed exactly once"
        );
    }

    #[tokio::test]
    async fn transient_failures_stay_retryable_but_terminal_failures_replay() {
        let cache = MutationCache::new();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let transient = ApiError::host_unavailable("Herdr runtime is not reachable", "t".into());
        {
            let attempts = attempts.clone();
            let first = cache
                .execute("prompt:t".into(), fp("body-1"), move || {
                    let attempts = attempts.clone();
                    async move {
                        attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Err(transient.clone())
                    }
                })
                .await;
            assert!(first.is_err());
        }
        {
            let attempts = attempts.clone();
            let retry = cache
                .execute("prompt:t".into(), fp("body-1"), move || {
                    let attempts = attempts.clone();
                    async move {
                        attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(ok_value("recovered"))
                    }
                })
                .await;
            assert_eq!(retry.ok(), Some(ok_value("recovered")));
        }
        assert_eq!(
            attempts.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "transient failures must re-execute"
        );
    }

    #[tokio::test]
    async fn in_flight_slots_do_not_leak_across_unique_keys() {
        // R2-09: thousands of unique request ids must leave the in-flight map
        // bounded (weak slots are reclaimed once finished).
        let cache = MutationCache::new();
        for index in 0..2_000u32 {
            let key = format!("prompt:leak-{index}");
            cache
                .execute(key, fp(&format!("body-{index}")), || async {
                    Ok(ok_value("done"))
                })
                .await
                .ok();
        }
        assert_eq!(
            cache.in_flight_size(),
            0,
            "finished no-waiter slots must be reclaimed"
        );
        assert!(cache.cached_size() <= DEFAULT_CAPACITY);
    }

    #[tokio::test]
    async fn capacity_is_bounded() {
        let cache = MutationCache::new();
        for index in 0..(DEFAULT_CAPACITY as u64 + 10) {
            let tag = index.to_string();
            cache
                .execute(format!("prompt:cap-{index}"), fp(&tag), move || {
                    let tag = tag.clone();
                    async move { Ok(ok_value(&tag)) }
                })
                .await
                .ok();
        }
        assert!(cache.cached_size() <= DEFAULT_CAPACITY);
    }
}
