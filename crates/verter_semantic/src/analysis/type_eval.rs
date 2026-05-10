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
    /// Function/method signature, if this is a function or method.
    pub function_signature: Option<FunctionSignature>,
    /// Object literal shape, if this is a const initialized with an object.
    pub object_shape: Option<ObjectExpr>,
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
}

// ---------------------------------------------------------------------------
// Evaluation environment
// ---------------------------------------------------------------------------

/// Evaluation environment holding symbol tables and evaluation state.
#[derive(Debug, Clone)]
pub struct EvalEnv {
    /// Type declarations: interfaces, type aliases.
    pub type_symbols: FxHashMap<String, TypeDeclInfo>,
    /// Value declarations: functions, constants, classes.
    pub value_symbols: FxHashMap<String, ValueDeclInfo>,
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

    /// Register a type declaration.
    pub fn add_type(&mut self, mut decl: TypeDeclInfo) {
        let name = decl.name.clone();
        let decl_id = self.stabilize_type_declaration_id(&name, decl.declaration_id);
        decl.declaration_id = decl_id;
        self.type_symbols.insert(name.clone(), decl);
    }

    /// Register a value declaration.
    pub fn add_value(&mut self, mut decl: ValueDeclInfo) {
        let name = decl.name.clone();
        let decl_id = self.stabilize_value_declaration_id(&name, decl.declaration_id);
        decl.declaration_id = decl_id;
        self.value_symbols.insert(name.clone(), decl);
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
        for (name, decl) in &other.type_symbols {
            if !self.type_symbols.contains_key(name) {
                self.add_type(decl.clone());
            }
        }
        for (name, decl) in &other.value_symbols {
            if !self.value_symbols.contains_key(name) {
                self.add_value(decl.clone());
            }
        }
        for (name, decl_id) in &other.type_decl_ids {
            if *decl_id == 0 {
                continue;
            }
            let stable_id = self.stabilize_type_declaration_id(name, *decl_id);
            if let Some(decl) = self.type_symbols.get_mut(name) {
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
            if let Some(decl) = self.value_symbols.get_mut(name) {
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
