//! The closed OFFICIAL-REJECT taxonomy: the [`CoreOfficialValidationRule`] enum (the
//! official-error classes the §1.2-core surface must reject) and the [`OfficialRejection`]
//! value (a rule class + the EXACT official `svelte@5.56.3` diagnostic code the violation
//! mirrors). Split out of `official_reject.rs` (the gate logic) as the pure rule/result
//! vocabulary the gate, the reject-corpus matrix, and the parse-parity matrix all consume.

use super::UnsupportedSvelteRuntimeSurface;

/// The closed taxonomy of OFFICIAL-error classes the §1.2 core surface must reject.
///
/// Each variant names ONE official compiler-error class (the canonical official
/// diagnostic code it corresponds to is [`representative_official_code`]); a
/// committed reject corpus row maps to exactly one variant, and every variant has at
/// least one corpus row (the exact-rule-coverage gate). A future official-reject rule
/// lands as a new variant + a corpus row.
///
/// [`representative_official_code`]: Self::representative_official_code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreOfficialValidationRule {
    /// A DUPLICATE top-level `<script>` (a second instance script, or a second module
    /// script) — official `script_duplicate`.
    ScriptDuplicate,
    /// A DUPLICATE top-level `<style>` (a second component-level `<style>`) — official
    /// `style_duplicate`.
    StyleDuplicate,
    /// A `<style>` whose CSS body fails to PARSE — a FAMILY of official CSS parse-phase codes
    /// (`css_expected_identifier`, `css_empty_declaration`, `css_selector_invalid`, plus the
    /// generic `expected_token` / `unexpected_eof` the parser primitives throw). Upstream's
    /// `read_style` parses the CSS body (and can throw) BEFORE `style_duplicate`, so a malformed
    /// 2nd-`<style>` body wins the first-error race over the duplicate. Like
    /// [`ParserStrictness`](Self::ParserStrictness), the EXACT code varies per body and is carried
    /// on the [`OfficialRejection::official_code`] (not a rule-per-code). Driven from the parser's
    /// RESERVED [`StyleBodyProbe`] slot, filled by the faithful `read/style.js` body reader.
    ///
    /// SCOPE: this is the `read_style` PARSE-ENTRY family only; the post-parse CSS validation /
    /// scoping family (`css_global_*`, nesting placement, …) is a deferred CSS-scoping vertical
    /// (`docs/arch/svelte-native-compiler-plan.md` debt ledger), never surfaced by Verter today.
    ///
    /// [`StyleBodyProbe`]: crate::svelte::parser::StyleBodyProbe
    StyleBodyParse,
    /// A `<script>` with an invalid `context` value (anything but `context="module"`)
    /// or a valued `module="x"` attribute — official `script_invalid_context` /
    /// `script_invalid_attribute_value`.
    ScriptInvalidContext,
    /// A `<script>` body that fails to parse — official `js_parse_error`. This is the parse-
    /// phase body slot: upstream's `read_script` runs Acorn on the body before validating the
    /// attributes, so a body syntax error, a same-lexical-scope `let` redeclaration, or TS-only
    /// syntax in a plain (JS) `<script>` is `js_parse_error`. Driven from the parser's RESERVED
    /// [`ScriptBodyProbe`] slot, filled here by parsing the body once with OXC.
    ScriptBodyParse,
    /// A `$` / `$$`-prefixed binding NAME in a declaration position (an identifier
    /// declarator `let $foo`, a `$props()` destructure local `let { a: $foo } =
    /// $props()`, or a bare `let $foo` `bind:this` target) — official
    /// `dollar_prefix_invalid`.
    DollarPrefixInvalid,
    /// A GLOBAL `$foo` (an undeclared lowercase-initial store-style reference) or
    /// `$$foo` reference in a script / template / bind / event position, an undeclared
    /// `bind:this` target, or a reserved magic object (`$$props` / `$$slots` /
    /// `$$restProps`) reference — official `global_reference_invalid` (the magic
    /// objects are auto-injected legacy globals a runes-client reference would leave
    /// undefined).
    GlobalReferenceInvalid,
    /// A child element NESTED inside a same-or-disallowed ancestor that the browser
    /// would REPAIR (a `<button>` in a `<button>`, an `<a>` in an `<a>`, a heading in
    /// a heading) — official `node_invalid_placement`.
    NodeInvalidPlacement,
    /// A block descendant inside a `<p>` that the browser AUTO-CLOSES the `<p>` before,
    /// followed by a SURVIVING EXPLICIT `</p>` close — official
    /// `element_invalid_closing_tag_autoclosed`. (The IMPLICIT case — a `<p>` with a
    /// block child but no explicit `</p>` — is official-ACCEPTED via autoclose, so it
    /// is a deferrable unsupported FEATURE, NOT this reject.)
    ElementInvalidClosingTagAutoclosed,
    /// An intrinsic HTML element left OPEN at end of input (`<button>x` with no
    /// `</button>`) — official `element_unclosed`.
    ElementUnclosed,
    /// A close tag that matched no open element — a stray `</div>` with nothing open,
    /// or a mismatched close that no ancestor closes — official
    /// `element_invalid_closing_tag`.
    ElementInvalidClosingTag,
    /// A VOID element carrying content or an explicit close tag (`<input></input>` /
    /// `<input>x</input>`) — official `void_element_invalid_content`.
    VoidElementInvalidContent,
    /// A `<script>` carrying a RESERVED attribute (`server` / `client` / `worker` /
    /// `test` / `default`) — official `script_reserved_attribute`.
    ScriptReservedAttribute,
    /// A rune called with the wrong arity / form (`$state(0, 1)`, `$props(1)`,
    /// `$derived.by(123)`, …) — official `rune_invalid_arguments` /
    /// `rune_invalid_arguments_length`. (Verter already fails these closed as an
    /// advanced-rune surface; carried here for reject-matrix coverage.)
    RuneInvalidArguments,
    /// A `$props()` destructure in an unsupported pattern (rest, computed, nested,
    /// `$bindable`, a default-bearing member, a duplicate `$props()` call) — official
    /// `props_*` and the related rune errors. (Verter already fails these closed;
    /// carried here for reject-matrix coverage.)
    PropsInvalidPattern,
    /// A DUPLICATE attribute / directive on one element — official `attribute_duplicate`.
    /// (Verter already fails this closed; carried here for reject-matrix coverage.)
    AttributeDuplicate,
    /// An invalid `<svelte:options>` (a duplicate / nested options element, a
    /// non-boolean `runes` value, or an unsupported axis) — official `options_*`.
    /// (Verter already fails these closed; carried here for reject-matrix coverage.)
    OptionsInvalid,
    /// A PARSER-STRICTNESS reject: malformed markup Verter's forgiving/recovery-based
    /// parser silently ACCEPTS but the official STRICT parser REJECTS at the parse phase
    /// (a raw `<` in text, a close tag with a trailing token, an empty attribute value,
    /// a nameless close, an unterminated tag / raw block / quoted value). This is the
    /// ONE broad rule for the whole parser-leniency family — the EXACT official code
    /// varies per recovery point (`tag_invalid_name` / `expected_token` /
    /// `expected_attribute_value` / `element_invalid_closing_tag` / `element_unclosed` /
    /// `unexpected_eof`) and is carried on the [`OfficialRejection::official_code`] of
    /// the refusal, NOT enumerated as a rule-per-code (the runtime contract is "no
    /// `Main`"; the exact code is test metadata). Driven from the typed
    /// [`ParsedSvelte::strict_parse_errors`] fact stream, never a raw-source heuristic.
    ///
    /// [`ParsedSvelte::strict_parse_errors`]: crate::svelte::parser::ParsedSvelte::strict_parse_errors
    ParserStrictness,
    /// A directive value that is a STATIC-TEXT value (`class:on="x"` / `use:foo="bar"`)
    /// rather than a JS expression in curly braces — official `directive_invalid_value`.
    /// Every directive family EXCEPT `style:` requires an expression value; only a
    /// `style:prop="text"` accepts a static-text value (it folds as a quoted string). A
    /// non-style directive whose value is a `Text` chunk (or a multi-chunk mixed value) is
    /// the official compile error.
    DirectiveInvalidValue,
    /// An attribute NAME on an intrinsic element (or `<svelte:element>`) that is not a
    /// valid HTML attribute name — official `attribute_invalid_name`. The official parser
    /// rejects a name whose FIRST character is a digit / `-` / `.`, or that CONTAINS any of
    /// `^ $ @ % & # ? ! | ( ) [ ] { } * + ~ ;`. A COMPONENT takes quoted prop keys, so an
    /// invalid prop name on a component is ACCEPTED (not this rule); a colon name
    /// (`foo:bar`), `data-x`, `aria-label`, and `_foo` are all valid.
    AttributeInvalidName,
}

impl CoreOfficialValidationRule {
    /// The exhaustive list of every rule variant — the exact-rule-coverage gate
    /// asserts every entry has at least one committed reject corpus row. Keep this in
    /// sync with the enum (a new variant must be added here).
    pub const ALL: &'static [CoreOfficialValidationRule] = &[
        CoreOfficialValidationRule::ScriptDuplicate,
        CoreOfficialValidationRule::StyleDuplicate,
        CoreOfficialValidationRule::StyleBodyParse,
        CoreOfficialValidationRule::ScriptInvalidContext,
        CoreOfficialValidationRule::ScriptBodyParse,
        CoreOfficialValidationRule::DollarPrefixInvalid,
        CoreOfficialValidationRule::GlobalReferenceInvalid,
        CoreOfficialValidationRule::NodeInvalidPlacement,
        CoreOfficialValidationRule::ElementInvalidClosingTagAutoclosed,
        CoreOfficialValidationRule::ElementUnclosed,
        CoreOfficialValidationRule::ElementInvalidClosingTag,
        CoreOfficialValidationRule::VoidElementInvalidContent,
        CoreOfficialValidationRule::ScriptReservedAttribute,
        CoreOfficialValidationRule::RuneInvalidArguments,
        CoreOfficialValidationRule::PropsInvalidPattern,
        CoreOfficialValidationRule::AttributeDuplicate,
        CoreOfficialValidationRule::OptionsInvalid,
        CoreOfficialValidationRule::ParserStrictness,
        CoreOfficialValidationRule::DirectiveInvalidValue,
        CoreOfficialValidationRule::AttributeInvalidName,
    ];

    /// The PascalCase rule name as it appears in a reject corpus row's `rule` field.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ScriptDuplicate => "ScriptDuplicate",
            Self::StyleDuplicate => "StyleDuplicate",
            Self::StyleBodyParse => "StyleBodyParse",
            Self::ScriptInvalidContext => "ScriptInvalidContext",
            Self::ScriptBodyParse => "ScriptBodyParse",
            Self::DollarPrefixInvalid => "DollarPrefixInvalid",
            Self::GlobalReferenceInvalid => "GlobalReferenceInvalid",
            Self::NodeInvalidPlacement => "NodeInvalidPlacement",
            Self::ElementInvalidClosingTagAutoclosed => "ElementInvalidClosingTagAutoclosed",
            Self::ElementUnclosed => "ElementUnclosed",
            Self::ElementInvalidClosingTag => "ElementInvalidClosingTag",
            Self::VoidElementInvalidContent => "VoidElementInvalidContent",
            Self::ScriptReservedAttribute => "ScriptReservedAttribute",
            Self::RuneInvalidArguments => "RuneInvalidArguments",
            Self::PropsInvalidPattern => "PropsInvalidPattern",
            Self::AttributeDuplicate => "AttributeDuplicate",
            Self::OptionsInvalid => "OptionsInvalid",
            Self::ParserStrictness => "ParserStrictness",
            Self::DirectiveInvalidValue => "DirectiveInvalidValue",
            Self::AttributeInvalidName => "AttributeInvalidName",
        }
    }

    /// Parse a PascalCase rule name (from a reject corpus row) into its rule, or
    /// `None` for an unrecognised name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.name() == name)
    }

    /// A REPRESENTATIVE official diagnostic code for this rule class — the code the
    /// pinned compiler MOST commonly emits for the class. A single rule can map to
    /// more than one official code (a `ScriptInvalidContext` rule is `script_invalid_context`
    /// for a bad `context` value but `script_invalid_attribute_value` for a valued `module`),
    /// so the freshness gate keys each corpus row on its OWN recorded `official_code`, not this
    /// — this is only the canonical-code documentation hook.
    #[must_use]
    pub fn representative_official_code(self) -> &'static str {
        match self {
            Self::ScriptDuplicate => "script_duplicate",
            Self::StyleDuplicate => "style_duplicate",
            // StyleBodyParse spans a FAMILY of official CSS parse-phase codes; the exact one is
            // carried per-refusal on `OfficialRejection::official_code` and keyed per-fixture in
            // the parse-parity corpus. This is the canonical-code documentation hook only.
            Self::StyleBodyParse => "css_expected_identifier",
            Self::ScriptInvalidContext => "script_invalid_context",
            Self::ScriptBodyParse => "js_parse_error",
            Self::DollarPrefixInvalid => "dollar_prefix_invalid",
            Self::GlobalReferenceInvalid => "global_reference_invalid",
            Self::NodeInvalidPlacement => "node_invalid_placement",
            Self::ElementInvalidClosingTagAutoclosed => "element_invalid_closing_tag_autoclosed",
            Self::ElementUnclosed => "element_unclosed",
            Self::ElementInvalidClosingTag => "element_invalid_closing_tag",
            Self::VoidElementInvalidContent => "void_element_invalid_content",
            Self::ScriptReservedAttribute => "script_reserved_attribute",
            Self::RuneInvalidArguments => "rune_invalid_arguments",
            Self::PropsInvalidPattern => "props_invalid_pattern",
            Self::AttributeDuplicate => "attribute_duplicate",
            Self::OptionsInvalid => "options_invalid",
            // ParserStrictness spans a FAMILY of official parse-phase codes; the exact
            // one is carried per-refusal on `OfficialRejection::official_code` and keyed
            // per-fixture in the parse-parity corpus. This is the canonical-code
            // documentation hook only — a representative member of the family.
            Self::ParserStrictness => "tag_invalid_name",
            Self::DirectiveInvalidValue => "directive_invalid_value",
            Self::AttributeInvalidName => "attribute_invalid_name",
        }
    }

    /// The machine-stable diagnostic id for an official-reject refusal
    /// (`svelte-official-reject-<official_code>`). Distinct from the
    /// `svelte-runtime-unsupported-` family (an unsupported FEATURE) — this is a
    /// MALFORMED-input rejection mirroring an official compile error.
    #[must_use]
    pub fn diagnostic_code(self) -> &'static str {
        match self {
            Self::ScriptDuplicate => "svelte-official-reject-script-duplicate",
            Self::StyleDuplicate => "svelte-official-reject-style-duplicate",
            Self::StyleBodyParse => "svelte-official-reject-style-body-parse",
            Self::ScriptInvalidContext => "svelte-official-reject-script-invalid-context",
            Self::ScriptBodyParse => "svelte-official-reject-script-body-parse",
            Self::DollarPrefixInvalid => "svelte-official-reject-dollar-prefix-invalid",
            Self::GlobalReferenceInvalid => "svelte-official-reject-global-reference-invalid",
            Self::NodeInvalidPlacement => "svelte-official-reject-node-invalid-placement",
            Self::ElementInvalidClosingTagAutoclosed => {
                "svelte-official-reject-element-invalid-closing-tag-autoclosed"
            }
            Self::ElementUnclosed => "svelte-official-reject-element-unclosed",
            Self::ElementInvalidClosingTag => "svelte-official-reject-element-invalid-closing-tag",
            Self::VoidElementInvalidContent => {
                "svelte-official-reject-void-element-invalid-content"
            }
            Self::ScriptReservedAttribute => "svelte-official-reject-script-reserved-attribute",
            Self::RuneInvalidArguments => "svelte-official-reject-rune-invalid-arguments",
            Self::PropsInvalidPattern => "svelte-official-reject-props-invalid-pattern",
            Self::AttributeDuplicate => "svelte-official-reject-attribute-duplicate",
            Self::OptionsInvalid => "svelte-official-reject-options-invalid",
            Self::ParserStrictness => "svelte-official-reject-parser-strictness",
            Self::DirectiveInvalidValue => "svelte-official-reject-directive-invalid-value",
            Self::AttributeInvalidName => "svelte-official-reject-attribute-invalid-name",
        }
    }

    /// A human-readable message naming the official-reject class + the official code
    /// it mirrors.
    #[must_use]
    pub fn message(self) -> String {
        let detail = match self {
            Self::ScriptDuplicate => "a duplicate `<script>` block",
            Self::StyleDuplicate => "a duplicate `<style>` block",
            Self::StyleBodyParse => "a `<style>` whose CSS body fails to parse",
            Self::ScriptInvalidContext => {
                "a `<script>` with an invalid `context` value or a valued `module` attribute"
            }
            Self::ScriptBodyParse => "a `<script>` body that fails to parse",
            Self::DollarPrefixInvalid => "a `$`-prefixed binding name",
            Self::GlobalReferenceInvalid => {
                "a global `$`-prefixed reference (an undeclared store-style `$foo`, a `$$foo`, or a reserved magic object)"
            }
            Self::NodeInvalidPlacement => {
                "an invalid HTML placement (a nested `<a>` / `<button>`, or a heading inside a heading)"
            }
            Self::ElementInvalidClosingTagAutoclosed => {
                "an explicit `</p>` closing a `<p>` the browser already auto-closed"
            }
            Self::ElementUnclosed => "an HTML element left open at end of input",
            Self::ElementInvalidClosingTag => "a close tag that closed no open element",
            Self::VoidElementInvalidContent => {
                "a void element carrying content or a closing tag"
            }
            Self::ScriptReservedAttribute => "a `<script>` with a reserved attribute",
            Self::RuneInvalidArguments => "a rune called with an invalid arity / form",
            Self::PropsInvalidPattern => "a `$props()` destructure in an unsupported pattern",
            Self::AttributeDuplicate => "a duplicate attribute / directive",
            Self::OptionsInvalid => "an invalid `<svelte:options>`",
            Self::ParserStrictness => {
                "malformed markup the official strict parser rejects (a raw `<`, a close \
                 tag with a trailing token, an empty attribute value, a nameless close, \
                 or an unterminated tag / block / value)"
            }
            Self::DirectiveInvalidValue => {
                "a directive with a static-text value (only `style:` accepts a text value; \
                 every other directive requires a JavaScript expression in curly braces)"
            }
            Self::AttributeInvalidName => {
                "an attribute with an invalid name on an intrinsic element (a name starting \
                 with a digit / `-` / `.`, or containing one of `^ $ @ % & # ? ! | ( ) [ ] \
                 { } * + ~ ;`)"
            }
        };
        format!(
            "Svelte client emission rejects {detail} — the official `svelte@5.56.3` compiler \
             also compile-errors it (`{}`).",
            self.representative_official_code()
        )
    }

    /// Map an already-fail-closed [`UnsupportedSvelteRuntimeSurface`] to the
    /// official-reject rule it corresponds to, for the reject-parity matrix. ONLY the
    /// surfaces that genuinely correspond to an official COMPILE-ERROR map to a rule;
    /// a pure "unsupported feature" surface (a `bind:checked`, an `{#if}`, a
    /// `<span>`) returns `None` (it is a deferrable unsupported feature, not an
    /// official reject).
    #[must_use]
    pub fn from_unsupported_surface(surface: &UnsupportedSvelteRuntimeSurface) -> Option<Self> {
        match surface {
            // NOTE: a template `attribute_duplicate` and a duplicate `<svelte:options>`
            // (`svelte_meta_duplicate`) are EXACT-CODE parse errors now minted by the parser
            // and carried by the official-reject gate, NOT mapped from an unsupported surface.
            // `OptionsAxis` here covers only the NON-duplicate unsupported options axes (a
            // nested / non-root placement, child content, a non-runes axis).
            UnsupportedSvelteRuntimeSurface::OptionsAxis { .. } => Some(Self::OptionsInvalid),
            // NOTE: a `MagicIdentifier` surface is NOT auto-mapped — `$$slots` is
            // official-ACCEPTED (a deferrable unsupported feature), while `$$props` /
            // `$$restProps` are official rejects that flow through the official-reject
            // gate (`GlobalReferenceInvalid`) instead, so the surface alone cannot
            // discriminate the reject class.
            _ => None,
        }
    }
}

/// One OFFICIAL-REJECT refusal: the [`CoreOfficialValidationRule`] class plus the EXACT
/// official `svelte@5.56.3` diagnostic code the violation mirrors.
///
/// Most rules map 1:1 to a single official code (their
/// [`CoreOfficialValidationRule::representative_official_code`]); the
/// [`CoreOfficialValidationRule::ParserStrictness`] rule spans a FAMILY of parse-phase
/// codes (`tag_invalid_name` / `expected_token` / …), so the exact code is carried here
/// (from the triggering [`ParsedSvelte::strict_parse_errors`] fact) rather than
/// enumerated as a rule-per-code. The refusal's downstream diagnostic and the
/// parse-parity corpus pin this precise code.
///
/// [`ParsedSvelte::strict_parse_errors`]: crate::svelte::parser::ParsedSvelte::strict_parse_errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficialRejection {
    /// The official-reject rule class.
    pub rule: CoreOfficialValidationRule,
    /// The exact official diagnostic code this refusal mirrors.
    pub official_code: &'static str,
}

impl OfficialRejection {
    /// A rejection whose official code is the rule's representative code (the 1:1 rules).
    #[must_use]
    pub(super) fn of(rule: CoreOfficialValidationRule) -> Self {
        Self {
            rule,
            official_code: rule.representative_official_code(),
        }
    }

    /// A rejection carrying a SITE-SPECIFIC official code for a multi-code rule — the exact
    /// code the detection site mirrors, which may differ from the rule's representative
    /// code (e.g. a `ScriptInvalidContext` rule whose `module="x"` site is the official
    /// `script_invalid_attribute_value`, not the representative `script_invalid_context`).
    #[must_use]
    pub(super) fn with_code(rule: CoreOfficialValidationRule, official_code: &'static str) -> Self {
        Self {
            rule,
            official_code,
        }
    }
}
