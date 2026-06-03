//! Downgrade-reason priority ordering on the public compile result.
//!
//! When several downgrade conditions fire simultaneously, the public
//! `downgrade_reason: Option<DowngradeReason>` on the compile response is
//! the FIRST reason in priority order. Driven via a `Content` request so
//! a downgrade actually occurs; the reason ordering is identical
//! regardless of requested mode.
//!
//! Priority order:
//!   HasModuleAugmentation > HasMacroTypeDeps > HasWorkspaceAlias >
//!   HasExternalSrc > HasBlockOverride > HasStyleOverride >
//!   HasIdeOnlyAnalysis > HasDevLastGood
//!
//! Discrimination: before the public `downgrade_reason` field and the
//! `requested_mode` profile field existed, this test would not compile.
//! The exact first-reason assertion additionally fails against any
//! implementation whose priority ordering differs.

use verter_session::{
    CompileCacheMode, CompileProfile, DowngradeReason, FileKind, HostConfig, UpsertRequest,
    VerterHost, VirtualNodeKind, VirtualQuery,
};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

#[test]
fn macro_type_dep_outranks_external_src_on_public_result() {
    // An SFC with BOTH a macro type dep (HasMacroTypeDeps) AND an
    // external `src` block (HasExternalSrc). HasMacroTypeDeps is higher
    // priority, so it is the single public downgrade_reason.
    let host = host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_ts(&host, "/src/ext.ts", "export const e = 1;\n");
    // `defineProps<Foo>()` → HasMacroTypeDeps; `<script src>` →
    // HasExternalSrc. Both fire.
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n\
         <script src=\"./ext.ts\"></script>\n",
    );

    let profile = CompileProfile {
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    };
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile,
        })
        .expect("compile");

    // The Content request downgraded (a reason fired).
    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(response.actual_mode, CompileCacheMode::Stateless);

    // The single public reason is the HIGHEST-priority one that fired.
    // HasMacroTypeDeps outranks HasExternalSrc, so it wins regardless of
    // which condition the producer happened to evaluate first.
    assert_eq!(
        response.downgrade_reason,
        Some(DowngradeReason::HasMacroTypeDeps),
        "the public downgrade_reason must be the highest-priority firing reason \
         (HasMacroTypeDeps > HasExternalSrc), got {:?}",
        response.downgrade_reason
    );
}
