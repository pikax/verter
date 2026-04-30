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
