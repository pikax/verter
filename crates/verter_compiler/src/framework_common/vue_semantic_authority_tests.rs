use std::sync::Arc;

use oxc_allocator::Allocator;
use verter_audit::observe::Uncaptured;

use super::*;
use crate::compile_request::demand::{close_demand, DemandSeed, TargetKind};
use crate::compile_request::{ProductKind, RuntimeStyleProcessing};
use crate::framework::vue::semantic::{VueBindingFact, VueComponentClass};
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::types::BindingType;

/// A registered Vue parse artifact of `source`, with Vue's standard
/// interpolation delimiters.
fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
    };
    use verter_language::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };
    let source_authority = RegisteredSourceAuthority::new().unwrap();
    let grammar_authority = CarrierGrammarAuthority::new().unwrap();
    let config =
        crate::framework_common::vue_grammar_config("{{", "}}", std::iter::empty::<&str>())
            .unwrap();
    grammar_authority
        .register_carrier_grammar(
            verter_language::FileLanguage::vue(),
            FrameworkAdapterSemanticVersion::new(1).unwrap(),
            CarrierParserGrammarVersion::new(1).unwrap(),
            config.clone(),
        )
        .unwrap();
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new("file:///fixture.vue"),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            verter_language::FileLanguage::vue(),
            Arc::from(source),
        )
        .unwrap();
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .unwrap();
    Arc::new(
        super::super::registered_carrier_projection::project_registered_accepted(&accepted)
            .expect("fixture source parses")
            .into_framework_parse_artifact(),
    )
}

/// The template facts as the whole legacy compile extracted them, run
/// with only the template-data target.
fn legacy_template_facts(
    source: &str,
    artifact: &FrameworkParseArtifact,
) -> Option<TemplateFactsProduct> {
    use crate::compile::compile_from_parsed_legacy;
    use crate::compile::types::{CodegenOptions, CompileTarget, ResolvedVueCompileOptions};

    let parsed = VueCarrierFrontend.parsed_sfc(artifact)?;
    let core_opts = CodegenOptions {
        target: CompileTarget::TEMPLATE_DATA,
        ..Default::default()
    };
    let verter_opts = ResolvedVueCompileOptions {
        source_map: false,
        ..Default::default()
    };
    let alloc = Allocator::new();
    let result = compile_from_parsed_legacy(
        source,
        parsed,
        &core_opts,
        &verter_opts,
        &VueMacroSemanticInput::Unavailable,
        &alloc,
    )
    .ok()?;
    let diagnostics = result
        .template_data_diagnostics
        .into_iter()
        .map(|d| RuntimeDiagnostic {
            severity: d.severity.into(),
            code: d.code,
            message: d.message,
            span: d
                .span
                .unwrap_or_else(|| verter_span::Span::new(0, source.len() as u32)),
        })
        .collect();
    match result.template_data {
        Some(data) => Some(TemplateFactsProduct { data, diagnostics }),
        None if parsed.template_ast().is_none() => Some(TemplateFactsProduct {
            data: RawTemplateData::default(),
            diagnostics,
        }),
        None => None,
    }
}

fn assert_facts_match_the_legacy_compile(name: &str, source: &str) {
    let artifact = artifact_for(source);
    let legacy = legacy_template_facts(source, &artifact);
    let served = VueSemanticAuthority.template_facts(source, &artifact);
    match (legacy, served) {
        (Some(legacy), Some(served)) => {
            assert_eq!(
                format!("{:#?}", served.data),
                format!("{:#?}", legacy.data),
                "{name}: template facts differ"
            );
            assert_eq!(
                served.diagnostics, legacy.diagnostics,
                "{name}: diagnostics"
            );
        }
        (None, None) => {}
        (legacy, served) => panic!(
            "{name}: legacy produced facts: {}, served: {}",
            legacy.is_some(),
            served.is_some()
        ),
    }
}

#[test]
fn template_facts_match_the_legacy_compile_over_the_conformance_corpus() {
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("verter_vue_conformance")
        .join("corpus")
        .join("cases");
    let mut cases = Vec::new();
    for group in std::fs::read_dir(&corpus).expect("corpus directory") {
        let group = group.expect("corpus entry").path();
        // Server-only cases are compiled for the server route only.
        if !group.is_dir() || group.file_name().is_some_and(|name| name == "ssr") {
            continue;
        }
        for case in std::fs::read_dir(&group).expect("corpus group") {
            let case = case.expect("corpus case").path();
            if case.extension().is_some_and(|ext| ext == "vue") {
                cases.push(case);
            }
        }
    }
    cases.sort();
    assert_eq!(
        cases.len(),
        33,
        "the conformance corpus has 33 client cases"
    );
    for case in &cases {
        let source = std::fs::read_to_string(case).expect("corpus case reads");
        assert_facts_match_the_legacy_compile(&case.display().to_string(), &source);
    }
}

#[test]
fn template_facts_match_the_legacy_compile_at_its_edges() {
    for (name, source) in [
        (
            "malformed condition",
            r#"<script setup>const ok = true</script><template><p v-if="ok +">x</p></template>"#,
        ),
        ("no template", "<script setup>const a = 1</script>"),
        ("empty file", ""),
        (
            "options api",
            "<script>export default { data() { return { n: 1 } }, props: ['p'] }</script><template><p>{{ n }} {{ p }}</p></template>",
        ),
        (
            "companion and setup",
            r#"<script>import { helper } from "./h"; const local = 1</script><script setup>import { ref } from "vue"; const count = ref(0); const items = getItems(); const { a } = useX(); defineProps(["title"])</script><template><p :title="title" @click="count++">{{ helper(local) }} {{ items.length }} {{ a }}</p><Child v-for="i in items" :key="i" /></template>"#,
        ),
        (
            "pug template",
            r#"<template lang="pug">p hello</template>"#,
        ),
    ] {
        assert_facts_match_the_legacy_compile(name, source);
    }
}

#[test]
fn the_authority_and_the_builder_agree() {
    let source = r#"<template><Child :n="count" /><KeepAlive /><x-y /></template>"#;
    let ast = super::super::vue_bridge::parse_sfc(source, None, None)
        .template_ast
        .expect("template block");
    let topology = build_template_topology(
        source,
        &ast,
        TopologyRecoveryFacts::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .expect("topology builds");
    let allocator = Allocator::default();
    let demand = close_demand(&[DemandSeed {
        product: ProductKind::RuntimeClient,
        target: TargetKind::ClientTree,
        runtime_source_map: false,
        style_processing: RuntimeStyleProcessing::Complete,
        has_template: true,
        has_script: true,
        style_count: 0,
    }]);
    let macros = VueMacroSemanticInput::Unavailable;
    let inputs = || VueSemanticInputs {
        source,
        topology: Some(&topology),
        reported_bindings: [
            ("Child", BindingType::SetupConst),
            ("count", BindingType::SetupMaybeRef),
        ]
        .into_iter()
        .collect(),
        setup_analysis: None,
        companion_analysis: None,
        macros: &macros,
        runtime_module: "vue",
        self_name: "App",
        template_is_ts: false,
        allocator: &allocator,
    };
    let mut ledger = CompilerWorkLedger::<Uncaptured>::new();

    let served = VueSemanticAuthority
        .snapshot(
            &VueSemanticAdmission { _private: () },
            inputs(),
            &demand,
            &mut ledger,
        )
        .expect("served snapshot");
    let built = build_semantic_snapshot(inputs(), &demand, &mut ledger).expect("snapshot");

    assert_eq!(served.families(), built.families());
    fn table<'a>(snapshot: &'a VueSemanticSnapshot<'_>) -> Vec<(&'a str, VueBindingFact)> {
        let mut facts: Vec<_> = snapshot.bindings().iter().collect();
        facts.sort_by_key(|(name, _)| *name);
        facts
    }
    assert_eq!(table(&served), table(&built));
    let classes = |snapshot: &VueSemanticSnapshot<'_>| {
        topology
            .nodes()
            .map(|node| snapshot.component_class(node))
            .collect::<Vec<_>>()
    };
    assert_eq!(classes(&served), classes(&built));
    assert!(classes(&served).contains(&VueComponentClass::Binding));
    assert!(classes(&served).contains(&VueComponentClass::BuiltIn));
}
