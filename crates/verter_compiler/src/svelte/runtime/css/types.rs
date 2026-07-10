//! The span-bearing Svelte CSS AST + the per-`<style>` scope-plan side table.
//!
//! The AST mirrors the official `svelte@5.56.3` CSS AST node family
//! (`phases/1-parse/read/style.js` — `StyleSheet` / `Rule` / `Atrule` /
//! `SelectorList` / `ComplexSelector` / `RelativeSelector` / `Combinator` /
//! the simple-selector nodes / `Block` / `Declaration`) with one Rust-shape
//! difference: the JS `metadata.rule` / `metadata.parent_rule` OBJECT
//! back-pointers are not stored — a walker (the analyzer, the selector
//! matcher, the renderer) carries its own ancestor-rule context, exactly as
//! the official zimmerframe visitors carry `context.path` / `state.rule`.
//! Every node carries a byte [`Span`] of ABSOLUTE offsets into the ORIGINAL
//! component source (the `<style>` body region), so source-position edits map
//! directly onto the component source.

use verter_span::Span;

use crate::svelte::runtime::ir::NodeId;

/// The parsed CSS body of one `<style>` block — the official `StyleSheet`
/// node (minus the open-tag attributes, which stay on the carrier's
/// [`SvelteStyle`](crate::svelte::parser::SvelteStyle)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleSheet {
    /// The CSS BODY span (between `<style>` and `</style>`) — the official
    /// `content.start`/`content.end` region, absolute in the component source.
    pub span: Span,
    /// The top-level rules / at-rules, in source order.
    pub children: Vec<StyleChild>,
}

/// One top-level or block-nested rule-position item (`Rule | Atrule`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleChild {
    /// A style rule (`selector-list { … }`).
    Rule(Rule),
    /// An at-rule (`@media … { … }` / `@import …;` / `@keyframes name { … }`).
    Atrule(Atrule),
}

/// An at-rule — the official `Atrule` node. `name` is the DECODED identifier
/// (unicode escapes resolved); `prelude` is the official raw TRIMMED value
/// text (escapes NOT decoded — the official `read_value` accumulates raw
/// characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atrule {
    /// The whole at-rule (from `@` through the block's `}` or the `;`).
    pub span: Span,
    /// The at-rule NAME (decoded identifier, e.g. `media` / `keyframes` /
    /// `-webkit-keyframes`).
    pub name: String,
    /// The span of the name identifier (after the `@`).
    pub name_span: Span,
    /// The TRIMMED prelude value text (the official `read_value` result).
    pub prelude: String,
    /// The RAW prelude region (from the byte after the name identifier to the
    /// byte before the block `{` / the terminator) — untrimmed, so consumers
    /// locate prelude tokens positionally.
    pub prelude_span: Span,
    /// The `{ … }` block, when present (`@media` / `@keyframes` /
    /// `@supports`); `None` for a statement at-rule (`@import …;`).
    pub block: Option<Block>,
}

/// A style rule — the official `Rule` node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// From the first selector byte through the block's `}`.
    pub span: Span,
    /// The selector list before the block.
    pub prelude: SelectorList,
    /// The declaration / nested-rule block.
    pub block: Block,
    /// The analysis metadata (populated by the analyzer walk).
    pub metadata: RuleMetadata,
}

/// The official `Rule.metadata` analysis facts (minus the `parent_rule`
/// object back-pointer — walkers carry ancestor context instead; the stored
/// [`is_nested`](Self::is_nested) fact records parent-rule EXISTENCE).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleMetadata {
    /// Whether ANY prelude selector is global (`:global(...)`-only or
    /// global-like).
    pub has_global_selectors: bool,
    /// Whether ANY prelude selector is local (non-global).
    pub has_local_selectors: bool,
    /// Whether this rule is a `:global { … }` BLOCK (a prelude selector
    /// starting with the argument-less `:global`).
    pub is_global_block: bool,
    /// Whether this rule is nested inside another rule (the official
    /// `metadata.parent_rule !== null` fact).
    pub is_nested: bool,
}

/// A selector list — the official `SelectorList` node (a rule prelude, or the
/// arguments of a pseudo-class selector).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorList {
    /// From the first selector byte to the end of the LAST selector (trailing
    /// whitespace excluded).
    pub span: Span,
    /// The comma-separated complex selectors.
    pub children: Vec<ComplexSelector>,
}

/// One comma-separated selector — the official `ComplexSelector` node (a
/// chain of relative selectors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexSelector {
    /// From the first byte of the first relative selector to the end of the
    /// last (pre-whitespace rewind point).
    pub span: Span,
    /// The combinator-chained relative selectors.
    pub children: Vec<RelativeSelector>,
    /// The analysis metadata (populated by the analyzer walk).
    pub metadata: ComplexSelectorMetadata,
}

/// The official `ComplexSelector.metadata` analysis facts (minus the `rule`
/// object back-pointer).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComplexSelectorMetadata {
    /// Whether EVERY child relative selector is global or global-like.
    pub is_global: bool,
    /// Whether the selector is used (globals are used by definition; the
    /// selector-to-template matcher marks the matched ones).
    pub used: bool,
}

/// One compound step of a complex selector — the official `RelativeSelector`
/// node (an optional leading combinator plus the compound's simple selectors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativeSelector {
    /// From the combinator start (when present) or the first simple-selector
    /// byte, to the compound's end (pre-whitespace rewind point).
    pub span: Span,
    /// The combinator joining this compound to the previous one (`None` for
    /// the first compound).
    pub combinator: Option<Combinator>,
    /// The compound's simple selectors, in source order.
    pub selectors: Vec<SimpleSelector>,
    /// The analysis metadata (populated by the analyzer walk).
    pub metadata: RelativeSelectorMetadata,
}

/// The official `RelativeSelector.metadata` analysis facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelativeSelectorMetadata {
    /// Whether the compound is `:global(...)` / `:global` per the official
    /// `is_global` rule (unscoped pseudo-classes / pseudo-elements only after
    /// the leading `:global`).
    pub is_global: bool,
    /// Whether the compound is global-LIKE (`:host` / `:root` /
    /// `::view-transition*`, or a compound following a `:global` block
    /// selector in the same complex selector).
    pub is_global_like: bool,
    /// Whether the compound matched a template element and receives the scope
    /// class (set by the selector-to-template matcher).
    pub scoped: bool,
}

/// A selector combinator — the official `Combinator` node. `name` is `"+"` /
/// `"~"` / `">"` / `"||"`, or `" "` for the whitespace descendant combinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combinator {
    /// The combinator token span; for the descendant combinator, the whole
    /// whitespace run.
    pub span: Span,
    /// The combinator name (`" "` for descendant).
    pub name: String,
}

/// One simple selector — the official simple-selector node family. Names are
/// DECODED identifiers (unicode escapes resolved, as the official
/// `read_identifier` does); attribute values / nth / percentage payloads stay
/// RAW text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    /// `div` / `*` / `svg|rect` (the namespace is read and IGNORED — `name`
    /// is the local part, matching official).
    Type {
        /// The node span (covers a namespace prefix when present).
        span: Span,
        /// The (local) type name; `*` for the universal selector.
        name: String,
    },
    /// `#id`.
    Id {
        /// The node span.
        span: Span,
        /// The id name (without `#`).
        name: String,
    },
    /// `.class`.
    Class {
        /// The node span.
        span: Span,
        /// The class name (without `.`).
        name: String,
    },
    /// `[attr]` / `[attr=value]` / `[attr~="v" i]`.
    Attribute {
        /// The node span (covers the brackets).
        span: Span,
        /// The attribute name.
        name: String,
        /// The matcher operator (`=` / `~=` / `^=` / `$=` / `*=` / `|=`),
        /// when present.
        matcher: Option<String>,
        /// The RAW attribute value (quotes stripped, escapes kept), when a
        /// matcher is present.
        value: Option<String>,
        /// The trailing flags run (`i` / `s`), when present.
        flags: Option<String>,
    },
    /// `:name` / `:name(args)` (`:global` / `:has` / `:is` / `:where` /
    /// `:not` / `:root` / `:host` / …).
    PseudoClass {
        /// The node span (INCLUDES the parenthesized args, matching official).
        span: Span,
        /// The pseudo-class name (without the `:`).
        name: String,
        /// The parenthesized selector-list arguments, when present.
        args: Option<SelectorList>,
    },
    /// `::name` (`::before` / `::view-transition` / …).
    PseudoElement {
        /// The node span (EXCLUDES any parenthesized args — the official node
        /// is pushed before its args are read and discarded).
        span: Span,
        /// The pseudo-element name (without the `::`).
        name: String,
    },
    /// A keyframe percentage step selector (`50%`).
    Percentage {
        /// The node span.
        span: Span,
        /// The raw matched text (`50%`).
        value: String,
    },
    /// An `An+B` nth token inside a pseudo-class (`2n+1` / `odd` /
    /// `2n+1 of `…). The value is the raw matched text — INCLUDING a
    /// consumed ` of ` arm, matching the official `REGEX_NTH_OF` read.
    Nth {
        /// The node span.
        span: Span,
        /// The raw matched text.
        value: String,
    },
    /// The `&` nesting selector.
    Nesting {
        /// The node span.
        span: Span,
    },
}

impl SimpleSelector {
    /// The node span.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Type { span, .. }
            | Self::Id { span, .. }
            | Self::Class { span, .. }
            | Self::Attribute { span, .. }
            | Self::PseudoClass { span, .. }
            | Self::PseudoElement { span, .. }
            | Self::Percentage { span, .. }
            | Self::Nth { span, .. }
            | Self::Nesting { span } => *span,
        }
    }
}

/// A `{ … }` block — the official `Block` node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// From the `{` through the `}`.
    pub span: Span,
    /// The block items, in source order.
    pub children: Vec<BlockChild>,
}

/// One block item (`Declaration | Rule | Atrule`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockChild {
    /// A `property: value` declaration.
    Declaration(Declaration),
    /// A nested rule.
    Rule(Rule),
    /// A nested at-rule.
    Atrule(Atrule),
}

/// A declaration — the official `Declaration` node. The span ends after the
/// value and BEFORE the `;` terminator (the official `end` capture point).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// From the property's first byte to the end of the value read (pre-`;`).
    pub span: Span,
    /// The RAW property text (e.g. `color` / `--x`).
    pub property: String,
    /// The TRIMMED value text (the official `read_value` result; comments
    /// skipped, quotes/URL parens respected).
    pub value: String,
}

/// The css output mode of one component — the official `css` compile axis
/// combined with the custom-element rule (`inject_styles = css === 'injected'
/// || is_custom_element`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssMode {
    /// The default `css: 'external'` mode — the scoped CSS is emitted as a
    /// separate artifact.
    External,
    /// The `css: 'injected'` / custom-element mode — the scoped CSS is
    /// injected by the component at runtime.
    Injected,
}

/// One LOCAL `@keyframes` name — a member of the scope-RENAME list (the
/// official `analysis.css.keyframes` entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyframeName {
    /// The keyframes name exactly as the official analysis records it (the
    /// raw trimmed prelude text).
    pub name: String,
    /// The span of the name token inside the at-rule prelude (the region the
    /// renderer prefixes with `<hash>-`).
    pub name_span: Span,
}

/// One `-global-`-prefixed `@keyframes` name — EXCLUDED from the rename list;
/// the renderer strips the `-global-` prefix instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalKeyframeName {
    /// The name WITHOUT the `-global-` prefix.
    pub name: String,
    /// The span of the FULL prefixed name token inside the at-rule prelude
    /// (the first 8 bytes are the `-global-` prefix the renderer removes).
    pub name_span: Span,
}

/// The per-`<style>` scope plan — the ONE shared fact table the scope-class
/// injection sites and the css emitter read. PROVEN BY CONSTRUCTION: a value
/// of this type exists ONLY when the css body parsed + analyzed, the
/// selector-to-template matcher (`css-prune.js` port) proved EVERY
/// selector⇄template relation (the `used`/`scoped` verdicts on
/// [`ast`](Self::ast) are authoritative), and the scoped render succeeded —
/// every failure mode is the typed
/// [`StylePlanFailure`](super::StylePlanFailure) `Err` of
/// [`complete_style_scope_plan`](super::complete_style_scope_plan); the type
/// carries no unprovable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenStyleScopePlan {
    /// The scope hash (`svelte-<djb2>` over the official css-hash input).
    pub hash: String,
    /// The rendered scoped stylesheet (the official `css.code`): the css body
    /// with scope classes applied, `:global(...)` unwrapped, unused/empty
    /// rules comment-pruned (external) or REMOVED (injected — the official
    /// `inject_styles && !dev` minified render), and local `@keyframes`
    /// renamed — produced by
    /// [`render_stylesheet`](super::render::render_stylesheet) from the
    /// matcher-verdict-bearing AST, mode-faithfully per [`mode`](Self::mode).
    /// MAY legitimately be empty — the OFFICIAL empty render of an empty /
    /// all-global-pruned stylesheet (`compiled.css.code = ''`), never a
    /// failure sentinel.
    pub css_code: String,
    /// The css source-map JSON for [`css_code`](Self::css_code) (the official
    /// `css.map`) — `Some` ONLY when the plan build was asked for it
    /// (`want_source_map`), generated by the render from the SAME shared
    /// transform that produced the code. Its mappings point rendered css
    /// positions back to the ORIGINAL component source.
    pub source_map: Option<String>,
    /// The CSS BODY span in the original component source.
    pub css_body_span: Span,
    /// The LOCAL `@keyframes` rename list (the official
    /// `analysis.css.keyframes`), source order.
    pub keyframes: Vec<KeyframeName>,
    /// The `-global-`-prefixed `@keyframes` names (prefix-strip list), source
    /// order.
    pub global_keyframes: Vec<GlobalKeyframeName>,
    /// Whether the component's css includes GLOBAL css (the official
    /// `analysis.css.has_global`).
    pub has_global: bool,
    /// The css output mode.
    pub mode: CssMode,
    /// The analyzed span-bearing CSS AST (metadata populated: the analyzer's
    /// global facts plus the matcher's `used` / `scoped` selector verdicts).
    pub ast: StyleSheet,
    /// The PROVEN template-side matcher facts (the official `css-prune.js`
    /// `prune(stylesheet, elements)` per-element scope marks).
    pub facts: MatchedTemplateFacts,
}

/// The template-side facts a proven match produces — the official
/// `element.metadata.scoped` marks, keyed by runtime-IR node id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchedTemplateFacts {
    /// The template elements at least one used selector might apply to (the
    /// elements that receive the scope class).
    pub scoped: rustc_hash::FxHashSet<NodeId>,
}

/// The narrow scope-class INJECTION view of a proven style plan — the ONE
/// `(hash, scoped-element set)` fact pair EVERY scope-class injection site
/// reads: the static skeleton bake, the `$.set_class` value/`css_hash`
/// threading, and the spread `$.attribute_effect` hash argument. All sites
/// derive from the same [`ProvenStyleScopePlan`] via
/// [`scope_facts`](ProvenStyleScopePlan::scope_facts), so the sites cannot
/// disagree on the hash or on which elements are scoped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssScopeFacts {
    /// The scope hash (`svelte-<djb2>`).
    pub hash: String,
    /// The scoped template elements (the matcher's per-element scope marks).
    pub scoped: rustc_hash::FxHashSet<NodeId>,
}

impl CssScopeFacts {
    /// The scope hash for one element: `Some(hash)` when `node` is scoped,
    /// `None` otherwise — the per-element read both injection sites share.
    #[must_use]
    pub fn hash_for(&self, node: NodeId) -> Option<&str> {
        self.scoped.contains(&node).then_some(self.hash.as_str())
    }
}

impl ProvenStyleScopePlan {
    /// The scope-injection facts of the plan. A constructed plan ALWAYS has
    /// proven facts (unprovable inputs never construct a plan — they fail the
    /// build with a typed [`StylePlanFailure`](super::StylePlanFailure)), so
    /// this projection is total.
    #[must_use]
    pub fn scope_facts(&self) -> CssScopeFacts {
        CssScopeFacts {
            hash: self.hash.clone(),
            scoped: self.facts.scoped.clone(),
        }
    }
}
