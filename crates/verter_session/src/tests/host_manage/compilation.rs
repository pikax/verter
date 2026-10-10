use super::*;

// Legacy trace-line formatting tests 5 ( clean-cut rule). `format_component_meta_trace_line`,
// `ComponentMetaTraceEvent`, and `ComponentMetaTraceLine` no longer
// exist; their replacement is `StructuredAuditEvent` tested
// in `component_meta_audit/structured_event.rs`.

#[test]
fn build_eval_script_source_without_parse_artifact_is_zero_work() {
    let source = r#"<script lang="ts">
interface Props {
  label: string
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#;

    assert!(
        VerterHost::build_eval_script_source("/App.vue", source, None).is_none(),
        "a classified carrier without its parse artifact must refuse before parse or publication"
    );
}

/// Extraction is gated on the file's LANGUAGE CLASSIFICATION, never on the
/// raw text: a NON-CARRIER file (`.ts` / `.d.ts`) whose text contains a
/// `<script ...>` ... `</script>` pair (a JSDoc `@example` block — the
/// vue-router@5 / @regle/core / unhead dist shape) passes through UNCHANGED.
/// The former unconditional forgiving raw scan blanked such a file down to
/// its documentation example, destroying its whole type surface.
#[test]
fn build_eval_script_source_never_script_scans_a_non_carrier_file() {
    let source = r#"/**
 * Usage example:
 * ```vue
 * <script setup>
 * const value = useReal()
 * </script>
 * ```
 */
export type Real = string | { path: string }
"#;

    for canonical in ["/dep.ts", "/dep.d.ts", "/dep.tsx", "/dep.mjs"] {
        let (eval, extracted) =
            VerterHost::build_eval_script_source_with_extraction(canonical, source, None)
                .unwrap_or_else(|| panic!("{canonical}: a non-carrier file passes through"));
        assert!(
            !extracted,
            "{canonical}: a non-carrier file must never report script extraction"
        );
        assert_eq!(
            eval.as_ref(),
            source,
            "{canonical}: a non-carrier file's source passes through unchanged"
        );
    }

    // Control: the SAME text under a carrier canonical without its parse
    // artifact is typed refusal — never a raw script scan and never a
    // blanked IndexedReady body.
    assert!(
        VerterHost::build_eval_script_source_with_extraction("/Doc.vue", source, None).is_none(),
        "an artifact-less carrier must refuse eval-source rather than blank or scan"
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
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let slot_names = hm_slot_names(&host, "/src/Comp.vue", &state);
    assert!(
        slot_names.contains(&"default".to_string()),
        "should have 'default': {:?}",
        slot_names
    );
    assert!(
        slot_names.contains(&"header".to_string()),
        "should have 'header': {:?}",
        slot_names
    );
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

#[test]
fn template_class_facts_accept_exact_vue_wrappers_and_reject_local_fakes() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        r#"
export interface Ref<T> { value: T }
export interface ShallowRef<T> { value: T }
export interface ComputedRef<T> { readonly value: T }
export interface WritableComputedRef<T> { value: T }
export type Reactive<T> = T
export type ShallowReactive<T> = T
"#,
    );
    let wrappers = [
        "Ref",
        "ShallowRef",
        "ComputedRef",
        "WritableComputedRef",
        "Reactive",
        "ShallowReactive",
    ];
    for (index, wrapper) in wrappers.into_iter().enumerate() {
        let canonical = format!("/workspace/src/Wrapper{index}.vue");
        upsert_vue(
            &host,
            &canonical,
            &format!(
                r#"<script setup lang="ts">
import type {{ {wrapper} as VueWrapper }} from 'vue'
const variant: VueWrapper<'primary' | 'secondary'> = null as never
</script><template><div :class="variant" /></template>"#
            ),
        );
        host.set_import_dependencies(
            &canonical,
            vec![exact_dependency(
                "vue",
                "/workspace/node_modules/vue/index.d.ts",
            )],
        );
        let analysis = host.get_analysis(&canonical).expect("analysis");
        let template = analysis.template.expect("template");
        let classes = template
            .elements
            .iter()
            .flat_map(|element| element.dynamic_classes.iter().map(String::as_str))
            .collect::<Vec<_>>();
        assert_eq!(classes, ["primary", "secondary"], "{wrapper}");
    }

    upsert_ts(
        &host,
        "/workspace/src/vue-types.ts",
        "export type { Ref as IndirectWrapper } from 'vue'",
    );
    host.set_import_dependencies(
        "/workspace/src/vue-types.ts",
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    upsert_vue(
        &host,
        "/workspace/src/Indirect.vue",
        r#"<script setup lang="ts">
import type { IndirectWrapper as LocalWrapper } from './vue-types'
const variant: LocalWrapper<'primary' | 'secondary'> = null as never
</script><template><div :class="variant" /></template>"#,
    );
    host.set_import_dependencies(
        "/workspace/src/Indirect.vue",
        vec![exact_dependency(
            "./vue-types",
            "/workspace/src/vue-types.ts",
        )],
    );
    let indirect = host
        .get_analysis("/workspace/src/Indirect.vue")
        .expect("analysis");
    let indirect_template = indirect.template.expect("template");
    assert_eq!(
        indirect_template.elements[0].dynamic_classes,
        ["primary", "secondary"]
    );

    upsert_vue(
        &host,
        "/workspace/src/Fake.vue",
        r#"<script setup lang="ts">
type Ref<T> = { value: T }
const variant: Ref<'primary' | 'secondary'> = null as never
</script><template><div :class="variant" /></template>"#,
    );
    let fake = host
        .get_analysis("/workspace/src/Fake.vue")
        .expect("analysis");
    assert!(
        fake.template
            .expect("template")
            .elements
            .iter()
            .all(|element| element.dynamic_classes.is_empty()),
        "a local same-name wrapper must not be treated as Vue provenance"
    );

    upsert_non_sfc(
        &host,
        "/workspace/node_modules/not-vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_vue(
        &host,
        "/workspace/src/PackageFake.vue",
        r#"<script setup lang="ts">
import type { Ref } from 'not-vue'
const variant: Ref<'primary' | 'secondary'> = null as never
</script><template><div :class="variant" /></template>"#,
    );
    host.set_import_dependencies(
        "/workspace/src/PackageFake.vue",
        vec![exact_dependency(
            "not-vue",
            "/workspace/node_modules/not-vue/index.d.ts",
        )],
    );
    let package_fake = host
        .get_analysis("/workspace/src/PackageFake.vue")
        .expect("analysis");
    assert!(
        package_fake
            .template
            .expect("template")
            .elements
            .iter()
            .all(|element| element.dynamic_classes.is_empty()),
        "a package-backed same-shape wrapper outside the exact Vue route must fail closed"
    );
}

#[test]
fn template_class_facts_classify_fully_substituted_terminal_wrapper_inner() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    let cases = [
        (
            "/workspace/src/Transparent.vue",
            "type Wrapped<T> = Ref<T>; const variant: Wrapped<'primary' | 'secondary'> = null as never",
            vec!["primary", "secondary"],
        ),
        (
            "/workspace/src/Nullable.vue",
            "type Wrapped<T> = Ref<T | null>; const variant: Wrapped<'primary'> = null as never",
            vec![],
        ),
        (
            "/workspace/src/Reordered.vue",
            "type Wrapped<Noise, Value> = Ref<Value>; const variant: Wrapped<number, 'primary' | 'secondary'> = null as never",
            vec!["primary", "secondary"],
        ),
        (
            "/workspace/src/MultiHop.vue",
            "type First<T> = Ref<T>; type Second<T> = First<T>; const variant: Second<'primary' | 'secondary'> = null as never",
            vec!["primary", "secondary"],
        ),
        (
            "/workspace/src/TransformedMultiHop.vue",
            "type First<T> = Ref<T | string>; type Second<T> = First<T>; const variant: Second<'primary'> = null as never",
            vec![],
        ),
    ];
    for (canonical, declarations, expected) in cases {
        upsert_vue(
            &host,
            canonical,
            &format!(
                r#"<script setup lang="ts">
import type {{ Ref }} from 'vue'
{declarations}
</script><template><div :class="variant" /></template>"#
            ),
        );
        host.set_import_dependencies(
            canonical,
            vec![exact_dependency(
                "vue",
                "/workspace/node_modules/vue/index.d.ts",
            )],
        );
        let template = host
            .get_analysis(canonical)
            .expect("analysis")
            .template
            .expect("template");
        assert_eq!(
            template.elements[0].dynamic_classes, expected,
            "terminal substitution mismatch for {canonical}"
        );
    }
}

#[test]
fn template_class_wrapper_artifact_binds_each_duplicate_terminal_route_exactly() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    let canonical = "/workspace/src/DuplicateRoutes.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { Ref as A, Ref as B } from 'vue'
type Wrapped<T> = B<T>
const viaA: A<'a'> = null as never
const viaB: Wrapped<'b'> = null as never
</script><template><div :class="viaA" /><div :class="viaB" /></template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let _ = host.get_analysis(canonical).expect("analysis");
    let facts = template_class_facts_for(&host, canonical);
    assert_eq!(facts.rows().len(), 2);
    let first = &facts.rows()[0];
    let second = &facts.rows()[1];
    for (row, label, local, aliases, value) in [
        (first, "viaA", "A", Vec::<&str>::new(), "a"),
        (second, "viaB", "B", vec!["Wrapped"], "b"),
    ] {
        assert_eq!(row.subject.label(), label);
        assert_eq!(row.wrapper.role, verter_type_expr::ReactiveWrapperRole::Ref);
        assert_eq!(
            row.wrapper
                .symbol
                .as_ref()
                .expect("terminal")
                .symbol
                .as_ref(),
            "Ref"
        );
        let provenance = row
            .wrapper
            .import_provenance
            .as_ref()
            .expect("exact route provenance");
        assert_eq!(provenance.local_binding.as_ref(), local);
        assert_eq!(provenance.import_source.as_ref(), "vue");
        assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
        assert_eq!(provenance.imported_name.as_ref(), "Ref");
        assert_eq!(
            provenance
                .local_alias_hops
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<&str>>(),
            aliases
        );
        assert_eq!(
            row.wrapper.inner_domain,
            verter_type_expr::ClosedLiteralDomain::Strings(Arc::from([Arc::<str>::from(value)]))
        );
        assert_eq!(row.domain, row.wrapper.inner_domain);
        assert!(row.wrapper.inner_source.is_some());
        assert_eq!(
            row.wrapper.completeness,
            verter_session_query::analysis::template_class_facts::TemplateClassFactsCompleteness::Complete
        );
    }
}

/// `ModelRef` is part of the closed Vue reactive-wrapper vocabulary.
///
/// `defineModel` is the one wrapper source Verter itself synthesises, so
/// omitting its wrapper type from the role vocabulary was a silent gap: an
/// exactly-routed `vue` `ModelRef<'a' | 'b'>` annotation published no domain.
/// The negative half still holds — a same-shaped LOCAL `ModelRef` has no
/// package-backed `vue` import edge, so it claims no route and publishes nothing.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn template_class_model_ref_is_in_the_wrapper_vocabulary() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface ModelRef<T> { value: T }\n",
    );
    let canonical = "/workspace/src/ModelWrapper.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { ModelRef } from 'vue'
type LocalModelRef<T> = { value: T }
const modelled: ModelRef<'model-a' | 'model-b'> = null as never
const fake: LocalModelRef<'fake'> = null as never
const inferred = defineModel<'infer-a' | 'infer-b'>()
</script><template>
  <div :class="modelled" />
  <span :class="fake" />
  <em :class="inferred" />
</template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );

    let template = host
        .get_analysis(canonical)
        .expect("lazy analysis")
        .template
        .expect("template");
    assert_eq!(
        template.elements[0].dynamic_classes,
        ["model-a", "model-b"],
        "an exact vue `ModelRef` route must peel to its inner closed domain"
    );
    assert!(
        template.elements[1].dynamic_classes.is_empty(),
        "a LOCAL type named `ModelRef` has no vue import edge and must publish \
         no closed domain"
    );
    // BOUNDARY, pinned rather than assumed: an UNANNOTATED
    // `defineModel<'a'|'b'>()` binding has no authored annotation, therefore no
    // producer-minted authored reference head, therefore no route candidate —
    // adding `ModelRef` to the vocabulary does NOT reach it. That is the
    // deferred inferred-head class, fail-closed and at scanner parity, not a
    // wrapper-vocabulary gap. This assertion fails if a future change ever
    // fabricates a domain for it without an exact composed route.
    assert!(
        template.elements[2].dynamic_classes.is_empty(),
        "an unannotated `defineModel()` binding has no authored head, so it must \
         publish no closed domain rather than a route-less guess"
    );

    let facts = template_class_facts_for(&host, canonical);
    let modelled = facts
        .rows()
        .iter()
        .find(|row| row.subject.label() == "modelled")
        .expect("modelled row");
    assert_eq!(
        modelled.wrapper.role,
        verter_type_expr::ReactiveWrapperRole::ModelRef,
        "`ModelRef` must classify as its OWN role, not collapse into `Ref`"
    );
    let provenance = modelled
        .wrapper
        .import_provenance
        .as_ref()
        .expect("exact vue route");
    assert_eq!(provenance.terminal_import_source.as_ref(), "vue");

    let fake = facts
        .rows()
        .iter()
        .find(|row| row.subject.label() == "fake")
        .expect("fake row");
    assert!(
        fake.wrapper.import_provenance.is_none(),
        "a local fake must claim no import provenance"
    );
    assert_ne!(
        fake.wrapper.role,
        verter_type_expr::ReactiveWrapperRole::ModelRef,
        "a local fake must not be granted the `ModelRef` role"
    );
}

#[test]
fn template_class_prop_wrappers_use_exact_routes_and_terminal_substitution() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/not-vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    let canonical = "/workspace/src/PropWrappers.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { Ref } from 'vue'
import type { Ref as OtherRef } from 'not-vue'
type Wrapped<T> = Ref<T>
type Transformed<T> = Ref<T | 'transformed-extra'>
type LocalRef<T> = { value: T }
const props = defineProps<{
  direct: Ref<'direct-a' | 'direct-b'>
  alias: Wrapped<'alias-a' | 'alias-b'>
  transformed: Transformed<'closed'>
  localFake: LocalRef<'local'>
  packageFake: OtherRef<'package'>
}>()
</script><template>
  <div :class="direct" />
  <div :class="props.alias" />
  <div :class="props.transformed" />
  <div :class="localFake" />
  <div :class="props.packageFake" />
</template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![
            exact_dependency("vue", "/workspace/node_modules/vue/index.d.ts"),
            exact_dependency("not-vue", "/workspace/node_modules/not-vue/index.d.ts"),
        ],
    );

    let template = host
        .get_analysis(canonical)
        .expect("lazy analysis")
        .template
        .expect("template");
    let classes = template
        .elements
        .iter()
        .map(|element| element.dynamic_classes.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        classes,
        [
            vec!["direct-a".to_string(), "direct-b".to_string()],
            vec!["alias-a".to_string(), "alias-b".to_string()],
            vec!["transformed-extra".to_string(), "closed".to_string()],
            vec![],
            vec![],
        ],
        "bare and root/member prop subjects must share exact wrapper routing"
    );

    let session = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: CompileProfile::default(),
        })
        .expect("normal compile");
    assert_eq!(session.actual_mode, CompileCacheMode::Session);
    let content = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: CompileProfile {
                requested_mode: CompileCacheMode::Content,
                ..CompileProfile::default()
            },
        })
        .expect("content compile");
    assert_eq!(
        content.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request with cross-file wrapper facts must downgrade rather than admit"
    );
    assert!(!content.cache_hit);
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "dependency-derived macro wrapper facts stay visible but return-only in Content"
    );

    let facts = template_class_facts_for(&host, canonical);
    for (label, authored_head, aliases, expected) in [
        (
            "direct",
            "Ref",
            Vec::<&str>::new(),
            vec!["direct-a", "direct-b"],
        ),
        (
            "alias",
            "Wrapped",
            vec!["Wrapped"],
            vec!["alias-a", "alias-b"],
        ),
        (
            "transformed",
            "Transformed",
            vec!["Transformed"],
            vec!["transformed-extra", "closed"],
        ),
    ] {
        let row = facts
            .rows()
            .iter()
            .find(|row| row.subject.label() == label)
            .expect("requested prop row");
        assert_eq!(row.wrapper.role, verter_type_expr::ReactiveWrapperRole::Ref);
        let verter_type_expr::ClosedLiteralDomain::Strings(values) = &row.wrapper.inner_domain
        else {
            panic!("expected closed wrapper inner for {label}");
        };
        assert_eq!(
            values.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
            expected
        );
        let provenance = row
            .wrapper
            .import_provenance
            .as_ref()
            .expect("exact prop route");
        assert_eq!(provenance.import_source.as_ref(), "vue");
        assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
        let verter_type_expr::facts::AuthoredReferenceHeadFact::Bare { local_name, args } =
            &provenance.authored_head
        else {
            panic!("expected bare authored macro head for {label}");
        };
        assert_eq!(local_name.as_ref(), authored_head);
        let [verter_type_expr::facts::AuthoredReferenceArgLocator::MacroPayload {
            payload: arg_payload,
            arg_index: 0,
        }] = args.as_ref()
        else {
            panic!("expected exact macro payload argument locator for {label}");
        };
        let verter_session_query::analysis::template_class_facts::TemplateClassSubject::Prop {
            payload: subject_payload,
            ..
        } = &row.subject
        else {
            panic!("expected prop subject for {label}");
        };
        assert_eq!(arg_payload, subject_payload);
        assert_eq!(
            provenance
                .local_alias_hops
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<&str>>(),
            aliases
        );
    }
    for label in ["localFake", "packageFake"] {
        let row = facts
            .rows()
            .iter()
            .find(|row| row.subject.label() == label)
            .expect("negative prop row");
        assert!(
            !matches!(
                row.domain,
                verter_type_expr::ClosedLiteralDomain::Strings(_)
            ),
            "{label} must not publish a closed class subset"
        );
    }

    let missing_canonical = "/workspace/src/MissingPropWrapper.vue";
    upsert_vue(
        &host,
        missing_canonical,
        r#"<script setup lang="ts">
import type { Ref as MissingRef } from 'missing-vue'
const props = defineProps<{ missing: MissingRef<'missing'> }>()
</script><template><div :class="props.missing" /></template>"#,
    );
    assert!(host
        .get_analysis(missing_canonical)
        .expect("missing lazy analysis")
        .template
        .expect("missing template")
        .elements[0]
        .dynamic_classes
        .is_empty());
    let missing_facts = template_class_facts_for(&host, missing_canonical);
    let missing = missing_facts
        .rows()
        .iter()
        .find(|row| row.subject.label() == "missing")
        .expect("missing prop row");
    assert!(missing.wrapper.import_provenance.is_none());
    assert!(!matches!(
        missing.domain,
        verter_type_expr::ClosedLiteralDomain::Strings(_)
    ));
}

#[test]
fn bare_template_subjects_fall_back_to_unique_define_props_fields() {
    let host = make_host();
    let cases = [
        (
            "/UnboundProps.vue",
            "defineProps<{ variant: 'primary' | 'secondary' }>()",
            "variant",
            vec!["primary", "secondary"],
        ),
        (
            "/BoundBareProps.vue",
            "const props = defineProps<{ variant: 'primary' | 'secondary' }>()",
            "variant",
            vec!["primary", "secondary"],
        ),
        (
            "/DefaultsBareProps.vue",
            "const props = withDefaults(defineProps<{ variant: 'primary' | 'secondary' }>(), { variant: 'primary' })",
            "variant",
            vec!["primary", "secondary"],
        ),
        (
            "/BoundMemberProps.vue",
            "const props = defineProps<{ variant: 'primary' | 'secondary' }>()",
            "props.variant",
            vec!["primary", "secondary"],
        ),
        (
            "/MissingBareProps.vue",
            "defineProps<{ size: 'sm' | 'lg' }>()",
            "variant",
            vec![],
        ),
    ];
    for (canonical, declarations, expression, expected) in cases {
        upsert_vue(
            &host,
            canonical,
            &format!(
                r#"<script setup lang="ts">{declarations}</script>
<template><div :class="{expression}" /></template>"#
            ),
        );
        let template = host
            .get_analysis(canonical)
            .expect("analysis")
            .template
            .expect("template");
        assert_eq!(
            template.elements[0].dynamic_classes, expected,
            "bare prop projection mismatch for {canonical}"
        );
    }
}

/// A template-class subject the class-fact builder cannot join is a DEMAND
/// outcome, not a cacheability verdict about the whole compile.
///
/// A `v-for` alias is neither a script binding nor a `defineProps` field, so
/// its row is `TemplateClassSubject::Unresolved` and the artifact completeness
/// is `ReturnOnly`. That must decline ONLY the class-fact-bearing rails (the
/// raw-template slot's semantic signature and the pure-content publish), never
/// taint the ENCLOSING compile tracer: an ordinary, diagnostic-free SFC must
/// keep its Session compile cache and its persisted raw-template analysis.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unjoinable_class_subjects_keep_the_enclosing_compile_cacheable() {
    let host = make_host();
    let compile = |canonical: &str| {
        host.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: CompileProfile::default(),
        })
        .expect("compile")
    };

    // (1) A `v-for` alias in a `:class` position — a completely valid SFC.
    let loop_alias = "/workspace/src/LoopAlias.vue";
    upsert_vue(
        &host,
        loop_alias,
        r#"<script setup lang="ts">
const items = ['a', 'b']
</script>
<template>
  <div v-for="item in items" :key="item" :class="item" />
</template>"#,
    );
    let first = compile(loop_alias);
    assert!(!first.cache_hit, "the first compile is cold");
    let second = compile(loop_alias);
    assert!(
        second.cache_hit,
        "an unjoinable `v-for` alias class subject must not make the whole \
         compile permanently non-cacheable"
    );

    // (2) An unresolvable IMPORTED class type — the row is `Unresolved`
    // through a missing dependency rather than an unjoinable subject.
    let missing_dep = "/workspace/src/MissingClassType.vue";
    upsert_vue(
        &host,
        missing_dep,
        r#"<script setup lang="ts">
import type { Absent } from './absent-module'
const variant: Absent = null as never
</script>
<template><div :class="variant" /></template>"#,
    );
    let cold = compile(missing_dep);
    assert!(!cold.cache_hit, "the first compile is cold");
    let warm = compile(missing_dep);
    assert!(
        warm.cache_hit,
        "an unresolvable imported class type must not make the whole compile \
         permanently non-cacheable"
    );

    // NEGATIVE half: the two NARROW class-fact rails are still in force. A
    // `ReturnOnly` fact set publishes no closed domain AND still declines the
    // raw-template semantic slot (`template_class_signature == None`), so a
    // later dependency arrival cannot be served a stale empty domain.
    assert!(
        host.get_analysis(loop_alias)
            .expect("loop analysis")
            .template
            .expect("template")
            .elements
            .iter()
            .all(|element| element.dynamic_classes.is_empty()),
        "an unjoinable subject must never publish a closed domain"
    );
    assert!(
        host.get_analysis(missing_dep)
            .expect("missing analysis")
            .template
            .expect("template")
            .elements[0]
            .dynamic_classes
            .is_empty(),
        "a missing-dependency subject must never publish a closed domain"
    );
    assert!(
        host.derived_raw_cache()
            .get(missing_dep)
            .is_none_or(|derived| derived.raw_template_analysis().is_none()),
        "a ReturnOnly class-fact set must still DECLINE the raw-template \
         semantic slot — deleting the transitive taint must not weaken the \
         narrow signature rail"
    );

    // ARMING CONTROL: the raw-template slot is not permanently closed. The
    // same host/profile admits it for an SFC whose class subjects all join and
    // resolve, so the `is_none()` assertion above is a real decline rather
    // than a slot nothing could ever populate.
    let resolved = "/workspace/src/ResolvedClass.vue";
    upsert_vue(
        &host,
        resolved,
        r#"<script setup lang="ts">
type Variant = 'primary' | 'secondary'
const variant: Variant = 'primary'
</script>
<template><div :class="variant" /></template>"#,
    );
    let _ = compile(resolved);
    assert_eq!(
        host.get_analysis(resolved)
            .expect("resolved analysis")
            .template
            .expect("template")
            .elements[0]
            .dynamic_classes,
        ["primary", "secondary"],
        "a fully joined and resolved class subject still publishes its domain"
    );
    assert!(
        host.derived_raw_cache()
            .get(resolved)
            .is_some_and(|derived| derived.raw_template_analysis().is_some()),
        "a Complete class-fact set must still ADMIT the raw-template slot"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "CorrelationMismatch")]
fn content_override_template_class_facts_are_return_only_but_visible() {
    let host = make_host();
    let canonical = "/workspace/src/OverrideClasses.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
type Variant = 'primary' | 'secondary'
const variant: Variant = 'primary'
</script><template><div /></template>"#,
    );
    let profile = CompileProfile {
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    };
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: canonical.to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry::unissued_for_test(
                "<div :class=\"variant\" />",
            )],
        })
        .expect("override");
    let template = host
        .raw_template_analysis_for_file(canonical)
        .expect("override template");
    assert_eq!(
        template.elements[0].dynamic_classes,
        ["primary", "secondary"]
    );
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile,
        })
        .expect("content override virtual file");
    assert!(!response.cache_hit);
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "overlay-derived template-class output must never enter the base content store"
    );
    assert!(
        host.derived_raw_cache()
            .get(canonical)
            .is_none_or(|derived| derived.raw_template_analysis().is_none()),
        "content override facts are served return-only and never base-published"
    );
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
fn compile_retention_refusal_suppresses_scheduler_and_template_companions() {
    use verter_session_query::facts::{
        fact_cache::{FactVersionRef, ReadSetSignature},
        fact_read_set::seal_canonical_signature,
        receipt::ResultReceipt,
    };
    use verter_session_query::retention::{ChargeClass, RetentionLimits, SemanticRetentionAccount};
    for admitted in [true, false] {
        let mut host = make_host();
        let facts = (0..1025)
            .map(|index| {
                let canonical_id = format!("/wide/{index}.vue");
                upsert_vue(&host, &canonical_id, "<template><div /></template>");
                FactVersionRef::FileWholeHash {
                    hash: current_whole_hash(&host, &canonical_id),
                    canonical_id,
                }
            })
            .collect();
        let facts = seal_canonical_signature(facts);
        let pages: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact {
                FactVersionRef::Receipt(page) => Some(page.clone()),
                _ => None,
            })
            .collect();
        let page_bytes: usize = pages.iter().map(|page| page.retained_charge_bytes()).sum();
        assert!(!pages.is_empty());
        let account = SemanticRetentionAccount::new(RetentionLimits {
            max_entry_bytes: if admitted { usize::MAX } else { page_bytes - 1 },
            ..RetentionLimits::defaults()
        });
        host.project_type_store = Arc::new(
            crate::project_type_store::ProjectTypeStore::with_retention_account(Arc::clone(
                &account,
            )),
        );
        let canonical = "/wide/Owner.vue";
        upsert_vue(
            &host,
            canonical,
            "<template><div class='complete' /></template>",
        );
        let profile = CompileProfile::default();
        let profile_hash = crate::hash::compile_profile_hash(&profile);
        let consumed = ResultReceipt::new(facts.to_vec());
        drop(facts);
        let facts = Arc::from(vec![FactVersionRef::Receipt(consumed)]);
        let compile = || {
            host.get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical.into()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("compile")
        };
        let output = crate::compile_fact_emission::with_extra_compile_observations(facts, compile);
        assert!(!output.cache_hit);
        let expected = if admitted {
            ChargeClass::Retained
        } else {
            ChargeClass::Pinned
        };
        assert!(pages
            .iter()
            .all(|page| page.retained_charge_class() == Some(expected)));
        assert_eq!(
            host.compile_cache()
                .get(canonical)
                .and_then(|state| state
                    .compile_slot_for_node(profile_hash)
                    .map(|slot| ReadSetSignature::new(Arc::clone(&slot.fact_dep_signature.facts))))
                .is_some(),
            admitted
        );
        assert_eq!(
            host.scheduler
                .try_get_artifact(canonical, profile_hash)
                .is_some(),
            admitted
        );
        assert_eq!(persisted_raw_template(&host, canonical).is_some(), admitted);
        if admitted {
            let warm = compile();
            assert!(warm.cache_hit);
            assert_eq!(warm.code, output.code);
        } else {
            let recomputed = compile();
            assert_eq!(recomputed.code, output.code);
        }
        drop((host, pages));
        assert_eq!(account.snapshot().retained_bytes, 0);
    }
}
