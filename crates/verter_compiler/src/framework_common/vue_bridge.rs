//! Vue carrier bridge: the frontend half of the Vue carrier.
//!
//! [`parse_sfc`] parses an SFC; [`VueParseCarrier`] wraps the parsed SFC;
//! [`build_vue_parse_artifact`] produces the unregistered artifact; the
//! projector owns inventory geometry. Typed carrier is reachable only through
//! the frontend's downcasts or [`open_vue_carrier`]
//! (`carrier_downcast_confined_to_owning_adapter`). Runtime-bundle
//! orchestration lives in [`super::vue_bundle`].

use std::any::Any;
use std::sync::Arc;

use verter_language::{
    parse_key_for, syntax_profile_id_for, CarrierParse, FileLanguage, FrameworkAdapterId,
    JsModuleKind, LanguageId, ScriptSourceType, UnregisteredFrameworkParseArtifact,
    VUE_SYNTAX_COMPATIBILITY_DOMAIN, VUE_SYNTAX_COMPATIBILITY_EPOCH,
};
use verter_span::Span;

use verter_parser::parser::types::ParsedSfc;
use verter_parser::types::NodeProp;

use crate::diagnostics::{CompilerErrorCode, Diagnostic, DiagnosticSeverity};
use crate::framework_common::FrameworkParseArtifact;
use crate::parser::types::RootNodeScript;
use verter_language::ParseOptions;

/// The concrete Vue carrier: the full parsed SFC behind the erasure
/// seam.
#[derive(Debug, verter_retention::heap_size::HeapSize)]
pub struct VueParseCarrier {
    parsed: Arc<ParsedSfc>,
    parse_options: ParseOptions,
}

impl VueParseCarrier {
    /// Wrap a parsed SFC together with the parse-affecting options that
    /// produced it.
    pub fn new(parsed: Arc<ParsedSfc>, parse_options: ParseOptions) -> Self {
        Self {
            parsed,
            parse_options,
        }
    }

    /// The wrapped parse result.
    pub fn parsed(&self) -> &ParsedSfc {
        &self.parsed
    }

    /// The wrapped parse result, as the shared handle.
    pub fn parsed_arc(&self) -> &Arc<ParsedSfc> {
        &self.parsed
    }

    /// Parse-affecting options stamped at artifact construction.
    pub fn parse_options(&self) -> &ParseOptions {
        &self.parse_options
    }
}

impl CarrierParse for VueParseCarrier {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Attribute lookup mirroring the session's historical
/// `extract_attrs` + `find_attr` semantics byte-for-byte: names match
/// case-insensitively, a present-but-empty value reads as `"true"`.
/// Returns the value string plus the value span when one exists.
fn attr_value(props: &[NodeProp], source: &str, name: &str) -> Option<(String, Option<Span>)> {
    props.iter().find_map(|p| {
        let attr_name = &source[p.start as usize..p.name_end as usize];
        if !attr_name.eq_ignore_ascii_case(name) {
            return None;
        }
        match (p.value_start, p.value_end) {
            (Some(s), Some(e)) => {
                let value = &source[s as usize..e as usize];
                if value.is_empty() {
                    Some(("true".to_string(), Some(Span::new(s, e))))
                } else {
                    Some((value.to_string(), Some(Span::new(s, e))))
                }
            }
            _ => Some(("true".to_string(), None)),
        }
    })
}

/// Resolve the SFC's script source type from `<script lang>` data —
/// the first script block (plain `<script>` before `<script setup>`)
/// carrying an explicit `lang` attribute decides; `tsx`/`jsx`/`js`
/// (ASCII-case-insensitively) map to their dialects, anything else is
/// TypeScript.
pub fn vue_script_source_type(parsed: &ParsedSfc, source: &str) -> ScriptSourceType {
    let lang = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .find_map(|script| {
            attr_value(&script.attributes, source, "lang")
                .map(|(value, _)| value)
                .filter(|v| v != "true")
        });

    // The JS module kinds pin the historical Vue carrier dialects:
    // `lang="js"` resolves the classic-script grammar
    // (`JsModuleKind::Script`) and `lang="jsx"` the module grammar
    // (`JsModuleKind::Module`) — the exact OXC `SourceType`s
    // (`script()` / `jsx()`) the Vue parse pipeline has always
    // computed for these rows.
    match lang.as_deref().map(|value| value.to_ascii_lowercase()) {
        Some(lang) if lang == "tsx" => ScriptSourceType::Tsx,
        Some(lang) if lang == "jsx" => ScriptSourceType::Jsx(JsModuleKind::Module),
        Some(lang) if lang == "js" => ScriptSourceType::Js(JsModuleKind::Script),
        _ => ScriptSourceType::Ts,
    }
}

/// Map the Vue carrier's own parse diagnostics onto the framework-neutral
/// mapped channel. Every entry carries a real retained span (`Diagnostic`'s
/// own `span` field, never a fabricated one); the code crosses as the
/// `CompilerErrorCode`'s stable `code_str()` spelling, and arguments pass
/// through directly (`verter_parser::diagnostics::Diagnostic::arguments` is
/// already `Vec<verter_language::DiagnosticArg>`).
fn vue_parse_diagnostics(
    diagnostics: &[crate::diagnostics::Diagnostic],
    parse_key: &verter_language::ParseKey,
) -> Vec<verter_language::LanguageDiagnostic> {
    let mut mapped: Vec<verter_language::LanguageDiagnostic> = diagnostics
        .iter()
        .map(|diagnostic| verter_language::LanguageDiagnostic {
            span: diagnostic.span,
            severity: match diagnostic.severity {
                crate::diagnostics::DiagnosticSeverity::Error => {
                    verter_language::LanguageDiagnosticSeverity::Error
                }
                crate::diagnostics::DiagnosticSeverity::Warning => {
                    verter_language::LanguageDiagnosticSeverity::Warning
                }
                crate::diagnostics::DiagnosticSeverity::Info => {
                    verter_language::LanguageDiagnosticSeverity::Info
                }
            },
            code: diagnostic.code.code_str(),
            arguments: diagnostic.arguments.clone(),
            message: diagnostic.message.clone(),
            // Every Vue parse-time diagnostic (`MissingSfcEntryBlock`,
            // `TemplateFunctionalUnsupported`, …) describes a defect the
            // compiler cannot safely produce output around — unlike
            // Svelte's recoverable strict-parse facts, there is no
            // "faithful tree, still compilable" middle ground here.
            blocks_compile: true,
        })
        .collect();
    verter_language::sort_language_diagnostics(parse_key, &mut mapped);
    mapped
}

/// Wrap a parsed Vue SFC for the registered projector.
pub fn build_vue_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSfc>,
    options: &ParseOptions,
) -> Arc<UnregisteredFrameworkParseArtifact> {
    let language = FileLanguage::vue();
    let syntax_profile =
        syntax_profile_id_for(&language, options).expect("the built-in Vue syntax profile exists");
    let parse_key = parse_key_for(
        source,
        &language,
        VUE_SYNTAX_COMPATIBILITY_DOMAIN,
        VUE_SYNTAX_COMPATIBILITY_EPOCH,
        &syntax_profile,
    )
    .expect("the built-in Vue parse identity exists");
    let diagnostics = vue_parse_diagnostics(&parsed.diagnostics, &parse_key);
    Arc::new(UnregisteredFrameworkParseArtifact::new(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        Arc::new(parse_key),
        Arc::new(syntax_profile),
        diagnostics,
        Arc::new(VueParseCarrier::new(parsed, options.clone())),
    ))
}

/// The registered-projector opener installed on the Vue framework leg
/// (`CarrierLeg::open`). Returns the artifact's erased carrier ONLY for a
/// Vue-adapter artifact — the sole cross-crate entry point for reaching a
/// Vue registered artifact's typed carrier.
#[doc(hidden)]
pub fn open_vue_carrier(artifact: &FrameworkParseArtifact) -> Option<Arc<dyn CarrierParse>> {
    artifact.erased_carrier_for_adapter(&FrameworkAdapterId::vue())
}

/// Parse an SFC source string into a [`ParsedSfc`].
///
/// Tokenizes in SFC mode with optional custom delimiters and custom element
/// prefixes. The result can be cached and passed to [`compile_from_parsed`]
/// to avoid re-parsing the same source.
pub(crate) fn parse_sfc(
    input: &str,
    delimiters: Option<(&str, &str)>,
    custom_elements: Option<&[String]>,
) -> ParsedSfc {
    verter_audit::attribute_n!(CarrierParse, input.len());
    let mut parsed = verter_parser::parser::parse_sfc(input, delimiters, custom_elements);
    // Official Vue's SFC parser (`packages/compiler-sfc/src/parse.ts`)
    // unconditionally rejects a carrier with no `<template>`, `<script>`, or
    // `<script setup>` block — see the `# 6676` regression test in
    // `parse.spec.ts` ("should throw error if no <template> or <script> is
    // present"). Verter deliberately narrows that one rule: a block-less,
    // trivia-only carrier (empty, or whitespace/comments only) is still
    // admitted as an empty component shell — pre-existing base behavior
    // (`empty_sfc_compiles_to_empty_component_shell`) for a freshly created
    // blank file. Any other block-less content (styles, custom blocks,
    // malformed comments, arbitrary top-level text) is still diagnosed.
    //
    // A `<script>`/`<script setup>` block only counts as a real entry block
    // when official Vue would keep it: `parse.ts`'s `ignoreEmpty` branch
    // (lines 159-169) drops a script/scriptSetup node from the descriptor
    // when `isEmpty()` (lines 421-429 — every child is whitespace-only text,
    // or there are no children at all) holds AND the block has no `src`
    // attribute. `<script/>`, `<script></script>`, and whitespace-only
    // script content are therefore NOT entry blocks, matching
    // `parse.spec.ts`'s "should ignore other nodes with no content" case.
    if parsed.template_ast.is_none()
        && !script_node_counts_as_entry_block(parsed.script_node.as_ref(), input)
        && !script_node_counts_as_entry_block(parsed.script_setup_node.as_ref(), input)
        && (!parsed.style_nodes.is_empty()
            || !parsed.unknown_nodes.is_empty()
            || !is_empty_sfc_trivia(input))
    {
        parsed.diagnostics.push(Diagnostic::error(
            "syntax",
            CompilerErrorCode::MissingSfcEntryBlock,
            crate::common::Span::new(0, input.len() as u32),
        ));
    }
    if let Some(template) = parsed.template_ast.as_ref() {
        if let Some(attribute) = template.root.attributes.iter().find(|attribute| {
            input[attribute.start as usize..attribute.name_end as usize]
                .eq_ignore_ascii_case("functional")
        }) {
            parsed.diagnostics.push(Diagnostic::error(
                "syntax",
                CompilerErrorCode::TemplateFunctionalUnsupported,
                crate::common::Span::new(attribute.start, attribute.name_end),
            ));
        }
    }
    verter_parser::diagnostics::sort_diagnostics(&mut parsed.diagnostics);
    parsed.has_errors = parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
    parsed
}

/// Whether a parsed `<script>`/`<script setup>` node counts as a real entry
/// block for `MissingSfcEntryBlock`, matching official Vue's `isEmpty()` +
/// `ignoreEmpty` combination (`compiler-sfc/src/parse.ts` lines 159-169 and
/// 421-429): a `src`-attributed block always counts (line 165's
/// `!hasAttr(node, 'src')` guard — `hasAttr` tests ATTRIBUTE PRESENCE, line
/// 413-415, never the attribute's value), otherwise it counts only when its
/// content has at least one non-whitespace byte — a self-closing tag (no
/// content span at all) or whitespace-only content does not.
///
/// `RootNodeScript::src` only carries the `src` attribute's VALUE span (`None`
/// for a valueless `src`, e.g. `<script src/>` / `<script src></script>`), so
/// testing it directly would wrongly treat a present-but-valueless `src` as
/// absent. Test presence on the full attribute list instead.
///
/// The name match is CASE-SENSITIVE, matching `hasAttr`'s `p.name === name`
/// (`parse.ts:413-415`): Vue's own attribute-name parsing preserves the
/// authored spelling verbatim (`onattribname`'s `name: getSlice(start, end)`
/// in `compiler-core/src/parser.ts` — no `toLowerCase()`), so `<script SRC>`
/// / `<script Src>` do NOT count as `src`-attributed there, and Verter's own
/// parser likewise never case-folds attribute name spans.
fn script_node_counts_as_entry_block(node: Option<&RootNodeScript>, input: &str) -> bool {
    let Some(node) = node else {
        return false;
    };
    let has_src_attr = node
        .attributes
        .iter()
        .any(|attribute| &input[attribute.start as usize..attribute.name_end as usize] == "src");
    if has_src_attr {
        return true;
    }
    match node.content {
        Some(span) => !input[span.start as usize..span.end as usize]
            .trim()
            .is_empty(),
        None => false,
    }
}

/// Verter deliberately admits a block-less, trivia-only carrier as an empty
/// component shell. Vue's parser rejects every carrier without a template or
/// script, so keep this exception narrower than the structural no-entry test:
/// styles, custom blocks, malformed comments, and arbitrary top-level text are
/// still diagnosed as [`CompilerErrorCode::MissingSfcEntryBlock`].
fn is_empty_sfc_trivia(mut source: &str) -> bool {
    loop {
        source = source.trim_start();
        if source.is_empty() {
            return true;
        }
        let Some(comment_body) = source.strip_prefix("<!--") else {
            return false;
        };
        let Some(comment_end) = comment_body.find("-->") else {
            return false;
        };
        source = &comment_body[comment_end + 3..];
    }
}
