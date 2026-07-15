//! Discriminating test (v8 AMENDMENT-S): a namespace import
//! (`import * as ns from "X"`) admits a fact whose
//! [`ResolvedImportClauseEntry::space`] is
//! [`verter_semantic::facts::registry::SymbolSpace::Namespace`].
//!
//! **Discrimination:** Pre-`1.f` state has no producer; pre-v8
//! state classified namespace imports as `Type` or `Value` only.
//! Both bugs make the test FAIL:
//!
//! 1. No producer → no entry → `expect` panics.
//! 2. Wrong space (Type/Value) → namespace counter stays at 0 and
//!    the `entry.space` assertion fails.
//!
//! Post-GREEN the producer classifies namespace imports correctly
//! and bumps `resolved_import_facts_namespace_admissions`.

use std::sync::Arc;

use verter_semantic::facts::registry::SymbolSpace;
use verter_session::session_view::{HostView, SessionView};
use verter_session::{
    CompileErrorPolicy, DependencyResolution, FileLanguage, HostConfig, UpsertRequest, VerterHost,
};

#[test]
fn namespace_import_admits_namespace_space_fact() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }));

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/lib.ts".to_string(),
            source: Arc::from("export const x = 1;\nexport const y = 2;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("lib upsert");

    // `import * as ns from "./lib"` — namespace import.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/ns_owner.ts".to_string(),
            source: Arc::from(
                "import * as lib from './lib';\nexport const total = lib.x + lib.y;\n",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("ns_owner upsert");

    let before_ns = host
        .project_type_store()
        .resolved_import_facts()
        .resolved_import_facts_namespace_admissions();

    host.set_import_dependencies(
        "/ns_owner.ts",
        vec![DependencyResolution {
            specifier: "./lib".to_string(),
            resolved_canonical_id: Some("/lib.ts".to_string()),
            possible_canonical_ids: vec!["/lib.ts".to_string()],
        }],
    );

    let after_ns = host
        .project_type_store()
        .resolved_import_facts()
        .resolved_import_facts_namespace_admissions();

    assert!(
        after_ns > before_ns,
        "namespace admission counter must advance after admitting a namespace import (before={before_ns}, after={after_ns})",
    );

    let view = HostView::new(Arc::clone(&host));
    let payload = view
        .resolved_import_facts("/ns_owner.ts")
        .expect("producer admitted a resolved-import-facts payload for the namespace import");

    let lib_entry = payload
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "lib")
        .expect("namespace local-binding `lib` must be admitted");

    assert_eq!(
        lib_entry.space,
        SymbolSpace::Namespace,
        "v8 AMENDMENT-S: `import * as lib from \"./lib\"` MUST classify as SymbolSpace::Namespace (not Type or Value)",
    );
    assert_eq!(
        lib_entry.resolved_canonical.as_ref().map(|c| c.as_ref()),
        Some("/lib.ts"),
        "the namespace import's resolved canonical must be the target file",
    );
}
