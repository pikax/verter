//! The STRICT FINITE element + static-attribute allowlists for the Svelte client
//! core.
//!
//! Element acceptance and static-attribute acceptance are NOT approximating
//! blocklists that mirror what official Svelte CAN lower — they are FINITE,
//! GOLDEN-OWNED ALLOWLISTS that enumerate EXACTLY what Verter's narrow client
//! emitter DOES lower. [`SupportedHtmlElement::try_from`] is the SOLE
//! element-acceptance authority and [`SupportedStaticAttr::classify`] is the SOLE
//! static-attribute-acceptance authority; the downstream plan/emitter consumes only
//! these typed facts (the DOM local-variable stem comes from
//! [`SupportedHtmlElement::var_stem`], NEVER the raw tag string).
//!
//! Adding a new element / attribute is INTENTIONALLY a two-edit change: extend the
//! finite enum here AND add a golden in the same change. Nothing can leak through an
//! approximation — an unrecognised tag / attribute fails closed BY CONSTRUCTION
//! (`try_from` / `classify` returns `None`).

/// The FINITE allowlist of intrinsic HTML elements the client core emits.
///
/// This is the EXACT §1.2-core element set, sourced from the kept positive
/// topology/golden surfaces (`<a>`, `<button>`, `<div>`, `<h1>`, `<input>`, `<p>`).
/// It is a typed enum (not a `String`) so the emitter consumes a proven element
/// fact, and [`var_stem`](Self::var_stem) supplies the DOM local-variable stem (so
/// no emission path uses the raw tag string as a JS identifier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SupportedHtmlElement {
    /// `<a>`.
    A,
    /// `<button>`.
    Button,
    /// `<div>`.
    Div,
    /// `<h1>`.
    H1,
    /// `<input>`.
    Input,
    /// `<p>`.
    P,
    /// `<video>` — the media host for the `muted` DOM-property write . It
    /// requires `importNode` (the official `from_html(…, TEMPLATE_USE_IMPORT_NODE)`
    /// flag), so the template flag is set by the skeleton planner, not here.
    Video,
    /// `<textarea>` — the `bind:value` host (via `$.bind_value` after
    /// `$.remove_textarea_child`).
    Textarea,
    /// `<select>` — the `bind:value` host (`$.bind_select_value`), single +
    /// `multiple`.
    Select,
    /// `<option>` — a `<select>` child carrying the option value.
    Option,
    /// `<audio>` — a media host for the media-binding family (`bind:currentTime`
    /// / `bind:paused` / `bind:duration` / …).
    Audio,
    /// `<details>` — the `bind:open` host (`$.bind_property('open','toggle',…)`).
    Details,
}

impl SupportedHtmlElement {
    /// The SOLE element-acceptance authority: classify an intrinsic tag into its
    /// typed [`SupportedHtmlElement`], or `None` for an out-of-allowlist tag (which
    /// the classifier fails closed). The tag match is CASE-SENSITIVE — an HTML tag in
    /// the runtime IR is already the lowercase author-written name, and only the
    /// lowercase forms are accepted.
    #[must_use]
    pub(super) fn try_from(tag: &str) -> Option<Self> {
        match tag {
            "a" => Some(Self::A),
            "button" => Some(Self::Button),
            "div" => Some(Self::Div),
            "h1" => Some(Self::H1),
            "input" => Some(Self::Input),
            "p" => Some(Self::P),
            "video" => Some(Self::Video),
            "textarea" => Some(Self::Textarea),
            "select" => Some(Self::Select),
            "option" => Some(Self::Option),
            "audio" => Some(Self::Audio),
            "details" => Some(Self::Details),
            _ => None,
        }
    }

    /// The DOM local-variable stem for this element (the name the emitter allocates
    /// the clone-root / walk var from — `var <stem> = root();`). EVERY supported
    /// element's stem is a valid, non-reserved JS identifier BY CONSTRUCTION, so the
    /// emitter never synthesizes an invalid `var var = …` / `var class = …` from a
    /// raw tag. This is the ONLY sanctioned source of an element's DOM var stem.
    #[must_use]
    pub(super) fn var_stem(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::Button => "button",
            Self::Div => "div",
            Self::H1 => "h1",
            Self::Input => "input",
            Self::P => "p",
            Self::Video => "video",
            Self::Textarea => "textarea",
            Self::Select => "select",
            Self::Option => "option",
            Self::Audio => "audio",
            Self::Details => "details",
        }
    }
}

/// The FINITE allowlist of static attributes the client core serializes into the
/// `$.from_html` skeleton.
///
/// An accepted [`SupportedStaticAttr`] is a CONTRACT that the attribute serializes
/// directly into the cloned template HTML with no official special-handling (no
/// reactive mirror, no property write, no default-clearing). It deliberately
/// excludes every name the official compiler treats specially in client output
/// (`autofocus` / `muted` / `defaultValue` / `defaultChecked` from
/// `NON_STATIC_PROPERTIES`, `dir`, `style`, input `value` / `checked`, `is`), so an
/// accepted attr can NEVER be the one a serializer would silently drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SupportedStaticAttr {
    /// `id` (global).
    Id,
    /// `title` (global).
    Title,
    /// `role` (global).
    Role,
    /// `class` (global) — ACCEPTED ONLY with a non-empty static value (the official
    /// compiler drops an exactly-empty `class=""`, so an empty class is not a
    /// serializable static attr).
    Class,
    /// A `data-*` custom-data attribute (global).
    Data,
    /// An `aria-*` accessibility attribute (global).
    Aria,
    /// `href` on `<a>`.
    AnchorHref,
    /// `type` on `<button>`.
    ButtonType,
    /// `disabled` on `<button>`.
    ButtonDisabled,
    /// `type` on `<input>`.
    InputType,
    /// `disabled` on `<input>`.
    InputDisabled,
    /// `multiple` on `<select>` — the multi-select boolean (serialized
    /// `multiple=""` into the cloned template, the official `<select multiple>`
    /// form).
    SelectMultiple,
    /// `contenteditable` (global) — the editable-host marker attribute that the
    /// `bind:innerHTML` / `bind:textContent` / `bind:innerText` family rides on
    /// (serialized `contenteditable=""` into the template). It is NOT a DOM
    /// property write — it bakes into the static skeleton like the official
    /// `<div contenteditable>` form.
    ContentEditable,
}

impl SupportedStaticAttr {
    /// The SOLE static-attribute-acceptance authority: classify a static attribute
    /// (`name`, the element's already-accepted [`SupportedHtmlElement`], and its
    /// literal value) into its typed [`SupportedStaticAttr`], or `None` for an
    /// out-of-allowlist attribute (which the classifier fails closed BEFORE emission).
    ///
    /// `value` is `None` for a valueless boolean attribute (`<button disabled>`).
    /// The decision is structural over the typed `(name, element)` pair — never a
    /// `starts_with("Pick<")`-style shape sniff beyond the explicit `data-` / `aria-`
    /// prefix families, which are matched exactly.
    #[must_use]
    pub(super) fn classify(
        name: &str,
        element: SupportedHtmlElement,
        value: Option<&str>,
    ) -> Option<Self> {
        // The global valued attrs allowed on EVERY supported element.
        match name {
            "id" => return Some(Self::Id),
            "title" => return Some(Self::Title),
            "role" => return Some(Self::Role),
            // `contenteditable` is the editable-host marker (any element) the
            // `bind:innerHTML` / `bind:textContent` / `bind:innerText` family rides
            // on. It bakes into the static skeleton (`contenteditable=""`) — it is
            // NOT a DOM-property write. Accepted valueless (`<div contenteditable>`)
            // or with the official `"true"` / `"false"` / `""` string value; a
            // userland enum value still serializes verbatim.
            "contenteditable" => return Some(Self::ContentEditable),
            // `class` is allowed ONLY with a non-empty static value — an
            // exactly-empty `class=""` (or a valueless `class`) is NOT a serializable
            // static attr (the official compiler elides an empty class).
            "class" => {
                return match value {
                    Some(v) if !v.is_empty() => Some(Self::Class),
                    _ => None,
                };
            }
            _ => {}
        }
        // `data-*` / `aria-*` families (a bare `data-` / `aria-` with no suffix is
        // not a real custom-data / aria attribute and is rejected).
        if let Some(rest) = name.strip_prefix("data-") {
            return (!rest.is_empty()).then_some(Self::Data);
        }
        if let Some(rest) = name.strip_prefix("aria-") {
            return (!rest.is_empty()).then_some(Self::Aria);
        }
        // The per-element valued / boolean attrs.
        match (element, name) {
            (SupportedHtmlElement::A, "href") => Some(Self::AnchorHref),
            (SupportedHtmlElement::Button, "type") => Some(Self::ButtonType),
            (SupportedHtmlElement::Button, "disabled") => Some(Self::ButtonDisabled),
            (SupportedHtmlElement::Input, "type") => Some(Self::InputType),
            (SupportedHtmlElement::Input, "disabled") => Some(Self::InputDisabled),
            (SupportedHtmlElement::Select, "multiple") => Some(Self::SelectMultiple),
            _ => None,
        }
    }
}

/// The pinned Svelte `RESERVED_WORDS` set (svelte@5.56.3,
/// `src/utils.js`'s `RESERVED_WORDS`). This is the STRICT reserved-word authority
/// for the client element-name safety check — NOT OXC's narrower `is_keyword` (which
/// omits `arguments` / `eval` / `implements` / `interface` / `package` / `private` /
/// `protected` / `public`). An element tag that IS a Svelte
/// reserved word would synthesize an invalid/reserved JS binding name (`var var = …`)
/// and is routed to the [`ElementName`](super::client::UnsupportedSvelteRuntimeSurface::ElementName)
/// refusal (5v). (Every accepted [`SupportedHtmlElement`] tag is already a non-reserved
/// identifier, so this gate is a precise diagnostic split, not the acceptance gate.)
pub(super) const SVELTE_RESERVED_WORDS: &[&str] = &[
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

/// Whether a tag is a Svelte reserved word (the strict `RESERVED_WORDS` membership).
#[must_use]
pub(super) fn is_svelte_reserved_word(tag: &str) -> bool {
    SVELTE_RESERVED_WORDS.contains(&tag)
}

// ---------------------------------------------------------------------------
// DOM attribute / property tables (mirrored from svelte@5.56.3 `src/utils.js`)
// ---------------------------------------------------------------------------
//
// These are the pinned official tables the client backend uses to decide a
// dynamic attribute's emission shape: a DOM-property write (`node.prop = value`)
// vs `$.set_attribute(node, 'name', value)`. They are TRANSCRIBED VERBATIM from
// `svelte@5.56.3` `src/utils.js` — `DOM_BOOLEAN_ATTRIBUTES`, `ATTRIBUTE_ALIASES`,
// `DOM_PROPERTIES`, and the `normalize_attribute` / `is_dom_property` functions —
// so the property-vs-attribute decision is byte-faithful to the official compiler
// (never a hand-guessed list). The freshness guard
// `dom_property_tables_match_pinned_svelte` keeps them honest.

/// The official `DOM_BOOLEAN_ATTRIBUTES` (svelte@5.56.3 `src/utils.js`): attributes
/// that are present-or-not (boolean). Used as the base of [`DOM_PROPERTIES`].
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

/// The official `ATTRIBUTE_ALIASES` (svelte@5.56.3 `src/utils.js`): attribute names
/// (lowercase) that alias to a differently-cased DOM PROPERTY name because the
/// attribute and the property behave differently. `class` is intentionally absent
/// (handled separately by `$.set_class`).
const ATTRIBUTE_ALIASES: &[(&str, &str)] = &[
    ("formnovalidate", "formNoValidate"),
    ("ismap", "isMap"),
    ("nomodule", "noModule"),
    ("playsinline", "playsInline"),
    ("readonly", "readOnly"),
    ("defaultvalue", "defaultValue"),
    ("defaultchecked", "defaultChecked"),
    ("srcobject", "srcObject"),
    ("novalidate", "noValidate"),
    ("allowfullscreen", "allowFullscreen"),
    ("disablepictureinpicture", "disablePictureInPicture"),
    ("disableremoteplayback", "disableRemotePlayback"),
];

/// The official `DOM_PROPERTIES` extras appended after `DOM_BOOLEAN_ATTRIBUTES`
/// (svelte@5.56.3 `src/utils.js`): the camelCase property aliases plus the
/// non-boolean reflected properties.
const DOM_PROPERTIES_EXTRA: &[&str] = &[
    "formNoValidate",
    "isMap",
    "noModule",
    "playsInline",
    "readOnly",
    "value",
    "volume",
    "defaultValue",
    "defaultChecked",
    "srcObject",
    "noValidate",
    "allowFullscreen",
    "disablePictureInPicture",
    "disableRemotePlayback",
];

/// The official `normalize_attribute` (svelte@5.56.3 `src/utils.js`):
/// `ATTRIBUTE_ALIASES[name.toLowerCase()] ?? name.toLowerCase()`. Lowercases the
/// name and maps a known alias to its camelCase property name (`readonly` →
/// `readOnly`); an un-aliased name returns its lowercase form.
#[must_use]
pub(super) fn normalize_attribute(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for (alias, prop) in ATTRIBUTE_ALIASES {
        if *alias == lower {
            return (*prop).to_string();
        }
    }
    lower
}

/// The official `is_dom_property` (svelte@5.56.3 `src/utils.js`): membership in
/// `DOM_PROPERTIES` = `DOM_BOOLEAN_ATTRIBUTES ∪ DOM_PROPERTIES_EXTRA`. The argument
/// is the ALREADY-[`normalize_attribute`]'d name (the official call site is
/// `is_dom_property(normalize_attribute(name))`), so `readOnly` / `isMap` / … are
/// matched, not their lowercase attribute spellings. A `true` result means the
/// dynamic attribute is emitted as a DOM-PROPERTY write (`node.<name> = value`); a
/// `false` result routes it through `$.set_attribute`.
#[must_use]
pub(super) fn is_dom_property(normalized_name: &str) -> bool {
    DOM_BOOLEAN_ATTRIBUTES.contains(&normalized_name)
        || DOM_PROPERTIES_EXTRA.contains(&normalized_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_accepts_exactly_the_twelve_core_tags() {
        // The SOLE element-acceptance authority accepts EXACTLY the core set (§1.2
        // six, the `video` media host for the `muted` property write, plus the
        // bindings-breadth hosts the 5c DOM-bind family adds:
        // `textarea`/`select`/`option`/`audio`/`details`).
        let expected = [
            "a", "button", "div", "h1", "input", "p", "video", "textarea", "select", "option",
            "audio", "details",
        ];
        let accepted: Vec<&str> = expected
            .into_iter()
            .filter(|t| SupportedHtmlElement::try_from(t).is_some())
            .collect();
        assert_eq!(accepted, expected);
        // A representative spread of out-of-allowlist tags is STILL rejected —
        // including the still-demoted breadth (`span` / `optgroup` / `img` / `slot` /
        // `datalist`) and a reserved-word tag. (`textarea`/`select`/`option` MOVED to
        // the accepted set above — the bindings-breadth allowlist additions.)
        for tag in [
            "span",
            "optgroup",
            "img",
            "slot",
            "datalist",
            "var",
            "class",
            "section",
            "ul",
            "li",
            "svg",
            "my-widget",
        ] {
            assert!(
                SupportedHtmlElement::try_from(tag).is_none(),
                "{tag} must NOT be in the element allowlist"
            );
        }
    }

    #[test]
    fn var_stem_is_a_valid_non_reserved_identifier_for_every_element() {
        // Every supported element's var stem must be a valid, NON-reserved JS
        // identifier (so the emitter never synthesizes `var var = …`).
        for el in [
            SupportedHtmlElement::A,
            SupportedHtmlElement::Button,
            SupportedHtmlElement::Div,
            SupportedHtmlElement::H1,
            SupportedHtmlElement::Input,
            SupportedHtmlElement::P,
            SupportedHtmlElement::Video,
            SupportedHtmlElement::Textarea,
            SupportedHtmlElement::Select,
            SupportedHtmlElement::Option,
            SupportedHtmlElement::Audio,
            SupportedHtmlElement::Details,
        ] {
            let stem = el.var_stem();
            assert!(!stem.is_empty());
            assert!(
                !is_svelte_reserved_word(stem),
                "var stem {stem} must not be a reserved word"
            );
            let mut chars = stem.chars();
            let first = chars.next().unwrap();
            assert!(
                first.is_ascii_alphabetic() || first == '_' || first == '$',
                "var stem {stem} must start with an identifier char"
            );
            assert!(
                chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$'),
                "var stem {stem} must be identifier-safe"
            );
        }
    }

    #[test]
    fn static_attr_allowlist_accepts_globals_and_per_tag_and_rejects_breadth() {
        use SupportedHtmlElement::*;
        // Globals (on any supported element).
        assert_eq!(
            SupportedStaticAttr::classify("id", Div, Some("x")),
            Some(SupportedStaticAttr::Id)
        );
        assert_eq!(
            SupportedStaticAttr::classify("title", P, Some("t")),
            Some(SupportedStaticAttr::Title)
        );
        assert_eq!(
            SupportedStaticAttr::classify("role", Div, Some("button")),
            Some(SupportedStaticAttr::Role)
        );
        assert_eq!(
            SupportedStaticAttr::classify("data-id", Div, Some("5")),
            Some(SupportedStaticAttr::Data)
        );
        assert_eq!(
            SupportedStaticAttr::classify("aria-label", Div, Some("x")),
            Some(SupportedStaticAttr::Aria)
        );
        // `class` ONLY with a non-empty value.
        assert_eq!(
            SupportedStaticAttr::classify("class", Div, Some("box")),
            Some(SupportedStaticAttr::Class)
        );
        assert_eq!(SupportedStaticAttr::classify("class", Div, Some("")), None);
        assert_eq!(SupportedStaticAttr::classify("class", Div, None), None);
        // Per-tag.
        assert_eq!(
            SupportedStaticAttr::classify("href", A, Some("/x")),
            Some(SupportedStaticAttr::AnchorHref)
        );
        assert_eq!(
            SupportedStaticAttr::classify("type", Button, Some("submit")),
            Some(SupportedStaticAttr::ButtonType)
        );
        assert_eq!(
            SupportedStaticAttr::classify("disabled", Button, None),
            Some(SupportedStaticAttr::ButtonDisabled)
        );
        assert_eq!(
            SupportedStaticAttr::classify("type", Input, Some("text")),
            Some(SupportedStaticAttr::InputType)
        );
        assert_eq!(
            SupportedStaticAttr::classify("disabled", Input, None),
            Some(SupportedStaticAttr::InputDisabled)
        );
        // The bindings-breadth static attrs (5c): `<select multiple>` (the
        // multi-select boolean) + `contenteditable` (the editable-host marker, any
        // element) bake into the cloned skeleton.
        assert_eq!(
            SupportedStaticAttr::classify("multiple", Select, None),
            Some(SupportedStaticAttr::SelectMultiple)
        );
        assert_eq!(
            SupportedStaticAttr::classify("contenteditable", Div, None),
            Some(SupportedStaticAttr::ContentEditable)
        );
        assert_eq!(
            SupportedStaticAttr::classify("contenteditable", Div, Some("true")),
            Some(SupportedStaticAttr::ContentEditable)
        );
        // `multiple` is `<select>`-only — it does NOT cross to another element.
        assert_eq!(SupportedStaticAttr::classify("multiple", Div, None), None);
        // Per-tag attrs do NOT cross to the wrong element (`href` only on `<a>`).
        assert_eq!(SupportedStaticAttr::classify("href", Div, Some("/x")), None);
        assert_eq!(
            SupportedStaticAttr::classify("type", Div, Some("text")),
            None
        );
        // The rejected breadth — every forbidden attr fails closed.
        for (name, el, val) in [
            ("is", Button, Some("my-btn")),
            ("defaultValue", Input, Some("x")),
            ("defaultChecked", Input, None),
            ("autofocus", Input, None),
            ("muted", Div, None),
            ("dir", Div, Some("ltr")),
            ("style", Div, Some("color:red")),
            ("value", Input, Some("x")),
            ("checked", Input, None),
            ("loading", Div, Some("lazy")),
            ("selected", Div, None),
            ("data-", Div, Some("x")),
            ("aria-", Div, Some("x")),
            ("onclick", Button, Some("x")),
            ("unknownattr", Div, Some("x")),
        ] {
            assert_eq!(
                SupportedStaticAttr::classify(name, el, val),
                None,
                "{name} on {el:?} must be rejected"
            );
        }
    }

    #[test]
    fn svelte_reserved_words_are_stricter_than_oxc_is_keyword() {
        // The Svelte RESERVED_WORDS include identifiers OXC's `is_keyword` omits — so
        // routing the element-name check through `is_svelte_reserved_word` is strictly
        // more conservative than `is_keyword`: a tag like `<arguments>` / `<eval>` /
        // `<interface>` is reserved under Svelte but NOT an OXC keyword.
        for word in [
            "arguments",
            "eval",
            "implements",
            "interface",
            "package",
            "private",
            "protected",
            "public",
        ] {
            assert!(
                is_svelte_reserved_word(word),
                "{word} must be a Svelte reserved word"
            );
            assert!(
                !crate::utils::oxc::bindings::keywords::is_keyword(word.as_bytes()),
                "{word} must NOT be an OXC keyword (proving RESERVED_WORDS is stricter)"
            );
        }
        // The exact pinned set has the official cardinality (svelte@5.56.3).
        assert_eq!(SVELTE_RESERVED_WORDS.len(), 48);
    }

    #[test]
    fn normalize_attribute_lowercases_and_aliases() {
        // `normalize_attribute` lowercases + maps a known alias to its camelCase
        // property name; an un-aliased name returns its lowercase form (svelte@5.56.3
        // `src/utils.js`).
        assert_eq!(normalize_attribute("readonly"), "readOnly");
        assert_eq!(normalize_attribute("READONLY"), "readOnly");
        assert_eq!(normalize_attribute("formnovalidate"), "formNoValidate");
        assert_eq!(normalize_attribute("ismap"), "isMap");
        assert_eq!(normalize_attribute("playsinline"), "playsInline");
        assert_eq!(normalize_attribute("defaultvalue"), "defaultValue");
        // An un-aliased name lowercases only.
        assert_eq!(normalize_attribute("disabled"), "disabled");
        assert_eq!(normalize_attribute("Hidden"), "hidden");
        assert_eq!(normalize_attribute("contenteditable"), "contenteditable");
        // `class` is intentionally NOT aliased (handled by `$.set_class`).
        assert_eq!(normalize_attribute("class"), "class");
    }

    #[test]
    fn is_dom_property_decides_property_vs_set_attribute() {
        // The pinned `DOM_PROPERTIES` membership (svelte@5.56.3 `src/utils.js`):
        // `DOM_BOOLEAN_ATTRIBUTES ∪` the camelCase aliases + reflected props. The
        // argument is the ALREADY-normalized name (the official call site is
        // `is_dom_property(normalize_attribute(name))`).
        // DOM-boolean attributes are properties.
        for name in [
            "disabled", "checked", "muted", "open", "required", "selected",
        ] {
            assert!(is_dom_property(name), "{name} must be a DOM property");
        }
        // The camelCase aliases (the normalized spelling) are properties.
        for name in [
            "readOnly",
            "isMap",
            "noModule",
            "playsInline",
            "formNoValidate",
        ] {
            assert!(
                is_dom_property(name),
                "{name} (normalized) must be a DOM property"
            );
        }
        // The non-boolean reflected properties.
        for name in [
            "value",
            "volume",
            "defaultValue",
            "defaultChecked",
            "srcObject",
        ] {
            assert!(is_dom_property(name), "{name} must be a DOM property");
        }
        // NOT DOM properties (route through `$.set_attribute`).
        for name in [
            "contenteditable",
            "hidden",
            "id",
            "title",
            "class",
            "data-x",
        ] {
            assert!(
                !is_dom_property(name),
                "{name} must NOT be a DOM property (uses set_attribute)"
            );
        }
        // The full official call site is `is_dom_property(normalize_attribute(name))`:
        // for `readonly`, `normalize_attribute` yields `readOnly` (the camelCase
        // PROPERTY name used in the emitted code), which is also a DOM property. (The
        // lowercase `readonly` is ALSO in the table — it is a DOM-boolean attribute —
        // so the property-vs-attribute decision is true either way; the normalization
        // is what makes the emitted member spelling `node.readOnly`, not the membership.)
        assert!(is_dom_property(&normalize_attribute("readonly")));
        assert_eq!(normalize_attribute("readonly"), "readOnly");
    }

    #[test]
    fn dom_boolean_attributes_has_the_pinned_cardinality() {
        // The committed `DOM_BOOLEAN_ATTRIBUTES` mirrors the pinned svelte@5.56.3 list
        // (28 entries); a desync from the official table fails this gate (and the
        // freshness check below, when node_modules is present).
        assert_eq!(DOM_BOOLEAN_ATTRIBUTES.len(), 28);
        // `DOM_PROPERTIES` = the booleans + 14 extra (camelCase aliases + reflected
        // props). The `is_dom_property` membership covers both halves.
        assert_eq!(DOM_PROPERTIES_EXTRA.len(), 14);
        assert_eq!(ATTRIBUTE_ALIASES.len(), 12);
    }

    // The committed DOM tables are TRANSCRIBED VERBATIM from svelte@5.56.3
    // `src/utils.js`; the live-freshness check against `node_modules` lives in the
    // `tests/` boundary file (`svelte_element_attr_boundary.rs`'s
    // `dom_property_tables_match_pinned_svelte`), keeping `std::fs` out of this src
    // module (the NativeFs invariant). The cardinality + behaviour assertions above
    // catch an accidental in-place edit here.
}
