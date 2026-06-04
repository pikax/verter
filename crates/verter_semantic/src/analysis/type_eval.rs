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
/// Today's observable behaviour is last-wins: [`primary`](Self::primary)
/// returns the LAST contributor. Earlier contributors are retained so a
/// later phase can compose real TypeScript declaration merging without
/// changing current behaviour.
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
    /// ordered overload group. Such a group lowers to a [`TypeDeclBody::Merged`]
    /// carrier so the project-semantic reducer can peer-merge it (NOT a bare
    /// intersection, which would heritage-shadow). Classes and type aliases do
    /// NOT merge (a duplicate-identifier error in TS), so any non-interface or
    /// mixed-kind group keeps today's last-wins [`TypeDeclBody::Single`].
    pub fn merged_body(&self) -> TypeDeclBody {
        if self.contributors.len() > 1
            && self
                .contributors
                .iter()
                .all(|decl| decl.kind == TypeDeclKind::Interface)
        {
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
    pub fn lookup_object(&self) -> Cow<'_, TypeExpr> {
        match self {
            Self::Single(body) => Cow::Borrowed(body),
            Self::Merged(merged) => {
                let mut properties = Vec::new();
                for contributor in &merged.contributors {
                    if let TypeExpr::Object(object) = contributor {
                        properties.extend(object.properties.iter().cloned());
                    }
                }
                Cow::Owned(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
            }
        }
    }

    /// The union of direct member names across every contributor, in first-seen
    /// order (shallow index view; not the semantic surface).
    pub fn merged_member_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for contributor in self.contributors() {
            if let TypeExpr::Object(object) = contributor {
                for member in &object.properties {
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
            }
        }
        names
    }
}

/// What kind of type declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDeclKind {
    Alias,
    Interface,
    Class,
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
}

/// An ordered group of same-name value declaration contributors, in
/// source/binder order (append-only).
///
/// Today's observable behaviour is last-wins: [`primary`](Self::primary)
/// returns the LAST contributor.
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
    /// Create a new evaluation environment with default limits.
    pub fn new() -> Self {
        Self {
            type_symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            augmentation_scopes: FxHashMap::default(),
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

    /// Look up the ordered contributor group for an augmentation-scoped type
    /// declaration, if any.
    pub fn augmentation_symbol(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&TypeDeclGroup> {
        // The map key is `(scope, name)`; probe without allocating a fresh key
        // when possible.
        self.augmentation_scopes
            .get(&(scope.clone(), name.to_string()))
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
