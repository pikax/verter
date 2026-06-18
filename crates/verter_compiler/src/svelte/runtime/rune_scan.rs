//! Scope-aware Svelte 5 rune-use detection.
//!
//! Reports whether any rune NAME (`$state` / `$derived` / `$props` / `$effect` /
//! `$bindable` / `$inspect` / `$host`) appears as an UNRESOLVED reference in a
//! script — i.e. a reference NOT bound to a local of the same name. This is the
//! syntax-side input to the runes-vs-legacy MODE inference.
//!
//! It reuses the SAME lexical-scope `ShadowStack` model the [`ScriptUseCollector`]
//! (in [`super::expr`]) uses — program / function / arrow / block / catch /
//! for-loop frames — so the shadowing semantics are identical across the two
//! syntax-side collectors; there is no second scope model.

use oxc_ast::ast::{
    ArrowFunctionExpression, BlockStatement, CatchClause, ForInStatement, ForOfStatement,
    ForStatement, Function, IdentifierReference, Program, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};

use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_pattern_names,
    collect_var_hoists, for_left_names, function_scope_names, ShadowStack,
};

/// The Svelte 5 rune NAMES (`compiler/utils.js` `RUNES`, minus the `$state.snapshot`
/// / `.raw` / `.by` member keypaths the detector reaches through the root
/// identifier). A component is in runes mode iff any of these appears as an
/// UNRESOLVED reference (not bound to a local) anywhere in a script — matching the
/// official `Array.from(scope.references.keys()).some(is_rune)`.
const RUNE_ROOT_NAMES: &[&str] = &[
    "$state",
    "$derived",
    "$props",
    "$effect",
    "$bindable",
    "$inspect",
    "$host",
];

/// A SCOPE-AWARE rune-use detector: it reports whether any rune NAME
/// (`$state`/`$derived`/`$props`/`$effect`/`$bindable`/`$inspect`/`$host`) appears
/// as an UNRESOLVED reference in a script — i.e. a reference NOT bound to a local
/// of the same name.
///
/// This mirrors the official runes-mode detection
/// (`phases/2-analyze/index.js`: `Array.from(scope.references.keys()).some(is_rune)`
/// over the binder-pruned reference set, where `get_global_keypath` returns null
/// when `scope.get(name) !== null`). A shadowing local — most importantly a
/// function PARAMETER named `$state` (`function f($state){ return $state }`) — is a
/// declared binding, so its references do NOT count as rune uses and the component
/// stays in LEGACY mode. The reference need NOT be a call (`const h = $host;` is a
/// runes-mode marker — though `$host` without parentheses is a separate official
/// error the runtime backend raises, the MODE is still runes).
#[derive(Default)]
pub(super) struct ScopeAwareRuneDetector {
    /// Whether an unresolved rune-name reference was seen.
    used: bool,
    /// The active lexical-scope shadow stack.
    scopes: ShadowStack,
}

impl ScopeAwareRuneDetector {
    /// Whether `name` is a rune name that is NOT shadowed by a local binding (so
    /// the reference counts as a rune use, forcing runes mode).
    fn is_unshadowed_rune(&self, name: &str) -> bool {
        RUNE_ROOT_NAMES.contains(&name) && !self.scopes.is_shadowed(name)
    }

    /// Whether the detector observed any unresolved rune reference.
    #[must_use]
    pub(super) fn used(&self) -> bool {
        self.used
    }
}

impl<'a> Visit<'a> for ScopeAwareRuneDetector {
    fn visit_program(&mut self, it: &Program<'a>) {
        // The program (script) scope: its own top-level declarations. A top-level
        // `let $state` is a parse error (the `$` prefix is reserved for non-params),
        // so the program frame normally carries no rune name — but pushing it is
        // harmless and keeps the scope model uniform with `ScriptUseCollector`.
        let mut frame = rustc_hash::FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        // A function PARAMETER named `$state` shadows the rune — the canonical X5
        // legacy case (`function f($state){ return $state }`).
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.scopes.push(frame);
        walk::walk_catch_clause(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        // An unresolved reference to a rune name is a rune use. A reference that
        // resolves to a local binding of the same name (the function-param `$state`)
        // is shadowed and does NOT count.
        if self.is_unshadowed_rune(it.name.as_str()) {
            self.used = true;
        }
        walk::walk_identifier_reference(self, it);
    }
}
