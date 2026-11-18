//! The producers that witness their own grammar write what they claim: every
//! script module script lowering writes for the Vue conformance corpus, and
//! every decoration around a module, parses as a complete module in its
//! dialect.

use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;

use super::VueMainDecoration;
use crate::assembly::fragment::{final_module_parse_errors, FragmentDialect, GrammarProducer};
use crate::compile::legacy_test_support::{compile, CodegenOptions, VerterCompileOptions};
use crate::compile_request::RuntimeHmrStrategy;
use crate::framework_common::RuntimeCompileOutput;

fn corpus_cases() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("verter_vue_conformance")
        .join("corpus")
        .join("cases");
    let mut cases = Vec::new();
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("the corpus is vendored in the repository") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "vue") {
                cases.push(path);
            }
        }
    }
    cases.sort();
    cases
}

fn assert_complete_module(witness_dialect: FragmentDialect, code: &str, what: &str) {
    if let Some(reason) = final_module_parse_errors(code, witness_dialect) {
        panic!("{what} does not parse as a complete module: {reason}\n{code}");
    }
}

#[test]
fn script_lowering_writes_complete_modules_for_the_corpus() {
    let cases = corpus_cases();
    assert!(cases.len() >= 33, "found {} corpus cases", cases.len());
    let mut lowered = 0;
    for case in &cases {
        let source = std::fs::read_to_string(case).unwrap();
        for force_js in [false, true] {
            let allocator = Allocator::new();
            let options = CodegenOptions::new().with_filename("App.vue");
            let verter_options = VerterCompileOptions {
                force_js,
                ..Default::default()
            };
            let result = compile(
                &source,
                &options,
                &verter_options,
                &crate::compile::VueMacroSemanticInput::Unavailable,
                &allocator,
            );
            let Some(script) = result.script.as_ref() else {
                continue;
            };
            let typescript = !force_js
                && script
                    .attrs
                    .iter()
                    .any(|(name, value)| name == "lang" && (value == "ts" || value == "tsx"));
            let dialect = if typescript {
                FragmentDialect::TypeScript
            } else {
                FragmentDialect::JavaScript
            };
            let witness = <crate::script::ScriptCodeGenResult<'_> as GrammarProducer>::CONTRACT;
            assert_eq!(
                witness,
                crate::assembly::fragment::SyntacticContract::CompleteModule
            );
            assert_complete_module(
                dialect,
                &script.code,
                &format!("the script of {}", case.display()),
            );
            lowered += 1;
        }
    }
    assert!(lowered > 0, "the corpus lowered no script");
}

#[test]
fn the_decoration_writes_complete_modules() {
    let compiled = RuntimeCompileOutput::default();
    for hmr in [
        RuntimeHmrStrategy::None,
        RuntimeHmrStrategy::Vite,
        RuntimeHmrStrategy::Webpack,
    ] {
        for is_production in [false, true] {
            for ssr in [false, true] {
                for custom in [0, 2] {
                    let decoration = VueMainDecoration {
                        hmr,
                        is_production,
                        custom_specifiers: (0..custom)
                            .map(|index| format!("/src/App.vue?vue&type=docs&index={index}"))
                            .collect(),
                        ..VueMainDecoration::default()
                    };
                    let witness = decoration.witness(FragmentDialect::JavaScript);
                    let prelude = decoration.prelude(compiled.qualified_styles.len(), custom);
                    let trailer = decoration.trailer(custom, "/src/App.vue", "vue", ssr);
                    let module = format!(
                        "{}const _sfc_main = {{}}\n{}export default _sfc_main\n",
                        prelude.code, trailer.code
                    );
                    assert_complete_module(
                        witness.dialect(),
                        &module,
                        &format!("the decoration (hmr {hmr:?}, production {is_production}, ssr {ssr}, {custom} custom blocks)"),
                    );
                }
            }
        }
    }
}
