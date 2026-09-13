//! Runtime proof that CSS `v-bind()` values reach the intended custom
//! properties on a MOUNTED element — initial state and across reactive
//! updates — for VERTER's own assembled client module, compared with the
//! pinned official compiler's module for the same source.
//!
//! A `contains()` check over the generated source cannot see the failure
//! class this guards: the registration key is applied by the runtime, which
//! prepends `--` itself, so a key that already carries `--` lands on
//! `----name`, a property no `var(--name)` reference can read, while the
//! module text still looks correct. Mounting observes what actually
//! happened. The comparison arm mounts the OFFICIAL module for the same
//! fixture side by side: both compilers name their properties differently,
//! but per update step the SET of applied values must agree.
//!
//! Behind `bf2-authoritative` (needs the provisioned oracle install).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use oxc_allocator::Allocator;
use serde_json::Value;
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::RuntimeCompileOptions;

use super::bf2_seed_matrix::run_bounded;
use super::{assemble_vue_main_module, CompileProfile, HmrStrategy};

/// The same surface the harness's own `vue-css-vars-runtime` spec mounts: a
/// bare prop, a quoted member expression, and an unaffected literal
/// declaration that must contribute no custom property.
const FIXTURE: &str = "<script setup>\n\
     const props = defineProps({ color: { type: String, default: 'red' }, \
     size: { type: String, default: '10px' } })\n\
     </script>\n\
     <template><div class=\"box\">boxed</div></template>\n\
     <style scoped>\n\
     .box { color: v-bind(color); font-size: v-bind('props.size'); background: teal; }\n\
     </style>\n";

/// The id the official compile is keyed by — hash-shaped, like the ids the
/// production plugin passes (a path-shaped id would not be spellable as a
/// custom-property prefix without CSS escaping).
const FIXTURE_ID: &str = "abc123";

/// Initial and two successive reactive states, the same trajectory the
/// harness spec tracks.
const PROP_STEPS: &str = r#"[
  { "color": "red", "size": "10px" },
  { "color": "blue", "size": "22px" },
  { "color": "green", "size": "22px" }
]"#;

/// Compile the fixture through the real Vue carrier (VDOM client backend)
/// and assemble the full runnable module — the same pipeline
/// [`super::compile_fixture`] drives, authored inline like
/// [`super::nested_v_for_runtime_proof`] so the assertion and its fixture
/// stay together.
fn assemble_inline_client_module(source: &str, canonical_id: &str) -> String {
    let provenance = crate::types::MetaProvenance::default();
    let (snapshot, artifact) = crate::parse::parse_vue_snapshot(
        canonical_id,
        source,
        verter_semantic::analysis::AnalysisScope::LSP,
        &provenance,
    );

    let allocator = Allocator::new();
    let compiled = VueCarrierCompiler
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some(canonical_id.to_string()),
                source_map: false,
                ssr: false,
                is_production: false,
                inline: Some(false),
                force_js: true,
                force_vapor: false,
                ..RuntimeCompileOptions::default()
            },
            &allocator,
        )
        .expect("the Vue carrier produces a runtime bundle for this fixture")
        .into_produced()
        .expect("the Vue carrier produces a runtime surface; it never refuses one");

    let profile = CompileProfile {
        filename: Some(canonical_id.to_string()),
        is_production: false,
        ssr: false,
        source_map: false,
        inline: Some(false),
        force_vapor: false,
        hmr_strategy: HmrStrategy::None,
        ..CompileProfile::default()
    };

    assemble_vue_main_module(canonical_id, &compiled, &snapshot.meta, &profile)
        .unwrap_or_else(|failure| panic!("the assembler failed closed: {failure:?}"))
        .code
}

fn harness_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/framework-conformance-harness")
}

/// Mount both modules through the pinned official client runtime in jsdom
/// via a Node subprocess, returning the executor's report for each.
fn mount_verter_and_official(
    fixture: &str,
    fixture_id: &str,
    verter_module: &str,
    prop_steps: &str,
) -> (Value, Value, String) {
    // Unique per CALL (same `--lib` binary, concurrent mounts): a PID-only
    // name collides; `verter_test_support::unique_temp_dir` mints a
    // per-process, per-call path.
    let temp = verter_test_support::unique_temp_dir("verter-css-vars-client-mount");
    std::fs::create_dir_all(&temp).expect("create the mount scratch dir");
    let fixture_path = temp.join("fixture.vue");
    let module_path = temp.join("module.vue.mjs");
    let steps_path = temp.join("prop-steps.json");
    std::fs::write(&fixture_path, fixture).expect("write the fixture");
    std::fs::write(&module_path, verter_module).expect("write the assembled module");
    std::fs::write(&steps_path, prop_steps).expect("write the prop steps");

    let script = r#"
import { readFileSync } from "node:fs";
import { executeVueClientMount, cleanupScratch } from "./src/execute-vue-runtime.mjs";
import { compileVueFixture } from "./src/invoke-vue-oracle.mjs";
const fixture = readFileSync(process.argv[1], "utf8");
const fixtureId = process.argv[2];
const verterModule = readFileSync(process.argv[3], "utf8");
const propSteps = JSON.parse(readFileSync(process.argv[4], "utf8"));
const official = compileVueFixture(fixture, fixtureId, { backend: "vdom", sourceMap: false, isProd: false });
const verter = await executeVueClientMount(verterModule, { propSteps });
const oracle = await executeVueClientMount(official.code, { propSteps });
cleanupScratch();
process.stdout.write("\n===CSS-VARS-MOUNT-REPORT===\n");
process.stdout.write(JSON.stringify({ verter, oracle, officialCode: official.code }));
"#;
    let mut command = Command::new("node");
    command
        .arg("--input-type=module")
        .arg("-e")
        .arg(script)
        .arg("--")
        .arg(&fixture_path)
        .arg(fixture_id)
        .arg(&module_path)
        .arg(&steps_path)
        .current_dir(harness_root());

    let finished = run_bounded(&mut command, Duration::from_secs(120));
    let _ = std::fs::remove_dir_all(&temp);

    assert!(
        !finished.timed_out,
        "the client mount did not finish within 120s — it was killed.\nstderr:\n{}",
        finished.stderr
    );
    assert_eq!(
        finished.code,
        Some(0),
        "the Node harness itself failed (not the mount under test).\nstdout:\n{}\nstderr:\n{}",
        finished.stdout,
        finished.stderr
    );
    // The dev runtime build prints its "development build" notice straight
    // to stdout, so the report is framed by a marker line, not guessed from
    // the first `{`.
    const REPORT_MARKER: &str = "===CSS-VARS-MOUNT-REPORT===";
    let report_json = finished
        .stdout
        .rsplit(REPORT_MARKER)
        .next()
        .unwrap_or_default()
        .trim();
    let report: Value = serde_json::from_str(report_json).unwrap_or_else(|error| {
        panic!(
            "the mount harness emitted no JSON report ({error}).\nstdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    });
    let verter = report.get("verter").cloned().unwrap_or(Value::Null);
    let oracle = report.get("oracle").cloned().unwrap_or(Value::Null);
    let official_code = report
        .get("officialCode")
        .and_then(Value::as_str)
        .unwrap_or("<absent>")
        .to_string();
    (verter, oracle, official_code)
}

/// `steps` of an executor report: one `(html, customProperties)` pair per
/// prop state, in order.
fn mount_steps(report: &Value, which: &str) -> Vec<(String, BTreeMap<String, String>)> {
    assert_eq!(
        report.get("ok").and_then(Value::as_bool),
        Some(true),
        "the {which} mount failed:\nerror:\n{}\nwarnings:\n{:?}",
        report
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("<absent>"),
        report.get("warnings").and_then(Value::as_array)
    );
    assert_eq!(
        report
            .get("warnings")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0),
        "the {which} mount warned:\n{:?}",
        report.get("warnings")
    );
    report
        .get("steps")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the {which} report carries steps"))
        .iter()
        .map(|step| {
            let html = step
                .get("html")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut properties = BTreeMap::new();
            for (name, value) in step
                .get("customProperties")
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("the {which} step carries customProperties"))
            {
                properties.insert(name.clone(), value.as_str().unwrap_or_default().to_string());
            }
            (html, properties)
        })
        .collect()
}

/// Sorted applied values of one step — the semantic comparison basis: the
/// two compilers NAME their properties differently, so what must agree is
/// the set of values that actually landed on the element.
fn applied_values(properties: &BTreeMap<String, String>) -> Vec<String> {
    let mut values: Vec<String> = properties.values().cloned().collect();
    values.sort();
    values
}

/// Mounts VERTER's own assembled client module and the official module for
/// the same fixture through the pinned runtime, and requires both to apply
/// the same custom-property values initially and across two reactive
/// updates — with exactly one `--` prefix per property on the Verter side.
#[test]
fn verter_css_vars_client_module_mounts_and_tracks_reactive_updates_like_the_official_one() {
    let canonical_id = "fixtures/vue/css-vars-client-mount.vue";
    let module_code = assemble_inline_client_module(FIXTURE, canonical_id);
    assert!(
        module_code.contains("_useCssVars"),
        "sanity: the fixture must exercise the CSS-vars registration path, got:\n{module_code}"
    );

    let (verter, oracle, official_code) =
        mount_verter_and_official(FIXTURE, FIXTURE_ID, &module_code, PROP_STEPS);

    let verter_steps = mount_steps(&verter, "Verter");
    let oracle_steps = mount_steps(&oracle, "official");
    assert_eq!(verter_steps.len(), 3, "three prop states were requested");
    assert_eq!(oracle_steps.len(), 3, "three prop states were requested");
    assert!(
        verter_steps[0].0.contains("boxed"),
        "the Verter mount rendered no content: {:?}",
        verter_steps[0].0
    );

    let expected = [
        vec!["10px".to_string(), "red".to_string()],
        vec!["22px".to_string(), "blue".to_string()],
        vec!["22px".to_string(), "green".to_string()],
    ];
    for (index, (html, properties)) in verter_steps.iter().enumerate() {
        assert_eq!(
            properties.len(),
            2,
            "state {index}: exactly the two v-bind expressions — the literal `teal` \
             declaration must contribute no custom property.\nhtml:\n{html}\nmodule:\n{module_code}"
        );
        for name in properties.keys() {
            assert!(
                name.starts_with("--") && !name.starts_with("----"),
                "state {index}: the runtime must apply exactly one `--` prefix; a doubled \
                 prefix would leave `var(--name)` reading nothing.\nmodule:\n{module_code}"
            );
        }
        assert_eq!(
            applied_values(properties),
            expected[index],
            "state {index}: the applied values must track the reactive prop change.\n\
             module:\n{module_code}"
        );
    }

    for (index, (verter_step, oracle_step)) in
        verter_steps.iter().zip(oracle_steps.iter()).enumerate()
    {
        assert_eq!(
            applied_values(&verter_step.1),
            applied_values(&oracle_step.1),
            "state {index}: Verter's mounted custom-property values must match the \
             official module's for the same input.\nVerter module:\n{module_code}\n\
             official module:\n{official_code}"
        );
    }
}
