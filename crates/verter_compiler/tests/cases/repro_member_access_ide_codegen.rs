//! Regression coverage: `<script setup>` incomplete member access (`a.`) must
//! compile to VALID IDE TSX through the token-scan recovery path.
//!
//! User symptom that motivated this (BUG-REPORT.md "Intellisense is lost (CRITICAL DX)"):
//! ```vue
//! <script setup>
//! let a = 1;
//! () => {
//!   a.
//!   return a
//! }
//! ```
//! At `a.` CTRL+Space triggered many recompiles then "No Suggestions"; TS & Volar
//! handle the trailing-dot case fine.
//!
//! ROOT CAUSE these tests lock down (at the codegen layer): the IDE codegen
//! (`CompileTarget::IDE`, the LSP path) preserves the user's incomplete `a.`
//! verbatim; without recovery it would emit synthetic scaffolding (`var
//! ___VERTER___instance …`, the recovered `return a`, or a closing `}`)
//! IMMEDIATELY adjacent to it with no statement-terminating boundary, so the
//! trailing `.` would absorb the first token of whatever follows (`a.var` /
//! `a.return a` / `a.}`) and the generated `.vue.tsx` virtual file would not be
//! valid TSX — the LSP would ship that broken file to tsgo/tsserver and a parse
//! error spanning the cursor would degrade the whole file → "No Suggestions". The
//! recovery plan (`ide/script_recover.rs`) inserts member/expression holes and
//! scope closers so this no longer happens; these tests guard against regressing.
//!
//! The project's own standard for IDE TSX is "OXC parses it clean" — every
//! `ide_*` regression in `compile_tests.rs` asserts `parsed.errors.is_empty()`.
//! These tests hold the incomplete-member-access case to that SAME bar.
//!
//! DISCRIMINATING: the working case (`a` with no dot) emits OXC-clean TSX with
//! `a` declared inside `___VERTER___TemplateBindingFN` — that arm is the positive
//! control. The `a.` arms each exercise the same root cause through a different
//! codegen path and would fail if recovery regressed.

use oxc_allocator::Allocator;
use verter_compiler::compile::{compile, CodegenOptions, CompileTarget, VerterCompileOptions};

/// Compile an SFC to IDE (`CompileTarget::IDE`) TSX — the exact target the LSP
/// uses (`CompileProfile { target: CompileTarget::IDE, .. }`).
fn ide_tsx(source: &str) -> String {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Index.vue".to_string()),
        target: CompileTarget::IDE,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let result = compile(source, &options, &verter_opts, &alloc);
    result
        .tsx
        .as_ref()
        .map(|t| t.code.clone())
        .unwrap_or_default()
}

/// OXC parse errors of a generated TSX string — empty == valid virtual file.
fn oxc_parse_errors(code: &str) -> Vec<String> {
    let alloc = Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    parsed.errors.iter().map(|e| e.to_string()).collect()
}

/// POSITIVE CONTROL: the working case (`a` with no trailing dot) must produce
/// OXC-clean IDE TSX. If this ever fails the test is mis-calibrated, not the bug.
#[test]
fn working_member_access_emits_valid_ide_tsx() {
    let source = "<script setup>\nlet a = 1;\n() => {\n  a\n  return a\n}\n</script>\n";
    let code = ide_tsx(source);
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "POSITIVE CONTROL: the working `a` case must emit valid TSX, got errors {:?}\n--- TSX ---\n{code}",
        oxc_parse_errors(&code)
    );
    assert!(
        code.contains("___VERTER___TemplateBindingFN"),
        "working case must wrap the setup body in the template-binding function"
    );
}

/// HEADLINE: the exact BUG-REPORT case — `a.` inside a multi-line arrow body.
/// The single token-scan recovery path keeps the body inside the
/// `___VERTER___TemplateBindingFN` wrapper, fills the dangling `a.` with a member
/// hole, and closes the open arrow brace, so the trailing `a.` can no longer
/// absorb the recovered `return` (`a.return a`). The IDE TSX parses clean.
#[test]
fn member_access_dot_in_multiline_arrow_emits_valid_ide_tsx() {
    let source = "<script setup>\nlet a = 1;\n() => {\n  a.\n  return a\n}\n</script>\n";
    let code = ide_tsx(source);
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `a.` inside a multi-line arrow emits INVALID IDE TSX \
         (trailing dot absorbs the next token). The LSP ships this to tsgo → \"No Suggestions\". \
         OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// Same root cause via a single top-level `a.` statement. The recovered member
/// hole stops the trailing `a.` from absorbing the synthetic
/// `var ___VERTER___instance …` scaffolding emitted right after the body
/// (`a.var …`); the IDE TSX parses clean.
#[test]
fn member_access_dot_top_level_emits_valid_ide_tsx() {
    let source = "<script setup>\nlet a = 1;\na.\n</script>\n";
    let code = ide_tsx(source);
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: top-level `a.` absorbs the synthetic scaffolding emitted after the \
         setup body (`a.var ___VERTER___instance`). OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// `const x = a.` — incomplete assignment RHS. The member hole prevents the
/// dangling dot from absorbing the following synthetic scaffolding.
#[test]
fn member_access_dot_assignment_rhs_emits_valid_ide_tsx() {
    let source = "<script setup>\nlet a = 1;\nconst x = a.\n</script>\n";
    let code = ide_tsx(source);
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `const x = a.` absorbs the following synthetic scaffolding. \
         OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// `a.` immediately before the arrow's closing `}`: the member hole stops the dot
/// from absorbing the `}` (`a.}`).
#[test]
fn member_access_dot_before_brace_emits_valid_ide_tsx() {
    let source = "<script setup>\nlet a = 1;\n() => {\n  a.\n}\n</script>\n";
    let code = ide_tsx(source);
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `a.` before the closing brace absorbs the `}}` (`a.}}`). \
         OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// The multi-line `a.` recovery must NOT strand the user's setup body at module
/// scope: the body stays inside `___VERTER___TemplateBindingFN` so the binding is
/// in a stable, completable virtual scope (TS/Volar parity). This asserts the
/// wrapper survives and the body is nested inside it.
#[test]
fn member_access_dot_multiline_keeps_template_binding_wrapper() {
    let with_template = "<script setup>\nlet a = 1;\n() => {\n  a.\n  return a\n}\n</script>\n<template>\n  <div>{{ a }}</div>\n</template>\n";
    let code = ide_tsx(with_template);
    // The single recovery path keeps `let a = 1; () => { a. … }` INSIDE the
    // `___VERTER___TemplateBindingFN` wrapper (body after the wrapper opening), the
    // same well-formed function body the working case places it in — so the body the
    // user is editing stays in a scope where completion resolves `a`. A regression
    // that stranded the body at module scope (before the wrapper) would fail here.
    let wrapper_idx = code
        .find("___VERTER___TemplateBindingFN")
        .expect("wrapper present");
    let body_idx = code.find("let a = 1;").expect("user body present");
    assert!(
        body_idx > wrapper_idx,
        "REGRESSION: the user's setup body (`let a = 1; () => {{ a. }}`) is stranded \
         at MODULE scope BEFORE the ___VERTER___TemplateBindingFN wrapper (recovery dropped the \
         wrapper). The body must stay nested INSIDE the wrapper as in the working case. \n--- TSX ---\n{code}"
    );
}

// ── Additional recovery-shape arms ─────────────────────────────────
// Each holds an incomplete `<script setup>` to the project bar "OXC parses the
// IDE TSX clean", exercising a distinct recovery operation (member hole,
// expression hole, scope closer). All RED before the recovery fix.

/// Optional-chain member hole: `a?.` must terminate with a property placeholder
/// so the `?.` cannot absorb the following scaffolding.
#[test]
fn member_access_optional_chain_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet a = 1;\na?.\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `a?.` must recover to a valid member access. OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// Expression hole — trailing binary operator (`a +`).
#[test]
fn trailing_binary_operator_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet a = 1;\na +\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: a trailing binary operator must recover with an operand. OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// Expression hole — trailing assignment RHS (`const x =`).
#[test]
fn trailing_assignment_rhs_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet a = 1;\nconst x =\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: a trailing assignment RHS must recover with an operand. OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// Expression hole — trailing conditional arm (`a ? 1 :`).
#[test]
fn trailing_conditional_arm_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet a = 1;\nconst x = a ? 1 :\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: a trailing conditional arm must recover with an operand. OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// Scope closer + member hole — unterminated call wrapping incomplete member
/// access (`foo(a.`): both the dangling dot AND the open paren must recover.
#[test]
fn unterminated_call_with_member_access_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet a = 1;\nfoo(a.\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `foo(a.` must recover (member hole + close paren). OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// Scope closer — unterminated call (`foo(`).
#[test]
fn unterminated_call_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet a = 1;\nfoo(\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: an unterminated call `foo(` must recover (close paren). OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// Scope closer — unbalanced nested arrow/block (`() => {` with no close).
#[test]
fn unbalanced_nested_arrow_block_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet a = 1;\nconst f = () => {\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: an unbalanced arrow body `() => {{` must recover (close brace). OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// CLEAN-PATH PRESERVATION: a `<script setup>` that parses cleanly must NOT be
/// reshaped by recovery. A complete member access (`a.toString()`) is preserved
/// VERBATIM — never rewritten to a recovery placeholder — and no expression-hole
/// marker (`(undefined)`) ever appears. Recovery only fires on a parse error, so
/// clean code carries zero synthetic recovery chunks. This pins the clean codegen
/// shape so the failure-path work cannot leak into it.
#[test]
fn clean_script_setup_output_is_not_reshaped_by_recovery() {
    let source = "<script setup>\nlet a = 1;\nconst c = a.toString();\n</script>\n";
    let code = ide_tsx(source);
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "clean SFC must emit valid TSX\n--- TSX ---\n{code}"
    );
    assert!(
        code.contains("___VERTER___TemplateBindingFN"),
        "clean SFC must wrap the body in the binding function\n--- TSX ---\n{code}"
    );
    // The complete member access survives byte-for-byte — recovery's member-hole
    // logic must NOT touch clean code (no `a.toStringvalueOf`, no `a.valueOf`).
    assert!(
        code.contains("a.toString()"),
        "clean member access must be preserved verbatim\n--- TSX ---\n{code}"
    );
    // No expression-hole placeholder is ever synthesized on the clean path.
    assert!(
        !code.contains("(undefined)"),
        "clean SFC must carry no synthetic expression-hole chunk\n--- TSX ---\n{code}"
    );
}

// ── Open-delimiter recovery ──────────────────────────────
// An open delimiter that requires a non-empty expression (grouping paren,
// computed-member bracket, arrow parenthesized body) must recover with a
// placeholder operand BEFORE the closer, not collapse to invalid empty
// delimiters (`const x = ()`, `foo[]`, `() => ()`).

/// `const x = (` — open grouping paren. RED before: `const x = ();` (invalid).
#[test]
fn open_grouping_paren_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nconst x = (\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `const x = (` must recover to `const x = (undefined)`, not invalid \
         `const x = ()`. OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// `foo[` — computed member bracket. RED before: `foo[];` (invalid).
#[test]
fn computed_member_bracket_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nlet foo = [1];\nfoo[\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `foo[` must recover to `foo[undefined]`, not invalid `foo[]`. \
         OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

/// `const f = () => (` — arrow parenthesized body. RED before: `() => ();` (invalid).
#[test]
fn arrow_parenthesized_body_emits_valid_ide_tsx() {
    let code = ide_tsx("<script setup>\nconst f = () => (\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "BUG: `const f = () => (` must recover to `() => (undefined)`, not invalid \
         `() => ()`. OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
}

// ── Control/condition-keyword header parens (whole class) ──
// A `(` directly after a control/condition keyword (`if`/`while`/`for`/`switch`/
// `catch`/`with`) is a REQUIRED header paren, NOT empty-valid call arguments. An
// open header must recover to VALID TSX with the keyword-specific completion — an
// `undefined` discriminant for `if`/`while`/`with`, the missing `;` separators for
// `for` (none for the iterator form), and a trailing `{}` block for `switch`/
// `catch`. RED before: `if (` collapsed to invalid `if ();`.

/// CLOSED MATRIX — every control/condition-keyword header row, each producing
/// OXC-parse-valid TSX with the exact keyword-specific completion (whitespace
/// stripped so the user's trailing newline before `</script>` is irrelevant).
#[test]
fn control_keyword_open_header_emits_valid_ide_tsx() {
    // (label, `<script setup>` body, completion the recovered TSX must contain)
    let cases: &[(&str, &str, &str)] = &[
        // condition parens — `undefined` discriminant, `;` completes the body.
        ("if", "if (", "if(undefined)"),
        ("while", "while (", "while(undefined)"),
        ("with", "with (", "with(undefined)"),
        // block headers — `undefined` discriminant AND a trailing `{}`.
        ("switch", "switch (", "switch(undefined){}"),
        ("catch", "try {} catch (", "catch(undefined){}"),
        // for headers — fill only the MISSING `;` separators (none for iterators).
        ("for-empty", "for (", "for(;;)"),
        ("for-partial-cstyle", "for (i = 0; i < n", "for(i=0;i<n;)"),
        ("for-of", "for (const x of items", "for(constxofitems)"),
        ("for-in", "for (const k in obj", "for(constkinobj)"),
    ];
    for (label, body, must_contain) in cases {
        let src = format!("<script setup>\n{body}\n</script>\n");
        let code = ide_tsx(&src);
        let errors = oxc_parse_errors(&code);
        assert!(
            errors.is_empty(),
            "REGRESSION (control-keyword `{label}`): `{body}` must recover to VALID TSX, not an \
             invalid empty header. OXC errors: {errors:?}\n--- TSX ---\n{code}"
        );
        let stripped: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            stripped.contains(must_contain),
            "control-keyword `{label}`: recovered TSX must contain completion `{must_contain}`\n\
             --- TSX ---\n{code}"
        );
    }
}

/// NEGATIVE arms of the same matrix: delimiters that are VALID empty must stay
/// placeholder-free — call args `f()`, array literal `[]`, block/object `{}`. A
/// control keyword used as a MEMBER name (`promise.catch(`, `obj.if(`) is a method
/// call, not a header, so it must NOT acquire a discriminant or a `{}` block.
#[test]
fn valid_empty_delimiters_and_member_calls_stay_placeholder_free() {
    // empty call args stay `f()`.
    let code = ide_tsx("<script setup>\nlet f = () => {};\nf(\n</script>\n");
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "empty call args must recover valid\n--- TSX ---\n{code}"
    );
    assert!(
        !code.contains("f(undefined)"),
        "empty call args must NOT get an `undefined` placeholder\n--- TSX ---\n{code}"
    );
    // empty array literal stays `[]`.
    let code = ide_tsx("<script setup>\nconst a = [\n</script>\n");
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "empty array literal must recover valid\n--- TSX ---\n{code}"
    );
    assert!(
        !code.contains("[undefined]"),
        "empty array literal must NOT get an `undefined` placeholder\n--- TSX ---\n{code}"
    );
    // empty block recovers valid (no placeholder concept).
    let code = ide_tsx("<script setup>\nfunction g() {\n</script>\n");
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "open block must recover valid\n--- TSX ---\n{code}"
    );
    // `promise.catch(` is a Promise method call — NOT a `catch` block header. RED
    // would be `promise.catch(undefined) {}` (invalid `.catch(){}`).
    let code = ide_tsx("<script setup>\nconst p = Promise.resolve();\np.catch(\n</script>\n");
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "member call `.catch(` must recover valid (NOT a catch-block header)\n--- TSX ---\n{code}"
    );
    assert!(
        !code.contains("catch(undefined) {}") && !code.contains("catch(undefined){}"),
        "`.catch(` member call must NOT be completed as a catch-block header\n--- TSX ---\n{code}"
    );
    // `obj.if(` is a member call (`if` is a legal property name after `.`) — the `(`
    // is empty-valid call args, never a condition header (`obj.if(undefined)`).
    let code = ide_tsx("<script setup>\nconst obj = {};\nobj.if(\n</script>\n");
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "member call `.if(` must recover valid (NOT a condition header)\n--- TSX ---\n{code}"
    );
    assert!(
        !code.contains("if(undefined)"),
        "`.if(` member call must NOT acquire a condition discriminant\n--- TSX ---\n{code}"
    );
    // `arr.for(` is a member call (`for` is a legal property name after `.`) — the `(`
    // is empty-valid call args; it must NOT be classified as a `for` header and gain
    // the `;;` C-style separators ([P3]: `ControlKind::For` must not trigger here).
    let code = ide_tsx("<script setup>\nconst arr = [];\narr.for(\n</script>\n");
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "member call `.for(` must recover valid (NOT a for header)\n--- TSX ---\n{code}"
    );
    assert!(
        !code.contains("for(;;)") && !code.contains("for (;;)"),
        "`.for(` member call must NOT be completed as a for header\n--- TSX ---\n{code}"
    );
}

// ── Member hole × control-keyword header (interaction) ────
// A dangling member access (`a.`) immediately FOLLOWED by a control/condition
// keyword on the next line. The recovered `a.valueOf` is a COMPLETED operand, so
// the following keyword must classify normally and complete its required header.
// RED before: the recovered dot left the scanner in `Dot` state, and the
// member-name guard then suppressed the keyword's header classification, so `if (`
// collapsed to empty call args → invalid `a.valueOf\nif ();`.

/// CLOSED MATRIX — every control/condition-keyword header row, each PRECEDED by a
/// dangling `a.` member hole, must still recover to OXC-parse-valid TSX with the
/// member hole filled AND the keyword's required-header completion intact.
#[test]
fn dangling_member_then_control_keyword_emits_valid_ide_tsx() {
    // (label, body after the dangling `a.`, completion the recovered TSX must contain)
    let cases: &[(&str, &str, &str)] = &[
        ("if", "if (", "if(undefined)"),
        ("while", "while (", "while(undefined)"),
        ("for", "for (", "for(;;)"),
        ("switch", "switch (", "switch(undefined){}"),
        ("try-catch", "try {} catch (", "catch(undefined){}"),
    ];
    for (label, tail, must_contain) in cases {
        // `let a = 1;` gives the dangling `a.` a real operand; the keyword follows on
        // the NEXT line so `a.` is a genuine dangling member (newline after the dot).
        let src = format!("<script setup>\nlet a = 1;\na.\n{tail}\n</script>\n");
        let code = ide_tsx(&src);
        let errors = oxc_parse_errors(&code);
        assert!(
            errors.is_empty(),
            "REGRESSION (dangling-member-then-`{label}`): `a.` then `{tail}` must recover to VALID \
             TSX, not an invalid empty header. OXC errors: {errors:?}\n--- TSX ---\n{code}"
        );
        let stripped: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        // The dangling dot is filled with the member hole (`a.valueOf`)...
        assert!(
            stripped.contains("a.valueOf"),
            "dangling-member-then-`{label}`: the dangling `a.` must be filled with the member hole \
             (`a.valueOf`)\n--- TSX ---\n{code}"
        );
        // ...AND the following control keyword still completes its required header.
        assert!(
            stripped.contains(must_contain),
            "dangling-member-then-`{label}`: the following control keyword must complete its header \
             (`{must_contain}`) — a recovered member operand must not suppress it\n--- TSX ---\n{code}"
        );
    }
}

// ── Top-level fact gate ──────────────────────────────────

/// A block-local declaration (inside a function body) must NOT be registered as a
/// setup binding — only top-level facts feed binding registration, mirroring the
/// clean top-level parser. RED before: `inner` leaks as a setup binding.
#[test]
fn block_local_declaration_not_registered_as_binding() {
    let code =
        ide_tsx("<script setup lang=\"ts\">\nfunction f() { const inner = 1; }\na.\n</script>\n");
    let errors = oxc_parse_errors(&code);
    assert!(
        errors.is_empty(),
        "recovered TSX must be valid. OXC errors: {errors:?}\n--- TSX ---\n{code}"
    );
    // The top-level `f` IS a binding (positive control).
    assert!(
        code.contains("typeof f"),
        "the top-level function `f` must still be registered as a binding\n--- TSX ---\n{code}"
    );
    // The block-local `inner` must NOT appear as a setup binding — it would show as
    // `inner: inner as unknown as typeof inner` in the shallowUnwrapRef block.
    assert!(
        !code.contains("typeof inner"),
        "BUG: block-local `inner` leaked into setup bindings\n--- TSX ---\n{code}"
    );
}

// ── Recovered macro LHS keeps macro semantics ────────────

/// A recovered `defineProps` binding used in the template must resolve. Template
/// `props.x` lowers to `__props.x`; recovery must emit the `const __props = props;`
/// alias (clean-lowering parity) so `__props` is not dangling. RED before: the
/// `props` LHS is marked Props (→ `__props.`) but no `__props` is declared.
#[test]
fn recovered_define_props_template_resolves_without_dangling_props() {
    let source = "<script setup>\nconst props = defineProps<{ x: number }>();\nprops.\n</script>\n\
                  <template>\n  <div>{{ props.x }}</div>\n</template>\n";
    let code = ide_tsx(source);
    assert!(
        oxc_parse_errors(&code).is_empty(),
        "recovered TSX must be valid\n--- TSX ---\n{code}"
    );
    assert!(
        code.contains("const __props = props"),
        "BUG: recovery must emit the `__props` alias for the recovered defineProps \
         binding (template `props.x` → `__props.x` would otherwise dangle)\n--- TSX ---\n{code}"
    );
    // No dangling `__props`: any `__props` member reference must be backed by a decl.
    assert!(
        !code.contains("__props.") || code.contains("const __props ="),
        "dangling __props reference (referenced but not declared)\n--- TSX ---\n{code}"
    );
}

// ── Clean-path GOLDEN preservation ───────────────────────

/// GOLDEN: a representative VALID `<script setup>` (import + props + setup binding +
/// template) must produce EXACTLY this IDE TSX. Recovery work must never reshape the
/// clean path; any drift in clean codegen is caught here byte-for-byte (line endings
/// normalized for cross-platform stability).
#[test]
fn clean_script_setup_output_matches_golden() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\n\
                  const props = defineProps<{ msg: string }>()\nconst count = ref(0)\n</script>\n\
                  <template>\n  <div>{{ msg }}{{ count }}</div>\n</template>\n";
    // Normalize line endings on BOTH sides so a CRLF checkout of the fixture cannot
    // spuriously fail the byte comparison (cross-platform portability rule).
    let code = ide_tsx(source).replace("\r\n", "\n");
    let golden = GOLDEN_CLEAN_IDE_TSX.replace("\r\n", "\n");
    assert_eq!(
        code, golden,
        "clean-path IDE TSX drifted from the golden — if this change is intentional, update the \
         tests/golden_clean_ide_tsx.snap fixture\n--- ACTUAL ---\n{code}"
    );
}

/// Pinned expected clean-path output for [`clean_script_setup_output_matches_golden`].
/// The fixture is produced by the clean codegen path itself; recovery work must never
/// reshape it.
const GOLDEN_CLEAN_IDE_TSX: &str = include_str!("golden_clean_ide_tsx.snap");
