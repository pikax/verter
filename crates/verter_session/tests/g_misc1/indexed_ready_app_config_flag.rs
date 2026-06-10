//! Track 2.2 / Track 2.3 — `IndexedReady::declares_interface_app_config`
//! fixture coverage.
//!
//! The flag is the production input the
//! `AppConfigNoOverrideProofDb` producer consults to short-circuit
//! files that demonstrably cannot contribute an
//! `interface AppConfig` override. This fixture proves:
//!
//! - File without `interface AppConfig` → flag is `false`.
//! - File with `interface AppConfig` → flag is `true`.
//! - File with `interface AppConfig` inside `declare module` → flag is `true`.
//! - File with `type AppConfig = ...` (alias) → flag is `false`.
//! - Upserting from no-AppConfig to has-AppConfig → flag toggles to `true`.
//!
//! Discrimination: each assertion compares the flag on the
//! materialized `IndexedReady` artifact against the expected value
//! for the upserted source. A tree without the flag fails to compile
//! this test, and a tree with a buggy producer would observe `false`
//! for the positive cases. The dual positive/negative coverage
//! discriminates a real detector from a hard-coded constant.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

fn make_host() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert_ts(host: &Arc<VerterHost>, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
}

/// Materialize `IndexedReady` via the production analyze path and
/// return the `declares_interface_app_config` flag. The
/// `analyze_with_audit` entrypoint forces the
/// `ensure_indexed_ready` materialisation step that drives our
/// projection.
fn read_flag(host: &Arc<VerterHost>, canonical: &str) -> Option<bool> {
    let _ = host.analyze_with_audit(canonical);
    host.project_type_store()
        .indexed()
        .get_any(canonical)
        .map(|ir| ir.declares_interface_app_config)
}

#[test]
fn indexed_ready_flag_false_for_file_without_app_config() {
    let host = make_host();
    let canonical = "/proj/no-app-config.ts";
    upsert_ts(
        &host,
        canonical,
        "export type Foo = { theme: string };\nexport interface Bar { theme: string }",
    );

    let flag = read_flag(&host, canonical).expect("IndexedReady must exist after upsert");
    assert!(
        !flag,
        "IndexedReady.declares_interface_app_config must be false for a file without `interface AppConfig`"
    );
}

#[test]
fn indexed_ready_flag_true_for_file_with_interface_app_config() {
    let host = make_host();
    let canonical = "/proj/has-app-config.ts";
    upsert_ts(
        &host,
        canonical,
        "export interface AppConfig { theme: string }",
    );

    let flag = read_flag(&host, canonical).expect("IndexedReady must exist after upsert");
    assert!(
        flag,
        "IndexedReady.declares_interface_app_config must be true for a file with `export interface AppConfig`"
    );
}

#[test]
fn indexed_ready_flag_true_for_interface_inside_declare_module() {
    let host = make_host();
    let canonical = "/proj/declare-module-app-config.ts";
    upsert_ts(
        &host,
        canonical,
        r#"
declare module '@nuxt/schema' {
    interface AppConfig {
        button: { variants: string[] }
    }
}
"#,
    );

    let flag = read_flag(&host, canonical).expect("IndexedReady must exist after upsert");
    assert!(
        flag,
        "IndexedReady.declares_interface_app_config must be true for `interface AppConfig` nested in `declare module`"
    );
}

#[test]
fn indexed_ready_flag_false_for_type_alias_app_config() {
    let host = make_host();
    let canonical = "/proj/type-alias-app-config.ts";
    upsert_ts(
        &host,
        canonical,
        "export type AppConfig = { theme: string };",
    );

    let flag = read_flag(&host, canonical).expect("IndexedReady must exist after upsert");
    assert!(
        !flag,
        "IndexedReady.declares_interface_app_config must be false for `type AppConfig` alias (only interfaces merge)"
    );
}

/// Track 2.3 — invalidation fixture: a file that starts without
/// `interface AppConfig` and is then upserted to declare one. The
/// `IndexedReady` artifact must reflect the new flag value, and the
/// reverse direction (delete the interface) must reset it to `false`.
///
/// Discrimination: the second upsert produces a fresh `IndexedReady`
/// keyed by the new content_hash. The flag toggle proves the
/// detector observes the new source, not a cached pre-edit value.
#[test]
fn indexed_ready_flag_toggles_on_upsert_to_has_app_config() {
    let host = make_host();
    let canonical = "/proj/will-add-app-config.ts";

    // Step 1: no AppConfig.
    upsert_ts(&host, canonical, "export interface Foo { theme: string }");
    let flag_before = read_flag(&host, canonical).expect("IndexedReady exists after first upsert");
    assert!(
        !flag_before,
        "pre-edit: flag must be false before the file declares interface AppConfig"
    );

    // Step 2: add interface AppConfig.
    upsert_ts(
        &host,
        canonical,
        "export interface Foo { theme: string }\nexport interface AppConfig { theme: string }",
    );
    let flag_after = read_flag(&host, canonical).expect("IndexedReady exists after second upsert");
    assert!(
        flag_after,
        "post-edit: flag must be true once the file declares interface AppConfig"
    );

    // Step 3: remove interface AppConfig again.
    upsert_ts(&host, canonical, "export interface Foo { theme: string }");
    let flag_removed = read_flag(&host, canonical).expect("IndexedReady exists after third upsert");
    assert!(
        !flag_removed,
        "reverse-edit: flag must be false once interface AppConfig is removed"
    );
}
