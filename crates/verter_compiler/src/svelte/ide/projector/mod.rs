//! The in-place Svelte IDE TSX projector.
//!
//! Drives ONE [`CodeTransform`] over the original `.svelte` source. Structural
//! connective tissue (block keywords, tag syntax, directive prefixes) is
//! overwritten in place; expression interiors (`{expr}`, block conditions,
//! script bodies) stay as Original chunks so they keep their source spans and
//! map back token-precisely. The ambient prelude is the module INTRO (always
//! the leading bytes — the `@jsxImportSource` pragma must lead; unmapped). The
//! whole template is wrapped in a `__verter_render` scope function, with
//! snippet declarators and declaration-tag bindings hoisted to the top of that
//! scope in source order via CodeTransform MOVE operations.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_span::Span;

use crate::code_transform::{CodeTransform, SourceMapOptions};

use super::await_scan::{
    pattern_default_await_keyword_offsets, rewrite_await_exprs_on, scan_await_positions,
};
use super::emit::{is_css_custom_property, DiagnosticSeverity, UnsupportedKind};
use super::prelude::{render_component_prelude, SvelteJsxNamespace};
use super::store_scan::{
    collect_declared_dollar_names, collect_pattern_dollar_names, scan_store_subscriptions,
};
use super::SvelteIdeDialect;
use crate::svelte::bind_contract::{lookup_bind_contract, BindContract, BindDirection};
use crate::svelte::parser::{
    ParsedSvelte, ScriptBodyGrammar, SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue,
    SvelteBlock, SvelteBlockKind, SvelteClauseKind, SvelteDirectiveKind, SvelteElement,
    SvelteElementKind, SvelteNode, SvelteScript, SvelteSpecialKind, SvelteTag, SvelteTagKind,
};
use store::rewrite_store_sub;

/// The `bind:` directive projection (F4/F5) — a continuation of the
/// `TemplateProjector` impl, extracted for file size.
mod bind;
mod directive;

/// The block-construct projection (`{#if}`/`{#each}`/`{#await}`/`{#key}`/
/// `{#snippet}`) — a continuation of the `TemplateProjector` impl, extracted for
/// file size.
mod block;

/// The `<svelte:*>` special-element + namespace projection (F8/F9/F10) — a
/// continuation of the `TemplateProjector` impl, extracted for file size.
mod special;

/// The F11 store auto-subscription rewrite — a continuation of the projector,
/// extracted for file size.
mod store;

/// Small syntactic identifier / literal-scanning helpers shared across the
/// continuation modules — extracted for file size.
mod ident;

/// The IDE-carrier PUBLIC-FACADE default-export synthesiser — extracted for
/// file size.
mod facade;

/// Framework-owned TypeScript inference for named DOM handlers in a Svelte 5
/// runes instance script.
mod event_inference;
mod template_mode;

use event_inference::apply_event_handler_param_inference;
use facade::svelte_public_facade;
use ident::{
    is_bare_tag_identifier, is_type_query_safe_lvalue, is_valid_binding_identifier,
    is_valid_component_reference, skip_string_literal,
};
use special::{detect_jsx_namespace, extract_props_annotation};
use template_mode::template_uses_host_rune;

/// A typed diagnostic the projector emitted for a flagged matrix construct (the
/// construct was still checked, never dropped). Re-exported with its severity:
/// most codes are `Error` (uncheckable); the experimental await-EXPRESSION (F6)
/// is `Information` (REAL-checked, just experimental).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvelteIdeUnsupportedDiagnostic {
    /// The machine-stable code (e.g. `svelte-await-experimental`).
    pub code: &'static str,
    /// A human-readable message.
    pub message: String,
    /// The reporting severity (`Error` for an unsupported construct,
    /// `Information` for the experimental-but-checked await-EXPRESSION).
    pub severity: DiagnosticSeverity,
    /// The offending span in the ORIGINAL source.
    pub span: Span,
}

/// The rendered Svelte IDE projection.
#[derive(Debug, Clone)]
pub struct SvelteIdeProjection {
    /// The generated TSX or JSX.
    pub code: String,
    /// The JSON source map (empty when source maps are skipped).
    pub source_map: String,
    /// Whether this is the JavaScript+JSX carrier (`.svelte.jsx`) rather than
    /// the TypeScript+JSX carrier (`.svelte.tsx`).
    pub is_jsx: bool,
    /// Typed-unsupported diagnostics for OUT-OF-SCOPE constructs.
    pub diagnostics: Vec<SvelteIdeUnsupportedDiagnostic>,
}

/// Project a parsed Svelte component into the IDE TSX artifact.
///
/// `filename` identifies the source for the map; `skip_source_map` produces an
/// empty `source_map`.
#[must_use]
pub fn project_svelte_ide(
    source: &str,
    parsed: &ParsedSvelte,
    filename: Option<&str>,
    skip_source_map: bool,
) -> SvelteIdeProjection {
    let dialect = SvelteIdeDialect::for_component(parsed);
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new(source, &allocator);
    let mut diagnostics = Vec::new();

    // The first template-markup byte: the render fragment opens here.
    let first_template = parsed
        .template
        .iter()
        .filter_map(node_span)
        .map(|s| s.start)
        .min();

    // F11 store auto-subscription (`$store` / `$store = v`) in the SCRIPT bodies,
    // applied BEFORE `strip_script_tags` (which may MOVE a trailing/interleaved
    // script body via `move_*` — an overwrite on an already-moved chunk would be
    // dropped or stranded). The classified store-subs are rewritten through the
    // `$`-byte / `=`-operator CodeTransform overwrites only (the identifier / RHS
    // bytes are preserved, so hover on the rewritten `$store` lands on the
    // original identifier). The script's lexically-declared `$`-names are also
    // collected here so the markup scans (a separate parse fragment) respect a
    // `let $x` declared in the script.
    let mut script_declared: Vec<String> = Vec::new();
    for content in [parsed.module_content(), parsed.instance_content()]
        .into_iter()
        .flatten()
    {
        let text = &source[content.start as usize..content.end as usize];
        script_declared.extend(collect_declared_dollar_names(text));
    }
    for content in [parsed.module_content(), parsed.instance_content()]
        .into_iter()
        .flatten()
    {
        let text = &source[content.start as usize..content.end as usize];
        for sub in scan_store_subscriptions(text) {
            rewrite_store_sub(&mut ct, content.start, &sub);
        }
    }

    // A Svelte `lang="ts"` body is authored as TypeScript, while the combined
    // IDE carrier is necessarily TSX. Rewrite TypeScript's angle-bracket
    // assertions before moving either script body so valid `<T>value` syntax
    // remains valid and mapped in the carrier.
    for script in [
        parsed.module_script.as_ref(),
        parsed.instance_script.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if script.lang.as_deref() == Some("ts") {
            if let Some(content) = script.content {
                crate::ide::script::rewrite_ts_type_assertions(
                    &source[content.start as usize..content.end as usize],
                    content.start,
                    &mut ct,
                );
            }
        }
    }

    // Classify template-only `$host` through the single scope-aware template
    // authority. The no-`$host` path performs no template traversal. A free
    // template `$host` can only promote an otherwise-legacy script candidate;
    // an explicit `runes={false}` remains authoritative.
    let script_classifies_legacy = component_is_legacy(source, parsed, false);
    let template_host_rune = script_classifies_legacy
        && parsed.forced_runes != Some(false)
        && template_uses_host_rune(&parsed.template, source, &script_declared);
    let legacy_mode = script_classifies_legacy && !template_host_rune;

    // A named DOM handler declared in a TypeScript RUNES instance script is in
    // framework-managed component scope. Apply its official `svelte/elements`
    // parameter tuple before any script body can be moved; CodeTransform moves
    // carry prior edits, while edits made after a move cannot target the moved
    // chunk by its former source position. JavaScript remains authored-JSDoc
    // only, and legacy/module scripts are deliberately outside this inference.
    if matches!(dialect, SvelteIdeDialect::TypeScript) && !legacy_mode {
        apply_event_handler_param_inference(source, parsed, &mut ct);
    }

    // 1) Strip the `<script>` tags. A script BEFORE the first markup byte keeps
    //    its body in place (top-level, mapped). A script that falls AT/AFTER the
    //    first markup byte (interleaved or trailing) would land INSIDE the render
    //    fragment, so its body is MOVED above the render fn (still mapped). All
    //    `<style>` blocks are opaque and removed wholesale.
    for script in [
        parsed.module_script.as_ref(),
        parsed.instance_script.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        strip_script_tags(&mut ct, Some(script), first_template);
    }
    for style in &parsed.styles {
        remove_span(&mut ct, style_full_span(style));
    }

    // 2) Capture the namespace axis now. Mode was finalized above so every
    // consumer (event inference and the prelude) observes the same fact.
    let namespace = detect_jsx_namespace(source, &parsed.template);

    // 3) The render scope function wrapping the template. With trailing/
    //    interleaved scripts MOVED above the render fn, the markup is contiguous
    //    from the first template byte to the source end, so the render fragment
    //    wraps the WHOLE markup (including element close tags the AST does not
    //    span individually).
    let region = first_template.map(|first| (first, source.len() as u32));
    // The LOCAL self-props contract (F8 `<svelte:self>`) — derived SYNTACTICALLY
    // from the instance script's `$props()` annotation (no resolver).
    let self_props_type = parsed
        .instance_content()
        .map(|c| &source[c.start as usize..c.end as usize])
        .and_then(extract_props_annotation);
    // Keep a copy for the PUBLIC-FACADE default export (the projector consumes
    // `self_props_type` for the `<svelte:self>` local contract).
    let facade_props_type = self_props_type.clone();
    let mut projector = TemplateProjector {
        ct: &mut ct,
        source,
        diagnostics: &mut diagnostics,
        snippet_moves: Vec::new(),
        decl_moves: Vec::new(),
        needs_self_contract: false,
        self_props_type,
        dialect,
        script_declared,
        block_declared: Vec::new(),
    };
    projector.project_template(&parsed.template, region);
    drop(projector);

    let prelude = render_component_prelude(namespace, legacy_mode, dialect);
    // Register the prelude as the unmapped intro after mode finalization.
    // CodeTransform still emits it before every authored/moved chunk and
    // publishes the `x_verter_helper_preamble_end` source-map boundary.
    ct.prepend_helper_preamble_content(&prelude);

    // Emit the component's PUBLIC-FACADE default export onto the IDE carrier
    // (the self-diagnostics surface; the bare-import target is the declaration
    // carrier, §2.2/§2.9). See [`svelte_public_facade`]
    // for the shape; it REPLACES the bare `export {};` marker. Appended through
    // `CodeTransform::append` (output-only, unmapped) so it perturbs no mapped
    // span — keeping CodeTransform the single source of truth for carrier text.
    ct.append(&svelte_public_facade(facade_props_type.as_deref(), dialect));

    // Experimental await-EXPRESSIONS (F6) — instance/module SCRIPT positions.
    // An `await` at instance-script top level OR inside a `$derived(...)` /
    // `$derived.by(...)` arg in the script is VALID top-level await under the
    // gate's `module/target: esnext`, so it is kept VERBATIM (its inner type
    // errors + hover survive). The markup positions are rewritten in the
    // projector walk. Each await-bearing position records ONE informational
    // diagnostic (the syntax is experimental, not unsupported — it is checked).
    for content in [parsed.module_content(), parsed.instance_content()]
        .into_iter()
        .flatten()
    {
        let text = &source[content.start as usize..content.end as usize];
        for at in scan_await_positions(text) {
            let span = Span::new(
                content.start + at.keyword_start,
                content.start + at.keyword_start + "await".len() as u32,
            );
            diagnostics.push(SvelteIdeUnsupportedDiagnostic {
                code: UnsupportedKind::AwaitExperimental.code(),
                message: UnsupportedKind::AwaitExperimental.message().to_string(),
                severity: UnsupportedKind::AwaitExperimental.severity(),
                span,
            });
        }
    }

    let code = ct.build_string();
    let source_map = if skip_source_map {
        String::new()
    } else {
        let opts = SourceMapOptions {
            source: filename,
            file: None,
            include_content: true,
        };
        // The preamble-aware variant injects the `x_verter_helper_preamble_end` boundary recorded by
        // `prepend_helper_preamble_content` above; the boundary is the only addition to the map JSON.
        ct.generate_map_json_with_preamble(opts)
    };

    SvelteIdeProjection {
        code,
        source_map,
        is_jsx: dialect.is_javascript(),
        diagnostics,
    }
}

/// Classify the component through the shared scope-aware Svelte mode authority.
/// The synthetic program keeps every script byte at its carrier-absolute offset
/// while blanking markup/style bytes, allowing instance and module top-level
/// scopes to remain distinct without reparsing each slot independently.
fn component_is_legacy(source: &str, parsed: &ParsedSvelte, template_uses_host_rune: bool) -> bool {
    let source_bytes = source.as_bytes();
    let mut eval = source_bytes
        .iter()
        .map(|byte| match byte {
            b'\n' | b'\r' => *byte,
            _ => b' ',
        })
        .collect::<Vec<_>>();
    for region in [parsed.module_content(), parsed.instance_content()]
        .into_iter()
        .flatten()
    {
        let start = region.start as usize;
        let end = region.end as usize;
        if start <= end && end <= source_bytes.len() {
            eval[start..end].copy_from_slice(&source_bytes[start..end]);
        }
    }
    let eval = String::from_utf8(eval)
        .expect("blanking non-script UTF-8 bytes with ASCII spaces preserves UTF-8");
    let allocator = Allocator::default();
    let source_type = match parsed.script_body_probes.first().map(|probe| probe.grammar) {
        Some(ScriptBodyGrammar::Ts) => SourceType::ts(),
        Some(ScriptBodyGrammar::Js) | None => SourceType::mjs(),
    };
    let program = Parser::new(&allocator, &eval, source_type).parse();
    let module_region = parsed
        .module_content()
        .map(|region| (region.start, region.end));
    !verter_parser::svelte_reactivity::infer_combined_program_mode(
        &program.program,
        module_region,
        parsed.forced_runes,
        template_uses_host_rune,
    )
    .is_runes()
}

/// The full span of a `<script>` block (open tag through `</script>`).
fn script_full_span(script: &SvelteScript) -> Span {
    // The parser records the open-tag span and the content span; the close tag
    // follows the content. We reconstruct the full removable range as
    // [tag_open.start, content.end + len("</script>")] when content exists,
    // else just the open tag (self-closed / empty).
    let start = script.tag_open.start;
    let end = match script.content {
        Some(content) => content.end,
        None => script.tag_open.end,
    };
    Span::new(start, end)
}

/// The full span of a `<style>` block — open tag through `</style>` close.
///
/// Component `<style>` blocks are opaque (CSS domain) and stripped wholesale,
/// so the removal MUST cover the trailing `</style>` (the parser's content
/// span excludes it) — otherwise a raw `</style>` leaks into the projected
/// module.
fn style_full_span(style: &crate::svelte::parser::SvelteStyle) -> Span {
    let start = style.tag_open.start;
    let end = match style.content {
        Some(content) => content.end + "</style>".len() as u32,
        None => style.tag_open.end,
    };
    Span::new(start, end)
}

/// Remove a span (overwrite with whitespace-equivalent nothing).
fn remove_span(ct: &mut CodeTransform, span: Span) {
    if span.start < span.end {
        ct.remove(span.start, span.end);
    }
}

/// Strip a script block's open + close tags.
///
/// `<script ...>BODY</script>` → the open tag and the close tag are removed.
/// When the script sits BEFORE the first markup byte (`first_template`), the
/// `BODY` stays in place (top-level, mapped). When it sits AT/AFTER the first
/// markup byte (interleaved or trailing), the `BODY` would land inside the
/// render fragment, so it is MOVED to offset 0 (above the render fn; still
/// mapped via `move_*`) — keeping the projected module a valid top-level
/// script + one render function.
fn strip_script_tags(
    ct: &mut CodeTransform,
    script: Option<&SvelteScript>,
    first_template: Option<u32>,
) {
    let Some(script) = script else { return };
    match script.content {
        Some(content) => {
            // Remove the open tag (`<script ...>`) up to the content start.
            if script.tag_open.start < content.start {
                ct.remove(script.tag_open.start, content.start);
            }
            // Remove the close tag `</script>` after the content.
            let close_end = content.end + "</script>".len() as u32;
            ct.remove(content.end, close_end);

            // If the script body falls AT/AFTER the first markup byte, move it
            // to the render-fragment open point (`first_template`) so it lands
            // ABOVE the render fn header (the same hoist anchor the snippet /
            // declaration moves use) rather than inside the JSX fragment — and
            // crucially AFTER the prelude pragma, never before it.
            if let Some(first) = first_template {
                if script.tag_open.start >= first && content.start < content.end {
                    ct.move_wrapped(content.start, content.end, first, "\n", "\n;");
                }
            }
        }
        None => {
            // Empty/self-closed script — remove the whole open tag.
            remove_span(ct, script_full_span(script));
        }
    }
}

/// The recursive template projector.
struct TemplateProjector<'ct, 'a> {
    ct: &'ct mut CodeTransform<'a>,
    source: &'a str,
    diagnostics: &'ct mut Vec<SvelteIdeUnsupportedDiagnostic>,
    /// Snippet declarator MOVE requests collected during the walk, applied
    /// after the body so they hoist to the TOP of the scope (source order).
    snippet_moves: Vec<SnippetMove>,
    /// Declaration-tag (`{const}`/`{let}`/`{@const}`) MOVE requests, hoisted to
    /// the render scope top so the declared binding is a real statement VISIBLE
    /// to sibling references (sibling-run scope) — an in-place IIFE would
    /// scope the binding locally and a following sibling could not see it.
    decl_moves: Vec<DeclMove>,
    /// Whether a `<svelte:self>` was projected — its LOCAL self-component
    /// contract (`__VerterSelfProps` + `__verter_self`) must be emitted at
    /// module scope (F8).
    needs_self_contract: bool,
    /// The self-props type text derived SYNTACTICALLY from the instance script's
    /// `$props()` annotation (LOCAL — no resolver). `None` ⇒ a permissive
    /// `Record<string, unknown>` contract (untyped `$props()`).
    self_props_type: Option<String>,
    /// Selects TS syntax or JavaScript+JSDoc for every synthetic type-bearing
    /// fragment. Original source bytes never change dialect.
    dialect: SvelteIdeDialect,
    /// The component SCRIPT's lexically-declared `$`-names (F11). A markup
    /// expression parses as a separate fragment, so it must consult this set to
    /// treat a script-declared `let $x` as an ORDINARY local (not a store-sub).
    script_declared: Vec<String>,
    /// The stack of `$`-names introduced by ENCLOSING markup block bindings
    /// (`{#each … as $item}` / `{:then $v}` / `{:catch $e}` / `{#snippet
    /// n($p)}` / `let:$prop`) currently in scope (F11). Each binding-introducer
    /// pushes its `$`-names while projecting its subtree and pops them after —
    /// so a `$`-named block binding is treated as an ORDINARY local (NOT a
    /// store-sub) inside its block, and never leaks to a sibling.
    block_declared: Vec<Vec<String>>,
}

/// A snippet declarator to hoist to the top of its scope function.
struct SnippetMove {
    /// The snippet name.
    name: String,
    /// The original snippet-name span. The name bytes are moved into the
    /// declarator so hover/definition/rename keep their authored identity.
    name_span: Span,
    /// The params span (excludes parens), if any.
    params: Option<Span>,
    /// A rewritten parameter list for store/await defaults. `None` keeps and
    /// moves the original parameter bytes so declaration hover/rename/definition
    /// remain mapped; `Some` is the bounded rewrite path.
    params_rewrite: Option<String>,
    /// The body span (between the snippet head and `{/snippet}`).
    body_span: Span,
}

/// A declaration-tag binding to hoist to the render scope top.
struct DeclMove {
    /// `true` for `let`, `false` for `const`.
    is_let: bool,
    /// The inner `x = e` declaration span (kept mapped when moved).
    inner_span: Span,
    /// When the inner carries an F11 store-sub, the TEXT-rewritten inner (the
    /// store-get/set helpers spliced in). A store-bearing inner cannot use the
    /// mapped `move_wrapped` path: the store rewrite's trailing close-paren falls
    /// at the move's END boundary, which `move_wrapped` classifies as OUTSIDE the
    /// moved range (it strands at the original, now-removed tag position). So a
    /// store-bearing inner is emitted as the rewritten TEXT at the hoist anchor
    /// instead (a bounded mapping degrade for the rare store-subscribed
    /// declaration value — the common non-store `{@const}` keeps full mapping via
    /// `move_wrapped`). `None` ⇒ the mapped move path.
    text_rewrite: Option<String>,
}

impl TemplateProjector<'_, '_> {
    /// Project the whole template into the render scope function.
    ///
    /// `region` is the markup byte range `[start, end)` the render fragment
    /// wraps — every byte outside the script/style blocks.
    fn project_template(&mut self, nodes: &[SvelteNode], region: Option<(u32, u32)>) {
        let Some((first, last)) = region else {
            // No template — emit an empty render function. The file is made a
            // valid module by the PUBLIC-FACADE `export default` appended after
            // the projector runs (no separate `export {};` marker needed).
            self.ct
                .append("\n;function __verter_render() { return (<></>); }\n");
            return;
        };

        // Snippet declarators are hoisted to MODULE scope, ABOVE the render
        // fragment's `return` (Svelte snippets are visible to preceding
        // siblings; an in-place `const` would TDZ-error a preceding `{@render}`
        // under the clean-type-check gate). Module-scope `const`
        // declarations are visible inside the render fn with no TDZ. The
        // declarator MOVEs land before the render-header insertion at `first`
        // (verified by the ordering test).
        for node in nodes {
            self.project_node(node);
        }

        // Hoist snippet declarators to MODULE scope (before the render fn).
        // Module-scope `const`s are visible inside the render fn with no TDZ,
        // and a preceding `{@render}` sibling references a later-declared
        // snippet cleanly. We move each declarator to `first` FIRST,
        // then prepend the render header at `first` with `prepend_left` — which
        // inserts at the chunk boundary BEFORE the already-moved declarator
        // chunks, landing the header below the declarators (module-scope
        // declarators, then `;function __verter_render()`).
        let snippet_moves = std::mem::take(&mut self.snippet_moves);
        for snip in &snippet_moves {
            self.emit_snippet_declarator(first, snip);
        }
        // Hoist declaration-tag bindings to MODULE scope too (before the render
        // fn). They reference script-/module-level symbols, so module scope is
        // valid, and as real statements every sibling reference resolves them
        // (sibling-run scope) — an in-place IIFE would not.
        let decl_moves = std::mem::take(&mut self.decl_moves);
        for decl in &decl_moves {
            let kw = if decl.is_let { "let " } else { "const " };
            match &decl.text_rewrite {
                None => {
                    // The mapped path — the original inner bytes are MOVED to the
                    // anchor (kept mapped via `move_wrapped`).
                    self.ct.move_wrapped(
                        decl.inner_span.start,
                        decl.inner_span.end,
                        first,
                        &format!("\n{kw}"),
                        ";\n",
                    );
                }
                Some(rewritten) => {
                    // The store-bearing text path (F11, P1-2): the original inner
                    // bytes do NOT travel (the store rewrite's trailing close-paren
                    // would strand at the move boundary), so REMOVE them in place
                    // and emit the rewritten declaration as text at the anchor.
                    // `append_left(first, …)` stacks in source order and lands
                    // ABOVE the render header (which is `prepend_left(first, …)` at
                    // the very end — prepends sit before appends at the same index).
                    self.ct.remove(decl.inner_span.start, decl.inner_span.end);
                    self.ct.append_left(first, &format!("\n{kw}{rewritten};\n"));
                }
            }
        }
        // The render fn closes here; the file is made a module by the
        // PUBLIC-FACADE `export default` appended after the projector runs.
        self.ct.append_left(last, "\n</>);\n}\n");
        // F8 `<svelte:self>` LOCAL contract — emitted at MODULE scope ABOVE the
        // render fn, as a PREFIX of the render-header insertion (one chunk, so it
        // reliably lands above `;function __verter_render()` regardless of
        // same-index insertion ordering). The self-props type is derived
        // syntactically from the instance script's `$props()` annotation (LOCAL
        // — no resolver); an untyped `$props()` degrades to a permissive
        // `Record<string, unknown>` contract.
        let self_contract = if self.needs_self_contract {
            let props_ty = self
                .self_props_type
                .clone()
                .unwrap_or_else(|| "Record<string, unknown>".to_string());
            match self.dialect {
                SvelteIdeDialect::TypeScript => format!(
                    "\ntype __VerterSelfProps = {props_ty};\n\
                     declare const __verter_self: abstract new (...args: never[]) => \
                     {{ $props: __VerterSelfProps }};\n"
                ),
                SvelteIdeDialect::JavaScript => format!(
                    "\n/** @typedef {{{props_ty}}} __VerterSelfProps */\n\
                     const __verter_self = /** @type {{{{ new (...args: any[]): {{ $props: __VerterSelfProps }} }}}} */ (/** @type {{any}} */ (class {{}}));\n"
                ),
            }
        } else {
            String::new()
        };
        self.ct.prepend_left(
            first,
            &format!("{self_contract}\n;function __verter_render() {{\nreturn (<>\n"),
        );
    }

    /// Emit one hoisted snippet declarator at the scope top.
    ///
    /// The declarator is moved to BEFORE the `;function __verter_render()`
    /// header we prepended at `scope_anchor` — i.e. it relocates the mapped
    /// snippet body to the scope top while branding it through
    /// `__verter_snippet`.
    fn emit_snippet_declarator(&mut self, scope_anchor: u32, snip: &SnippetMove) {
        self.emit_snippet_binding(scope_anchor, snip, "const ", "");
    }

    /// Emit one snippet binding while moving every unchanged authored chunk
    /// (name, params, body) independently. Only synthetic punctuation/type
    /// context is unmapped.
    fn emit_snippet_binding(
        &mut self,
        scope_anchor: u32,
        snip: &SnippetMove,
        prefix: &str,
        annotation: &str,
    ) {
        let header_tail = match (&snip.params_rewrite, snip.params) {
            (Some(params), _) => format!("{} = __verter_snippet(({}) => (<>\n", annotation, params),
            (None, Some(params)) if params.start < params.end => {
                format!("{} = __verter_snippet((", annotation)
            }
            _ => format!("{} = __verter_snippet(() => (<>\n", annotation),
        };
        self.ct.move_wrapped(
            snip.name_span.start,
            snip.name_span.end,
            scope_anchor,
            prefix,
            &header_tail,
        );
        if snip.params_rewrite.is_none() {
            if let Some(params) = snip.params.filter(|params| params.start < params.end) {
                self.ct
                    .move_wrapped(params.start, params.end, scope_anchor, "", ") => (<>\n");
            }
        }
        // Move the snippet body separately so both the public binding name and
        // parameter declarations/body retain their source-map identity.
        if snip.body_span.start < snip.body_span.end {
            self.ct.move_wrapped(
                snip.body_span.start,
                snip.body_span.end,
                scope_anchor,
                "",
                "\n</>));\n",
            );
        } else {
            self.ct.append_left(scope_anchor, "\n</>));\n");
        }
    }

    fn slice(&self, span: Span) -> &str {
        &self.source[span.start as usize..span.end as usize]
    }

    /// Push a markup-block binding scope: the `$`-names introduced by the block's
    /// binding fragment(s) (each item/index pattern, await then/catch binding,
    /// snippet params, `let:` slot props) are treated as ORDINARY locals while
    /// projecting the block's subtree (F11). The names are collected
    /// STRUCTURALLY from the binding pattern via `collect_pattern_dollar_names`.
    /// `pattern_spans` are the source spans of the binding fragments — each is
    /// sliced and parsed; an empty resulting set still pushes a (cheap) frame so
    /// the matching `pop_block_bindings` stays balanced.
    fn push_block_bindings(&mut self, pattern_spans: &[Option<Span>]) {
        let mut names = Vec::new();
        for span in pattern_spans.iter().flatten() {
            names.extend(collect_pattern_dollar_names(self.slice(*span)));
        }
        self.block_declared.push(names);
    }

    /// Rewrite every markup-expression construct inside a block-binding PATTERN's
    /// DEFAULT-VALUE expressions and return the rewritten text: the F11 store-READ
    /// subscriptions (`{ x = $store }` → `__verter_store_get(store)`) AND the F6
    /// experimental await-EXPRESSIONS (`{ x = await load() }` → `__verter_await_expr(
    /// load())`). A block-binding pattern (an `{#each … as PATTERN}` item, a snippet
    /// PARAM list, a `{:then}`/`{:catch}` binding) is SLICED into a synthesised SYNC
    /// projection string (a `.map((PARAMS) => …)` arrow head, a
    /// `__verter_snippet((PARAMS) => …)` head, a `const BINDING: T = …` declarator)
    /// — so a raw `await` in a default would be INVALID TSX, and both rewrites must
    /// run on the TEXT before it is emitted, NOT via CodeTransform ops on the
    /// original (relocated / overwritten) span. The bound NAMES stay locals; only
    /// the default initializers (ordinary read contexts) are rewritten. Store
    /// defaults are scanned against the currently-in-scope declared names UNIONED
    /// with the names this pattern introduces (so a default referencing a sibling
    /// binding stays a local). The INFORMATIONAL await diagnostic is recorded
    /// against the ORIGINAL absolute pattern span (the sliced text carries no
    /// source span).
    fn rewrite_pattern_text_defaults(&mut self, pattern_span: Span) -> String {
        let pattern_text = &self.source[pattern_span.start as usize..pattern_span.end as usize];
        let has_dollar = pattern_text.contains('$');
        let has_await = pattern_text.contains("await");
        if !has_dollar && !has_await {
            return pattern_text.to_string();
        }
        if has_await {
            self.record_pattern_default_await_diagnostics_in(pattern_span);
        }
        let mut declared = self.declared_dollar_names();
        declared.extend(collect_pattern_dollar_names(pattern_text));
        // Re-slice (the `declared`-building borrow above ended) so the call below
        // takes a fresh `&'a str` not tied to `&self`.
        let pattern_text = &self.source[pattern_span.start as usize..pattern_span.end as usize];
        self.rewrite_pattern_default_store_subs_text(pattern_text, &declared)
    }

    /// Record one INFORMATIONAL `svelte-await-experimental` diagnostic per
    /// experimental await-expression inside a block-binding PATTERN's
    /// DEFAULT-VALUE expressions, WITHOUT applying the byte transform. The pattern
    /// text alone is not a parseable module, so it is scanned inside the SAME
    /// `const [{pattern}] = null as any;` wrapper the rewrite uses; each found
    /// keyword position is translated past the wrapper prefix back to the ABSOLUTE
    /// source offset so the hint lands on the real `await` token.
    fn record_pattern_default_await_diagnostics_in(&mut self, pattern_span: Span) {
        let pattern_text = &self.source[pattern_span.start as usize..pattern_span.end as usize];
        for kw in pattern_default_await_keyword_offsets(pattern_text) {
            let kw_start = pattern_span.start + kw;
            let span = Span::new(kw_start, kw_start + "await".len() as u32);
            self.push_diag(span, UnsupportedKind::AwaitExperimental);
        }
    }

    /// Pop the most recent markup-block binding scope pushed by
    /// `push_block_bindings`.
    fn pop_block_bindings(&mut self) {
        self.block_declared.pop();
    }

    /// Project one template node.
    fn project_node(&mut self, node: &SvelteNode) {
        match node {
            SvelteNode::Text(_) => { /* literal text — valid JSX text, kept */ }
            SvelteNode::Comment(span) => {
                // `<!-- ... -->` is valid JSX only inside `{/* */}`. Remove it
                // to keep the projection clean (comments carry no types).
                remove_span(self.ct, *span);
            }
            SvelteNode::Interpolation(span) => {
                // `{expr}` is valid JSX child syntax. We re-emit the enclosing
                // braces as overwrites so the EXPRESSION INTERIOR begins its
                // own Original chunk — the source map then emits a token at the
                // expression's start, giving per-expression (hover-precise)
                // mapping rather than line-granular. The `{` is at span.start-1
                // and the `}` at span.end.
                if span.start > 0 {
                    self.ct.overwrite(span.start - 1, span.start, "{");
                }
                self.ct.overwrite(span.end, span.end + 1, "}");
                // An experimental await-EXPRESSION inside the interpolation (F6)
                // is REWRITTEN to a real checkable form: `await ARG` →
                // F6 await-EXPRESSIONS + F11 store auto-subscriptions in a markup
                // interpolation are rewritten through the shared markup-expression
                // entry (`$store` / `await e` interior CodeTransform overwrites,
                // identifier/RHS/ARG bytes preserved).
                self.rewrite_store_subs_in(*span);
            }
            SvelteNode::Element(el) => self.project_element(el),
            SvelteNode::Block(block) => self.project_block(block),
            SvelteNode::Tag(tag) => self.project_tag(tag),
        }
    }

    /// Project an element / component / special element.
    fn project_element(&mut self, el: &SvelteElement) {
        match &el.kind {
            SvelteElementKind::NestedStyle => {
                // Nested `<style>` — opaque, stripped from projection.
                remove_span(self.ct, el.open_span);
                for child in &el.children {
                    if let SvelteNode::Text(span) = child {
                        remove_span(self.ct, *span);
                    }
                }
                return;
            }
            SvelteElementKind::Special(kind) => {
                self.project_special_element(el, *kind);
                return;
            }
            _ => {}
        }
        // Intrinsic or component element: project attributes, then children.
        for attr in &el.attributes {
            self.project_attribute(el, attr);
        }
        if matches!(el.kind, SvelteElementKind::Component) {
            self.inject_native_component_call_check(el);
        }
        // `let:` slot-prop directives introduce a binding scoped to this element's
        // CHILDREN (`<C let:item={$row}>{$row}</C>`). Collect each `let:` binding's
        // `$`-names and push them while projecting the children so a `$`-named
        // slot-prop binding is not mis-rewritten as a store-sub. The
        // binding is the value alias when present (`let:item={alias}`), else the
        // bare local (shorthand `let:item`).
        let let_binding_names = self.collect_let_directive_dollar_names(el);
        let pushed = !let_binding_names.is_empty();
        if pushed {
            self.block_declared.push(let_binding_names);
        }
        // Immediate `{#snippet}` children are lexical declarations owned by
        // this element. Svelte component children also become named snippet
        // props. Keep them in a declaration-before-return IIFE around the
        // element: forward/mutual/recursive references work, sibling elements
        // can reuse the same names, and no generated identifier leaks through
        // hover/definition/rename.
        let mut scoped_snippets = Vec::new();
        for child in &el.children {
            match child {
                SvelteNode::Block(block) => match &block.kind {
                    SvelteBlockKind::Snippet {
                        name,
                        name_text,
                        params,
                    } => scoped_snippets
                        .push(self.prepare_snippet_move(block, *name, name_text, *params)),
                    _ => self.project_node(child),
                },
                _ => self.project_node(child),
            }
        }
        if !scoped_snippets.is_empty() {
            self.emit_element_snippet_scope(el, &scoped_snippets);
        }
        if pushed {
            self.pop_block_bindings();
        }
    }

    /// Emit immediate child snippets in an IIFE whose lexical scope owns the
    /// element. Component-owned snippets are also wired as named props and
    /// contextually typed from the component's public Svelte prop contract.
    fn emit_element_snippet_scope(&mut self, el: &SvelteElement, snippets: &[SnippetMove]) {
        let anchor = el.open_span.start;
        let is_component = matches!(el.kind, SvelteElementKind::Component);

        for (index, snip) in snippets.iter().enumerate() {
            let prefix = match (index == 0, is_component, self.dialect) {
                (true, true, SvelteIdeDialect::JavaScript) => format!(
                    "{{(() => {{\n/** @type {{NonNullable<__VerterComponentProps<typeof {}>[{:?}]>}} */\nconst ",
                    el.name, snip.name
                ),
                (false, true, SvelteIdeDialect::JavaScript) => format!(
                    "/** @type {{NonNullable<__VerterComponentProps<typeof {}>[{:?}]>}} */\nconst ",
                    el.name, snip.name
                ),
                (true, _, _) => "{(() => {\nconst ".to_string(),
                _ => "const ".to_string(),
            };
            let annotation = if is_component && self.dialect == SvelteIdeDialect::TypeScript {
                format!(
                    ": NonNullable<__VerterComponentProps<typeof {}>[{:?}]>",
                    el.name, snip.name
                )
            } else {
                String::new()
            };
            self.emit_snippet_binding(anchor, snip, &prefix, &annotation);
        }

        self.ct.append_left(anchor, "return (\n");
        if is_component {
            let insertion = snippets.iter().fold(String::new(), |mut out, snip| {
                use std::fmt::Write;
                let _ = write!(out, " {}={{{}}}", snip.name, snip.name);
                out
            });
            self.ct
                .append_left(el.open_span.end.saturating_sub(1), &insertion);
        }
        let close = el.close_span.unwrap_or(el.open_span).end;
        self.ct.append_left(close, "\n); })()}");
    }

    /// Add a private direct-call check for a component's prop bag.
    ///
    /// Svelte 5's native component is callable as `(internals, props)`. The JSX
    /// namespace adapter handles ordinary concrete components, while this
    /// direct call preserves a generic component's own higher-rank inference:
    /// `__verter_component(C)(internals, { items, render })` returns `C`
    /// unchanged for native components, so TypeScript infers the component's
    /// generic parameters from the authored prop object instead of first
    /// collapsing them through a conditional `ComponentProps<C>` projection.
    /// The check is a JSX spread and contributes no runtime props. Its synthetic
    /// scaffolding stays unmapped, while each copied object-literal prop key maps
    /// to the authored attribute name. TypeScript can choose this private check
    /// as the sole reporting site for an assignability error, so leaving the key
    /// unmapped would silently drop a real user diagnostic at the LSP boundary.
    fn inject_native_component_call_check(&mut self, el: &SvelteElement) {
        if !is_valid_component_reference(&el.name) {
            return;
        }
        // A childful Svelte component passes a generated `children` Snippet,
        // not the raw JSX child value. Fabricating that callable here would
        // erase snippet parameters. Keep the JSX adapter authoritative for
        // this shape; the typed gate covers both a valid generic use and a
        // cross-prop mismatch while children are present.
        if el.children.iter().any(|child| match child {
            SvelteNode::Text(span) => !self.slice(*span).trim().is_empty(),
            _ => true,
        }) {
            return;
        }

        let internals = match self.dialect {
            SvelteIdeDialect::TypeScript => {
                "null! as import(\"svelte\").ComponentInternals".to_string()
            }
            SvelteIdeDialect::JavaScript => {
                "/** @type {import(\"svelte\").ComponentInternals} */ (null)".to_string()
            }
        };
        let mut segments = vec![(
            format!(" {{...(__verter_component({})({}, {{ ", el.name, internals),
            None,
        )];
        let mut has_props = false;
        for attr in &el.attributes {
            match &attr.kind {
                SvelteAttributeKind::Plain {
                    name,
                    name_span,
                    value,
                } if !name.is_empty() => {
                    if is_css_custom_property(name) {
                        continue;
                    }
                    // Map only identifier-shaped keys whose generated bytes are
                    // identical to the authored attribute. Quoting or escaping a
                    // non-identifier changes offsets, so that synthetic spelling
                    // must stay unmapped rather than producing a misleading range.
                    let (key, key_source) = if is_valid_binding_identifier(name) {
                        (name.clone(), Some(name_span.start))
                    } else {
                        (format!("{name:?}"), None)
                    };
                    let value = match value {
                        Some(SvelteAttributeValue::Expression(span)) => {
                            let raw = self.slice(*span).to_string();
                            format!("({})", self.rewrite_store_subs_in_text(&raw))
                        }
                        Some(SvelteAttributeValue::Text(span)) => {
                            format!("{:?}", self.slice(*span))
                        }
                        Some(SvelteAttributeValue::Mixed(_)) => return,
                        None => "true".to_string(),
                    };
                    if has_props {
                        segments.push((", ".to_string(), None));
                    }
                    segments.push((key, key_source));
                    segments.push((format!(": {value}"), None));
                    has_props = true;
                }
                SvelteAttributeKind::Spread(span) => {
                    let raw = self.slice(*span).trim().to_string();
                    let expr = raw.strip_prefix("...").unwrap_or(&raw).trim().to_string();
                    if has_props {
                        segments.push((", ".to_string(), None));
                    }
                    // Parentheses and store rewrites make this spelling differ
                    // from the authored spread. Keep it synthetic; the ordinary
                    // JSX spread remains the source-mapped reporting site.
                    segments.push((
                        format!("...({})", self.rewrite_store_subs_in_text(&expr)),
                        None,
                    ));
                    has_props = true;
                }
                SvelteAttributeKind::Directive(dir)
                    if matches!(
                        dir.kind,
                        SvelteDirectiveKind::Bind | SvelteDirectiveKind::On
                    ) && dir.local != "this" =>
                {
                    let Some(SvelteAttributeValue::Expression(span)) = &dir.value else {
                        continue;
                    };
                    let value =
                        if dir.kind == SvelteDirectiveKind::Bind && self.is_function_binding(dir) {
                            match self.dialect {
                                SvelteIdeDialect::TypeScript => format!(
                                    "null! as __VerterComponentProps<typeof {}>[{:?}]",
                                    el.name, dir.local
                                ),
                                SvelteIdeDialect::JavaScript => format!(
                                "/** @type {{__VerterComponentProps<typeof {}>[{:?}]}} */ (null)",
                                el.name, dir.local
                            ),
                            }
                        } else {
                            let raw = self.slice(*span).to_string();
                            self.rewrite_store_subs_in_text(&raw)
                        };
                    let name = if dir.kind == SvelteDirectiveKind::On {
                        format!("on{}", dir.local)
                    } else {
                        dir.local.clone()
                    };
                    if has_props {
                        segments.push((", ".to_string(), None));
                    }
                    let local_source_start = self
                        .slice(attr.span)
                        .find(&dir.local)
                        .map_or(attr.span.start, |offset| attr.span.start + offset as u32);
                    let (key, key_source) = if is_valid_binding_identifier(&name) {
                        let source = (name == dir.local).then_some(local_source_start);
                        (name, source)
                    } else {
                        // `on:` prefixes and quoted property names are generated
                        // spellings, not byte-identical authored ranges.
                        (format!("{name:?}"), None)
                    };
                    segments.push((key, key_source));
                    segments.push((format!(": ({value})"), None));
                    has_props = true;
                }
                _ => {}
            }
        }
        segments.push((" }), {})}".to_string(), None));
        let close_offset = if el.self_closing { 2 } else { 1 };
        if let Some(at) = el.open_span.end.checked_sub(close_offset) {
            let mapped = segments
                .iter()
                .map(|(content, source_start)| {
                    (
                        at,
                        source_start.map(|source_start| (source_start, 0)),
                        self.ct.alloc_str(content),
                    )
                })
                .collect::<Vec<_>>();
            self.ct.batch_prepend_left_with_source_map(&mapped);
        }
    }

    /// Collect the `$`-prefixed binding names introduced by the `let:` slot-prop
    /// directives on `el`. The binding is the value alias (`let:item={$row}` →
    /// `$row`, possibly a destructuring pattern) when present, else the bare local
    /// (shorthand `let:item` → `item`). The `let:` form is identified
    /// STRUCTURALLY by the parser-classified `SvelteDirectiveKind::Let` (the
    /// parser is the directive-prefix authority), never by string-sniffing the
    /// raw attribute name.
    fn collect_let_directive_dollar_names(&self, el: &SvelteElement) -> Vec<String> {
        let mut names = Vec::new();
        for attr in &el.attributes {
            if let SvelteAttributeKind::Directive(dir) = &attr.kind {
                if dir.kind != SvelteDirectiveKind::Let {
                    continue;
                }
                match &dir.value {
                    Some(SvelteAttributeValue::Expression(span)) => {
                        // `let:item={alias}` — the alias is the binding (an
                        // identifier or a destructuring pattern).
                        names.extend(collect_pattern_dollar_names(self.slice(*span)));
                    }
                    _ => {
                        // Shorthand `let:item` — the local name IS the binding.
                        names.extend(collect_pattern_dollar_names(&dir.local));
                    }
                }
            }
        }
        names
    }

    /// Rewrite the MATCHING `</original-name>` close tag's NAME to `replacement`
    /// (no-op for a self-closing element).
    fn rewrite_close_tag_name(&mut self, el: &SvelteElement, replacement: &str) {
        if let Some((close_start, _)) = self.matching_close_tag_span(el) {
            self.rewrite_close_at(close_start, el.name.len(), replacement);
        }
    }

    /// The MATCHING `</name>` close-tag span of `el`, as `(start, end)` —
    /// `start` at the `<` of `</name`, `end` just past the closing `>`. Reads the
    /// span the PARSER recorded during its string/brace-aware child walk (the
    /// parser is the close-tag authority); a `</name>` appearing inside a
    /// descendant string/template literal is therefore never mistaken for this
    /// element's real close tag. `None` for a self-closing or unterminated
    /// element.
    fn matching_close_tag_span(&self, el: &SvelteElement) -> Option<(u32, u32)> {
        el.close_span.map(|s| (s.start, s.end))
    }

    /// Overwrite the name run of a `</name` close tag starting at `close_start`.
    fn rewrite_close_at(&mut self, close_start: u32, name_len: usize, replacement: &str) {
        let name_start = close_start + 2; // after `</`
        let name_end = name_start + name_len as u32;
        self.ct.overwrite(name_start, name_end, replacement);
    }

    /// Remove a declaration tag (`{const ...}`/`{let ...}`/`{@const ...}`) in
    /// place and queue its inner `x = e` for hoisting to the render scope top.
    fn hoist_declaration_tag(&mut self, tag: &SvelteTag, is_let: bool) {
        // F11 (P1-2): a store-sub in a `{@const x = $store}` / `{@let}` VALUE is
        // rewritten MOVE-SAFELY. The declaration inner is MOVED to the scope top.
        // A mapped `move_wrapped` over the original bytes CANNOT carry the store
        // rewrite's trailing close-paren when the store is the inner's last token
        // (`{@const c = $count}`): an `append_left(inner.end, ")")` falls at the
        // move's END boundary, which `move_wrapped` classifies OUTSIDE the range —
        // stranding the `)` at the original, now-removed tag position. So a
        // store-bearing inner is TEXT-rewritten and emitted as text at the hoist
        // anchor (bounded mapping degrade for the rare store-subscribed value);
        // the common non-store `{@const}` keeps full mapping via `move_wrapped`.
        // F6: a markup await in a `{@const x = await load()}` / `{@let}` VALUE is a
        // markup-expression position — `__verter_render` stays SYNC, so a raw
        // `await` left on the mapped move would be INVALID TSX. The inner is
        // diverted to the await-safe TEXT path (the same path the store-sub case
        // uses) whenever it carries EITHER a store-sub `$` OR an `await`. The
        // INFORMATIONAL await diagnostic anchors on the ORIGINAL absolute keyword
        // position (the text fragment carries no source span).
        // Slice from the `'a`-lifetime source (not `&self`) so the immutable slice
        // borrow does not block the `&mut self` diagnostic record below.
        let inner_text = &self.source[tag.inner.start as usize..tag.inner.end as usize];
        self.record_await_diagnostics_in(tag.inner);
        let text_rewrite = if inner_text.contains('$') || inner_text.contains("await") {
            let rewritten = self.rewrite_store_subs_in_text(inner_text);
            // Only divert to the text path when the rewrite actually changed the
            // inner (a `$`-byte that was NOT a store-sub — a rune / `$$`-magic /
            // local — or an `await` substring that was NOT an experimental await
            // leaves the inner untouched and stays on the mapped move).
            (rewritten != inner_text).then_some(rewritten)
        } else {
            None
        };
        // Remove the whole tag from the JSX position (`{const ` … `}`) — the
        // inner declaration is moved out, so its bytes are relocated.
        self.ct.remove(tag.span.start, tag.inner.start);
        self.ct.remove(tag.inner.end, tag.span.end);
        self.decl_moves.push(DeclMove {
            is_let,
            inner_span: tag.inner,
            text_rewrite,
        });
    }

    /// Project a standalone tag.
    fn project_tag(&mut self, tag: &SvelteTag) {
        // F11: a store-sub in a tag's inner expression (`{@html $x}`,
        // `{@render snip($x)}`, `{@debug $x}`, `{@attach $a}`, `{@const x = $s}`)
        // is rewritten. The value-expression tags rewrite their inner HERE through
        // the `$`-span overwrite (the overwrites below touch only the surrounding
        // brace/prefix bytes, composing with the interior `$`-span rewrite). The
        // declaration tags (`{@const}`/`{@let}`) MOVE their inner to the scope
        // top, so they handle the store rewrite inside `hoist_declaration_tag`
        // (a TEXT rewrite emitted at the hoist anchor — the trailing close-paren
        // cannot travel with the mapped move boundary), NOT through the span
        // overwrite here.
        if !matches!(
            tag.kind,
            SvelteTagKind::Const | SvelteTagKind::LegacyConst | SvelteTagKind::Let
        ) {
            self.rewrite_store_subs_in(tag.inner);
        }
        match tag.kind {
            SvelteTagKind::Render => {
                // `{@render snippet(args)}` → `{snippet(args)}` — checks through
                // Snippet's call signature. Overwrite `{@render ` → `{`.
                self.ct.overwrite(tag.span.start, tag.inner.start, "{");
                // Close `}` stays.
                self.rewrite_tag_close(tag, "}");
            }
            SvelteTagKind::Html => {
                // `{@html e}` → `{__verter_html_check(e)}` is overkill; a string
                // position checks `e` — overwrite `{@html ` → `{(`, close → `)}`.
                match self.dialect {
                    SvelteIdeDialect::TypeScript => {
                        self.ct.overwrite(tag.span.start, tag.inner.start, "{(");
                        self.rewrite_tag_close(tag, ") as unknown as string}");
                    }
                    SvelteIdeDialect::JavaScript => {
                        self.ct.overwrite(
                            tag.span.start,
                            tag.inner.start,
                            "{(/** @type {string} */ (/** @type {unknown} */ (",
                        );
                        self.rewrite_tag_close(tag, ")))}");
                    }
                }
            }
            SvelteTagKind::Const | SvelteTagKind::LegacyConst => {
                // `{const x = e}` / `{@const x = e}` → a `const x = e;` HOISTED
                // to the render scope top (a real statement, visible to sibling
                // references — sibling-run scope). The inner `x = e` is
                // moved (kept mapped); the original tag is removed in place.
                self.hoist_declaration_tag(tag, false);
            }
            SvelteTagKind::Let => {
                self.hoist_declaration_tag(tag, true);
            }
            SvelteTagKind::Debug => {
                // `{@debug a, b}` → `{__verter_void([a, b])}` void reference.
                self.ct
                    .overwrite(tag.span.start, tag.inner.start, "{__verter_void([");
                self.rewrite_tag_close(tag, "])}");
            }
            SvelteTagKind::Attach => {
                // `{@attach e}` → `{__verter_attach(e)}` checker argument.
                self.ct
                    .overwrite(tag.span.start, tag.inner.start, "{__verter_attach(");
                self.rewrite_tag_close(tag, ")}");
            }
            SvelteTagKind::Unknown => {
                self.push_diag(tag.span, UnsupportedKind::Unknown);
                self.ct
                    .overwrite(tag.span.start, tag.inner.start, "{__verter_void(");
                self.rewrite_tag_close(tag, ")}");
            }
        }
    }

    /// Rewrite a tag's closing `}` (the last `}` before tag.span.end).
    fn rewrite_tag_close(&mut self, tag: &SvelteTag, replacement: &str) {
        // The tag ends with `}`. Overwrite the final `}` (tag.span.end-1..end).
        if tag.span.end > tag.inner.end {
            self.ct.overwrite(tag.inner.end, tag.span.end, replacement);
        }
    }

    /// Rewrite every experimental await-EXPRESSION (F6) inside a MARKUP
    /// expression `span` (a markup interpolation, an attribute / directive value,
    /// a block-head condition / iterable, a tag inner) to the real checkable form
    /// `await ARG` → `__verter_await_expr(ARG)`. `__verter_render` STAYS SYNC: a
    /// raw `await` outside an async fn would be INVALID TSX, so EVERY markup
    /// expression position is rewritten (not just interpolations) — otherwise an
    /// `<img src={await load()} />` would leak a raw `await` into the sync render
    /// fn. The PromiseLike-constrained helper flows `Awaited<typeof ARG>` to the
    /// use site; the awaited ARG bytes stay Original (hover / mapping preserved),
    /// only the synthetic wrapper bytes are inserted via CodeTransform ops (no
    /// post-hoc string splice). The scan is grammar-correct (OXC) — it catches a
    /// leading `{await x}`, a NESTED `{foo(await bar())}`, and an `await` inside
    /// `$derived(await …)` in markup, and SKIPS an `await` inside an async
    /// fn/arrow body. One INFORMATIONAL diagnostic is recorded per await position.
    /// Composes with any boundary overwrite the caller applies to the same span.
    pub(super) fn rewrite_await_exprs_in(&mut self, span: Span) {
        // Slice from the `'a`-lifetime source (copied out of `self` so the borrow
        // is NOT tied to `&self` and does not block the mutable `self.ct` /
        // `self.push_diag` uses below).
        let source = self.source;
        let body = &source[span.start as usize..span.end as usize];
        // The byte transform routes the SHARED `rewrite_await_exprs_on` helper (the
        // one entry the TEXT path also calls), so both paths emit byte-identical
        // wrapper ops — there is one source of truth for the rewrite.
        rewrite_await_exprs_on(self.ct, span.start, body);
        self.record_await_diagnostics_in(span);
    }

    /// Record one INFORMATIONAL `svelte-await-experimental` diagnostic per
    /// experimental await-expression in a MARKUP expression `span`, WITHOUT
    /// applying the byte transform. The span-based [`rewrite_await_exprs_in`] runs
    /// this alongside its in-place transform; the TEXT-path markup-expression
    /// entries (the F8 dynamic-component `this`, the hoisted `{@const}`/`{@let}`
    /// inner) call this directly because their byte transform happens on a copied
    /// text fragment that carries no source span — the diagnostic must still anchor
    /// at the ORIGINAL absolute `await` keyword position so the hint lands on the
    /// real source token.
    pub(super) fn record_await_diagnostics_in(&mut self, span: Span) {
        let source = self.source;
        let body = &source[span.start as usize..span.end as usize];
        for at in scan_await_positions(body) {
            let kw_start = span.start + at.keyword_start;
            let kw = Span::new(kw_start, kw_start + "await".len() as u32);
            self.push_diag(kw, UnsupportedKind::AwaitExperimental);
        }
    }

    fn push_diag(&mut self, span: Span, kind: UnsupportedKind) {
        self.diagnostics.push(SvelteIdeUnsupportedDiagnostic {
            code: kind.code(),
            message: kind.message().to_string(),
            severity: kind.severity(),
            span,
        });
    }

    /// Find the byte index of the first `needle` char at or after `from`.
    fn find_char_after(&self, from: u32, needle: char) -> Option<u32> {
        let bytes = self.source.as_bytes();
        let mut i = from as usize;
        while i < bytes.len() {
            if bytes[i] == needle as u8 {
                return Some(i as u32);
            }
            i += 1;
        }
        None
    }

    /// Find the start index of the last `needle` substring before `before`.
    fn find_str_before(&self, before: u32, needle: &str) -> Option<u32> {
        let hay = &self.source[..(before as usize).min(self.source.len())];
        hay.rfind(needle).map(|i| i as u32)
    }
}

fn node_span(node: &SvelteNode) -> Option<Span> {
    Some(match node {
        SvelteNode::Text(s) | SvelteNode::Comment(s) | SvelteNode::Interpolation(s) => *s,
        SvelteNode::Element(el) => el.open_span,
        SvelteNode::Block(b) => b.span,
        SvelteNode::Tag(t) => t.span,
    })
}
