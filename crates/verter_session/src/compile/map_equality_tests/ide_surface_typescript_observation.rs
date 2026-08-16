//! The IDE/TSX product family, observed by the real TypeScript compiler inside
//! the workspace declaration domain.
//!
//! An IDE projection is a JSX module whose meaning lives behind
//! `@jsxImportSource @verter/svelte-jsx` (`crates/verter_compiler/src/svelte/ide/prelude.rs:30`)
//! and `@verter/types`. Observed without those resolvable, TypeScript types
//! every reference to them `any` and a correct projection observes exactly like
//! a broken one. So the observation runs in the harness's WORKSPACE domain: the
//! `@verter/*` declaration packages are mapped to their own on-disk directories
//! through TypeScript's own `paths`, JSX is enabled so the pragma is not inert,
//! and a missing package or unresolved reference REFUSES the observation rather
//! than degrading it.
//!
//! The Svelte carrier's projection is documented as type-checking clean through
//! TSGO (`crates/verter_compiler/src/svelte/carrier.rs:186`). That is a claim;
//! what these tests record is what TypeScript actually says.
//!
//! Run with
//! `cargo test -p verter_session --lib --features bf2-authoritative
//! ide_surface_typescript_observation -- --test-threads=1 --nocapture`.
//!
//! WITHOUT `--features bf2-authoritative` this module is not compiled in, so a
//! filter naming it matches ZERO tests and `cargo test` still exits 0. Read the
//! `running N tests` line, never the exit code. libtest's filter is one literal
//! substring — it has no alternation, so `"a\\|b"` matches nothing at all.

use super::bf2_seed_matrix::{harness_root, run_bounded, TempCandidate, ORACLE_TIMEOUT};
use super::*;

use crate::framework::framework_product_surface_tests::host_with;

/// Drive the harness's observation validator over one artifact in the workspace
/// declaration domain.
fn observe_in_workspace_domain(label: &str, artifacts: &[(&str, &str)]) -> Value {
    let artifacts: Vec<Value> = artifacts
        .iter()
        .map(|(file_name, code)| json!({ "fileName": file_name, "code": code }))
        .collect();
    let input = TempCandidate::write(
        label,
        &json!({
            "frameworkDomain": "workspace",
            "artifacts": artifacts,
        })
        .to_string(),
    );

    let mut command = Command::new("node");
    command
        .arg(harness_root().join("bin/observe-typescript.mjs"))
        .arg("--input")
        .arg(&input.path)
        .current_dir(harness_root());
    let finished = run_bounded(&mut command, ORACLE_TIMEOUT);
    assert!(
        !finished.timed_out,
        "{label}: the TypeScript observation did not finish within {ORACLE_TIMEOUT:?}.\n\
         stderr:\n{}",
        finished.stderr
    );
    let record: Value = serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: the observer emitted no JSON ({error}).\nstdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    });
    assert_eq!(
        finished.code,
        Some(0),
        "{label}: the observation was REFUSED ({record}). A refusal here means the workspace \
         declaration domain is incomplete — build the TypeScript packages with `pnpm build:ts` — \
         and it is deliberately NOT degraded into an `any`-typed observation.",
    );
    assert_eq!(
        record["observationDomain"]["framework"], "workspace",
        "{label}: observed outside the workspace declaration domain"
    );
    record
}

/// The IDE profile — the one the LSP uses. `CompileTarget::IDE` carries the TSX
/// bit, which is what drives `want_ide` through the carrier.
fn ide_profile() -> CompileProfile {
    CompileProfile {
        target: verter_compiler::compile::CompileTarget::IDE,
        source_map: true,
        ..CompileProfile::default()
    }
}

/// Publish one carrier's IDE projection through the shipped route.
fn published_ide(canonical: &str, source: &str, language: verter_language::FileLanguage) -> String {
    let host = host_with(canonical, source, language);
    let profile = ide_profile();
    assert!(
        host.ensure_ide_compiled(canonical, &profile)
            .unwrap_or_else(|error| panic!("{canonical}: ensure_ide_compiled failed: {error:?}")),
        "{canonical}: the carrier reported no IDE projection"
    );
    let ide = host
        .get_ide(canonical, &profile)
        .unwrap_or_else(|| panic!("{canonical}: no IDE product after a successful ensure"));
    assert!(
        !ide.code.is_empty(),
        "{canonical}: the IDE product is empty"
    );
    ide.code.to_string()
}

fn diagnostics_of(record: &Value) -> Vec<String> {
    record["diagnostics"]
        .as_array()
        .expect("the observation carries a diagnostics array")
        .iter()
        .map(|diagnostic| {
            format!(
                "TS{} {}:{:?} {}",
                diagnostic["code"],
                diagnostic["source"].as_str().unwrap_or("<none>"),
                diagnostic["start"]["line"],
                diagnostic["message"][0].as_str().unwrap_or("")
            )
        })
        .collect()
}

const SVELTE_COMPONENT: &str = "<script lang=\"ts\">\n  let { label, disabled = false }: { label: string; disabled?: boolean } = $props();\n  let count = $state(0);\n</script>\n\n<button {disabled} onclick={() => count++}>{label}: {count}</button>\n";
const VUE_COMPONENT: &str = "<script setup lang=\"ts\">\nconst props = defineProps<{ label: string; disabled?: boolean }>()\nconst count = 0\n</script>\n\n<template>\n  <button :disabled=\"props.disabled\">{{ props.label }}: {{ count }}</button>\n</template>\n";

// ══════════════════════════════════════════════════════════════════════════
// The domain itself is decisive — the planted control
// ══════════════════════════════════════════════════════════════════════════

/// PLANTED CONTROL — a correct JSX surface and a broken one observe
/// DIFFERENTLY in the workspace domain.
///
/// Without the `@verter/svelte-jsx` declarations resolvable, `@jsxImportSource`
/// is inert, every element type is `any`, and both artifacts below observe
/// identically with zero diagnostics. This is the test that proves the domain
/// could have said otherwise — the `any == any` failure mode, ruled out.
///
/// The plant is proven applied: the marker attribute is absent from the correct
/// artifact and present exactly once in the broken one, and the two strings
/// differ.
#[test]
fn the_workspace_domain_distinguishes_a_correct_jsx_surface_from_a_broken_one() {
    const PLANT: &str = "verterPlantedInvalidAttribute";
    let correct = "/** @jsxImportSource @verter/svelte-jsx */\nexport function Probe() {\n  return <button disabled={true}>ok</button>;\n}\n".to_string();
    let broken = correct.replacen("disabled={true}", &format!("{PLANT}={{1}}"), 1);

    assert_ne!(broken, correct, "the plant did not change the artifact");
    assert_eq!(
        correct.matches(PLANT).count(),
        0,
        "the plant's marker already occurs in the correct artifact, so finding it afterwards \
         would prove nothing"
    );
    assert_eq!(
        broken.matches(PLANT).count(),
        1,
        "the plant's marker is not present exactly once after planting"
    );

    let correct_record =
        observe_in_workspace_domain("control/correct", &[("/Probe.tsx", &correct)]);
    let broken_record = observe_in_workspace_domain("control/broken", &[("/Probe.tsx", &broken)]);

    assert_eq!(
        diagnostics_of(&correct_record),
        Vec::<String>::new(),
        "the correct JSX surface does not type-check in the workspace domain"
    );
    let broken_diagnostics = diagnostics_of(&broken_record);
    assert!(
        !broken_diagnostics.is_empty(),
        "the broken JSX surface produced NO diagnostic, so the domain cannot distinguish it from \
         a correct one — the element types are almost certainly `any`"
    );
    assert!(
        broken_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains(PLANT)),
        "the broken surface's diagnostics do not name the planted attribute, so the failure is \
         not the one the plant introduced: {broken_diagnostics:?}"
    );
    // Both ran under the same environment, so the difference is the artifact's.
    assert_eq!(
        correct_record["compilerOptions"], broken_record["compilerOptions"],
        "the two observations ran under different compiler options"
    );
    assert_eq!(
        correct_record["typescript"], broken_record["typescript"],
        "the two observations ran under different TypeScript versions"
    );
    // And the domain really is the JSX one: `jsx` is enabled and the workspace
    // packages are mapped, or the pragma above would have been inert.
    assert_eq!(
        correct_record["compilerOptions"]["jsx"], "ReactJSX",
        "JSX is not enabled, so the `@jsxImportSource` pragma is inert"
    );
    let mappings = correct_record["compilerOptions"]["pathMappings"]
        .as_array()
        .expect("the workspace domain records its path mappings");
    for package in ["@verter/svelte-jsx", "@verter/types"] {
        assert!(
            mappings.iter().any(|mapping| mapping == package),
            "the workspace domain does not map `{package}`: {mappings:?}"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════
// The published IDE surfaces, per carrier
// ══════════════════════════════════════════════════════════════════════════

/// The Svelte carrier's published IDE projection, observed.
///
/// `crates/verter_compiler/src/svelte/carrier.rs:186` states the output "is a
/// `.svelte.tsx` that type-checks clean through TSGO". This records what
/// TypeScript says about it in the workspace declaration domain, in both
/// directions: it fails if a clean surface starts producing diagnostics AND if
/// a diagnostic-producing one becomes clean.
#[test]
fn the_svelte_ide_projection_observes_as_this_suite_records_it() {
    let code = published_ide(
        "/probe/Ide.svelte",
        SVELTE_COMPONENT,
        verter_language::FileLanguage::svelte(),
    );
    assert!(
        code.contains("@jsxImportSource @verter/svelte-jsx"),
        "the Svelte IDE projection lost its JSX pragma, so this observation is not measuring the \
         projected surface:\n{code}"
    );
    let record = observe_in_workspace_domain("svelte/ide", &[("/Ide.svelte.tsx", &code)]);
    let diagnostics = diagnostics_of(&record);
    println!("svelte IDE projection diagnostics: {diagnostics:#?}");
    assert_eq!(
        diagnostics,
        Vec::<String>::new(),
        "the Svelte IDE projection does not type-check clean in the workspace declaration \
         domain, contrary to the claim at `crates/verter_compiler/src/svelte/ide/mod.rs`. This \
         suite records the observation; it owns no correction."
    );
}

/// The Vue carrier's published IDE projection, observed the same way.
#[test]
fn the_vue_ide_projection_observes_as_this_suite_records_it() {
    let canonical = "/probe/Ide.vue";
    let host = host_with(
        canonical,
        VUE_COMPONENT,
        verter_language::FileLanguage::vue(),
    );
    let profile = ide_profile();
    assert!(
        host.ensure_ide_compiled(canonical, &profile)
            .expect("vue ensure_ide_compiled"),
        "the Vue carrier reported no IDE projection"
    );
    let code = host
        .get_ide(canonical, &profile)
        .expect("no Vue IDE product after a successful ensure")
        .code
        .to_string();
    assert!(!code.is_empty(), "the Vue IDE product is empty");

    // The Vue projection is NOT a standalone module: it imports its own
    // generated companion (`./Ide.vue.verter.js`). Observing the `.tsx` alone
    // refuses on that unresolved specifier — correctly, since the companion's
    // types are exactly what the projection's own references mean. The
    // companion is the SAME carrier's public-API surface in its
    // TypeScript-labeled rendering (`TscResponse::ts_labeled_code`, the
    // rendering every fixed `.verter.ts`-shaped destination takes), so the cell
    // is a two-artifact observation.
    assert!(
        code.contains("./Ide.vue.verter.js"),
        "the Vue IDE projection no longer imports its companion, so this cell's artifact set is \
         stale:\n{code}"
    );
    let companion = host
        .get_public_api(canonical)
        .expect("vue public api")
        .expect("the Vue carrier publishes a companion surface")
        .ts_labeled_code()
        .to_string();

    let record = observe_in_workspace_domain(
        "vue/ide",
        &[("/Ide.vue.tsx", &code), ("/Ide.vue.verter.ts", &companion)],
    );
    let diagnostics = diagnostics_of(&record);
    println!("vue IDE projection diagnostics: {diagnostics:#?}");
    assert_eq!(
        diagnostics,
        Vec::<String>::new(),
        "the Vue IDE projection does not type-check clean in the workspace declaration domain. \
         This suite records the observation; it owns no correction."
    );
}
