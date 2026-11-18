use oxc_allocator::Allocator;
use oxc_span::SourceType;

use super::*;
use crate::analysis::build_script_analysis_with_scope;
use crate::analysis::scope::AnalysisScope;

/// The analysis the compiler builds for a script block.
fn analyze(source: &str) -> ScriptAnalysisSnapshot {
    let allocator = Allocator::default();
    build_script_analysis_with_scope(source, SourceType::ts(), &allocator, AnalysisScope::BUILD)
}

/// Build a view from a `<script setup>` source and an optional companion
/// `<script>` source.
fn view_of(setup: &str, companion: Option<&str>) -> ScriptOriginView {
    let setup = analyze(setup);
    let companion = companion.map(analyze);
    ScriptOriginView::build(Some(&setup), companion.as_ref())
}

fn export(
    source: &'static str,
    imported: &'static str,
    step: OriginStep,
) -> ResolvedOrigin<'static> {
    ResolvedOrigin::Export {
        source,
        imported,
        step,
    }
}

#[test]
fn a_renamed_named_import_is_its_export() {
    let view = view_of("import { ref as r } from 'vue'", None);
    assert_eq!(
        view.origin_of("r"),
        export("vue", "ref", OriginStep::NamedImport)
    );
    assert_eq!(view.origin_of("ref"), ResolvedOrigin::Undeclared);
}

#[test]
fn a_default_import_is_the_default_export() {
    let view = view_of("import Child from './Child.vue'", None);
    assert_eq!(
        view.origin_of("Child"),
        export("./Child.vue", "default", OriginStep::NamedImport)
    );
}

#[test]
fn a_namespace_import_resolves_its_members() {
    let view = view_of("import * as V from 'vue'", None);
    assert_eq!(
        view.origin_of("V"),
        ResolvedOrigin::Namespace { source: "vue" }
    );
    assert_eq!(
        view.callee_origin(CalleeRef::Member {
            object: "V",
            property: "ref",
        }),
        export("vue", "ref", OriginStep::NamespaceMember)
    );
    assert_eq!(
        view.callee_origin(CalleeRef::Name("V")),
        ResolvedOrigin::Namespace { source: "vue" }
    );
}

#[test]
fn a_member_of_anything_but_a_namespace_is_not_an_export() {
    let view = view_of("import { ref } from 'vue'\nconst store = makeStore()", None);
    assert_eq!(
        view.callee_origin(CalleeRef::Member {
            object: "store",
            property: "ref",
        }),
        ResolvedOrigin::Local
    );
    assert_eq!(
        view.callee_origin(CalleeRef::Member {
            object: "ref",
            property: "value",
        }),
        ResolvedOrigin::Local
    );
    assert_eq!(
        view.callee_origin(CalleeRef::Member {
            object: "missing",
            property: "ref",
        }),
        ResolvedOrigin::Undeclared
    );
}

#[test]
fn destructuring_a_namespace_proves_each_member() {
    let view = view_of(
        "import * as V from 'vue'\nconst { computed, ref: r, ...rest } = V",
        None,
    );
    assert_eq!(
        view.origin_of("computed"),
        export("vue", "computed", OriginStep::NamespaceDestructure)
    );
    assert_eq!(
        view.origin_of("r"),
        export("vue", "ref", OriginStep::NamespaceDestructure)
    );
    assert_eq!(view.origin_of("rest"), ResolvedOrigin::Local);
}

#[test]
fn destructuring_with_a_default_or_from_a_mutable_binding_proves_nothing() {
    let view = view_of(
        "import * as V from 'vue'\nconst { ref = fallback } = V\nlet W = V\nconst { computed } = W",
        None,
    );
    assert_eq!(view.origin_of("ref"), ResolvedOrigin::Local);
    assert_eq!(view.origin_of("W"), ResolvedOrigin::Local);
    assert_eq!(view.origin_of("computed"), ResolvedOrigin::Local);
}

#[test]
fn const_alias_chains_stay_proven() {
    let view = view_of(
        "import { ref } from 'vue'\nconst mk = ref\nconst mk2 = mk\nconst mk3 = (mk2 as typeof ref)",
        None,
    );
    for name in ["mk", "mk2", "mk3"] {
        assert_eq!(
            view.origin_of(name),
            export("vue", "ref", OriginStep::ConstAlias),
            "{name}"
        );
    }
}

#[test]
fn an_alias_of_a_namespace_is_the_namespace() {
    let view = view_of("import * as V from 'vue'\nconst W = V", None);
    assert_eq!(
        view.origin_of("W"),
        ResolvedOrigin::Namespace { source: "vue" }
    );
    assert_eq!(
        view.callee_origin(CalleeRef::Member {
            object: "W",
            property: "computed",
        }),
        export("vue", "computed", OriginStep::NamespaceMember)
    );
}

#[test]
fn local_declarations_are_local() {
    let view = view_of(
        "function ref() {}\nasync function load() {}\nclass Store {}",
        None,
    );
    assert_eq!(view.origin_of("ref"), ResolvedOrigin::Local);
    assert_eq!(view.origin_of("load"), ResolvedOrigin::Local);
    assert_eq!(view.origin_of("Store"), ResolvedOrigin::Local);
    assert_eq!(view.initializer_of("ref"), None);
}

#[test]
fn declarations_the_analysis_does_not_record_prove_nothing() {
    let view = view_of(
        "",
        Some("import { ref } from 'vue'\nexport const shared = ref\nenum Mode { A }"),
    );
    assert_eq!(view.origin_of("shared"), ResolvedOrigin::Undeclared);
    assert_eq!(view.origin_of("Mode"), ResolvedOrigin::Undeclared);
    assert_eq!(view.len(), 1);
}

#[test]
fn a_mutable_alias_is_never_proven() {
    let view = view_of("import { ref } from 'vue'\nlet r = ref\nvar s = ref", None);
    assert_eq!(view.origin_of("r"), ResolvedOrigin::Local);
    assert_eq!(view.origin_of("s"), ResolvedOrigin::Local);
}

#[test]
fn an_import_from_another_module_names_that_module() {
    let view = view_of("import { ref } from './mine'", None);
    assert_eq!(
        view.origin_of("ref"),
        export("./mine", "ref", OriginStep::NamedImport)
    );
}

#[test]
fn an_alias_used_before_its_declaration_is_local() {
    let view = view_of(
        "import { ref } from 'vue'\nconst early = later\nconst later = ref",
        None,
    );
    assert_eq!(view.origin_of("early"), ResolvedOrigin::Local);
    assert_eq!(
        view.origin_of("later"),
        export("vue", "ref", OriginStep::ConstAlias)
    );
}

#[test]
fn imports_are_hoisted_above_aliases() {
    let view = view_of("const mk = ref\nimport { ref } from 'vue'", None);
    assert_eq!(
        view.origin_of("mk"),
        export("vue", "ref", OriginStep::ConstAlias)
    );
}

#[test]
fn type_only_imports_declare_no_value() {
    let view = view_of(
        "import type { Ref } from 'vue'\nimport { type ComputedRef, ref } from 'vue'",
        None,
    );
    assert_eq!(view.origin_of("Ref"), ResolvedOrigin::Undeclared);
    assert_eq!(view.origin_of("ComputedRef"), ResolvedOrigin::Undeclared);
    assert_eq!(
        view.origin_of("ref"),
        export("vue", "ref", OriginStep::NamedImport)
    );
    assert_eq!(view.len(), 1);
}

#[test]
fn a_name_declared_twice_is_never_proven() {
    let view = view_of(
        "import { ref } from 'vue'\nconst mk = ref",
        Some("function ref() {}"),
    );
    assert_eq!(view.origin_of("ref"), ResolvedOrigin::Local);
    assert_eq!(view.origin_of("mk"), ResolvedOrigin::Local);
}

#[test]
fn an_import_repeated_in_both_scripts_stays_proven() {
    let view = view_of(
        "import { ref } from 'vue'\nimport * as V from 'vue'\nconst n = ref(0)",
        Some("import { ref } from 'vue'\nimport * as V from 'vue'"),
    );
    assert_eq!(
        view.origin_of("ref"),
        export("vue", "ref", OriginStep::NamedImport)
    );
    assert_eq!(
        view.origin_of("V"),
        ResolvedOrigin::Namespace { source: "vue" }
    );
    let callee = view.initializer_of("n").and_then(|fact| fact.callee);
    assert_eq!(
        callee.map(|callee| view.callee_origin(callee)),
        Some(export("vue", "ref", OriginStep::NamedImport))
    );

    let differing = view_of(
        "import { ref } from 'vue'",
        Some("import { ref } from './mine'"),
    );
    assert_eq!(differing.origin_of("ref"), ResolvedOrigin::Local);
}

#[test]
fn the_companion_script_is_read_before_setup() {
    let view = view_of(
        "const r = shared",
        Some("import { ref } from 'vue'\nconst shared = ref"),
    );
    assert_eq!(
        view.origin_of("shared"),
        export("vue", "ref", OriginStep::ConstAlias)
    );
    assert_eq!(
        view.origin_of("r"),
        export("vue", "ref", OriginStep::ConstAlias)
    );
}

#[test]
fn initializers_record_the_declaration_kind_and_callee() {
    let view = view_of(
        "import { ref as r } from 'vue'\nimport * as V from 'vue'\n\
         const n = r(0)\nlet m = V.ref(0)\nvar plain = 1\nconst { a } = r(0)",
        None,
    );
    assert_eq!(
        view.initializer_of("n"),
        Some(InitializerFact {
            kind: DeclarationKind::Const,
            callee: Some(CalleeRef::Name("r")),
        })
    );
    assert_eq!(
        view.initializer_of("m"),
        Some(InitializerFact {
            kind: DeclarationKind::Let,
            callee: Some(CalleeRef::Member {
                object: "V",
                property: "ref",
            }),
        })
    );
    assert_eq!(
        view.initializer_of("plain"),
        Some(InitializerFact {
            kind: DeclarationKind::Var,
            callee: None,
        })
    );
    // A destructured binding is a property of the call's result, not the
    // call's value.
    assert_eq!(
        view.initializer_of("a"),
        Some(InitializerFact {
            kind: DeclarationKind::Const,
            callee: None,
        })
    );
    assert_eq!(view.initializer_of("r"), None);
    assert_eq!(view.initializer_of("missing"), None);
    let Some(InitializerFact {
        callee: Some(callee),
        ..
    }) = view.initializer_of("m")
    else {
        panic!("m has a call initializer");
    };
    assert_eq!(
        view.callee_origin(callee),
        export("vue", "ref", OriginStep::NamespaceMember)
    );
}

#[test]
fn a_call_wrapped_in_a_type_assertion_still_names_its_callee() {
    let view = view_of(
        "import { ref } from 'vue'\nconst n = ref(0) as unknown\nconst e = (ref(1))!\nconst s = ref(2) satisfies object",
        None,
    );
    assert_eq!(
        view.initializer_of("n").and_then(|fact| fact.callee),
        Some(CalleeRef::Name("ref"))
    );
    for name in ["e", "s"] {
        assert_eq!(
            view.initializer_of(name).and_then(|fact| fact.callee),
            Some(CalleeRef::Name("ref")),
            "{name}"
        );
    }
}

#[test]
fn nested_declarations_are_not_top_level_names() {
    let view = view_of(
        "import { ref } from 'vue'\nfunction setup() { const inner = ref(0); return inner }",
        None,
    );
    assert_eq!(view.origin_of("inner"), ResolvedOrigin::Undeclared);
    assert_eq!(view.initializer_of("inner"), None);
    assert_eq!(view.len(), 2);
}

#[test]
fn no_scripts_give_an_empty_view() {
    let view = ScriptOriginView::build(None, None);
    assert!(view.is_empty());
    assert_eq!(view.len(), 0);
    assert_eq!(view.origin_of("ref"), ResolvedOrigin::Undeclared);
    assert_eq!(view.initializer_of("ref"), None);
}
