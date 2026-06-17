//! Host-shared cache-key identity for resolved named Vue macro types.
//!
//! The types and trait that host-owned caches (e.g. the semantic graph's
//! `VueMacroElements` slot behind `HostResolvedNamedTypeKey`) use to memoize
//! resolved named types across requests within one workspace generation —
//! this is the `HostResolvedNamedTypeKey` INNER identity, which is Vue
//! semantics and therefore lives under the Vue script module.
//!
//! The underlying key is the exact tuple `(name, surface, base_offset,
//! companion_cache_key, type_param_bindings)` that the type-surface engine
//! already used for per-context memoization — promoted to a host-shared
//! identity, with additional `(canonical_id, whole_hash)` scoping provided
//! by the adapter.
use crate::utils::oxc::script::type_surface::{BlockedTypeSurface, ResolvedElements};
use std::sync::Arc;

/// Cache key for a fully-resolved named local symbol.
///
/// Note: `companion_cache_key` and `type_param_bindings` are `Arc<[…]>` so
/// child contexts produced by `instantiate_type_params_ctx` share the same
/// underlying slice without deep-cloning.
///
/// `from_root_body` is part of the key because resolving the same
/// named type from different positions (macro-T own-body vs heritage
/// descent) yields structurally different `ResolvedElements` — each
/// resolved prop carries a `declared_in_macro_type_arg` fact whose
/// value depends on the caller's `from_root_body` position. Without
/// this dimension a single cache slot would erroneously serve both
/// positions (the "cache-incomplete" risk).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedNamedTypeCacheKey {
    pub name: Box<[u8]>,
    pub surface: Option<BlockedTypeSurface>,
    pub base_offset: u32,
    pub from_root_body: bool,
    pub companion_cache_key: Arc<[Box<[u8]>]>,
    pub type_param_bindings: Arc<[ResolvedTypeParamBindingCacheKey]>,
}

/// Stable identity for a generic parameter binding — matches the semantic
/// identity used by `type_param_bindings_cache_key` inside the resolver.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedTypeParamBindingCacheKey {
    pub name: Box<[u8]>,
    pub bound: Box<[u8]>,
}

/// Injected cache handle. Implementations must be `Send + Sync` so a single
/// adapter instance can be cloned into child contexts and shared across
/// concurrent resolver threads.
///
/// Contract: `get` is read-only and must not mutate the cache. `insert`
/// overwrites any prior entry under the same key (the resolver never asks
/// for reconciliation; two callers computing the same key must produce
/// structurally equal results).
pub trait NamedTypeCache: std::fmt::Debug + Send + Sync {
    fn get(&self, key: &ResolvedNamedTypeCacheKey) -> Option<Arc<ResolvedElements>>;
    fn insert(&self, key: ResolvedNamedTypeCacheKey, value: Arc<ResolvedElements>);
}
