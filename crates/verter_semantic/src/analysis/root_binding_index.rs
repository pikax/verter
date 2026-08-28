//! Owner-aware root value-binding index.
//!
//! Answers, for one identifier reference in a script's parsed program,
//! whether the name resolves to a real runtime-surviving LOCAL declaration
//! under Vue's owner scope topology (module vs instance), to the
//! language/runtime GLOBAL by identity, or is statically INDETERMINATE
//! (`with`, a sloppy direct `eval` whose leak reaches the query, ambiguous
//! owner topology, or a non-clean parse).
//!
//! This is binding resolution, not type resolution: it never calls, is never
//! reachable from, and never participates in `ProjectSemanticDispatch` /
//! `SemanticQueryKey` dispatch. Construction and querying live ONLY in the
//! shallow/macro-analysis producer path (`build_script_analysis_inner` and
//! its two consumers). See
//! the binding-index design (v3 plus
//! the OXC-boundary addendum) for the full ratified design this module
//! implements.
//!
//! # The index itself
//!
//! ONE dedicated binding clone is built per parse (never the shared retained
//! `Program` other consumers walk by flat statement index):
//!
//! - Module-owned top-level statements are copied as direct Program-root
//!   siblings, in original order, exactly as authored.
//! - `ImportDeclaration` / `TSImportEqualsDeclaration` top-level statements
//!   ALWAYS stay Program-root siblings regardless of `TopLevelOwnerTable`
//!   owner — Vue's compiler hoists imports to true module scope, and an ES
//!   import cannot syntactically live inside a function body anyway.
//! - Every OTHER instance-owned top-level statement is wrapped as the body of
//!   ONE synthetic, non-binding anonymous function EXPRESSION (never a named
//!   `FunctionDeclaration`, which would bind its own name in Program scope),
//!   appended after the module-owned statements. This is a real child scope
//!   of Program root, matching Vue's actual compiled shape
//!   (`<script setup>` content becomes the body of `setup()`, itself nested
//!   in the options object at module top level).
//! - `Program.directives` are classified per-directive via
//!   `TopLevelOwnerTable::owner_of_span` (directives are not covered by the
//!   per-statement owner table) and placed in the matching prologue.
//! - The shared [`crate::analysis::runtime_survival_erasure`] projection
//!   (Vue's delta) runs over the WHOLE clone before binding, so an ambient
//!   `declare`, a type-only construct, or a type-only
//!   `TSImportEqualsDeclaration` never fools the binder into treating it as a
//!   runtime value.
//! - The clone's `Program.source_type` is copied from the RETAINED parse's
//!   own resolved `source_type` (the parser's disambiguated value), never the
//!   caller's separate pre-parse parameter.
//! - Only an AMBIGUOUS owner topology (more than one conflicting module
//!   owner, or more than one instance owner) — or a script whose own parse
//!   recovered from errors — skips the clone/bind entirely, answering
//!   [`BindingResolution::Indeterminate`] for every query. An ABSENT module
//!   owner is NOT ambiguous and NOT degenerate: a `<script setup>`-only
//!   SFC (no plain `<script>` block at all) has no module owner and is the
//!   single most common real-world Vue SFC shape — it still binds and
//!   resolves normally, with `Local` results attributed to the sole
//!   instance owner instead (see `resolve_from_program_root` /
//!   `BuiltState::owner_for_symbol`).
//!
//! Correlation between the clone's bound `IdentifierReference`s and the
//! CALLER's original retained AST nodes is by BYTE SPAN, never by node
//! identity or `SymbolId`/`ReferenceId` (those are dense indices local to
//! this one clone's bind and mean nothing elsewhere): spans are stable `u32`
//! offsets into the same source text both trees were parsed from.

use oxc_allocator::{Allocator, CloneIn};
use oxc_ast::ast::*;
use oxc_ast::{AstBuilder, NONE};
use oxc_ast_visit::{walk, Visit, VisitMut};
use oxc_semantic::{ReferenceId, ScopeId, Scoping, SemanticBuilder, SymbolFlags, SymbolId};
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};
use verter_type_expr::{DeclBindingKey, TopLevelOwnerId, TopLevelOwnerKind};

use crate::analysis::runtime_survival_erasure::{ErasureDelta, RuntimeSurvivalProjection};
use crate::analysis::top_level_owners::TopLevelOwnerTable;

/// Per-consumer resolution start scope (v3 amendment).
///
/// Vue's compiler RELOCATES a `defineProps`/`defineModel`/Options `props:`
/// runtime-constructor argument out of `setup()` before `setup()` ever runs
/// (the argument is emitted directly into the component's `props` option) —
/// so an ordinary instance-local, non-import declaration sitting textually
/// beside the macro call is never shadow-relevant to it. A `defineExpose`-
/// style consumer genuinely executes inside the compiled `setup()` body and
/// must see instance-local declarations normally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartScope {
    /// Resolve as if the queried identifier were spliced directly at Program
    /// (module) root — never sees an instance-owned declaration, regardless
    /// of where the identifier's AST node physically sits in the clone.
    ProgramRoot,
    /// Resolve from the identifier's own natural owner scope (the wrapper's
    /// function scope for an instance-owned reference, Program root for a
    /// module-owned one) — the ordinary lexical walk.
    OwnerNaturalScope,
}

/// Binding-resolution outcome for one identifier query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingResolution {
    /// No local declaration binds this name in the runtime-surviving scope
    /// graph reachable from the query's start scope — safe to treat as the
    /// language/runtime global by identity.
    Global,
    /// Bound to a real local, runtime-surviving value declaration. Resolve
    /// through the general authored-value-reference route keyed by this
    /// pair — never through name-based global semantics.
    Local(DeclBindingKey),
    /// Static resolution does not apply (a `with`/sloppy-direct-`eval` scope
    /// the walk passes through, ambiguous/missing owner topology, or a
    /// non-clean parse). Fails closed — never defaults to `Global`.
    Indeterminate,
}

impl From<BindingResolution> for verter_type_expr::ConstructorBindingOutcome {
    fn from(resolution: BindingResolution) -> Self {
        match resolution {
            BindingResolution::Global => verter_type_expr::ConstructorBindingOutcome::Global,
            BindingResolution::Local(key) => {
                verter_type_expr::ConstructorBindingOutcome::Local(key)
            }
            BindingResolution::Indeterminate => {
                verter_type_expr::ConstructorBindingOutcome::Indeterminate
            }
        }
    }
}

/// One dedicated per-parse binding index. Transient: never persisted,
/// memoized across file versions, or exposed as a `ProjectTypeStore` cache
/// layer.
pub struct RootBindingIndex {
    state: IndexState,
}

enum IndexState {
    /// Ambiguous/missing owner topology, or a non-clean parse. Every query
    /// answers `Indeterminate`.
    Degenerate,
    Built(Box<BuiltState>),
}

struct BuiltState {
    scoping: Scoping,
    /// Byte-span-keyed correlation from the ORIGINAL retained AST's
    /// identifier spans to this clone's bound reference identity.
    references: FxHashMap<verter_span::Span, ReferenceEntry>,
    /// Absent for a real, valid topology: a `<script setup>`-only SFC has
    /// NO module (`<script>`) region at all — that is not degenerate, it is
    /// the single most common real-world Vue SFC shape. Only an AMBIGUOUS
    /// module topology (more than one conflicting module owner) is
    /// degenerate; that case never reaches `Built` at all (see `build`).
    module_owner: Option<TopLevelOwnerId>,
    instance_owner: Option<TopLevelOwnerId>,
    /// Authored owner for a Program-root-landing binding, keyed by the bound
    /// symbol's OWN `SymbolId` (never by name text — two distinct bindings
    /// can share a spelling, and a name-keyed map cannot tell them apart)
    /// and populated by BYTE-SPAN correlation, never by statement/import
    /// kind: every symbol `iter_bindings_in` `program_root_scope_id` has its
    /// declaring identifier's span (`Scoping::symbol_span`) matched against
    /// whichever ORIGINAL top-level statement span contains it
    /// (`root_statement_owners` in [`build_and_bind`]), and that statement's
    /// OWN authored owner wins — never `module_owner` by default. This
    /// covers every root-landing statement kind uniformly (an
    /// Instance-owned import lands at Program root and keeps its Instance
    /// owner even beside a Module region; a Frontmatter-owned declaration
    /// keeps Frontmatter; Module-owned declarations resolve to Module,
    /// matching the old default) and is self-erasure-safe: a type-only
    /// import the runtime-survival projection erases before binding
    /// produces no `SymbolId` at all, so it never enters this map. A lookup
    /// miss (a symbol with no mapped statement, unreachable in practice)
    /// falls through to `module_owner.or(instance_owner)`. Both resolution
    /// arms (`resolve_from_program_root` AND `resolve_natural`) consult this
    /// SAME map keyed by the SAME `SymbolId` — there is exactly one owner-
    /// attribution strategy for a Program-root-bound symbol, not one per
    /// arm.
    root_binding_owner_by_symbol: FxHashMap<SymbolId, TopLevelOwnerId>,
    /// Program-root symbols whose declaration + every recorded
    /// `Scoping::symbol_redeclarations` span resolve to MORE THAN ONE
    /// distinct owner (e.g. a Module-owned and an Instance-owned import
    /// sharing a local name, merged by OXC's binder onto one canonical
    /// `SymbolId` that keeps only the FIRST declaration's `symbol_span`).
    /// A same-owner redeclaration (`var Custom = 1; var Custom = 2;` in the
    /// same script) is legal and NOT recorded here — only a genuine
    /// cross-owner collision is, and querying this set first fails closed
    /// to `Indeterminate` rather than guessing among the conflicting
    /// owners. Consulted by BOTH resolution arms — an ambiguous symbol
    /// fails closed regardless of which `StartScope` reached it.
    root_ambiguous_binding_symbols: FxHashSet<SymbolId>,
    program_root_scope_id: ScopeId,
    /// The synthetic instance wrapper's function scope, when an instance
    /// side exists.
    wrapper_scope_id: Option<ScopeId>,
    /// The nearest VARIABLE-environment scope (function, Program root,
    /// class-static-block, or `declare namespace` block — see
    /// `ScopeFlags::is_var`) enclosing a non-optional call whose callee is a
    /// Reference named `eval` (an identifier, possibly wrapped in grouping
    /// parentheses or TypeScript type-assertion forms that compile away),
    /// where that variable-environment scope is sloppy (non-strict) mode.
    /// Sloppy direct `eval`'s `var`/function-declaration leak attaches to
    /// that nearest variable environment, not the exact lexical block
    /// containing the call, so marking the block itself would miss sibling
    /// references outside it but still within the same function/program. A
    /// strict-mode direct eval can never leak a binding outward, and a
    /// locally shadowed `eval` name is an ordinary function call with no
    /// scope-injection power — neither is recorded here. Optional-call and
    /// comma-expression callees are spec-indirect and are not recorded.
    sloppy_eval_scopes: FxHashSet<ScopeId>,
}

struct ReferenceEntry {
    name: String,
    reference_id: ReferenceId,
}

#[cfg(test)]
thread_local! {
    /// Thread-local, not a process-global static: the assertions that read
    /// it are exact equalities against zero, so a concurrently-running test
    /// building an index of its own would turn them into intermittent false
    /// failures under threaded libtest. `build` runs on its caller's thread
    /// (its `&Program` is arena-bound and never crosses one), so a
    /// thread-local counter observes exactly the builds the reading test
    /// caused.
    static BUILD_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_build_count() -> usize {
    BUILD_COUNT.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_test_build_count() {
    BUILD_COUNT.with(|count| count.set(0));
}

impl RootBindingIndex {
    /// Build the index once per parse. `program`/`owners` are the RETAINED
    /// parse and its validated owner table; `parse_errors` is whether that
    /// parse recovered from errors (a non-clean parse never yields a trusted
    /// index).
    #[must_use]
    pub fn build(program: &Program<'_>, owners: &TopLevelOwnerTable, parse_errors: bool) -> Self {
        #[cfg(test)]
        BUILD_COUNT.with(|count| count.set(count.get() + 1));
        if parse_errors {
            return Self {
                state: IndexState::Degenerate,
            };
        }

        // Module is optional (a `<script setup>`-only SFC has none — the
        // most common real-world Vue SFC shape), so "absent" and
        // "ambiguous" must be distinguished the same way Instance already
        // is below: only the latter is degenerate. `unique_owner_of_kind`
        // alone conflates the two into `None`.
        let has_module_owner = owners
            .statements()
            .iter()
            .map(|statement| statement.owner)
            .chain(owners.regions().iter().map(|region| region.owner))
            .any(|owner| owner.kind() == TopLevelOwnerKind::Module);
        let module_owner = if has_module_owner {
            match owners.unique_owner_of_kind(TopLevelOwnerKind::Module) {
                Some(owner) => Some(owner),
                None => {
                    return Self {
                        state: IndexState::Degenerate,
                    }
                }
            }
        } else {
            None
        };

        // Instance is optional (a plain script with no `<script setup>` has
        // none), so "absent" and "ambiguous" must be distinguished: only the
        // latter is degenerate.
        let has_instance_owner = owners
            .statements()
            .iter()
            .map(|statement| statement.owner)
            .chain(owners.regions().iter().map(|region| region.owner))
            .any(|owner| owner.kind() == TopLevelOwnerKind::Instance);
        let instance_owner = if has_instance_owner {
            match owners.unique_owner_of_kind(TopLevelOwnerKind::Instance) {
                Some(owner) => Some(owner),
                None => {
                    return Self {
                        state: IndexState::Degenerate,
                    }
                }
            }
        } else {
            None
        };

        let allocator = Allocator::default();
        let built = build_and_bind(&allocator, program, owners, module_owner, instance_owner);
        Self {
            state: IndexState::Built(Box::new(built)),
        }
    }

    /// Resolve the identifier whose ORIGINAL-AST byte span is `at`.
    #[must_use]
    pub fn resolve_value_identifier(
        &self,
        at: verter_span::Span,
        start_scope: StartScope,
    ) -> BindingResolution {
        let IndexState::Built(state) = &self.state else {
            return BindingResolution::Indeterminate;
        };
        let Some(entry) = state.references.get(&at) else {
            // The queried span has no correlated bound reference — the
            // identifier was erased by the runtime-survival projection, or
            // never made it into the clone. Fail closed rather than guess.
            return BindingResolution::Indeterminate;
        };
        // `with` / direct-`eval` are read off the identifier's OWN PHYSICAL
        // scope, regardless of `start_scope` — `ProgramRoot` only overrides
        // WHERE we search for a declaring name, never whether the reference's
        // real textual position is dynamically ambiguous. Checking this once,
        // walking the physical scope's full ancestor chain to true root,
        // covers both modes: a runtime-constructor argument physically
        // sitting inside a `with`/sloppy-eval block in the instance script is
        // `Indeterminate` even though `ProgramRoot` mode never looks at that
        // scope for the name search itself.
        let physical_scope = state.scoping.get_reference(entry.reference_id).scope_id();
        if state.chain_is_indeterminate(physical_scope) {
            return BindingResolution::Indeterminate;
        }
        match start_scope {
            StartScope::ProgramRoot => state.resolve_from_program_root(&entry.name),
            StartScope::OwnerNaturalScope => state.resolve_natural(entry),
        }
    }

    /// Test-only introspection: how many scopes are recorded as containing a
    /// possible direct `eval`. A `with`-shadowed eval callee's
    /// classification is unobservable through [`Self::resolve_value_identifier`]
    /// for ANY consuming reference physically inside (or nested within) the
    /// `with` block, because that same query is already forced
    /// `Indeterminate` by the independent `with`-ancestor check above —
    /// this exposes the internal record directly so a regression that makes
    /// a with-shadowed callee wrongly "provably safe" (and so never
    /// recorded here at all) still fails a test.
    #[cfg(test)]
    pub(crate) fn sloppy_eval_scope_count(&self) -> usize {
        match &self.state {
            IndexState::Built(state) => state.sloppy_eval_scopes.len(),
            IndexState::Degenerate => 0,
        }
    }
}

impl BuiltState {
    /// The ONE owner-attribution strategy for a resolved `SymbolId`, shared
    /// by both `StartScope` arms. A Program-root-bound symbol (present in
    /// `root_binding_owner_by_symbol`) uses its span-correlated AUTHORED
    /// owner, gated first by `root_ambiguous_binding_symbols` (a genuine
    /// cross-owner collision fails closed regardless of caller). A symbol
    /// declared exactly at the synthetic instance wrapper's OWN top-level
    /// function scope (a genuine `<script setup>` top-level declaration —
    /// there is only ever one instance owner, so this can never collide)
    /// attributes to the sole instance owner.
    ///
    /// Every OTHER declaring scope — anything nested one level deeper than
    /// either top level (a function/block-local inside `mod()`/`fm()`/
    /// `setup()`), and anything under a non-wrapped owner kind (e.g.
    /// Frontmatter, which lands at Program root only for its OWN top-level
    /// statements, never for locals nested inside one) — fails closed.
    /// `DeclBindingKey` names a TOP-LEVEL owner declaration (the entry the
    /// general authored-value-reference route looks up); a nested local has
    /// no such top-level entry, so returning `owner_of_scope`'s old binary
    /// module-vs-instance guess here would either collide two distinct
    /// nested bindings sharing a name onto the SAME `(owner, name)` key, or
    /// (for a non-wrapped owner kind) silently mislabel a Frontmatter-owned
    /// nested local as Module. Never guess: fail closed instead.
    fn owner_for_symbol(&self, symbol_id: SymbolId) -> Option<TopLevelOwnerId> {
        if self.root_ambiguous_binding_symbols.contains(&symbol_id) {
            return None;
        }
        if let Some(owner) = self.root_binding_owner_by_symbol.get(&symbol_id) {
            return Some(*owner);
        }
        let declaring_scope = self.scoping.symbol_scope_id(symbol_id);
        if self.wrapper_scope_id == Some(declaring_scope) {
            return self.instance_owner.or(self.module_owner);
        }
        None
    }

    fn resolve_from_program_root(&self, name: &str) -> BindingResolution {
        match self
            .scoping
            .find_binding(self.program_root_scope_id, name.into())
        {
            // Found at Program root — attribute through the SAME
            // symbol-keyed owner authority `resolve_natural` uses (see
            // `owner_for_symbol`). `None` covers both a genuine cross-owner
            // ambiguity and the practically-unreachable case neither
            // `module_owner` nor `instance_owner` exists — either way this
            // fails closed rather than fabricating an owner.
            Some(symbol_id) => match self.owner_for_symbol(symbol_id) {
                Some(owner) => {
                    BindingResolution::Local(DeclBindingKey::new(owner, name.to_string()))
                }
                None => BindingResolution::Indeterminate,
            },
            None => BindingResolution::Global,
        }
    }

    fn resolve_natural(&self, entry: &ReferenceEntry) -> BindingResolution {
        let reference = self.scoping.get_reference(entry.reference_id);
        match reference.symbol_id() {
            // Attribute through the SAME symbol-keyed owner authority
            // `resolve_from_program_root` uses — a Program-root-bound
            // symbol reached via its natural lexical scope (e.g. a module
            // variable referenced from inside the instance wrapper) gets
            // the SAME authored owner and the SAME ambiguity gate as when
            // reached from `ProgramRoot` mode; a genuine instance
            // top-level symbol (declared directly in the wrapper's own
            // function scope) attributes to the sole instance owner; any
            // more deeply nested local fails closed (see
            // `owner_for_symbol`).
            Some(symbol_id) => match self.owner_for_symbol(symbol_id) {
                Some(owner) => {
                    BindingResolution::Local(DeclBindingKey::new(owner, entry.name.clone()))
                }
                None => BindingResolution::Indeterminate,
            },
            None => BindingResolution::Global,
        }
    }

    /// Whether the identifier's own physical scope, walking its full ancestor
    /// chain up to true Program root, passes through a `with` scope or a
    /// sloppy-mode direct-eval-containing scope. Checked once per query,
    /// from the reference's real textual position — independent of
    /// `StartScope`, which only changes where we search for a declaring name.
    fn chain_is_indeterminate(&self, from: ScopeId) -> bool {
        self.scoping.scope_ancestors(from).any(|scope| {
            self.scoping.scope_flags(scope).is_with() || self.sloppy_eval_scopes.contains(&scope)
        })
    }
}

/// Build the dedicated binding clone in `allocator`, bind it, and extract the
/// owned query state. The clone (and `allocator`) is dropped at the end of
/// this function — only owned data (`Scoping`, the span map, scope ids)
/// survives.
fn build_and_bind(
    allocator: &Allocator,
    program: &Program<'_>,
    owners: &TopLevelOwnerTable,
    module_owner: Option<TopLevelOwnerId>,
    instance_owner: Option<TopLevelOwnerId>,
) -> BuiltState {
    let ast = AstBuilder::new(allocator);

    let mut root_body = ast.vec();
    let mut wrapper_body_stmts = ast.vec();
    // One entry per ORIGINAL top-level statement that lands at Program root
    // (import or not), recording its own authored owner. Statements are
    // sibling AST nodes, so their spans never overlap — a bound symbol's
    // declaring-identifier span is contained in exactly one of these below.
    let mut root_statement_owners: Vec<(oxc_span::Span, TopLevelOwnerId)> = Vec::new();
    for (index, stmt) in program.body.iter().enumerate() {
        let is_import = matches!(
            stmt,
            Statement::ImportDeclaration(_) | Statement::TSImportEqualsDeclaration(_)
        );
        let owner = owners.statement(index).owner;
        let cloned = stmt.clone_in(allocator);
        if !is_import && instance_owner.is_some() && owner.kind() == TopLevelOwnerKind::Instance {
            wrapper_body_stmts.push(cloned);
        } else {
            root_statement_owners.push((stmt.span(), owner));
            root_body.push(cloned);
        }
    }

    let mut root_directives = ast.vec();
    let mut wrapper_directives = ast.vec();
    for directive in &program.directives {
        let cloned = directive.clone_in(allocator);
        let owner = owners.owner_of_span(directive.span.into());
        if instance_owner.is_some()
            && matches!(owner, Some(owner) if owner.kind() == TopLevelOwnerKind::Instance)
        {
            wrapper_directives.push(cloned);
        } else {
            root_directives.push(cloned);
        }
    }

    let has_wrapper_content = instance_owner.is_some()
        && (!wrapper_body_stmts.is_empty() || !wrapper_directives.is_empty());
    if has_wrapper_content {
        let synthetic_span = oxc_span::Span::new(0, 0);
        let function_body =
            ast.alloc_function_body(synthetic_span, wrapper_directives, wrapper_body_stmts);
        let params = ast.alloc_formal_parameters(
            synthetic_span,
            FormalParameterKind::FormalParameter,
            ast.vec(),
            NONE,
        );
        let wrapper_expr = ast.expression_function(
            synthetic_span,
            FunctionType::FunctionExpression,
            None,
            false,
            false,
            false,
            NONE,
            NONE,
            params,
            NONE,
            Some(function_body),
        );
        root_body.push(ast.statement_expression(synthetic_span, wrapper_expr));
    }

    let mut clone = ast.program(
        program.span,
        program.source_type,
        program.source_text,
        ast.vec(),
        None,
        root_directives,
        root_body,
    );

    RuntimeSurvivalProjection::new(allocator, ErasureDelta::vue()).visit_program(&mut clone);

    let built = SemanticBuilder::new().build(&clone);

    let mut collector = ReferenceCollector {
        references: FxHashMap::default(),
        eval_callee_reference_ids: Vec::new(),
    };
    collector.visit_program(&clone);

    let scoping = built.semantic.into_scoping();

    let program_root_scope_id = clone.scope_id.get().unwrap_or(scoping.root_scope_id());

    // Authored owner per Program-root-bound `SymbolId` (never per name text
    // — see `BuiltState::root_binding_owner_by_symbol`), by span containment
    // against `root_statement_owners`. A symbol whose erased source
    // statement never made it into the clone (a type-only import) never
    // appears in `iter_bindings_in` at all — self-erasure-safe by
    // construction, no separate erasure check needed here. A REDECLARED
    // symbol (`symbol_redeclarations` non-empty — e.g. `var Custom = 1; var
    // Custom = 2;`, or two conflicting imports under different owners)
    // merges onto ONE canonical `SymbolId` in OXC's binder, keeping only
    // the FIRST declaration's span as `symbol_span`. Trusting that alone
    // would silently attribute whichever declaration the binder happened
    // to keep first — instead, every declaration span (the primary one
    // plus each redeclaration's own recorded span) is resolved to its own
    // owner and the OWNER SET decides: a same-owner redeclaration (legal,
    // ordinary) still resolves normally, and only a genuine CROSS-owner
    // collision marks the symbol ambiguous (see
    // `BuiltState::root_ambiguous_binding_symbols`). Every declaration span
    // for a Program-root-bound symbol is expected to fall inside exactly
    // one `root_statement_owners` entry — but a span that fails to map is
    // treated as ambiguous too, never silently dropped from the set: an
    // owner computed from a PARTIAL span set is not an authored owner, it
    // is a guess with a gap in it, and this module never guesses.
    let owner_of_span = |span: oxc_span::Span| {
        root_statement_owners
            .iter()
            .find_map(|&(stmt_span, owner)| {
                (stmt_span.start <= span.start && span.end <= stmt_span.end).then_some(owner)
            })
    };
    let mut root_binding_owner_by_symbol = FxHashMap::default();
    let mut root_ambiguous_binding_symbols = FxHashSet::default();
    for symbol_id in scoping.iter_bindings_in(program_root_scope_id) {
        let mut owners: Vec<TopLevelOwnerId> = Vec::new();
        let mut every_span_mapped = true;
        for span in std::iter::once(scoping.symbol_span(symbol_id)).chain(
            scoping
                .symbol_redeclarations(symbol_id)
                .iter()
                .map(|redeclaration| redeclaration.span),
        ) {
            match owner_of_span(span) {
                Some(owner) => owners.push(owner),
                None => every_span_mapped = false,
            }
        }
        owners.sort_unstable();
        owners.dedup();
        match owners.as_slice() {
            [owner] if every_span_mapped => {
                root_binding_owner_by_symbol.insert(symbol_id, *owner);
            }
            _ => {
                root_ambiguous_binding_symbols.insert(symbol_id);
            }
        }
    }

    // The synthetic wrapper, when built, is always the LAST root-body
    // statement (appended after every module-owned statement) — locate it
    // structurally, never by name/text.
    let wrapper_scope_id = has_wrapper_content
        .then(|| match clone.body.last() {
            Some(Statement::ExpressionStatement(expr_stmt)) => match &expr_stmt.expression {
                Expression::FunctionExpression(function) => function.scope_id.get(),
                _ => None,
            },
            _ => None,
        })
        .flatten();

    let mut sloppy_eval_scopes = FxHashSet::default();
    for reference_id in collector.eval_callee_reference_ids {
        let reference = scoping.get_reference(reference_id);
        let scope_id = reference.scope_id();
        // A `with` object can supply its OWN `eval` property at runtime,
        // intercepting the lookup before it ever reaches the statically
        // resolved binding below — OXC documents walking past `with`
        // scopes as a known resolution limitation
        // (`oxc_semantic::is_global_reference`), so a callee lexically
        // resolved to a local symbol can still be the real intrinsic
        // `%eval%` at runtime (`with ({ eval: trueEval }) { eval(...) }`).
        // Every provable-safety argument below assumes the resolved
        // symbol is what actually runs, which does not hold once a
        // `with` sits anywhere on the callee's own physical scope chain
        // — checked unconditionally, before any other shortcut.
        let with_shadow_possible = scoping
            .scope_ancestors(scope_id)
            .any(|scope| scoping.scope_flags(scope).is_with());
        // The spec's direct-eval test is a VALUE-identity check (is the
        // resolved value `SameValue` as the intrinsic `%eval%`), not an
        // "is this name bound" check — `var eval = window.eval; eval(...)`
        // is still direct eval despite `eval` being a local binding, and
        // that can never be proven statically. A FUNCTION declaration's OWN
        // value is a fresh function object, never referentially equal to
        // `%eval%` — but only as long as no OTHER declaration/assignment
        // could still reach this binding with a different value:
        // `symbol_is_mutated` covers every ordinary write REFERENCE
        // anywhere in the program (`eval = window.eval;`, regardless of
        // call-site order), but a `var`/function REDECLARATION with its own
        // initializer (`var eval = trueEval;`) is a declaration, not a
        // `Reference`, and `symbol_is_mutated` cannot see it — requiring
        // `symbol_redeclarations` to be empty too closes that gap.
        // `symbol_is_mutated`/`symbol_redeclarations` only ever see writes
        // that are themselves `Reference`s to this identifier. A
        // declaration sitting at the OUTERMOST scope of a real classic
        // (`ModuleKind::Script`) script is ALSO installed as a property of
        // the global object (Annex B `GlobalDeclarationInstantiation`), so
        // its storage is shared with `globalThis`/`window`/any other alias
        // of the global object — a property WRITE through one of those
        // aliases mutates the very binding an unqualified `eval` resolves
        // to without ever emitting a `Reference` to the `eval` identifier
        // itself, which no identifier-based check can see. This is
        // `is_script()` specifically, not merely "not a module": an ES
        // Module's top-level bindings are never global-object-aliased, and
        // neither is CommonJS's (the whole file body is wrapped in a
        // function at load time), so both stay exempt. Any declaration
        // nested inside a function (never reachable via a global-object
        // property write, regardless of module kind) carries no such risk
        // either and stays provable. Every other local binding kind
        // (var/let/const/param/class/import/catch), any mutated function
        // binding, any redeclared one, and any callee reachable through a
        // `with` falls through and still gets recorded — fail closed
        // rather than guess.
        let declared_at_global_aliasable_scope = |symbol_id: SymbolId| {
            program.source_type.is_script()
                && scoping.symbol_scope_id(symbol_id) == program_root_scope_id
        };
        if !with_shadow_possible {
            if let Some(symbol_id) = reference.symbol_id() {
                let flags = scoping.symbol_flags(symbol_id);
                let provably_safe = flags.contains(SymbolFlags::Function)
                    && !scoping.symbol_is_mutated(symbol_id)
                    && scoping.symbol_redeclarations(symbol_id).is_empty()
                    && !declared_at_global_aliasable_scope(symbol_id);
                if provably_safe {
                    continue;
                }
            }
        }
        // Direct eval from a STRICT caller (class body, class field
        // initializer, strict function) gets a fresh variable environment
        // (ECMA-262 PerformEval) and MUST NOT leak `var` into a surrounding
        // sloppy variable environment. Check the CALL SITE's own
        // strictness first — climbing to the nearest `is_var()` ancestor
        // and then reading THAT scope's flags would walk past a strict
        // non-var class body into a sloppy Program and wrongly record it.
        // Only a sloppy caller then climbs to the nearest variable
        // environment, which is the leak target.
        let call_scope = reference.scope_id();
        if scoping.scope_flags(call_scope).is_strict_mode() {
            continue;
        }
        let var_scope_id = scoping
            .scope_ancestors(call_scope)
            .find(|&scope| scoping.scope_flags(scope).is_var())
            .unwrap_or(call_scope);
        if !scoping.scope_flags(var_scope_id).is_strict_mode() {
            sloppy_eval_scopes.insert(var_scope_id);
        }
    }

    BuiltState {
        scoping,
        references: collector.references,
        module_owner,
        instance_owner,
        root_binding_owner_by_symbol,
        root_ambiguous_binding_symbols,
        program_root_scope_id,
        wrapper_scope_id,
        sloppy_eval_scopes,
    }
}

/// Read-only, post-bind walk collecting: (1) every `IdentifierReference`'s
/// SFC-absolute span (== byte span, since the clone was parsed/cloned from
/// the same source text) paired with its bound reference identity, and (2)
/// every non-optional call whose callee is a Reference named `eval` (used
/// to derive [`BuiltState::sloppy_eval_scopes`] post-bind). Grouping
/// parentheses and TypeScript type-assertion wrappers preserve that
/// Reference; comma-expressions and optional-call forms do not.
struct ReferenceCollector {
    references: FxHashMap<verter_span::Span, ReferenceEntry>,
    eval_callee_reference_ids: Vec<ReferenceId>,
}

/// Spec-direct eval's callee is a Reference whose referenced name is
/// `"eval"` (EvaluateCall). The grouping operator does not apply GetValue,
/// so nested parentheses still yield that Reference. TypeScript `as` /
/// `satisfies` / `!` / type-assertion / instantiation wrappers compile
/// away and likewise preserve it (`Expression::get_inner_expression`
/// peels exactly those forms). Sequence expressions, member access, and
/// every other GetValue-forcing wrapper return `None` — those are
/// spec-indirect. Optional-call is gated separately on
/// `CallExpression::optional` (a different Call production).
fn eval_callee_reference<'a>(callee: &'a Expression<'a>) -> Option<&'a IdentifierReference<'a>> {
    match callee.get_inner_expression() {
        Expression::Identifier(ident) if ident.name == "eval" => Some(ident),
        _ => None,
    }
}

impl<'a> Visit<'a> for ReferenceCollector {
    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        if let Some(reference_id) = ident.reference_id.get() {
            self.references.insert(
                ident.span.into(),
                ReferenceEntry {
                    name: ident.name.as_str().to_string(),
                    reference_id,
                },
            );
        }
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        if !expr.optional {
            if let Some(callee) = eval_callee_reference(&expr.callee) {
                if let Some(reference_id) = callee.reference_id.get() {
                    self.eval_callee_reference_ids.push(reference_id);
                }
            }
        }
        walk::walk_call_expression(self, expr);
    }
}

#[allow(dead_code)]
const fn _assert_source_type_is_copy(_: SourceType) {}

/// Resolve one runtime-constructor-position identifier (a bare
/// `defineProps`/`defineModel`/Options `props:` spelling, or one element of
/// a constructor array) against `index`, always from
/// [`StartScope::ProgramRoot`] — the shared gate every consumer runs before
/// applying the ten-spelling runtime-constructor mapping. See "Consumer
/// wiring" in the binding-index design.
#[must_use]
pub(crate) fn resolve_constructor_binding(
    index: &RootBindingIndex,
    ident: &IdentifierReference<'_>,
) -> verter_type_expr::ConstructorBindingEntry {
    verter_type_expr::ConstructorBindingEntry {
        identity: verter_type_expr::RuntimeConstructorIdentity::classify(ident.name.as_str()),
        resolution: index
            .resolve_value_identifier(ident.span.into(), StartScope::ProgramRoot)
            .into(),
    }
}

#[cfg(test)]
#[path = "root_binding_index_tests.rs"]
mod root_binding_index_tests;
