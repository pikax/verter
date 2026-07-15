//! Terminal-TypeExpr utility publication invariant.
//!
//! A direct field type such as `Partial<EditorOptions>` is a terminal
//! published utility root, not an intermediate carrier. It must still
//! publish the finite utility result (`Partial` makes every member
//! optional). The carrier-stop regressions are covered by the
//! generic `Omit<Partial<...>>` and ChatMessages-shaped tests where the
//! utility lives inside transit/consumer input rather than at the
//! publication boundary.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

// Each entry module intentionally gets its own copy of this stateless
// fixture helper (no statics/atomics/OnceCell), so the per-entry scopes
// stay disjoint and share no state. The "duplicate mod" the lint reports
// is the intended layout, not an accident — keep the allow at every site.
#[allow(clippy::duplicate_mod)]
#[path = "../component_meta_audit/harness.rs"]
mod harness;

use verter_session::audited_request::AuditedRequest;
use verter_type_expr::{ObjectMember, TypeExpr};

const EDITOR_OPTIONS_TS: &str = r#"
export interface EditorOptions {
  editable?: boolean;
  textDirection?: 'ltr' | 'rtl';
  tabindex?: number;
  clipboardTextSerializer?: (slice: unknown) => string;
  focusEvents?: { onFocus?: () => void };
  keymap?: Record<string, () => void>;
  paste?: (e: Event) => void;
  content?: string;
  element?: HTMLElement;
}
"#;

const EDITOR_VUE: &str = r#"<script setup lang="ts">
import type { EditorOptions } from './editor_options';
defineProps<{ editorOptions: Partial<EditorOptions> }>();
</script>
<template><div></div></template>
"#;

#[test]
fn terminal_partial_field_type_publishes_finite_utility_shape() {
    let host = harness::build_hermetic_host_with_lib(
        &[
            ("/editor_options.ts", EDITOR_OPTIONS_TS),
            ("/Editor.vue", EDITOR_VUE),
        ],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );

    let (_analysis, resolved, _audit) = AuditedRequest::builder()
        .attach_to(std::sync::Arc::clone(&host))
        .resolve_component_meta("/Editor.vue")
        .expect("hermetic resolve must succeed");

    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("component meta should include evaluated types");
    let field = evaluated
        .props
        .iter()
        .find(|field| field.name == "editorOptions")
        .expect("editorOptions prop should be published");

    let field_ty = verter_session::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/Editor.vue",
        field.r#type.present().expect("present source"),
    )
    .unwrap_or_else(|| panic!("`editorOptions`'s published source must demand-materialize"));
    let TypeExpr::Object(object) = &field_ty else {
        panic!(
            "direct terminal Partial<EditorOptions> should resolve to an Object, got {field_ty:?}"
        );
    };

    const EXPECTED_MEMBERS: &[&str] = &[
        "editable",
        "textDirection",
        "tabindex",
        "clipboardTextSerializer",
        "focusEvents",
        "keymap",
        "paste",
        "content",
        "element",
    ];

    let mut published_names: Vec<&str> = Vec::new();
    for member in object.properties.iter() {
        let ObjectMember::Property(property) = member else {
            continue;
        };
        published_names.push(property.name.as_str());
        assert!(
            property.optional,
            "Partial<EditorOptions> should make `{}` optional",
            property.name
        );
    }
    published_names.sort_unstable();

    let mut expected = EXPECTED_MEMBERS.to_vec();
    expected.sort_unstable();

    assert_eq!(
        published_names, expected,
        "direct terminal Partial<EditorOptions> must publish exactly the finite utility keyspace"
    );
}
