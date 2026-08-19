//! `compile_with_audit`/`compile_with_audit_options` accept `CompileTarget`
//! as a public Rust-caller preset-selection convenience, but only
//! `BUNDLER`/`IDE`/`TSC` are sanctioned — every other constructible bit
//! combination must refuse with a typed diagnostic rather than silently
//! falling through to a best-effort product-set guess. `CompileTarget`
//! remains `pub` at this one boundary (unlike the general session
//! construction path, which never lets a caller touch it at all — see
//! `host_resolve::compile_request_build`), so this refusal IS the
//! enforcement that keeps its public bitflags surface from bypassing
//! option admission.

use std::sync::Arc;

use verter_session::{CompileTarget, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   const greeting = 'hello';\n\
                   </script>\n\
                   <template><div>{{ greeting }}</div></template>\n";

fn build_host() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/u.vue".into()),
        input_id: "/u.vue".into(),
        source: Arc::from(SFC),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });
    host
}

/// `STYLE` alone is a constructible `CompileTarget` bit combination (no
/// caller has ever passed it here — verified by the workspace-wide grep
/// `request_from_target`'s own doc comment cites) but is NOT one of the
/// three sanctioned presets. DISCRIMINATING: without the preset gate this
/// would silently fall through to `request_from_target`'s empty-products
/// default (a RuntimeClient compile) instead of refusing.
#[test]
fn compile_with_audit_refuses_an_unsanctioned_compile_target_value() {
    let host = build_host();
    let (result, record) = host
        .compile_with_audit("/u.vue", CompileTarget::STYLE)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };
    assert!(
        result.script.is_none() && result.template.is_none() && result.tsx.is_none(),
        "an unsanctioned CompileTarget must produce NO compiled surface \
         (script={:?}, template={:?}, tsx={:?})",
        result.script.is_some(),
        result.template.is_some(),
        result.tsx.is_some(),
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "VerterE004" && e.message.contains("BUNDLER/IDE/TSC")),
        "expected a VerterE004 diagnostic naming the sanctioned preset set, got: {:?}",
        result.errors
    );
    assert_eq!(
        record.canonical_id, "/u.vue",
        "the refusal record still carries the canonical id"
    );
}

/// A bit UNION across two sanctioned presets (`BUNDLER | IDE`) is ALSO not
/// itself a sanctioned preset value (`CompileTarget::BUNDLER` and
/// `CompileTarget::IDE` are distinct bitsets; their union is a THIRD,
/// unsanctioned value) — the same fail-closed rule applies to combinations,
/// not just wholly novel bits.
#[test]
fn compile_with_audit_refuses_a_union_of_two_sanctioned_presets() {
    let host = build_host();
    let (result, _record) = host
        .compile_with_audit("/u.vue", CompileTarget::BUNDLER | CompileTarget::IDE)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };
    assert!(
        result.errors.iter().any(|e| e.code == "VerterE004"),
        "BUNDLER|IDE is not itself a sanctioned preset and must refuse, got: {:?}",
        result.errors
    );
}
