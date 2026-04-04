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
    ///
    /// Handles:
    /// - `TypeExpr::Object` — direct properties
    /// - `TypeExpr::Intersection` — scan parts right-to-left, indexing direct
    ///   object members. Right-to-left precedence ensures the interface's own
    ///   object tail (last part) wins over inherited parts (earlier parts).
    ///   Only direct Object members are indexed; heritage Ref parts are skipped.
    ///   Nested transparent intersections are flattened so declaration-merged
    ///   interfaces still expose members from earlier object slices.
    pub fn build_member_index(&mut self) {
        Self::index_transparent_object_members(&mut self.member_index, &self.body);
    }

    /// Index direct object members into the member_index map.
    /// Existing entries are NOT overwritten (preserves right-to-left precedence
    /// when called from intersection traversal).
    fn index_object_members(
        member_index: &mut rustc_hash::FxHashMap<String, PreparedMember>,
        obj: &crate::analysis::type_expr::ObjectExpr,
    ) {
        for member in &obj.properties {
            if let ObjectMember::Property(prop) = member {
                // entry API: only insert if not already present
                member_index
                    .entry(prop.name.clone())
                    .or_insert_with(|| PreparedMember {
                        ty: prop.ty.clone(),
                        optional: prop.optional,
                        readonly: prop.readonly,
                        is_method: false,
                    });
            }
        }
    }

    fn index_transparent_object_members(
        member_index: &mut rustc_hash::FxHashMap<String, PreparedMember>,
        body: &TypeExpr,
    ) {
        match body {
            TypeExpr::Object(obj) => Self::index_object_members(member_index, obj),
            TypeExpr::Intersection(parts) => {
                for part in parts.iter().rev() {
                    Self::index_transparent_object_members(member_index, part);
                }
            }
            TypeExpr::Parenthesized(inner) => {
                Self::index_transparent_object_members(member_index, inner);
            }
            _ => {}
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

    // -----------------------------------------------------------------------
    // Workstream D: intersection member indexing tests
    // -----------------------------------------------------------------------

    fn make_object(props: &[(&str, TypeExpr, bool)]) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: props
                .iter()
                .map(|(name, ty, optional)| {
                    ObjectMember::Property(ObjectProperty {
                        name: (*name).into(),
                        ty: ty.clone(),
                        optional: *optional,
                        readonly: false,
                    })
                })
                .collect(),
        }))
    }

    #[test]
    fn intersection_tail_object_members_indexed() {
        // Simulates: interface Foo extends Bar { own: string }
        // Lowered as: Intersection([Ref("Bar"), Object({ own: string })])
        let body = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::named("Bar"), // heritage ref
            make_object(&[("own", TypeExpr::Primitive(PrimitiveName::String), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Foo"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        // KEY ASSERTION: 'own' from the intersection tail should be indexed
        assert!(
            decl.member("own").is_some(),
            "own member from intersection tail should be indexed"
        );
        assert!(
            matches!(
                decl.member("own").unwrap().ty,
                TypeExpr::Primitive(PrimitiveName::String)
            ),
            "own should be string"
        );

        // Negative: 'Bar' heritage ref members should NOT be indexed
        // (they don't exist as direct members)
        assert!(
            decl.member("Bar").is_none(),
            "heritage ref name should not be indexed as a member"
        );
    }

    #[test]
    fn intersection_own_members_win_over_heritage() {
        // interface Foo extends Bar { mode: number }
        // where Bar also has mode: string
        // Lowered as: Intersection([Object({mode: string}), Object({mode: number})])
        let body = TypeExpr::Intersection(Arc::from(vec![
            make_object(&[("mode", TypeExpr::Primitive(PrimitiveName::String), false)]),
            make_object(&[("mode", TypeExpr::Primitive(PrimitiveName::Number), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Foo"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        // Right-to-left precedence: the LAST object in the intersection wins
        let mode = decl.member("mode").expect("mode should be indexed");
        assert!(
            matches!(mode.ty, TypeExpr::Primitive(PrimitiveName::Number)),
            "own member (last in intersection) should win, got {:?}",
            mode.ty
        );
    }

    #[test]
    fn non_object_body_still_produces_empty_index() {
        let body = TypeExpr::Primitive(PrimitiveName::String);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "T"),
            TypeDeclKind::Alias,
            body,
        );
        decl.build_member_index();

        assert!(
            decl.member_index.is_empty(),
            "primitive body should have empty member index"
        );
    }

    #[test]
    fn interface_with_two_heritage_clauses_and_own_tail() {
        // interface Foo extends A, B { own: boolean }
        // Lowered as: Intersection([Ref("A"), Ref("B"), Object({own: boolean})])
        let body = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::named("A"),
            TypeExpr::named("B"),
            make_object(&[("own", TypeExpr::Primitive(PrimitiveName::Boolean), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Foo"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        assert!(
            decl.member("own").is_some(),
            "own member should be indexed from trailing object"
        );
        assert!(
            matches!(
                decl.member("own").unwrap().ty,
                TypeExpr::Primitive(PrimitiveName::Boolean)
            ),
            "own should be boolean"
        );
        // Heritage Ref names should NOT appear as members
        assert!(decl.member("A").is_none());
        assert!(decl.member("B").is_none());
    }

    #[test]
    fn missing_member_returns_none_without_panic() {
        let body = make_object(&[(
            "existing",
            TypeExpr::Primitive(PrimitiveName::String),
            false,
        )]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "T"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        assert!(decl.member("existing").is_some());
        assert!(
            decl.member("nonexistent").is_none(),
            "missing member should return None"
        );
        assert!(
            decl.member("").is_none(),
            "empty string member should return None"
        );
    }

    #[test]
    fn generic_alias_body_with_type_args_not_indexed() {
        // type Wrapper<T> = Array<T> — generic ref body, NOT indexable
        let body = TypeExpr::named_with_args("Array", vec![TypeExpr::named("T")]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Wrapper"),
            TypeDeclKind::Alias,
            body,
        );
        decl.build_member_index();

        assert!(
            decl.member_index.is_empty(),
            "generic ref body should not be indexed"
        );
    }

    #[test]
    fn merged_interface_nested_intersections_keep_earlier_members() {
        let body = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::Intersection(Arc::from(vec![make_object(&[(
                "first",
                TypeExpr::Primitive(PrimitiveName::String),
                false,
            )])])),
            make_object(&[("second", TypeExpr::Primitive(PrimitiveName::Number), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Merged"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        assert!(decl.member("first").is_some());
        assert!(decl.member("second").is_some());
    }
}
