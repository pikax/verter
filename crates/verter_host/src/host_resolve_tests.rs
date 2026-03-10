use std::sync::Arc;

use verter_span::Span;

use crate::shared::read_lock;
use crate::{
    CompileErrorPolicy, CompileProfile, FileKind, HostConfig, HostDiagnostic, HostError,
    HostSeverity, PublicApiMode, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

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
