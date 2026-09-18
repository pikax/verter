//! Internal general TypeExpr transit closure.
//!
//! Vue `defineOptions` / `defineExpose` named-member values classify through
//! the node-domain raised-shape fold into the sealed
//! [`NamedTypeMemberOutput`] vocabulary. This pins every reachable arm
//! (Primitive, Literal, generic-Ref, EmptyObject, Opaque) on the live Vue
//! options/expose path. A fold-to-vocabulary mis-map fails here; a renamed
//! TypeExpr helper would not.

use std::sync::Arc;

use verter_semantic::analysis::types::AnalyzedMacroKind;
use verter_session::typeinfo::framework_surface::{NamedTypeMember, NamedTypeMemberOutput};
use verter_session::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use verter_session::{LanguageRegistry, UpsertRequest, VerterHost};
use verter_type_expr::{LiteralValue, PrimitiveName};

use super::harness;

const VOCAB_VUE: &str = r#"<script setup lang="ts">
class Box<T> { value!: T }
defineOptions<{
  name: 'Widget';
  inheritAttrs?: boolean;
  empty: {};
  boxed: Box<string>;
  mixed: string | number;
}>();
defineExpose<{
  focus(): void;
  readonly count: number;
}>();
</script>
<template><div></div></template>
"#;

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn macro_dtos(
    host: &VerterHost,
    canonical: &str,
    kind: AnalyzedMacroKind,
) -> Arc<verter_session::typeinfo::framework_surface::MacroSurfaceDtos> {
    let snapshot = host
        .get_analysis(canonical)
        .unwrap_or_else(|| panic!("{canonical} must analyze after upsert"));
    let macro_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == kind)
        .unwrap_or_else(|| panic!("{canonical} must declare {kind:?}"));
    host.vue_macro_dtos(&VueMacroSurfaceRequest {
        owner_canonical: Arc::from(canonical),
        macro_index,
        macro_kind: kind,
        root_identity: [0u8; 16],
        level: TypeInfoQueryLevel::FullMetadata,
    })
}

fn member<'a>(members: &'a [NamedTypeMember], name: &str) -> &'a NamedTypeMember {
    members.iter().find(|m| m.name == name).unwrap_or_else(|| {
        panic!(
            "missing member `{name}` in {:?}",
            members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>()
        )
    })
}

/// Regression: Vue options/expose classification published names + required
/// flags only. A Primitive/Literal/Ref/EmptyObject/Opaque mis-map on this
/// path failed no behavioral test.
#[test]
fn vue_options_expose_named_members_classify_into_sealed_vocabulary() {
    let host = harness::build_hermetic_host_with_lib(
        &[("/Vocab.vue", VOCAB_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    upsert_vue(&host, "/Vocab.vue", VOCAB_VUE);

    let options = macro_dtos(&host, "/Vocab.vue", AnalyzedMacroKind::DefineOptions);
    let options_members = &options
        .options
        .as_ref()
        .expect("defineOptions publishes an options surface")
        .members;
    let option_names: Vec<&str> = options_members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        option_names,
        ["name", "inheritAttrs", "empty", "boxed", "mixed"],
        "options members keep source order"
    );

    assert_eq!(
        member(options_members, "name").value,
        Some(NamedTypeMemberOutput::Literal(LiteralValue::String(
            "Widget".into()
        ))),
        "`name: 'Widget'` classifies as Literal"
    );
    let inherit = member(options_members, "inheritAttrs");
    assert!(inherit.is_optional, "`inheritAttrs?` stays optional");
    assert_eq!(
        inherit.value,
        Some(NamedTypeMemberOutput::Primitive(PrimitiveName::Boolean)),
        "`inheritAttrs?: boolean` classifies as Primitive"
    );
    assert_eq!(
        member(options_members, "empty").value,
        Some(NamedTypeMemberOutput::EmptyObject),
        "`empty: {{}}` classifies as EmptyObject"
    );
    assert_eq!(
        member(options_members, "boxed").value,
        Some(NamedTypeMemberOutput::Ref { name: "Box".into() }),
        "`boxed: Box<string>` classifies as name-only generic Ref"
    );
    assert_eq!(
        member(options_members, "mixed").value,
        Some(NamedTypeMemberOutput::Opaque),
        "`mixed: string | number` degrades to Opaque"
    );

    let expose = macro_dtos(&host, "/Vocab.vue", AnalyzedMacroKind::DefineExpose);
    let expose_members = &expose
        .expose
        .as_ref()
        .expect("defineExpose publishes an expose surface")
        .members;
    let expose_names: Vec<&str> = expose_members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        expose_names,
        ["focus", "count"],
        "expose members keep source order"
    );
    assert_eq!(
        member(expose_members, "focus").value,
        Some(NamedTypeMemberOutput::Opaque),
        "`focus(): void` degrades to Opaque"
    );
    assert_eq!(
        member(expose_members, "count").value,
        Some(NamedTypeMemberOutput::Primitive(PrimitiveName::Number)),
        "`readonly count: number` classifies as Primitive"
    );
}
