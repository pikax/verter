//! Static-template serialization: turning a cleaned item sequence into the static
//! representation a template factory clones.
//!
//! Two representations share ONE cleaned-item sequence + ONE static-attribute
//! selection ([`collect_static_attrs`]), so they can never bake a divergent
//! element / attribute / child set:
//!
//! - [`serialize_clean_items`] — the HTML-string skeleton (the `$.from_html` backtick
//!   argument): the official `stringify` output.
//! - [`objectify_region`] — the `fragments: 'tree'` `$.from_tree` array literal: the
//!   `svelte@5.56.3` `Template.as_tree` / `objectify` mirror (a JS array literal
//!   instead of a backtick string; a rendered `<!>` anchor is a sparse-array hole,
//!   a text child decodes to `node.data`, esrap's non-multiline array/object bytes).

use super::css::types::CssScopeFacts;
use super::entity_decode::{decode_text_entities, escape_html_attr_context};
use super::html::{cannot_be_set_statically, is_custom_element, is_void_element};
use super::ir::{AttrIr, IrNode, NodeId, StaticAttrValue, SvelteRuntimeIr};
use super::whitespace::{clean_nodes, CleanContext, CleanItem};

/// Serialize a CLEANED node sequence into the static-HTML skeleton, with NO
/// inter-node separator (the official skeleton concatenates directly). A
/// `TextRun` emits its skeleton text (raw cleaned text, or a single ` `
/// placeholder for a dynamic run); a `Node` is serialized in place (an element
/// recurses; any other rendered node — component / block / `{@html}` / renderable
/// special / comment — is a `<!>` hydration anchor). `ctx` is the cleaning context
/// the items were produced under (the namespace / parent / whitespace state).
pub(super) fn serialize_clean_items(
    ir: &SvelteRuntimeIr,
    items: &[CleanItem],
    ctx: CleanContext,
    css: Option<&CssScopeFacts>,
    html: &mut String,
) {
    for item in items {
        match item {
            CleanItem::TextRun { text, .. } => html.push_str(text),
            CleanItem::Node(id) => match ir.node(*id) {
                IrNode::Element(el) => serialize_element(ir, el, *id, ctx, css, html),
                // A RETAINED comment (only present under `preserveComments`) serializes
                // as `<!--data-->` (a bare `<!>` for an empty `<!---->`) — the official
                // `stringify` comment arm. The IR `text` is the FULL raw comment
                // (delimiters included), so a non-empty comment is emitted verbatim.
                IrNode::Comment { text, .. } => {
                    if comment_inner_data(text).is_empty() {
                        html.push_str("<!>");
                    } else {
                        html.push_str(text);
                    }
                }
                // A component / renderable special / block / tag at a rendered position
                // is a `<!>` anchor the DOM walk resolves.
                _ => html.push_str("<!>"),
            },
        }
    }
}

/// Serialize an element and its children into the skeleton. `ctx` is the cleaning
/// context the element sits in; its children are cleaned/serialized under the
/// CHILD context ([`CleanContext::for_children_of`]) — the namespace propagates,
/// the whitespace-preservation flag stays set once a `<pre>` / `<textarea>`
/// ancestor turned it on, and the SVG-`<text>` flag tracks for `can_remove_entirely`.
fn serialize_element(
    ir: &SvelteRuntimeIr,
    el: &crate::svelte::runtime::ir::ElementIr,
    node: NodeId,
    ctx: CleanContext,
    css: Option<&CssScopeFacts>,
    html: &mut String,
) {
    html.push('<');
    html.push_str(&el.tag);
    // A CUSTOM element sets ALL its attributes via PROPERTIES at runtime, so its
    // static attributes are NOT serialized into the skeleton — EXCEPT the `is`
    // attribute, which must stay in the cloned HTML for the customized-built-in
    // upgrade (official: `is_static_element` is false for a custom element, and the
    // `is` attribute is the one exception kept in the template). A plain element
    // (including `<video>`, which needs `importNode` but is NOT a custom element)
    // serializes its static attributes normally.
    //
    // The SCOPE HASH for THIS element — `Some` iff the selector-to-template
    // matcher marked this NodeId scoped (the shared per-element read of the ONE
    // scope-injection fact pair).
    let scope_hash = css.and_then(|facts| facts.hash_for(node));
    serialize_static_attrs(&el.attrs, is_custom_element(el), scope_hash, html);
    if el.children.is_empty() && is_void_element(&el.tag) {
        html.push_str("/>");
        return;
    }
    html.push('>');
    serialize_element_children(ir, &el.children, ctx.for_children_of(&el.tag), css, html);
    html.push_str("</");
    html.push_str(&el.tag);
    html.push('>');
}

/// Serialize an element's children into the skeleton, via the shared
/// [`clean_nodes`] partition (the same one a template region's roots use). `ctx`
/// is the CHILD cleaning context (namespace / parent tag / whitespace state).
///
/// The official `is_controlled` optimization (`process_children`): an `{#each}`
/// block or an `{@html}` tag that is the SOLE cleaned child of an element is
/// CONTROLLED — the runtime walks it without a `<!>` hydration anchor, so the
/// element body stays empty in the skeleton. Only `EachBlock` / `HtmlTag` qualify.
fn serialize_element_children(
    ir: &SvelteRuntimeIr,
    children: &[NodeId],
    ctx: CleanContext,
    css: Option<&CssScopeFacts>,
    html: &mut String,
) {
    let items = clean_nodes(ir, children, ctx);
    if is_sole_controlled(ir, &items) {
        return;
    }
    serialize_clean_items(ir, &items, ctx, css, html);
}

/// Whether a cleaned child sequence is EXACTLY one controlled `{#each}` / `{@html}`
/// node (the official `is_controlled` case): the runtime mounts it through its own
/// anchor with no `<!>` marker, so the element body is empty in the skeleton.
fn is_sole_controlled(ir: &SvelteRuntimeIr, items: &[CleanItem]) -> bool {
    let [CleanItem::Node(only)] = items else {
        return false;
    };
    matches!(
        ir.node(*only),
        IrNode::Block(crate::svelte::runtime::ir::BlockIr::Each { .. })
            | IrNode::Tag(crate::svelte::runtime::ir::TagIr::Html { .. })
    )
}

/// Append the static attributes of an element to the skeleton (`class="x"`),
/// eliding dynamic / directive attributes (resolved by the DOM walk + ops).
///
/// `scope_hash` is `Some(hash)` when THIS element is css-scoped: the STATIC
/// scope-class injection site (the official `RegularElement.js` bake — one of
/// the two must-agree injection sites). A static/valueless `class` gets the
/// hash appended into its literal (`class="card"` → `class="card svelte-<hash>"`;
/// an empty/valueless class becomes `class="svelte-<hash>"`), and a scoped
/// element with NO class attribute at all synthesizes `class="svelte-<hash>"`
/// (the official synthetic empty-class attribute the analysis pushes for a
/// scoped element, flowing through the same bake). A DYNAMIC/mixed class or a
/// `class:` directive routes the hash through `$.set_class` instead (the
/// dynamic injection site); a spread routes it through `$.attribute_effect`.
///
/// This handles the GENERAL static-attribute case (escaping, quoting). It does
/// NOT special-case bind-driven input-default removal: the official compiler
/// strips a static `value` / `checked` / `group` default from the template when a
/// `bind:value` / `bind:group` is present on the same input (emitting
/// `$.remove_input_defaults`).
///
/// TODO(follow-up): bind-aware input-default removal (`$.remove_input_defaults`
/// plus the static-template default stripping for `bind:group` / `bind:value`) is
/// part of the dedicated bindings-breadth work and is intentionally out of scope
/// for this serializer — the static default is preserved here, and the
/// bind-aware stripping is owned by the bindings layer.
fn serialize_static_attrs(
    attrs: &[AttrIr],
    is_custom: bool,
    scope_hash: Option<&str>,
    html: &mut String,
) {
    // Format the SHARED collected attribute list (the single selection authority the
    // `$.from_tree` objectifier reads too) into the cloned-HTML skeleton: ESCAPE-ONLY
    // (the value is already producer-decoded), a valueless attribute serializing as
    // `name=""`.
    for attr in collect_static_attrs(attrs, is_custom, scope_hash) {
        html.push(' ');
        html.push_str(&attr.name);
        html.push_str("=\"");
        html.push_str(&escape_html_attr_context(&attr.value));
        html.push('"');
    }
}

/// One resolved static attribute the cloned template bakes — the SINGLE selection
/// authority both the HTML-string serializer ([`serialize_static_attrs`]) and the
/// `$.from_tree` objectifier ([`objectify_element`]) read, so the two template
/// representations can never bake a different attribute set. `name` is the final
/// (ASCII-lowercased, or literal `class` for the scope bake/synth) key; `value` is
/// the RAW producer-decoded value (`""` for a valueless boolean), so each formatter
/// applies its OWN escaping (HTML attribute-escape vs the JS single-quote literal).
struct CollectedAttr {
    /// The final attribute key (lowercased HTML name, or `class` for the bake/synth).
    name: String,
    /// The raw producer-decoded value (`""` for a valueless boolean).
    value: String,
}

/// Collect the ordered static attributes an element's cloned template bakes — the
/// official `RegularElement.js` static-attribute selection, shared by the HTML-string
/// serializer and the `$.from_tree` objectifier.
///
/// Shares [`serialize_static_attrs`]'s attribute-selection logic exactly: a SPREAD switches
/// the whole element to the runtime `$.attribute_effect` fold (no baked attribute); a
/// static `class`/`style` whose element carries a `class:`/`style:` directive is pulled
/// OUT (its base rides `$.set_class`/`$.set_style`); a `cannot_be_set_statically`
/// attribute and a `bind:group` `value` are excluded; a custom element keeps ONLY `is`;
/// an EMPTY `class=""` is dropped; the scope hash bakes into (or synthesizes) the
/// `class` value. The value is stored RAW (decoded) — the caller escapes.
fn collect_static_attrs(
    attrs: &[AttrIr],
    is_custom: bool,
    scope_hash: Option<&str>,
) -> Vec<CollectedAttr> {
    // A SPREAD on the element switches its WHOLE attribute strategy to the single
    // `$.attribute_effect` fold: EVERY co-located attribute — including the static ones —
    // moves into the runtime object literal, so NONE is baked into the cloned skeleton
    // (the official `Element.js` spread path emits a bare `<div></div>`; the scope
    // hash rides the fold's `css_hash` argument, never the skeleton).
    if attrs.iter().any(|a| matches!(a, AttrIr::Spread { .. })) {
        return Vec::new();
    }
    let mut out: Vec<CollectedAttr> = Vec::new();
    // The official `RegularElement.js` rule: a static `class` / `style` stays baked
    // into the skeleton ONLY when the element carries NO `class:` / `style:`
    // directive — a directive pulls the attribute OUT into the merged `$.set_class`
    // / `$.set_style` (the base value becomes the call's `value` arg). Scan once.
    let has_class_directive = attrs.iter().any(|a| matches!(a, AttrIr::Class { .. }));
    let has_style_directive = attrs.iter().any(|a| matches!(a, AttrIr::Style { .. }));
    // Whether ANY `class` ATTRIBUTE exists (static / dynamic / mixed — the official
    // synthetic-class check is over every `Attribute` named `class`): a scoped
    // element with NO class attribute synthesizes `class="svelte-<hash>"` at the
    // end of the attribute run; one with a DYNAMIC/mixed class leaves the hash to
    // the `$.set_class` site.
    let has_class_attr = attrs.iter().any(|a| {
        matches!(
            a,
            AttrIr::Static { name, .. } | AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. }
                if name.eq_ignore_ascii_case("class")
        )
    });
    // The static-bake hash applies only on the STATIC class path (no `class:`
    // directive — a directive routes the base through `$.set_class`) and never
    // on a custom element (whose attributes are runtime property writes).
    let bake_hash = scope_hash.filter(|_| !has_class_directive && !is_custom);
    // The official `bind:group` form emits a static `value="X"` as a runtime
    // `input.value = input.__value = 'X'` write (the group-value `__value` source),
    // NOT a baked static `value` attr — so a `value` on a `bind:group` input is pulled
    // OUT of the cloned skeleton (the pinned svelte@5.56.3 group template is a bare
    // `<input type="radio"/>`).
    let has_group_bind = attrs
        .iter()
        .any(|a| matches!(a, AttrIr::Bind { target, .. } if target == "group"));
    for attr in attrs {
        if let AttrIr::Static { name, value } = attr {
            // A static `value` on a `bind:group` input is pulled out of the skeleton
            // (it becomes the runtime `__value` write).
            if name == "value" && has_group_bind {
                continue;
            }
            // A "cannot be set statically" attribute (`autofocus` / `muted` /
            // `defaultValue` / `defaultChecked`) is NEVER in the skeleton — it is
            // applied at runtime via a property write / `$.autofocus` (the
            // `NonStaticProperty` op the ops pass emits). The official
            // `cannot_be_set_statically` exclusion.
            if cannot_be_set_statically(name) {
                continue;
            }
            // A static `class` / `style` whose element ALSO carries a `class:` /
            // `style:` directive is pulled OUT of the skeleton (its value becomes the
            // base arg to the merged `$.set_class` / `$.set_style`). The NAME matches
            // case-insensitively — the same normalization the surface gate and the
            // plan's base-consumption arms apply.
            if (name.eq_ignore_ascii_case("class") && has_class_directive)
                || (name.eq_ignore_ascii_case("style") && has_style_directive)
            {
                continue;
            }
            // A CUSTOM element's attributes are set via PROPERTIES at runtime, so
            // they are NOT in the skeleton — EXCEPT `is`, which stays for the
            // customized-built-in upgrade.
            if is_custom && name != "is" {
                continue;
            }
            // The STATIC scope-class bake (the official `RegularElement.js`
            // rule over the scoped element): a valueless/empty class becomes
            // the bare hash; a valued class appends ` <hash>` after its value
            // (the IR value is ALREADY decoded at the producer boundary). The
            // hash itself is plain ASCII (no escapable characters), so the
            // RAW-value contract (` <hash>` appended to the raw value) escapes
            // identically to an escape-then-append form.
            if let (Some(hash), true) = (bake_hash, name.eq_ignore_ascii_case("class")) {
                let value = match value {
                    Some(StaticAttrValue { value }) if !value.is_empty() => {
                        format!("{} {hash}", value.as_str())
                    }
                    _ => hash.to_string(),
                };
                out.push(CollectedAttr {
                    name: "class".to_string(),
                    value,
                });
                continue;
            }
            // The official compiler DROPS a static `class` whose value is the EXACTLY
            // EMPTY string (`<div class="">` → `<div>`) — an empty class has no
            // effect. A valueless `class` (`<div class>`, `value: None`) is NOT this
            // case (it serializes as `class=""`), and `class=" "` (a space) is kept.
            if name == "class"
                && matches!(value, Some(StaticAttrValue { value }) if value.is_empty())
            {
                continue;
            }
            // The official client template serializer lowercases a static attribute
            // NAME on an HTML element (`template.js`: `is_html ? key.toLowerCase() :
            // key`). Every element the supported client surface serializes is HTML
            // (an SVG / MathML element fails closed at the element allowlist gate), so
            // the name is unconditionally ASCII-lowercased — `data-FooBar` →
            // `data-foobar`, `aria-LabelledBy` → `aria-labelledby`. The allowlisted
            // names (`id` / `class` / `href` / `type` / …) are already lowercase, so
            // only the case-preserving `data-*` / `aria-*` families observe a change.
            // A valueless boolean attribute (`<input disabled>`) collects the EMPTY
            // string (`name=""` in HTML, `name: ''` in the tree); a valued attribute
            // collects its raw producer-decoded value.
            out.push(CollectedAttr {
                name: name.to_ascii_lowercase(),
                value: match value {
                    Some(StaticAttrValue { value }) => value.as_str().to_string(),
                    None => String::new(),
                },
            });
        }
    }
    // A scoped element with NO class attribute at all synthesizes the scope
    // class at the END of the attribute run — the official synthetic empty
    // class attribute (pushed onto `node.attributes` by the analysis) baked
    // through the same `value === '' → value = hash` rule.
    if let Some(hash) = bake_hash {
        if !has_class_attr {
            out.push(CollectedAttr {
                name: "class".to_string(),
                value: hash.to_string(),
            });
        }
    }
    out
}

// ── `$.from_tree` objectification (the CSP-safe `fragments: 'tree'` factory) ──
//
// A faithful port of `svelte@5.56.3`'s `Template.as_tree` / `objectify`
// (`transform-template/template.js`): the SAME cleaned-item sequence the HTML-string
// serializer consumes is emitted as a JS ARRAY LITERAL instead of a backtick string.
// The two representations bake identical attributes (the shared [`collect_static_attrs`])
// and identical structure (element name → optional attrs object / `null` → spread
// children); a text child decodes to `node.data` (the object literal is a JS string,
// unlike the raw-entity HTML clone), a rendered `<!>` anchor (block / component /
// renderable) is a sparse-array HOLE (`objectify` returns `null` → an elided element),
// and a kept comment is `['// data']` / a hole. The contract is the STRUCTURAL
// tree topology the `$.from_tree` runtime consumes — the emitted array/object
// SHAPE (element name → attrs → spread children, sparse holes for anchors), not
// cosmetic byte-parity with esrap's printer.

/// Objectify a REGION's cleaned root sequence into the `$.from_tree` array literal —
/// the `as_tree` mirror of the HTML-string skeleton. Applies the `as_tree`
/// leading-comment unshift: when the first root serializes to a template COMMENT node,
/// a `null` hole is prepended for the `effect.start` anchor.
pub(super) fn objectify_region(
    ir: &SvelteRuntimeIr,
    items: &[CleanItem],
    ctx: CleanContext,
    css: Option<&CssScopeFacts>,
) -> String {
    let mut elements: Vec<Option<String>> = Vec::new();
    // `as_tree`: if the first template node's type is `comment`, UNSHIFT a
    // `{ comment, data: undefined }` (→ `null`, a hole) so the runtime has an
    // `effect.start` anchor before it. In the cleaned sequence EVERY non-element
    // root node serializes to a template comment — an authored `IrNode::Comment`
    // (`<!--data-->` / `['// data']`) OR a rendered `<!>` hydration anchor (a block /
    // component / renderable special / `{@html}` / `{@render}` first-root, all of which
    // the html serializer emits as `<!>` and `objectify` returns `null` for). A text /
    // element first-root is `nodes[0].type === 'text' | 'element'` — no unshift. (A root
    // interpolation is the flush-sequence ` ` text placeholder, a `TextRun`, not a node.)
    if let Some(CleanItem::Node(id)) = items.first() {
        if !matches!(ir.node(*id), IrNode::Element(_)) {
            elements.push(None);
        }
    }
    elements.extend(objectify_items(ir, items, ctx, css, false));
    render_js_array(&elements)
}

/// Objectify a CLEANED item sequence into the ordered array elements (each `Some(js)`
/// or `None` for a sparse hole). `strip_first_text_newline` applies the `pre`/`textarea`
/// leading-newline strip to item 0 when it is a text literal.
fn objectify_items(
    ir: &SvelteRuntimeIr,
    items: &[CleanItem],
    ctx: CleanContext,
    css: Option<&CssScopeFacts>,
    strip_first_text_newline: bool,
) -> Vec<Option<String>> {
    items
        .iter()
        .enumerate()
        .map(|(idx, item)| objectify_item(ir, item, ctx, css, strip_first_text_newline && idx == 0))
        .collect()
}

/// Objectify one cleaned item: a text run → its decoded JS string literal (`node.data`
/// join); an element → its `['name', attrs?, ...children?]` array; a kept comment →
/// `['// data']` (or a hole for an empty `<!---->`); every other rendered node (a
/// `<!>` anchor: block / component / renderable special / `{@html}` / interpolation) →
/// `None`, a sparse-array hole (`objectify` returns `null`).
fn objectify_item(
    ir: &SvelteRuntimeIr,
    item: &CleanItem,
    ctx: CleanContext,
    css: Option<&CssScopeFacts>,
    strip_leading_newline: bool,
) -> Option<String> {
    match item {
        CleanItem::TextRun { text, .. } => {
            // `objectify` a text node uses the DECODED `node.data` join (a JS string),
            // NOT the raw-entity form the HTML clone keeps. An interpolation run's ` `
            // placeholder decodes to ` ` (the official `flush_sequence` space text).
            let mut decoded = decode_text_entities(text);
            if strip_leading_newline {
                decoded = strip_one_leading_newline(&decoded);
            }
            Some(js_string_literal(&decoded))
        }
        CleanItem::Node(id) => match ir.node(*id) {
            IrNode::Element(el) => Some(objectify_element(ir, el, *id, ctx, css)),
            // A kept comment (only present under `preserveComments`) → `['// data']`;
            // an empty `<!---->` → `null` (a hole), the official `objectify` comment arm.
            // `data` is the comment's INNER content (the IR `text` is the full raw comment).
            IrNode::Comment { text, .. } => {
                let data = comment_inner_data(text);
                if data.is_empty() {
                    None
                } else {
                    Some(render_js_array(&[Some(js_string_literal(&format!(
                        "// {data}"
                    )))]))
                }
            }
            // A component / renderable special / block / `{@html}` / interpolation at a
            // rendered position is a `<!>` hydration anchor — `objectify` returns `null`,
            // which esrap prints as an elided (sparse) array element.
            _ => None,
        },
    }
}

/// Objectify an element into its `['name', attrs?, ...children?]` array literal — the
/// `objectify` element arm. The attrs object is pushed ONLY when the element has any
/// baked attribute OR any child (a childless no-attr element is `['name']`; a
/// childless with-attrs element is `['name', { … }]`; a with-children no-attrs element
/// is `['name', null, …]`). Children are objectified under the CHILD cleaning context
/// (namespace / `<pre>` whitespace / SVG-`<text>`), and a sole controlled `{#each}` /
/// `{@html}` child leaves the element childless (the `is_controlled` skeleton rule).
fn objectify_element(
    ir: &SvelteRuntimeIr,
    el: &crate::svelte::runtime::ir::ElementIr,
    node: NodeId,
    ctx: CleanContext,
    css: Option<&CssScopeFacts>,
) -> String {
    let mut elements: Vec<Option<String>> = vec![Some(js_string_literal(&el.tag))];
    let scope_hash = css.and_then(|facts| facts.hash_for(node));
    let collected = collect_static_attrs(&el.attrs, is_custom_element(el), scope_hash);
    let child_ctx = ctx.for_children_of(&el.tag);
    let child_items = clean_nodes(ir, &el.children, child_ctx);
    let strip_newline = el.tag == "pre" || el.tag == "textarea";
    let children: Vec<Option<String>> = if is_sole_controlled(ir, &child_items) {
        Vec::new()
    } else {
        objectify_items(ir, &child_items, child_ctx, css, strip_newline)
    };
    // The attrs slot rides ONLY when there are attributes OR children (else the element
    // is a bare `['name']`); with children but no attributes it is the literal `null`.
    if !collected.is_empty() || !children.is_empty() {
        let attrs = if collected.is_empty() {
            "null".to_string()
        } else {
            render_js_object(&collected)
        };
        elements.push(Some(attrs));
    }
    elements.extend(children);
    render_js_array(&elements)
}

/// Render an ordered array of elements (each `Some(rendered)` or `None` for a sparse
/// hole) as esrap's NON-MULTILINE `ArrayExpression` bytes: elements joined by `, `,
/// a hole (a JS `null` element) printed as a bare `,` with no surrounding space
/// (`['a',, 'c']`), a leading/trailing hole keeping its comma. (Long arrays esrap
/// wraps multi-line; that is cosmetic carrier formatting the conformance oracle
/// normalizes, so the emitter always prints single-line.)
fn render_js_array(elements: &[Option<String>]) -> String {
    let n = elements.len();
    let mut body = String::new();
    for (idx, el) in elements.iter().enumerate() {
        // esrap writes a `, ` between elements, BUT a hole (a JS `null` element) gets no
        // leading space — the comma abuts the previous element (`'a',,`).
        if idx > 0 && el.is_some() {
            body.push(' ');
        }
        if let Some(s) = el {
            body.push_str(s);
        }
        // A non-last element, or ANY hole (`!child` in esrap's `sequence`), gets a
        // trailing separator.
        if idx + 1 < n || el.is_none() {
            body.push(',');
        }
    }
    format!("[{body}]")
}

/// Render a collected attribute list as esrap's NON-MULTILINE `ObjectExpression` bytes:
/// `{ key: 'value', … }` (padded braces, `, ` between properties). A key that is a valid
/// JS identifier stays unquoted (`class`); otherwise it is a single-quoted string
/// (`'data-x'`) — the `b.key` rule. The value is always a single-quoted string literal.
fn render_js_object(attrs: &[CollectedAttr]) -> String {
    let body = attrs
        .iter()
        .map(|attr| {
            let key = if is_valid_js_identifier(&attr.name) {
                attr.name.clone()
            } else {
                js_string_literal(&attr.name)
            };
            format!("{key}: {}", js_string_literal(&attr.value))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {body} }}")
}

/// Whether `name` is a valid JS identifier (esrap's `b.key` unquoted-key rule,
/// `regex_is_valid_identifier = /^[a-zA-Z_$][a-zA-Z_$0-9]*$/`).
fn is_valid_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Render a JS single-quoted string literal, matching esrap's `quote` escaping
/// (`\\`, the quote char, `\n`, `\r`; every other char — including `"` and `\t` —
/// verbatim). svelte's `b.literal` carries no `raw`, so esrap re-quotes the value.
fn js_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// The INNER content of a raw HTML comment — the official svelte comment `data` (the
/// bytes between `<!--` and `-->`). The IR stores the FULL raw comment span (delimiters
/// included), so both the skeleton `stringify` and the tree `objectify` recover the
/// data here: an empty data (`<!---->`) becomes the bare `<!>` / a `null` hole, and a
/// non-empty data drives `<!--data-->` / `['// data']`.
pub(super) fn comment_inner_data(text: &str) -> &str {
    text.strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .unwrap_or(text)
}

/// Strip ONE leading `\r?\n` (the official `regex_starts_with_newline` used by
/// `objectify` for the `pre` / `textarea` first-child literal).
fn strip_one_leading_newline(s: &str) -> String {
    s.strip_prefix("\r\n")
        .or_else(|| s.strip_prefix('\n'))
        .unwrap_or(s)
        .to_string()
}
