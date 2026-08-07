use syn::visit::Visit;

const COMPLETION_SOURCE: &str = include_str!("../../src/features/completion.rs");

fn pat_is_svelte_snippet(pat: &syn::Pat) -> bool {
    match pat {
        syn::Pat::Struct(pat) => pat
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "SvelteSnippet"),
        syn::Pat::TupleStruct(pat) => pat
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "SvelteSnippet"),
        syn::Pat::Type(pat) => pat_is_svelte_snippet(&pat.pat),
        syn::Pat::Reference(pat) => pat_is_svelte_snippet(&pat.pat),
        _ => false,
    }
}

fn pat_ident(pat: &syn::Pat) -> Option<&syn::Ident> {
    match pat {
        syn::Pat::Ident(pat) => Some(&pat.ident),
        syn::Pat::Type(pat) => pat_ident(&pat.pat),
        syn::Pat::Reference(pat) => pat_ident(&pat.pat),
        _ => None,
    }
}

fn expr_is_binding_field(expr: &syn::Expr, binding: &syn::Ident, field: &str) -> bool {
    let syn::Expr::Reference(reference) = expr else {
        return false;
    };
    let syn::Expr::Field(access) = reference.expr.as_ref() else {
        return false;
    };
    let syn::Member::Named(member) = &access.member else {
        return false;
    };
    let syn::Expr::Path(base) = access.base.as_ref() else {
        return false;
    };
    member == field && base.path.is_ident(binding)
}

#[derive(Default)]
struct ContinueFinder(bool);

impl<'ast> Visit<'ast> for ContinueFinder {
    fn visit_expr_continue(&mut self, _expr: &'ast syn::ExprContinue) {
        self.0 = true;
    }
}

fn stmt_is_role_gate(stmt: &syn::Stmt, binding: &syn::Ident) -> bool {
    let syn::Stmt::Local(local) = stmt else {
        return false;
    };
    if !pat_is_svelte_snippet(&local.pat) {
        return false;
    }
    let Some(init) = &local.init else {
        return false;
    };
    if !expr_is_binding_field(&init.expr, binding, "callable_role") {
        return false;
    }
    let Some((_, diverge)) = &init.diverge else {
        return false;
    };
    let mut finder = ContinueFinder::default();
    finder.visit_expr(diverge);
    finder.0
}

fn call_is_items_push(call: &syn::ExprCall) -> bool {
    let syn::Expr::MethodCall(method) = call.func.as_ref() else {
        return false;
    };
    method.method == "push"
        && matches!(
            method.receiver.as_ref(),
            syn::Expr::Path(path) if path.path.is_ident("items")
        )
}

#[derive(Default)]
struct StatementReads {
    emissions: usize,
    forbidden_outside_emission: Vec<String>,
}

impl<'ast> Visit<'ast> for StatementReads {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if call_is_items_push(call) {
            self.emissions += 1;
            return;
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "push"
            && matches!(
                call.receiver.as_ref(),
                syn::Expr::Path(path) if path.path.is_ident("items")
            )
        {
            self.emissions += 1;
            return;
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &field.member {
            if name == "type_annotation" || name == "terminal_display" {
                self.forbidden_outside_emission.push(name.to_string());
            }
        }
        syn::visit::visit_expr_field(self, field);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "is_snippet_type_annotation")
        {
            self.forbidden_outside_emission
                .push("is_snippet_type_annotation".to_string());
        }
        syn::visit::visit_expr_path(self, path);
    }
}

fn expr_reads_prop_definitions(expr: &syn::Expr) -> bool {
    #[derive(Default)]
    struct Finder(bool);

    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
            if matches!(&field.member, syn::Member::Named(name) if name == "prop_definitions") {
                self.0 = true;
            }
            syn::visit::visit_expr_field(self, field);
        }
    }

    let mut finder = Finder::default();
    finder.visit_expr(expr);
    finder.0
}

#[derive(Debug, Default)]
struct PropLoopGuard {
    has_role_gate: bool,
    emissions_after_gate: usize,
    emissions_before_gate: usize,
    forbidden_outside_emission: Vec<String>,
}

fn inspect_prop_loop(expr: &syn::ExprForLoop) -> PropLoopGuard {
    let mut result = PropLoopGuard::default();
    let Some(binding) = pat_ident(&expr.pat) else {
        return result;
    };

    let mut iterator_reads = StatementReads::default();
    iterator_reads.visit_expr(&expr.expr);
    result
        .forbidden_outside_emission
        .extend(iterator_reads.forbidden_outside_emission);

    for stmt in &expr.body.stmts {
        if stmt_is_role_gate(stmt, binding) {
            result.has_role_gate = true;
            continue;
        }
        let mut reads = StatementReads::default();
        reads.visit_stmt(stmt);
        if result.has_role_gate {
            result.emissions_after_gate += reads.emissions;
        } else {
            result.emissions_before_gate += reads.emissions;
        }
        result
            .forbidden_outside_emission
            .extend(reads.forbidden_outside_emission);
    }
    result
}

#[derive(Default)]
struct FunctionGuard {
    prop_loops: Vec<PropLoopGuard>,
}

impl<'ast> Visit<'ast> for FunctionGuard {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        if expr_reads_prop_definitions(&expr.expr) {
            self.prop_loops.push(inspect_prop_loop(expr));
            return;
        }
        syn::visit::visit_expr_for_loop(self, expr);
    }
}

fn inspect_function(source: &str, expected: &str) -> FunctionGuard {
    let file = syn::parse_file(source).expect("Rust source parses");
    let function = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == expected => Some(function),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {expected}"));
    let mut guard = FunctionGuard::default();
    guard.visit_block(&function.block);
    guard
}

fn assert_typed_role_authority(source: &str, expected: &str) {
    let guard = inspect_function(source, expected);
    assert!(
        !guard.prop_loops.is_empty(),
        "{expected} must emit prop completions from a prop_definitions loop"
    );
    for prop_loop in guard.prop_loops {
        assert!(
            prop_loop.has_role_gate,
            "{expected} must gate each prop-emission loop on that prop's SvelteSnippet role"
        );
        assert_eq!(
            prop_loop.emissions_before_gate, 0,
            "{expected} emitted a prop before its typed role gate"
        );
        assert!(
            prop_loop.emissions_after_gate >= 1,
            "{expected}'s typed role gate must dominate a prop emission"
        );
        assert!(
            prop_loop.forbidden_outside_emission.is_empty(),
            "{expected} uses display-derived eligibility control outside the emitted item's detail: {:?}",
            prop_loop.forbidden_outside_emission
        );
    }
}

#[test]
fn svelte_prop_completions_are_gated_only_by_typed_callable_role() {
    let file = syn::parse_file(COMPLETION_SOURCE).expect("completion.rs parses");
    assert!(
        !file.items.iter().any(
            |item| matches!(item, syn::Item::Fn(function) if function.sig.ident == "is_snippet_type_annotation")
        ),
        "legacy annotation classifier must be deleted"
    );

    for expected in [
        "svelte_snippet_slot_completions",
        "svelte_render_callee_completions",
    ] {
        assert_typed_role_authority(COMPLETION_SOURCE, expected);
    }
}

/// Mutation recipe: weaken `assert_typed_role_authority` to count any local
/// `SvelteSnippet` pattern or inspect only `if` conditions; the dead-gate and
/// match/filter/helper cases below must then fail to be rejected.
#[test]
fn guard_rejects_dead_role_checks_and_display_control_expression_forms() {
    let dead_gate = r#"
        fn target(template: Template, mut items: Vec<Item>) {
            let SvelteSnippet { .. } = unrelated.callable_role else { return };
            for prop_def in &template.prop_definitions {
                if prop_def.type_annotation.is_some() {
                    items.push(Item::default());
                }
            }
        }
    "#;
    let dead = inspect_function(dead_gate, "target");
    assert!(
        dead.prop_loops
            .iter()
            .all(|loop_guard| !loop_guard.has_role_gate),
        "a dead typed destructure outside the emission loop is not authority"
    );

    for control in [
        "if prop_def.type_annotation.is_some() { items.push(Item::default()); }",
        "match &prop_def.terminal_display { Some(_) => items.push(Item::default()), None => {} }",
        "if is_snippet_type_annotation(&prop_def.type_annotation) { items.push(Item::default()); }",
    ] {
        let source = format!(
            "fn target(template: Template, mut items: Vec<Item>) {{\
             for prop_def in &template.prop_definitions {{\
             let SvelteSnippet {{ .. }} = &prop_def.callable_role else {{ continue; }};\
             {control}\
             }}\
             }}"
        );
        let guard = inspect_function(&source, "target");
        assert!(
            guard
                .prop_loops
                .iter()
                .all(|loop_guard| !loop_guard.forbidden_outside_emission.is_empty()),
            "display control form escaped the guard: {control}"
        );
    }

    let filtered = r#"
        fn target(template: Template, mut items: Vec<Item>) {
            for prop_def in template.prop_definitions.iter()
                .filter(|prop_def| prop_def.type_annotation.is_some())
            {
                let SvelteSnippet { .. } = &prop_def.callable_role else { continue; };
                items.push(Item::default());
            }
        }
    "#;
    let guard = inspect_function(filtered, "target");
    assert!(
        guard
            .prop_loops
            .iter()
            .all(|loop_guard| !loop_guard.forbidden_outside_emission.is_empty()),
        "iterator/filter control must be inspected too"
    );
}
