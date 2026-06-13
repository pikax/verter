//! Known-but-unsupported language semantics (structural).
//!
//! `.svelte` classifies through a LANDED `LanguageRegistry` row — a
//! known language, never unknown-extension fallthrough — but no carrier
//! implementation is registered behind its adapter id. The
//! row-without-carrier is the STRUCTURAL source of the typed
//! `UnsupportedLanguage` state: host dispatch finds the row, finds no
//! carrier, and returns the typed error. Never a silent empty result,
//! never a panic; when a carrier implementation registers for the
//! adapter, this error path goes dead naturally.

use std::sync::Arc;

use verter_session::{
    CompileProfile, FileLanguage, HostConfig, HostError, UpsertRequest, VerterHost,
};

const SVELTE_FIXTURE: &str =
    "<script lang=\"ts\">\n  export let name: string;\n</script>\n\n<h1>Hello {name}</h1>\n";

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

fn assert_typed_unsupported(err: &HostError) {
    match err {
        HostError::Scheduler(verter_scheduler::job::SchedulerError::UnsupportedLanguage {
            file_id,
            adapter_id,
        }) => {
            assert_eq!(file_id, "/src/Box.svelte");
            assert_eq!(
                adapter_id,
                &verter_session::FrameworkAdapterId::svelte(),
                "the typed error must carry the carrier-less row's adapter id"
            );
        }
        other => panic!(
            "expected the typed UnsupportedLanguage scheduler error, got: {other:?} \
             (a stringly StageFailed or a silent success would break the \
             row-without-carrier contract)"
        ),
    }
}

/// An explicit-kind request (the FFI `"svelte"` string maps to this
/// language) surfaces the typed error from dispatch — asserting the
/// ERROR KIND, not just non-success.
#[test]
fn explicit_svelte_kind_surfaces_typed_unsupported_language() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let err = upsert_svelte(&host, FileLanguage::svelte())
        .expect_err("a carrier-less framework language must not parse silently");
    assert_typed_unsupported(&err);
}

/// A path-classified request (no explicit kind — the host classifier
/// resolves `.svelte` through the registry row) reaches the same typed
/// state. DISCRIMINATING vs the retired routing: `.svelte` used to be
/// an unknown extension routed as a plain script, which would have
/// "succeeded" by parsing Svelte source as TypeScript.
#[test]
fn path_classified_svelte_surfaces_typed_unsupported_language() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let classified = host.language_classifier().classify("/src/Box.svelte");
    assert_eq!(classified, FileLanguage::svelte());
    let err = upsert_svelte(&host, classified)
        .expect_err("path-classified .svelte must not parse silently");
    assert_typed_unsupported(&err);
}

/// Cross-file import of a carrier-less language: an importer that
/// references a `.svelte` file keeps working. The `.svelte` upsert
/// fails typed (DISCRIMINATING vs the retired routing, where `.svelte`
/// parsed silently as TypeScript and the import "succeeded" with
/// garbage), and the importer's own upsert, compile, and meta queries
/// complete without hanging — the dependency failure stays typed and
/// local, never poisoning subsequent requests.
#[test]
fn importer_of_svelte_file_degrades_typed_without_poisoning() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let err = upsert_svelte(&host, FileLanguage::svelte())
        .expect_err("the carrier-less dependency itself must fail typed");
    assert_typed_unsupported(&err);

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
        .expect("an importer of a carrier-less file must keep working");
    assert!(
        update.changed,
        "the importer upsert must register fresh content"
    );

    let profile = CompileProfile {
        target: verter_compiler::compile::CompileTarget::IDE,
        ..CompileProfile::default()
    };
    host.ensure_compiled("/src/App.vue", &profile)
        .expect("importer compile must not be poisoned by the failed dependency");
    assert!(
        host.get_ide("/src/App.vue", &profile).is_some(),
        "the importer's IDE virtual file must exist"
    );

    // Meta resolution over the importer terminates and reflects the
    // importer's own surface — no hang, no panic, no dependency bleed.
    let meta = host
        .get_component_meta("/src/App.vue")
        .expect("component meta over the importer must resolve");
    assert!(
        meta.props.iter().any(|p| p.name == "label"),
        "the importer's own props must resolve despite the carrier-less import"
    );
}

/// LSP-exposure inertness, host half: a watched `.svelte` file produces
/// no virtual-file state — compile requests fail typed, and the IDE
/// virtual-file pipeline yields nothing to sync to a type provider.
#[test]
fn svelte_produces_no_virtual_file_state() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_svelte(&host, FileLanguage::svelte());

    let profile = CompileProfile {
        target: verter_compiler::compile::CompileTarget::IDE,
        ..CompileProfile::default()
    };
    assert!(
        host.ensure_compiled("/src/Box.svelte", &profile).is_err(),
        "compile of a carrier-less language must fail typed, not succeed empty"
    );
    assert!(
        host.get_ide("/src/Box.svelte", &profile).is_none(),
        "no IDE virtual file may exist for a carrier-less language"
    );
}

/// A re-upsert that carries a DIFFERENT resolved language re-homes the
/// file onto the new language — the scheduler must not keep executing
/// the Source stage with the stale language of the first upsert.
/// DISCRIMINATING both ways: with a stale-node scheduler, the second
/// upsert silently parses `.svelte` as TypeScript (first direction) or
/// keeps failing a now-plain-script request (second direction).
#[test]
fn relabelled_upsert_reroutes_to_the_new_language() {
    // Direction 1: script first, then the carrier-less framework row.
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
    let err = upsert_svelte(&host, FileLanguage::svelte())
        .expect_err("re-upserting as the carrier-less language must fail typed");
    assert_typed_unsupported(&err);

    // Direction 2: the carrier-less row first, then a plain script.
    let host = VerterHost::new_standalone(HostConfig::default());
    let err = upsert_svelte(&host, FileLanguage::svelte())
        .expect_err("the carrier-less upsert fails typed");
    assert_typed_unsupported(&err);
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
