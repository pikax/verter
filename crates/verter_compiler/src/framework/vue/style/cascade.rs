//! The style cascade: edit planning and merging over one parsed sheet, the
//! rewrite facts and outcomes every stage reports.

use crate::code_transform::advance_generated_position;
use crate::framework::vue::style::css_vars::{
    plan_authored_v_bind_edits, relocate_edits, VBindVar,
};
use crate::framework::vue::style::modules::module_classes_and_edits_from_ir;
use crate::framework::vue::style::scoped::scoped_edits_and_facts_from_ir;
use crate::framework_common::RuntimeOutputDescriptor;
use crate::style_planner::{
    authored_or_parsed_ir, build_transform_output, observe_style_ir, record_input_dependencies,
    AuthoredStyleInput, ParsedStyleIr, PlainCssInput, StyleRewriteFailure,
    StyleRewriteFailureClass, StyleRewriteStage, StyleSourceIdentity, VerifiedPlainCss,
};
use oxc_allocator::Allocator;
use oxc_sourcemap::{SourceMap, Token};
use verter_css_syntax::{
    CssDialect, PreprocessorIdentity, QualifiedStyleResult, StyleDiagnostic, StyleStage,
};
use verter_span::Span;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VueStyleRewriteMask {
    pub v_bind: bool,
    pub css_modules: bool,
    pub scoped_selector: bool,
    pub keyframes: bool,
    pub deep: bool,
    pub slotted: bool,
    pub global: bool,
}

impl VueStyleRewriteMask {
    /// Whether any stage rewrote the block.
    #[must_use]
    pub const fn any_applied(&self) -> bool {
        self.v_bind
            || self.css_modules
            || self.scoped_selector
            || self.keyframes
            || self.deep
            || self.slotted
            || self.global
    }
}

#[derive(Debug, Clone, Default)]
pub struct VueStyleFacts {
    pub v_bind_vars: Vec<VBindVar>,
    pub module_classes: Vec<(String, String)>,
    pub refusals: Vec<StyleRewriteFailure>,
    pub rewrites: VueStyleRewriteMask,
    /// A parse of the cascade's INPUT surveyed its inclusions and answered
    /// whether they reach bytes nothing here parsed — or `None` when no such
    /// parse has run.
    ///
    /// Private, and three-state on purpose. The two states a `bool` conflates
    /// are the ones that matter: "surveyed, nothing foreign" and "never
    /// surveyed" are opposite answers to the question consumers ask, and only
    /// the tri-state distinguishes them. It is also what memoizes the
    /// recorder — shared planning parses the cascade input once, and a sheet
    /// with no inclusions must not read as "not recorded yet" and let a later
    /// observation publish a different space's answer as the input's.
    ///
    /// Read through [`Self::pulls_in_unparsed_bytes`], which fails closed.
    pub(crate) input_pulls_in_unparsed_bytes: Option<bool>,
}

impl VueStyleFacts {
    /// The facts of an authored-`v-bind()` stage: whether it rewrote anything and
    /// the variables it found.
    pub(crate) fn authored_v_bind(rewrote: bool, v_bind_vars: Vec<VBindVar>) -> Self {
        Self {
            rewrites: VueStyleRewriteMask {
                v_bind: rewrote,
                ..VueStyleRewriteMask::default()
            },
            v_bind_vars,
            ..Self::default()
        }
    }

    /// Whether this block pulls in stylesheet bytes nothing here parsed.
    ///
    /// `true` means the block's declared surface is incomplete: classes,
    /// custom properties and `v-bind()` calls may exist outside anything
    /// Verter saw, so a consumer publishing a complete-looking inventory has
    /// to fail open on it.
    ///
    /// **An unsurveyed block answers `true`.** `false` is the STRONG claim —
    /// "nothing outside these bytes can contribute to this block's surface" —
    /// and no parse has earned it until one has run. The reachable state is a
    /// cascade whose parse of the input never completed, with neither CSS
    /// Modules nor scoping requested to clear the output: nothing surveys the
    /// input, and answering "exhaustive" there is exactly the wrong-complete
    /// direction.
    ///
    /// A parse that COMPLETED has surveyed the block, and the answer is
    /// recorded from it before any stage plans an edit — so a stage that then
    /// refuses (an untrusted `v-bind()` rewrite target, an indented-layout
    /// mutation) does not un-survey it. A parse that merely RECOVERED still
    /// answers `true`, through the owner's own `discarded_input` check rather
    /// than through the absence of a recording.
    ///
    /// It is deliberately neither `!dependencies.is_empty()` nor a fold over
    /// an inclusion list — the style-syntax owner answers it for the whole
    /// parse, because a parse that recovered records FEWER inclusions than the
    /// sheet has, not more.
    #[must_use]
    pub const fn pulls_in_unparsed_bytes(&self) -> bool {
        match self.input_pulls_in_unparsed_bytes {
            Some(pulls) => pulls,
            None => true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum StyleRewriteOutcome {
    Unchanged {
        facts: VueStyleFacts,
    },
    Rewritten {
        code: String,
        source_map: String,
        facts: VueStyleFacts,
        output_descriptor: Box<RuntimeOutputDescriptor>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StyleEdit {
    Overwrite { span: Span, content: String },
    Insert { at: u32, content: String },
}

pub(crate) enum PreparedEdit<'a> {
    Overwrite {
        start: u32,
        end: u32,
        content: &'a str,
    },
    Insert {
        at: u32,
        content: &'a str,
    },
}

pub(crate) fn intern_edit_content<'a>(
    allocator: &'a Allocator,
    interned: &mut Vec<&'a str>,
    content: &str,
) -> &'a str {
    if let Some(existing) = interned.iter().copied().find(|s| *s == content) {
        existing
    } else {
        let interned_content = allocator.alloc_str(content);
        interned.push(interned_content);
        interned_content
    }
}

impl StyleEdit {
    pub(crate) const fn start(&self) -> u32 {
        match self {
            Self::Overwrite { span, .. } => span.start,
            Self::Insert { at, .. } => *at,
        }
    }

    pub(crate) const fn end(&self) -> u32 {
        match self {
            Self::Overwrite { span, .. } => span.end,
            Self::Insert { at, .. } => *at,
        }
    }
}

/// Result of running Vue's style cascade (`run_vue_style_cascade` /
/// `run_vue_style_cascade_verified`) end to end. The cascade never
/// hard-fails: a stage that cannot safely run is recorded in
/// `stage_failures` and the run continues on a best-effort basis instead of
/// aborting, so callers get one code path for both the fully-successful and
/// the degraded case.
#[derive(Debug, Clone)]
pub struct VueStyleCascadeOutcome {
    /// Final code after every requested stage's edits, qualified by the stage
    /// it now belongs to, the dialect it is written in, what produced it, and
    /// the inclusions and refusals observed along the way.
    ///
    /// The bytes are reachable only through this carrier: a caller that took a
    /// bare `String` here would have to re-derive which byte space it is
    /// holding, and every caller would answer that separately.
    /// The SOLE publication route for this run's diagnostics: `facts.refusals`
    /// and `stage_failures` below are each authority's own record of what it
    /// reported, and the carrier is where those records are projected into the
    /// shared diagnostic vocabulary exactly once. A consumer that reads both
    /// and re-projects reports every refusal twice, which is why projecting a
    /// failure into a diagnostic is not something a consumer can spell.
    pub result: QualifiedStyleResult,
    /// Authored-source-to-final-code map composed across every rewrite. Empty
    /// when no rewrite occurred or composition could not be completed.
    pub source_map: String,
    /// This run's accumulated style facts, including
    /// `facts.pulls_in_unparsed_bytes()` — the answer every entry point
    /// publishes for "does this block declare its whole surface".
    pub facts: VueStyleFacts,
    /// Stage-level failures (as opposed to `facts.refusals`' soft,
    /// individually-tolerated per-selector refusals): the authored-v-bind,
    /// CSS-Modules, or scoped-selector stage could not run at all. The
    /// authored-v-bind stage keeps whatever output preceded it on failure;
    /// the CSS-Modules and scoped-selector stages clear the output to empty
    /// and skip any stage after them, since their output is unsafe to use.
    pub stage_failures: Vec<StyleRewriteFailure>,
}

impl VueStyleCascadeOutcome {
    /// Final code after every requested stage's edits. Byte-identical to the
    /// cascade input when no stage produced any edit.
    #[must_use]
    pub fn code(&self) -> &str {
        self.result.code()
    }
}

/// Reports whether the cascade result can be published without exposing
/// output a stage wiped. The recorded refusal carrier is the authority:
/// [`QualifiedStyleResult::is_refused`] is what the runner wrote when it
/// cleared. An authored-`v-bind()` rewrite failure that left the input
/// intact is not a refusal and stays publishable. A parse miss that never
/// entered a later stage is still unpublished once the runner records
/// [`CascadeOutput::ClearedByRefusal`].
#[must_use]
pub fn cascade_output_is_publishable(
    outcome: &VueStyleCascadeOutcome,
    authored_code: &str,
) -> bool {
    // Emptiness is a shape two outcomes share — a wiped output and an
    // authored `<style></style>` — so publication reads the refusal flag,
    // never `code().is_empty()` and never the failing stage's identity.
    !(outcome.result.is_refused() && !authored_code.is_empty())
}

/// Returns an honest map for a requested cascade result: the accumulated map
/// for rewritten content, an identity map for byte-identical passthrough, or
/// no map when rewritten bytes could not be composed through every rewrite.
///
/// [`build_identity_source_map`] emits a token per authored character, so
/// every generated position resolves to the true authored position — not
/// just chunk/line boundaries.
#[must_use]
pub fn cascade_requested_source_map(
    outcome: &VueStyleCascadeOutcome,
    authored_code: &str,
    source_name: &str,
) -> Option<String> {
    if !outcome.source_map.is_empty() {
        return Some(outcome.source_map.clone());
    }
    if outcome.code() != authored_code {
        return None;
    }
    Some(build_identity_source_map(authored_code, source_name))
}

/// A genuinely byte-accurate identity map: one token per authored
/// character, generated position == authored position at every one.
/// Deliberately not `CodeTransform::generate_map` on a zero-edit transform —
/// that emits tokens only at chunk/line boundaries.
pub(crate) fn build_identity_source_map(source: &str, source_name: &str) -> String {
    let mut tokens: Vec<Token> = Vec::with_capacity(source.chars().count().max(1));
    let mut line = 0u32;
    let mut column = 0u32;
    let mut char_buf = [0u8; 4];
    for ch in source.chars() {
        tokens.push(Token::new(line, column, line, column, Some(0), None));
        advance_generated_position(ch.encode_utf8(&mut char_buf), &mut line, &mut column);
    }
    tokens.push(Token::new(line, column, line, column, Some(0), None));
    SourceMap::new(
        None,
        Vec::new(),
        None,
        vec![std::borrow::Cow::Owned(source_name.to_owned())],
        vec![Some(std::borrow::Cow::Owned(source.to_owned()))],
        tokens.into_boxed_slice(),
        None,
    )
    .to_json_string()
}

/// The map a materialized rewrite produced, if one was requested and built.
///
/// A single terminal `CodeTransform` per cascade run means there is never an
/// accumulated map to compose a later stage onto: every plan carries authored
/// coordinates right up to that one transform, so the map it generates is
/// already authored-source-to-final-code.
pub(crate) type MaterializedMap = Option<Box<SourceMap<'static>>>;

/// Applies a run's collected edits against `code`, returning the new
/// `(code, map)` pair when there was anything to rewrite, or `None` when there
/// was not (in which case the caller keeps the input bytes as its output).
pub(crate) fn apply_cascade_stage(
    code: &str,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    stage: StyleRewriteStage,
    edits: Vec<StyleEdit>,
    want_source_map: bool,
) -> Result<Option<(String, MaterializedMap)>, StyleRewriteFailure> {
    if edits.is_empty() {
        return Ok(None);
    }
    let Some((code, source_map)) = build_transform_output(
        code,
        dialect,
        stage,
        edits,
        want_source_map.then_some(source.source_name),
        want_source_map,
    )?
    else {
        unreachable!("non-empty edits always produce a rewrite")
    };
    Ok(Some((code, source_map.map(Box::new))))
}

/// The stage the bytes a cascade run was handed belong to, with the provenance
/// that stage carries.
///
/// The preprocessed arm names its producer because the cascade did not make
/// those bytes and must not claim to know who did: recording an unrewritten
/// pass-through as [`StyleProducer::ExternalAnonymous`] would say "the tool
/// supplied no identity" about a tool that may well have supplied one further
/// upstream. Only the entry point handed the bytes knows, so it says.
#[derive(Debug, Clone)]
pub enum CascadeInput {
    /// The carrier's own `<style>` bytes, in the authored dialect.
    Authored,
    /// Plain CSS an external preprocessor already produced, and the identity
    /// the caller has for that tool.
    Preprocessed(PreprocessorIdentity),
}

impl CascadeInput {
    const fn stage(&self) -> StyleStage {
        match self {
            Self::Authored => StyleStage::Authored,
            Self::Preprocessed(_) => StyleStage::Preprocessed,
        }
    }
}

/// What a finished cascade run left as its output.
///
/// The three states are genuinely different answers to "who produced these
/// bytes", and the pair `(Option<bytes>, cleared: bool)` they replace could
/// spell a fourth that means nothing. `ClearedByRefusal` in particular is NOT
/// "rewritten to empty": a stage that cannot run safely wipes the output so a
/// half-applied rewrite is never exposed, and nothing produced what is left.
/// Emptiness alone cannot tell the two apart, since an authored
/// `<style></style>` is empty as well.
pub(crate) enum CascadeOutput {
    /// No stage produced bytes; the cascade's input still stands.
    Passthrough,
    /// A stage rewrote the input into these bytes, with the map the one
    /// terminal transform generated.
    Rewritten {
        code: String,
        source_map: MaterializedMap,
    },
    /// A stage refused and wiped the output.
    ClearedByRefusal,
}

/// What recording a refusal does to the run's output.
///
/// Named at every site that records one, so "was the output cleared" is
/// decided where the refusal is raised rather than recovered downstream.
/// Neither half of a recorded failure determines it: the same `AuthoredVBind`
/// stage raising the same `ParseFailure` class clears a `<style module>`
/// request and leaves a v-bind-only request's bytes published, because what
/// differs is what the request needed. A publication read keyed on the class
/// (or on the stage) would therefore be guessing, which is why
/// [`cascade_output_is_publishable`] asks what the runner recorded instead.
#[derive(Clone, Copy)]
pub(crate) enum RefusalEffect {
    /// The cascade's input still stands; only this stage's rewrite was lost.
    KeepsOutput,
    /// The block's meaning depended on a rewrite that could not be planned,
    /// so publishing the un-rewritten bytes would be actively wrong.
    ClearsOutput,
}

/// The refusals one cascade run recorded, with the run's clearing state
/// derived from their effects.
///
/// The clearing state is not a field a caller sets: the only way to reach it
/// is to record a refusal and name its effect, so a run cannot be cleared
/// without the refusal that cleared it, and a clearing refusal cannot be
/// recorded without clearing.
pub(crate) struct StageFailures {
    failures: Vec<StyleRewriteFailure>,
    cleared_by_refusal: bool,
}

impl StageFailures {
    const fn new() -> Self {
        Self {
            failures: Vec::new(),
            cleared_by_refusal: false,
        }
    }

    fn record(&mut self, failure: StyleRewriteFailure, effect: RefusalEffect) {
        self.failures.push(failure);
        self.cleared_by_refusal |= matches!(effect, RefusalEffect::ClearsOutput);
    }

    pub(crate) fn into_vec(self) -> Vec<StyleRewriteFailure> {
        self.failures
    }
}

/// The CSS-only stages one cascade run may plan, after the plain-CSS gate has
/// answered.
///
/// `refusal` is `Some` exactly when the gate refused, and the gate refusing is
/// exactly why `module`/`scoped` are cleared. Holding both in one value is
/// what keeps "which stages were planned" and "was a refusal recorded" from
/// becoming two flags a second entry point could set inconsistently.
pub(crate) struct CssStageRequest {
    pub(crate) module: bool,
    pub(crate) scoped: bool,
    pub(crate) refusal: Option<StyleRewriteFailure>,
}

impl CssStageRequest {
    /// Admit `<style module>` / `<style scoped>` only for bytes that are
    /// already plain CSS.
    ///
    /// [`PlainCssInput::try_new`] is the SOLE predicate. Spelling a second one
    /// here (`dialect == CssDialect::Css`) would make the admitted and the
    /// refused sets complements only by review: a dialect that is neither
    /// plain CSS nor externally preprocessed would be admitted here and
    /// rejected there, skipping the rewrite with no diagnostic.
    pub(crate) fn gated(input: AuthoredStyleInput<'_>, module: bool, scoped: bool) -> Self {
        if !(module || scoped) {
            return Self::admitted(false, false);
        }
        match PlainCssInput::try_new(
            input.code,
            input.dialect,
            input.source.source_name,
            input.source.source_space_token,
            input.source.content_artifact_token,
        ) {
            Ok(_) => Self::admitted(module, scoped),
            Err(refusal) => Self {
                module: false,
                scoped: false,
                refusal: Some(refusal),
            },
        }
    }

    /// Stages over bytes whose plain-CSS grammar the caller already proved —
    /// there is nothing left for the gate to decide.
    const fn admitted(module: bool, scoped: bool) -> Self {
        Self {
            module,
            scoped,
            refusal: None,
        }
    }
}

pub(crate) struct SharedVueStylePlan {
    edits: Vec<StyleEdit>,
    pub(crate) facts: VueStyleFacts,
    terminal_stage: StyleRewriteStage,
    pub(crate) failures: StageFailures,
}

impl SharedVueStylePlan {
    fn refused(
        facts: VueStyleFacts,
        terminal_stage: StyleRewriteStage,
        mut failures: StageFailures,
        failure: StyleRewriteFailure,
    ) -> Self {
        failures.record(failure, RefusalEffect::ClearsOutput);
        Self {
            edits: Vec::new(),
            facts,
            terminal_stage,
            failures,
        }
    }
}

/// The one order a shared plan keeps its edits in, from the moment the first
/// stage's edits are planned to the terminal transform: ascending by start,
/// then by end.
pub(crate) fn edit_order_key(edit: &StyleEdit) -> (u32, u32) {
    (edit.start(), edit.end())
}

/// Edits held in [`edit_order_key`] order AND pairwise disjoint.
///
/// Both halves are ONE type invariant rather than a documented precondition or
/// a debug assertion. Order alone would be the weaker claim: the merge's
/// `Insert` arm inspects only the nearest earlier edit by start offset, and
/// that single candidate is the unique possible container exactly when the
/// stream it scans is disjoint — with disjointness, `prior[i - 2].end <=
/// prior[i - 1].start < at`, so no earlier edit can reach the offset. Carrying
/// disjointness in the type is what stops a future caller from handing that arm
/// an overlapping stream it would silently mis-read; stating the precondition
/// on the arm left it checked by review.
///
/// The only routes in are [`Self::from_planned`] (sorts, then checks) and
/// [`Self::from_ordered`] (checks edits already in plan order), and the only
/// route on is [`merge_shared_stage_edits`], which re-mints through
/// `from_ordered` because a merge composes two disjoint streams and the
/// composition need not be disjoint. A three-stage plan therefore sorts each
/// stage's own edits exactly once and never re-sorts what a previous merge
/// already ordered.
///
/// A debug assertion would not have held it: the workspace's sanctioned
/// assertion macro force-evaluates its condition in every profile, so
/// asserting sortedness here would run the scan in shipped builds too.
///
/// `build_transform_output` keeps its own independent sort-and-check as the
/// backstop for every route, including the direct per-stage transforms that
/// never build a shared plan.
pub(crate) struct PlanDisjointEdits(Vec<StyleEdit>);

impl PlanDisjointEdits {
    /// The plan a stage that refused to run contributes.
    const fn empty() -> Self {
        Self(Vec::new())
    }

    /// Establish the invariant over one stage's freshly planned edits.
    fn from_planned(mut edits: Vec<StyleEdit>) -> Option<Self> {
        edits.sort_by_key(edit_order_key);
        Self::from_ordered(edits)
    }

    /// Establish the invariant over edits already in [`edit_order_key`] order.
    /// Refuses when two edits touch the same bytes; zero-width inserts on an
    /// overwrite boundary are disjoint. Reads the maintained order rather than
    /// restoring it.
    fn from_ordered(edits: Vec<StyleEdit>) -> Option<Self> {
        let mut previous_end = 0;
        for edit in &edits {
            if edit.start() < previous_end {
                return None;
            }
            previous_end = previous_end.max(edit.end());
        }
        Some(Self(edits))
    }

    fn into_vec(self) -> Vec<StyleEdit> {
        self.0
    }
}

pub(crate) fn edit_overwrite_span(edit: &StyleEdit) -> Option<Span> {
    match edit {
        StyleEdit::Overwrite { span, .. } => Some(*span),
        StyleEdit::Insert { .. } => None,
    }
}

pub(crate) fn edit_overlaps_span(edit: &StyleEdit, span: Span) -> bool {
    match edit {
        StyleEdit::Overwrite {
            span: candidate, ..
        } => candidate.start < span.end && span.start < candidate.end,
        StyleEdit::Insert { at, .. } => span.start < *at && *at < span.end,
    }
}

pub(crate) fn span_contains_edit(span: Span, edit: &StyleEdit) -> bool {
    match edit {
        StyleEdit::Overwrite {
            span: candidate, ..
        } => span.start <= candidate.start && candidate.end <= span.end,
        StyleEdit::Insert { at, .. } => span.start <= *at && *at < span.end,
    }
}

/// Merge a later stage into authored-coordinate edits. A later edit wholly
/// inside an earlier overwrite targets source bytes that no longer reach that
/// stage and is discarded. A later deletion may subsume earlier work in bytes
/// it removes. Every other intersection is refused because retaining either
/// edit would misrepresent stage order.
pub(crate) fn merge_shared_stage_edits(
    prior: PlanDisjointEdits,
    later: Vec<StyleEdit>,
) -> Option<PlanDisjointEdits> {
    let PlanDisjointEdits(prior) = prior;
    let mut later = later;
    later.sort_by_key(edit_order_key);
    let mut keep_prior = vec![true; prior.len()];
    let mut keep_later = vec![true; later.len()];
    for (later_index, later_edit) in later.iter().enumerate() {
        let later_span = match later_edit {
            // Only the nearest earlier edit by start offset is inspected. That
            // single candidate is the unique possible container because
            // [`PlanDisjointEdits`] carries `prior`'s disjointness in its type:
            // there is no way to reach this arm with a stream whose earlier
            // edits overlap, so the arm needs no scan of its own.
            StyleEdit::Insert { at, .. } => {
                let prior_index = prior.partition_point(|edit| edit.start() < *at);
                if let Some(candidate) = prior_index.checked_sub(1).and_then(|i| prior.get(i)) {
                    if let Some(span) = edit_overwrite_span(candidate) {
                        if span.start < *at && *at < span.end {
                            keep_later[later_index] = false;
                        }
                    }
                }
                continue;
            }
            StyleEdit::Overwrite { span, .. } => *span,
        };
        let mut prior_index = prior.partition_point(|edit| edit.end() <= later_span.start);
        while let Some(prior_edit) = prior.get(prior_index) {
            if prior_edit.start() >= later_span.end {
                break;
            }
            if !edit_overlaps_span(prior_edit, later_span) {
                prior_index += 1;
            } else if edit_overwrite_span(prior_edit).is_some_and(|prior_span| {
                prior_span.start < later_span.start && later_span.end < prior_span.end
            }) {
                keep_later[later_index] = false;
                break;
            } else if span_contains_edit(later_span, prior_edit)
                && matches!(later_edit, StyleEdit::Overwrite { content, .. } if content.is_empty())
            {
                keep_prior[prior_index] = false;
                prior_index += 1;
            } else {
                return None;
            }
        }
    }

    // Both retained streams are in plan order, so interleaving them yields
    // plan order — this is what makes the returned invariant hold without a
    // second sort.
    let capacity = prior.len() + later.len();
    let mut retained_prior = prior
        .into_iter()
        .zip(keep_prior)
        .filter_map(|(edit, keep)| keep.then_some(edit))
        .peekable();
    let mut retained_later = later
        .into_iter()
        .zip(keep_later)
        .filter_map(|(edit, keep)| keep.then_some(edit))
        .peekable();
    let mut merged = Vec::with_capacity(capacity);
    loop {
        let take_prior = match (retained_prior.peek(), retained_later.peek()) {
            (Some(prior_edit), Some(later_edit)) => {
                edit_order_key(prior_edit) <= edit_order_key(later_edit)
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let next = if take_prior {
            retained_prior.next()
        } else {
            retained_later.next()
        };
        match next {
            Some(edit) => merged.push(edit),
            None => break,
        }
    }
    // `later` carries no disjointness of its own, and a composition of two
    // disjoint streams need not be disjoint, so the invariant is re-established
    // here rather than assumed.
    PlanDisjointEdits::from_ordered(merged)
}

/// Build every compatible Vue rewrite from one syntax IR. The plans retain
/// authored coordinates until the terminal `CodeTransform`, so compatible
/// stages need neither an intermediate stylesheet nor a second parse.
pub(crate) fn shared_vue_style_plan(
    ir: &ParsedStyleIr,
    dialect: CssDialect,
    scope_id: &str,
    module: bool,
    scoped: bool,
) -> SharedVueStylePlan {
    let origin = ir.source().origin();
    let terminal_stage = if scoped {
        StyleRewriteStage::PostPreprocessScoping
    } else if module {
        StyleRewriteStage::PostPreprocessModules
    } else {
        StyleRewriteStage::AuthoredVBind
    };
    let mut facts = VueStyleFacts::default();
    // Recorded from the parse, before any stage plans an edit. Every route
    // into a plan lands here, so the derived "does this block declare its
    // whole surface" answer cannot depend on which entry point the same
    // request took, and a stage that refuses to plan does not un-survey a
    // parse that already read the block's inclusions.
    record_input_dependencies(&mut facts, ir);
    let mut failures = StageFailures::new();

    observe_style_ir(StyleRewriteStage::AuthoredVBind, ir);
    let mut edits = match plan_authored_v_bind_edits(ir, dialect, scope_id) {
        Ok((edits, v_bind_vars)) => {
            facts.v_bind_vars = v_bind_vars;
            facts.rewrites.v_bind = !edits.is_empty();
            // The one sort of this stage's own edits, and the one place the
            // plan's order-and-disjointness invariant is established. Every
            // merge below re-establishes it over its own composition, so no
            // later gate has to re-derive it.
            match PlanDisjointEdits::from_planned(edits) {
                Some(edits) => edits,
                None => {
                    return SharedVueStylePlan::refused(
                        facts,
                        terminal_stage,
                        failures,
                        StyleRewriteFailure::new(
                            StyleRewriteFailureClass::OverlappingEdits,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            None,
                        ),
                    );
                }
            }
        }
        Err(failure) => {
            // The cascade's input bytes still stand, and every later stage
            // plans those same bytes, so a refusal here costs the `v-bind()`
            // lowering and nothing else.
            failures.record(failure, RefusalEffect::KeepsOutput);
            PlanDisjointEdits::empty()
        }
    };

    if module {
        observe_style_ir(StyleRewriteStage::PostPreprocessModules, ir);
        let (module_edits, classes) =
            match module_classes_and_edits_from_ir(ir, CssDialect::Css, scope_id) {
                Ok(planned) => planned,
                Err(failure) => {
                    return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
                }
            };
        facts.rewrites.css_modules |= !module_edits.is_empty();
        facts.module_classes.extend(classes);
        // Relocated here, with the stage that actually produced these edits,
        // so a `checked_sub` underflow on this group is reported against
        // `PostPreprocessModules` rather than borrowing whichever stage
        // happens to be terminal.
        let module_edits = match relocate_edits(
            module_edits,
            origin,
            StyleRewriteStage::PostPreprocessModules,
            dialect,
        ) {
            Ok(edits) => edits,
            Err(failure) => {
                return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
            }
        };
        let Some(merged) = merge_shared_stage_edits(edits, module_edits) else {
            return SharedVueStylePlan::refused(
                facts,
                terminal_stage,
                failures,
                StyleRewriteFailure::new(
                    StyleRewriteFailureClass::OverlappingEdits,
                    StyleRewriteStage::PostPreprocessModules,
                    dialect,
                    None,
                ),
            );
        };
        edits = merged;
    }
    if scoped {
        observe_style_ir(StyleRewriteStage::PostPreprocessScoping, ir);
        let (scope_edits, scope_facts) = match scoped_edits_and_facts_from_ir(ir, scope_id) {
            Ok(planned) => planned,
            Err(failure) => {
                return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
            }
        };
        // Accumulated, never assigned: a plan folds every stage's observations
        // into one fact set, so a stage that arrives later must not be able to
        // clear what an earlier one recorded. Today no earlier stage sets these
        // fields, which is exactly why an assignment would look correct right
        // up until one does.
        facts.rewrites.deep |= scope_facts.rewrites.deep;
        facts.rewrites.slotted |= scope_facts.rewrites.slotted;
        facts.rewrites.global |= scope_facts.rewrites.global;
        facts.rewrites.keyframes |= scope_facts.rewrites.keyframes;
        facts.rewrites.scoped_selector |= scope_facts.rewrites.scoped_selector;
        facts.refusals.extend(scope_facts.refusals);
        // Relocated here, with the stage that actually produced these edits —
        // see the module-edit relocation above.
        let scope_edits = match relocate_edits(
            scope_edits,
            origin,
            StyleRewriteStage::PostPreprocessScoping,
            dialect,
        ) {
            Ok(edits) => edits,
            Err(failure) => {
                return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
            }
        };
        let Some(merged) = merge_shared_stage_edits(edits, scope_edits) else {
            return SharedVueStylePlan::refused(
                facts,
                terminal_stage,
                failures,
                StyleRewriteFailure::new(
                    StyleRewriteFailureClass::OverlappingEdits,
                    StyleRewriteStage::PostPreprocessScoping,
                    dialect,
                    None,
                ),
            );
        };
        edits = merged;
    }

    // No terminal disjointness gate: `edits` is a [`PlanDisjointEdits`], so
    // every route that produced it already refused rather than composed
    // overlapping spans.
    SharedVueStylePlan {
        edits: edits.into_vec(),
        facts,
        terminal_stage,
        failures,
    }
}

/// The ONE route from a parsed IR to a finished cascade outcome. Every entry
/// point — authored-only, the full authored cascade, and the verified plain-CSS
/// one — reaches materialization through here, so a request that names the same
/// stages is answered the same way whatever spelled it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_shared_vue_style_plan(
    ir: &ParsedStyleIr,
    input: CascadeInput,
    code: &str,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    scope_id: &str,
    css: CssStageRequest,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    let SharedVueStylePlan {
        edits,
        facts,
        terminal_stage,
        mut failures,
    } = shared_vue_style_plan(ir, dialect, scope_id, css.module, css.scoped);
    // Recorded before materialization, not after: a gate refusal already
    // dropped the stages the request's meaning depended on, so building a
    // transform whose output is about to be wiped would be work nobody reads.
    if let Some(refusal) = css.refusal {
        failures.record(refusal, RefusalEffect::ClearsOutput);
    }
    let output = if failures.cleared_by_refusal {
        CascadeOutput::ClearedByRefusal
    } else {
        match apply_cascade_stage(
            code,
            source,
            dialect,
            terminal_stage,
            edits,
            want_source_map,
        ) {
            Ok(Some((code, source_map))) => CascadeOutput::Rewritten { code, source_map },
            Ok(None) => CascadeOutput::Passthrough,
            Err(failure) => {
                // The terminal transform materializes the WHOLE plan, so a
                // failure here loses every stage's rewrite at once and leaves
                // nothing safe to publish.
                failures.record(failure, RefusalEffect::ClearsOutput);
                CascadeOutput::ClearedByRefusal
            }
        }
    };
    finish_vue_style_cascade(input, dialect, code, output, facts, failures.into_vec())
}

/// Assemble the one cascade outcome shape from a finished run.
///
/// Every cascade entry point lands here, so the rule that decides which byte
/// space the result belongs to — and therefore which space each refusal's span
/// addresses — is written once rather than re-derived per entry point.
///
/// `input` is the stage the cascade was handed: authored bytes for a carrier's
/// own `<style>` content, preprocessed bytes when an external tool produced
/// them. A run that changed nothing stays at that stage; a run that produced
/// output is a framework-rewritten result.
///
/// Shared planning hands every compatible stage the same cascade-input IR, so
/// a later-stage refusal's span always addresses the cascade's own input
/// space — authored carrier bytes or admitted preprocessed CSS — never a
/// later rewrite that has not been materialized yet. Clearing the output
/// after a refusal does not move the bytes the refusal was reported against.
pub(crate) fn finish_vue_style_cascade(
    input: CascadeInput,
    dialect: CssDialect,
    input_code: &str,
    output: CascadeOutput,
    facts: VueStyleFacts,
    stage_failures: Vec<StyleRewriteFailure>,
) -> VueStyleCascadeOutcome {
    let input_stage = input.stage();
    // Projected once, here, for every entry point. This is the SOLE
    // publication route (see `VueStyleCascadeOutcome.result`), so doing it
    // per-consumer instead would re-derive the same strings once per reader
    // and lose the single-route property that keeps a refusal from being
    // reported twice. A run with no refusals collects an empty `Vec`, which
    // allocates nothing — the cost is exactly one projection per refusal, and
    // only sheets that actually refused pay it.
    let diagnostics: Vec<StyleDiagnostic> = facts
        .refusals
        .iter()
        .chain(stage_failures.iter())
        .map(|failure| failure.to_diagnostic(input_stage))
        .collect();
    // The output state carries the bytes that state owns, so each arm reads
    // its own — a rewrite cannot reach for bytes a passthrough never minted.
    let (result, source_map) = match output {
        // Nothing produced these (absent) bytes, so nothing claims them. The
        // stage still names the space the refusals' own coordinates belong
        // to, which is what makes them placeable.
        CascadeOutput::ClearedByRefusal => (
            QualifiedStyleResult::refused(input_stage, dialect, diagnostics),
            String::new(),
        ),
        CascadeOutput::Rewritten { code, source_map } => (
            QualifiedStyleResult::framework_rewritten(dialect, code, diagnostics),
            source_map
                .map(|map| map.to_json_string())
                .unwrap_or_default(),
        ),
        CascadeOutput::Passthrough => {
            let result = match input {
                CascadeInput::Authored => {
                    QualifiedStyleResult::authored(dialect, input_code, diagnostics)
                }
                CascadeInput::Preprocessed(producer) => {
                    QualifiedStyleResult::preprocessed(producer, input_code, diagnostics)
                }
            };
            (result, String::new())
        }
    };
    VueStyleCascadeOutcome {
        result,
        source_map,
        facts,
        stage_failures,
    }
}

/// Runs only Vue's authored-dialect `v-bind()` stage.
///
/// This is the explicit entry point for a bundler Main render whose separate
/// style-module pipeline owns preprocessing and every plain-CSS-only stage.
/// It never requests CSS Modules or selector scoping, so authored SCSS/Sass/
/// Less/Stylus cannot be mistaken for completed CSS. The returned facts still
/// drive `_useCssVars` generation in the runtime module.
///
/// Literally [`run_vue_style_cascade`] with neither attribute set: those are
/// the same request — `v-bind()` lowering and nothing else — and delegating
/// rather than re-deriving it is what keeps the two entry points from
/// answering it differently. A second route here diverged on exactly one
/// recorded fact (whether a parse that survived but whose `v-bind()` planning
/// refused had surveyed the block's inclusions), which no signature could have
/// caught.
pub fn run_vue_style_authored_only(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    run_vue_style_cascade(input, scope_id, false, false, want_source_map)
}

/// Runs Vue's authored-v-bind → CSS-Modules → scoped-selector cascade.
/// Compatible stages derive authored-coordinate edits from one
/// `StyleSyntaxIr` and materialize them once. A plan whose edits cannot be
/// composed returns a terminal refusal without materializing partial output
/// or entering a staged fallback. Every CSS input — including v-bind-only —
/// takes that shared plan, with a terminal stage naming the last stage that
/// actually ran. `module`/`scoped` mirror the SFC's
/// `<style module>`/`<style scoped>` attributes; a non-CSS dialect with
/// either attribute set refuses at the [`PlainCssInput`] gate and never
/// parses those bytes as CSS.
///
/// A stage that cannot safely run does not abort the whole cascade — see
/// [`VueStyleCascadeOutcome::stage_failures`]. The authored-v-bind stage
/// runs against the authored bytes regardless of whether it itself
/// succeeds, so a v-bind rewrite failure still lets CSS-Modules/scoped-selector
/// plan those same authored bytes.
///
/// A parse miss is recorded exactly once, by the parse that ran. What it does
/// to the output depends on what was asked for, and the answer is the same one
/// [`run_vue_style_authored_only`] gives, because they are the same request:
/// with neither `module` nor `scoped` the only work was `v-bind()` lowering,
/// nothing was rewritten, and the authored bytes are published beside the
/// diagnostic rather than deleted. With either attribute set the block's
/// meaning depends on a rewrite that could not be planned — unhashed class
/// names, or selectors that would apply to the whole document instead of this
/// component — so publishing the authored bytes would be actively wrong and
/// the output is cleared.
pub fn run_vue_style_cascade(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
    module: bool,
    scoped: bool,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    verter_audit::attribute_scope!(CssTransform);
    // One gate for every dialect, answered before anything is parsed. A
    // non-CSS `<style module>`/`<style scoped>` loses those stages here and
    // records the refusal that lost them; a v-bind-only request never consults
    // it. There is no second branch to keep in step with this one.
    let css = CssStageRequest::gated(input, module, scoped);
    match authored_or_parsed_ir(input, StyleRewriteStage::AuthoredVBind) {
        Ok(parsed) => run_shared_vue_style_plan(
            &parsed,
            CascadeInput::Authored,
            input.code,
            input.source,
            input.dialect,
            scope_id,
            css,
            want_source_map,
        ),
        Err(failure) => {
            // Nothing parsed, so nothing planned. See this function's doc
            // comment: only a request whose meaning depends on a CSS rewrite
            // clears on a parse miss.
            let mut failures = StageFailures::new();
            failures.record(
                failure,
                if module || scoped {
                    RefusalEffect::ClearsOutput
                } else {
                    RefusalEffect::KeepsOutput
                },
            );
            if let Some(refusal) = css.refusal {
                failures.record(refusal, RefusalEffect::ClearsOutput);
            }
            finish_vue_style_cascade(
                CascadeInput::Authored,
                input.dialect,
                input.code,
                if failures.cleared_by_refusal {
                    CascadeOutput::ClearedByRefusal
                } else {
                    CascadeOutput::Passthrough
                },
                VueStyleFacts::default(),
                failures.into_vec(),
            )
        }
    }
}

/// Runs the full Vue style cascade from native-CSS grammar provenance.
///
/// The supplied parse is the cascade's only CSS parse. Compatible stages
/// plan over it and materialize once. `input_stage` names where these bytes
/// came from. Plain CSS reaching this entry is either a carrier's own
/// authored CSS or an external preprocessor's output, and only the caller
/// knows which — the bytes look the same either way. It is a parameter
/// rather than an inference because guessing writes the wrong provenance
/// onto every result and, worse, the wrong coordinate space onto every
/// refusal.
///
/// The authored-`v-bind()` stage runs on both: a preprocessor leaves
/// `v-bind()` untouched in its output, so lowering it is still this cascade's
/// job. It simply runs against preprocessed bytes, and the recorded stage says
/// so.
#[allow(clippy::too_many_arguments)]
pub fn run_vue_style_cascade_verified(
    verified: VerifiedPlainCss<'_>,
    input_stage: CascadeInput,
    source_name: &str,
    source_space_token: &str,
    content_artifact_token: &str,
    scope_id: &str,
    module: bool,
    scoped: bool,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    verter_audit::attribute_scope!(CssTransform);
    let code = verified.code();
    let source = StyleSourceIdentity {
        source_name,
        source_space_token,
        content_artifact_token,
    };
    let parsed = ParsedStyleIr::from_existing(verified.ir.clone());
    run_shared_vue_style_plan(
        &parsed,
        input_stage,
        code,
        source,
        CssDialect::Css,
        scope_id,
        // `VerifiedPlainCss` already carries the plain-CSS grammar proof the
        // authored gate exists to establish, so there is nothing left to gate.
        CssStageRequest::admitted(module, scoped),
        want_source_map,
    )
}

/// Type-state-gated entry point for Vue transforms over CSS-grammar-proven
/// bytes.
#[allow(clippy::too_many_arguments)]
pub fn transform_vue_style(
    verified: VerifiedPlainCss<'_>,
    input_stage: CascadeInput,
    source_name: &str,
    source_space_token: &str,
    content_artifact_token: &str,
    scope_id: &str,
    module: bool,
    scoped: bool,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    run_vue_style_cascade_verified(
        verified,
        input_stage,
        source_name,
        source_space_token,
        content_artifact_token,
        scope_id,
        module,
        scoped,
        want_source_map,
    )
}

#[cfg(test)]
mod merge_shared_stage_edits_tests {
    use super::{merge_shared_stage_edits, span_contains_edit, PlanDisjointEdits, StyleEdit};
    use verter_span::Span;

    /// Establishes the plan invariant over `prior` the same way
    /// `shared_vue_style_plan` does, then unwraps the merged result so each case
    /// reads as edits in, edits out. `prior` is expected to satisfy the
    /// invariant: a stream that does not cannot reach a merge at all, which is
    /// what `overlapping_prior_edits_cannot_be_minted_into_a_plan` pins.
    fn merge(prior: Vec<StyleEdit>, later: Vec<StyleEdit>) -> Option<Vec<StyleEdit>> {
        let prior = PlanDisjointEdits::from_planned(prior)
            .expect("a merge's prior stream is disjoint by construction");
        merge_shared_stage_edits(prior, later).map(PlanDisjointEdits::into_vec)
    }

    fn overwrite(start: u32, end: u32, content: &str) -> StyleEdit {
        StyleEdit::Overwrite {
            span: Span::new(start, end),
            content: content.to_string(),
        }
    }

    fn insert(at: u32, content: &str) -> StyleEdit {
        StyleEdit::Insert {
            at,
            content: content.to_string(),
        }
    }

    /// Path 1: an earlier overwrite strictly containing a later edit discards
    /// the later edit — the later stage targeted bytes the earlier stage
    /// already replaced.
    ///
    /// Mutation recipe: drop the `keep_later[later_index] = false` in the
    /// strictly-contained overwrite arm — the merge then keeps both edits and
    /// `PlanDisjointEdits::from_ordered` refuses, so `expect` panics.
    #[test]
    fn earlier_overwrite_strictly_containing_later_edit_discards_the_later_edit() {
        let prior = vec![overwrite(0, 20, "var(--sc1-tone)")];
        let later = vec![overwrite(5, 10, ".hashed")];

        let merged = merge(prior.clone(), later.clone())
            .expect("a later edit inside an earlier overwrite must not refuse the merge");

        assert_eq!(merged, prior, "only the earlier overwrite survives");
    }

    /// Path 2: a later deletion (an empty-content overwrite) strictly
    /// containing one or more earlier edits discards those earlier edits
    /// instead — the later stage rewrote the whole region away, so the
    /// earlier work inside it never reaches the source.
    ///
    /// Mutation recipe: drop `keep_prior[prior_index] = false` from that arm —
    /// the earlier edits survive inside the deletion, the merged vector is no
    /// longer disjoint, and `expect` panics.
    #[test]
    fn later_empty_overwrite_strictly_containing_earlier_edits_discards_the_earlier_edits() {
        let prior = vec![overwrite(5, 10, "one"), overwrite(12, 15, "two")];
        let later = vec![overwrite(0, 20, "")];

        let merged = merge(prior.clone(), later.clone())
            .expect("a later deletion subsuming earlier edits must not refuse the merge");

        assert_eq!(
            merged, later,
            "only the later deletion survives; both earlier edits are discarded"
        );
    }

    /// Path 2's boundary complement: a deletion `[start, end)` removes no byte
    /// at `end`, so an earlier insert positioned exactly at the deletion's end
    /// addresses the gap AFTER the removed bytes and must survive the merge.
    /// This is the shape a later deletion would see the moment any stage
    /// deletion follows a scope-attribute insert.
    ///
    /// Mutation recipe: the boundary insert is shielded by two independent
    /// rails, and the recipe has to break both. Widen `edit_overlaps_span`'s
    /// insert arm to the closed `span.start <= *at && *at <= span.end` AND
    /// relax the scan's break to `prior_edit.start() > later_span.end` — the
    /// boundary insert is then read as overlapping the deletion, the half-open
    /// containment arm no longer subsumes it, and the merge refuses instead of
    /// keeping it. Widening the overlap arm alone cannot bite: the scan breaks
    /// on `insert.start() == later_span.end` before overlap is consulted.
    #[test]
    fn earlier_insert_at_a_later_deletions_end_survives() {
        let prior = vec![insert(20, "[data-v-sc1]")];
        let later = vec![overwrite(0, 20, "")];

        let merged = merge(prior.clone(), later.clone())
            .expect("an insert at a deletion's end offset must not refuse the merge");

        assert_eq!(
            merged,
            vec![later[0].clone(), prior[0].clone()],
            "the deletion survives and the boundary insert is kept, in plan order"
        );
    }

    /// A later deletion that only partially overlaps an earlier edit — never
    /// strictly containing it — is neither path 1 nor path 2, so the merge
    /// refuses rather than silently dropping or keeping either edit.
    ///
    /// Two independent rails refuse it, and the recipe has to break both: the
    /// classifier's trailing `return None`, and the re-mint of the merged
    /// vector through `PlanDisjointEdits::from_ordered`.
    ///
    /// Mutation recipe: replace that `return None` with `prior_index += 1` AND
    /// make `PlanDisjointEdits::from_ordered` return `Some(Self(edits))`
    /// unconditionally — the merge then hands the terminal transform two edits
    /// whose spans overlap. Breaking either rail alone still refuses.
    #[test]
    fn partially_overlapping_edits_refuse_the_merge() {
        let prior = vec![overwrite(0, 10, "one")];
        let later = vec![overwrite(5, 15, "")];

        assert!(
            merge(prior, later).is_none(),
            "a partial overlap must refuse rather than guess an ordering"
        );
    }

    /// The classifier only ever compares a LATER edit against the prior ones,
    /// so two prior edits that overlap each other would pass it untouched — and
    /// the `Insert` arm reads only the nearest prior edit by start offset, so it
    /// would mis-read them rather than refuse. Nothing downstream can catch that,
    /// which is why the mint is where it is refused: a stream whose own edits
    /// overlap can never become a [`PlanDisjointEdits`], so it can never be
    /// handed to a merge at all.
    ///
    /// Mutation recipe: make `PlanDisjointEdits::from_ordered` return
    /// `Some(Self(edits))` unconditionally — the overlapping pair mints a plan
    /// and this case resolves to a `Some` it must not.
    #[test]
    fn overlapping_prior_edits_cannot_be_minted_into_a_plan() {
        let overlapping = vec![overwrite(0, 10, "one"), overwrite(5, 15, "two")];

        assert!(
            PlanDisjointEdits::from_planned(overlapping).is_none(),
            "overlapping edits must never become a plan a merge can read"
        );
    }

    /// The re-mint over the merged vector is not redundant with the classifier
    /// either: `later` carries no invariant of its own, so two LATER edits that
    /// overlap each other reach the merged vector untouched. Only the re-mint
    /// sees them, and it must refuse — handing the terminal transform
    /// overlapping spans is the half-ordered rewrite the whole merge exists to
    /// prevent.
    ///
    /// Mutation recipe: make `PlanDisjointEdits::from_ordered` return
    /// `Some(Self(edits))` unconditionally — every other merge case still
    /// refuses through the classifier, and only this one resolves to a `Some`
    /// it must not.
    #[test]
    fn overlapping_later_edits_refuse_even_when_the_prior_plan_is_disjoint() {
        // Neither rail below inspects this pair: the classifier walks `later`
        // against `prior`, never `later` against itself.
        let prior = vec![insert(40, "[data-v-sc1]")];
        let later = vec![overwrite(0, 10, "one"), overwrite(5, 15, "two")];

        assert!(
            merge(prior, later).is_none(),
            "overlapping edits must never reach the terminal transform"
        );
    }

    /// The containment scan reads the NEAREST earlier edit by start offset, so
    /// it has to find the right container when several prior edits precede the
    /// insertion point. That is exactly what [`PlanDisjointEdits`] buys the arm:
    /// with `prior` disjoint, the nearest-by-start edit is the only one that can
    /// contain the offset, so one candidate is enough.
    /// A scan that read a fixed prior edit instead would splice a scope
    /// attribute into the middle of a `v-bind()` replacement whenever any
    /// unrelated rewrite came first.
    ///
    /// Mutation recipe: replace the `partition_point` lookup with a fixed
    /// index — the interior insert is no longer recognised as contained, and
    /// this merge hands back three edits instead of two.
    #[test]
    fn later_insert_finds_its_container_past_an_unrelated_earlier_overwrite() {
        let prior = vec![
            overwrite(0, 10, "var(--sc1-tone)"),
            overwrite(20, 40, ".hashed"),
        ];
        let later = vec![insert(30, "[data-v-sc1]")];

        let merged = merge(prior.clone(), later)
            .expect("an insert inside the later overwrite must not refuse the merge");

        assert_eq!(
            merged, prior,
            "the interior insert is dropped and both overwrites survive"
        );
    }

    /// Only a DELETION may subsume earlier work. A later overwrite that
    /// contains an earlier edit but writes bytes of its own has no defined
    /// composition — keeping the earlier edit would apply it to bytes the
    /// later stage replaced, dropping it would silently lose that stage's
    /// rewrite — so the merge refuses instead of choosing.
    ///
    /// Mutation recipe: weaken the arm's
    /// `matches!(later_edit, StyleEdit::Overwrite { content, .. } if content.is_empty())`
    /// guard to `matches!(later_edit, StyleEdit::Overwrite { .. })` — the
    /// earlier edit is silently discarded and this merge resolves.
    #[test]
    fn later_non_empty_overwrite_containing_an_earlier_edit_refuses_the_merge() {
        let prior = vec![overwrite(5, 10, "var(--sc1-tone)")];
        let later = vec![overwrite(0, 20, ".hashed { color: red; }")];

        assert!(
            merge(prior, later).is_none(),
            "only an empty later overwrite (a deletion) may subsume earlier edits"
        );
    }

    /// Path 1 for the insert shape: a later insertion point strictly inside an
    /// earlier overwrite addresses bytes that overwrite already replaced, so
    /// it is discarded. A scope-attribute insert landing inside a `v-bind()`
    /// replacement is the live shape — keeping it would splice the attribute
    /// into the middle of `var(--sc1-tone)`.
    ///
    /// Mutation recipe: delete the `keep_later[later_index] = false` in the
    /// `StyleEdit::Insert` arm — the insert survives and the merge hands back
    /// two edits instead of one.
    #[test]
    fn later_insert_strictly_inside_an_earlier_overwrite_is_discarded() {
        let prior = vec![overwrite(0, 20, "var(--sc1-tone)")];
        let later = vec![insert(10, "[data-v-sc1]")];

        let merged = merge(prior.clone(), later)
            .expect("an insert inside an earlier overwrite must not refuse the merge");

        assert_eq!(
            merged, prior,
            "only the earlier overwrite survives; the interior insert is dropped"
        );
    }

    /// The boundaries are not interior. An insert exactly at an overwrite's
    /// start or end still addresses live authored bytes, so both survive —
    /// which is what keeps the discard rule from eating the common
    /// scope-attribute insert that abuts a rewritten selector.
    ///
    /// Mutation recipe: widen the interior test to `span.start <= *at && *at <=
    /// span.end` and both boundary inserts vanish from the merge.
    #[test]
    fn later_insert_on_an_earlier_overwrite_boundary_survives() {
        for at in [0, 20] {
            let prior = vec![overwrite(0, 20, "var(--sc1-tone)")];
            let later = vec![insert(at, "[data-v-sc1]")];

            let merged =
                merge(prior, later.clone()).expect("a boundary insert must not refuse the merge");

            assert!(
                merged.contains(&later[0]),
                "an insert at offset {at} abuts the overwrite rather than \
                 sitting inside it: {merged:?}"
            );
            assert_eq!(merged.len(), 2, "{merged:?}");
        }
    }

    /// The insert scan reads the nearest earlier edit by start offset, so an
    /// insert must survive an unrelated overwrite that merely precedes it.
    ///
    /// Mutation recipe: drop the `edit_overwrite_span` containment test in the
    /// insert arm and every insert following any overwrite is discarded.
    #[test]
    fn later_insert_after_a_disjoint_earlier_overwrite_survives() {
        let prior = vec![overwrite(0, 10, "var(--sc1-tone)")];
        let later = vec![insert(30, "[data-v-sc1]")];

        let merged = merge(prior.clone(), later.clone())
            .expect("a disjoint insert must not refuse the merge");

        assert_eq!(merged, vec![prior[0].clone(), later[0].clone()]);
    }

    /// Deletion subsumption reads inserts against the same half-open span the
    /// rest of the module does: an insert is contained only strictly interior,
    /// because `[start, end)` owns no position at `end`. The predicate is
    /// reachable only behind `edit_overlaps_span`'s strictly-interior insert
    /// guard, so no merge case can distinguish the end edge — it is pinned
    /// here, against the predicate itself. The left edge stays closed to
    /// mirror the overwrite arm's `span.start <= candidate.start`.
    ///
    /// Mutation recipe: revert the arm to `*at <= span.end` — the
    /// end-boundary assertion fails.
    #[test]
    fn span_containment_for_inserts_ends_at_the_half_open_edge() {
        let deletion = Span::new(0, 20);

        assert!(
            span_contains_edit(deletion, &insert(10, "[data-v-sc1]")),
            "an insert strictly inside the deleted bytes is subsumed"
        );
        assert!(
            !span_contains_edit(deletion, &insert(20, "[data-v-sc1]")),
            "position 20 is the gap after the deleted bytes, not a deleted byte"
        );
    }
}
