//! Lightweight type evaluator for component metadata resolution.
//!
//! Reduces [`TypeExpr`] trees into normalized forms using symbol tables.
//! Handles common TypeScript utility types, `keyof`, `typeof`, indexed
//! access, and generic substitution without requiring a full TS type checker.
//!
//! # Design
//!
//! The evaluator operates on an [`EvalEnv`] containing:
//! - **Type symbols**: interfaces, type aliases, and their bodies as `TypeExpr`
//! - **Value symbols**: functions, constants, classes with structured signatures
//! - **Type bindings**: generic parameter -> argument mappings for instantiation
//!
//! Evaluation is demand-driven with cycle detection and configurable limits.

use std::borrow::Cow;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_type_expr::*;

pub type DeclarationId = u64;

// ---------------------------------------------------------------------------
// Symbol table types
// ---------------------------------------------------------------------------

/// A type declaration in the evaluator's symbol table.
#[derive(Debug, Clone)]
pub struct TypeDeclInfo {
    pub name: String,
    pub declaration_id: DeclarationId,
    pub kind: TypeDeclKind,
    pub type_parameters: Vec<TypeParam>,
    pub body: TypeExpr,
}

/// An ordered group of same-name type declaration contributors, in
/// source/binder order (append-only).
///
/// Every contributor is retained so [`merged_body`](Self::merged_body) can
/// compose real TypeScript declaration merging (interface+interface and
/// interface+class fold into a [`TypeDeclBody::Merged`] carrier).
/// [`primary`](Self::primary) returns the LAST contributor — the
/// last-wins representative used where a single body is required (alias
/// groups, single-contributor groups).
#[derive(Debug, Clone)]
pub struct TypeDeclGroup {
    /// Contributors in source/binder order. Always non-empty once created.
    pub contributors: Vec<TypeDeclInfo>,
}

impl TypeDeclGroup {
    /// Create a group seeded with a single contributor.
    pub fn new(decl: TypeDeclInfo) -> Self {
        Self {
            contributors: vec![decl],
        }
    }

    /// The authoritative contributor under today's last-wins semantics: the
    /// LAST one appended.
    pub fn primary(&self) -> &TypeDeclInfo {
        self.contributors
            .last()
            .expect("TypeDeclGroup is never empty")
    }

    /// Mutable access to the last (authoritative) contributor.
    pub fn primary_mut(&mut self) -> &mut TypeDeclInfo {
        self.contributors
            .last_mut()
            .expect("TypeDeclGroup is never empty")
    }

    /// All contributors in source/binder order.
    pub fn contributors(&self) -> &[TypeDeclInfo] {
        &self.contributors
    }

    /// Produce the merge-aware declaration body for this group.
    ///
    /// Multiple same-name `interface` declarations in one scope are merged by
    /// TypeScript: their members union and same-name methods accumulate into an
    /// ordered overload group. An `interface` + `class` group ALSO merges — the
    /// interface members augment the class INSTANCE type (the class's
    /// value / static / constructor side lives on a SEPARATE value declaration,
    /// untouched by this type-side merge; the class's type-side `body` is its
    /// instance-member `Object`). Such a group lowers to a
    /// [`TypeDeclBody::Merged`] carrier so the project-semantic reducer can
    /// peer-merge it (NOT a bare intersection, which would heritage-shadow).
    ///
    /// A type `alias` never merges (a duplicate-identifier error in TS); any
    /// group containing an alias — or any single-contributor group — keeps
    /// today's last-wins [`TypeDeclBody::Single`].
    pub fn merged_body(&self) -> TypeDeclBody {
        // `interface`+`interface` and `interface`+`class` are the two valid
        // same-name type merges. Two same-name `class` declarations are a
        // duplicate-identifier ERROR in TypeScript; folding such a (malformed)
        // group into a `Merged` carrier is benign — it produces a union of the
        // two class instance bodies for invalid input rather than special-casing
        // it, which keeps the predicate simple and never affects well-formed code.
        let all_mergeable = self
            .contributors
            .iter()
            .all(|decl| matches!(decl.kind, TypeDeclKind::Interface | TypeDeclKind::Class));
        if self.contributors.len() > 1 && all_mergeable {
            TypeDeclBody::Merged(MergedTypeBody {
                contributors: self.contributors.iter().map(|d| d.body.clone()).collect(),
                kinds: self.contributors.iter().map(|d| d.kind).collect(),
            })
        } else {
            TypeDeclBody::Single(self.primary().body.clone())
        }
    }
}

/// The body of a type declaration, carrying same-file declaration-merge
/// provenance.
///
/// [`Single`](Self::Single) is the non-merged path — one declaration, lowered
/// exactly as before. [`Merged`](Self::Merged) carries every same-name
/// `interface` contributor's body in source order so the project-semantic
/// reducer can peer-merge them into one surface (member union + ordered method
/// overload groups). A merged declaration MUST reach the reducer as this
/// distinct carrier; collapsing it to a bare `TypeExpr::Intersection` would
/// route it through heritage-shadow member semantics — the wrong rule.
#[derive(Debug, Clone)]
pub enum TypeDeclBody {
    /// A single declaration's body.
    Single(TypeExpr),
    /// Multiple same-name interface contributors, in source order.
    Merged(MergedTypeBody),
}

/// The ordered contributor bodies + kinds of a merged declaration.
#[derive(Debug, Clone)]
pub struct MergedTypeBody {
    /// Contributor bodies in source/binder order.
    pub contributors: Vec<TypeExpr>,
    /// Contributor kinds, parallel to [`contributors`](Self::contributors).
    pub kinds: Vec<TypeDeclKind>,
}

impl TypeDeclBody {
    /// Construct a non-merged single body.
    pub fn single(body: TypeExpr) -> Self {
        Self::Single(body)
    }

    /// Whether this body carries more than one merged contributor.
    pub fn is_merged(&self) -> bool {
        matches!(self, Self::Merged(_))
    }

    /// Every contributor body in source order (one element for `Single`).
    pub fn contributors(&self) -> &[TypeExpr] {
        match self {
            Self::Single(body) => std::slice::from_ref(body),
            Self::Merged(merged) => &merged.contributors,
        }
    }

    /// The last-wins representative contributor body (the final declaration).
    pub fn primary(&self) -> &TypeExpr {
        self.contributors()
            .last()
            .expect("TypeDeclBody always has at least one contributor")
    }

    /// A single object surface unioning every contributor's direct members.
    ///
    /// This is a SHALLOW index projection for same-file member enumeration,
    /// dependency tracking, and member-index construction ONLY — it is never
    /// the semantic merge. The semantic declaration merge (member precedence,
    /// method overload accumulation) is performed exclusively by the
    /// project-semantic reducer over the `MergedDecl` carrier. The projection
    /// is an `Object` (never an `Intersection`), so it cannot accidentally
    /// route through the intersection heritage-shadow reducer.
    ///
    /// A contributor carrying `extends`/`implements` heritage lowers to an
    /// `Intersection([<heritage Ref…>, <own Object>])`; this projection descends
    /// such intersection arms and collects the OWN object members (the heritage
    /// `Ref` arms carry no direct members and are not flattened — inherited
    /// members surface only through the semantic reducer, exactly as for a single
    /// `interface extends Base` body). Without this descent a heritage-carrying
    /// merged contributor would drop even its own members from the shallow index.
    pub fn lookup_object(&self) -> Cow<'_, TypeExpr> {
        match self {
            Self::Single(body) => Cow::Borrowed(body),
            Self::Merged(merged) => {
                let mut properties = Vec::new();
                for contributor in &merged.contributors {
                    collect_direct_object_members(contributor, &mut properties);
                }
                Cow::Owned(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
            }
        }
    }

    /// The union of direct member names across every contributor, in first-seen
    /// order (shallow index view; not the semantic surface).
    pub fn merged_member_names(&self) -> Vec<String> {
        let mut members = Vec::new();
        for contributor in self.contributors() {
            collect_direct_object_members(contributor, &mut members);
        }
        let mut names = Vec::new();
        for member in &members {
            let name = match member {
                ObjectMember::Property(prop) => Some(prop.name.clone()),
                ObjectMember::Method(method) => Some(method.name.clone()),
                _ => None,
            };
            if let Some(name) = name {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }
}

/// Collect the DIRECT object members of `body` into `out`, descending
/// `Intersection`/`Parenthesized` arms. Object arms contribute their members;
/// every other arm (notably a heritage `Ref` from `extends`/`implements`)
/// carries no direct member and is skipped — inherited members surface only
/// through the semantic reducer, never this shallow index. Mirrors the
/// session-side `collect_direct_object_properties` descent so the merged and
/// single shallow-index views stay consistent.
fn collect_direct_object_members(body: &TypeExpr, out: &mut Vec<ObjectMember>) {
    match body {
        TypeExpr::Object(object) => out.extend(object.properties.iter().cloned()),
        TypeExpr::Intersection(parts) => {
            for part in parts.iter() {
                collect_direct_object_members(part, out);
            }
        }
        TypeExpr::Parenthesized(inner) => collect_direct_object_members(inner, out),
        _ => {}
    }
}

/// What kind of type declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDeclKind {
    Alias,
    Interface,
    Class,
}

/// The type-projected value of one `enum` member.
///
/// An enum member always has a NAME (recorded on the presence rail). Its VALUE
/// is either statically FOLDED to a literal, or DEFERRED — a computed /
/// expression / member-reference initializer the literal-enum reducer does not
/// constant-fold (`B = 1 << 2`, `B = someFn()`, `~A`, or a bare member after an
/// unknown running value). A deferred member is NEVER dropped and NEVER given a
/// fabricated literal: it carries the narrowest SOUND primitive DOMAIN proven
/// from its initializer-expression KIND, so every type/value projection surface
/// (`typeof Enum`, `keyof typeof Enum`, `Enum.Member`, the enum type union) sees
/// the member with an honest, never-under-approximating type.
#[derive(Debug, Clone, PartialEq)]
pub enum EnumMemberValue {
    /// Statically folded to a literal (string / ±numeric / auto-increment).
    /// This literal is BOTH the projected type AND the value-body fingerprint
    /// basis — the only members the foldable rail observes.
    Folded(TypeExpr),
    /// Value deferred. Carries the narrowest SOUND primitive domain proven from
    /// the initializer-expression kind — `number`, `string`, `number | string`,
    /// or `unknown` — NEVER the enum self-reference and NEVER a fabricated
    /// literal. The member's NAME stays on the presence rail; only its foldable
    /// VALUE is absent (it is projected out of the fingerprint, NOT out of the
    /// type surfaces).
    Deferred(TypeExpr),
}

impl EnumMemberValue {
    /// The type this member PROJECTS to on every type/value surface (`typeof
    /// Enum`, `Enum.Member`, the enum type union): the folded literal for a
    /// foldable member, or the degraded primitive domain for a deferred one.
    /// ALWAYS present — a deferred member is degraded, never dropped.
    pub fn projected_type(&self) -> &TypeExpr {
        match self {
            Self::Folded(ty) | Self::Deferred(ty) => ty,
        }
    }

    /// The folded literal value, or `None` when the member's value is deferred.
    /// This is the foldable-only view the value-body fingerprint
    /// ([`crate`]-external `value_body_for_hash`) observes — a deferred member
    /// MUST NOT enter the fingerprint, because its degraded domain is not a
    /// value edit.
    pub fn folded_literal(&self) -> Option<&TypeExpr> {
        match self {
            Self::Folded(ty) => Some(ty),
            Self::Deferred(_) => None,
        }
    }
}

/// A value declaration in the evaluator's value symbol table.
#[derive(Debug, Clone)]
pub struct ValueDeclInfo {
    pub name: String,
    pub declaration_id: DeclarationId,
    pub kind: ValueDeclKind,
    /// Explicit type annotation, if present.
    pub type_annotation: Option<TypeExpr>,
    /// Function/method signatures. Empty = non-callable; length 1 = the
    /// common single-declaration case; length > 1 = an overload group
    /// (source order; the trailing entry may be the implementation).
    pub signatures: Vec<FunctionSignature>,
    /// Object literal shape, if this is a const initialized with an object.
    pub object_shape: Option<ObjectExpr>,
    /// The ordered member inventory of a [`ValueDeclKind::Enum`] declaration,
    /// in SOURCE declaration order (TS enum members are ordered — the
    /// auto-increment of a bare member depends on the preceding member, and the
    /// `typeof Enum` object surfaces members in declaration order).
    ///
    /// This is the SINGLE source of truth for BOTH enum rails, so the two can
    /// never diverge by construction: the member NAME is recorded for EVERY
    /// statically-named member (all four `TSEnumMemberName` variants resolve to
    /// a static name via the SAME `static_name` helper the production
    /// `index_enum` header walk uses), while each member's [`EnumMemberValue`]
    /// is [`Folded`](EnumMemberValue::Folded) when the value is statically
    /// foldable (string / ±numeric / auto-increment) or
    /// [`Deferred`](EnumMemberValue::Deferred) when it is unfoldable (a computed
    /// `1 << 2`, a member-reference `B = A`, or a bare member after an unknown
    /// running value). A deferred member carries its DEGRADED sound primitive
    /// domain, so it is honestly typed on every surface, never dropped. The
    /// `Folded` subset is therefore an intrinsic subset of the full member set:
    /// - The NAME rail — [`ValueDeclGroup::merged_enum_member_names`] — is the
    ///   member-presence authority (`enum_headers`, `parse_stable_hash`,
    ///   enum `MemberPresence` facts); it must match `index_enum` exactly.
    /// - The PROJECTION rail — every member's
    ///   [`projected_type`](EnumMemberValue::projected_type) — drives the
    ///   `typeof Enum` object, the `Enum.Member` projection, and the enum type
    ///   union (folded literal for foldable members, degraded primitive for
    ///   deferred ones), so NO known member ever vanishes from a type surface.
    /// - The FOLDABLE rail — the [`folded_literal`](EnumMemberValue::folded_literal)
    ///   subset, projected by [`ValueDeclGroup::merged_enum_members`] — is the
    ///   value-body fact fingerprint basis ONLY; a deferred member's degraded
    ///   domain is not a value edit and stays out of it.
    ///
    /// `Some(..)` exactly when this value decl is an enum (possibly an empty
    /// member list); `None` for every non-enum value declaration.
    pub enum_members: Option<Vec<(String, EnumMemberValue)>>,
}

/// An ordered group of same-name value declaration contributors, in
/// source/binder order (append-only).
///
/// [`merged_signatures`](Self::merged_signatures) concatenates every
/// contributor's signatures in source order (the function-overload group);
/// [`primary`](Self::primary) returns the LAST contributor, the last-wins
/// representative used where a single declaration is required.
#[derive(Debug, Clone)]
pub struct ValueDeclGroup {
    /// Contributors in source/binder order. Always non-empty once created.
    pub contributors: Vec<ValueDeclInfo>,
}

impl ValueDeclGroup {
    /// Create a group seeded with a single contributor.
    pub fn new(decl: ValueDeclInfo) -> Self {
        Self {
            contributors: vec![decl],
        }
    }

    /// The authoritative contributor under today's last-wins semantics: the
    /// LAST one appended.
    pub fn primary(&self) -> &ValueDeclInfo {
        self.contributors
            .last()
            .expect("ValueDeclGroup is never empty")
    }

    /// Mutable access to the last (authoritative) contributor.
    pub fn primary_mut(&mut self) -> &mut ValueDeclInfo {
        self.contributors
            .last_mut()
            .expect("ValueDeclGroup is never empty")
    }

    /// All contributors in source/binder order.
    pub fn contributors(&self) -> &[ValueDeclInfo] {
        &self.contributors
    }

    /// The merged overload signature set: every contributor's signatures
    /// concatenated in source order. A function declared with bodiless
    /// overloads followed by an implementation contributes one signature per
    /// declaration, so the returned vector is the full ordered overload group
    /// (the trailing implementation entry carries `has_implementation_body`).
    /// For a single contributor this is exactly its own signatures.
    pub fn merged_signatures(&self) -> Vec<FunctionSignature> {
        if self.contributors.len() == 1 {
            return self.contributors[0].signatures.clone();
        }
        self.contributors
            .iter()
            .flat_map(|decl| decl.signatures.iter().cloned())
            .collect()
    }

    /// The merged enum member inventory: every contributor's
    /// [`enum_members`](ValueDeclInfo::enum_members) folded in source order, the
    /// SHARED basis both enum rails derive from so they can never disagree on
    /// which contributor owns a member. TS declaration merging lets
    /// `enum E { A }` and a later `enum E { B = 1 }` contribute to one enum, so
    /// the member set is the UNION of all same-name contributors — using only
    /// the last (last-wins `primary()`) contributor would drop the earlier
    /// declarations' members.
    ///
    /// TS forbids a duplicate member NAME across merged enum bodies, so the
    /// collision path is defensive: on a name already seen, the FIRST
    /// contributor's entry (name AND [`EnumMemberValue`]) wins deterministically,
    /// so every derived rail agrees on every member's owning contributor.
    /// Returns `None` when no contributor is an enum.
    pub fn merged_enum_unified(&self) -> Option<Vec<(String, EnumMemberValue)>> {
        let mut is_enum = false;
        let mut merged: Vec<(String, EnumMemberValue)> = Vec::new();
        for decl in &self.contributors {
            let Some(members) = decl.enum_members.as_ref() else {
                continue;
            };
            is_enum = true;
            for (name, value) in members {
                if !merged.iter().any(|(existing, _)| existing == name) {
                    merged.push((name.clone(), value.clone()));
                }
            }
        }
        is_enum.then_some(merged)
    }

    /// The FULL ordered member-NAME set of the merged enum — EVERY
    /// statically-named member, including ones whose VALUE is deferred. This is
    /// the member-presence authority: it must match what the production
    /// `index_enum` header walk records (both resolve names via `static_name`),
    /// so a seeded `DeclHeaderIndex` reconstructs `enum_headers` identically and
    /// the enum `MemberPresence` facts / `parse_stable_hash` enum-header fold
    /// stay correct for seeded artifacts. Returns `None` when no contributor is
    /// an enum.
    pub fn merged_enum_member_names(&self) -> Option<Vec<String>> {
        self.merged_enum_unified()
            .map(|merged| merged.into_iter().map(|(name, _)| name).collect())
    }

    /// The merged enum's FOLDABLE rail: every member with a statically known
    /// value-literal, in source order. Members whose value is DEFERRED are
    /// projected out (their degraded domain is not a folded literal), so this
    /// rail is reserved for the value-body fact fingerprint
    /// (`value_body_for_hash`) — the ONLY consumer that must observe foldable
    /// members alone. Type/value PROJECTION surfaces (`typeof Enum`,
    /// `Enum.Member`, [`enum_type_union`](Self::enum_type_union)) read the FULL
    /// member set via [`EnumMemberValue::projected_type`] instead, so a deferred
    /// member is degraded, never dropped. Returns `None` when no contributor is
    /// an enum.
    pub fn merged_enum_members(&self) -> Option<Vec<(String, TypeExpr)>> {
        self.merged_enum_unified().map(|merged| {
            merged
                .into_iter()
                .filter_map(|(name, value)| value.folded_literal().cloned().map(|lit| (name, lit)))
                .collect()
        })
    }

    /// The enum's TYPE-space body: the UNION of EVERY member's projected type,
    /// derived from the SAME full merged member set the name rail derives from.
    /// An `enum` is dual-space — a VALUE (its `typeof` object / `Enum.Member`
    /// projection) and a TYPE (the union used when the enum names a type) — and
    /// BOTH spaces carry exactly the same member set so they can never diverge.
    ///
    /// Honesty floor: a deferred member contributes its DEGRADED sound primitive
    /// arm ([`EnumMemberValue::projected_type`]), so a NON-EMPTY enum NEVER
    /// collapses to `never` (an all-deferred enum is `number` / `string` /
    /// `number | string` / `unknown`, never the empty bottom) and NEVER narrows
    /// to the folded subset (a partial enum keeps both the folded literals and
    /// the deferred members' degraded arms). Distinct arms are deduped, so an
    /// all-`number`-degraded enum unwraps to `number` rather than `number |
    /// number`.
    ///
    /// This is the SINGLE source of truth for the enum type body. The
    /// per-declaration eval-env walk (`extract_enum`) cannot see same-name
    /// merged contributors, so it registers only a non-served placeholder type
    /// body and the lazily-served body memo defers to this derivation. Returns
    /// `None` when the group is not an enum; an empty member list yields `never`.
    pub fn enum_type_union(&self) -> Option<TypeExpr> {
        let members = self.merged_enum_unified()?;
        let mut arms: Vec<TypeExpr> = Vec::new();
        for (_, value) in &members {
            let arm = value.projected_type().clone();
            if !arms.contains(&arm) {
                arms.push(arm);
            }
        }
        Some(TypeExpr::union(arms))
    }
}

/// What kind of value declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueDeclKind {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
    /// TypeScript enum declaration — dual-space: type (union of members)
    /// and value (object with member lookup).
    Enum,
}

/// A function signature extracted from a declaration.
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub parameters: Vec<FunctionParam>,
    pub return_type: Option<TypeExpr>,
    pub type_parameters: Vec<TypeParam>,
    /// Whether this signature is backed by an implementation body (vs. a
    /// bodiless overload / ambient declaration). Used by a later phase to
    /// hide the implementation signature behind preceding overloads.
    pub has_implementation_body: bool,
}

// ---------------------------------------------------------------------------
// Evaluation environment
// ---------------------------------------------------------------------------

/// The ambient declaration-augmentation scope an inner declaration belongs to.
///
/// Ambient augmentation blocks (`declare module "X" { ... }` and
/// `declare global { ... }`) do NOT contribute to the file's top-level symbol
/// table — their inner declarations augment a DIFFERENT module's surface (the
/// canonical Vue/Vite `declare module "vue"` pattern) or the global scope.
/// They are retained in a SEPARATE scoped inventory so cross-file augmentation
/// can stitch them onto the augmented declaration on demand, without polluting
/// file-scope `type_symbols`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AugmentationScopeKind {
    /// `declare global { ... }` — augments the global scope.
    Global,
    /// `declare module "<specifier>" { ... }` — augments the module reached by
    /// the RAW specifier as written in the source. The owner crate keeps the
    /// specifier verbatim; the session layer resolves it to a canonical id (for
    /// relative specifiers) when it stitches augmenters through the
    /// augmentation index.
    Module(String),
}

/// Evaluation environment holding symbol tables and evaluation state.
#[derive(Debug, Clone)]
pub struct EvalEnv {
    /// Type declarations: interfaces, type aliases. Each name maps to an
    /// ordered group of contributors (append-only, source/binder order).
    pub type_symbols: FxHashMap<String, TypeDeclGroup>,
    /// Value declarations: functions, constants, classes. Each name maps to
    /// an ordered group of contributors (append-only, source/binder order).
    pub value_symbols: FxHashMap<String, ValueDeclGroup>,
    /// Ambient declaration-augmentation inventory: `(scope, name)` → ordered
    /// contributor group. Holds the RETAINED bodies of declarations nested in
    /// `declare module "X" { ... }` / `declare global { ... }` blocks so a
    /// scoped declaration lookup can address them. Kept separate from
    /// `type_symbols` — these inner declarations never enter the file's
    /// top-level surface.
    pub augmentation_scopes: FxHashMap<(AugmentationScopeKind, String), TypeDeclGroup>,
    /// Value-space counterpart to [`augmentation_scopes`](Self::augmentation_scopes):
    /// the RETAINED bodies of VALUE declarations (`const`/`let`/`var`,
    /// `function`, `class`, `enum`) nested in `declare module "X" { ... }` /
    /// `declare global { ... }` blocks. Kept separate from `value_symbols`
    /// (file scope) — these augment another module's value surface and are the
    /// typed source for value-space module-augmentation facts.
    pub augmentation_value_scopes: FxHashMap<(AugmentationScopeKind, String), ValueDeclGroup>,
    /// Stable ids assigned to type declarations inserted into this environment.
    type_decl_ids: FxHashMap<String, DeclarationId>,
    /// Stable ids assigned to value declarations inserted into this environment.
    value_decl_ids: FxHashMap<String, DeclarationId>,
    /// Generic type parameter bindings for the current instantiation.
    pub type_bindings: FxHashMap<String, Arc<TypeExpr>>,
    /// Evaluation limits.
    pub limits: EvalLimits,
    /// Total evaluation steps consumed (monotonically increasing).
    steps: usize,
    /// Monotonic declaration ordinal used to assign stable ids.
    next_declaration_id: DeclarationId,
    /// Preserve canonical vue `VNode` slot return types symbolically while
    /// still normalizing other slot return types during defineSlots expansion.
    pub preserve_canonical_vue_vnode_slot_returns: bool,
}

/// Configurable limits for the evaluator.
#[derive(Debug, Clone)]
pub struct EvalLimits {
    pub max_depth: usize,
    pub max_union_expansion: usize,
    pub max_mapped_keys: usize,
    /// Maximum nested `evaluate_mapped()` calls. Default: 3.
    pub max_mapped_depth: usize,
    /// Safety-net total step limit. Default: 50_000.
    pub max_steps: usize,
    /// Maximum nested `evaluate_ref` calls (reference chain depth). Default: 8.
    pub max_ref_depth: usize,
}

impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_union_expansion: 64,
            max_mapped_keys: 128,
            max_mapped_depth: 3,
            max_steps: 50_000,
            max_ref_depth: 8,
        }
    }
}

impl EvalEnv {
    /// Total number of declaration-body CONTRIBUTORS this environment
    /// holds across the file-scope type/value tables and the
    /// augmentation-scope inventories. Each contributor corresponds to
    /// one lowered declaration body (a same-name merge group of `k`
    /// declarations counts `k`). This is the deterministic
    /// "bodies lowered" measure the host's demand-scoping counters
    /// observe when a whole-file environment is built.
    #[must_use]
    pub fn total_decl_count(&self) -> usize {
        self.type_symbols
            .values()
            .map(|g| g.contributors().len())
            .sum::<usize>()
            + self
                .value_symbols
                .values()
                .map(|g| g.contributors().len())
                .sum::<usize>()
            + self
                .augmentation_scopes
                .values()
                .map(|g| g.contributors().len())
                .sum::<usize>()
            + self
                .augmentation_value_scopes
                .values()
                .map(|g| g.contributors().len())
                .sum::<usize>()
    }

    /// Create a new evaluation environment with default limits.
    pub fn new() -> Self {
        Self {
            type_symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            augmentation_scopes: FxHashMap::default(),
            augmentation_value_scopes: FxHashMap::default(),
            type_decl_ids: FxHashMap::default(),
            value_decl_ids: FxHashMap::default(),
            type_bindings: FxHashMap::default(),
            limits: EvalLimits::default(),
            steps: 0,
            next_declaration_id: 0,
            preserve_canonical_vue_vnode_slot_returns: false,
        }
    }

    /// Create an environment with custom limits.
    pub fn with_limits(limits: EvalLimits) -> Self {
        Self {
            limits,
            ..Self::new()
        }
    }

    /// Register a type declaration, appending it to the named group in
    /// source/binder order (creating the group if absent).
    pub fn add_type(&mut self, mut decl: TypeDeclInfo) {
        let name = decl.name.clone();
        let decl_id = self.stabilize_type_declaration_id(&name, decl.declaration_id);
        decl.declaration_id = decl_id;
        match self.type_symbols.get_mut(&name) {
            Some(group) => group.contributors.push(decl),
            None => {
                self.type_symbols.insert(name, TypeDeclGroup::new(decl));
            }
        }
    }

    /// Register a value declaration, appending it to the named group in
    /// source/binder order (creating the group if absent).
    pub fn add_value(&mut self, mut decl: ValueDeclInfo) {
        let name = decl.name.clone();
        let decl_id = self.stabilize_value_declaration_id(&name, decl.declaration_id);
        decl.declaration_id = decl_id;
        match self.value_symbols.get_mut(&name) {
            Some(group) => group.contributors.push(decl),
            None => {
                self.value_symbols.insert(name, ValueDeclGroup::new(decl));
            }
        }
    }

    /// Register a type declaration nested in an ambient augmentation block
    /// (`declare module "X"` / `declare global`), appending it to the named
    /// group inside the augmentation scope (creating the group if absent).
    /// These declarations are retained for cross-file augmentation stitching
    /// and never enter the file-scope `type_symbols`.
    pub fn add_augmentation_type(&mut self, scope: AugmentationScopeKind, decl: TypeDeclInfo) {
        match self
            .augmentation_scopes
            .get_mut(&(scope.clone(), decl.name.clone()))
        {
            Some(group) => group.contributors.push(decl),
            None => {
                let name = decl.name.clone();
                self.augmentation_scopes
                    .insert((scope, name), TypeDeclGroup::new(decl));
            }
        }
    }

    /// Register a VALUE declaration nested in an ambient augmentation block,
    /// appending it to the named group inside the augmentation value scope
    /// (creating the group if absent). Retained for value-space cross-file
    /// augmentation facts; never enters file-scope `value_symbols`.
    pub fn add_augmentation_value(&mut self, scope: AugmentationScopeKind, decl: ValueDeclInfo) {
        match self
            .augmentation_value_scopes
            .get_mut(&(scope.clone(), decl.name.clone()))
        {
            Some(group) => group.contributors.push(decl),
            None => {
                let name = decl.name.clone();
                self.augmentation_value_scopes
                    .insert((scope, name), ValueDeclGroup::new(decl));
            }
        }
    }

    /// Returns the total number of evaluation steps consumed so far.
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// Returns whether the step budget has been exhausted.
    pub fn budget_exhausted(&self) -> bool {
        self.steps >= self.limits.max_steps
    }

    /// Merge declarations from another environment without overwriting
    /// declarations already present in `self`.
    pub fn extend_missing(&mut self, other: EvalEnv) {
        self.extend_missing_from_ref(&other);
    }

    /// Merge declarations from another environment by reference without
    /// cloning the full environment up front.
    pub fn extend_missing_from_ref(&mut self, other: &EvalEnv) {
        for (name, group) in &other.type_symbols {
            if !self.type_symbols.contains_key(name) {
                for decl in group.contributors() {
                    self.add_type(decl.clone());
                }
            }
        }
        for (name, group) in &other.value_symbols {
            if !self.value_symbols.contains_key(name) {
                for decl in group.contributors() {
                    self.add_value(decl.clone());
                }
            }
        }
        for (name, decl_id) in &other.type_decl_ids {
            if *decl_id == 0 {
                continue;
            }
            let stable_id = self.stabilize_type_declaration_id(name, *decl_id);
            if let Some(group) = self.type_symbols.get_mut(name) {
                let decl = group.primary_mut();
                if decl.declaration_id == 0 {
                    decl.declaration_id = stable_id;
                }
            }
        }
        for (name, decl_id) in &other.value_decl_ids {
            if *decl_id == 0 {
                continue;
            }
            let stable_id = self.stabilize_value_declaration_id(name, *decl_id);
            if let Some(group) = self.value_symbols.get_mut(name) {
                let decl = group.primary_mut();
                if decl.declaration_id == 0 {
                    decl.declaration_id = stable_id;
                }
            }
        }
        self.next_declaration_id = self.next_declaration_id.max(other.next_declaration_id);
    }

    pub fn type_declaration_id(&self, name: &str) -> Option<DeclarationId> {
        self.type_decl_ids.get(name).copied()
    }

    pub fn value_declaration_id(&self, name: &str) -> Option<DeclarationId> {
        self.value_decl_ids.get(name).copied()
    }

    fn stabilize_type_declaration_id(
        &mut self,
        name: &str,
        declaration_id: DeclarationId,
    ) -> DeclarationId {
        if declaration_id != 0 {
            let stable_id = *self
                .type_decl_ids
                .entry(name.to_string())
                .or_insert(declaration_id);
            self.next_declaration_id = self.next_declaration_id.max(stable_id);
            stable_id
        } else if let Some(existing) = self.type_decl_ids.get(name).copied() {
            existing
        } else {
            let decl_id = self.allocate_declaration_id();
            self.type_decl_ids.insert(name.to_string(), decl_id);
            decl_id
        }
    }

    fn stabilize_value_declaration_id(
        &mut self,
        name: &str,
        declaration_id: DeclarationId,
    ) -> DeclarationId {
        if declaration_id != 0 {
            let stable_id = *self
                .value_decl_ids
                .entry(name.to_string())
                .or_insert(declaration_id);
            self.next_declaration_id = self.next_declaration_id.max(stable_id);
            stable_id
        } else if let Some(existing) = self.value_decl_ids.get(name).copied() {
            existing
        } else {
            let decl_id = self.allocate_declaration_id();
            self.value_decl_ids.insert(name.to_string(), decl_id);
            decl_id
        }
    }

    fn allocate_declaration_id(&mut self) -> DeclarationId {
        self.next_declaration_id += 1;
        self.next_declaration_id
    }
}

impl Default for EvalEnv {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod shallow_index_tests {
    use super::*;

    fn prop(name: &str, ty: TypeExpr) -> ObjectMember {
        ObjectMember::Property(ObjectProperty::synthetic_public(
            name.to_string(),
            ty,
            false,
            false,
        ))
    }

    fn object(members: Vec<ObjectMember>) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: members,
        }))
    }

    /// A merged contributor carrying `extends` heritage lowers to
    /// `Intersection([Ref(Base), Object({ own })])`. The shallow index
    /// projection must still surface the contributor's OWN members (the
    /// pre-fix Object-only match dropped the entire intersection contributor,
    /// losing even `a`). Heritage members behind `Ref(Base)` stay out of the
    /// shallow index (they are not direct members) — same as a single
    /// `interface extends Base` body.
    #[test]
    fn lookup_object_recovers_own_members_from_heritage_contributor() {
        // interface X extends Base { a: number }  +  interface X { b: boolean }
        let heritage_contributor = TypeExpr::intersection(vec![
            TypeExpr::named("Base"),
            object(vec![prop("a", TypeExpr::primitive(PrimitiveName::Number))]),
        ]);
        let plain_contributor =
            object(vec![prop("b", TypeExpr::primitive(PrimitiveName::Boolean))]);
        let body = TypeDeclBody::Merged(MergedTypeBody {
            contributors: vec![heritage_contributor, plain_contributor],
            kinds: vec![TypeDeclKind::Interface, TypeDeclKind::Interface],
        });

        let names = body.merged_member_names();
        assert!(
            names.contains(&"a".to_string()),
            "own member `a` from the heritage-carrying contributor must survive the shallow index; got {names:?}"
        );
        assert!(
            names.contains(&"b".to_string()),
            "own member `b` must survive; got {names:?}"
        );
        // Heritage members behind `Ref(Base)` are NOT direct members and stay
        // out of the shallow index (resolved later by the semantic reducer).
        assert!(
            !names.contains(&"Base".to_string()),
            "the heritage Ref name must not appear as a member; got {names:?}"
        );

        // The projection is an `Object` (never an `Intersection`) so it cannot
        // route through the heritage-shadow reducer.
        let projected = body.lookup_object();
        let TypeExpr::Object(obj) = projected.as_ref() else {
            panic!("lookup_object must project to an Object; got {projected:?}");
        };
        let projected_names: Vec<&str> = obj
            .properties
            .iter()
            .filter_map(|m| match m {
                ObjectMember::Property(p) => Some(p.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(projected_names.contains(&"a"), "got {projected_names:?}");
        assert!(projected_names.contains(&"b"), "got {projected_names:?}");
    }
}
