// SPDX-License-Identifier: AGPL-3.0-only
//! Thread-safe JIT compilation cache for the progressive trust model.
//!
//! Caches compiled kernels keyed by WGSL source hash, avoiding redundant
//! compilation for repeated shader executions. Supports re-validation by
//! tracking execution counts per entry.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use coral_reef_cpu::types::ExecuteCpuRequest;

use crate::error::JitError;
use crate::translate::CompiledKernel;

/// FNV-style hash of the WGSL source to use as cache key.
fn shader_cache_key(request: &ExecuteCpuRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    request.wgsl_source.hash(&mut hasher);
    if let Some(ep) = &request.entry_point {
        ep.hash(&mut hasher);
    }
    hasher.finish()
}

/// Metadata about a cached JIT compilation.
struct CacheEntry {
    kernel: Arc<CompiledKernel>,
    /// Number of times this entry has been executed via JIT.
    execution_count: u64,
    /// Whether the kernel has been validated against the interpreter.
    validated: bool,
}

/// Configuration for the progressive trust re-validation policy.
#[derive(Debug, Clone, Copy)]
pub struct RevalidationPolicy {
    /// Re-validate after this many JIT executions (0 = never re-validate).
    pub revalidate_every_n: u64,
}

impl Default for RevalidationPolicy {
    fn default() -> Self {
        Self {
            revalidate_every_n: 1000,
        }
    }
}

/// Thread-safe JIT compilation cache.
pub struct JitCache {
    entries: Mutex<HashMap<u64, CacheEntry>>,
    policy: RevalidationPolicy,
}

impl JitCache {
    /// Create a new empty cache with the default re-validation policy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            policy: RevalidationPolicy::default(),
        }
    }

    /// Create a new cache with a custom re-validation policy.
    #[must_use]
    pub fn with_policy(policy: RevalidationPolicy) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            policy,
        }
    }

    /// Look up a cached kernel for the given request.
    ///
    /// Returns `(kernel, needs_revalidation)` — the caller should re-validate
    /// when the second element is `true`.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[expect(clippy::significant_drop_tightening, reason = "MutexGuard must live while entry is borrowed")]
    pub fn get(
        &self,
        request: &ExecuteCpuRequest,
    ) -> Option<(Arc<CompiledKernel>, bool)> {
        let key = shader_cache_key(request);
        let mut entries = self.entries.lock().expect("cache lock poisoned");
        let entry = entries.get_mut(&key)?;
        entry.execution_count += 1;

        let needs_revalidation = self.policy.revalidate_every_n > 0
            && entry.validated
            && entry.execution_count % self.policy.revalidate_every_n == 0;

        Some((Arc::clone(&entry.kernel), needs_revalidation))
    }

    /// Insert a compiled kernel into the cache.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn insert(
        &self,
        request: &ExecuteCpuRequest,
        kernel: CompiledKernel,
        validated: bool,
    ) {
        self.insert_arc(request, Arc::new(kernel), validated);
    }

    /// Insert a pre-wrapped `Arc<CompiledKernel>` into the cache.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn insert_arc(
        &self,
        request: &ExecuteCpuRequest,
        kernel: Arc<CompiledKernel>,
        validated: bool,
    ) {
        let key = shader_cache_key(request);
        self.entries.lock().expect("cache lock poisoned").insert(
            key,
            CacheEntry {
                kernel,
                execution_count: 0,
                validated,
            },
        );
    }

    /// Mark a cached entry as validated.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn mark_validated(&self, request: &ExecuteCpuRequest) {
        let key = shader_cache_key(request);
        if let Some(entry) = self
            .entries
            .lock()
            .expect("cache lock poisoned")
            .get_mut(&key)
        {
            entry.validated = true;
        }
    }

    /// Invalidate a cache entry (e.g., after failed re-validation).
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn invalidate(&self, request: &ExecuteCpuRequest) {
        let key = shader_cache_key(request);
        self.entries
            .lock()
            .expect("cache lock poisoned")
            .remove(&key);
    }

    /// Number of cached entries.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().expect("cache lock poisoned").len()
    }

    /// Whether the cache is empty.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().expect("cache lock poisoned").is_empty()
    }
}

impl Default for JitCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile a shader and cache the result, or retrieve from cache.
///
/// Returns `(kernel, cache_hit, needs_revalidation)`.
///
/// # Errors
///
/// Returns [`JitError`] if compilation fails.
pub fn compile_cached(
    cache: &JitCache,
    request: &ExecuteCpuRequest,
) -> Result<(Arc<CompiledKernel>, bool, bool), JitError> {
    if let Some((kernel, needs_revalidation)) = cache.get(request) {
        return Ok((kernel, true, needs_revalidation));
    }

    let kernel = Arc::new(crate::compile_to_kernel(request)?);
    cache.insert_arc(request, Arc::clone(&kernel), false);

    Ok((kernel, false, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_request() -> ExecuteCpuRequest {
        ExecuteCpuRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: coral_reef_cpu::types::ExecutionStrategy::Jit,
        }
    }

    #[test]
    fn cache_miss_then_hit() {
        let cache = JitCache::new();
        let req = test_request();
        assert!(cache.get(&req).is_none());

        let result = compile_cached(&cache, &req);
        assert!(result.is_ok());
        let (_, cache_hit, _) = result.expect("compile");
        assert!(!cache_hit);

        let (_, cache_hit, _) = compile_cached(&cache, &req).expect("cached");
        assert!(cache_hit);
    }

    #[test]
    fn revalidation_trigger() {
        let policy = RevalidationPolicy {
            revalidate_every_n: 3,
        };
        let cache = JitCache::with_policy(policy);
        let req = test_request();

        compile_cached(&cache, &req).expect("compile");
        cache.mark_validated(&req);

        for i in 1..=6 {
            let (_, _, needs_reval) = compile_cached(&cache, &req).expect("cached");
            if i % 3 == 0 {
                assert!(needs_reval, "should trigger revalidation at exec #{i}");
            } else {
                assert!(
                    !needs_reval,
                    "should NOT trigger revalidation at exec #{i}"
                );
            }
        }
    }

    #[test]
    fn invalidate_removes_entry() {
        let cache = JitCache::new();
        let req = test_request();
        compile_cached(&cache, &req).expect("compile");
        assert_eq!(cache.len(), 1);
        cache.invalidate(&req);
        assert!(cache.is_empty());
    }
}
