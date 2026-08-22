//! PublicApi / TSC / declaration cells, observed by real TypeScript
//! inside the pinned framework closure.
//!
//! Published declarations are `import("vue").PublicProps & …` /
//! `import("svelte").Component<…>`. Without the framework resolvable,
//! TypeScript types those `any` under `skipLibCheck` and a correct
//! surface observes identically to an empty one.
//!
//! Artifacts are rooted in the pinned install
//! (`.oracle-installs/<framework>`). An unresolved module REFUSES the
//! observation (`ModuleResolutionError`), never degrades. Assertions
//! read the checker's view, never declaration bytes.
//!
//! `cargo test -p verter_session --lib --features bf2-authoritative
//! public_api_typescript_observation -- --test-threads=1 --nocapture`
//!
//! Without the feature this module is not compiled. Read the
//! `running N tests` line, never the exit code.

use super::bf2_seed_matrix::{harness_root, run_bounded, TempCandidate, ORACLE_TIMEOUT};
use super::*;

use crate::framework::framework_product_surface_tests::host_with;
use crate::PublicApiMode;

/// The pinned official Vue package version the goldens and the observation
/// domain are both realized from. Cross-checked against the committed Vue
/// golden records by
/// [`the_observation_domains_match_the_committed_golden_pins`].
const VUE_PINNED_PACKAGE_VERSION: &str = "3.6.0-rc.5";

/// Which pinned framework closure gives an artifact's imports their meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domain {
    Vue,
    Svelte,
}

impl Domain {
    fn wire(self) -> &'static str {
        match self {
            Self::Vue => "vue",
            Self::Svelte => "svelte",
        }
    }

    fn language(self) -> verter_language::FileLanguage {
        match self {
            Self::Vue => verter_language::FileLanguage::vue(),
            Self::Svelte => verter_language::FileLanguage::svelte(),
        }
    }
}

/// The observation record the harness's validator produced, or its refusal.
#[derive(Debug)]
enum Observation {
    Taken(Value),
    Refused { unresolved: Value },
}

impl Observation {
    fn taken(self, label: &str) -> Value {
        match self {
            Self::Taken(record) => record,
            Self::Refused { unresolved } => panic!(
                "{label}: the TypeScript observation was REFUSED because these module \
                 references do not resolve in the pinned closure: {unresolved}"
            ),
        }
    }
}

/// Drive the harness's observation validator over one artifact, in one domain.
fn observe(
    label: &str,
    domain: Domain,
    file_name: &str,
    code: &str,
    check_declaration_files: bool,
) -> Observation {
    let input = TempCandidate::write(
        label,
        &json!({
            "frameworkDomain": domain.wire(),
            "checkDeclarationFiles": check_declaration_files,
            "artifacts": [{ "fileName": file_name, "code": code }],
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
    assert!(
        matches!(finished.code, Some(0) | Some(3)),
        "{label}: the observer exited with {:?} instead of observing or refusing.\n\
         stdout:\n{}\nstderr:\n{}",
        finished.code,
        finished.stdout,
        finished.stderr
    );
    let record: Value = serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: the observer emitted no JSON ({error}).\nstdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    });
    if finished.code == Some(3) {
        return Observation::Refused {
            unresolved: record.get("unresolved").cloned().unwrap_or(Value::Null),
        };
    }
    Observation::Taken(record)
}

/// Publish one cell's declaration through the shipped public-API route and
/// observe it.
///
/// The artifact's FILE NAME carries the declaration-only claim: a `.d.ts` name
/// makes TypeScript apply ambient-context rules, so a runtime value statement in
/// the surface becomes a real diagnostic. That is the semantic
/// declaration-only check — not a byte scan for `defineComponent(`.
fn publish_and_observe(
    label: &str,
    domain: Domain,
    canonical: &str,
    source: &str,
    mode: PublicApiMode,
    file_name: &str,
    check_declaration_files: bool,
) -> Value {
    let host = host_with(canonical, source, domain.language());
    let response = host
        .get_public_api_with_mode(canonical, mode, None)
        .unwrap_or_else(|error| panic!("{label}: the public-API route failed: {error:?}"))
        .unwrap_or_else(|| panic!("{label}: the public-API route published nothing"));
    let code = response.ts_labeled_code().to_string();
    observe(label, domain, file_name, &code, check_declaration_files).taken(label)
}

fn diagnostics_of(record: &Value) -> Vec<String> {
    record["diagnostics"]
        .as_array()
        .expect("the observation carries a diagnostics array")
        .iter()
        .map(|diagnostic| {
            format!(
                "{}:{} {}",
                diagnostic["code"], diagnostic["source"], diagnostic["message"][0]
            )
        })
        .collect()
}

/// The observed type of a module's `default` export.
fn default_export_type<'a>(record: &'a Value, file_name: &str) -> &'a Value {
    &record["modules"][file_name]["exports"]["default"]["type"]
}

/// The member names of an observed object-like type, sorted.
fn member_names(observed: &Value) -> Vec<String> {
    observed["members"]
        .as_object()
        .map(|members| {
            let mut names: Vec<String> = members.keys().cloned().collect();
            names.sort();
            names
        })
        .unwrap_or_default()
}

// The harness's committed fixtures.
const VUE_PROPS_EMIT: &str = include_str!(
    "../../../../../packages/framework-conformance-harness/fixtures/vue/props-emit.vue"
);
const SVELTE_PROPS_EVENTS: &str = include_str!(
    "../../../../../packages/framework-conformance-harness/fixtures/svelte/props-events.svelte"
);

/// A Svelte component with a TYPED `$props()` destructure, an instance export
/// and a bindable prop.
const SVELTE_TYPED: &str = "<script lang=\"ts\">\n  let { label, disabled = false }: { label: string; disabled?: boolean } = $props();\n  export function focus(): void {}\n</script>\n\n<button {disabled}>{label}</button>\n";

/// The same component with an UNTYPED `$props()` destructure.
const SVELTE_UNTYPED: &str = "<script>\n  let { label, disabled = false } = $props();\n</script>\n\n<button {disabled}>{label}</button>\n";

// Vue — the three public-API modes

#[test]
fn the_vue_public_surface_types_its_declared_props_and_emits_against_the_pinned_vue_closure() {
    for (mode, label) in [
        (PublicApiMode::Public, "vue/public"),
        (PublicApiMode::Testing, "vue/testing"),
        (PublicApiMode::Declaration, "vue/declaration"),
    ] {
        // `.ts` for every mode here: the declaration-only claim is checked
        // separately below, and the Public/Testing surfaces legitimately carry
        // runtime statements.
        let file_name = "/App.vue.ts";
        let record = publish_and_observe(
            label,
            Domain::Vue,
            "/probe/PropsEmit.vue",
            VUE_PROPS_EMIT,
            mode,
            file_name,
            false,
        );

        assert_eq!(
            record["observationDomain"]["framework"], "vue",
            "{label}: observed outside the pinned Vue closure"
        );
        assert_eq!(
            record["observationDomain"]["packageVersion"].as_str(),
            Some(VUE_PINNED_PACKAGE_VERSION),
            "{label}: observed against a different Vue package version than the pinned one"
        );
        assert_eq!(
            diagnostics_of(&record),
            Vec::<String>::new(),
            "{label}: the published surface does not type-check against the pinned Vue closure"
        );

        // The instance type the declaration constructs.
        let component = default_export_type(&record, file_name);
        let instance = &component["constructSignatures"][0]["returnType"];
        assert!(
            !instance.is_null(),
            "{label}: the published surface is not constructible: {component}"
        );

        // PROPS — semantic: `$props` is the pinned `PublicProps` intersected
        // with the declared surface, so both halves must be visible.
        let props = instance["members"]["$props"]["display"]
            .as_str()
            .unwrap_or_else(|| panic!("{label}: the instance publishes no `$props`"));
        assert!(
            props.contains("label: string"),
            "{label}: the declared `label` prop is not in the typed instance surface: {props}"
        );
        assert!(
            props.contains("disabled?: boolean"),
            "{label}: the declared optional `disabled` prop is not optional: {props}"
        );
        assert!(
            props.contains("VNodeProps") || props.contains("PublicProps"),
            "{label}: the pinned Vue `PublicProps` did not participate, so the closure was not \
             actually consulted: {props}"
        );

        // EVENTS — the declared emit keeps its literal name in the typed
        // `$emit` signature.
        let emit = instance["members"]["$emit"]["display"]
            .as_str()
            .unwrap_or_else(|| panic!("{label}: the instance publishes no `$emit`"));
        assert!(
            emit.contains("\"toggle\""),
            "{label}: the declared `toggle` event is not in the typed emit surface: {emit}"
        );
    }
}

/// The `Declaration` mode surface really is declaration-only, checked by
/// TypeScript's own ambient-context rules rather than by scanning bytes: a
/// runtime value statement in a `.d.ts` is `TS1036`.
#[test]
fn only_the_vue_declaration_mode_surface_is_ambient_clean_as_a_dts() {
    let declaration = publish_and_observe(
        "vue/declaration-as-dts",
        Domain::Vue,
        "/probe/PropsEmit.vue",
        VUE_PROPS_EMIT,
        PublicApiMode::Declaration,
        "/App.vue.d.ts",
        true,
    );
    assert_eq!(
        diagnostics_of(&declaration),
        Vec::<String>::new(),
        "the Declaration-mode surface is not a valid `.d.ts`"
    );

    // The discriminating half: the Public-mode surface carries a real
    // `defineComponent(...)` call, so the SAME check rejects it. Without this,
    // the assertion above would pass for a mode that publishes runtime code.
    let public = publish_and_observe(
        "vue/public-as-dts",
        Domain::Vue,
        "/probe/PropsEmit.vue",
        VUE_PROPS_EMIT,
        PublicApiMode::Public,
        "/App.vue.d.ts",
        true,
    );
    let public_diagnostics = diagnostics_of(&public);
    assert!(
        !public_diagnostics.is_empty(),
        "the Public-mode surface passed the ambient-context check, so that check cannot \
         distinguish a declaration-only surface from one carrying runtime code"
    );
    // The rejection is the AMBIENT-DECLARATION rule family, not some unrelated
    // type error: TS1046 (a top-level declaration without `declare`/`export`)
    // and TS1254 (a non-literal `const` initializer in an ambient context) are
    // exactly what `const __comp = defineComponent({ … })` violates.
    assert!(
        public_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.starts_with("1046:")),
        "the Public-mode surface's ambient rejection does not include the \
         top-level-declaration rule: {public_diagnostics:?}"
    );
    assert!(
        public_diagnostics
            .iter()
            .all(|diagnostic| ["1036:", "1046:", "1254:"]
                .iter()
                .any(|code| diagnostic.starts_with(code))),
        "the Public-mode surface was rejected for something other than the ambient-declaration \
         rules, so this check is not measuring declaration-only-ness: {public_diagnostics:?}"
    );
}

// Svelte — typed, untyped, and the declaration mode

#[test]
fn a_typed_svelte_props_surface_types_its_props_exports_and_bindings() {
    for (mode, label) in [
        (PublicApiMode::Public, "svelte/typed/public"),
        (PublicApiMode::Declaration, "svelte/typed/declaration"),
    ] {
        let file_name = "/Api.svelte.d.ts";
        let record = publish_and_observe(
            label,
            Domain::Svelte,
            "/probe/Typed.svelte",
            SVELTE_TYPED,
            mode,
            file_name,
            true,
        );

        assert_eq!(
            record["observationDomain"],
            json!({
                "framework": "svelte",
                "packageVersion": super::svelte_official_conformance_matrix::SVELTE_PINNED_PACKAGE_VERSION,
            }),
            "{label}: observed outside the pinned Svelte closure"
        );
        // The Svelte shim is documented as a strictly valid `.d.ts`
        // (`crates/verter_session/src/framework/api_projectors/svelte.rs:117-124`);
        // observing it UNDER a `.d.ts` name is that claim checked by TypeScript.
        assert_eq!(
            diagnostics_of(&record),
            Vec::<String>::new(),
            "{label}: the published Svelte surface is not a clean `.d.ts` against the pinned \
             Svelte closure"
        );

        let component = default_export_type(&record, file_name);
        let call = &component["callSignatures"][0];
        assert!(
            !call.is_null(),
            "{label}: the published surface is not the pinned callable `Component`: {component}"
        );

        // PROPS — the second parameter of the pinned `Component` call
        // signature, structurally expanded by the checker.
        let props = &call["parameters"][1];
        assert_eq!(
            member_names(props),
            vec!["disabled".to_string(), "label".to_string()],
            "{label}: the typed props surface is not what the component declares: {props}"
        );
        assert_eq!(
            props["members"]["label"]["display"], "string",
            "{label}: the `label` prop's type drifted"
        );
        assert_eq!(
            props["members"]["disabled"]["optional"], true,
            "{label}: the defaulted `disabled` prop is not optional"
        );

        // EXPORTS — the call signature's return type carries the instance
        // export alongside the pinned contract's own legacy members.
        let exports = &call["returnType"];
        assert!(
            member_names(exports).contains(&"focus".to_string()),
            "{label}: the instance export is missing from the typed exports: {exports}"
        );
        assert_eq!(
            exports["members"]["focus"]["display"], "() => void",
            "{label}: the instance export's type drifted"
        );

        // BINDINGS — the pinned contract surfaces the third generic argument.
        assert!(
            component["members"]["z_$$bindings"]["display"].is_string(),
            "{label}: the pinned Component's bindings member is absent, so the closure was not \
             actually consulted: {component}"
        );
    }
}

/// A TYPE ANNOTATION does not change which props a Svelte component publishes.
///
/// Two components differing ONLY in whether the `$props()` destructure carries
/// a type annotation must publish the SAME prop NAMES to TypeScript. This
/// replaces the former characterization of the untyped surface as empty, and it
/// is a permanent discriminator in both directions: it fails if the untyped
/// surface regresses to empty, AND it fails if the untyped surface drifts away
/// from the typed one in either direction — a prop appearing on one side only
/// is a divergence whichever side gained it.
///
/// The typed control is what makes either failure decisive: it runs through the
/// same machinery, the same observation domain and the same compiler options
/// (all three asserted equal below), so a difference cannot be attributed to
/// the harness.
#[test]
fn a_svelte_props_type_annotation_does_not_change_which_props_are_published() {
    let file_name = "/Api.svelte.d.ts";
    let untyped = publish_and_observe(
        "svelte/untyped/public",
        Domain::Svelte,
        "/probe/Untyped.svelte",
        SVELTE_UNTYPED,
        PublicApiMode::Public,
        file_name,
        true,
    );
    assert_eq!(
        diagnostics_of(&untyped),
        Vec::<String>::new(),
        "the untyped surface does not type-check, which would be a different finding"
    );
    let untyped_props =
        &default_export_type(&untyped, file_name)["callSignatures"][0]["parameters"][1];

    // The control: the SAME machinery, the SAME domain, a component differing
    // only in its `$props()` type annotation.
    let typed = publish_and_observe(
        "svelte/typed/control",
        Domain::Svelte,
        "/probe/Typed.svelte",
        SVELTE_TYPED,
        PublicApiMode::Public,
        file_name,
        true,
    );
    let typed_props = &default_export_type(&typed, file_name)["callSignatures"][0]["parameters"][1];
    assert_eq!(
        member_names(typed_props),
        vec!["disabled".to_string(), "label".to_string()],
        "the control's props are not visible either, so this observation cannot distinguish an \
         empty surface from an unobservable one"
    );
    // The claim itself: annotating the destructure changes nothing about WHICH
    // props are published.
    assert_eq!(
        member_names(untyped_props),
        member_names(typed_props),
        "the untyped `$props()` destructure publishes a different prop set than the \
         byte-equivalent typed component: untyped={untyped_props}, typed={typed_props}"
    );
    assert_eq!(
        untyped["observationDomain"], typed["observationDomain"],
        "the two observations ran in different closures, so they are not comparable"
    );
    assert_eq!(
        untyped["typescript"], typed["typescript"],
        "the two observations ran under different TypeScript versions"
    );
    assert_eq!(
        untyped["compilerOptions"], typed["compilerOptions"],
        "the two observations ran under different compiler options"
    );
}

/// Untyped `$props()` destructuring still declares a real public component
/// surface: TypeScript must see each authored prop, including the optionality
/// established by a default value.
#[test]
fn an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript() {
    let file_name = "/Api.svelte.d.ts";
    let record = publish_and_observe(
        "svelte/untyped/correct-surface",
        Domain::Svelte,
        "/probe/Untyped.svelte",
        SVELTE_UNTYPED,
        PublicApiMode::Public,
        file_name,
        true,
    );
    assert_eq!(
        diagnostics_of(&record),
        Vec::<String>::new(),
        "the public surface failed before TypeScript could inspect its props"
    );

    let props = &default_export_type(&record, file_name)["callSignatures"][0]["parameters"][1];
    assert_eq!(
        member_names(props),
        vec!["disabled".to_string(), "label".to_string()],
        "TypeScript does not see the two props authored by the untyped `$props()` destructure: \
         {props}"
    );
    assert_eq!(
        props["members"]["label"]["optional"], false,
        "the non-defaulted `label` prop is not required on the TypeScript-visible surface"
    );
    assert_eq!(
        props["members"]["disabled"]["optional"], true,
        "the defaulted `disabled` prop is not optional on the TypeScript-visible surface"
    );
}

/// The committed `props-events.svelte` fixture publishes every prop its untyped
/// `$props()` destructure declares.
#[test]
fn the_committed_svelte_props_fixture_publishes_its_untyped_destructure_props() {
    let file_name = "/PropsEvents.svelte.d.ts";
    let record = publish_and_observe(
        "svelte/props-events/public",
        Domain::Svelte,
        "/probe/PropsEvents.svelte",
        SVELTE_PROPS_EVENTS,
        PublicApiMode::Public,
        file_name,
        true,
    );
    assert_eq!(
        diagnostics_of(&record),
        Vec::<String>::new(),
        "the published surface does not type-check against the pinned Svelte closure"
    );
    let props = &default_export_type(&record, file_name)["callSignatures"][0]["parameters"][1];
    assert_eq!(
        member_names(props),
        vec![
            "disabled".to_string(),
            "label".to_string(),
            "ontoggle".to_string()
        ],
        "the fixture's `label` / `disabled` / `ontoggle` props are not all on the published \
         surface: {props}"
    );
    // The DEFAULTED member is the only optional one — the destructure default is
    // what makes a key optional, so publishing all three as optional (or all
    // three as required) would pass a names-only assertion and still be wrong.
    assert_eq!(
        props["members"]["disabled"]["optional"], true,
        "the defaulted `disabled` prop is not optional"
    );
    assert_eq!(
        props["members"]["label"]["optional"], false,
        "the non-defaulted `label` prop is not required"
    );
    assert_eq!(
        props["members"]["ontoggle"]["optional"], false,
        "the non-defaulted `ontoggle` prop is not required"
    );
}

/// The Svelte projector returns `Ok(None)` for `Testing`, so there is no
/// artifact to observe — recorded as a route fact rather than an empty
/// observation.
#[test]
fn the_svelte_testing_mode_publishes_no_artifact_to_observe() {
    let host = host_with(
        "/probe/Typed.svelte",
        SVELTE_TYPED,
        verter_language::FileLanguage::svelte(),
    );
    assert!(
        host.get_public_api_with_mode("/probe/Typed.svelte", PublicApiMode::Testing, None)
            .expect("the Svelte testing mode is not an error")
            .is_none(),
        "the Svelte Testing mode now publishes an artifact, which must then be observed too"
    );
}

/// The observation domains are the SAME pins the committed goldens were
/// generated against.
///
/// The golden records are the committed authority reachable from here — each
/// digest-verified before it is read — so this compares the pin the observation
/// ran under against the pin the corpus was built from, rather than trusting a
/// transcribed literal alone.
#[test]
fn the_observation_domains_match_the_committed_golden_pins() {
    let svelte = super::svelte_official_conformance_matrix::pinned_svelte_domain();
    assert_eq!(
        svelte["packageVersion"].as_str(),
        Some(super::svelte_official_conformance_matrix::SVELTE_PINNED_PACKAGE_VERSION),
        "the committed Svelte goldens name a different pin than the one asserted above"
    );

    // The Vue side: every committed Vue record must name the Vue pin the
    // observation domain runs under.
    let goldens = harness_root().join("goldens");
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(goldens.join("manifest.json")).expect("the manifest is readable"),
    )
    .expect("the manifest is JSON");
    let mut seen = 0usize;
    for (name, digest) in manifest["entries"]
        .as_object()
        .expect("the manifest carries entries")
        .iter()
        .filter(|(name, _)| name.starts_with("vue/"))
    {
        let digest = digest.as_str().expect("a digest string");
        let record: Value = serde_json::from_str(
            &std::fs::read_to_string(goldens.join("records").join(format!("{digest}.json")))
                .unwrap_or_else(|error| panic!("{name}: cannot read the record: {error}")),
        )
        .unwrap_or_else(|error| panic!("{name}: the record is not JSON: {error}"));
        assert_eq!(
            record["domain"]["packageVersion"].as_str(),
            Some(VUE_PINNED_PACKAGE_VERSION),
            "{name}: this Vue golden names a different official package version than the \
             observation domain runs under"
        );
        seen += 1;
    }
    assert_eq!(seen, 36, "expected 36 committed Vue goldens, saw {seen}");
}
