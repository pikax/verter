//! Empty-`macro_type_deps` semantic-axis clearing.
//!
//! When a compile / public-API compute carries an empty
//! `macro_type_deps` list, the cold path SKIPS the external-macro-type
//! collector SETUP (it would return `(None, vec![], empty_set)` anyway —
//! `collect_external_macro_types` iterates only `macro_type_deps`). But
//! `sync_transitive_macro_type_dependencies` MUST still run with the
//! empty set: it unconditionally calls
//! `WorkspaceAccess::replace_semantic_transitive(canonical, {})`, which
//! CLEARS the file's `semantic_transitive` dependency axis (closes F15).
//! Dropping that clearing would leave a stale cross-file dependency
//! edge after a macro type dep is removed.
//!
//! This test pins the clearing is unconditional:
//!   1. An SFC with `defineProps<Foo>()` importing `Foo` from a sibling
//!      `.ts` compiles → `semantic_transitive` contains `/src/types.ts`.
//!   2. The SFC is edited so the macro stops consuming `Foo`
//!      (`defineProps<{ a: number }>()`) while KEEPING the
//!      `import type { Foo } from './types'` line. `macro_type_deps` is
//!      now empty, but the parsed import edge is unchanged — so the
//!      upsert's `replace_parsed_edges` does NOT itself clear
//!      `semantic_transitive` (it only clears on input-differing parsed
//!      edges). The ONLY thing that can clear the stale `/src/types.ts`
//!      edge is the compile path's unconditional sync.
//!
//! Discrimination: an implementation that gated the SYNC (not just the
//! collector setup) behind `!macro_type_deps.is_empty()` would leave the
//! stale `/src/types.ts` edge after step 2, failing the
//! `assert!(deps.is_empty())` below. Because the import edge is held
//! constant, the upsert cannot mask the regression — only the
//! unconditional sync clears the axis, so the test passes ONLY when the
//! sync stays unconditional.

use verter_session::for_tests::workspace_semantic_transitive_deps_for_tests;
use verter_session::{
    CompileProfile, FileLanguage, HostConfig, UpsertRequest, VerterHost, VirtualNodeKind,
    VirtualQuery,
};

fn upsert(host: &VerterHost, canonical: &str, source: &str, kind: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_language: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

fn compile_script(host: &VerterHost, canonical: &str) {
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            // Host default (Session) — the cold path runs the collector +
            // sync. The semantic-axis clearing is mode-independent.
            compile_profile: CompileProfile::default(),
        })
        .expect("compile produces a virtual file");
}

const COMP: &str = "/src/Comp.vue";
const TYPES: &str = "/src/types.ts";

/// SFC carrying a cross-file macro type dependency (`Foo` from `./types`).
const WITH_MACRO_DEP: &str = "<script setup lang=\"ts\">\n\
     import type { Foo } from './types';\n\
     defineProps<Foo>();\n\
     </script>\n";

/// Same SFC with the SAME `import type { Foo }` line kept (so the parsed
/// import edge to `./types` is unchanged and the upsert cannot clear the
/// semantic axis on its own), but the macro no longer consumes `Foo` —
/// `macro_type_deps` is now empty. Only the compile-path sync can clear
/// the stale `/src/types.ts` edge.
const WITHOUT_MACRO_DEP: &str = "<script setup lang=\"ts\">\n\
     import type { Foo } from './types';\n\
     type Local = Foo;\n\
     defineProps<{ a: number }>();\n\
     </script>\n";

#[test]
fn empty_macro_type_deps_still_clears_semantic_transitive_axis() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        TYPES,
        "export interface Foo { a: number; }\n",
        FileLanguage::script_ts(),
    );
    upsert(&host, COMP, WITH_MACRO_DEP, FileLanguage::vue());

    // (1) Compile with the macro type dep present → the semantic axis
    // records the transitive cross-file dependency.
    compile_script(&host, COMP);
    let deps_before = workspace_semantic_transitive_deps_for_tests(&host, COMP);
    assert!(
        deps_before.contains(TYPES),
        "an SFC with `defineProps<Foo>()` importing Foo from './types' must record \
         '{TYPES}' on the semantic_transitive axis (got {deps_before:?})"
    );

    // (2) Remove the macro type dep entirely → empty `macro_type_deps`.
    // The recompute must run `sync_transitive_macro_type_dependencies`
    // with the empty set, CLEARING the stale '/src/types.ts' edge.
    upsert(&host, COMP, WITHOUT_MACRO_DEP, FileLanguage::vue());
    compile_script(&host, COMP);
    let deps_after = workspace_semantic_transitive_deps_for_tests(&host, COMP);
    assert!(
        deps_after.is_empty(),
        "removing the macro type dep makes `macro_type_deps` empty; the unconditional sync MUST \
         clear the semantic_transitive axis, but it still contains {deps_after:?} — the empty-deps \
         collector-setup skip must NOT have gated the clearing"
    );
}
