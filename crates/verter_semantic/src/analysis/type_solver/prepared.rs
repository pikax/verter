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
use verter_type_expr::{
    MappedModifier, ObjectExpr, ObjectMember, PrimitiveName, TypeExpr, TypeParam,
};

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

    /// Structural wrapper classification computed at preparation time.
    /// Enables the solver to fast-path identity wrappers, pure overlays,
    /// key filters, key remaps, and transparent aliases.
    pub wrapper_shape: PreparedWrapperShape,

    /// Projection classification computed at preparation time.
    /// Determines how the solver can project individual members without
    /// fully instantiating the declaration body.
    pub projection_class: PreparedProjectionClass,
}

// ---------------------------------------------------------------------------
// Structural wrapper classification
// ---------------------------------------------------------------------------

/// Structural wrapper metadata classified at preparation time.
///
/// Enables the solver to fast-path identity wrappers, pure overlays,
/// key filters, key remaps, and transparent aliases without lowering full
/// bodies or dispatching on helper names.
#[derive(Debug, Clone, Default)]
pub struct PreparedWrapperShape {
    pub kind: PreparedWrapperKind,
    /// Which type parameter is the "source" (e.g., T in `{ [K in keyof T]: T[K] }`).
    pub source_param_index: Option<u16>,
    pub key_filter: PreparedKeyFilterShape,
    pub key_remap: PreparedKeyRemapShape,
    pub value_rule: PreparedValueRuleShape,
    pub modifiers: PreparedSurfaceModifiers,
}

/// Classification of the wrapper kind.
///
/// Covers structural mapped-type patterns only. Alias forwarding (including
/// transparent pass-through aliases like `type A<T> = B<T>`) is handled by
/// `PreparedProjectionClass::ForwardSubject(IdentityParams)` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreparedWrapperKind {
    /// Not a recognized structural wrapper pattern.
    #[default]
    None,
    /// Identity: `{ [K in keyof T]: T[K] }` — collapse to base subject.
    Identity,
    /// Pure overlay: only modifier changes (optional/readonly), no key/value transform.
    PureOverlay,
    /// Key filter: `Pick`/`Omit`-style literal key filtering.
    KeyFilter,
    /// Key remap: template literal or case transform on keys.
    KeyRemap,
}

/// How the declaration filters its source keyspace (classified at prep time).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PreparedKeyFilterShape {
    #[default]
    All,
    IncludeLiteral(Vec<String>),
    ExcludeLiteral(Vec<String>),
    Opaque(TypeExpr),
}

/// How the declaration remaps key names (classified at prep time).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PreparedKeyRemapShape {
    #[default]
    Identity,
    Prefix(String),
    Suffix(String),
    CaseTransform(PreparedCaseTransformKind),
    Opaque(TypeExpr),
}

/// Case transform kinds for key remapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedCaseTransformKind {
    Capitalize,
    Uncapitalize,
    Uppercase,
    Lowercase,
}

/// How the declaration transforms member values (classified at prep time).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PreparedValueRuleShape {
    /// Value is `T[K]` — pass through unchanged.
    #[default]
    PassThrough,
    /// Value involves a transform over `T[K]`.
    Transform(TypeExpr),
}

/// Surface modifiers (optional/readonly) for structural wrapper classification.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreparedSurfaceModifiers {
    /// `Some(true)` = add optional, `Some(false)` = remove optional, `None` = unchanged.
    pub optional: Option<bool>,
    /// `Some(true)` = add readonly, `Some(false)` = remove readonly, `None` = unchanged.
    pub readonly: Option<bool>,
}

// ---------------------------------------------------------------------------
// Projection classification
// ---------------------------------------------------------------------------

/// Projection-oriented classification of a prepared type declaration.
///
/// Computed at prep time alongside `wrapper_shape`. Determines how the solver
/// can project individual members without fully instantiating the declaration.
///
/// This is intentionally separate from `PreparedWrapperShape` which classifies
/// mapped-type structural patterns. Projection classification covers the
/// broader question of how member access should be routed.
#[derive(Debug, Clone, Default)]
pub enum PreparedProjectionClass {
    /// Declaration has a `member_index` — project directly from it.
    DirectMembers,
    /// Declaration is a structural wrapper (identity, overlay, key filter, etc.).
    /// Projection delegates through the wrapper shape.
    Wrapper,
    /// Declaration body is a single `Ref` to another type, possibly with args.
    /// Projection can forward to the target without full instantiation.
    ForwardSubject(PreparedForwardPayload),
    /// Cannot be projected structurally — fall back to full instantiation.
    #[default]
    Opaque,
}

/// Structured forwarding payload for `PreparedProjectionClass::ForwardSubject`.
///
/// Stores the target type reference and its arguments as symbolic `TypeExpr`
/// values (not arena `NodeId`s), because this metadata is computed at prep
/// time before any request arena exists.
#[derive(Debug, Clone)]
pub struct PreparedForwardPayload {
    /// Target type name (e.g., `"ComponentConfig"`).
    pub target_name: String,
    /// Symbolic type arguments passed to the target in alias scope.
    /// For `type A = B<X, Y>`, this is `[X, Y]` as `TypeExpr` values.
    pub target_args: Vec<TypeExpr>,
    /// How the alias's own type parameters map to the forwarded args.
    pub forwarding_kind: PreparedForwardingKind,
}

/// How an alias's type parameters relate to the forwarded target's arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedForwardingKind {
    /// Args are exactly the alias's own params in order: `type A<T> = B<T>`.
    /// The alias is structurally transparent for projection purposes.
    IdentityParams,
    /// Args include concrete types or reordered/partial params:
    /// `type A = B<X, Y>` or `type A<T> = B<T, string>`.
    AppliedAlias,
}

/// A member in the prepared member index — pre-extracted from the declaration
/// body for O(1) property lookup.
#[derive(Debug, Clone)]
pub struct PreparedMember {
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
    /// Declared accessibility of the member, carried verbatim from the IR
    /// [`verter_type_expr::ObjectProperty::visibility`] /
    /// [`verter_type_expr::MethodSignature::visibility`]. `Public` for every
    /// non-class origin; class members carry their `TSAccessibility`. The
    /// published-prop surface re-applies a `Public`-only filter at the
    /// publication boundary, so non-public class members stay recorded here.
    pub visibility: verter_type_expr::MemberVisibility,
    /// OXC declaration-site spans of this member, carried verbatim from the
    /// IR [`verter_type_expr::ObjectProperty::spans`] /
    /// [`verter_type_expr::ObjectMethod::spans`] so the macro-surface
    /// own-member overlay (`backfill_member_index_surface`) appends members
    /// with their real spans instead of `MemberSpans::default()`.
    pub spans: verter_type_expr::MemberSpans,
    /// Canonical file the member's declaration lives in — the defining file of
    /// the owning [`PreparedTypeDecl`] (its `root_identity.canonical_id`),
    /// stamped at `build_member_index`. The overlay pairs the member's
    /// `spans` with this file. Empty for a member indexed without a known
    /// defining file (test-only fixtures).
    pub declaration_origin: String,
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
            wrapper_shape: PreparedWrapperShape::default(),
            projection_class: PreparedProjectionClass::default(),
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
        // The defining file of this declaration is the declaration site of
        // every own-body member it indexes (heritage Ref parts are skipped),
        // so each `PreparedMember` is stamped with it. The macro-surface
        // overlay pairs the member's `spans` with this file.
        let declaration_origin = self.root_identity.canonical_id.clone();
        Self::index_transparent_object_members(
            &mut self.member_index,
            &self.body,
            &declaration_origin,
        );
    }

    /// Index direct object members into the member_index map.
    /// Existing entries are NOT overwritten (preserves right-to-left precedence
    /// when called from intersection traversal).
    fn index_object_members(
        member_index: &mut rustc_hash::FxHashMap<String, PreparedMember>,
        obj: &verter_type_expr::ObjectExpr,
        declaration_origin: &str,
    ) {
        for member in &obj.properties {
            match member {
                ObjectMember::Property(prop) => {
                    // entry API: only insert if not already present
                    member_index
                        .entry(prop.name.clone())
                        .or_insert_with(|| PreparedMember {
                            ty: prop.ty.clone(),
                            optional: prop.optional,
                            readonly: prop.readonly,
                            is_method: false,
                            // Carry the IR property's declared accessibility.
                            visibility: prop.visibility,
                            // Carry the IR property's OXC declaration-site
                            // spans + this declaration's defining file so the
                            // overlay append is span-rich.
                            spans: prop.spans,
                            declaration_origin: declaration_origin.to_string(),
                        });
                }
                ObjectMember::Method(method) => {
                    // Own method members are also direct own-body members
                    // — index them so the macro-surface own-member overlay
                    // (`build_instantiate` → `backfill_member_index_surface`)
                    // can stamp `declared_in_macro_type_arg` for an own
                    // interface method (e.g. `interface Slots { default(): VNode[] }`).
                    // The value is the method's function shape, mirroring
                    // the generic object lowering that materialises methods
                    // as canonical `Function` nodes.
                    member_index
                        .entry(method.name.clone())
                        .or_insert_with(|| PreparedMember {
                            ty: verter_type_expr::TypeExpr::Function(std::sync::Arc::new(
                                method.function.clone(),
                            )),
                            optional: method.optional,
                            readonly: false,
                            is_method: true,
                            // Carry the IR method's declared accessibility.
                            visibility: method.visibility,
                            // Carry the IR method's OXC member spans + defining
                            // file.
                            spans: method.spans,
                            declaration_origin: declaration_origin.to_string(),
                        });
                }
                _ => {}
            }
        }
    }

    fn index_transparent_object_members(
        member_index: &mut rustc_hash::FxHashMap<String, PreparedMember>,
        body: &TypeExpr,
        declaration_origin: &str,
    ) {
        match body {
            TypeExpr::Object(obj) => {
                Self::index_object_members(member_index, obj, declaration_origin)
            }
            TypeExpr::Intersection(parts) => {
                for part in parts.iter().rev() {
                    Self::index_transparent_object_members(member_index, part, declaration_origin);
                }
            }
            TypeExpr::Parenthesized(inner) => {
                Self::index_transparent_object_members(member_index, inner, declaration_origin);
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

    /// Classify the structural wrapper shape from the body and type parameters.
    ///
    /// Must be called after `type_parameters` is populated. Sets `self.wrapper_shape`.
    pub fn classify_wrapper_shape(&mut self) {
        self.wrapper_shape = classify_wrapper_shape_inner(&self.body, &self.type_parameters);
    }

    /// Classify the projection class from the body, member index, and wrapper shape.
    ///
    /// Must be called after `build_member_index()` and `classify_wrapper_shape()`.
    /// Sets `self.projection_class`.
    pub fn classify_projection(&mut self) {
        self.projection_class = classify_projection_inner(
            &self.body,
            &self.type_parameters,
            &self.member_index,
            &self.wrapper_shape,
        );
    }
}

/// Check if a TypeExpr is a bare `Ref` to the given name with no type args.
fn is_bare_ref(expr: &TypeExpr, name: &str) -> bool {
    matches!(expr, TypeExpr::Ref { name: n, type_arguments } if &**n == name && type_arguments.is_empty())
}

/// Check if a TypeExpr is `T[K]` (indexed access of base by param).
fn is_passthrough_value(value: &TypeExpr, base_name: &str, param_name: &str) -> bool {
    match value {
        TypeExpr::IndexedAccess { object, index } => {
            is_bare_ref(object, base_name) && is_bare_ref(index, param_name)
        }
        _ => false,
    }
}

/// Classify the body of a mapped type declaration into a `PreparedWrapperShape`.
fn classify_wrapper_shape_inner(
    body: &TypeExpr,
    type_params: &[TypeParam],
) -> PreparedWrapperShape {
    // Only classify mapped type bodies with at least one type param
    let (param, source, value, optional, readonly, name_type) = match body {
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => (parameter, source, value, optional, readonly, name_type),
        _ => {
            // Non-mapped body — not a structural wrapper. Alias forwarding
            // is handled by PreparedProjectionClass::ForwardSubject instead.
            return PreparedWrapperShape::default();
        }
    };

    // Source must be `keyof T` where T is a type parameter
    let base_param = match &**source {
        TypeExpr::KeyOf(inner) => match &**inner {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => type_params
                .iter()
                .position(|tp| tp.name == **name)
                .map(|idx| (idx, &**name)),
            _ => None,
        },
        _ => None,
    };

    let (source_param_index, base_name) = match base_param {
        Some((idx, name)) => (idx, name),
        None => return PreparedWrapperShape::default(),
    };

    // Classify optional/readonly modifiers
    let opt_mod = match optional {
        MappedModifier::None => None,
        MappedModifier::Add => Some(true),
        MappedModifier::Remove => Some(false),
    };
    let ro_mod = match readonly {
        MappedModifier::None => None,
        MappedModifier::Add => Some(true),
        MappedModifier::Remove => Some(false),
    };
    let modifiers = PreparedSurfaceModifiers {
        optional: opt_mod,
        readonly: ro_mod,
    };

    // Check value rule: is it `T[K]` (passthrough)?
    let value_rule = if is_passthrough_value(value, base_name, param) {
        PreparedValueRuleShape::PassThrough
    } else {
        PreparedValueRuleShape::Transform((**value).clone())
    };

    // Check name_type for key remap
    let key_remap = match name_type {
        None => PreparedKeyRemapShape::Identity,
        Some(nt) => classify_key_remap(nt, param),
    };

    // Determine the kind
    let is_passthrough = matches!(value_rule, PreparedValueRuleShape::PassThrough);
    let is_identity_remap = matches!(key_remap, PreparedKeyRemapShape::Identity);

    let kind = if is_passthrough && is_identity_remap && opt_mod.is_none() && ro_mod.is_none() {
        PreparedWrapperKind::Identity
    } else if is_passthrough && is_identity_remap {
        PreparedWrapperKind::PureOverlay
    } else if !is_identity_remap {
        PreparedWrapperKind::KeyRemap
    } else {
        PreparedWrapperKind::None
    };

    PreparedWrapperShape {
        kind,
        source_param_index: Some(source_param_index as u16),
        key_filter: PreparedKeyFilterShape::All,
        key_remap,
        value_rule,
        modifiers,
    }
}

/// Classify key remap from a name_type expression.
fn classify_key_remap(name_type: &TypeExpr, param: &str) -> PreparedKeyRemapShape {
    match name_type {
        // `` `prefix${K & string}` `` or `` `${K & string}suffix` ``
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            // Check for single expression that is K & string or just K
            if expressions.len() == 1 && quasis.len() == 2 {
                let expr_is_param = is_param_or_param_intersect_string(&expressions[0], param);
                if expr_is_param {
                    let prefix = &quasis[0];
                    let suffix = &quasis[1];
                    if suffix.is_empty() && !prefix.is_empty() {
                        return PreparedKeyRemapShape::Prefix(prefix.clone());
                    }
                    if prefix.is_empty() && !suffix.is_empty() {
                        return PreparedKeyRemapShape::Suffix(suffix.clone());
                    }
                }
            }
            PreparedKeyRemapShape::Opaque(name_type.clone())
        }
        // `Capitalize<K & string>` etc.
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.len() == 1 => {
            let arg_is_param = is_param_or_param_intersect_string(&type_arguments[0], param);
            if arg_is_param {
                match &**name {
                    "Capitalize" => {
                        return PreparedKeyRemapShape::CaseTransform(
                            PreparedCaseTransformKind::Capitalize,
                        )
                    }
                    "Uncapitalize" => {
                        return PreparedKeyRemapShape::CaseTransform(
                            PreparedCaseTransformKind::Uncapitalize,
                        )
                    }
                    "Uppercase" => {
                        return PreparedKeyRemapShape::CaseTransform(
                            PreparedCaseTransformKind::Uppercase,
                        )
                    }
                    "Lowercase" => {
                        return PreparedKeyRemapShape::CaseTransform(
                            PreparedCaseTransformKind::Lowercase,
                        )
                    }
                    _ => {}
                }
            }
            PreparedKeyRemapShape::Opaque(name_type.clone())
        }
        _ => PreparedKeyRemapShape::Opaque(name_type.clone()),
    }
}

/// Check if an expression is `K` or `K & string` (common in mapped type name remaps).
fn is_param_or_param_intersect_string(expr: &TypeExpr, param: &str) -> bool {
    // Direct param ref
    if is_bare_ref(expr, param) {
        return true;
    }
    // K & string
    if let TypeExpr::Intersection(parts) = expr {
        if parts.len() == 2 {
            let has_param = parts.iter().any(|p| is_bare_ref(p, param));
            let has_string = parts
                .iter()
                .any(|p| matches!(p, TypeExpr::Primitive(PrimitiveName::String)));
            return has_param && has_string;
        }
    }
    false
}

/// Classify the projection class for a prepared type declaration.
///
/// Priority order:
/// 1. If `member_index` is non-empty and the body only contains direct object
///    members → `DirectMembers` (interfaces, object aliases).
/// 2. If `wrapper_shape.kind` is a recognized structural wrapper → `Wrapper`.
/// 3. If the body is a single `Ref` (possibly parenthesized) → `ForwardSubject`.
/// 4. Otherwise → `Opaque`.
fn classify_projection_inner(
    body: &TypeExpr,
    type_params: &[TypeParam],
    member_index: &FxHashMap<String, PreparedMember>,
    wrapper_shape: &PreparedWrapperShape,
) -> PreparedProjectionClass {
    // 1. Direct members — interfaces and object-bodied aliases.
    if !member_index.is_empty() && body_supports_direct_member_projection(body) {
        return PreparedProjectionClass::DirectMembers;
    }

    // 2. Structural wrapper — mapped types with recognized patterns.
    if !matches!(wrapper_shape.kind, PreparedWrapperKind::None) {
        return PreparedProjectionClass::Wrapper;
    }

    // 3. Forward subject — body is a single Ref to another type.
    if let Some(payload) = extract_forward_payload(body, type_params) {
        return PreparedProjectionClass::ForwardSubject(payload);
    }

    PreparedProjectionClass::Opaque
}

fn body_supports_direct_member_projection(body: &TypeExpr) -> bool {
    match body {
        TypeExpr::Object(_) => true,
        TypeExpr::Intersection(parts) => parts.iter().all(body_supports_direct_member_projection),
        TypeExpr::Parenthesized(inner) => body_supports_direct_member_projection(inner),
        _ => false,
    }
}

/// Try to extract a forward-subject payload from a declaration body.
///
/// Matches bodies of the form `Ref { name, type_arguments }` (allowing
/// `Parenthesized` wrapping). Returns `None` for unions, intersections,
/// conditionals, mapped types, objects, and other non-forwarding shapes.
fn extract_forward_payload(
    body: &TypeExpr,
    type_params: &[TypeParam],
) -> Option<PreparedForwardPayload> {
    match body {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let forwarding_kind = classify_forwarding_kind(type_arguments, type_params);
            Some(PreparedForwardPayload {
                target_name: name.to_string(),
                target_args: type_arguments.to_vec(),
                forwarding_kind,
            })
        }
        TypeExpr::Parenthesized(inner) => extract_forward_payload(inner, type_params),
        _ => None,
    }
}

/// Determine whether the forwarded args are an identity pass-through of the
/// alias's own type parameters, or an applied (concrete/remapped) alias.
fn classify_forwarding_kind(
    target_args: &[TypeExpr],
    alias_params: &[TypeParam],
) -> PreparedForwardingKind {
    // Identity: args must be exactly the alias params in order, with no extras.
    if !alias_params.is_empty()
        && target_args.len() == alias_params.len()
        && target_args
            .iter()
            .zip(alias_params.iter())
            .all(|(arg, param)| is_bare_ref(arg, &param.name))
    {
        PreparedForwardingKind::IdentityParams
    } else {
        PreparedForwardingKind::AppliedAlias
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

    use verter_type_expr::{LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName};

    use super::*;

    #[test]
    fn prepared_type_decl_member_index_from_object_body() {
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic(
                    "label".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic(
                    "count".into(),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    true,
                    false,
                )),
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
    fn prepared_type_decl_member_index_indexes_method_syntax() {
        // Method-syntax members (`default(props): any`) must be indexed
        // distinctly from property-valued functions (`fn: () => void`),
        // carrying `is_method == true`, so the macro-surface own-member
        // overlay can stamp `declared_in_macro_type_arg` for an own
        // interface method. A property and a method coexist in one body to
        // prove BOTH branches of `index_object_members` run.
        //
        // Discrimination: removing the `ObjectMember::Method` arm from
        // `index_object_members` makes `decl.member("greet")` return `None`
        // (the method is never indexed), so both the `is_some` and the
        // `is_method` assertions below FAIL.
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic(
                    "label".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                )),
                ObjectMember::Method(verter_type_expr::MethodSignature::synthetic(
                    "greet".into(),
                    verter_type_expr::FunctionExpr::synthetic(
                        vec![],
                        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                        vec![],
                    ),
                    false,
                )),
            ],
        }));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Slots"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        // The property member is indexed as a non-method.
        let label = decl.member("label").expect("property `label` indexed");
        assert!(
            !label.is_method,
            "a property member must carry is_method=false",
        );

        // The method-syntax member is indexed AND flagged is_method=true.
        let greet = decl
            .member("greet")
            .expect("method-syntax member `greet` MUST be indexed (the Method branch)");
        assert!(
            greet.is_method,
            "a method-syntax member (`greet(): any`) MUST carry is_method=true; \
             a property-valued function would carry is_method=false",
        );
        // The method's value is its function shape.
        assert!(
            matches!(greet.ty, TypeExpr::Function(_)),
            "the method member's value type must be a Function shape, got {:?}",
            greet.ty,
        );
    }

    #[test]
    fn prepared_member_index_carries_spans_and_declaration_origin() {
        // The member-index producer (`index_object_members`) must carry the IR
        // member's OXC declaration-site spans AND stamp the declaration's
        // defining file (`root_identity.canonical_id`) onto each
        // `PreparedMember`. The macro-surface overlay
        // (`backfill_member_index_surface` in verter_session) reads these so an
        // appended own-body member reaches the graph `SurfaceMember` span-rich
        // instead of `MemberSpans::default()` (the codex#2 P1 / Claude P2-b
        // finding).
        //
        // Discrimination: before the fix `PreparedMember` had no `spans` field
        // (it could not carry them) and no `declaration_origin`; the producer
        // dropped `prop.spans` / `method.spans`. This test pins BOTH the
        // property and method branches carrying NON-default spans + the
        // defining file. If the producer reverted to dropping spans, the
        // `prop_spans.name` / `method_spans.declaration` assertions FAIL (they
        // would be `None`), and the `declaration_origin` equality FAILS.
        use verter_span::Span;
        use verter_type_expr::MemberSpans;

        let prop_spans = MemberSpans {
            declaration: Some(Span::new(10, 30)),
            name: Some(Span::new(10, 15)),
            type_annotation: Some(Span::new(17, 30)),
        };
        let method_spans = MemberSpans {
            declaration: Some(Span::new(40, 60)),
            name: Some(Span::new(40, 45)),
            type_annotation: None,
        };
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::with_spans(
                    "label".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                    prop_spans,
                )),
                ObjectMember::Method(verter_type_expr::MethodSignature::with_spans(
                    "greet".into(),
                    verter_type_expr::FunctionExpr::synthetic(
                        vec![],
                        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                        vec![],
                    ),
                    false,
                    method_spans,
                )),
            ],
        }));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/decl_origin.ts", "Slots"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();

        // PROPERTY member: real spans + the declaration's defining file.
        let label = decl.member("label").expect("property `label` indexed");
        assert_eq!(
            label.spans, prop_spans,
            "PreparedMember must carry the property's OXC spans verbatim, not default()"
        );
        assert_eq!(
            label.declaration_origin, "/decl_origin.ts",
            "PreparedMember must stamp the declaration's defining file"
        );

        // METHOD member: real spans + the declaration's defining file.
        let greet = decl.member("greet").expect("method `greet` indexed");
        assert_eq!(
            greet.spans, method_spans,
            "PreparedMember must carry the method's OXC spans verbatim, not default()"
        );
        assert_eq!(greet.declaration_origin, "/decl_origin.ts");

        // NEGATIVE: a genuinely-absent member is still absent (the producer did
        // not fabricate entries).
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
                    ObjectMember::Property(ObjectProperty::synthetic(
                        (*name).into(),
                        ty.clone(),
                        *optional,
                        false,
                    ))
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

    // -----------------------------------------------------------------------
    // Wrapper shape classification tests
    // -----------------------------------------------------------------------

    fn make_type_param(name: &str) -> TypeParam {
        TypeParam {
            name: name.into(),
            constraint: None,
            default: None,
        }
    }

    /// Helper: `{ [K in keyof T]: T[K] }` — identity mapped type
    fn identity_mapped_body(base: &str, param: &str) -> TypeExpr {
        TypeExpr::Mapped {
            parameter: param.into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named(base)))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named(base)),
                index: Arc::new(TypeExpr::named(param)),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: None,
        }
    }

    #[test]
    fn classify_identity_mapped_type() {
        let body = identity_mapped_body("T", "K");
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Identity"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::Identity);
        assert_eq!(decl.wrapper_shape.source_param_index, Some(0));
        assert!(matches!(
            decl.wrapper_shape.value_rule,
            PreparedValueRuleShape::PassThrough
        ));
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShape::Identity
        ));
        // Negative: must not be PureOverlay
        assert_ne!(decl.wrapper_shape.kind, PreparedWrapperKind::PureOverlay);
    }

    #[test]
    fn classify_partial_overlay() {
        // { [K in keyof T]?: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::Add,
            readonly: MappedModifier::None,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "MyPartial"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::PureOverlay);
        assert_eq!(decl.wrapper_shape.modifiers.optional, Some(true));
        // Negative: readonly unchanged, key remap is identity, value is passthrough
        assert_eq!(decl.wrapper_shape.modifiers.readonly, None);
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShape::Identity
        ));
        assert!(matches!(
            decl.wrapper_shape.value_rule,
            PreparedValueRuleShape::PassThrough
        ));
    }

    #[test]
    fn classify_required_overlay() {
        // { [K in keyof T]-?: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::Remove,
            readonly: MappedModifier::None,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "MyRequired"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::PureOverlay);
        assert_eq!(decl.wrapper_shape.modifiers.optional, Some(false));
        // Negative: readonly unchanged
        assert_eq!(decl.wrapper_shape.modifiers.readonly, None);
    }

    #[test]
    fn classify_readonly_overlay() {
        // { readonly [K in keyof T]: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::Add,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "MyReadonly"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::PureOverlay);
        assert_eq!(decl.wrapper_shape.modifiers.readonly, Some(true));
        // Negative: optional unchanged
        assert_eq!(decl.wrapper_shape.modifiers.optional, None);
    }

    #[test]
    fn classify_prefix_key_remap() {
        // { [K in keyof T as `data-${K & string}`]: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::TemplateLiteral {
                quasis: vec!["data-".into(), String::new()],
                expressions: Arc::from(vec![TypeExpr::Intersection(Arc::from(vec![
                    TypeExpr::named("K"),
                    TypeExpr::Primitive(PrimitiveName::String),
                ]))]),
            })),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "DataPrefixed"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::KeyRemap);
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShape::Prefix(ref p) if p == "data-"
        ));
        // Negative: value is still passthrough
        assert!(matches!(
            decl.wrapper_shape.value_rule,
            PreparedValueRuleShape::PassThrough
        ));
    }

    #[test]
    fn classify_capitalize_key_remap() {
        // { [K in keyof T as Capitalize<K & string>]: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::named_with_args(
                "Capitalize",
                vec![TypeExpr::Intersection(Arc::from(vec![
                    TypeExpr::named("K"),
                    TypeExpr::Primitive(PrimitiveName::String),
                ]))],
            ))),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "CapKeys"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::KeyRemap);
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShape::CaseTransform(PreparedCaseTransformKind::Capitalize)
        ));
    }

    #[test]
    fn classify_identity_alias_not_wrapper() {
        // type Alias<T, U> = Other<T, U>
        // This is an identity-forwarding alias, NOT a structural wrapper.
        // Handled by PreparedProjectionClass::ForwardSubject(IdentityParams).
        let body =
            TypeExpr::named_with_args("Other", vec![TypeExpr::named("T"), TypeExpr::named("U")]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Alias"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T"), make_type_param("U")];
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        // Wrapper shape: None — not a mapped type
        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::None);
        assert_eq!(decl.wrapper_shape.source_param_index, None);
        // Projection class: ForwardSubject(IdentityParams)
        match &decl.projection_class {
            PreparedProjectionClass::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "Other");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::IdentityParams
                );
            }
            other => panic!("expected ForwardSubject(IdentityParams), got {:?}", other),
        }
    }

    #[test]
    fn classify_non_structural_body() {
        // type Foo<T> = T extends string ? T : never
        let body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::named("T")),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::named("T")),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Foo"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::None);
        // Negative: source_param_index is None for non-structural bodies
        assert_eq!(decl.wrapper_shape.source_param_index, None);
    }

    #[test]
    fn classify_value_transform_not_identity() {
        // { [K in keyof T]: Wrap<T[K]> } — value transform, not identity
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::named_with_args(
                "Wrap",
                vec![TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::named("T")),
                    index: Arc::new(TypeExpr::named("K")),
                }],
            )),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Wrapped"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![make_type_param("T")];
        decl.classify_wrapper_shape();

        // Has a value transform, so not Identity or PureOverlay
        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::None);
        assert!(matches!(
            decl.wrapper_shape.value_rule,
            PreparedValueRuleShape::Transform(_)
        ));
    }

    #[test]
    fn classify_no_type_params_is_none() {
        // type Foo = string — no type params, no wrapper
        let body = TypeExpr::Primitive(PrimitiveName::String);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Foo"),
            TypeDeclKind::Alias,
            body,
        );
        decl.classify_wrapper_shape();

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKind::None);
        // Negative: no source param for non-generic types
        assert_eq!(decl.wrapper_shape.source_param_index, None);
    }

    // -----------------------------------------------------------------------
    // Projection classification tests
    // -----------------------------------------------------------------------

    #[test]
    fn projection_interface_is_direct_members() {
        // interface Props { msg: string }
        let body = make_object(&[("msg", TypeExpr::Primitive(PrimitiveName::String), false)]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Props"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClass::DirectMembers
        ));
    }

    #[test]
    fn projection_object_alias_is_direct_members() {
        // type Props = { msg: string }
        let body = make_object(&[("msg", TypeExpr::Primitive(PrimitiveName::String), false)]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Props"),
            TypeDeclKind::Alias,
            body,
        );
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClass::DirectMembers
        ));
    }

    #[test]
    fn projection_identity_alias_is_forward_identity() {
        // type A<T> = B<T>
        let body = TypeExpr::Ref {
            name: "B".into(),
            type_arguments: vec![TypeExpr::named("T")].into(),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "A"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![TypeParam {
            name: "T".into(),
            constraint: None,
            default: None,
        }];
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        match &decl.projection_class {
            PreparedProjectionClass::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "B");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::IdentityParams
                );
            }
            other => panic!("expected ForwardSubject(IdentityParams), got {:?}", other),
        }
    }

    #[test]
    fn projection_concrete_alias_is_forward_applied() {
        // type ChatShimmer = ComponentConfig<Theme, AppConfig, 'chatShimmer'>
        let body = TypeExpr::Ref {
            name: "ComponentConfig".into(),
            type_arguments: vec![
                TypeExpr::named("Theme"),
                TypeExpr::named("AppConfig"),
                TypeExpr::string_literal("chatShimmer"),
            ]
            .into(),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "ChatShimmer"),
            TypeDeclKind::Alias,
            body,
        );
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        match &decl.projection_class {
            PreparedProjectionClass::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "ComponentConfig");
                assert_eq!(payload.target_args.len(), 3);
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::AppliedAlias
                );
            }
            other => panic!("expected ForwardSubject(AppliedAlias), got {:?}", other),
        }
    }

    #[test]
    fn projection_partial_remap_alias_is_forward_applied() {
        // type A<T> = B<T, string>
        let body = TypeExpr::Ref {
            name: "B".into(),
            type_arguments: vec![
                TypeExpr::named("T"),
                TypeExpr::Primitive(PrimitiveName::String),
            ]
            .into(),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "A"),
            TypeDeclKind::Alias,
            body,
        );
        decl.type_parameters = vec![TypeParam {
            name: "T".into(),
            constraint: None,
            default: None,
        }];
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        match &decl.projection_class {
            PreparedProjectionClass::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "B");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::AppliedAlias
                );
            }
            other => panic!("expected ForwardSubject(AppliedAlias), got {:?}", other),
        }
    }

    #[test]
    fn projection_union_is_opaque() {
        // type A = string | number — not a forward subject
        let body = TypeExpr::Union(
            vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]
            .into(),
        );
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "A"),
            TypeDeclKind::Alias,
            body,
        );
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClass::Opaque
        ));
    }

    #[test]
    fn projection_intersection_is_opaque() {
        // type A = B & C — not a forward subject
        let body = TypeExpr::Intersection(vec![TypeExpr::named("B"), TypeExpr::named("C")].into());
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "A"),
            TypeDeclKind::Alias,
            body,
        );
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClass::Opaque
        ));
    }

    #[test]
    fn projection_interface_with_heritage_and_own_members_is_opaque() {
        // interface Props extends Base { own: string }
        let body = TypeExpr::intersection(vec![
            TypeExpr::named("Base"),
            make_object(&[("own", TypeExpr::Primitive(PrimitiveName::String), false)]),
        ]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Props"),
            TypeDeclKind::Interface,
            body,
        );
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClass::Opaque
        ));
    }

    #[test]
    fn projection_parenthesized_ref_is_forward() {
        // type A = (B<X>) — parenthesized refs still classify as forwarded
        let body = TypeExpr::Parenthesized(Arc::new(TypeExpr::Ref {
            name: "B".into(),
            type_arguments: vec![TypeExpr::named("X")].into(),
        }));
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "A"),
            TypeDeclKind::Alias,
            body,
        );
        decl.build_member_index();
        decl.classify_wrapper_shape();
        decl.classify_projection();

        match &decl.projection_class {
            PreparedProjectionClass::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "B");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::AppliedAlias
                );
            }
            other => panic!("expected ForwardSubject, got {:?}", other),
        }
    }
}
