//! Prepared declaration structures consumed by the solver.
//!
//! Prepared declarations sit between `ShallowFileState`/frontier state and the
//! solver. They normalize declaration shape, build lookup tables, and classify
//! dependencies — but they do NOT perform full semantic solving.
//!
//! These structures are the long-term replacement for:
//! - `PreparedImportedTypeAlias`
//! - `PreparedLocalImportedTypeAlias`
//! - `PreparedImportedDeclContext`

use rustc_hash::FxHashMap;

use super::host::ResolvedRootIdentity;
use crate::analysis::type_eval::{FunctionSignature, TypeDeclKind, ValueDeclKind};
use crate::analysis::type_expr::{ObjectExpr, ObjectMember, TypeExpr, TypeParam};

// ---------------------------------------------------------------------------
// Prepared type declaration
// ---------------------------------------------------------------------------

/// Solver-facing prepared type declaration.
///
/// Prepared declarations are cache-owned, declaration-only, and intentionally
/// shallow: they preserve symbolic bodies rather than eagerly normalizing them.
///
/// Keyed by `(canonical_id, symbol_name, source_hash)` in the host cache.
///
/// The `kind` field currently uses `TypeDeclKind` for backward compatibility
/// with existing session code. It will migrate to `PreparedDeclKind` when the
/// session preparation code is updated (Milestone 2).
#[derive(Debug, Clone)]
pub struct PreparedTypeDecl {
    /// Canonical identity of the defining file + symbol name.
    pub root_identity: ResolvedRootIdentity,

    /// The exported name (may differ from symbol_name due to aliasing).
    pub exported_name: Option<String>,

    /// Declaration kind.
    pub kind: TypeDeclKind,

    /// Generic type parameters.
    pub type_parameters: Vec<TypeParam>,

    /// The symbolic body — NOT eagerly evaluated.
    pub body: TypeExpr,

    /// Member index for direct property/method lookup without walking the body.
    /// Populated for interfaces and object-like aliases. Default: empty.
    pub member_index: FxHashMap<String, PreparedMember>,

    /// Same-file symbol references needed for local closure.
    pub local_deps: Vec<String>,

    /// Cross-file symbol references (canonical_id + name pairs).
    pub external_deps: Vec<PreparedExternalDep>,

    /// Pre-resolved name context: maps bare names appearing in the body
    /// to their resolved root identities. Built at prepare time from the
    /// defining file's local and import scope. Allows the solver to resolve
    /// cross-file references without going back to the host for route discovery.
    pub name_resolution: FxHashMap<String, ResolvedRootIdentity>,

    /// Declaration provenance metadata.
    pub provenance: DeclProvenance,

    /// Cache dependency contract for invalidation. Records the defining
    /// file hash, barrel/reexport participants, and local closure participants
    /// at preparation time. Used to check if this prepared entry is still valid.
    pub cache_deps: PreparedCacheDeps,
}

/// A member in the prepared member index — pre-extracted from the declaration
/// body for O(1) property lookup.
#[derive(Debug, Clone)]
pub struct PreparedMember {
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
}

/// A cross-file dependency reference in a prepared declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PreparedExternalDep {
    pub canonical_id: String,
    pub symbol_name: String,
}

/// Provenance metadata for a prepared declaration.
#[derive(Debug, Clone, Default)]
pub struct DeclProvenance {
    /// Route kind that resolved this declaration (direct, alias, wildcard).
    pub route_kind: Option<String>,
    /// Source text range if available (for diagnostics).
    pub source_range: Option<(u32, u32)>,
    /// Barrel files traversed to reach the defining file.
    pub barrel_hops: Vec<String>,
}

/// Prepared declaration kind — broader than `TypeDeclKind` to support
/// declaration merging and enum dual-space treatment.
///
/// Not yet used as the primary `kind` field — this is reserved for Milestone 2
/// when session preparation code is updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreparedDeclKind {
    Alias,
    Interface,
    Class,
    Enum,
    Merged,
}

impl From<TypeDeclKind> for PreparedDeclKind {
    fn from(kind: TypeDeclKind) -> Self {
        match kind {
            TypeDeclKind::Alias => Self::Alias,
            TypeDeclKind::Interface => Self::Interface,
            TypeDeclKind::Class => Self::Class,
        }
    }
}

// ---------------------------------------------------------------------------
// Prepared value declaration
// ---------------------------------------------------------------------------

/// Solver-facing prepared value declaration.
///
/// Required for `typeof` without building an `EvalEnv`. Supports:
/// - `typeof x`
/// - dotted paths: `typeof ns.foo.bar`
/// - class/constructor/static-member queries
#[derive(Debug, Clone)]
pub struct PreparedValueDecl {
    /// Canonical identity.
    pub root_identity: ResolvedRootIdentity,

    /// Exported name if different from the symbol name.
    pub exported_name: Option<String>,

    /// Value declaration kind.
    pub kind: ValueDeclKind,

    /// Type annotation on the value declaration (e.g. `const x: T`).
    pub type_annotation: Option<TypeExpr>,

    /// Function signature if the value is a function.
    pub function_signature: Option<FunctionSignature>,

    /// Object shape if the value is a const object / namespace.
    pub object_shape: Option<ObjectExpr>,

    /// Member index for dotted path lookup (e.g. `typeof ns.member`).
    pub member_index: FxHashMap<String, PreparedValueMember>,

    /// For enum values: member name -> literal value mapping.
    pub enum_members: Option<FxHashMap<String, TypeExpr>>,

    /// Cross-file dependencies.
    pub external_deps: Vec<PreparedExternalDep>,

    /// Pre-resolved name context for bare names in type annotations
    /// attached to this value declaration. Same semantics as
    /// `PreparedTypeDecl::name_resolution`.
    pub name_resolution: FxHashMap<String, ResolvedRootIdentity>,

    /// Cache dependency contract for invalidation.
    pub cache_deps: PreparedCacheDeps,
}

/// Prepared value declaration kind — broader than `ValueDeclKind` to handle
/// enum objects and namespace objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreparedValueDeclKind {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
    EnumObject,
    Namespace,
}

impl From<ValueDeclKind> for PreparedValueDeclKind {
    fn from(kind: ValueDeclKind) -> Self {
        match kind {
            ValueDeclKind::Const => Self::Const,
            ValueDeclKind::Let => Self::Let,
            ValueDeclKind::Var => Self::Var,
            ValueDeclKind::Function => Self::Function,
            ValueDeclKind::AsyncFunction => Self::AsyncFunction,
            ValueDeclKind::Class => Self::Class,
            ValueDeclKind::Enum => Self::EnumObject,
        }
    }
}

/// A member in a prepared value's member index (for dotted typeof paths).
#[derive(Debug, Clone)]
pub struct PreparedValueMember {
    pub ty: TypeExpr,
    pub is_method: bool,
}

// ---------------------------------------------------------------------------
// Prepared cache dependency contract
// ---------------------------------------------------------------------------

/// Records the full dependency/provenance set used to build a prepared
/// declaration. Used for invalidation.
#[derive(Debug, Clone, Default)]
pub struct PreparedCacheDeps {
    /// Defining file canonical id + hash.
    pub defining_file: Option<(String, u64)>,
    /// Every barrel/reexport file that participated in route selection.
    pub barrel_participants: Vec<(String, u64)>,
    /// Local closure participant identities.
    pub local_closure_participants: Vec<String>,
}

impl PreparedCacheDeps {
    /// Check whether all recorded participants still match their cached version.
    pub fn is_valid(&self, current_hashes: &FxHashMap<String, u64>) -> bool {
        if let Some((ref id, hash)) = self.defining_file {
            if current_hashes.get(id.as_str()).copied() != Some(hash) {
                return false;
            }
        }
        for (id, hash) in &self.barrel_participants {
            if current_hashes.get(id.as_str()).copied() != Some(*hash) {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Builder helpers
// ---------------------------------------------------------------------------

impl PreparedTypeDecl {
    /// Create a new prepared type declaration with minimal fields.
    /// Extra fields (member_index, deps, provenance) are defaulted.
    pub fn new(root_identity: ResolvedRootIdentity, kind: TypeDeclKind, body: TypeExpr) -> Self {
        Self {
            root_identity,
            exported_name: None,
            kind,
            type_parameters: Vec::new(),
            body,
            member_index: FxHashMap::default(),
            local_deps: Vec::new(),
            external_deps: Vec::new(),
            name_resolution: FxHashMap::default(),
            provenance: DeclProvenance::default(),
            cache_deps: PreparedCacheDeps::default(),
        }
    }

    /// Build a member index from an object-like body.
    pub fn build_member_index(&mut self) {
        if let TypeExpr::Object(ref obj) = self.body {
            for member in &obj.properties {
                if let ObjectMember::Property(prop) = member {
                    self.member_index.insert(
                        prop.name.clone(),
                        PreparedMember {
                            ty: prop.ty.clone(),
                            optional: prop.optional,
                            readonly: prop.readonly,
                            is_method: false,
                        },
                    );
                }
            }
        }
    }

    /// Look up a member by name. O(1) if the member index is populated.
    pub fn member(&self, name: &str) -> Option<&PreparedMember> {
        self.member_index.get(name)
    }

    /// Classify using the broader PreparedDeclKind.
    pub fn prepared_kind(&self) -> PreparedDeclKind {
        PreparedDeclKind::from(self.kind)
    }
}

impl PreparedValueDecl {
    /// Create a new prepared value declaration with minimal fields.
    pub fn new(root_identity: ResolvedRootIdentity, kind: ValueDeclKind) -> Self {
        Self {
            root_identity,
            exported_name: None,
            kind,
            type_annotation: None,
            function_signature: None,
            object_shape: None,
            member_index: FxHashMap::default(),
            enum_members: None,
            external_deps: Vec::new(),
            name_resolution: FxHashMap::default(),
            cache_deps: PreparedCacheDeps::default(),
        }
    }

    /// Classify using the broader PreparedValueDeclKind.
    pub fn prepared_kind(&self) -> PreparedValueDeclKind {
        PreparedValueDeclKind::from(self.kind)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::analysis::type_expr::{
        LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
    };

    use super::*;

    #[test]
    fn prepared_type_decl_member_index_from_object_body() {
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "label".into(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "count".into(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: true,
                    readonly: false,
                }),
            ],
        }));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Props"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        let label = decl.member("label").expect("label should exist");
        assert!(!label.optional);
        assert!(matches!(
            label.ty,
            TypeExpr::Primitive(PrimitiveName::String)
        ));

        let count = decl.member("count").expect("count should exist");
        assert!(count.optional);

        assert!(decl.member("missing").is_none());
    }

    #[test]
    fn prepared_decl_kind_from_type_decl_kind() {
        assert_eq!(
            PreparedDeclKind::from(TypeDeclKind::Alias),
            PreparedDeclKind::Alias
        );
        assert_eq!(
            PreparedDeclKind::from(TypeDeclKind::Interface),
            PreparedDeclKind::Interface
        );
        assert_eq!(
            PreparedDeclKind::from(TypeDeclKind::Class),
            PreparedDeclKind::Class
        );
    }

    #[test]
    fn prepared_cache_deps_valid_when_hashes_match() {
        let deps = PreparedCacheDeps {
            defining_file: Some(("/types.ts".into(), 12345)),
            barrel_participants: vec![("/barrel.ts".into(), 67890)],
            local_closure_participants: vec!["Inner".into()],
        };

        let mut hashes = FxHashMap::default();
        hashes.insert("/types.ts".to_string(), 12345u64);
        hashes.insert("/barrel.ts".to_string(), 67890u64);
        assert!(deps.is_valid(&hashes));

        // Stale defining file
        hashes.insert("/types.ts".to_string(), 99999u64);
        assert!(!deps.is_valid(&hashes));
    }

    #[test]
    fn prepared_cache_deps_invalid_when_barrel_changes() {
        let deps = PreparedCacheDeps {
            defining_file: Some(("/types.ts".into(), 100)),
            barrel_participants: vec![("/index.ts".into(), 200)],
            local_closure_participants: vec![],
        };

        let mut hashes = FxHashMap::default();
        hashes.insert("/types.ts".to_string(), 100u64);
        hashes.insert("/index.ts".to_string(), 300u64); // changed
        assert!(!deps.is_valid(&hashes));
    }

    #[test]
    fn prepared_value_decl_enum_members() {
        let mut decl = PreparedValueDecl::new(
            ResolvedRootIdentity::new("/enums.ts", "Color"),
            ValueDeclKind::Const,
        );

        let mut members = FxHashMap::default();
        members.insert("Red".into(), TypeExpr::Literal(LiteralValue::Number(0.0)));
        members.insert("Green".into(), TypeExpr::Literal(LiteralValue::Number(1.0)));
        decl.enum_members = Some(members);

        let enum_members = decl.enum_members.as_ref().unwrap();
        assert_eq!(enum_members.len(), 2);
        assert!(enum_members.contains_key("Red"));
    }

    #[test]
    fn prepared_type_decl_prepared_kind() {
        let decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "T"),
            TypeDeclKind::Interface,
            TypeExpr::Primitive(PrimitiveName::String),
        );
        assert_eq!(decl.prepared_kind(), PreparedDeclKind::Interface);
    }
}
