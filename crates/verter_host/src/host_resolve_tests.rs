use std::sync::Arc;

use verter_span::Span;

use crate::shared::read_lock;
use crate::{
    CompileErrorPolicy, CompileProfile, FileKind, HostConfig, HostDiagnostic, HostError,
    HostSeverity, PublicApiMode, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};
use verter_core::compile::CompileTarget;

fn strict_host() -> VerterHost {
    VerterHost::new(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn profile() -> CompileProfile {
    CompileProfile::default()
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_non_sfc(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn compile_main_error(host: &VerterHost, canonical_id: &str) -> crate::DiagnosticsSnapshot {
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile(),
    }) {
        Err(HostError::CompileError { diagnostics }) => diagnostics,
        Err(other) => panic!("expected compile error, got {other:?}"),
        Ok(result) => panic!(
            "expected compile error, got successful response {}",
            result.id
        ),
    }
}

fn public_api_code(host: &VerterHost, canonical_id: &str) -> String {
    host.get_public_api(canonical_id)
        .unwrap_or_else(|| panic!("expected public api output for {canonical_id}"))
        .code
        .to_string()
}

fn public_api_code_with_mode(host: &VerterHost, canonical_id: &str, mode: PublicApiMode) -> String {
    host.get_public_api_with_mode(canonical_id, mode)
        .unwrap_or_else(|| panic!("expected public api output for {canonical_id}"))
        .code
        .to_string()
}

fn find_diag<'a>(diagnostics: &'a crate::DiagnosticsSnapshot, code: &str) -> &'a HostDiagnostic {
    diagnostics
        .diagnostics
        .iter()
        .find(|diag| diag.code == code)
        .unwrap_or_else(|| {
            panic!(
                "expected diagnostic {code}, got {:?}",
                diagnostics.diagnostics
            )
        })
}

fn assert_missing_src_compile_error(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
    specifier: &str,
    expected_tag: &str,
) {
    let diagnostics = compile_main_error(host, canonical_id);
    let missing = find_diag(&diagnostics, "HOST_MISSING_EXTERNAL_SOURCE");

    assert_eq!(missing.severity, HostSeverity::Error);
    assert!(
        missing.message.contains(specifier),
        "missing external source message should mention {specifier}: {}",
        missing.message
    );
    assert!(
        !missing.message.contains("HOST_MISSING_MACRO_TYPE_DEP"),
        "message should not mention macro type deps: {}",
        missing.message
    );

    let start = source.find(expected_tag).unwrap() as u32;
    let end = source[start as usize..]
        .find('>')
        .map(|offset| start + offset as u32 + 1)
        .unwrap();
    assert_eq!(
        missing.span,
        Some(Span::new(start, end)),
        "missing external source span should point at the owning tag"
    );

    {
        let files = read_lock(&host.files);
        let entry = files.get(canonical_id).unwrap();
        assert!(
            entry.compile_slots.is_empty(),
            "failed compile must not leave cached outputs behind"
        );
    }
    assert!(
        host.get_ide(canonical_id, &profile()).is_none(),
        "failed compile must not expose IDE output"
    );
}

#[test]
fn missing_script_src_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<script src=\"./missing.ts\"></script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    assert_missing_src_compile_error(&host, "/src/Comp.vue", source, "./missing.ts", "<script");
}

#[test]
fn missing_template_src_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<template src=\"./missing.html\"></template>\n<script setup>const n = 1</script>";
    upsert_vue(&host, "/src/Comp.vue", source);

    assert_missing_src_compile_error(
        &host,
        "/src/Comp.vue",
        source,
        "./missing.html",
        "<template",
    );
}

#[test]
fn missing_style_src_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<template><div/></template>\n<style src=\"./missing.css\"></style>";
    upsert_vue(&host, "/src/Comp.vue", source);

    assert_missing_src_compile_error(&host, "/src/Comp.vue", source, "./missing.css", "<style");
}

#[test]
fn external_src_can_compile_via_owner_dependency_mapping() {
    let host = strict_host();
    let source =
        "<template src=\"@/partials/panel.html\"></template>\n<script setup>const n = 1</script>";
    upsert_vue(&host, "/src/Comp.vue", source);
    upsert_non_sfc(&host, "/src/partials/panel.html", "<div>{{ n }}</div>");
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "@/partials/panel.html".to_string(),
            resolved_canonical_id: Some("/src/partials/panel.html".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed when the real external source is registered as a dep");

    assert!(
        !response.diagnostics.has_errors,
        "resolved external src should not keep missing-source diagnostics"
    );
    assert!(
        response.code.contains("render"),
        "resolved compile should produce render code, got: {}",
        response.code
    );
}

#[test]
fn missing_macro_type_dependency_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    let missing = find_diag(&diagnostics, "HOST_MISSING_MACRO_TYPE_DEP");

    assert_eq!(missing.severity, HostSeverity::Error);
    assert!(
        missing.message.contains("./types"),
        "macro type dep message should mention the missing import source: {}",
        missing.message
    );
    assert!(
        !missing.message.contains("HOST_MISSING_EXTERNAL_SOURCE"),
        "macro type dep message should not be reported as an external src failure: {}",
        missing.message
    );

    let import_start = source.find("import type").unwrap() as u32;
    let import_end = import_start + "import type { Props } from './types'".len() as u32;
    assert_eq!(
        missing.span,
        Some(Span::new(import_start, import_end)),
        "macro type dep span should point at the owning import"
    );

    {
        let files = read_lock(&host.files);
        let entry = files.get("/src/Comp.vue").unwrap();
        assert!(
            entry.compile_slots.is_empty(),
            "failed macro type dep compile must not cache outputs"
        );
    }
    assert!(
        host.get_ide("/src/Comp.vue", &profile()).is_none(),
        "failed macro type dep compile must not expose IDE output"
    );
}

#[test]
fn missing_template_src_retries_successfully_after_dependency_arrives() {
    let host = strict_host();
    let source =
        "<template src=\"./resolved.html\"></template>\n<script setup>const n = 1</script>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diag| diag.code == "HOST_MISSING_EXTERNAL_SOURCE"),
        "first compile should fail on the missing template source"
    );

    upsert_non_sfc(&host, "/src/resolved.html", "<div>resolved</div>");

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed once the external template source exists");

    assert!(
        !response.diagnostics.has_errors,
        "resolved compile should not keep the old missing-source error"
    );
    assert!(
        response.code.contains("render"),
        "resolved compile should produce code, got: {}",
        response.code
    );
    assert!(
        !response.code.contains("missing external source"),
        "resolved compile output must not contain the previous error text"
    );
}

#[test]
fn missing_macro_type_dependency_retries_successfully_after_dependency_arrives() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diag| diag.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "first compile should fail on the missing macro type dependency"
    );

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { msg: string }",
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed once the macro type dependency exists");

    assert!(
        !response.diagnostics.has_errors,
        "resolved compile should not keep the old missing macro type error"
    );
    assert!(
        response.code.contains("export default"),
        "resolved compile should produce a main module, got: {}",
        response.code
    );
    assert!(
        !response.code.contains("HOST_MISSING_MACRO_TYPE_DEP"),
        "resolved compile output must not contain the previous missing-type error"
    );
}

#[test]
fn public_api_resolves_transitive_imported_define_emits_type() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Emits } from './types'\nconst emit = defineEmits<Emits>()\n</script>\n<template><button @click=\"emit('submit', 'ok')\">Send</button></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { BaseEmits } from './base'\nexport interface Emits extends BaseEmits { confirm: [id: number] }",
    );
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface BaseEmits { (e: 'submit', payload: string): void }",
    );

    let code = public_api_code(&host, "/src/Comp.vue");
    assert!(
        code.contains("\"onSubmit\"?: (payload: string) => void"),
        "public api should include transitive imported submit handler props, got: {code}"
    );
    assert!(
        code.contains("\"onConfirm\"?: (...args: [id: number]) => void"),
        "public api should include local tuple emits handler props, got: {code}"
    );
}

#[test]
fn invalid_imported_define_props_type_keeps_object_like_error() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    upsert_non_sfc(&host, "/src/types.ts", "export type Props = string");

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    let invalid = find_diag(&diagnostics, "XInvalidMacroType");
    assert!(
        invalid.message.contains(
            "defineProps() type argument 'Props' must resolve to an object-like props type."
        ),
        "expected object-like props diagnostic, got: {}",
        invalid.message
    );
}

#[test]
fn invalid_imported_define_emits_type_keeps_emits_shape_error() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Emits } from './types'\nconst emit = defineEmits<Emits>()\n</script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Emits { foo: string }",
    );

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    let invalid = find_diag(&diagnostics, "XInvalidMacroType");
    assert!(
        invalid
            .message
            .contains("defineEmits() type argument 'Emits' must resolve to emit call signatures or a named-tuple emits object."),
        "expected emits-shape diagnostic, got: {}",
        invalid.message
    );
}

#[test]
fn testing_public_api_exposes_internal_script_setup_bindings() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst count = ref(1)\nconst label = 'ready'\n</script>\n<template><div>{{ count }} {{ label }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let public = public_api_code(&host, "/src/Comp.vue");
    let testing = public_api_code_with_mode(&host, "/src/Comp.vue", PublicApiMode::Testing);

    assert!(
        testing.contains("count: typeof count"),
        "testing mode should expose ref bindings on the instance: {testing}"
    );
    assert!(
        testing.contains("label: typeof label"),
        "testing mode should expose local bindings on the instance: {testing}"
    );
    assert!(
        !testing.contains("ref: typeof ref"),
        "value imports must not be exposed as instance bindings: {testing}"
    );
    assert!(
        !public.contains("count: typeof count"),
        "public mode must remain unchanged: {public}"
    );
}

#[test]
fn testing_public_api_ignores_define_expose_narrowing() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst foo = ref(1)\nconst bar = ref('hidden')\ndefineExpose({ foo })\n</script>\n<template><div>{{ foo }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let testing = public_api_code_with_mode(&host, "/src/Comp.vue", PublicApiMode::Testing);

    assert!(
        testing.contains("foo: typeof foo"),
        "testing mode should retain explicitly exposed bindings: {testing}"
    );
    assert!(
        testing.contains("bar: typeof bar"),
        "testing mode should also retain non-exposed internal bindings: {testing}"
    );
    assert!(
        !testing.contains("defineExpose({ foo }) as"),
        "testing mode should not narrow the debug instance to defineExpose: {testing}"
    );
}

// ── TSC extract cache tests ──────────────────────────────────────────────

#[test]
fn public_api_cache_populated_on_first_call() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/test/Cached.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div /></template>"#,
    );

    // First call should populate the cache
    let api = host.get_public_api("/test/Cached.vue");
    assert!(api.is_some(), "should produce public API output");

    // Verify cache is populated
    let files = read_lock(&host.files);
    let entry = files.get("/test/Cached.vue").expect("entry exists");
    assert!(
        entry.cached_tsc_extract.is_some(),
        "cached_tsc_extract should be populated after first get_public_api call"
    );
}

#[test]
fn public_api_cache_reused_on_second_call() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/test/Reuse.vue",
        r#"<script setup lang="ts">
defineProps<{ x: number }>()
defineEmits<{ (e: 'click'): void }>()
</script>
<template><div /></template>"#,
    );

    let api1 = host.get_public_api("/test/Reuse.vue").expect("first call");
    let api2 = host.get_public_api("/test/Reuse.vue").expect("second call");
    assert_eq!(
        api1.code, api2.code,
        "two consecutive calls must produce identical code"
    );
}

#[test]
fn public_api_cache_cleared_on_source_change() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/test/Clear.vue",
        r#"<script setup lang="ts">
defineProps<{ a: string }>()
</script>
<template><div /></template>"#,
    );

    // Populate cache
    let _api = host.get_public_api("/test/Clear.vue");
    {
        let files = read_lock(&host.files);
        let entry = files.get("/test/Clear.vue").expect("entry");
        assert!(
            entry.cached_tsc_extract.is_some(),
            "cache should be populated"
        );
    }

    // Upsert with changed source
    upsert_vue(
        &host,
        "/test/Clear.vue",
        r#"<script setup lang="ts">
defineProps<{ b: number }>()
</script>
<template><div /></template>"#,
    );

    // Cache should be cleared
    {
        let files = read_lock(&host.files);
        let entry = files.get("/test/Clear.vue").expect("entry");
        assert!(
            entry.cached_tsc_extract.is_none(),
            "cached_tsc_extract should be cleared after source change"
        );
    }
}

#[test]
fn public_api_cache_cleared_on_template_change() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/test/TplChange.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div /></template>"#,
    );

    // Populate cache
    let _api = host.get_public_api("/test/TplChange.vue");
    {
        let files = read_lock(&host.files);
        let entry = files.get("/test/TplChange.vue").expect("entry");
        assert!(
            entry.cached_tsc_extract.is_some(),
            "cache should be populated after first get_public_api call"
        );
    }

    // Upsert with changed template only (script unchanged)
    upsert_vue(
        &host,
        "/test/TplChange.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><span /></template>"#,
    );

    // Cache should be cleared because root_element_tag is template-derived
    {
        let files = read_lock(&host.files);
        let entry = files.get("/test/TplChange.vue").expect("entry");
        assert!(
            entry.cached_tsc_extract.is_none(),
            "cached_tsc_extract should be cleared after template change"
        );
    }
}

#[test]
fn public_api_output_updates_after_template_root_change() {
    let host = strict_host();

    // Start with <div> root — should produce HTMLAttributes for $attrs
    upsert_vue(
        &host,
        "/test/RootTag.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div /></template>"#,
    );
    let api1 = public_api_code(&host, "/test/RootTag.vue");
    assert!(
        api1.contains("HTMLAttributes"),
        "div root should produce HTMLAttributes in $attrs: {api1}"
    );

    // Change template root from <div> to no-template (component-only)
    // Using a component tag (PascalCase) as root means root_element_tag = None
    upsert_vue(
        &host,
        "/test/RootTag.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><MyChild /></template>"#,
    );
    let api2 = public_api_code(&host, "/test/RootTag.vue");

    // After fix: output should differ because root_element_tag changes
    assert_ne!(
        api1, api2,
        "public API output must change when template root element changes"
    );
    assert!(
        !api2.contains("HTMLAttributes"),
        "component root should NOT produce HTMLAttributes: {api2}"
    );
}

#[test]
fn public_api_cache_cleared_on_descriptor_change() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/test/DescChange.vue",
        r#"<script setup lang="ts" generic="T">
defineProps<{ x: T }>()
</script>
<template><div /></template>"#,
    );

    // Populate cache
    let _api = host.get_public_api("/test/DescChange.vue");
    {
        let files = read_lock(&host.files);
        let entry = files.get("/test/DescChange.vue").expect("entry");
        assert!(
            entry.cached_tsc_extract.is_some(),
            "cache should be populated after first get_public_api call"
        );
    }

    // Upsert with changed generic attribute (script content identical, descriptor different)
    upsert_vue(
        &host,
        "/test/DescChange.vue",
        r#"<script setup lang="ts" generic="T, U">
defineProps<{ x: T }>()
</script>
<template><div /></template>"#,
    );

    // Cache should be cleared because generic_params is descriptor-derived
    {
        let files = read_lock(&host.files);
        let entry = files.get("/test/DescChange.vue").expect("entry");
        assert!(
            entry.cached_tsc_extract.is_none(),
            "cached_tsc_extract should be cleared after descriptor change (generic attr)"
        );
    }
}

// ── DependencyResolution tests ────────────────────────────────────────────

/// Exact alias resolution via structured DependencyResolution record.
/// This is the core fix: when `@/components/base` resolves to `/src/components/base/index.ts`,
/// the host should find it via the exact record rather than failing on basename heuristics.
#[test]
fn exact_alias_resolves_via_dependency_resolution() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from '@/components/base'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/App.vue", source);
    upsert_non_sfc(
        &host,
        "/src/components/base/index.ts",
        "export interface Props { msg: string }",
    );

    // Provide structured resolution: specifier → exact canonical ID
    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::DependencyResolution {
            specifier: "@/components/base".to_string(),
            resolved_canonical_id: Some("/src/components/base/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Compile should succeed — exact resolution bypasses basename heuristics
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/App.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed with exact dependency resolution");

    assert!(
        !response.diagnostics.has_errors,
        "no HOST_MISSING_MACRO_TYPE_DEP with exact resolution: {:?}",
        response.diagnostics
    );
    assert!(
        response.code.contains("export default"),
        "should produce main module output"
    );
    // Negative: should not contain error markers
    assert!(
        !response.code.contains("HOST_MISSING"),
        "output must not contain error markers"
    );
}

/// Relative import resolves to directory index via candidate probing.
#[test]
fn relative_import_resolves_via_directory_index_file() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    // Dep is at /src/types/index.ts (NOT /src/types.ts)
    upsert_non_sfc(
        &host,
        "/src/types/index.ts",
        "export interface Props { msg: string }",
    );
    // No set_import_dependencies — direct path probing should find /src/types/index.ts

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed via directory index probing");

    assert!(
        !response.diagnostics.has_errors,
        "directory index resolution should work: {:?}",
        response.diagnostics
    );
}

/// Relative import `./type` resolves to `./type.d.ts` via Phase 3 probing.
#[test]
fn relative_import_resolves_dts_extension() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props, Emits } from './type'\nconst props = defineProps<Props>()\ndefineEmits<Emits>()\n</script>\n<template><div>{{ props.order }}</div></template>";
    upsert_vue(&host, "/src/components/Comp.vue", source);
    // The dependency is a .d.ts file (common in projects that separate type declarations)
    upsert_non_sfc(
        &host,
        "/src/components/type.d.ts",
        "export interface Props { order: string | null }\nexport interface Emits { updatePrice: [number]; updateStatus: [string]; }",
    );
    // No set_import_dependencies — Phase 3 probing should find /src/components/type.d.ts

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/components/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed via .d.ts extension probing");

    assert!(
        !response.diagnostics.has_errors,
        ".d.ts extension resolution should work: {:?}",
        response.diagnostics
    );
    // Positive: prop names should be resolved
    assert!(
        response.code.contains("order"),
        "should resolve 'order' prop from .d.ts"
    );
    // Negative: no missing dep errors
    assert!(
        !response.code.contains("HOST_MISSING"),
        "output must not contain error markers"
    );
}

/// Barrel file `export * from './Drawer'` chain resolves types through re-exports.
#[test]
fn barrel_export_star_resolves_type_through_reexport_chain() {
    let host = strict_host();
    // SFC imports from a barrel file
    let source = "<script setup lang=\"ts\">\nimport type { DrawerEmits } from './base'\ndefineEmits<DrawerEmits>()\n</script>\n<template><div>ok</div></template>";
    upsert_vue(&host, "/src/components/Comp.vue", source);

    // Barrel file: export * from './Drawer'
    upsert_non_sfc(&host, "/src/components/base.ts", "export * from './Drawer'");

    // Intermediate file: export { DrawerEmits } from './types'
    upsert_non_sfc(
        &host,
        "/src/components/Drawer.ts",
        "export type { DrawerEmits } from './drawer-types'",
    );

    // Final file: defines the actual type
    upsert_non_sfc(
        &host,
        "/src/components/drawer-types.ts",
        "export interface DrawerEmits { close: []; open: [value: boolean]; }",
    );

    // Set dependency chain
    host.set_import_dependencies(
        "/src/components/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/components/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/components/base.ts",
        vec![crate::DependencyResolution {
            specifier: "./Drawer".to_string(),
            resolved_canonical_id: Some("/src/components/Drawer.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/components/Drawer.ts",
        vec![crate::DependencyResolution {
            specifier: "./drawer-types".to_string(),
            resolved_canonical_id: Some("/src/components/drawer-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/components/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed via barrel file re-export chain");

    // Positive: emits should be resolved
    assert!(
        !response.diagnostics.has_errors,
        "barrel file re-export chain should resolve: {:?}",
        response.diagnostics
    );
    // Negative: no error markers
    assert!(
        !response.code.contains("HOST_MISSING"),
        "output must not contain error markers"
    );
    assert!(
        !response.code.contains("XInvalidMacroType"),
        "output must not contain invalid macro type errors"
    );
}

/// Exact resolution to an unloaded file should NOT silently fall back to heuristics.
#[test]
fn exact_resolution_to_unloaded_file_reports_error() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from '@/foo'\nconst props = defineProps<Props>()\n</script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    // Provide resolution pointing to a file that is NOT loaded in the host
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "@/foo".to_string(),
            resolved_canonical_id: Some("/src/foo/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    // Do NOT upsert /src/foo/index.ts

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "should get HOST_MISSING_MACRO_TYPE_DEP when exact resolution target is not loaded: {:?}",
        diagnostics.diagnostics
    );
}

/// Directory index resolution follows extension priority (.ts before .js).
#[test]
fn directory_index_resolution_follows_extension_priority() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    // Both .ts and .js index files exist
    upsert_non_sfc(
        &host,
        "/src/types/index.ts",
        "export interface Props { msg: string }",
    );
    upsert_non_sfc(&host, "/src/types/index.js", "// no types here");

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed — .ts has priority over .js");

    assert!(
        !response.diagnostics.has_errors,
        "should resolve to index.ts (higher priority): {:?}",
        response.diagnostics
    );
}

/// Candidate-list fallback resolves to first loaded candidate.
#[test]
fn candidate_list_resolves_to_first_loaded() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    // Only the second candidate is loaded
    upsert_non_sfc(
        &host,
        "/src/types-b/index.ts",
        "export interface Props { msg: string }",
    );

    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "@/types".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![
                "/src/types-a/index.ts".to_string(),
                "/src/types-b/index.ts".to_string(),
            ],
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed via candidate list fallback");

    assert!(
        !response.diagnostics.has_errors,
        "should resolve to first loaded candidate: {:?}",
        response.diagnostics
    );
}

/// Exact resolution invalidates on dep change.
#[test]
fn exact_resolution_invalidates_on_dep_change() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    upsert_non_sfc(
        &host,
        "/src/types/index.ts",
        "export interface Props { msg: string }",
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "@/types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // First compile
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("first compile should succeed");

    // Change the dependency — should trigger invalidation
    upsert_non_sfc(
        &host,
        "/src/types/index.ts",
        "export interface Props { msg: string; count: number }",
    );

    // Compile slots should be cleared
    let files = read_lock(&host.files);
    let entry = files.get("/src/Comp.vue").expect("entry exists");
    assert!(
        entry.compile_slots.is_empty(),
        "compile slots should be cleared after dep change"
    );
}

/// Barrel chain ending at a `.vue` SFC resolves the exported type.
/// Chain: Consumer.vue → base.ts (export *) → Drawer.ts (export type) → drawer.vue (defines type)
#[test]
fn barrel_chain_ending_at_vue_sfc_resolves_type() {
    let host = strict_host();

    // Consumer SFC imports DrawerEmits through a barrel chain
    let consumer_source = r#"<script setup lang="ts">
import type { DrawerEmits } from './base'
defineEmits<DrawerEmits>()
</script>
<template><div>ok</div></template>"#;
    upsert_vue(&host, "/src/Consumer.vue", consumer_source);

    // Barrel file: re-exports everything from Drawer
    upsert_non_sfc(&host, "/src/base.ts", "export * from './Drawer'");

    // Intermediate: re-exports the type from the .vue SFC
    upsert_non_sfc(
        &host,
        "/src/Drawer.ts",
        "export type { DrawerEmits } from './drawer.vue'",
    );

    // The .vue SFC that defines the actual type
    upsert_vue(
        &host,
        "/src/drawer.vue",
        r#"<script setup lang="ts">
export interface DrawerEmits { close: []; open: [value: boolean]; }
</script>
<template><div>drawer</div></template>"#,
    );

    // Wire up dependency chain
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/base.ts",
        vec![crate::DependencyResolution {
            specifier: "./Drawer".to_string(),
            resolved_canonical_id: Some("/src/Drawer.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/Drawer.ts",
        vec![crate::DependencyResolution {
            specifier: "./drawer.vue".to_string(),
            resolved_canonical_id: Some("/src/drawer.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Consumer.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed via barrel chain ending at .vue SFC");

    // Positive: emits should be resolved
    assert!(
        !response.diagnostics.has_errors,
        "barrel chain ending at .vue should resolve types: {:?}",
        response.diagnostics
    );
    // Negative: no error markers
    assert!(
        !response.code.contains("HOST_MISSING"),
        "output must not contain HOST_MISSING error markers"
    );
    assert!(
        !response.code.contains("XInvalidMacroType"),
        "output must not contain XInvalidMacroType errors"
    );
}

/// Same as barrel_chain_ending_at_vue_sfc_resolves_type but with a bare specifier
/// (like `@/components/base`) instead of a relative import, and absolute paths.
/// This mirrors the JS unplugin test more closely.
#[test]
fn barrel_chain_with_bare_specifier_resolves_type() {
    let host = strict_host();

    // Consumer SFC imports DrawerEmits through a bare specifier (path alias)
    let consumer_source = r#"<script setup lang="ts">
import type { DrawerEmits } from "@/components/base"
defineEmits<DrawerEmits>()
</script>
<template><div>ok</div></template>"#;
    upsert_vue(&host, "/project/src/Consumer.vue", consumer_source);

    // Barrel file: re-exports everything from Drawer
    upsert_non_sfc(
        &host,
        "/project/components/base/index.ts",
        "export * from './Drawer'",
    );

    // Intermediate: re-exports the type from the .vue SFC
    upsert_non_sfc(
        &host,
        "/project/components/base/Drawer/index.ts",
        "export type { DrawerEmits } from './src/index.vue'",
    );

    // The .vue SFC that defines the actual type
    upsert_vue(
        &host,
        "/project/components/base/Drawer/src/index.vue",
        r#"<script setup lang="ts">
export interface DrawerEmits { close: []; open: [value: boolean]; disposed: [] }
defineEmits<DrawerEmits>()
</script>
<template><div>drawer</div></template>"#,
    );

    // Wire up dependency chain — bare specifier on consumer
    host.set_import_dependencies(
        "/project/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "@/components/base".to_string(),
            resolved_canonical_id: Some("/project/components/base/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/project/components/base/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./Drawer".to_string(),
            resolved_canonical_id: Some("/project/components/base/Drawer/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/project/components/base/Drawer/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./src/index.vue".to_string(),
            resolved_canonical_id: Some(
                "/project/components/base/Drawer/src/index.vue".to_string(),
            ),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/project/src/Consumer.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed with bare specifier barrel chain ending at .vue SFC");

    // Positive: emits should be resolved
    assert!(
        !response.diagnostics.has_errors,
        "bare specifier barrel chain ending at .vue should resolve types: {:?}",
        response.diagnostics
    );
    // Negative: no error markers
    assert!(
        !response.code.contains("HOST_MISSING"),
        "output must not contain HOST_MISSING error markers"
    );
    assert!(
        !response.code.contains("XInvalidMacroType"),
        "output must not contain XInvalidMacroType errors"
    );
}

/// Vue SFC with both <script> and <script setup> — types from companion block
/// should be visible when resolving through a barrel chain.
#[test]
fn vue_sfc_companion_script_types_visible_for_setup_resolution() {
    let host = strict_host();

    // Consumer imports Props through a barrel chain ending at a .vue SFC
    let consumer_source = r#"<script setup lang="ts">
import type { Props } from './base'
defineProps<Props>()
</script>
<template><div>consumer</div></template>"#;
    upsert_vue(&host, "/src/Consumer.vue", consumer_source);

    // Barrel file
    upsert_non_sfc(&host, "/src/base.ts", "export * from './Widget'");

    // Intermediate re-export
    upsert_non_sfc(
        &host,
        "/src/Widget.ts",
        "export type { Props } from './widget.vue'",
    );

    // The .vue SFC with a companion <script> defining a base interface,
    // and <script setup> defining Props that extends it.
    upsert_vue(
        &host,
        "/src/widget.vue",
        r#"<script lang="ts">
export interface Base { foo: string }
</script>
<script setup lang="ts">
export interface Props extends Base { bar: number }
</script>
<template><div>widget</div></template>"#,
    );

    // Wire up dependency chain
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/base.ts",
        vec![crate::DependencyResolution {
            specifier: "./Widget".to_string(),
            resolved_canonical_id: Some("/src/Widget.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/Widget.ts",
        vec![crate::DependencyResolution {
            specifier: "./widget.vue".to_string(),
            resolved_canonical_id: Some("/src/widget.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Consumer.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed with companion script types");

    // Positive: both props from Base and Props should be resolved
    assert!(
        !response.diagnostics.has_errors,
        "companion script types should resolve: {:?}",
        response.diagnostics
    );
    // The output should contain both prop names from the extends chain
    let code = &response.code;
    assert!(
        code.contains("foo") && code.contains("bar"),
        "both 'foo' (from Base) and 'bar' (from Props) should appear in output: {code}"
    );
    // Negative: no error markers
    assert!(
        !code.contains("HOST_MISSING"),
        "output must not contain HOST_MISSING error markers"
    );
    assert!(
        !code.contains("XInvalidMacroType"),
        "output must not contain XInvalidMacroType errors"
    );
}

/// Same barrel chain test but with default HostConfig (dev_mode=true,
/// DevServeLastKnownGood) and Windows-style paths. This reproduces the
/// conditions under the NAPI path that fails while strict_host tests pass.
#[test]
fn barrel_chain_vue_sfc_with_dev_mode_and_windows_paths() {
    let host = VerterHost::new(HostConfig::default()); // dev_mode: true, DevServeLastKnownGood

    let consumer_source = r#"<script setup lang="ts">
import type { DrawerEmits } from "@/components/base"
defineEmits<DrawerEmits>()
</script>
<template><div>ok</div></template>"#;
    upsert_vue(&host, "c:/Users/test/temp/Consumer.vue", consumer_source);

    upsert_non_sfc(
        &host,
        "c:/Users/test/temp/base/index.ts",
        "export * from './Drawer'",
    );

    upsert_non_sfc(
        &host,
        "c:/Users/test/temp/base/Drawer/index.ts",
        "export type { DrawerEmits } from './src/index.vue'",
    );

    upsert_vue(
        &host,
        "c:/Users/test/temp/base/Drawer/src/index.vue",
        r#"<script setup lang="ts">
export interface DrawerEmits { close: []; open: [value: boolean]; disposed: [] }
defineEmits<DrawerEmits>()
</script>
<template><div>drawer</div></template>"#,
    );

    host.set_import_dependencies(
        "c:/Users/test/temp/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "@/components/base".to_string(),
            resolved_canonical_id: Some("c:/Users/test/temp/base/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "c:/Users/test/temp/base/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./Drawer".to_string(),
            resolved_canonical_id: Some("c:/Users/test/temp/base/Drawer/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "c:/Users/test/temp/base/Drawer/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./src/index.vue".to_string(),
            resolved_canonical_id: Some("c:/Users/test/temp/base/Drawer/src/index.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("c:/Users/test/temp/Consumer.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed with dev_mode and Windows paths");

    assert!(
        !response.diagnostics.has_errors,
        "dev_mode barrel chain should resolve types: {:?}",
        response.diagnostics
    );
    assert!(
        !response.code.contains("HOST_MISSING"),
        "output must not contain HOST_MISSING error markers"
    );
    assert!(
        !response.code.contains("XInvalidMacroType"),
        "output must not contain XInvalidMacroType errors"
    );
}

/// Regression: a real-world SFC with `style="..."` + `:style="{...}"` and Vue 3.4
/// same-name shorthand bindings must NOT produce a false-positive `DuplicateAttribute`
/// diagnostic through the host pipeline. The parser correctly distinguishes static
/// attributes from v-bind directives; this test guards against stale-binary regressions.
#[test]
fn duplicate_attribute_regression_does_not_appear_through_host_pipeline() {
    let host = strict_host();
    // Key traits: UTF-8 multibyte comment, static style + dynamic :style, same-name shorthand
    let source = r#"<script setup lang="ts">
// Verter — UTF-8 multibyte: «»
import { ref } from 'vue'
const stickyTop = ref(true)
const height = ref('100px')
</script>
<template>
  <div
    style="overflow: auto"
    :style="{ height }"
    :sticky-top
  >
    content
  </div>
</template>"#;

    let upsert_result = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/Regression.vue".to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert should succeed");

    // No DuplicateAttribute in parse-phase diagnostics
    let dup_parse: Vec<_> = upsert_result
        .diagnostics
        .diagnostics
        .iter()
        .filter(|d| d.code == "DuplicateAttribute")
        .collect();
    assert!(
        dup_parse.is_empty(),
        "upsert should not produce DuplicateAttribute parse diagnostics, got: {dup_parse:?}"
    );

    // Compile through IDE target (closest to the original reproduction path)
    let ide_profile = CompileProfile {
        target: CompileTarget::IDE,
        ..profile()
    };
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Regression.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: ide_profile,
        })
        .expect("IDE compile should succeed for this SFC");

    // No DuplicateAttribute in compile-phase diagnostics
    let dup_compile: Vec<_> = response
        .diagnostics
        .diagnostics
        .iter()
        .filter(|d| d.code == "DuplicateAttribute")
        .collect();
    assert!(
        dup_compile.is_empty(),
        "compile should not produce DuplicateAttribute diagnostics, got: {dup_compile:?}"
    );
    assert!(
        !response.diagnostics.has_errors,
        "compile should have no errors at all"
    );
}

#[test]
fn expand_relative_candidates_produces_correct_paths() {
    let host = VerterHost::new(HostConfig::default());
    let candidates = host.expand_relative_candidates("/workspace/src/App.vue", "./types");

    // Direct resolution
    assert_eq!(candidates[0], "/workspace/src/types");

    // Extension candidates
    assert!(
        candidates.contains(&"/workspace/src/types.ts".to_string()),
        "should include .ts extension variant"
    );
    assert!(
        candidates.contains(&"/workspace/src/types.tsx".to_string()),
        "should include .tsx extension variant"
    );

    // Index candidates
    assert!(
        candidates.contains(&"/workspace/src/types/index.ts".to_string()),
        "should include /index.ts variant"
    );

    // Should NOT contain the bare specifier itself
    assert!(
        !candidates.contains(&"./types".to_string()),
        "should not contain the raw specifier"
    );
}

#[test]
fn expand_relative_candidates_handles_parent_traversal() {
    let host = VerterHost::new(HostConfig::default());
    let candidates = host.expand_relative_candidates("/workspace/src/deep/file.vue", "../utils");

    assert_eq!(candidates[0], "/workspace/src/utils");
    assert!(
        candidates.contains(&"/workspace/src/utils.ts".to_string()),
        "should resolve parent traversal correctly"
    );
}

#[test]
fn diagnostics_generation_increments_on_successful_recompile() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    // First compile — error path (missing macro type dep)
    let _ = compile_main_error(&host, "/src/Comp.vue");
    let gen1 = host
        .get_diagnostics_generation("/src/Comp.vue")
        .expect("gen should exist after first compile");
    assert!(
        gen1 >= 1,
        "generation should be at least 1 after error-path compile"
    );

    // Load the dependency and recompile — success path
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { msg: string }",
    );
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed after dep loaded");

    let gen2 = host
        .get_diagnostics_generation("/src/Comp.vue")
        .expect("gen should exist after second compile");
    assert!(
        gen2 > gen1,
        "diagnostics_generation should increment on success recompile: gen1={gen1}, gen2={gen2}"
    );
    assert!(
        !response.diagnostics.has_errors,
        "success compile should have no errors"
    );
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "success compile should not contain HOST_MISSING_MACRO_TYPE_DEP: {:?}",
        response.diagnostics.diagnostics
    );
}

#[test]
fn diagnostics_generation_increments_on_source_change() {
    let host = strict_host();
    let source1 = "<script setup lang=\"ts\">\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source1);

    // Compile to populate latest_diagnostics
    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some("/src/Comp.vue".to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile(),
    });
    let gen1 = host
        .get_diagnostics_generation("/src/Comp.vue")
        .expect("gen should exist after compile");

    // Upsert with different source — triggers latest_diagnostics.clear()
    let source2 = "<script setup lang=\"ts\">\nconst b = 2\n</script>\n<template><div>{{ b }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source2);

    let gen2 = host
        .get_diagnostics_generation("/src/Comp.vue")
        .expect("gen should exist after source change");
    assert!(
        gen2 > gen1,
        "diagnostics_generation should increment on source change: gen1={gen1}, gen2={gen2}"
    );
}

#[test]
fn bump_diagnostics_generation_increments_without_recompile() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some("/src/Comp.vue".to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile(),
    });
    let gen1 = host
        .get_diagnostics_generation("/src/Comp.vue")
        .expect("gen should exist after compile");

    host.bump_diagnostics_generation("/src/Comp.vue");

    let gen2 = host
        .get_diagnostics_generation("/src/Comp.vue")
        .expect("gen should exist after bump");
    assert!(
        gen2 > gen1,
        "bump_diagnostics_generation should increment: gen1={gen1}, gen2={gen2}"
    );
}

#[test]
fn bump_diagnostics_generation_is_noop_for_missing_file() {
    let host = strict_host();
    host.bump_diagnostics_generation("/nonexistent.vue");
    assert!(
        host.get_diagnostics_generation("/nonexistent.vue")
            .is_none(),
        "bump on nonexistent file should not create an entry"
    );
}

#[test]
fn invalidate_compile_slots_forces_recompile() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let p = profile();
    let _ = host.ensure_compiled("/src/Comp.vue", &p);
    // Second call should be a cache hit (no-op)
    let _ = host.ensure_compiled("/src/Comp.vue", &p);

    // After invalidation, ensure_compiled should recompile
    host.invalidate_compile_slots("/src/Comp.vue");
    let gen_before = host
        .get_diagnostics_generation("/src/Comp.vue")
        .unwrap_or(0);
    let _ = host.ensure_compiled("/src/Comp.vue", &p);
    let gen_after = host
        .get_diagnostics_generation("/src/Comp.vue")
        .unwrap_or(0);
    assert!(
        gen_after > gen_before,
        "ensure_compiled after invalidate_compile_slots should recompile and bump diagnostics_generation: before={gen_before}, after={gen_after}"
    );
}

#[test]
fn invalidate_compile_slots_is_noop_for_missing_file() {
    let host = strict_host();
    host.invalidate_compile_slots("/nonexistent.vue");
    // Should not panic
}
