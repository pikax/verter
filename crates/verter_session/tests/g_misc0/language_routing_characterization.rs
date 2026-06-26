//! Characterization pin: Vue + TS routing through the host stage executor
//! is byte-identical across the file-language classification substrate.
//!
//! The pinned IDE TSX output and shallow symbol inventory below are a
//! byte-exact capture of the host's dispatch for a `.vue` + `.ts`
//! fixture pair, so any drift in `.vue`-vs-script routing (parse
//! dispatch, virtual-file gating, shallow symbol inventory) fails this
//! test byte-for-byte.

use std::sync::Arc;

use verter_session::{CompileProfile, FileLanguage, HostConfig, UpsertRequest, VerterHost};

const VUE_FIXTURE: &str = "<script setup lang=\"ts\">\nimport { ref } from 'vue';\nconst count = ref(1);\n</script>\n\n<template>\n  <div :data-count=\"count\">{{ count }}</div>\n</template>\n";

const TS_FIXTURE: &str =
    "export interface Box { v: number }\nexport const make = (): Box => ({ v: 1 });\n";

/// Byte-exact dispatch capture for `VUE_FIXTURE` (see module docs).
const EXPECTED_IDE_CODE: &str = r#"import { ref } from 'vue';
import type { Prettify as ___VERTER___Prettify, ExtractComponentProps as ___VERTER___ExtractComponentProps, ExtractLeafElement as ___VERTER___ExtractLeafElement } from "@verter/types";
import { shallowUnwrapRef as ___VERTER___shallowUnwrapRef, enhanceElementWithProps as ___VERTER___enhanceElementWithProps, extractRenderComponent as ___VERTER___extractRenderComponent, instantiateComponent as ___VERTER___instantiateComponent, extractArgumentsFromRenderSlot as ___VERTER___extractArgumentsFromRenderSlot, runCustomDirective as ___VERTER___runCustomDirective, retrieveSetupDirectives as ___VERTER___retrieveSetupDirectives, strictRenderSlot as ___VERTER___strictRenderSlot, checkRequiredSlots as ___VERTER___checkRequiredSlots } from "@verter/types";
;export function ___VERTER___TemplateBindingFN() {


const count = ref(1);

// @ts-ignore
let ___VERTER___instance!: Omit<InstanceType<import('./App.vue.verter.ts')['default']>, '$attrs'> & { $attrs: ___VERTER___Attrs };
void ___VERTER___instance;
const ___VERTER___directiveAccessor = ___VERTER___retrieveSetupDirectives(___VERTER___instance);
void ___VERTER___directiveAccessor;

const ___VERTER___unwrapped = ___VERTER___shallowUnwrapRef({
    count: count as unknown as typeof count
  });
{ /* verter-destructured-start */let { 
    count } = ___VERTER___unwrapped; /* verter-destructured-end */
<>
  <div data-count={count}>{ count }</div>
</>
} // close block scope

function ___VERTER___Comp98() {
  return {} as HTMLElementTagNameMap["div"];
}
function ___VERTER___getRootComponent() { return ___VERTER___Comp98(); }
function ___VERTER___getRootComponentPassedProps() { return {"data-count": count}; }
type ___VERTER___RootElement = ReturnType<typeof ___VERTER___getRootComponent>;
type ___VERTER___RootElementProps = ___VERTER___Prettify<Omit<
  ___VERTER___ExtractComponentProps<___VERTER___RootElement>,
  keyof ReturnType<typeof ___VERTER___getRootComponentPassedProps>
>>;

type ___VERTER___Attrs = ___VERTER___attributes & ___VERTER___RootElementProps;

void ___VERTER___getRootComponent; void ___VERTER___getRootComponentPassedProps;
void ___VERTER___Comp98;
void (___VERTER___instance).valueOf;

return {};
} // close templateBindingFN

export { default } from './App.vue.verter.ts';

type ___VERTER___attributes = {};
"#;

fn upsert(host: &VerterHost, canonical: &str, source: &str, kind: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: kind,
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

#[test]
fn vue_and_ts_routing_snapshot() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host, "/src/App.vue", VUE_FIXTURE, FileLanguage::vue());
    upsert(&host, "/src/util.ts", TS_FIXTURE, FileLanguage::script_ts());

    let profile = CompileProfile {
        target: verter_compiler::compile::CompileTarget::IDE,
        ..CompileProfile::default()
    };
    host.ensure_compiled("/src/App.vue", &profile)
        .expect("vue compiles");

    let ide = host
        .get_ide("/src/App.vue", &profile)
        .expect("vue file has IDE output");
    assert!(!ide.is_jsx, "ts SFC must produce .tsx output");
    assert_eq!(
        ide.code.as_ref(),
        EXPECTED_IDE_CODE,
        "IDE TSX output for the .vue fixture must be byte-identical to the \
         pinned dispatch capture"
    );

    // The IDE virtual-file pipeline is gated to SFC carriers — a plain
    // script never produces IDE output.
    assert!(
        host.get_ide("/src/util.ts", &profile).is_none(),
        "plain script must not produce IDE output"
    );

    let symbols = host.list_file_symbols("/src/util.ts");
    let mut names: Vec<String> = symbols.iter().map(|s| s.name.to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["Box".to_string(), "make".to_string()],
        "script shallow inventory must be unchanged"
    );
}
