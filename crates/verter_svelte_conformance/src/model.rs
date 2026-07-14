//! Typed roles model for the Svelte CSS-scoping conformance coverage matrix.
//!
//! Every semantic decision in the conformance manifest is expressed over the
//! exhaustive typed enums in this module — never over strings. The model keeps
//! the two representation languages strictly apart:
//!
//! - [`TemplateValueRepresentation`] is the HTML template-attribute spelling
//!   axis (literal text, HTML named / decimal / hex character references, a
//!   mixed-form spelling, plus the two UNCERTAINTY forms `Dynamic` and
//!   `Spread`, which are not static representation-equivalents of anything).
//! - [`SelectorValueRepresentation`] is the CSS selector spelling axis
//!   (literal identifiers, `\26 `-style hex escapes, `\&`-style identity
//!   escapes, and a mixed-escape spelling). HTML entities never appear on
//!   this axis and CSS escapes never appear on the template axis.
//!
//! The model owns rendering (each row renders to a compilable `.svelte`
//! fixture via [`render_fixture`]) and stable identity ([`slug`]); the
//! covering-array engine ([`crate::covering_array`]) sees only fixed-width
//! ordinal rows. [`classify`] is the typed constraint function that maps a
//! decoded row to its [`Disposition`]; the concrete refusal / diagnostic /
//! constraint enums bridge onto the engine's opaque partition ids through
//! their dense [`ordinal`](RefusalKind::ordinal)s.
//!
//! Grounding: the constraint rules and the grounded family verdicts were
//! established against the pinned official `svelte@5.56.3` compiler (kept
//! selectors stay scoped, absent values prune with `css_unused_selector`,
//! a parentless `&` rejects with `css_nesting_selector_invalid_placement`)
//! and against Verter's typed fail-closed surface (a legacy `<slot>` region
//! refuses with `svelte-runtime-unsupported-style-selector`).

use crate::covering_array::{self, Row};

/// Number of covering factors (the fixed row width).
pub const FACTOR_COUNT: usize = 9;

/// Factor index of [`SelectorKind`].
pub const FACTOR_SELECTOR_KIND: usize = 0;
/// Factor index of [`TemplateValueRepresentation`].
pub const FACTOR_TEMPLATE_VALUE: usize = 1;
/// Factor index of [`SelectorValueRepresentation`].
pub const FACTOR_SELECTOR_VALUE: usize = 2;
/// Factor index of [`Target`].
pub const FACTOR_TARGET: usize = 3;
/// Factor index of [`Quoting`].
pub const FACTOR_QUOTING: usize = 4;
/// Factor index of [`ElementRegion`].
pub const FACTOR_ELEMENT_REGION: usize = 5;
/// Factor index of [`CssSource`].
pub const FACTOR_CSS_SOURCE: usize = 6;
/// Factor index of [`StructuralKind`].
pub const FACTOR_STRUCTURAL_KIND: usize = 7;
/// Factor index of [`MatchOutcome`].
pub const FACTOR_MATCH_OUTCOME: usize = 8;

/// Declare an exhaustive model enum: dense ordinals in declaration order, a
/// complete `ALL` inventory, a stable slug-safe `id()` fragment, and the
/// ordinal round-trip used to bridge onto the covering-array partition ids.
macro_rules! model_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $id:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        $vis enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// Every variant, in declaration (= ordinal) order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant ),+ ];

            /// The stable, slug-safe identifier fragment of this level.
            #[must_use]
            pub fn id(self) -> &'static str {
                match self { $( Self::$variant => $id ),+ }
            }

            /// Dense ordinal (declaration order, starting at 0).
            #[must_use]
            pub fn ordinal(self) -> u16 {
                self as u16
            }

            /// Inverse of [`ordinal`](Self::ordinal); `None` out of range.
            #[must_use]
            pub fn from_ordinal(ordinal: u16) -> Option<Self> {
                Self::ALL.get(usize::from(ordinal)).copied()
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Generative input factors (covering factors 0..8)
// ---------------------------------------------------------------------------

model_enum! {
    /// The kind of simple selector the style rule targets (factor 0).
    pub enum SelectorKind {
        /// A class selector (`.b`).
        Class => "cls",
        /// An id selector (`#b`).
        Id => "id",
        /// An attribute selector (`[data-x="b"]` / presence `[data-x]`).
        Attribute => "attr",
        /// A type (element-name) selector (`div`).
        Type => "type",
        /// The universal selector (`*`).
        Universal => "univ",
        /// The CSS nesting selector (`&`). Valid only inside a parent rule
        /// (`.b { & { … } }`, `:global(&)`); a parentless `&` is rejected by
        /// the official compiler (`css_nesting_selector_invalid_placement`).
        Nesting => "nest",
    }
}

model_enum! {
    /// How the template attribute VALUE is spelled (factor 1) — the HTML
    /// spelling language. `Dynamic` and `Spread` are UNCERTAINTY forms: they
    /// are not static representation-equivalents and are excluded from every
    /// static [`SemanticValueFamily`].
    pub enum TemplateValueRepresentation {
        /// Literal text (`class="a b"`).
        Literal => "lit",
        /// A named character reference (`&amp;` / `&copy;`).
        HtmlNamedEntity => "named",
        /// A decimal character reference (`&#32;`).
        HtmlDecimalEntity => "dec",
        /// A hex character reference (`&#x20;`).
        HtmlHexEntity => "hex",
        /// A spelling mixing at least two distinct reference forms
        /// (`a&#38;&#x62;` — decimal plus hex).
        MixedLiteralEntity => "mixent",
        /// An expression value (`class={…}`) — an uncertainty form.
        Dynamic => "dyn",
        /// A spread attribute (`{...rest}`) — an uncertainty form.
        Spread => "spread",
    }
}

model_enum! {
    /// How the CSS selector spells the targeted value (factor 2) — the CSS
    /// escape language, fully distinct from the HTML template axis.
    pub enum SelectorValueRepresentation {
        /// A literal identifier (`.b`).
        Literal => "lit",
        /// A hex escape with its space terminator (`.a\26 b`).
        CssEscapeHex => "eschex",
        /// An identity (character) escape (`.a\&b`).
        CssEscapeChar => "eschar",
        /// Hex and identity escapes mixed (`.a\26 b\&c`).
        Mixed => "escmix",
    }
}

model_enum! {
    /// Which attribute the template authors on the subject element (factor 3).
    pub enum Target {
        /// The `class` attribute.
        Class => "cls",
        /// The `id` attribute.
        Id => "id",
        /// A data attribute (`data-x`).
        Attr => "attr",
    }
}

model_enum! {
    /// How the authored attribute is quoted (factor 4).
    pub enum Quoting {
        /// A double-quoted value (`class="a b"`).
        Quoted => "q",
        /// An unquoted value (`class=a&#32;b` — no literal whitespace).
        Unquoted => "uq",
        /// A valueless (boolean-form) attribute (`data-x`).
        Boolean => "bool",
    }
}

model_enum! {
    /// The template region hosting the subject element (factor 5).
    pub enum ElementRegion {
        /// A plain static element at the fragment root.
        StaticElement => "el",
        /// A `<svelte:element this={tag}>` dynamic element.
        SvelteElement => "dynel",
        /// A static element passed as component children (`<Child>…</Child>`).
        Component => "comp",
        /// A static element inside a control-flow block (`{#if}`).
        Block => "blk",
        /// A static element inside a `{#snippet}` rendered via `{@render}`.
        Snippet => "snip",
        /// A static element inside a `<slot>` fallback region. The slot outlet
        /// lowers block-semantically (`$.slot(node, $$props, …)` against a `<!>`
        /// anchor) and its fallback fragment projects through the
        /// selector-to-template matcher (official `SlotElement` semantics), so
        /// these cells are Supported: the fallback subject scopes/prunes exactly
        /// like its static-element twin.
        LegacySlot => "slot",
    }
}

model_enum! {
    /// Where the component's CSS lives (factor 6).
    pub enum CssSource {
        /// A regular `<style>` block emitted as external scoped CSS.
        External => "ext",
        /// `<svelte:options css="injected" />` — CSS injected into the module.
        Injected => "inj",
    }
}

model_enum! {
    /// The structural shape of the style rule set (factor 7).
    pub enum StructuralKind {
        /// A single plain rule.
        Plain => "plain",
        /// The subject rule plus an always-unused rule that must prune.
        Pruning => "prune",
        /// The subject selector wrapped in `:global(…)` (never pruned).
        Global => "glob",
        /// A nested rule (`SEL { &:hover { … } }`; for [`SelectorKind::Nesting`]
        /// the parent carries the target-reading selector: `.b { & { … } }`).
        Nested => "nest",
        /// A descendant combinator (`.wrap SEL`) with a wrapper element.
        Combinator => "comb",
    }
}

model_enum! {
    /// The DECLARED-expected match verdict the fixture is constructed to
    /// yield (factor 8). Differential and metamorphic suites OBSERVE the
    /// actual verdict and compare against this declaration.
    pub enum MatchOutcome {
        /// The selector certainly matches the subject (kept + scoped).
        Match => "m",
        /// The selector certainly cannot match (pruned as unused).
        NoMatch => "n",
        /// The matcher cannot statically decide (kept fail-open).
        Maybe => "u",
    }
}

// ---------------------------------------------------------------------------
// Execution axis (NOT a covering factor)
// ---------------------------------------------------------------------------

model_enum! {
    /// The compile backend axis: every selected source row expands to BOTH
    /// backends; the axis never participates in the covering design.
    pub enum CompileTarget {
        /// `generate: 'client'`.
        Client => "client",
        /// `generate: 'server'`.
        Server => "server",
    }
}

// ---------------------------------------------------------------------------
// Classification enums
// ---------------------------------------------------------------------------

/// Verter-declared refusal cells: officially-compilable fixtures whose
/// faithful scoped emission Verter fails closed on today. Currently
/// UNINHABITED — every officially-compilable covering cell is Supported (the
/// `<slot>` fallback region now lowers + scopes through the shared matcher) —
/// the typed rail is retained so a future refusal lands as a variant plus its
/// observation arm, never a parallel hand-authored list.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RefusalKind {}

impl RefusalKind {
    /// Every variant, in declaration (= ordinal) order.
    pub const ALL: &'static [Self] = &[];

    /// The stable, slug-safe identifier fragment of this level.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {}
    }

    /// Dense ordinal (declaration order, starting at 0).
    #[must_use]
    pub fn ordinal(self) -> u16 {
        match self {}
    }

    /// Inverse of [`ordinal`](Self::ordinal); `None` out of range.
    #[must_use]
    pub fn from_ordinal(_ordinal: u16) -> Option<Self> {
        None
    }
}

model_enum! {
    /// Official-oracle-rejected cells: the pinned `svelte@5.56.3` compiler
    /// rejects the rendered fixture with the named diagnostic.
    pub enum DiagnosticKind {
        /// A parentless nesting selector (`& { … }` / `.wrap & { … }` at the
        /// stylesheet top level) — `css_nesting_selector_invalid_placement`.
        CssNestingSelectorInvalidPlacement => "css-nesting-selector-invalid-placement",
    }
}

model_enum! {
    /// Structurally impossible level combinations: no fixture can be
    /// authored that coherently exhibits the combination.
    pub enum ConstraintKind {
        /// Boolean (valueless) quoting on a value-matching target
        /// (`class` / `id`): there is no value for the selector to read.
        BooleanQuotingOnValuedTarget => "boolean-quoting-on-valued-target",
        /// A valueless attribute cannot carry a value representation other
        /// than the degenerate literal spelling.
        BooleanQuotingCarriesNoValue => "boolean-quoting-carries-no-value",
        /// A spread attribute has no authored attribute text, so no authored
        /// quoting; the `Quoted` level is the canonical carrier.
        SpreadCarriesNoQuoting => "spread-carries-no-quoting",
        /// CSS escape spellings on a selector with no value string (the
        /// universal / type / nesting selectors and the presence-only
        /// attribute selector): the escapable characters cannot occur.
        SelectorEscapeOnValuelessSelector => "selector-escape-on-valueless-selector",
        /// `*` matches every element; a NoMatch / Maybe declaration is
        /// incoherent.
        UniversalSelectorAlwaysMatches => "universal-selector-always-matches",
        /// A `:global(…)` selector is never pruned and never uncertain; only
        /// Match is declarable.
        GlobalSelectorNeverPrunes => "global-selector-never-prunes",
        /// A Match declaration where the selector kind does not read the
        /// authored target attribute (e.g. a class selector against an
        /// id-only subject).
        SelectorCannotReadTarget => "selector-cannot-read-target",
        /// A Match declaration through an ESCAPED attribute-selector value:
        /// the official matcher compares attribute-selector value text RAW
        /// (`[data-x="a\26 b"]` and `[data-x=a\26 b]` never equal the decoded
        /// attribute value `a&b`), so an escape spelling can never match.
        AttrSelectorValueEscapeNeverMatches => "attr-selector-value-escape-never-matches",
        /// A Maybe declaration with no uncertainty source (fully static
        /// value on a statically named element).
        MaybeNeedsUncertainSource => "maybe-needs-uncertain-source",
        /// A spread-valued subject makes every attribute-reading selector
        /// verdict uncertain; Match / NoMatch are not declarable.
        SpreadOutcomeAlwaysUncertain => "spread-outcome-always-uncertain",
        /// A type selector against `<svelte:element this={…}>` is always
        /// uncertain; Match / NoMatch are not declarable.
        SvelteElementTagUncertain => "svelte-element-tag-uncertain",
    }
}

/// The typed disposition of a row, produced solely by [`classify`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Disposition {
    /// A Supported conformance cell: rendered, golden-compared, matcher-fact
    /// checked.
    Supported,
    /// A Verter-declared refusal cell (officially compilable).
    Refused(RefusalKind),
    /// An official-oracle-rejected cell.
    OracleRejected(DiagnosticKind),
    /// A structurally impossible combination (never rendered, never a case).
    Invalid(ConstraintKind),
}

impl Disposition {
    /// Bridge onto the covering-array partition: the opaque engine ids ARE
    /// the model enums' dense ordinals, 1:1 in both directions.
    #[must_use]
    pub fn partition(self) -> covering_array::Partition {
        match self {
            Disposition::Supported => covering_array::Partition::Supported,
            Disposition::Refused(kind) => {
                covering_array::Partition::Refused(covering_array::RefusalKind(kind.ordinal()))
            }
            Disposition::OracleRejected(kind) => covering_array::Partition::OracleRejected(
                covering_array::DiagnosticKind(kind.ordinal()),
            ),
            Disposition::Invalid(_) => covering_array::Partition::Invalid,
        }
    }
}

impl TemplateValueRepresentation {
    /// Whether this level is an UNCERTAINTY form (`Dynamic` / `Spread`) —
    /// excluded from every static equivalence family.
    #[must_use]
    pub fn is_uncertainty_form(self) -> bool {
        matches!(self, Self::Dynamic | Self::Spread)
    }
}

// ---------------------------------------------------------------------------
// Decoded row
// ---------------------------------------------------------------------------

/// A fully decoded row: one typed level per factor.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RowLevels {
    /// Factor 0.
    pub selector_kind: SelectorKind,
    /// Factor 1.
    pub template_value: TemplateValueRepresentation,
    /// Factor 2.
    pub selector_value: SelectorValueRepresentation,
    /// Factor 3.
    pub target: Target,
    /// Factor 4.
    pub quoting: Quoting,
    /// Factor 5.
    pub region: ElementRegion,
    /// Factor 6.
    pub css_source: CssSource,
    /// Factor 7.
    pub structural: StructuralKind,
    /// Factor 8.
    pub outcome: MatchOutcome,
}

impl RowLevels {
    /// Decode an ordinal row; `None` when any level is out of range.
    #[must_use]
    pub fn decode(row: Row<FACTOR_COUNT>) -> Option<Self> {
        Some(RowLevels {
            selector_kind: SelectorKind::from_ordinal(row.0[FACTOR_SELECTOR_KIND])?,
            template_value: TemplateValueRepresentation::from_ordinal(
                row.0[FACTOR_TEMPLATE_VALUE],
            )?,
            selector_value: SelectorValueRepresentation::from_ordinal(
                row.0[FACTOR_SELECTOR_VALUE],
            )?,
            target: Target::from_ordinal(row.0[FACTOR_TARGET])?,
            quoting: Quoting::from_ordinal(row.0[FACTOR_QUOTING])?,
            region: ElementRegion::from_ordinal(row.0[FACTOR_ELEMENT_REGION])?,
            css_source: CssSource::from_ordinal(row.0[FACTOR_CSS_SOURCE])?,
            structural: StructuralKind::from_ordinal(row.0[FACTOR_STRUCTURAL_KIND])?,
            outcome: MatchOutcome::from_ordinal(row.0[FACTOR_MATCH_OUTCOME])?,
        })
    }

    /// Inverse of [`decode`](Self::decode).
    #[must_use]
    pub fn encode(&self) -> Row<FACTOR_COUNT> {
        Row([
            self.selector_kind.ordinal(),
            self.template_value.ordinal(),
            self.selector_value.ordinal(),
            self.target.ordinal(),
            self.quoting.ordinal(),
            self.region.ordinal(),
            self.css_source.ordinal(),
            self.structural.ordinal(),
            self.outcome.ordinal(),
        ])
    }
}

/// The per-factor level counts, in factor-index order.
#[must_use]
pub fn factor_cardinalities() -> [u16; FACTOR_COUNT] {
    [
        SelectorKind::ALL.len() as u16,
        TemplateValueRepresentation::ALL.len() as u16,
        SelectorValueRepresentation::ALL.len() as u16,
        Target::ALL.len() as u16,
        Quoting::ALL.len() as u16,
        ElementRegion::ALL.len() as u16,
        CssSource::ALL.len() as u16,
        StructuralKind::ALL.len() as u16,
        MatchOutcome::ALL.len() as u16,
    ]
}

// ---------------------------------------------------------------------------
// Classification (typed constraint functions)
// ---------------------------------------------------------------------------

/// Classify a decoded row into its [`Disposition`]. The SOLE constraint
/// authority; rule order is first-match-wins:
///
/// 1. carrier-existence constraints (the fixture cannot even be authored),
/// 2. official oracle rejects (authored but rejected before matching),
/// 3. declared-outcome coherence constraints (authored and compiled, but the
///    declared verdict is officially incoherent),
/// 4. Verter-declared refusals (officially coherent, Verter fails closed),
/// 5. Supported.
#[must_use]
pub fn classify(levels: &RowLevels) -> Disposition {
    let kind = levels.selector_kind;
    let template = levels.template_value;
    let selector = levels.selector_value;
    let target = levels.target;
    let quoting = levels.quoting;
    let region = levels.region;
    let structural = levels.structural;
    let outcome = levels.outcome;

    // 1. Carrier existence.
    if quoting == Quoting::Boolean && target != Target::Attr {
        return Disposition::Invalid(ConstraintKind::BooleanQuotingOnValuedTarget);
    }
    if quoting == Quoting::Boolean && template != TemplateValueRepresentation::Literal {
        return Disposition::Invalid(ConstraintKind::BooleanQuotingCarriesNoValue);
    }
    if template == TemplateValueRepresentation::Spread && quoting != Quoting::Quoted {
        return Disposition::Invalid(ConstraintKind::SpreadCarriesNoQuoting);
    }
    let selector_is_valueless = matches!(
        kind,
        SelectorKind::Type | SelectorKind::Universal | SelectorKind::Nesting
    ) || (kind == SelectorKind::Attribute
        && quoting == Quoting::Boolean);
    if selector != SelectorValueRepresentation::Literal && selector_is_valueless {
        return Disposition::Invalid(ConstraintKind::SelectorEscapeOnValuelessSelector);
    }

    // 2. Official oracle rejects: a nesting selector outside a parent rule
    // (`& { … }` / `.wrap & { … }` at the stylesheet top level) — the pinned
    // compiler rejects with `css_nesting_selector_invalid_placement`.
    if kind == SelectorKind::Nesting
        && matches!(
            structural,
            StructuralKind::Plain | StructuralKind::Pruning | StructuralKind::Combinator
        )
    {
        return Disposition::OracleRejected(DiagnosticKind::CssNestingSelectorInvalidPlacement);
    }

    // 3. Declared-outcome coherence.
    if kind == SelectorKind::Universal && outcome != MatchOutcome::Match {
        return Disposition::Invalid(ConstraintKind::UniversalSelectorAlwaysMatches);
    }
    if structural == StructuralKind::Global && outcome != MatchOutcome::Match {
        return Disposition::Invalid(ConstraintKind::GlobalSelectorNeverPrunes);
    }
    // Whether the selector kind reads the authored target attribute (the
    // nesting selector reads it through its parent, which adapts per target;
    // type / universal selectors read the element, not the attribute).
    let reads_target = match kind {
        SelectorKind::Class => target == Target::Class,
        SelectorKind::Id => target == Target::Id,
        SelectorKind::Attribute => target == Target::Attr,
        SelectorKind::Type | SelectorKind::Universal | SelectorKind::Nesting => true,
    };
    if outcome == MatchOutcome::Match && !reads_target {
        return Disposition::Invalid(ConstraintKind::SelectorCannotReadTarget);
    }
    if kind == SelectorKind::Attribute
        && selector != SelectorValueRepresentation::Literal
        && outcome == MatchOutcome::Match
    {
        return Disposition::Invalid(ConstraintKind::AttrSelectorValueEscapeNeverMatches);
    }
    let selector_reads_attributes = matches!(
        kind,
        SelectorKind::Class | SelectorKind::Id | SelectorKind::Attribute | SelectorKind::Nesting
    );
    if template == TemplateValueRepresentation::Spread
        && selector_reads_attributes
        && outcome != MatchOutcome::Maybe
    {
        return Disposition::Invalid(ConstraintKind::SpreadOutcomeAlwaysUncertain);
    }
    if region == ElementRegion::SvelteElement
        && kind == SelectorKind::Type
        && outcome != MatchOutcome::Maybe
    {
        return Disposition::Invalid(ConstraintKind::SvelteElementTagUncertain);
    }
    let value_uncertain = match template {
        TemplateValueRepresentation::Spread => selector_reads_attributes,
        TemplateValueRepresentation::Dynamic => reads_target && selector_reads_attributes,
        _ => false,
    };
    let element_uncertain = region == ElementRegion::SvelteElement && kind == SelectorKind::Type;
    if outcome == MatchOutcome::Maybe && !(value_uncertain || element_uncertain) {
        return Disposition::Invalid(ConstraintKind::MaybeNeedsUncertainSource);
    }

    // 4. Everything else is a Supported conformance cell (the `<slot>`
    // fallback region included — it classifies identically to its
    // static-element twin).
    Disposition::Supported
}

// ---------------------------------------------------------------------------
// Compile options
// ---------------------------------------------------------------------------

/// Typed per-case compile options, serialized to the JSON object the Node
/// generator spreads into `compiler.compile(source, { …, ...options })`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ManifestCompileOptions {
    /// `customElement: true`.
    pub custom_element: bool,
    /// `filename: null` (the JS side maps it to `undefined`, exercising the
    /// css-hash filename fallback).
    pub filename_undefined: bool,
    /// `fragments: 'tree'` — the CSP-safe `$.from_tree` objectified-clone
    /// factory. The flip changes ONLY the root FACTORY (`$.from_tree` vs the
    /// html-string `$.from_html`): the css scope token itself is baked by the
    /// SHARED static-attribute authority (`template_serialize.rs`) read by both
    /// the html-string serializer and the `$.from_tree` objectifier, so the
    /// token is byte-identical across the two modes — and for this spread cell it
    /// rides the SAME `$.attribute_effect(…, 'svelte-<hash>')`
    /// (`HashStringArgument`) carrier in html AND tree. The oracle differential's
    /// scope-token axes are therefore scope-token-identical across the flip and
    /// do NOT themselves observe it; see [`compile_options`] for how the tree
    /// axis is actually pinned.
    pub fragments_tree: bool,
}

impl ManifestCompileOptions {
    /// Deterministic JSON object rendering (stable field order; `{}` when
    /// every option is default).
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields: Vec<String> = Vec::new();
        if self.custom_element {
            fields.push("\"customElement\":true".to_string());
        }
        if self.filename_undefined {
            fields.push("\"filename\":null".to_string());
        }
        if self.fragments_tree {
            fields.push("\"fragments\":\"tree\"".to_string());
        }
        format!("{{{}}}", fields.join(","))
    }
}

/// The compile options of a row. Default for every row EXCEPT the scoped-CSS
/// tree cell: a supported scoped static-element cell (an external scoped
/// `<style>`, a plain type selector that certainly matches on the class target)
/// is compiled under `fragments: 'tree'`, so the corpus exercises the tree-mode
/// `$.from_tree` root factory alongside the html-string `$.from_html` factory
/// every other scoped cell covers. The flip is orthogonal to classification (it
/// never changes a row's disposition / outcome), so it shifts only that cell's
/// compiled carrier + golden. Selects exactly one row.
///
/// Why the oracle differential cannot discriminate this flip (the scope-token
/// orthogonality contract, NOT a coverage hole): the css scope token is baked by
/// the SHARED static-attribute authority (`template_serialize.rs`) read by both
/// the html-string serializer and the `$.from_tree` objectifier, so the token is
/// byte-identical in html and tree — and for this spread cell it rides the same
/// `$.attribute_effect(…, css_hash)` (`HashStringArgument`) carrier in both. The
/// oracle differential (`oracle_differential.rs`) compares ONLY scope-token
/// occurrences, never the outer factory, so its `ScopeTokenDelivery` /
/// `ScopedClassTopology` axes produce identical signatures for html and tree —
/// forcing Verter to emit `$.from_html` would leave the differential green. The
/// tree flip is instead pinned by TWO other rails: (1) this manifest-cell
/// assertion (`compile_options_serialize_deterministically`), and (2) the
/// committed `$.from_tree` golden regenerated + checked under `--conformance`.
/// Verter's OWN `$.from_html`-vs-`$.from_tree` root-factory discrimination is
/// covered by the separate `svelte_client_emit_topology.rs` corpus over the
/// `options/fragments_tree_*` fixtures.
#[must_use]
pub fn compile_options(levels: &RowLevels) -> ManifestCompileOptions {
    let is_scoped_static_tree_cell = levels.region == ElementRegion::StaticElement
        && levels.css_source == CssSource::External
        && levels.structural == StructuralKind::Plain
        && levels.selector_kind == SelectorKind::Type
        && levels.outcome == MatchOutcome::Match
        && levels.target == Target::Class;
    ManifestCompileOptions {
        fragments_tree: is_scoped_static_tree_cell,
        ..ManifestCompileOptions::default()
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render a row's `.svelte` fixture source.
///
/// # Panics
///
/// Panics when the row classifies [`Disposition::Invalid`] — invalid rows are
/// structurally unconstructible and never become manifest cases.
#[must_use]
pub fn render_fixture(levels: &RowLevels) -> String {
    assert!(
        !matches!(classify(levels), Disposition::Invalid(_)),
        "render_fixture: {levels:?} is structurally unconstructible (Invalid rows never render)"
    );

    let mut source = String::new();
    if levels.css_source == CssSource::Injected {
        source.push_str("<svelte:options css=\"injected\" />\n\n");
    }
    source.push_str(&script_block(levels));
    source.push_str(&markup(levels));
    source.push_str("\n\n<style>\n");
    source.push_str(&style_rules(levels));
    source.push_str("\n</style>\n");
    source
}

/// The stable, cross-platform-safe slug of a row: the nine level id
/// fragments joined by `-`, in factor order.
#[must_use]
pub fn slug(levels: &RowLevels) -> String {
    [
        levels.selector_kind.id(),
        levels.template_value.id(),
        levels.selector_value.id(),
        levels.target.id(),
        levels.quoting.id(),
        levels.region.id(),
        levels.css_source.id(),
        levels.structural.id(),
        levels.outcome.id(),
    ]
    .join("-")
}

// ---------------------------------------------------------------------------
// Rendering internals — the value scheme
//
// The semantic subject value S is chosen so BOTH representation languages can
// spell it: the selector axis fixes S (a plain ident for literal selectors,
// an ampersand-carrying ident for escaped selectors), and the template axis
// then spells S — or, for the corpus-canonical class cells, the token
// SEPARATOR — in its own language. Every (spelling, verdict) pair below was
// grounded against the pinned official compiler.
// ---------------------------------------------------------------------------

impl Target {
    /// The authored attribute name of this target.
    fn attr_name(self) -> &'static str {
        match self {
            Target::Class => "class",
            Target::Id => "id",
            Target::Attr => "data-x",
        }
    }
}

/// The semantic subject value the selector targets on Match / Maybe rows.
fn subject_value(
    selector: SelectorValueRepresentation,
    template: TemplateValueRepresentation,
) -> &'static str {
    match selector {
        SelectorValueRepresentation::Literal => match template {
            // `©` (U+A9) is CSS-ident-legal literally and has a named
            // reference — the one distinguished char both literal-selector
            // and named-entity spellings can carry.
            TemplateValueRepresentation::HtmlNamedEntity => "a\u{a9}b",
            // Two spellable positions so decimal + hex forms can mix.
            TemplateValueRepresentation::MixedLiteralEntity => "bb",
            _ => "b",
        },
        SelectorValueRepresentation::CssEscapeHex | SelectorValueRepresentation::CssEscapeChar => {
            "a&b"
        }
        SelectorValueRepresentation::Mixed => "a&b&c",
    }
}

/// The template-language spelling of the STATIC attribute value (never
/// called for the uncertainty forms).
fn spelled_template_value(levels: &RowLevels) -> String {
    let selector = levels.selector_value;
    let template = levels.template_value;

    // The corpus-canonical class SEPARATOR cells: the entity spells the
    // token separator itself (`class="a&#32;b"` tokenizes to `a`,`b` —
    // unquoted-legal too, since the value carries no literal whitespace).
    if levels.target == Target::Class && selector == SelectorValueRepresentation::Literal {
        match template {
            TemplateValueRepresentation::HtmlDecimalEntity => return "a&#32;b".to_string(),
            TemplateValueRepresentation::HtmlHexEntity => return "a&#x20;b".to_string(),
            _ => {}
        }
    }

    let spelled: &str = match selector {
        SelectorValueRepresentation::Literal => match template {
            TemplateValueRepresentation::Literal => "b",
            TemplateValueRepresentation::HtmlNamedEntity => "a&copy;b",
            TemplateValueRepresentation::HtmlDecimalEntity => "&#98;",
            TemplateValueRepresentation::HtmlHexEntity => "&#x62;",
            TemplateValueRepresentation::MixedLiteralEntity => "&#98;&#x62;",
            TemplateValueRepresentation::Dynamic | TemplateValueRepresentation::Spread => {
                unreachable!("uncertainty forms have no static spelling")
            }
        },
        SelectorValueRepresentation::CssEscapeHex | SelectorValueRepresentation::CssEscapeChar => {
            match template {
                TemplateValueRepresentation::Literal => "a&b",
                TemplateValueRepresentation::HtmlNamedEntity => "a&amp;b",
                TemplateValueRepresentation::HtmlDecimalEntity => "a&#38;b",
                TemplateValueRepresentation::HtmlHexEntity => "a&#x26;b",
                TemplateValueRepresentation::MixedLiteralEntity => "a&#38;&#x62;",
                TemplateValueRepresentation::Dynamic | TemplateValueRepresentation::Spread => {
                    unreachable!("uncertainty forms have no static spelling")
                }
            }
        }
        SelectorValueRepresentation::Mixed => match template {
            TemplateValueRepresentation::Literal => "a&b&c",
            TemplateValueRepresentation::HtmlNamedEntity => "a&amp;b&amp;c",
            TemplateValueRepresentation::HtmlDecimalEntity => "a&#38;b&#38;c",
            TemplateValueRepresentation::HtmlHexEntity => "a&#x26;b&#x26;c",
            TemplateValueRepresentation::MixedLiteralEntity => "a&amp;b&#38;c",
            TemplateValueRepresentation::Dynamic | TemplateValueRepresentation::Spread => {
                unreachable!("uncertainty forms have no static spelling")
            }
        },
    };
    // A class attribute prefixes a bystander token when quoting permits a
    // literal separator; unquoted class values stay a single token.
    if levels.target == Target::Class && levels.quoting == Quoting::Quoted {
        format!("a {spelled}")
    } else {
        spelled.to_string()
    }
}

/// The expression text of a `Dynamic` value. Certain outcomes use a
/// conditional whose branches BOTH resolve to the subject value — one spelled
/// through the JS-string escape language (the third representation language,
/// distinct from HTML entities and CSS escapes).
fn dynamic_expression(levels: &RowLevels) -> String {
    if levels.outcome == MatchOutcome::Maybe {
        return "value".to_string();
    }
    let value = subject_value(levels.selector_value, TemplateValueRepresentation::Dynamic);
    let (plain, escaped) = match value {
        "b" => ("b", "'\\u0062'"),
        "a&b" => ("a&b", "'a\\u0026b'"),
        "a&b&c" => ("a&b&c", "'a\\u0026b\\u0026c'"),
        _ => unreachable!("dynamic subject values are drawn from the closed table"),
    };
    if levels.target == Target::Class {
        format!("flag ? 'a {plain}' : {escaped}")
    } else {
        format!("flag ? '{plain}' : {escaped}")
    }
}

/// The authored subject attribute (`class="a b"` / `data-x` / `{...rest}` /
/// `class={…}`), complete with its quoting form.
fn subject_attr(levels: &RowLevels) -> String {
    if levels.quoting == Quoting::Boolean {
        return "data-x".to_string();
    }
    match levels.template_value {
        TemplateValueRepresentation::Spread => "{...rest}".to_string(),
        TemplateValueRepresentation::Dynamic => {
            let name = levels.target.attr_name();
            let expr = dynamic_expression(levels);
            if levels.quoting == Quoting::Quoted {
                format!("{name}=\"{{{expr}}}\"")
            } else {
                format!("{name}={{{expr}}}")
            }
        }
        _ => {
            let name = levels.target.attr_name();
            let value = spelled_template_value(levels);
            if levels.quoting == Quoting::Quoted {
                format!("{name}=\"{value}\"")
            } else {
                format!("{name}={value}")
            }
        }
    }
}

/// The `<script>` block (empty when the row needs no bindings).
fn script_block(levels: &RowLevels) -> String {
    let mut lines: Vec<&'static str> = Vec::new();
    if levels.region == ElementRegion::Component {
        lines.push("import Child from './Child.svelte';");
    }
    if levels.region == ElementRegion::SvelteElement {
        lines.push("let tag = $state('div');");
    }
    if levels.region == ElementRegion::Block {
        lines.push("let open = $state(true);");
    }
    match levels.template_value {
        TemplateValueRepresentation::Dynamic => {
            lines.push(if levels.outcome == MatchOutcome::Maybe {
                "let { value } = $props();"
            } else {
                "let { flag } = $props();"
            })
        }
        TemplateValueRepresentation::Spread => lines.push("let { rest } = $props();"),
        _ => {}
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut block = String::from("<script>\n");
    for line in lines {
        block.push('\t');
        block.push_str(line);
        block.push('\n');
    }
    block.push_str("</script>\n\n");
    block
}

/// The template markup: the subject element (wrapped for the combinator
/// structure) hosted in its region.
fn markup(levels: &RowLevels) -> String {
    let attr = subject_attr(levels);
    let subject = if levels.region == ElementRegion::SvelteElement {
        format!("<svelte:element this={{tag}} {attr}>x</svelte:element>")
    } else {
        format!("<div {attr}>x</div>")
    };
    let subject = if levels.structural == StructuralKind::Combinator {
        format!("<div class=\"wrap\">\n\t{subject}\n</div>")
    } else {
        subject
    };
    match levels.region {
        ElementRegion::StaticElement | ElementRegion::SvelteElement => subject,
        ElementRegion::Component => format!("<Child>\n\t{subject}\n</Child>"),
        ElementRegion::Block => format!("{{#if open}}\n\t{subject}\n{{/if}}"),
        ElementRegion::Snippet => {
            format!("{{#snippet subject()}}\n\t{subject}\n{{/snippet}}\n\n{{@render subject()}}")
        }
        ElementRegion::LegacySlot => format!("<slot>\n\t{subject}\n</slot>"),
    }
}

/// The CSS-language spelling of the value carried by a value-reading
/// selector (`.b` / `.a\26 b` / `.\7a z` …).
fn spelled_selector_value(levels: &RowLevels) -> String {
    if levels.outcome == MatchOutcome::NoMatch {
        // The never-present value `zz`, spelled per representation.
        return match levels.selector_value {
            SelectorValueRepresentation::Literal => "zz",
            SelectorValueRepresentation::CssEscapeHex => "\\7a z",
            SelectorValueRepresentation::CssEscapeChar => "\\zz",
            SelectorValueRepresentation::Mixed => "\\7a \\z",
        }
        .to_string();
    }
    match levels.selector_value {
        SelectorValueRepresentation::Literal => {
            subject_value(SelectorValueRepresentation::Literal, levels.template_value).to_string()
        }
        SelectorValueRepresentation::CssEscapeHex => "a\\26 b".to_string(),
        SelectorValueRepresentation::CssEscapeChar => "a\\&b".to_string(),
        SelectorValueRepresentation::Mixed => "a\\26 b\\&c".to_string(),
    }
}

/// The subject selector text (no structural composition applied).
fn subject_selector(levels: &RowLevels) -> String {
    match levels.selector_kind {
        SelectorKind::Universal => "*".to_string(),
        SelectorKind::Type => if levels.outcome == MatchOutcome::NoMatch {
            "p"
        } else {
            "div"
        }
        .to_string(),
        SelectorKind::Nesting => "&".to_string(),
        SelectorKind::Class => format!(".{}", spelled_selector_value(levels)),
        SelectorKind::Id => format!("#{}", spelled_selector_value(levels)),
        SelectorKind::Attribute => {
            if levels.quoting == Quoting::Boolean {
                if levels.outcome == MatchOutcome::NoMatch {
                    "[data-zz]".to_string()
                } else {
                    "[data-x]".to_string()
                }
            } else {
                format!("[data-x=\"{}\"]", spelled_selector_value(levels))
            }
        }
    }
}

/// The parent rule selector a nesting-selector subject nests under: it reads
/// the target attribute with the canonical literal spelling.
fn nesting_parent_selector(levels: &RowLevels) -> String {
    let value = if levels.outcome == MatchOutcome::NoMatch {
        "zz"
    } else {
        subject_value(SelectorValueRepresentation::Literal, levels.template_value)
    };
    match levels.target {
        Target::Class => format!(".{value}"),
        Target::Id => format!("#{value}"),
        Target::Attr => {
            if levels.quoting == Quoting::Boolean {
                if levels.outcome == MatchOutcome::NoMatch {
                    "[data-zz]".to_string()
                } else {
                    "[data-x]".to_string()
                }
            } else {
                format!("[data-x=\"{value}\"]")
            }
        }
    }
}

/// The `<style>` rule set, composed per structural kind.
fn style_rules(levels: &RowLevels) -> String {
    let rule = |selector: &str| format!("\t{selector} {{\n\t\tcolor: red;\n\t}}");
    let selector = subject_selector(levels);
    match levels.structural {
        StructuralKind::Plain => rule(&selector),
        StructuralKind::Pruning => format!(
            "{}\n\n\t.unused-prune {{\n\t\tcolor: blue;\n\t}}",
            rule(&selector)
        ),
        StructuralKind::Global => rule(&format!(":global({selector})")),
        StructuralKind::Nested => {
            if levels.selector_kind == SelectorKind::Nesting {
                let parent = nesting_parent_selector(levels);
                format!("\t{parent} {{\n\t\t& {{\n\t\t\tcolor: red;\n\t\t}}\n\t}}")
            } else {
                format!(
                    "\t{selector} {{\n\t\tcolor: red;\n\n\t\t&:hover {{\n\t\t\tcolor: blue;\n\t\t}}\n\t}}"
                )
            }
        }
        StructuralKind::Combinator => rule(&format!(".wrap {selector}")),
    }
}

// ---------------------------------------------------------------------------
// Semantic value families (metamorphic grounding data)
// ---------------------------------------------------------------------------

model_enum! {
    /// The representation KIND of one family rendering. Kinds are language-
    /// typed (HTML entity forms, CSS escape forms, JS string forms) and never
    /// merged; there is deliberately NO uncertainty-form kind.
    pub enum RenderingKind {
        /// Literal HTML template text.
        TemplateLiteral => "template-literal",
        /// An HTML named character reference.
        HtmlNamedEntity => "html-named-entity",
        /// An HTML decimal character reference.
        HtmlDecimalEntity => "html-decimal-entity",
        /// An HTML hex character reference.
        HtmlHexEntity => "html-hex-entity",
        /// A CSS hex escape (`\26 `).
        CssEscapeHex => "css-escape-hex",
        /// A CSS identity escape (`\&`).
        CssEscapeChar => "css-escape-char",
        /// A literal JS string in an expression value.
        JsStringLiteral => "js-string-literal",
        /// A JS string spelled with `\u` escapes.
        JsStringEscape => "js-string-escape",
    }
}

impl RenderingKind {
    /// The template-axis representation this kind exercises, when it is a
    /// template-language kind (`None` for the CSS and JS languages). Never an
    /// uncertainty form by construction.
    #[must_use]
    pub fn template_representation(self) -> Option<TemplateValueRepresentation> {
        match self {
            RenderingKind::TemplateLiteral => Some(TemplateValueRepresentation::Literal),
            RenderingKind::HtmlNamedEntity => Some(TemplateValueRepresentation::HtmlNamedEntity),
            RenderingKind::HtmlDecimalEntity => {
                Some(TemplateValueRepresentation::HtmlDecimalEntity)
            }
            RenderingKind::HtmlHexEntity => Some(TemplateValueRepresentation::HtmlHexEntity),
            RenderingKind::CssEscapeHex
            | RenderingKind::CssEscapeChar
            | RenderingKind::JsStringLiteral
            | RenderingKind::JsStringEscape => None,
        }
    }
}

/// One concrete rendering of a family's base value, typed by language kind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FamilyRendering {
    /// The representation language of this rendering.
    pub kind: RenderingKind,
    /// The rendered spelling.
    pub rendered: &'static str,
}

/// The grounded expected verdict of a family: a concrete selector plus the
/// verdict it MUST produce against every rendering of the family.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GroundedVerdict {
    /// The CSS selector the verdict is grounded on.
    pub selector: &'static str,
    /// The expected verdict.
    pub outcome: MatchOutcome,
}

/// A set of representation-equivalent VALUES that MUST produce identical
/// matcher facts, with a grounded expected verdict (so two identically-wrong
/// implementations cannot pass by mutual equality alone). Uncertainty forms
/// (`Dynamic` / `Spread`) are never family members.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SemanticValueFamily {
    /// Stable family name.
    pub name: &'static str,
    /// The shared decoded semantic value.
    pub base_value: &'static str,
    /// The equivalent renderings (≥ 2, of distinct representation kinds).
    pub renderings: Vec<FamilyRendering>,
    /// The grounded expected verdict.
    pub verdict: GroundedVerdict,
}

/// The static equivalence families the metamorphic suites execute. Every
/// grounded verdict below was verified against the pinned official compiler
/// (the selector stays kept + scoped for Match members).
#[must_use]
pub fn semantic_value_families() -> Vec<SemanticValueFamily> {
    vec![
        // `class="a b"` ≡ `class="a&#32;b"` ≡ `class="a&#x20;b"`: the space
        // separator spelled literally / as a decimal reference / as a hex
        // reference — identical token facts (`a`, `b`); `.b` matches.
        SemanticValueFamily {
            name: "class-token-space-separator",
            base_value: "a b",
            renderings: vec![
                FamilyRendering {
                    kind: RenderingKind::TemplateLiteral,
                    rendered: "a b",
                },
                FamilyRendering {
                    kind: RenderingKind::HtmlDecimalEntity,
                    rendered: "a&#32;b",
                },
                FamilyRendering {
                    kind: RenderingKind::HtmlHexEntity,
                    rendered: "a&#x20;b",
                },
            ],
            verdict: GroundedVerdict {
                selector: ".b",
                outcome: MatchOutcome::Match,
            },
        },
        // The ampersand-carrying token across all four static HTML spellings:
        // one decoded value `a&b`, matched by the escaped selector.
        SemanticValueFamily {
            name: "ampersand-class-token",
            base_value: "a&b",
            renderings: vec![
                FamilyRendering {
                    kind: RenderingKind::TemplateLiteral,
                    rendered: "a&b",
                },
                FamilyRendering {
                    kind: RenderingKind::HtmlNamedEntity,
                    rendered: "a&amp;b",
                },
                FamilyRendering {
                    kind: RenderingKind::HtmlDecimalEntity,
                    rendered: "a&#38;b",
                },
                FamilyRendering {
                    kind: RenderingKind::HtmlHexEntity,
                    rendered: "a&#x26;b",
                },
            ],
            verdict: GroundedVerdict {
                selector: ".a\\26 b",
                outcome: MatchOutcome::Match,
            },
        },
        // The CSS-language spellings of the SAME selector value: a hex escape
        // and an identity escape decode to one selector ident `a&b`.
        SemanticValueFamily {
            name: "css-escape-spellings",
            base_value: "a&b",
            renderings: vec![
                FamilyRendering {
                    kind: RenderingKind::CssEscapeHex,
                    rendered: "a\\26 b",
                },
                FamilyRendering {
                    kind: RenderingKind::CssEscapeChar,
                    rendered: "a\\&b",
                },
            ],
            verdict: GroundedVerdict {
                selector: ".a\\26 b",
                outcome: MatchOutcome::Match,
            },
        },
        // The JS-string language: a literal space and its ` ` escape
        // produce one expression value `a b` — identical enumerated facts.
        SemanticValueFamily {
            name: "js-string-escapes",
            base_value: "a b",
            renderings: vec![
                FamilyRendering {
                    kind: RenderingKind::JsStringLiteral,
                    rendered: "'a b'",
                },
                FamilyRendering {
                    kind: RenderingKind::JsStringEscape,
                    rendered: "'a\\u0020b'",
                },
            ],
            verdict: GroundedVerdict {
                selector: ".b",
                outcome: MatchOutcome::Match,
            },
        },
    ]
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
