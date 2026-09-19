//! Source-backed projection plan and durable use identities.
//!
//! Plan products reference admitted expression IDs, source-unit lineage,
//! lexical scopes and framework facts. They do not store generated text,
//! choose answers via the native type engine, or treat offsets/content
//! hashes as durable use identities. Incomplete observations cannot warm
//! a complete cache.
//!
//! [`ProjectionPlan::syntax_obligations`] is the only CodeTransform-facing
//! surface. Vue IDE routing stays on the existing companion path until
//! Vue atomic activation switches the live route.

use std::collections::BTreeMap;

use oxc_span::GetSpan;
use rustc_hash::FxHashMap;

use verter_identity::canonical::Canonical;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ParseKey, SourceUnitId, SyntaxProfileId};

use crate::assembly::source_unit::{carrier_revision, carrier_source_id};
use crate::ast::types::{AstNodeKind, ElementNode, ElementNodeConditionKind, InterpolationNode};
use crate::ide::{event_to_jsx_name, get_directive_name};
use crate::parser::types::ParsedSfc;
use crate::template::oxc::parse_template_expressions;
use crate::template::oxc::types::{
    OxcNodeData, OxcParsedAst, OxcParsedElement, OxcParsedExpression,
};
use crate::types::{NodeId, NodeProp, NodeTag};

pub mod origin;
pub use origin::ObservationRole;

const USE_DOMAIN: &str = "verter.compiler.projection_plan.component_use_id.v1";
const ORIGIN_DOMAIN: &str = "verter.compiler.projection_plan.binding_origin_id.v1";
const SCOPE_DOMAIN: &str = "verter.compiler.projection_plan.lexical_scope_id.v1";
const EXPR_DOMAIN: &str = "verter.compiler.projection_plan.admitted_expression_id.v1";
const SNAPSHOT_DOMAIN: &str = "verter.compiler.projection_plan.snapshot_id.v1";
const BINDER_DOMAIN: &str = "verter.compiler.projection_plan.generic_binder_ref.v1";
const INPUT_BASIS_DOMAIN: &str = "verter.compiler.projection_plan.input_basis.v1";

/// Logical component-use identity: one component expression in one
/// lexical environment, distinct from revision-qualified observations.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentUseId(Canonical);

/// Origin of a same-spelled binding: lexical scope plus binder kind, not
/// an offset into generated text.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BindingOriginId(Canonical);

/// Nested lexical environment (template root, v-for, v-slot).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LexicalScopeId(Canonical);

/// Admitted expression identity (kind + logical spelling + occurrence).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AdmittedExpressionId(Canonical);

/// Exact input snapshot the plan was observed against.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlanSnapshotId(Canonical);

/// Accepted two-binder family member (`T` / `U`) attached to one use.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GenericBinderRef(Canonical);

macro_rules! id_debug {
    ($t:ident) => {
        impl core::fmt::Debug for $t {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, concat!(stringify!($t), "({})"), self.0.digest().to_hex())
            }
        }
        impl $t {
            /// Compact digest hex, for tests and harness observations.
            #[must_use]
            pub fn digest_hex(&self) -> String {
                self.0.digest().to_hex()
            }
            #[must_use]
            pub fn canonical_bytes(&self) -> &[u8] {
                self.0.bytes()
            }
            #[must_use]
            pub fn digest(&self) -> CanonicalDigest {
                self.0.digest()
            }
        }
    };
}

id_debug!(ComponentUseId);
id_debug!(BindingOriginId);
id_debug!(LexicalScopeId);
id_debug!(AdmittedExpressionId);
id_debug!(PlanSnapshotId);
id_debug!(GenericBinderRef);

/// Kind of an admitted template expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionKind {
    /// Static component tag name.
    ComponentTag,
    /// Dynamic `<component :is>` target.
    ComponentIs,
    /// Attribute / directive value.
    AttributeValue,
    /// `v-if` / `v-else-if` condition.
    BranchCondition,
    /// `v-for` iterable source.
    VForSource,
    /// `v-slot` parameter list.
    SlotParams,
    /// Mustache interpolation.
    Interpolation,
}

impl ExpressionKind {
    fn tag(self) -> &'static str {
        match self {
            Self::ComponentTag => "component-tag",
            Self::ComponentIs => "component-is",
            Self::AttributeValue => "attribute-value",
            Self::BranchCondition => "branch-condition",
            Self::VForSource => "v-for-source",
            Self::SlotParams => "slot-params",
            Self::Interpolation => "interpolation",
        }
    }
}

/// How a binding was introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderKind {
    /// `v-for` alias.
    VForAlias,
    /// Slot prop / `v-slot` parameter.
    SlotProp,
}

impl BinderKind {
    fn tag(self) -> &'static str {
        match self {
            Self::VForAlias => "v-for-alias",
            Self::SlotProp => "slot-prop",
        }
    }
}

/// Ordered-attribute operation kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeOpKind {
    /// `v-bind` spread (no argument).
    VBind,
    /// `v-on` spread (no argument) or `@event` / `v-on:event`.
    VOn,
    /// Static attribute.
    Static,
    /// Bound attribute (`:foo` / `v-bind:foo`).
    Bound,
    /// Custom directive (`v-focus`, `v-click-outside`, \ldots).
    Directive,
    /// `v-model`.
    Model,
}

/// One ordered attribute operation on a component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedAttributeOp {
    /// Dense source order among this use's emitted operations (control-flow
    /// props such as `v-if` / `v-for` / `v-slot` are not operations).
    pub index: u32,
    /// Operation kind.
    pub kind: AttributeOpKind,
    /// Logical attribute / event / directive name (not a byte offset).
    pub name: String,
    /// Admitted value expression, when present.
    pub expression: Option<AdmittedExpressionId>,
    /// Admitted dynamic argument expression (`:[key]`), when present.
    pub argument: Option<AdmittedExpressionId>,
    /// Authored `|modifier` list, in source order.
    pub modifiers: Vec<String>,
    /// Whether this bound/value-bearing channel participates in inference.
    pub inference_participation: bool,
}

/// Outcome of one template branch member. Not a concatenated condition
/// string and not a native TypeScript flow graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchOutcome {
    /// `v-if` / `v-else-if` taken arm.
    Taken,
    /// `v-else` arm of the originating `v-if` expression.
    Else,
}

/// One template control-flow edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEdge {
    /// Condition expression identity (originating `v-if` for `v-else`).
    pub expression: AdmittedExpressionId,
    /// Branch outcome.
    pub outcome: BranchOutcome,
    /// Lexical environment in which the condition is evaluated.
    pub lexical_env: LexicalScopeId,
    /// Preceding chain conditions that this arm excludes (else-if / else).
    /// Empty for an independent `v-if`. Not a concatenated condition string.
    pub excluded: Vec<AdmittedExpressionId>,
}

/// Snapshot-qualified occurrence of an admitted expression. Distinct from
/// the durable [`AdmittedExpressionId`]: offsets belong here, not in the id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionOccurrence {
    /// Durable logical identity.
    pub id: AdmittedExpressionId,
    /// Expression kind.
    pub kind: ExpressionKind,
    /// Authored spelling at this snapshot.
    pub spelling: String,
    /// Inclusive start offset in the snapshot source.
    pub start: u32,
    /// Exclusive end offset in the snapshot source.
    pub end: u32,
    /// Lexical environment the expression was admitted in.
    pub lexical_env: LexicalScopeId,
}

/// A named slot provided by one component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedSlot {
    /// Slot name (`default`, authored static name, or dynamic spelling).
    pub name: String,
    /// Admitted dynamic slot-name expression (`#[name]`), when present.
    pub expression: Option<AdmittedExpressionId>,
    /// Slot contents participate in the inference transaction.
    pub inference_participation: bool,
}

/// One component use observation: logical id plus this snapshot's inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentUse {
    /// Durable logical identity.
    pub id: ComponentUseId,
    /// The component expression (tag or `:is`).
    pub component_expression: AdmittedExpressionId,
    /// Enclosing lexical environment.
    pub lexical_env: LexicalScopeId,
    /// Ordered attribute operations.
    pub operations: Vec<OrderedAttributeOp>,
    /// Slots this use provides.
    pub provided_slots: Vec<ProvidedSlot>,
    /// Hover / definition / references / edits participation.
    pub observation_roles: Vec<ObservationRole>,
    /// Accepted two-binder family members for this use.
    pub generic_binders: Vec<GenericBinderRef>,
    /// Constructor-family expose-value channel stays in the inference transaction.
    pub expose_participation: bool,
}

/// One binding origin observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingOrigin {
    /// Durable origin identity.
    pub id: BindingOriginId,
    /// Binder kind.
    pub kind: BinderKind,
    /// Authored spelling.
    pub name: String,
    /// Scope that introduced the binder.
    pub lexical_env: LexicalScopeId,
}

/// Why a plan is not complete. Incomplete observations cannot warm a
/// complete-plan cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incompleteness {
    /// Parser recorded syntax errors.
    ParseErrors,
    /// Template language the Vue HTML plan does not admit.
    UnknownTemplateLang(String),
    /// An expression required by the plan was not admitted.
    UnadmittedExpression {
        /// Expression kind that failed admission.
        kind: ExpressionKind,
    },
    /// No Vue parse carrier / template was available.
    MissingParse,
    /// Caller source bytes or parse configuration do not match the artifact.
    ParseSnapshotMismatch,
}

/// Completeness of a constructed plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanCompleteness {
    /// Every required syntax/obligation product was admitted.
    Complete,
    /// Explicit incomplete product; must not warm a complete cache.
    Incomplete {
        /// Reasons, in encounter order.
        reasons: Vec<Incompleteness>,
    },
}

/// Syntax/obligation product exposed to CodeTransform. No type answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxObligation {
    /// Owning logical use, when the obligation belongs to one.
    pub use_id: Option<ComponentUseId>,
    /// Obligation kind (emit-facing, not a type query).
    pub kind: ObligationKind,
    /// Admitted expression the obligation names, when any.
    pub expression: Option<AdmittedExpressionId>,
}

/// CodeTransform-facing obligation kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObligationKind {
    /// Emit a component use.
    ComponentUse,
    /// Emit an ordered attribute operation.
    AttributeOp,
    /// Emit a branch edge.
    Branch,
    /// Emit a provided slot.
    Slot,
}

/// Source-backed projection plan for one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionPlan {
    /// Snapshot this observation was built against.
    pub snapshot: PlanSnapshotId,
    /// Established input-basis identity for this snapshot.
    pub input_basis: InputBasisId,
    /// Template source-unit lineage (not a content hash).
    pub template_unit: SourceUnitId,
    /// Completeness; incomplete plans are explicit products.
    pub completeness: PlanCompleteness,
    /// Component uses in source order.
    pub uses: Vec<ComponentUse>,
    /// Binding origins in source order.
    pub origins: Vec<BindingOrigin>,
    /// Branch edges in source order.
    pub branches: Vec<BranchEdge>,
    /// Snapshot-qualified expression occurrences, keyed by durable id.
    expressions: Vec<ExpressionOccurrence>,
    /// Syntax/obligation products for CodeTransform.
    syntax_obligations: Vec<SyntaxObligation>,
}

impl ProjectionPlan {
    /// Syntax/obligation products. Mapping geometry stays with CodeTransform.
    #[must_use]
    pub fn syntax_obligations(&self) -> &[SyntaxObligation] {
        &self.syntax_obligations
    }

    /// Snapshot-qualified occurrences for admitted expression IDs.
    #[must_use]
    pub fn expressions(&self) -> &[ExpressionOccurrence] {
        &self.expressions
    }

    /// Resolve a durable expression id to this snapshot's source occurrence.
    #[must_use]
    pub fn expression(&self, id: &AdmittedExpressionId) -> Option<&ExpressionOccurrence> {
        self.expressions
            .iter()
            .find(|occurrence| occurrence.id == *id)
    }

    /// True when every required product was admitted.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self.completeness, PlanCompleteness::Complete)
    }

    /// Observation fingerprint used by fresh/incremental equality.
    #[must_use]
    pub fn observation_key(&self) -> String {
        let uses: Vec<String> = self.uses.iter().map(|u| u.id.digest_hex()).collect();
        let origins: Vec<String> = self.origins.iter().map(|o| o.id.digest_hex()).collect();
        let branches: Vec<String> = self
            .branches
            .iter()
            .map(|b| {
                let excluded = b
                    .excluded
                    .iter()
                    .map(AdmittedExpressionId::digest_hex)
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{}:{:?}:{}:{}",
                    b.expression.digest_hex(),
                    b.outcome,
                    b.lexical_env.digest_hex(),
                    excluded
                )
            })
            .collect();
        format!(
            "{}|{}|{}|{}|complete={}",
            self.snapshot.digest_hex(),
            uses.join(","),
            origins.join(","),
            branches.join(","),
            self.is_complete()
        )
    }
}

/// Refusal when an incomplete plan is offered to a complete cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompleteCacheRefusal {
    /// The plan is incomplete, cancelled, or configuration-missing.
    Incomplete,
}

/// Complete-plan cache. Incomplete/cancelled observations cannot warm it.
#[derive(Clone, Debug, Default)]
pub struct CompletePlanCache {
    entries: BTreeMap<PlanSnapshotId, ProjectionPlan>,
}

impl CompletePlanCache {
    /// Admit a complete plan. Incomplete products are refused and do not
    /// occupy a complete-cache slot.
    pub fn admit(&mut self, plan: ProjectionPlan) -> Result<&ProjectionPlan, CompleteCacheRefusal> {
        if !plan.is_complete() {
            return Err(CompleteCacheRefusal::Incomplete);
        }
        Ok(self.entries.entry(plan.snapshot.clone()).or_insert(plan))
    }

    /// Lookup by snapshot. Misses are not completeness claims.
    #[must_use]
    pub fn get(&self, snapshot: &PlanSnapshotId) -> Option<&ProjectionPlan> {
        self.entries.get(snapshot)
    }
}

/// Inputs for plan construction. The caller supplies an already-admitted
/// parse; this builder does not reparse generated companion text.
pub struct PlanInput<'a> {
    /// Carrier canonical id (lineage, not a content hash).
    pub canonical_id: &'a str,
    /// Authored SFC bytes for this snapshot. Must match the parse artifact.
    pub source: &'a str,
    /// Admitted parse.
    pub parsed: &'a ParsedSfc,
    /// Parse-key of the bound artifact, when the caller supplied one.
    pub parse_key: Option<&'a ParseKey>,
    /// Syntax profile of the bound artifact, when the caller supplied one.
    pub syntax_profile: Option<&'a SyntaxProfileId>,
}

/// Build a fresh plan from an admitted parse.
#[must_use]
pub fn build_projection_plan(input: PlanInput<'_>) -> ProjectionPlan {
    let source_id = carrier_source_id(input.canonical_id);
    let revision = carrier_revision(input.source);
    let template_unit = SourceUnitId::from_lineage(&source_id, "template");
    let snapshot = mint_snapshot(
        &source_id,
        &revision,
        &template_unit,
        input.parse_key,
        input.syntax_profile,
    );
    let input_basis = InputBasisId::from_canonical(&SnapshotBasis(&snapshot));

    let mut reasons = Vec::new();
    if input.parsed.has_errors() {
        reasons.push(Incompleteness::ParseErrors);
    }
    let Some(ast) = input.parsed.template_ast() else {
        reasons.push(Incompleteness::MissingParse);
        return unfinished_plan(snapshot, input_basis, template_unit, reasons);
    };
    if let Some(lang) = unknown_template_lang(ast, input.source) {
        reasons.push(Incompleteness::UnknownTemplateLang(lang));
    }

    let alloc = oxc_allocator::Allocator::new();
    let oxc = parse_template_expressions(
        ast,
        input.source,
        &alloc,
        oxc_span::SourceType::tsx(),
        false,
    );
    let root_scope = mint_scope(None, "template-root", &[], "", "", "", "", 0);
    let mut builder = PlanBuilder {
        source: input.source,
        ast,
        oxc: &oxc,
        template_unit: template_unit.clone(),
        uses: Vec::new(),
        origins: Vec::new(),
        branches: Vec::new(),
        expressions: Vec::new(),
        obligations: Vec::new(),
        reasons,
        use_counts: FxHashMap::default(),
        expr_counts: FxHashMap::default(),
        scope_counts: FxHashMap::default(),
    };
    let root_children = ast
        .root
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);
    builder.walk_nodes(root_children, &root_scope, &[]);
    builder.finish(snapshot, input_basis, template_unit)
}

/// Incrementally construct a plan for `input`. A complete previous plan
/// for the same snapshot is reused; incomplete previous observations
/// never occupy the complete cache and are rebuilt.
#[must_use]
pub fn build_projection_plan_incremental(
    previous: Option<&ProjectionPlan>,
    input: PlanInput<'_>,
) -> ProjectionPlan {
    let source_id = carrier_source_id(input.canonical_id);
    let revision = carrier_revision(input.source);
    let template_unit = SourceUnitId::from_lineage(&source_id, "template");
    let snapshot = mint_snapshot(
        &source_id,
        &revision,
        &template_unit,
        input.parse_key,
        input.syntax_profile,
    );
    if let Some(prev) = previous {
        if prev.snapshot == snapshot && prev.is_complete() {
            return prev.clone();
        }
    }
    build_projection_plan(input)
}

struct SnapshotBasis<'a>(&'a PlanSnapshotId);
impl CanonicalEncode for SnapshotBasis<'_> {
    const DOMAIN_TAG: &'static str = INPUT_BASIS_DOMAIN;
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_bytes(1, self.0.canonical_bytes());
    }
}

pub(crate) fn mint_snapshot(
    source_id: &verter_identity::identity::SourceId,
    revision: &verter_identity::identity::SourceRevision,
    template_unit: &SourceUnitId,
    parse_key: Option<&ParseKey>,
    syntax_profile: Option<&SyntaxProfileId>,
) -> PlanSnapshotId {
    let mut encoder = CanonicalEncoder::new(SNAPSHOT_DOMAIN);
    encoder.field_bytes(1, source_id.digest().as_bytes());
    encoder.field_bytes(2, revision.digest().as_bytes());
    encoder.field_bytes(3, template_unit.digest().as_bytes());
    encoder.field_option(4, parse_key.map(|key| key.canonical_bytes()));
    encoder.field_option(5, syntax_profile.map(|profile| profile.canonical_bytes()));
    PlanSnapshotId(Canonical::from_encoder(&encoder))
}

#[allow(clippy::too_many_arguments)]
fn mint_scope(
    parent: Option<&LexicalScopeId>,
    kind: &str,
    binders: &[String],
    spelling: &str,
    host: &str,
    path: &str,
    content_sig: &str,
    occurrence: u32,
) -> LexicalScopeId {
    let mut encoder = CanonicalEncoder::new(SCOPE_DOMAIN);
    encoder.field_option(1, parent.map(|p| p.canonical_bytes()));
    encoder.field_str(2, kind);
    encoder.field_str(3, spelling);
    encoder.field_str(4, host);
    encoder.field_str(5, path);
    encoder.field_u32(6, occurrence);
    encoder.field_str(7, content_sig);
    encoder.field_u32(8, binders.len() as u32);
    for (i, name) in binders.iter().enumerate() {
        encoder.field_str(9 + i as u16, name);
    }
    LexicalScopeId(Canonical::from_encoder(&encoder))
}

#[allow(clippy::too_many_arguments)]
fn mint_expression(
    unit: &SourceUnitId,
    env: &LexicalScopeId,
    kind: ExpressionKind,
    spelling: &str,
    path: &str,
    role: &str,
    owner: Option<&ComponentUseId>,
    occurrence: u32,
) -> AdmittedExpressionId {
    let mut encoder = CanonicalEncoder::new(EXPR_DOMAIN);
    encoder.field_bytes(1, unit.digest().as_bytes());
    encoder.field_bytes(2, env.digest().as_bytes());
    encoder.field_str(3, kind.tag());
    encoder.field_str(4, spelling);
    encoder.field_str(5, path);
    encoder.field_str(6, role);
    encoder.field_option(7, owner.map(|use_id| use_id.canonical_bytes()));
    encoder.field_u32(8, occurrence);
    AdmittedExpressionId(Canonical::from_encoder(&encoder))
}

#[allow(clippy::too_many_arguments)]
fn mint_use(
    unit: &SourceUnitId,
    env: &LexicalScopeId,
    kind: ExpressionKind,
    spelling: &str,
    path: &[String],
    ops_sig: &str,
    slots_sig: &str,
    occurrence: u32,
) -> ComponentUseId {
    let mut encoder = CanonicalEncoder::new(USE_DOMAIN);
    encoder.field_bytes(1, unit.digest().as_bytes());
    encoder.field_bytes(2, env.digest().as_bytes());
    encoder.field_str(3, kind.tag());
    encoder.field_str(4, spelling);
    encoder.field_str(5, ops_sig);
    encoder.field_str(6, slots_sig);
    encoder.field_u32(7, occurrence);
    encoder.field_u32(8, path.len() as u32);
    for (i, seg) in path.iter().enumerate() {
        encoder.field_str(9 + i as u16, seg);
    }
    ComponentUseId(Canonical::from_encoder(&encoder))
}

fn mint_origin(
    unit: &SourceUnitId,
    env: &LexicalScopeId,
    kind: BinderKind,
    name: &str,
) -> BindingOriginId {
    let mut encoder = CanonicalEncoder::new(ORIGIN_DOMAIN);
    encoder.field_bytes(1, unit.digest().as_bytes());
    encoder.field_bytes(2, env.digest().as_bytes());
    encoder.field_str(3, kind.tag());
    encoder.field_str(4, name);
    BindingOriginId(Canonical::from_encoder(&encoder))
}

fn mint_binder(use_id: &ComponentUseId, name: &str, ordinal: u8) -> GenericBinderRef {
    let mut encoder = CanonicalEncoder::new(BINDER_DOMAIN);
    encoder.field_bytes(1, use_id.canonical_bytes());
    encoder.field_str(2, name);
    encoder.field_u32(3, u32::from(ordinal));
    GenericBinderRef(Canonical::from_encoder(&encoder))
}

fn unfinished_plan(
    snapshot: PlanSnapshotId,
    input_basis: InputBasisId,
    template_unit: SourceUnitId,
    reasons: Vec<Incompleteness>,
) -> ProjectionPlan {
    ProjectionPlan {
        snapshot,
        input_basis,
        template_unit,
        completeness: PlanCompleteness::Incomplete { reasons },
        uses: Vec::new(),
        origins: Vec::new(),
        branches: Vec::new(),
        expressions: Vec::new(),
        syntax_obligations: Vec::new(),
    }
}

fn unknown_template_lang(ast: &crate::ast::types::TemplateAst, source: &str) -> Option<String> {
    let span = ast.root.lang?;
    if span.start >= span.end {
        return None;
    }
    let lang = source[span.start as usize..span.end as usize]
        .trim()
        .to_ascii_lowercase();
    match lang.as_str() {
        "" | "html" | "vue" => None,
        _ => Some(lang),
    }
}

struct OpenBranch {
    originating: AdmittedExpressionId,
    seen: Vec<AdmittedExpressionId>,
}

#[derive(Hash, Eq, PartialEq)]
struct UseCountKey {
    env: CanonicalDigest,
    spelling: String,
    path: String,
    ops_sig: String,
    slots_sig: String,
}

#[derive(Hash, Eq, PartialEq)]
struct ExprCountKey {
    env: CanonicalDigest,
    kind: &'static str,
    spelling: String,
    path: String,
    role: String,
    owner: Option<CanonicalDigest>,
}

#[derive(Hash, Eq, PartialEq)]
struct ScopeCountKey {
    env: CanonicalDigest,
    kind: String,
    binders: String,
    spelling: String,
    host: String,
    path: String,
    content_sig: String,
}

struct PlanBuilder<'a> {
    source: &'a str,
    ast: &'a crate::ast::types::TemplateAst,
    oxc: &'a OxcParsedAst<'a>,
    template_unit: SourceUnitId,
    uses: Vec<ComponentUse>,
    origins: Vec<BindingOrigin>,
    branches: Vec<BranchEdge>,
    expressions: Vec<ExpressionOccurrence>,
    obligations: Vec<SyntaxObligation>,
    reasons: Vec<Incompleteness>,
    use_counts: FxHashMap<UseCountKey, u32>,
    expr_counts: FxHashMap<ExprCountKey, u32>,
    scope_counts: FxHashMap<ScopeCountKey, u32>,
}

impl<'a> PlanBuilder<'a> {
    fn finish(
        self,
        snapshot: PlanSnapshotId,
        input_basis: InputBasisId,
        template_unit: SourceUnitId,
    ) -> ProjectionPlan {
        let completeness = if self.reasons.is_empty() {
            PlanCompleteness::Complete
        } else {
            PlanCompleteness::Incomplete {
                reasons: self.reasons,
            }
        };
        ProjectionPlan {
            snapshot,
            input_basis,
            template_unit,
            completeness,
            uses: self.uses,
            origins: self.origins,
            branches: self.branches,
            expressions: self.expressions,
            syntax_obligations: self.obligations,
        }
    }

    fn walk_nodes(&mut self, ids: &[NodeId], env: &LexicalScopeId, ancestors: &[String]) {
        let mut chain: Option<OpenBranch> = None;
        for &id in ids {
            match &self.ast.nodes[id.0].kind {
                AstNodeKind::Comment(_) => {}
                AstNodeKind::Text(text) => {
                    if !text.is_whitespace_only {
                        chain = None;
                    }
                }
                AstNodeKind::Interpolation(interp) => {
                    chain = None;
                    self.admit_interpolation(id, interp, env, ancestors);
                }
                AstNodeKind::Element(el) => {
                    self.walk_element(id, el, env, ancestors, &mut chain);
                }
            }
        }
    }

    fn walk_element(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        env: &LexicalScopeId,
        ancestors: &[String],
        chain: &mut Option<OpenBranch>,
    ) {
        let oxc_el = match &self.oxc.data[id.0] {
            OxcNodeData::Element(parsed) => Some(parsed.as_ref()),
            _ => None,
        };
        let host = open_tag_name(&el.tag_open, self.source);
        if has_v_pre(el, self.source) {
            *chain = None;
            return;
        }

        // Vue evaluates same-element v-if before v-for; the condition lives in
        // the parent environment. Loop aliases are not visible to it.
        self.record_condition(el, oxc_el, env, ancestors, host, chain);

        let mut child_env = env.clone();
        if let Some(v_for) = el.v_for.as_ref() {
            child_env = self.push_vfor_scope(el, v_for, oxc_el, env, host, ancestors);
        }

        let component_env = child_env.clone();
        if let Some(v_slot) = el.v_slot.as_ref() {
            child_env = self.push_slot_scope(el, v_slot, &child_env, oxc_el, host, ancestors);
        }

        if el.tag_type.is_component() || is_dynamic_component(el, self.source) {
            self.record_component_use(el, oxc_el, &component_env, ancestors, host);
        } else {
            self.scan_unadmitted_props(el, oxc_el);
        }

        let children = el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(host.to_string());
        self.walk_nodes(children, &child_env, &child_ancestors);
    }

    fn record_condition(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'a>>,
        env: &LexicalScopeId,
        ancestors: &[String],
        host: &str,
        chain: &mut Option<OpenBranch>,
    ) {
        let Some(condition) = el.v_condition.as_ref() else {
            if !el.tag_type.is_template() {
                *chain = None;
            }
            return;
        };
        match condition.kind {
            ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                let spelling = prop_value(self.source, &condition.prop).unwrap_or("");
                let start = condition.prop.value_start.unwrap_or(condition.prop.start);
                let end = condition.prop.value_end.unwrap_or(condition.prop.name_end);
                if spelling.trim().is_empty()
                    || oxc_unadmitted(oxc_el.and_then(|el| el.condition.as_ref()), spelling)
                {
                    self.reasons.push(Incompleteness::UnadmittedExpression {
                        kind: ExpressionKind::BranchCondition,
                    });
                    return;
                }
                let expr = self.admit_expr(
                    env,
                    ExpressionKind::BranchCondition,
                    spelling.trim(),
                    start,
                    end,
                    ancestors,
                    host,
                    "branch",
                    None,
                );
                let excluded = match condition.kind {
                    ElementNodeConditionKind::If => {
                        *chain = Some(OpenBranch {
                            originating: expr.clone(),
                            seen: vec![expr.clone()],
                        });
                        Vec::new()
                    }
                    ElementNodeConditionKind::ElseIf => {
                        let excluded = chain
                            .as_ref()
                            .map(|open| open.seen.clone())
                            .unwrap_or_default();
                        if let Some(open) = chain.as_mut() {
                            open.seen.push(expr.clone());
                        } else {
                            *chain = Some(OpenBranch {
                                originating: expr.clone(),
                                seen: vec![expr.clone()],
                            });
                        }
                        excluded
                    }
                    ElementNodeConditionKind::Else => Vec::new(),
                };
                self.push_branch(expr, BranchOutcome::Taken, env, excluded);
            }
            ElementNodeConditionKind::Else => {
                if let Some(open) = chain.as_ref() {
                    let excluded = open.seen.clone();
                    self.push_branch(open.originating.clone(), BranchOutcome::Else, env, excluded);
                }
            }
        }
    }

    fn push_branch(
        &mut self,
        expression: AdmittedExpressionId,
        outcome: BranchOutcome,
        env: &LexicalScopeId,
        excluded: Vec<AdmittedExpressionId>,
    ) {
        self.obligations.push(SyntaxObligation {
            use_id: None,
            kind: ObligationKind::Branch,
            expression: Some(expression.clone()),
        });
        self.branches.push(BranchEdge {
            expression,
            outcome,
            lexical_env: env.clone(),
            excluded,
        });
    }

    fn push_vfor_scope(
        &mut self,
        el: &ElementNode,
        v_for: &NodeProp,
        oxc_el: Option<&OxcParsedElement<'a>>,
        env: &LexicalScopeId,
        host: &str,
        ancestors: &[String],
    ) -> LexicalScopeId {
        let Some(value) = prop_value(self.source, v_for) else {
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::VForSource,
            });
            return env.clone();
        };
        let Some(parsed) = oxc_el.and_then(|el| el.v_for.as_ref()) else {
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::VForSource,
            });
            return env.clone();
        };
        if !parsed.parsed.result.is_ok() {
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::VForSource,
            });
            return env.clone();
        }
        let start = v_for.value_start.unwrap_or(v_for.start);
        let end = v_for.value_end.unwrap_or(v_for.name_end);
        let names: Vec<String> = parsed
            .parsed
            .locals
            .iter()
            .map(|span| self.source[span.start as usize..span.end as usize].to_string())
            .filter(|n| !n.is_empty())
            .collect();
        let (source_spelling, src_start, src_end) = parsed
            .parsed
            .result
            .right
            .as_ref()
            .map(|_| {
                let right_start = parsed.parsed.result.right_offset;
                let right_end = end;
                if (right_start as usize) < (right_end as usize)
                    && (right_end as usize) <= self.source.len()
                {
                    (
                        self.source[right_start as usize..right_end as usize]
                            .trim()
                            .to_string(),
                        right_start,
                        right_end,
                    )
                } else {
                    (value.trim().to_string(), start, end)
                }
            })
            .unwrap_or_else(|| (value.trim().to_string(), start, end));
        let _ = self.admit_expr(
            env,
            ExpressionKind::VForSource,
            &source_spelling,
            src_start,
            src_end,
            ancestors,
            host,
            "v-for",
            None,
        );
        let spelling = value.trim();
        let content_sig = scope_content_sig(el, self.ast, self.source);
        let occurrence = self.next_scope_occurrence(
            env,
            "v-for",
            &names,
            spelling,
            host,
            ancestors,
            &content_sig,
        );
        let path = ancestors.join("/");
        let scope = mint_scope(
            Some(env),
            "v-for",
            &names,
            spelling,
            host,
            &path,
            &content_sig,
            occurrence,
        );
        for name in &names {
            let id = mint_origin(&self.template_unit, &scope, BinderKind::VForAlias, name);
            self.origins.push(BindingOrigin {
                id,
                kind: BinderKind::VForAlias,
                name: name.clone(),
                lexical_env: scope.clone(),
            });
        }
        scope
    }

    fn push_slot_scope(
        &mut self,
        el: &ElementNode,
        v_slot: &NodeProp,
        env: &LexicalScopeId,
        oxc_el: Option<&OxcParsedElement<'a>>,
        host: &str,
        ancestors: &[String],
    ) -> LexicalScopeId {
        let val = prop_value(self.source, v_slot);
        let parsed = oxc_el.and_then(|el| el.v_slot.as_ref());
        if v_slot.is_dynamic == Some(true) {
            match parsed.and_then(|slot| slot.dynamic_name.as_ref()) {
                Some(name)
                    if !oxc_unadmitted(Some(name), slot_name_parse_slice(self.source, v_slot)) => {}
                _ => {
                    self.reasons.push(Incompleteness::UnadmittedExpression {
                        kind: ExpressionKind::AttributeValue,
                    });
                    return env.clone();
                }
            }
        }
        if let Some(val) = val {
            if !val.trim().is_empty() {
                match parsed {
                    Some(slot) if slot.parsed.result.is_ok() => {}
                    _ => {
                        self.reasons.push(Incompleteness::UnadmittedExpression {
                            kind: ExpressionKind::SlotParams,
                        });
                        return env.clone();
                    }
                }
                let start = v_slot.value_start.unwrap_or(v_slot.start);
                let end = v_slot.value_end.unwrap_or(v_slot.name_end);
                let _ = self.admit_expr(
                    env,
                    ExpressionKind::SlotParams,
                    val.trim(),
                    start,
                    end,
                    ancestors,
                    host,
                    "slot-params",
                    None,
                );
            }
        }
        let names = slot_local_names(self.source, parsed);
        let spelling = val.map(str::trim).unwrap_or("");
        let content_sig = scope_content_sig(el, self.ast, self.source);
        let occurrence = self.next_scope_occurrence(
            env,
            "v-slot",
            &names,
            spelling,
            host,
            ancestors,
            &content_sig,
        );
        let path = ancestors.join("/");
        let scope = mint_scope(
            Some(env),
            "v-slot",
            &names,
            spelling,
            host,
            &path,
            &content_sig,
            occurrence,
        );
        for name in &names {
            let id = mint_origin(&self.template_unit, &scope, BinderKind::SlotProp, name);
            self.origins.push(BindingOrigin {
                id,
                kind: BinderKind::SlotProp,
                name: name.clone(),
                lexical_env: scope.clone(),
            });
        }
        scope
    }

    #[allow(clippy::too_many_arguments)]
    fn next_scope_occurrence(
        &mut self,
        env: &LexicalScopeId,
        kind: &str,
        binders: &[String],
        spelling: &str,
        host: &str,
        ancestors: &[String],
        content_sig: &str,
    ) -> u32 {
        let key = ScopeCountKey {
            env: env.digest(),
            kind: kind.to_string(),
            binders: binders.join("\0"),
            spelling: spelling.to_string(),
            host: host.to_string(),
            path: ancestors.join("/"),
            content_sig: content_sig.to_string(),
        };
        let slot = self.scope_counts.entry(key).or_insert(0);
        let n = *slot;
        *slot += 1;
        n
    }

    fn record_component_use(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'a>>,
        env: &LexicalScopeId,
        ancestors: &[String],
        host: &str,
    ) {
        let (kind, spelling, expr_start, expr_end) = self.component_expression(el, oxc_el);
        let drafts = self.collect_op_drafts(el, oxc_el);
        let ops_sig = op_signature(&drafts);
        let slot_drafts = slot_drafts(el, self.ast, self.source);
        let slots_sig = slot_drafts
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let path_key = ancestors.join("/");
        let occurrence = {
            let slot = self
                .use_counts
                .entry(UseCountKey {
                    env: env.digest(),
                    spelling: spelling.clone(),
                    path: path_key,
                    ops_sig: ops_sig.clone(),
                    slots_sig: slots_sig.clone(),
                })
                .or_insert(0);
            let n = *slot;
            *slot += 1;
            n
        };
        let use_id = mint_use(
            &self.template_unit,
            env,
            kind,
            &spelling,
            ancestors,
            &ops_sig,
            &slots_sig,
            occurrence,
        );
        let expr = self.admit_expr(
            env,
            kind,
            &spelling,
            expr_start,
            expr_end,
            ancestors,
            host,
            &format!("{}:{ops_sig}", kind.tag()),
            Some(&use_id),
        );
        let operations = self.materialize_ops(drafts, env, ancestors, host, &use_id);
        let provided_slots =
            self.materialize_slots(slot_drafts, oxc_el, env, ancestors, host, &use_id);
        let generic_binders = vec![mint_binder(&use_id, "T", 0), mint_binder(&use_id, "U", 1)];
        self.obligations.push(SyntaxObligation {
            use_id: Some(use_id.clone()),
            kind: ObligationKind::ComponentUse,
            expression: Some(expr.clone()),
        });
        for op in &operations {
            self.obligations.push(SyntaxObligation {
                use_id: Some(use_id.clone()),
                kind: ObligationKind::AttributeOp,
                expression: op.expression.clone(),
            });
            if let Some(argument) = op.argument.as_ref() {
                self.obligations.push(SyntaxObligation {
                    use_id: Some(use_id.clone()),
                    kind: ObligationKind::AttributeOp,
                    expression: Some(argument.clone()),
                });
            }
        }
        for slot in &provided_slots {
            self.obligations.push(SyntaxObligation {
                use_id: Some(use_id.clone()),
                kind: ObligationKind::Slot,
                expression: slot.expression.clone(),
            });
        }
        self.uses.push(ComponentUse {
            id: use_id,
            component_expression: expr,
            lexical_env: env.clone(),
            operations,
            provided_slots,
            observation_roles: vec![
                ObservationRole::Hover,
                ObservationRole::Definition,
                ObservationRole::References,
                ObservationRole::Edits,
            ],
            generic_binders,
            expose_participation: true,
        });
    }

    fn component_expression(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'a>>,
    ) -> (ExpressionKind, String, u32, u32) {
        let (tag_start, tag_end, tag_name) = open_tag_name_span(&el.tag_open, self.source);
        if is_dynamic_component(el, self.source) {
            if let Some((index, is_prop)) = el
                .props
                .iter()
                .enumerate()
                .find(|(_, p)| is_is_attr(p, self.source))
            {
                if is_prop.is_directive {
                    if let Some(val) = prop_value(self.source, is_prop) {
                        let parsed = oxc_el.and_then(|parsed| parsed.prop(index));
                        if oxc_unadmitted(parsed.and_then(|p| p.exp.as_ref()), val)
                            || val.trim().is_empty()
                        {
                            self.reasons.push(Incompleteness::UnadmittedExpression {
                                kind: ExpressionKind::ComponentIs,
                            });
                        }
                        let start = is_prop.value_start.unwrap_or(is_prop.start);
                        let end = is_prop.value_end.unwrap_or(is_prop.name_end);
                        return (
                            ExpressionKind::ComponentIs,
                            val.trim().to_string(),
                            start,
                            end,
                        );
                    }
                    if let Some(arg) = arg_name(is_prop, self.source) {
                        let start = is_prop.arg_start.unwrap_or(is_prop.start);
                        let end = is_prop.arg_end.unwrap_or(is_prop.name_end);
                        return (ExpressionKind::ComponentIs, kebab_to_camel(arg), start, end);
                    }
                } else if let Some(val) = prop_value(self.source, is_prop) {
                    let start = is_prop.value_start.unwrap_or(is_prop.start);
                    let end = is_prop.value_end.unwrap_or(is_prop.name_end);
                    return (
                        ExpressionKind::ComponentTag,
                        val.trim().to_string(),
                        start,
                        end,
                    );
                }
            }
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::ComponentIs,
            });
            return (
                ExpressionKind::ComponentIs,
                tag_name.to_string(),
                tag_start,
                tag_end,
            );
        }
        (
            ExpressionKind::ComponentTag,
            tag_name.to_string(),
            tag_start,
            tag_end,
        )
    }

    fn scan_unadmitted_props(&mut self, el: &ElementNode, oxc_el: Option<&OxcParsedElement<'a>>) {
        for (index, prop) in el.props.iter().enumerate() {
            if classify_op(prop, self.source).is_none() {
                continue;
            }
            let parsed = oxc_el.and_then(|el| el.prop(index));
            let value = prop_value(self.source, prop).unwrap_or("");
            let arg = arg_parse_slice(self.source, prop);
            if oxc_unadmitted(parsed.and_then(|p| p.exp.as_ref()), value)
                || oxc_unadmitted(parsed.and_then(|p| p.arg.as_ref()), arg)
            {
                self.reasons.push(Incompleteness::UnadmittedExpression {
                    kind: ExpressionKind::AttributeValue,
                });
            }
        }
    }

    fn collect_op_drafts(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'a>>,
    ) -> Vec<OpDraft> {
        let mut drafts = Vec::new();
        let mut dense = 0u32;
        for (index, prop) in el.props.iter().enumerate() {
            let Some((kind, name)) = classify_op(prop, self.source) else {
                continue;
            };
            let parsed = oxc_el.and_then(|el| el.prop(index));
            let raw_value = prop_value(self.source, prop);
            let arg_inner = dynamic_arg_inner(self.source, prop);
            let arg_unadmitted = oxc_unadmitted(
                parsed.and_then(|p| p.arg.as_ref()),
                arg_parse_slice(self.source, prop),
            );
            let exp_unadmitted =
                oxc_unadmitted(parsed.and_then(|p| p.exp.as_ref()), raw_value.unwrap_or(""));
            if arg_unadmitted || exp_unadmitted {
                self.reasons.push(Incompleteness::UnadmittedExpression {
                    kind: ExpressionKind::AttributeValue,
                });
            }
            let value = if kind == AttributeOpKind::Static {
                raw_value.map(|val| {
                    (
                        val.to_string(),
                        prop.value_start.unwrap_or(prop.start),
                        prop.value_end.unwrap_or(prop.name_end),
                    )
                })
            } else if let Some(val) = raw_value {
                Some((
                    val.trim().to_string(),
                    prop.value_start.unwrap_or(prop.start),
                    prop.value_end.unwrap_or(prop.name_end),
                ))
            } else {
                shorthand_value(prop, self.source, kind)
            };
            drafts.push(OpDraft {
                index: dense,
                kind,
                name,
                value_spelling: value.as_ref().map(|(s, _, _)| s.clone()),
                value_start: value.as_ref().map(|(_, s, _)| *s).unwrap_or(0),
                value_end: value.as_ref().map(|(_, _, e)| *e).unwrap_or(0),
                arg_spelling: arg_inner.map(|(s, _, _)| s.to_string()),
                arg_start: arg_inner.map(|(_, s, _)| s).unwrap_or(0),
                arg_end: arg_inner.map(|(_, _, e)| e).unwrap_or(0),
                modifiers: modifier_names(prop, self.source),
                unadmitted: arg_unadmitted || exp_unadmitted,
            });
            dense += 1;
        }
        drafts
    }

    fn materialize_ops(
        &mut self,
        drafts: Vec<OpDraft>,
        env: &LexicalScopeId,
        ancestors: &[String],
        host: &str,
        owner: &ComponentUseId,
    ) -> Vec<OrderedAttributeOp> {
        let mut ops = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let argument = if !draft.unadmitted {
                draft.arg_spelling.as_ref().map(|spelling| {
                    self.admit_expr(
                        env,
                        ExpressionKind::AttributeValue,
                        spelling,
                        draft.arg_start,
                        draft.arg_end,
                        ancestors,
                        host,
                        &format!("arg:{}", draft.name),
                        Some(owner),
                    )
                })
            } else {
                None
            };
            let expression = if !draft.unadmitted {
                draft.value_spelling.as_ref().map(|spelling| {
                    self.admit_expr(
                        env,
                        ExpressionKind::AttributeValue,
                        spelling,
                        draft.value_start,
                        draft.value_end,
                        ancestors,
                        host,
                        &format!("value:{}", draft.name),
                        Some(owner),
                    )
                })
            } else {
                None
            };
            let participates = op_participates(draft.kind, expression.is_some());
            ops.push(OrderedAttributeOp {
                index: draft.index,
                kind: draft.kind,
                name: draft.name,
                expression,
                argument,
                modifiers: draft.modifiers,
                inference_participation: participates,
            });
        }
        ops
    }

    fn materialize_slots(
        &mut self,
        drafts: Vec<SlotDraft>,
        host_oxc: Option<&OxcParsedElement<'a>>,
        env: &LexicalScopeId,
        ancestors: &[String],
        host: &str,
        owner: &ComponentUseId,
    ) -> Vec<ProvidedSlot> {
        let mut slots = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let expression = if let Some(prop) = draft.prop.as_ref() {
                if prop.is_dynamic == Some(true) {
                    if let Some((spelling, start, end)) = dynamic_arg_inner(self.source, prop) {
                        let oxc_el = draft
                            .child_id
                            .and_then(|id| match &self.oxc.data[id.0] {
                                OxcNodeData::Element(parsed) => Some(parsed.as_ref()),
                                _ => None,
                            })
                            .or(host_oxc);
                        let parsed = oxc_el
                            .and_then(|el| el.v_slot.as_ref())
                            .and_then(|slot| slot.dynamic_name.as_ref());
                        if oxc_unadmitted(parsed, slot_name_parse_slice(self.source, prop)) {
                            self.reasons.push(Incompleteness::UnadmittedExpression {
                                kind: ExpressionKind::AttributeValue,
                            });
                            None
                        } else {
                            Some(self.admit_expr(
                                env,
                                ExpressionKind::AttributeValue,
                                spelling,
                                start,
                                end,
                                ancestors,
                                host,
                                &format!("slot-name:{}", draft.name),
                                Some(owner),
                            ))
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            slots.push(ProvidedSlot {
                name: draft.name,
                expression,
                inference_participation: true,
            });
        }
        slots
    }

    fn admit_interpolation(
        &mut self,
        id: NodeId,
        interp: &InterpolationNode,
        env: &LexicalScopeId,
        ancestors: &[String],
    ) {
        if interp.inner_start >= interp.inner_end {
            return;
        }
        let raw = &self.source[interp.inner_start as usize..interp.inner_end as usize];
        let spelling = raw.trim();
        if spelling.is_empty() {
            return;
        }
        let parsed = match &self.oxc.data[id.0] {
            OxcNodeData::Interpolation(expr) => Some(expr),
            _ => None,
        };
        if oxc_unadmitted(parsed, raw) {
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::Interpolation,
            });
            return;
        }
        let _ = self.admit_expr(
            env,
            ExpressionKind::Interpolation,
            spelling,
            interp.inner_start,
            interp.inner_end,
            ancestors,
            "",
            "interpolation",
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn admit_expr(
        &mut self,
        env: &LexicalScopeId,
        kind: ExpressionKind,
        spelling: &str,
        start: u32,
        end: u32,
        ancestors: &[String],
        host: &str,
        role: &str,
        owner: Option<&ComponentUseId>,
    ) -> AdmittedExpressionId {
        let path = expr_path(ancestors, host);
        let occurrence = {
            let slot = self
                .expr_counts
                .entry(ExprCountKey {
                    env: env.digest(),
                    kind: kind.tag(),
                    spelling: spelling.to_string(),
                    path: path.clone(),
                    role: role.to_string(),
                    owner: owner.map(ComponentUseId::digest),
                })
                .or_insert(0);
            let n = *slot;
            *slot += 1;
            n
        };
        let id = mint_expression(
            &self.template_unit,
            env,
            kind,
            spelling,
            &path,
            role,
            owner,
            occurrence,
        );
        self.expressions.push(ExpressionOccurrence {
            id: id.clone(),
            kind,
            spelling: spelling.to_string(),
            start,
            end,
            lexical_env: env.clone(),
        });
        id
    }
}

fn oxc_unadmitted(expr: Option<&OxcParsedExpression<'_>>, authored: &str) -> bool {
    match expr {
        Some(parsed) => {
            if parsed.errors.is_some() {
                return true;
            }
            if parsed.multi_statement {
                return false;
            }
            let Some(expression) = parsed.expression.as_ref() else {
                return true;
            };
            let end = expression.span().end as usize;
            authored.get(end..).is_none_or(|tail| {
                !tail
                    .trim_matches(|c: char| c.is_whitespace() || c == ';')
                    .is_empty()
            })
        }
        None => false,
    }
}

fn is_dynamic_component(el: &ElementNode, source: &str) -> bool {
    open_tag_name(&el.tag_open, source).eq_ignore_ascii_case("component")
}

fn is_is_attr(prop: &NodeProp, source: &str) -> bool {
    if !prop.is_directive {
        return source.get(prop.start as usize..prop.name_end as usize) == Some("is");
    }
    if get_directive_name(prop, source) != "bind" {
        return false;
    }
    match (prop.arg_start, prop.arg_end) {
        (Some(s), Some(e)) if s < e => source[s as usize..e as usize] == *"is",
        _ => false,
    }
}

fn open_tag_name_span<'a>(tag: &NodeTag, source: &'a str) -> (u32, u32, &'a str) {
    let bytes = source.as_bytes();
    let mut i = tag.start as usize + 1;
    let end = tag.name_end as usize;
    if i < end && bytes.get(i) == Some(&b'/') {
        i += 1;
    }
    if i > end {
        return (tag.name_end, tag.name_end, "");
    }
    (i as u32, tag.name_end, &source[i..end])
}

fn open_tag_name<'a>(tag: &NodeTag, source: &'a str) -> &'a str {
    open_tag_name_span(tag, source).2
}

fn prop_value<'a>(source: &'a str, prop: &NodeProp) -> Option<&'a str> {
    let s = prop.value_start?;
    let e = prop.value_end?;
    if s > e || (e as usize) > source.len() {
        return None;
    }
    Some(&source[s as usize..e as usize])
}

fn classify_op(prop: &NodeProp, source: &str) -> Option<(AttributeOpKind, String)> {
    if !prop.is_directive {
        let name = &source[prop.start as usize..prop.name_end as usize];
        return Some((AttributeOpKind::Static, name.to_string()));
    }
    let dir = get_directive_name(prop, source);
    match dir {
        "if" | "else-if" | "else" | "for" | "slot" | "once" | "pre" => None,
        "on" => match arg_name(prop, source) {
            Some(arg) => Some((AttributeOpKind::VOn, event_to_jsx_name(arg))),
            None => Some((AttributeOpKind::VOn, "v-on".to_string())),
        },
        "model" => Some((
            AttributeOpKind::Model,
            arg_name(prop, source).unwrap_or("modelValue").to_string(),
        )),
        "bind" => {
            if let Some(arg) = arg_name(prop, source) {
                Some((AttributeOpKind::Bound, arg.to_string()))
            } else {
                Some((AttributeOpKind::VBind, "v-bind".to_string()))
            }
        }
        other => Some((AttributeOpKind::Directive, other.to_string())),
    }
}

fn op_participates(kind: AttributeOpKind, has_value: bool) -> bool {
    match kind {
        AttributeOpKind::Static => has_value,
        _ => true,
    }
}

fn kebab_to_camel(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper = false;
    for c in input.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn arg_parse_slice<'a>(source: &'a str, prop: &NodeProp) -> &'a str {
    match (prop.arg_start, prop.arg_end) {
        (Some(start), Some(end)) if start < end && (end as usize) <= source.len() => {
            &source[start as usize..end as usize]
        }
        _ => "",
    }
}

fn slot_name_parse_slice<'a>(source: &'a str, prop: &NodeProp) -> &'a str {
    let raw = arg_parse_slice(source, prop);
    if raw.starts_with('[') && raw.ends_with(']') && raw.len() >= 2 {
        &raw[1..raw.len() - 1]
    } else {
        raw
    }
}

fn dynamic_arg_inner<'a>(source: &'a str, prop: &NodeProp) -> Option<(&'a str, u32, u32)> {
    if prop.is_dynamic != Some(true) {
        return None;
    }
    let start = prop.arg_start?;
    let end = prop.arg_end?;
    if start >= end || (end as usize) > source.len() {
        return None;
    }
    let raw = &source[start as usize..end as usize];
    let (inner_start, inner_end) = if raw.starts_with('[') && raw.ends_with(']') && raw.len() >= 2 {
        (start + 1, end - 1)
    } else {
        (start, end)
    };
    if inner_start >= inner_end {
        return None;
    }
    Some((
        source[inner_start as usize..inner_end as usize].trim(),
        inner_start,
        inner_end,
    ))
}

fn shorthand_value(
    prop: &NodeProp,
    source: &str,
    kind: AttributeOpKind,
) -> Option<(String, u32, u32)> {
    if !matches!(kind, AttributeOpKind::Bound | AttributeOpKind::Model) {
        return None;
    }
    if prop.is_dynamic == Some(true) {
        return None;
    }
    let arg = arg_name(prop, source)?;
    let start = prop.arg_start?;
    let end = prop.arg_end?;
    Some((kebab_to_camel(arg), start, end))
}

fn expr_path(ancestors: &[String], host: &str) -> String {
    if host.is_empty() {
        ancestors.join("/")
    } else if ancestors.is_empty() {
        host.to_string()
    } else {
        format!("{}/{host}", ancestors.join("/"))
    }
}

fn op_signature(ops: &[OpDraft]) -> String {
    ops.iter()
        .map(|op| {
            let value = op.value_spelling.as_deref().unwrap_or("");
            let argument = op.arg_spelling.as_deref().unwrap_or("");
            let modifiers = op.modifiers.join(".");
            format!("{:?}:{}:{value}:{argument}:{modifiers}", op.kind, op.name)
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn modifier_names(prop: &NodeProp, source: &str) -> Vec<String> {
    prop.modifiers
        .iter()
        .filter(|span| span.start < span.end && (span.end as usize) <= source.len())
        .map(|span| source[span.start as usize..span.end as usize].to_string())
        .collect()
}

fn has_v_pre(el: &ElementNode, source: &str) -> bool {
    el.props
        .iter()
        .any(|prop| prop.is_directive && get_directive_name(prop, source) == "pre")
}

fn authored_ops_sig(el: &ElementNode, source: &str) -> String {
    el.props
        .iter()
        .filter_map(|prop| {
            let (kind, name) = classify_op(prop, source)?;
            let value = if kind == AttributeOpKind::Static {
                prop_value(source, prop).unwrap_or("")
            } else {
                prop_value(source, prop).map(str::trim).unwrap_or("")
            };
            let argument = dynamic_arg_inner(source, prop)
                .map(|(spelling, _, _)| spelling)
                .unwrap_or("");
            let modifiers = modifier_names(prop, source).join(".");
            Some(format!("{kind:?}:{name}:{value}:{argument}:{modifiers}"))
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn scope_content_sig(
    el: &ElementNode,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
) -> String {
    let mut parts = vec![authored_ops_sig(el, source)];
    push_descendant_use_sigs(el, ast, source, &mut parts);
    parts.join(";")
}

fn push_descendant_use_sigs(
    el: &ElementNode,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
    parts: &mut Vec<String>,
) {
    if el.tag_type.is_component() || is_dynamic_component(el, source) {
        let tag = open_tag_name(&el.tag_open, source);
        let slots = slot_drafts(el, ast, source)
            .into_iter()
            .map(|slot| slot.name)
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!("{tag}|{}|{slots}", authored_ops_sig(el, source)));
    }
    let Some(content) = el.content.as_ref() else {
        return;
    };
    for &child_id in &content.children {
        if let AstNodeKind::Element(child) = &ast.nodes[child_id.0].kind {
            push_descendant_use_sigs(child, ast, source, parts);
        }
    }
}

fn arg_name<'a>(prop: &NodeProp, source: &'a str) -> Option<&'a str> {
    let s = prop.arg_start?;
    let e = prop.arg_end?;
    if s >= e {
        return None;
    }
    Some(&source[s as usize..e as usize])
}

fn slot_local_names(
    source: &str,
    parsed: Option<&crate::template::oxc::types::OxcParsedVSlot<'_>>,
) -> Vec<String> {
    let Some(parsed) = parsed else {
        return Vec::new();
    };
    parsed
        .parsed
        .locals
        .iter()
        .map(|span| source[span.start as usize..span.end as usize].to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

struct OpDraft {
    index: u32,
    kind: AttributeOpKind,
    name: String,
    value_spelling: Option<String>,
    value_start: u32,
    value_end: u32,
    arg_spelling: Option<String>,
    arg_start: u32,
    arg_end: u32,
    modifiers: Vec<String>,
    unadmitted: bool,
}

struct SlotDraft {
    name: String,
    prop: Option<NodeProp>,
    child_id: Option<NodeId>,
}

fn slot_drafts(
    el: &ElementNode,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
) -> Vec<SlotDraft> {
    let mut slots = Vec::new();
    if let Some(v_slot) = el.v_slot.as_ref() {
        slots.push(SlotDraft {
            name: arg_name(v_slot, source).unwrap_or("default").to_string(),
            prop: Some(v_slot.clone()),
            child_id: None,
        });
        // Component-level slot content belongs to that slot; it is not also
        // implicit default content.
        return slots;
    }
    let Some(content) = el.content.as_ref() else {
        return slots;
    };
    let mut has_default_child = false;
    for &child_id in &content.children {
        match &ast.nodes[child_id.0].kind {
            AstNodeKind::Interpolation(_) => has_default_child = true,
            AstNodeKind::Text(text) if !text.is_whitespace_only => has_default_child = true,
            AstNodeKind::Element(child) if child.tag_type.is_template() => {
                if let Some(v_slot) = child.v_slot.as_ref() {
                    let name = arg_name(v_slot, source).unwrap_or("default").to_string();
                    if !slots.iter().any(|s| s.name == name) {
                        slots.push(SlotDraft {
                            name,
                            prop: Some(v_slot.clone()),
                            child_id: Some(child_id),
                        });
                    }
                } else {
                    has_default_child = true;
                }
            }
            AstNodeKind::Element(_) => has_default_child = true,
            _ => {}
        }
    }
    if has_default_child && !slots.iter().any(|s| s.name == "default") {
        slots.push(SlotDraft {
            name: "default".to_string(),
            prop: None,
            child_id: None,
        });
    }
    slots
}

/// Parse helper for crate tests and the Vue projection backend.
pub fn plan_from_source(canonical_id: &str, source: &str) -> ProjectionPlan {
    let parsed = crate::compile::parse_sfc(source, None, None);
    build_projection_plan(PlanInput {
        canonical_id,
        source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    })
}

fn snapshot_without_parse(
    canonical_id: &str,
    source: &str,
) -> (PlanSnapshotId, InputBasisId, SourceUnitId) {
    let source_id = carrier_source_id(canonical_id);
    let revision = carrier_revision(source);
    let template_unit = SourceUnitId::from_lineage(&source_id, "template");
    let snapshot = mint_snapshot(&source_id, &revision, &template_unit, None, None);
    let input_basis = InputBasisId::from_canonical(&SnapshotBasis(&snapshot));
    (snapshot, input_basis, template_unit)
}

/// Explicit incomplete product when no Vue parse carrier is available.
#[must_use]
pub fn incomplete_missing_parse(canonical_id: &str, source: &str) -> ProjectionPlan {
    let (snapshot, input_basis, template_unit) = snapshot_without_parse(canonical_id, source);
    unfinished_plan(
        snapshot,
        input_basis,
        template_unit,
        vec![Incompleteness::MissingParse],
    )
}

/// Explicit incomplete product when caller source/profile does not bind the artifact.
#[must_use]
pub fn incomplete_parse_snapshot_mismatch(canonical_id: &str, source: &str) -> ProjectionPlan {
    let (snapshot, input_basis, template_unit) = snapshot_without_parse(canonical_id, source);
    unfinished_plan(
        snapshot,
        input_basis,
        template_unit,
        vec![Incompleteness::ParseSnapshotMismatch],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS_BASE: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const x = 1;\n",
        "</script>\n",
        "<template>\n",
        "  <Foo :bar=\"x\" />\n",
        "  <Bar />\n",
        "</template>\n",
    );
    const IDS_COMMENT: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const x = 1;\n",
        "</script>\n",
        "<template>\n",
        "  <!-- note -->\n",
        "  <Foo :bar=\"x\" />\n",
        "  <Bar />\n",
        "</template>\n",
    );
    const IDS_SIBLING: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const x = 1;\n",
        "</script>\n",
        "<template>\n",
        "  <Foo :bar=\"x\" />\n",
        "  <Baz />\n",
        "  <Bar />\n",
        "</template>\n",
    );
    const SHADOW: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const items = [1];\n",
        "</script>\n",
        "<template>\n",
        "  <div v-for=\"item in items\" :key=\"item\">\n",
        "    <Child v-slot=\"{ item }\">{{ item }}</Child>\n",
        "  </div>\n",
        "</template>\n",
    );
    const MALFORMED: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const items = [1];\n",
        "</script>\n",
        "<template>\n",
        "  <div v-for=\"item in\" />\n",
        "</template>\n",
    );
    const PUG: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const x = 1;\n",
        "</script>\n",
        "<template lang=\"pug\">\n",
        "Foo\n",
        "</template>\n",
    );

    fn use_hexes(plan: &ProjectionPlan) -> Vec<String> {
        plan.uses.iter().map(|u| u.id.digest_hex()).collect()
    }

    #[test]
    fn stp9_ids_comment_and_unrelated_sibling_preserve_use_identities() {
        let base = plan_from_source("file:///ids.vue", IDS_BASE);
        let commented = plan_from_source("file:///ids.vue", IDS_COMMENT);
        let sibling = plan_from_source("file:///ids.vue", IDS_SIBLING);
        assert!(base.is_complete(), "{:?}", base.completeness);
        assert!(commented.is_complete(), "{:?}", commented.completeness);
        assert!(sibling.is_complete(), "{:?}", sibling.completeness);
        assert_eq!(base.uses.len(), 2, "{:?}", use_hexes(&base));
        assert_eq!(sibling.uses.len(), 3, "{:?}", use_hexes(&sibling));
        let base_foo = &base.uses[0].id;
        let base_bar = &base.uses[1].id;
        assert_eq!(base_foo, &commented.uses[0].id);
        assert_eq!(base_bar, &commented.uses[1].id);
        assert_eq!(base_foo, &sibling.uses[0].id);
        assert_eq!(base_bar, &sibling.uses[2].id);
        assert_ne!(&sibling.uses[1].id, base_foo);
        assert_ne!(&sibling.uses[1].id, base_bar);
        assert_ne!(base_foo, base_bar);
    }

    #[test]
    fn stp9_shadow_nested_slot_and_loop_origins_are_distinct() {
        let plan = plan_from_source("file:///shadow.vue", SHADOW);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let items: Vec<&BindingOrigin> = plan.origins.iter().filter(|o| o.name == "item").collect();
        assert_eq!(items.len(), 2, "{:?}", plan.origins);
        assert_ne!(items[0].id, items[1].id);
        assert_ne!(items[0].lexical_env, items[1].lexical_env);
        assert_ne!(items[0].kind, items[1].kind);
        assert!(items.iter().any(|o| o.kind == BinderKind::VForAlias));
        assert!(items.iter().any(|o| o.kind == BinderKind::SlotProp));
        assert_eq!(plan.uses.len(), 1);
        assert_eq!(plan.uses[0].generic_binders.len(), 2);
        assert!(!plan.uses[0].observation_roles.is_empty());
        assert!(plan
            .uses
            .iter()
            .any(|u| u.provided_slots.iter().any(|s| s.name == "default")));
    }

    #[test]
    fn stp9_type_free_plan_module_does_not_call_typeinfo() {
        let src = include_str!("mod.rs");
        let production = src
            .split("#[cfg(test)]")
            .next()
            .expect("production portion");
        for needle in [
            "TypeInfo",
            "TypeInfoCore",
            "CompileTypeInfo",
            "assignability",
            "Assignable",
            "RelationKind",
            "type_info::",
        ] {
            assert!(
                !production.contains(needle),
                "plan construction must not call {needle}"
            );
        }
        for path in crate_paths(production) {
            assert!(
                crate_path_allowed(&path),
                "plan construction must not reach {path}"
            );
        }
        let input = PlanInput {
            canonical_id: "file:///ids.vue",
            source: IDS_BASE,
            parsed: &crate::compile::parse_sfc(IDS_BASE, None, None),
            parse_key: None,
            syntax_profile: None,
        };
        let _ = build_projection_plan(input);
        let number = concat!(
            "<script setup lang=\"ts\">\n",
            "const x: number = 1;\n",
            "</script>\n",
            "<template>\n",
            "  <Foo :bar=\"x\" />\n",
            "</template>\n",
        );
        let string = concat!(
            "<script setup lang=\"ts\">\n",
            "const x: string = 'a';\n",
            "</script>\n",
            "<template>\n",
            "  <Foo :bar=\"x\" />\n",
            "</template>\n",
        );
        let from_number = plan_from_source("file:///typed.vue", number);
        let from_string = plan_from_source("file:///typed.vue", string);
        assert!(from_number.is_complete());
        assert!(from_string.is_complete());
        assert_eq!(from_number.uses.len(), from_string.uses.len());
        assert_eq!(
            from_number.uses[0]
                .operations
                .iter()
                .map(|op| (op.kind, op.name.as_str(), op.inference_participation))
                .collect::<Vec<_>>(),
            from_string.uses[0]
                .operations
                .iter()
                .map(|op| (op.kind, op.name.as_str(), op.inference_participation))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            from_number
                .expressions()
                .iter()
                .map(|e| (e.kind, e.spelling.as_str()))
                .collect::<Vec<_>>(),
            from_string
                .expressions()
                .iter()
                .map(|e| (e.kind, e.spelling.as_str()))
                .collect::<Vec<_>>()
        );
    }

    const TYPE_FREE_CRATE_ALLOWLIST: &[&str] = &[
        "crate::assembly::source_unit",
        "crate::ast::types",
        "crate::ide::event_to_jsx_name",
        "crate::ide::get_directive_name",
        "crate::parser::types",
        "crate::template::oxc",
        "crate::types",
        "crate::compile::parse_sfc",
    ];

    fn crate_path_allowed(path: &str) -> bool {
        TYPE_FREE_CRATE_ALLOWLIST
            .iter()
            .any(|allow| path == *allow || path.starts_with(&format!("{allow}::")))
    }

    fn ident_len(src: &str) -> usize {
        let mut chars = src.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
            _ => return 0,
        }
        let mut n = 1;
        for c in chars {
            if c.is_ascii_alphanumeric() || c == '_' {
                n += 1;
            } else {
                break;
            }
        }
        n
    }

    fn crate_paths(src: &str) -> Vec<String> {
        let mut paths = Vec::new();
        let mut rest = src;
        while let Some(at) = rest.find("crate::") {
            let from = &rest[at..];
            let mut end = "crate::".len();
            loop {
                let ident = ident_len(&from[end..]);
                if ident == 0 {
                    break;
                }
                end += ident;
                if from[end..].starts_with("::") {
                    end += 2;
                    continue;
                }
                break;
            }
            let path = from[..end].strip_suffix("::").unwrap_or(&from[..end]);
            if from[path.len()..].starts_with("::{") {
                if let Some(close) = from[path.len() + 3..].find('}') {
                    let inner = &from[path.len() + 3..path.len() + 3 + close];
                    for item in inner.split(',') {
                        let name = item.split(" as ").next().unwrap_or(item).trim();
                        if ident_len(name) == name.len() && !name.is_empty() {
                            paths.push(format!("{path}::{name}"));
                        }
                    }
                    rest = &from[path.len() + 3 + close..];
                    continue;
                }
            }
            paths.push(path.to_string());
            rest = &from[path.len()..];
        }
        paths
    }

    #[test]
    fn stp9_complete_cache_rejects_malformed_and_unknown_syntax() {
        let malformed = plan_from_source("file:///malformed.vue", MALFORMED);
        assert!(!malformed.is_complete(), "{:?}", malformed.completeness);
        let pug = plan_from_source("file:///pug.vue", PUG);
        assert!(!pug.is_complete(), "{:?}", pug.completeness);
        match &pug.completeness {
            PlanCompleteness::Incomplete { reasons } => {
                assert!(
                    reasons
                        .iter()
                        .any(|r| matches!(r, Incompleteness::UnknownTemplateLang(l) if l == "pug")),
                    "{reasons:?}"
                );
            }
            PlanCompleteness::Complete => panic!("pug must be incomplete"),
        }
        let mut cache = CompletePlanCache::default();
        assert_eq!(
            cache.admit(malformed.clone()).err(),
            Some(CompleteCacheRefusal::Incomplete)
        );
        assert!(cache.get(&malformed.snapshot).is_none());
        assert_eq!(
            cache.admit(pug.clone()).err(),
            Some(CompleteCacheRefusal::Incomplete)
        );
        let clean = plan_from_source("file:///ids.vue", IDS_BASE);
        assert!(clean.is_complete());
        cache
            .admit(clean.clone())
            .expect("complete plan warms the cache");
        assert!(cache.get(&clean.snapshot).is_some());
    }

    #[test]
    fn stp9_determinism_fresh_matches_incremental() {
        let parsed = crate::compile::parse_sfc(IDS_BASE, None, None);
        let input = PlanInput {
            canonical_id: "file:///ids.vue",
            source: IDS_BASE,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
        };
        let fresh = build_projection_plan(input);
        let again = build_projection_plan(PlanInput {
            canonical_id: "file:///ids.vue",
            source: IDS_BASE,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
        });
        let incremental = build_projection_plan_incremental(
            Some(&fresh),
            PlanInput {
                canonical_id: "file:///ids.vue",
                source: IDS_BASE,
                parsed: &parsed,
                parse_key: None,
                syntax_profile: None,
            },
        );
        assert_eq!(fresh.observation_key(), again.observation_key());
        assert_eq!(fresh.observation_key(), incremental.observation_key());
        assert_eq!(fresh.uses, incremental.uses);
        assert_eq!(fresh.branches, incremental.branches);
        let from_none = build_projection_plan_incremental(
            None,
            PlanInput {
                canonical_id: "file:///ids.vue",
                source: IDS_BASE,
                parsed: &parsed,
                parse_key: None,
                syntax_profile: None,
            },
        );
        assert_eq!(fresh.observation_key(), from_none.observation_key());
        let malformed_parsed = crate::compile::parse_sfc(MALFORMED, None, None);
        let incomplete = build_projection_plan(PlanInput {
            canonical_id: "file:///malformed.vue",
            source: MALFORMED,
            parsed: &malformed_parsed,
            parse_key: None,
            syntax_profile: None,
        });
        let incomplete_inc = build_projection_plan_incremental(
            Some(&incomplete),
            PlanInput {
                canonical_id: "file:///malformed.vue",
                source: MALFORMED,
                parsed: &malformed_parsed,
                parse_key: None,
                syntax_profile: None,
            },
        );
        assert_eq!(
            incomplete.observation_key(),
            incomplete_inc.observation_key()
        );
        assert!(!incomplete.is_complete());
        assert!(fresh
            .syntax_obligations()
            .iter()
            .any(|o| o.kind == ObligationKind::ComponentUse));
    }

    #[test]
    fn syntax_obligations_do_not_embed_type_answers() {
        let plan = plan_from_source("file:///ids.vue", IDS_BASE);
        for obligation in plan.syntax_obligations() {
            assert!(
                matches!(
                    obligation.kind,
                    ObligationKind::ComponentUse
                        | ObligationKind::AttributeOp
                        | ObligationKind::Branch
                        | ObligationKind::Slot
                ),
                "{obligation:?}"
            );
        }
        assert!(plan.uses.iter().all(|u| u.generic_binders.len() == 2));
        assert!(plan.uses[0]
            .operations
            .iter()
            .any(|op| op.kind == AttributeOpKind::Bound && op.name == "bar"));
    }

    fn sfc(template: &str) -> String {
        format!("<script setup lang=\"ts\">\nconst x = 1;\n</script>\n<template>\n{template}\n</template>\n")
    }

    fn cache_refuses(source: &str) {
        let plan = plan_from_source("file:///dirty.vue", source);
        assert!(!plan.is_complete(), "{:?}", plan.completeness);
        let mut cache = CompletePlanCache::default();
        assert_eq!(
            cache.admit(plan.clone()).err(),
            Some(CompleteCacheRefusal::Incomplete)
        );
        assert!(cache.get(&plan.snapshot).is_none());
    }

    #[test]
    fn sibling_same_binder_scopes_mint_distinct_origins() {
        let src = sfc(concat!(
            "  <ul v-for=\"item in a\" :key=\"item\"><Foo :row=\"item\" /></ul>\n",
            "  <ul v-for=\"item in b\" :key=\"item\"><Bar :row=\"item\" /></ul>\n",
            "  <Wrap><Qux v-slot=\"{ row }\">{{ row }}</Qux></Wrap>\n",
            "  <Wrap><Quux v-slot=\"{ row }\">{{ row }}</Quux></Wrap>\n",
        ));
        let plan = plan_from_source("file:///sibling-scope.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let items: Vec<&BindingOrigin> = plan
            .origins
            .iter()
            .filter(|o| o.name == "item" && o.kind == BinderKind::VForAlias)
            .collect();
        assert_eq!(items.len(), 2, "{:?}", plan.origins);
        assert_ne!(items[0].id, items[1].id);
        assert_ne!(items[0].lexical_env, items[1].lexical_env);
        let rows: Vec<&BindingOrigin> = plan
            .origins
            .iter()
            .filter(|o| o.name == "row" && o.kind == BinderKind::SlotProp)
            .collect();
        assert_eq!(rows.len(), 2, "{:?}", plan.origins);
        assert_ne!(rows[0].id, rows[1].id);
        assert_ne!(rows[0].lexical_env, rows[1].lexical_env);
        let commented = sfc(concat!(
            "  <!-- note -->\n",
            "  <ul v-for=\"item in a\" :key=\"item\"><Foo :row=\"item\" /></ul>\n",
            "  <div />\n",
            "  <ul v-for=\"item in b\" :key=\"item\"><Bar :row=\"item\" /></ul>\n",
            "  <Wrap><Qux v-slot=\"{ row }\">{{ row }}</Qux></Wrap>\n",
            "  <Wrap><Quux v-slot=\"{ row }\">{{ row }}</Quux></Wrap>\n",
        ));
        let after = plan_from_source("file:///sibling-scope.vue", &commented);
        assert_eq!(
            items[0].id,
            after.origins.iter().find(|o| o.name == "item").unwrap().id
        );
    }

    #[test]
    fn same_tag_and_unrelated_subtree_insertion_preserve_use_ids() {
        let base = sfc("  <section><Foo :value=\"x\" /></section>\n");
        let same_tag = sfc("  <Foo :other=\"x\" />\n  <section><Foo :value=\"x\" /></section>\n");
        let subtree = sfc(
            "  <aside><Foo :value=\"y\" /></aside>\n  <section><Foo :value=\"x\" /></section>\n",
        );
        let base_plan = plan_from_source("file:///ids-struct.vue", &base);
        let same_tag_plan = plan_from_source("file:///ids-struct.vue", &same_tag);
        let subtree_plan = plan_from_source("file:///ids-struct.vue", &subtree);
        assert!(base_plan.is_complete(), "{:?}", base_plan.completeness);
        assert!(
            same_tag_plan.is_complete(),
            "{:?}",
            same_tag_plan.completeness
        );
        assert!(
            subtree_plan.is_complete(),
            "{:?}",
            subtree_plan.completeness
        );
        let original = &base_plan.uses[0].id;
        assert_eq!(original, &same_tag_plan.uses[1].id);
        assert_ne!(&same_tag_plan.uses[0].id, original);
        assert_eq!(original, &subtree_plan.uses[1].id);
        assert_ne!(&subtree_plan.uses[0].id, original);
        let value_expr = |plan: &ProjectionPlan, use_idx: usize| {
            let op = plan.uses[use_idx]
                .operations
                .iter()
                .find(|op| op.name == "value")
                .expect("value");
            op.expression.clone().expect("value expr")
        };
        let base_value = value_expr(&base_plan, 0);
        assert_eq!(base_value, value_expr(&same_tag_plan, 1));
        assert_eq!(base_value, value_expr(&subtree_plan, 1));
        assert_eq!(
            base_plan.expression(&base_value).unwrap().spelling,
            same_tag_plan.expression(&base_value).unwrap().spelling
        );
        assert_eq!(base_plan.expression(&base_value).unwrap().spelling, "x");
    }

    #[test]
    fn component_vfor_uses_loop_scope_for_inputs() {
        let src = sfc("  <Comp v-for=\"item in items\" :row=\"item\" />\n");
        let plan = plan_from_source("file:///loop-comp.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        assert_eq!(plan.uses.len(), 1);
        let origin = plan
            .origins
            .iter()
            .find(|o| o.name == "item")
            .expect("item origin");
        assert_eq!(plan.uses[0].lexical_env, origin.lexical_env);
        let row = plan.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "row")
            .expect("row op");
        let expr = plan
            .expression(row.expression.as_ref().expect("row expr"))
            .expect("locator");
        assert_eq!(expr.lexical_env, origin.lexical_env);
    }

    #[test]
    fn vif_on_vfor_evaluates_in_parent_scope() {
        let same = sfc("  <li v-for=\"item in items\" v-if=\"show\">{{ item }}</li>\n");
        let nested = sfc(
            "  <template v-for=\"item in items\"><li v-if=\"item.ok\">{{ item }}</li></template>\n",
        );
        let same_plan = plan_from_source("file:///loop-if.vue", &same);
        let nested_plan = plan_from_source("file:///loop-if-nested.vue", &nested);
        assert!(same_plan.is_complete(), "{:?}", same_plan.completeness);
        assert!(nested_plan.is_complete(), "{:?}", nested_plan.completeness);
        let same_origin = same_plan
            .origins
            .iter()
            .find(|o| o.name == "item")
            .expect("item origin");
        assert_eq!(same_plan.branches.len(), 1);
        assert_ne!(same_plan.branches[0].lexical_env, same_origin.lexical_env);
        let nested_origin = nested_plan
            .origins
            .iter()
            .find(|o| o.name == "item")
            .expect("nested item");
        assert_eq!(nested_plan.branches.len(), 1);
        assert_eq!(
            nested_plan.branches[0].lexical_env,
            nested_origin.lexical_env
        );
    }

    #[test]
    fn chained_elseif_retains_predecessor_exclusions() {
        let chained = sfc("  <Foo v-if=\"a\" /><Bar v-else-if=\"b\" /><Baz v-else />\n");
        let independent = sfc("  <Foo v-if=\"a\" /><Bar v-if=\"b\" />\n");
        let chained_plan = plan_from_source("file:///branch.vue", &chained);
        let independent_plan = plan_from_source("file:///branch.vue", &independent);
        assert!(
            chained_plan.is_complete(),
            "{:?}",
            chained_plan.completeness
        );
        assert_eq!(chained_plan.branches.len(), 3);
        assert!(chained_plan.branches[0].excluded.is_empty());
        assert_eq!(chained_plan.branches[1].excluded.len(), 1);
        assert_eq!(chained_plan.branches[1].outcome, BranchOutcome::Taken);
        assert_eq!(chained_plan.branches[2].outcome, BranchOutcome::Else);
        assert_eq!(chained_plan.branches[2].excluded.len(), 2);
        assert_ne!(
            chained_plan.observation_key(),
            independent_plan.observation_key()
        );
        assert!(independent_plan
            .branches
            .iter()
            .all(|b| b.excluded.is_empty()));
    }

    #[test]
    fn malformed_required_expressions_are_incomplete() {
        cache_refuses(&sfc("  {{ foo. }}\n"));
        cache_refuses(&sfc("  {{ foo( }}\n"));
        cache_refuses(&sfc("  <component />\n"));
        cache_refuses(&sfc("  <div :title=\"foo(\" />\n"));
        cache_refuses(&sfc("  <Foo :[foo(]=\"bar\" />\n"));
        cache_refuses(&sfc("  <Foo :title=\"foo(\" />\n"));
        let recovered = plan_from_source(
            "file:///unadmitted.vue",
            &sfc("  <Foo :title=\"foo(\" />\n"),
        );
        assert!(!recovered.is_complete());
        let title = recovered.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "title")
            .expect("title");
        assert!(title.expression.is_none());
        assert!(recovered
            .syntax_obligations()
            .iter()
            .any(|o| { o.kind == ObligationKind::AttributeOp && o.expression.is_none() }));
    }

    #[test]
    fn empty_slot_patterns_are_complete_malformed_are_not() {
        let empty_obj = sfc("  <Foo v-slot=\"{}\">ok</Foo>\n");
        let empty_arr = sfc("  <Foo v-slot=\"[]\">ok</Foo>\n");
        let malformed = sfc("  <Foo v-slot=\"{.\">ok</Foo>\n");
        let obj = plan_from_source("file:///slot-empty.vue", &empty_obj);
        let arr = plan_from_source("file:///slot-empty.vue", &empty_arr);
        assert!(obj.is_complete(), "{:?}", obj.completeness);
        assert!(arr.is_complete(), "{:?}", arr.completeness);
        cache_refuses(&malformed);
    }

    #[test]
    fn provided_slots_follow_owner_and_ignore_whitespace() {
        let compact = sfc("  <Foo><template #named>ok</template></Foo>\n");
        let formatted = sfc("  <Foo>\n    <template #named>ok</template>\n  </Foo>\n");
        let nested = sfc("  <Foo><Bar #named>ok</Bar></Foo>\n");
        let compact_plan = plan_from_source("file:///slots.vue", &compact);
        let formatted_plan = plan_from_source("file:///slots.vue", &formatted);
        let nested_plan = plan_from_source("file:///slots.vue", &nested);
        assert!(compact_plan.is_complete());
        assert_eq!(
            compact_plan.uses[0]
                .provided_slots
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            formatted_plan.uses[0]
                .provided_slots
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(compact_plan.uses[0]
            .provided_slots
            .iter()
            .any(|s| s.name == "named" && s.inference_participation));
        assert!(!compact_plan.uses[0]
            .provided_slots
            .iter()
            .any(|s| s.name == "default"));
        let foo = nested_plan
            .uses
            .iter()
            .find(|u| {
                nested_plan
                    .expression(&u.component_expression)
                    .is_some_and(|e| e.spelling == "Foo")
            })
            .expect("Foo");
        let bar = nested_plan
            .uses
            .iter()
            .find(|u| {
                nested_plan
                    .expression(&u.component_expression)
                    .is_some_and(|e| e.spelling == "Bar")
            })
            .expect("Bar");
        assert!(foo.provided_slots.iter().any(|s| s.name == "default"));
        assert!(!foo.provided_slots.iter().any(|s| s.name == "named"));
        assert!(bar.provided_slots.iter().any(|s| s.name == "named"));
        let component_level = sfc("  <Foo #named>hi</Foo>\n");
        let named_plan = plan_from_source("file:///slots.vue", &component_level);
        let names: Vec<&str> = named_plan.uses[0]
            .provided_slots
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["named"]);
    }

    #[test]
    fn inference_participation_is_syntactic_not_fixture_names() {
        let src = sfc(concat!(
            "  <List :items=\"xs\" @change=\"handler\" @rename=\"y\" v-bind=\"props\">\n",
            "    default\n",
            "  </List>\n",
        ));
        let plan = plan_from_source("file:///infer.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let use_ = &plan.uses[0];
        assert!(use_.expose_participation);
        let change = use_
            .operations
            .iter()
            .find(|op| op.kind == AttributeOpKind::VOn && op.name == "onChange")
            .expect("onChange");
        assert!(change.inference_participation);
        let items = use_
            .operations
            .iter()
            .find(|op| op.name == "items")
            .expect("items");
        assert!(items.inference_participation);
        let spread = use_
            .operations
            .iter()
            .find(|op| op.kind == AttributeOpKind::VBind)
            .expect("v-bind");
        assert!(spread.inference_participation);
        assert!(use_
            .provided_slots
            .iter()
            .any(|s| s.name == "default" && s.inference_participation));
        let on_change = sfc("  <List :onChange=\"handler\" />\n");
        let equivalent = plan_from_source("file:///infer.vue", &on_change);
        let bound = equivalent.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "onChange")
            .expect("bound onChange");
        assert!(bound.inference_participation);
        let static_src = sfc("  <List modelValue=\"hello\" />\n");
        let static_plan = plan_from_source("file:///infer.vue", &static_src);
        let static_op = static_plan.uses[0]
            .operations
            .iter()
            .find(|op| op.kind == AttributeOpKind::Static && op.name == "modelValue")
            .expect("static modelValue");
        assert!(static_op.inference_participation);
        assert!(static_op.expression.is_some());
        let bound_lit = sfc("  <List :modelValue=\"'hello'\" />\n");
        let bound_plan = plan_from_source("file:///infer.vue", &bound_lit);
        let bound_op = bound_plan.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "modelValue")
            .expect("bound modelValue");
        assert!(bound_op.inference_participation);
    }

    #[test]
    fn expression_occurrence_survives_comment_insertion() {
        let base = plan_from_source("file:///ids.vue", IDS_BASE);
        let commented = plan_from_source("file:///ids.vue", IDS_COMMENT);
        let op = base.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "bar")
            .expect("bar");
        let id = op.expression.as_ref().expect("bar expr");
        let before = base.expression(id).expect("locator");
        let after = commented.expression(id).expect("locator after comment");
        assert_eq!(before.spelling, "x");
        assert_eq!(after.spelling, "x");
        assert_eq!(&IDS_BASE[before.start as usize..before.end as usize], "x");
        assert_eq!(&IDS_COMMENT[after.start as usize..after.end as usize], "x");
        assert!(after.start > before.start);
    }

    #[test]
    fn plan_walk_reuses_admitted_vfor_product() {
        let src = include_str!("mod.rs");
        let production = src.split("#[cfg(test)]").next().expect("production");
        assert!(
            !production.contains("parse_vfor_with_bindings_sliced"),
            "loop walk must consume the retained v-for product"
        );
        assert!(
            !production.contains("parent_locals"),
            "plan walk must not thread a write-only parent_locals accumulation"
        );
        let src = sfc("  <div v-for=\"item in items\" :key=\"item\">{{ item }}</div>\n");
        let plan = plan_from_source("file:///vfor.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        assert_eq!(plan.origins.len(), 1);
        let source = plan
            .expressions()
            .iter()
            .find(|e| e.kind == ExpressionKind::VForSource)
            .expect("v-for source");
        assert_eq!(source.spelling, "items");
        assert_eq!(&src[source.start as usize..source.end as usize], "items");
    }

    #[test]
    fn same_parent_same_signature_insertion_preserves_surviving_use() {
        let base = sfc("  <section><Foo :value=\"foo\" /></section>\n");
        let inserted = sfc("  <section><Foo :value=\"is\" /><Foo :value=\"foo\" /></section>\n");
        let unrelated =
            sfc("  <section><Foo :value=\"unrelated\" /><Foo :value=\"foo\" /></section>\n");
        let base_plan = plan_from_source("file:///same-sig.vue", &base);
        let inserted_plan = plan_from_source("file:///same-sig.vue", &inserted);
        let unrelated_plan = plan_from_source("file:///same-sig.vue", &unrelated);
        assert!(base_plan.is_complete(), "{:?}", base_plan.completeness);
        assert!(inserted_plan.is_complete());
        assert!(unrelated_plan.is_complete());
        let original = &base_plan.uses[0].id;
        assert_eq!(original, &inserted_plan.uses[1].id);
        assert_ne!(&inserted_plan.uses[0].id, original);
        assert_eq!(original, &unrelated_plan.uses[1].id);
        assert_ne!(&unrelated_plan.uses[0].id, original);
    }

    #[test]
    fn component_occurrence_slices_match_spelling() {
        let static_src = sfc("  <Comp />\n");
        let dynamic_src = sfc("  <component :is=\"Chosen\" />\n");
        let static_plan = plan_from_source("file:///span.vue", &static_src);
        let dynamic_plan = plan_from_source("file:///span.vue", &dynamic_src);
        assert!(static_plan.is_complete(), "{:?}", static_plan.completeness);
        assert!(
            dynamic_plan.is_complete(),
            "{:?}",
            dynamic_plan.completeness
        );
        let static_occ = static_plan
            .expression(&static_plan.uses[0].component_expression)
            .expect("static occ");
        assert_eq!(static_occ.spelling, "Comp");
        assert_eq!(
            &static_src[static_occ.start as usize..static_occ.end as usize],
            "Comp"
        );
        let dynamic_occ = dynamic_plan
            .expression(&dynamic_plan.uses[0].component_expression)
            .expect("dynamic occ");
        assert_eq!(dynamic_occ.spelling, "Chosen");
        assert_eq!(
            &dynamic_src[dynamic_occ.start as usize..dynamic_occ.end as usize],
            "Chosen"
        );
        assert_eq!(dynamic_occ.kind, ExpressionKind::ComponentIs);
    }

    #[test]
    fn shorthand_vbind_retains_same_name_expression() {
        let short = sfc("  <Foo :value />\n");
        let explicit = sfc("  <Foo :value=\"value\" />\n");
        let short_plan = plan_from_source("file:///short.vue", &short);
        let explicit_plan = plan_from_source("file:///short.vue", &explicit);
        assert!(short_plan.is_complete(), "{:?}", short_plan.completeness);
        assert!(explicit_plan.is_complete());
        let short_op = short_plan.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "value")
            .expect("short value");
        let id = short_op.expression.as_ref().expect("shorthand expr");
        let occ = short_plan.expression(id).expect("locator");
        assert_eq!(occ.spelling, "value");
        assert_eq!(&short[occ.start as usize..occ.end as usize], "value");
        let explicit_op = explicit_plan.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "value")
            .expect("explicit value");
        assert_eq!(
            explicit_plan
                .expression(explicit_op.expression.as_ref().expect("explicit expr"))
                .unwrap()
                .spelling,
            "value"
        );
        let dyn_is = sfc("  <component :is />\n");
        let dyn_plan = plan_from_source("file:///short-is.vue", &dyn_is);
        assert!(dyn_plan.is_complete(), "{:?}", dyn_plan.completeness);
        let is_occ = dyn_plan
            .expression(&dyn_plan.uses[0].component_expression)
            .expect("is occ");
        assert_eq!(is_occ.spelling, "is");
        assert_eq!(&dyn_is[is_occ.start as usize..is_occ.end as usize], "is");
    }

    #[test]
    fn dynamic_attribute_argument_is_admitted() {
        let src = sfc("  <Foo :[key]=\"value\" />\n");
        let plan = plan_from_source("file:///dyn-arg.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let op = plan.uses[0]
            .operations
            .iter()
            .find(|op| op.name.contains("key"))
            .expect("dynamic op");
        let arg = op.argument.as_ref().expect("argument identity");
        let occ = plan.expression(arg).expect("arg locator");
        assert_eq!(occ.spelling, "key");
        assert_eq!(&src[occ.start as usize..occ.end as usize], "key");
        let value = op.expression.as_ref().expect("value identity");
        assert_eq!(plan.expression(value).unwrap().spelling, "value");
    }

    #[test]
    fn event_handler_keys_keep_authored_on_prefix_distinct() {
        let src = sfc("  <Foo @change=\"a\" @onChange=\"b\" :onChange=\"c\" />\n");
        let plan = plan_from_source("file:///events.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let names: Vec<&str> = plan.uses[0]
            .operations
            .iter()
            .map(|op| op.name.as_str())
            .collect();
        assert!(names.contains(&"onChange"), "{names:?}");
        assert!(names.contains(&"onOnChange"), "{names:?}");
        let von_on_change = plan.uses[0]
            .operations
            .iter()
            .find(|op| op.kind == AttributeOpKind::VOn && op.name == "onOnChange");
        assert!(von_on_change.is_some(), "{names:?}");
        let bound = plan.uses[0]
            .operations
            .iter()
            .find(|op| op.kind == AttributeOpKind::Bound && op.name == "onChange");
        assert!(bound.is_some(), "{names:?}");
    }

    #[test]
    fn static_is_is_complete_without_identifier_kind() {
        let static_src = sfc("  <component is=\"Foo\" />\n");
        let bound_src = sfc("  <component :is=\"Foo\" />\n");
        let static_plan = plan_from_source("file:///is.vue", &static_src);
        let bound_plan = plan_from_source("file:///is.vue", &bound_src);
        assert!(static_plan.is_complete(), "{:?}", static_plan.completeness);
        assert!(bound_plan.is_complete(), "{:?}", bound_plan.completeness);
        let static_occ = static_plan
            .expression(&static_plan.uses[0].component_expression)
            .expect("static is");
        assert_eq!(static_occ.kind, ExpressionKind::ComponentTag);
        assert_eq!(static_occ.spelling, "Foo");
        assert_eq!(
            &static_src[static_occ.start as usize..static_occ.end as usize],
            "Foo"
        );
        let bound_occ = bound_plan
            .expression(&bound_plan.uses[0].component_expression)
            .expect("bound is");
        assert_eq!(bound_occ.kind, ExpressionKind::ComponentIs);
        assert_eq!(bound_occ.spelling, "Foo");
    }

    #[test]
    fn malformed_dynamic_slot_names_cannot_warm_complete_cache() {
        cache_refuses(&sfc("  <Foo><template #[foo(]>hi</template></Foo>\n"));
        cache_refuses(&sfc("  <Foo v-slot:[foo(]=\"{}\">hi</Foo>\n"));
        let valid = sfc("  <Foo><template #[name]>hi</template></Foo>\n");
        let plan = plan_from_source("file:///dyn-slot.vue", &valid);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let name = plan
            .expressions()
            .iter()
            .find(|e| e.spelling == "name")
            .expect("dynamic slot name");
        assert_eq!(&valid[name.start as usize..name.end as usize], "name");
    }

    #[test]
    fn unrelated_loop_subtree_does_not_shift_scope_or_use() {
        let base = sfc("  <section><div v-for=\"item in items\"><Foo /></div></section>\n");
        let inserted = sfc(concat!(
            "  <aside><div v-for=\"item in items\"><Bar /></div></aside>\n",
            "  <section><div v-for=\"item in items\"><Foo /></div></section>\n",
        ));
        let base_plan = plan_from_source("file:///scope-insert.vue", &base);
        let inserted_plan = plan_from_source("file:///scope-insert.vue", &inserted);
        assert!(base_plan.is_complete(), "{:?}", base_plan.completeness);
        assert!(inserted_plan.is_complete());
        let base_origin = base_plan
            .origins
            .iter()
            .find(|o| o.name == "item")
            .expect("base item");
        let surviving = inserted_plan
            .origins
            .iter()
            .find(|o| {
                o.name == "item"
                    && inserted_plan.uses.iter().any(|u| {
                        u.lexical_env == o.lexical_env && {
                            inserted_plan
                                .expression(&u.component_expression)
                                .is_some_and(|e| e.spelling == "Foo")
                        }
                    })
            })
            .expect("surviving item");
        assert_eq!(base_origin.id, surviving.id);
        let base_foo = &base_plan.uses[0].id;
        let inserted_foo = inserted_plan
            .uses
            .iter()
            .find(|u| {
                inserted_plan
                    .expression(&u.component_expression)
                    .is_some_and(|e| e.spelling == "Foo")
            })
            .expect("Foo")
            .id
            .clone();
        assert_eq!(base_foo, &inserted_foo);
    }

    #[test]
    fn type_free_crate_path_allowlist_rejects_helper_escape() {
        let dirty = "fn forbidden() { crate::ide::template::choose_answer(); }\n";
        assert!(crate_paths(dirty)
            .iter()
            .any(|path| !crate_path_allowed(path)));
        assert!(crate_path_allowed("crate::ide::get_directive_name"));
        assert!(!crate_path_allowed("crate::ide::template::choose_answer"));
    }

    fn value_expr<'a>(
        plan: &'a ProjectionPlan,
        use_idx: usize,
        name: &str,
    ) -> &'a AdmittedExpressionId {
        plan.uses[use_idx]
            .operations
            .iter()
            .find(|op| op.name == name)
            .and_then(|op| op.expression.as_ref())
            .expect("value expr")
    }

    #[test]
    fn same_parent_attribute_expr_ids_anchor_to_owning_use() {
        let base = sfc("  <Foo :value=\"x\" label=\"old\" />\n");
        let inserted =
            sfc("  <Foo :value=\"x\" label=\"new\" />\n  <Foo :value=\"x\" label=\"old\" />\n");
        let base_plan = plan_from_source("file:///expr-owner.vue", &base);
        let inserted_plan = plan_from_source("file:///expr-owner.vue", &inserted);
        assert!(base_plan.is_complete(), "{:?}", base_plan.completeness);
        assert!(
            inserted_plan.is_complete(),
            "{:?}",
            inserted_plan.completeness
        );
        let original_use = &base_plan.uses[0].id;
        assert_eq!(original_use, &inserted_plan.uses[1].id);
        assert_ne!(&inserted_plan.uses[0].id, original_use);
        let base_value = value_expr(&base_plan, 0, "value").clone();
        let surviving = value_expr(&inserted_plan, 1, "value").clone();
        let inserted_value = value_expr(&inserted_plan, 0, "value").clone();
        assert_eq!(base_value, surviving);
        assert_ne!(base_value, inserted_value);
        assert_eq!(base_plan.expression(&base_value).unwrap().spelling, "x");
        assert_eq!(inserted_plan.expression(&surviving).unwrap().spelling, "x");
    }

    #[test]
    fn same_path_loop_insertion_preserves_surviving_scope_and_use() {
        let base = sfc("  <div v-for=\"item in items\"><Foo /></div>\n");
        let inserted = sfc(concat!(
            "  <div v-for=\"item in items\"><Bar /></div>\n",
            "  <div v-for=\"item in items\"><Foo /></div>\n",
        ));
        let base_plan = plan_from_source("file:///same-path-loop.vue", &base);
        let inserted_plan = plan_from_source("file:///same-path-loop.vue", &inserted);
        assert!(base_plan.is_complete(), "{:?}", base_plan.completeness);
        assert!(
            inserted_plan.is_complete(),
            "{:?}",
            inserted_plan.completeness
        );
        let base_origin = base_plan
            .origins
            .iter()
            .find(|o| o.name == "item")
            .expect("base item");
        let surviving = inserted_plan
            .origins
            .iter()
            .find(|o| {
                o.name == "item"
                    && inserted_plan.uses.iter().any(|u| {
                        u.lexical_env == o.lexical_env
                            && inserted_plan
                                .expression(&u.component_expression)
                                .is_some_and(|e| e.spelling == "Foo")
                    })
            })
            .expect("surviving item");
        assert_eq!(base_origin.id, surviving.id);
        let base_foo = &base_plan.uses[0].id;
        let inserted_foo = inserted_plan
            .uses
            .iter()
            .find(|u| {
                inserted_plan
                    .expression(&u.component_expression)
                    .is_some_and(|e| e.spelling == "Foo")
            })
            .expect("Foo");
        assert_eq!(base_foo, &inserted_foo.id);
    }

    #[test]
    fn slot_only_sibling_insertion_preserves_surviving_use() {
        let base = sfc("  <Foo><template #a>A</template></Foo>\n");
        let inserted = sfc(concat!(
            "  <Foo><template #b>B</template></Foo>\n",
            "  <Foo><template #a>A</template></Foo>\n",
        ));
        let base_plan = plan_from_source("file:///slot-insert.vue", &base);
        let inserted_plan = plan_from_source("file:///slot-insert.vue", &inserted);
        assert!(base_plan.is_complete(), "{:?}", base_plan.completeness);
        assert!(
            inserted_plan.is_complete(),
            "{:?}",
            inserted_plan.completeness
        );
        let original = &base_plan.uses[0].id;
        let surviving = inserted_plan
            .uses
            .iter()
            .find(|u| u.provided_slots.iter().any(|s| s.name == "a"))
            .expect("#a");
        let inserted_use = inserted_plan
            .uses
            .iter()
            .find(|u| u.provided_slots.iter().any(|s| s.name == "b"))
            .expect("#b");
        assert_eq!(original, &surviving.id);
        assert_ne!(original, &inserted_use.id);
    }

    #[test]
    fn dynamic_slot_obligations_name_admitted_expressions() {
        let src = sfc(concat!(
            "  <Foo>\n",
            "    <template #[a]>A</template>\n",
            "    <template #[b]>B</template>\n",
            "  </Foo>\n",
        ));
        let plan = plan_from_source("file:///dyn-slots.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let slots: Vec<_> = plan
            .syntax_obligations()
            .iter()
            .filter(|o| o.kind == ObligationKind::Slot)
            .collect();
        assert_eq!(slots.len(), 2, "{slots:?}");
        let names: Vec<&str> = slots
            .iter()
            .map(|slot| {
                let id = slot.expression.as_ref().expect("dynamic slot expr");
                plan.expression(id).expect("locator").spelling.as_str()
            })
            .collect();
        assert_eq!(names, ["a", "b"]);
        assert_ne!(slots[0].expression.as_ref(), slots[1].expression.as_ref());
    }

    #[test]
    fn invalid_expression_suffix_cannot_warm_complete_cache() {
        cache_refuses(&sfc("  <Foo :value=\"foo; @\" />\n"));
        let clean = sfc("  <Foo :value=\"foo\" />\n");
        let plan = plan_from_source("file:///suffix-clean.vue", &clean);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let mut cache = CompletePlanCache::default();
        cache
            .admit(plan.clone())
            .expect("valid foo warms the cache");
        let dirty = plan_from_source(
            "file:///suffix-dirty.vue",
            &sfc("  <Foo :value=\"foo; @\" />\n"),
        );
        assert!(!dirty.is_complete(), "{:?}", dirty.completeness);
        assert_eq!(
            cache.admit(dirty).err(),
            Some(CompleteCacheRefusal::Incomplete)
        );
        cache_refuses(&sfc("  <Foo :value=\"foo(\" />\n"));
    }

    #[test]
    fn directive_form_and_modifiers_are_distinct_operations() {
        let von = plan_from_source("file:///form.vue", &sfc("  <Foo v-on=\"x\" />\n"));
        let at_on = plan_from_source("file:///form.vue", &sfc("  <Foo @on=\"x\" />\n"));
        let focus = plan_from_source("file:///form.vue", &sfc("  <Foo v-focus=\"x\" />\n"));
        let bound = plan_from_source("file:///form.vue", &sfc("  <Foo :focus=\"x\" />\n"));
        let prevent =
            plan_from_source("file:///form.vue", &sfc("  <Foo @click.prevent=\"x\" />\n"));
        let plain = plan_from_source("file:///form.vue", &sfc("  <Foo @click=\"x\" />\n"));
        assert!(von.is_complete() && at_on.is_complete());
        assert!(focus.is_complete() && bound.is_complete());
        assert_ne!(von.uses[0].id, at_on.uses[0].id);
        assert_ne!(focus.uses[0].id, bound.uses[0].id);
        assert_eq!(von.uses[0].operations[0].kind, AttributeOpKind::VOn);
        assert_eq!(von.uses[0].operations[0].name, "v-on");
        assert_eq!(at_on.uses[0].operations[0].kind, AttributeOpKind::VOn);
        assert_eq!(at_on.uses[0].operations[0].name, "onOn");
        assert_eq!(focus.uses[0].operations[0].kind, AttributeOpKind::Directive);
        assert_eq!(focus.uses[0].operations[0].name, "focus");
        assert_eq!(bound.uses[0].operations[0].kind, AttributeOpKind::Bound);
        assert_eq!(bound.uses[0].operations[0].name, "focus");
        assert_eq!(prevent.uses[0].operations[0].modifiers, ["prevent"]);
        assert!(plain.uses[0].operations[0].modifiers.is_empty());
        assert_ne!(prevent.uses[0].id, plain.uses[0].id);
    }

    #[test]
    fn v_pre_subtree_is_not_projected() {
        let src = sfc(concat!(
            "  <div v-pre><Foo :x=\"bad(\">{{ raw }}</Foo></div>\n",
            "  <Bar />\n",
        ));
        let plan = plan_from_source("file:///vpre.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        assert_eq!(plan.uses.len(), 1, "{:?}", use_hexes(&plan));
        let occ = plan
            .expression(&plan.uses[0].component_expression)
            .expect("Bar");
        assert_eq!(occ.spelling, "Bar");
    }

    #[test]
    fn static_attribute_whitespace_and_empty_values_are_preserved() {
        let padded = plan_from_source("file:///static.vue", &sfc("  <Foo label=\"  hi  \" />\n"));
        let trimmed = plan_from_source("file:///static.vue", &sfc("  <Foo label=\"hi\" />\n"));
        let empty = plan_from_source("file:///static.vue", &sfc("  <Foo label=\"\" />\n"));
        let bound_empty = plan_from_source("file:///static.vue", &sfc("  <Foo :label=\"''\" />\n"));
        assert!(padded.is_complete() && trimmed.is_complete());
        assert!(empty.is_complete() && bound_empty.is_complete());
        let padded_op = padded.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "label")
            .expect("padded");
        let trimmed_op = trimmed.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "label")
            .expect("trimmed");
        assert_eq!(
            padded
                .expression(padded_op.expression.as_ref().expect("padded expr"))
                .unwrap()
                .spelling,
            "  hi  "
        );
        assert_eq!(
            trimmed
                .expression(trimmed_op.expression.as_ref().expect("trimmed expr"))
                .unwrap()
                .spelling,
            "hi"
        );
        assert_ne!(padded.uses[0].id, trimmed.uses[0].id);
        let empty_op = empty.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "label")
            .expect("empty");
        assert!(empty_op.inference_participation);
        assert_eq!(
            empty
                .expression(empty_op.expression.as_ref().expect("empty expr"))
                .unwrap()
                .spelling,
            ""
        );
        let bound_op = bound_empty.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "label")
            .expect("bound empty");
        assert!(bound_op.inference_participation);
        assert_eq!(empty_op.index, 0);
        let with_if = plan_from_source(
            "file:///static.vue",
            &sfc("  <Foo v-if=\"a\" :row=\"x\" />\n"),
        );
        let row = with_if.uses[0]
            .operations
            .iter()
            .find(|op| op.name == "row")
            .expect("row");
        assert_eq!(row.index, 0);
    }

    #[test]
    fn op_signature_uses_draft_spellings_not_expression_scan() {
        let src = include_str!("mod.rs");
        let production = src.split("#[cfg(test)]").next().expect("production");
        assert!(
            production.contains("fn op_signature(ops: &[OpDraft])"),
            "op signatures must be built from drafts, not by scanning expressions"
        );
        let many = sfc(&(0..32)
            .map(|i| format!("  <Foo :value=\"x\" k{i}=\"{i}\" />\n"))
            .collect::<String>());
        let plan = plan_from_source("file:///sig-scale.vue", &many);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        assert_eq!(plan.uses.len(), 32);
    }
}
