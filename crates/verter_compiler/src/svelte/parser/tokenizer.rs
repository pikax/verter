//! The Svelte byte tokenizer + recursive-descent template parser.
//!
//! A single forward byte scan over the component source produces a
//! [`ParsedSvelte`]. The scan is INFALLIBLE: malformed or out-of-scope
//! constructs collect an inline [`SvelteParseDiagnostic`] and the scan
//! continues — the matrix's parse-without-crash contract. Expression interiors
//! are NOT parsed (no type lowering in this crate, per the thin-adapters
//! guard); the parser records their spans and leaves the bytes to the
//! projector.
//!
//! Brace-depth tracking inside expressions is string-aware (single/double/
//! template quotes) so a `}` inside a string literal does not close an
//! interpolation early. Element nesting is tracked so the parser pairs open and
//! close tags and recovers on a mismatch.

use verter_span::Span;

use super::options_custom_element::{CustomElementDescriptor, CustomElementShadow};
use super::template_ast::{
    script_body_grammar_for_source, CloseTagViolation, CloseTagViolationKind,
    OptionsCustomElementProbe, OptionsCustomElementTextTag, ParsedSvelte, ScriptBodyGrammar,
    ScriptBodyProbe, StyleBodyProbe, SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue,
    SvelteBlock, SvelteBlockClause, SvelteBlockKind, SvelteClauseKind, SvelteDirective,
    SvelteDirectiveKind, SvelteElement, SvelteElementKind, SvelteNode, SvelteParseDiagnostic,
    SvelteParseRejectFact, SvelteParseRejectKind, SvelteScript, SvelteSpecialKind,
    SvelteStrictParseError, SvelteStrictParseErrorKind, SvelteStyle, SvelteTag, SvelteTagKind,
};
use super::tokenizer_scan::{
    classify_element, declaration_tag_kind, duplicate_attribute_key, find_matching_brace_in,
    is_tag_name_byte, is_void_element, nonempty_span, paragraph_autocloses_on_block_child,
    root_only_meta_tag_name, DuplicateKeyClass,
};

/// Parse Svelte component `source` into a [`ParsedSvelte`].
#[must_use]
pub fn parse_svelte(source: &str) -> ParsedSvelte {
    let mut parser = SvelteParser::new(source);
    parser.parse_root();
    parser.finish()
}

/// The well-formedness classification of a close tag's boundary (the bytes after `</`),
/// produced by [`SvelteParser::classify_close_boundary`]. The malformed variants each map
/// to a distinct official parse-phase code so every close-tag boundary fails closed with
/// the exact code the pinned `svelte@5.56.3` compiler emits.
enum CloseBoundary {
    /// A well-formed close: a name followed by optional whitespace then `>`. The caller
    /// reads the name separately (via `close_tag_name_bytes`) and applies match / ancestor
    /// / stray / void rules.
    Clean,
    /// `</>` — a genuine nameless close (official `element_invalid_closing_tag`).
    Nameless,
    /// `</ name>` (whitespace before the name) OR a name with a trailing token
    /// (`</div x>`, `</div/>`) OR EOF before `>` (`</div`) — official `expected_token`.
    ExpectedToken,
    /// `</` at EOF — official `unexpected_eof`.
    UnexpectedEof,
}

/// A consumed close tag (the result of [`SvelteParser::consume_and_classify_close`]): the
/// close span, the close NAME (independent of boundary well-formedness), and whether the
/// boundary was WELL-FORMED (a clean close), so the caller can recognise a matching close
/// even when its boundary is malformed.
struct ConsumedClose {
    span: Span,
    name: Option<String>,
    clean: bool,
}

/// The forward byte parser state.
pub(super) struct SvelteParser<'a> {
    src: &'a [u8],
    text: &'a str,
    pos: usize,
    instance_script: Option<SvelteScript>,
    module_script: Option<SvelteScript>,
    styles: Vec<SvelteStyle>,
    template: Vec<SvelteNode>,
    diagnostics: Vec<SvelteParseDiagnostic>,
    /// The CLOSE-TAG well-formedness violations observed during the walk (an element
    /// open at EOF, a stray / mismatched close, or a void element with content / an
    /// explicit close). Recorded faithfully at the recovery points so the
    /// official-reject gate can fail closed.
    close_tag_violations: Vec<CloseTagViolation>,
    /// The STRICT-PARSE errors observed during the walk — markup Verter's forgiving
    /// parser recovers from but the official STRICT parser rejects. Recorded faithfully
    /// at each recovery point through `record_strict_parse_error` so the official-reject
    /// gate can fail closed (no `Main`) instead of accepting a divergent module.
    pub(super) strict_parse_errors: Vec<SvelteStrictParseError>,
    /// The PARSE-DOMAIN official-reject facts observed during the walk — the `<script>`
    /// attribute / duplicate-script rejects, the template duplicate attribute /
    /// duplicate-`<svelte:options>`, and the explicit-`</p>` autoclose. Recorded at the
    /// discovery point through `record_parse_reject` so the official-reject gate arbitrates
    /// them by `encounter_order` against the close-tag and strict-parse rails.
    parse_reject_facts: Vec<SvelteParseRejectFact>,
    /// The RESERVED script-body-parse slots — one per `<script>` block with a body, each
    /// carrying an `encounter_order` minted at the upstream-faithful body-parse position
    /// (after the open-tag attribute-duplicate, before the source-order semantic-attr
    /// validation). The parser reserves the slot; the official-reject gate fills it.
    script_body_probes: Vec<ScriptBodyProbe>,
    /// The RESERVED style-body-parse slots — one per top-level `<style>` block, each carrying
    /// an `encounter_order` minted at the upstream `read_style` body-parse position (BEFORE the
    /// `style_duplicate` check) + the CSS body content-start. The parser reserves the slot; the
    /// official-reject gate fills it by running a faithful CSS-body reader.
    style_body_probes: Vec<StyleBodyProbe>,
    /// The RESERVED `<svelte:options customElement={EXPR}>` validation slots — reserved by the
    /// `read_options` finalization at the options-attribute source-order position. The parser
    /// reserves the slot; the official-reject gate fills it with OXC.
    options_custom_element_probes: Vec<OptionsCustomElementProbe>,
    /// The RETAINED string-tag `customElement="my-el"` descriptors — resolved at the
    /// `read_options` finalization position when the text tag VALIDATES, keyed by the
    /// attribute's text-value span. The runtime lowering consumes only these (never a raw
    /// source re-slice); a rejected text tag mints its fact directly and retains nothing.
    options_custom_element_text_tags: Vec<OptionsCustomElementTextTag>,
    /// The walk-time encounter orders for the first root `<svelte:options>` element's
    /// expression-valued `customElement` attributes' PARSE positions, keyed by each attribute's
    /// expression span — one order PER attribute, drawn during the forward pass at THAT
    /// attribute's position in the open-tag loop (where upstream's `read_expression` parses the
    /// value). `read_options_finalize` reads the matching span's order when resolving the probe,
    /// so a malformed-expression `js_parse_error` / `expected_token` rides its OWN attribute's
    /// source position (beating a later template defect / duplicate, losing to an earlier one),
    /// distinct from the finalization position the VALIDATION fault rides. Keyed per-expression
    /// (not a single first-occurrence latch) so a duplicate `customElement`'s fault competes at
    /// its own position and an `attribute_duplicate` minted between two occurrences wins.
    options_ce_attr_parse_orders: Vec<(Span, u32)>,
    /// The root-only `<svelte:*>` meta tag names already encountered at the component root
    /// (`svelte:options` / `svelte:head` / `svelte:window` / `svelte:document` /
    /// `svelte:body`). A SECOND occurrence of the same name mints `svelte_meta_duplicate`,
    /// mirroring upstream's `parser.meta_tags` set in `element.js`.
    root_meta_tags_seen: Vec<&'static str>,
    /// The names of the elements whose children are CURRENTLY being scanned (the open
    /// ancestor stack, innermost last). A foreign close tag that matches a name in
    /// this stack closes an ANCESTOR (an implicitly-closed intervening element, which
    /// official accepts); a close matching nothing here is a stray / mismatched close.
    open_stack: Vec<String>,
    /// The monotonic discovery counter shared by BOTH parse-defect rails (close-tag
    /// violations and strict-parse errors). Each defect draws the NEXT value when it is
    /// PROVEN/recorded during the single forward pass, so the two rails share one global
    /// discovery order — the official-reject gate arbitrates competing defects by minimum
    /// `encounter_order`, matching official (which stops at the first parse error).
    next_defect_seq: u32,
    /// The SINGLE parser-wide script-body grammar, computed ONCE from the whole source at
    /// construction (the first lowercase `<script … lang="ts">` with an exact `ts` value) —
    /// mirroring upstream's one `parser.ts` flag set in the `Parser` constructor. EVERY
    /// `<script>` body probe uses this grammar, so a plain script + a later `lang="ts"` script
    /// parses the whole component as TS, and `lang="TS"` / `lang="tsx"` / `lang="typescript"`
    /// (not the exact `ts` value) stays JS. NOT a per-script `script.lang` choice.
    script_body_grammar: ScriptBodyGrammar,
}

impl<'a> SvelteParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            src: source.as_bytes(),
            text: source,
            pos: 0,
            instance_script: None,
            module_script: None,
            styles: Vec::new(),
            template: Vec::new(),
            diagnostics: Vec::new(),
            close_tag_violations: Vec::new(),
            strict_parse_errors: Vec::new(),
            parse_reject_facts: Vec::new(),
            script_body_probes: Vec::new(),
            style_body_probes: Vec::new(),
            options_custom_element_probes: Vec::new(),
            options_custom_element_text_tags: Vec::new(),
            options_ce_attr_parse_orders: Vec::new(),
            root_meta_tags_seen: Vec::new(),
            open_stack: Vec::new(),
            next_defect_seq: 0,
            // Compute the parser-wide TS flag ONCE over the whole source, exactly as upstream's
            // `Parser` constructor does (the first lowercase `<script … lang="ts">` scan).
            script_body_grammar: script_body_grammar_for_source(source),
        }
    }

    /// Draw the NEXT monotonic discovery sequence for a parse defect (a close-tag
    /// violation or a strict-parse error). `pub(super)` so the sibling `strict_facts`
    /// module's `impl SvelteParser` can draw from the SAME counter — the two rails share
    /// one global discovery order so the official-reject gate's minimum-`encounter_order`
    /// arbitration sees a single forward-pass sequence across both streams.
    pub(super) fn next_defect_seq(&mut self) -> u32 {
        let seq = self.next_defect_seq;
        self.next_defect_seq += 1;
        seq
    }

    fn finish(mut self) -> ParsedSvelte {
        // Order BOTH parse-defect rails by their monotonic `encounter_order` (the single
        // forward-pass DISCOVERY order — NOT source position, since an `Unclosed` is
        // proven at EOF yet anchors at its earlier open tag). The values are pushed in
        // discovery order already, so these sorts are stable identities; they are kept
        // explicit so the gate's `.first()`-is-earliest-discovered invariant is visible.
        self.close_tag_violations.sort_by_key(|v| v.encounter_order);
        self.strict_parse_errors.sort_by_key(|e| e.encounter_order);
        self.parse_reject_facts.sort_by_key(|f| f.encounter_order);
        self.script_body_probes.sort_by_key(|p| p.encounter_order);
        self.style_body_probes.sort_by_key(|p| p.encounter_order);
        self.options_custom_element_probes
            .sort_by_key(|p| p.encounter_order);
        ParsedSvelte {
            instance_script: self.instance_script,
            module_script: self.module_script,
            styles: self.styles,
            template: self.template,
            diagnostics: self.diagnostics,
            close_tag_violations: self.close_tag_violations,
            strict_parse_errors: self.strict_parse_errors,
            parse_reject_facts: self.parse_reject_facts,
            script_body_probes: self.script_body_probes,
            style_body_probes: self.style_body_probes,
            options_custom_element_probes: self.options_custom_element_probes,
            options_custom_element_text_tags: self.options_custom_element_text_tags,
        }
    }

    /// Record a close-tag well-formedness violation. The `encounter_order` is drawn from
    /// the shared monotonic defect counter at the moment the violation is PROVEN (for an
    /// `Unclosed`, the EOF/unwind point), so it shares one discovery order with the
    /// strict-parse-error rail.
    fn record_close_violation(&mut self, kind: CloseTagViolationKind, tag: &str, span: Span) {
        let encounter_order = self.next_defect_seq();
        self.close_tag_violations.push(CloseTagViolation {
            kind,
            tag: tag.to_ascii_lowercase(),
            span,
            encounter_order,
        });
    }

    /// Record a PARSE-DOMAIN official-reject fact (a `<script>` attribute / duplicate-script
    /// reject, or an explicit-`</p>` autoclose) — the SINGLE sink the script-domain and
    /// autoclose mint sites route through. The `encounter_order` is drawn from the SAME
    /// shared defect counter as the close-tag and strict-parse rails at the moment the
    /// defect is discovered, so all three rails share one global forward-pass discovery
    /// order the official-reject gate arbitrates by minimum `encounter_order`.
    fn record_parse_reject(
        &mut self,
        kind: SvelteParseRejectKind,
        official_code: &'static str,
        span: Span,
    ) {
        let encounter_order = self.next_defect_seq();
        self.parse_reject_facts.push(SvelteParseRejectFact {
            kind,
            official_code,
            span,
            encounter_order,
        });
    }

    /// Mint the PARSE-DOMAIN script-parse facts for one `<script>` open tag the instant it is
    /// parsed, in the UPSTREAM encounter order (`element.js` + `read_script`):
    ///
    /// 1. the script-BODY parse slot — `read_script` runs Acorn on the body BEFORE validating
    ///    the reserved/context/module attributes, so a RESERVED encounter slot is allocated
    ///    here (the gate fills it: a body parse failure becomes `js_parse_error` at this
    ///    reserved order, strictly AFTER the open-tag attribute-duplicate and BEFORE the
    ///    source-order semantic-attribute faults);
    /// 2. the FIRST source-order reserved/context/module semantic-attribute fault — official
    ///    validates the attributes in source order AFTER the body parse.
    ///
    /// The open-tag `attribute_duplicate` is minted EARLIER, during `parse_open_tag_attributes`
    /// (the SINGLE open-tag attribute loop shared by every tag, including `<script>`), so it
    /// already precedes this script's body-probe in encounter order — matching official, which
    /// throws `attribute_duplicate` in the open-tag loop before `read_script`. The
    /// `script_duplicate` fact (a second instance / module script) is minted by the CALLER
    /// AFTER this method returns, so it lands LAST in this script's encounter order — matching
    /// official, which throws `script_duplicate` only after `read_script` returns a clean body
    /// and clean attributes. Each step draws the next monotonic `encounter_order`, so the
    /// per-script order is strictly increasing.
    fn record_script_parse_facts(&mut self, script: &SvelteScript) {
        // (1) reserve the body-parse slot at the upstream body-parse position. The grammar is
        // the SINGLE parser-wide flag (`self.script_body_grammar`), computed once at construction
        // from the first lowercase `<script … lang="ts">`, NOT this script's own `lang` — exactly
        // as upstream parses EVERY `<script>` body with the one `parser.ts` flag. So a plain
        // script using TS-only syntax is `js_parse_error` UNLESS some `<script lang="ts">` flipped
        // the whole parse to TS; an uppercase / `tsx` / `typescript` `lang` (not the exact `ts`
        // value) leaves the grammar JS. Only a script with an actual body span gets a probe.
        if let Some(body_span) = script.content {
            self.record_body_probe(body_span, self.script_body_grammar);
        }
        // (2) the FIRST source-order reserved/context/module semantic-attribute fault, with
        // its exact official code (the `ScriptInvalidContext` kind carries the exact code:
        // `script_invalid_context` for context, `script_invalid_attribute_value` for a valued
        // module, `script_reserved_attribute` for a reserved name).
        if let Some((official_code, span)) = script.first_semantic_attribute_fault(self.text) {
            let kind = match official_code {
                "script_reserved_attribute" => SvelteParseRejectKind::ScriptReservedAttribute,
                _ => SvelteParseRejectKind::ScriptInvalidContext,
            };
            self.record_parse_reject(kind, official_code, span);
        }
    }

    /// Reserve a script-body-parse slot at the current discovery point: draw the next
    /// monotonic `encounter_order` and record a [`ScriptBodyProbe`] carrying it, the body
    /// `Span`, and the grammar. The parser does NOT parse the body — the official-reject gate
    /// fills the slot (a body parse failure → `js_parse_error` at the reserved order).
    fn record_body_probe(&mut self, body_span: Span, grammar: ScriptBodyGrammar) {
        let encounter_order = self.next_defect_seq();
        self.script_body_probes.push(ScriptBodyProbe {
            encounter_order,
            body_span,
            grammar,
        });
    }

    /// Reserve a CSS style-body-parse slot at the current discovery point (the upstream
    /// `read_style` position, before the duplicate-style check): draw the next monotonic
    /// `encounter_order` and record a [`StyleBodyProbe`] carrying it and the CSS body's
    /// content-start offset. The parser does NOT parse the CSS — the official-reject gate fills
    /// the slot with a faithful `read/style.js` body reader (a CSS body parse failure → the
    /// exact CSS parse code at the reserved order).
    fn record_style_body_probe(&mut self, content_start: u32) {
        let encounter_order = self.next_defect_seq();
        self.style_body_probes.push(StyleBodyProbe {
            encounter_order,
            content_start,
        });
    }

    /// Record one element's tag name into the root-only `<svelte:*>` meta-tag tracker, minting
    /// the official parse errors for a root-only meta tag (`svelte:options` / `svelte:head` /
    /// `svelte:window` / `svelte:document` / `svelte:body`), mirroring upstream's `element.js`
    /// order EXACTLY:
    ///
    /// 1. a SECOND occurrence of the same root-only meta name → `svelte_meta_duplicate`
    ///    (checked first, so it wins over a placement defect on the same tag);
    /// 2. else a NON-root (`at_root == false`) occurrence → `svelte_meta_invalid_placement`.
    ///
    /// The name is recorded into the seen-set UNCONDITIONALLY (upstream sets
    /// `parser.meta_tags[name] = true` regardless), so a later occurrence still duplicates. A
    /// non-meta / non-root-only tag is ignored. `open_span` anchors the reject at the open tag.
    fn note_root_meta_tag(&mut self, name: &str, at_root: bool, open_span: Span) {
        let Some(meta) = root_only_meta_tag_name(name) else {
            return;
        };
        if self.root_meta_tags_seen.contains(&meta) {
            self.record_parse_reject(
                SvelteParseRejectKind::SvelteMetaDuplicate,
                "svelte_meta_duplicate",
                open_span,
            );
        } else if !at_root {
            self.record_parse_reject(
                SvelteParseRejectKind::SvelteMetaInvalidPlacement,
                "svelte_meta_invalid_placement",
                open_span,
            );
        }
        self.root_meta_tags_seen.push(meta);
    }

    /// The parser-FINALIZATION `read_options` + `disallow_children` equivalent: mirror upstream's
    /// `Parser` constructor, which (after the root walk) finds the FIRST root `<svelte:options>`,
    /// validates its attributes in SOURCE ORDER, then disallows its children.
    ///
    /// Mints exact-code `OptionsInvalid` parse facts for the official `read_options` /
    /// `disallow_children` rejects:
    /// - a spread / directive (a non-`Attribute`) → `svelte_options_invalid_attribute`;
    /// - a boolean axis (`runes` / `immutable` / `preserveWhitespace` / `accessors`) with a
    ///   non-boolean value → `svelte_options_invalid_attribute_value`;
    /// - `tag` → `svelte_options_deprecated_tag` (always);
    /// - `customElement` boolean-shorthand → `svelte_options_invalid_customelement`; a Text value
    ///   → `validate_custom_element_tag` (`svelte_options_invalid_tagname` /
    ///   `svelte_options_reserved_tagname`), and an ACCEPTED text tag retains its RESOLVED
    ///   descriptor as an [`OptionsCustomElementTextTag`] (the runtime lowering consumes only the
    ///   retained descriptor, never a raw-source re-slice); an EXPRESSION value → a RESOLVED
    ///   [`OptionsCustomElementProbe`] (the parser runs the one validate+extract engine HERE and
    ///   retains the typed outcome; the gate mints the retained reject code at the reserved
    ///   orders, the runtime lowering consumes the retained accepted value);
    /// - `namespace` not in {`html`, `svg`, `mathml`} → `svelte_options_invalid_attribute_value`;
    /// - `css` not `injected` → `svelte_options_invalid_attribute_value`;
    /// - any OTHER name → `svelte_options_unknown_attribute`;
    /// - and, when NO attribute faults, child content → `svelte_meta_invalid_content`.
    ///
    /// Each fault / probe draws the next monotonic `encounter_order` in attribute SOURCE ORDER
    /// (all > every root-walk fact, since this runs after the walk), so the minimum-order fault
    /// wins — faithful to upstream throwing on the FIRST faulting attribute, then
    /// `disallow_children`. A DIRECTLY-classifiable fault STOPS the scan (upstream's throw); an
    /// EXPRESSION-valued `customElement` resolves its probe and CONTINUES (a retained reject only
    /// mints once the gate arbitrates it, so a later attribute may still fault first). An
    /// ACCEPTED axis mints NOTHING — a valid `customElement` value (Text tag / `null` / a
    /// conforming object) is LOWERED by the native client path via the retained descriptor, and
    /// the other accepted axes (a valid `namespace="svg"`, a boolean `runes={true}`, …) are
    /// refused later as unsupported features, never an official reject.
    fn read_options_finalize(&mut self) {
        // Upstream `findIndex` over the ROOT fragment nodes for the first `SvelteOptions`. Only a
        // ROOT-level options participates (a nested one is `svelte_meta_invalid_placement`, minted
        // during the walk). Extract the attributes + child-presence so the immutable borrow of
        // `self.template` ends before minting facts.
        let Some((attributes, has_children)) = self.first_root_options_element() else {
            return;
        };

        // Walk the attributes in SOURCE ORDER, mirroring upstream's `for (const attribute …)`.
        for attr in &attributes {
            match &attr.kind {
                // A spread / directive / `{@attach}` is not an `Attribute` →
                // `svelte_options_invalid_attribute`.
                SvelteAttributeKind::Spread(_)
                | SvelteAttributeKind::Directive(_)
                | SvelteAttributeKind::Attach { .. } => {
                    self.record_options_invalid("svelte_options_invalid_attribute", attr.span);
                    return;
                }
                SvelteAttributeKind::Plain { name, value, .. } => {
                    match name.as_str() {
                        "runes" | "immutable" | "preserveWhitespace" | "accessors" => {
                            if !options_value_is_boolean(self.text, value) {
                                self.record_options_invalid(
                                    "svelte_options_invalid_attribute_value",
                                    attr.span,
                                );
                                return;
                            }
                            // a valid boolean axis — accepted (refused later as a feature).
                        }
                        "tag" => {
                            // `tag` is ALWAYS the deprecated-tag hard error.
                            self.record_options_invalid("svelte_options_deprecated_tag", attr.span);
                            return;
                        }
                        "customElement" => {
                            match value {
                                // Boolean shorthand (`value === true`) → invalid customElement.
                                None => {
                                    self.record_options_invalid(
                                        "svelte_options_invalid_customelement",
                                        attr.span,
                                    );
                                    return;
                                }
                                // A Text value → `validate_tag` on the literal string.
                                Some(SvelteAttributeValue::Text(span)) => {
                                    let tag = &self.text[span.start as usize..span.end as usize];
                                    if let Some(code) =
                                        super::template_ast::validate_custom_element_tag(Some(tag))
                                    {
                                        self.record_options_invalid(code, attr.span);
                                        return;
                                    }
                                    // A VALID Text custom-element tag — accepted: resolve the
                                    // descriptor ONCE here (the string-tag form's fixed axes)
                                    // and RETAIN it, keyed by the text span, exactly as the
                                    // expression arm retains its probe resolution. The runtime
                                    // lowering consumes ONLY this retained descriptor — it
                                    // never re-slices the tag from raw source.
                                    self.options_custom_element_text_tags.push(
                                        OptionsCustomElementTextTag {
                                            text_span: *span,
                                            descriptor: CustomElementDescriptor {
                                                tag: Some(tag.to_string()),
                                                shadow: CustomElementShadow::Open,
                                                props: Vec::new(),
                                                extend: None,
                                                inject_styles: true,
                                            },
                                        },
                                    );
                                }
                                // An EXPRESSION value → resolve the probe HERE (the one
                                // validate+extract engine; the typed outcome is retained on
                                // the slot for the gate + the lowering).
                                Some(SvelteAttributeValue::Expression(span)) => {
                                    self.record_options_custom_element_probe(*span);
                                    // CONTINUE: a retained reject mints only when the gate
                                    // arbitrates it, so a later attribute may still fault.
                                }
                                // A MIXED value (text + interpolation runs) is a multi-chunk
                                // value, so `get_static_value` is `null`. Upstream branches on
                                // `value[0].type === 'Text'`: a Mixed value whose FIRST chunk is
                                // TEXT takes the Text path → `validate_tag(null)` →
                                // `svelte_options_invalid_tagname`; one whose first chunk is an
                                // EXPRESSION (`customElement="{x}…"`) takes the expression path →
                                // `svelte_options_invalid_customelement`. The first chunk is Text
                                // iff the value span does not START with `{` (the Mixed span
                                // excludes the quotes).
                                Some(SvelteAttributeValue::Mixed(span)) => {
                                    let first_is_text = self
                                        .src
                                        .get(span.start as usize)
                                        .is_some_and(|&b| b != b'{');
                                    let code = if first_is_text {
                                        "svelte_options_invalid_tagname"
                                    } else {
                                        "svelte_options_invalid_customelement"
                                    };
                                    self.record_options_invalid(code, attr.span);
                                    return;
                                }
                            }
                        }
                        "namespace" => {
                            if !options_namespace_is_valid(self.text, value) {
                                self.record_options_invalid(
                                    "svelte_options_invalid_attribute_value",
                                    attr.span,
                                );
                                return;
                            }
                        }
                        "css" => {
                            if !options_css_is_injected(self.text, value) {
                                self.record_options_invalid(
                                    "svelte_options_invalid_attribute_value",
                                    attr.span,
                                );
                                return;
                            }
                        }
                        _ => {
                            self.record_options_invalid(
                                "svelte_options_unknown_attribute",
                                attr.span,
                            );
                            return;
                        }
                    }
                }
            }
        }

        // No directly-classifiable attribute fault — `disallow_children` runs LAST (a higher
        // encounter order than any reserved `customElement` probe, so a `customElement` fault
        // still wins). Child content on the options element is `svelte_meta_invalid_content`.
        if has_children {
            if let Some(span) = self.first_root_options_open_span() {
                self.record_options_invalid("svelte_meta_invalid_content", span);
            }
        }
    }

    /// The FIRST ROOT-level `<svelte:options>` element's attributes (cloned) + whether it has
    /// child content, or `None` when there is no root options element. (Upstream's `findIndex`
    /// over the root fragment — root-level only.)
    fn first_root_options_element(&self) -> Option<(Vec<SvelteAttribute>, bool)> {
        self.template.iter().find_map(|node| match node {
            SvelteNode::Element(el)
                if matches!(
                    el.kind,
                    SvelteElementKind::Special(SvelteSpecialKind::Options)
                ) =>
            {
                Some((el.attributes.clone(), !el.children.is_empty()))
            }
            _ => None,
        })
    }

    /// The FIRST ROOT-level `<svelte:options>` element's open-tag span (the report anchor for the
    /// child-content fact).
    fn first_root_options_open_span(&self) -> Option<Span> {
        self.template.iter().find_map(|node| match node {
            SvelteNode::Element(el)
                if matches!(
                    el.kind,
                    SvelteElementKind::Special(SvelteSpecialKind::Options)
                ) =>
            {
                Some(el.open_span)
            }
            _ => None,
        })
    }

    /// Record an `OptionsInvalid` parse fact carrying its exact site code, drawing the next
    /// monotonic `encounter_order` (the options-finalization position, after every walk fact).
    fn record_options_invalid(&mut self, official_code: &'static str, span: Span) {
        self.record_parse_reject(SvelteParseRejectKind::OptionsInvalid, official_code, span);
    }

    /// RESOLVE an `<svelte:options customElement={EXPR}>` validation slot at the options-
    /// finalization discovery point — the position upstream's `read_options` runs. The expression
    /// is parsed ONCE here through the one validate+extract engine
    /// (`resolve_custom_element_expr`) and the typed outcome is RETAINED on the probe (exactly as
    /// upstream retains `AST.SvelteOptions['customElement']`): the exact official reject code on
    /// the `Err` side, the accepted typed value on the `Ok` side. The slot still draws BOTH
    /// arbitration orders — the next monotonic `encounter_order` (for a `svelte_options_*`
    /// VALIDATION fault) paired with the walk-time `parse_encounter_order` stashed for THIS
    /// attribute's expression span when the element was parsed (for an attribute-expression
    /// `js_parse_error` / `expected_token`) — and the official-reject gate mints the retained
    /// code at the correct one. A missing stash (no walk-time order was drawn for this span)
    /// falls back to the finalization order — the conservative same-position default.
    fn record_options_custom_element_probe(&mut self, expr_span: Span) {
        let encounter_order = self.next_defect_seq();
        let parse_encounter_order = self
            .options_ce_attr_parse_orders
            .iter()
            .find(|(span, _)| *span == expr_span)
            .map(|&(_, order)| order)
            .unwrap_or(encounter_order);
        let expr_src = &self.text[expr_span.start as usize..expr_span.end as usize];
        let resolution = super::options_custom_element::resolve_custom_element_expr(expr_src);
        self.options_custom_element_probes
            .push(OptionsCustomElementProbe {
                encounter_order,
                parse_encounter_order,
                expr_span,
                resolution,
            });
    }

    /// Reserve the attribute-expression PARSE encounter order for a `customElement={EXPR}` attribute
    /// AT its discovery point in the open-tag attribute loop — drawn ONLY when the caller has
    /// determined this open tag is the FIRST root `<svelte:options>` (the `draw_…` gate) AND the
    /// attribute is `customElement` with an EXPRESSION value. The order is drawn HERE (interleaved
    /// with the per-attribute duplicate tracker) so the customElement parse fault competes with
    /// later same-tag attribute defects by source position — beating a LATER duplicate `foo foo`
    /// and losing to an EARLIER one — exactly as upstream's during-loop `read_expression` does.
    /// EVERY expression-valued occurrence draws its OWN order, keyed by its expression span: a
    /// duplicate `customElement={EXPR}` attribute's parse fault competes at the DUPLICATE's
    /// position (after every earlier attribute's duplicate mint, before its own), never riding
    /// the first occurrence's earlier position.
    fn note_options_ce_attr_parse_order(&mut self, attr: &SvelteAttribute) {
        let SvelteAttributeKind::Plain {
            name,
            value: Some(SvelteAttributeValue::Expression(expr_span)),
            ..
        } = &attr.kind
        else {
            return;
        };
        if name == "customElement" {
            let order = self.next_defect_seq();
            self.options_ce_attr_parse_orders.push((*expr_span, order));
        }
    }

    /// Whether the just-parsed `children` of a `<p>` contain a DIRECT disallowed block
    /// child that auto-closes the `<p>` (the official `<p>` autoclosing-children set,
    /// restricted to real intrinsic HTML elements — a component / `<svelte:*>` / custom
    /// element never auto-closes a `<p>`). Drives the explicit-`</p>` autoclose reject mint.
    fn paragraph_has_direct_autoclose_child(children: &[SvelteNode]) -> bool {
        children.iter().any(|child| {
            if let SvelteNode::Element(c) = child {
                matches!(c.kind, SvelteElementKind::Intrinsic)
                    && !c.name.contains('-')
                    && paragraph_autocloses_on_block_child(&c.name.to_ascii_lowercase())
            } else {
                false
            }
        })
    }

    fn len(&self) -> usize {
        self.src.len()
    }

    fn at(&self, pos: usize) -> u8 {
        self.src.get(pos).copied().unwrap_or(0)
    }

    fn cur(&self) -> u8 {
        self.at(self.pos)
    }

    fn eof(&self) -> bool {
        self.pos >= self.len()
    }

    fn slice(&self, span: Span) -> &'a str {
        let (s, e) = (span.start as usize, span.end as usize);
        self.text.get(s..e).unwrap_or("")
    }

    fn diag(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.diagnostics.push(SvelteParseDiagnostic {
            code,
            message: message.into(),
            span,
        });
    }

    /// Whether the source from `pos` begins with `needle` (ASCII, case-sensitive).
    fn starts_with_at(&self, pos: usize, needle: &[u8]) -> bool {
        self.src
            .get(pos..pos + needle.len())
            .is_some_and(|s| s == needle)
    }

    /// Whether the source from `pos` begins with `needle`, ASCII-case-insensitive.
    fn starts_with_ci_at(&self, pos: usize, needle: &[u8]) -> bool {
        self.src
            .get(pos..pos + needle.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(needle))
    }

    // ── Root scan ──────────────────────────────────────────────────────

    /// Scan the top-level component body: text, comments, `<script>` /
    /// `<style>` blocks, elements, and template tags/blocks.
    fn parse_root(&mut self) {
        let mut text_start = self.pos;
        while !self.eof() {
            let iter_start = self.pos;
            let b = self.cur();
            if b == b'<' {
                // A comment, a script/style block, or an element.
                self.flush_text(&mut text_start);
                if self.starts_with_at(self.pos, b"<!--") {
                    let node = self.parse_comment();
                    self.template.push(node);
                } else if self.try_parse_special_block_root() {
                    // consumed a <script>/<style> block at root scope
                } else {
                    // ROOT-scope element scan: a root-only `<svelte:*>` meta tag here is at the
                    // component root (placement-valid).
                    let node = self.parse_element_or_recover(true);
                    self.template.extend(node);
                }
                text_start = self.pos;
            } else if b == b'{' {
                self.flush_text(&mut text_start);
                let node = self.parse_brace_construct();
                self.template.extend(node);
                text_start = self.pos;
            } else {
                self.pos += 1;
            }
            // FORWARD-PROGRESS INVARIANT: every iteration over a `<` / `{` construct
            // MUST advance `pos` (a sub-parser consumes its bytes or records a strict
            // fact + advances to EOF). An iteration that left `pos` unmoved while not at
            // EOF is a no-forward-progress bug — it would re-enter the same byte forever
            // (an unbounded re-parse, each pass pushing a strict fact, exhausting memory).
            // Make the class STRUCTURALLY impossible: assert in debug, and in release
            // record one fail-closed strict fact + advance one byte so the scan can never
            // spin. `parse_element_or_recover` (and the comment / special-block readers)
            // already guarantee progress; this is the backstop that turns any future
            // no-advance recovery into a single fact + a step instead of a hang.
            self.ensure_root_progress(iter_start);
        }
        // trailing text
        if text_start < self.pos {
            self.template.push(SvelteNode::Text(Span::new(
                text_start as u32,
                self.pos as u32,
            )));
        }
        // PARSE FINALIZATION — upstream's `read_options` + `disallow_children` run in the
        // `Parser` constructor AFTER the full root walk, so the options facts arbitrate at an
        // encounter order LATER than every template/script/style defect (an earlier parse defect
        // legitimately wins). Mints the first faulting options attribute's exact code, then the
        // child-content defect — exactly the upstream order.
        self.read_options_finalize();
    }

    /// Enforce the root-scan forward-progress invariant: if the iteration that began at
    /// `iter_start` left `pos` unmoved while not at EOF, record one `expected_token`
    /// strict fact (so the input fails closed) and advance a single byte so the scan
    /// cannot spin. A no-op on the normal advancing path.
    fn ensure_root_progress(&mut self, iter_start: usize) {
        if self.pos == iter_start && !self.eof() {
            debug_assert!(
                false,
                "svelte root scan made no forward progress at byte {iter_start} (a recovery \
                 point consumed no input) — this is a no-forward-progress bug"
            );
            self.record_expected_token(Span::new(iter_start as u32, (iter_start + 1) as u32));
            self.pos += 1;
        }
    }

    fn flush_text(&mut self, text_start: &mut usize) {
        if *text_start < self.pos {
            self.template.push(SvelteNode::Text(Span::new(
                *text_start as u32,
                self.pos as u32,
            )));
        }
        *text_start = self.pos;
    }

    /// At a `<`, try to consume a top-level `<script>` or `<style>` block,
    /// recording it on the parser. Returns `true` when one was consumed.
    fn try_parse_special_block_root(&mut self) -> bool {
        if self.starts_with_ci_at(self.pos, b"<script") && self.is_tag_boundary(self.pos + 7) {
            if let Some(script) = self.parse_script_block() {
                // Mint the per-script parse facts in upstream encounter order (attribute
                // duplicate → reserved body-parse slot → source-order reserved/context/module
                // fault) the instant the open tag is parsed — for EVERY script, duplicate or
                // not. Official reads + parses + validates a `<script>` BEFORE throwing the
                // duplicate-script error, so a duplicate script's body / attribute defect is
                // discovered (and wins) ahead of the duplicate fact recorded below.
                self.record_script_parse_facts(&script);
                if script.is_module {
                    if self.module_script.is_none() {
                        self.module_script = Some(script);
                    } else {
                        // A SECOND module script — official `script_duplicate`, discovered
                        // AFTER this script's own attribute rejects. The parser still
                        // retains only the first.
                        self.record_parse_reject(
                            SvelteParseRejectKind::ScriptDuplicate,
                            "script_duplicate",
                            script.tag_open,
                        );
                    }
                } else if self.instance_script.is_none() {
                    self.instance_script = Some(script);
                } else {
                    // A SECOND instance script — official `script_duplicate`.
                    self.record_parse_reject(
                        SvelteParseRejectKind::ScriptDuplicate,
                        "script_duplicate",
                        script.tag_open,
                    );
                }
            }
            return true;
        }
        if self.starts_with_ci_at(self.pos, b"<style") && self.is_tag_boundary(self.pos + 6) {
            if let Some(style) = self.parse_style_block() {
                // Reserve the CSS body-parse slot at the upstream `read_style` position — BEFORE
                // the duplicate-style check, for EVERY top-level `<style>` (duplicate or not).
                // Upstream's `element.js` calls `read_style` (which PARSES the CSS body and can
                // throw) BEFORE `if (current.css) e.style_duplicate(start)`, so a malformed body
                // (this style's OR an earlier one's) is discovered ahead of the duplicate fact
                // minted below — exactly mirrored by drawing the probe's `encounter_order` here.
                // Only a style with an actual body span gets a probe.
                if let Some(content) = style.content {
                    self.record_style_body_probe(content.start);
                }
                // A SECOND top-level `<style>` — official `style_duplicate` (`element.js`:
                // `if (current.css) e.style_duplicate(start)`), minted AFTER `read_style`,
                // parallel to `script_duplicate`. The parser still retains only the first.
                if !self.styles.is_empty() {
                    self.record_parse_reject(
                        SvelteParseRejectKind::StyleDuplicate,
                        "style_duplicate",
                        style.tag_open,
                    );
                }
                self.styles.push(style);
            }
            return true;
        }
        false
    }

    /// Whether `pos` is a tag-name boundary (whitespace, `>`, `/`, or EOF) —
    /// distinguishes `<script>` from `<scripted>`.
    fn is_tag_boundary(&self, pos: usize) -> bool {
        match self.src.get(pos) {
            None => true,
            Some(&b) => b.is_ascii_whitespace() || b == b'>' || b == b'/',
        }
    }

    // ── Script / style blocks ──────────────────────────────────────────

    fn parse_script_block(&mut self) -> Option<SvelteScript> {
        let open_start = self.pos;
        // Attributes begin AFTER the `<script` tag name (not at `self.pos + 1`, which
        // would scan `script` itself as a phantom attribute). A `None` return means the
        // open tag never reached `>` before EOF (an unterminated open tag): make forward
        // progress to EOF and record the strict fact so the root loop terminates and the
        // input fails closed — see `parse_open_tag_attributes_or_recover`.
        let (attributes, open_end, self_closing) =
            self.parse_open_tag_attributes_or_recover(open_start, self.pos + 7)?;
        let tag_open = Span::new(open_start as u32, open_end as u32);
        let lang = attr_text_value(&attributes, self, "lang");
        // The `module` / `context` attribute NAME is matched CASE-SENSITIVELY, mirroring
        // official's `attribute.name === 'module'` / `=== 'context'` checks: a capitalized
        // `Module` / `Context` is an UNKNOWN attribute (so the script stays an INSTANCE
        // script and a second instance script is a `script_duplicate`), never the module
        // script.
        let is_module = script_attr_marks_module(&attributes, self);
        if self_closing {
            // A SELF-CLOSING `<script />` is not a valid raw-text element — official
            // expects a `>` (then the raw body), so a bare `/>` is `expected_token`. Record
            // the strict fact so the input fails closed (no `Main`) instead of being treated
            // as a content-less script block. Advance past the consumed `/>` (the open tag's
            // bytes were consumed up to `open_end`) so the root loop makes forward progress
            // and cannot re-enter the same `<script` — a no-advance return here is an
            // unbounded re-parse loop.
            self.pos = open_end;
            self.record_expected_token(tag_open);
            return Some(SvelteScript {
                is_module,
                tag_open,
                content: None,
                attributes,
                lang,
            });
        }
        self.pos = open_end;
        let content_start = self.pos;
        // The `<script>` close is STRICT (a trailing token leaves the block unclosed —
        // official `element_unclosed`).
        let close = self.find_close_tag(b"script", true);
        match close {
            Some((content_end, after)) => {
                self.pos = after;
                Some(SvelteScript {
                    is_module,
                    tag_open,
                    content: Some(Span::new(content_start as u32, content_end as u32)),
                    attributes,
                    lang,
                })
            }
            None => {
                self.diag(
                    "unterminated-script",
                    "unterminated <script> block",
                    tag_open,
                );
                // No recognised `</script>` close before EOF (an unterminated block, or a
                // `</script x>` close official does not recognise) — official
                // `element_unclosed`.
                self.record_element_unclosed(tag_open);
                self.pos = self.len();
                Some(SvelteScript {
                    is_module,
                    tag_open,
                    content: Some(Span::new(content_start as u32, self.len() as u32)),
                    attributes,
                    lang,
                })
            }
        }
    }

    fn parse_style_block(&mut self) -> Option<SvelteStyle> {
        let open_start = self.pos;
        // Attributes begin AFTER the `<style` tag name (not at `self.pos + 1`). A `None`
        // return is an unterminated open tag: advance to EOF + record the fact (forward
        // progress) so the root loop terminates and the input fails closed.
        let (attributes, open_end, self_closing) =
            self.parse_open_tag_attributes_or_recover(open_start, self.pos + 6)?;
        let tag_open = Span::new(open_start as u32, open_end as u32);
        if self_closing {
            // A SELF-CLOSING `<style />` is not a valid raw-text element — official expects
            // a `>` (then the raw CSS body), so a bare `/>` is `expected_token`. Record the
            // strict fact so the input fails closed (no `Main`) instead of being treated as
            // a content-less style block. Advance past the consumed `/>` (the open tag's
            // bytes were consumed up to `open_end`) so the root loop makes forward progress
            // and cannot re-enter the same `<style` — a no-advance return here is an
            // unbounded re-parse loop.
            self.pos = open_end;
            self.record_expected_token(tag_open);
            return Some(SvelteStyle {
                tag_open,
                content: None,
                attributes,
            });
        }
        self.pos = open_end;
        let content_start = self.pos;
        // The `<style>` close is LENIENT (official's CSS reader tolerates a trailing
        // token: `</style x>` compiles).
        match self.find_close_tag(b"style", false) {
            Some((content_end, after)) => {
                self.pos = after;
                Some(SvelteStyle {
                    tag_open,
                    content: Some(Span::new(content_start as u32, content_end as u32)),
                    attributes,
                })
            }
            None => {
                self.diag("unterminated-style", "unterminated <style> block", tag_open);
                // No recognised `</style>` close before EOF. Unlike `<script>` (whose
                // raw-block close-recognition failure is `element_unclosed`), official's
                // CSS reader reaches EOF inside the unterminated rule and errors
                // `css_expected_identifier`.
                self.record_css_expected_identifier(tag_open);
                self.pos = self.len();
                Some(SvelteStyle {
                    tag_open,
                    content: Some(Span::new(content_start as u32, self.len() as u32)),
                    attributes,
                })
            }
        }
    }

    /// Find the matching close for a TOP-LEVEL raw-content block (`<script>` / `<style>`)
    /// from `self.pos`. Returns `(content_end, after_close)`. Scans raw (script/style
    /// contents are opaque) — it does not descend into nested markup. The tag name is
    /// matched CASE-SENSITIVELY, mirroring official (a `</Style>` / `</Script>` does NOT
    /// close the block).
    ///
    /// `strict_close` mirrors official's DIVERGENT raw-text-element close handling:
    /// - STRICT (`<script>`): the close is ONLY `</script` + optional whitespace + `>`. A
    ///   trailing token (`</script x>`), a slash (`</script/>`), or a LONGER-name
    ///   continuation (`</scriptfoo>`) is NOT the close — the scan continues (the block is
    ///   left open at EOF ⇒ the caller records `element_unclosed`).
    /// - LENIENT (`<style>`): official's CSS reader matches the `</style` NAME PREFIX and
    ///   then reads to `>` / EOF, so a LONGER-name continuation (`</stylefoo>`,
    ///   `</style-x>`), a trailing token (`</style x>`), whitespace (`</style >`), or even
    ///   EOF before `>` (`</style`) all CLOSE the style block. A SHORTER prefix (`</styl`)
    ///   does NOT match (the block stays open at EOF ⇒ the caller records
    ///   `css_expected_identifier`).
    fn find_close_tag(&self, tag: &[u8], strict_close: bool) -> Option<(usize, usize)> {
        let mut p = self.pos;
        while p < self.len() {
            if self.at(p) == b'<' && self.at(p + 1) == b'/' && self.starts_with_at(p + 2, tag) {
                let after_name = p + 2 + tag.len();
                if strict_close {
                    // STRICT (`<script>`): accept ONLY `</tag>` + optional whitespace +
                    // `>`. A trailing token / longer-name continuation means this is NOT the
                    // close — keep scanning (the block is left open ⇒ `element_unclosed`).
                    if self.at(after_name) == b'>' {
                        return Some((p, (after_name + 1).min(self.len())));
                    }
                    if self.at(after_name).is_ascii_whitespace() {
                        let mut q = after_name;
                        while q < self.len() && self.at(q).is_ascii_whitespace() {
                            q += 1;
                        }
                        if self.at(q) == b'>' {
                            return Some((p, (q + 1).min(self.len())));
                        }
                    }
                } else {
                    // LENIENT (`<style>`): the `</style` NAME PREFIX is the close — read to
                    // `>` / EOF regardless of any name continuation / trailing token
                    // (official's CSS reader tolerates them).
                    let mut q = after_name;
                    while q < self.len() && self.at(q) != b'>' {
                        q += 1;
                    }
                    return Some((p, (q + 1).min(self.len())));
                }
            }
            p += 1;
        }
        None
    }

    /// Find the matching close for a NESTED raw-content element (a nested `<style>`) from
    /// `self.pos`, mirroring official's raw-text-element reader: the close is the LITERAL
    /// `</tag>` string — a CASE-SENSITIVE `</tag` immediately followed by `>`. ANY other
    /// `</tag…` occurrence (a trailing token `</style x>`, a slash `</style/>`, whitespace
    /// `</style >`, a longer name `</stylefoo>`, a different case `</Style>`) is NOT the
    /// close — it is opaque body text and the scan continues. Returns
    /// `(content_end, after_close)` on the clean `</tag>`. When EOF is reached with no clean
    /// close, the raw-text reader hit end of input expecting the close `>` — official
    /// `expected_token` — so it records that strict fact (on EVERY EOF path, so the fact
    /// DOMINATES the recovery return) and returns `None`. The caller does NOT record a fact
    /// for the no-close case (this helper owns it). Scans raw (the content is opaque CSS).
    fn find_nested_raw_close(&mut self, tag: &[u8]) -> Option<(usize, usize)> {
        let content_start = self.pos;
        let mut p = self.pos;
        while p < self.len() {
            // The close is the LITERAL `</tag>` (case-sensitive name + immediate `>`); any
            // deviation is body text, not the close.
            if self.at(p) == b'<' && self.at(p + 1) == b'/' && self.starts_with_at(p + 2, tag) {
                let after_name = p + 2 + tag.len();
                if self.at(after_name) == b'>' {
                    let after = (after_name + 1).min(self.len());
                    return Some((p, after));
                }
            }
            p += 1;
        }
        // EOF with no clean `</tag>` close — the raw-text reader reached end of input
        // expecting the close `>` (official `expected_token`). Record the strict fact AND
        // advance to EOF in this same block (the fact DOMINATES the recovery exit), then
        // report no-close to the caller.
        self.record_expected_token(Span::new(content_start as u32, self.len() as u32));
        self.pos = self.len();
        None
    }

    // ── Comments ───────────────────────────────────────────────────────

    fn parse_comment(&mut self) -> SvelteNode {
        let start = self.pos;
        // skip "<!--"
        self.pos += 4;
        // An EMPTY comment lead (`<!--` with NOTHING after it) is cut off immediately —
        // official `unexpected_eof`. A STARTED-but-unterminated comment (`<!-- oops` with
        // content but no `-->`) is official `expected_token`. Captured before the scan.
        let empty_at_eof = self.pos >= self.len();
        while self.pos < self.len() {
            if self.starts_with_at(self.pos, b"-->") {
                self.pos += 3;
                return SvelteNode::Comment(Span::new(start as u32, self.pos as u32));
            }
            self.pos += 1;
        }
        self.diag(
            "unterminated-comment",
            "unterminated comment",
            Span::new(start as u32, self.len() as u32),
        );
        // Reached EOF without the closing `-->`: an EMPTY `<!--` is `unexpected_eof`; a
        // started-but-unterminated comment is `expected_token`. Recorded UNCONDITIONALLY
        // (the kind is selected first), so the strict fact DOMINATES the EOF recovery exit.
        let kind = if empty_at_eof {
            SvelteStrictParseErrorKind::UnexpectedEof
        } else {
            SvelteStrictParseErrorKind::ExpectedToken
        };
        self.record_strict_parse_error(kind, Span::new(start as u32, self.len() as u32));
        self.pos = self.len();
        SvelteNode::Comment(Span::new(start as u32, self.len() as u32))
    }

    // ── Elements ───────────────────────────────────────────────────────

    /// Parse an element at `<`, recovering by emitting the `<` as text on a
    /// malformed tag.
    fn parse_element_or_recover(&mut self, at_root: bool) -> Vec<SvelteNode> {
        let start = self.pos;
        if self.at(self.pos + 1) == b'/' {
            // A close tag reached at this scope. The element-child scan
            // (`parse_children_until_close`) handles a matching / ancestor close
            // inline; this path is only reached for a close at the ROOT or a block
            // scope. The SINGLE close-boundary classifier records the malformed-boundary
            // strict fact (nameless `</>` / trailing token / `</` at EOF). A WELL-FORMED
            // close matching NOTHING open is a stray / mismatched close —
            // `element_invalid_closing_tag` (or `void_element_invalid_content` for a void
            // name); a malformed-boundary close already recorded its strict fact.
            let close = self.consume_and_classify_close();
            if close.clean {
                if let Some(close_name) = close.name {
                    if !self.open_stack.iter().any(|a| a == &close_name) {
                        self.record_stray_or_void_close(&close_name, close.span);
                    }
                }
            }
            return Vec::new();
        }
        // Parse the tag name. A valid element tag name STARTS with an ASCII letter
        // (`[a-zA-Z]`); a `<` followed by a digit / punctuation name byte (`<1`, `<.`)
        // does NOT begin a valid tag even though that byte can appear LATER in a name —
        // official rejects it (`tag_invalid_name`), so it must take the recovery path
        // (not be mis-parsed as an element whose name starts with a digit).
        let name_start = self.pos + 1;
        let mut p = name_start;
        while p < self.len() && is_tag_name_byte(self.at(p)) {
            p += 1;
        }
        let valid_name_start = p > name_start && self.at(name_start).is_ascii_alphabetic();
        if !valid_name_start {
            // Not a real tag (`<` followed by a byte that cannot BEGIN a tag name) —
            // recover by emitting `<` as literal text. Official STRICT-parses this as a
            // tag start and REJECTS it, with a code that DEPENDS on the following byte:
            //   - `<` at EOF                       → `unexpected_eof`
            //   - `<!…` (a markup-declaration lead, e.g. `<!x`, distinct from the `<!--`
            //     comment handled earlier) → `expected_token` (a recognised declaration /
            //     comment token is expected after `<!`)
            //   - any other malformed name start (`<.`, `<}`, `<1`, `< `) → `tag_invalid_name`
            let lt_span = Span::new(start as u32, (start + 1) as u32);
            if start + 1 >= self.len() {
                self.record_unexpected_eof(lt_span);
            } else if self.at(name_start) == b'!' {
                self.record_expected_token(lt_span);
            } else {
                self.record_tag_invalid_name(lt_span);
            }
            self.pos += 1;
            return vec![SvelteNode::Text(Span::new(start as u32, self.pos as u32))];
        }
        let name_span = Span::new(name_start as u32, p as u32);
        let name = self.slice(name_span).to_string();
        let kind = classify_element(&name);

        // A DUPLICATE root-only `<svelte:*>` meta element (a second `<svelte:options>` /
        // `<svelte:head>` / …) — official `svelte_meta_duplicate`, minted right after the tag
        // name is read (BEFORE the open-tag attribute loop), so it precedes this element's own
        // `attribute_duplicate` in encounter order (matching upstream `element.js`). The check
        // is on the second occurrence of the same name ANYWHERE (root or nested), mirroring
        // upstream's `parser.meta_tags` set.
        let element_open_span_start = Span::new(start as u32, (start + 1 + name.len()) as u32);
        self.note_root_meta_tag(&name, at_root, element_open_span_start);

        // The customElement attribute-expression PARSE encounter orders ride the positions
        // upstream's `read_expression` reaches DURING the attribute loop, so each is drawn at its
        // `customElement` attribute's discovery point (interleaved with the open tag's duplicate
        // tracker), NOT after the whole open tag. The gate to draw them is "the FIRST root
        // `<svelte:options>`": this open tag is at the root, classifies as the options special
        // element, and no earlier options element drew any parse order yet. (Upstream's
        // `findIndex` over the root fragment picks the FIRST `<svelte:options>`.)
        let draw_options_ce_parse_order = at_root
            && matches!(kind, SvelteElementKind::Special(SvelteSpecialKind::Options))
            && self.options_ce_attr_parse_orders.is_empty();
        let facts_before_open = self.strict_parse_errors.len();
        let Some((attributes, open_end, self_closing)) =
            self.parse_open_tag_attributes_inner(p, false, draw_options_ce_parse_order)
        else {
            // Unterminated open tag — emit text and bail. A truncated intrinsic open tag
            // reaches end of input mid-construct (`<div`, `<div id`) ⇒ official
            // `unexpected_eof` — UNLESS the attribute parse already recorded an earlier,
            // equally-specific EOF fact (an `id=` value at EOF, an unterminated quoted
            // value), which is the authoritative first-in-document-order one.
            self.diag(
                "unterminated-tag",
                "unterminated element open tag",
                Span::new(start as u32, self.len() as u32),
            );
            if self.strict_parse_errors.len() == facts_before_open {
                self.record_unexpected_eof(Span::new(start as u32, self.len() as u32));
            }
            self.pos = self.len();
            return Vec::new();
        };
        let open_span = Span::new(start as u32, open_end as u32);
        self.pos = open_end;

        let void = self_closing || is_void_element(&name);
        let mut children = Vec::new();
        let mut close_span = None;
        if !void {
            if matches!(kind, SvelteElementKind::NestedStyle) {
                // Nested <style> inside template markup — opaque content closed by the
                // LITERAL `</style>` only. Unlike the TOP-LEVEL `<style>` (whose CSS-aware
                // reader matches the close by a lenient name prefix and consumes to `>`), a
                // NESTED `<style>` close is the exact `</style>` raw-text-element close: any
                // deviation (a trailing token, whitespace before `>`, a longer name, a
                // different case) is body text, and reaching EOF with no clean close is
                // official `expected_token` (recorded inside `find_nested_raw_close` on
                // every no-close path).
                let content_start = self.pos;
                if let Some((content_end, after)) = self.find_nested_raw_close(b"style") {
                    self.pos = after;
                    // The NestedStyle text child spans content + close tag (the
                    // projector strips it whole); the close span is recorded for
                    // close-tag-aware consumers but stays inside that removed run.
                    children.push(SvelteNode::Text(Span::new(
                        content_start as u32,
                        self.pos as u32,
                    )));
                    close_span = Some(Span::new(content_end as u32, after as u32));
                }
                // No clean `</style>` close before EOF — `find_nested_raw_close` already
                // recorded the `expected_token` strict fact AND advanced `self.pos` to EOF,
                // so there is nothing more to do on the no-close path.
            } else {
                let (kids, close) = self.parse_children_until_close(&name, open_span);
                children = kids;
                close_span = close;
            }
        }

        vec![SvelteNode::Element(SvelteElement {
            name,
            name_span,
            kind,
            attributes,
            children,
            self_closing: void,
            open_span,
            close_span,
        })]
    }

    /// Parse child nodes until the matching `</name>` close (or EOF). Returns the
    /// children plus the consumed `</name>` close-tag span (`None` if the element
    /// is unterminated / closed implicitly by an ancestor close).
    ///
    /// The element is pushed onto the OPEN-STACK for the duration so a deeper foreign
    /// close can recognise it as an ancestor. Close-tag well-formedness is recorded
    /// faithfully: a foreign close that matches an OPEN ANCESTOR unwinds (the
    /// intervening element is implicitly closed — official accepts it); a foreign
    /// close that matches NOTHING open is a stray / mismatched close (recorded, then
    /// skipped so this element keeps scanning for its real close); reaching EOF with
    /// no close leaves an intrinsic element UNCLOSED (recorded — official
    /// `element_unclosed`).
    fn parse_children_until_close(
        &mut self,
        name: &str,
        open_span: Span,
    ) -> (Vec<SvelteNode>, Option<Span>) {
        self.open_stack.push(name.to_string());
        let mut children = Vec::new();
        let mut text_start = self.pos;
        while !self.eof() {
            let b = self.cur();
            if b == b'<' {
                // A close tag at this scope? Recognise the close by NAME (independent of
                // boundary well-formedness) so a MALFORMED-boundary close of THIS element
                // (`</div/>` while parsing `<div>`) is still recognised as this element's
                // close — recorded `expected_token` AND the element closed (never left
                // open as a spurious `element_unclosed`).
                if self.at(self.pos + 1) == b'/' {
                    // `close_tag_name_bytes` lowercases; match the (also-lowercased) element
                    // / ancestor names case-insensitively (HTML close-tag matching). The
                    // BOUNDARY is classified non-consuming so an ANCESTOR unwind (which must
                    // NOT consume the close — the ancestor frame consumes it) happens ONLY
                    // for a well-formed boundary.
                    let close_name = self.close_tag_name_bytes(self.pos + 2);
                    let matches_self =
                        close_name.as_deref() == Some(name.to_ascii_lowercase().as_str());
                    let matches_ancestor = close_name.as_deref().is_some_and(|cn| {
                        self.open_stack.iter().any(|a| a.eq_ignore_ascii_case(cn))
                    });
                    let boundary_clean = matches!(
                        self.classify_close_boundary(self.pos + 2),
                        CloseBoundary::Clean
                    );
                    // Flush pending text before consuming the close.
                    if text_start < self.pos {
                        children.push(SvelteNode::Text(Span::new(
                            text_start as u32,
                            self.pos as u32,
                        )));
                    }
                    // A WELL-FORMED close of an OPEN ANCESTOR (NOT this element) — this
                    // element is implicitly closed (official pops intervening elements);
                    // unwind WITHOUT consuming so the ancestor frame consumes the close.
                    if boundary_clean && matches_ancestor && !matches_self {
                        self.open_stack.pop();
                        return (children, None);
                    }
                    if matches_self {
                        // THE close of this element (possibly with a malformed boundary —
                        // `consume_and_classify_close` records the strict fact). Close it.
                        let close = self.consume_and_classify_close();
                        // An explicit `</p>` SURVIVING for a `<p>` that the browser already
                        // auto-closed (a direct disallowed block child) is official
                        // `element_invalid_closing_tag_autoclosed`, ANCHORED at this `</p>`
                        // close (NOT the `<p>` open). Mint it here — the close-handling
                        // authority — so its `encounter_order` is the moment the surviving
                        // `</p>` is consumed (an earlier inner parse defect beats it; a later
                        // stray loses to it). A WELL-FORMED `</p>` boundary only (a
                        // malformed-boundary close already recorded its own strict fact, which
                        // is the earlier/authoritative defect). The IMPLICIT case (no explicit
                        // `</p>`) is official-ACCEPTED via autoclose — minted NOTHING.
                        if close.clean
                            && name.eq_ignore_ascii_case("p")
                            && matches!(classify_element(name), SvelteElementKind::Intrinsic)
                            && Self::paragraph_has_direct_autoclose_child(&children)
                        {
                            self.record_parse_reject(
                                SvelteParseRejectKind::ParagraphAutoclose,
                                "element_invalid_closing_tag_autoclosed",
                                close.span,
                            );
                        }
                        self.open_stack.pop();
                        return (children, Some(close.span));
                    }
                    // A FOREIGN close that is NOT a clean ancestor. Consume + classify the
                    // boundary (a malformed boundary records its strict fact here, AHEAD of
                    // any ancestor absorption — so a malformed-boundary close, e.g. a
                    // trailing slash/token, cannot be silently absorbed as an ancestor close
                    // without recording its strict fact).
                    let close = self.consume_and_classify_close();
                    if close.clean {
                        if let Some(close_name) = close.name {
                            // A STRAY / mismatched well-formed close (matches nothing open)
                            // — record the violation and KEEP scanning for this element's
                            // real close (so a `<div>…</span>…</div>` still closes `<div>`).
                            self.record_stray_or_void_close(&close_name, close.span);
                        }
                    }
                    text_start = self.pos;
                    continue;
                }
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                if self.starts_with_at(self.pos, b"<!--") {
                    let c = self.parse_comment();
                    children.push(c);
                } else {
                    // NESTED child scan (inside an element or a block clause): a root-only
                    // `<svelte:*>` meta tag here is NOT at the component root.
                    let nodes = self.parse_element_or_recover(false);
                    children.extend(nodes);
                }
                text_start = self.pos;
            } else if b == b'{' {
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                // A block-closing/clause token belongs to an enclosing block —
                // stop child scan so the block parser sees it.
                if self.is_block_close_or_clause() {
                    self.open_stack.pop();
                    return (children, None);
                }
                let nodes = self.parse_brace_construct();
                children.extend(nodes);
                text_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        if text_start < self.pos {
            children.push(SvelteNode::Text(Span::new(
                text_start as u32,
                self.pos as u32,
            )));
        }
        // Reached EOF with no matching close: an intrinsic HTML element is UNCLOSED
        // (official `element_unclosed`). A component / `<svelte:*>` element is NOT in
        // the close-tag universe (it fails closed as an unsupported feature regardless).
        if matches!(classify_element(name), SvelteElementKind::Intrinsic) {
            self.record_close_violation(CloseTagViolationKind::Unclosed, name, open_span);
        }
        self.open_stack.pop();
        (children, None)
    }

    /// Classify the WELL-FORMEDNESS of a close tag whose `</` precedes `pos` (i.e. `pos`
    /// is the first byte after `</`), driven from the parser's own bytes (no source
    /// product-path scanning). Non-consuming. This is the SINGLE close-tag boundary
    /// classifier every close site routes through so the recorded official code matches
    /// the pinned compiler at EVERY boundary form:
    ///
    /// - `</` at EOF (no further bytes) → [`CloseBoundary::UnexpectedEof`]
    ///   (`unexpected_eof`).
    /// - `</>` (immediately `>`, no name) → [`CloseBoundary::Nameless`]
    ///   (`element_invalid_closing_tag`).
    /// - `</ name>` / `</ >` (WHITESPACE before any name) → [`CloseBoundary::ExpectedToken`]
    ///   (`expected_token`): the parser expects the name immediately after `</`.
    /// - a name followed by anything but optional-whitespace-then-`>` — a trailing token
    ///   (`</div x>`, `</div/>`) or EOF before `>` (`</div`) → [`CloseBoundary::ExpectedToken`]
    ///   (`expected_token`).
    /// - a name followed by optional whitespace then `>` → [`CloseBoundary::Clean`] with
    ///   the lowercased name (the caller then applies match / ancestor / stray / void
    ///   classification).
    fn classify_close_boundary(&self, pos: usize) -> CloseBoundary {
        if pos >= self.len() {
            // `</` at EOF.
            return CloseBoundary::UnexpectedEof;
        }
        // Read the tag-name bytes.
        let mut pn = pos;
        while pn < self.len() && is_tag_name_byte(self.at(pn)) {
            pn += 1;
        }
        if pn == pos {
            // No name byte at `pos`.
            if self.at(pos) == b'>' {
                // `</>` — a genuine nameless close.
                return CloseBoundary::Nameless;
            }
            // `</ …>` (whitespace, or any non-name non-`>` byte) — the name is expected
            // immediately after `</`.
            return CloseBoundary::ExpectedToken;
        }
        // Skip whitespace after the name.
        let mut pw = pn;
        while pw < self.len() && self.at(pw).is_ascii_whitespace() {
            pw += 1;
        }
        if pw < self.len() && self.at(pw) == b'>' {
            CloseBoundary::Clean
        } else {
            // A trailing token after the name, or EOF before `>`.
            CloseBoundary::ExpectedToken
        }
    }

    /// Consume a close tag (at `</`), record the strict fact for a malformed BOUNDARY (a
    /// nameless `</>`, a `</ name>` / trailing-token, or a `</` at EOF), and return a
    /// [`ConsumedClose`]: the close span, the close NAME (the tag-name bytes after `</`,
    /// independent of boundary well-formedness — `None` only for a nameless `</>` or `</`
    /// at EOF), and whether the boundary was WELL-FORMED. Advances `self.pos` past the
    /// close.
    ///
    /// The name is reported even for a MALFORMED boundary (`</div/>` yields `Some("div")`)
    /// so the caller can still recognise it as the (malformed) close of the matching
    /// element — and close the element — instead of leaving it open. The strict fact for
    /// the malformed boundary is recorded HERE regardless.
    fn consume_and_classify_close(&mut self) -> ConsumedClose {
        let close_start = self.pos as u32;
        let name = self.close_tag_name_bytes(self.pos + 2);
        let boundary = self.classify_close_boundary(self.pos + 2);
        self.consume_close_tag();
        let span = Span::new(close_start, self.pos as u32);
        let clean = match boundary {
            CloseBoundary::Clean => true,
            CloseBoundary::Nameless => {
                self.record_nameless_close(span);
                false
            }
            CloseBoundary::ExpectedToken => {
                self.record_expected_token(span);
                false
            }
            CloseBoundary::UnexpectedEof => {
                self.record_unexpected_eof(span);
                false
            }
        };
        ConsumedClose { span, name, clean }
    }

    /// The lowercased tag-name bytes of a close tag at `pos` (just after `</`), or `None`
    /// when no name byte is present (`</>`, `</ …>`, `</` at EOF). Non-consuming. Used to
    /// recognise the close NAME independent of the boundary well-formedness.
    fn close_tag_name_bytes(&self, pos: usize) -> Option<String> {
        let mut p = pos;
        while p < self.len() && is_tag_name_byte(self.at(p)) {
            p += 1;
        }
        if p == pos {
            return None;
        }
        self.text.get(pos..p).map(|s| s.to_ascii_lowercase())
    }

    /// Record a STRAY close tag as the right violation class: a close of a VOID
    /// element (`</input>`) is `void_element_invalid_content` (a void element cannot
    /// have a closing tag); any other unmatched close is `element_invalid_closing_tag`.
    /// A component / `<svelte:*>` close name is NOT in the close-tag universe.
    fn record_stray_or_void_close(&mut self, close_name: &str, span: Span) {
        if !matches!(classify_element(close_name), SvelteElementKind::Intrinsic) {
            return;
        }
        let kind = if is_void_element(close_name) {
            CloseTagViolationKind::VoidElementInvalidContent
        } else {
            CloseTagViolationKind::InvalidClosingTag
        };
        self.record_close_violation(kind, close_name, span);
    }

    fn consume_close_tag(&mut self) {
        // at `</`
        let mut p = self.pos + 2;
        while p < self.len() && self.at(p) != b'>' {
            p += 1;
        }
        self.pos = (p + 1).min(self.len());
    }

    /// Parse a SPECIAL-block (`<script>` / `<style>`) open tag's attributes starting at
    /// `from` (just after the tag name), GUARANTEEING forward progress on an unterminated
    /// tag. On a `None` (the tag never reached `>` before EOF) this records the strict
    /// fact and advances `self.pos` to EOF — so the caller's `?` still bails, but the root
    /// loop can never re-enter the same `<` (the no-forward-progress hang). `open_start`
    /// is the `<` position (the open-tag span start).
    ///
    /// The strict fact: a special open tag truncated at EOF (it never reaches its `>`)
    /// reaches end of input mid-construct — official `unexpected_eof`. If the attribute
    /// parse ALREADY recorded a more specific fact (an empty `lang=` value at EOF →
    /// `expected_attribute_value`), that earlier fact is left as the authoritative
    /// first-in-document-order one.
    fn parse_open_tag_attributes_or_recover(
        &mut self,
        open_start: usize,
        from: usize,
    ) -> Option<(Vec<SvelteAttribute>, usize, bool)> {
        let facts_before = self.strict_parse_errors.len();
        match self.parse_open_tag_attributes(from, true) {
            Some(result) => Some(result),
            None => {
                self.diag(
                    "unterminated-tag",
                    "unterminated special-block open tag",
                    Span::new(open_start as u32, self.len() as u32),
                );
                // Only mint the truncated-open-tag `unexpected_eof` when the attribute parse
                // did not already record an earlier, more specific fact.
                if self.strict_parse_errors.len() == facts_before {
                    self.record_unexpected_eof(Span::new(open_start as u32, self.len() as u32));
                }
                self.pos = self.len();
                None
            }
        }
    }

    /// Parse an open tag's attributes starting at `from` (just after the tag name). Returns
    /// `(attributes, position_after_'>', self_closing)`, or `None` if the tag is
    /// unterminated. `special_block` selects the `=`-at-EOF code: a SPECIAL-block
    /// (`<script>` / `<style>`) `attr=` truncated at EOF is official `expected_attribute_value`,
    /// while an INTRINSIC-element `attr=` at EOF reaches end of input ⇒ `unexpected_eof`.
    fn parse_open_tag_attributes(
        &mut self,
        from: usize,
        special_block: bool,
    ) -> Option<(Vec<SvelteAttribute>, usize, bool)> {
        self.parse_open_tag_attributes_inner(from, special_block, false)
    }

    /// The shared open-tag attribute loop, with `draw_options_ce_parse_order` selecting whether to
    /// draw the first root `<svelte:options>` element's `customElement={EXPR}` attribute-expression
    /// PARSE encounter orders — one per expression-valued occurrence, each AT its own attribute's
    /// discovery point in the loop. Drawing them here (rather than after the whole open tag)
    /// interleaves each parse fault correctly with the `attribute_duplicate` minted by
    /// `note_attribute_for_duplicate` — so a `customElement={}` / `customElement={1 2}` parse fault
    /// beats a LATER duplicate `foo foo` (and its OWN duplicate mint) and loses to an EARLIER one,
    /// exactly as upstream's during-loop `read_expression` does.
    fn parse_open_tag_attributes_inner(
        &mut self,
        from: usize,
        special_block: bool,
        draw_options_ce_parse_order: bool,
    ) -> Option<(Vec<SvelteAttribute>, usize, bool)> {
        let mut p = from;
        let mut attributes = Vec::new();
        // The duplicate-attribute tracker for THIS open tag, mirroring upstream's per-element
        // `unique_names` set (`element.js`): the FIRST collision mints `attribute_duplicate`
        // at the colliding attribute's discovery point (so its `encounter_order` interleaves
        // correctly with the open tag's attribute strict facts), and only once per tag.
        let mut seen_attr_keys: Vec<(DuplicateKeyClass, String)> = Vec::new();
        let mut dup_minted = false;
        loop {
            // skip whitespace
            while p < self.len() && self.at(p).is_ascii_whitespace() {
                p += 1;
            }
            // Skip an HTML comment inside the open tag (5.53 tolerance): a
            // `<!-- ... -->` between attributes is consumed (not recorded) so the
            // following real attributes are not lost.
            if self.starts_with_at(p, b"<!--") {
                let mut q = p + 4;
                while q < self.len() && !self.starts_with_at(q, b"-->") {
                    q += 1;
                }
                p = (q + 3).min(self.len());
                continue;
            }
            if p >= self.len() {
                return None;
            }
            let b = self.at(p);
            if b == b'>' {
                return Some((attributes, p + 1, false));
            }
            if b == b'/' && self.at(p + 1) == b'>' {
                return Some((attributes, p + 2, true));
            }
            if b == b'{' {
                // A spread `{...x}`, shorthand `{value}`, or an inline tag
                // (`{@attach}`/comment) used as an attribute.
                let (attr, next) = self.parse_brace_attribute(p);
                self.note_attribute_for_duplicate(&attr, &mut seen_attr_keys, &mut dup_minted);
                attributes.push(attr);
                p = next;
                continue;
            }
            // A named attribute or directive.
            let (attr, next) = self.parse_named_attribute(p, special_block);
            // Draw the customElement parse order BEFORE the duplicate-tracker for THIS attribute, so
            // the parse fault rides the position upstream's `read_expression` reaches first (the
            // value is read during the attribute parse, BEFORE the attribute's own duplicate
            // check). For a DUPLICATE `customElement={EXPR}` this ordering is load-bearing: the
            // duplicate occurrence's parse fault beats its OWN `attribute_duplicate` mint (drawn
            // one step later) while still losing to any duplicate minted at an EARLIER attribute
            // — exactly upstream's read-value-then-check-duplicate per-attribute order.
            if draw_options_ce_parse_order {
                self.note_options_ce_attr_parse_order(&attr);
            }
            self.note_attribute_for_duplicate(&attr, &mut seen_attr_keys, &mut dup_minted);
            attributes.push(attr);
            p = next;
        }
    }

    /// Record one open-tag attribute into the per-tag duplicate tracker, minting
    /// `attribute_duplicate` at the FIRST collision (the official `element.js` rule: the
    /// `type + name` key is case-sensitive; a `this` attribute is exempt — never recorded,
    /// never a collision). Mints at most once per tag (`dup_minted` latches). The shared
    /// [`duplicate_attribute_key`] normalization is the SINGLE copy of the bind→Attribute /
    /// class/style-namespace / non-checkable-form rules.
    fn note_attribute_for_duplicate(
        &mut self,
        attr: &SvelteAttribute,
        seen: &mut Vec<(DuplicateKeyClass, String)>,
        dup_minted: &mut bool,
    ) {
        let Some((class, name)) = duplicate_attribute_key(attr) else {
            return;
        };
        if *dup_minted {
            return;
        }
        if seen.iter().any(|(c, n)| *c == class && n == name) {
            self.record_parse_reject(
                SvelteParseRejectKind::AttributeDuplicate,
                "attribute_duplicate",
                attr.span,
            );
            *dup_minted = true;
            return;
        }
        // `this` is exempt (`<svelte:element bind:this this=..>` is allowed): never recorded,
        // so it neither triggers nor causes a collision.
        if name != "this" {
            seen.push((class, name.to_string()));
        }
    }

    /// Parse a `{ ... }` attribute (spread / shorthand / inline). Returns the
    /// attribute and the position after the closing brace.
    fn parse_brace_attribute(&mut self, from: usize) -> (SvelteAttribute, usize) {
        let inner_start = from + 1;
        let end = self.find_matching_brace(inner_start);
        // An UNCLOSED `{` attribute expression at EOF (`<button {x` / `<button onclick={…`
        // with no matching `}`) is official `expected_token` (the brace expected its close),
        // NOT the bare truncated-tag `unexpected_eof`. Record the strict fact here so the
        // open-tag fallback's fact-count guard leaves it as the authoritative code.
        if end >= self.len() {
            self.record_expected_token(Span::new(from as u32, self.len() as u32));
        }
        let inner = Span::new(inner_start as u32, end as u32);
        let span = Span::new(from as u32, (end + 1).min(self.len()) as u32);
        let body = self.slice(inner).trim_start();
        let attr = if body.starts_with("...") {
            SvelteAttribute {
                kind: SvelteAttributeKind::Spread(inner),
                span,
            }
        } else if let Some(expr_span) = attach_attribute_expr_span(self.src, inner) {
            // `{@attach expr}` in ATTRIBUTE position — the official 5.29 `AttachTag`
            // (attribute-only; the child-position form is the official `expected_tag`
            // reject). A DEDICATED kind carrying the expression span (after the
            // `@attach` keyword + separating whitespace, mirroring the child-form
            // tag parse) so lowering never re-slices the body text.
            SvelteAttribute {
                kind: SvelteAttributeKind::Attach { expr_span },
                span,
            }
        } else if body.starts_with('@') || body.starts_with('#') || body.starts_with('/') {
            // A NON-attach tag sigil used in attribute position — record as a plain
            // attribute carrying the inner span so the projector can dispatch on
            // the leading sigil.
            SvelteAttribute {
                kind: SvelteAttributeKind::Plain {
                    name: String::new(),
                    name_span: Span::new(inner_start as u32, inner_start as u32),
                    value: Some(SvelteAttributeValue::Expression(inner)),
                },
                span,
            }
        } else {
            // Attribute-value shorthand `{name}` → name == value expression. The
            // NAME is the FULLY-trimmed identifier (`{ foo }` ⇒ name `foo`, not
            // `foo ` — official `read_attribute`'s shorthand produces the bare
            // identifier name). The value EXPRESSION span stays the inner span
            // (surrounding whitespace inside the braces is harmless to OXC's reparse
            // of the identifier), kept SEPARATE from the trimmed name.
            SvelteAttribute {
                kind: SvelteAttributeKind::Plain {
                    name: body.trim_end().to_string(),
                    name_span: inner,
                    value: Some(SvelteAttributeValue::Expression(inner)),
                },
                span,
            }
        };
        (attr, (end + 1).min(self.len()))
    }

    /// Parse a named attribute or directive at `from`. Returns the attribute and the
    /// position after it. `special_block` selects the `=`-at-EOF code (a SPECIAL-block
    /// `attr=` truncated at EOF is `expected_attribute_value`; an INTRINSIC-element `attr=`
    /// at EOF is `unexpected_eof`).
    fn parse_named_attribute(
        &mut self,
        from: usize,
        special_block: bool,
    ) -> (SvelteAttribute, usize) {
        let mut p = from;
        // The attribute name runs until whitespace, `=`, `/`, `>`, or EOF.
        while p < self.len() {
            let b = self.at(p);
            if b.is_ascii_whitespace()
                || b == b'='
                || b == b'>'
                || (b == b'/' && self.at(p + 1) == b'>')
            {
                break;
            }
            p += 1;
        }
        let name_span = Span::new(from as u32, p as u32);
        let raw_name = self.slice(name_span).to_string();

        // Optional value.
        let mut value: Option<SvelteAttributeValue> = None;
        let mut after = p;
        // skip ws before '='
        let mut q = p;
        while q < self.len() && self.at(q).is_ascii_whitespace() {
            q += 1;
        }
        if self.at(q) == b'=' {
            let eq_pos = q;
            q += 1;
            while q < self.len() && self.at(q).is_ascii_whitespace() {
                q += 1;
            }
            // An `=` followed immediately by the tag-closing `>` is an EMPTY attribute
            // value (`id=>`, `lang=>`) — official `expected_attribute_value`. An `=` at EOF
            // (`<div id=` with no `>`) is CONSTRUCT-dependent: an INTRINSIC-element attribute
            // reaches end of input ⇒ `unexpected_eof`, while a SPECIAL-block (`<script>` /
            // `<style>`) attribute reader specifically expected the value ⇒
            // `expected_attribute_value`. (An unquoted value `id=x`, a quoted `id="x"`, or a
            // `/>` self-close are NOT empty: each begins a real value / a distinct close,
            // classified elsewhere.)
            if q >= self.len() {
                if special_block {
                    self.record_empty_attribute_value(Span::new(from as u32, self.len() as u32));
                } else {
                    self.record_unexpected_eof(Span::new(from as u32, self.len() as u32));
                }
            } else if self.at(q) == b'>' {
                self.record_empty_attribute_value(Span::new(from as u32, (eq_pos + 1) as u32));
            }
            let (val, next) = self.parse_attribute_value(q);
            value = val;
            after = next;
        }

        let span = Span::new(from as u32, after as u32);
        // Directive? `prefix:local|mods`
        if let Some(colon) = raw_name.find(':') {
            let prefix = &raw_name[..colon];
            let rest = &raw_name[colon + 1..];
            let mut parts = rest.split('|');
            let local = parts.next().unwrap_or("").to_string();
            let modifiers: Vec<String> = parts.map(|s| s.to_string()).collect();
            let dkind = SvelteDirectiveKind::from_prefix(prefix);
            // `svelte:` is an element namespace, never a directive — but element
            // names are handled before this point, so a `svelte:` here is an odd
            // attribute; classify as Unknown directive (parse-without-crash).
            let kind = SvelteAttributeKind::Directive(SvelteDirective {
                kind: dkind,
                local,
                modifiers,
                value,
            });
            return (SvelteAttribute { kind, span }, after);
        }

        (
            SvelteAttribute {
                kind: SvelteAttributeKind::Plain {
                    name: raw_name,
                    name_span,
                    value,
                },
                span,
            },
            after,
        )
    }

    /// Parse an attribute value at `from` (a quoted string, a `{expr}`, or a
    /// mixed run). Returns the value and the position after it.
    fn parse_attribute_value(&mut self, from: usize) -> (Option<SvelteAttributeValue>, usize) {
        let b = self.at(from);
        if b == b'"' || b == b'\'' {
            let quote = b;
            let body_start = from + 1;
            let mut p = body_start;
            let mut saw_brace = false;
            while p < self.len() && self.at(p) != quote {
                if self.at(p) == b'{' {
                    saw_brace = true;
                }
                p += 1;
            }
            if p >= self.len() {
                // Reached EOF without the closing quote (`id="oops`) — official
                // `unexpected_eof`.
                self.record_unexpected_eof(Span::new(from as u32, self.len() as u32));
            }
            let body = Span::new(body_start as u32, p as u32);
            let after = (p + 1).min(self.len());
            let value = if saw_brace {
                SvelteAttributeValue::Mixed(body)
            } else {
                SvelteAttributeValue::Text(body)
            };
            (Some(value), after)
        } else if b == b'{' {
            let inner_start = from + 1;
            let end = self.find_matching_brace(inner_start);
            // An UNCLOSED `{` expression value at EOF (`<button onclick={…` with no matching
            // `}`) is official `expected_token` (the brace expected its close), NOT the bare
            // truncated-tag `unexpected_eof`. Record the strict fact so the open-tag
            // fallback's fact-count guard leaves it as the authoritative code.
            if end >= self.len() {
                self.record_expected_token(Span::new(from as u32, self.len() as u32));
            }
            let inner = Span::new(inner_start as u32, end as u32);
            (
                Some(SvelteAttributeValue::Expression(inner)),
                (end + 1).min(self.len()),
            )
        } else {
            // Unquoted value: every byte that is not whitespace / `>` / quote belongs to
            // the value, mirroring official's unquoted-value reader. A `/` is an ORDINARY
            // value byte — the `/>` self-close marker only terminates the value once at
            // least one value byte has been read (`p > from`). A LEADING `/` (an `=`
            // immediately followed by `/`) is therefore consumed AS the value, so `id=/>`
            // parses as `id="/"` + a NORMAL `>` close (the element stays open ⇒
            // `element_unclosed`), NOT a self-close — whereas `id=x/>` reads value `x` then
            // self-closes at `/>`, exactly as the pinned `svelte@5.56.3` parser does.
            let mut p = from;
            while p < self.len() {
                let c = self.at(p);
                if c.is_ascii_whitespace()
                    || c == b'>'
                    || (c == b'/' && p > from && self.at(p + 1) == b'>')
                {
                    break;
                }
                p += 1;
            }
            (
                Some(SvelteAttributeValue::Text(Span::new(from as u32, p as u32))),
                p,
            )
        }
    }

    // ── Brace constructs ───────────────────────────────────────────────

    /// At a `{`, dispatch on the construct (`{#...}` block, `{@...}`/`{const}`/
    /// `{let}` tag, or plain `{expr}` interpolation).
    fn parse_brace_construct(&mut self) -> Vec<SvelteNode> {
        let start = self.pos;
        let next = self.at(self.pos + 1);
        match next {
            b'#' => self.parse_block(),
            b'@' => vec![self.parse_at_tag()],
            b'/' => {
                // Stray block-close at this scope — consume it and warn.
                let end = self.find_matching_brace(self.pos + 1);
                self.diag(
                    "unexpected-block-close",
                    "unexpected block-closing tag",
                    Span::new(start as u32, (end + 1) as u32),
                );
                self.pos = (end + 1).min(self.len());
                Vec::new()
            }
            b':' => {
                // Stray clause at this scope — consume it and warn.
                let end = self.find_matching_brace(self.pos + 1);
                self.diag(
                    "unexpected-clause",
                    "unexpected block clause",
                    Span::new(start as u32, (end + 1) as u32),
                );
                self.pos = (end + 1).min(self.len());
                Vec::new()
            }
            _ => {
                // Either a declaration tag (`{const x = …}` / `{let x = …}`) or
                // a plain interpolation `{expr}`.
                let inner_start = self.pos + 1;
                let end = self.find_matching_brace(inner_start);
                let inner = Span::new(inner_start as u32, end as u32);
                let body = self.slice(inner);
                let trimmed = body.trim_start();
                self.pos = (end + 1).min(self.len());
                if let Some(kind) = declaration_tag_kind(trimmed) {
                    let keyword_len = if matches!(kind, SvelteTagKind::Const) {
                        5
                    } else {
                        3
                    };
                    let lead_ws = body.len() - body.trim_start().len();
                    let decl_inner_start = inner.start as usize + lead_ws + keyword_len;
                    vec![SvelteNode::Tag(SvelteTag {
                        kind,
                        span: Span::new(start as u32, self.pos as u32),
                        inner: Span::new(
                            decl_inner_start.min(inner.end as usize) as u32,
                            inner.end,
                        ),
                    })]
                } else {
                    vec![SvelteNode::Interpolation(inner)]
                }
            }
        }
    }

    /// Parse a `{#...}` block.
    fn parse_block(&mut self) -> Vec<SvelteNode> {
        let start = self.pos;
        let head_inner_start = self.pos + 2; // skip `{#`
        let head_end = self.find_matching_brace(head_inner_start);
        let head = self.slice(Span::new(head_inner_start as u32, head_end as u32));
        self.pos = (head_end + 1).min(self.len());

        let mut keyword_end = 0;
        while keyword_end < head.len() && head.as_bytes()[keyword_end].is_ascii_alphabetic() {
            keyword_end += 1;
        }
        let keyword = &head[..keyword_end];
        let head_rest_start = head_inner_start + keyword_end;
        let head_rest = &head[keyword_end..];

        match keyword {
            "if" => self.parse_if_block(start, head_rest_start, head_rest),
            "each" => self.parse_each_block(start, head_rest_start, head_rest),
            "await" => self.parse_await_block(start, head_rest_start, head_rest),
            "key" => self.parse_key_block(start, head_rest_start, head_rest),
            "snippet" => self.parse_snippet_block(start, head_rest_start, head_rest),
            _ => {
                self.diag(
                    "unknown-block",
                    format!("unknown block `{{#{keyword}}}`"),
                    Span::new(start as u32, self.pos as u32),
                );
                // Best-effort: scan children until a matching `{/keyword}`.
                let kw = keyword.to_string();
                let children = self.parse_block_children(&[]);
                self.consume_block_close(&kw);
                vec![SvelteNode::Block(SvelteBlock {
                    kind: SvelteBlockKind::Key,
                    span: Span::new(start as u32, self.pos as u32),
                    head_expr: None,
                    children,
                    clauses: Vec::new(),
                })]
            }
        }
    }

    fn parse_if_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        let head_expr = nonempty_span(head_rest_start, head_rest);
        let children = self.parse_block_children(&["else", "/if"]);
        let mut clauses = Vec::new();
        loop {
            match self.peek_clause_keyword() {
                Some(kw) if kw == "else" => {
                    let (clause_kind, expr, tag_span, body) = self.parse_else_clause();
                    clauses.push(SvelteBlockClause {
                        kind: clause_kind,
                        expr,
                        tag_span,
                        children: body,
                    });
                    if matches!(clause_kind, SvelteClauseKind::Else) {
                        break;
                    }
                }
                _ => break,
            }
        }
        self.consume_block_close("if");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::If,
            span: Span::new(start as u32, self.pos as u32),
            head_expr,
            children,
            clauses,
        })]
    }

    fn parse_each_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        // `expr as item, index (key)` — `as`/item optional (the no-item form).
        let (list_expr, item, index, key) =
            super::block_head::parse_each_head(head_rest_start, head_rest);
        let children = self.parse_block_children(&["else", "/each"]);
        let mut clauses = Vec::new();
        if matches!(self.peek_clause_keyword().as_deref(), Some("else")) {
            let (_kind, _expr, tag_span, body) = self.parse_else_clause();
            clauses.push(SvelteBlockClause {
                kind: SvelteClauseKind::Else,
                expr: None,
                tag_span,
                children: body,
            });
        }
        self.consume_block_close("each");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Each { item, index, key },
            span: Span::new(start as u32, self.pos as u32),
            head_expr: list_expr,
            children,
            clauses,
        })]
    }

    fn parse_await_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        // `{#await expr}` or `{#await expr then v}` or `{#await expr catch e}`.
        let trimmed = head_rest.trim();
        let (promise_expr, then_inline, catch_inline) =
            super::block_head::parse_await_head(head_rest_start, head_rest);
        let _ = trimmed;
        let mut then_binding = then_inline;
        let mut catch_binding = catch_inline;
        let children = self.parse_block_children(&["then", "catch", "/await"]);
        let mut clauses = Vec::new();
        loop {
            match self.peek_clause_keyword().as_deref() {
                Some("then") => {
                    let (binding, tag_span, body) = self.parse_then_or_catch("then");
                    then_binding = binding.or(then_binding);
                    clauses.push(SvelteBlockClause {
                        kind: SvelteClauseKind::Then,
                        expr: binding,
                        tag_span,
                        children: body,
                    });
                }
                Some("catch") => {
                    let (binding, tag_span, body) = self.parse_then_or_catch("catch");
                    catch_binding = binding.or(catch_binding);
                    clauses.push(SvelteBlockClause {
                        kind: SvelteClauseKind::Catch,
                        expr: binding,
                        tag_span,
                        children: body,
                    });
                }
                _ => break,
            }
        }
        self.consume_block_close("await");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Await {
                then_binding,
                catch_binding,
            },
            span: Span::new(start as u32, self.pos as u32),
            head_expr: promise_expr,
            children,
            clauses,
        })]
    }

    fn parse_key_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        let head_expr = nonempty_span(head_rest_start, head_rest);
        let children = self.parse_block_children(&["/key"]);
        self.consume_block_close("key");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Key,
            span: Span::new(start as u32, self.pos as u32),
            head_expr,
            children,
            clauses: Vec::new(),
        })]
    }

    fn parse_snippet_block(
        &mut self,
        start: usize,
        head_rest_start: usize,
        head_rest: &str,
    ) -> Vec<SvelteNode> {
        // `name(params)`
        let (name_span, name_text, params) =
            super::block_head::parse_snippet_head(head_rest_start, head_rest);
        let children = self.parse_block_children(&["/snippet"]);
        self.consume_block_close("snippet");
        vec![SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Snippet {
                name: name_span,
                name_text,
                params,
            },
            span: Span::new(start as u32, self.pos as u32),
            head_expr: None,
            children,
            clauses: Vec::new(),
        })]
    }

    /// Parse a block body run, stopping at any of the `stoppers` clause/close
    /// keywords (without the `{:`/`{/` prefix; `/if` etc. denote the close).
    fn parse_block_children(&mut self, _stoppers: &[&str]) -> Vec<SvelteNode> {
        let mut children = Vec::new();
        let mut text_start = self.pos;
        while !self.eof() {
            let b = self.cur();
            if b == b'{' {
                if self.is_block_close_or_clause() {
                    if text_start < self.pos {
                        children.push(SvelteNode::Text(Span::new(
                            text_start as u32,
                            self.pos as u32,
                        )));
                    }
                    return children;
                }
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                let nodes = self.parse_brace_construct();
                children.extend(nodes);
                text_start = self.pos;
            } else if b == b'<' {
                if text_start < self.pos {
                    children.push(SvelteNode::Text(Span::new(
                        text_start as u32,
                        self.pos as u32,
                    )));
                }
                if self.starts_with_at(self.pos, b"<!--") {
                    let c = self.parse_comment();
                    children.push(c);
                } else {
                    // NESTED child scan (inside an element or a block clause): a root-only
                    // `<svelte:*>` meta tag here is NOT at the component root.
                    let nodes = self.parse_element_or_recover(false);
                    children.extend(nodes);
                }
                text_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        if text_start < self.pos {
            children.push(SvelteNode::Text(Span::new(
                text_start as u32,
                self.pos as u32,
            )));
        }
        children
    }

    /// Whether the brace at `self.pos` opens a block clause (`{:`) or close
    /// (`{/`).
    fn is_block_close_or_clause(&self) -> bool {
        self.cur() == b'{' && (self.at(self.pos + 1) == b':' || self.at(self.pos + 1) == b'/')
    }

    /// Peek the keyword of a `{:keyword ...}` clause at `self.pos`, without
    /// consuming.
    fn peek_clause_keyword(&self) -> Option<String> {
        if self.cur() != b'{' || self.at(self.pos + 1) != b':' {
            return None;
        }
        let mut p = self.pos + 2;
        let kw_start = p;
        while p < self.len() && self.at(p).is_ascii_alphabetic() {
            p += 1;
        }
        self.text.get(kw_start..p).map(|s| s.to_string())
    }

    /// Parse an `{:else}` / `{:else if expr}` clause and its body.
    ///
    /// Returns the clause kind, the optional condition span, the clause-tag head
    /// span (`{:else…}` INCLUDING braces — overwritten verbatim by the
    /// projector), and the body.
    fn parse_else_clause(&mut self) -> (SvelteClauseKind, Option<Span>, Span, Vec<SvelteNode>) {
        // at `{:else...}`
        let tag_start = self.pos;
        let inner_start = self.pos + 2;
        let head_end = self.find_matching_brace(inner_start);
        let head = self.slice(Span::new(inner_start as u32, head_end as u32));
        self.pos = (head_end + 1).min(self.len());
        let tag_span = Span::new(tag_start as u32, self.pos as u32);
        let rest = head.trim_start_matches("else");
        let trimmed = rest.trim_start();
        let (kind, expr) = if let Some(after_if) = trimmed.strip_prefix("if") {
            let expr_text = after_if.trim();
            let expr_offset = inner_start
                + (head.len() - rest.len())
                + (rest.len() - trimmed.len())
                + 2
                + (after_if.len() - after_if.trim_start().len());
            (
                SvelteClauseKind::ElseIf,
                nonempty_span(expr_offset, expr_text),
            )
        } else {
            (SvelteClauseKind::Else, None)
        };
        let body = self.parse_block_children(&["else", "/if", "/each"]);
        (kind, expr, tag_span, body)
    }

    /// Parse a `{:then v}` / `{:catch e}` clause and its body.
    ///
    /// Returns the optional binding span, the clause-tag head span (`{:then…}` /
    /// `{:catch…}` INCLUDING braces — overwritten verbatim by the projector),
    /// and the body.
    fn parse_then_or_catch(&mut self, keyword: &str) -> (Option<Span>, Span, Vec<SvelteNode>) {
        let tag_start = self.pos;
        let inner_start = self.pos + 2;
        let head_end = self.find_matching_brace(inner_start);
        let head = self.slice(Span::new(inner_start as u32, head_end as u32));
        self.pos = (head_end + 1).min(self.len());
        let tag_span = Span::new(tag_start as u32, self.pos as u32);
        let rest = head.trim_start().trim_start_matches(keyword);
        let binding_text = rest.trim();
        let offset =
            inner_start + (head.len() - rest.len()) + (rest.len() - rest.trim_start().len());
        let binding = nonempty_span(offset, binding_text);
        let body = self.parse_block_children(&["then", "catch", "/await"]);
        (binding, tag_span, body)
    }

    /// Consume the matching `{/keyword}` close, warning if it is missing.
    fn consume_block_close(&mut self, keyword: &str) {
        if self.cur() == b'{' && self.at(self.pos + 1) == b'/' {
            let inner_start = self.pos + 2;
            let head_end = self.find_matching_brace(inner_start);
            let name = self.slice(Span::new(inner_start as u32, head_end as u32));
            if name.trim() == keyword {
                self.pos = (head_end + 1).min(self.len());
                return;
            }
        }
        self.diag(
            "unterminated-block",
            format!("missing `{{/{keyword}}}` close"),
            Span::new(
                self.pos.min(self.len()) as u32,
                self.pos.min(self.len()) as u32,
            ),
        );
    }

    /// Parse an `{@...}` tag.
    fn parse_at_tag(&mut self) -> SvelteNode {
        let start = self.pos;
        let inner_start = self.pos + 1; // skip `{`, keep `@`
        let end = self.find_matching_brace(inner_start);
        let inner = self.slice(Span::new(inner_start as u32, end as u32));
        self.pos = (end + 1).min(self.len());
        // inner starts with `@keyword`
        let after_at = &inner[1..];
        let mut kw_end = 0;
        while kw_end < after_at.len() && after_at.as_bytes()[kw_end].is_ascii_alphabetic() {
            kw_end += 1;
        }
        let keyword = &after_at[..kw_end];
        let kind = match keyword {
            "render" => SvelteTagKind::Render,
            "html" => SvelteTagKind::Html,
            "const" => SvelteTagKind::LegacyConst,
            "debug" => SvelteTagKind::Debug,
            "attach" => SvelteTagKind::Attach,
            _ => SvelteTagKind::Unknown,
        };
        if matches!(kind, SvelteTagKind::Unknown) {
            self.diag(
                "unknown-tag",
                format!("unknown tag `{{@{keyword}}}`"),
                Span::new(start as u32, self.pos as u32),
            );
        }
        // The inner expression span begins after `@keyword` plus separating ws.
        let body_after_kw = &after_at[kw_end..];
        let lead = body_after_kw.len() - body_after_kw.trim_start().len();
        let expr_start = inner_start + 1 + kw_end + lead;
        let expr_end = end;
        SvelteNode::Tag(SvelteTag {
            kind,
            span: Span::new(start as u32, self.pos as u32),
            inner: Span::new(expr_start.min(expr_end) as u32, expr_end as u32),
        })
    }

    /// Find the matching closing `}` for a brace opened just before
    /// `inner_start` (i.e. `inner_start` is the first inner byte). Returns the
    /// index of the closing `}` (or EOF). STRING- AND COMMENT-AWARE so a `}`
    /// inside a quote, a `//` line comment, a `/* */` block comment, or a regex
    /// literal does not close the interpolation early.
    ///
    /// Delegates to the shared [`find_matching_brace_in`] free function so the
    /// JS-aware brace scan is ONE implementation reused by the tokenizer and the
    /// runtime mixed-attribute lowering (no second hand-rolled brace scanner).
    fn find_matching_brace(&self, inner_start: usize) -> usize {
        find_matching_brace_in(self.src, inner_start)
    }
}

// ── Free helpers ───────────────────────────────────────────────────────

/// The EXPRESSION span of an attribute-position `{@attach expr}` body, or `None`
/// when the brace body is not an `@attach` tag. `inner` is the brace-inner span
/// (braces excluded). The keyword scan mirrors the child-form tag parse
/// (`parse_at_tag`): the `@` is followed by the ASCII-alphabetic keyword run, which
/// must be exactly `attach` (`@attachment` scans the longer keyword and does NOT
/// match); the expression begins after the keyword plus any separating whitespace
/// and runs to the closing brace. An empty expression (`{@attach}`) yields an empty
/// span — the downstream expression parse fails it closed.
fn attach_attribute_expr_span(src: &[u8], inner: Span) -> Option<Span> {
    let text = std::str::from_utf8(&src[inner.start as usize..inner.end as usize]).ok()?;
    // Allow leading whitespace inside the braces (consistent with the brace-attribute
    // body dispatch, which trims before sigil-matching).
    let lead_ws = text.len() - text.trim_start().len();
    let body = &text[lead_ws..];
    let after_at = body.strip_prefix('@')?;
    let kw_end = after_at
        .bytes()
        .position(|b| !b.is_ascii_alphabetic())
        .unwrap_or(after_at.len());
    if &after_at[..kw_end] != "attach" {
        return None;
    }
    // The expression starts after `@attach` plus separating whitespace.
    let after_kw = &after_at[kw_end..];
    let expr_lead = after_kw.len() - after_kw.trim_start().len();
    let expr_start = inner.start as usize + lead_ws + 1 + kw_end + expr_lead;
    Some(Span::new(
        expr_start.min(inner.end as usize) as u32,
        inner.end,
    ))
}

/// Read a quoted/text attribute value by name (case-insensitive) from a parsed
/// attribute list, returning the value text.
fn attr_text_value(attrs: &[SvelteAttribute], parser: &SvelteParser, name: &str) -> Option<String> {
    attrs.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain { name: n, value, .. } if n.eq_ignore_ascii_case(name) => {
            match value {
                Some(SvelteAttributeValue::Text(span)) => Some(parser.slice(*span).to_string()),
                Some(SvelteAttributeValue::Expression(span)) => {
                    Some(parser.slice(*span).to_string())
                }
                Some(SvelteAttributeValue::Mixed(span)) => Some(parser.slice(*span).to_string()),
                None => Some("true".to_string()),
            }
        }
        _ => None,
    })
}

/// Whether a `<script>`'s attributes mark it as the MODULE script — a valueless
/// `module` flag OR a `context="module"` text attribute. The attribute NAME is matched
/// CASE-SENSITIVELY (official's `attribute.name === 'module'` / `=== 'context'`), so a
/// capitalized `Module` / `Context` does NOT mark a module script (it is an unknown
/// attribute, leaving the script as an instance script).
fn script_attr_marks_module(attrs: &[SvelteAttribute], parser: &SvelteParser) -> bool {
    attrs.iter().any(|a| match &a.kind {
        SvelteAttributeKind::Plain { name, value, .. } if name == "module" => value.is_none(),
        SvelteAttributeKind::Plain { name, value, .. } if name == "context" => {
            matches!(value, Some(SvelteAttributeValue::Text(span)) if parser.slice(*span) == "module")
        }
        _ => false,
    })
}

/// The STATIC value of a `<svelte:options>` attribute, mirroring upstream's `get_static_value`
/// (`read/options.js`) restricted to what the parse domain can statically determine. A shorthand
/// (no value) is `True`; a Text value is its string `Str`; an EXPRESSION value carries a bare
/// boolean / string literal as `Bool` / `Str`, and any non-literal (an identifier, a number, an
/// operation, a mixed value) is `Dynamic` (upstream's `chunk.expression.type !== 'Literal'` →
/// `null`).
enum OptionsStaticValue {
    /// A boolean-shorthand attribute (no value) — upstream's `value === true`.
    True,
    /// A boolean literal `{true}` / `{false}` (the value itself is consumed elsewhere; here only
    /// its boolean-ness matters for `get_boolean_value` validity).
    Bool,
    /// A static string — a Text value, or a single-string-literal expression.
    Str(String),
    /// A non-statically-resolvable value (an identifier, number, operation, or mixed value) —
    /// upstream's `null` static value.
    Dynamic,
}

/// Classify a `<svelte:options>` attribute value into its [`OptionsStaticValue`], faithful to
/// upstream `get_static_value`. The expression inner text is recognised ONLY as a single complete
/// boolean / string literal (the whole trimmed inner is exactly `true` / `false` / `'…'` / `"…"`);
/// anything else (an identifier, a number, an operation, a mixed value) is `Dynamic`.
fn options_static_value(text: &str, value: &Option<SvelteAttributeValue>) -> OptionsStaticValue {
    match value {
        None => OptionsStaticValue::True,
        Some(SvelteAttributeValue::Text(span)) => {
            OptionsStaticValue::Str(text[span.start as usize..span.end as usize].to_string())
        }
        Some(SvelteAttributeValue::Mixed(_)) => OptionsStaticValue::Dynamic,
        Some(SvelteAttributeValue::Expression(span)) => {
            let inner = text[span.start as usize..span.end as usize].trim();
            match inner {
                "true" | "false" => OptionsStaticValue::Bool,
                _ => match parse_single_string_literal(inner) {
                    Some(s) => OptionsStaticValue::Str(s),
                    None => OptionsStaticValue::Dynamic,
                },
            }
        }
    }
}

/// If `inner` is EXACTLY a single quoted string literal (`'…'` or `"…"` with the matching close
/// quote at the very end and no escapes / trailing tokens), return its string contents; otherwise
/// `None` (a non-literal expression, faithful to upstream treating only a bare `Literal` as a
/// static value). Escapes are not modelled — an option string value (a namespace / css / tag) is
/// a plain identifier-like token in practice; an escaped form falls through to `Dynamic`, which is
/// a SAFE direction (it can only DROP an accept into an unsupported-feature refusal, never mint a
/// wrong reject code for an officially-accepted input — the only such strings are `svg` / `mathml`
/// / `html` / `injected`, none of which need escapes).
fn parse_single_string_literal(inner: &str) -> Option<String> {
    let bytes = inner.as_bytes();
    let &first = bytes.first()?;
    if first != b'\'' && first != b'"' {
        return None;
    }
    if bytes.len() < 2 || *bytes.last()? != first {
        return None;
    }
    let body = &inner[1..inner.len() - 1];
    // No inner unescaped matching quote (which would mean it is not a single literal).
    if body.as_bytes().contains(&first) {
        return None;
    }
    Some(body.to_string())
}

/// Whether a `<svelte:options>` boolean axis (`runes` / `immutable` / `preserveWhitespace` /
/// `accessors`) value is a BOOLEAN, mirroring upstream's `get_boolean_value` (a non-boolean
/// `get_static_value` is `svelte_options_invalid_attribute_value`). The shorthand `value === true`
/// counts as boolean.
fn options_value_is_boolean(text: &str, value: &Option<SvelteAttributeValue>) -> bool {
    matches!(
        options_static_value(text, value),
        OptionsStaticValue::True | OptionsStaticValue::Bool
    )
}

/// Whether a `<svelte:options namespace>` value is a VALID namespace, mirroring upstream
/// (`get_static_value` in {`html`, `svg`, `mathml`} OR the SVG / MathML namespace URLs). A
/// shorthand / boolean / dynamic value is invalid (the static value is not one of the strings).
fn options_namespace_is_valid(text: &str, value: &Option<SvelteAttributeValue>) -> bool {
    const NAMESPACE_SVG: &str = "http://www.w3.org/2000/svg";
    const NAMESPACE_MATHML: &str = "http://www.w3.org/1998/Math/MathML";
    match options_static_value(text, value) {
        OptionsStaticValue::Str(s) => {
            matches!(s.as_str(), "html" | "svg" | "mathml")
                || s == NAMESPACE_SVG
                || s == NAMESPACE_MATHML
        }
        _ => false,
    }
}

/// Whether a `<svelte:options css>` value is the VALID `injected` (the only accepted value
/// upstream). A shorthand / boolean / dynamic / other-string value is invalid. The static
/// value mirrors upstream `get_static_value`: a Text value (`css="injected"`) or a single
/// static STRING-LITERAL expression (`css={'injected'}` / `css={"injected"}`) — the ONE
/// authority for the `css === 'injected'` check, shared by the options official-reject
/// classification and the runtime css-output-mode detection so the two can never diverge.
pub(crate) fn options_css_is_injected(text: &str, value: &Option<SvelteAttributeValue>) -> bool {
    matches!(options_static_value(text, value), OptionsStaticValue::Str(s) if s == "injected")
}
