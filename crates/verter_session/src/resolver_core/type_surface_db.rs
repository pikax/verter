//! Shared projected type-surface operations.
//!
//! Replaces request-local projection results that die with each request.
//! Answers subject-level surface/keyspace/member queries from semantic identity.
//!
//! Only stable outcomes are published. Fuse-tripped, truncated, or interrupted
//! results must stay request-local and uncached.
//!
//! Concurrent cold requests for the same `TypeSurfaceOpKey` coalesce via singleflight.

use std::hash::Hash;
use std::sync::Arc;

use crate::resolver_core::{FactVersionRef, SingleflightGroup, StoreView, ValidatedFactCache};

use verter_semantic::analysis::type_solver::query_engine::{
    ProjectedKeyspace, ProjectedMember, ProjectedSurface,
};

/// Canonical subject identity for a type surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeSurfaceKey {
    /// Canonical file owning the declaration.
    pub canonical_owner: String,
    /// Symbol name in that file.
    pub symbol_name: String,
    /// Hash of type argument bindings (e.g., `Props<T>` with `T = string`).
    pub instantiation_hash: u64,
    /// Hash of conditional/store-view context.
    pub context_hash: u64,
}

/// Operation key: a specific projection operation over a subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeSurfaceOpKey {
    Surface(TypeSurfaceKey),
    Keyspace(TypeSurfaceKey),
    Member {
        subject: TypeSurfaceKey,
        member_name: String,
    },
    RoutedExpr {
        subject: TypeSurfaceKey,
        route: crate::resolver_core::RouteDemand,
    },
}

impl TypeSurfaceOpKey {
    pub fn subject(&self) -> &TypeSurfaceKey {
        match self {
            TypeSurfaceOpKey::Surface(s) => s,
            TypeSurfaceOpKey::Keyspace(s) => s,
            TypeSurfaceOpKey::Member { subject, .. } => subject,
            TypeSurfaceOpKey::RoutedExpr { subject, .. } => subject,
        }
    }
}

/// Result of a projection operation.
#[derive(Debug, Clone)]
pub enum TypeSurfaceOpResult {
    Surface(ProjectedSurface),
    Keyspace(ProjectedKeyspace),
    Member(ProjectedMember),
    Expr(verter_semantic::analysis::type_expr::TypeExpr),
    /// Stable miss — the projection yielded no result.
    Miss,
}

impl TypeSurfaceOpResult {
    pub fn as_surface(&self) -> Option<&ProjectedSurface> {
        match self {
            TypeSurfaceOpResult::Surface(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_keyspace(&self) -> Option<&ProjectedKeyspace> {
        match self {
            TypeSurfaceOpResult::Keyspace(k) => Some(k),
            _ => None,
        }
    }

    pub fn as_member(&self) -> Option<&ProjectedMember> {
        match self {
            TypeSurfaceOpResult::Member(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_expr(&self) -> Option<&verter_semantic::analysis::type_expr::TypeExpr> {
        match self {
            TypeSurfaceOpResult::Expr(expr) => Some(expr),
            _ => None,
        }
    }

    pub fn is_miss(&self) -> bool {
        matches!(self, TypeSurfaceOpResult::Miss)
    }
}

/// Shared DB for projected type-surface operations.
pub struct TypeSurfaceDb {
    ops: ValidatedFactCache<TypeSurfaceOpKey, TypeSurfaceOpResult>,
    singleflight: SingleflightGroup<TypeSurfaceOpKey, Arc<TypeSurfaceOpResult>, ()>,
}

impl TypeSurfaceDb {
    pub fn new() -> Self {
        Self {
            ops: ValidatedFactCache::default(),
            singleflight: SingleflightGroup::default(),
        }
    }

    /// Look up a cached projection result if valid.
    pub fn get<V: StoreView>(
        &self,
        op_key: &TypeSurfaceOpKey,
        view: &V,
    ) -> Option<Arc<TypeSurfaceOpResult>> {
        self.ops.get_if_valid(op_key, view)
    }

    /// Look up or compute a projection result.
    ///
    /// `compute` should return `Some(result)` for stable outcomes that should
    /// be published. Return `None` for fuse-tripped or degraded results that
    /// must NOT be cached.
    pub fn get_or_project<V, F>(
        &self,
        op_key: TypeSurfaceOpKey,
        view: &V,
        compute: F,
    ) -> Option<Arc<TypeSurfaceOpResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<TypeSurfaceOpResult>,
    {
        self.get_or_project_with_facts(op_key, view, || {
            compute().map(|result| (result, Vec::new()))
        })
    }

    /// Look up or compute a projection result with fact validation.
    pub fn get_or_project_with_facts<V, F>(
        &self,
        op_key: TypeSurfaceOpKey,
        view: &V,
        compute: F,
    ) -> Option<Arc<TypeSurfaceOpResult>>
    where
        V: StoreView,
        F: FnOnce() -> Option<(TypeSurfaceOpResult, Vec<FactVersionRef>)>,
    {
        if let Some(result) = self.ops.get_if_valid(&op_key, view) {
            return Some(result);
        }

        let flight = self
            .singleflight
            .run(op_key.clone(), view.compat_token(), || {
                if let Some(result) = self.ops.get_if_valid(&op_key, view) {
                    return Ok(result);
                }
                match compute() {
                    Some((result, facts)) => {
                        let arc = Arc::new(result);
                        self.ops.insert_arc(op_key.clone(), arc.clone(), facts);
                        Ok(arc)
                    }
                    None => Err(()),
                }
            });

        match flight {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// Publish a stable projection result directly.
    ///
    /// Only call this for stable, completed results. Never publish
    /// fuse-tripped or degraded results.
    pub fn publish(&self, op_key: TypeSurfaceOpKey, result: TypeSurfaceOpResult) {
        self.ops.insert(op_key, result, Vec::new());
    }

    /// Publish a stable projection result as Arc.
    pub fn publish_arc(&self, op_key: TypeSurfaceOpKey, result: Arc<TypeSurfaceOpResult>) {
        self.ops.insert_arc(op_key, result, Vec::new());
    }

    /// Publish a stable projection result with explicit fact validation.
    pub fn publish_with_facts(
        &self,
        op_key: TypeSurfaceOpKey,
        result: TypeSurfaceOpResult,
        facts: Vec<FactVersionRef>,
    ) {
        self.ops.insert(op_key, result, facts);
    }

    /// Clear all cached projections.
    pub fn clear(&self) {
        self.ops.clear();
        self.singleflight.clear();
    }

    /// Evict all cached projections owned by one canonical declaration file.
    pub fn evict_owner(&self, canonical_owner: &str) {
        let keys: Vec<_> = self
            .ops
            .snapshot_all()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| key.subject().canonical_owner == canonical_owner)
            .collect();
        for key in keys {
            self.ops.remove(&key);
        }
    }
}

impl Default for TypeSurfaceDb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{FactVersionRef, StoreView, StoreViewCompatToken};
    use verter_semantic::analysis::type_expr::TypeExpr;

    struct TestView {
        token: StoreViewCompatToken,
    }

    impl TestView {
        fn new(token: u64) -> Self {
            Self {
                token: StoreViewCompatToken(token),
            }
        }
    }

    impl StoreView for TestView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }

        fn validates(&self, _fact: &FactVersionRef) -> bool {
            true
        }
    }

    fn test_subject() -> TypeSurfaceKey {
        TypeSurfaceKey {
            canonical_owner: "foo.vue".to_owned(),
            symbol_name: "FooProps".to_owned(),
            instantiation_hash: 0,
            context_hash: 0,
        }
    }

    #[test]
    fn publish_and_get_surface() {
        let db = TypeSurfaceDb::new();
        let view = TestView::new(1);
        let key = TypeSurfaceOpKey::Surface(test_subject());

        db.publish(
            key.clone(),
            TypeSurfaceOpResult::Surface(ProjectedSurface {
                members: vec![],
                call_signatures: vec![],
                construct_signatures: vec![],
                has_index_signature: false,
            }),
        );

        let result = db.get(&key, &view);
        assert!(result.is_some());
        assert!(result.unwrap().as_surface().is_some());
    }

    #[test]
    fn publish_and_get_member() {
        let db = TypeSurfaceDb::new();
        let view = TestView::new(1);
        let key = TypeSurfaceOpKey::Member {
            subject: test_subject(),
            member_name: "name".to_owned(),
        };

        db.publish(
            key.clone(),
            TypeSurfaceOpResult::Member(ProjectedMember {
                name: "name".to_owned(),
                ty: TypeExpr::Primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::String,
                ),
                optional: false,
                readonly: false,
                is_method: false,
            }),
        );

        let result = db.get(&key, &view);
        assert!(result.is_some());
        assert!(result.unwrap().as_member().is_some());
    }

    #[test]
    fn publish_and_get_keyspace() {
        let db = TypeSurfaceDb::new();
        let view = TestView::new(1);
        let key = TypeSurfaceOpKey::Keyspace(test_subject());

        db.publish(
            key.clone(),
            TypeSurfaceOpResult::Keyspace(ProjectedKeyspace {
                members: vec!["a".to_owned(), "b".to_owned()],
                has_index_signature: false,
            }),
        );

        let result = db.get(&key, &view);
        assert!(result.is_some());
        let ks = result.unwrap();
        assert_eq!(ks.as_keyspace().unwrap().members.len(), 2);
    }

    #[test]
    fn miss_is_cached() {
        let db = TypeSurfaceDb::new();
        let view = TestView::new(1);
        let key = TypeSurfaceOpKey::Surface(test_subject());

        db.publish(key.clone(), TypeSurfaceOpResult::Miss);

        let result = db.get(&key, &view);
        assert!(result.is_some());
        assert!(result.unwrap().is_miss());
    }

    #[test]
    fn get_or_project_caches_stable() {
        let db = TypeSurfaceDb::new();
        let view = TestView::new(1);
        let key = TypeSurfaceOpKey::Keyspace(test_subject());
        let call_count = std::sync::atomic::AtomicU32::new(0);

        let r1 = db.get_or_project(key.clone(), &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(TypeSurfaceOpResult::Keyspace(ProjectedKeyspace {
                members: vec!["x".to_owned()],
                has_index_signature: false,
            }))
        });
        assert!(r1.is_some());

        let r2 = db.get_or_project(key.clone(), &view, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        });
        assert!(r2.is_some());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn fuse_tripped_not_cached() {
        let db = TypeSurfaceDb::new();
        let view = TestView::new(1);
        let key = TypeSurfaceOpKey::Surface(test_subject());

        // Simulate fuse trip: return None from compute.
        let r1 = db.get_or_project(key.clone(), &view, || None);
        assert!(r1.is_none());

        // Next call should recompute, not hit cache.
        let r2 = db.get_or_project(key.clone(), &view, || {
            Some(TypeSurfaceOpResult::Surface(ProjectedSurface {
                members: vec![],
                call_signatures: vec![],
                construct_signatures: vec![],
                has_index_signature: false,
            }))
        });
        assert!(r2.is_some());
    }

    #[test]
    fn clear_removes_all() {
        let db = TypeSurfaceDb::new();
        let view = TestView::new(1);
        let key = TypeSurfaceOpKey::Surface(test_subject());

        db.publish(key.clone(), TypeSurfaceOpResult::Miss);
        db.clear();

        assert!(db.get(&key, &view).is_none());
    }
}
