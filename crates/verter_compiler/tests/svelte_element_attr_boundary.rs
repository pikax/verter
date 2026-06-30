//! The COMPLETENESS GATE for the strict finite element + static-attr allowlists.
//!
//! This is the convergence guarantee for the element/attr boundary: any new
//! tag/attr requires extending a finite enum (`SupportedHtmlElement` /
//! `SupportedStaticAttr`) AND adding a row here in the same change — nothing can leak
//! through an approximation.
//!
//! ## Element matrix
//! Probes a COMMITTED enumeration of the full HTML tag universe (the TS DOM lib
//! `HTMLElementTagNameMap`, captured as [`HTML_TAG_UNIVERSE`]) plus explicit corpora
//! for Svelte specials (`slot` / `svelte:*`), the full Svelte `RESERVED_WORDS`,
//! hyphenated/custom tags, and special-content tags. The expected SUPPORTED set is
//! EXACTLY `{a, audio, button, details, div, h1, input, option, p, select, textarea,
//! video}`; EVERY other tag compiles fail-closed with NO `Main` module. (The 5c bind
//! hosts `textarea`/`select`/`option` emit a bare clone frame for empty content; their
//! special-content interiors are content-gated.) (Hermetic: the tag universe is committed, so the gate runs
//! with no `node` / no `node_modules` at test runtime; the
//! [`html_tag_universe_is_complete`] freshness check — feature-gated behind a live
//! `node_modules` read — keeps the committed list honest.)
//!
//! ## Attr matrix
//! Crosses the allowed elements with every allowed attr (golden-compared: the attr
//! MUST appear in the emitted `from_html` skeleton exactly as the allowlist
//! specifies) and with the still-forbidden attrs (`defaultValue` / `defaultChecked` /
//! `dir` / static `style` / `value` / `checked` / `is` / `loading` / `selected` /
//! an arbitrary unknown name), asserting each fails BEFORE emission. (Dynamic attrs,
//! boolean DOM props, `autofocus`, `muted` on ANY element, and `class` / `style` are
//! SUPPORTED and have positive rows.)
//!
//! The skeleton-presence assertion is the SERIALIZER-CONTRACT gate: an accepted
//! static attr can NEVER be silently dropped (the root-cause of the `defaultValue`
//! leak — accepted at classification, dropped at serialization).

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{compile_client, ClientCompileError, SvelteRuntimeOptions};

/// Compile a source through the client backend, returning the emitted JS or the
/// typed refusal.
fn compile(source: &str) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false).map(|m| m.code)
}

/// Whether a component COMPILES to an emitted `Main` module (a non-fail-closed
/// emission).
fn emits_main(source: &str) -> bool {
    matches!(compile(source), Ok(js) if js.contains("export default function"))
}

/// The EXACT supported element set (the finite client-core allowlist). Any change to
/// this set is a deliberate enum + golden change, asserted by
/// [`element_matrix_supports_exactly_the_thirteen_core_tags`]. `video` joined as the
/// media host for the `muted` DOM-property write; `textarea`/`select`/`option`/
/// `audio`/`details` joined as the bindings-breadth DOM-bind hosts (each emits a
/// bare clone frame with empty content; their special-content / non-bind interiors are
/// content-gated, not element-gated); `span` is the plain inline structural host the
/// component slot / `{#snippet}` body fixtures need.
const SUPPORTED_ELEMENTS: &[&str] = &[
    "a", "audio", "button", "details", "div", "h1", "input", "option", "p", "select", "span",
    "textarea", "video",
];

/// The full HTML tag universe (the TS DOM lib `HTMLElementTagNameMap`,
/// svelte/TS-pinned). COMMITTED so the matrix is hermetic. Every tag NOT in
/// [`SUPPORTED_ELEMENTS`] must compile fail-closed.
const HTML_TAG_UNIVERSE: &[&str] = &[
    "a",
    "abbr",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "bdi",
    "bdo",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "map",
    "mark",
    "menu",
    "meta",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "slot",
    "small",
    "source",
    "span",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

/// The full Svelte `RESERVED_WORDS` set (svelte@5.56.3 `src/utils.js`) — a
/// reserved-word HTML tag fails closed at the element gate (it is not in the
/// allowlist, and a reserved word additionally routes to the `5v` naming refusal). A
/// committed copy keeps the corpus hermetic; the source-of-truth is the production
/// `SVELTE_RESERVED_WORDS` const (the boundary test only needs the same membership).
const SVELTE_RESERVED_WORDS: &[&str] = &[
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Wrap a single element under test in a runes-mode component (a trailing reactive
/// `{c}` button keeps the component runes-mode + emitting, so a SUPPORTED element
/// reaches a real `Main` rather than the legacy / root-text path). The element under
/// test renders BEFORE the trailing button.
/// The HTML void elements (no closing tag permitted) — `<{tag}></{tag}>` is the
/// official `void_element_invalid_content` reject for these, so a void tag must be
/// emitted in its bare void form (`<input>`), NOT as an open/close pair.
const VOID_HTML_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

fn component_with_element(tag: &str) -> String {
    // A non-void unknown tag parses as an open/close pair (`<tag></tag>`); a VOID tag
    // must be its bare void form (`<input>`) — an explicit close `</input>` is the
    // official `void_element_invalid_content` compile error. The interior is empty
    // (static), so a SUPPORTED element emits a bare clone frame.
    let element = if VOID_HTML_TAGS.contains(&tag) {
        format!("<{tag}>")
    } else {
        format!("<{tag}></{tag}>")
    };
    format!(
        "<script>let c = $state(0);</script>\n{element}\n<button onclick={{() => c++}}>{{c}}</button>\n"
    )
}

/// Tags the Svelte PARSER handles specially — they do NOT lower to a renderable
/// intrinsic element in an SFC template (`<script>` / `<style>` become the component's
/// script/style blocks; `<title>` only renders under `<svelte:head>`; the document
/// structural tags are not template-renderable). For these, a `<tag></tag>` in the
/// template never produces an intrinsic-element clone of `tag`, so the relevant
/// invariant is "the emitted skeleton never CLONES `<tag>` as an element", not "the
/// compile fails closed" (there may be no element under test at all).
const SVELTE_SPECIAL_PARSE_TAGS: &[&str] = &[
    "script", "style", "template", "title", "head", "body", "html", "noscript",
];

/// Whether the emitted module (if any) clones `<tag>` as an intrinsic element — i.e.
/// the `from_html` skeleton contains a `<tag` open-tag token. A component that fails
/// closed (no module) trivially does not clone it.
fn skeleton_clones_tag(source: &str, tag: &str) -> bool {
    let Ok(js) = compile(source) else {
        return false;
    };
    let Some(skeleton) = first_from_html_skeleton(&js) else {
        return false;
    };
    skeleton.contains(&format!("<{tag}"))
}

#[test]
fn element_matrix_supports_exactly_the_thirteen_core_tags() {
    // The POSITIVE half: EXACTLY the allowlist tags emit a `Main` that CLONES the tag as
    // an intrinsic element; the count and membership are both pinned (a shrink OR a widen
    // fails here). The Svelte-special-parse tags (`script` / `style` / …) are excluded
    // from the positive set — they never lower to an intrinsic element (a `<script>`
    // becomes the script block), so they cannot be "supported elements". (The bindings-
    // breadth DOM-bind hosts emit a bare clone frame for the empty-content
    // `component_with_element` source; their special-content interiors are content-
    // gated separately.)
    let mut supported: Vec<&str> = HTML_TAG_UNIVERSE
        .iter()
        .copied()
        .filter(|tag| !SVELTE_SPECIAL_PARSE_TAGS.contains(tag))
        .filter(|tag| {
            let src = component_with_element(tag);
            emits_main(&src) && skeleton_clones_tag(&src, tag)
        })
        .collect();
    supported.sort_unstable();
    let mut expected: Vec<&str> = SUPPORTED_ELEMENTS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        supported, expected,
        "the element allowlist must support EXACTLY {{a, audio, button, details, div, h1, \
         input, option, p, select, span, textarea, video}} — a tag cloned into a Main outside \
         that set is a leak; a tag in the set failing to emit is an over-reach. (`span` is the \
         plain inline structural host the component-slot / `{{#snippet}}`-body fixtures need; \
         `ul` / `li` have no live conformance need.)"
    );
    // Belt-and-suspenders on the cardinality (the convergence count gate).
    assert_eq!(
        supported.len(),
        13,
        "exactly thirteen elements are in the client-core allowlist (the §1.2 / bind-host \
         tags + span)"
    );
}

#[test]
fn element_matrix_every_non_allowlisted_html_tag_fails_closed_with_no_main() {
    // The NEGATIVE half: every HTML tag NOT in the allowlist either compiles
    // fail-closed (a typed refusal, NO `Main`, no panic) OR — for a Svelte-special-parse
    // tag that never becomes an intrinsic element — emits a module that does NOT clone
    // the tag. Either way, an out-of-allowlist tag NEVER appears as a cloned intrinsic
    // element in the emitted skeleton. This is the exhaustive cover over the committed
    // HTML tag universe.
    let mut leaks = Vec::new();
    for &tag in HTML_TAG_UNIVERSE {
        if SUPPORTED_ELEMENTS.contains(&tag) {
            continue;
        }
        let src = component_with_element(tag);
        if SVELTE_SPECIAL_PARSE_TAGS.contains(&tag) {
            // The special-parse tag must NOT clone as an intrinsic element.
            if skeleton_clones_tag(&src, tag) {
                leaks.push(format!(
                    "<{tag}> (special-parse) was cloned into a skeleton"
                ));
            }
            continue;
        }
        match compile(&src) {
            Err(ClientCompileError::Unsupported(_)) => {}
            Ok(js) => leaks.push(format!("<{tag}> emitted a Main:\n{js}")),
            Err(other) => leaks.push(format!("<{tag}> errored unexpectedly: {other:?}")),
        }
    }
    assert!(
        leaks.is_empty(),
        "out-of-allowlist HTML tags must never clone as an intrinsic element:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn element_matrix_still_demoted_special_content_tags_fail_closed() {
    // The special-content-model tags NOT yet supported (`optgroup` / `selectedcontent`
    // / `datalist`) still fail closed at the element gate — none is in the allowlist.
    // (`textarea` / `select` / `option` MOVED to the allowlist as 5c bind hosts; they
    // emit a bare clone frame for empty content — see the positive matrix above — and
    // their special-content interiors are gated by the content-model test below.)
    for tag in ["optgroup", "selectedcontent", "datalist"] {
        let src = component_with_element(tag);
        assert!(
            !emits_main(&src),
            "the still-demoted special-content tag <{tag}> must fail closed (no Main)"
        );
        assert!(
            matches!(
                compile(&src),
                Err(ClientCompileError::Unsupported(
                    verter_compiler::svelte::runtime::UnsupportedSvelteRuntimeSurface::Element { .. }
                ))
            ),
            "<{tag}> must fail closed with the regular-element refusal"
        );
    }
}

#[test]
fn element_matrix_allowed_bind_hosts_fail_closed_on_unsupported_special_content() {
    // The 5c bind hosts are accepted as ELEMENTS but their SPECIAL-CONTENT-MODEL
    // interiors (the surfaces 5c does not emit) fail closed at the content gate, not
    // the element gate: a `<textarea>` with interior text/interpolation is the raw-text
    // value surface; an `<option>` with an interpolation child is the `option.__value`
    // tracking surface. Each must fail closed (no divergent Main).
    let cases = [
        // textarea with static text content (the raw-text-value surface).
        (
            "textarea_static_content",
            "<script>let c = $state(0);</script>\n<textarea>hi</textarea>\n<button onclick={() => c++}>{c}</button>\n",
            "textarea",
        ),
        // textarea with interpolation content.
        (
            "textarea_interp_content",
            "<script>let c = $state(0);</script>\n<textarea>{c}</textarea>\n<button onclick={() => c++}>{c}</button>\n",
            "textarea",
        ),
        // option with interpolation content (the __value tracking surface).
        (
            "option_interp_content",
            "<script>let c = $state(0);</script>\n<select><option>{c}</option></select>\n<button onclick={() => c++}>{c}</button>\n",
            "option",
        ),
    ];
    for (label, src, expected_tag) in cases {
        assert!(
            !emits_main(src),
            "{label}: the unsupported special content must fail closed (no Main)"
        );
        match compile(src) {
            Err(ClientCompileError::Unsupported(
                verter_compiler::svelte::runtime::UnsupportedSvelteRuntimeSurface::Element {
                    tag,
                    ..
                },
            )) => {
                assert_eq!(
                    tag, expected_tag,
                    "{label}: must fail closed on the special-content host <{expected_tag}>"
                );
            }
            other => panic!("{label}: expected a content-gate Element refusal, got {other:?}"),
        }
    }
}

#[test]
fn element_matrix_hyphenated_custom_tags_fail_closed() {
    // A hyphenated custom element is the web-components surface (5h), not the
    // regular-element allowlist.
    for tag in ["my-widget", "x-foo", "ce-button", "app-root"] {
        assert!(
            !emits_main(&component_with_element(tag)),
            "the hyphenated custom element <{tag}> must fail closed (no Main)"
        );
    }
}

#[test]
fn element_matrix_raw_slot_fails_closed() {
    // A raw `<slot>` must NEVER reach intrinsic emission (Verter's parser does not
    // model the official `SlotElement`).
    let src = "<script>let c = $state(0); c = 1;</script>\n<slot></slot>\n";
    assert!(!emits_main(src), "a raw <slot> must fail closed (no Main)");
    assert!(
        matches!(
            compile(src),
            Err(ClientCompileError::Unsupported(
                verter_compiler::svelte::runtime::UnsupportedSvelteRuntimeSurface::Element { tag, .. }
            )) if tag == "slot"
        ),
        "a raw <slot> must fail closed with the regular-element refusal"
    );
}

#[test]
fn element_matrix_svelte_special_elements_fail_closed() {
    // The host / renderable `<svelte:*>` specials fail closed (5f-b), as does a STANDALONE
    // `<svelte:fragment>` (the transparent-wrapper surface — the 5f-a fragment surface is
    // the `slot=`-bearing NAMED slot, absorbed into its parent component). The
    // component-INVOCATION specials `<svelte:component>` / `<svelte:self>` are NOW SUPPORTED
    // (5f-a) and emit a `$.component(...)` / recursive call — they are NOT in this
    // fail-closed corpus. `<svelte:options>` is a compile-option carrier (gated elsewhere).
    for tag in [
        "svelte:element",
        "svelte:fragment",
        "svelte:window",
        "svelte:document",
        "svelte:body",
        "svelte:head",
        "svelte:boundary",
    ] {
        let src = format!(
            "<script>let c = $state(0);</script>\n<{tag}></{tag}>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        assert!(
            !emits_main(&src),
            "the Svelte special <{tag}> must fail closed (no Main)"
        );
    }
}

#[test]
fn element_matrix_reserved_word_tags_fail_closed() {
    // Every Svelte `RESERVED_WORDS` tag fails closed — none is in the element
    // allowlist, and a reserved-word tag additionally routes to the `5v` naming
    // refusal (its DOM var name would be an invalid/reserved JS binding).
    let mut leaks = Vec::new();
    for &word in SVELTE_RESERVED_WORDS {
        let src = component_with_element(word);
        match compile(&src) {
            Err(ClientCompileError::Unsupported(_)) => {}
            Ok(js) => leaks.push(format!("reserved-word tag <{word}> emitted a Main:\n{js}")),
            Err(other) => leaks.push(format!("<{word}> errored unexpectedly: {other:?}")),
        }
    }
    assert!(
        leaks.is_empty(),
        "reserved-word tags must fail closed (no Main):\n{}",
        leaks.join("\n")
    );
}

// ── Attr matrix ────────────────────────────────────────────────────────────────

/// One allowed-attr row: the host element, the attribute markup, and the EXACT token
/// the emitted `from_html` skeleton must contain (the serializer-contract proof — an
/// accepted attr is never silently dropped).
struct AllowedAttrRow {
    /// A label for diagnostics.
    label: &'static str,
    /// The element markup carrying the attr.
    markup: &'static str,
    /// The exact attr token the skeleton must contain.
    skeleton_token: &'static str,
}

/// The allowed-attr × allowed-element rows. Each MUST emit a `Main` whose `from_html`
/// skeleton contains the attr token EXACTLY as the allowlist specifies.
const ALLOWED_ATTR_ROWS: &[AllowedAttrRow] = &[
    AllowedAttrRow {
        label: "div_id",
        markup: "<div id=\"x\"></div>",
        skeleton_token: "id=\"x\"",
    },
    AllowedAttrRow {
        label: "div_title",
        markup: "<div title=\"t\"></div>",
        skeleton_token: "title=\"t\"",
    },
    AllowedAttrRow {
        label: "div_role",
        markup: "<div role=\"button\"></div>",
        skeleton_token: "role=\"button\"",
    },
    AllowedAttrRow {
        label: "div_class_nonempty",
        markup: "<div class=\"box\"></div>",
        skeleton_token: "class=\"box\"",
    },
    AllowedAttrRow {
        label: "div_data",
        markup: "<div data-id=\"5\"></div>",
        skeleton_token: "data-id=\"5\"",
    },
    AllowedAttrRow {
        label: "div_aria",
        markup: "<div aria-label=\"x\"></div>",
        skeleton_token: "aria-label=\"x\"",
    },
    AllowedAttrRow {
        label: "anchor_href",
        markup: "<a href=\"/x\">y</a>",
        skeleton_token: "href=\"/x\"",
    },
    AllowedAttrRow {
        label: "button_type",
        markup: "<button type=\"submit\">y</button>",
        skeleton_token: "type=\"submit\"",
    },
    AllowedAttrRow {
        label: "button_disabled",
        markup: "<button disabled>y</button>",
        // A valueless boolean attr serializes as `name=""` in the official skeleton.
        skeleton_token: "disabled=\"\"",
    },
    AllowedAttrRow {
        label: "input_type",
        markup: "<input type=\"text\" />",
        skeleton_token: "type=\"text\"",
    },
    AllowedAttrRow {
        label: "input_disabled",
        markup: "<input disabled />",
        skeleton_token: "disabled=\"\"",
    },
];

/// Extract the FIRST `$.from_html(`...`)` template literal body from an emitted
/// module (the static skeleton), or `None`.
fn first_from_html_skeleton(code: &str) -> Option<String> {
    let start = code.find("$.from_html(`")?;
    let after = &code[start + "$.from_html(`".len()..];
    let close = after.find('`')?;
    Some(after[..close].to_string())
}

#[test]
fn attr_matrix_allowed_attrs_emit_and_serialize_into_the_skeleton() {
    // The allowed-attr serializer contract: each allowed attr on an allowed element
    // EMITS a `Main` AND appears in the `from_html` skeleton EXACTLY as the allowlist
    // specifies — an accepted attr can NEVER be silently dropped (the `defaultValue`
    // leak's root cause).
    for row in ALLOWED_ATTR_ROWS {
        // The element under test renders before a trailing reactive button (keeps the
        // component runes-mode + reactive so it reaches a real `Main`).
        let src = format!(
            "<script>let c = $state(0);</script>\n{}\n<button onclick={{() => c++}}>{{c}}</button>\n",
            row.markup
        );
        let js = compile(&src).unwrap_or_else(|e| {
            panic!("{}: expected an emitted Main, got refusal {e:?}", row.label)
        });
        assert!(
            js.contains("export default function"),
            "{}: expected an emitted Main:\n{js}",
            row.label
        );
        let skeleton = first_from_html_skeleton(&js).unwrap_or_else(|| {
            panic!(
                "{}: emitted module has no from_html skeleton:\n{js}",
                row.label
            )
        });
        assert!(
            skeleton.contains(row.skeleton_token),
            "{}: the accepted attr must serialize as `{}` into the skeleton (a serializer \
             drop is the silent-drop bug) — skeleton was `{skeleton}`",
            row.label,
            row.skeleton_token
        );
    }
}

#[test]
fn attr_matrix_forbidden_attrs_fail_before_emission_on_allowed_elements() {
    // Every forbidden attr — on an ALLOWLISTED host so the ELEMENT gate does not mask
    // the attr gate — fails closed BEFORE emission (no `Main`). `is` is the exception:
    // it is rejected at the element gate (a customized built-in), which is still a
    // fail-closed refusal.
    //
    // Note: `autofocus` (a non-static property → `$.autofocus`) and `muted` on ANY
    // element (`is_dom_property('muted')` is element-agnostic, so `<div muted>` →
    // `div.muted = true` exactly like `<video muted>`) are now SUPPORTED and have
    // dedicated positive cases; a STATIC `style` / `dir` (no `style:` / no reflection
    // support) stay forbidden.
    let forbidden: &[(&str, &str)] = &[
        ("default_value", "<input defaultValue=\"x\" />"),
        ("default_checked", "<input defaultChecked />"),
        ("dir", "<div dir=\"ltr\">d</div>"),
        ("style", "<div style=\"color:red\">d</div>"),
        ("input_value", "<input value=\"x\" />"),
        ("input_checked", "<input checked />"),
        ("is", "<button is=\"my-btn\">x</button>"),
        ("loading_on_div", "<div loading=\"lazy\">d</div>"),
        ("selected_on_div", "<div selected>d</div>"),
        ("empty_class", "<div class=\"\">d</div>"),
        ("unknown_name", "<div totally-unknown=\"1\">d</div>"),
    ];
    let mut leaks = Vec::new();
    for (label, markup) in forbidden {
        let src = format!(
            "<script>let c = $state(0);</script>\n{markup}\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        match compile(&src) {
            Err(ClientCompileError::Unsupported(_)) => {}
            Ok(js) => leaks.push(format!("{label}: forbidden attr emitted a Main:\n{js}")),
            Err(other) => leaks.push(format!("{label}: errored unexpectedly: {other:?}")),
        }
    }
    assert!(
        leaks.is_empty(),
        "forbidden attrs must fail before emission (no Main):\n{}",
        leaks.join("\n")
    );
}

#[test]
fn attr_matrix_forbidden_attr_never_appears_in_any_skeleton() {
    // A discriminating NEGATIVE: a forbidden attr name (`defaultValue`) must NOT leak
    // into ANY emitted skeleton. (The pre-restructure bug ACCEPTED `defaultValue` then
    // the serializer DROPPED it — but a different drift could accept-and-serialize it.
    // Either way, a `Main` whose skeleton carries `defaultValue` is the divergence.)
    // Since the component fails closed, there is no module at all — assert that.
    let src = "<script>let c = $state(0);</script>\n<input defaultValue=\"x\" />\n<button onclick={() => c++}>{c}</button>\n";
    match compile(src) {
        Ok(js) => panic!("a `defaultValue` input must fail closed, got a Main:\n{js}"),
        Err(ClientCompileError::Unsupported(_)) => {}
        Err(other) => panic!("expected a fail-closed refusal, got {other:?}"),
    }
}

// ── Directive accept==emittable contract ────────────────────────────────────────

/// One accepted-directive row: a FULLY-SUPPORTED component source, and the EXACT
/// runtime-op token the emitted `Main` must contain. This is the directive analogue
/// of the static-attr serializer contract ([`ALLOWED_ATTR_ROWS`]) — it proves
/// ACCEPTED == EMITTABLE for the `bind:` / `on*` / interpolation surfaces: a
/// classifier-accepted directive shape ALWAYS produces its runtime op, NEVER a
/// silently-dropped accept (the `bind:value` `expr:None` accept-then-drop class).
struct AcceptedDirectiveRow {
    /// A label for diagnostics.
    label: &'static str,
    /// The full component source carrying the accepted directive.
    source: &'static str,
    /// The exact runtime-op token the emitted `Main` body must contain.
    op_token: &'static str,
}

/// The accepted directive × runtime-op rows. Each MUST emit a `Main` whose body
/// contains the runtime op the classifier-accepted shape implies.
const ACCEPTED_DIRECTIVE_ROWS: &[AcceptedDirectiveRow] = &[
    AcceptedDirectiveRow {
        // The SHORTHAND `bind:value` — its lowering synthesizes the bound `value`
        // identifier, so it is accepted AND emits the binding op (a fail-close of the
        // shorthand would be a regression: the accepted set must stay emittable, NOT
        // shrink to nothing). RED if the shorthand were dropped to no op.
        label: "shorthand_bind_value",
        source: "<script>let value = $state(\"\");</script>\n<input bind:value />\n<button onclick={() => value = \"x\"}>{value}</button>\n",
        op_token: "$.bind_value(",
    },
    AcceptedDirectiveRow {
        // The EXPLICIT §1.2 `bind:value={name}`.
        label: "explicit_bind_value",
        source: "<script>let name = $state(\"\");</script>\n<input bind:value={name} />\n<p>{name}</p>\n",
        op_token: "$.bind_value(",
    },
    AcceptedDirectiveRow {
        // `bind:this={el}` on an intrinsic element to a non-prop identifier.
        label: "bind_this",
        source: "<script>let el; let c = $state(0);</script>\n<div bind:this={el}></div>\n<button onclick={() => c++}>{c}</button>\n",
        op_token: "$.bind_this(",
    },
    AcceptedDirectiveRow {
        // A delegated, modifier-free inline-arrow event handler.
        label: "delegated_event",
        source: "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        op_token: "$.delegated(",
    },
    AcceptedDirectiveRow {
        // A reactive interpolation of a `$state` signal → the grouped reactive-text
        // write.
        label: "reactive_interpolation",
        source: "<script>let c = $state(0);</script>\n<p>{c}</p>\n<button onclick={() => c++}>x</button>\n",
        op_token: "$.set_text(",
    },
    // ── dynamic attributes + boolean DOM props + class/style ──────────
    AcceptedDirectiveRow {
        // A dynamic attribute → `$.set_attribute`.
        label: "dynamic_attr",
        source: "<script>let id = $state('x');</script>\n<button onclick={() => id += '!'} id={id}></button>\n",
        op_token: "$.set_attribute(",
    },
    AcceptedDirectiveRow {
        // A boolean DOM property → a direct property write.
        label: "boolean_property_disabled",
        source: "<script>let v = $state(false);</script>\n<button onclick={() => v = !v} disabled={v}></button>\n",
        op_token: ".disabled = $.get(v)",
    },
    AcceptedDirectiveRow {
        // `muted` on the media host → a property write.
        label: "muted_on_video",
        source: "<script>let v = $state(false);</script>\n<video onclick={() => v = !v} muted={v}></video>\n",
        op_token: ".muted = $.get(v)",
    },
    AcceptedDirectiveRow {
        // `autofocus` → the init-only `$.autofocus` helper.
        label: "autofocus_dynamic",
        source: "<script>let v = $state(true);</script>\n<input onclick={() => v = !v} autofocus={v}>\n",
        op_token: "$.autofocus(",
    },
    AcceptedDirectiveRow {
        // `class={…}` → `$.set_class` (with `$.clsx`).
        label: "dynamic_class",
        source: "<script>let c = $state('a');</script>\n<button onclick={() => c += '!'} class={c}></button>\n",
        op_token: "$.set_class(",
    },
    AcceptedDirectiveRow {
        // `style={…}` → `$.set_style`.
        label: "dynamic_style",
        source: "<script>let s = $state('color:red');</script>\n<button onclick={() => s = 'color:blue'} style={s}></button>\n",
        op_token: "$.set_style(",
    },
    AcceptedDirectiveRow {
        // A `class:` directive → the merged `$.set_class`.
        label: "class_directive",
        source: "<script>let on = $state(false);</script>\n<button onclick={() => on = !on} class:foo={on}></button>\n",
        op_token: "$.set_class(",
    },
    AcceptedDirectiveRow {
        // A `style:` directive → the merged `$.set_style`.
        label: "style_directive",
        source: "<script>let color = $state('red');</script>\n<button onclick={() => color = 'blue'} style:color={color}></button>\n",
        op_token: "$.set_style(",
    },
];

#[test]
fn directive_matrix_accepted_shapes_emit_their_runtime_op() {
    // The directive accept==emittable contract: every accepted `bind:` / `on*` /
    // interpolation shape EMITS a `Main` AND that Main carries the runtime op the
    // accepted shape implies — a classifier-accepted directive can NEVER be silently
    // dropped at op collection / emission (the `bind:value` `expr:None` accept-then-
    // drop class, the directive analogue of the `defaultValue` serializer leak).
    for row in ACCEPTED_DIRECTIVE_ROWS {
        let js = compile(row.source).unwrap_or_else(|e| {
            panic!(
                "{}: expected an emitted Main for an accepted directive, got refusal {e:?}",
                row.label
            )
        });
        assert!(
            js.contains("export default function"),
            "{}: expected an emitted Main:\n{js}",
            row.label
        );
        assert!(
            js.contains(row.op_token),
            "{}: the accepted directive shape must emit `{}` (an accept-then-drop is the \
             silent-divergence bug) — emitted Main was:\n{js}",
            row.label,
            row.op_token
        );
    }
}

#[test]
fn shorthand_bind_value_emits_bind_value_and_is_byte_equivalent_to_explicit() {
    // A DISCRIMINATING regression guard for the shorthand-`bind:value` fix: the
    // shorthand `<input bind:value />` (bound expression synthesized as the `value`
    // identifier) must emit the SAME `$.bind_value(input, () => $.get(value),
    // ($$value) => $.set(value, $$value))` two-way binding as the explicit
    // `bind:value={value}` — official `svelte@5.56.3` parity. (RED against a tree
    // that drops the shorthand binding op to nothing, OR mis-shapes the getter/
    // setter.)
    let shorthand = "<script>let value = $state(\"\");</script>\n<input bind:value />\n<button onclick={() => value = \"x\"}>{value}</button>\n";
    let js = compile(shorthand)
        .unwrap_or_else(|e| panic!("shorthand bind:value must emit a Main, got {e:?}"));
    assert!(
        js.contains("$.bind_value(input, () => $.get(value), ($$value) => $.set(value, $$value))"),
        "shorthand bind:value must emit the full two-way `$.bind_value` binding:\n{js}"
    );
    // NEGATIVE: the binding op is NOT dropped — the Main must NOT lack `$.bind_value`.
    assert!(
        js.contains("$.bind_value("),
        "shorthand bind:value must NOT silently drop its binding op:\n{js}"
    );
}

// ── Duplicate attribute / directive (5a) ────────────────────────────────────────

/// The EXACT official code an `OfficialReject`-channel refusal carries (the official-reject
/// gate's exact-code rail), or `None` when the source compiles or fails closed through the
/// unsupported-feature channel. A template `attribute_duplicate` is an official EXACT-CODE
/// parse error carried here, not the unsupported-feature channel.
fn official_reject_code(source: &str) -> Option<&'static str> {
    match compile(source) {
        Err(ClientCompileError::OfficialReject(rejection)) => Some(rejection.official_code),
        _ => None,
    }
}

#[test]
fn duplicate_attribute_keys_match_official_attribute_duplicate_rule() {
    // The official `attribute_duplicate` parse rule, mirrored: an element with two
    // entries sharing the same normalized `(type-class, name)` key fails closed as an
    // EXACT-CODE official reject (`attribute_duplicate`), minted by the parser's open-tag
    // attribute loop. The key is CASE-SENSITIVE; `Attribute` / `class:` / `style:` are
    // distinct namespaces; a `bind:X` normalizes to `Attribute`+`X`; the duplicate fires
    // BEFORE the per-attr classification (so a `value` + `bind:value` collision reports the
    // DUPLICATE code, not the static-`value` refusal).
    let dup_code = "attribute_duplicate";
    let duplicates: &[(&str, &str)] = &[
        // Two plain `Attribute`s with the same name.
        (
            "static_id",
            "<div id=\"a\" id=\"b\"><button onclick={() => c++}>{c}</button></div>",
        ),
        // A Svelte-5 event ATTRIBUTE (`onclick`, a plain attribute) repeated.
        (
            "event_attr",
            "<button onclick={() => c++} onclick={() => c--}>{c}</button>",
        ),
        // Two `bind:value` directives (both normalize to `Attribute`+`value`).
        ("bind_value", "<input bind:value={v} bind:value={v} />"),
        // A static `value` Attribute + a `bind:value` (normalized) collide.
        ("static_plus_bind", "<input value=\"x\" bind:value={v} />"),
        // Two `class:active` directives in the `class:` namespace.
        (
            "class_directive",
            "<div class:active={on} class:active={on}><button onclick={() => on = !on}>x</button></div>",
        ),
        // Two `style:color` directives in the `style:` namespace.
        (
            "style_directive",
            "<div style:color={c} style:color={c}><button onclick={() => c2++}>{c2}</button></div>",
        ),
    ];
    for (label, markup) in duplicates {
        // A `$state` keeps the component runes-mode; the surface under test is the
        // duplicate attribute. The bindings referenced by the markup are declared.
        let src = format!(
            "<script>let c = $state(0); let c2 = $state(0); let v = $state(''); let on = $state(true);</script>\n{markup}\n"
        );
        assert_eq!(
            official_reject_code(&src),
            Some(dup_code),
            "duplicate::{label} must fail closed with the official `attribute_duplicate` code"
        );
    }
}

#[test]
fn distinct_namespace_and_repeatable_directives_are_not_duplicates() {
    // NEGATIVE: the duplicate rule keys on `(type-class, name)`, so DISTINCT namespaces
    // (`class` Attribute vs `class:` directive, `style` vs `style:`) and REPEATABLE
    // directives (`on:` / `use:` / `transition:`) are NOT duplicates. Each of these
    // may itself be unsupported (a `class:` directive is 5a `dynamic-attribute`, a
    // `use:` is 5f), but NONE must report the official `attribute_duplicate` code. `this`
    // is also exempt.
    let dup_code = "attribute_duplicate";
    let non_duplicates: &[(&str, &str)] = &[
        // A plain `class` Attribute + a `class:active` directive — distinct namespaces.
        (
            "class_attr_plus_directive",
            "<div class=\"box\" class:active={on}><button onclick={() => on = !on}>x</button></div>",
        ),
        // A plain `style` Attribute + a `style:color` directive — distinct namespaces.
        (
            "style_attr_plus_directive",
            "<div style=\"x\" style:color={c}><button onclick={() => c++}>{c}</button></div>",
        ),
        // Two `on:click` LEGACY directives — `OnDirective` may repeat in official.
        (
            "repeated_on_directive",
            "<button on:click={() => c++} on:click={() => c--}>{c}</button>",
        ),
        // Two `use:` directives — `UseDirective` may repeat.
        (
            "repeated_use_directive",
            "<div use:a use:b><button onclick={() => c++}>{c}</button></div>",
        ),
        // Distinct attribute names — no collision.
        (
            "distinct_names",
            "<div id=\"a\" title=\"b\"><button onclick={() => c++}>{c}</button></div>",
        ),
    ];
    for (label, markup) in non_duplicates {
        let src = format!(
            "<script>let c = $state(0); let on = $state(true); function a() {{}} function b() {{}}</script>\n{markup}\n"
        );
        assert_ne!(
            official_reject_code(&src),
            Some(dup_code),
            "non_duplicate::{label} must NOT report the official `attribute_duplicate` code"
        );
    }
}

// ── Freshness: the committed HTML tag universe stays honest ─────────────────────

/// Read the live TS DOM lib `HTMLElementTagNameMap` tag list, if a pinned
/// `typescript` is installed under `node_modules`. Returns `None` (so the freshness
/// check is SKIPPED, keeping the default run hermetic) when no TS lib is present.
fn live_html_tag_universe() -> Option<Vec<String>> {
    use std::path::PathBuf;
    let pnpm = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../node_modules/.pnpm");
    let entries = std::fs::read_dir(&pnpm).ok()?;
    // Pick any installed `typescript@*` package (the tag map is stable across the
    // pinned versions); read its `lib.dom.d.ts`.
    let mut lib_path = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("typescript@") {
            let candidate = entry
                .path()
                .join("node_modules/typescript/lib/lib.dom.d.ts");
            if candidate.is_file() {
                lib_path = Some(candidate);
                break;
            }
        }
    }
    let text = std::fs::read_to_string(lib_path?).ok()?;
    // Slice the `interface HTMLElementTagNameMap { … }` body and pull the quoted keys.
    let start = text.find("interface HTMLElementTagNameMap {")?;
    let body = &text[start..];
    let end = body.find("\n}")?;
    let body = &body[..end];
    let mut tags = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('"') {
            if let Some(close) = rest.find('"') {
                tags.push(rest[..close].to_string());
            }
        }
    }
    tags.sort_unstable();
    tags.dedup();
    (!tags.is_empty()).then_some(tags)
}

#[test]
fn html_tag_universe_is_complete() {
    // FRESHNESS: when a pinned `typescript` is installed, the committed
    // `HTML_TAG_UNIVERSE` must EQUAL the live `HTMLElementTagNameMap`, so a TS-lib bump
    // that adds an element cannot silently escape the element matrix. SKIPPED (not
    // failed) when no TS lib is present (keeps the default `cargo nextest` run
    // hermetic / node_modules-free).
    let Some(live) = live_html_tag_universe() else {
        eprintln!("html_tag_universe_is_complete: no TS DOM lib present — skipping (hermetic)");
        return;
    };
    let mut committed: Vec<String> = HTML_TAG_UNIVERSE.iter().map(|s| s.to_string()).collect();
    committed.sort_unstable();
    committed.dedup();
    assert_eq!(
        committed, live,
        "the committed HTML_TAG_UNIVERSE must equal the live TS DOM \
         `HTMLElementTagNameMap` — regenerate the committed list when the TS lib changes"
    );
    // Every supported element is a real HTML tag (the allowlist references nothing
    // fictional).
    for el in SUPPORTED_ELEMENTS {
        assert!(
            committed.iter().any(|t| t == el),
            "the supported element {el} must be a real HTML tag"
        );
    }
}

// ── Freshness: the DOM attribute/property tables stay pinned to svelte ───────────

/// The committed `DOM_BOOLEAN_ATTRIBUTES` (svelte@5.56.3 `src/utils.js`), mirrored
/// here so the freshness check is a self-contained boundary (the production copy in
/// `client_allowlist.rs` is module-private). The production `is_dom_property` is
/// exercised against the same membership by the in-crate unit tests.
const DOM_BOOLEAN_ATTRIBUTES: &[&str] = &[
    "allowfullscreen",
    "async",
    "autofocus",
    "autoplay",
    "checked",
    "controls",
    "default",
    "disabled",
    "formnovalidate",
    "indeterminate",
    "inert",
    "ismap",
    "loop",
    "multiple",
    "muted",
    "nomodule",
    "novalidate",
    "open",
    "playsinline",
    "readonly",
    "required",
    "reversed",
    "seamless",
    "selected",
    "webkitdirectory",
    "defer",
    "disablepictureinpicture",
    "disableremoteplayback",
];

/// The committed `ATTRIBUTE_ALIASES` VALUES (the camelCase property names) from
/// svelte@5.56.3 `src/utils.js` (`class` is intentionally absent).
const ATTRIBUTE_ALIAS_VALUES: &[&str] = &[
    "formNoValidate",
    "isMap",
    "noModule",
    "playsInline",
    "readOnly",
    "defaultValue",
    "defaultChecked",
    "srcObject",
    "noValidate",
    "allowFullscreen",
    "disablePictureInPicture",
    "disableRemotePlayback",
];

/// Read the live `svelte/src/utils.js`, if a pinned `svelte` is installed under
/// `node_modules`. Returns `None` (so the freshness check is SKIPPED, keeping the
/// default run hermetic) when no `svelte` source is present.
fn live_svelte_utils() -> Option<String> {
    use std::path::PathBuf;
    let utils = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../node_modules/.pnpm/svelte@5.56.3/node_modules/svelte/src/utils.js");
    std::fs::read_to_string(utils).ok()
}

/// Pull every single-quoted string literal from a body slice, in order, SKIPPING
/// `//` line comments (the official `ATTRIBUTE_ALIASES` body has a commented-out
/// `class: 'className'` note that must NOT be read as a real entry).
fn extract_quoted(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let code = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        let mut chars = code.chars();
        while let Some(c) = chars.next() {
            if c == '\'' {
                let mut s = String::new();
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    s.push(d);
                }
                out.push(s);
            }
        }
    }
    out
}

#[test]
fn dom_property_tables_match_pinned_svelte() {
    // FRESHNESS: when the pinned `svelte` source is present, the committed DOM
    // attribute/property tables (mirrored above, and TRANSCRIBED into
    // `client_allowlist.rs`) must EQUAL the live `svelte/src/utils.js` tables — a
    // `svelte` bump that changes them cannot silently desync the property-vs-attribute
    // decision. SKIPPED (hermetic) when no `node_modules` is present.
    let Some(src) = live_svelte_utils() else {
        eprintln!(
            "dom_property_tables_match_pinned_svelte: no pinned svelte — skipping (hermetic)"
        );
        return;
    };
    // The `DOM_BOOLEAN_ATTRIBUTES` array body.
    let bool_start = src
        .find("const DOM_BOOLEAN_ATTRIBUTES = [")
        .expect("svelte DOM_BOOLEAN_ATTRIBUTES")
        + "const DOM_BOOLEAN_ATTRIBUTES = [".len();
    let bool_end = src[bool_start..].find(']').map(|i| bool_start + i).unwrap();
    let live_booleans = extract_quoted(&src[bool_start..bool_end]);
    assert_eq!(
        live_booleans,
        DOM_BOOLEAN_ATTRIBUTES
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        "DOM_BOOLEAN_ATTRIBUTES desynced from pinned svelte — regenerate the committed \
         tables in client_allowlist.rs AND this mirror"
    );
    // The `ATTRIBUTE_ALIASES` object body — the quoted VALUES (the camelCase props).
    let alias_start = src
        .find("const ATTRIBUTE_ALIASES = {")
        .expect("svelte ATTRIBUTE_ALIASES")
        + "const ATTRIBUTE_ALIASES = {".len();
    let alias_end = src[alias_start..]
        .find('}')
        .map(|i| alias_start + i)
        .unwrap();
    let live_alias_values = extract_quoted(&src[alias_start..alias_end]);
    assert_eq!(
        live_alias_values,
        ATTRIBUTE_ALIAS_VALUES
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        "ATTRIBUTE_ALIASES values desynced from pinned svelte — regenerate the committed \
         tables in client_allowlist.rs AND this mirror"
    );
}
