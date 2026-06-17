//! Opaque structural-carrier payloads + their sanctioned accessor surface.
//!
//! The three structural carriers that apply type arguments at a reference
//! site — [`TypeOfCarrier`] (`typeof f<Arg>`), [`BareRefCarrier`]
//! (`Foo<Arg>`), and [`ImportTypeCarrier`] (`import("m").G<Arg>`) — store
//! their fields PRIVATELY. The anti-tail invariant (no production site
//! hand-binds a carrier's `type_args` field, bypassing the sanctioned descent
//! accessor [`SemanticNodeData::carrier_type_args`]) is therefore enforced BY
//! CONSTRUCTION: a `carrier.type_args` field bind is unrepresentable outside
//! this module, regardless of `cfg` / `#[path]` / `include!` / macro / alias —
//! exactly the rustc module-resolution surface a source scanner could never
//! fully model. The compiler enforces the boundary on the real compiled
//! program.
//!
//! STRUCTURAL CONFINEMENT. The sanctioned
//! crate-visible accessor surface lives INSIDE this module, in the
//! `impl SemanticNodeData` block below: `carrier_type_args` /
//! `map_carrier_type_args` (`pub(crate)`), `new_typeof` / `new_bare_ref` /
//! `new_import_type` (`pub`), and `typeof_head` / `bare_ref_head` /
//! `import_type_head` (`pub(crate)`). The carriers' OWN payload methods
//! (`new`, the head getters, `arg_nodes`, `with_type_args`) are PRIVATE to
//! this module, so they are reachable ONLY from that accessor block — NOT from
//! the ~6000-line parent `semantic_query` module. The raw-args surface is thus
//! COMPILER-CONFINED to `carrier.rs`: a sibling `impl carrier::BareRefCarrier`
//! in `semantic_query.rs` that reads `self.type_args` — or calls the private
//! `arg_nodes` — fails to compile (`E0616`, field is private). That confinement
//! is what makes the local shape guard
//! `carrier_module_has_no_public_type_args_surface` (scoped to `carrier.rs`)
//! COMPLETE: the single module it scans IS the entire raw-args surface.
//!
//! Access discipline:
//!
//! - Construction ([`TypeOfCarrier::new`] etc.) is PRIVATE and passes the
//!   args slice IN — construction is not a tail; the invariant bans
//!   DESCENT/REBIND outside the accessor. Carriers are built only through
//!   [`SemanticNodeData::new_typeof`] / `new_bare_ref` / `new_import_type`.
//! - Head fields (everything except the args) read through PRIVATE head
//!   getters that NEVER return `type_args`; the `*_head` accessors expose them.
//! - The raw args read ([`arg_nodes`](TypeOfCarrier::arg_nodes)) and the
//!   head-preserving rebuild ([`with_type_args`](TypeOfCarrier::with_type_args))
//!   are PRIVATE — so the sole crate-wide descent channel is
//!   [`SemanticNodeData::carrier_type_args`] and the sole rebuild channel is
//!   [`SemanticNodeData::map_carrier_type_args`].

use std::sync::Arc;

use super::{NodeScopeId, SemanticNodeData, SemanticNodeId, ValueRootKey};

/// Borrowed head view of a [`SemanticNodeData::TypeOf`] carrier —
/// `(value_root, path)`, NEVER its `type_args` (descend those through
/// [`SemanticNodeData::carrier_type_args`]).
pub(crate) type TypeOfHead<'a> = (&'a ValueRootKey, &'a Arc<[Arc<str>]>);

/// Borrowed head view of a [`SemanticNodeData::BareRef`] carrier —
/// `(name, scope)`, NEVER its `type_args`.
pub(crate) type BareRefHead<'a> = (&'a Arc<str>, &'a NodeScopeId);

/// Borrowed head view of a [`SemanticNodeData::ImportType`] carrier —
/// `(specifier, qualifier, typeof_query)`, NEVER its `type_args`.
pub(crate) type ImportTypeHead<'a> = (&'a Arc<str>, &'a Arc<[Arc<str>]>, bool);

/// Deferred `typeof value.path<args>` carrier payload.
///
/// `value_root` is the mode-free `typeof` query identity; `path` stores the
/// remaining dotted member segments projected from that root; `type_args`
/// carries any instantiation-expression arguments (`typeof C.make<string>`),
/// each already structurally lowered. Empty `type_args` is a bare `typeof`.
/// Mirrors [`verter_type_expr::ValueRef::type_args`]; carried unresolved so
/// the structural lowerer applies no instantiation at lowering time.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TypeOfCarrier {
    value_root: ValueRootKey,
    path: Arc<[Arc<str>]>,
    type_args: Arc<[SemanticNodeId]>,
}

impl TypeOfCarrier {
    fn new(
        value_root: ValueRootKey,
        path: Arc<[Arc<str>]>,
        type_args: Arc<[SemanticNodeId]>,
    ) -> Self {
        Self {
            value_root,
            path,
            type_args,
        }
    }

    fn value_root(&self) -> &ValueRootKey {
        &self.value_root
    }

    fn path(&self) -> &Arc<[Arc<str>]> {
        &self.path
    }

    /// Raw args read — PRIVATE to this module, so descent flows only through
    /// [`SemanticNodeData::carrier_type_args`].
    fn arg_nodes(&self) -> &[SemanticNodeId] {
        &self.type_args
    }

    /// Head-preserving rebuild — PRIVATE to this module, so reconstruction
    /// flows only through [`SemanticNodeData::map_carrier_type_args`].
    fn with_type_args(&self, type_args: Arc<[SemanticNodeId]>) -> Self {
        Self {
            value_root: self.value_root.clone(),
            path: self.path.clone(),
            type_args,
        }
    }
}

/// Unresolved bare-name reference carrier payload (`Foo` / `Foo<Arg>`).
///
/// `name` is the unresolved type name as written; `scope` is the lexical
/// scope the reference was captured in (declaration-origin file + content
/// generation + optional inner scope), so demand-time resolution can re-key
/// its bare-name lookup without a host query at lowering time; `type_args`
/// are the arguments applied at the reference site (`Foo<Arg>`), each already
/// structurally lowered (empty for a bare `Foo`). Carried unresolved so the
/// query-free structural lowerer can represent `Foo<Arg>` without performing
/// the eager `InstantiationRef` resolution.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BareRefCarrier {
    name: Arc<str>,
    scope: NodeScopeId,
    type_args: Arc<[SemanticNodeId]>,
}

impl BareRefCarrier {
    fn new(name: Arc<str>, scope: NodeScopeId, type_args: Arc<[SemanticNodeId]>) -> Self {
        Self {
            name,
            scope,
            type_args,
        }
    }

    fn name(&self) -> &Arc<str> {
        &self.name
    }

    fn scope(&self) -> &NodeScopeId {
        &self.scope
    }

    fn arg_nodes(&self) -> &[SemanticNodeId] {
        &self.type_args
    }

    fn with_type_args(&self, type_args: Arc<[SemanticNodeId]>) -> Self {
        Self {
            name: Arc::clone(&self.name),
            scope: self.scope.clone(),
            type_args,
        }
    }
}

/// Unresolved dynamic-import type carrier payload.
///
/// The typed-IR mirror of [`verter_type_expr::TypeExpr::ImportType`]:
/// `import("specifier").qualifier<type_args>` / `typeof import("specifier")`.
/// `specifier` is the module specifier inside `import("…")`; `qualifier` is
/// the dotted qualifier path after the import (empty for a bare module
/// reference); `type_args` are the arguments applied at the import-type site;
/// `typeof_query` is `true` for `typeof import("…")` (the module's
/// value-export namespace), `false` for `import("…")` in type position.
/// Carried unresolved so the structural graph never performs module
/// resolution at lowering time.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ImportTypeCarrier {
    specifier: Arc<str>,
    qualifier: Arc<[Arc<str>]>,
    type_args: Arc<[SemanticNodeId]>,
    typeof_query: bool,
}

impl ImportTypeCarrier {
    fn new(
        specifier: Arc<str>,
        qualifier: Arc<[Arc<str>]>,
        type_args: Arc<[SemanticNodeId]>,
        typeof_query: bool,
    ) -> Self {
        Self {
            specifier,
            qualifier,
            type_args,
            typeof_query,
        }
    }

    fn specifier(&self) -> &Arc<str> {
        &self.specifier
    }

    fn qualifier(&self) -> &Arc<[Arc<str>]> {
        &self.qualifier
    }

    fn typeof_query(&self) -> bool {
        self.typeof_query
    }

    fn arg_nodes(&self) -> &[SemanticNodeId] {
        &self.type_args
    }

    fn with_type_args(&self, type_args: Arc<[SemanticNodeId]>) -> Self {
        Self {
            specifier: Arc::clone(&self.specifier),
            qualifier: Arc::clone(&self.qualifier),
            type_args,
            typeof_query: self.typeof_query,
        }
    }
}

/// The sanctioned crate-visible carrier accessor surface. These eight methods
/// are the ONLY `pub`/`pub(crate)` methods in this module; every carrier
/// payload method above is PRIVATE, so this block
/// is the SOLE reachable path to construct a carrier, descend / rebuild its
/// args, or read its head fields — and it lives in `carrier.rs` alongside the
/// private payloads, confining the raw-args surface to this one module.
impl SemanticNodeData {
    /// The structural `type_args` carrier slice for the three unresolved
    /// carriers that apply type arguments at their reference site —
    /// [`BareRef`](Self::BareRef) (`Foo<Arg>`), [`TypeOf`](Self::TypeOf)
    /// (`typeof f<Arg>`), and [`ImportType`](Self::ImportType)
    /// (`import("m").G<Arg>`). Returns the empty slice for every other
    /// variant.
    ///
    /// This is the SINGLE structural accessor a graph walker uses to reach
    /// a carrier's type arguments, so a future carrier that grows a
    /// `type_args` field is covered by extending this one method rather than
    /// each walker hand-matching every carrier. The non-carrier arm is an
    /// EXHAUSTIVE enumeration with NO `_` wildcard: a new `SemanticNodeData`
    /// variant fails to compile HERE, forcing the author to classify it as a
    /// carrier (the first arm) or not (the second). A wildcard would instead
    /// silently return `&[]` for a future carrier that grew a `type_args`
    /// field, defeating that contract. It is PURE — no host / query /
    /// resolution: it only EXPOSES the args for structural descent
    /// (preservation, rendering, rewriting, scanning, classification) and
    /// never RESOLVES a carrier or APPLIES instantiation meaning (that is
    /// demand-time carrier resolution).
    #[must_use]
    pub(crate) fn carrier_type_args(&self) -> &[SemanticNodeId] {
        match self {
            // The opaque carriers expose their args ONLY through this single
            // accessor: `arg_nodes` is PRIVATE to this module, so no other
            // module can descend a carrier's `type_args`.
            Self::BareRef(c) => c.arg_nodes(),
            Self::TypeOf(c) => c.arg_nodes(),
            Self::ImportType(c) => c.arg_nodes(),
            // EXHAUSTIVE non-carrier enumeration — NO `_` wildcard, so a new
            // variant forces a compile error at this accessor (see docstring).
            Self::Alias(_)
            | Self::Object(_)
            | Self::Union(_)
            | Self::Intersection(_)
            | Self::Primitive(_)
            | Self::Literal(_)
            | Self::Opaque(_)
            | Self::Array { .. }
            | Self::Tuple { .. }
            | Self::TemplateLiteral { .. }
            | Self::KeyOf { .. }
            | Self::IndexedAccess { .. }
            | Self::Mapped { .. }
            | Self::TypeParam { .. }
            | Self::Infer { .. }
            | Self::MergedDecl { .. }
            | Self::Conditional { .. }
            | Self::VueMacroElements(_)
            | Self::Function { .. }
            | Self::DeclRef { .. }
            | Self::InstantiationRef { .. }
            | Self::RawFallback { .. }
            | Self::ConstructorType { .. }
            | Self::SyntheticBinding { .. } => &[],
        }
    }

    /// Construct a [`TypeOf`](Self::TypeOf) carrier (`typeof value.path<args>`).
    /// `type_args` is carried IN at construction — the anti-tail rule bans
    /// DESCENT / REBIND outside [`carrier_type_args`](Self::carrier_type_args)
    /// / [`map_carrier_type_args`](Self::map_carrier_type_args), not
    /// construction.
    #[must_use]
    pub fn new_typeof(
        value_root: ValueRootKey,
        path: Arc<[Arc<str>]>,
        type_args: Arc<[SemanticNodeId]>,
    ) -> Self {
        Self::TypeOf(TypeOfCarrier::new(value_root, path, type_args))
    }

    /// Construct a [`BareRef`](Self::BareRef) carrier (`Foo` / `Foo<Arg>`).
    #[must_use]
    pub fn new_bare_ref(
        name: Arc<str>,
        scope: NodeScopeId,
        type_args: Arc<[SemanticNodeId]>,
    ) -> Self {
        Self::BareRef(BareRefCarrier::new(name, scope, type_args))
    }

    /// Construct an [`ImportType`](Self::ImportType) carrier
    /// (`import("m").qualifier<args>` / `typeof import("m")`).
    #[must_use]
    pub fn new_import_type(
        specifier: Arc<str>,
        qualifier: Arc<[Arc<str>]>,
        type_args: Arc<[SemanticNodeId]>,
        typeof_query: bool,
    ) -> Self {
        Self::ImportType(ImportTypeCarrier::new(
            specifier,
            qualifier,
            type_args,
            typeof_query,
        ))
    }

    /// Head fields of a [`TypeOf`](Self::TypeOf) carrier — `(value_root, path)`.
    /// NEVER returns `type_args` (descend those through
    /// [`carrier_type_args`](Self::carrier_type_args)). `None` for any
    /// non-`TypeOf` node.
    #[must_use]
    pub(crate) fn typeof_head(&self) -> Option<TypeOfHead<'_>> {
        match self {
            Self::TypeOf(c) => Some((c.value_root(), c.path())),
            _ => None,
        }
    }

    /// Head fields of a [`BareRef`](Self::BareRef) carrier — `(name, scope)`.
    /// NEVER returns `type_args`. `None` for any non-`BareRef` node.
    #[must_use]
    pub(crate) fn bare_ref_head(&self) -> Option<BareRefHead<'_>> {
        match self {
            Self::BareRef(c) => Some((c.name(), c.scope())),
            _ => None,
        }
    }

    /// Head fields of an [`ImportType`](Self::ImportType) carrier —
    /// `(specifier, qualifier, typeof_query)`. NEVER returns `type_args`.
    /// `None` for any non-`ImportType` node.
    #[must_use]
    pub(crate) fn import_type_head(&self) -> Option<ImportTypeHead<'_>> {
        match self {
            Self::ImportType(c) => Some((c.specifier(), c.qualifier(), c.typeof_query())),
            _ => None,
        }
    }

    /// Rebuild a carrier preserving its head fields but swapping in `new_args`
    /// — the sole crate-wide carrier reconstruction channel (the carrier's
    /// own head-preserving rebuild `with_type_args` is PRIVATE to the carrier
    /// module). Used by substitution to rewrite a carrier's structural
    /// `type_args`. `None` for a non-carrier node (no carrier args to map).
    ///
    /// Like [`carrier_type_args`](Self::carrier_type_args), the non-carrier arm
    /// is an EXHAUSTIVE enumeration with NO `_` wildcard: a new
    /// `SemanticNodeData` variant fails to compile HERE, forcing the author to
    /// classify it as a (rebuildable) carrier or not. A wildcard would instead
    /// silently refuse to rebuild a future carrier that grew a `type_args`
    /// field, dropping a substitution. Keep this enumeration in sync with
    /// `carrier_type_args`.
    #[must_use]
    pub(crate) fn map_carrier_type_args(&self, new_args: Arc<[SemanticNodeId]>) -> Option<Self> {
        match self {
            Self::TypeOf(c) => Some(Self::TypeOf(c.with_type_args(new_args))),
            Self::BareRef(c) => Some(Self::BareRef(c.with_type_args(new_args))),
            Self::ImportType(c) => Some(Self::ImportType(c.with_type_args(new_args))),
            // EXHAUSTIVE non-carrier enumeration — NO `_` wildcard (see
            // docstring); mirrors `carrier_type_args`.
            Self::Alias(_)
            | Self::Object(_)
            | Self::Union(_)
            | Self::Intersection(_)
            | Self::Primitive(_)
            | Self::Literal(_)
            | Self::Opaque(_)
            | Self::Array { .. }
            | Self::Tuple { .. }
            | Self::TemplateLiteral { .. }
            | Self::KeyOf { .. }
            | Self::IndexedAccess { .. }
            | Self::Mapped { .. }
            | Self::TypeParam { .. }
            | Self::Infer { .. }
            | Self::MergedDecl { .. }
            | Self::Conditional { .. }
            | Self::VueMacroElements(_)
            | Self::Function { .. }
            | Self::DeclRef { .. }
            | Self::InstantiationRef { .. }
            | Self::RawFallback { .. }
            | Self::ConstructorType { .. }
            | Self::SyntheticBinding { .. } => None,
        }
    }
}
