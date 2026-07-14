// The honest KNOWN_DIVERGENCES allow-list DATA — the enumerated long tail of
// generated-corpus differential divergences that REMAIN as legitimate
// feature-emission deferrals. Every IR-CONTRACT defect (where the IR carried a
// WRONG fact a backend could not recover) has been FIXED and removed from this
// list; what REMAINS are emission-LAYER deferrals — the IR carries enough facts,
// and the owning runtime emission LAYER emits the concrete shape:
//
//   - `5a` class-style-merge / option-value layer — the class:/style: directive
//     merge with the class/style attribute into one set_class/set_style attr-part
//     (the IR-topology coalesce); <option>/<datalist> value property-vs-attribute.
//     (the client backend emits dynamic attrs + class/style + boolean
//     DOM props; the remaining 5a rows are the IR slot/attr-part TOPOLOGY coalesce —
//     the client emitter already produces the correct merged $.set_class/$.set_style.)
//   - `5c` form-control setter layer — a `value`/`checked` dynamic attribute's typed
//     form-control setter ($.set_value / $.set_checked / $.set_selected /
//     $.set_default_*) slot, alongside bind:value/bind:checked. (Split out of the
//     former 5a typed-setter layer: the value sub-case is a form-control / bindings
//     surface, not a 5a class/style surface.)
//   - `5f` special-element / region / slot emission layer — the <svelte:element>
//     $.element wrapper topology; the window/body/document/head region
//     comment-anchor SHAPE; the standalone <Component>/{@render} slot text mount.
//
// Every row is a REAL divergence proven by `known_divergences_are_real` (a stale
// row that no longer diverges fails that guard). This file is DERIVED from the
// discovery pass (`enumerate_divergences_discovery`, run with --ignored) over the
// pinned svelte@5.56.3 compiler — re-run the discovery pass + the data emitter when
// the IR projection or corpus changes.
//
// Included by `diff_oracle_tests.rs` via `include!`.

#[rustfmt::skip]
const KNOWN_DIVERGENCES_DATA: [DivergenceRow; 97] = [
    // (`generated/001_root_component.svelte` — a standalone root `<Component>` — converged
    // with the component vertical: its `HelperSet` + `DecodedText` rows were removed.)
    // The `<svelte:element>` CLIENT emission converged in 5f-b (the comment-anchor +
    // `$.element` wrapper + `$.attribute_effect` fold): its helper_set / static_html / factory
    // / decoded_text divergence rows were removed as stale. The SSR-side dynamic-surface plan
    // for the dynamic element (the `<!>` node-path + the attribute/spread dynamic-slot
    // representation) is NOT yet converged — it is owned by the unimplemented SSR backend.
    DivergenceRow {
        fixture: "generated/002_root_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the <svelte:element this={...}> SSR node-path plan (the client `$.element` emission converged in 5f-b; the SSR dynamic-element comment-anchor node-path is the unimplemented SSR backend's concern)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    // 003/004/005 (`<svelte:window|body|document>` roots) + 006 (`<svelte:head>` root) CONVERGED
    // in 5f-b: the no-DOM host root is now an init-only region (a global host emits no
    // `$.comment` / `$.append`; a head-only root emits `$.head(...)` with no root comment/append),
    // matching official's folded region. (Their KNOWN_DIVERGENCES rows were removed as stale.)
    DivergenceRow {
        fixture: "generated/032_attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/034_attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/035_attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/041_dir_class_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/042_dir_class_quoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/043_dir_class_shorthand.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/044_dir_style_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/045_dir_style_quoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/046_dir_style_important.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"expr\"] }] != Verter [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/046_dir_style_important.svelte",
        axis: DiffAxis::DirectiveExprs,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official directive-exprs [] != Verter [CandDirectiveExpr { kind: \"style\", shape: \"expr\" }]",
    },
    // 060 (`<svelte:element onclick={ev}>`): the CLIENT emission converged in 5f-b (the
    // onclick folds into `$.attribute_effect`); its helper_set / static_html / factory /
    // decoded_text rows were removed as stale. The SSR dynamic-surface plan still represents
    // the dynamic element's attribute as an attribute slot (vs official's spread) + lacks the
    // comment-anchor node-path — the unimplemented SSR backend's concern.
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR attr-part plan (client `$.attribute_effect` fold converged in 5f-b; the SSR dynamic-attribute representation is the unimplemented SSR backend's concern)",
        summary: "official attr-parts [] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"onclick\", chunks: [\"expr\"] }]",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR node-path plan (the unimplemented SSR backend's concern)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR dynamic-slot plan (attribute vs official spread; the unimplemented SSR backend's concern)",
        summary: "official dynamic-slots {\"spread\": 1} != Verter {\"attribute\": 1}",
    },
    // 061 (`<svelte:element onfocus={ev}>`): client emission converged in 5f-b; the SSR
    // dynamic-surface rows remain (the unimplemented SSR backend's concern).
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR attr-part plan (client `$.attribute_effect` fold converged in 5f-b; the SSR dynamic-attribute representation is the unimplemented SSR backend's concern)",
        summary: "official attr-parts [] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"onfocus\", chunks: [\"expr\"] }]",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR node-path plan (the unimplemented SSR backend's concern)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR dynamic-slot plan (attribute vs official spread; the unimplemented SSR backend's concern)",
        summary: "official dynamic-slots {\"spread\": 1} != Verter {\"attribute\": 1}",
    },
    // 062 (`<svelte:element onclickcapture={ev}>`): client emission converged in 5f-b; the
    // SSR dynamic-surface rows remain (the unimplemented SSR backend's concern).
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR attr-part plan (client `$.attribute_effect` fold converged in 5f-b; the SSR dynamic-attribute representation is the unimplemented SSR backend's concern)",
        summary: "official attr-parts [] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"onclickcapture\", chunks: [\"expr\"] }]",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR node-path plan (the unimplemented SSR backend's concern)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y12-ssr -> SSR dynamic-element surface: the dynamic element's SSR dynamic-slot plan (attribute vs official spread; the unimplemented SSR backend's concern)",
        summary: "official dynamic-slots {\"spread\": 1} != Verter {\"attribute\": 1}",
    },
    // 063/064/065 (`<svelte:window|body|document on*>` event roots) CONVERGED in 5f-b: the
    // global-host event root is now a no-DOM init-only region emitting only `$.event` (no
    // `$.comment` / `$.append`), matching official. (Their KNOWN_DIVERGENCES rows were removed
    // as stale.)
    DivergenceRow {
        fixture: "generated/068_pair_root_element__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/070_pair_root_element__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/071_pair_root_element__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/075_pair_root_svg__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/077_pair_root_svg__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/078_pair_root_svg__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/082_pair_root_mathml__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/084_pair_root_mathml__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/085_pair_root_mathml__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/089_pair_root_if_block__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"block\": 1, \"class\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/091_pair_root_if_block__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"block\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/092_pair_root_if_block__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"block\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/096_pair_root_each_block__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"block\": 1, \"class\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/098_pair_root_each_block__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"block\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/099_pair_root_each_block__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"block\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/103_pair_root_key_block__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"block\": 1, \"class\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/105_pair_root_key_block__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"block\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/106_pair_root_key_block__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"block\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"block\": 1}",
    },
    DivergenceRow {
        fixture: "generated/109_pair_text_dynamic__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1, \"text\": 1} != Verter {\"attribute\": 1, \"text\": 1}",
    },
    DivergenceRow {
        fixture: "generated/110_pair_text_dynamic__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"text\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"text\": 1}",
    },
    DivergenceRow {
        fixture: "generated/112_pair_text_mixed__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1, \"text\": 1} != Verter {\"attribute\": 1, \"text\": 1}",
    },
    DivergenceRow {
        fixture: "generated/113_pair_text_mixed__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"text\": 1, \"value\": 1} != Verter {\"attribute\": 1, \"text\": 1}",
    },
    DivergenceRow {
        fixture: "generated/115_pair_text_named_entity__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/116_pair_text_named_entity__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/117_pair_attr_static__dir_class_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"expr\", \"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }]",
    },
    // NOTE: the 117 `StaticHtml` divergence (a static `class="box"` baked into the
    // skeleton when a `class:` directive is present) is FIXED — the static
    // base is now pulled OUT of the `from_html` skeleton into the merged `$.set_class`
    // base arg, matching official. The row is removed (no longer divergent).
    DivergenceRow {
        fixture: "generated/118_pair_attr_static__dir_style_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/120_pair_attr_dynamic__dir_class_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_attribute\", attr: \"id\", chunks: [\"expr\"] }, CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"id\", chunks: [\"expr\"] }, CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/121_pair_attr_dynamic__dir_style_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_attribute\", attr: \"id\", chunks: [\"expr\"] }, CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"id\", chunks: [\"expr\"] }, CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/123_pair_attr_mixed__dir_class_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"literal\", \"expr\", \"literal\", \"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }, CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"literal\", \"expr\", \"literal\"] }]",
    },
    DivergenceRow {
        fixture: "generated/123_pair_attr_mixed__dir_class_unquoted.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1, \"class\": 1}",
    },
    DivergenceRow {
        fixture: "generated/124_pair_attr_mixed__dir_style_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"literal\", \"expr\", \"literal\"] }, CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"literal\", \"expr\", \"literal\"] }, CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/124_pair_attr_mixed__dir_style_unquoted.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1, \"style\": 1} != Verter {\"attribute\": 1, \"style\": 1}",
    },
    DivergenceRow {
        fixture: "generated/125_pair_attr_mixed__dir_bind_value.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"bind\": 1, \"class\": 1} != Verter {\"attribute\": 1, \"bind\": 1}",
    },
    DivergenceRow {
        fixture: "generated/138_pair_text_dynamic__dir_class_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/139_pair_text_dynamic__dir_style_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/140_pair_text_mixed__dir_class_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_class\", attr: \"class\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/141_pair_text_mixed__dir_style_unquoted.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y14 -> 5a (class/style-merge layer): a class:/style: directive is merged with the element's class/style attribute into ONE setter (and the style:|important array-wrapped value shape) — the 5a class/style-merge layer owns it",
        summary: "official attr-parts [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"expr\", \"directive\"] }] != Verter [CandAttrPart { helper: \"set_style\", attr: \"style\", chunks: [\"directive\"] }]",
    },
    DivergenceRow {
        fixture: "generated/145_ws_datalist.svelte",
        axis: DiffAxis::StaticHtml,
        root_cause: "Y17 -> 5a (option/datalist-value layer): an <option>/<datalist> value is a non-static DOM property (option.value), not a plain static attribute — the 5a option/datalist-value layer owns it",
        summary: "official static-html [\"<datalist><option></option></datalist>\"] != Verter [\"<datalist><option value=\\\"a\\\"></option></datalist>\"]",
    },
    DivergenceRow {
        fixture: "generated/145_ws_datalist.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y17 -> 5a (option/datalist-value layer): an <option>/<datalist> value is a non-static DOM property (option.value), not a plain static attribute — the 5a option/datalist-value layer owns it",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"child\"] }]] != Verter []",
    },
    // ── svg / mathml root element emission (CATEGORY-4 POST-RELEASE deferral) ──
    // This differential oracle is a PLAN-LEVEL IR projection: it reads
    // `plan_static_templates` (the static-template plan), NOT the emitted client module.
    // At the plan level an svg/mathml ROOT projects as an html-namespaced `$.from_html`
    // clone where official emits the `$.from_svg` / `$.from_mathml` root helper, so each
    // such root diverges on BOTH the clone-template factory family (`from_html` vs
    // `from_svg`/`from_mathml`) and the owned-helper set (Verter's PLAN carries
    // `from_html`; the official `from_svg`/`from_mathml` is outside the owned-helper
    // universe). The REAL `compile_client` compile FAILS CLOSED on an svg/mathml root (a
    // non-`html` namespace is refused at the resolver and svg/mathml elements fail closed
    // — see the namespace fail-close tests): no `$.from_html` is EVER emitted for these
    // roots. This row tracks ONLY the plan-projection divergence for the deferred
    // svg/mathml element-emission surface — see the svelte-native-compiler-plan Decisions
    // Log svg/mathml element-emission D-row.
    DivergenceRow {
        fixture: "generated/007_root_svg.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/007_root_svg.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/008_root_mathml.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/008_root_mathml.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/073_pair_root_svg__attr_static.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/073_pair_root_svg__attr_static.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/074_pair_root_svg__attr_dynamic.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/074_pair_root_svg__attr_dynamic.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/075_pair_root_svg__attr_mixed.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/075_pair_root_svg__attr_mixed.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/076_pair_root_svg__attr_boolean.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/076_pair_root_svg__attr_boolean.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/077_pair_root_svg__attr_shorthand.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/077_pair_root_svg__attr_shorthand.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/078_pair_root_svg__attr_shorthand_spaced.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/078_pair_root_svg__attr_shorthand_spaced.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/079_pair_root_svg__attr_spread.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\", \"attribute_effect\"] != Verter [\"append\", \"attribute_effect\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/079_pair_root_svg__attr_spread.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/080_pair_root_mathml__attr_static.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/080_pair_root_mathml__attr_static.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/081_pair_root_mathml__attr_dynamic.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/081_pair_root_mathml__attr_dynamic.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/082_pair_root_mathml__attr_mixed.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/082_pair_root_mathml__attr_mixed.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/083_pair_root_mathml__attr_boolean.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/083_pair_root_mathml__attr_boolean.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/084_pair_root_mathml__attr_shorthand.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/084_pair_root_mathml__attr_shorthand.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/085_pair_root_mathml__attr_shorthand_spaced.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/085_pair_root_mathml__attr_shorthand_spaced.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/086_pair_root_mathml__attr_spread.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\", \"attribute_effect\"] != Verter [\"append\", \"attribute_effect\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/086_pair_root_mathml__attr_spread.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/149_ws_svg_interior.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/149_ws_svg_interior.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/150_ws_svg_title.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/150_ws_svg_title.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/151_ws_svg_anchor.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): Verter's PLAN-projected owned helper set carries `from_html` where official emits the out-of-universe $.from_svg / $.from_mathml root helper (the real compile_client compile fails closed on an svg/mathml root; the deferred svg/mathml element-emission surface)",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/151_ws_svg_anchor.svelte",
        axis: DiffAxis::Factory,
        root_cause: "svg/mathml element emission (CATEGORY-4 post-release deferral): at the PLAN level (this oracle reads plan_static_templates, not the emitted module) an svg/mathml root PROJECTS as an html-namespaced $.from_html clone where official emits the $.from_svg / $.from_mathml root helper — the REAL compile_client compile fails closed on an svg/mathml root (see the namespace fail-close tests), so no $.from_html is emitted; a separate deferred element-emission surface",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
];
