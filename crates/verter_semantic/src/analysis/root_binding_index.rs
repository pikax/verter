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
//! `docs/arch/refactor/rev11/evidence/CM1/binding-index-design.md` (v3 plus
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
//!   `owner_of_scope`).
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
use oxc_semantic::{ReferenceId, ScopeId, Scoping, SemanticBuilder};
use oxc_span::SourceType;
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
    program_root_scope_id: ScopeId,
    /// The synthetic instance wrapper's function scope, when an instance
    /// side exists.
    wrapper_scope_id: Option<ScopeId>,
    /// Scopes that DIRECTLY (not via descendant propagation) contain a
    /// non-optional call to the bare name `eval`, where that exact scope is
    /// sloppy (non-strict) mode. A strict-mode direct eval can never leak a
    /// binding outward and is never recorded here.
    sloppy_eval_scopes: FxHashSet<ScopeId>,
}

struct ReferenceEntry {
    name: String,
    reference_id: ReferenceId,
}

impl RootBindingIndex {
    /// Build the index once per parse. `program`/`owners` are the RETAINED
    /// parse and its validated owner table; `parse_errors` is whether that
    /// parse recovered from errors (a non-clean parse never yields a trusted
    /// index).
    #[must_use]
    pub fn build(program: &Program<'_>, owners: &TopLevelOwnerTable, parse_errors: bool) -> Self {
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
}

impl BuiltState {
    fn resolve_from_program_root(&self, name: &str) -> BindingResolution {
        match self
            .scoping
            .find_binding(self.program_root_scope_id, name.into())
        {
            // Found at Program root. Its true owner is the module owner
            // when one exists; when there is none (a `<script setup>`-only
            // SFC), the ONLY statements that can still land at Program
            // root are imports, which always land there regardless of
            // owner kind — so the sole instance owner is the correct tag.
            // Neither existing is unreachable in a genuinely degenerate
            // topology (build() never reaches Built() there), but this
            // fails closed rather than fabricating an owner if it ever is.
            Some(_symbol_id) => match self.module_owner.or(self.instance_owner) {
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
            Some(symbol_id) => {
                let decl_scope = self.scoping.symbol_scope_id(symbol_id);
                BindingResolution::Local(DeclBindingKey::new(
                    self.owner_of_scope(decl_scope),
                    entry.name.clone(),
                ))
            }
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

    /// Module-vs-instance owner attribution for a declaring scope: instance
    /// iff the scope IS the wrapper's function scope or a descendant of it;
    /// module otherwise — falling back to instance (a `<script setup>`-only
    /// SFC's import) when there is no module owner at all, and to the
    /// ordinary-file placeholder only in the practically unreachable case
    /// neither exists.
    fn owner_of_scope(&self, scope: ScopeId) -> TopLevelOwnerId {
        if let Some(wrapper) = self.wrapper_scope_id {
            if self
                .scoping
                .scope_ancestors(scope)
                .any(|ancestor| ancestor == wrapper)
            {
                return self
                    .instance_owner
                    .or(self.module_owner)
                    .unwrap_or(TopLevelOwnerId::ordinary_file());
            }
        }
        self.module_owner
            .or(self.instance_owner)
            .unwrap_or(TopLevelOwnerId::ordinary_file())
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
        let scope_id = scoping.get_reference(reference_id).scope_id();
        if !scoping.scope_flags(scope_id).is_strict_mode() {
            sloppy_eval_scopes.insert(scope_id);
        }
    }

    BuiltState {
        scoping,
        references: collector.references,
        module_owner,
        instance_owner,
        program_root_scope_id,
        wrapper_scope_id,
        sloppy_eval_scopes,
    }
}

/// Read-only, post-bind walk collecting: (1) every `IdentifierReference`'s
/// SFC-absolute span (== byte span, since the clone was parsed/cloned from
/// the same source text) paired with its bound reference identity, and (2)
/// every non-optional `eval(...)` call's callee reference id (used to derive
/// [`BuiltState::sloppy_eval_scopes`] post-bind).
struct ReferenceCollector {
    references: FxHashMap<verter_span::Span, ReferenceEntry>,
    eval_callee_reference_ids: Vec<ReferenceId>,
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
            if let Expression::Identifier(callee) = &expr.callee {
                if callee.name == "eval" {
                    if let Some(reference_id) = callee.reference_id.get() {
                        self.eval_callee_reference_ids.push(reference_id);
                    }
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
/// wiring" in `docs/arch/refactor/rev11/evidence/CM1/binding-index-design.md`.
#[must_use]
pub(crate) fn resolve_constructor_binding(
    index: &RootBindingIndex,
    ident: &IdentifierReference<'_>,
) -> verter_type_expr::ConstructorBindingEntry {
    verter_type_expr::ConstructorBindingEntry {
        spelling: std::sync::Arc::from(ident.name.as_str()),
        resolution: index
            .resolve_value_identifier(ident.span.into(), StartScope::ProgramRoot)
            .into(),
    }
}

#[cfg(test)]
#[path = "root_binding_index_tests.rs"]
mod root_binding_index_tests;
