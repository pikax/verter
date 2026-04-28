//! Phase 5b §5.A — TDD seed for resolver coverage gap: deeply
//! indexed paths (`A['c']['full']['bar']`) lose path context across
//! hops in the engine's `project_type_member` chain.
//!
//! **Root cause (per sub-plan §5 commit 8):** engine's
//! `project_type_member` chains via the prepared-decl resolver and
//! drops path context across hops. `ProjectPath{base, [seg1, seg2,
//! seg3, seg4], Expanded}` walks via `PathWalker` with a single
//! consistent context, never losing intermediate hops.
//!
//! **Pre-Phase-5b behaviour:** for `Cfg['nested']['theme']['palette']`,
//! the resolver loses one of the intermediate hops and surfaces the
//! WRONG branch's members, the WRONG type, or `Unknown`.
//!
//! **Post-Phase-5b expected:** props extracted are exactly the leaf
//! `palette` members (`primary`, `secondary`), with no leakage from
//! sibling branches at any intermediate level.
//!
//! This seed remains RED through the end of Phase 5b. It closes in
//! 5f §8 via callsite migration.

use crate::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

/// Indexed path through a generic instantiation: `WrappedConfig<Theme>['nested']['palette']`.
/// Pre-fix, the engine's `project_type_member` chain loses generic
/// substitution context across the hop sequence — the inner
/// `T['palette']` hop sees `T` as unbound and the resolver returns
/// nothing or returns the wrong branch. Post-fix, `ProjectPath`
/// dispatch threads `T → Theme` consistently across all hops and the
/// leaf `palette` members surface.
const INDEXED_PATHS_VUE: &str = r#"<script setup lang="ts">
import type { WrappedConfig, Theme } from './deep_cfg';
defineProps<WrappedConfig<Theme>['nested']['palette']>();
</script>
<template><div /></template>
"#;

const DEEP_CFG_TS: &str = r#"export interface WrappedConfig<T> {
  nested: T;
  topSibling: { foo: string };
}
export interface Theme {
  palette: {
    primary: string;
    secondary: string;
  };
  typography: {
    font: string;
  };
}
"#;

#[test]
#[ignore = "Phase 5b §5.A seed: closes in Phase 5f (commit 8) via dispatch ProjectPath migration. Verified FAIL pre-impl on commit 1."]
fn resolver_coverage_indexed_paths_deep_chain() {
    let host = build_hermetic_host_with_lib(
        &[("/c.vue", INDEXED_PATHS_VUE), ("/deep_cfg.ts", DEEP_CFG_TS)],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();

    // Discriminating positive: leaf `palette` members surface.
    for required in ["primary", "secondary"] {
        assert!(
            prop_names.iter().any(|n| n == required),
            "deep-indexed leaf `palette.{required}` must surface; got {prop_names:?}"
        );
    }

    // Discriminating negative: NO sibling-branch leakage from any of
    // the intermediate hops.
    for forbidden in [
        "font",       // theme.typography.font
        "foo",        // WrappedConfig.topSibling.foo
        "typography", // sibling of palette in Theme
        "topSibling", // sibling of nested in WrappedConfig
        "nested",     // intermediate hop, not leaf
    ] {
        assert!(
            !prop_names.iter().any(|n| n == forbidden),
            "sibling `{forbidden}` must NOT leak into Cfg['nested']['theme']['palette']; got {prop_names:?}"
        );
    }

    // Discriminating exact-arity: exactly the two leaf members. If
    // pre-fix surfaces the wrong branch's members or extra leakage,
    // this fails.
    assert_eq!(
        prop_names.len(),
        2,
        "deep-indexed projection must surface exactly 2 leaf members; got {} ({:?})",
        prop_names.len(),
        prop_names
    );
}
