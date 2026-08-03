use syn::visit::Visit;

const COMPLETION_SOURCE: &str = include_str!("../src/features/completion.rs");

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

#[derive(Default)]
struct FunctionGuard {
    typed_role_guards: usize,
    forbidden_condition_reads: Vec<String>,
}

impl<'ast> Visit<'ast> for FunctionGuard {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if pat_is_svelte_snippet(&local.pat) {
            self.typed_role_guards += 1;
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_if(&mut self, expr: &'ast syn::ExprIf) {
        struct ConditionReads(Vec<String>);
        impl<'ast> Visit<'ast> for ConditionReads {
            fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
                if let syn::Member::Named(name) = &field.member {
                    if name == "type_annotation" || name == "terminal_display" {
                        self.0.push(name.to_string());
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
                    self.0.push("is_snippet_type_annotation".to_string());
                }
                syn::visit::visit_expr_path(self, path);
            }
        }

        let mut reads = ConditionReads(Vec::new());
        reads.visit_expr(&expr.cond);
        self.forbidden_condition_reads.extend(reads.0);
        syn::visit::visit_expr_if(self, expr);
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
        assert!(
            guard.typed_role_guards >= 1,
            "{expected} must destructure PropCallableRole::SvelteSnippet before emitting a prop"
        );
        assert!(
            guard.forbidden_condition_reads.is_empty(),
            "{expected} must not gate on display text: {:?}",
            guard.forbidden_condition_reads
        );
    }
}
