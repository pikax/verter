use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_semantic::analysis::{build_script_analysis_with_scope, AnalysisScope};

use super::*;

/// The origin table of a `<script setup>` source, with an optional
/// companion `<script>` source.
fn table_of(setup: &str, companion: Option<&str>) -> FrameworkOriginTable {
    let analyze = |source: &str| {
        let allocator = Allocator::default();
        build_script_analysis_with_scope(source, SourceType::ts(), &allocator, AnalysisScope::BUILD)
    };
    let setup = analyze(setup);
    let companion = companion.map(analyze);
    build_origin_table(
        ScriptOriginView::build(Some(&setup), companion.as_ref()),
        "vue",
    )
}

#[test]
fn a_renamed_import_of_an_api_is_that_api() {
    let table = table_of("import { ref as r } from 'vue'", None);
    assert_eq!(
        table.origin("r"),
        FrameworkOrigin::Api(FrameworkApi::Ref, OriginEvidence::NamedImport)
    );
    assert_eq!(table.callee_api("r"), Some(FrameworkApi::Ref));
}

#[test]
fn a_member_of_the_framework_namespace_is_its_api() {
    let table = table_of("import * as V from 'vue'", None);
    assert_eq!(table.origin("V"), FrameworkOrigin::Namespace);
    assert_eq!(table.callee_api("V.ref"), Some(FrameworkApi::Ref));
    assert_eq!(
        table.callee_origin(CalleeRef::Member {
            object: "V",
            property: "shallowReactive",
        }),
        FrameworkOrigin::Api(
            FrameworkApi::ShallowReactive,
            OriginEvidence::NamespaceMember
        )
    );
    assert_eq!(table.callee_api("V.watch"), None);
    assert_eq!(table.callee_api("V.ref.call"), None);
}

#[test]
fn a_destructured_namespace_member_is_its_api() {
    let table = table_of("import * as V from 'vue'\nconst { computed } = V", None);
    assert_eq!(
        table.origin("computed"),
        FrameworkOrigin::Api(FrameworkApi::Computed, OriginEvidence::NamespaceDestructure)
    );
}

#[test]
fn a_const_alias_chain_of_an_api_is_that_api() {
    let table = table_of(
        "import { ref } from 'vue'\nconst mk = ref\nconst mk2 = mk",
        None,
    );
    let alias = FrameworkOrigin::Api(FrameworkApi::Ref, OriginEvidence::ConstAlias);
    assert_eq!(table.origin("mk"), alias);
    assert_eq!(table.origin("mk2"), alias);
}

#[test]
fn a_local_function_spelled_like_an_api_is_shadowed() {
    let table = table_of("function ref() {}", None);
    assert_eq!(table.origin("ref"), FrameworkOrigin::Shadowed);
    assert_eq!(table.callee_api("ref"), None);
}

#[test]
fn a_mutable_alias_is_never_proven() {
    let table = table_of("import { ref } from 'vue'\nlet r = ref", None);
    assert_eq!(table.origin("r"), FrameworkOrigin::Unknown);
}

#[test]
fn an_api_spelling_imported_from_another_module_is_shadowed() {
    let table = table_of("import { ref } from './mine'", None);
    assert_eq!(table.origin("ref"), FrameworkOrigin::Shadowed);
}

#[test]
fn an_alias_used_before_its_declaration_is_unknown() {
    let table = table_of(
        "import { ref } from 'vue'\nconst mk2 = mk\nconst mk = ref",
        None,
    );
    assert_eq!(table.origin("mk2"), FrameworkOrigin::Unknown);
    assert_eq!(
        table.origin("mk"),
        FrameworkOrigin::Api(FrameworkApi::Ref, OriginEvidence::ConstAlias)
    );
}

#[test]
fn an_api_spelling_with_nothing_imported_is_unknown() {
    let table = table_of("const n = ref(0)", None);
    assert_eq!(table.origin("ref"), FrameworkOrigin::Unknown);
    assert_eq!(table.callee_api("ref"), None);
}

#[test]
fn a_vue_runtime_package_counts_as_the_framework_module() {
    let table = table_of("import { reactive } from '@vue/reactivity'", None);
    assert_eq!(
        table.origin("reactive"),
        FrameworkOrigin::Api(FrameworkApi::Reactive, OriginEvidence::NamedImport)
    );
}

#[test]
fn a_companion_script_import_is_seen_from_setup() {
    let table = table_of("const n = r(0)", Some("import { ref as r } from 'vue'"));
    assert_eq!(table.callee_api("r"), Some(FrameworkApi::Ref));
}
