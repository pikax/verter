use super::*;
use std::sync::Arc;

const LAZY_ANALYSIS_SFC: &str = r#"<template><div>{{ msg }}</div></template>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<style>
.foo { color: red; }
</style>"#;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn make_lazy_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::None,
        ..HostConfig::default()
    })
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    })
    .unwrap();
}

#[cfg(not(feature = "scheduler"))]
fn mutate_lazy_analysis_source(host: &VerterHost) {
    let mut files = crate::shared::write_lock(&host.files);
    let entry = files.get_mut("App.vue").expect("App.vue should exist");
    let broken = entry
        .source
        .replace("<script", "<scripx")
        .replace("</script>", "</scripx>")
        .replace("<style", "<styla")
        .replace("</style>", "</styla>");
    entry.source = Arc::from(broken);
}

#[cfg(not(feature = "scheduler"))]
fn clear_cached_parse(host: &VerterHost) {
    let mut files = crate::shared::write_lock(&host.files);
    let entry = files.get_mut("App.vue").expect("App.vue should exist");
    entry.cached_parse = None;
}

#[test]
fn build_eval_script_source_without_cached_parse_still_extracts_script_blocks() {
    let source = r#"<script lang="ts">
interface Props {
  label: string
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#;

    let extracted = VerterHost::build_eval_script_source(source, None);
    assert!(
        extracted.contains("interface Props"),
        "script content should be preserved without cached parse, got: {extracted}"
    );
    assert!(
        extracted.contains("defineProps<Props>()"),
        "script setup content should be preserved without cached parse, got: {extracted}"
    );
    assert!(
        !extracted.contains("<template>"),
        "template markup must not be passed into type evaluation, got: {extracted}"
    );
}

#[test]
fn imported_eval_inputs_parse_vue_dependency_key_aliases() {
    let host = make_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types/index.ts".to_string(),
            source: Arc::from("export * from '../Link.vue'\nexport * from '../Button.vue'"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    upsert_vue(
        &host,
        "/src/Link.vue",
        r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw' | 'custom'> {
  loading?: boolean
  label?: string
}
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'loading'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
    );

    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let snapshot = host
        .get_analysis_snapshot_internal("/src/App.vue", None)
        .expect("analysis snapshot should exist");
    let dep_resolutions = host.dependency_resolutions_for_eval("/src/App.vue");
    let inputs = host.imported_eval_inputs("/src/App.vue", &snapshot, &dep_resolutions);

    assert!(
        inputs
            .sources
            .iter()
            .any(|source| source.contains("export type LinkPropsKeys")),
        "tracked eval sources should include extracted LinkPropsKeys alias, got: {:?}",
        inputs.sources
    );

    let mut env = verter_analysis::type_eval_build::parse_and_build_env("");
    for dep_source in &inputs.sources {
        env.extend_missing(verter_analysis::type_eval_build::parse_and_build_env(
            dep_source,
        ));
    }
    let link_keys = env
        .type_symbols
        .get("LinkPropsKeys")
        .expect("eval env should contain LinkPropsKeys");
    assert!(
        matches!(
            link_keys.body,
            verter_analysis::type_expr::TypeExpr::Union(_)
        ),
        "LinkPropsKeys should lower to a literal union, got: {:?}",
        link_keys.body
    );
}

#[test]
fn evaluated_child_props_preserve_inherited_omit_fields_from_imported_key_aliases() {
    let host = make_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types/index.ts".to_string(),
            source: Arc::from("export * from '../Link.vue'\nexport * from '../Button.vue'"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    upsert_vue(
        &host,
        "/src/Link.vue",
        r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}

export type LinkPropsKeys = 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
}
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
    );

    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let (source, cached_parse, _) = host
        .current_eval_state("/src/App.vue")
        .expect("App eval state should exist");
    let eval_source = VerterHost::build_eval_script_source(&source, cached_parse.as_deref());
    let mut env = verter_analysis::type_eval_build::parse_and_build_env(&eval_source);
    let snapshot = host
        .get_analysis_snapshot_internal("/src/App.vue", None)
        .expect("analysis snapshot should exist");
    let dep_resolutions = host.dependency_resolutions_for_eval("/src/App.vue");
    let inputs = host.imported_eval_inputs("/src/App.vue", &snapshot, &dep_resolutions);
    for dep_source in &inputs.sources {
        env.extend_missing(verter_analysis::type_eval_build::parse_and_build_env(
            dep_source,
        ));
    }

    let child = env
        .type_symbols
        .get("ChildProps")
        .expect("ChildProps should exist")
        .body
        .clone();
    let evaluated = verter_analysis::type_eval::evaluate(&child, &mut env);
    let child_shape = match evaluated {
        verter_analysis::type_expr::TypeExpr::Object(obj) => obj,
        other => panic!("ChildProps should evaluate to an object, got: {other:?}"),
    };
    let names: Vec<String> = child_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            verter_analysis::type_expr::ObjectMember::Property(prop) => Some(prop.name.clone()),
            _ => None,
        })
        .collect();

    assert!(
        names.iter().any(|name| name == "loading"),
        "evaluated ChildProps should include inherited icon props, got: {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "label"),
        "evaluated ChildProps should include inherited button props, got: {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "href"),
        "evaluated ChildProps should include remaining link props, got: {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "status"),
        "evaluated ChildProps should keep local props, got: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name == "icon"),
        "evaluated ChildProps should omit icon props, got: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name == "replace"),
        "evaluated ChildProps should omit imported key alias members, got: {names:?}"
    );
}

#[test]
fn raw_template_analysis_extracts_css_var_names() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div :style=\"{ '--theme-color': color }\">A</div></template>",
    );

    let template = host
        .raw_template_analysis_for_file("/src/A.vue")
        .expect("raw template analysis should be computed");
    assert!(
        template
            .css_var_names
            .iter()
            .any(|name| name == "--theme-color"),
        "raw template analysis should include CSS vars from :style bindings"
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn override_template_analysis_helper_uses_content_override() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div>A</div></template>",
    );

    let profile = CompileProfile::default();
    let profile_hash = crate::hash::compile_profile_hash(&profile);
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/A.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Template,
                index: 0,
                code: Arc::from("<div :style=\"{ '--theme-color': color }\">A</div>"),
                source_map: None,
            }],
        })
        .expect("template override should succeed");

    let template = host
        .compute_override_template_analysis("/src/A.vue", profile_hash)
        .expect("override template analysis should be computed");
    assert!(
        template
            .css_var_names
            .iter()
            .any(|name| name == "--theme-color"),
        "override template analysis should reflect the overridden template"
    );
}

/// @ai-generated - get_analysis populates resolved_canonical_id for relative imports
#[test]
fn get_analysis_resolves_relative_import() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child msg=\"hello\" /></template>",
    );

    let analysis = host.get_analysis("/project/Parent.vue").unwrap();
    let child_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Child.vue")
        .unwrap();
    assert_eq!(
        child_import.resolved_canonical_id.as_deref(),
        Some("/project/Child.vue"),
        "relative import should resolve to canonical ID"
    );
}

/// @ai-generated - get_analysis resolves imports via alias map
#[test]
fn get_analysis_resolves_alias_import() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/src/components/Child.vue",
        "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child/></template>",
    );
    // Configure workspace resolver with alias
    {
        host.workspace().configure_resolver(vec![
            verter_analysis::project_resolver::IdeProjectConfig {
                root: "/project".to_string(),
                workspace_root: "/project".to_string(),
                tsconfig_path: None,
                provider_root: "/project".to_string(),
                workspace_aliases: vec![verter_vfs::WorkspaceAlias {
                    find: "@/".to_string(),
                    replacement: "/project/src/".to_string(),
                }],
                compiler_options:
                    verter_analysis::project_resolver::IdeProjectCompilerOptions::default(),
                references: vec![],
                membership: verter_analysis::project_resolver::ProjectMembership::MatchAll,
            },
        ]);
    }

    let analysis = host.get_analysis("/project/src/App.vue").unwrap();
    let child_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "@/components/Child.vue")
        .unwrap();
    assert_eq!(
        child_import.resolved_canonical_id.as_deref(),
        Some("/project/src/components/Child.vue"),
        "alias import should resolve via alias map"
    );
}

/// @ai-generated - get_analysis resolves imports with extension guessing
#[test]
fn get_analysis_resolves_extension_guessing() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child'\n</script>\n<template><Child/></template>",
    );

    let analysis = host.get_analysis("/project/Parent.vue").unwrap();
    let child_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Child")
        .unwrap();
    assert_eq!(
        child_import.resolved_canonical_id.as_deref(),
        Some("/project/Child.vue"),
        "extension-less import should resolve via .vue guessing"
    );
}

/// @ai-generated - get_analysis leaves bare specifiers unresolved
#[test]
fn get_analysis_bare_specifier_unresolved() {
    let host = make_host();
    upsert_vue(
        &host,
        "App.vue",
        "<script setup>\nimport { ref } from 'vue'\n</script>\n<template><div/></template>",
    );

    let analysis = host.get_analysis("App.vue").unwrap();
    let vue_import = analysis.imports.iter().find(|i| i.source == "vue").unwrap();
    assert!(
        vue_import.resolved_canonical_id.is_none(),
        "bare specifier 'vue' should not resolve (no node_modules resolution)"
    );
}

/// @ai-generated - get_analysis leaves unregistered file imports unresolved
#[test]
fn get_analysis_missing_file_unresolved() {
    let host = make_host();
    upsert_vue(
        &host,
        "App.vue",
        "<script setup>\nimport Missing from './Missing.vue'\n</script>\n<template><div/></template>",
    );

    let analysis = host.get_analysis("App.vue").unwrap();
    let missing_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Missing.vue")
        .unwrap();
    assert!(
        missing_import.resolved_canonical_id.is_none(),
        "import of unregistered file should not resolve"
    );
}

#[test]
fn get_analysis_uses_cached_parse_for_lazy_analysis() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, source is immutable in the scheduler snapshot,
    // so mutating host.files has no effect. The scheduler path reads from
    // HostSourceData.cached_parse directly. We just verify get_analysis()
    // returns correct lazy-recomputed data with AnalysisLevel::None.
    #[cfg(not(feature = "scheduler"))]
    mutate_lazy_analysis_source(&host);

    let analysis = host.get_analysis("App.vue").unwrap();

    assert!(
        analysis.bindings.iter().any(|b| b.name == "msg"),
        "lazy script analysis should reuse cached parse for bindings"
    );
    assert_eq!(
        analysis.styles.len(),
        1,
        "lazy style analysis should reuse cached parse for style blocks"
    );
    let css = analysis.styles[0]
        .css
        .as_ref()
        .expect("CSS analysis should exist for cached style block");
    assert!(
        css.classes.iter().any(|class| class.name == "foo"),
        "lazy style analysis should preserve CSS classes"
    );
    assert!(
        analysis
            .module_references
            .iter()
            .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
        "lazy script analysis should preserve module references"
    );
}

#[test]
fn get_analysis_falls_back_when_cached_parse_missing() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, cached_parse is immutable in HostSourceData
    // and always present for Vue SFCs. The scheduler path handles both
    // cached_parse present and absent cases. We just verify correctness.
    #[cfg(not(feature = "scheduler"))]
    clear_cached_parse(&host);

    let analysis = host.get_analysis("App.vue").unwrap();

    assert!(
        analysis.bindings.iter().any(|b| b.name == "msg"),
        "source fallback should still recover bindings"
    );
    assert_eq!(
        analysis.styles.len(),
        1,
        "source fallback should still recover style blocks"
    );
    let css = analysis.styles[0]
        .css
        .as_ref()
        .expect("CSS analysis should exist for fallback style block");
    assert!(
        css.classes.iter().any(|class| class.name == "foo"),
        "source fallback should preserve CSS classes"
    );
    assert!(
        analysis
            .module_references
            .iter()
            .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
        "source fallback should preserve module references"
    );
}

/// @ai-generated - get_export_span for .vue file returns binding span
#[test]
fn get_export_span_vue_binding() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    let span = host.get_export_span("Child.vue", "msg");
    assert!(span.is_some(), "should find 'msg' binding in .vue file");
    let (start, end) = span.unwrap();
    let source = host.get_source("Child.vue").unwrap();
    let spanned = &source[start as usize..end as usize];
    assert_eq!(spanned, "msg", "span should cover the binding identifier");
}

/// @ai-generated - get_export_span for .vue file returns None for unknown binding
#[test]
fn get_export_span_vue_unknown_binding() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    assert!(
        host.get_export_span("Child.vue", "nonexistent").is_none(),
        "unknown binding should return None"
    );
}

/// @ai-generated - get_export_span for .ts file returns export signature span
#[test]
fn get_export_span_ts_file() {
    let host = make_host();
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "utils.ts".to_string(),
        source: Arc::from("export function helper() { return 1; }"),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .unwrap();

    let span = host.get_export_span("utils.ts", "helper");
    assert!(span.is_some(), "should find 'helper' export in .ts file");
    let (start, end) = span.unwrap();
    let source = host.get_source("utils.ts").unwrap();
    let spanned = &source[start as usize..end as usize];
    assert_eq!(
        spanned, "helper",
        "span should cover the function identifier"
    );
}

/// @ai-generated - get_export_span for .vue default import finds first binding
#[test]
fn get_export_span_vue_default() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    let span = host.get_export_span("Child.vue", "default");
    assert!(
        span.is_some(),
        "default export of .vue should resolve to first binding"
    );
}

/// @ai-generated - resolve_import public method works
#[test]
fn resolve_import_public_method() {
    let host = make_host();
    upsert_vue(&host, "/project/Child.vue", "<template><div/></template>");
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
    );

    assert_eq!(
        host.resolve_import("/project/Parent.vue", "./Child.vue")
            .as_deref(),
        Some("/project/Child.vue")
    );
    // Bare specifiers that aren't in the file map resolve to None
    assert!(host
        .resolve_import("/project/Parent.vue", "lodash")
        .is_none());
}

#[test]
fn resolve_import_public_method_handles_relative_full_paths() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/src/components/BarrelComp.vue",
        "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
    );
    upsert_ts(
        &host,
        "/project/src/components/index.ts",
        "export { default as BarrelComp } from './BarrelComp.vue'",
    );
    upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport { BarrelComp } from './components'\n</script>\n<template><BarrelComp /></template>",
    );

    assert_eq!(
        host.resolve_import("/project/src/components/index.ts", "./BarrelComp.vue")
            .as_deref(),
        Some("/project/src/components/BarrelComp.vue"),
        "relative imports from full-path barrel files should resolve to the child SFC"
    );
}

fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .unwrap();
}

#[test]
fn enriches_destructured_composable_bindings() {
    let host = make_host();

    // Composable that returns { x: ref, y: ref, reset: function }
    upsert_ts(
        &host,
        "/project/useMouse.ts",
        r#"
import { ref } from 'vue'
export function useMouse() {
const x = ref(0)
const y = ref(0)
function reset() { x.value = 0; y.value = 0 }
return { x, y, reset }
}
"#,
    );

    // SFC that destructures the composable return
    upsert_vue(
        &host,
        "/project/App.vue",
        r#"<script setup>
import { useMouse } from './useMouse.ts'
const { x, y, reset } = useMouse()
</script>
<template><div>{{ x }} {{ y }}</div></template>"#,
    );

    let analysis = host.get_analysis("/project/App.vue").unwrap();

    // x and y should be enriched to Ref (from composable return shape)
    let x_binding = analysis.bindings.iter().find(|b| b.name == "x").unwrap();
    assert_eq!(
        x_binding.reactivity_kind,
        verter_analysis::ReactivityKind::Ref,
        "x should be enriched from MaybeRef to Ref via composable return shape"
    );

    let y_binding = analysis.bindings.iter().find(|b| b.name == "y").unwrap();
    assert_eq!(
        y_binding.reactivity_kind,
        verter_analysis::ReactivityKind::Ref,
        "y should be enriched from MaybeRef to Ref via composable return shape"
    );

    // reset should stay as a function (ReactivityKind::None since it's not reactive)
    let reset_binding = analysis
        .bindings
        .iter()
        .find(|b| b.name == "reset")
        .unwrap();
    assert_eq!(
        reset_binding.reactivity_kind,
        verter_analysis::ReactivityKind::None,
        "reset (a function) should be None, not reactive"
    );

    // Negative: non-enriched bindings should not be affected
    assert!(
        !x_binding.is_reactive
            || x_binding.reactivity_kind != verter_analysis::ReactivityKind::MaybeRef,
        "x should NOT remain MaybeRef after enrichment"
    );
}

#[test]
fn get_export_span_follows_reexport_to_vue() {
    let host = make_host();

    // Target: Popup.vue with a binding
    upsert_vue(
        &host,
        "/project/Popup.vue",
        "<script setup>\nconst message = 'hello'\n</script>\n<template><div>{{ message }}</div></template>",
    );

    // Barrel: index.ts re-exports Popup.vue as default
    upsert_ts(
        &host,
        "/project/index.ts",
        "export { default as Popup } from './Popup.vue'",
    );

    // Follow the re-export: "Popup" in index.ts → default in Popup.vue
    let result = host.get_export_span_follow_reexports("/project/index.ts", "Popup");

    assert!(result.is_some(), "should follow re-export to Popup.vue");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/Popup.vue",
        "should resolve to Popup.vue canonical ID"
    );
    assert!(
        start < end,
        "should have a valid span in Popup.vue (start={start}, end={end})"
    );
    // Negative: should NOT return index.ts
    assert_ne!(
        canonical_id, "/project/index.ts",
        "must NOT return the barrel file itself"
    );
}

#[test]
fn get_export_span_follows_reexport_to_vue_full_paths() {
    let host = make_host();

    upsert_vue(
        &host,
        "/project/src/components/BarrelComp.vue",
        "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
    );
    upsert_ts(
        &host,
        "/project/src/components/index.ts",
        "export { default as BarrelComp } from './BarrelComp.vue'",
    );

    let result =
        host.get_export_span_follow_reexports("/project/src/components/index.ts", "BarrelComp");

    assert!(
        result.is_some(),
        "should follow full-path barrel re-export to BarrelComp.vue"
    );
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/src/components/BarrelComp.vue",
        "should resolve to the full child Vue canonical ID"
    );
    assert!(start < end, "should return a valid span in BarrelComp.vue");
}

#[test]
fn get_export_span_follows_named_reexport() {
    let host = make_host();

    // Target: utils.ts with an exported function
    upsert_ts(
        &host,
        "/project/utils.ts",
        "export function helper() { return 42 }",
    );

    // Barrel: re-exports helper as myHelper
    upsert_ts(
        &host,
        "/project/index.ts",
        "export { helper as myHelper } from './utils.ts'",
    );

    let result = host.get_export_span_follow_reexports("/project/index.ts", "myHelper");

    assert!(result.is_some(), "should follow named re-export");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/utils.ts",
        "should resolve to utils.ts"
    );
    assert!(start < end, "should have a valid span");
    // Negative: should NOT return barrel
    assert_ne!(canonical_id, "/project/index.ts");
}

#[test]
fn get_export_span_follows_multi_hop_chain() {
    let host = make_host();

    upsert_ts(&host, "/project/a.ts", "export { b } from './b.ts'");
    upsert_ts(&host, "/project/b.ts", "export { c as b } from './c.ts'");
    upsert_ts(&host, "/project/c.ts", "export const c = 42");

    // Should follow a→b→c (no depth limit, cycle detection only)
    let result = host.get_export_span_follow_reexports("/project/a.ts", "b");
    assert!(result.is_some(), "should follow the chain");
    let (canonical_id, _, _) = result.unwrap();
    assert_eq!(canonical_id, "/project/c.ts", "should reach c.ts");
}

#[test]
fn get_export_span_local_export_unchanged() {
    let host = make_host();

    upsert_ts(&host, "utils.ts", "export function foo() { return 1 }");

    // Local export — no re-export, returns span in same file
    let result = host.get_export_span_follow_reexports("utils.ts", "foo");

    assert!(result.is_some(), "should find local export");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "utils.ts",
        "local export should return same file"
    );
    assert!(start < end, "should have a valid span");
}

#[test]
fn follow_reexport_cycle_same_binding() {
    let host = make_host();

    // A re-exports foo from B, B re-exports foo from A → cycle
    upsert_ts(&host, "a.ts", "export { foo } from './b.ts'");
    upsert_ts(&host, "b.ts", "export { foo } from './a.ts'");

    let result = host.get_export_span_follow_reexports("a.ts", "foo");
    assert!(
        result.is_none(),
        "cycle on same binding should return None, got: {result:?}"
    );
}

#[test]
fn follow_reexport_same_file_different_binding() {
    let host = make_host();

    // A re-exports foo from B (as foo→bar), B re-exports bar from A (as bar→baz),
    // A has a local baz export. Different bindings each hop → not a cycle.
    upsert_ts(
        &host,
        "/project/a.ts",
        "export { bar as foo } from './b.ts'\nexport const baz = 99",
    );
    upsert_ts(
        &host,
        "/project/b.ts",
        "export { baz as bar } from './a.ts'",
    );

    let result = host.get_export_span_follow_reexports("/project/a.ts", "foo");
    assert!(
        result.is_some(),
        "different bindings through same files should resolve, not be treated as cycle"
    );
    let (canonical_id, _, _) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/a.ts",
        "should resolve to a.ts local baz export"
    );
}

#[test]
fn follow_reexport_indirect_cycle() {
    let host = make_host();

    // A→B→C→A with same binding name "x" at each hop
    upsert_ts(&host, "a.ts", "export { x } from './b.ts'");
    upsert_ts(&host, "b.ts", "export { x } from './c.ts'");
    upsert_ts(&host, "c.ts", "export { x } from './a.ts'");

    let result = host.get_export_span_follow_reexports("a.ts", "x");
    assert!(
        result.is_none(),
        "indirect 3-file cycle should return None, got: {result:?}"
    );
}

#[test]
fn follow_reexport_deep_chain_no_limit() {
    let host = make_host();

    // 15-hop chain: f0→f1→f2→...→f14→terminal.ts
    // Each hop renames: val0→val1→...→val14→val
    for i in 0..15 {
        let next = if i < 14 {
            format!("f{}.ts", i + 1)
        } else {
            "terminal.ts".to_string()
        };
        let next_binding = if i < 14 {
            format!("val{}", i + 1)
        } else {
            "val".to_string()
        };
        let src = format!(
            "export {{ {} as val{} }} from './{}'",
            next_binding, i, next
        );
        upsert_ts(&host, &format!("/project/f{}.ts", i), &src);
    }
    upsert_ts(&host, "/project/terminal.ts", "export const val = 'done'");

    let result = host.get_export_span_follow_reexports("/project/f0.ts", "val0");
    assert!(
        result.is_some(),
        "15-hop chain should resolve without depth limit"
    );
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/terminal.ts",
        "should reach terminal.ts"
    );
    assert!(start < end, "should have a valid span");
}

fn compile_template(host: &VerterHost, id: &str) {
    host.get_virtual_file(crate::types::VirtualQuery {
        raw_id: Some(format!("{id}?vue&type=template")),
        canonical_id: None,
        node_kind: None,
        compile_profile: crate::types::CompileProfile::default(),
    })
    .unwrap();
}

#[test]
fn prop_shorthand_detected() {
    let host = make_host();
    upsert_vue(
        &host,
        "MyComp.vue",
        "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
    );
    // `:bar` with no value → shorthand; `:bar="bar"` → not shorthand
    upsert_vue(
        &host,
        "App.vue",
        r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar /><MyComp :bar="bar" /></template>"#,
    );
    compile_template(&host, "App.vue");

    let analysis = host.get_analysis("App.vue").unwrap();
    let tmpl = analysis
        .template
        .as_ref()
        .expect("should have template analysis");
    assert!(
        tmpl.components.len() >= 2,
        "should have at least 2 component usages, got {}",
        tmpl.components.len()
    );

    // First usage: `:bar` (shorthand)
    let comp1 = &tmpl.components[0];
    assert_eq!(comp1.props.len(), 1, "first usage has 1 prop");
    assert!(
        comp1.props[0].is_shorthand,
        "`:bar` (no value) should be shorthand"
    );

    // Second usage: `:bar="bar"` (not shorthand)
    let comp2 = &tmpl.components[1];
    assert_eq!(comp2.props.len(), 1, "second usage has 1 prop");
    assert!(
        !comp2.props[0].is_shorthand,
        "`:bar=\"bar\"` should NOT be shorthand"
    );
}

#[test]
fn prop_name_span_covers_name() {
    let host = make_host();
    upsert_vue(
        &host,
        "MyComp.vue",
        "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
    );
    let sfc = r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar="bar" foo="static" /></template>"#;
    upsert_vue(&host, "App.vue", sfc);
    compile_template(&host, "App.vue");

    let analysis = host.get_analysis("App.vue").unwrap();
    let tmpl = analysis
        .template
        .as_ref()
        .expect("should have template analysis");
    assert!(!tmpl.components.is_empty());

    let comp = &tmpl.components[0];
    // Find the bound prop `:bar`
    let bound_prop = comp.props.iter().find(|p| p.name == "bar").unwrap();
    let source = host.get_source("App.vue").unwrap();
    let name_text = &source[bound_prop.name_span.start as usize..bound_prop.name_span.end as usize];
    assert_eq!(
        name_text, "bar",
        "name_span should cover 'bar' (the arg, not ':')"
    );
    assert!(
        bound_prop.name_span.start >= bound_prop.span.start,
        "name_span should be within the full prop span"
    );

    // Find the static prop `foo`
    let static_prop = comp.props.iter().find(|p| p.name == "foo").unwrap();
    let name_text =
        &source[static_prop.name_span.start as usize..static_prop.name_span.end as usize];
    assert_eq!(name_text, "foo", "static prop name_span should cover 'foo'");
    assert!(
        !static_prop.is_shorthand,
        "static prop should not be shorthand"
    );
}

#[test]
fn arc_shared_fields_are_pointer_equal() {
    let host = make_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    let a1 = host.get_analysis("App.vue").unwrap();
    let a2 = host.get_analysis("App.vue").unwrap();

    // Arc-shared fields should be pointer-equal between two calls
    // on the same unchanged file.
    assert!(
        Arc::ptr_eq(&a1.module_references, &a2.module_references),
        "module_references should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.macros, &a2.macros),
        "macros should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.styles, &a2.styles),
        "styles should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.vue_api_calls, &a2.vue_api_calls),
        "vue_api_calls should be Arc-shared (pointer equal)"
    );
}

#[test]
fn enriched_imports_do_not_affect_stored_data() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\nconst x = 1\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
    );

    // First call: enriches imports with resolved_canonical_id
    let a1 = host.get_analysis("/project/Parent.vue").unwrap();
    assert!(
        a1.imports[0].resolved_canonical_id.is_some(),
        "enriched import should have resolved_canonical_id"
    );

    // Verify stored data is not mutated by checking that the
    // internal stored imports still have None
    #[cfg(feature = "scheduler")]
    {
        use crate::host_executor::HostSourceData;
        let source_snap = host
            .scheduler
            .try_get_source("/project/Parent.vue")
            .expect("scheduler should have Parent.vue");
        let hd = source_snap
            .downcast_data::<HostSourceData>()
            .expect("source data should be HostSourceData");
        assert!(
            hd.parse.script_analysis.imports[0]
                .resolved_canonical_id
                .is_none(),
            "stored import should NOT be mutated by get_analysis enrichment"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = crate::shared::read_lock(&host.files);
        let entry = files.get("/project/Parent.vue").unwrap();
        assert!(
            entry.script_analysis.imports[0]
                .resolved_canonical_id
                .is_none(),
            "stored import should NOT be mutated by get_analysis enrichment"
        );
    }
}

#[test]
fn get_analysis_batch_returns_all_existing() {
    let host = make_host();
    upsert_vue(
        &host,
        "A.vue",
        "<script setup>\nconst a = 1\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "B.vue",
        "<script setup>\nconst b = 2\n</script>\n<template><div/></template>",
    );

    let results = host.get_analysis_batch(&["A.vue", "B.vue", "NonExistent.vue"]);
    assert_eq!(results.len(), 2, "should return only existing files");
    assert!(
        results.iter().any(|(id, _)| id == "A.vue"),
        "should contain A.vue"
    );
    assert!(
        results.iter().any(|(id, _)| id == "B.vue"),
        "should contain B.vue"
    );
    // Negative: should NOT contain non-existent
    assert!(
        !results.iter().any(|(id, _)| id == "NonExistent.vue"),
        "should not contain non-existent file"
    );
}

#[test]
fn get_analysis_batch_matches_individual() {
    let host = make_host();
    upsert_vue(
        &host,
        "A.vue",
        "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><div/></template>",
    );

    let individual = host.get_analysis("A.vue").unwrap();
    let batch = host.get_analysis_batch(&["A.vue"]);
    assert_eq!(batch.len(), 1);
    let (_, batch_snap) = &batch[0];

    assert_eq!(
        individual.bindings.len(),
        batch_snap.bindings.len(),
        "batch bindings count should match individual"
    );
    assert_eq!(
        individual.imports.len(),
        batch_snap.imports.len(),
        "batch imports count should match individual"
    );
    assert_eq!(
        individual.script_flags, batch_snap.script_flags,
        "batch script_flags should match individual"
    );
}

#[test]
fn get_analysis_batch_empty_returns_empty() {
    let host = make_host();
    let results = host.get_analysis_batch(&[]);
    assert!(results.is_empty(), "empty batch should return empty vec");
}

// ── Export signature tests ──────────────────────────────────────

fn upsert_ts_result(host: &VerterHost, id: &str, src: &str) -> crate::HostUpdateResult {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .unwrap()
}

/// @ai-generated - upsert of .ts file returns export signatures
#[test]
fn upsert_returns_export_signatures_for_ts() {
    let host = make_host();
    let result = upsert_ts_result(
        &host,
        "index.ts",
        r#"export const foo = 1;
export type Bar = string;
export { default as Button } from './Button.vue';
"#,
    );

    assert!(
        !result.export_signatures.is_empty(),
        "upsert should return export signatures for .ts files"
    );

    let foo_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "foo")
        .expect("should have 'foo' export");
    assert!(!foo_sig.is_type, "foo is a value export");
    assert!(
        foo_sig.reexport_source.is_none(),
        "foo is local, not a re-export"
    );

    let bar_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "Bar")
        .expect("should have 'Bar' export");
    assert!(bar_sig.is_type, "Bar is a type export");

    let button_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "Button")
        .expect("should have 'Button' re-export");
    assert_eq!(
        button_sig.reexport_source.as_deref(),
        Some("./Button.vue"),
        "Button re-export source should be './Button.vue'"
    );
    assert_eq!(
        button_sig.reexport_local.as_deref(),
        Some("default"),
        "Button re-export local name should be 'default'"
    );
}

/// @ai-generated - get_analysis includes export signatures
#[test]
fn get_analysis_includes_export_signatures() {
    let host = make_host();
    upsert_ts(
        &host,
        "utils.ts",
        "export function helper() { return 1; }\nexport type Util = number;",
    );

    let analysis = host.get_analysis("utils.ts").unwrap();
    assert!(
        !analysis.export_signatures.is_empty(),
        "analysis should include export signatures"
    );

    let helper_sig = analysis
        .export_signatures
        .iter()
        .find(|s| s.name == "helper")
        .expect("should have 'helper' export");
    assert!(!helper_sig.is_type);

    let util_sig = analysis
        .export_signatures
        .iter()
        .find(|s| s.name == "Util")
        .expect("should have 'Util' export");
    assert!(util_sig.is_type);
}

/// @ai-generated - resolve_exports follows re-export chains
#[test]
fn resolve_exports_follows_reexport_chains() {
    let host = make_host();

    upsert_vue(
        &host,
        "/project/Button.vue",
        "<script setup>\ndefineProps({ label: String })\n</script>\n<template><button>{{ label }}</button></template>",
    );

    upsert_ts(
        &host,
        "/project/components/index.ts",
        "export { default as Button } from './Button.vue';",
    );

    // Set up dependency so ./Button.vue resolves from components/index.ts
    host.set_import_dependencies(
        "/project/components/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./Button.vue".to_string(),
            resolved_canonical_id: Some("/project/Button.vue".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/components/index.ts");
    assert!(
        !exports.is_empty(),
        "barrel file should have resolved exports"
    );

    let button = exports
        .iter()
        .find(|e| e.name == "Button")
        .expect("should have 'Button' resolved export");
    assert_eq!(
        button.source_canonical_id.as_deref(),
        Some("/project/Button.vue"),
        "Button should resolve to Button.vue"
    );
    assert_eq!(
        button.source_name, "default",
        "Button maps to 'default' in the source file"
    );
}

/// @ai-generated - resolve_exports handles direct local exports
#[test]
fn resolve_exports_local_exports() {
    let host = make_host();
    upsert_ts(
        &host,
        "utils.ts",
        "export const FOO = 1;\nexport type Bar = string;",
    );

    let exports = host.resolve_exports("utils.ts");
    assert_eq!(exports.len(), 2, "should have 2 exports");

    let foo = exports.iter().find(|e| e.name == "FOO").unwrap();
    assert!(
        foo.source_canonical_id.is_none(),
        "local export has no source file"
    );
    assert_eq!(foo.source_name, "FOO");
    assert!(!foo.is_type);

    let bar = exports.iter().find(|e| e.name == "Bar").unwrap();
    assert!(bar.is_type);
}

/// @ai-generated - resolve_exports handles wildcard re-exports
#[test]
fn resolve_exports_wildcard_reexports() {
    let host = make_host();

    upsert_ts(
        &host,
        "/project/types.ts",
        "export type Foo = string;\nexport type Bar = number;",
    );
    upsert_ts(&host, "/project/index.ts", "export * from './types';");

    host.set_import_dependencies(
        "/project/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/project/types.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/index.ts");
    assert!(
        exports.iter().any(|e| e.name == "Foo"),
        "wildcard re-export should include Foo"
    );
    assert!(
        exports.iter().any(|e| e.name == "Bar"),
        "wildcard re-export should include Bar"
    );

    let foo = exports.iter().find(|e| e.name == "Foo").unwrap();
    assert_eq!(
        foo.source_canonical_id.as_deref(),
        Some("/project/types.ts"),
        "Foo should trace back to types.ts"
    );
}

/// @ai-generated - resolve_exports detects circular re-exports
#[test]
fn resolve_exports_circular_protection() {
    let host = make_host();

    upsert_ts(&host, "a.ts", "export * from './b';");
    upsert_ts(&host, "b.ts", "export * from './a';");

    host.set_import_dependencies(
        "a.ts",
        vec![crate::DependencyResolution {
            specifier: "./b".to_string(),
            resolved_canonical_id: Some("b.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    host.set_import_dependencies(
        "b.ts",
        vec![crate::DependencyResolution {
            specifier: "./a".to_string(),
            resolved_canonical_id: Some("a.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Should not infinite loop
    let exports = host.resolve_exports("a.ts");
    // The result is empty because both files only re-export each other with no local exports
    assert!(
        exports.is_empty(),
        "circular re-exports with no local exports should return empty"
    );
}

/// @ai-generated - resolve_exports multi-level barrel chain
#[test]
fn resolve_exports_multi_level_barrel() {
    let host = make_host();

    upsert_ts(&host, "/project/deep.ts", "export const DEEP = 42;");
    upsert_ts(&host, "/project/mid.ts", "export { DEEP } from './deep';");
    upsert_ts(&host, "/project/top.ts", "export { DEEP } from './mid';");

    host.set_import_dependencies(
        "/project/mid.ts",
        vec![crate::DependencyResolution {
            specifier: "./deep".to_string(),
            resolved_canonical_id: Some("/project/deep.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    host.set_import_dependencies(
        "/project/top.ts",
        vec![crate::DependencyResolution {
            specifier: "./mid".to_string(),
            resolved_canonical_id: Some("/project/mid.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/top.ts");
    let deep = exports
        .iter()
        .find(|e| e.name == "DEEP")
        .expect("should have DEEP");
    assert_eq!(
        deep.source_canonical_id.as_deref(),
        Some("/project/deep.ts"),
        "should trace through two levels to deep.ts"
    );
}

#[test]
fn get_semantic_hash_returns_hash_for_loaded_file() {
    let host = make_host();
    upsert_vue(&host, "App.vue", "<template><div>hi</div></template>");
    let hash = host.get_semantic_hash("App.vue");
    assert!(hash.is_some(), "loaded file should return a semantic hash");
    assert_ne!(hash.unwrap(), [0u8; 16], "hash should not be all zeros");
}

#[test]
fn get_semantic_hash_returns_none_for_missing_file() {
    let host = make_host();
    assert!(
        host.get_semantic_hash("nonexistent.vue").is_none(),
        "missing file should return None"
    );
}

#[test]
fn get_semantic_hash_changes_on_content_change() {
    let host = make_host();
    upsert_vue(&host, "App.vue", "<template><div>a</div></template>");
    let h1 = host.get_semantic_hash("App.vue").unwrap();
    upsert_vue(&host, "App.vue", "<template><div>b</div></template>");
    let h2 = host.get_semantic_hash("App.vue").unwrap();
    assert_ne!(h1, h2, "semantic hash should change when content changes");
}

fn resolve_expanded_state(
    host: &VerterHost,
    canonical_or_alias: &str,
) -> crate::meta_resolve::ResolvedComponentMetaState {
    host.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)
        .expect("expanded resolved state should exist")
}

fn resolved_macro_by_type<'a>(
    state: &'a crate::meta_resolve::ResolvedComponentMetaState,
    type_name: &str,
) -> &'a crate::meta_resolve::ResolvedMacroMeta {
    state
        .resolved_macros
        .iter()
        .find(|meta| meta.type_name == type_name)
        .unwrap_or_else(|| panic!("missing resolved macro for {type_name}"))
}

#[test]
fn resolve_imported_type_from_ts_dep() {
    let host = make_host();
    // Upsert the .ts type file
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ButtonProps { label: string; size?: number }",
    );
    // Upsert the .vue file that imports from ./types
    upsert_vue(
        &host,
        "/Button.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script><template><div /></template>"#,
    );

    let state = resolve_expanded_state(&host, "/Button.vue");
    let resolved = resolved_macro_by_type(&state, "ButtonProps");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();

    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        props.contains(&"size"),
        "expanded props should contain 'size', got: {:?}",
        props
    );
}

#[test]
fn resolve_component_meta_returns_no_resolved_macros_for_no_imported_type_deps() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Simple.vue",
        r#"<script setup lang="ts">
defineProps<{ count: number }>()
</script><template><div /></template>"#,
    );
    let state = resolve_expanded_state(&host, "/Simple.vue");
    assert!(
        state.resolved_macros.is_empty(),
        "should not resolve any cross-file macros when there are no imported type deps"
    );
}

#[test]
fn resolve_imported_type_from_vue_dep() {
    let host = make_host();
    upsert_vue(
        &host,
        "/types.vue",
        "<script setup lang=\"ts\">export interface Props { label: string }</script>\n<template><div /></template>",
    );
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types.vue'\ndefineProps<Props>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let resolved = resolved_macro_by_type(&state, "Props");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        !resolved
            .declaration
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("<template>"),
        "declaration text must not leak raw SFC markup, got: {:?}",
        resolved.declaration.text
    );
}

#[test]
fn resolve_imported_type_from_dual_script_vue_dep() {
    let host = make_host();
    upsert_vue(
        &host,
        "/types.vue",
        "<script lang=\"ts\">\nexport interface DualProps { title: string; count: number }\n</script>\n<script setup lang=\"ts\">\n// empty setup block\n</script>\n<template><div /></template>",
    );
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { DualProps } from './types.vue'\ndefineProps<DualProps>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let resolved = resolved_macro_by_type(&state, "DualProps");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        props.contains(&"title"),
        "expanded props should contain 'title' from companion script, got: {:?}",
        props
    );
}

#[test]
fn resolve_imported_type_from_vue_dep_without_vue_suffix_uses_file_kind() {
    let host = make_host();
    // Use .vue extension so that VFS resolution can resolve the import.
    // The test verifies that Vue SFC script extraction works for deps
    // that are stored with VueSfc file kind.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/types.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">export interface Props { label: string }</script>\n<template><div /></template>",
            ),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types.vue'\ndefineProps<Props>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let resolved = resolved_macro_by_type(&state, "Props");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        !resolved
            .declaration
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("<template>"),
        "declaration text must NOT contain raw SFC markup, got: {:?}",
        resolved.declaration.text
    );
}

#[test]
fn resolve_component_meta_uses_workspace_type_resolution_for_package_declarations() {
    let ws = Arc::new(verter_vfs::MemoryWorkspace::new(
        verter_vfs::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean; label?: string }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        "<script setup lang=\"ts\">\nimport type { FancyProps } from 'fancy'\ndefineProps<FancyProps>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/workspace/src/Consumer.vue");
    let resolved = resolved_macro_by_type(&state, "FancyProps");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        props.contains(&"open"),
        "expanded props should contain fields from the package declaration entrypoint, got: {:?}",
        props
    );
}

// ═══════════════════════════════════════════════════════════
// enrich_imported_types tests
// ═══════════════════════════════════════════════════════════

fn upsert_non_sfc(host: &VerterHost, id: &str, src: &str) {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .unwrap();
}

/// resolve_component_meta(Expanded) populates prop fields from imported interface
#[test]
fn enrich_basic_imported_interface() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let props: Vec<&str> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "props should include 'label': {:?}",
        props
    );
    // Negative: get_analysis must NOT have enriched the snapshot
    let analysis = host.get_analysis("/src/Comp.vue").unwrap();
    let dp = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert!(
        dp.prop_fields.is_empty(),
        "get_analysis must NOT enrich prop_fields"
    );
}

/// resolve_component_meta(Expanded) merges props from intersection types
#[test]
fn enrich_intersection_merges_all_deps() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/a.ts", "export interface A { x: string }");
    upsert_non_sfc(&host, "/src/b.ts", "export interface B { y: number }");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { A } from './a'
import type { B } from './b'
defineProps<A & B>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let names: Vec<&str> = state
        .resolved_macros
        .iter()
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.as_str())
        .collect();
    assert!(names.contains(&"x"), "should have 'x' from A: {:?}", names);
    assert!(names.contains(&"y"), "should have 'y' from B: {:?}", names);
}

/// resolve_component_meta(Expanded) wraps call-signature emit payloads in brackets
#[test]
fn enrich_emit_call_signature_wraps_brackets() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/events.ts",
        "export interface Events { (e: 'change', id: number): void }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let emits: Vec<_> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineEmits)
        .flat_map(|m| m.emits.iter())
        .collect();
    let change = emits.iter().find(|e| e.name == "change");
    assert!(change.is_some(), "should have 'change' emit");
    let payload = change.unwrap().payload_type.as_deref().unwrap_or("");
    assert!(
        payload.starts_with('[') && payload.ends_with(']'),
        "call-signature payload should be wrapped in brackets, got: {payload}"
    );
}

/// resolve_component_meta(Expanded) extracts slot bindings from imported type
#[test]
fn enrich_slot_bindings_from_imported_type() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default: (props: { row: string; index: number }) => any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let slots: Vec<_> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
        .collect();
    let default_slot = slots.iter().find(|s| s.name == "default");
    assert!(default_slot.is_some(), "should have 'default' slot");
    let bindings = &default_slot.unwrap().bindings;
    assert!(!bindings.is_empty(), "slot should have bindings");
    let binding_names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(
        binding_names.contains(&"row"),
        "should have 'row': {:?}",
        binding_names
    );
    assert!(
        binding_names.contains(&"index"),
        "should have 'index': {:?}",
        binding_names
    );
}

/// resolve_component_meta(Expanded) captures method-style slot signatures
#[test]
fn enrich_slot_method_style() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default(props: { item: string }): any; header(props: { title: string }): any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let slot_names: Vec<&str> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        slot_names.contains(&"default"),
        "should have 'default': {:?}",
        slot_names
    );
    assert!(
        slot_names.contains(&"header"),
        "should have 'header': {:?}",
        slot_names
    );
}

/// resolve_component_meta(Expanded) resolves nested type references
#[test]
fn enrich_nested_type_expansion() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"export type Status = 'active' | 'inactive'
export interface Props { name: string; status: Status }"#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let prop_names: Vec<&str> = state
        .resolved_macros
        .iter()
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"name"),
        "should have 'name': {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"status"),
        "should have 'status': {:?}",
        prop_names
    );
    // Negative: props should not contain 'Status' as a prop (it's a type, not a prop)
    assert!(
        !prop_names.contains(&"Status"),
        "Status is a type, not a prop"
    );
}

/// resolve_component_meta(Expanded) extracts slot return types
#[test]
fn enrich_slot_return_type_property_style() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default: (props: { row: string }) => VNode[]; header: (props: {}) => any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let slots: Vec<_> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
        .collect();

    let default_slot = slots.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "default slot should have return type VNode[]"
    );

    let header_slot = slots.iter().find(|s| s.name == "header").unwrap();
    assert_eq!(
        header_slot.return_type.as_deref(),
        Some("any"),
        "header slot should have return type any"
    );
}

/// @ai-generated - local defineSlots with return types
#[test]
fn local_slot_return_type_property_style() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
defineSlots<{
  default: (props: { item: string }) => VNode[]
  header: (props: {}) => any
}>()
</script>
<template><div /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let ds = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
        .expect("should have DefineSlots macro");

    let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "local default slot should have return type"
    );
}

/// @ai-generated - local defineSlots with method-style return types
#[test]
fn local_slot_return_type_method_style() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): VNode[]
}>()
</script>
<template><div /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let ds = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
        .expect("should have DefineSlots macro");

    let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "method-style slot should have return type"
    );
}

// ═══════════════════════════════════════════════════════════
// Template slots via lazy analysis (compute_template_analysis_if_missing)
// ═══════════════════════════════════════════════════════════

/// @ai-generated - template slots detected via lazy META compilation
#[test]
fn template_slots_via_analysis_only() {
    let host = make_host(); // analysis_level: Full → scope includes template
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template analysis should be populated");
    assert_eq!(tpl.defined_slots.len(), 1);
    assert_eq!(tpl.defined_slots[0].name, "default");
}

/// @ai-generated - named slots detected via lazy META compilation
#[test]
fn template_slots_named() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<template><slot name="header" /><slot /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template analysis should be populated");
    assert_eq!(tpl.defined_slots.len(), 2);
    assert!(tpl.defined_slots.iter().any(|s| s.name == "header"));
    assert!(tpl.defined_slots.iter().any(|s| s.name == "default"));
}

/// @ai-generated - template analysis not computed when scope doesn't include template
#[test]
fn template_slots_not_computed_on_lazy_host() {
    let host = make_lazy_host(); // analysis_level: None → scope excludes template
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    assert!(
        analysis.template.is_none(),
        "template should not be computed when scope excludes it"
    );
}

/// @ai-generated - persisted template analysis reused on second call
#[test]
fn template_slots_persisted_across_calls() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let a1 = host.get_analysis("/Comp.vue").unwrap();
    assert!(a1.template.is_some(), "first call should compute template");

    let a2 = host.get_analysis("/Comp.vue").unwrap();
    assert!(
        a2.template.is_some(),
        "second call should reuse persisted template"
    );
    assert_eq!(
        a2.template.unwrap().defined_slots.len(),
        1,
        "persisted template should have the slot"
    );
}

/// @ai-generated - template slots computed even when type deps are unresolved
#[test]
fn template_slots_with_unresolved_type_deps() {
    let host = make_host();
    // Don't upsert ./types.ts — the dep is unresolved
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<Foo>()
</script>
<template><slot /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template should be computed even with unresolved type deps");
    assert_eq!(
        tpl.defined_slots.len(),
        1,
        "should detect the <slot> despite unresolved type dep"
    );
}

// ── Fix 1: effective_target + resolved_dependency_targets ──────────

#[test]
fn effective_target_returns_resolved_when_present() {
    let res = crate::types::DependencyResolution {
        specifier: "./types".to_string(),
        resolved_canonical_id: Some("/src/types.ts".to_string()),
        possible_canonical_ids: vec!["/src/types.js".to_string(), "/src/types.d.ts".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/types.ts"),
        "resolved_canonical_id should win over possibles"
    );
}

#[test]
fn effective_target_picks_dts_over_ts_over_js() {
    let res = crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec![
            "/src/utils.js".to_string(),
            "/src/utils.ts".to_string(),
            "/src/utils.d.ts".to_string(),
        ],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/utils.d.ts"),
        ".d.ts should have highest priority"
    );
}

#[test]
fn effective_target_picks_ts_over_js() {
    let res = crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/src/utils.jsx".to_string(), "/src/utils.tsx".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/utils.tsx"),
        ".tsx should win over .jsx"
    );
}

#[test]
fn effective_target_returns_none_when_empty() {
    let res = crate::types::DependencyResolution {
        specifier: "./missing".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: Vec::new(),
    };
    assert_eq!(res.effective_target(), None);
}

#[test]
fn effective_target_vue_only_when_no_script_candidates() {
    let res = crate::types::DependencyResolution {
        specifier: "./Comp".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/src/Comp.vue".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/Comp.vue"),
        ".vue should be returned when it is the only candidate"
    );
}

#[test]
fn effective_target_prefers_dcts_over_cjs() {
    let res = crate::types::DependencyResolution {
        specifier: "./lib".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/lib/index.cjs".to_string(), "/lib/index.d.cts".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/lib/index.d.cts"),
        ".d.cts should win over .cjs"
    );
}

#[test]
fn resolved_dependency_targets_uses_effective_target() {
    let mut dep_resolutions = rustc_hash::FxHashMap::default();
    // Resolved: should use resolved_canonical_id only
    dep_resolutions.insert(
        "./types".to_string(),
        crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: vec!["/src/types.js".to_string()],
        },
    );
    // Unresolved: should use highest-priority possible
    dep_resolutions.insert(
        "./utils".to_string(),
        crate::types::DependencyResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![
                "/src/utils.js".to_string(),
                "/src/utils.d.ts".to_string(),
            ],
        },
    );
    // No resolution at all
    dep_resolutions.insert(
        "./missing".to_string(),
        crate::types::DependencyResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        },
    );

    let targets = VerterHost::resolved_dependency_targets(&dep_resolutions);

    assert!(
        targets.contains("/src/types.ts"),
        "should include resolved ID"
    );
    assert!(
        !targets.contains("/src/types.js"),
        "should NOT include possibles when resolved exists"
    );
    assert!(
        targets.contains("/src/utils.d.ts"),
        "should include highest-priority possible"
    );
    assert!(
        !targets.contains("/src/utils.js"),
        "should NOT include lower-priority possible"
    );
    assert_eq!(targets.len(), 2, "missing should not contribute a target");
}
