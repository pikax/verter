//! Component-meta cache key types.
//!
//! These keys were previously private to `component_meta_query_engine.rs`.
//! The engine's authoritative `FxHashMap` caches now live in
//! [`crate::component_meta_caches`] as host-owned typed DBs. Both the
//! engine's per-request read-through views and the host DBs share
//! these key types, so they live in a public-to-the-crate module.
//!
//! ## `Arc<str>` / `Arc<TypeExpr>` fields
//!
//! Each previously-`String` field becomes `Arc<str>` and each previously-
//! owned `TypeExpr` (in `PreparedSubstitutionKey::Entries`) becomes
//! `Arc<TypeExpr>`. Two reasons:
//!
//! 1. **Cheap cloning across threads.** Host DBs are shared across all
//!    request threads via `DashMap<K, Arc<Entry>>`. Cloning a key on
//!    every lookup is a refcount bump for `Arc<str>`, but a heap
//!    allocation + memcpy for `String`. Since canonical ids and
//!    symbol names are repeatedly cloned per query, the savings
//!    compound.
//!
//! 2. **Hash determinism.** `Arc<str>` and `Arc<TypeExpr>` hash via
//!    the underlying value (deref-through), so two keys that share
//!    the same canonical id produce identical hashes regardless of
//!    which `Arc` instance carries the string.

use std::sync::Arc;

use verter_semantic::analysis::type_expr::TypeExpr;

use crate::resolver_core::RouteDemand;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PreparedSubstitutionKey {
    Empty,
    Entries(Vec<(Arc<str>, Arc<TypeExpr>)>),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PreparedSurfaceCacheKey {
    pub canonical_id: Arc<str>,
    pub symbol_name: Arc<str>,
    pub substitutions: PreparedSubstitutionKey,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PreparedMemberCacheKey {
    pub canonical_id: Arc<str>,
    pub symbol_name: Arc<str>,
    pub member_name: Arc<str>,
    pub kind: PreparedMemberCacheKind,
    pub substitutions: PreparedSubstitutionKey,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum PreparedMemberCacheKind {
    Requested,
    InheritedRoute,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PreparedTargetCacheKey {
    pub active_scope_canonical_id: Arc<str>,
    pub decl_canonical_id: Arc<str>,
    pub decl_symbol_name: Arc<str>,
    pub requested_name: Arc<str>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RoutedExprSurfaceCacheKey {
    pub scope_canonical_id: Arc<str>,
    pub root_symbol: Arc<str>,
    pub route: RouteDemand,
}
