//! The parse-domain cross-declaration lenses a file's declaration bodies are
//! fingerprinted and route-classified through: [`ShallowLens`] maps a
//! `Ref(name)` site to its cross-declaration reference identity, and
//! [`RouteLens`] classifies authored import routes for the route-fact
//! producer. Both are built once per shallow file state, installed on the
//! declaration-body memo, and shared by every consumer.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_session_query::facts::{CrossDeclLens, CrossDeclRef, SymbolSpace};

// ──────────────────────────────────────────────────────────────────
// Lens — maps `Ref(name)` sites to cross-decl reference identities
// (R12 parse-domain — NO resolved_canonical).
// ──────────────────────────────────────────────────────────────────

/// Resolve `name` against the shallow state's local-symbol +
/// import-binding tables (header data). Falls back to `Unresolved` for
/// free references.
#[derive(Debug)]
pub struct ShallowLens {
    locals: FxHashSet<verter_type_expr::DeclBindingKey>,
    value_locals: FxHashSet<verter_type_expr::DeclBindingKey>,
    exported: FxHashSet<verter_type_expr::DeclBindingKey>,
    /// Maps a public exported name to its backing LOCAL declaration name
    /// for `export { Foo as Bar }` / `export { Foo }` (the latter maps a
    /// name to itself). Built ONLY from `ExportTarget::Local` entries —
    /// reexports are excluded, so they never compute body facts through
    /// the lazy path. The lazy `Export(Bar, …)` fact preserves the public
    /// key `Bar` while lowering/hashing the backing local `Foo`.
    local_export_targets:
        FxHashMap<verter_type_expr::DeclBindingKey, verter_type_expr::DeclBindingKey>,
    /// Maps `local_binding_name → source_specifier`.
    import_targets: FxHashMap<verter_type_expr::DeclBindingKey, Arc<str>>,
}

impl ShallowLens {
    /// Assemble the lens from its owned header tables. The single builder
    /// over a finished shallow file state calls this exactly once per state;
    /// every consumer shares the resulting instance.
    pub fn new(
        locals: FxHashSet<verter_type_expr::DeclBindingKey>,
        value_locals: FxHashSet<verter_type_expr::DeclBindingKey>,
        exported: FxHashSet<verter_type_expr::DeclBindingKey>,
        local_export_targets: FxHashMap<
            verter_type_expr::DeclBindingKey,
            verter_type_expr::DeclBindingKey,
        >,
        import_targets: FxHashMap<verter_type_expr::DeclBindingKey, Arc<str>>,
    ) -> Self {
        Self {
            locals,
            value_locals,
            exported,
            local_export_targets,
            import_targets,
        }
    }

    /// The backing LOCAL declaration of the public exported name `key`
    /// (`export { Foo as Bar }` / `export { Foo }`); `None` for a reexport or
    /// a name that is not exported.
    pub fn local_export_target(
        &self,
        key: &verter_type_expr::DeclBindingKey,
    ) -> Option<&verter_type_expr::DeclBindingKey> {
        self.local_export_targets.get(key)
    }

    /// Whether `key` names a public export of the file.
    pub fn is_exported(&self, key: &verter_type_expr::DeclBindingKey) -> bool {
        self.exported.contains(key)
    }

    pub(crate) fn for_owner(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> OwnedShallowLens<'_> {
        OwnedShallowLens { base: self, owner }
    }

    fn resolve_in(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        space: SymbolSpace,
    ) -> Option<CrossDeclRef> {
        let key = verter_type_expr::DeclBindingKey::new(owner, name);
        if let Some(specifier) = self.import_targets.get(&key) {
            return Some(CrossDeclRef::ImportRef {
                specifier: Arc::clone(specifier),
                binding: Arc::from(name),
                space,
            });
        }
        if self.locals.contains(&key) || self.value_locals.contains(&key) {
            return Some(CrossDeclRef::LocalDecl {
                name: Arc::from(name),
                space,
            });
        }
        Some(CrossDeclRef::Unresolved {
            name: Arc::from(name),
            space,
        })
    }
}

impl CrossDeclLens for ShallowLens {
    fn resolve(&self, name: &str, space: SymbolSpace) -> Option<CrossDeclRef> {
        self.resolve_in(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            name,
            space,
        )
    }
}

pub(crate) struct OwnedShallowLens<'a> {
    base: &'a ShallowLens,
    owner: verter_type_expr::TopLevelOwnerId,
}

impl CrossDeclLens for OwnedShallowLens<'_> {
    fn resolve(&self, name: &str, space: SymbolSpace) -> Option<CrossDeclRef> {
        self.base.resolve_in(self.owner, name, space)
    }
}

/// The route-fact producer's hash-free classification lens: the AUTHORED
/// import target (specifier + imported name) plus header TYPE-symbol
/// membership, derived once from the finished `ShallowFileState` beside the
/// fingerprint [`ShallowLens`]. A SECOND view of the same shallow tables —
/// NOT a fingerprint-lens widening: this lens never feeds a hash. Both views
/// are pure parse domain; no resolved canonical is retained anywhere on the
/// artifact.
#[derive(Debug)]
pub struct RouteLens {
    canonical_id: Arc<str>,
    type_symbols: FxHashSet<verter_type_expr::DeclBindingKey>,
    import_targets:
        FxHashMap<verter_type_expr::DeclBindingKey, verter_session_query::facts::ImportRouteTarget>,
}

impl RouteLens {
    /// Assemble the lens from its owned tables. Built exactly once per
    /// shallow file state, from its final routed state, beside the
    /// fingerprint [`ShallowLens`].
    pub fn new(
        canonical_id: Arc<str>,
        type_symbols: FxHashSet<verter_type_expr::DeclBindingKey>,
        import_targets: FxHashMap<
            verter_type_expr::DeclBindingKey,
            verter_session_query::facts::ImportRouteTarget,
        >,
    ) -> Self {
        Self {
            canonical_id,
            type_symbols,
            import_targets,
        }
    }

    pub fn for_owner(&self, owner: verter_type_expr::TopLevelOwnerId) -> OwnedRouteLens<'_> {
        OwnedRouteLens { base: self, owner }
    }
}

pub struct OwnedRouteLens<'a> {
    base: &'a RouteLens,
    owner: verter_type_expr::TopLevelOwnerId,
}

impl verter_session_query::facts::RouteFactLens for OwnedRouteLens<'_> {
    fn resolve_import_route(
        &self,
        local: &str,
        _space: SymbolSpace,
    ) -> Option<verter_session_query::facts::ImportRouteTarget> {
        self.base
            .import_targets
            .get(&verter_type_expr::DeclBindingKey::new(self.owner, local))
            .cloned()
    }
    fn has_type_symbol(&self, name: &str) -> bool {
        self.base
            .type_symbols
            .contains(&verter_type_expr::DeclBindingKey::new(self.owner, name))
    }
    fn own_canonical_id(&self) -> Arc<str> {
        Arc::clone(&self.base.canonical_id)
    }
    fn own_top_level_owner(&self) -> verter_type_expr::TopLevelOwnerId {
        self.owner
    }
}
