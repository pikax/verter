use super::*;
use crate::cursor::ScriptLanguage;
use crate::ide::vue_projection::binder_capture::capture_binder_plan;
use crate::ide::vue_projection::script_setup::ScriptBlockInput;

fn product(source: &str, public: &str) -> DeclarationProjectionProduct {
    use verter_parser::oxc_parse::faults::{self, Reservation};
    let _parses = faults::force_reservations_here(&[Reservation::Parse]);
    let parses_before = faults::reservations_here_of(Reservation::Parse);
    let capture = capture_binder_plan(
        None,
        Some(ScriptBlockInput {
            content: source,
            content_start: 0,
            lang: Some(ScriptLanguage::TypeScript),
        }),
        None,
    )
    .unwrap();
    let parses_after_capture = faults::reservations_here_of(Reservation::Parse);
    assert_eq!(parses_after_capture, parses_before + 1);
    let result: <crate::framework_common::vue_projection_backend::VueProjectionBackend as
        crate::framework_common::capability::ProjectionBackend>::Declarations = build_declaration_product(
        DeclarationContractInput {
            module: DeclarationModuleIdentity {
                canonical_id: "/ws/App.vue".into(),
                declaration_file: "/ws/App.d.vue.ts".into(),
            },
            public_declaration: public,
            module_scope: &[],
            source,
        },
        &capture,
    );
    assert_eq!(
        faults::reservations_here_of(Reservation::Parse),
        parses_after_capture + 1,
        "only the generated contract is parsed; captured authored dependencies are not reparsed"
    );
    result
}

#[test]
fn dependency_closure_retains_imports_and_lifts_local_types_and_value_queries() {
    let source = "import type { Remote } from './remote';\nconst marker: unique symbol = Symbol();\ntype Local = { remote: Remote; marker: typeof marker };\ndefineProps<Local>();";
    let result = product(
        source,
        "declare const component: new () => Local;\nexport default component;",
    );
    assert!(result.is_publishable(), "{:?}", result.diagnostics);
    assert_eq!(result.module.canonical_id, "/ws/App.vue");
    assert_eq!(
        result.closure.retained_imports,
        ["import type { Remote } from './remote';"]
    );
    assert!(result.declaration_source.contains("type Local ="));
    assert!(result
        .declaration_source
        .contains("declare const marker: unique symbol"));
    assert!(!result.declaration_source.contains("Symbol()"));
    assert!(!result.declaration_source.contains("defineProps"));
    let generated = result.declaration_source.find("type Local").unwrap() as u32;
    let authored = source.find("type Local").unwrap() as u32;
    assert!(result.map.mappings.iter().any(|mapping| {
        mapping.generated_start <= generated
            && generated < mapping.generated_end
            && mapping.source_start + generated - mapping.generated_start == authored
    }));
}

#[test]
fn private_module_specifiers_are_refused_but_literal_data_is_preserved() {
    for private in [
        "./App.vue.ts",
        "./App.vue.js",
        "./App.vue.tsx",
        "./App.vue.verter.ts",
        "./App.vue.verter",
        "./App.vue.__verter_test",
        "./App.vue.tsx.__verter_types",
        "./App.svelte.verter",
        "./App.svelte.jsx.__verter_types",
    ] {
        let result = product(
            "",
            &format!("export type Public = typeof import({private:?}).default;"),
        );
        assert!(!result.is_publishable(), "{private}");
        assert!(result
            .closure
            .refused
            .contains(&ClosureRefusal::PrivateCheckingPath {
                text: private.into()
            }));
    }
    let result = product(
        "",
        "export type Public = './App.vue.ts'; // import('./App.vue.ts')",
    );
    assert!(result.is_publishable(), "{:?}", result.diagnostics);
    for public in ["./App.vue", "./App.d.vue.ts", "./module.svelte.ts"] {
        let carrier = product(
            "",
            &format!("export type Public = typeof import({public:?}).default;"),
        );
        assert!(carrier.is_publishable(), "{:?}", carrier.diagnostics);
    }
}

#[test]
fn diagnostics_and_refusals_prevent_atomic_publication() {
    let mut result = product("", "export type Public = string;");
    assert!(result.is_publishable());
    result.diagnostics.push(DeclarationDiagnostic {
        start: 0,
        end: 0,
        message: "unnameable".into(),
    });
    assert!(!result.is_publishable());
    result.diagnostics.clear();
    result
        .closure
        .refused
        .push(ClosureRefusal::InaccessibleCapture {
            name: "hidden".into(),
        });
    assert!(!result.is_publishable());
}

#[test]
fn checking_only_bindings_and_unnameable_values_are_refused() {
    let checking = product(
        "",
        "export type Public = ReturnType<typeof __VerterSetupScope>;",
    );
    assert!(!checking.is_publishable());
    assert!(checking
        .closure
        .refused
        .contains(&ClosureRefusal::InaccessibleCapture {
            name: "__VerterSetupScope".into()
        }));
    let inaccessible = product(
        "const value = effect(); defineProps<{ value: typeof value }>();",
        "export type Public = typeof value;",
    );
    assert!(!inaccessible.is_publishable());
    assert!(inaccessible
        .closure
        .refused
        .contains(&ClosureRefusal::InaccessibleCapture {
            name: "value".into()
        }));
    let absolute = product(
        "import type { External } from '/ws/types'; defineProps<External>();",
        "export type Public = External;",
    );
    assert!(!absolute.is_publishable());
    assert!(absolute
        .closure
        .refused
        .contains(&ClosureRefusal::WorkspaceOnlyImport {
            specifier: "/ws/types".into()
        }));
}

#[test]
fn map_preserves_multibyte_and_multiline_authored_declarations() {
    let source = "import type { Remote } from './types';\ntype Local = {\n  café: Remote;\n};\ndefineProps<Local>();";
    let result = product(
        source,
        "declare const component: new () => Local; export default component;",
    );
    assert!(result.is_publishable(), "{:?}", result.diagnostics);
    for token in ["Remote", "Local", "café"] {
        let authored = source.find(token).unwrap() as u32;
        let generated = result.declaration_source.find(token).unwrap() as u32;
        assert!(result
            .map
            .mappings
            .iter()
            .any(|mapping| mapping.generated_start <= generated
                && generated < mapping.generated_end
                && mapping.source_start + generated - mapping.generated_start == authored));
    }
}

#[test]
fn captured_fragment_maps_keep_full_carrier_offsets() {
    let script = "type Local = { value: string }; defineProps<Local>();";
    let source = format!(
        "<script setup lang=\"ts\">\n{script}\n</script><template>{{{{ value }}}}</template>"
    );
    let start = source.find(script).unwrap() as u32;
    let capture = capture_binder_plan(
        None,
        Some(ScriptBlockInput {
            content: script,
            content_start: start,
            lang: Some(ScriptLanguage::TypeScript),
        }),
        None,
    )
    .unwrap();
    let product = build_declaration_product(
        DeclarationContractInput {
            module: DeclarationModuleIdentity {
                canonical_id: "/ws/App.vue".into(),
                declaration_file: "/ws/App.d.vue.ts".into(),
            },
            public_declaration: "export type Public = Local;",
            module_scope: &[],
            source: &source,
        },
        &capture,
    );
    assert!(product.is_publishable(), "{:?}", product.diagnostics);
    let generated = product.declaration_source.find("Local").unwrap() as u32;
    let authored = source.find("Local").unwrap() as u32;
    assert!(product
        .map
        .mappings
        .iter()
        .any(|mapping| mapping.generated_start <= generated
            && generated < mapping.generated_end
            && mapping.source_start + generated - mapping.generated_start == authored));
}
