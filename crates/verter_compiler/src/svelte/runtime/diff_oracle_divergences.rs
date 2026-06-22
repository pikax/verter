// The honest KNOWN_DIVERGENCES allow-list DATA — the enumerated long tail of
// generated-corpus differential divergences that REMAIN as legitimate
// feature-emission deferrals. Every IR-CONTRACT defect (where the IR carried a
// WRONG fact a backend could not recover) has been FIXED and removed from this
// list; what REMAINS are emission-LAYER deferrals — the IR carries enough facts,
// and the owning runtime emission LAYER emits the concrete shape:
//
//   - `5m` namespace-root-helper layer — namespace-aware root-helper selection
//     ($.from_svg / $.from_mathml for an SVG/MathML root).
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
const KNOWN_DIVERGENCES_DATA: [DivergenceRow; 122] = [
    DivergenceRow {
        fixture: "generated/001_root_component.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [\"append\"] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/001_root_component.svelte",
        axis: DiffAxis::DecodedText,
        root_cause: "Y19 -> 5f (standalone-slot-mount layer): a standalone <Component>/{@render} slot text node is mounted as a $.text seed by the 5f standalone-slot-mount layer (the IR text value is entity-decoded, so the seed is correct once 5f emits it)",
        summary: "official decoded-text [\"hello\"] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/002_root_svelte_element.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official owned-helper-set [\"append\", \"comment\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/002_root_svelte_element.svelte",
        axis: DiffAxis::StaticHtml,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official static-html [] != Verter [\"<!>\"]",
    },
    DivergenceRow {
        fixture: "generated/002_root_svelte_element.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official factory-kinds [] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/002_root_svelte_element.svelte",
        axis: DiffAxis::DecodedText,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official decoded-text [\"hello\"] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/002_root_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/003_root_svelte_window.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [] != Verter [\"append\", \"comment\"]",
    },
    DivergenceRow {
        fixture: "generated/004_root_svelte_body.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [] != Verter [\"append\", \"comment\"]",
    },
    DivergenceRow {
        fixture: "generated/005_root_svelte_document.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [] != Verter [\"append\", \"comment\"]",
    },
    DivergenceRow {
        fixture: "generated/006_root_svelte_head.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [\"head\"] != Verter [\"append\", \"comment\", \"head\"]",
    },
    DivergenceRow {
        fixture: "generated/007_root_svg.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/007_root_svg.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/008_root_mathml.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/008_root_mathml.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
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
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official attr-parts [] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"onclick\", chunks: [\"expr\"] }]",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official owned-helper-set [\"append\", \"attribute_effect\", \"comment\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::StaticHtml,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official static-html [] != Verter [\"<!>\"]",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official factory-kinds [] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::DecodedText,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official decoded-text [\"x\"] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/060_event_delegated_click_on_svelte_element.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official dynamic-slots {\"spread\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official attr-parts [] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"onfocus\", chunks: [\"expr\"] }]",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official owned-helper-set [\"append\", \"attribute_effect\", \"comment\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::StaticHtml,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official static-html [] != Verter [\"<!>\"]",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official factory-kinds [] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::DecodedText,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official decoded-text [\"x\"] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/061_event_nondelegated_focus_on_svelte_element.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official dynamic-slots {\"spread\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::AttrParts,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official attr-parts [] != Verter [CandAttrPart { helper: \"set_attribute\", attr: \"onclickcapture\", chunks: [\"expr\"] }]",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official owned-helper-set [\"append\", \"attribute_effect\", \"comment\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::StaticHtml,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official static-html [] != Verter [\"<!>\"]",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official factory-kinds [] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::DecodedText,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official decoded-text [\"x\"] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::NodePaths,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official node-paths [[CandNodePath { base: \"fragment\", steps: [\"first_child\"] }]] != Verter []",
    },
    DivergenceRow {
        fixture: "generated/062_event_capture_click_on_svelte_element.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y12-emission -> 5f (dynamic-element-emission layer): the <svelte:element this={...}> $.element wrapper topology (the comment-anchor, child mount, and $.attribute_effect spread vs Verter's <!> from_html clone) — the 5f dynamic-element-emission layer owns it (the this dynamic-tag IR fact is modeled)",
        summary: "official dynamic-slots {\"spread\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/063_event_window_resize.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [\"event\"] != Verter [\"append\", \"comment\", \"event\"]",
    },
    DivergenceRow {
        fixture: "generated/064_event_body_click.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [\"event\"] != Verter [\"append\", \"comment\", \"event\"]",
    },
    DivergenceRow {
        fixture: "generated/065_event_document_click.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y18-region -> 5f (special-element-region layer): a <svelte:window>/body/document/head region comment-anchor SHAPE (Verter plans an append + comment anchor where official folds the global region) — the 5f special-element-region layer owns the region shape (the delegation is fixed)",
        summary: "official owned-helper-set [\"event\"] != Verter [\"append\", \"comment\", \"event\"]",
    },
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
        fixture: "generated/073_pair_root_svg__attr_static.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/073_pair_root_svg__attr_static.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/074_pair_root_svg__attr_dynamic.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/074_pair_root_svg__attr_dynamic.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/075_pair_root_svg__attr_mixed.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/075_pair_root_svg__attr_mixed.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/075_pair_root_svg__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/076_pair_root_svg__attr_boolean.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/076_pair_root_svg__attr_boolean.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/077_pair_root_svg__attr_shorthand.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/077_pair_root_svg__attr_shorthand.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/077_pair_root_svg__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/078_pair_root_svg__attr_shorthand_spaced.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/078_pair_root_svg__attr_shorthand_spaced.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/078_pair_root_svg__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/079_pair_root_svg__attr_spread.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\", \"attribute_effect\"] != Verter [\"append\", \"attribute_effect\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/079_pair_root_svg__attr_spread.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/080_pair_root_mathml__attr_static.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/080_pair_root_mathml__attr_static.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/081_pair_root_mathml__attr_dynamic.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/081_pair_root_mathml__attr_dynamic.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/082_pair_root_mathml__attr_mixed.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/082_pair_root_mathml__attr_mixed.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/082_pair_root_mathml__attr_mixed.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13 -> 5a (typed-setter-slot layer): a class/style/value dynamic attribute is realized by a TYPED setter ($.set_class / $.set_style / $.set_value) routed as a typed slot — the 5a typed-setter layer owns it",
        summary: "official dynamic-slots {\"class\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/083_pair_root_mathml__attr_boolean.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/083_pair_root_mathml__attr_boolean.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/084_pair_root_mathml__attr_shorthand.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/084_pair_root_mathml__attr_shorthand.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/084_pair_root_mathml__attr_shorthand.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/085_pair_root_mathml__attr_shorthand_spaced.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/085_pair_root_mathml__attr_shorthand_spaced.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/085_pair_root_mathml__attr_shorthand_spaced.svelte",
        axis: DiffAxis::DynamicSlots,
        root_cause: "Y13-value -> 5c (form-control setter layer): a `value`/`checked` dynamic attribute is realized by a TYPED form-control setter ($.set_value / $.set_checked / $.set_selected / $.set_default_*) — the 5c form-control/bindings layer owns it (the value sub-case was split out of the 5a typed-setter layer; class/style stay 5a)",
        summary: "official dynamic-slots {\"value\": 1} != Verter {\"attribute\": 1}",
    },
    DivergenceRow {
        fixture: "generated/086_pair_root_mathml__attr_spread.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\", \"attribute_effect\"] != Verter [\"append\", \"attribute_effect\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/086_pair_root_mathml__attr_spread.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_mathml\"] != Verter [\"from_html\"]",
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
    DivergenceRow {
        fixture: "generated/149_ws_svg_interior.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/149_ws_svg_interior.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/150_ws_svg_title.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/150_ws_svg_title.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/151_ws_svg_anchor.svelte",
        axis: DiffAxis::HelperSet,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official owned-helper-set [\"append\"] != Verter [\"append\", \"from_html\"]",
    },
    DivergenceRow {
        fixture: "generated/151_ws_svg_anchor.svelte",
        axis: DiffAxis::Factory,
        root_cause: "Y9 -> 5m (namespace-root-helper layer): an SVG/MathML root must clone via $.from_svg / $.from_mathml, not $.from_html — the 5m namespace-aware root-helper layer owns the factory-family selection",
        summary: "official factory-kinds [\"from_svg\"] != Verter [\"from_html\"]",
    },
];
