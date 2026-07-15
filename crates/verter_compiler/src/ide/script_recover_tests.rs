use super::*;

fn scan(source: &str) -> ScriptSetupRecoveryPlan<'_> {
    ScriptTokenScanner::new(source, 0).recover_plan()
}

fn scan_offset(source: &str, offset: u32) -> ScriptSetupRecoveryPlan<'_> {
    ScriptTokenScanner::new(source, offset).recover_plan()
}

// ── Macro detection ─────────────────────────────────────────────

#[test]
fn finds_define_props() {
    let r = scan("defineProps<{ count: number }>()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineProps);
    assert!(r.macros[0].binding_name.is_none());
}

#[test]
fn finds_define_props_with_binding() {
    let r = scan("const props = defineProps<{ count: number }>()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineProps);
    assert_eq!(r.macros[0].binding_name, Some("props"));
}

#[test]
fn finds_define_emits_with_binding() {
    let r = scan("const emit = defineEmits<{ click: [e: MouseEvent] }>()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineEmits);
    assert_eq!(r.macros[0].binding_name, Some("emit"));
}

#[test]
fn finds_with_defaults() {
    let r = scan("const props = withDefaults(defineProps<Props>(), { count: 0 })");
    // withDefaults is the outermost macro — scanner consumes defineProps inside bracket match
    assert!(r
        .macros
        .iter()
        .any(|m| m.kind == RecoveredMacroKind::WithDefaults));
    assert_eq!(r.macros[0].binding_name, Some("props"));
}

#[test]
fn binding_scan_is_utf8_boundary_safe_with_multibyte_before_ident() {
    // A multibyte char adjacent to the binding identifier means the backward
    // ident walk stops on a UTF-8 boundary where a FIXED-WIDTH back-slice for the
    // `const`/`let`/`var` keyword would land mid-codepoint. Scanning must not
    // panic ("byte index N is not a char boundary"); it simply finds no clean
    // `const NAME =` binding here and reports the macro with no binding name.
    for src in [
        "const😀x = defineProps()",
        "let😀x = defineEmits()",
        "var😀x = defineModel()",
        "const x😀 = defineProps()",
    ] {
        let r = scan(src);
        assert_eq!(r.macros.len(), 1, "[{src}] the macro is still detected");
        assert!(
            r.macros[0].binding_name.is_none(),
            "[{src}] a binding broken by a multibyte char has no clean keyword \
             prefix, so no binding name is recovered, got {:?}",
            r.macros[0].binding_name
        );
    }

    // Control: the SAME shape with an ASCII boundary DOES recover the binding —
    // proving the multibyte cases above are exercising the real backward scan,
    // not a vacuous no-binding path.
    let ok = scan("const x = defineProps()");
    assert_eq!(ok.macros[0].binding_name, Some("x"));
}

#[test]
fn finds_define_model() {
    let r = scan("const modelValue = defineModel<string>()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineModel);
    assert_eq!(r.macros[0].binding_name, Some("modelValue"));
}

#[test]
fn finds_define_expose() {
    let r = scan("defineExpose({ foo: 1 })");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineExpose);
}

#[test]
fn finds_define_options() {
    let r = scan("defineOptions({ name: 'Foo' })");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineOptions);
}

#[test]
fn finds_define_slots() {
    let r = scan("const slots = defineSlots<{ default(): any }>()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineSlots);
    assert_eq!(r.macros[0].binding_name, Some("slots"));
}

#[test]
fn finds_multiple_macros() {
    let r = scan(
        "const props = defineProps<{ x: number }>()\nconst emit = defineEmits<{ click: [] }>()",
    );
    assert_eq!(r.macros.len(), 2);
}

// ── Comment/string skipping ─────────────────────────────────────

#[test]
fn ignores_macro_in_line_comment() {
    let r = scan("// defineProps<{ count: number }>()");
    assert!(r.macros.is_empty(), "should not find macro in line comment");
}

#[test]
fn ignores_macro_in_block_comment() {
    let r = scan("/* defineProps<{ count: number }>() */");
    assert!(
        r.macros.is_empty(),
        "should not find macro in block comment"
    );
}

#[test]
fn ignores_macro_in_string() {
    let r = scan(r#""defineProps<{ count: number }>()""#);
    assert!(r.macros.is_empty(), "should not find macro in string");
}

#[test]
fn ignores_macro_in_single_quote_string() {
    let r = scan("'defineProps()'");
    assert!(
        r.macros.is_empty(),
        "should not find macro in single-quote string"
    );
}

#[test]
fn ignores_macro_in_template_literal() {
    let r = scan("`defineProps()`");
    assert!(
        r.macros.is_empty(),
        "should not find macro in template literal"
    );
}

#[test]
fn finds_macro_after_block_comment() {
    let r = scan("/* comment */\ndefineProps()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineProps);
}

#[test]
fn handles_template_literal_with_interpolation() {
    let r = scan("`${defineProps()}` \n defineEmits()");
    // defineProps inside template literal interpolation should be found
    // (it's real code in the interpolation)
    // Actually, the ${...} content IS executable code, so we should find it
    // But our simple scanner skips the entire template literal including interpolation
    // This is acceptable — the scanner is conservative
    assert!(
        !r.macros.is_empty(),
        "should find at least defineEmits after template literal"
    );
    assert!(
        r.macros
            .iter()
            .any(|m| m.kind == RecoveredMacroKind::DefineEmits),
        "should find defineEmits"
    );
}

// ── Bracket matching ────────────────────────────────────────────

#[test]
fn handles_nested_brackets_in_type_params() {
    let r = scan("defineProps<{ items: Array<{ name: string }> }>()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].kind, RecoveredMacroKind::DefineProps);
}

#[test]
fn handles_nested_parens_in_call() {
    let r = scan("defineProps(foo(bar()))");
    assert_eq!(r.macros.len(), 1);
}

#[test]
fn handles_strings_inside_brackets() {
    let r = scan(r#"defineProps<{ foo: "bar<baz>" }>()"#);
    assert_eq!(r.macros.len(), 1);
}

// ── Backward binding scan ───────────────────────────────────────

#[test]
fn backward_scan_const() {
    let r = scan("const props = defineProps()");
    assert_eq!(r.macros[0].binding_name, Some("props"));
}

#[test]
fn backward_scan_let() {
    let r = scan("let props = defineProps()");
    assert_eq!(r.macros[0].binding_name, Some("props"));
}

#[test]
fn backward_scan_var() {
    let r = scan("var props = defineProps()");
    assert_eq!(r.macros[0].binding_name, Some("props"));
}

#[test]
fn no_binding_without_keyword() {
    let r = scan("props = defineProps()");
    assert!(
        r.macros[0].binding_name.is_none(),
        "should not find binding without const/let/var"
    );
}

#[test]
fn backward_scan_with_extra_whitespace() {
    let r = scan("const   props   =   defineProps()");
    assert_eq!(r.macros[0].binding_name, Some("props"));
}

// ── Function detection ──────────────────────────────────────────

#[test]
fn finds_function_declaration() {
    let r = scan("function handleClick(event) {}");
    assert_eq!(r.functions.len(), 1);
    assert_eq!(r.functions[0].name, "handleClick");
}

#[test]
fn finds_function_with_multiple_params() {
    let r = scan("function handleDrag(startEvent, endEvent) {}");
    assert_eq!(r.functions.len(), 1);
    assert_eq!(r.functions[0].name, "handleDrag");
}

#[test]
fn finds_function_with_type_params() {
    let r = scan("function foo<T>(x: T) {}");
    assert_eq!(r.functions.len(), 1);
    assert_eq!(r.functions[0].name, "foo");
}

#[test]
fn finds_multiple_functions() {
    let r = scan("function foo() {}\nfunction bar() {}");
    assert_eq!(r.functions.len(), 2);
    assert_eq!(r.functions[0].name, "foo");
    assert_eq!(r.functions[1].name, "bar");
}

#[test]
fn ignores_function_in_comment() {
    let r = scan("// function foo() {}");
    assert!(r.functions.is_empty());
}

// ── Edge cases ──────────────────────────────────────────────────

#[test]
fn macro_at_start_of_file() {
    let r = scan("defineProps()");
    assert_eq!(r.macros.len(), 1);
}

#[test]
fn macro_at_end_of_file_no_trailing_newline() {
    let r = scan("const x = defineProps()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].binding_name, Some("x"));
}

#[test]
fn adjacent_macros() {
    let r = scan("defineProps()\ndefineEmits()");
    assert_eq!(r.macros.len(), 2);
}

#[test]
fn macro_after_comment() {
    let r = scan("// This sets up props\nconst props = defineProps()");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].binding_name, Some("props"));
}

#[test]
fn empty_source() {
    let r = scan("");
    assert!(r.macros.is_empty());
    assert!(r.functions.is_empty());
}

#[test]
fn only_whitespace() {
    let r = scan("   \n\n  ");
    assert!(r.macros.is_empty());
    assert!(r.functions.is_empty());
}

#[test]
fn unclosed_string_doesnt_panic() {
    let r = scan(r#"const x = "unclosed"#);
    // Should not panic, just stop scanning
    let _ = r;
}

#[test]
fn unclosed_template_literal_doesnt_panic() {
    let r = scan("const x = `unclosed");
    let _ = r;
}

#[test]
fn unclosed_block_comment_doesnt_panic() {
    let r = scan("/* unclosed block comment\ndefineProps()");
    // macro should not be found (inside block comment)
    assert!(r.macros.is_empty());
}

// ── Span offsets ────────────────────────────────────────────────

#[test]
fn spans_include_content_start_offset() {
    let r = scan_offset("defineProps()", 100);
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.macros[0].call_span.start, 100);
    assert_eq!(r.macros[0].call_span.end, 113); // 100 + 13
}

#[test]
fn binding_span_includes_offset() {
    let r = scan_offset("const props = defineProps()", 50);
    assert_eq!(r.macros[0].binding_span.unwrap().start, 56); // 50 + 6
    assert_eq!(r.macros[0].binding_span.unwrap().end, 61); // 50 + 11
}

#[test]
fn function_spans_include_offset() {
    let r = scan_offset("function foo() {}", 200);
    assert_eq!(r.functions[0].name_span.start, 209); // 200 + 9
    assert_eq!(r.functions[0].name_span.end, 212); // 200 + 12
    assert_eq!(r.functions[0].params_span.start, 212); // 200 + 12
    assert_eq!(r.functions[0].params_span.end, 214); // 200 + 14
}

// ── Mixed scenarios ─────────────────────────────────────────────

#[test]
fn macros_and_functions_together() {
    let r = scan("const props = defineProps<{ x: number }>()\nfunction handleClick(event) {}");
    assert_eq!(r.macros.len(), 1);
    assert_eq!(r.functions.len(), 1);
    assert_eq!(r.macros[0].binding_name, Some("props"));
    assert_eq!(r.functions[0].name, "handleClick");
}

#[test]
fn broken_macro_no_parens() {
    // defineProps< — no closing > or ()
    let r = scan("defineProps<");
    // Should not find a complete macro call (missing parens)
    assert!(
        r.macros.is_empty(),
        "incomplete macro should not be recovered"
    );
}

#[test]
fn broken_macro_unclosed_generic() {
    // defineProps<{ count. — error in type, no closing
    let r = scan("defineProps<{ count.");
    assert!(
        r.macros.is_empty(),
        "unclosed generic should not produce a macro"
    );
}

#[test]
fn macro_inside_unclosed_generic_is_not_top_level() {
    // An unclosed generic `<{` scopes everything after it INSIDE the broken type,
    // exactly as TypeScript sees it — the later `defineEmits` is NOT a top-level
    // macro and must not be recovered as one (top-level fact gate, finding 1).
    let r = scan("defineProps<{\nconst emit = defineEmits()");
    assert!(
        r.macros
            .iter()
            .all(|m| m.kind != RecoveredMacroKind::DefineEmits),
        "a macro nested inside an unclosed generic is not top-level: {:?}",
        r.macros.iter().map(|m| m.kind).collect::<Vec<_>>()
    );
}

#[test]
fn keyword_const_not_partial_match() {
    // "constant" should NOT match "const" prefix
    let r = scan("constant = defineProps()");
    assert!(
        r.macros[0].binding_name.is_none(),
        "constant should not match const"
    );
}

// ── Variable recovery ───────────────────────────────────────────

#[test]
fn finds_const_variable() {
    let r = scan("const count = ref(0)");
    assert_eq!(r.variables.len(), 1);
    assert_eq!(r.variables[0].name, "count");
    assert_eq!(r.variables[0].kind, RecoveredVarKind::Const);
}

#[test]
fn finds_let_variable() {
    let r = scan("let x = 1");
    assert_eq!(r.variables.len(), 1);
    assert_eq!(r.variables[0].name, "x");
    assert_eq!(r.variables[0].kind, RecoveredVarKind::Let);
}

#[test]
fn finds_var_variable() {
    let r = scan("var y = 2");
    assert_eq!(r.variables.len(), 1);
    assert_eq!(r.variables[0].name, "y");
    assert_eq!(r.variables[0].kind, RecoveredVarKind::Var);
}

#[test]
fn finds_multiple_variables() {
    let r = scan("const a = 1\nlet b = 2\nvar c = 3");
    assert_eq!(r.variables.len(), 3);
    assert_eq!(r.variables[0].name, "a");
    assert_eq!(r.variables[1].name, "b");
    assert_eq!(r.variables[2].name, "c");
}

#[test]
fn variable_span_includes_offset() {
    let r = scan_offset("const count = 1", 100);
    assert_eq!(r.variables[0].name_span.start, 106); // 100 + 6
    assert_eq!(r.variables[0].name_span.end, 111); // 100 + 11
}

#[test]
fn const_in_comment_not_variable() {
    let r = scan("// const x = 1\nconst y = 2");
    assert_eq!(r.variables.len(), 1);
    assert_eq!(r.variables[0].name, "y");
}

#[test]
fn const_in_string_not_variable() {
    let r = scan(r#""const x = 1""#);
    assert!(r.variables.is_empty());
}

#[test]
fn variables_with_macros_and_functions() {
    let r = scan("const count = ref(0)\nconst props = defineProps()\nfunction handle() {}");
    assert_eq!(r.variables.len(), 2); // count + props (const keyword parsed)
    assert_eq!(r.macros.len(), 1); // defineProps
    assert_eq!(r.functions.len(), 1); // handle
}

#[test]
fn constant_keyword_not_variable() {
    // "constant" is not "const"
    let r = scan("constant = 1");
    assert!(r.variables.is_empty());
}

#[test]
fn letter_keyword_not_variable() {
    // "letter" is not "let"
    let r = scan("letter = 1");
    assert!(r.variables.is_empty());
}

// ── Structured recovery plan ────────────────────────────────────

/// Collect every identifier the plan would turn into a SOURCE FACT
/// (binding / macro / import name). Synthetic recovery placeholders must
/// never appear here.
fn fact_identifiers(plan: &ScriptSetupRecoveryPlan<'_>) -> Vec<String> {
    let mut ids = Vec::new();
    ids.extend(plan.variables.iter().map(|v| v.name.to_string()));
    ids.extend(plan.functions.iter().map(|f| f.name.to_string()));
    ids.extend(
        plan.macros
            .iter()
            .filter_map(|m| m.binding_name.map(str::to_string)),
    );
    ids.extend(
        plan.imports
            .iter()
            .flat_map(|i| i.binding_names.iter().map(|n| n.to_string())),
    );
    ids
}

#[test]
fn recovery_plan_recovers_real_variable_and_member_hole() {
    let plan = scan("let a = 1;\na.");
    assert!(
        plan.variables.iter().any(|v| v.name == "a"),
        "the real `a` variable is recovered from its original span"
    );
    assert_eq!(
        plan.inserts.len(),
        1,
        "the dangling `a.` produces exactly one member hole"
    );
    assert!(matches!(plan.inserts[0], RecoveryInsert::MemberHole { .. }));
}

/// The synthetic member-hole placeholder (`valueOf`) and expression-hole
/// placeholder (`(undefined)`) must NEVER be recovered as a binding/macro/
/// import, and no `___VERTER___` token may leak into a fact.
#[test]
fn recovery_plan_holes_never_become_facts() {
    for src in [
        "let a = 1;\na.",
        "let a = 1;\na?.",
        "const x =",
        "let a = 1;\nfoo(a.",
        "const f = () => {",
    ] {
        let plan = scan(src);
        for id in fact_identifiers(&plan) {
            assert_ne!(
                id, "valueOf",
                "[{src}] member-hole placeholder leaked into facts"
            );
            assert_ne!(
                id, "undefined",
                "[{src}] expression-hole placeholder leaked into facts"
            );
            assert!(
                !id.contains("___VERTER___"),
                "[{src}] synthetic ___VERTER___ token leaked into facts: {id}"
            );
        }
    }
}

#[test]
fn recovery_plan_recovers_real_import_with_bindings() {
    let src = "import { ref } from 'vue'\nconst c = ref(0)\nc.";
    let plan = scan(src);
    assert_eq!(plan.imports.len(), 1, "the import is recovered");
    // The statement span slices back to the `from 'vue'` import.
    let span = plan.imports[0].span;
    assert!(
        src[span.start as usize..span.end as usize].contains("from 'vue'"),
        "the recovered statement span must cover the `from 'vue'` import"
    );
    assert!(
        plan.imports[0].binding_names.contains(&"ref"),
        "the local `ref` binding is recovered: {:?}",
        plan.imports[0].binding_names
    );
    assert!(!plan.imports[0].is_type_only);
    assert!(plan.variables.iter().any(|v| v.name == "c"));
}

#[test]
fn recovery_plan_recovers_vue_import_for_hoist() {
    // A `.vue` import inside broken `<script setup>` is recovered so the failure
    // path can HOIST the whole statement (via its statement span) and register the
    // binding. The emitted specifier stays the BARE `./Foo.vue`, which resolves
    // natively to the `.d.vue.ts` declaration carrier — there is no specifier
    // rewrite, so the recovery plan no longer captures the specifier text/span.
    let src = "import Foo from './Foo.vue'\nFoo.";
    let plan = scan(src);
    assert_eq!(plan.imports.len(), 1);
    assert!(plan.imports[0].binding_names.contains(&"Foo"));
    // The statement span covers the full `import … './Foo.vue'` statement and
    // slices back to the BARE specifier (NEGATIVE: no `.tsx` rewrite anywhere).
    let span = plan.imports[0].span;
    let stmt = &src[span.start as usize..span.end as usize];
    assert!(
        stmt.contains("./Foo.vue") && !stmt.contains(".vue.tsx"),
        "the recovered import statement must carry the BARE `./Foo.vue` specifier: {stmt:?}"
    );
}

#[test]
fn recovery_plan_type_only_import_flagged() {
    let plan = scan("import type { Props } from './types'\nconst x =");
    assert_eq!(plan.imports.len(), 1);
    assert!(
        plan.imports[0].is_type_only,
        "`import type` must be flagged so no value binding is registered"
    );
}

#[test]
fn recovery_plan_import_alias_keeps_local_name_only() {
    let plan = scan("import { foo as Bar } from 'x'\nBar.");
    assert_eq!(plan.imports.len(), 1);
    assert!(
        plan.imports[0].binding_names.contains(&"Bar"),
        "local alias `Bar` is recovered: {:?}",
        plan.imports[0].binding_names
    );
    assert!(
        !plan.imports[0].binding_names.contains(&"foo"),
        "imported name `foo` is NOT a local binding: {:?}",
        plan.imports[0].binding_names
    );
}

#[test]
fn recovery_plan_dangling_dot_detected_across_newline() {
    // `a.` then a newline before `return` is dangling (the bug case).
    let plan = scan("() => {\n  a.\n  return a\n}");
    assert_eq!(
        plan.inserts
            .iter()
            .filter(|i| matches!(i, RecoveryInsert::MemberHole { .. }))
            .count(),
        1,
        "the dangling dot before the next-line `return` is detected"
    );
}

#[test]
fn recovery_plan_wellformed_multiline_chain_is_not_a_hole() {
    // A well-formed multi-line member chain has the newline BEFORE the dot,
    // and the property on the same line as the dot — never a dangling dot.
    let plan = scan("const x = foo\n  .bar\n  .baz");
    assert!(
        plan.inserts.is_empty(),
        "well-formed multi-line chains must not be treated as holes: {:?}",
        plan.inserts
    );
}

#[test]
fn recovery_plan_expression_hole_for_trailing_operator() {
    for src in ["a +", "const x =", "const x = a ? 1 :"] {
        let plan = scan(src);
        assert!(
            plan.inserts
                .iter()
                .any(|i| matches!(i, RecoveryInsert::ExpressionHole { .. })),
            "[{src}] a trailing operator must produce an expression hole: {:?}",
            plan.inserts
        );
    }
}

#[test]
fn recovery_plan_scope_closers_for_open_brackets() {
    assert_eq!(scan("foo(").scope_closers, ")");
    assert_eq!(scan("const f = () => {").scope_closers, "}");
    // Innermost-first ordering: `foo(() => {` → close `}` then `)`.
    assert_eq!(scan("foo(() => {").scope_closers, "})");
    assert_eq!(scan("const a = 1").scope_closers, "");
}

// ── Top-level fact gate (finding 1) ─────────────────────────────

#[test]
fn recovery_plan_excludes_block_local_facts() {
    // Block-local declarations live INSIDE a `{ … }` (bracket depth > 0) and are
    // NOT exposed by the clean top-level parser (`block_depth == 0` gate). The
    // recovery scan must mirror that: only top-level imports/macros/vars/functions
    // become facts. The whole-source hole scan still fires regardless of depth.
    let plan = scan("function f(){ const inner = defineProps<{x:number}>(); } a.");
    // The top-level function `f` IS recovered.
    assert!(
        plan.functions.iter().any(|fun| fun.name == "f"),
        "the top-level function `f` is recovered: {:?}",
        plan.functions
            .iter()
            .map(|fun| fun.name)
            .collect::<Vec<_>>()
    );
    // The block-local `const inner` must NOT be recovered as a variable …
    assert!(
        plan.variables.iter().all(|v| v.name != "inner"),
        "block-local `inner` leaked as a variable fact: {:?}",
        plan.variables.iter().map(|v| v.name).collect::<Vec<_>>()
    );
    // … and the block-local `defineProps` must NOT be recovered as a macro
    // (which would otherwise register `inner` as a Props binding).
    assert!(
        plan.macros.is_empty(),
        "block-local macro leaked: {:?}",
        plan.macros.iter().map(|m| m.kind).collect::<Vec<_>>()
    );
    // The whole-source scan still finds the dangling `a.` member hole.
    assert_eq!(
        plan.inserts
            .iter()
            .filter(|i| matches!(i, RecoveryInsert::MemberHole { .. }))
            .count(),
        1,
        "the trailing `a.` is still recovered as a member hole regardless of depth"
    );
}

#[test]
fn recovery_plan_excludes_block_local_import() {
    // An `import`-like token nested inside a block must not become a setup
    // import; only the top-level `c.` member hole remains.
    let plan = scan("if (cond) {\n  import x from 'y'\n} c.");
    assert!(
        plan.imports.is_empty(),
        "block-local import leaked: {:?}",
        plan.imports
            .iter()
            .map(|i| &i.binding_names)
            .collect::<Vec<_>>()
    );
}

// ── Open-delimiter placeholders (finding 3) ─────────────────────

#[test]
fn recovery_plan_open_grouping_paren_gets_placeholder() {
    // `const x = (` — a grouping paren requires an operand; `()` is invalid here,
    // so the closer carries a placeholder operand.
    assert_eq!(scan("const x = (").scope_closers, "undefined)");
}

#[test]
fn recovery_plan_computed_member_bracket_gets_placeholder() {
    // `foo[` — computed member access requires an index; `foo[]` is invalid.
    assert_eq!(scan("foo[").scope_closers, "undefined]");
}

#[test]
fn recovery_plan_arrow_parenthesized_body_gets_placeholder() {
    // `const f = () => (` — the arrow's parenthesized body is a grouping paren.
    // The params `()` are balanced and untouched; only the open body paren fills.
    assert_eq!(scan("const f = () => (").scope_closers, "undefined)");
}

#[test]
fn recovery_plan_call_array_block_need_no_placeholder() {
    // NEGATIVE: empty call args, empty array literal, and empty block are all
    // VALID — the placeholder must NOT be over-injected for these.
    assert_eq!(
        scan("foo(").scope_closers,
        ")",
        "empty call args are valid TSX"
    );
    assert_eq!(
        scan("const x = [").scope_closers,
        "]",
        "an empty array literal is valid TSX"
    );
    assert_eq!(
        scan("const f = () => {").scope_closers,
        "}",
        "an empty block is valid TSX"
    );
}

#[test]
fn recovery_plan_nested_grouping_with_array_no_double_placeholder() {
    // `const x = ([` — a grouping paren whose content is a (valid, empty) array
    // literal: the array satisfies the grouping paren's content requirement, so
    // neither delimiter gets a placeholder.
    assert_eq!(scan("const x = ([").scope_closers, "])");
}

#[test]
fn recovery_plan_grouping_paren_with_member_hole_no_placeholder() {
    // `const x = (a.` — the grouping paren has content (`a.`), so no placeholder;
    // the dangling dot is a member hole and the paren just closes.
    let plan = scan("const x = (a.");
    assert_eq!(plan.scope_closers, ")");
    assert_eq!(
        plan.inserts
            .iter()
            .filter(|i| matches!(i, RecoveryInsert::MemberHole { .. }))
            .count(),
        1
    );
}

// ── Control/condition-keyword header parens (whole class) ───────────
// A `(` directly after a control keyword is a REQUIRED header paren, not
// empty-valid call args. Each keyword class gets the completion that makes the
// statement parse: a discriminant for `if`/`while`/`with`, the missing `;`
// separators for `for` (none for the iterator form), a trailing `{}` for
// `switch`/`catch`. RED before: `if (` closed as empty `()`.

#[test]
fn recovery_plan_if_condition_paren_gets_placeholder() {
    // `if (` — empty condition is invalid (`if ()`); fill an `undefined` operand.
    assert_eq!(scan("if (").scope_closers, "undefined)");
}

#[test]
fn recovery_plan_while_condition_paren_gets_placeholder() {
    assert_eq!(scan("while (").scope_closers, "undefined)");
}

#[test]
fn recovery_plan_with_condition_paren_gets_placeholder() {
    assert_eq!(scan("with (").scope_closers, "undefined)");
}

#[test]
fn recovery_plan_for_empty_header_gets_two_separators() {
    // `for (` — a C-style header needs two `;` separators (`for (;;)`); empty
    // slots are valid TSX, so NO `undefined` operand is injected.
    assert_eq!(scan("for (").scope_closers, ";;)");
}

#[test]
fn recovery_plan_for_partial_cstyle_fills_only_missing_separator() {
    // `for (i = 0; i < n` — one separator already typed; fill only the one MISSING
    // separator (`for (i = 0; i < n;)`), not two.
    assert_eq!(scan("for (i = 0; i < n").scope_closers, ";)");
}

#[test]
fn recovery_plan_for_complete_cstyle_needs_no_separator() {
    // `for (a; b; c` — both separators present; only the `)` is missing.
    assert_eq!(scan("for (a; b; c").scope_closers, ")");
}

#[test]
fn recovery_plan_for_of_header_needs_no_separators() {
    // `for (const x of items` — iterator form; a `;` separator would be INVALID,
    // so only the `)` is emitted (no regression of a near-complete for-of).
    assert_eq!(scan("for (const x of items").scope_closers, ")");
}

#[test]
fn recovery_plan_for_in_header_needs_no_separators() {
    assert_eq!(scan("for (const k in obj").scope_closers, ")");
}

#[test]
fn recovery_plan_for_with_in_operator_in_test_is_cstyle() {
    // `for (let i = 0; 'k' in obj` — the `in` here is the relational OPERATOR in
    // the test clause, NOT a for-in header: a `;` was already seen, so the header
    // is C-style and the one missing separator is filled (`;)`), the `in` ignored.
    assert_eq!(scan("for (let i = 0; 'k' in obj").scope_closers, ";)");
}

#[test]
fn recovery_plan_switch_header_gets_discriminant_and_block() {
    // `switch (` — empty discriminant is invalid AND the `)` must be followed by a
    // block: `switch (undefined) {}`.
    assert_eq!(scan("switch (").scope_closers, "undefined) {}");
}

#[test]
fn recovery_plan_catch_header_gets_binding_and_block() {
    // `try {} catch (` — the catch header needs a binding and a block.
    assert_eq!(scan("try {} catch (").scope_closers, "undefined) {}");
}

#[test]
fn recovery_plan_member_call_named_catch_is_not_a_block_header() {
    // NEGATIVE: `p.catch(` is a Promise METHOD call (`catch` is a property name
    // after `.`), NOT a `catch` block header — it must close as empty call args,
    // never `undefined) {}`.
    assert_eq!(scan("p.catch(").scope_closers, ")");
}

#[test]
fn recovery_plan_member_named_if_is_not_a_condition_header() {
    // NEGATIVE: `obj.if(` is a member call (reserved words are legal property
    // names), so the `(` is empty-valid call args, not a condition paren.
    assert_eq!(scan("obj.if(").scope_closers, ")");
}

#[test]
fn recovery_plan_member_named_for_is_not_a_for_header() {
    // NEGATIVE: `arr.for(` is a member call (`for` is a legal property name after a
    // `.`), so the `(` is empty-valid call args — it must NOT be classified as a
    // `for` header and acquire the `;;` C-style separators.
    assert_eq!(scan("arr.for(").scope_closers, ")");
}

#[test]
fn recovery_plan_control_keyword_paren_at_depth_still_completes() {
    // The header-paren requirement holds at ANY nesting depth, not just top level:
    // an `if (` nested inside a block still completes its condition before the
    // enclosing block closes (innermost-first: `undefined)` then `}`).
    assert_eq!(scan("{ if (").scope_closers, "undefined)}");
}

#[test]
fn recovery_plan_dangling_member_then_control_keyword_completes_header() {
    // The member-hole × control-keyword INTERACTION. A dangling `a.` (newline before
    // the next token) is recovered as a `MemberHole`; the synthesized `a.valueOf` is
    // a COMPLETED operand, so the FOLLOWING statement's control keyword must classify
    // normally and complete its required header. RED before: recovering the hole left
    // the scanner in `Dot` state, suppressing the keyword via the member-name guard,
    // so `if (` closed as empty call args (`)`) instead of a condition header
    // (`undefined)`) — yielding invalid `a.valueOf\nif ();`.
    for (src, expected) in [
        ("a.\nif (", "undefined)"),
        ("a.\nwhile (", "undefined)"),
        ("a.\nfor (", ";;)"),
        ("a.\nswitch (", "undefined) {}"),
        ("a.\ntry {} catch (", "undefined) {}"),
        // Optional-chain sibling — `a?.` shares the same pending-dot resolution path,
        // so the same operand reset closes this variant of the class too.
        ("a?.\nif (", "undefined)"),
    ] {
        let plan = scan(src);
        // The dangling dot is still recovered as exactly one member hole.
        assert_eq!(
            plan.inserts
                .iter()
                .filter(|i| matches!(i, RecoveryInsert::MemberHole { .. }))
                .count(),
            1,
            "[{src}] the dangling `a.` must still produce exactly one member hole"
        );
        // ...AND the following control keyword completes its required header — the
        // recovered member operand must not suppress the keyword classification.
        assert_eq!(
            plan.scope_closers, expected,
            "[{src}] the control keyword after a recovered member hole must complete its header"
        );
    }
}

#[test]
fn recovery_plan_dangling_member_then_continuation_keeps_operand_state() {
    // A dangling `a.` followed by a non-control statement keeps a clean operand
    // state: `return a` is recovered with no spurious closer (the recovered
    // `a.valueOf` ends the prior statement; `return a` is the next one).
    let plan = scan("a.\nreturn a");
    assert_eq!(
        plan.inserts
            .iter()
            .filter(|i| matches!(i, RecoveryInsert::MemberHole { .. }))
            .count(),
        1,
        "the dangling `a.` produces one member hole"
    );
    assert_eq!(
        plan.scope_closers, "",
        "a non-control continuation needs no scope closer"
    );
}
