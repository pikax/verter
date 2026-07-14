//! Complete free-reference collector for `<script setup>` top-level bindings.
//!
//! Drives unused-binding liveness (issue #7): a top-level setup binding that is
//! referenced NOWHERE in the script body is a candidate for an unused-local
//! diagnostic. The collector answers the inverse question — which setup binding
//! names appear as a FREE reference somewhere in the program — so the IDE
//! lowering can keep those bindings' value-reads alive and demote only the
//! genuinely-unreferenced ones.
//!
//! ## Why a `Visit`-based collector
//!
//! Liveness for a user-visible diagnostic must be SOUND: a missed reference
//! demotes a genuinely-used binding to a type-only read and produces a
//! false-positive TS6133. The previous hand-rolled walker had `_ => {}` arms on
//! both the statement and expression matches, so whole construct families
//! (class bodies, static blocks, labeled statements) and member/computed
//! assignment targets (`foo.value++`, `foo.x = y`, `foo[key] = y`) were silently
//! skipped — exactly the false-positive vectors this collector closes. Building
//! on `oxc_ast_visit::Visit` means the default `walk::*` traverses EVERY
//! statement and expression kind; this collector overrides ONLY the
//! scope-boundary and declaration nodes (to model lexical shadowing) and the
//! free-identifier sinks (to record uses), so no construct is ever silently
//! dropped.
//!
//! ## Scope model
//!
//! A reference suppresses against a stack of scope frames. The PROGRAM frame is
//! intentionally EMPTY: the program's top-level declarations ARE the setup
//! bindings under test, so they must not suppress themselves. Every function /
//! arrow body, `{ … }` block, `for` loop, `catch` clause, `switch` body, class
//! expression body, class static block, and TS module block pushes a frame
//! carrying the names it lexically introduces; a setup name shadowed by such an
//! inner binding is NOT counted as a use of the top-level binding.
//!
//! ## Conservative direction
//!
//! Over-counting a use is the SAFE direction (it only suppresses a diagnostic);
//! under-counting is the unacceptable false-positive direction. Accordingly the
//! collector counts identifier references in TS type positions too (a `typeof
//! name` query, a type annotation referencing a value-space name): these are
//! treated as uses so a binding referenced only from type space is never
//! demoted.

use oxc_ast::ast::*;
use oxc_ast_visit::{walk, Visit};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashSet;

use crate::common::Span;

/// Collect which setup binding names are referenced as free variables anywhere
/// in `program`.
///
/// Returns the subset of `setup_names` that appear as a free reference (in value
/// OR type position), with lexically-scoped shadowing suppression: a `setup`
/// name shadowed by an inner-scope binding at the reference site is not counted.
/// Top-level program declarations are the setup bindings themselves and never
/// suppress.
pub fn collect_setup_binding_refs<'a>(
    program: &'a Program<'a>,
    setup_names: &FxHashSet<&str>,
) -> FxHashSet<&'a str> {
    let mut collector = SetupRefCollector {
        sink: RefSink::Names {
            setup_names: Some(setup_names),
            refs: FxHashSet::default(),
        },
        // The program frame is EMPTY on purpose — top-level declarations are the
        // setup bindings under test and must not suppress their own references.
        scopes: vec![FxHashSet::default()],
    };
    collector.visit_program(program);
    match collector.sink {
        RefSink::Names { refs, .. } => refs,
        RefSink::Spans { .. } => unreachable!("Names sink installed above"),
    }
}

/// Collect EVERY free identifier root referenced anywhere in `expr`, with
/// lexically-scoped shadowing suppression but NO name filter and NO global
/// filter.
///
/// This is the complete `Visit`-based companion to [`collect_setup_binding_refs`]
/// for callers that do not yet know the target name set — notably the
/// `<style> v-bind(expr)` liveness scan, where the partial recursive-descent
/// collector silently dropped global-named roots, assignment LHS targets, and
/// any construct family behind a `_ => {}` arm. Because the default `walk::*`
/// traversal visits every expression kind, no construct is skipped; the only
/// names withheld are those a local inner scope shadows. Collecting too many
/// roots is the SAFE direction for an unused-binding gate (it only suppresses a
/// diagnostic), and the caller intersects this set against the real binding
/// inventory.
pub fn collect_expression_free_refs<'a>(expr: &'a Expression<'a>) -> FxHashSet<&'a str> {
    let mut collector = SetupRefCollector {
        sink: RefSink::Names {
            setup_names: None,
            refs: FxHashSet::default(),
        },
        scopes: vec![FxHashSet::default()],
    };
    collector.visit_expression(expr);
    match collector.sink {
        RefSink::Names { refs, .. } => refs,
        RefSink::Spans { .. } => unreachable!("Names sink installed above"),
    }
}

/// Collect EVERY free type-name root referenced anywhere in `ts_type` into
/// `out`, with lexically-scoped shadowing suppression but NO name filter.
///
/// This is the TYPE-position companion to [`collect_expression_free_refs`] for
/// the v-slot type-annotation LIVENESS feeder. It drives the SAME complete
/// `SetupRefCollector` `Visit` over a `TSType`: the default `walk::*` traversal
/// descends into EVERY nested type position — `TSFunctionType` / `TSConstructorType`
/// / `TSMethodSignature` / call / index / construct-signature parameters, mapped-
/// type constraints, `TSInferType`, `TSImportType`, `TSTemplateLiteralType`,
/// `TSTypePredicate`, and qualified-name roots — and routes each type-name leaf
/// through the single `visit_ts_type_name` sink. The retired hand-rolled type
/// walker (`collect_type_references`) had `_ => {}` arms that skipped those
/// subtrees, so a `typeof Helper` in a function-type PARAM was never reached and
/// the binding it named was falsely demoted to a type-only read. Collecting too
/// many roots is the SAFE direction for an unused-binding gate (it only
/// suppresses a diagnostic); the caller intersects this set against the real
/// binding inventory.
pub fn collect_type_free_ref_names<'a>(ts_type: &'a TSType<'a>) -> FxHashSet<&'a str> {
    let mut collector = SetupRefCollector {
        sink: RefSink::Names {
            setup_names: None,
            refs: FxHashSet::default(),
        },
        scopes: vec![FxHashSet::default()],
    };
    collector.visit_ts_type(ts_type);
    match collector.sink {
        RefSink::Names { refs, .. } => refs,
        RefSink::Spans { .. } => unreachable!("Names sink installed above"),
    }
}

/// Collect the reference SPANS of every free identifier in `expr` (value AND TS
/// type positions), excluding the names in `ignored` and any name shadowed by an
/// inner lexical scope, into `out`.
///
/// This is the SPAN-returning companion to [`collect_expression_free_refs`] for
/// the TEMPLATE main-expression LIVENESS feeder (interpolations / v-if / directive
/// values / dynamic args). That caller has a freshly-parsed `Expression` with
/// substring-relative spans and slices the file `source` itself (shifting by the
/// expression offset), so it consumes `Span`s rather than borrowed names. It is
/// built on the SAME complete `Visit` walker as the name collector — the default
/// `walk::*` traversal visits every node, so a reference inside a nested callback
/// / function body / statement family can never be silently dropped (the failure
/// mode of the retired hand-rolled span walker, which had a `_ => {}` arm).
///
/// `ignored` filters names by exact match (empty for the template-main caller,
/// which threads no per-expression scope locals). Global-named identifiers
/// (`Date`, `Map`) ARE retained: a `<script setup>` binding may shadow a JS
/// global, so the `is_global` completion / `_ctx`-prefixing filter must NOT gate
/// liveness.
pub fn collect_expression_free_ref_spans<'a>(
    expr: &'a Expression<'a>,
    ignored: &FxHashSet<&[u8]>,
    out: &mut FxHashSet<Span>,
) {
    let mut collector = SetupRefCollector {
        sink: RefSink::Spans {
            ignored,
            spans: std::mem::take(out),
        },
        scopes: vec![FxHashSet::default()],
    };
    collector.visit_expression(expr);
    match collector.sink {
        RefSink::Spans { spans, .. } => *out = spans,
        RefSink::Names { .. } => unreachable!("Spans sink installed above"),
    }
}

/// Collect the free-reference NAMES of every identifier in a v-slot binding
/// pattern's DEFAULT-VALUE expressions (`{ x = expr }`, `[y = expr]`, nested)
/// into `out`.
///
/// The binding-pattern grammar is a closed structural set (identifier / object /
/// array / assignment), matched exhaustively here; each default's `Expression`
/// is handed to the COMPLETE [`collect_expression_free_refs`] walker, so a
/// reference inside a nested callback in a pattern default can never be dropped.
/// NAMES (not spans) are collected so the v-slot LIVENESS path never depends on
/// the partial wrapped→file-relative span shift, which does not recurse into
/// callback bodies. Type annotations are NOT walked here — the caller collects
/// type-space references separately via the equally-complete
/// [`collect_type_free_ref_names`] `Visit`-over-`TSType` walker.
pub fn collect_pattern_default_free_ref_names<'a>(
    pattern: &'a BindingPattern<'a>,
    out: &mut FxHashSet<&'a str>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(_) => {}
        BindingPattern::AssignmentPattern(assign) => {
            out.extend(collect_expression_free_refs(&assign.right));
            collect_pattern_default_free_ref_names(&assign.left, out);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_default_free_ref_names(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_default_free_ref_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_default_free_ref_names(elem, out);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_default_free_ref_names(&rest.argument, out);
            }
        }
    }
}

/// Where a [`SetupRefCollector`] records the free references it finds.
///
/// Both arms share the SINGLE `Visit` walker and the SINGLE scope model, so a
/// liveness path can never accidentally route through a partial walker: there is
/// exactly one complete traversal, and the sink only decides how a discovered
/// reference is recorded.
enum RefSink<'a, 'b> {
    /// Record free identifier NAMES. When `setup_names` is `Some`, only those
    /// names are kept (liveness over a `<script setup>` program); when `None`,
    /// every free identifier root is kept (style `v-bind` expression scan).
    Names {
        setup_names: Option<&'b FxHashSet<&'b str>>,
        refs: FxHashSet<&'a str>,
    },
    /// Record free identifier SPANS, excluding names in `ignored`. Used by the
    /// v-for / v-slot liveness feeders (which carry `Span`s downstream).
    Spans {
        ignored: &'b FxHashSet<&'b [u8]>,
        spans: FxHashSet<Span>,
    },
}

/// Collects free references, tracking lexical scope frames for shadow
/// suppression. The [`RefSink`] decides whether names or spans are recorded;
/// the scope model and the complete `Visit` traversal are shared by both.
struct SetupRefCollector<'a, 'b> {
    sink: RefSink<'a, 'b>,
    /// Active lexical scope frames (bottom = program). Each frame holds the names
    /// declared directly in that scope.
    scopes: Vec<FxHashSet<&'a str>>,
}

impl<'a, 'b> SetupRefCollector<'a, 'b> {
    /// Record a free reference to `name` (occurring at `span`) when it is not
    /// shadowed in any active scope frame and the sink's filter admits it.
    ///
    /// A binding named like a JS global (`const Map = ref(0)`, `const Date = ...`)
    /// shadows that global by construction, so a reference to it is a genuine use
    /// of the local. The `is_global` filter is a completion / `_ctx`-prefixing
    /// heuristic and MUST NOT gate liveness: dropping a global-named reference here
    /// demotes a used binding to a type-only read and false-positives TS6133.
    /// Keywords cannot be binding names, so no keyword guard is needed.
    fn record(&mut self, name: &'a str, span: oxc_span::Span) {
        if self.is_shadowed(name) {
            return;
        }
        match &mut self.sink {
            // `Some(set)` filters to the target binding names (liveness); `None`
            // records every free identifier root (style v-bind scan).
            RefSink::Names { setup_names, refs } => match setup_names {
                Some(set) if !set.contains(name) => {}
                _ => {
                    refs.insert(name);
                }
            },
            // The span sink excludes the directive's own scope locals (v-for /
            // v-slot variables) by name; globals are retained for liveness.
            RefSink::Spans { ignored, spans } => {
                if !ignored.contains(name.as_bytes()) {
                    spans.insert(span.into());
                }
            }
        }
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.scopes.iter().any(|frame| frame.contains(name))
    }

    /// Add a binding pattern's declared identifiers to the current (top) frame.
    fn declare_pattern(&mut self, pattern: &BindingPattern<'a>) {
        let mut names = Vec::new();
        collect_binding_pattern_names(pattern, &mut names);
        if let Some(frame) = self.scopes.last_mut() {
            for n in names {
                frame.insert(n);
            }
        }
    }

    /// Add a single binding identifier to the current (top) frame.
    fn declare_name(&mut self, name: &'a str) {
        if let Some(frame) = self.scopes.last_mut() {
            frame.insert(name);
        }
    }

    /// Pre-scan a statement list for the DIRECT lexical bindings it introduces
    /// (`let`/`const`/`var` declarators, function / class / enum / namespace
    /// declaration ids, import locals) WITHOUT descending into nested blocks or
    /// function bodies. Hoisting `var` declarations from nested blocks are not
    /// modelled (over-counting a use is safe), so only the direct level is scanned.
    fn declare_block_bindings(&mut self, stmts: &[Statement<'a>]) {
        for stmt in stmts {
            self.declare_statement_binding(stmt);
        }
    }

    fn declare_statement_binding(&mut self, stmt: &Statement<'a>) {
        match stmt {
            Statement::VariableDeclaration(v) => {
                for d in &v.declarations {
                    self.declare_pattern(&d.id);
                }
            }
            Statement::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    self.declare_name(id.name.as_str());
                }
            }
            Statement::ClassDeclaration(c) => {
                if let Some(id) = &c.id {
                    self.declare_name(id.name.as_str());
                }
            }
            Statement::TSEnumDeclaration(e) => self.declare_name(e.id.name.as_str()),
            Statement::TSModuleDeclaration(m) => {
                if let TSModuleDeclarationName::Identifier(id) = &m.id {
                    self.declare_name(id.name.as_str());
                }
            }
            Statement::ImportDeclaration(import) => {
                if let Some(specifiers) = &import.specifiers {
                    for spec in specifiers {
                        let local = match spec {
                            ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
                        };
                        self.declare_name(local.name.as_str());
                    }
                }
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    self.declare_declaration_binding(decl);
                }
            }
            _ => {}
        }
    }

    fn declare_declaration_binding(&mut self, decl: &Declaration<'a>) {
        match decl {
            Declaration::VariableDeclaration(v) => {
                for d in &v.declarations {
                    self.declare_pattern(&d.id);
                }
            }
            Declaration::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    self.declare_name(id.name.as_str());
                }
            }
            Declaration::ClassDeclaration(c) => {
                if let Some(id) = &c.id {
                    self.declare_name(id.name.as_str());
                }
            }
            Declaration::TSEnumDeclaration(e) => self.declare_name(e.id.name.as_str()),
            Declaration::TSModuleDeclaration(m) => {
                if let TSModuleDeclarationName::Identifier(id) = &m.id {
                    self.declare_name(id.name.as_str());
                }
            }
            _ => {}
        }
    }

    /// Declare the names a function/arrow params list introduces into the current
    /// (function/arrow) frame.
    fn declare_params(&mut self, params: &FormalParameters<'a>) {
        for param in &params.items {
            self.declare_pattern(&param.pattern);
        }
        if let Some(rest) = &params.rest {
            self.declare_pattern(&rest.rest.argument);
        }
    }
}

impl<'a, 'b> Visit<'a> for SetupRefCollector<'a, 'b> {
    // ── Free identifier sinks ───────────────────────────────────────────

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        // This is the single free-identifier sink. The default `walk::*` routes
        // EVERY value-position identifier here, INCLUDING the leaf of an
        // assignment / update target (`foo = x`, `[foo] = xs`, `{ foo } = obj`,
        // `foo++`) and the object root of a member-target (`foo.x = y`,
        // `foo[key] = y`). All of those are uses that keep the declaration live.
        self.record(it.name.as_str(), it.span);
        walk::walk_identifier_reference(self, it);
    }

    fn visit_ts_type_name(&mut self, it: &TSTypeName<'a>) {
        // A type-position identifier (a `typeof name` query, a type annotation
        // naming a value-space binding) counts as a use — the conservative
        // direction. `walk_ts_type_name` routes the `IdentifierReference` arm
        // through `visit_identifier_reference`, so the qualified-name left is the
        // only arm needing explicit recording here.
        if let TSTypeName::IdentifierReference(id) = it {
            self.record(id.name.as_str(), id.span);
        }
        walk::walk_ts_type_name(self, it);
    }

    // ── Scope boundaries: push a frame with the scope's own bindings ─────

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        let mut frame = FxHashSet::default();
        // A function EXPRESSION's own id is bound in its own body (recursion name);
        // a declaration's id is bound in the enclosing scope (recorded there).
        if !it.is_declaration() {
            if let Some(id) = &it.id {
                frame.insert(id.name.as_str());
            }
        }
        self.scopes.push(frame);
        self.declare_params(&it.params);
        if let Some(body) = &it.body {
            self.declare_block_bindings(&body.statements);
        }
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(FxHashSet::default());
        self.declare_params(&it.params);
        self.declare_block_bindings(&it.body.statements);
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.scopes.push(FxHashSet::default());
        self.declare_block_bindings(&it.body);
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = FxHashSet::default();
        if let Some(ForStatementInit::VariableDeclaration(decl)) = &it.init {
            collect_var_decl_names(decl, &mut frame);
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        let mut frame = FxHashSet::default();
        if let ForStatementLeft::VariableDeclaration(decl) = &it.left {
            collect_var_decl_names(decl, &mut frame);
        }
        self.scopes.push(frame);
        walk::walk_for_in_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        let mut frame = FxHashSet::default();
        if let ForStatementLeft::VariableDeclaration(decl) = &it.left {
            collect_var_decl_names(decl, &mut frame);
        }
        self.scopes.push(frame);
        walk::walk_for_of_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        self.scopes.push(FxHashSet::default());
        if let Some(param) = &it.param {
            self.declare_pattern(&param.pattern);
        }
        self.declare_block_bindings(&it.body.body);
        walk::walk_catch_clause(self, it);
        self.scopes.pop();
    }

    fn visit_switch_statement(&mut self, it: &SwitchStatement<'a>) {
        // A `switch` body is one shared block scope across all cases.
        let mut frame = FxHashSet::default();
        for case in &it.cases {
            for stmt in &case.consequent {
                let mut names = Vec::new();
                collect_statement_binding_names(stmt, &mut names);
                frame.extend(names);
            }
        }
        self.scopes.push(frame);
        walk::walk_switch_statement(self, it);
        self.scopes.pop();
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        // A named class EXPRESSION binds its id only inside its own body.
        if matches!(it.r#type, ClassType::ClassExpression) {
            if let Some(id) = &it.id {
                let mut frame = FxHashSet::default();
                frame.insert(id.name.as_str());
                self.scopes.push(frame);
                walk::walk_class(self, it);
                self.scopes.pop();
                return;
            }
        }
        walk::walk_class(self, it);
    }

    fn visit_static_block(&mut self, it: &StaticBlock<'a>) {
        self.scopes.push(FxHashSet::default());
        self.declare_block_bindings(&it.body);
        walk::walk_static_block(self, it);
        self.scopes.pop();
    }

    fn visit_ts_module_block(&mut self, it: &TSModuleBlock<'a>) {
        self.scopes.push(FxHashSet::default());
        self.declare_block_bindings(&it.body);
        walk::walk_ts_module_block(self, it);
        self.scopes.pop();
    }
}

/// Collect the binding identifier names introduced by a binding pattern.
fn collect_binding_pattern_names<'a>(pattern: &BindingPattern<'a>, out: &mut Vec<&'a str>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => out.push(ident.name.as_str()),
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_pattern_names(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_binding_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_binding_pattern_names(elem, out);
            }
            if let Some(rest) = &arr.rest {
                collect_binding_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_binding_pattern_names(&assign.left, out);
        }
    }
}

fn collect_var_decl_names<'a>(decl: &VariableDeclaration<'a>, out: &mut FxHashSet<&'a str>) {
    for d in &decl.declarations {
        let mut names = Vec::new();
        collect_binding_pattern_names(&d.id, &mut names);
        out.extend(names);
    }
}

fn collect_statement_binding_names<'a>(stmt: &Statement<'a>, out: &mut Vec<&'a str>) {
    match stmt {
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                collect_binding_pattern_names(&d.id, out);
            }
        }
        Statement::FunctionDeclaration(f) => {
            if let Some(id) = &f.id {
                out.push(id.name.as_str());
            }
        }
        Statement::ClassDeclaration(c) => {
            if let Some(id) = &c.id {
                out.push(id.name.as_str());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::*;

    fn refs(source: &str, names: &[&str]) -> FxHashSet<String> {
        let alloc = Allocator::default();
        let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
        assert!(ret.errors.is_empty(), "parse errors: {:?}", ret.errors);
        let set: FxHashSet<&str> = names.iter().copied().collect();
        collect_setup_binding_refs(&ret.program, &set)
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn basic_free_reference_in_computed_initializer() {
        // The top-level decl `doubled` must not suppress the `count` reference in
        // ITS OWN initializer — BindingVisitor would (it ignores top-level decls).
        let r = refs(
            "const count = ref(0);\nconst doubled = computed(() => count.value);",
            &["count", "doubled"],
        );
        assert!(
            r.contains("count"),
            "count used inside doubled's initializer"
        );
        assert!(!r.contains("doubled"), "doubled is referenced nowhere");
    }

    #[test]
    fn member_lhs_update_counts_as_use() {
        // `c.value++` — the previous walker dropped the member-root of an update
        // target, missing this use (the headline false-positive vector).
        let r = refs("const c = ref(0);\nc.value++;", &["c"]);
        assert!(r.contains("c"), "c.value++ is a use of c");
    }

    #[test]
    fn member_lhs_assignment_counts_as_use() {
        let r = refs("const foo = reactive({});\nfoo.x = 1;", &["foo"]);
        assert!(r.contains("foo"), "foo.x = 1 is a use of foo");
    }

    #[test]
    fn computed_member_lhs_counts_as_use() {
        let r = refs(
            "const foo = reactive({});\nconst key = 'a';\nfoo[key] = 1;",
            &["foo", "key"],
        );
        assert!(r.contains("foo"), "foo[key] = 1 is a use of foo");
        assert!(r.contains("key"), "computed key references key");
    }

    #[test]
    fn bare_identifier_assignment_counts_as_use() {
        let r = refs("let foo = 0;\nfoo = 1;", &["foo"]);
        assert!(r.contains("foo"), "foo = 1 is a use of foo");
    }

    #[test]
    fn class_body_reference_counts_as_use() {
        // Class declarations were a `_ => {}` skip in the previous walker.
        let r = refs(
            "const dep = 1;\nclass C { m() { return dep; } }",
            &["dep", "C"],
        );
        assert!(r.contains("dep"), "dep referenced in class method body");
        assert!(!r.contains("C"), "C is referenced nowhere");
    }

    #[test]
    fn static_block_reference_counts_as_use() {
        let r = refs(
            "const dep = 1;\nclass C { static { console.log(dep); } }",
            &["dep"],
        );
        assert!(r.contains("dep"), "dep referenced in class static block");
    }

    #[test]
    fn labeled_statement_reference_counts_as_use() {
        let r = refs("const dep = 1;\nouter: { console.log(dep); }", &["dep"]);
        assert!(r.contains("dep"), "dep referenced inside labeled statement");
    }

    #[test]
    fn destructuring_default_reference_counts_as_use() {
        let r = refs(
            "const fallback = 1;\nfunction f({ x = fallback } = {}) { return x; }",
            &["fallback"],
        );
        assert!(
            r.contains("fallback"),
            "fallback used as a destructuring default"
        );
    }

    #[test]
    fn typeof_query_counts_as_use() {
        let r = refs("const cfg = { a: 1 };\ntype T = typeof cfg;", &["cfg"]);
        assert!(
            r.contains("cfg"),
            "typeof cfg is a (conservative) use of cfg"
        );
    }

    #[test]
    fn param_shadow_suppresses() {
        let r = refs(
            "const count = ref(0);\nfunction foo(count: number) { return count; }",
            &["count"],
        );
        assert!(!r.contains("count"), "count shadowed by function param");
    }

    #[test]
    fn inner_const_shadow_suppresses() {
        let r = refs(
            "const count = ref(0);\nfunction foo() { const count = 42; return count; }",
            &["count"],
        );
        assert!(!r.contains("count"), "count shadowed by inner const");
    }

    #[test]
    fn block_scope_shadow_suppresses() {
        let r = refs(
            "const x = ref(0);\n{ const x = 1; console.log(x); }",
            &["x"],
        );
        assert!(!r.contains("x"), "x shadowed by block-scoped const");
    }

    #[test]
    fn arrow_param_shadow_suppresses() {
        let r = refs(
            "const count = ref(0);\nconst fn2 = (count: number) => count * 2;",
            &["count"],
        );
        assert!(!r.contains("count"), "count shadowed by arrow param");
    }

    #[test]
    fn truly_unused_is_empty() {
        let r = refs(
            "const count = ref(0);\nconst unused = ref(42);",
            &["count", "unused"],
        );
        assert!(r.is_empty(), "neither binding is referenced: {r:?}");
    }

    #[test]
    fn partial_shadow_keeps_free_reference() {
        let r = refs(
            "const count = ref(0);\nconst d = count.value;\nfunction foo(count: number) { return count; }",
            &["count"],
        );
        assert!(
            r.contains("count"),
            "count freely referenced in d's initializer"
        );
    }

    #[test]
    fn global_named_setup_binding_is_recorded() {
        // A `<script setup>` local that SHADOWS a JS global (`const Map = ref(0)`)
        // is by construction a user-declared binding. The previous `is_global`
        // guard in `record_use` silently dropped its references, so a genuinely
        // used `Map`/`Date` got demoted to a type-only read → false TS6133. The
        // collector is already scoped to `setup_names`; the global filter is wrong
        // here. `Map.value++` must record `Map`.
        let r = refs("const Map = ref(0);\nMap.value++;", &["Map"]);
        assert!(
            r.contains("Map"),
            "a setup binding named like a JS global must still be recorded as used"
        );
    }

    #[test]
    fn global_named_setup_binding_free_reference_is_recorded() {
        let r = refs("const Date = ref(0);\nconst d = Date.value;", &["Date"]);
        assert!(
            r.contains("Date"),
            "a free reference to a global-named setup binding must be recorded"
        );
    }

    #[test]
    fn tagged_template_reference_counts_as_use() {
        // Tagged-template tag + interpolation both route through
        // visit_identifier_reference via the default walk.
        let r = refs(
            "const tag = (s: TemplateStringsArray) => s;\nconst dep = 1;\nconst out = tag`x${dep}`;",
            &["tag", "dep", "out"],
        );
        assert!(r.contains("tag"), "tagged-template tag is a use");
        assert!(r.contains("dep"), "tagged-template interpolation is a use");
    }

    #[test]
    fn optional_chaining_reference_counts_as_use() {
        let r = refs("const obj = { a: 1 };\nconst v = obj?.a;", &["obj"]);
        assert!(r.contains("obj"), "optional-chaining root is a use");
    }

    #[test]
    fn decorator_reference_counts_as_use() {
        let r = refs(
            "const deco = (_: any) => {};\nclass C { @deco m() {} }",
            &["deco", "C"],
        );
        assert!(r.contains("deco"), "decorator expression is a use");
    }

    #[test]
    fn jsx_expression_reference_counts_as_use() {
        let alloc = Allocator::default();
        let src = "const dep = 1;\nconst el = <div>{dep}</div>;";
        let ret = Parser::new(&alloc, src, SourceType::tsx()).parse();
        assert!(ret.errors.is_empty(), "parse errors: {:?}", ret.errors);
        let set: FxHashSet<&str> = ["dep", "el"].into_iter().collect();
        let r: FxHashSet<String> = collect_setup_binding_refs(&ret.program, &set)
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        assert!(r.contains("dep"), "JSX expression container is a use");
    }

    #[test]
    fn default_param_reference_counts_as_use() {
        let r = refs(
            "const fallback = 1;\nfunction f(x = fallback) { return x; }",
            &["fallback"],
        );
        assert!(r.contains("fallback"), "default-parameter value is a use");
    }

    #[test]
    fn getter_setter_reference_counts_as_use() {
        let r = refs(
            "const dep = 1;\nconst o = { get v() { return dep; } };",
            &["dep"],
        );
        assert!(r.contains("dep"), "getter body reference is a use");
    }

    #[test]
    fn object_shorthand_reference_counts_as_use() {
        let r = refs("const dep = 1;\nconst o = { dep };", &["dep"]);
        assert!(r.contains("dep"), "object shorthand is a use of dep");
    }

    #[test]
    fn multiple_bindings_discriminate() {
        let r = refs(
            "const a = ref(1);\nconst b = ref(2);\nconst c = ref(3);\nconst sum = computed(() => a.value + c.value);",
            &["a", "b", "c", "sum"],
        );
        assert!(r.contains("a"));
        assert!(!r.contains("b"));
        assert!(r.contains("c"));
        assert!(!r.contains("sum"));
    }
}
