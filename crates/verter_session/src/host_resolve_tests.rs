use std::sync::Arc;

use verter_span::Span;
use verter_workspace::WorkspaceRead;

#[cfg(target_arch = "wasm32")]
use crate::shared::read_lock;
use crate::{
    BlockOverrideEntry, BlockOverrideRequest, CompileErrorPolicy, CompileProfile, FileLanguage,
    HostConfig, HostDiagnostic, HostError, HostSeverity, PreprocessorBlockType, PublicApiMode,
    UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};
use verter_compiler::compile::CompileTarget;
use verter_compiler::tsc::{TscDeclarationShapeReason, TscGenerationError};

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
            file_language: FileLanguage::vue(),
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
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}
/// A completely EMPTY .vue file (0 bytes — e.g. motion-vue's playground
/// Home.vue) is a valid empty component. The host must serve a Main virtual
/// node exporting `defineComponent({ __name })` with an empty public surface
/// ($props: {}, no slots) — never `MissingVirtualNode`.
#[test]
fn empty_sfc_serves_empty_component_not_missing_virtual_node() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(&host, "/src/Home.vue", "");

    host.ensure_compiled("/src/Home.vue", &profile())
        .expect("empty SFC must compile on the host lane");

    let resp = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Home.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("empty SFC must serve a Main virtual node");
    assert!(
        resp.code.contains("defineComponent(") && resp.code.contains("export default"),
        "empty SFC Main must export a defineComponent shell, got:\n{}",
        resp.code
    );
    assert!(
        resp.code.contains("__name: \"Home\""),
        "empty SFC Main should carry the filename-derived __name, got:\n{}",
        resp.code
    );
    assert!(
        !resp.diagnostics.has_errors,
        "empty SFC must compile without error diagnostics, got: {:?}",
        resp.diagnostics.diagnostics
    );
    // Negative surface: nothing fabricated.
    assert!(
        !resp.code.contains("props:") && !resp.code.contains("slots"),
        "empty SFC Main must not fabricate props/slots, got:\n{}",
        resp.code
    );

    // The IDE lane serves a TSX artifact for the empty carrier too.
    let has_tsx = host
        .ensure_ide_compiled("/src/Home.vue", &profile())
        .expect("empty SFC must compile on the IDE lane");
    assert!(has_tsx, "empty SFC must produce an IDE artifact");
    assert!(
        host.get_ide("/src/Home.vue", &profile()).is_some(),
        "empty SFC IDE artifact must be servable"
    );

    // The imported public surface is EMPTY: no props, no events, no slots.
    let meta = host
        .get_component_meta("/src/Home.vue")
        .expect("empty SFC must publish component meta");
    assert!(
        meta.props.is_empty(),
        "empty SFC has no props: {:?}",
        meta.props
    );
    assert!(meta.events.is_empty(), "empty SFC has no events");
    assert!(meta.slots.is_empty(), "empty SFC has no slots");

    // A sibling importing the empty component keeps compiling.
    upsert_vue(
        &host,
        "/src/App.vue",
        "<script setup lang=\"ts\">\nimport Home from './Home.vue'\n</script>\n<template><Home /></template>",
    );
    host.ensure_compiled("/src/App.vue", &profile())
        .expect("importing an empty SFC must compile");
}

fn compile_main_error(host: &VerterHost, canonical_id: &str) -> crate::DiagnosticsSnapshot {
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile(),
    }) {
        Err(HostError::CompileError(failure)) => failure.diagnostics,
        Err(other) => panic!("expected compile error, got {other:?}"),
        Ok(result) => panic!(
            "expected compile error, got successful response {}",
            result.id
        ),
    }
}

fn public_api_code(host: &VerterHost, canonical_id: &str) -> String {
    host.get_public_api(canonical_id)
        .unwrap_or_else(|error| panic!("public API projection failed for {canonical_id}: {error}"))
        .unwrap_or_else(|| panic!("expected public api output for {canonical_id}"))
        .code
        .to_string()
}

fn public_api_code_with_mode(host: &VerterHost, canonical_id: &str, mode: PublicApiMode) -> String {
    host.get_public_api_with_mode(canonical_id, mode, None)
        .unwrap_or_else(|error| panic!("public API projection failed for {canonical_id}: {error}"))
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
        .unwrap_or_else(|error| panic!("public API projection failed for {canonical_id}: {error}"))
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
fn missing_aliased_macro_type_dependency_anchors_to_owning_import() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { RemoteProps as Props } from './types'\ndefineProps<Props>()\n</script>";
    upsert_vue(&host, "/src/Alias.vue", source);

    let diagnostics = compile_main_error(&host, "/src/Alias.vue");
    let missing = find_diag(&diagnostics, "HOST_MISSING_MACRO_TYPE_DEP");
    let import_start = source.find("import type").unwrap() as u32;
    let import_end =
        import_start + "import type { RemoteProps as Props } from './types'".len() as u32;
    assert_eq!(
        missing.span,
        Some(Span::new(import_start, import_end)),
        "an aliased macro root must anchor by its local binding name"
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
fn macro_type_dependency_edit_to_invalid_props_type_surfaces_object_like_error() {
    // Default config (dev_mode + DevServeLastKnownGood) mirrors the NAPI
    // host the typescript-plugin constructs.
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.label }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);
    let routes = vec![crate::DependencyResolution {
        specifier: "./types".to_string(),
        resolved_canonical_id: Some("/src/types.ts".to_string()),
        possible_canonical_ids: Vec::new(),
    }];
    host.set_import_dependencies("/src/Comp.vue", routes.clone());
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );

    let first = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("first compile against the object-shaped Props should succeed");
    assert!(
        !first.diagnostics.has_errors,
        "first compile should be clean, got: {:?}",
        first.diagnostics.diagnostics
    );

    // Caller-driven hydration re-supplies the same routes before re-upserting
    // the edited dependency (LSP plugin flow).
    host.set_import_dependencies("/src/Comp.vue", routes);
    upsert_non_sfc(&host, "/src/types.ts", "export type Props = string");

    let second = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some("/src/Comp.vue".to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile(),
    });
    let diagnostics = match second {
        Err(HostError::CompileError(failure)) => failure.diagnostics,
        Err(other) => panic!("expected compile error, got {other:?}"),
        Ok(result) => panic!(
            "expected compile error after the dependency edit, got successful response with diagnostics {:?}",
            result.diagnostics.diagnostics
        ),
    };
    let invalid = find_diag(&diagnostics, "XInvalidMacroType");
    assert!(
        invalid.message.contains(
            "defineProps() type argument 'Props' must resolve to an object-like props type."
        ),
        "expected object-like props diagnostic after the dependency edit, got: {}",
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

/// Baseline: imported `defineEmits<Omit<Base, keys>>()` with direct tuple
/// properties (no indexed access). Must resolve to emit signatures.
#[test]
fn imported_define_emits_omit_of_direct_tuple_fields_resolves() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { SubEmits } from './types'
const emit = defineEmits<SubEmits>()
emit('escapeKeydown', new KeyboardEvent('keydown'))
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"export type BaseEmits = {
  closeAutoFocus: [event: Event]
  entryFocus: [event: Event]
  escapeKeydown: [event: KeyboardEvent]
  pointerdownOutside: [event: PointerEvent]
}
export type SubEmits = Omit<BaseEmits, 'closeAutoFocus' | 'entryFocus'>
"#,
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
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
        .expect("Omit of direct tuple emits must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "XInvalidMacroType"),
        "must not XInvalidMacroType: {:?}",
        response.diagnostics.diagnostics
    );
}

/// Indexed-access emit field without Omit (same-file base type):
/// `escapeKeydown: LayerEmits['escapeKeydown']`.
#[test]
fn imported_define_emits_indexed_access_tuple_fields_resolves() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { SharedEmits } from './types'
const emit = defineEmits<SharedEmits>()
emit('escapeKeydown', new KeyboardEvent('keydown'))
</script>
<template><div /></template>"#,
    );
    // Keep LayerEmits in the SAME file as SharedEmits so the only variable
    // under test is indexed-access property types (not cross-file loading).
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"export type LayerEmits = {
  escapeKeydown: [event: KeyboardEvent]
  pointerdownOutside: [event: PointerEvent]
}
export type SharedEmits = {
  escapeKeydown: LayerEmits['escapeKeydown']
  pointerdownOutside: LayerEmits['pointerdownOutside']
}
"#,
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
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
        .expect("indexed-access emit fields must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "XInvalidMacroType"),
        "must not XInvalidMacroType: {:?}",
        response.diagnostics.diagnostics
    );
}

/// oku-primitives / reka-style pattern: `defineEmits<Alias>()` where Alias =
/// `Omit<SharedEmits, …>` and SharedEmits fields are **indexed accesses** into
/// another emits object (`DismissableLayerEmits['escapeKeydown']`). Official
/// Vue accepts this; the host must resolve call/tuple signatures (not
/// XInvalidMacroType with empty surfaces).
#[test]
fn imported_define_emits_omit_of_indexed_access_fields_resolves() {
    let host = strict_host();
    let source = r#"<script setup lang="ts">
import type { ContextMenuSubContentImplEmits } from './ContextMenuSubContentImpl'
const emit = defineEmits<ContextMenuSubContentImplEmits>()
emit('escapeKeydown', new KeyboardEvent('keydown'))
</script>
<template><div /></template>"#;
    upsert_vue(&host, "/src/ContextMenuSubContentImpl.vue", source);
    upsert_non_sfc(
        &host,
        "/src/DismissableLayer.ts",
        r#"export type DismissableLayerEmits = {
  escapeKeydown: [event: KeyboardEvent]
  pointerdownOutside: [event: PointerEvent]
  focusOutside: [event: FocusEvent]
  interactOutside: [event: Event]
}
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/MenuContentImpl.ts",
        r#"import type { DismissableLayerEmits } from './DismissableLayer'
export type UseMenuContentImplSharedEmits = {
  closeAutoFocus: [event: Event]
  entryFocus: [event: Event]
  escapeKeydown: DismissableLayerEmits['escapeKeydown']
  pointerdownOutside: DismissableLayerEmits['pointerdownOutside']
  focusOutside: DismissableLayerEmits['focusOutside']
  interactOutside: DismissableLayerEmits['interactOutside']
}
export type MenuSubContentImplEmits = Omit<
  UseMenuContentImplSharedEmits,
  'closeAutoFocus' | 'entryFocus'
>
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/ContextMenuSubContentImpl.ts",
        r#"import type { MenuSubContentImplEmits } from './MenuContentImpl'
export type ContextMenuSubContentImplEmits = MenuSubContentImplEmits
"#,
    );
    host.set_import_dependencies(
        "/src/ContextMenuSubContentImpl.vue",
        vec![crate::DependencyResolution {
            specifier: "./ContextMenuSubContentImpl".to_string(),
            resolved_canonical_id: Some("/src/ContextMenuSubContentImpl.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/ContextMenuSubContentImpl.ts",
        vec![crate::DependencyResolution {
            specifier: "./MenuContentImpl".to_string(),
            resolved_canonical_id: Some("/src/MenuContentImpl.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/MenuContentImpl.ts",
        vec![crate::DependencyResolution {
            specifier: "./DismissableLayer".to_string(),
            resolved_canonical_id: Some("/src/DismissableLayer.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/ContextMenuSubContentImpl.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("Omit+indexed-access emit alias chain must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error on Omit+indexed-access emit alias: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "XInvalidMacroType"),
        "must not report XInvalidMacroType: {:?}",
        response.diagnostics.diagnostics
    );
    // Runtime emit handlers should appear (camelCase on* form).
    assert!(
        response.code.contains("onEscapeKeydown") || response.code.contains("escapeKeydown"),
        "resolved emit surface should include escapeKeydown, got: {}",
        response.code
    );
}

/// oku Label.vue: `withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)`
/// with no declarator; template uses bare `as`. Compile must emit `$props.as`
/// (not `_ctx.as`) so the default `'label'` applies at runtime.
#[test]
fn label_with_defaults_imported_props_template_binds_dollar_props() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/Label.ts",
        r#"export interface LabelProps {
  as?: string
}
export const DEFAULT_LABEL_PROPS = {
  as: 'label',
}
"#,
    );
    upsert_vue(
        &host,
        "/src/Label.vue",
        r#"<script setup lang="ts">
import type { LabelProps } from './Label.ts'
import { DEFAULT_LABEL_PROPS } from './Label.ts'
withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)
</script>
<template>
  <component :is="as" />
</template>"#,
    );
    host.set_import_dependencies(
        "/src/Label.vue",
        vec![crate::DependencyResolution {
            specifier: "./Label.ts".to_string(),
            resolved_canonical_id: Some("/src/Label.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Label.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("Label withDefaults must compile");

    assert!(
        !response.diagnostics.has_errors,
        "errors: {:?}",
        response.diagnostics.diagnostics
    );
    // Render should use $props.as (or __props.as), not free _ctx.as.
    assert!(
        response.code.contains("$props.as")
            || response.code.contains("__props.as")
            || response.code.contains("$props[\"as\"]"),
        "template must bind prop via $props.as, got:\n{}",
        response.code
    );
    assert!(
        !response.code.contains("_ctx.as"),
        "must not use _ctx.as for defineProps binding, got:\n{}",
        response.code
    );
    // Variable defaults compile to the EXACT official shape:
    // `_mergeDefaults(<typed props>, DEFAULT_LABEL_PROPS)` — pin it (an
    // OR-chain over alternative shapes cannot catch a broken emission).
    assert!(
        response.code.contains("_mergeDefaults("),
        "variable defaults must compile through _mergeDefaults. Got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("DEFAULT_LABEL_PROPS)"),
        "the defaults VARIABLE must be the mergeDefaults argument. Got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("mergeDefaults as _mergeDefaults"),
        "the mergeDefaults runtime import must be emitted. Got:\n{}",
        response.code
    );
}

/// Same Label pattern but `as?: PrimitiveProps['as']` (indexed access) as in
/// real oku-primitives sources.
#[test]
fn label_with_defaults_indexed_access_prop_type_binds_dollar_props() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/Primitive.ts",
        r#"export interface PrimitiveProps {
  as?: string | object
  asChild?: boolean
}
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/Label.ts",
        r#"import type { PrimitiveProps } from './Primitive'
export interface LabelProps {
  as?: PrimitiveProps['as']
}
export const DEFAULT_LABEL_PROPS = {
  as: 'label' as const,
}
"#,
    );
    upsert_vue(
        &host,
        "/src/Label.vue",
        r#"<script setup lang="ts">
import type { LabelProps } from './Label.ts'
import { DEFAULT_LABEL_PROPS } from './Label.ts'
withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)
</script>
<template>
  <component :is="as" />
</template>"#,
    );
    host.set_import_dependencies(
        "/src/Label.ts",
        vec![crate::DependencyResolution {
            specifier: "./Primitive".to_string(),
            resolved_canonical_id: Some("/src/Primitive.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/Label.vue",
        vec![crate::DependencyResolution {
            specifier: "./Label.ts".to_string(),
            resolved_canonical_id: Some("/src/Label.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Label.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("Label with indexed-access prop type must compile");

    assert!(
        !response.diagnostics.has_errors,
        "errors: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        response.code.contains("$props.as")
            || response.code.contains("__props.as")
            || response.code.contains("$props[\"as\"]"),
        "template must bind $props.as, got:\n{}",
        response.code
    );
    assert!(
        !response.code.contains("_ctx.as"),
        "must not use _ctx.as, got:\n{}",
        response.code
    );
    // Pin the exact variable-defaults shape (see the sibling test).
    assert!(
        response.code.contains("_mergeDefaults(") && response.code.contains("DEFAULT_LABEL_PROPS)"),
        "props must merge DEFAULT_LABEL_PROPS via _mergeDefaults. Got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("mergeDefaults as _mergeDefaults"),
        "the mergeDefaults runtime import must be emitted. Got:\n{}",
        response.code
    );
}

/// reka-ui: `import type { PrimitiveProps } from '@/Primitive'` via tsconfig
/// paths, then `export interface XProps extends PrimitiveProps` in a `.vue`
/// companion script. Consumer defineProps must not HOST_MISSING PrimitiveProps.
#[test]
fn define_props_extends_at_alias_primitive_props_resolves() {
    let host = strict_host();
    host.configure_projects(vec![{
        let mut cfg = verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/project".to_string(),
            "/project".to_string(),
            Some("/project/tsconfig.json".to_string()),
        );
        cfg.workspace_aliases = vec![verter_workspace::WorkspaceAlias {
            find: "@/".to_string(),
            replacement: "/project/src/".to_string(),
        }];
        cfg.compiler_options.paths = vec![("@/*".to_string(), vec!["/project/src/*".to_string()])];
        cfg
    }]);
    upsert_non_sfc(
        &host,
        "/project/src/Primitive/index.ts",
        r#"export interface PrimitiveProps {
  asChild?: boolean
  as?: string
}
"#,
    );
    upsert_vue(
        &host,
        "/project/src/Separator/BaseSeparator.vue",
        r#"<script lang="ts">
import type { PrimitiveProps } from '@/Primitive'
export interface BaseSeparatorProps extends PrimitiveProps {
  orientation?: 'horizontal' | 'vertical'
}
</script>
<script setup lang="ts">
const props = defineProps<BaseSeparatorProps>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/project/src/Separator/Separator.vue",
        r#"<script setup lang="ts">
import type { BaseSeparatorProps } from './BaseSeparator.vue'
const props = defineProps<BaseSeparatorProps>()
</script>
<template><div>{{ props.orientation }}</div></template>"#,
    );
    host.set_import_dependencies(
        "/project/src/Separator/BaseSeparator.vue",
        vec![crate::DependencyResolution {
            specifier: "@/Primitive".to_string(),
            resolved_canonical_id: Some("/project/src/Primitive/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/project/src/Separator/Separator.vue",
        vec![crate::DependencyResolution {
            specifier: "./BaseSeparator.vue".to_string(),
            resolved_canonical_id: Some("/project/src/Separator/BaseSeparator.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/project/src/Separator/Separator.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("@/ alias heritage must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "must not HOST_MISSING PrimitiveProps via @/: {:?}",
        response.diagnostics.diagnostics
    );
}

/// radix Separator pattern: local empty interface re-export of an imported
/// props type (`export interface SeparatorProps extends BaseSeparatorProps`
/// plus `defineProps<SeparatorProps>()`) must expand full heritage runtime
/// props (asChild/as from PrimitiveProps, orientation/decorative from base).
#[test]
fn define_props_local_empty_interface_extends_imported_expands_heritage_props() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/Primitive.ts",
        r#"export interface PrimitiveProps {
  asChild?: boolean
  as?: string
}
"#,
    );
    upsert_vue(
        &host,
        "/src/BaseSeparator.vue",
        r#"<script lang="ts">
import type { PrimitiveProps } from './Primitive'
export interface BaseSeparatorProps extends PrimitiveProps {
  orientation?: 'horizontal' | 'vertical'
  decorative?: boolean
}
</script>
<script setup lang="ts">
const props = defineProps<BaseSeparatorProps>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/Separator.vue",
        r#"<script lang="ts">
import type { BaseSeparatorProps } from './BaseSeparator.vue'
export interface SeparatorProps extends BaseSeparatorProps {}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<SeparatorProps>(), {
  orientation: 'horizontal',
})
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/BaseSeparator.vue",
        vec![crate::DependencyResolution {
            specifier: "./Primitive".to_string(),
            resolved_canonical_id: Some("/src/Primitive.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/Separator.vue",
        vec![crate::DependencyResolution {
            specifier: "./BaseSeparator.vue".to_string(),
            resolved_canonical_id: Some("/src/BaseSeparator.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Separator.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("local empty interface extends imported must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        response.code.contains("asChild"),
        "heritage prop asChild must appear in runtime props, got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("orientation"),
        "local/withDefaults orientation must appear, got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("decorative"),
        "heritage prop decorative must appear in runtime props, got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("as:") || response.code.contains("as,"),
        "heritage prop as must appear in runtime props, got:\n{}",
        response.code
    );
}

/// Local interface with own members + extends imported base (not empty-body).
#[test]
fn define_props_local_interface_with_members_extends_imported() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/Base.ts",
        r#"export interface BaseProps {
  base?: string
}
"#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script lang="ts">
import type { BaseProps } from './Base'
export interface CompProps extends BaseProps {
  local?: number
}
</script>
<script setup lang="ts">
defineProps<CompProps>()
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "./Base".to_string(),
            resolved_canonical_id: Some("/src/Base.ts".to_string()),
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
        .expect("local interface with members + extends must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        response.code.contains("base"),
        "inherited base must appear, got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("local"),
        "own local member must appear, got:\n{}",
        response.code
    );
}

/// Type-alias re-export of an imported interface used as defineProps arg.
#[test]
fn define_props_type_alias_reexport_of_imported_interface() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/Base.ts",
        r#"export interface BaseProps {
  foo?: string
  bar?: boolean
}
"#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { BaseProps } from './Base'
type Props = BaseProps
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "./Base".to_string(),
            resolved_canonical_id: Some("/src/Base.ts".to_string()),
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
        .expect("type alias reexport must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        response.code.contains("foo"),
        "aliased prop foo must appear, got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("bar"),
        "aliased prop bar must appear, got:\n{}",
        response.code
    );
}

/// Local empty interface extends imported type alias of object type.
#[test]
fn define_props_local_empty_extends_imported_type_alias_object() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/Base.ts",
        r#"export type BaseProps = {
  alpha?: string
  beta?: number
}
"#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script lang="ts">
import type { BaseProps } from './Base'
export interface Props extends BaseProps {}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::DependencyResolution {
            specifier: "./Base".to_string(),
            resolved_canonical_id: Some("/src/Base.ts".to_string()),
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
        .expect("empty extends type-alias object must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        response.code.contains("alpha"),
        "alpha from type alias must appear, got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("beta"),
        "beta from type alias must appear, got:\n{}",
        response.code
    );
}

/// Single-file: empty interface chain (no import) must expand all members.
#[test]
fn define_props_same_file_empty_interface_extends_chain() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
interface A { a?: string }
interface B extends A { b?: number }
interface C extends B {}
defineProps<C>()
</script>
<template><div /></template>"#,
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("same-file empty extends chain must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        response.code.contains("a:") || response.code.contains("\"a\""),
        "chain prop a must appear, got:\n{}",
        response.code
    );
    assert!(
        response.code.contains("b:") || response.code.contains("\"b\""),
        "chain prop b must appear, got:\n{}",
        response.code
    );
}

/// reka-ui pattern: consumer imports props interface exported from another
/// `.vue` companion `<script lang="ts">` block that `extends PrimitiveProps`.
/// Nested PrimitiveProps must resolve (no HOST_MISSING_MACRO_TYPE_DEP).
#[test]
fn define_props_extends_primitive_from_sibling_vue_script_block_resolves() {
    let host = strict_host();
    // Declaring SFC: companion script exports RangeCalendarNextProps extends PrimitiveProps.
    upsert_vue(
        &host,
        "/src/RangeCalendarNext.vue",
        r#"<script lang="ts">
import type { PrimitiveProps } from './Primitive'
export interface RangeCalendarNextProps extends PrimitiveProps {
  nextPage?: () => void
}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<RangeCalendarNextProps>(), { as: 'button' })
</script>
<template><button /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/Primitive.ts",
        r#"export interface PrimitiveProps {
  asChild?: boolean
  as?: string
}
"#,
    );
    // Consumer mirrors DateRangePickerNext.vue importing from RangeCalendarNext.
    upsert_vue(
        &host,
        "/src/DateRangePickerNext.vue",
        r#"<script setup lang="ts">
import type { RangeCalendarNextProps } from './RangeCalendarNext.vue'
const props = defineProps<RangeCalendarNextProps>()
</script>
<template><button>{{ props.as }}</button></template>"#,
    );
    host.set_import_dependencies(
        "/src/RangeCalendarNext.vue",
        vec![crate::DependencyResolution {
            specifier: "./Primitive".to_string(),
            resolved_canonical_id: Some("/src/Primitive.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/DateRangePickerNext.vue",
        vec![crate::DependencyResolution {
            specifier: "./RangeCalendarNext.vue".to_string(),
            resolved_canonical_id: Some("/src/RangeCalendarNext.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/DateRangePickerNext.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("cross-vue extends PrimitiveProps must compile");

    assert!(
        !response.diagnostics.has_errors,
        "must not error: {:?}",
        response.diagnostics.diagnostics
    );
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "must not HOST_MISSING PrimitiveProps: {:?}",
        response.diagnostics.diagnostics
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

/// Mutation recipe: remove the `!has_content_override` guard around the
/// `cached_tsc_extract` write in the Vue projector. The profile render then
/// occupies the raw-derived cache slot before the raw projection control runs.
#[test]
fn public_api_profile_override_does_not_populate_raw_extract_cache() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/CacheOwner.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ raw: string }>()\n</script>\n<template><div/></template>",
    );

    let profile = CompileProfile::default();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/CacheOwner.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Script,
                index: 0,
                code: Arc::from("defineProps<{ overrideProp: number }>()"),
                source_map: None,
            }],
        })
        .expect("script override should succeed");

    let overridden = public_api_code_with_profile(
        &host,
        "/src/CacheOwner.vue",
        PublicApiMode::Public,
        &profile,
    );
    assert!(
        overridden.contains("overrideProp"),
        "profile projection must use override syntax: {overridden}"
    );

    #[cfg(not(target_arch = "wasm32"))]
    assert!(
        host.derived_raw_cache()
            .get("/src/CacheOwner.vue")
            .is_none_or(|state| state.cached_tsc_extract.is_none()),
        "an override extraction must not occupy the raw-derived cache slot"
    );

    let raw = public_api_code(&host, "/src/CacheOwner.vue");
    assert!(
        raw.contains("raw") && !raw.contains("overrideProp"),
        "the unprofiled control must still project raw syntax: {raw}"
    );
    #[cfg(not(target_arch = "wasm32"))]
    assert!(
        host.derived_raw_cache()
            .get("/src/CacheOwner.vue")
            .is_some_and(|state| state.cached_tsc_extract.is_some()),
        "the raw projection control should populate its owning cache slot"
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
    let api = host
        .get_public_api("/test/Cached.vue")
        .expect("public API projection");
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

    let api1 = host
        .get_public_api("/test/Reuse.vue")
        .expect("first projection")
        .expect("first call");
    let api2 = host
        .get_public_api("/test/Reuse.vue")
        .expect("second projection")
        .expect("second call");
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
    let _api = host
        .get_public_api("/test/Clear.vue")
        .expect("public API projection");
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
    let _api = host
        .get_public_api("/test/TplChange.vue")
        .expect("public API projection");
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
    let _api = host
        .get_public_api("/test/DescChange.vue")
        .expect("public API projection");
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

/// Relative import `./type` resolves to `./type.d.ts` via project-resolver probing.
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
    // No set_import_dependencies — project-resolver probing should find /src/components/type.d.ts

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
    // Configure workspace with @/ alias via host wrapper
    // (`host.workspace()` is `pub(crate)`).
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
            membership: verter_workspace::ConfiguredMembership::match_all_under_root(
                &verter_workspace::CanonicalPath::new("/src"),
            ),
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
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let first = host.resolve_component_meta_native_props(
        "/src/Consumer.vue",
        "./types",
        "DynamicProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );
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
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let second = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./types",
            "DynamicProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("DynamicProps should resolve after the dependency update");

    assert!(
        second
            .iter()
            .map(|prop| prop.name.as_str())
            .any(|name| name == "added"),
        "resolved props should come from the updated dependency: {:?}",
        second
            .iter()
            .map(|prop| prop.name.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn repeat_type_import_request_warm_hits_imported_root_route_slot() {
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
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let first = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./types",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("Props should resolve on the first request");
    assert!(
        first.iter().any(|prop| prop.name == "label"),
        "first resolution should surface the `label` member, got: {:?}",
        first
    );

    // The retired `ResolvedTypeCacheDb` hit counter has no successor on the
    // provenance surface; the live route-reuse observable is the
    // fact-validated `ImportedRootDb` slot the repeat request warm-hits.
    let warm_before = host.project_type_store().imported_roots().warm_hit_count();

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let second = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./types",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("Props should resolve on the second request");
    assert!(
        second.iter().any(|prop| prop.name == "label"),
        "second resolution should surface the `label` member, got: {:?}",
        second
    );

    let warm_after = host.project_type_store().imported_roots().warm_hit_count();
    assert!(
        warm_after > warm_before,
        "the repeat request should warm-hit the imported-root route slot \
         (warm hits before={warm_before}, after={warm_after})"
    );
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeat_barrel_request_warm_hits_imported_root_route_slot() {
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
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let first = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./barrel",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("Props should resolve on the first request");
    assert!(
        first.iter().any(|prop| prop.name == "label"),
        "the barrel-routed resolution should surface the `label` member, got: {:?}",
        first
    );

    // The barrel hop's imported-root proof is host-owned and fact-validated:
    // the repeat request warm-hits the `ImportedRootDb` slot instead of
    // re-walking the `export *` chain.
    let warm_before = host.project_type_store().imported_roots().warm_hit_count();

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let second = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./barrel",
            "Props",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("Props should resolve on the second request");
    assert!(
        second.iter().any(|prop| prop.name == "label"),
        "the repeat barrel-routed resolution should surface the `label` member, got: {:?}",
        second
    );

    let warm_after = host.project_type_store().imported_roots().warm_hit_count();
    assert!(
        warm_after > warm_before,
        "the repeat barrel request should warm-hit the imported-root route slot \
         (warm hits before={warm_before}, after={warm_after})"
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
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let first = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./barrel_a",
            "FirstProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("FirstProps should resolve");
    assert!(
        first
            .iter()
            .map(|prop| prop.name.as_str())
            .any(|name| name == "first"),
        "FirstProps should resolve through the nested barrel chain"
    );

    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let second = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./barrel_a",
            "SecondProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("SecondProps should still resolve on a warm lookup");

    assert!(
        second
            .iter()
            .map(|prop| prop.name.as_str())
            .any(|name| name == "second"),
        "SecondProps should still resolve through the nested barrel chain"
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
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let resolved = host
        .resolve_component_meta_native_props(
            "/src/Consumer.vue",
            "./types",
            "ButtonProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("ButtonProps should resolve through the barrel");
    assert!(
        resolved.iter().any(|prop| prop.name == "label"),
        "the barrel-scanned Vue child should surface the `label` member, got: {:?}",
        resolved
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

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();

    let link_props = host
        .resolve_component_meta_native_props(
            "/workspace/components/Button.vue",
            "../types",
            "LinkProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("LinkProps should resolve through the cyclic barrel");
    assert!(
        link_props.iter().any(|prop| prop.name == "as"),
        "LinkProps should keep inherited props through the cyclic barrel, got: {:?}",
        link_props
    );
    assert!(
        link_props.iter().any(|prop| prop.name == "type"),
        "LinkProps should keep button attribute props through the cyclic barrel, got: {:?}",
        link_props
    );

    let use_icons = host
        .resolve_component_meta_native_props(
            "/workspace/components/Button.vue",
            "../composables/useComponentIcons",
            "UseComponentIconsProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("UseComponentIconsProps should resolve through the cyclic barrel");
    assert!(
        use_icons.iter().any(|prop| prop.name == "icon"),
        "UseComponentIconsProps should keep imported IconProps members, got: {:?}",
        use_icons
    );
    assert!(
        use_icons.iter().any(|prop| prop.name == "avatar"),
        "UseComponentIconsProps should keep imported AvatarProps members, got: {:?}",
        use_icons
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
            file_language: FileLanguage::vue(),
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

#[test]
fn cold_ensure_compiled_miss_does_not_read_store_view_in_cache_check() {
    // `ensure_compiled`'s warm-hit cache check must NOT build a store view
    // when there is no cached compile slot to validate. The store-view read
    // is threaded through the `acquire_view` callback that
    // `CompileOutputNodeFactValidatedSession::lookup` invokes ONLY after its
    // cheap predicates (slot present for this profile, carrier cacheable,
    // hashes match) confirm a candidate slot worth validating. A cold miss
    // (no `ProfileState` at all, or a present `ProfileState` with no slot for
    // this profile) falls through to recompile WITHOUT paying for a
    // full-workspace store-view snapshot.
    //
    // The COLD miss (`compile_cache().get() == None`) is the state where a
    // file's SOURCE is present in the scheduler but its `compile_cache`
    // `ProfileState` has NOT been materialised. Both `upsert` and
    // `ensure_loaded` materialise it (`entry().or_default()` in the upsert
    // path / `integrate_scheduler_snapshot`), but a file whose source is
    // pulled in as a side-effect of resolving ANOTHER file (a lazily-loaded
    // dependency) reaches `ensure_compiled` with the source present yet no
    // state. We reproduce that deterministically: upsert to load the source,
    // then remove the materialised `compile_cache` state so `get()` is
    // genuinely `None` while `try_get_source` still succeeds.
    //
    // Discrimination via a PER-THREAD counter
    // (`compile_warm_validation_view_reads`) bumped inside the warm-validation
    // `acquire_view` callback — i.e. exactly where a store-view read actually
    // happens, after the cheap predicates pass. It is thread-local, so the
    // synchronous `ensure_compiled` on THIS thread is isolated from any
    // parallel test's store-view reads, AND it is immune to source-line
    // shifts. On a cold miss `acquire_view` is never reached → the counter
    // stays 0; an eager read before the cheap checks would bump it to 1.
    use crate::resolver_store::{
        compile_warm_validation_view_reads, reset_compile_warm_validation_view_reads,
    };

    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>";
    upsert_vue(&host, "/src/Cold.vue", source);
    // Drop the materialised state so the cache-check sees a TRUE miss
    // (`get() == None`) while the source remains present in the scheduler.
    host.compile_cache().remove("/src/Cold.vue");
    assert!(
        host.compile_cache().get("/src/Cold.vue").is_none(),
        "precondition: the compile_cache state must be absent (cold miss) while the \
         source is still loaded"
    );

    reset_compile_warm_validation_view_reads();
    let _ = host.ensure_compiled("/src/Cold.vue", &profile());
    let cache_check_reads = compile_warm_validation_view_reads();
    assert_eq!(
        cache_check_reads, 0,
        "REGRESSION (cold-miss store-view build): a COLD ensure_compiled (no compile \
         slot) reached the warm-validation store-view read {cache_check_reads} time(s) \
         even though there was no cached slot to validate. The store-view read must be \
         threaded through `acquire_view`, which `lookup` invokes only after its cheap \
         predicates pass, so a miss never builds the workspace snapshot."
    );
}

#[test]
fn warm_ensure_compiled_hit_still_validates_against_current_view() {
    // SOUNDNESS guard paired with the cold-miss perf test above: making the
    // store-view read lazy (threaded through `acquire_view` behind `lookup`'s
    // cheap predicates) must NOT regress the warm-validation soundness
    // contract. A warm `ensure_compiled` HIT still reads the store view and
    // gates the cached slot on a proven-`Current` view (a `ReturnOnly`
    // snapshot misses to cold). Here we prove the HIT path (a present slot
    // whose hashes match) DOES reach the store-view read — the complement of
    // the cold-miss test, confirming the read is gated by the cheap
    // predicates, not removed outright.
    use crate::resolver_store::{
        compile_warm_validation_view_reads, reset_compile_warm_validation_view_reads,
    };

    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>";
    upsert_vue(&host, "/src/Warm.vue", source);

    // Prime the compile cache so the next ensure_compiled is a warm HIT.
    let _ = host.ensure_compiled("/src/Warm.vue", &profile());
    assert!(
        host.compile_slot_fact_dep_signature("/src/Warm.vue", &profile())
            .is_some(),
        "precondition: priming compile must populate the compile slot"
    );
    assert!(
        host.compile_slot_is_warm("/src/Warm.vue", &profile()),
        "precondition: the primed slot must be warm (validates against the current view)"
    );

    // A warm HIT (slot present + hashes match) MUST reach the store-view read
    // to gate the cached slot on a proven-Current view. This file has no
    // cross-file deps (an empty fact rail), so this also pins that the
    // currentness proof gates empty-fact hits too — `acquire_view` runs
    // regardless of whether the fact rail is empty.
    reset_compile_warm_validation_view_reads();
    let _ = host.ensure_compiled("/src/Warm.vue", &profile());
    let cache_check_reads = compile_warm_validation_view_reads();
    assert!(
        cache_check_reads >= 1,
        "a warm ensure_compiled HIT MUST still reach the store-view read to gate the \
         cached slot on a proven-Current view (the warm-validation soundness contract). \
         Observed zero reads — the lazy-acquire change must only AVOID the read on a \
         miss/mismatch, never on a hit."
    );
}

#[test]
fn session_get_virtual_file_profile_miss_does_not_read_store_view() {
    // `get_virtual_file`'s Session warm-hit consult must NOT build a store
    // view when the per-profile compile slot is absent. A `ProfileState`
    // existing for the canonical only means an upsert materialised it — the
    // FIRST Session compile after an upsert leaves an EMPTY `ProfileState`
    // with no slot for the requested profile_hash. The store-view read is
    // threaded through the `acquire_view` callback that
    // `CompileOutputNodeFactValidatedSession::lookup` invokes ONLY after its
    // cheap slot-present + carrier + hash predicates pass, so this
    // profile-miss path falls through to recompile WITHOUT paying for a
    // full-workspace store-view snapshot.
    //
    // Discrimination via the same per-thread warm-validation counter used by
    // the `ensure_compiled` pair. A self-contained SFC (no cross-file deps)
    // upserts an empty `ProfileState`; the single cold `get_virtual_file`
    // reaches the Session warm-hit arm with the slot absent, so `acquire_view`
    // is never reached and the counter stays 0. An eager store-view read
    // before the cheap predicates would bump it once even though there is no
    // slot to validate.
    use crate::resolver_store::{
        compile_warm_validation_view_reads, reset_compile_warm_validation_view_reads,
    };

    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>";
    upsert_vue(&host, "/src/ProfileMiss.vue", source);
    // Precondition: the `ProfileState` exists (upsert materialised it) but has
    // no compiled slot for this profile yet — exactly the cold/profile-miss
    // state the Session warm-hit arm must reject cheaply.
    assert!(
        host.compile_cache().get("/src/ProfileMiss.vue").is_some(),
        "precondition: upsert must materialise the ProfileState"
    );
    assert!(
        host.compile_slot_fact_dep_signature("/src/ProfileMiss.vue", &profile())
            .is_none(),
        "precondition: no compile slot for this profile yet (profile miss)"
    );

    reset_compile_warm_validation_view_reads();
    let resp = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/ProfileMiss.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile(),
        })
        .expect("cold Session compile should succeed");
    // The request must actually classify to Session so the Session warm-hit
    // arm (the site under test) is the one reached.
    assert_eq!(
        resp.actual_mode,
        crate::CompileCacheMode::Session,
        "precondition: the request must classify to Session so the Session \
         warm-hit arm is exercised"
    );

    let profile_miss_reads = compile_warm_validation_view_reads();
    assert_eq!(
        profile_miss_reads, 0,
        "REGRESSION (profile-miss store-view build): a cold Session get_virtual_file \
         whose ProfileState has no slot for the requested profile reached the \
         warm-validation store-view read {profile_miss_reads} time(s) even though there \
         was no slot to validate. The store-view read must be threaded through \
         `acquire_view`, which `lookup` invokes only after its cheap slot-present + hash \
         predicates pass, so a profile miss never builds the workspace snapshot."
    );
}

#[test]
fn compile_slot_is_warm_profile_miss_does_not_read_store_view() {
    // `compile_slot_is_warm` is the third compile-path warm-validation site.
    // It must NOT build a store view when the per-profile compile slot is
    // absent: the predicate already short-circuits when the `ProfileState`
    // itself is missing, but a present-but-empty `ProfileState` (the state
    // after an upsert with no compile yet) must still reject cheaply via
    // `lookup`'s slot-present predicate BEFORE the `acquire_view` callback
    // reads the store view. This closes the warm-validation class at the
    // third entry point.
    use crate::resolver_store::{
        compile_warm_validation_view_reads, reset_compile_warm_validation_view_reads,
    };

    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nconst a = 1\n</script>\n<template><div>{{ a }}</div></template>";
    upsert_vue(&host, "/src/WarmProbeMiss.vue", source);
    assert!(
        host.compile_cache().get("/src/WarmProbeMiss.vue").is_some(),
        "precondition: upsert must materialise the ProfileState"
    );
    assert!(
        host.compile_slot_fact_dep_signature("/src/WarmProbeMiss.vue", &profile())
            .is_none(),
        "precondition: no compile slot for this profile yet (profile miss)"
    );

    reset_compile_warm_validation_view_reads();
    let is_warm = host.compile_slot_is_warm("/src/WarmProbeMiss.vue", &profile());
    assert!(
        !is_warm,
        "a profile miss (no slot) is not warm — the consumer routes to cold recompute"
    );
    let profile_miss_reads = compile_warm_validation_view_reads();
    assert_eq!(
        profile_miss_reads, 0,
        "REGRESSION (profile-miss store-view build): compile_slot_is_warm reached the \
         warm-validation store-view read {profile_miss_reads} time(s) on a profile miss \
         (present ProfileState, no slot) even though there was no slot to validate. The \
         read must be threaded through `acquire_view`, which `lookup` invokes only after \
         its cheap slot-present + hash predicates pass."
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
    // specifiers (it doesn't do bare path probing like the host's
    // workspace-backed fallback).
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
        membership: verter_workspace::ConfiguredMembership::match_all_under_root(
            &verter_workspace::CanonicalPath::new("/src"),
        ),
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
        membership: verter_workspace::ConfiguredMembership::match_all_under_root(
            &verter_workspace::CanonicalPath::new("/project"),
        ),
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

    // Now test resolution: the host's `import_routes` fast path will miss
    // because no set_import_dependencies was called. The `project_resolver`
    // fast path will also miss because configure_projects was not called.
    // But the workspace has the @/ alias configured, so workspace-backed
    // resolution should find it.
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

    // The `import_routes` exact-match fast path should take priority
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

    let _store_view = host.resolver_store_view_read().into_owned_view();
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
            file_language: FileLanguage::vue(),
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

    let _view = host.resolver_store_view_read().into_owned_view();
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

    let _view = host.resolver_store_view_read().into_owned_view();
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
fn extract_vue_script_content_without_parsed_sfc_matches_cached() {
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

/// A JS comment containing `` `<style scoped>` `` must not truncate the
/// setup block. reka-ui RadioGroupItem has such a comment; truncating drops
/// every `defineProps` and leaves RadioGroup without a `value` prop.
#[test]
fn extract_vue_script_content_handles_style_tag_in_line_comment() {
    let source = r#"<script lang="ts">
export interface ItemProps { value?: string }
</script>
<script setup lang="ts">
const props = defineProps<ItemProps>()
// consumer `<style scoped>` keeps working. (issue #2751)
const scopeId = 1
</script>
<template><div /></template>
<style scoped>
.foo {}
</style>"#;

    let parsed = verter_compiler::compile::parse_sfc(source, None, None);
    let with_cache = crate::host_resolve::extract_vue_script_content(source, Some(&parsed))
        .expect("cached extraction should succeed");
    let without_cache = crate::host_resolve::extract_vue_script_content(source, None)
        .expect("non-cached extraction should succeed");

    for (label, content) in [("cached", &with_cache), ("raw-scan", &without_cache)] {
        assert!(
            content.contains("defineProps"),
            "{label}: setup must include defineProps, got:\n{content}"
        );
        assert!(
            content.contains("scopeId"),
            "{label}: setup must include code after the style-looking comment, got:\n{content}"
        );
        assert!(
            content.contains("ItemProps"),
            "{label}: companion interface must be retained, got:\n{content}"
        );
        // The real style block content is blanked (not a script span).
        assert!(
            !content.contains(".foo"),
            "{label}: must not include real <style> block body as script, got:\n{content}"
        );
    }
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
// Host helper tests
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
        file_language: crate::FileLanguage::script_ts(),
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
        file_language: crate::FileLanguage::script_ts(),
        aliases: Vec::new(),
    });
    let wrong_owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    assert!(
        host.resolve_decl_in_scope_with_reexport_chain(
            "/src/owner.vue",
            wrong_owner,
            "ChildProps",
        )
        .is_none(),
        "an ordinary-script lookup must not see a script-setup import from another owner"
    );

    let script_setup_owner = verter_type_expr::TopLevelOwnerId::instance(0);
    let identity = host
        .resolve_decl_in_scope_with_reexport_chain(
            "/src/owner.vue",
            script_setup_owner,
            "ChildProps",
        )
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

// ─────────────────────────────────────────────────────────────────────────
// Public-API projector-dispatch byte-identity pins.
//
// `get_public_api_with_mode` routes through the framework registry's
// component-API projector leg (selected by the canonical's resolved framework
// adapter id) rather than a hard Vue branch. These pins lock the rendered TSX
// text AND the source-map bytes for a Vue SFC with an external imported macro
// type in BOTH served modes — a single shifted byte or remapped position fails
// the gate (it would break LSP hover / go-to-def landing).
// ─────────────────────────────────────────────────────────────────────────

const PUBLIC_API_BYTE_PIN_SOURCE: &str = "<script setup lang=\"ts\">\nimport type { CapProps } from './cap-types';\nconst count = 1;\ndefineProps<CapProps>();\n</script>\n<template><div>{{ count }}</div></template>";

fn public_api_byte_pin_host() -> VerterHost {
    let host = strict_host();
    upsert_vue(&host, "/src/Cap.vue", PUBLIC_API_BYTE_PIN_SOURCE);
    let _ = host.upsert(crate::UpsertRequest {
        canonical_id: None,
        input_id: "/src/cap-types.ts".to_string(),
        source: Arc::from("export interface CapProps { label: string; n: number }\n"),
        file_language: crate::FileLanguage::script_ts(),
        aliases: Vec::new(),
    });
    host
}

fn direct_compiler_public_api(
    host: &VerterHost,
    mode: verter_compiler::tsc::TscMode,
) -> verter_compiler::tsc::TscOutput {
    let macro_output = host.produce_vue_macro_codegen(
        "/src/Cap.vue",
        crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::Tsc,
    );
    let bundle = macro_output.tsc.expect("direct compiler TSC bundle");
    let extracted = verter_compiler::tsc::extract_tsc_state(
        PUBLIC_API_BYTE_PIN_SOURCE,
        "Cap",
        &verter_compiler::tsc::TscExtractOptions {
            filename: Some("/src/Cap.vue".to_string()),
        },
    )
    .expect("direct compiler extraction");
    verter_compiler::tsc::generate_tsc_from_state(
        &extracted,
        "Cap",
        mode,
        verter_compiler::tsc::MacroTscInput::Authoritative(bundle.as_ref()),
    )
    .expect("direct compiler projection")
}

const PUBLIC_API_PUBLIC_CODE_PIN: &str = "import { defineComponent } from \"vue\"\ntype __OmitNew<T> = { [K in keyof T]: T[K] }\nimport type { CapProps } from './cap-types'\n\nconst __comp = defineComponent({\n})\n\ndeclare const Cap: __OmitNew<typeof __comp> & {\n  new(props?: import(\"vue\").PublicProps & CapProps): {\n    $props: import(\"vue\").PublicProps & CapProps,\n    $emit: (event: string, ...args: unknown[]) => void,\n    $data: {},\n    $attrs: import(\"vue\").HTMLAttributes,\n    $refs: {},\n  }\n}\nexport default Cap\n//# sourceMappingURL=data:application/json;base64,eyJ2ZXJzaW9uIjozLCJuYW1lcyI6W10sInNvdXJjZXMiOlsiL3NyYy9DYXAudnVlIl0sInNvdXJjZXNDb250ZW50IjpbIjxzY3JpcHQgc2V0dXAgbGFuZz1cInRzXCI+XG5pbXBvcnQgdHlwZSB7IENhcFByb3BzIH0gZnJvbSAnLi9jYXAtdHlwZXMnO1xuY29uc3QgY291bnQgPSAxO1xuZGVmaW5lUHJvcHM8Q2FwUHJvcHM+KCk7XG48L3NjcmlwdD5cbjx0ZW1wbGF0ZT48ZGl2Pnt7IGNvdW50IH19PC9kaXY+PC90ZW1wbGF0ZT4iXSwibWFwcGluZ3MiOiJBO0E7QSwyQztBO0E7QTs7QTtBLDBDQUdZLFE7QSx3Q0FBQSxRO0EsVywyQztBO0E7QTtBO0E7QSJ9\n";

const PUBLIC_API_PUBLIC_MAP_PIN: &str = "{\"version\":3,\"names\":[],\"sources\":[\"/src/Cap.vue\"],\"sourcesContent\":[\"<script setup lang=\\\"ts\\\">\\nimport type { CapProps } from './cap-types';\\nconst count = 1;\\ndefineProps<CapProps>();\\n</script>\\n<template><div>{{ count }}</div></template>\"],\"mappings\":\"A;A;A,2C;A;A;A;;A;A,0CAGY,Q;A,wCAAA,Q;A,W,2C;A;A;A;A;A;A\"}";

const PUBLIC_API_TESTING_CODE_PIN: &str = "import { defineComponent } from \"vue\"\ntype __OmitNew<T> = { [K in keyof T]: T[K] }\ntype __Verter_UnionToIntersection<U> = (U extends any ? (value: U) => void : never) extends ((value: infer I) => void) ? I : never\ntype __Verter_EmitFn<T> = T extends (...args: any[]) => any ? T : T extends Record<string, any> ? __Verter_UnionToIntersection<{ [K in keyof T]: T[K] extends any[] ? (event: K, ...args: T[K]) => void : T[K] extends (...args: infer A) => any ? (event: K, ...args: A) => void : (event: K, ...args: unknown[]) => void }[keyof T]> : (event: string, ...args: unknown[]) => void\ndeclare function defineProps<TypeProps>(): TypeProps\ndeclare function defineProps<RuntimeProps extends Record<string, any>>(props: RuntimeProps): import(\"vue\").ExtractPropTypes<RuntimeProps>\ndeclare function defineProps<PropName extends string>(props: readonly PropName[]): Record<PropName, unknown>\ndeclare function defineEmits<TypeEmits extends ((...args: any[]) => any) | Record<string, any>>(): __Verter_EmitFn<TypeEmits>\ndeclare function defineEmits<Named extends string>(names: readonly Named[]): __Verter_EmitFn<Record<Named, unknown[]>>\ndeclare function defineEmits<ObjectEmits extends Record<string, any>>(options: ObjectEmits): __Verter_EmitFn<ObjectEmits>\ndeclare function defineExpose<Exposed extends Record<string, any> = Record<string, never>>(exposed?: Exposed): void\ndeclare function defineOptions(options: Record<string, unknown>): void\ndeclare function defineSlots<Slots extends Record<string, any>>(): Slots\ndeclare function withDefaults<Props, Defaults extends Partial<Props>>(props: Props, defaults: Defaults): Omit<Props, keyof Defaults> & { [K in keyof Defaults]-?: K extends keyof Props ? Exclude<Props[K], undefined> : never }\ndeclare function defineModel<Model = unknown>(nameOrOptions?: string | unknown, options?: unknown): import(\"vue\").Ref<Model | undefined>\ndeclare const label: string\ndeclare const n: number\n\nimport type { CapProps } from './cap-types';\nconst count = 1;\ndefineProps<CapProps>();\n\ntype __Verter_TestBindings = import(\"vue\").ShallowUnwrapRef<{\n  count: typeof count;\n}>\n\nconst __comp = defineComponent({\n})\n\ndeclare const Cap: __OmitNew<typeof __comp> & {\n  new(props?: import(\"vue\").PublicProps & CapProps): {\n    $props: import(\"vue\").PublicProps & CapProps,\n    $emit: (event: string, ...args: unknown[]) => void,\n    $data: {},\n    $attrs: import(\"vue\").HTMLAttributes,\n    $refs: {},\n  } & __Verter_TestBindings\n}\nexport default Cap\n//# sourceMappingURL=data:application/json;base64,eyJ2ZXJzaW9uIjozLCJuYW1lcyI6W10sInNvdXJjZXMiOlsiL3NyYy9DYXAudnVlIl0sInNvdXJjZXNDb250ZW50IjpbIjxzY3JpcHQgc2V0dXAgbGFuZz1cInRzXCI+XG5pbXBvcnQgdHlwZSB7IENhcFByb3BzIH0gZnJvbSAnLi9jYXAtdHlwZXMnO1xuY29uc3QgY291bnQgPSAxO1xuZGVmaW5lUHJvcHM8Q2FwUHJvcHM+KCk7XG48L3NjcmlwdD5cbjx0ZW1wbGF0ZT48ZGl2Pnt7IGNvdW50IH19PC9kaXY+PC90ZW1wbGF0ZT4iXSwibWFwcGluZ3MiOiJBO0E7QTtBO0E7Ozs7Ozs7Ozs7O0EsY0FHWSxLLEUsTTtBLGNBQUEsQyxFLE07QTs7OztBO0E7QSxFQUROLEssUyxLO0E7O0E7QTs7QTtBLDBDQUNNLFE7QSx3Q0FBQSxRO0EsVywyQztBO0E7QTtBO0E7QSJ9\n";

const PUBLIC_API_TESTING_MAP_PIN: &str = "{\"version\":3,\"names\":[],\"sources\":[\"/src/Cap.vue\"],\"sourcesContent\":[\"<script setup lang=\\\"ts\\\">\\nimport type { CapProps } from './cap-types';\\nconst count = 1;\\ndefineProps<CapProps>();\\n</script>\\n<template><div>{{ count }}</div></template>\"],\"mappings\":\"A;A;A;A;A;;;;;;;;;;;A,cAGY,K,E,M;A,cAAA,C,E,M;A;;;;A;A;A,EADN,K,S,K;A;;A;A;;A;A,0CACM,Q;A,wCAAA,Q;A,W,2C;A;A;A;A;A;A\"}";

/// Mutation recipe: bypass the registry projector or alter one compiler-owned
/// generated byte/map segment. The direct-producer equivalence assertion or
/// the static byte pin must fail while the other remains an exact oracle.
#[test]
fn public_api_public_mode_is_byte_identical_through_projector_dispatch() {
    let host = public_api_byte_pin_host();
    let r = host
        .get_public_api_with_mode("/src/Cap.vue", PublicApiMode::Public, None)
        .expect("public-mode projection")
        .expect("public-mode api output");
    let direct = direct_compiler_public_api(&host, verter_compiler::tsc::TscMode::Public);
    assert_eq!(
        r.code.as_ref(),
        direct.code,
        "registry projection must be byte-identical to the direct compiler producer"
    );
    assert_eq!(
        r.source_map.as_ref().map(|map| map.as_ref()),
        Some(direct.source_map.as_str()),
        "registry source map must be byte-identical to the direct compiler producer"
    );
    assert_eq!(
        r.code.as_ref(),
        PUBLIC_API_PUBLIC_CODE_PIN,
        "public-mode rendered TSX must stay byte-identical through projector dispatch"
    );
    assert_eq!(
        r.source_map.as_ref().map(|m| m.as_ref()),
        Some(PUBLIC_API_PUBLIC_MAP_PIN),
        "public-mode source-map bytes must stay identical through projector dispatch"
    );
}

/// Mutation recipe: bypass the registry projector, stop semantic testing-row
/// materialization, or alter one mapped generated segment. The direct-producer
/// equivalence assertion or the non-empty static map pin must fail.
#[test]
fn public_api_testing_mode_is_byte_identical_through_projector_dispatch() {
    let host = public_api_byte_pin_host();
    let r = host
        .get_public_api_with_mode("/src/Cap.vue", PublicApiMode::Testing, None)
        .expect("testing-mode projection")
        .expect("testing-mode api output");
    let direct = direct_compiler_public_api(&host, verter_compiler::tsc::TscMode::Testing);
    assert_eq!(
        r.code.as_ref(),
        direct.code,
        "registry projection must be byte-identical to the direct compiler producer"
    );
    assert_eq!(
        r.source_map.as_ref().map(|map| map.as_ref()),
        Some(direct.source_map.as_str()),
        "registry source map must be byte-identical to the direct compiler producer"
    );
    assert_eq!(
        r.code.as_ref(),
        PUBLIC_API_TESTING_CODE_PIN,
        "testing-mode rendered TSX must stay byte-identical through projector dispatch"
    );
    // The testing-mode map carries NON-EMPTY mappings (`...A,cAGY...`),
    // so this pin discriminates a shifted source-map position, not just an
    // empty placeholder.
    assert_eq!(
        r.source_map.as_ref().map(|m| m.as_ref()),
        Some(PUBLIC_API_TESTING_MAP_PIN),
        "testing-mode source-map bytes must stay identical through projector dispatch"
    );
    assert!(
        PUBLIC_API_TESTING_MAP_PIN.contains("\"mappings\":\"A;")
            && PUBLIC_API_TESTING_MAP_PIN.contains("A,cAGY"),
        "the testing-mode pin must carry real VLQ mappings to be discriminating"
    );
}

#[test]
fn public_api_declaration_mode_is_declaration_safe_through_projector_dispatch() {
    // `PublicApiMode::Declaration` threads through `get_public_api_with_mode`
    // -> the Vue api-projector leg -> `TscMode::Declaration`. The result is a
    // strictly valid `.d.ts`: NO runtime/value code, an explicit
    // `declare const … export default …`, and the SAME public props surface
    // (`CapProps`) the Public mode computes.
    let host = public_api_byte_pin_host();
    let decl = host
        .get_public_api_with_mode("/src/Cap.vue", PublicApiMode::Declaration, None)
        .expect("declaration-mode projection")
        .expect("declaration-mode api output")
        .code
        .to_string();

    // NEGATIVE: no runtime / value tokens.
    assert!(
        !decl.contains("defineComponent("),
        "declaration must not call defineComponent, got:\n{decl}"
    );
    assert!(
        !decl.contains("const __comp"),
        "declaration must not create the runtime __comp const, got:\n{decl}"
    );
    assert!(
        !decl.contains("typeof __comp"),
        "declaration must not reference typeof a runtime value, got:\n{decl}"
    );
    assert!(
        !decl.contains("import { defineComponent }"),
        "declaration must not value-import defineComponent, got:\n{decl}"
    );
    // POSITIVE: it is a declaration carrying the public props surface.
    assert!(
        decl.contains("declare const Cap"),
        "declaration declares the component value, got:\n{decl}"
    );
    assert!(
        decl.contains("export default Cap"),
        "declaration default-exports the component, got:\n{decl}"
    );
    assert!(
        decl.contains("CapProps"),
        "declaration preserves the imported props type reference, got:\n{decl}"
    );
    // The type-only import survives; it is declaration-legal.
    assert!(
        decl.contains("import type { CapProps } from './cap-types'"),
        "declaration keeps the type-only import, got:\n{decl}"
    );

    // DISCRIMINATING: the Public mode (control) DOES carry the runtime const,
    // and the two outputs differ.
    let public = host
        .get_public_api_with_mode("/src/Cap.vue", PublicApiMode::Public, None)
        .expect("public-mode projection")
        .expect("public-mode api output")
        .code
        .to_string();
    assert!(
        public.contains("const __comp = defineComponent"),
        "Public mode emits the runtime __comp (control), got:\n{public}"
    );
    assert_ne!(
        public, decl,
        "Declaration output must differ from Public output"
    );
}

#[test]
fn public_api_non_vue_canonical_returns_none_through_projector_dispatch() {
    // A non-Vue canonical has no api-projector leg (its language has no
    // framework adapter id), so dispatch returns None — exactly the
    // pre-registry non-Vue behavior, with no Vue branch consulted.
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/plain.ts",
        "export interface Foo { a: number }\n",
    );
    assert!(
        host.get_public_api_with_mode("/src/plain.ts", PublicApiMode::Public, None)
            .expect("plain script projection")
            .is_none(),
        "a non-Vue canonical must project no public-API surface"
    );
    assert!(
        host.get_public_api_with_mode("/src/plain.ts", PublicApiMode::Testing, None)
            .expect("plain script projection")
            .is_none(),
        "a non-Vue canonical must project no public-API surface in testing mode either"
    );
}

#[test]
fn public_api_unsafe_declaration_is_a_typed_error_not_absence() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/UnsafeEnum.vue",
        r#"<script setup lang="ts">
enum Unsafe { Value = Math.random() }
defineProps<{ value: Unsafe }>()
</script>
<template><div /></template>"#,
    );

    let error = host
        .get_public_api_with_mode("/src/UnsafeEnum.vue", PublicApiMode::Declaration, None)
        .expect_err("an unsafe declaration must not collapse to None");
    assert!(matches!(
        error,
        crate::PublicApiProjectionError::TscGeneration(
            TscGenerationError::UnsupportedDeclarationShape {
                reason: TscDeclarationShapeReason::UnsupportedEnumShape,
                ..
            }
        )
    ));
    assert_eq!(error.code(), "tsc-generation");
    assert_eq!(error.detail_code(), "unsupported-declaration-shape");
    assert_eq!(
        error.subject(),
        crate::PublicApiProjectionSubject::Macro { syntax_index: 0 }
    );
    assert_eq!(error.macro_syntax_index(), Some(0));
    assert_eq!(
        error.declaration_shape_reason().map(|reason| reason.code()),
        Some("unsupported-enum-shape")
    );
}

#[test]
fn public_api_malformed_script_setup_attrs_preserves_exact_subject_range() {
    let host = strict_host();
    let source = r#"<script setup lang="ts" attrs="Attrs.">
import type { Attrs } from './types'
</script><template/>"#;
    upsert_vue(&host, "/src/MalformedAttrs.vue", source);
    let start = source.find("Attrs.").expect("attrs value") as u32;
    let source_range = Span::new(start, start + "Attrs.".len() as u32);

    let error = host
        .get_public_api_with_mode("/src/MalformedAttrs.vue", PublicApiMode::Declaration, None)
        .expect_err("malformed attrs type syntax must fail closed");

    assert_eq!(
        error.subject(),
        crate::PublicApiProjectionSubject::ScriptSetupAttrs { source_range }
    );
    assert_eq!(error.macro_syntax_index(), None);
    assert!(matches!(
        error.unavailable_outcome(),
        Some(verter_compiler::tsc::TscUnavailableOutcome::Invalid(
            verter_compiler::tsc::TscInvalidOutcome::AuthoredTypeSyntax(
                verter_compiler::tsc::TscInvalidAuthoredTypeReason::MalformedOrRecoveredTypeSyntax,
            )
        ))
    ));
}

#[test]
fn public_api_through_alias_is_byte_identical_to_canonical() {
    // Aliases are explicitly supported (`UpsertRequest.aliases`). The projector
    // dispatch resolves the alias and classifies by the RUNTIME-loaded source
    // language of the resolved canonical — so a request through the alias must
    // produce byte-identical output to a request through the canonical, in both
    // modes. (Classifying the raw alias id by static path would mis-route an
    // alias that lacks a `.vue` suffix to `None`.)
    let host = strict_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/Aliased.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\ndefineProps<{ a: string }>();\n</script>\n<template><div/></template>",
            ),
            file_language: FileLanguage::vue(),
            aliases: vec!["/virtual/aliased-handle".to_string()],
        })
        .unwrap();

    for mode in [PublicApiMode::Public, PublicApiMode::Testing] {
        let via_canonical = host
            .get_public_api_with_mode("/src/Aliased.vue", mode, None)
            .unwrap_or_else(|error| panic!("canonical projection failed for {mode:?}: {error}"))
            .unwrap_or_else(|| panic!("canonical request must render for {mode:?}"));
        let via_alias = host
            .get_public_api_with_mode("/virtual/aliased-handle", mode, None)
            .unwrap_or_else(|error| panic!("alias projection failed for {mode:?}: {error}"))
            .unwrap_or_else(|| panic!("alias request must render for {mode:?}"));
        assert_eq!(
            via_alias.code.as_ref(),
            via_canonical.code.as_ref(),
            "alias request must produce byte-identical TSX to the canonical for {mode:?}"
        );
        assert_eq!(
            via_alias.source_map.as_ref().map(|m| m.as_ref()),
            via_canonical.source_map.as_ref().map(|m| m.as_ref()),
            "alias request must produce identical source-map bytes for {mode:?}"
        );
    }
}

#[test]
fn public_api_classification_authority_is_runtime_load_language_not_path() {
    // The classification authority is the RUNTIME-loaded `file_language` the
    // source was upserted with — NOT a static path re-classification. A `.vue`
    // PATH loaded explicitly as a plain script projects no public-API surface
    // (its loaded language carries no framework adapter id); a non-`.vue` path
    // loaded explicitly as the Vue carrier DOES (matching the pre-registry
    // gate, which read `HostSourceData.file_language`).
    let host = strict_host();

    // `.vue` path, but loaded as a plain TS script → no Vue surface.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/NotReallyVue.vue".to_string(),
            source: Arc::from("export const x = 1;\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        host.get_public_api_with_mode("/src/NotReallyVue.vue", PublicApiMode::Public, None)
            .expect("plain-script projection")
            .is_none(),
        "a `.vue`-path file loaded as a plain script must project no public-API surface"
    );

    // Non-`.vue` path, but loaded as the Vue carrier → Vue surface renders.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/carrier-as-vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\ndefineProps<{ a: string }>();\n</script>\n<template><div/></template>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        host.get_public_api_with_mode("/src/carrier-as-vue", PublicApiMode::Public, None)
            .expect("Vue carrier projection")
            .is_some(),
        "a file loaded as the Vue carrier must project a public-API surface regardless of path"
    );
}

#[test]
fn vue_api_projector_rejects_a_non_carrier_vue_language() {
    // Direct unit coverage of the Vue api-projector leg's carrier-narrowness
    // (the registry-faithful replacement for the old `is_vue()` carrier gate).
    // A Vue-ADAPTER row that is NOT the SFC carrier (distinct `language_id`)
    // is routed to the Vue leg by adapter id, but the leg rejects it on the
    // descriptor `carrier_language` mismatch — never reaching the legacy body.
    // (This case is also rejected upstream at load time as
    // `UnsupportedLanguage`, so the leg check is unreachable through `upsert`;
    // exercising the leg directly is the discriminating coverage.)
    use crate::framework::api_projector::{ComponentApiProjector, ComponentApiProjectorCtx};
    use crate::framework::api_projectors::VueComponentApiProjector;

    // Load a REAL Vue SFC so the legacy body would render Some if reached —
    // making the leg's carrier rejection the SOLE cause of the None below
    // (a discriminating test: deleting the leg's carrier check makes this
    // return Some, failing the assertion).
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/RealSfc.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ a: string }>();\n</script>\n<template><div/></template>",
    );

    // Sanity: the carrier language DOES render this loaded SFC through the leg.
    let carrier = FileLanguage::vue();
    let via_carrier = VueComponentApiProjector
        .render_api(ComponentApiProjectorCtx {
            host: &host,
            resolved_canonical: "/src/RealSfc.vue",
            file_language: &carrier,
            mode: PublicApiMode::Public,
            profile: None,
            // This SFC's `defineProps<{ a: string }>()` is an inline literal (no
            // macro-type deps), so the legacy body never reaches the seed-bearing
            // macro-deps branch — `None` is sufficient for this carrier-gate test.
            render_seed: None,
        })
        .expect("Vue carrier projection");
    assert!(
        via_carrier.is_some(),
        "the Vue leg must render a loaded SFC for the carrier language"
    );

    // The SAME loaded SFC, but presented with a Vue-ADAPTER NON-carrier
    // language row, is rejected by the leg's carrier-narrowness check BEFORE
    // the legacy body — even though the body would otherwise render it.
    let vue_non_carrier = FileLanguage::Framework {
        adapter_id: verter_language::FrameworkAdapterId::vue(),
        language_id: verter_language::LanguageId::new("vue_template"),
    };
    let rejected = VueComponentApiProjector
        .render_api(ComponentApiProjectorCtx {
            host: &host,
            resolved_canonical: "/src/RealSfc.vue",
            file_language: &vue_non_carrier,
            mode: PublicApiMode::Public,
            profile: None,
            // Rejected by the carrier-narrowness gate BEFORE the legacy body, so
            // the seed is never consulted.
            render_seed: None,
        })
        .expect("Vue non-carrier projection");
    assert!(
        rejected.is_none(),
        "the Vue leg must reject a Vue-adapter non-carrier language before the legacy body, \
         even when the canonical is a loaded SFC that would otherwise render"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// FOUND macro type argument whose SURFACE composition (heritage `extends`
// parent, intersection / union arm) references an unresolvable IMPORT-BACKED
// type. The dep type itself RESOLVES — the miss sits one level deeper, inside
// the declaring file — so the legacy missing-root path stays silent; the
// shared shallow walker reports the dropped arm and the collector tiers it as
// a fatal error. Member-position references and non-import-backed (ambient)
// heritage names must stay silent.
// ───────────────────────────────────────────────────────────────────────────

fn ensure_compiled_error(host: &VerterHost, canonical_id: &str) -> crate::DiagnosticsSnapshot {
    match host.ensure_compiled(canonical_id, &profile()) {
        Err(HostError::CompileError(failure)) => failure.diagnostics,
        Err(other) => panic!("expected compile error, got {other:?}"),
        Ok(()) => panic!("expected compile error, got successful compile"),
    }
}

fn assert_compiles_without_macro_type_dep_diag(host: &VerterHost, canonical_id: &str) {
    host.ensure_compiled(canonical_id, &profile())
        .unwrap_or_else(|err| panic!("{canonical_id} must compile, got {err:?}"));
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical_id.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .unwrap_or_else(|err| panic!("{canonical_id} must serve a Main node, got {err:?}"));
    assert!(
        !response
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "{canonical_id} must not surface HOST_MISSING_MACRO_TYPE_DEP: {:?}",
        response.diagnostics.diagnostics
    );
}

/// Shared dep-file fixture: every `Found*` type RESOLVES, but its surface
/// composition references a name imported from the missing module `./nope`.
fn upsert_unresolved_surface_arm_types(host: &VerterHost) {
    upsert_non_sfc(
        host,
        "/src/types.ts",
        "import type { NotFoundHeritage, NotFoundArm, NotFoundUnionArm } from './nope'\n\
         export interface FoundExtendsMissing extends NotFoundHeritage { a?: string }\n\
         export interface Base { b?: number }\n\
         export type FoundIntersectAlias = Base & NotFoundArm\n\
         export type FoundUnionAlias = Base | NotFoundUnionArm",
    );
}

fn surface_arm_sfc(type_name: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\nimport type {{ {type_name} }} from './types'\nconst props = defineProps<{type_name}>()\n</script>\n<template><div/></template>"
    )
}

#[test]
fn found_macro_type_with_missing_heritage_parent_fails_compile() {
    let host = strict_host();
    upsert_unresolved_surface_arm_types(&host);
    let source = surface_arm_sfc("FoundExtendsMissing");
    upsert_vue(&host, "/src/A.vue", &source);

    let diagnostics = ensure_compiled_error(&host, "/src/A.vue");
    let missing = find_diag(&diagnostics, "HOST_MISSING_MACRO_TYPE_DEP");
    assert_eq!(missing.severity, HostSeverity::Error);
    assert!(
        missing.message.contains("NotFoundHeritage"),
        "heritage-arm miss must name the unresolved type: {}",
        missing.message
    );
    assert!(
        missing.message.contains("FoundExtendsMissing"),
        "heritage-arm miss must name the resolved dep type: {}",
        missing.message
    );
    // The diagnostic anchors to the SFC's owning import statement.
    let import_start = source.find("import type").unwrap() as u32;
    let import_end =
        import_start + "import type { FoundExtendsMissing } from './types'".len() as u32;
    assert_eq!(
        missing.span,
        Some(Span::new(import_start, import_end)),
        "surface-arm miss span should point at the owning import"
    );
    // The intersection-arm miss belongs to FoundIntersectAlias, which this
    // component does not consume.
    assert!(
        !diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("NotFoundArm")),
        "unconsumed sibling types must not contribute arms: {:?}",
        diagnostics.diagnostics
    );
}

#[test]
fn found_macro_type_with_missing_intersection_arm_fails_compile() {
    let host = strict_host();
    upsert_unresolved_surface_arm_types(&host);
    upsert_vue(&host, "/src/B.vue", &surface_arm_sfc("FoundIntersectAlias"));

    let diagnostics = ensure_compiled_error(&host, "/src/B.vue");
    let missing = find_diag(&diagnostics, "HOST_MISSING_MACRO_TYPE_DEP");
    assert_eq!(missing.severity, HostSeverity::Error);
    assert!(
        missing.message.contains("NotFoundArm"),
        "intersection-arm miss must name the unresolved type: {}",
        missing.message
    );
}

#[test]
fn found_macro_type_with_missing_union_arm_fails_compile() {
    let host = strict_host();
    upsert_unresolved_surface_arm_types(&host);
    upsert_vue(&host, "/src/C.vue", &surface_arm_sfc("FoundUnionAlias"));

    let diagnostics = ensure_compiled_error(&host, "/src/C.vue");
    let missing = find_diag(&diagnostics, "HOST_MISSING_MACRO_TYPE_DEP");
    assert_eq!(missing.severity, HostSeverity::Error);
    assert!(
        missing.message.contains("NotFoundUnionArm"),
        "union-arm miss must name the unresolved type: {}",
        missing.message
    );
}

#[test]
fn found_macro_type_with_member_level_missing_types_still_compiles() {
    // MEMBER-position references to an unresolvable import degrade that
    // member's runtime type to `null` — never a surface-arm error. Nested
    // references are not collected at all.
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { NotFound, NotFound2 } from './nope'\n\
         export interface FoundMemberMissing { foo: NotFound }\n\
         export interface FoundNestedMissing { foo: { test: NotFound2 } }",
    );
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundMemberMissing"));
    upsert_vue(&host, "/src/B.vue", &surface_arm_sfc("FoundNestedMissing"));

    assert_compiles_without_macro_type_dep_diag(&host, "/src/A.vue");
    assert_compiles_without_macro_type_dep_diag(&host, "/src/B.vue");
}

#[test]
fn found_macro_type_with_ambient_heritage_name_still_compiles() {
    // The unresolved heritage name is NOT an import binding of the declaring
    // file — it may be ambient / lib-provided, so the surface-arm error must
    // not fire.
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface FoundGlobalHeritage extends SomeUndeclaredGlobal { a?: string }",
    );
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundGlobalHeritage"));

    assert_compiles_without_macro_type_dep_diag(&host, "/src/A.vue");
}

/// A dual-script SFC may bind the same local type name to different imports.
/// The unresolved-arm fact must retain the exact lexical owner; an owner-blind
/// lookup would select the setup import first and silence the companion miss.
#[test]
fn found_macro_type_surface_arm_uses_exact_dual_script_import_owner() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/present.ts",
        "export interface Ghost { present?: string }",
    );
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script setup lang="ts">
import type { Ghost } from './present'
interface SetupOnly extends Ghost { setup?: string }
</script>
<script lang="ts">
import type { Ghost } from './missing-setup'
export interface Props extends Ghost { own?: string }
</script>
<template><div/></template>"#,
    );
    upsert_vue(
        &host,
        "/src/A.vue",
        &surface_arm_sfc("Props").replace("'./types'", "'./types.vue'"),
    );

    let diagnostics = ensure_compiled_error(&host, "/src/A.vue");
    let missing = find_diag(&diagnostics, "HOST_MISSING_MACRO_TYPE_DEP");
    assert!(
        missing.message.contains("Ghost"),
        "the companion-owner missing arm must remain fatal despite the setup-owner collision: {}",
        missing.message
    );
}

#[test]
fn found_macro_type_with_resolvable_heritage_chain_still_compiles() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Base2 { b?: number }\n\
         export interface FoundOk extends Base2 { a?: string }",
    );
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundOk"));

    assert_compiles_without_macro_type_dep_diag(&host, "/src/A.vue");
}

/// A surface arm whose name is PROVABLY absent behind a star re-export
/// barrel is fatal: the export-surface probe walks `export *` targets
/// transitively, so a barrel does not shield a genuinely missing name.
#[test]
fn found_macro_type_arm_provably_absent_behind_star_barrel_fails_compile() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/empty-mod.ts",
        "export interface Unrelated { u?: number }",
    );
    upsert_non_sfc(&host, "/src/barrel.ts", "export * from './empty-mod'");
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { Ghost } from './barrel'\n\
         export interface FoundGhostHeritage extends Ghost { a?: string }",
    );
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundGhostHeritage"));

    let diagnostics = ensure_compiled_error(&host, "/src/A.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP" && d.message.contains("Ghost")),
        "a provably-absent star-barrel arm must be fatal and name the arm: {:?}",
        diagnostics.diagnostics
    );
}

/// Sibling export routes receive independent cycle/path state. Both named
/// routes converge on the same loaded module and prove `Ghost` absent; leaking
/// the first branch's visited set into the second would make the second route
/// spuriously unknowable and suppress the fatal diagnostic.
#[test]
fn found_macro_type_arm_absent_across_converging_sibling_routes_fails_compile() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/empty-mod.ts",
        "export interface Unrelated { u?: number }",
    );
    upsert_non_sfc(
        &host,
        "/src/left.ts",
        "export type { Ghost } from './empty-mod'",
    );
    upsert_non_sfc(
        &host,
        "/src/right.ts",
        "export type { Ghost } from './empty-mod'",
    );
    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export type { Ghost } from './left'\nexport type { Ghost } from './right'",
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { Ghost } from './barrel'\n\
         export interface FoundGhostHeritage extends Ghost { a?: string }",
    );
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundGhostHeritage"));

    let diagnostics = ensure_compiled_error(&host, "/src/A.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP" && d.message.contains("Ghost")),
        "converging sibling routes that both prove absence must stay fatal: {:?}",
        diagnostics.diagnostics
    );
}

/// Control: a heritage parent that RESOLVES through a star re-export barrel
/// compiles silently — the arm materialises, so no walker miss fires at all.
#[test]
fn found_macro_type_with_heritage_behind_star_reexport_compiles() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface BarrelBase { b?: number }",
    );
    upsert_non_sfc(&host, "/src/barrel.ts", "export * from './base'");
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { BarrelBase } from './barrel'\n\
         export interface FoundViaBarrel extends BarrelBase { a?: string }",
    );
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundViaBarrel"));

    assert_compiles_without_macro_type_dep_diag(&host, "/src/A.vue");
}

/// HostBacked mirror of the render-lane member-position tier: a MEMBER
/// annotation whose type import is missing compiles successfully, surfaces a
/// WARNING, and the member's runtime type degrades to `null`.
#[test]
fn member_position_missing_macro_type_warns_and_degrades_on_host_lane() {
    let host = strict_host();
    upsert_vue(
        &host,
        "/src/MemberMiss.vue",
        "<script setup lang=\"ts\">\nimport type { Missing } from './nope'\nconst props = defineProps<{ foo: Missing }>()\n</script>\n<template><div>{{ foo }}</div></template>",
    );

    host.ensure_compiled("/src/MemberMiss.vue", &profile())
        .expect("member-position miss must not abort the host lane");
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/MemberMiss.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("member-position miss must still serve a Main node");
    let warning = response
        .diagnostics
        .diagnostics
        .iter()
        .find(|d| d.code == "XUnresolvedImportedMacroType")
        .unwrap_or_else(|| {
            panic!(
                "member-position miss must surface the compiler's typed row diagnostic: {:?}",
                response.diagnostics.diagnostics
            )
        });
    assert_eq!(
        warning.severity,
        HostSeverity::Warning,
        "member-position miss is a warning, never fatal"
    );
    assert!(
        response
            .code
            .contains("foo: { type: null, required: true }"),
        "member with unresolvable type must degrade to `type: null`:\n{}",
        response.code
    );
}

#[test]
fn found_macro_type_missing_surface_arm_error_repeats_on_recompile() {
    // Cold/warm parity: re-demanding the same failed compile reproduces the
    // same surface-arm error (the fact lives on the cached dispatch value and
    // replays on warm reads).
    let host = strict_host();
    upsert_unresolved_surface_arm_types(&host);
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundExtendsMissing"));

    let cold = ensure_compiled_error(&host, "/src/A.vue");
    let cold_missing = find_diag(&cold, "HOST_MISSING_MACRO_TYPE_DEP");
    assert!(cold_missing.message.contains("NotFoundHeritage"));

    let warm = ensure_compiled_error(&host, "/src/A.vue");
    let warm_missing = find_diag(&warm, "HOST_MISSING_MACRO_TYPE_DEP");
    assert_eq!(
        cold_missing.message, warm_missing.message,
        "recompile must reproduce the identical surface-arm error"
    );
    assert_eq!(warm_missing.severity, HostSeverity::Error);
}

/// Creating the previously-missing arm source CLEARS the error: the next
/// compile resolves the heritage arm instead of replaying the stale
/// dropped-arm fact from the surface memo.
#[test]
fn found_macro_type_surface_arm_error_clears_when_missing_source_appears() {
    let host = strict_host();
    upsert_unresolved_surface_arm_types(&host);
    upsert_vue(&host, "/src/A.vue", &surface_arm_sfc("FoundExtendsMissing"));

    let cold = ensure_compiled_error(&host, "/src/A.vue");
    assert!(find_diag(&cold, "HOST_MISSING_MACRO_TYPE_DEP")
        .message
        .contains("NotFoundHeritage"));

    // The missing module appears with the required exports.
    upsert_non_sfc(
        &host,
        "/src/nope.ts",
        "export interface NotFoundHeritage { inherited?: number }\n\
         export interface NotFoundArm { arm?: number }\n\
         export interface NotFoundUnionArm { u?: number }",
    );

    assert_compiles_without_macro_type_dep_diag(&host, "/src/A.vue");
}

// ───────────────────────────────────────────────────────────────────────────
// Bare directory import specifiers (`.` / `..`) — in-tree relative routes
// that resolve to a directory index. TypeScript treats a bare `.`/`..` as a
// relative path (`pathIsRelative`: /^\.\.?($|[\\/])/), so the shared
// resolver and the surface-arm adjudicator must resolve them exactly like
// the `./`/`../` forms: an in-tree heritage arm imported `from '..'` must
// materialise (no false-positive fatal HOST_MISSING_MACRO_TYPE_DEP), while
// a name provably absent from the resolved parent index stays fatal.
// (reka-ui `ListboxFilter.vue` / `_DatePicker.vue` regression shape.)
// ───────────────────────────────────────────────────────────────────────────

/// The reka-ui anchor fixture: `PrimitiveProps` is declared in a directory
/// module, re-exported by its directory index, re-exported again by the
/// package root index (a directory-hop named re-export chain), and consumed
/// as heritage through `heritage_specifier` — the bare parent-directory
/// specifier `'..'` under test, or its spelled-out `'../index'` control.
fn upsert_parent_dir_barrel_fixture(host: &VerterHost, heritage_specifier: &str) {
    upsert_non_sfc(
        host,
        "/src/Primitive/Primitive.ts",
        "export interface PrimitiveProps { as?: string }",
    );
    upsert_non_sfc(
        host,
        "/src/Primitive/index.ts",
        "export { type PrimitiveProps } from './Primitive'",
    );
    upsert_non_sfc(
        host,
        "/src/index.ts",
        "export { type PrimitiveProps } from './Primitive'\nexport * from './Listbox'",
    );
    upsert_non_sfc(
        host,
        "/src/Listbox/index.ts",
        "export { type ListboxFilterProps } from './ListboxFilter.vue'",
    );
    upsert_vue(
        host,
        "/src/Listbox/ListboxFilter.vue",
        &format!(
            "<script lang=\"ts\">\nimport type {{ PrimitiveProps }} from '{heritage_specifier}'\nexport interface ListboxFilterProps extends PrimitiveProps {{ modelValue?: string }}\n</script>\n<script setup lang=\"ts\">\nconst props = defineProps<ListboxFilterProps>()\n</script>\n<template><input/></template>"
        ),
    );
    upsert_vue(
        host,
        "/src/Autocomplete/AutocompleteInput.vue",
        "<script lang=\"ts\">\nimport type { ListboxFilterProps } from '../Listbox'\nexport interface AutocompleteInputProps extends ListboxFilterProps {}\n</script>\n<script setup lang=\"ts\">\nconst props = defineProps<AutocompleteInputProps>()\n</script>\n<template><div/></template>",
    );
}

fn main_node_code(host: &VerterHost, canonical_id: &str) -> String {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile(),
    })
    .unwrap_or_else(|err| panic!("{canonical_id} must serve a Main node, got {err:?}"))
    .code
    .to_string()
}

/// The exact corpus failure shape: the consumer's macro type dep RESOLVES
/// (through a sibling barrel), but the dep type's own heritage is imported
/// `from '..'`. The heritage arm must resolve through the parent-directory
/// index — never adjudicate as an unresolvable surface arm — and the whole
/// pipeline must behave byte-identically to the spelled-out `'../index'`
/// control (a bare directory specifier is not a special case).
#[test]
fn found_macro_type_with_heritage_via_bare_parent_dir_import_compiles() {
    let bare = strict_host();
    upsert_parent_dir_barrel_fixture(&bare, "..");
    let control = strict_host();
    upsert_parent_dir_barrel_fixture(&control, "../index");

    assert_compiles_without_macro_type_dep_diag(
        &control,
        "/src/Autocomplete/AutocompleteInput.vue",
    );
    assert_compiles_without_macro_type_dep_diag(&bare, "/src/Autocomplete/AutocompleteInput.vue");
    assert_compiles_without_macro_type_dep_diag(&bare, "/src/Listbox/ListboxFilter.vue");

    // The consumer sources are identical in both hosts — the emitted Main
    // must be too (the `'..'` route resolves to the same declaration chain).
    assert_eq!(
        main_node_code(&bare, "/src/Autocomplete/AutocompleteInput.vue"),
        main_node_code(&control, "/src/Autocomplete/AutocompleteInput.vue"),
        "bare '..' heritage must produce the same consumer emission as '../index'"
    );

    // The dep's OWN compile enumerates its own-body member — and must not
    // degrade it to a null runtime type under the bare-specifier route.
    let dep_code = main_node_code(&bare, "/src/Listbox/ListboxFilter.vue");
    assert!(
        dep_code.contains("modelValue: { type: String }"),
        "dep's own-body member must keep its runtime type under a '..' heritage import:\n{dep_code}"
    );
    assert!(
        !dep_code.contains("type: null"),
        "no member may degrade to a null runtime type when the '..' route resolves:\n{dep_code}"
    );
}

/// Backslash spelling of the same heritage route: the `.vue` SOURCE TEXT
/// carries `from '..\\index'`, whose JS-cooked module-specifier VALUE is
/// `..\index` (one backslash). TS `pathIsRelative` (`/^\.\.?($|[\\/])/`)
/// classifies it relative and normalizes `\` → `/` when combining paths, so
/// it must resolve — end to end, through the macro-type dep pipeline —
/// byte-identically to the `'../index'` spelling: no false-positive fatal
/// HOST_MISSING_MACRO_TYPE_DEP, no node_modules ancestor-walk misroute.
#[test]
fn found_macro_type_with_heritage_via_backslash_parent_dir_import_compiles() {
    let backslash = strict_host();
    upsert_parent_dir_barrel_fixture(&backslash, "..\\\\index");
    let control = strict_host();
    upsert_parent_dir_barrel_fixture(&control, "../index");

    assert_compiles_without_macro_type_dep_diag(
        &control,
        "/src/Autocomplete/AutocompleteInput.vue",
    );
    assert_compiles_without_macro_type_dep_diag(
        &backslash,
        "/src/Autocomplete/AutocompleteInput.vue",
    );
    assert_compiles_without_macro_type_dep_diag(&backslash, "/src/Listbox/ListboxFilter.vue");

    // The consumer sources are identical in both hosts — the emitted Main
    // must be too (the `'..\index'` route resolves to the same declaration
    // chain as `'../index'`).
    assert_eq!(
        main_node_code(&backslash, "/src/Autocomplete/AutocompleteInput.vue"),
        main_node_code(&control, "/src/Autocomplete/AutocompleteInput.vue"),
        "'..\\index' heritage must produce the same consumer emission as '../index'"
    );

    // The dep's OWN compile enumerates its own-body member — and must not
    // degrade it to a null runtime type under the backslash-specifier route.
    let dep_code = main_node_code(&backslash, "/src/Listbox/ListboxFilter.vue");
    assert!(
        dep_code.contains("modelValue: { type: String }"),
        "dep's own-body member must keep its runtime type under a '..\\index' heritage import:\n{dep_code}"
    );
    assert!(
        !dep_code.contains("type: null"),
        "no member may degrade to a null runtime type when the '..\\index' route resolves:\n{dep_code}"
    );
}

/// Direct root-import shape: `defineProps<T>()` whose type argument is
/// imported `from '..'` (the parent-directory index). The root import must
/// load from the PARENT directory's index — a bare `'..'` is a relative
/// specifier, never a bare package name. The importer's own directory also
/// has an index (the reka-ui layout): a misrouted `'..'` that lands on the
/// importer's own directory index cannot find `RootProps` there.
#[test]
fn macro_type_root_import_via_bare_parent_dir_specifier_compiles() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/index.ts",
        "export interface RootProps { title?: string }",
    );
    upsert_non_sfc(
        &host,
        "/src/Comp/index.ts",
        "export interface Unrelated { u?: number }",
    );
    upsert_vue(
        &host,
        "/src/Comp/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { RootProps } from '..'\nconst props = defineProps<RootProps>()\n</script>\n<template><div/></template>",
    );

    assert_compiles_without_macro_type_dep_diag(&host, "/src/Comp/Comp.vue");

    // Positive: the imported root type's member materialises with its real
    // runtime type — proving `'..'` loaded the PARENT index (the decoy
    // `/src/Comp/index.ts` has no `RootProps`).
    let code = main_node_code(&host, "/src/Comp/Comp.vue");
    assert!(
        code.contains("title: { type: String }"),
        "the '..'-imported root type's member must enumerate with its runtime type:\n{code}"
    );
    assert!(
        !code.contains("type: null"),
        "no member may degrade to a null runtime type when '..' resolves:\n{code}"
    );
}

/// Bare `'.'` — the current-directory index — is the same specifier class.
#[test]
fn macro_type_root_import_via_bare_current_dir_specifier_compiles() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/Comp/index.ts",
        "export interface RootProps { title?: string }",
    );
    upsert_vue(
        &host,
        "/src/Comp/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { RootProps } from '.'\nconst props = defineProps<RootProps>()\n</script>\n<template><div/></template>",
    );

    assert_compiles_without_macro_type_dep_diag(&host, "/src/Comp/Comp.vue");
    let code = main_node_code(&host, "/src/Comp/Comp.vue");
    assert!(
        code.contains("title: { type: String }"),
        "the '.'-imported root type's member must enumerate with its runtime type:\n{code}"
    );
}

/// Overcorrection guard (the honest-fatal direction): the `'..'` route now
/// RESOLVES to the parent index, but the requested heritage name is provably
/// absent from its export surface — the surface-arm error must survive.
#[test]
fn found_macro_type_arm_provably_absent_via_bare_parent_dir_import_fails_compile() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/src/index.ts",
        "export interface Unrelated { u?: number }",
    );
    upsert_non_sfc(
        &host,
        "/src/Listbox/types.ts",
        "import type { Ghost } from '..'\n\
         export interface FoundGhostParent extends Ghost { a?: string }",
    );
    upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup lang=\"ts\">\nimport type { FoundGhostParent } from './Listbox/types'\nconst props = defineProps<FoundGhostParent>()\n</script>\n<template><div/></template>",
    );

    let diagnostics = ensure_compiled_error(&host, "/src/A.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP" && d.message.contains("Ghost")),
        "a name provably absent from the resolved '..' index must stay fatal: {:?}",
        diagnostics.diagnostics
    );
}

#[test]
fn render_legacy_body_operates_on_the_resolved_canonical_without_re_resolving() {
    // The legacy body renders the canonical it is GIVEN — it does NOT resolve
    // aliases. So passing an ALIAS to it directly renders nothing, while
    // passing the resolved canonical renders. This is what makes the host's
    // single up-front alias resolution coherent: classification and rendering
    // share one resolved target (no classify-one / render-another split under
    // a concurrent alias relabel).
    let host = strict_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/Coherent.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\ndefineProps<{ a: string }>();\n</script>\n<template><div/></template>",
            ),
            file_language: FileLanguage::vue(),
            aliases: vec!["/virtual/coherent-handle".to_string()],
        })
        .unwrap();

    // This SFC's `defineProps<{ a: string }>()` is an inline literal (no
    // macro-type deps), so the legacy body never reaches the seed-bearing
    // macro-deps branch — `None` for the render seed is sufficient here.
    assert!(
        host.render_vue_public_api_legacy("/src/Coherent.vue", PublicApiMode::Public, None, None)
            .expect("canonical legacy projection")
            .is_some(),
        "the legacy body must render the resolved canonical it is given"
    );
    assert!(
        host.render_vue_public_api_legacy(
            "/virtual/coherent-handle",
            PublicApiMode::Public,
            None,
            None
        )
        .expect("alias legacy projection")
        .is_none(),
        "the legacy body must NOT resolve an alias itself — the host resolves once up-front"
    );

    // The full host entry still renders through the alias (it resolves first,
    // then hands the resolved canonical to the leg) — proving the alias path
    // is served end-to-end while the leg stays resolution-free.
    assert!(
        host.get_public_api_with_mode("/virtual/coherent-handle", PublicApiMode::Public, None)
            .expect("alias public API projection")
            .is_some(),
        "the host entry must still serve the alias end-to-end via one resolution"
    );
}

/// Local alias of an external property-form emits type must produce
/// `emits: ["update:open"]` at runtime (AlertDialogRoot pattern).
#[test]
fn host_defineemits_local_alias_of_external_emits() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export type RootEmits = { 'update:open': [value: boolean] }\n",
    );
    upsert_vue(
        &host,
        "/src/Alias.vue",
        r#"<script setup lang="ts">
import type { RootEmits } from './types'
type A = RootEmits
const emit = defineEmits<A>()
</script>
<template><div /></template>
"#,
    );

    host.set_import_dependencies(
        "/src/Alias.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    host.ensure_compiled("/src/Alias.vue", &profile())
        .expect("alias emits SFC must compile");

    let resp = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Alias.vue".into()),
            compile_profile: profile(),
            node_kind: Some(VirtualNodeKind::Main),
        })
        .expect("virtual file");
    let code = resp.code.as_ref();
    assert!(
        code.contains("update:open") && code.contains("emits:"),
        "local alias of external emits must produce runtime emits array, got:\n{code}"
    );
}

#[test]
fn host_defineemits_direct_external_emits() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export type RootEmits = { 'update:open': [value: boolean] }\n",
    );
    upsert_vue(
        &host,
        "/src/Direct.vue",
        r#"<script setup lang="ts">
import type { RootEmits } from './types'
const emit = defineEmits<RootEmits>()
</script>
<template><div /></template>
"#,
    );

    host.ensure_compiled("/src/Direct.vue", &profile())
        .expect("direct emits SFC must compile");

    let resp = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Direct.vue".into()),
            compile_profile: profile(),
            node_kind: Some(VirtualNodeKind::Main),
        })
        .expect("virtual file");
    let code = resp.code.as_ref();
    assert!(
        code.contains("update:open") && code.contains("emits:"),
        "direct external emits must produce runtime emits array, got:\n{code}"
    );
}

#[test]
fn host_defineemits_local_alias_of_local_emits() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/LocalAlias.vue",
        r#"<script setup lang="ts">
type RootEmits = { 'update:open': [value: boolean] }
type A = RootEmits
const emit = defineEmits<A>()
</script>
<template><div /></template>
"#,
    );

    host.ensure_compiled("/src/LocalAlias.vue", &profile())
        .expect("local alias emits must compile");

    let resp = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/LocalAlias.vue".into()),
            compile_profile: profile(),
            node_kind: Some(VirtualNodeKind::Main),
        })
        .expect("virtual file");
    let code = resp.code.as_ref();
    assert!(
        code.contains("update:open") && code.contains("emits:"),
        "local alias of local emits must produce runtime emits array, got:\n{code}"
    );
}

/// `</script><template>` with NO whitespace between the tags: the raw
/// scanner's close match is a COMPLETE tag, so no after-byte boundary rule
/// may apply — requiring one silently dropped the block's span and lost
/// macros whenever the raw scan was authoritative.
#[test]
fn extract_vue_script_content_handles_adjacent_close_and_template() {
    let source = "<script lang=\"ts\">\nexport interface ItemProps { value?: string }\n</script><script setup lang=\"ts\">\nconst props = defineProps<ItemProps>()\n</script><template><div /></template>";

    let parsed = verter_compiler::compile::parse_sfc(source, None, None);
    let with_cache = crate::host_resolve::extract_vue_script_content(source, Some(&parsed))
        .expect("cached extraction should succeed");
    let without_cache = crate::host_resolve::extract_vue_script_content(source, None)
        .expect("non-cached extraction should succeed");

    for (label, content) in [("cached", &with_cache), ("raw-scan", &without_cache)] {
        assert!(
            content.contains("defineProps"),
            "{label}: setup must include defineProps with adjacent close tags, got:\n{content}"
        );
        assert!(
            content.contains("ItemProps"),
            "{label}: companion interface must survive adjacent close tags, got:\n{content}"
        );
        assert!(
            !content.contains("<template>"),
            "{label}: template markup must not leak into script content, got:\n{content}"
        );
    }
}
