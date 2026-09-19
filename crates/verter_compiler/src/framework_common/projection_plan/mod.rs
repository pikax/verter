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

use rustc_hash::FxHashMap;

use verter_identity::canonical::Canonical;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ParseKey, SourceUnitId, SyntaxProfileId};

use crate::assembly::source_unit::{carrier_revision, carrier_source_id};
use crate::ast::types::{AstNodeKind, ElementNode, ElementNodeConditionKind, InterpolationNode};
use crate::ide::get_directive_name;
use crate::parser::types::ParsedSfc;
use crate::template::oxc::parse_template_expressions;
use crate::template::oxc::types::{
    OxcNodeData, OxcParsedAst, OxcParsedElement, OxcParsedExpression,
};
use crate::types::{NodeId, NodeProp, NodeTag};

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
    /// `v-on` / `@event`.
    VOn,
    /// Static attribute.
    Static,
    /// Bound attribute (`:foo` / `v-bind:foo`) or custom directive.
    Bound,
    /// `v-model`.
    Model,
}

/// One ordered attribute operation on a component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedAttributeOp {
    /// Source order among this use's operations.
    pub index: u32,
    /// Operation kind.
    pub kind: AttributeOpKind,
    /// Logical attribute / event name (not a byte offset).
    pub name: String,
    /// Admitted value expression, when present.
    pub expression: Option<AdmittedExpressionId>,
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

/// Observation channels a use participates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationRole {
    Hover,
    Definition,
    References,
    Edits,
}

/// A named slot provided by one component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedSlot {
    /// Slot name (`default`, authored static name, or dynamic spelling).
    pub name: String,
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
    let root_scope = mint_scope(None, "template-root", &[], "", "", 0);
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
    builder.walk_nodes(root_children, &root_scope, &[], &[]);
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

fn mint_snapshot(
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

fn mint_scope(
    parent: Option<&LexicalScopeId>,
    kind: &str,
    binders: &[String],
    spelling: &str,
    host: &str,
    occurrence: u32,
) -> LexicalScopeId {
    let mut encoder = CanonicalEncoder::new(SCOPE_DOMAIN);
    encoder.field_option(1, parent.map(|p| p.canonical_bytes()));
    encoder.field_str(2, kind);
    encoder.field_str(3, spelling);
    encoder.field_str(4, host);
    encoder.field_u32(5, occurrence);
    encoder.field_u32(6, binders.len() as u32);
    for (i, name) in binders.iter().enumerate() {
        encoder.field_str(7 + i as u16, name);
    }
    LexicalScopeId(Canonical::from_encoder(&encoder))
}

fn mint_expression(
    unit: &SourceUnitId,
    env: &LexicalScopeId,
    kind: ExpressionKind,
    spelling: &str,
    occurrence: u32,
) -> AdmittedExpressionId {
    let mut encoder = CanonicalEncoder::new(EXPR_DOMAIN);
    encoder.field_bytes(1, unit.digest().as_bytes());
    encoder.field_bytes(2, env.digest().as_bytes());
    encoder.field_str(3, kind.tag());
    encoder.field_str(4, spelling);
    encoder.field_u32(5, occurrence);
    AdmittedExpressionId(Canonical::from_encoder(&encoder))
}

fn mint_use(
    unit: &SourceUnitId,
    env: &LexicalScopeId,
    kind: ExpressionKind,
    spelling: &str,
    path: &[String],
    ops_sig: &str,
    occurrence: u32,
) -> ComponentUseId {
    let mut encoder = CanonicalEncoder::new(USE_DOMAIN);
    encoder.field_bytes(1, unit.digest().as_bytes());
    encoder.field_bytes(2, env.digest().as_bytes());
    encoder.field_str(3, kind.tag());
    encoder.field_str(4, spelling);
    encoder.field_str(5, ops_sig);
    encoder.field_u32(6, occurrence);
    encoder.field_u32(7, path.len() as u32);
    for (i, seg) in path.iter().enumerate() {
        encoder.field_str(8 + i as u16, seg);
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
    use_counts: FxHashMap<(CanonicalDigest, String, String, String), u32>,
    expr_counts: FxHashMap<(CanonicalDigest, &'static str, String), u32>,
    scope_counts: FxHashMap<(CanonicalDigest, String, String, String, String), u32>,
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

    fn walk_nodes(
        &mut self,
        ids: &[NodeId],
        env: &LexicalScopeId,
        parent_locals: &[String],
        ancestors: &[String],
    ) {
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
                    self.admit_interpolation(id, interp, env);
                }
                AstNodeKind::Element(el) => {
                    self.walk_element(id, el, env, parent_locals, ancestors, &mut chain);
                }
            }
        }
    }

    fn walk_element(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        env: &LexicalScopeId,
        parent_locals: &[String],
        ancestors: &[String],
        chain: &mut Option<OpenBranch>,
    ) {
        let oxc_el = match &self.oxc.data[id.0] {
            OxcNodeData::Element(parsed) => Some(parsed.as_ref()),
            _ => None,
        };
        let host = open_tag_name(&el.tag_open, self.source);

        let mut child_env = env.clone();
        let mut child_locals: Vec<String> = parent_locals.to_vec();
        if let Some(v_for) = el.v_for.as_ref() {
            let (scope, locals) = self.push_vfor_scope(v_for, oxc_el, env, host);
            child_env = scope;
            child_locals.extend(locals);
        }

        self.record_condition(el, oxc_el, &child_env, chain);

        let component_env = child_env.clone();
        if let Some(v_slot) = el.v_slot.as_ref() {
            let (scope, locals) = self.push_slot_scope(v_slot, &child_env, oxc_el, host);
            child_env = scope;
            child_locals.extend(locals);
        }

        if el.tag_type.is_component() || is_dynamic_component(el, self.source) {
            self.record_component_use(el, oxc_el, &component_env, ancestors);
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
        self.walk_nodes(children, &child_env, &child_locals, &child_ancestors);
    }

    fn record_condition(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'a>>,
        env: &LexicalScopeId,
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
                    || oxc_unadmitted(oxc_el.and_then(|el| el.condition.as_ref()))
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
        v_for: &NodeProp,
        oxc_el: Option<&OxcParsedElement<'a>>,
        env: &LexicalScopeId,
        host: &str,
    ) -> (LexicalScopeId, Vec<String>) {
        let Some(value) = prop_value(self.source, v_for) else {
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::VForSource,
            });
            return (env.clone(), Vec::new());
        };
        let Some(parsed) = oxc_el.and_then(|el| el.v_for.as_ref()) else {
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::VForSource,
            });
            return (env.clone(), Vec::new());
        };
        if !parsed.parsed.result.is_ok() {
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::VForSource,
            });
            return (env.clone(), Vec::new());
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
        let source_spelling = parsed
            .parsed
            .result
            .right
            .as_ref()
            .map(|_| {
                let off = parsed.parsed.result.right_offset as usize;
                let slice = &self.source[start as usize..end as usize];
                slice.get(off..).unwrap_or(value).trim().to_string()
            })
            .unwrap_or_else(|| value.trim().to_string());
        let _ = self.admit_expr(
            env,
            ExpressionKind::VForSource,
            &source_spelling,
            start,
            end,
        );
        let spelling = value.trim();
        let occurrence = self.next_scope_occurrence(env, "v-for", &names, spelling, host);
        let scope = mint_scope(Some(env), "v-for", &names, spelling, host, occurrence);
        for name in &names {
            let id = mint_origin(&self.template_unit, &scope, BinderKind::VForAlias, name);
            self.origins.push(BindingOrigin {
                id,
                kind: BinderKind::VForAlias,
                name: name.clone(),
                lexical_env: scope.clone(),
            });
        }
        (scope, names)
    }

    fn push_slot_scope(
        &mut self,
        v_slot: &NodeProp,
        env: &LexicalScopeId,
        oxc_el: Option<&OxcParsedElement<'a>>,
        host: &str,
    ) -> (LexicalScopeId, Vec<String>) {
        let val = prop_value(self.source, v_slot);
        let parsed = oxc_el.and_then(|el| el.v_slot.as_ref());
        if let Some(val) = val {
            if !val.trim().is_empty() {
                match parsed {
                    Some(slot) if slot.parsed.result.is_ok() => {}
                    _ => {
                        self.reasons.push(Incompleteness::UnadmittedExpression {
                            kind: ExpressionKind::SlotParams,
                        });
                        return (env.clone(), Vec::new());
                    }
                }
                let start = v_slot.value_start.unwrap_or(v_slot.start);
                let end = v_slot.value_end.unwrap_or(v_slot.name_end);
                let _ = self.admit_expr(env, ExpressionKind::SlotParams, val.trim(), start, end);
            }
        }
        let names = slot_local_names(self.source, parsed);
        let spelling = val.map(str::trim).unwrap_or("");
        let occurrence = self.next_scope_occurrence(env, "v-slot", &names, spelling, host);
        let scope = mint_scope(Some(env), "v-slot", &names, spelling, host, occurrence);
        for name in &names {
            let id = mint_origin(&self.template_unit, &scope, BinderKind::SlotProp, name);
            self.origins.push(BindingOrigin {
                id,
                kind: BinderKind::SlotProp,
                name: name.clone(),
                lexical_env: scope.clone(),
            });
        }
        (scope, names)
    }

    fn next_scope_occurrence(
        &mut self,
        env: &LexicalScopeId,
        kind: &str,
        binders: &[String],
        spelling: &str,
        host: &str,
    ) -> u32 {
        let key = (
            env.digest(),
            kind.to_string(),
            binders.join("\0"),
            spelling.to_string(),
            host.to_string(),
        );
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
    ) {
        let (kind, spelling) = self.component_expression(el, oxc_el);
        let tag_start = el.tag_open.start;
        let tag_end = el.tag_open.name_end;
        let expr = self.admit_expr(env, kind, &spelling, tag_start, tag_end);
        let operations = self.collect_ops(el, oxc_el, env);
        let ops_sig = operations
            .iter()
            .map(|op| format!("{:?}:{}", op.kind, op.name))
            .collect::<Vec<_>>()
            .join(",");
        let path_key = ancestors.join("/");
        let occurrence = {
            let slot = self
                .use_counts
                .entry((env.digest(), spelling.clone(), path_key, ops_sig.clone()))
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
            occurrence,
        );
        let provided_slots = collect_provided_slots(el, self.ast, self.source);
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
        }
        for _slot in &provided_slots {
            self.obligations.push(SyntaxObligation {
                use_id: Some(use_id.clone()),
                kind: ObligationKind::Slot,
                expression: None,
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
    ) -> (ExpressionKind, String) {
        if is_dynamic_component(el, self.source) {
            if let Some((index, is_prop)) = el
                .props
                .iter()
                .enumerate()
                .find(|(_, p)| is_is_attr(p, self.source))
            {
                if let Some(val) = prop_value(self.source, is_prop) {
                    let parsed = oxc_el.and_then(|parsed| parsed.prop(index));
                    if oxc_unadmitted(parsed.and_then(|p| p.exp.as_ref())) || val.trim().is_empty()
                    {
                        self.reasons.push(Incompleteness::UnadmittedExpression {
                            kind: ExpressionKind::ComponentIs,
                        });
                    }
                    return (ExpressionKind::ComponentIs, val.trim().to_string());
                }
            }
            self.reasons.push(Incompleteness::UnadmittedExpression {
                kind: ExpressionKind::ComponentIs,
            });
            return (
                ExpressionKind::ComponentIs,
                open_tag_name(&el.tag_open, self.source).to_string(),
            );
        }
        (
            ExpressionKind::ComponentTag,
            open_tag_name(&el.tag_open, self.source).to_string(),
        )
    }

    fn scan_unadmitted_props(&mut self, el: &ElementNode, oxc_el: Option<&OxcParsedElement<'a>>) {
        for (index, prop) in el.props.iter().enumerate() {
            if classify_op(prop, self.source).is_none() {
                continue;
            }
            let parsed = oxc_el.and_then(|el| el.prop(index));
            if oxc_unadmitted(parsed.and_then(|p| p.exp.as_ref()))
                || oxc_unadmitted(parsed.and_then(|p| p.arg.as_ref()))
            {
                self.reasons.push(Incompleteness::UnadmittedExpression {
                    kind: ExpressionKind::AttributeValue,
                });
            }
        }
    }

    fn collect_ops(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'a>>,
        env: &LexicalScopeId,
    ) -> Vec<OrderedAttributeOp> {
        let mut ops = Vec::new();
        for (index, prop) in el.props.iter().enumerate() {
            let Some((kind, name)) = classify_op(prop, self.source) else {
                continue;
            };
            let parsed = oxc_el.and_then(|el| el.prop(index));
            let arg_unadmitted = oxc_unadmitted(parsed.and_then(|p| p.arg.as_ref()));
            let exp_unadmitted = oxc_unadmitted(parsed.and_then(|p| p.exp.as_ref()));
            if arg_unadmitted || exp_unadmitted {
                self.reasons.push(Incompleteness::UnadmittedExpression {
                    kind: ExpressionKind::AttributeValue,
                });
            }
            let expression = match prop_value(self.source, prop) {
                Some(val) if !exp_unadmitted && !arg_unadmitted => {
                    let start = prop.value_start.unwrap_or(prop.start);
                    let end = prop.value_end.unwrap_or(prop.name_end);
                    Some(self.admit_expr(
                        env,
                        ExpressionKind::AttributeValue,
                        val.trim(),
                        start,
                        end,
                    ))
                }
                _ => None,
            };
            ops.push(OrderedAttributeOp {
                index: index as u32,
                kind,
                name,
                expression,
                inference_participation: op_participates(kind),
            });
        }
        ops
    }

    fn admit_interpolation(
        &mut self,
        id: NodeId,
        interp: &InterpolationNode,
        env: &LexicalScopeId,
    ) {
        if interp.inner_start >= interp.inner_end {
            return;
        }
        let spelling = self.source[interp.inner_start as usize..interp.inner_end as usize].trim();
        if spelling.is_empty() {
            return;
        }
        let parsed = match &self.oxc.data[id.0] {
            OxcNodeData::Interpolation(expr) => Some(expr),
            _ => None,
        };
        if oxc_unadmitted(parsed) {
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
        );
    }

    fn admit_expr(
        &mut self,
        env: &LexicalScopeId,
        kind: ExpressionKind,
        spelling: &str,
        start: u32,
        end: u32,
    ) -> AdmittedExpressionId {
        let occurrence = {
            let slot = self
                .expr_counts
                .entry((env.digest(), kind.tag(), spelling.to_string()))
                .or_insert(0);
            let n = *slot;
            *slot += 1;
            n
        };
        let id = mint_expression(&self.template_unit, env, kind, spelling, occurrence);
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

fn oxc_unadmitted(expr: Option<&OxcParsedExpression<'_>>) -> bool {
    match expr {
        Some(parsed) => {
            parsed.errors.is_some() || (parsed.expression.is_none() && !parsed.multi_statement)
        }
        None => false,
    }
}

fn is_dynamic_component(el: &ElementNode, source: &str) -> bool {
    open_tag_name(&el.tag_open, source).eq_ignore_ascii_case("component")
}

fn is_is_attr(prop: &NodeProp, source: &str) -> bool {
    if !prop.is_directive {
        return false;
    }
    if get_directive_name(prop, source) != "bind" {
        return false;
    }
    match (prop.arg_start, prop.arg_end) {
        (Some(s), Some(e)) if s < e => source[s as usize..e as usize] == *"is",
        _ => false,
    }
}

fn open_tag_name<'a>(tag: &NodeTag, source: &'a str) -> &'a str {
    let bytes = source.as_bytes();
    let mut i = tag.start as usize + 1;
    let end = tag.name_end as usize;
    if i < end && bytes.get(i) == Some(&b'/') {
        i += 1;
    }
    if i > end {
        return "";
    }
    &source[i..end]
}

fn prop_value<'a>(source: &'a str, prop: &NodeProp) -> Option<&'a str> {
    let s = prop.value_start?;
    let e = prop.value_end?;
    if s >= e {
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
        "if" | "else-if" | "else" | "for" | "slot" | "once" => None,
        "on" => Some((
            AttributeOpKind::VOn,
            event_handler_key(arg_name(prop, source).unwrap_or("on")),
        )),
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
        other => Some((
            AttributeOpKind::Bound,
            arg_name(prop, source).unwrap_or(other).to_string(),
        )),
    }
}

fn event_handler_key(arg: &str) -> String {
    if arg.len() >= 3 && arg.starts_with("on") {
        let rest = &arg[2..];
        if rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            return arg.to_string();
        }
    }
    let mut out = String::from("on");
    let mut chars = arg.chars();
    if let Some(c) = chars.next() {
        out.push(c.to_ascii_uppercase());
        out.extend(chars);
    }
    out
}

fn op_participates(kind: AttributeOpKind) -> bool {
    !matches!(kind, AttributeOpKind::Static)
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

fn provided_slot(name: String) -> ProvidedSlot {
    ProvidedSlot {
        name,
        inference_participation: true,
    }
}

fn collect_provided_slots(
    el: &ElementNode,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
) -> Vec<ProvidedSlot> {
    let mut slots = Vec::new();
    if let Some(v_slot) = el.v_slot.as_ref() {
        slots.push(provided_slot(
            arg_name(v_slot, source).unwrap_or("default").to_string(),
        ));
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
                        slots.push(provided_slot(name));
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
        slots.push(provided_slot("default".to_string()));
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
    fn vif_on_vfor_evaluates_in_loop_scope() {
        let src = sfc("  <li v-for=\"item in items\" v-if=\"item.ok\">{{ item }}</li>\n");
        let plan = plan_from_source("file:///loop-if.vue", &src);
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        let origin = plan
            .origins
            .iter()
            .find(|o| o.name == "item")
            .expect("item origin");
        assert_eq!(plan.branches.len(), 1);
        assert_eq!(plan.branches[0].lexical_env, origin.lexical_env);
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
        cache_refuses(&sfc("  <component :is />\n"));
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
        let plan = plan_from_source(
            "file:///vfor.vue",
            &sfc("  <div v-for=\"item in items\" :key=\"item\">{{ item }}</div>\n"),
        );
        assert!(plan.is_complete(), "{:?}", plan.completeness);
        assert_eq!(plan.origins.len(), 1);
    }
}
