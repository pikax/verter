//! Known-but-unsupported language semantics (structural) + the Svelte carrier
//! positive-routing tests.
//!
//! The typed `UnsupportedLanguage` state is the STRUCTURAL pre-vertical state for
//! EVERY carrier-less row: host dispatch finds the registry row, finds no
//! carrier, and returns the typed error (never a silent empty, never a panic).
//! Because the Svelte carrier registers, the `.svelte` upsert SUCCEEDS — the
//! positive parse/routing tests below exercise that path, and the typed
//! `UnsupportedLanguage` path STAYS in the substrate (exercised by the Vue
//! framework-template + same-adapter-non-carrier rows further down — the
//! structural pre-vertical state for every carrier-less row).

use std::sync::Arc;

use verter_session::{
    CompileProfile, FileLanguage, HostConfig, HostError, UpsertRequest, VerterHost,
};

const SVELTE_FIXTURE: &str =
    "<script lang=\"ts\">\n  let { name }: { name: string } = $props();\n</script>\n\n<h1>Hello {name}</h1>\n";

fn upsert_svelte(host: &VerterHost, file_language: FileLanguage) -> Result<(), HostError> {
    host.upsert(UpsertRequest {
        canonical_id: Some("/src/Box.svelte".to_string()),
        input_id: "/src/Box.svelte".to_string(),
        source: Arc::from(SVELTE_FIXTURE),
        file_language,
        aliases: Vec::new(),
    })
    .map(|_| ())
}

/// An explicit-kind request (the FFI `"svelte"` string maps to this language)
/// PARSES through the registered Svelte carrier — a `.svelte` request is never
/// typed-rejected. DISCRIMINATING: the upsert SUCCEEDS and a source snapshot exists.
#[test]
fn explicit_svelte_kind_parses_through_the_registered_carrier() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_svelte(&host, FileLanguage::svelte())
        .expect("the registered Svelte carrier parses the .svelte upsert");
    assert!(
        host.get_source("/src/Box.svelte").is_some(),
        "the Svelte carrier produces a source snapshot"
    );
}

/// A path-classified request (no explicit kind — the host classifier resolves
/// `.svelte` through the registry row) routes to the same registered carrier.
#[test]
fn path_classified_svelte_parses_through_the_registered_carrier() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let classified = host.language_classifier().classify("/src/Box.svelte");
    assert_eq!(classified, FileLanguage::svelte());
    upsert_svelte(&host, classified).expect("path-classified .svelte parses through the carrier");
    assert!(host.get_source("/src/Box.svelte").is_some());
}

/// Cross-file import of a `.svelte` from a `.vue` importer: BOTH parse, and the
/// importer's own props resolve — the carrier import no longer fails typed.
#[test]
fn vue_importer_of_svelte_file_resolves_both() {
    let host = VerterHost::new_standalone(HostConfig::default());

    upsert_svelte(&host, FileLanguage::svelte())
        .expect("the .svelte dependency parses through its carrier");

    let importer = "<script setup lang=\"ts\">\n\
                    import Widget from './Box.svelte';\n\
                    defineProps<{ label: string }>();\n\
                    </script>\n\
                    <template><Widget /></template>\n";
    let update = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/App.vue".to_string()),
            input_id: "/src/App.vue".to_string(),
            source: Arc::from(importer),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("the Vue importer upserts");
    assert!(
        update.changed,
        "the importer upsert must register fresh content"
    );

    let profile = CompileProfile {
        target: verter_compiler::compile::CompileTarget::IDE,
        ..CompileProfile::default()
    };
    host.ensure_compiled("/src/App.vue", &profile)
        .expect("importer compile resolves");
    assert!(
        host.get_ide("/src/App.vue", &profile).is_some(),
        "the importer's IDE virtual file must exist"
    );

    // Meta resolution over the importer terminates and reflects the
    // importer's own surface — no hang, the `.svelte` import resolves cleanly.
    let meta = host
        .get_component_meta("/src/App.vue")
        .expect("component meta over the importer must resolve");
    assert!(
        meta.props.iter().any(|p| p.name == "label"),
        "the importer's own props must resolve"
    );
}

/// A re-upsert that carries a DIFFERENT resolved language re-homes the file onto
/// the new language — the scheduler must not keep executing the Source stage
/// with the stale language of the first upsert. Both rows now parse (script and
/// the Svelte carrier); the discriminator is that each upsert produces the
/// row-appropriate source state, never the stale one's artifact.
#[test]
fn relabelled_upsert_reroutes_to_the_new_language() {
    // Direction 1: a plain script first, then the Svelte carrier row.
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Box.svelte".to_string()),
            input_id: "/src/Box.svelte".to_string(),
            source: Arc::from("export const x = 1;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("an explicit plain-script upsert parses as a script");
    assert!(update.changed, "the script upsert registers fresh content");
    upsert_svelte(&host, FileLanguage::svelte())
        .expect("re-upserting as the Svelte carrier parses through the carrier");
    assert!(
        host.get_source("/src/Box.svelte").is_some(),
        "the re-homed Svelte upsert produces source state"
    );

    // Direction 2: the Svelte carrier row first, then a plain script.
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_svelte(&host, FileLanguage::svelte()).expect("the Svelte carrier upsert parses");
    let update = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Box.svelte".to_string()),
            input_id: "/src/Box.svelte".to_string(),
            source: Arc::from("export const x = 1;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("re-upserting as an explicit plain script must succeed");
    assert!(
        update.changed,
        "the re-homed upsert registers fresh content"
    );
    assert!(
        host.get_source("/src/Box.svelte").is_some(),
        "the re-homed script upsert must produce source state"
    );
}

/// A byte-identical re-upsert that changes ONLY the language row is a
/// REAL change: the language routes parse dispatch, so the
/// quintuple-unchanged fast path must not report `changed: false`
/// across a relabel. DISCRIMINATING: with a hash-only no-change
/// predicate, a dialect relabel (Ts → Tsx, identical parse output
/// today) silently fast-paths and downstream state never observes the
/// new row.
#[test]
fn byte_identical_relabel_reports_changed() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "export const x = 1;\n";
    let upsert = |language: FileLanguage| {
        host.upsert(UpsertRequest {
            canonical_id: Some("/src/widget.ts".to_string()),
            input_id: "/src/widget.ts".to_string(),
            source: Arc::from(src),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("script upsert succeeds")
    };

    let first = upsert(FileLanguage::script(verter_session::ScriptSourceType::Ts));
    assert!(first.changed, "first upsert registers fresh content");

    // Same bytes, same language: the quintuple-unchanged fast path.
    let same = upsert(FileLanguage::script(verter_session::ScriptSourceType::Ts));
    assert!(!same.changed, "identical bytes + identical row is a no-op");

    // Same bytes, DIFFERENT language row: a real change.
    let relabel = upsert(FileLanguage::script(verter_session::ScriptSourceType::Tsx));
    assert!(
        relabel.changed,
        "a language relabel must not fast-path as unchanged"
    );
}

/// A framework TEMPLATE row (an external template owned by a component)
/// is NOT a carrier — it carries an adapter id but no carrier language —
/// so the carrier parse dispatch must reject it as the typed
/// `UnsupportedLanguage` state, never route it through the SFC parse path.
///
/// DISCRIMINATING: a dispatch keyed on `adapter_id()` alone (or one that
/// skips the registry carrier-language check) would route a Vue-adapter
/// template through the SFC parse path or fall it through to the
/// plain-script parse — both regressions surface here.
#[test]
fn vue_framework_template_row_is_typed_unsupported_not_routed_to_sfc_parse() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let template_row = FileLanguage::FrameworkTemplate {
        adapter_id: verter_session::FrameworkAdapterId::vue(),
        owner_hint: None,
    };
    let err = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Widget.html".to_string()),
            input_id: "/src/Widget.html".to_string(),
            source: Arc::from("<div>{{ x }}</div>"),
            file_language: template_row,
            aliases: Vec::new(),
        })
        .expect_err("a framework template is not a carrier and must not parse silently");
    match err {
        HostError::Scheduler(verter_scheduler::job::SchedulerError::UnsupportedLanguage {
            adapter_id,
            ..
        }) => {
            assert_eq!(adapter_id, verter_session::FrameworkAdapterId::vue());
        }
        other => panic!("expected typed UnsupportedLanguage for a template row, got: {other:?}"),
    }
}

/// A SAME-ADAPTER NON-CARRIER `Framework` row — the Vue adapter id but a
/// language id that is NOT the `vue` SFC carrier language — must be typed
/// unsupported. Dispatch is keyed on the FULL `(adapter_id, carrier
/// language id)` row, so a Vue-adapter row in a non-SFC language is not
/// routed through the SFC parse path.
#[test]
fn vue_adapter_non_carrier_language_row_is_typed_unsupported() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let non_carrier_row = FileLanguage::Framework {
        adapter_id: verter_session::FrameworkAdapterId::vue(),
        language_id: verter_session::LanguageId::new("vue_template"),
    };
    let err = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Inline.vue_template".to_string()),
            input_id: "/src/Inline.vue_template".to_string(),
            source: Arc::from("<div/>"),
            file_language: non_carrier_row,
            aliases: Vec::new(),
        })
        .expect_err("a same-adapter non-carrier language must not parse through the SFC path");
    match err {
        HostError::Scheduler(verter_scheduler::job::SchedulerError::UnsupportedLanguage {
            adapter_id,
            ..
        }) => {
            assert_eq!(adapter_id, verter_session::FrameworkAdapterId::vue());
        }
        other => panic!(
            "expected typed UnsupportedLanguage for a same-adapter non-carrier row, got: {other:?}"
        ),
    }
}
