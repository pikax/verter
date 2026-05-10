#![deny(missing_docs)]
//! Public DTO types for the host typeinfo substrate.
//!
//! These types form the boundary between the host's internal symbol /
//! semantic-graph state and downstream consumers (the `@verter/typeinfo`
//! TS package, MCP tools, IDE integrations). They are designed to be:
//!
//! - **Immutable.** No interior mutability or back-references into host
//!   state. Consumers may stash them across requests.
//! - **`Send + Sync`.** Safe to ship across thread boundaries and into
//!   the FFI / WASM adapter.
//! - **Span-aware.** `SymbolEntry::span` is SFC-absolute per the
//!   project's `verter_span` invariants.

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use crate::semantic_query::ProjectionMode;

// ---------------------------------------------------------------------------
// Symbol inventory
// ---------------------------------------------------------------------------

/// One top-level symbol declared in a file.
///
/// Returned by [`crate::VerterHost::list_file_symbols`] — each entry
/// describes a single declaration captured by the file's shallow symbol
/// inventory ([`crate::resolver_core::shallow_file_state::ShallowFileState`]).
/// The entry is purely descriptive: it does not carry resolved type data
/// or generic-instantiation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    /// The local declaration name. For type-and-value class / enum
    /// declarations the same name surfaces twice with distinct
    /// [`SymbolKind`] entries.
    pub name: String,
    /// Discriminator naming the declaration class.
    pub kind: SymbolKind,
    /// SFC-absolute span sourced from the
    /// [`verter_semantic::analysis::types::LocalDeclarationEntry`] for
    /// this declaration, when one is present in the analysis snapshot.
    /// `None` when the script-analysis snapshot did not capture a span
    /// (e.g. ambient declarations synthesised at lower stages).
    pub span: Option<verter_span::Span>,
    /// `true` when the declaration is exported from the file. Imported
    /// symbols are NOT surfaced (the inventory describes declarations
    /// owned by this file).
    pub is_exported: bool,
}

/// Discriminator for a [`SymbolEntry`].
///
/// Mirrors the project's shallow-state taxonomy: `TypeAlias`,
/// `Interface`, and `Class` come from
/// [`verter_semantic::analysis::type_eval::TypeDeclKind`]; the value
/// kinds (`Const`, `Let`, `Var`, `Function`, `AsyncFunction`, `Enum`)
/// come from
/// [`verter_semantic::analysis::type_eval::ValueDeclKind`]. Class /
/// Enum surface twice in the inventory — once as a type entry and
/// once as a value entry — so consumers downstream of the inventory
/// can disambiguate the two namespaces without re-analysing the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// `type Foo = ...`
    TypeAlias,
    /// `interface Foo { ... }`
    Interface,
    /// `class Foo {}` — the type-side projection.
    Class,
    /// `const foo = ...`
    Const,
    /// `let foo`
    Let,
    /// `var foo`
    Var,
    /// `function foo() { ... }`
    Function,
    /// `async function foo() { ... }`
    AsyncFunction,
    /// `class Foo {}` — the value-side projection.
    ClassValue,
    /// `enum Foo {}` — appears with both a type and a value entry.
    Enum,
}

// ---------------------------------------------------------------------------
// Type-expression evaluation request
// ---------------------------------------------------------------------------

/// Request for [`crate::VerterHost::evaluate_type_expression_with_audit`].
///
/// Encodes the full inputs needed to synthesise a scratch TypeScript
/// file, evaluate one trailing `type __VerterScratch = <expression>`
/// declaration in the scope of `scope`, and return the resolved
/// semantic-graph node id.
///
/// The scratch URI is derived from a sha256 of `(scope_canonical,
/// expression, serialised(extra_imports))` per §5.3 — two scopes
/// evaluating the same expression always get distinct URIs.
#[derive(Debug, Clone)]
pub struct EvaluateTypeExpressionRequest {
    /// Canonical id of the file the expression evaluates against. The
    /// scratch file's import resolution and eval environment are
    /// rooted at this scope.
    pub scope: String,
    /// The TypeScript type expression body — a single type expression
    /// (`InstanceType<typeof default>['$props']`, `Pick<Foo, 'a'>`,
    /// `string`, …). Wrapped at synthesis time inside
    /// `type __VerterScratch = <expression>;`.
    pub expression: String,
    /// Additional imports to inject at the top of the scratch file.
    /// Each entry resolves through the host's normal import-route
    /// machinery so the expression can reference symbols not already
    /// in the source-scope.
    pub extra_imports: Vec<ImportSpec>,
    /// Projection mode for the terminal evaluation. `Identity` /
    /// `Navigate` / `Expanded` / `Shallow` follow the project's mode
    /// contract — see `/type-resolution` skill.
    pub mode: ProjectionMode,
    /// When `true`, the scratch file is published to the
    /// host-owned typeinfo scratch cache (default size 64) and a
    /// repeat request with the same URI hits the cache. When `false`,
    /// the synthesis is one-shot and the scratch file is dropped at
    /// the end of the call.
    pub cacheable: bool,
}

/// One structured import to inject into the scratch file synthesised
/// by [`EvaluateTypeExpressionRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSpec {
    /// Raw import specifier (e.g. `"./types"`, `"reka-ui"`).
    pub specifier: String,
    /// Per-binding shape. Multiple bindings on a single specifier
    /// surface as a single grouped `import` declaration in the
    /// synthesised source.
    pub bindings: Vec<NamedImport>,
}

/// One binding within an [`ImportSpec`].
///
/// `Default` corresponds to `import X from '...'`; `Named` to
/// `import { X } from '...'` / `import { X as Y } from '...'`;
/// `Namespace` to `import * as X from '...'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedImport {
    /// `import <local_name> from "<specifier>"`
    Default {
        /// Local binding name.
        local_name: String,
    },
    /// `import { <exported_name> [as <local_alias>] } from "<specifier>"`
    /// — `type_only` controls whether the binding is rendered with
    /// the `type` qualifier.
    Named {
        /// Original exported name.
        exported_name: String,
        /// Optional local rename.
        local_alias: Option<String>,
        /// `true` for `import { type X }` / `import type { X }`.
        type_only: bool,
    },
    /// `import * as <local_name> from "<specifier>"`
    Namespace {
        /// Local namespace name.
        local_name: String,
    },
}

// ---------------------------------------------------------------------------
// Re-export aliases for the public host API
// ---------------------------------------------------------------------------

/// Type-arguments slice accepted by the public `resolve_named_symbol`
/// host methods. Aliased to a slice of [`TypeExpr`] for clarity at the
/// call site — the lowering happens inside the host method per §5.2.
pub type TypeArgList<'a> = &'a [Arc<TypeExpr>];
