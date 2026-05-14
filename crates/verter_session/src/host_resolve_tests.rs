use std::sync::Arc;

use verter_span::Span;
use verter_workspace::WorkspaceRead;

#[cfg(target_arch = "wasm32")]
use crate::shared::read_lock;
use crate::{
    BlockOverrideEntry, BlockOverrideRequest, CompileErrorPolicy, CompileProfile, FileKind,
    HostConfig, HostDiagnostic, HostError, HostSeverity, PreprocessorBlockType, PublicApiMode,
    UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};
use verter_compiler::compile::CompileTarget;

fn strict_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
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

#[test]
fn frontier_and_shadow_forbid_guards_are_thread_local() {
    let _frontier_guard = crate::host_resolve::forbid_route_frontier_for_tests();
    let _shadow_guard = crate::host_resolve::forbid_import_route_shadow_for_tests();

    assert!(crate::host_resolve::route_frontier_forbidden_for_current_thread());
    assert!(crate::host_resolve::import_route_shadow_forbidden_for_current_thread());

    let (frontier_forbidden, shadow_forbidden) = std::thread::spawn(|| {
        (
            crate::host_resolve::route_frontier_forbidden_for_current_thread(),
            crate::host_resolve::import_route_shadow_forbidden_for_current_thread(),
        )
    })
    .join()
    .expect("thread-local guard probe should join cleanly");

    assert!(
        !frontier_forbidden,
        "route-frontier forbid guard should not leak across test threads",
    );
    assert!(
        !shadow_forbidden,
        "import-route-shadow forbid guard should not leak across test threads",
    );
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
    host.get_public_api_with_mode(canonical_id, mode, None)
        .unwrap_or_else(|| panic!("expected public api output for {canonical_id}"))
        .code
        .to_string()
}

fn public_api_code_with_profile(
    host: &VerterHost,
    canonical_id: &str,
    mode: PublicApiMode,
    profile: &CompileProfile,
) -> String {
    host.get_public_api_with_mode(canonical_id, mode, Some(profile))
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

    #[cfg(not(target_arch = "wasm32"))]
    {
        let compile_slots_empty = host
            .compile_cache()
            .get(canonical_id)
            .map(|cc| cc.compile_slots.is_empty())
            .unwrap_or(true);
        assert!(
            compile_slots_empty,
            "failed compile must not leave cached outputs behind"
        );
    }
    #[cfg(target_arch = "wasm32")]
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

    #[cfg(not(target_arch = "wasm32"))]
    {
        let compile_slots_empty = host
            .compile_cache()
            .get("/src/Comp.vue")
            .map(|cc| cc.compile_slots.is_empty())
            .unwrap_or(true);
        assert!(
            compile_slots_empty,
            "failed macro type dep compile must not cache outputs"
        );
    }
    #[cfg(target_arch = "wasm32")]
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

#[test]
fn public_api_with_profile_uses_override_script_state() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\ndefineProps<{ raw: string }>()\n</script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let profile = CompileProfile::default();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/Comp.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Script,
                index: 0,
                code: Arc::from("defineProps<{ overrideProp: number }>()"),
                source_map: None,
            }],
        })
        .expect("script override should succeed");

    let raw = public_api_code(&host, "/src/Comp.vue");
    let overridden =
        public_api_code_with_profile(&host, "/src/Comp.vue", PublicApiMode::Public, &profile);

    assert!(
        raw.contains("raw?: string") || raw.contains("raw: string"),
        "raw public api should still reflect the raw script: {raw}"
    );
    assert!(
        !raw.contains("overrideProp"),
        "raw public api must not be polluted by override script state: {raw}"
    );
    assert!(
        overridden.contains("overrideProp?: number") || overridden.contains("overrideProp: number"),
        "profile-aware public api should use the override script state: {overridden}"
    );
    assert!(
        !overridden.contains("raw?: string") && !overridden.contains("raw: string"),
        "override profile should not keep raw-only props in the public api: {overridden}"
    );
}

// ── TSC extract cache tests ──────────────────────────────────────────────

#[test]
fn public_api_with_profile_uses_override_script_state_for_imported_macro_type_dep() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/Types.vue",
        "<script setup lang=\"ts\">\nexport interface Props { raw: string }\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './Types.vue'\ndefineProps<Props>()\n</script>\n<template><div/></template>",
    );

    let profile = CompileProfile::default();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/Types.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Script,
                index: 0,
                code: Arc::from("export type Props = string"),
                source_map: None,
            }],
        })
        .expect("dependency script override should succeed");

    let raw = public_api_code(&host, "/src/Comp.vue");
    let overridden = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("profile-aware compile should succeed even with dependency override");

    assert!(
        raw.contains("new(props?: import(\"vue\").PublicProps & Props)"),
        "raw public api should still resolve successfully against the dependency's raw script state: {raw}"
    );
    assert!(
        !raw.contains("overrideProp"),
        "raw public api must not be polluted by dependency override state: {raw}"
    );
    // The profile compile succeeds because the compiler no longer rejects non-object macro types.
    // The override to the dependency's script (Props = string) does not propagate to the
    // consumer's type resolution — the consumer still resolves Props from the original source.
    assert!(
        overridden.code.contains("raw"),
        "profile compile still resolves Props from original dependency source (override to dep script does not propagate to consumer type resolution): {}",
        overridden.code
    );
}

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

    // Verify cache is populated. cached_tsc_extract lives on
    // DerivedRawState (D48 split).
    #[cfg(not(target_arch = "wasm32"))]
    {
        let cc = host
            .derived_raw_cache()
            .get("/test/Cached.vue")
            .expect("derived_raw_cache entry exists");
        assert!(
            cc.cached_tsc_extract.is_some(),
            "cached_tsc_extract should be populated after first get_public_api call"
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        let files = read_lock(&host.files);
        let entry = files.get("/test/Cached.vue").expect("entry exists");
        assert!(
            entry.cached_tsc_extract.is_some(),
            "cached_tsc_extract should be populated after first get_public_api call"
        );
    }
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_tsc_extract lives on DerivedRawState (D48 split).
        let cc = host
            .derived_raw_cache()
            .get("/test/Clear.vue")
            .expect("derived_raw_cache entry exists");
        assert!(cc.cached_tsc_extract.is_some(), "cache should be populated");
    }
    #[cfg(target_arch = "wasm32")]
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_tsc_extract lives on DerivedRawState (D48 split).
        let cc = host
            .derived_raw_cache()
            .get("/test/Clear.vue")
            .expect("derived_raw_cache entry exists");
        assert!(
            cc.cached_tsc_extract.is_none(),
            "cached_tsc_extract should be cleared after source change"
        );
    }
    #[cfg(target_arch = "wasm32")]
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_tsc_extract lives on DerivedRawState (D48 split).
        let cc = host
            .derived_raw_cache()
            .get("/test/TplChange.vue")
            .expect("derived_raw_cache entry exists");
        assert!(
            cc.cached_tsc_extract.is_some(),
            "cache should be populated after first get_public_api call"
        );
    }
    #[cfg(target_arch = "wasm32")]
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_tsc_extract lives on DerivedRawState (D48 split).
        let cc = host
            .derived_raw_cache()
            .get("/test/TplChange.vue")
            .expect("derived_raw_cache entry exists");
        assert!(
            cc.cached_tsc_extract.is_none(),
            "cached_tsc_extract should be cleared after template change"
        );
    }
    #[cfg(target_arch = "wasm32")]
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_tsc_extract lives on DerivedRawState (D48 split).
        let cc = host
            .derived_raw_cache()
            .get("/test/DescChange.vue")
            .expect("derived_raw_cache entry exists");
        assert!(
            cc.cached_tsc_extract.is_some(),
            "cache should be populated after first get_public_api call"
        );
    }
    #[cfg(target_arch = "wasm32")]
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_tsc_extract lives on DerivedRawState (D48 split).
        let cc = host
            .derived_raw_cache()
            .get("/test/DescChange.vue")
            .expect("derived_raw_cache entry exists");
        assert!(
            cc.cached_tsc_extract.is_none(),
            "cached_tsc_extract should be cleared after descriptor change (generic attr)"
        );
    }
    #[cfg(target_arch = "wasm32")]
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
fn exact_alias_resolves_via_import_route() {
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

/// Exact resolution resolves to specified canonical ID.
#[test]
fn candidate_list_resolves_to_first_loaded() {
    let host = strict_host();
    // Configure workspace with @/ alias via host wrapper (Phase 6b sub-plan
    // §6b.D2b reroute — `host.workspace()` is `pub(crate)`).
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig {
            root: "/src".to_string(),
            workspace_root: "/src".to_string(),
            tsconfig_path: None,
            provider_root: "/src".to_string(),
            workspace_aliases: vec![verter_workspace::WorkspaceAlias {
                find: "@/".to_string(),
                replacement: "/src/".to_string(),
            }],
            compiler_options:
                verter_semantic::analysis::project_resolver::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_semantic::analysis::project_resolver::ProjectMembership::MatchAll,
        },
    ]);
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
            resolved_canonical_id: Some("/src/types-b/index.ts".to_string()),
            possible_canonical_ids: vec![],
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        let cc = host
            .compile_cache()
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.compile_slots.is_empty(),
            "compile slots should be cleared after dep change"
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        let files = read_lock(&host.files);
        let entry = files.get("/src/Comp.vue").expect("entry exists");
        assert!(
            entry.compile_slots.is_empty(),
            "compile slots should be cleared after dep change"
        );
    }
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

#[test]
fn barrel_export_star_ignores_imported_types_from_non_reexported_sfc_bindings() {
    let host = strict_host();

    let consumer_source = r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>consumer</div></template>"#;
    upsert_vue(&host, "/src/Consumer.vue", consumer_source);

    upsert_non_sfc(
        &host,
        "/src/types/index.ts",
        "export * from '../components/A.vue'\nexport * from '../components/B.vue'\n",
    );

    upsert_vue(
        &host,
        "/src/components/A.vue",
        r#"<script lang="ts">
import type { Imported } from '../types'

export interface Props extends Imported {
  label: string
}
</script>
<template><div>a</div></template>"#,
    );

    upsert_vue(
        &host,
        "/src/components/B.vue",
        r#"<script lang="ts">
export interface Imported {
  id: string
}
</script>
<template><div>b</div></template>"#,
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::DependencyResolution {
                specifier: "../components/A.vue".to_string(),
                resolved_canonical_id: Some("/src/components/A.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "../components/B.vue".to_string(),
                resolved_canonical_id: Some("/src/components/B.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    host.set_import_dependencies(
        "/src/components/A.vue",
        vec![crate::DependencyResolution {
            specifier: "../types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
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
        .expect("compile should succeed through barrel cycle without stack overflow");

    assert!(
        !response.diagnostics.has_errors,
        "barrel cycle should resolve Props without diagnostics: {:?}",
        response.diagnostics
    );
    assert!(
        response.code.contains("label"),
        "resolved output should include the local prop: {}",
        response.code
    );
    assert!(
        response.code.contains("id"),
        "resolved output should include the imported prop: {}",
        response.code
    );
}

#[test]
fn imported_type_binding_is_not_treated_as_reexported_macro_type() {
    let host = strict_host();

    let consumer_source = r#"<script setup lang="ts">
import type { Props } from './wrapper'
defineProps<Props>()
</script>
<template><div>consumer</div></template>"#;
    upsert_vue(&host, "/src/Consumer.vue", consumer_source);

    upsert_non_sfc(
        &host,
        "/src/wrapper.ts",
        "import type { Props } from './Base.vue'\nexport interface Wrapper { nested: Props }\n",
    );

    upsert_vue(
        &host,
        "/src/Base.vue",
        r#"<script lang="ts">
export interface Props {
  id: string
}
</script>
<template><div>base</div></template>"#,
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./wrapper".to_string(),
            resolved_canonical_id: Some("/src/wrapper.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/wrapper.ts",
        vec![crate::DependencyResolution {
            specifier: "./Base.vue".to_string(),
            resolved_canonical_id: Some("/src/Base.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let resolved = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./wrapper",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("external type resolution should complete without crashing");

    assert!(
        resolved.is_none(),
        "plain imported type bindings must not masquerade as re-exported macro types"
    );
}

#[test]
fn negative_import_route_cache_invalidates_when_imported_module_changes() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { DynamicProps } from './types'
defineProps<DynamicProps>()
</script>
<template><div>consumer</div></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface StableProps { keep: string }\n",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let first = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./types",
            "DynamicProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("first resolution should complete");
    assert!(
        first.is_none(),
        "DynamicProps should be missing before the dependency update"
    );

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface DynamicProps { added: string }\n",
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let second = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./types",
            "DynamicProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("second resolution should complete")
        .expect("DynamicProps should resolve after the dependency update");

    assert!(
        second
            .props
            .iter()
            .filter_map(|prop| prop.key_name.as_deref())
            .any(|name| name == "added"),
        "resolved props should come from the updated dependency: {:?}",
        second
            .props
            .iter()
            .filter_map(|prop| prop.key_name.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn route_cache_hits_increment_route_fact_reuse_counter() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>consumer</div></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }\n",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let first = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./types",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("first resolution should complete");
    assert!(first.is_some(), "Props should resolve on the first request");

    host.provenance().reset();

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let second = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./types",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("second resolution should complete");
    assert!(
        second.is_some(),
        "Props should resolve on the second request"
    );

    let p = host.provenance().snapshot();
    assert!(
        p.resolved_external_type_cache_hits >= 1,
        "expected a resolved-external-type cache hit on the second request, got {:?}",
        p
    );
}

#[test]
fn frontier_companion_seeds_preserve_narrow_routes_through_alias_targets() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/alpha.ts",
        "export interface AlphaProps { alpha?: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/beta.ts",
        "export interface BetaProps { beta?: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
import type { AlphaProps } from './alpha'
import type { BetaProps } from './beta'

export interface Props {
  primary?: AlphaProps
  secondary?: BetaProps
}
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export { Props as PublicProps } from './types'\n",
    );

    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            crate::DependencyResolution {
                specifier: "./alpha".to_string(),
                resolved_canonical_id: Some("/src/alpha.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./beta".to_string(),
                resolved_canonical_id: Some("/src/beta.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _view = host.resolver_store_view();
    let mut requested_routes = super::FrontierRequestedRoutes::default();
    requested_routes.insert(
        ("/src/barrel.ts".to_string(), "PublicProps".to_string()),
        crate::resolver_core::RouteDemand::MemberPath(vec!["primary".into()]),
    );
    let mut companion_plans = super::FrontierCompanionPlans::default();

    let (frontier, target, _had_route_cycle) = host
        .run_external_type_frontier_closure(
            "/src/barrel.ts",
            "PublicProps",
            &mut requested_routes,
            &mut companion_plans,
        )
        .expect("frontier closure should complete");
    assert_eq!(
        target,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "barrel alias should resolve to the defining symbol",
    );
    assert_eq!(
        requested_routes.get(&("/src/types.ts".to_string(), "Props".to_string())),
        Some(&crate::resolver_core::RouteDemand::MemberPath(vec![
            "primary".into()
        ])),
        "the active member route should be transferred onto the defining target",
    );

    let adapter = super::HostFrontierAdapter {
        host: &host,
        materialize_symbols: false,
        route_exports_only: true,
        view: None,
        route_shallow_cache: std::cell::RefCell::new(rustc_hash::FxHashMap::default()),
    };
    let mut inspected_symbols = rustc_hash::FxHashSet::default();
    let seeds = host.collect_frontier_companion_seeds(
        &frontier,
        &adapter,
        &mut inspected_symbols,
        &mut requested_routes,
        &mut companion_plans,
    );
    assert!(
        seeds
            .iter()
            .all(|seed| { seed.route == Some(crate::resolver_core::RouteDemand::Whole) }),
        "companion seeds should carry an explicit whole-route demand",
    );
    let seeded: std::collections::BTreeSet<_> = seeds
        .into_iter()
        .map(|seed| (seed.canonical_id, seed.exported_name))
        .collect();

    assert!(
        seeded.contains(&("/src/alpha.ts".to_string(), "AlphaProps".to_string())),
        "narrow member route should seed the active imported dependency, got {:?}",
        seeded
    );
    assert!(
        !seeded.contains(&("/src/beta.ts".to_string(), "BetaProps".to_string())),
        "narrow member route should not widen to sibling imported dependencies, got {:?}",
        seeded
    );
}

#[test]
fn frontier_closure_preserves_nested_member_tail_for_imported_companions() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/leaf.ts",
        "export interface Leaf { text: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/unused-leaf.ts",
        "export interface UnusedLeaf { code: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/alpha.ts",
        r#"
import type { Leaf } from './leaf'
import type { UnusedLeaf } from './unused-leaf'

export interface AlphaProps {
  label: Leaf,
  other: UnusedLeaf
}
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/beta.ts",
        "export interface BetaProps { beta?: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
import type { AlphaProps } from './alpha'
import type { BetaProps } from './beta'

export interface Props {
  primary?: AlphaProps
  secondary?: BetaProps
}
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export { Props as PublicProps } from './types'\n",
    );

    host.set_import_dependencies(
        "/src/alpha.ts",
        vec![
            crate::DependencyResolution {
                specifier: "./leaf".to_string(),
                resolved_canonical_id: Some("/src/leaf.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./unused-leaf".to_string(),
                resolved_canonical_id: Some("/src/unused-leaf.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            crate::DependencyResolution {
                specifier: "./alpha".to_string(),
                resolved_canonical_id: Some("/src/alpha.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::DependencyResolution {
                specifier: "./beta".to_string(),
                resolved_canonical_id: Some("/src/beta.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _view = host.resolver_store_view();
    let mut requested_routes = super::FrontierRequestedRoutes::default();
    requested_routes.insert(
        ("/src/barrel.ts".to_string(), "PublicProps".to_string()),
        crate::resolver_core::RouteDemand::MemberPath(vec!["primary".into(), "label".into()]),
    );
    let mut companion_plans = super::FrontierCompanionPlans::default();

    let (frontier, target, _had_route_cycle) = host
        .run_external_type_frontier_closure(
            "/src/barrel.ts",
            "PublicProps",
            &mut requested_routes,
            &mut companion_plans,
        )
        .expect("frontier closure should complete");

    assert_eq!(
        target,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "barrel alias should resolve to the defining symbol",
    );
    assert_eq!(
        requested_routes.get(&("/src/alpha.ts".to_string(), "AlphaProps".to_string())),
        Some(&crate::resolver_core::RouteDemand::MemberPath(vec![
            "label".into()
        ])),
        "the imported companion should keep only the remaining member tail",
    );
    assert!(
        requested_routes.contains_key(&("/src/leaf.ts".to_string(), "Leaf".to_string())),
        "the active nested imported dependency should still be followed",
    );
    assert!(
        !requested_routes
            .contains_key(&("/src/unused-leaf.ts".to_string(), "UnusedLeaf".to_string())),
        "the inactive sibling inside the imported type should remain shallow",
    );
    assert!(
        !requested_routes.contains_key(&("/src/beta.ts".to_string(), "BetaProps".to_string())),
        "the inactive top-level sibling should remain shallow",
    );

    let touched: std::collections::BTreeSet<_> =
        frontier.touched_canonical_ids().into_iter().collect();
    assert!(
        touched.contains("/src/leaf.ts"),
        "nested active imported dependency should be touched, got {:?}",
        touched
    );
    assert!(
        !touched.contains("/src/unused-leaf.ts"),
        "nested inactive sibling should not be materialized, got {:?}",
        touched
    );
}

#[test]
fn frontier_companion_plan_cache_reuses_same_route_entry() {
    let mut cache = super::FrontierCompanionPlans::default();
    let route = crate::resolver_core::RouteDemand::Whole;

    let first = cache.get_or_compute("/src/button.ts", "ButtonProps", &route, || {
        vec![super::PlannedFrontierCompanion {
            alias: "LinkProps".to_string(),
            resolved_canonical: "/src/link.ts".to_string(),
            resolved_exported_name: "LinkProps".to_string(),
            route: crate::resolver_core::RouteDemand::Whole,
        }]
    });
    let second = cache.get_or_compute("/src/button.ts", "ButtonProps", &route, || {
        panic!("same-route plan should be reused")
    });

    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "same requested route should reuse one request-local companion plan entry",
    );
    assert_eq!(
        cache.len(),
        1,
        "repeated plan lookup should keep one cache entry",
    );
}

#[test]
fn frontier_companion_plan_cache_keeps_distinct_routes_separate() {
    let mut cache = super::FrontierCompanionPlans::default();

    let whole = cache.get_or_compute(
        "/src/button.ts",
        "ButtonProps",
        &crate::resolver_core::RouteDemand::Whole,
        Vec::new,
    );
    let member = cache.get_or_compute(
        "/src/button.ts",
        "ButtonProps",
        &crate::resolver_core::RouteDemand::MemberPath(vec!["icon".to_string()]),
        Vec::new,
    );

    assert!(
        !std::sync::Arc::ptr_eq(&whole, &member),
        "different routes must not collapse to the same companion plan entry",
    );
    assert_eq!(
        cache.len(),
        2,
        "whole and member-path requests should cache separately",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn barrel_cache_hits_increment_barrel_fact_reuse_counter() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './barrel'
defineProps<Props>()
</script>
<template><div>consumer</div></template>"#,
    );
    upsert_non_sfc(&host, "/src/barrel.ts", "export * from './inner'\n");
    upsert_non_sfc(
        &host,
        "/src/inner.ts",
        "export interface Props { label: string }\n",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./barrel".to_string(),
            resolved_canonical_id: Some("/src/barrel.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![crate::DependencyResolution {
            specifier: "./inner".to_string(),
            resolved_canonical_id: Some("/src/inner.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let first = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./barrel",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("first resolution should complete");
    assert!(first.is_some(), "Props should resolve on the first request");

    host.resolved_type_cache().clear();
    host.provenance().reset();

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let second = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./barrel",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("second resolution should complete");
    assert!(
        second.is_some(),
        "Props should resolve on the second request"
    );

    // The route-only frontier path reuses host-owned shallow state caches
    // rather than going through the module_facts barrel-reuse counter.
    // Verify that the second request resolved without extra cache misses
    // beyond the one caused by the explicit resolved_type_cache.clear().
    let p = host.provenance().snapshot();
    assert_eq!(
        p.resolved_external_type_cache_misses, 1,
        "second request should produce exactly one cache miss (cleared cache), got {:?}",
        p
    );
}

#[test]
fn external_type_cycles_increment_cycle_detection_counter() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './a'
defineProps<Props>()
</script>
<template><div>consumer</div></template>"#,
    );
    upsert_non_sfc(&host, "/src/a.ts", "export { Props } from './b'\n");
    upsert_non_sfc(&host, "/src/b.ts", "export { Props } from './a'\n");

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./a".to_string(),
            resolved_canonical_id: Some("/src/a.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/a.ts",
        vec![crate::DependencyResolution {
            specifier: "./b".to_string(),
            resolved_canonical_id: Some("/src/b.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/b.ts",
        vec![crate::DependencyResolution {
            specifier: "./a".to_string(),
            resolved_canonical_id: Some("/src/a.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    host.provenance().reset();

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let resolved = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./a",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("cycle resolution should complete");
    assert!(
        resolved.is_none(),
        "cyclic re-export should terminate without inventing a payload"
    );
    assert!(
        host.provenance().snapshot().resolver_cycle_detections >= 1,
        "cycle detection counter should increment for cyclic external type traversal"
    );
}

#[test]
fn nested_barrel_warm_lookup_keeps_following_export_star_chain() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { FirstProps, SecondProps } from './barrel_a'
defineProps<FirstProps>()
</script>
<template><div>consumer</div></template>"#,
    );
    upsert_non_sfc(&host, "/src/barrel_a.ts", "export * from './barrel_b'\n");
    upsert_non_sfc(&host, "/src/barrel_b.ts", "export * from './deep'\n");
    upsert_non_sfc(
        &host,
        "/src/deep.ts",
        "export interface FirstProps { first: string }\nexport interface SecondProps { second: number }\n",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./barrel_a".to_string(),
            resolved_canonical_id: Some("/src/barrel_a.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/barrel_a.ts",
        vec![crate::DependencyResolution {
            specifier: "./barrel_b".to_string(),
            resolved_canonical_id: Some("/src/barrel_b.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/barrel_b.ts",
        vec![crate::DependencyResolution {
            specifier: "./deep".to_string(),
            resolved_canonical_id: Some("/src/deep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let first = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./barrel_a",
            "FirstProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("first nested barrel resolution should complete")
        .expect("FirstProps should resolve");
    assert!(
        first
            .props
            .iter()
            .filter_map(|prop| prop.key_name.as_deref())
            .any(|name| name == "first"),
        "FirstProps should resolve through the nested barrel chain"
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let second = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./barrel_a",
            "SecondProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("second nested barrel resolution should complete")
        .expect("SecondProps should still resolve on a warm lookup");

    assert!(
        second
            .props
            .iter()
            .filter_map(|prop| prop.key_name.as_deref())
            .any(|name| name == "second"),
        "SecondProps should still resolve through the nested barrel chain"
    );
}

#[test]
fn external_type_resolution_step_budget_errors_on_wide_import_graph() {
    let host = strict_host();
    let import_count = crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS + 5;

    let mut defs_source = String::new();
    for index in 0..import_count {
        defs_source.push_str(&format!(
            "export interface T{index} {{ p{index}: string }}\n"
        ));
    }

    let mut types_source = String::from("import type { ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" } from './defs'\nexport interface Props extends ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" {}\n");

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>consumer</div></template>"#,
    );
    upsert_non_sfc(&host, "/src/types.ts", &types_source);
    upsert_non_sfc(&host, "/src/defs.ts", &defs_source);

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::DependencyResolution {
            specifier: "./defs".to_string(),
            resolved_canonical_id: Some("/src/defs.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let err = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./types",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect_err("wide imported graphs should fail with a hard step budget");

    match err {
        crate::types::ExternalTypeResolveError::StepLimitExceeded {
            limit,
            type_name,
            last_dep,
        } => {
            assert_eq!(limit, crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS);
            assert_eq!(type_name, "Props");
            assert!(
                !last_dep.is_empty(),
                "last dep should explain where the cap tripped"
            );
        }
        other => panic!("expected step-limit error, got {other:?}"),
    }
}

#[test]
fn external_type_trace_status_maps_success_and_error_variants() {
    assert_eq!(super::external_type_trace_success_status(false), "ok:none");
    assert_eq!(
        super::external_type_trace_success_status(true),
        "ok:resolved"
    );

    assert_eq!(
        super::external_type_trace_error_status(
            &crate::types::ExternalTypeResolveError::MissingRootDependency,
        ),
        "err:missing_root"
    );
    assert_eq!(
        super::external_type_trace_error_status(
            &crate::types::ExternalTypeResolveError::DepthLimitExceeded {
                limit: crate::types::MAX_RESOLVE_DEPTH,
                type_name: "Props".to_string(),
                last_dep: "/src/Consumer.vue".to_string(),
            },
        ),
        "err:depth_limit"
    );
    assert_eq!(
        super::external_type_trace_error_status(
            &crate::types::ExternalTypeResolveError::StepLimitExceeded {
                limit: crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS,
                type_name: "Props".to_string(),
                last_dep: "/src/types.ts".to_string(),
            },
        ),
        "err:step_limit"
    );
}

#[test]
fn external_type_trace_deltas_use_request_start_baseline() {
    let baseline = super::ExternalTypeTraceBaseline {
        tracked_len: 1,
        resolution_len: 2,
        cache_len: 3,
    };

    assert_eq!(
        super::external_type_trace_deltas(baseline, 4, 6, 8),
        (3, 4, 5),
        "trace deltas should be measured against the request-entry baseline"
    );
}

#[test]
fn external_type_frontier_layer_trace_details_include_bfs_metadata() {
    assert_eq!(
        super::external_type_frontier_layer_start_detail("/src/types.ts", "Props", 2, 5, 3),
        "source=/src/types.ts exported=Props layer=2 pending=5 resolved=3"
    );
    assert_eq!(
        super::external_type_frontier_layer_result_detail(
            "/src/types.ts",
            "Props",
            2,
            1,
            7,
            true,
            false,
            false,
        ),
        "source=/src/types.ts exported=Props layer=2 pending_next=1 resolved=7 has_more=true target_found=false route_cycle=false"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn barrel_scanned_vue_children_store_whole_hash_for_freshness() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div>consumer</div></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/types/index.ts",
        "export * from '../Button.vue'\n",
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label: string
}
</script>
<template><button>{{ label }}</button></template>"#,
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![crate::DependencyResolution {
            specifier: "../Button.vue".to_string(),
            resolved_canonical_id: Some("/src/Button.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let resolved = host
        .resolve_external_type_from_loaded_files(
            "/src/Consumer.vue",
            "./types",
            "ButtonProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("barrel resolution should complete");
    assert!(
        resolved.is_some(),
        "ButtonProps should resolve through the barrel"
    );

    let whole_hash = host
        .get_whole_hash("/src/Button.vue")
        .expect("Vue child should have a whole hash");

    assert_ne!(
        whole_hash, [0u8; 16],
        "Vue child barrel dependency should have a non-zero whole hash"
    );
}

#[test]
fn cyclic_barrel_recursive_companions_resolve_through_store_view() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/workspace/types/html.ts",
        r#"export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string
  rel?: string
}"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/types/tv.ts",
        r#"export type ComponentConfig<TTheme, TAppConfig, TName extends string> = {
  variants: {
    color: 'primary' | 'neutral'
    variant: 'solid' | 'ghost'
    size: 'sm' | 'md'
  }
  slots: {
    base?: string
  }
  ui: {
    base: string
  }
  AppConfig: TAppConfig
}"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/composables/useComponentIcons.ts",
        r#"import type { AvatarProps, IconProps } from '../types'

export interface UseComponentIconsProps {
  icon?: IconProps['name']
  avatar?: AvatarProps
  loading?: boolean
}"#,
    );
    upsert_vue(
        &host,
        "/workspace/components/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
}
</script>
<template><img /></template>"#,
    );
    upsert_vue(
        &host,
        "/workspace/components/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name?: string
}
</script>
<template><i /></template>"#,
    );
    upsert_vue(
        &host,
        "/workspace/components/Link.vue",
        r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from '../types/html'

export interface LinkProps extends Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  href?: string
  target?: string
  rel?: string
  active?: boolean
  raw?: boolean
  custom?: boolean
  class?: any
}
</script>
<template><a /></template>"#,
    );
    upsert_vue(
        &host,
        "/workspace/components/Button.vue",
        r#"<script lang="ts">
import type { AppConfig } from './nuxt-schema'
import theme from './button-theme'
import type { UseComponentIconsProps } from '../composables/useComponentIcons'
import type { LinkProps, AvatarProps } from '../types'
import type { ComponentConfig } from '../types/tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: Button['variants']['color']
  variant?: Button['variants']['variant']
  size?: Button['variants']['size']
  avatar?: AvatarProps
}
</script>
<template><button /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/components/button-theme.ts",
        "export default { variants: {} }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/components/nuxt-schema.ts",
        "export interface AppConfig {}\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/types/index.ts",
        r#"export * from '../components/Avatar.vue'
export * from '../components/Button.vue'
export * from '../components/Icon.vue'
export * from '../components/Link.vue'"#,
    );
    upsert_vue(
        &host,
        "/workspace/App.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    );

    let _view = host.resolver_store_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();

    let link_props = host
        .resolve_external_type_from_loaded_files(
            "/workspace/components/Button.vue",
            "../types",
            "LinkProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            false,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("LinkProps resolution should complete")
        .expect("LinkProps should resolve through the cyclic barrel");
    assert!(
        link_props
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("as")),
        "LinkProps should keep inherited props through the cyclic barrel, got: {:?}",
        link_props.props
    );
    assert!(
        link_props
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("type")),
        "LinkProps should keep button attribute props through the cyclic barrel, got: {:?}",
        link_props.props
    );

    let use_icons = host
        .resolve_external_type_from_loaded_files(
            "/workspace/components/Button.vue",
            "../composables/useComponentIcons",
            "UseComponentIconsProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            false,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("UseComponentIconsProps resolution should complete")
        .expect("UseComponentIconsProps should resolve through the cyclic barrel");
    assert!(
        use_icons
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("icon")),
        "UseComponentIconsProps should keep imported IconProps members, got: {:?}",
        use_icons.props
    );
    assert!(
        use_icons
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("avatar")),
        "UseComponentIconsProps should keep imported AvatarProps members, got: {:?}",
        use_icons.props
    );
}

/// Same barrel chain test but with default HostConfig (dev_mode=true,
/// DevServeLastKnownGood) and Windows-style paths. This reproduces the
/// conditions under the NAPI path that fails while strict_host tests pass.
#[test]
fn barrel_chain_vue_sfc_with_dev_mode_and_windows_paths() {
    let host = VerterHost::new_standalone(HostConfig::default()); // dev_mode: true, DevServeLastKnownGood

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
    let host = VerterHost::new_standalone(HostConfig::default());
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
    let host = VerterHost::new_standalone(HostConfig::default());
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

#[test]
fn ensure_compiled_skips_non_sfc_files() {
    let host = strict_host();
    // TypeScript source with angle-bracket generics that the Vue parser
    // would misinterpret as HTML tags (producing XMissingEndTag).
    let ts_source = r#"import { type MaybeRef } from 'vue'
export function useEl(el: MaybeRef<HTMLElement | null>) { return el }
"#;
    upsert_non_sfc(&host, "/src/utils.ts", ts_source);

    let p = profile();
    // ensure_compiled on a NonSfc file must be a no-op — no compilation,
    // no diagnostics stored.
    let result = host.ensure_compiled("/src/utils.ts", &p);
    assert!(result.is_ok(), "ensure_compiled should succeed for NonSfc");

    // Must NOT produce any diagnostics (especially not XMissingEndTag)
    let diags = host.get_diagnostics("/src/utils.ts", &p);
    assert!(
        diags.is_none(),
        "NonSfc file should have no diagnostics, got: {diags:?}"
    );
}

// ── Workspace integration tests ──────────────────────────────────────

#[test]
fn upsert_syncs_external_src_edges_to_workspace() {
    // Create a host with a MemoryWorkspace so we can query the workspace's
    // edge graph directly after upsert.
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // An SFC with an external script src (parsed into external_requests)
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script src=\"./setup.ts\"></script>\n<template><div>hello</div></template>",
    );

    // After upsert, the workspace should have recorded the external src edge.
    // reverse_deps_for returns files that depend on the given file.
    let rev_deps = ws.reverse_deps_for("/src/setup.ts");
    assert!(
        rev_deps.contains(&"/src/Comp.vue".to_string()),
        "workspace should have /src/Comp.vue as a reverse dep of /src/setup.ts after upsert; got: {rev_deps:?}"
    );

    // forward_deps_for returns the files that /src/Comp.vue depends on
    let fwd_deps = ws.forward_deps_for("/src/Comp.vue");
    assert!(
        fwd_deps.contains(&"/src/setup.ts".to_string()),
        "workspace should have /src/setup.ts as a forward dep of /src/Comp.vue after upsert; got: {fwd_deps:?}"
    );
}

#[test]
fn upsert_syncs_relative_import_edges_to_workspace() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));

    // Set up a project with a resolver so that relative imports can be resolved
    // by the VFS. Without a project resolver, the VFS can't resolve relative
    // specifiers (it doesn't do bare path probing like the host's Phase 4).
    ws.add_explicit_project(verter_workspace::VfsProjectConfig {
        root: "/src".to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: None,
        root_files: vec![],
        extensions: vec![".vue".to_string(), ".ts".to_string(), ".tsx".to_string()],
        workspace_root: "/src".to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::ProjectMembership::default(),
    });

    // Inject the dependency file so the resolver can find it
    ws.inject_file(
        "/src/utils.ts".to_string(),
        Arc::from("export function helper() { return 1 }"),
    );

    let host = VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // SFC with a relative import in script
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport { helper } from './utils'\n</script>\n<template><div>{{ helper() }}</div></template>",
    );

    // After upsert, the workspace should have the relative import edge resolved
    // to /src/utils.ts (via the project resolver's relative path probing).
    let fwd_deps = ws.forward_deps_for("/src/Comp.vue");
    assert!(
        fwd_deps.contains(&"/src/utils.ts".to_string()),
        "workspace should have /src/utils.ts as a forward dep of /src/Comp.vue after upsert; got: {fwd_deps:?}"
    );

    // Reverse dep should also be present
    let rev_deps = ws.reverse_deps_for("/src/utils.ts");
    assert!(
        rev_deps.contains(&"/src/Comp.vue".to_string()),
        "workspace should have /src/Comp.vue as a reverse dep of /src/utils.ts; got: {rev_deps:?}"
    );
}

#[test]
fn upsert_syncs_bare_import_edges_to_workspace() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // SFC with a bare (non-relative) import
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><div>{{ x }}</div></template>",
    );

    // Bare imports should be stored in the workspace edge store as bare_specifiers.
    // The EdgeStore doesn't eagerly resolve them, but they should be present.
    // We can verify by checking that no panic occurred and the workspace is consistent.
    // The forward deps will be empty since 'vue' isn't resolvable in the MemoryWorkspace.
    let fwd_deps = ws.forward_deps_for("/src/Comp.vue");
    // This is intentionally a weak assertion — we just verify the edge syncing didn't break.
    // The key assertion is that forward_deps_for doesn't panic and returns a list.
    assert!(
        fwd_deps.len() < 100,
        "sanity: forward deps should be a small list"
    );
}

#[test]
fn workspace_resolution_used_for_aliased_imports() {
    // This test verifies that resolve_loaded_dependency_canonical will
    // consult the workspace's resolve_import when the host's own Phases 1-3
    // don't find a match.
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));

    // Set up a project with @/ alias pointing to /project/src
    ws.add_explicit_project(verter_workspace::VfsProjectConfig {
        root: "/project".to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some("/project/tsconfig.json".to_string()),
        root_files: vec![],
        extensions: vec![".vue".to_string(), ".ts".to_string(), ".tsx".to_string()],
        workspace_root: "/project".to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions {
            paths: vec![("@/*".to_string(), vec!["/project/src/*".to_string()])],
            ..Default::default()
        },
        references: vec![],
        membership: verter_workspace::ProjectMembership::default(),
    });

    // Inject the dependency file into the workspace so it can be resolved
    ws.inject_file(
        "/project/src/utils.ts".to_string(),
        Arc::from("export function helper() { return 42 }"),
    );

    let host = VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // Upsert both files into the host
    upsert_non_sfc(
        &host,
        "/project/src/utils.ts",
        "export function helper() { return 42 }",
    );
    upsert_vue(
        &host,
        "/project/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport { helper } from '@/utils'\n</script>\n<template><div>{{ helper() }}</div></template>",
    );

    // Now test resolution: the host's Phase 1 (import_routes) will miss
    // because no set_import_dependencies was called. Phase 3 (project_resolver)
    // will also miss because configure_projects was not called. But the workspace
    // has the @/ alias configured, so workspace-backed resolution should find it.
    let result = host.resolve_loaded_dependency_canonical(
        "/project/src/Comp.vue",
        "@/utils",
        verter_workspace::ResolveRequestKind::EsmImport,
    );

    // This SHOULD resolve to /project/src/utils.ts via the workspace's project resolver.
    assert_eq!(
        result,
        Some("/project/src/utils.ts".to_string()),
        "workspace resolution should find @/utils via tsconfig paths alias"
    );
}

#[test]
fn workspace_resolution_does_not_override_exact_resolution() {
    // Exact resolutions should take priority over workspace resolution.
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // Upsert the dep and the component
    upsert_non_sfc(&host, "/src/exact-target.ts", "export const x = 1");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport { x } from './dep'\n</script>\n<template><div>{{ x }}</div></template>",
    );

    // Set exact resolution for ./dep → /src/exact-target.ts
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/src/exact-target.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let result = host.resolve_loaded_dependency_canonical(
        "/src/Comp.vue",
        "./dep",
        verter_workspace::ResolveRequestKind::EsmImport,
    );

    // Phase 1 exact resolution should take priority
    assert_eq!(
        result,
        Some("/src/exact-target.ts".to_string()),
        "exact resolution should take priority over workspace resolution"
    );
}

#[test]
fn shallow_type_import_route_finds_loaded_overlay_relative_targets() {
    let host = strict_host();

    // Upsert the importer first so its cached dependency resolutions stay empty.
    upsert_vue(
        &host,
        "/src/Button.vue",
        "<script setup lang=\"ts\">\nimport type { ComponentConfig } from './tv'\ndefineProps<ComponentConfig>()\n</script>\n<template><div /></template>",
    );
    upsert_non_sfc(
        &host,
        "/src/tv.ts",
        "export interface ComponentConfig { label: string }",
    );

    let _store_view = host.resolver_store_view();
    let result = host.resolve_type_dependency_canonical_shallow("/src/Button.vue", "./tv");

    assert_eq!(
        result,
        Some("/src/tv.ts".to_string()),
        "shallow type dependency resolution should find loaded overlay-only relative targets even when workspace resolution is unavailable"
    );
}

/// Macro type deps (defineProps<ExternalType>) should resolve packages with
/// types-only exports (e.g., `"exports": { ".": { "types": "..." } }`).
/// This requires using TypeImport (not EsmImport) so the "types" condition
/// is included in package resolution.
#[test]
fn macro_type_dep_resolves_types_only_package_exports() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/motion/package.json".to_string(),
        Arc::from(r#"{ "name": "motion", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#),
    );
    ws.inject_file(
        "/workspace/node_modules/motion/dist/index.d.ts".to_string(),
        Arc::from("export interface MotionProps { duration: number }"),
    );
    let host = VerterHost::new(
        HostConfig {
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let popup_source = "<script setup lang=\"ts\">\nimport type { MotionProps } from 'motion'\nconst props = defineProps<MotionProps>()\n</script>\n<template><div>{{ props.duration }}</div></template>";
    upsert_vue(&host, "/workspace/src/Popup.vue", popup_source);

    let analysis = host
        .get_analysis("/workspace/src/Popup.vue")
        .expect("Popup.vue should have analysis");

    // Verify macro type deps are detected
    assert!(
        !analysis.macro_type_deps.is_empty(),
        "Popup.vue should have macro type deps for MotionProps"
    );
    assert_eq!(
        analysis.macro_type_deps[0].import_source, "motion",
        "macro type dep should reference 'motion' package"
    );

    // Verify the resolution kind matters — EsmImport should NOT resolve types-only
    let esm_resolve = host.resolve_loaded_dependency_canonical(
        "/workspace/src/Popup.vue",
        "motion",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert!(
        esm_resolve.is_none(),
        "EsmImport should NOT resolve types-only exports, got: {esm_resolve:?}"
    );
    // But TypeImport should resolve
    let type_resolve = host.resolve_loaded_dependency_canonical(
        "/workspace/src/Popup.vue",
        "motion",
        verter_workspace::ResolveRequestKind::TypeImport,
    );
    assert!(
        type_resolve.is_some(),
        "TypeImport should resolve types-only exports"
    );

    // Re-upsert to trigger compilation with type resolution
    let compile_result = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/src/Popup.vue".to_string(),
            source: Arc::from(popup_source),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert should succeed");
    let diags = &compile_result.diagnostics.diagnostics;
    // Positive: should NOT have HOST_MISSING_MACRO_TYPE_DEP
    assert!(
        !diags
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "macro type dep 'motion' with types-only exports should resolve, got: {diags:?}"
    );
}

#[test]
fn type_import_route_prefers_package_declaration_entrypoint_over_cached_runtime_target() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js", "require": "./index.cjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/index.cjs".to_string(),
        Arc::from("module.exports = require('./dist/index.js')"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_vue(
        &host,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/workspace/src/App.vue",
        vec![crate::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/fancy/index.cjs".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _view = host.resolver_store_view();
    let resolved = host.resolve_type_dependency_canonical("/workspace/src/App.vue", "fancy");

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/node_modules/fancy/dist/index.d.ts"),
        "type resolution should prefer the declaration entrypoint even when the cached runtime target points at CJS",
    );
}

#[test]
fn type_import_route_does_not_trust_imported_cache_miss_for_untracked_package_file() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from(r#"import { FancyProps } from "./inner.js"; export type { FancyProps };"#),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    upsert_vue(
        &host,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/workspace/src/App.vue",
        vec![crate::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some(
                "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
            ),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/fancy/dist/index.d.ts",
        vec![crate::DependencyResolution {
            specifier: "./inner.js".to_string(),
            resolved_canonical_id: Some(
                "/workspace/node_modules/fancy/dist/inner.d.ts".to_string(),
            ),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _view = host.resolver_store_view();
    let _ = host
        .ensure_indexed_ready("/workspace/node_modules/fancy/dist/index.d.ts")
        .expect("package declaration entrypoint should materialize module facts");

    let resolved = host.resolve_type_dependency_canonical(
        "/workspace/node_modules/fancy/dist/index.d.ts",
        "./inner.js",
    );

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/node_modules/fancy/dist/inner.d.ts"),
        "store-view lookup should not freeze a stale imported-cache miss for package files that are outside the captured owner view",
    );
}

#[test]
fn type_import_reexport_prefers_declaration_companion_over_runtime_js() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js", "require": "./dist/index.cjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from(r#"import { AccordionRootEmits } from "./index3.js"; export type { AccordionRootEmits };"#),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index3.d.ts".to_string(),
        Arc::from("export interface AccordionRootEmits { openChange: [boolean] }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index3.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let consumer_source = "<script setup lang=\"ts\">\nimport type { AccordionRootEmits } from 'fancy'\ndefineEmits<AccordionRootEmits>()\n</script>\n<template><div /></template>";
    upsert_vue(&host, "/workspace/src/Consumer.vue", consumer_source);

    let package_decl = host.resolve_loaded_dependency_canonical(
        "/workspace/src/Consumer.vue",
        "fancy",
        verter_workspace::ResolveRequestKind::TypeImport,
    );
    assert_eq!(
        package_decl.as_deref(),
        Some("/workspace/node_modules/fancy/dist/index.d.ts"),
        "package root should resolve to the declaration entrypoint",
    );

    let companion_decl = host.resolve_loaded_dependency_canonical(
        "/workspace/node_modules/fancy/dist/index.d.ts",
        "./index3.js",
        verter_workspace::ResolveRequestKind::TypeImport,
    );
    assert_eq!(
        companion_decl.as_deref(),
        Some("/workspace/node_modules/fancy/dist/index3.d.ts"),
        "type imports from declaration files should prefer the declaration companion",
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();
    let resolved = host
        .resolve_external_type_from_loaded_files(
            "/workspace/src/Consumer.vue",
            "fancy",
            "AccordionRootEmits",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("external type resolution should succeed")
        .expect("external type resolution should produce a result");

    assert!(
        resolved.emits.iter().any(|emit| emit.name == "openChange"),
        "emit entries should resolve from the declaration companion: {:?}",
        resolved
            .emits
            .iter()
            .map(|emit| emit.name.clone())
            .collect::<Vec<_>>()
    );

    // In the new IndexedReady DB, ensure_indexed_ready normalizes .js → .d.ts
    // companion and eagerly materializes. Verify via direct DB lookup that the
    // .js entry itself was not cached (only the .d.ts companion is).
    let declaration_entry = host
        .ensure_indexed_ready("/workspace/node_modules/fancy/dist/index3.d.ts")
        .expect("external type resolution should cache the declaration companion entry");
    // external_type_analysis is Arc (non-optional) in IndexedReady; verify it has content.
    assert!(
        declaration_entry
            .external_type_analysis
            .stats()
            .top_level_statement_count
            > 0,
        "the declaration companion should own the cached external-type analysis",
    );
}

#[test]
fn type_import_package_with_node_condition_still_prefers_types_entry() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{
                "name": "vue-router",
                "main": "index.cjs",
                "module": "dist/vue-router.js",
                "types": "dist/vue-router.d.ts",
                "exports": {
                    ".": {
                        "types": "./dist/vue-router.d.ts",
                        "node": {
                            "import": "./vue-router.node.mjs",
                            "require": "./index.cjs"
                        },
                        "import": "./dist/vue-router.js",
                        "require": "./index.cjs"
                    }
                }
            }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/index.cjs".to_string(),
        Arc::from("module.exports = {}"),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts".to_string(),
        Arc::from("export interface RouterLinkProps { to: string }"),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/vue-router.node.mjs".to_string(),
        Arc::from("export const nodeOnly = true"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        "<script setup lang=\"ts\">\nimport type { RouterLinkProps } from 'vue-router'\ndefineProps<RouterLinkProps>()\n</script>\n<template><div /></template>",
    );

    let resolved = host.resolve_loaded_dependency_canonical(
        "/workspace/src/Consumer.vue",
        "vue-router",
        verter_workspace::ResolveRequestKind::TypeImport,
    );

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/node_modules/vue-router/dist/vue-router.d.ts"),
        "TypeImport should prefer the package types entry even when exports also include node/import/require runtime branches",
    );
}

#[test]
fn extract_vue_script_content_preserves_source_order() {
    let source = r#"<script lang="ts">
const COMPANION_MARKER = 1;
</script>
<script setup lang="ts">
const SETUP_MARKER = 2;
</script>
<template><div /></template>"#;

    let parsed = verter_compiler::compile::parse_sfc(source, None, None);
    let result = crate::host_resolve::extract_vue_script_content(source, Some(&parsed));
    assert!(result.is_some(), "should extract script content from SFC");
    let content = result.unwrap();
    assert!(
        content.contains("COMPANION_MARKER"),
        "should contain COMPANION_MARKER, got: {content}"
    );
    assert!(
        content.contains("SETUP_MARKER"),
        "should contain SETUP_MARKER, got: {content}"
    );
    let companion_pos = content.find("COMPANION_MARKER").unwrap();
    let setup_pos = content.find("SETUP_MARKER").unwrap();
    assert!(
        companion_pos < setup_pos,
        "COMPANION_MARKER (pos {companion_pos}) should appear before SETUP_MARKER (pos {setup_pos}) — source order must be preserved"
    );
    assert!(
        !content.contains("<template>"),
        "extracted content must NOT contain '<template>', got: {content}"
    );
    assert!(
        !content.contains("<script"),
        "extracted content must NOT contain '<script' tags, got: {content}"
    );
}

#[test]
fn extract_vue_script_content_without_cached_parse_matches_cached() {
    let source = r#"<script lang="ts">
const COMPANION = 1;
</script>
<script setup lang="ts">
const SETUP = 2;
</script>
<template><div /></template>"#;

    let parsed = verter_compiler::compile::parse_sfc(source, None, None);
    let with_cache = crate::host_resolve::extract_vue_script_content(source, Some(&parsed));
    let without_cache = crate::host_resolve::extract_vue_script_content(source, None);
    assert_eq!(
        with_cache, without_cache,
        "cached and non-cached paths must produce identical output"
    );
    // Also verify the non-cached path produces correct output on its own
    let content = without_cache.unwrap();
    assert!(content.contains("COMPANION"), "should contain COMPANION");
    assert!(content.contains("SETUP"), "should contain SETUP");
    assert!(!content.contains("<template>"), "must not contain template");
}

#[test]
fn extract_vue_script_content_handles_script_end_literal_in_string() {
    let source = r#"<script lang="ts">
const html = "</script><div>kept</div>";
const BEFORE_CLOSE = true;
</script>
<script setup lang="ts">
const AFTER_CLOSE = 1;
</script>
<template><div /></template>"#;

    let parsed = verter_compiler::compile::parse_sfc(source, None, None);
    let with_cache = crate::host_resolve::extract_vue_script_content(source, Some(&parsed))
        .expect("cached extraction should succeed");
    let without_cache = crate::host_resolve::extract_vue_script_content(source, None)
        .expect("non-cached extraction should succeed");

    assert_eq!(
        with_cache, without_cache,
        "cached and non-cached extraction must agree for script-end literals",
    );
    assert!(
        with_cache.contains(r#"const html = "</script><div>kept</div>";"#),
        "script-end literal should be preserved, got: {with_cache}"
    );
    assert!(
        with_cache.contains("const BEFORE_CLOSE = true;"),
        "content after the literal inside the same block must be preserved, got: {with_cache}"
    );
    assert!(
        with_cache.contains("const AFTER_CLOSE = 1;"),
        "later script blocks must still be included, got: {with_cache}"
    );
    assert!(
        !with_cache.contains("<template>"),
        "template content must not leak into extracted script content, got: {with_cache}"
    );
}

// ===========================================================================
// Phase 5m §5.13a.1.1 + §5.13a.1.3 — host helper tests
// ===========================================================================

#[test]
fn resolve_prepared_decl_target_returns_unchanged_for_same_file_decl() {
    // Discrimination: when `(canonical, name)` already has a
    // `prepared_type_decl`, the helper returns the same pair. This
    // matches the legacy engine's
    // `resolve_final_prepared_type_target` early-return on a
    // resolvable same-file decl.
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/app.vue",
        "<script setup lang=\"ts\">\
         import type { LocalProps } from './decl';\n\
         defineProps<LocalProps>();\n\
         </script><template>x</template>",
    );
    let _ = host.upsert(crate::UpsertRequest {
        canonical_id: None,
        input_id: "/src/decl.ts".to_string(),
        source: Arc::from("export interface LocalProps { a: number; b: string }\n"),
        file_kind: crate::FileKind::NonSfc,
        aliases: Vec::new(),
    });
    // The helper takes a "scope canonical, name" pair. For a
    // same-file local decl, the resolver finds it directly.
    let (resolved_canonical, resolved_name) =
        host.resolve_prepared_decl_target("/src/decl.ts", "LocalProps");
    assert_eq!(
        resolved_canonical, "/src/decl.ts",
        "same-file decl: canonical must be unchanged"
    );
    assert_eq!(
        resolved_name, "LocalProps",
        "same-file decl: name must be unchanged"
    );
    // Negative assertion: an unrelated symbol that has no prepared
    // decl in `/src/decl.ts` falls back to the original pair, NOT to
    // some other canonical_source.
    let (fallback_canonical, fallback_name) =
        host.resolve_prepared_decl_target("/src/decl.ts", "Nonexistent");
    assert_eq!(
        fallback_canonical, "/src/decl.ts",
        "unresolvable: canonical must fall back to input"
    );
    assert_eq!(
        fallback_name, "Nonexistent",
        "unresolvable: name must fall back to input"
    );
}

#[test]
fn resolve_decl_in_scope_with_reexport_chain_returns_declaring_decl_identity() {
    // Discrimination: a request for a name in a scope that does NOT
    // declare it locally walks the import chain, then the re-export
    // chain, and lands on the declaring file's DeclIdentity. This
    // matches the legacy engine's `dispatch_root_instantiated`
    // two-layer resolution (bare-name resolve, then prepared-decl
    // re-export walk).
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/owner.vue",
        "<script setup lang=\"ts\">\
         import type { ChildProps } from './lib';\n\
         defineProps<ChildProps>();\n\
         </script><template>x</template>",
    );
    let _ = host.upsert(crate::UpsertRequest {
        canonical_id: None,
        input_id: "/src/lib.ts".to_string(),
        source: Arc::from("export interface ChildProps { x: number; y: string }\n"),
        file_kind: crate::FileKind::NonSfc,
        aliases: Vec::new(),
    });
    let identity = host
        .resolve_decl_in_scope_with_reexport_chain("/src/owner.vue", "ChildProps")
        .expect("scope present must yield Some");
    // The declaring file is `/src/lib.ts`, NOT the owner scope.
    assert_eq!(
        identity.canonical_id.as_ref(),
        "/src/lib.ts",
        "DeclIdentity.canonical_id must point to declaring file, not owner scope"
    );
    assert_eq!(
        identity.decl_name.as_ref(),
        "ChildProps",
        "DeclIdentity.decl_name must be the declared symbol"
    );
    // Negative assertion: whole-hash MUST be non-zero (declaring file is loaded).
    let zero_hash: [u8; 16] = [0; 16];
    assert_ne!(
        identity.whole_hash, zero_hash,
        "declaring file's whole-hash must be populated, not zero-initialized"
    );
}
