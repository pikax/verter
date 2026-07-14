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

/// The macro-payload locator `capture` emits: anchored to the component's
/// `default` value symbol under the analyzer's local-file convention, at the
/// macro call's source-order ordinal and payload position.
fn macro_payload_locator(macro_index: u32, payload: MacroPayloadPosition) -> AuthoredBodyLocator {
    AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(""),
            symbol: Arc::from("default"),
            space: LocatorSymbolSpace::Value,
        },
        macro_index,
        payload,
    })
}

/// A fully-synthetic authored-type payload ref (fixed hash bytes) for
/// validate / hash-shape tests that construct candidates directly.
fn payload_ref(
    macro_index: u32,
    payload: MacroPayloadPosition,
    seed: u8,
) -> AuthoredTypePayloadRef {
    AuthoredTypePayloadRef {
        locator: macro_payload_locator(macro_index, payload),
        payload_hash: [seed; 16],
    }
}

#[test]
fn captures_props_generic_argument_type() {
    // A `$props<Props>()` generic argument captures an authored payload ref:
    // the content-free MacroPayload locator (the component's `default` value
    // anchor, call ordinal 0, TypeArgument position) plus a structural hash
    // of the authored type.
    let c = capture("let { name } = $props<Props>();");
    let props = c.props.expect("props candidate");
    assert!(props.from_generic_argument);
    let payload = props.props_type.as_ref().expect("props payload ref");
    assert_eq!(
        payload.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeArgument)
    );
}

#[test]
fn captures_props_destructuring_annotation_type() {
    // The destructuring annotation captures at the DISTINCT TypeAnnotation
    // payload position (a binding annotation, not a generic argument).
    let c = capture("let { name }: Props = $props();");
    let props = c.props.expect("props candidate");
    assert!(!props.from_generic_argument);
    let payload = props.props_type.as_ref().expect("props payload ref");
    assert_eq!(
        payload.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeAnnotation)
    );
}

#[test]
fn inline_and_instantiation_payloads_are_captured_not_dropped() {
    // An inline object-literal generic argument is a REAL authored payload —
    // captured as a payload ref (position + structural hash). The former
    // bare-`Ref`-only narrowing FAIL-CLOSED here and lost the payload.
    let c = capture("let { name } = $props<{ name: string }>();");
    let props = c.props.expect("props candidate");
    assert!(props.from_generic_argument);
    let inline = props.props_type.as_ref().expect("inline payload captured");
    assert_eq!(
        inline.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeArgument)
    );

    // An instantiation carrying type arguments captures at the annotation
    // position — the arguments ride in the hashed authored payload.
    let c = capture("let { name }: Props<string> = $props();");
    let props = c.props.expect("props candidate");
    let inst = props
        .props_type
        .as_ref()
        .expect("instantiation payload captured");
    assert_eq!(
        inst.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeAnnotation)
    );

    // An authored generic argument still WINS over the annotation (the
    // pre-payload-ref capture precedence, kept).
    let c = capture("let { name }: Named = $props<{ inline: string }>();");
    let props = c.props.expect("props candidate");
    assert!(props.from_generic_argument);
    let winner = props.props_type.as_ref().expect("generic-arg payload wins");
    assert_eq!(
        winner.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeArgument)
    );
}

#[test]
fn captures_props_from_eval_source_with_imports_and_exports() {
    // Mirror the synth-injection eval-source shape: leading blanks, an
    // import-type, the props destructuring, and an exported function.
    let src = "                  \n  import type { WidgetProps } from './props';\n  let { props }: WidgetProps = $props();\n  export function focus() {}\n";
    let c = capture(src);
    let props = c.props.expect("props candidate from eval-source");
    let payload = props
        .props_type
        .as_ref()
        .expect("the destructuring annotation must capture a props payload ref");
    assert_eq!(
        payload.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeAnnotation)
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
    // A destructuring default `size = 'md'` captures the RHS source
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
    // `value = $bindable(false)` captures the `$bindable` first arg
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
    // DISCRIMINATING: `value = $bindable()` (NO arg) is bindable but
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
    // DISCRIMINATING: editing a default VALUE changes the stable
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
    // capturable Svelte dispatcher. The type argument captures as an authored
    // payload ref at the call's TypeArgument position.
    let c = capture(
            "import { createEventDispatcher } from 'svelte';\ntype Events = { change: number };\nconst dispatch = createEventDispatcher<Events>();",
        );
    let payload = c
        .dispatcher_events
        .as_ref()
        .expect("dispatcher payload ref");
    assert_eq!(
        payload.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeArgument)
    );
    assert_eq!(c.dispatcher_import_source.as_deref(), Some("svelte"));
}

#[test]
fn inline_literal_dispatcher_payload_is_captured() {
    // An inline event-map literal is a REAL authored payload — captured as a
    // payload ref (the former bare-`Ref`-only narrowing dropped it; the
    // svelte vertical exercises exactly this `createEventDispatcher<{…}>()`
    // shape).
    let c = capture(
            "import { createEventDispatcher } from 'svelte';\nconst dispatch = createEventDispatcher<{ change: number }>();",
        );
    let payload = c
        .dispatcher_events
        .as_ref()
        .expect("inline dispatcher payload captured");
    assert_eq!(
        payload.locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeArgument)
    );
    assert_eq!(c.dispatcher_import_source.as_deref(), Some("svelte"));
}

#[test]
fn props_and_dispatcher_calls_draw_distinct_macro_ordinals() {
    // The payload locator is a re-resolution ADDRESS: two distinct authored
    // macro calls must never alias one ordinal, even across capture kinds —
    // `$props` and dispatcher captures draw from ONE shared source-order
    // counter.
    let c = capture(
            "import { createEventDispatcher } from 'svelte';\nlet { a } = $props<{ a: string }>();\nconst d = createEventDispatcher<{ save: string }>();",
        );
    let props_locator = c
        .props
        .as_ref()
        .and_then(|p| p.props_type.as_ref())
        .map(|r| r.locator.clone())
        .expect("props payload ref");
    let dispatcher_locator = c
        .dispatcher_events
        .as_ref()
        .map(|r| r.locator.clone())
        .expect("dispatcher payload ref");
    assert_eq!(
        props_locator,
        macro_payload_locator(0, MacroPayloadPosition::TypeArgument)
    );
    assert_eq!(
        dispatcher_locator,
        macro_payload_locator(1, MacroPayloadPosition::TypeArgument)
    );
    assert_ne!(props_locator, dispatcher_locator);
}

#[test]
fn distinct_authored_payload_types_produce_distinct_candidate_hashes() {
    // Cache-identity discrimination for AUTHORED PAYLOAD CONTENT: each pair
    // below carries the SAME locator shape and differs ONLY in the authored
    // type — under the former bare-`Ref`-only narrowing both sides captured
    // NOTHING and every pair COLLIDED onto one candidate hash.
    let pairs: &[(&str, &str)] = &[
        // (1) inline generic-argument object types.
        (
            "let { a } = $props<{ a: string }>();",
            "let { a } = $props<{ a: number }>();",
        ),
        // (2) destructuring-annotation forms.
        (
            "let { a }: { a: string } = $props();",
            "let { a }: { a: number } = $props();",
        ),
        // (3) instantiations carrying type arguments.
        (
            "let { a }: Props<string> = $props();",
            "let { a }: Props<number> = $props();",
        ),
        // (4) dispatcher inline event maps.
        (
            "import { createEventDispatcher } from 'svelte';\nconst d = createEventDispatcher<{ save: string }>();",
            "import { createEventDispatcher } from 'svelte';\nconst d = createEventDispatcher<{ save: number }>();",
        ),
    ];
    for (left, right) in pairs {
        assert_ne!(
            stable_candidate_hash(&capture(left)),
            stable_candidate_hash(&capture(right)),
            "distinct authored payloads must occupy distinct candidate slots: {left} vs {right}"
        );
    }
}

#[test]
fn distinct_payload_shapes_yield_distinct_payload_hashes() {
    // The payload hash discriminates every authored SHAPE class: bare refs by
    // name, refs vs inline objects, instantiations by their type arguments.
    let props_hash = |src: &str| {
        capture(src)
            .props
            .expect("props candidate")
            .props_type
            .expect("payload ref")
            .payload_hash
    };
    let bare_a = props_hash("let { a } = $props<PropsA>();");
    let bare_b = props_hash("let { a } = $props<PropsB>();");
    let inline = props_hash("let { a } = $props<{ a: string }>();");
    let inst_string = props_hash("let { a } = $props<PropsA<string>>();");
    let inst_number = props_hash("let { a } = $props<PropsA<number>>();");
    assert_ne!(bare_a, bare_b, "distinct bare refs discriminate");
    assert_ne!(bare_a, inline, "a ref and an inline object discriminate");
    assert_ne!(
        bare_a, inst_string,
        "a bare ref and its instantiation discriminate"
    );
    assert_ne!(
        inst_string, inst_number,
        "instantiation type arguments discriminate"
    );
    // Determinism: identical authored content re-captured hashes identically.
    assert_eq!(inline, props_hash("let { a } = $props<{ a: string }>();"));
}

#[test]
fn payload_hash_is_stable_across_formatting_only_edits() {
    // The payload hash is span-free (the shared alpha-normalised structural
    // fingerprint), so a formatting-only edit — shifted byte offsets, same
    // authored type — keeps BOTH the payload hash and the candidate hash
    // stable (the content-addressed slot's cosmetic invariance).
    let compact = capture("let { a } = $props<{ a: string }>();");
    let spaced = capture("\n\n   let   { a }   =   $props<{ a: string }>();");
    let hash_of = |c: &SvelteScriptCandidates| {
        c.props
            .as_ref()
            .unwrap()
            .props_type
            .as_ref()
            .unwrap()
            .payload_hash
    };
    assert_eq!(
        hash_of(&compact),
        hash_of(&spaced),
        "a formatting-only edit must not change the payload hash"
    );
    assert_eq!(
        stable_candidate_hash(&compact),
        stable_candidate_hash(&spaced),
        "a formatting-only edit must not change the candidate hash"
    );
}

#[test]
fn records_dispatcher_import_source_for_userland_lookalike() {
    // A `createEventDispatcher` imported from a userland module records its
    // source so resolved-validation can reject it (provenance, not name).
    let c = capture(
            "import { createEventDispatcher } from './fake-svelte';\nconst dispatch = createEventDispatcher<Events>();",
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
    assert_eq!(
        facts.validated_snippet_members.as_ref(),
        &["row".to_string()][..]
    );
}

#[test]
fn validate_emits_dispatcher_only_when_resolved_to_svelte_package() {
    // A real `svelte`-resolved `createEventDispatcher` contributes
    // `dispatcher_events`; a userland look-alike does NOT (provenance, not a
    // name match). DISCRIMINATING: a name-only test would accept both.
    let provider = SvelteScriptProvider;
    let make_candidates = |src: &str| SvelteScriptCandidates {
        dispatcher_events: Some(payload_ref(0, MacroPayloadPosition::TypeArgument, 0x2A)),
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
    assert_eq!(
        real.dispatcher_events,
        Some(payload_ref(0, MacroPayloadPosition::TypeArgument, 0x2A)),
        "a svelte-resolved dispatcher contributes EMITS (the payload ref passes through verbatim)"
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
            props_type: Some(payload_ref(0, MacroPayloadPosition::TypeAnnotation, 0x11)),
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
    assert_eq!(
        facts.props_type,
        Some(payload_ref(0, MacroPayloadPosition::TypeAnnotation, 0x11)),
        "the props payload ref passes through verbatim"
    );
    assert_eq!(facts.bindable_members.as_ref(), &["value".to_string()][..]);
    assert_eq!(facts.instance_exports.as_ref(), &["focus".to_string()][..]);
}

/// A fully-populated candidate set exercising EVERY stable-hash input.
fn full_candidates() -> SvelteScriptCandidates {
    SvelteScriptCandidates {
        props: Some(SveltePropsCandidate {
            call_span: Span::new(10, 30),
            props_type: Some(payload_ref(0, MacroPayloadPosition::TypeArgument, 0x11)),
            from_generic_argument: true,
            bindable_members: vec!["value".to_string()],
            prop_defaults: vec![AnalyzedDefaultValue {
                key: "size".to_string(),
                value: "'md'".to_string(),
                span: Span::new(12, 16),
            }],
            props_leaf_members: None,
        }),
        snippet_candidates: vec![SvelteSnippetImportCandidate {
            local_binding: "Snippet".to_string(),
            import_source: "svelte".to_string(),
            member_name: "row".to_string(),
        }],
        instance_exports: vec!["focus".to_string()],
        module_exports: vec!["meta".to_string()],
        legacy_props: vec![SvelteLegacyProp {
            name: "legacy".to_string(),
            has_default: true,
        }],
        dispatcher_events: Some(payload_ref(1, MacroPayloadPosition::TypeArgument, 0x22)),
        dispatcher_import_source: Some("svelte".to_string()),
    }
}

#[test]
fn stable_candidate_hash_discriminates_every_input() {
    // Cache-identity discrimination: EVERY semantic hash input, perturbed
    // independently, must change the stable candidate hash — an input silently
    // dropped from the hash fails its arm. (VERSION 4 keys are intentionally
    // distinct from legacy VERSION 3 keys — no legacy-byte compatibility.)
    let base_hash = stable_candidate_hash(&full_candidates());

    // (1) props payload CONTENT (the structural payload hash).
    let mut c = full_candidates();
    c.props
        .as_mut()
        .unwrap()
        .props_type
        .as_mut()
        .unwrap()
        .payload_hash = [0x99; 16];
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "the props payload hash must fold into the hash"
    );

    // (2) props payload POSITION (the locator's macro ordinal).
    let mut c = full_candidates();
    c.props.as_mut().unwrap().props_type =
        Some(payload_ref(7, MacroPayloadPosition::TypeArgument, 0x11));
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "the props payload locator must fold into the hash"
    );

    // (3) generic-argument origin flag.
    let mut c = full_candidates();
    c.props.as_mut().unwrap().from_generic_argument = false;
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "the generic-argument origin must fold into the hash"
    );

    // (4) bindable members.
    let mut c = full_candidates();
    c.props.as_mut().unwrap().bindable_members = vec!["other".to_string()];
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "bindable members must fold into the hash"
    );

    // (5) prop defaults (an edited default VALUE).
    let mut c = full_candidates();
    c.props.as_mut().unwrap().prop_defaults[0].value = "'lg'".to_string();
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "prop defaults must fold into the hash"
    );

    // (6) snippet members.
    let mut c = full_candidates();
    c.snippet_candidates[0].member_name = "cell".to_string();
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "snippet members must fold into the hash"
    );

    // (7) legacy-prop metadata (optionality flip).
    let mut c = full_candidates();
    c.legacy_props[0].has_default = false;
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "legacy-prop metadata must fold into the hash"
    );

    // (8) instance exports.
    let mut c = full_candidates();
    c.instance_exports = vec!["blur".to_string()];
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "instance exports must fold into the hash"
    );

    // (9) module exports.
    let mut c = full_candidates();
    c.module_exports = vec!["config".to_string()];
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "module exports must fold into the hash"
    );

    // (10) dispatcher payload CONTENT (the structural payload hash).
    let mut c = full_candidates();
    c.dispatcher_events.as_mut().unwrap().payload_hash = [0x99; 16];
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "the dispatcher payload hash must fold into the hash"
    );

    // (11) dispatcher payload POSITION (the locator's macro ordinal).
    let mut c = full_candidates();
    c.dispatcher_events = Some(payload_ref(9, MacroPayloadPosition::TypeArgument, 0x22));
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "the dispatcher payload locator must fold into the hash"
    );

    // (12) dispatcher import source.
    let mut c = full_candidates();
    c.dispatcher_import_source = Some("./fake-svelte".to_string());
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "the dispatcher import source must fold into the hash"
    );

    // (13) props presence.
    let mut c = full_candidates();
    c.props = None;
    assert_ne!(
        stable_candidate_hash(&c),
        base_hash,
        "props presence must fold into the hash"
    );

    // COSMETIC-INVARIANCE negative: spans are NOT hash inputs — the
    // content-addressed candidate slot stays stable across formatting-only
    // edits that merely shift byte offsets.
    let mut c = full_candidates();
    c.props.as_mut().unwrap().call_span = Span::new(999, 1024);
    c.props.as_mut().unwrap().prop_defaults[0].span = Span::new(500, 504);
    assert_eq!(
        stable_candidate_hash(&c),
        base_hash,
        "spans must NOT fold into the hash (cosmetic invariance)"
    );
}

#[test]
fn stable_candidate_hash_golden_is_deterministic() {
    // Deterministic golden for the VERSION 4 candidate hash: two independent
    // constructions hash identically, and the bytes are pinned so a silently
    // dropped / reordered hash input fails loudly. An INTENTIONAL hash-shape
    // change must bump `SvelteScriptProvider::VERSION` and re-pin.
    let a = stable_candidate_hash(&full_candidates());
    let b = stable_candidate_hash(&full_candidates());
    assert_eq!(a, b, "the candidate hash is deterministic");
    assert_eq!(
        a,
        [
            0x7f, 0x9b, 0xab, 0x9d, 0x8f, 0x39, 0x29, 0xf3, 0x52, 0xe6, 0xff, 0x36, 0x57, 0x3b,
            0x1f, 0x73
        ],
        "the VERSION 4 golden candidate hash"
    );
}

#[test]
fn svelte_carriers_are_no_type_expr() {
    // Compile-time witnesses: every persisted / candidate Svelte carrier owns
    // NO transitive `TypeExpr` (the transient lowered IR never escapes the
    // capture producer).
    use static_assertions::assert_impl_all;
    use verter_no_typeexpr::NoTypeExpr;
    assert_impl_all!(SveltePropsCandidate: NoTypeExpr);
    assert_impl_all!(SvelteSnippetImportCandidate: NoTypeExpr);
    assert_impl_all!(SvelteLegacyProp: NoTypeExpr);
    assert_impl_all!(SvelteScriptCandidates: NoTypeExpr);
    assert_impl_all!(SvelteScriptFacts: NoTypeExpr);
}

// ── Deref-side ordinal accessor (`lower_props_annotation_at`) ──

/// Parse `src` and run `f` over the program (the accessor takes a borrowed
/// OXC program, so the arena must outlive the call).
fn with_program<R>(src: &str, f: impl FnOnce(&Program<'_>) -> R) -> R {
    let alloc = Allocator::default();
    let program = Parser::new(&alloc, src, SourceType::ts()).parse().program;
    f(&program)
}

#[test]
fn deref_accessor_agrees_with_capture_stamped_ordinal() {
    // DISCRIMINATING mint↔deref agreement: the capture stamps the payload
    // locator's `macro_index` through the shared ordinal walk; the deref-side
    // accessor must re-derive the SAME position. A tracked dispatcher call
    // precedes the `$props()` call, so the props ordinal is 1 — a naive
    // `$props`-only re-walk would address ordinal 0 and desynchronize.
    let src = "\
import { createEventDispatcher } from 'svelte';\n\
const dispatch = createEventDispatcher<{ save: string }>();\n\
let { name }: { name: string } = $props();\n";
    let c = capture(src);
    let props_ref = c
        .props
        .as_ref()
        .and_then(|p| p.props_type.as_ref())
        .expect("annotation payload ref captured");
    let AuthoredBodyLocator::MacroPayload(locator) = &props_ref.locator else {
        panic!("capture emits a MacroPayload locator");
    };
    assert_eq!(
        locator.macro_index, 1,
        "the tracked dispatcher call consumes ordinal 0, `$props()` takes 1"
    );
    assert_eq!(locator.payload, MacroPayloadPosition::TypeAnnotation);

    with_program(src, |program| {
        // The stamped ordinal derefs to the authored annotation, and the
        // lowered payload fingerprints EXACTLY as the capture's payload_hash
        // (same walk + same lowering ⇒ the same authored type).
        match lower_props_annotation_at(program, src, None, locator.macro_index) {
            PropsAnnotationLowering::Annotation(lowered) => {
                let outcome = compute_semantic_hash(&lowered, SymbolSpace::Type, &UnresolvedLens);
                assert_eq!(
                    outcome.hash, props_ref.payload_hash,
                    "the deref-lowered annotation must fingerprint as the captured payload"
                );
            }
            other => panic!("expected the authored annotation, got {other:?}"),
        }
        // The dispatcher's ordinal is NOT a `$props()` position — a typed
        // shape miss, never the neighbouring annotation.
        assert!(
            matches!(
                lower_props_annotation_at(program, src, None, 0),
                PropsAnnotationLowering::NoPropsCall
            ),
            "ordinal 0 addresses the dispatcher call, not a $props declarator"
        );
    });
}

#[test]
fn deref_accessor_module_region_gate_matches_capture() {
    // An exported `$props()` declarator inside the MODULE-script region is
    // not a component macro: the capture assigns it no ordinal, and the
    // accessor's replayed walk must agree — the instance-block `$props()`
    // keeps ordinal 0 on both sides.
    let module_stmt = "export let { m }: { m: string } = $props();\n";
    let instance_stmt = "let { name }: { name: number } = $props();\n";
    let src = format!("{module_stmt}{instance_stmt}");
    let region = Some((0u32, module_stmt.len() as u32));
    let c = capture_with_module_region(&src, region);
    let props_ref = c
        .props
        .as_ref()
        .and_then(|p| p.props_type.as_ref())
        .expect("instance annotation payload ref captured");
    let AuthoredBodyLocator::MacroPayload(locator) = &props_ref.locator else {
        panic!("capture emits a MacroPayload locator");
    };
    assert_eq!(
        locator.macro_index, 0,
        "the module-block exported $props consumes no ordinal"
    );
    with_program(&src, |program| {
        match lower_props_annotation_at(program, &src, region, 0) {
            PropsAnnotationLowering::Annotation(lowered) => {
                let outcome = compute_semantic_hash(&lowered, SymbolSpace::Type, &UnresolvedLens);
                assert_eq!(
                    outcome.hash, props_ref.payload_hash,
                    "ordinal 0 must address the INSTANCE $props annotation"
                );
            }
            other => panic!("expected the instance annotation, got {other:?}"),
        }
    });
}

#[test]
fn deref_accessor_absent_positions_are_typed_misses() {
    // A `$props()` with no authored annotation is the typed `Unannotated`
    // absence; an ordinal past the macro list is the typed `NoPropsCall`
    // miss. Neither fabricates a body.
    let src = "let { a } = $props();\n";
    with_program(src, |program| {
        assert!(
            matches!(
                lower_props_annotation_at(program, src, None, 0),
                PropsAnnotationLowering::Unannotated
            ),
            "an unannotated $props call is a typed annotation absence"
        );
        assert!(
            matches!(
                lower_props_annotation_at(program, src, None, 7),
                PropsAnnotationLowering::NoPropsCall
            ),
            "an out-of-range ordinal is a typed position miss"
        );
    });
}

// ── Provider-owned candidate re-anchoring (`absolutize_candidates`) ──

/// Capture through the PROVIDER entry (the envelope carries the provider's
/// `stable_hash`), as the session's candidate-store path does.
fn provider_capture(src: &str) -> FrameworkScriptCandidates {
    let alloc = Allocator::default();
    let program = Parser::new(&alloc, src, SourceType::ts()).parse().program;
    SvelteScriptProvider
        .capture(ScriptCandidateCx {
            source: src,
            program: &program,
            module_script_region: None,
        })
        .expect("candidates captured")
}

fn envelope_payload(envelope: &FrameworkScriptCandidates) -> &SvelteScriptCandidates {
    envelope
        .payload
        .downcast_ref::<SvelteScriptCandidates>()
        .expect("the svelte provider owns the payload")
}

#[test]
fn absolutize_candidates_fills_empty_anchors_and_rehashes_coherently() {
    let src = "\
import { createEventDispatcher } from 'svelte';\n\
const dispatch = createEventDispatcher<{ save: string }>();\n\
let { name }: { name: string } = $props();\n";
    let captured = provider_capture(src);
    let captured_hash = captured.stable_hash;
    let captured_payload_hash = envelope_payload(&captured)
        .props
        .as_ref()
        .and_then(|p| p.props_type.as_ref())
        .expect("props payload ref")
        .payload_hash;

    let filled = SvelteScriptProvider.absolutize_candidates(captured, "/w/App.svelte");
    let payload = envelope_payload(&filled);
    let props_ref = payload
        .props
        .as_ref()
        .and_then(|p| p.props_type.as_ref())
        .expect("props payload ref survives");
    let AuthoredBodyLocator::MacroPayload(props_locator) = &props_ref.locator else {
        panic!("MacroPayload locator");
    };
    assert_eq!(
        props_locator.anchor.canonical_id.as_ref(),
        "/w/App.svelte",
        "the empty props anchor absolutizes to the producing canonical"
    );
    let dispatcher_ref = payload
        .dispatcher_events
        .as_ref()
        .expect("dispatcher payload ref survives");
    let AuthoredBodyLocator::MacroPayload(dispatcher_locator) = &dispatcher_ref.locator else {
        panic!("MacroPayload locator");
    };
    assert_eq!(
        dispatcher_locator.anchor.canonical_id.as_ref(),
        "/w/App.svelte",
        "the empty dispatcher anchor absolutizes to the producing canonical"
    );

    // Hash coherence: the envelope hash is REBUILT from the filled payload
    // (payload and hash never disagree), and it MOVED (the hash folds the
    // payload refs, anchors included).
    assert_eq!(
        filled.stable_hash,
        stable_candidate_hash(payload),
        "the rebuilt stable_hash matches its own payload"
    );
    assert_ne!(
        filled.stable_hash, captured_hash,
        "filling the anchor changes the folded candidate hash"
    );
    // The payload_hash axis fingerprints the authored TYPE, not the anchor —
    // untouched by the re-anchor.
    assert_eq!(
        props_ref.payload_hash, captured_payload_hash,
        "the authored-type payload hash is anchor-independent"
    );
}

#[test]
fn absolutize_candidates_never_rewrites_a_filled_anchor() {
    // Idempotent fill-only-empty: a second absolutization under a DIFFERENT
    // canonical leaves the filled anchors (and the coherent hash) untouched —
    // a non-empty anchor may be a cross-file resolver canonical and is never
    // rewritten.
    let src = "let { name }: { name: string } = $props();\n";
    let filled = SvelteScriptProvider.absolutize_candidates(provider_capture(src), "/w/App.svelte");
    let refilled = SvelteScriptProvider.absolutize_candidates(filled.clone(), "/w/Other.svelte");
    assert_eq!(
        envelope_payload(&refilled)
            .props
            .as_ref()
            .and_then(|p| p.props_type.as_ref())
            .map(|r| &r.locator),
        envelope_payload(&filled)
            .props
            .as_ref()
            .and_then(|p| p.props_type.as_ref())
            .map(|r| &r.locator),
        "a filled anchor is never rewritten"
    );
    assert_eq!(
        refilled.stable_hash, filled.stable_hash,
        "the no-op pass keeps the envelope hash"
    );
}

#[test]
fn absolutize_candidates_keeps_the_sentinel_for_an_empty_canonical() {
    // With no producing identity there is nothing to absolutize to — the
    // envelope passes through with the sentinel (and its original hash)
    // intact rather than stamping another empty anchor.
    let src = "let { name }: { name: string } = $props();\n";
    let captured = provider_capture(src);
    let captured_hash = captured.stable_hash;
    let kept = SvelteScriptProvider.absolutize_candidates(captured, "");
    assert!(
        envelope_payload(&kept)
            .props
            .as_ref()
            .and_then(|p| p.props_type.as_ref())
            .is_some_and(|r| {
                let AuthoredBodyLocator::MacroPayload(locator) = &r.locator else {
                    return false;
                };
                locator.anchor.canonical_id.is_empty()
            }),
        "the empty-canonical pass keeps the sentinel anchor"
    );
    assert_eq!(kept.stable_hash, captured_hash, "and the original hash");
}
