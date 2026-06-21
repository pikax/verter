//! The OFFICIAL-REJECT validation gate — the core of the "official rejects ⇒ Verter
//! must reject" parity quadrant.
//!
//! The structural shape allowlists (the element/attr allowlist, the instance-script
//! item allowlist) prove a surface is one Verter EMITS; they do NOT prove the input
//! is one the official `svelte@5.56.3` compiler ACCEPTS. A §1.2-core-shaped input
//! that official COMPILE-ERRORS (a duplicate declaration, a `$`-prefixed binding, a
//! duplicate / mis-`context`-ed `<script>`, an invalid HTML placement, a global
//! `$foo` reference) must therefore ALSO fail closed in Verter — accepting malformed
//! Svelte changes the observable contract from "compile error, no module" to "module
//! exists", which is not behaviorally identical.
//!
//! This module owns:
//! - [`CoreOfficialValidationRule`] — the typed taxonomy of the official-error
//!   classes the §1.2 core surface must reject, with an exhaustive
//!   [`CoreOfficialValidationRule::ALL`] list;
//! - [`official_reject_gate`] — the analysis-domain detector for the rules that
//!   Verter did not previously fail closed (script duplicate / invalid context,
//!   dollar-prefixed bindings, duplicate accepted declarations, invalid HTML
//!   placement, global `$foo` / `$$foo` references) driven EXCLUSIVELY from the typed
//!   parse + the OXC AST (never a raw-source heuristic);
//! - [`CoreOfficialValidationRule::from_unsupported_surface`] — the mapping from the
//!   already-fail-closed [`UnsupportedSvelteRuntimeSurface`] codes that ALSO
//!   correspond to an official-reject class (duplicate attribute, invalid
//!   `<svelte:options>`, magic identifier) so the reject-parity matrix can classify
//!   every committed reject row to one rule.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use rustc_hash::FxHashSet;

use super::expr::{collect_pattern_names, reparse_module, ShadowStack};
use super::official_rule::{CoreOfficialValidationRule, OfficialRejection};
use crate::svelte::parser::{
    CloseTagViolationKind, ParsedSvelte, ScriptBodyGrammar, SvelteAttribute, SvelteAttributeKind,
    SvelteAttributeValue, SvelteElement, SvelteElementKind, SvelteNode, SvelteParseRejectKind,
};

/// Run the OFFICIAL-REJECT analysis gate over a parsed component: detect the
/// official-error classes Verter did not previously fail closed, returning the FIRST
/// rule violated (with its exact official code), or `None` when the component is free of
/// these classes.
///
/// Runs EARLY in `compile_client` (before the unsupported-feature classifier) so a
/// genuinely-malformed input is rejected for being malformed — not later
/// mis-attributed to an unsupported feature. Driven EXCLUSIVELY from the typed parse
/// (`ParsedSvelte`, including its `strict_parse_errors` fact stream) + the OXC AST of
/// each script/template expression; never a raw `<script` byte-scan or a regex over
/// type text.
#[must_use]
pub fn official_reject_gate(source: &str, parsed: &ParsedSvelte) -> Option<OfficialRejection> {
    // ─── PARSE PHASE (official `phases/1-parse`) — the SINGLE parser-owned,
    // encounter-ordered defect stream is the SOLE parse-error arbitration source. Every
    // parse defect (close-tag / strict-parse / script-domain / explicit-`</p>` autoclose)
    // was recorded with an `encounter_order` minted at its DISCOVERY point in the parser's
    // single forward pass; the gate selects the FIRST-discovered (minimum `encounter_order`)
    // unsuppressed defect, matching official, which stops at the first parse error. Source
    // span is the report ANCHOR only — it NEVER arbitrates. ───
    if let Some(rejection) = select_parse_phase_defect(source, parsed) {
        return Some(rejection);
    }

    // ─── ANALYZE PHASE (official `phases/2-analyze`) — runs ONLY on a CLEAN parse (the
    // parse-defect stream above was empty). Ordered by upstream PASS order (NOT span):
    // (a) the script-scope / global `$`-reference checks (`scope.js` + the store-subscription
    // guard) run in the module → instance walk, BEFORE (b) the template-walk
    // `node_invalid_placement`. So a script declaration / global-reference defect wins over a
    // concurrent template placement defect. ───

    // The accepted top-level local names (declared in either script) — the referents a
    // `$foo` store-style reference / `bind:this` target may legally name. Collected
    // once for the reference scans.
    let declared = declared_top_level_locals(source, parsed);

    // (a.1) Script name rules (`scope.js`): a `$`-prefixed declaration
    // (`dollar_prefix_invalid`). Scanned over each script's top-level declarators. (A
    // same-lexical-scope `let`/`const` redeclaration is a PARSE-phase `js_parse_error` owned by
    // the body-probe slot, not an analyze rule, so it is not re-detected here.)
    for script_src in script_sources(source, parsed) {
        if let Some(rejection) = scan_script_declaration_rules(script_src) {
            return Some(rejection);
        }
    }

    // (a.2) Global `$foo` / `$$foo` references in script + template + bind + event
    // positions. A `$foo` is a violation only when `foo` is NOT a declared accepted local
    // AND lowercase-initial (`global_reference_invalid`); a `$$foo` (non-reserved) is always
    // `global_reference_invalid`; the reserved magic objects carry their EXACT runes-mode
    // codes (`$$props` → `legacy_props_invalid`, `$$restProps` → `legacy_rest_props_invalid`).
    // This scan ALSO covers a `$`-prefixed `bind:this={$foo}` target (the directive value is
    // one of the scanned template expression sources). (`$$slots`, which official ACCEPTS, is
    // a deferrable unsupported feature — never an official reject here.)
    if let Some(rejection) = scan_global_dollar_references(source, parsed, &declared) {
        return Some(rejection);
    }

    // (b) The template-walk `node_invalid_placement` — the disallowed-descendant REPAIR
    // families (a nested `<a>` / `<button>`, a heading-in-heading). Runs LAST in the analyze
    // phase (after the script-scope / global-reference checks) and ONLY on a clean parse (the
    // explicit-`</p>` autoclose is now a PARSE defect minted by the parser, so it is excluded
    // from this scan).
    if let Some(rule) = scan_html_placement(&parsed.template, &mut Vec::new()) {
        return Some(OfficialRejection::of(rule));
    }

    None
}

/// Select the FIRST-discovered (minimum `encounter_order`) unsuppressed PARSE defect from
/// the parser's three encounter-ordered fact rails — the SOLE parse-error arbitration
/// source. `span` (the report anchor on each fact) NEVER participates; arbitration is
/// purely by `encounter_order`, the parser's single forward-pass discovery sequence.
///
/// The rails:
/// - the parser-recorded [`CloseTagViolation`]s — an unclosed intrinsic element
///   (`element_unclosed`), a stray / mismatched close (`element_invalid_closing_tag`), or a
///   void-content close (`void_element_invalid_content`). An `Unclosed` for a `<p>` that is
///   in an IMPLICIT-autoclose situation (a direct disallowed block child, NO explicit
///   `</p>`) is SUPPRESSED — official AUTO-CLOSES it and ACCEPTS, so that `<p>` is a
///   deferrable unsupported FEATURE downstream, never `element_unclosed`;
/// - the strict-parse facts ([`ParsedSvelte::strict_parse_errors`]) — the single broad
///   [`CoreOfficialValidationRule::ParserStrictness`] rule, carrying the exact official code;
/// - the parse-domain reject facts ([`ParsedSvelte::parse_reject_facts`]) — the `<script>`
///   attribute / duplicate-script rejects and the explicit-`</p>` autoclose, each minted at
///   its parser discovery point with its exact official code.
///
/// [`CloseTagViolation`]: crate::svelte::parser::CloseTagViolation
/// [`ParsedSvelte::strict_parse_errors`]: crate::svelte::parser::ParsedSvelte::strict_parse_errors
/// [`ParsedSvelte::parse_reject_facts`]: crate::svelte::parser::ParsedSvelte::parse_reject_facts
fn select_parse_phase_defect(source: &str, parsed: &ParsedSvelte) -> Option<OfficialRejection> {
    // The `<p>` elements in an IMPLICIT-autoclose situation (a direct disallowed block child
    // but NO explicit `</p>`) — their parser-reported `Unclosed` is a FEATURE, not a reject,
    // so it is suppressed below. (The EXPLICIT-`</p>` autoclose is a parse_reject_fact, not
    // an Unclosed.)
    let implicit_autoclose_p_spans =
        collect_implicit_autoclose_paragraph_open_spans(&parsed.template);

    // Track the minimum-`encounter_order` unsuppressed defect across all three rails.
    let mut best: Option<(u32, OfficialRejection)> = None;
    let mut consider = |order: u32, rejection: OfficialRejection| {
        if best.is_none_or(|(o, _)| order < o) {
            best = Some((order, rejection));
        }
    };

    for v in &parsed.close_tag_violations {
        let rule = match v.kind {
            CloseTagViolationKind::Unclosed => {
                // Suppress an `Unclosed` for a `<p>` official auto-closes (the implicit-
                // autoclose feature case) — it is not `element_unclosed`.
                if implicit_autoclose_p_spans.contains(&v.span.start) {
                    continue;
                }
                CoreOfficialValidationRule::ElementUnclosed
            }
            CloseTagViolationKind::InvalidClosingTag => {
                CoreOfficialValidationRule::ElementInvalidClosingTag
            }
            CloseTagViolationKind::VoidElementInvalidContent => {
                CoreOfficialValidationRule::VoidElementInvalidContent
            }
        };
        consider(v.encounter_order, OfficialRejection::of(rule));
    }

    for fact in &parsed.strict_parse_errors {
        consider(
            fact.encounter_order,
            OfficialRejection {
                rule: CoreOfficialValidationRule::ParserStrictness,
                official_code: fact.official_code,
            },
        );
    }

    for fact in &parsed.parse_reject_facts {
        let rule = match fact.kind {
            SvelteParseRejectKind::ScriptReservedAttribute => {
                CoreOfficialValidationRule::ScriptReservedAttribute
            }
            SvelteParseRejectKind::ScriptInvalidContext => {
                CoreOfficialValidationRule::ScriptInvalidContext
            }
            SvelteParseRejectKind::ScriptDuplicate => CoreOfficialValidationRule::ScriptDuplicate,
            SvelteParseRejectKind::StyleDuplicate => CoreOfficialValidationRule::StyleDuplicate,
            SvelteParseRejectKind::AttributeDuplicate => {
                CoreOfficialValidationRule::AttributeDuplicate
            }
            // A duplicate / nested root-only `<svelte:*>` meta element, OR an invalid
            // `<svelte:options>` attribute / child-content (the `read_options` finalization) —
            // all ride the `OptionsInvalid` rule (the meta-element class), carrying the exact site
            // code (`svelte_meta_duplicate` / `svelte_meta_invalid_placement` /
            // `svelte_options_*` / `svelte_meta_invalid_content`) per fact.
            SvelteParseRejectKind::SvelteMetaDuplicate
            | SvelteParseRejectKind::SvelteMetaInvalidPlacement
            | SvelteParseRejectKind::OptionsInvalid => CoreOfficialValidationRule::OptionsInvalid,
            SvelteParseRejectKind::ParagraphAutoclose => {
                CoreOfficialValidationRule::ElementInvalidClosingTagAutoclosed
            }
            // The body-parse reject is NOT carried as a parse_reject_fact (the parser does not
            // run OXC); it is filled from the RESERVED body-probe slots below.
            SvelteParseRejectKind::ScriptBodyParse => continue,
        };
        consider(
            fact.encounter_order,
            OfficialRejection::with_code(rule, fact.official_code),
        );
    }

    // FILL the RESERVED script-body-parse slots: parse each script body ONCE with OXC at the
    // probe's grammar (plain `<script>` = JS, `lang="ts"` = TS). A parse FAILURE mints
    // `js_parse_error` at the probe's RESERVED `encounter_order` (the upstream-faithful
    // body-parse position — strictly after the open-tag attribute-duplicate, before the
    // source-order semantic-attr faults — NOT the body span or this execution time). A body
    // that parses CLEAN contributes NO defect.
    for probe in &parsed.script_body_probes {
        let body = &source[probe.body_span.start as usize..probe.body_span.end as usize];
        if script_body_fails_to_parse(body, probe.grammar) {
            consider(
                probe.encounter_order,
                OfficialRejection::with_code(
                    CoreOfficialValidationRule::ScriptBodyParse,
                    "js_parse_error",
                ),
            );
        }
    }

    // FILL the RESERVED style-body-parse slots: run the faithful `read/style.js` CSS body reader
    // from each `<style>`'s content-start. A CSS body parse FAILURE mints the EXACT upstream CSS
    // parse code (`css_expected_identifier` / `css_empty_declaration` / `css_selector_invalid` /
    // `expected_token` / `unexpected_eof`) at the probe's RESERVED `encounter_order` — the
    // upstream `read_style` position, BEFORE the `style_duplicate` check — so a malformed 2nd
    // (or 1st) style body wins the first-error race over the duplicate. A body that parses CLEAN
    // contributes NO defect (the later `style_duplicate` / unsupported-`<style>` rail wins). The
    // reader parses from the ORIGINAL source cursor (NOT an isolated slice): upstream's nested
    // CSS readers run PAST `</style>` into the rest of the source, and that decides the code.
    for probe in &parsed.style_body_probes {
        if let Some(code) =
            super::css_reject::css_body_parse_error(source, probe.content_start as usize)
        {
            consider(
                probe.encounter_order,
                OfficialRejection::with_code(CoreOfficialValidationRule::StyleBodyParse, code),
            );
        }
    }

    // FILL the RESERVED `<svelte:options customElement={EXPR}>` validation slots: parse each
    // expression with OXC and run upstream's two checks. A SYNTACTIC attribute-expression PARSE
    // fault — a malformed prefix (`js_parse_error`) OR a clean prefix with trailing junk before the
    // `}` (`expected_token`) — mints AT THE PARSE POSITION (`parse_encounter_order` — upstream's
    // `read_expression` runs during the `<svelte:options>` attribute loop, so it beats a LATER
    // template defect / duplicate attribute and loses to an EARLIER one); a parseable-and-fully-
    // consumed-but-invalid expression mints the EXACT `svelte_options_*` code AT THE FINALIZATION
    // POSITION (`encounter_order` — upstream's `read_options` runs after the whole template parse,
    // losing to ANY template parse defect). An expression upstream ACCEPTS (a `null` literal, a
    // valid object) contributes NO defect (refused later as the unsupported `customElement` feature).
    for probe in &parsed.options_custom_element_probes {
        let expr_src = &source[probe.expr_span.start as usize..probe.expr_span.end as usize];
        if let Some(code) = super::options_reject::options_custom_element_expr_error(expr_src) {
            let order = if is_options_ce_attribute_parse_fault(code) {
                probe.parse_encounter_order
            } else {
                probe.encounter_order
            };
            consider(
                order,
                OfficialRejection::with_code(CoreOfficialValidationRule::OptionsInvalid, code),
            );
        }
    }

    best.map(|(_, rejection)| rejection)
}

/// Whether a `customElement={EXPR}` disposition code is a SYNTACTIC attribute-expression PARSE fault
/// — one upstream raises during the `<svelte:options>` attribute loop (`read_expression`), so it
/// rides the attribute's source position (`parse_encounter_order`) rather than the `read_options`
/// finalization position. The two parse-phase codes are `js_parse_error` (a malformed prefix /
/// empty inner) and `expected_token` (a clean prefix with trailing junk before the `}` — the
/// missing brace). Every `svelte_options_*` code is a finalization VALIDATION fault (NOT this).
fn is_options_ce_attribute_parse_fault(code: &str) -> bool {
    matches!(code, "js_parse_error" | "expected_token")
}

/// Whether a `<script>` body FAILS to parse the way upstream's Acorn parse does — the
/// body-probe fill. A plain `<script>` parses as JS (`SourceType::mjs()` — module JS, no TS,
/// no JSX, the Acorn-equivalent: TS-only syntax in a plain script is a parse error); a
/// `lang="ts"` body parses as TS (`SourceType::ts()`). A panic OR a non-empty parser error
/// set is a failure (`js_parse_error`).
///
/// Plus the ONE parse-phase error OXC's PARSER defers to its binder but Acorn raises at parse:
/// a same-scope LEXICAL (`let`/`const`) REDECLARATION (`let a; let a`). It is detected
/// structurally on the parsed program's TOP-LEVEL declarators (the §1.2-core script surface is
/// top-level `let`/`const`), so it stays a body-slot `js_parse_error` — never a later analyze
/// fallback. Driven from the typed [`ScriptBodyProbe`] grammar + the typed AST, never a text
/// heuristic.
fn script_body_fails_to_parse(body: &str, grammar: ScriptBodyGrammar) -> bool {
    let alloc = Allocator::default();
    let source_type = match grammar {
        ScriptBodyGrammar::Js => oxc_span::SourceType::mjs(),
        ScriptBodyGrammar::Ts => oxc_span::SourceType::ts(),
    };
    let parsed = oxc_parser::Parser::new(&alloc, alloc.alloc_str(body), source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return true;
    }
    top_level_lexical_redeclaration(&parsed.program)
}

/// Whether the program's TOP-LEVEL declarations contain a same-scope LEXICAL redeclaration —
/// a name bound by `let` / `const` that is also bound by ANOTHER top-level `let` / `const` /
/// `var` declarator (the ECMAScript early SyntaxError Acorn raises at parse but OXC's parser
/// defers to its binder). `var`/`var` re-binding of the same name (legal in JS) is NOT a
/// redeclaration.
///
/// SCOPE (deliberate, NOT an over-claim): this detects ONLY the `let` / `const` redeclaration
/// reachable in the §1.2-core SUPPORTED script surface (top-level `let` / `const` — `$state` /
/// props-destructure / `bind:this` locals). A redeclaration involving a top-level FUNCTION /
/// CLASS / IMPORT declaration (`function f(){} function f(){}`, `class A{} class A{}`,
/// `import x; let x`) — which upstream also `js_parse_error`s — is NOT detected here and does not
/// need to be: a top-level function / class / import is itself OUTSIDE the §1.2-core allowlist, so
/// such a component fails closed as an unsupported FEATURE BEFORE this body-probe code matters. So
/// no REACHABLE official-reject in the supported surface is missed (characterized by
/// `redeclaration_scope_is_let_const_only_function_collisions_fail_closed`).
fn top_level_lexical_redeclaration(program: &Program) -> bool {
    use oxc_ast::ast::VariableDeclarationKind;
    // (name, was_lexical) in source order across the top-level variable declarators.
    let mut bound: Vec<(String, bool)> = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        let lexical = matches!(
            decl.kind,
            VariableDeclarationKind::Let | VariableDeclarationKind::Const
        );
        for d in &decl.declarations {
            let mut names = Vec::new();
            collect_pattern_names(&d.id, &mut names);
            for name in names {
                // A collision is a redeclaration error when EITHER the prior or the current
                // binding is lexical (`let`/`const`); two `var`s of the same name are legal.
                if let Some((_, prior_lexical)) = bound.iter().find(|(n, _)| *n == name) {
                    if *prior_lexical || lexical {
                        return true;
                    }
                }
                bound.push((name, lexical));
            }
        }
    }
    false
}

/// The module + instance script content sources (the inner text of each `<script>`), in
/// MODULE-then-INSTANCE order — matching upstream's analyze pass, which constructs the module
/// scope before the instance scope (`phases/2-analyze/index.js`) and walks
/// `[module, instance, template]`. So a module-script defect (a global `$foo` reference, a
/// `$`-prefixed binding) is reported BEFORE an instance-script defect. Empty when neither
/// script is present.
fn script_sources<'a>(source: &'a str, parsed: &ParsedSvelte) -> Vec<&'a str> {
    let mut out = Vec::new();
    for content in [parsed.module_content(), parsed.instance_content()]
        .into_iter()
        .flatten()
    {
        out.push(&source[content.start as usize..content.end as usize]);
    }
    out
}

/// Scan ONE script's top-level declarations for a `$` / `$$`-prefixed binding NAME (a
/// declaration-position binding official's `validate_identifier_name` binder rejects —
/// `dollar_prefix_invalid`). Driven from the OXC AST of the reparsed script. Returns an
/// [`OfficialRejection`], or `None`.
///
/// A SAME-lexical-scope duplicate declaration (`let a; let a`) is NOT detected here — it is a
/// PARSE-phase error Acorn (and the OXC body-probe) rejects, owned by the body-probe
/// `js_parse_error` slot (a clean body never reaches the analyze phase). So this scan is the
/// `$`-prefix binder check only.
fn scan_script_declaration_rules(script_source: &str) -> Option<OfficialRejection> {
    let alloc = Allocator::default();
    let program = reparse_module(&alloc, script_source)?;

    // The top-level declarator names (every `let`/`const`/`var` declarator pattern
    // name), in source order.
    let mut decl_names: Vec<String> = Vec::new();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            collect_pattern_names(&d.id, &mut decl_names);
        }
    }

    // A `$` / `$$`-prefixed binding name in ANY top-level declarator pattern position.
    // Official `validate_identifier_name` errors at the binder for a `$`-prefixed binding at
    // the top level (`dollar_prefix_invalid`). (A clean body that reaches here parses fine,
    // so a same-scope redeclaration would already have failed the body-probe.)
    if decl_names.iter().any(|n| n.starts_with('$')) {
        return Some(OfficialRejection::of(
            CoreOfficialValidationRule::DollarPrefixInvalid,
        ));
    }

    None
}

/// Scan for a GLOBAL `$foo` / `$$foo` reference in any script or template expression
/// position. Driven from the OXC AST of each expression source, scope-awarely. Returns an
/// [`OfficialRejection`] carrying the EXACT site-specific code.
fn scan_global_dollar_references(
    source: &str,
    parsed: &ParsedSvelte,
    declared: &FxHashSet<String>,
) -> Option<OfficialRejection> {
    // The script bodies + every template interpolation / directive / attribute
    // expression source.
    let mut sources: Vec<String> = script_sources(source, parsed)
        .into_iter()
        .map(str::to_string)
        .collect();
    collect_template_expression_sources(source, &parsed.template, &mut sources);

    for src in &sources {
        if let Some(rejection) = scan_dollar_refs_in_expression(src, declared) {
            return Some(rejection);
        }
    }
    None
}

/// Scan ONE expression / statement source for a global `$`-prefixed identifier
/// reference, scope-awarely (a local binding of the same name is not a global ref).
fn scan_dollar_refs_in_expression(
    src: &str,
    declared: &FxHashSet<String>,
) -> Option<OfficialRejection> {
    let alloc = Allocator::default();
    // Parse as a statement source; a bare expression source is wrapped so it parses.
    let program = reparse_module(&alloc, src).or_else(|| {
        let wrapped = format!("({src});");
        reparse_module(&alloc, &wrapped)
    })?;
    let mut scan = DollarRefScan {
        declared,
        scopes: ShadowStack::default(),
        found: None,
    };
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    scan.found
}

/// The scope-aware scan state for a global `$`-prefixed reference.
struct DollarRefScan<'a> {
    declared: &'a FxHashSet<String>,
    scopes: ShadowStack,
    found: Option<OfficialRejection>,
}

impl DollarRefScan<'_> {
    /// The EXACT official code a `$`-prefixed reference `name` violates, or `None` when it
    /// is not a global-reference violation.
    ///
    /// Mirrors the official `analyze` store-subscription guard
    /// (`phases/2-analyze/index.js`): a `$`-prefixed reference is checked UNLESS it is a
    /// recognised RUNE (`is_rune(name)`). The reserved magic objects carry their EXACT
    /// runes-mode codes (`$$props` → `legacy_props_invalid`, `$$restProps` →
    /// `legacy_rest_props_invalid`); a general `$$foo` (double-dollar) or an undeclared
    /// lowercase-initial `$foo` store subscription is `global_reference_invalid`.
    fn global_violation_code(&self, name: &str) -> Option<&'static str> {
        if !name.starts_with('$') || name == "$" {
            return None;
        }
        // A recognised Svelte RUNE root reference (`$state` / `$derived` / `$props` /
        // `$effect` / `$bindable` / `$inspect` / `$host`, plus their `.raw` / `.by` /
        // `.pre` / … member keypaths reached through the root identifier) is NOT a
        // global store reference — official excludes it via `is_rune(name)`. A
        // shadowed rune name (a local of the same name) is also not a global ref.
        if super::rune_scan::RUNE_ROOT_NAMES.contains(&name) {
            return None;
        }
        if self.scopes.is_shadowed(name) || self.declared.contains(name) {
            // A locally-declared `$`-prefixed binding is invalid too, but that is the
            // `DollarPrefixInvalid` class owned by the declaration scan; here we only
            // flag an UNDECLARED global reference.
            return None;
        }
        // `$$slots` is ACCEPTED by official (it is a valid auto-injected magic object);
        // Verter refuses it only as an unsupported FEATURE (the deferrable 5w
        // magic-identifier path), NEVER an official reject. So it is not a violation
        // here — fall through to the magic-identifier refusal downstream.
        if name == "$$slots" {
            return None;
        }
        // `$$props` / `$$restProps` are an OFFICIAL REJECT in runes mode, each with its OWN
        // exact code.
        if name == "$$props" {
            return Some("legacy_props_invalid");
        }
        if name == "$$restProps" {
            return Some("legacy_rest_props_invalid");
        }
        // A general `$$foo` (double-dollar) is `global_reference_invalid`.
        if name.as_bytes().get(1) == Some(&b'$') {
            return Some("global_reference_invalid");
        }
        // A `$foo` (single-dollar) is a violation only when `foo` is undeclared AND
        // lowercase-initial (the official non-existent-store-subscription rule).
        let store = &name[1..];
        if self.declared.contains(store) || self.scopes.is_shadowed(store) {
            return None;
        }
        if store.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
            Some("global_reference_invalid")
        } else {
            None
        }
    }
}

impl<'a> oxc_ast_visit::Visit<'a> for DollarRefScan<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = FxHashSet::default();
        super::expr::collect_direct_decls(&it.body, &mut frame);
        super::expr::collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        oxc_ast_visit::walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.scopes.push(super::expr::function_scope_names(it));
        oxc_ast_visit::walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.scopes.push(super::expr::arrow_scope_names(it));
        oxc_ast_visit::walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scopes.push(super::expr::block_scope_names(it));
        oxc_ast_visit::walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        if self.found.is_none() {
            if let Some(code) = self.global_violation_code(it.name.as_str()) {
                self.found = Some(OfficialRejection::with_code(
                    CoreOfficialValidationRule::GlobalReferenceInvalid,
                    code,
                ));
            }
        }
        oxc_ast_visit::walk::walk_identifier_reference(self, it);
    }
}

/// The set of declared TOP-LEVEL local names across the instance + module scripts (an
/// accepted `bind:this` target / `$foo`-store referent must be one of these). Driven
/// from the OXC AST of each script's top-level declarators.
fn declared_top_level_locals(source: &str, parsed: &ParsedSvelte) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    for script_src in script_sources(source, parsed) {
        let alloc = Allocator::default();
        let Some(program) = reparse_module(&alloc, script_src) else {
            continue;
        };
        for stmt in &program.body {
            if let Statement::VariableDeclaration(decl) = stmt {
                for d in &decl.declarations {
                    let mut decl_names = Vec::new();
                    collect_pattern_names(&d.id, &mut decl_names);
                    for n in decl_names {
                        names.insert(n);
                    }
                }
            }
        }
    }
    names
}

/// The text value an attribute / directive value span carries (a quoted body, an
/// expression inner, or a mixed value), or `None` for a valueless directive.
fn directive_value_text(source: &str, value: &Option<SvelteAttributeValue>) -> Option<String> {
    let span = match value.as_ref()? {
        SvelteAttributeValue::Text(span)
        | SvelteAttributeValue::Expression(span)
        | SvelteAttributeValue::Mixed(span) => span,
    };
    Some(source[span.start as usize..span.end as usize].to_string())
}

/// Collect every template EXPRESSION source (interpolations, directive expressions,
/// attribute expression values, spreads) under `nodes` into `out`. Used by the
/// global-`$`-ref scan to cover template / bind / event positions.
fn collect_template_expression_sources(source: &str, nodes: &[SvelteNode], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            SvelteNode::Interpolation(span) => {
                out.push(source[span.start as usize..span.end as usize].to_string());
            }
            SvelteNode::Element(el) => {
                for attr in &el.attributes {
                    push_attribute_expression_sources(source, attr, out);
                }
                collect_template_expression_sources(source, &el.children, out);
            }
            SvelteNode::Block(block) => {
                collect_template_expression_sources(source, &block.children, out);
                for clause in &block.clauses {
                    collect_template_expression_sources(source, &clause.children, out);
                }
            }
            _ => {}
        }
    }
}

/// Push the expression sources an attribute carries (a directive's bound/handler
/// expression, an expression-valued plain attribute, a spread) into `out`.
fn push_attribute_expression_sources(source: &str, attr: &SvelteAttribute, out: &mut Vec<String>) {
    match &attr.kind {
        SvelteAttributeKind::Directive(dir) => {
            if let Some(text) = directive_value_text(source, &dir.value) {
                out.push(text);
            }
        }
        SvelteAttributeKind::Plain { value, .. } => {
            if let Some(SvelteAttributeValue::Expression(span)) = value {
                out.push(source[span.start as usize..span.end as usize].to_string());
            }
        }
        SvelteAttributeKind::Spread(span) => {
            out.push(source[span.start as usize..span.end as usize].to_string());
        }
    }
}

/// Scan template nodes for an invalid HTML placement (the ANALYZE-phase `node_invalid_placement`
/// check), carrying the ANCESTOR element tag stack (root..parent). Returns the FIRST violating
/// rule in document order, or `None`.
///
/// ONE official mechanism on the §1.2 element universe: the REPAIR families (`a`/`button`/`h1..h6`
/// nesting) — a disallowed DESCENDANT is `node_invalid_placement`. (The `<p>` explicit-`</p>`
/// AUTO-CLOSE family is a PARSE defect — minted by the parser at the surviving `</p>` close as an
/// `element_invalid_closing_tag_autoclosed` fact — NOT a placement check; the implicit-autoclose
/// `<p>` is official-ACCEPTED.)
fn scan_html_placement(
    nodes: &[SvelteNode],
    ancestors: &mut Vec<String>,
) -> Option<CoreOfficialValidationRule> {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                let tag = element_html_tag(el);
                if let Some(tag) = &tag {
                    // REPAIR families: this element disallowed inside an ancestor.
                    if repair_placement_violation(tag, ancestors) {
                        return Some(CoreOfficialValidationRule::NodeInvalidPlacement);
                    }
                }
                // Descend with this element pushed as the new innermost ancestor (only
                // a real HTML element contributes to the ancestor chain).
                if let Some(tag) = tag {
                    ancestors.push(tag);
                    let found = scan_html_placement(&el.children, ancestors);
                    ancestors.pop();
                    if found.is_some() {
                        return found;
                    }
                } else if let Some(rule) = scan_html_placement(&el.children, ancestors) {
                    return Some(rule);
                }
            }
            SvelteNode::Block(block) => {
                if let Some(rule) = scan_html_placement(&block.children, ancestors) {
                    return Some(rule);
                }
                for clause in &block.clauses {
                    if let Some(rule) = scan_html_placement(&clause.children, ancestors) {
                        return Some(rule);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// The open-tag-span starts of every `<p>` element in an IMPLICIT-autoclose situation
/// (a DIRECT disallowed block child but NO explicit `</p>` close). Official AUTO-CLOSES
/// such a `<p>` and ACCEPTS it (a warning), so the parser's `Unclosed` violation for it
/// is suppressed (the case is a deferrable unsupported FEATURE, never `element_unclosed`).
fn collect_implicit_autoclose_paragraph_open_spans(nodes: &[SvelteNode]) -> Vec<u32> {
    let mut out = Vec::new();
    collect_implicit_autoclose_paragraphs_into(nodes, &mut out);
    out
}

fn collect_implicit_autoclose_paragraphs_into(nodes: &[SvelteNode], out: &mut Vec<u32>) {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                if element_html_tag(el).as_deref() == Some("p")
                    && el.close_span.is_none()
                    && paragraph_direct_autoclose_child(el).is_some()
                {
                    out.push(el.open_span.start);
                }
                collect_implicit_autoclose_paragraphs_into(&el.children, out);
            }
            SvelteNode::Block(block) => {
                collect_implicit_autoclose_paragraphs_into(&block.children, out);
                for clause in &block.clauses {
                    collect_implicit_autoclose_paragraphs_into(&clause.children, out);
                }
            }
            _ => {}
        }
    }
}

/// The HTML tag name of an element node, or `None` for a non-HTML element (a
/// component, a `<svelte:*>` special element, a custom element) — those do not
/// participate in the HTML auto-repair placement rules ("custom elements can be
/// anything").
fn element_html_tag(el: &SvelteElement) -> Option<String> {
    match el.kind {
        SvelteElementKind::Intrinsic => {
            if el.name.contains('-') {
                None
            } else {
                Some(el.name.to_ascii_lowercase())
            }
        }
        _ => None,
    }
}

/// Whether placing `child` inside the given ancestor chain (innermost last) is an
/// official REPAIR-family placement violation (`node_invalid_placement`). Mirrors
/// `is_tag_valid_with_ancestor` for the §1.2 element universe — the REPAIRED-descendant
/// families (`a`/`button`/`h1..h6`): any ancestor in the chain that disallows `child`
/// as a descendant repairs the HTML. (The `<p>` AUTO-CLOSE family is handled separately
/// in `scan_html_placement`, gated on a surviving explicit `</p>`.)
fn repair_placement_violation(child: &str, ancestors: &[String]) -> bool {
    ancestors
        .iter()
        .rev()
        .any(|ancestor| repair_disallowed_descendant(ancestor, child))
}

/// The official `disallowed_children` REPAIR families restricted to the §1.2 element
/// universe (`a` / `button` / `h1`): each disallows the listed descendants such that
/// the browser repairs the HTML.
fn repair_disallowed_descendant(ancestor: &str, child: &str) -> bool {
    match ancestor {
        "a" => child == "a",
        "button" => child == "button",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            matches!(child, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
        }
        _ => false,
    }
}

/// The disallowed block child that triggers a `<p>` autoclose, if any — the FIRST
/// DIRECT child element of `p` whose lowercased HTML tag is in the official `<p>`
/// autoclose descendant set. Shared with the parse-domain feature gate (the implicit
/// autoclose) so both surfaces read ONE block-child predicate.
pub(super) fn paragraph_direct_autoclose_child(p: &SvelteElement) -> Option<String> {
    p.children.iter().find_map(|child| {
        if let SvelteNode::Element(c) = child {
            let tag = element_html_tag(c)?;
            // The official `<p>` autoclosing-children predicate is the parser-owned shared
            // tag-list (`tokenizer_scan`), so the parser's explicit-`</p>` autoclose mint and
            // this gate-side implicit-autoclose suppression scan read ONE block-child rule.
            crate::svelte::parser::tokenizer_scan::paragraph_autocloses_on_block_child(&tag)
                .then_some(tag)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svelte::parser::parse_svelte;

    /// Run the gate over a component source, returning the violated RULE class (the
    /// exact official code is asserted separately where it matters).
    fn gate(source: &str) -> Option<CoreOfficialValidationRule> {
        let parsed = parse_svelte(source);
        official_reject_gate(source, &parsed).map(|r| r.rule)
    }

    // ── DollarPrefixInvalid (declaration position) ───────────────────────────────

    #[test]
    fn dollar_prefixed_props_destructure_local_is_dollar_prefix_invalid() {
        // `let { a: $foo } = $props()` — the DESTRUCTURE-position `$foo` binding is the
        // official `dollar_prefix_invalid` (a declaration, caught at the binder).
        assert_eq!(
            gate("<script>let { a: $foo } = $props();</script>\n<p>{$foo}</p>\n"),
            Some(CoreOfficialValidationRule::DollarPrefixInvalid)
        );
    }

    #[test]
    fn dollar_prefixed_identifier_declarator_is_dollar_prefix_invalid() {
        // `let $$anchor = 1` — an IDENTIFIER-position `$$`-prefixed binding.
        assert_eq!(
            gate("<script>let c = $state(0); let $$anchor = 1;</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::DollarPrefixInvalid)
        );
    }

    #[test]
    fn plain_named_declarations_are_not_dollar_prefix_invalid() {
        // NEGATIVE: a plain (non-`$`) declaration is never a dollar-prefix violation —
        // the §1.2 fixture's `let name`/`let count` must pass the gate cleanly.
        assert_eq!(
            gate("<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n"),
            None
        );
    }

    // ── ScriptBodyParse (same-scope redeclaration) ───────────────────────────────

    #[test]
    fn duplicate_state_declaration_is_script_body_parse() {
        // `let a = $state(0); let a = $state(1);` — a same-lexical-scope `let` redeclaration
        // Acorn (and the OXC body-probe) rejects in the PARSE phase: `js_parse_error`, owned by
        // the body-parse slot (NOT a later analyze-phase `declaration_duplicate`).
        assert_eq!(
            gate("<script>let a = $state(0); let a = $state(1);</script>\n<button onclick={() => a++}>{a}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptBodyParse)
        );
    }

    #[test]
    fn distinct_names_are_not_a_body_parse_error() {
        // NEGATIVE: distinct declarator names never collide — the body parses cleanly.
        assert_eq!(
            gate("<script>let a = $state(0); let b = $state(1);</script>\n<button onclick={() => a++}>{a}{b}</button>\n"),
            None
        );
    }

    // ── GlobalReferenceInvalid + the rune exclusion ──────────────────────────────

    #[test]
    fn runes_are_not_global_reference_violations() {
        // The CRITICAL negative: `$state` / `$derived` / `$props` / `$effect` etc. are
        // RUNE references, NOT undeclared store subscriptions — the gate must NOT flag
        // them as global-reference violations (the official `is_rune(name)` exclusion).
        // A component that ONLY uses runes passes cleanly.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
    }

    #[test]
    fn undeclared_dollar_foo_reference_is_global_reference_invalid() {
        // `{$foo}` — an undeclared lowercase-initial `$foo` store subscription in runes
        // mode is `global_reference_invalid`.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$foo}{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
    }

    #[test]
    fn double_dollar_reference_is_global_reference_invalid() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$$bar}{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
    }

    #[test]
    fn dollar_slots_reference_is_not_a_global_violation() {
        // NEGATIVE: `$$slots` is ACCEPTED by official (a valid magic object) — the gate
        // must NOT flag it as a global-reference reject (it is a deferrable unsupported
        // FEATURE handled downstream, not an official reject).
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$$slots}{c}</button>\n"),
            None
        );
    }

    #[test]
    fn uppercase_dollar_reference_is_not_a_global_violation() {
        // NEGATIVE: `$Foo` (uppercase-initial store name) is accepted by official (the
        // `/[a-z]/` lowercase-initial rule), so it is not a global-reference violation.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>x{$Foo}{c}</button>\n"),
            None
        );
    }

    #[test]
    fn dollar_props_magic_read_is_global_reference_invalid() {
        // `$$props` in the script — the official `legacy_props_invalid` class (mapped to
        // the GlobalReferenceInvalid rule).
        assert_eq!(
            gate("<script>let c = $state(0); let p = $$props;</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
    }

    #[test]
    fn shadowed_dollar_name_is_not_a_global_violation() {
        // NEGATIVE: a `$`-name bound by a local (an arrow param) is shadowed — not a
        // global reference. (`$foo` declared as a param shadows the global.)
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={($foo) => c++}>{c}</button>\n"),
            None
        );
    }

    // ── bind:this targets ────────────────────────────────────────────────────────

    #[test]
    fn dollar_prefixed_bind_this_target_is_global_reference_invalid() {
        // `bind:this={$foo}` (no declaration) — the `$foo` REFERENCE is the official
        // `global_reference_invalid` class.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<div bind:this={$foo}></div>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid)
        );
    }

    #[test]
    fn undeclared_plain_bind_this_target_is_accepted() {
        // NEGATIVE: `bind:this={missing}` (an undeclared PLAIN identifier) is ACCEPTED by
        // official (the binding is implicitly created) — the gate must NOT reject it.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<div bind:this={missing}></div>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
    }

    // ── HTML placement ───────────────────────────────────────────────────────────

    #[test]
    fn nested_button_is_node_invalid_placement() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button><button>x</button></button>\n"),
            Some(CoreOfficialValidationRule::NodeInvalidPlacement)
        );
    }

    #[test]
    fn nested_anchor_is_node_invalid_placement() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<a href=\"/\"><a href=\"/x\">x</a></a>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::NodeInvalidPlacement)
        );
    }

    #[test]
    fn heading_in_heading_is_node_invalid_placement() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<h1><h1>x</h1></h1>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::NodeInvalidPlacement)
        );
    }

    #[test]
    fn paragraph_with_block_descendant_and_explicit_close_is_element_autoclosed() {
        // `<p><div>…</div></p>` and `<p><p>…</p></p>` — a `<p>` auto-closed by a block
        // child WITH a surviving EXPLICIT `</p>`: official
        // `element_invalid_closing_tag_autoclosed`.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><div>x</div></p>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTagAutoclosed)
        );
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><p>x</p></p>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTagAutoclosed)
        );
    }

    #[test]
    fn paragraph_with_block_descendant_but_no_explicit_close_is_not_a_reject() {
        // FALSE-POSITIVE FIX: `<p><div>x</div>` with NO explicit `</p>` is official-
        // ACCEPTED (the browser auto-closes the `<p>`, a warning). It must NOT be an
        // official reject — neither `element_invalid_closing_tag_autoclosed` NOR
        // `element_unclosed` (the parser sees the `<p>` as unclosed, but official
        // auto-closes it). The gate returns None; the implicit case fails closed as an
        // unsupported FEATURE downstream.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><div>x</div>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<p><h1>x</h1>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
    }

    // ── close-tag well-formedness rules ──────────────────────────────────────────

    #[test]
    fn unclosed_button_is_element_unclosed() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}"),
            Some(CoreOfficialValidationRule::ElementUnclosed)
        );
    }

    #[test]
    fn stray_close_is_element_invalid_closing_tag() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n</div>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTag)
        );
    }

    #[test]
    fn mismatched_close_is_element_invalid_closing_tag() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<button onclick={() => c++}><div>{c}</span></button>\n"),
            Some(CoreOfficialValidationRule::ElementInvalidClosingTag)
        );
    }

    #[test]
    fn void_element_explicit_close_is_void_invalid_content() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<input></input>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::VoidElementInvalidContent)
        );
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<input>x</input>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::VoidElementInvalidContent)
        );
    }

    #[test]
    fn well_formed_section_1_2_records_no_close_tag_reject() {
        // NEGATIVE: the §1.2 headline shape (well-formed, all closed, void `<input>`
        // self-closed) is NOT a close-tag violation.
        assert_eq!(
            gate("<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n"),
            None
        );
    }

    #[test]
    fn button_inside_anchor_is_accepted() {
        // NEGATIVE: `<a><button>` is VALID (official accepts it) — the gate must NOT
        // reject every nested element, only the disallowed-descendant families.
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<a href=\"/\"><button onclick={() => c++}>{c}</button></a>\n"),
            None
        );
    }

    #[test]
    fn sibling_supported_elements_are_accepted() {
        // NEGATIVE: the §1.2-class sibling element layout (`<h1>` + `<input>` +
        // `<button>` at the root) is a valid placement — no violation.
        assert_eq!(
            gate("<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n"),
            None
        );
    }

    // ── script-domain rules ──────────────────────────────────────────────────────

    #[test]
    fn duplicate_instance_script_is_script_duplicate() {
        assert_eq!(
            gate("<script>let c = $state(0);</script>\n<script>let d = $state(0);</script>\n<button onclick={() => c++}>{c}{d}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptDuplicate)
        );
    }

    #[test]
    fn invalid_script_context_is_script_invalid_context() {
        assert_eq!(
            gate("<script context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptInvalidContext)
        );
    }

    #[test]
    fn reserved_script_attribute_is_script_reserved_attribute() {
        // `<script server>` — a RESERVED script attribute: official `script_reserved_attribute`.
        assert_eq!(
            gate("<script server>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::ScriptReservedAttribute)
        );
    }

    #[test]
    fn duplicate_script_attribute_is_attribute_duplicate() {
        // `<script lang="js" lang="js">` — a DUPLICATE script attribute: official
        // `attribute_duplicate` (the element-attribute loop runs for the top-level script).
        assert_eq!(
            gate("<script lang=\"js\" lang=\"js\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            Some(CoreOfficialValidationRule::AttributeDuplicate)
        );
    }

    #[test]
    fn capitalized_context_attribute_name_is_not_a_reject() {
        // FALSE-POSITIVE FIX: `<script Context="bad">` — `Context` (capital C) is an
        // UNKNOWN attribute (official emits a `script_unknown_attribute` WARNING and
        // ACCEPTS), NOT `script_invalid_context`. The attribute NAME match is
        // case-sensitive, so the gate must NOT over-reject it.
        assert_eq!(
            gate("<script Context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
    }

    #[test]
    fn valued_module_attribute_is_script_invalid_context() {
        // A valued `module="x"` is the official `script_invalid_attribute_value` (mapped
        // to the ScriptInvalidContext rule), and it wins over the duplicate-script
        // refusal (official validates per-script attributes first).
        assert_eq!(
            gate("<script module=\"x\">const K = 1;</script>\n<button>x</button>\n"),
            Some(CoreOfficialValidationRule::ScriptInvalidContext)
        );
    }

    #[test]
    fn valid_module_context_is_accepted() {
        // NEGATIVE: a valid `context="module"` / `<script module>` is not a violation.
        assert_eq!(
            gate("<script context=\"module\">const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
        assert_eq!(
            gate("<script module>const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n"),
            None
        );
    }

    // ── from_unsupported_surface mapping ─────────────────────────────────────────

    #[test]
    fn from_unsupported_surface_maps_only_the_official_reject_surfaces() {
        use crate::svelte::runtime::UnsupportedSvelteRuntimeSurface;
        let span = verter_span::Span::new(0, 0);
        // OptionsAxis (a NON-duplicate unsupported options axis) maps; an unsupported FEATURE
        // does not. (A template `attribute_duplicate` and a duplicate `<svelte:options>` are
        // now EXACT-CODE parser facts carried by the official-reject gate, NOT mapped from an
        // unsupported surface, so there is no `DuplicateAttribute` surface to map.)
        assert_eq!(
            CoreOfficialValidationRule::from_unsupported_surface(
                &UnsupportedSvelteRuntimeSurface::OptionsAxis { span }
            ),
            Some(CoreOfficialValidationRule::OptionsInvalid)
        );
        // A pure unsupported FEATURE (a `{#if}` block) is NOT an official reject.
        assert_eq!(
            CoreOfficialValidationRule::from_unsupported_surface(
                &UnsupportedSvelteRuntimeSurface::Block {
                    construct: "if",
                    span,
                }
            ),
            None
        );
        // An AdvancedRune surface is NOT auto-mapped (ambiguous: official-reject arity
        // vs deferrable `$state.raw`).
        assert_eq!(
            CoreOfficialValidationRule::from_unsupported_surface(
                &UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: "$state.raw",
                    span,
                }
            ),
            None
        );
    }

    #[test]
    fn rule_names_round_trip() {
        for &rule in CoreOfficialValidationRule::ALL {
            assert_eq!(
                CoreOfficialValidationRule::from_name(rule.name()),
                Some(rule)
            );
        }
        assert_eq!(CoreOfficialValidationRule::from_name("NotARule"), None);
    }
}
