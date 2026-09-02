//! Root-level SFC block types.
//!
//! Each Vue SFC has up to four kinds of root blocks:
//! - `<script>` / `<script setup>` → [`RootNodeScript`]
//! - `<style>` (multiple allowed) → [`RootNodeStyle`]
//! - `<template>` → [`RootNodeTemplate`] (with children in [`RootNodeTemplateContent`])
//! - Custom blocks (e.g., `<i18n>`, `<docs>`) → [`RootNodeUnknown`]
//!
//! These types store the tag positions, special attributes (lang, scoped,
//! setup, etc.), and the raw content span between open and close tags.

use smallvec::SmallVec;

use crate::{
    ast::types::{ConditionalChain, TemplateAst},
    common::Span,
    cursor::ScriptLanguage,
    diagnostics::Diagnostic,
    types::{NodeId, NodeProp, NodeTag},
};

/// Discriminant for SFC root block kind, resolved from the tag name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootNodeKind {
    Script,
    Template,
    Style,
    /// Any tag that isn't `script`, `template`, or `style` (e.g., `<i18n>`).
    Unknown,
}

/// Style preprocessor language, parsed from `<style lang="...">`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleLang {
    Css = 0,
    Scss,
    Sass,
    Less,
    Stylus,
    /// `lang="postcss"` and its equally common spelling `lang="pcss"`. One
    /// recognised dialect with no native grammar here:
    /// PostCSS blocks are CSS-shaped and the IDE serves them with the CSS
    /// service, while the rewrite pipeline still treats them as content an
    /// external tool owns. Classifying it is what lets those two answers
    /// differ; folding it into [`Self::Unknown`] made a mainstream Vue
    /// configuration indistinguishable from a typo and served it nothing.
    PostCss,
    Unknown,
}

/// A parsed `<script>` or `<script setup>` SFC root block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeScript {
    /// Tag span for `<script ...>`.
    pub tag_open: NodeTag,
    /// Tag span for `</script>`, or `None` if self-closing / unclosed.
    pub tag_close: Option<NodeTag>,

    /// Whether the `setup` attribute was present.
    pub is_setup: bool,
    /// Parsed `lang` attribute (e.g., `ts`, `tsx`).
    pub lang: Option<ScriptLanguage>,
    /// The `lang` attribute's VALUE, HTML-entity-decoded — Vue's own
    /// `block.lang`. `None` when the attribute is absent.
    ///
    /// Kept beside the parsed [`Self::lang`] because the two answer different
    /// questions. [`Self::lang`] answers "which dialect do we generate for",
    /// where `ts` and `typescript` are the same answer and every unrecognised
    /// spelling collapses to [`ScriptLanguage::Unknown`]. This answers "did the
    /// author write the same thing in both blocks", which is the comparison
    /// Vue's `compileScript` performs (`scriptLang !== scriptSetupLang`) — and
    /// under the parsed enum `ts` vs `typescript`, and two DIFFERENT
    /// unrecognised spellings, would both compare equal while Vue rejects.
    ///
    /// It is the DECODED value, not the source bytes, because that is what Vue
    /// compares: its SFC parser entity-decodes attribute values, so
    /// `lang="t&#115;"` is `ts` there and the pair `t&#115;`/`ts` is accepted.
    /// Comparing the source bytes rejects it — a FALSE refusal, and a build
    /// that refuses valid Vue is worse than one that mislabels it. The same
    /// decoded text also drives [`Self::lang`], so a dialect is classified from
    /// what the author wrote rather than from how they spelled it.
    pub lang_value: Option<Box<str>>,
    /// `src="..."` attribute value span (external script source).
    pub src: Option<Span>,
    /// `generic="..."` attribute value span (generic type parameters for `<script setup>`).
    pub generic: Option<Span>,
    /// `attrs="..."` or `attributes="..."` attribute value span (typed `$attrs`).
    pub attrs: Option<Span>,

    /// All attributes on the tag (including already-parsed special ones).
    pub attributes: Vec<NodeProp>,

    /// Raw content span between open and close tags. `None` if self-closing.
    pub content: Option<Span>,
}

/// The authored script dialect of an SFC — the ONE classification every
/// generated-companion consumer shares.
///
/// TypeScript decides what to typecheck AND how to PARSE from a file's
/// extension/ScriptKind, not from a compiler flag
/// (`typescript/lib/typescript.js`'s `getScriptKindFromFileName`): `.ts`/`.tsx`
/// are ALWAYS checked (so `strict`/`noImplicitAny` fires on every untyped
/// parameter) while `.js`/`.jsx` are checked only under `checkJs`, and
/// independently `.jsx`/`.tsx` accept JSX syntax while `.js`/`.ts` do not (in a
/// `.ts` file `<div/>` parses as a type assertion). Both axes are load-bearing,
/// so this is a FOUR-way classification and never a `is_javascript: bool` —
/// collapsing it mislabels `lang="jsx"` as `.js` and `lang="tsx"` as `.ts`,
/// which turns an authored `<div/>` into a syntax error.
///
/// Every companion Verter hands an engine is labelled from here: the IDE
/// validation carrier's `.jsx`/`.tsx` extension reads
/// [`Self::is_javascript`] (the carrier is JSX-bearing by construction — the
/// template lowers into it — so only the JS-vs-TS axis is open there), and the
/// public-API stub's extension reads [`Self::extension`]. One classifier, so
/// the two surfaces of the same file can never disagree about its ScriptKind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfcScriptDialect {
    /// `lang="js"` / `lang="javascript"` / no `lang` — `.js`, JS ScriptKind.
    JavaScript,
    /// `lang="jsx"` — `.jsx`, JSX ScriptKind.
    Jsx,
    /// `lang="ts"` / `lang="typescript"`, and every generated declaration
    /// surface — `.ts`, TS ScriptKind.
    TypeScript,
    /// `lang="tsx"` — `.tsx`, TSX ScriptKind.
    Tsx,
}

impl SfcScriptDialect {
    /// Whether this dialect is JavaScript (checked only under `checkJs`)
    /// rather than TypeScript (always checked).
    #[must_use]
    pub const fn is_javascript(self) -> bool {
        matches!(self, Self::JavaScript | Self::Jsx)
    }

    /// Whether this dialect's ScriptKind accepts JSX syntax.
    #[must_use]
    pub const fn is_jsx(self) -> bool {
        matches!(self, Self::Jsx | Self::Tsx)
    }

    /// The file extension (without a leading dot) TypeScript maps to this
    /// dialect's ScriptKind.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::JavaScript => "js",
            Self::Jsx => "jsx",
            Self::TypeScript => "ts",
            Self::Tsx => "tsx",
        }
    }
}

/// The `lang` of an SFC's `<script setup>` and its plain `<script>` disagree.
///
/// Vue REJECTS such an SFC outright (`@vue/compiler-sfc`'s `compileScript`
/// throws ``[@vue/compiler-sfc] <script> and <script setup> must have the same
/// language type.``), and so does Verter — the parser emits
/// [`CompilerErrorCode::ScriptLangMismatch`] for it.
///
/// [`CompilerErrorCode::ScriptLangMismatch`]: crate::diagnostics::CompilerErrorCode::ScriptLangMismatch
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfcScriptLangConflict {
    /// The `<script setup>` block's decoded `lang` value, `None` when absent.
    pub setup_lang: Option<Box<str>>,
    /// The plain `<script>` block's decoded `lang` value, `None` when absent.
    pub script_lang: Option<Box<str>>,
}

/// Whether the SFC's two script blocks disagree about `lang`.
///
/// **The single implementation of this rule.** Both blocks must be present for a
/// conflict to exist — a lone block never conflicts with anything.
///
/// The comparison is over each block's DECODED `lang` value, which is exactly
/// what Vue's `compileScript` compares:
///
/// ```js
/// const scriptLang = script && script.lang
/// const scriptSetupLang = scriptSetup && scriptSetup.lang
/// if (script && scriptSetup && scriptLang !== scriptSetupLang) throw new Error(
///   `[@vue/compiler-sfc] <script> and <script setup> must have the same language type.`)
/// ```
///
/// (`@vue/compiler-sfc` 3.5.x, `compileScript`.) `block.lang` there is the
/// entity-DECODED attribute value, so `lang="t&#115;"` is `ts` and Vue accepts
/// it beside `lang="ts"`. That is why [`RootNodeScript::lang_value`] holds the
/// decoded text and not the source bytes: comparing bytes rejects a pair Vue
/// accepts, and refusing valid Vue is a worse failure than mislabelling it.
///
/// What stays rejected, because Vue rejects it too: `lang="ts"` beside
/// `lang="typescript"` (both name TypeScript, but they are different strings),
/// an absent `lang` beside `lang="js"` (both mean JavaScript), and two DIFFERENT
/// unrecognised spellings (`lang="coffee"` / `lang="cson"`). Comparing the
/// parsed [`ScriptLanguage`] instead would pass all three through Verter and
/// fail them in Vue — an SFC that builds here and not there.
#[must_use]
pub fn sfc_script_lang_conflict(
    script_setup: Option<&RootNodeScript>,
    script: Option<&RootNodeScript>,
) -> Option<SfcScriptLangConflict> {
    let (setup, plain) = (script_setup?, script?);
    (setup.lang_value != plain.lang_value).then(|| SfcScriptLangConflict {
        setup_lang: setup.lang_value.clone(),
        script_lang: plain.lang_value.clone(),
    })
}

/// Classify an SFC's authored script dialect.
///
/// An explicit `lang` wins, `<script setup>`'s before the plain `<script>`'s; a
/// script block with NO `lang` is JavaScript (Vue's own default); an SFC with
/// no script block at all (template-only) and an unrecognised `lang`
/// (`lang="coffee"`) both default to TypeScript.
///
/// **A mixed-language SFC has no authored dialect, and this function is not the
/// place that decides what to do about it.** Such an SFC is invalid Vue — Vue's
/// own `compileScript` THROWS — and the parser reports it
/// ([`sfc_script_lang_conflict`], `ScriptLangMismatch`). A whole-project BUILD
/// must refuse it outright rather than label it, because either label is a
/// guess that silently corrupts the result: `.ts`/`.tsx` strict-checks the
/// JavaScript block (a flood of implicit-any errors on code the project never
/// asked to have checked) and `.js`/`.jsx` deletes every genuine diagnostic in
/// the TypeScript one. That refusal lives at the build boundary
/// (`verter_tsc`), which emits the diagnostic and generates NO companion at
/// all.
///
/// This classifier still has to return something for the surfaces that keep
/// working on a transiently-invalid file — an editor must not go dark on the
/// keystroke between `<script setup lang="ts">` and its sibling gaining the
/// same `lang` — and there the answer is fail-CLOSED to TypeScript: over-
/// reporting is recoverable, silently dropping a block's diagnostics is not.
#[must_use]
pub fn sfc_script_dialect(
    script_setup: Option<&RootNodeScript>,
    script: Option<&RootNodeScript>,
) -> SfcScriptDialect {
    if sfc_script_lang_conflict(script_setup, script).is_some() {
        return SfcScriptDialect::TypeScript;
    }
    let has_any_script = script_setup.is_some() || script.is_some();
    let lang = script_setup
        .and_then(|s| s.lang)
        .or_else(|| script.and_then(|s| s.lang));
    match lang {
        Some(ScriptLanguage::TypeScript) => SfcScriptDialect::TypeScript,
        Some(ScriptLanguage::TSX) => SfcScriptDialect::Tsx,
        Some(ScriptLanguage::JavaScript) => SfcScriptDialect::JavaScript,
        Some(ScriptLanguage::JSX) => SfcScriptDialect::Jsx,
        None if has_any_script => SfcScriptDialect::JavaScript,
        None | Some(ScriptLanguage::Unknown) => SfcScriptDialect::TypeScript,
    }
}

/// A parsed `<style>` SFC root block.
///
/// Multiple `<style>` blocks are allowed in a single SFC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeStyle {
    pub tag_open: NodeTag,
    pub tag_close: Option<NodeTag>,

    /// Parsed `lang` attribute (e.g., `scss`, `less`).
    pub lang: Option<StyleLang>,
    /// Whether the `scoped` attribute was present.
    pub scoped: bool,
    /// Whether the `module` attribute was present.
    pub module: bool,

    pub attributes: Vec<NodeProp>,

    /// Raw content span between open and close tags. `None` if self-closing.
    pub content: Option<Span>,
}

/// A parsed custom/unknown SFC root block (e.g., `<i18n>`, `<docs>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeUnknown {
    pub tag_open: NodeTag,
    pub tag_close: Option<NodeTag>,

    pub attributes: Vec<NodeProp>,

    pub content: Option<Span>,
}

/// A parsed `<template>` SFC root block.
///
/// Unlike other root nodes, the template's children are parsed into a full
/// AST (via [`super::super::ast::builder::TemplateAstBuilder`]) and stored
/// in [`RootNodeTemplateContent::children`] as [`NodeId`] references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeTemplate {
    pub tag_open: NodeTag,
    pub tag_close: Option<NodeTag>,

    /// Raw `lang` attribute value span (e.g., `pug`). Not parsed into an
    /// enum since template languages are open-ended.
    pub lang: Option<Span>,

    pub attributes: Vec<NodeProp>,

    /// Template content region with parsed child node IDs. `None` if the
    /// template is self-closing or the content region was never opened.
    pub content: Option<RootNodeTemplateContent>,
}

/// The content region of a `<template>` root block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeTemplateContent {
    /// Byte offset immediately after `<template ...>` closing `>`.
    pub start: u32,
    /// Byte offset of `</template>` opening `<` (or EOF).
    pub end: u32,
    /// Root-level children of the template (arena node IDs).
    pub children: SmallVec<[NodeId; 4]>,
    /// Pre-computed v-if/v-else-if/v-else chains among root children.
    pub v_if_chains: SmallVec<[ConditionalChain; 1]>,
}

/// Finalized SFC parse result, suitable for caching and borrowing during codegen.
///
/// Produced by [`super::Syntax::into_parsed_sfc()`] after tokenization completes.
/// All fields are owned; consumers borrow via accessor methods. Stored behind
/// `Arc` in the host for zero-copy reuse across compile profiles.
#[derive(Debug, Clone)]
pub struct ParsedSfc {
    pub template_ast: Option<TemplateAst>,
    pub script_node: Option<RootNodeScript>,
    pub script_setup_node: Option<RootNodeScript>,
    pub style_nodes: Vec<RootNodeStyle>,
    pub unknown_nodes: Vec<RootNodeUnknown>,
    pub has_style_scope: bool,
    pub has_style_module: bool,
    pub is_vapor: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub has_errors: bool,
}

impl ParsedSfc {
    pub fn script(&self) -> Option<&RootNodeScript> {
        self.script_node.as_ref().filter(|s| !s.is_setup)
    }

    pub fn script_setup(&self) -> Option<&RootNodeScript> {
        self.script_setup_node.as_ref().filter(|s| s.is_setup)
    }

    pub fn style_nodes(&self) -> &[RootNodeStyle] {
        &self.style_nodes
    }

    pub fn unknown_nodes(&self) -> &[RootNodeUnknown] {
        &self.unknown_nodes
    }

    pub fn template_ast(&self) -> Option<&TemplateAst> {
        self.template_ast.as_ref()
    }

    pub fn has_errors(&self) -> bool {
        self.has_errors
    }

    pub fn has_style_scope(&self) -> bool {
        self.has_style_scope
    }

    pub fn is_vapor(&self) -> bool {
        self.is_vapor
    }

    /// Clone diagnostics out for ownership transfer to compile results.
    /// This is the only clone needed — all other accesses are borrows.
    pub fn clone_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.clone()
    }

    /// Bytes this parse result retains independently of the source `&str`:
    /// collection buffers by capacity, boxed script `lang` values, diagnostic
    /// message buffers, and the template arena's own retained bytes.
    pub fn retained_bytes(&self) -> usize {
        let mut n = std::mem::size_of::<Self>();
        if let Some(ast) = &self.template_ast {
            n = n.saturating_add(ast.retained_bytes());
        }
        n = n.saturating_add(script_retained_bytes(self.script_node.as_ref()));
        n = n.saturating_add(script_retained_bytes(self.script_setup_node.as_ref()));
        n = n.saturating_add(vec_cap_bytes(&self.style_nodes));
        for style in &self.style_nodes {
            n = n.saturating_add(vec_cap_bytes(&style.attributes));
            n = n.saturating_add(props_modifier_spill(&style.attributes));
        }
        n = n.saturating_add(vec_cap_bytes(&self.unknown_nodes));
        for unknown in &self.unknown_nodes {
            n = n.saturating_add(vec_cap_bytes(&unknown.attributes));
            n = n.saturating_add(props_modifier_spill(&unknown.attributes));
        }
        n = n.saturating_add(vec_cap_bytes(&self.diagnostics));
        for diagnostic in &self.diagnostics {
            n = n.saturating_add(diagnostic.message.capacity());
            n = n.saturating_add(vec_cap_bytes(&diagnostic.arguments));
            for argument in &diagnostic.arguments {
                if let verter_language::DiagnosticArg::Text(text) = argument {
                    n = n.saturating_add(text.capacity());
                }
            }
        }
        n
    }
}

fn vec_cap_bytes<T>(items: &Vec<T>) -> usize {
    items.capacity().saturating_mul(std::mem::size_of::<T>())
}

fn script_retained_bytes(script: Option<&RootNodeScript>) -> usize {
    let Some(script) = script else {
        return 0;
    };
    let mut n = vec_cap_bytes(&script.attributes);
    n = n.saturating_add(props_modifier_spill(&script.attributes));
    if let Some(lang) = &script.lang_value {
        n = n.saturating_add(lang.len());
    }
    n
}

fn props_modifier_spill(props: &[NodeProp]) -> usize {
    props
        .iter()
        .map(|prop| {
            if prop.modifiers.spilled() {
                prop.modifiers
                    .capacity()
                    .saturating_mul(std::mem::size_of::<crate::common::Span>())
            } else {
                0
            }
        })
        .fold(0usize, usize::saturating_add)
}

impl StyleLang {
    /// Parse a style language from the raw `lang` attribute bytes.
    ///
    /// Byte-exact for every dialect an external tool compiles: those tools
    /// look the spelling up by exact bytes, so `lang="SCSS"` names nothing
    /// that can build the block and must not resolve here either. `css` is
    /// matched ASCII-case-insensitively instead, because plain CSS reaches no
    /// such table — nothing downstream can fail on its casing, and refusing
    /// `lang="CSS"` would cost an unambiguously-CSS block every CSS feature
    /// the editor serves.
    pub fn from_bytes(lang: &[u8]) -> Self {
        if lang.eq_ignore_ascii_case(b"css") {
            return StyleLang::Css;
        }
        match lang {
            b"scss" => StyleLang::Scss,
            b"sass" => StyleLang::Sass,
            b"less" => StyleLang::Less,
            b"stylus" | b"styl" => StyleLang::Stylus,
            // Both spellings name the same dialect everywhere the ecosystem
            // keys on them — neither has a `@vue/compiler-sfc` processor entry,
            // and the file-extension route here already reads `.pcss` as
            // postcss. Naming only one left the other indistinguishable from a
            // typo and served it nothing.
            b"postcss" | b"pcss" => StyleLang::PostCss,
            _ => StyleLang::Unknown,
        }
    }
}
