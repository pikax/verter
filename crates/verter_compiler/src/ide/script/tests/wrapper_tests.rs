//! Wrapper helpers + glue tests (D10 cohort).

use super::*;

// ── Generic wrapper tests ─────────────────────────────────────

#[test]
fn generic_wrapper_simple() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="T">
const value = {} as unknown as T
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<T>()"),
        "wrapper should have <T>: {}",
        code
    );
}

#[test]
fn generic_wrapper_with_extends() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="T extends string">
const value = {} as unknown as T
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<T extends string>()"),
        "wrapper should have <T extends string>: {}",
        code
    );
}

#[test]
fn generic_wrapper_multiple() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="K extends string, V">
const k = {} as unknown as K
const v = {} as unknown as V
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<K extends string, V>()"),
        "wrapper should have multiple generics: {}",
        code
    );
}

#[test]
fn non_generic_wrapper_unchanged() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const msg = 'hello'
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN()"),
        "non-generic should have no angle brackets: {}",
        code
    );
    assert!(
        !code.contains("function ___VERTER___TemplateBindingFN<"),
        "non-generic should NOT have angle brackets: {}",
        code
    );
}

#[test]
fn generic_wrapper_invalid_syntax_fallback() {
    // "T in string" is invalid TS (should be "extends"), but the raw
    // string should still pass through so TypeScript surfaces the error.
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="T in string">
const value = 'hello'
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<T in string>()"),
        "invalid generic should still be emitted raw: {}",
        code
    );
}

// ── Helper imports tests ──────────────────────────────────────

#[test]
fn helper_imports_emitted() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    assert!(
        code.contains("import type { Prettify as ___VERTER___Prettify"),
        "should have Prettify import: {}",
        code
    );
    assert!(
        code.contains("import { shallowUnwrapRef as ___VERTER___shallowUnwrapRef"),
        "should have shallowUnwrapRef import: {}",
        code
    );
    assert!(
        !code.contains("import type { default as ___VERTER___Self }"),
        "self-import should no longer be emitted: {}",
        code
    );
}

#[test]
fn helper_imports_hoisted_before_wrapper() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
    let import_pos = code.find("import type { Prettify").unwrap();
    assert!(
        import_pos < fn_pos,
        "helper imports should be before wrapper function"
    );
}

// ── GlobalComponents fallback NAV-PROBE locator ───────────────────

/// The locator and the emitter share one emission contract: locating the const
/// NAME span inside REAL emitted output must return the probe MEMBER offset.
#[test]
fn nav_probe_locator_roundtrips_real_emission() {
    let mut buf = String::from("PREAMBLE;");
    super::super::wrapper::emit_global_component_fallbacks(
        &mut buf,
        &["GlobalEmitComp".to_string(), "ElButton".to_string()],
        false,
    );
    for name in ["GlobalEmitComp", "ElButton"] {
        let decl = format!("const {name} = ");
        let name_start = (buf.find(&decl).expect("const emitted") + "const ".len()) as u32;
        let name_end = name_start + name.len() as u32;
        let probe = global_component_nav_probe_offset(&buf, name_start, name_end)
            .unwrap_or_else(|| panic!("locator must resolve the {name} probe:\n{buf}"));
        assert_eq!(
            &buf[probe as usize..probe as usize + name.len()],
            name,
            "probe offset must point at the member identifier"
        );
        assert_eq!(
            &buf[probe as usize - 2..probe as usize],
            ").",
            "probe member must be a property access on the GlobalComponents nav call"
        );
    }
    // The emitted probe rides the imported @verter/types nav helper, never an
    // import('vue') type query and never a new top-level `vue` import.
    assert!(
        buf.contains("void ___VERTER___globalComponentsNav().GlobalEmitComp;"),
        "nav probe emitted: {buf}"
    );
    assert!(
        !buf.contains("import('vue')"),
        "no import('vue') query: {buf}"
    );
}

/// Fail-closed: a span that is NOT a fallback const (foreign text, tampered
/// emission, a JS-mode const) locates nothing.
#[test]
fn nav_probe_locator_fails_closed_on_foreign_spans() {
    // A user const that shadows the shape but lacks the probe line.
    let tsx =
        "const GlobalEmitComp = {} as ___VERTER___GlobalComponentType<'GlobalEmitComp'>;\nother();";
    let start = "const ".len() as u32;
    let end = start + "GlobalEmitComp".len() as u32;
    assert_eq!(global_component_nav_probe_offset(tsx, start, end), None);

    // JS-mode emission has no probe.
    let mut js = String::new();
    super::super::wrapper::emit_global_component_fallbacks(&mut js, &["VIcon".to_string()], true);
    assert!(!js.contains("___VERTER___globalComponentsNav()"));
    let js_start = (js.find("const VIcon").expect("js const") + "const ".len()) as u32;
    assert_eq!(
        global_component_nav_probe_offset(&js, js_start, js_start + "VIcon".len() as u32),
        None
    );

    // A garbage span never resolves.
    assert_eq!(global_component_nav_probe_offset("abc", 0, 2), None);
}
