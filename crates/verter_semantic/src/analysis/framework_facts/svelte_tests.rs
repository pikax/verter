//! Tests for the Svelte script-fact provider (extracted from `svelte.rs`
//! to keep the production module under the oversize-file guard).

use super::*;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

fn capture(src: &str) -> SvelteScriptCandidates {
    capture_with_module_region(src, None)
}

fn capture_with_module_region(
    src: &str,
    module_region: Option<(u32, u32)>,
) -> SvelteScriptCandidates {
    let alloc = Allocator::default();
    let program = Parser::new(&alloc, src, SourceType::ts()).parse().program;
    capture_svelte_candidates(src, &program, module_region)
}

#[test]
fn captures_props_generic_argument_type() {
    let c = capture("let { name } = $props<{ name: string }>();");
    let props = c.props.expect("props candidate");
    assert!(props.from_generic_argument);
    assert!(props.props_type.is_some());
}

#[test]
fn captures_props_destructuring_annotation_type() {
    let c = capture("let { name }: { name: string } = $props();");
    let props = c.props.expect("props candidate");
    assert!(!props.from_generic_argument);
    assert!(props.props_type.is_some());
}

#[test]
fn captures_props_from_eval_source_with_imports_and_exports() {
    // Mirror the synth-injection eval-source shape: leading blanks, an
    // import-type, the props destructuring, and an exported function.
    let src = "                  \n  import type { WidgetProps } from './props';\n  let { props }: { props: WidgetProps } = $props();\n  export function focus() {}\n";
    let c = capture(src);
    let props = c.props.expect("props candidate from eval-source");
    assert!(
        props.props_type.is_some(),
        "the destructuring annotation must lower to a props type, got None"
    );
    assert!(c.instance_exports.contains(&"focus".to_string()));
}

#[test]
fn captures_bindable_members() {
    let c = capture("let { value = $bindable(), label } = $props();");
    let props = c.props.expect("props candidate");
    assert_eq!(props.bindable_members, vec!["value".to_string()]);
}

#[test]
fn captures_destructuring_default_value() {
    // GAP 1: a destructuring default `size = 'md'` captures the RHS source
    // text as the prop default. DISCRIMINATING: a member with NO default
    // (`label`) records no default entry.
    let c = capture("let { size = 'md', label } = $props();");
    let props = c.props.expect("props candidate");
    let size = props
        .prop_defaults
        .iter()
        .find(|d| d.key == "size")
        .expect("the `size` default is captured");
    assert_eq!(
        size.value, "'md'",
        "the default value is the RHS source text"
    );
    assert!(
        !props.prop_defaults.iter().any(|d| d.key == "label"),
        "a prop WITHOUT a default records no default entry, got {:?}",
        props.prop_defaults
    );
}

#[test]
fn captures_bindable_default_first_argument() {
    // GAP 1: `value = $bindable(false)` captures the `$bindable` first arg
    // `false` as the prop default AND marks `value` bindable.
    let c = capture("let { value = $bindable(false) } = $props();");
    let props = c.props.expect("props candidate");
    assert_eq!(props.bindable_members, vec!["value".to_string()]);
    let value = props
        .prop_defaults
        .iter()
        .find(|d| d.key == "value")
        .expect("the `$bindable(false)` default is captured");
    assert_eq!(value.value, "false");
}

#[test]
fn bindable_no_arg_is_bindable_with_no_default() {
    // GAP 1 DISCRIMINATING: `value = $bindable()` (NO arg) is bindable but
    // contributes NO default — distinct from `$bindable(false)`.
    let c = capture("let { value = $bindable() } = $props();");
    let props = c.props.expect("props candidate");
    assert_eq!(props.bindable_members, vec!["value".to_string()]);
    assert!(
        props.prop_defaults.is_empty(),
        "a no-arg `$bindable()` records no default, got {:?}",
        props.prop_defaults
    );
}

#[test]
fn prop_defaults_fold_into_stable_candidate_hash() {
    // GAP 1 DISCRIMINATING: editing a default VALUE changes the stable
    // candidate hash (so the content-addressed candidate cache misses).
    let a = capture("let { size = 'md' } = $props();");
    let b = capture("let { size = 'lg' } = $props();");
    assert_ne!(
        stable_candidate_hash(&a),
        stable_candidate_hash(&b),
        "an edited default value must change the stable candidate hash"
    );
}

#[test]
fn records_snippet_import_candidate_pairs_without_validating() {
    let src = "import type { Snippet } from 'svelte';\nlet { row }: { row: Snippet } = $props();";
    let c = capture(src);
    assert_eq!(c.snippet_candidates.len(), 1);
    let cand = &c.snippet_candidates[0];
    assert_eq!(cand.member_name, "row");
    assert_eq!(cand.import_source, "svelte");
    // No validation here — the pair is recorded raw.
}

#[test]
fn records_snippet_candidate_from_generic_props_argument() {
    // `$props<{ row: Snippet }>()` records the snippet candidate from the
    // GENERIC argument (not just the destructuring annotation).
    // DISCRIMINATING: without the generic-arg scan this records 0 candidates.
    let src = "import type { Snippet } from 'svelte';\nlet p = $props<{ row: Snippet }>();";
    let c = capture(src);
    assert_eq!(
        c.snippet_candidates.len(),
        1,
        "generic-arg snippet candidate"
    );
    assert_eq!(c.snippet_candidates[0].member_name, "row");
    assert_eq!(c.snippet_candidates[0].import_source, "svelte");
}

#[test]
fn records_userland_snippet_import_source_for_resolved_validation_rejection() {
    // A `Snippet` from a userland module is RECORDED with its source so the
    // resolved-validation can reject it (structural, never a name match).
    let src =
        "import type { Snippet } from './fake-svelte';\nlet { row }: { row: Snippet } = $props();";
    let c = capture(src);
    assert_eq!(c.snippet_candidates.len(), 1);
    assert_eq!(c.snippet_candidates[0].import_source, "./fake-svelte");
}

#[test]
fn captures_instance_exports() {
    let c = capture("export const helper = 1;\nexport function go() {}\nlet local = 2;");
    assert!(c.instance_exports.contains(&"helper".to_string()));
    assert!(c.instance_exports.contains(&"go".to_string()));
    assert!(!c.instance_exports.contains(&"local".to_string()));
}

#[test]
fn exported_runtime_enum_is_an_instance_export() {
    // A plain `export enum E { ... }` is a RUNTIME value binding (the TS
    // stripper lowers it to a runtime JS object), so it IS an instance EXPOSE
    // member. An ambient `export declare enum D` has no runtime emit and is
    // NOT a member; `export type Foo = ...` (type-space) is never a member.
    // DISCRIMINATING: a blanket "all leftover declarations are type-only"
    // wildcard would drop `E`.
    let src = "export enum E { A, B }\nexport declare enum D { X }\nexport type Foo = number;";
    let c = capture(src);
    assert!(
        c.instance_exports.contains(&"E".to_string()),
        "the runtime enum `E` must surface as an instance export, got {:?}",
        c.instance_exports
    );
    assert!(
        !c.instance_exports.contains(&"D".to_string()),
        "the ambient `declare enum D` has no runtime emit and must NOT be a member, got {:?}",
        c.instance_exports
    );
    assert!(
        !c.instance_exports.contains(&"Foo".to_string()),
        "the type alias `Foo` must NOT be a member, got {:?}",
        c.instance_exports
    );
}

#[test]
fn exported_namespace_is_not_an_instance_export() {
    // `export namespace N { ... }` (a `TSModuleDeclaration`) is FULLY stripped
    // by the TS stripper (`strip_types::typescript` removes every
    // `TSModuleDeclaration`, unlike `enum` which it converts to runtime JS),
    // so it produces NO runtime binding and is NOT an instance EXPOSE member.
    // A sibling runtime `export const` in the same script stays. This pins the
    // stripper-aligned rule: enum → member, namespace/module → no member.
    let src = "export namespace N { export const x = 1; }\nexport const real = 2;";
    let c = capture(src);
    assert!(
        c.instance_exports.contains(&"real".to_string()),
        "the runtime `const real` must be an instance export, got {:?}",
        c.instance_exports
    );
    assert!(
        !c.instance_exports.contains(&"N".to_string()),
        "a stripped `namespace N` must NOT surface as a runtime member, got {:?}",
        c.instance_exports
    );
}

#[test]
fn type_only_exports_are_not_instance_exports() {
    // Type-only exports are NOT runtime instance members and must not surface
    // as phantom EXPOSE members:
    //   - `export type { Foo }`   — the whole-statement type-only re-export.
    //   - `export { type Bar, baz }` — an inline `type` specifier (`Bar` is
    //     type-only and dropped; `baz` is a value re-export and stays).
    //   - `export const qux`      — a real value export (stays).
    // DISCRIMINATING: without the `export_kind.is_type()` filter, `Foo` and
    // `Bar` would wrongly enter `instance_exports`.
    let src = "type Foo = number;\nconst Bar = 1;\nconst baz = 2;\nexport type { Foo };\nexport { type Bar, baz };\nexport const qux = 3;";
    let c = capture(src);
    assert!(
        c.instance_exports.contains(&"baz".to_string()),
        "the value re-export `baz` must surface as an instance export, got {:?}",
        c.instance_exports
    );
    assert!(
        c.instance_exports.contains(&"qux".to_string()),
        "the value export `qux` must surface as an instance export, got {:?}",
        c.instance_exports
    );
    assert!(
        !c.instance_exports.contains(&"Foo".to_string()),
        "the type-only re-export `Foo` must NOT surface as an instance member, got {:?}",
        c.instance_exports
    );
    assert!(
        !c.instance_exports.contains(&"Bar".to_string()),
        "the inline `type Bar` specifier must NOT surface as an instance member, got {:?}",
        c.instance_exports
    );
}

#[test]
fn module_exports_are_split_from_instance_exports_by_region() {
    // `export const meta` in the MODULE script region is a module export
    // (NOT an instance member); `export const ready` in the instance region
    // is an instance export. DISCRIMINATING: without the region split both
    // would land in instance_exports.
    let src = "export const meta = 1;\nexport const ready = true;\nlet local = 2;";
    // The module region covers the first export only (`export const meta`).
    let meta_end = src.find('\n').unwrap() as u32;
    let c = capture_with_module_region(src, Some((0, meta_end)));
    assert!(
        c.module_exports.contains(&"meta".to_string()),
        "`meta` in the module region is a module export, got module={:?} instance={:?}",
        c.module_exports,
        c.instance_exports
    );
    assert!(
        !c.instance_exports.contains(&"meta".to_string()),
        "`meta` (module export) must NOT be an instance member"
    );
    assert!(
        c.instance_exports.contains(&"ready".to_string()),
        "`ready` outside the module region is an instance export"
    );
}

#[test]
fn captures_legacy_export_let_props() {
    let c = capture("export let name;\nexport let count = 0;");
    let names: Vec<&str> = c.legacy_props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"name"));
    assert!(names.contains(&"count"));
    let count = c.legacy_props.iter().find(|p| p.name == "count").unwrap();
    assert!(count.has_default);
}

#[test]
fn legacy_export_let_and_var_are_props_not_instance_exports() {
    // A legacy `export let` / `export var` is a PROP, NOT an instance-script
    // EXPOSE member — it must NOT enter `instance_exports` (it would otherwise
    // surface under both PROPS and EXPOSE). `export const` / `export function`
    // ARE instance members. DISCRIMINATING: a kind-blind capture put `name` /
    // `legacyVar` in BOTH.
    let c = capture(
            "export let name;\nexport var legacyVar;\nexport const ready = true;\nexport function focus() {}",
        );
    assert!(
        c.legacy_props.iter().any(|p| p.name == "name"),
        "`export let name` is a legacy prop"
    );
    assert!(
        c.legacy_props.iter().any(|p| p.name == "legacyVar"),
        "`export var legacyVar` is a legacy prop"
    );
    assert!(
        !c.instance_exports.contains(&"name".to_string()),
        "`export let name` must NOT be an instance EXPOSE member, got {:?}",
        c.instance_exports
    );
    assert!(
        !c.instance_exports.contains(&"legacyVar".to_string()),
        "`export var legacyVar` must NOT be an instance EXPOSE member, got {:?}",
        c.instance_exports
    );
    // `export const` / `export function` ARE instance members.
    assert!(c.instance_exports.contains(&"ready".to_string()));
    assert!(c.instance_exports.contains(&"focus".to_string()));
}

#[test]
fn reexport_specifier_of_a_prop_local_is_a_prop_not_an_instance_export() {
    // `let local; export { local as leaked }` re-exports a PROP-kind local —
    // it is a re-exported prop, NOT an instance EXPOSE member. A `const`
    // re-export IS an instance member. DISCRIMINATING: an unconditional
    // specifier push put `leaked` into instance_exports.
    let c = capture(
        "let local = 1;\nconst stable = 2;\nexport { local as leaked, stable as exposed };",
    );
    assert!(
        c.legacy_props.iter().any(|p| p.name == "leaked"),
        "the re-exported prop-local `leaked` is a prop, got legacy_props={:?}",
        c.legacy_props
    );
    assert!(
        !c.instance_exports.contains(&"leaked".to_string()),
        "the re-exported prop-local `leaked` must NOT be an instance EXPOSE member, got {:?}",
        c.instance_exports
    );
    // A `const` re-export IS an instance member.
    assert!(
        c.instance_exports.contains(&"exposed".to_string()),
        "the re-exported const `exposed` IS an instance member, got {:?}",
        c.instance_exports
    );
    assert!(!c.legacy_props.iter().any(|p| p.name == "exposed"));
}

#[test]
fn module_region_let_does_not_misclassify_an_instance_const_reexport() {
    // A MODULE-script `let conf` must NOT cause an INSTANCE-script
    // `const conf; export { conf as exposed }` to be mis-routed to props. The
    // prop-local scan is INSTANCE-region scoped AND subtracts const names.
    // DISCRIMINATING: an unscoped scan would route `exposed` to legacy_props.
    let src = "let conf = 1;\nconst conf2 = 2;\nexport { conf2 as exposed };";
    // The module region covers ONLY the first line (`let conf`).
    let module_end = src.find('\n').unwrap() as u32;
    let c = capture_with_module_region(src, Some((0, module_end)));
    assert!(
        c.instance_exports.contains(&"exposed".to_string()),
        "an instance `const` re-export is an EXPOSE member, got instance={:?} props={:?}",
        c.instance_exports,
        c.legacy_props
    );
    assert!(
        !c.legacy_props.iter().any(|p| p.name == "exposed"),
        "the instance `const` re-export must NOT be a prop"
    );
}

#[test]
fn const_local_reexport_is_expose_even_with_same_name_let_absent() {
    // Subtraction rule: a `const x; export { x as y }` is EXPOSE (no `let x`
    // exists to mark it prop-kind).
    let c = capture("const x = 1;\nexport { x as y };");
    assert!(c.instance_exports.contains(&"y".to_string()));
    assert!(!c.legacy_props.iter().any(|p| p.name == "y"));
}

#[test]
fn module_const_does_not_subtract_an_instance_prop_let_reexport() {
    // A MODULE-region `const value` must NOT subtract an INSTANCE-region
    // `let value; export { value as propValue }` from props (the subtraction
    // is INSTANCE-region scoped). DISCRIMINATING: an all-region subtraction
    // dropped `value` from prop-locals and routed `propValue` to EXPOSE.
    let src = "const value = 1;\nlet value2 = 2;\nexport { value2 as propValue };";
    let module_end = src.find('\n').unwrap() as u32;
    let c = capture_with_module_region(src, Some((0, module_end)));
    assert!(
            c.legacy_props.iter().any(|p| p.name == "propValue"),
            "an instance prop-let re-export is a PROP even with a module const of a different name, got props={:?} instance={:?}",
            c.legacy_props,
            c.instance_exports
        );
    assert!(
        !c.instance_exports.contains(&"propValue".to_string()),
        "the instance prop-let re-export must NOT be EXPOSE"
    );
}

#[test]
fn captures_dispatcher_event_type_argument() {
    // The dispatcher factory MUST be imported (so its source is recordable
    // for provenance) — an untracked global `createEventDispatcher` is not a
    // capturable Svelte dispatcher.
    let c = capture(
            "import { createEventDispatcher } from 'svelte';\nconst dispatch = createEventDispatcher<{ change: number }>();",
        );
    assert!(c.dispatcher_events.is_some());
    assert_eq!(c.dispatcher_import_source.as_deref(), Some("svelte"));
}

#[test]
fn records_dispatcher_import_source_for_userland_lookalike() {
    // A `createEventDispatcher` imported from a userland module records its
    // source so resolved-validation can reject it (provenance, not name).
    let c = capture(
            "import { createEventDispatcher } from './fake-svelte';\nconst dispatch = createEventDispatcher<{ change: number }>();",
        );
    assert!(c.dispatcher_events.is_some());
    assert_eq!(c.dispatcher_import_source.as_deref(), Some("./fake-svelte"));
}

#[test]
fn untracked_global_dispatcher_is_not_captured() {
    // No import of `createEventDispatcher` ⇒ not a provenance-checkable Svelte
    // dispatcher ⇒ not captured (discriminating: the old name-only capture
    // would record it).
    let c = capture("const dispatch = createEventDispatcher<{ change: number }>();");
    assert!(c.dispatcher_events.is_none());
    assert!(c.dispatcher_import_source.is_none());
}

#[test]
fn validate_rejects_userland_snippet_lookalike() {
    // Resolved-validation: a Snippet candidate whose import resolves to a
    // userland file (NOT the svelte package) is rejected — discriminating: a
    // name match would accept it.
    let provider = SvelteScriptProvider;
    let candidates = SvelteScriptCandidates {
        snippet_candidates: vec![SvelteSnippetImportCandidate {
            local_binding: "Snippet".to_string(),
            import_source: "./fake-svelte".to_string(),
            member_name: "row".to_string(),
        }],
        ..Default::default()
    };
    let envelope = FrameworkScriptCandidates {
        adapter_id: FrameworkAdapterId::svelte(),
        provider_version: SvelteScriptProvider::VERSION,
        stable_hash: [0u8; 16],
        payload: Arc::new(candidates),
    };
    let targets = vec![super::super::ResolvedImportTarget {
        specifier: "./fake-svelte".to_string(),
        resolved_canonical: Some("/src/fake-svelte.ts".to_string()),
        // A userland relative import is workspace-owned, not package-backed ⇒
        // no typed package identity (the structural rejection signal).
        package: None,
    }];
    let cx = ResolvedValidationCx {
        candidates: &envelope,
        resolved_import_targets: &targets,
        capability_on: &|_| true,
    };
    assert!(
        provider.validate(cx).is_none(),
        "a userland Snippet look-alike must NOT validate as snippet-typed"
    );
}

#[test]
fn validate_accepts_real_svelte_snippet_import() {
    let provider = SvelteScriptProvider;
    let candidates = SvelteScriptCandidates {
        snippet_candidates: vec![SvelteSnippetImportCandidate {
            local_binding: "Snippet".to_string(),
            import_source: "svelte".to_string(),
            member_name: "row".to_string(),
        }],
        ..Default::default()
    };
    let envelope = FrameworkScriptCandidates {
        adapter_id: FrameworkAdapterId::svelte(),
        provider_version: SvelteScriptProvider::VERSION,
        stable_hash: [0u8; 16],
        payload: Arc::new(candidates),
    };
    let targets = vec![super::super::ResolvedImportTarget {
        specifier: "svelte".to_string(),
        resolved_canonical: Some("/project/node_modules/svelte/src/index.d.ts".to_string()),
        // The session classified the import as the `svelte` PACKAGE (the
        // typed identity the provider tests structurally).
        package: Some(super::super::ResolvedPackage::named("svelte")),
    }];
    let cx = ResolvedValidationCx {
        candidates: &envelope,
        resolved_import_targets: &targets,
        capability_on: &|_| true,
    };
    let facts = provider
        .validate(cx)
        .expect("real svelte snippet validates");
    let facts = facts
        .as_any()
        .downcast_ref::<SvelteScriptFacts>()
        .expect("svelte facts");
    assert_eq!(facts.validated_snippet_members, vec!["row".to_string()]);
}

#[test]
fn validate_emits_dispatcher_only_when_resolved_to_svelte_package() {
    // A real `svelte`-resolved `createEventDispatcher` contributes
    // `dispatcher_events`; a userland look-alike does NOT (provenance, not a
    // name match). DISCRIMINATING: a name-only test would accept both.
    let provider = SvelteScriptProvider;
    let make_candidates = |src: &str| SvelteScriptCandidates {
        dispatcher_events: Some(TypeExpr::Object(std::sync::Arc::new(
            verter_type_expr::ObjectExpr {
                properties: Vec::new(),
            },
        ))),
        dispatcher_import_source: Some(src.to_string()),
        ..Default::default()
    };
    let envelope = |c: SvelteScriptCandidates| FrameworkScriptCandidates {
        adapter_id: FrameworkAdapterId::svelte(),
        provider_version: SvelteScriptProvider::VERSION,
        stable_hash: [0u8; 16],
        payload: Arc::new(c),
    };

    // (1) Real svelte dispatcher ⇒ EMITS facts present.
    let real_env = envelope(make_candidates("svelte"));
    let real_targets = vec![super::super::ResolvedImportTarget {
        specifier: "svelte".to_string(),
        resolved_canonical: Some("/project/node_modules/svelte/index.d.ts".to_string()),
        package: Some(super::super::ResolvedPackage::named("svelte")),
    }];
    let real = provider
        .validate(ResolvedValidationCx {
            candidates: &real_env,
            resolved_import_targets: &real_targets,
            capability_on: &|_| true,
        })
        .expect("real svelte dispatcher validates");
    let real = real.as_any().downcast_ref::<SvelteScriptFacts>().unwrap();
    assert!(
        real.dispatcher_events.is_some(),
        "a svelte-resolved dispatcher contributes EMITS"
    );

    // (2) Userland look-alike ⇒ NO EMITS facts (and no other inventory ⇒ no
    // facts at all).
    let fake_env = envelope(make_candidates("./fake-svelte"));
    let fake_targets = vec![super::super::ResolvedImportTarget {
        specifier: "./fake-svelte".to_string(),
        resolved_canonical: Some("/src/fake-svelte.ts".to_string()),
        package: None,
    }];
    let fake = provider.validate(ResolvedValidationCx {
        candidates: &fake_env,
        resolved_import_targets: &fake_targets,
        capability_on: &|_| true,
    });
    assert!(
        fake.is_none(),
        "a userland createEventDispatcher look-alike must NOT contribute EMITS"
    );
}

#[test]
fn validate_passes_through_parse_domain_inventory() {
    // props_type / bindable / legacy / instance exports pass through verbatim
    // (no package provenance needed for those).
    let provider = SvelteScriptProvider;
    let candidates = SvelteScriptCandidates {
        props: Some(SveltePropsCandidate {
            props_type: Some(TypeExpr::Ref {
                name: Arc::from("Props"),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            }),
            bindable_members: vec!["value".to_string()],
            ..Default::default()
        }),
        instance_exports: vec!["focus".to_string()],
        ..Default::default()
    };
    let envelope = FrameworkScriptCandidates {
        adapter_id: FrameworkAdapterId::svelte(),
        provider_version: SvelteScriptProvider::VERSION,
        stable_hash: [0u8; 16],
        payload: Arc::new(candidates),
    };
    let facts = provider
        .validate(ResolvedValidationCx {
            candidates: &envelope,
            resolved_import_targets: &[],
            capability_on: &|_| true,
        })
        .expect("props/exports inventory validates");
    let facts = facts.as_any().downcast_ref::<SvelteScriptFacts>().unwrap();
    assert!(
        matches!(&facts.props_type, Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Props")
    );
    assert_eq!(facts.bindable_members, vec!["value".to_string()]);
    assert_eq!(facts.instance_exports, vec!["focus".to_string()]);
}
