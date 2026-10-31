//! Canonical display policy.
//!
//! Display is a PROJECTION over the typed [`SemanticQueryValue`] /
//! [`SemanticNodeData`] graph — NEVER a stored or re-parsed string. The single
//! [`display`] entry point walks an already-computed value plus a display-only
//! [`DisplayNeeds`] bitset and emits a [`DisplayString`]. This ties two
//! architecture rules together:
//!
//! - the **typed-IR-only** rule: a display string is never re-parsed by a
//!   resolver — display only ever *reads* the typed graph, never dispatches a
//!   query, re-parses, or re-resolves.
//! - the **CodeTransform** rule: a typed result is never post-hoc string-spliced
//!   — every rendered token is computed structurally from node data + `needs`.
//!
//! `display_needs` is DISPLAY-ONLY: it never enters a typed-value family key
//! (it is masked to `⊥` unconditionally by
//! [`apply_mask`](super::demand::apply_mask)), so two queries differing only in
//! their display needs share one cached typed value and differ ONLY in the
//! string [`display`] projects from it. See
//! `.claude/skills/type-resolution/SKILL.md` §14.
//!
//! ### Note on [`DisplayFacet::ExpandAliases`]
//!
//! Whether an alias is kept as a name or inlined as a body is decided UPSTREAM,
//! at resolution time, by `EvalPolicy.alias_preservation` (§14.3): a value
//! resolved with `alias_preservation = Keep` is a lazy named reference
//! ([`SemanticNodeData::DeclRef`] / [`SemanticNodeData::InstantiationRef`]) with
//! NO body materialised in the graph; a value resolved with `Inline` is the
//! already-inlined body node. `display` honours that decision by rendering
//! whatever the graph carries — it MUST NOT follow a lazy reference (that would
//! require a dispatch). `ExpandAliases` therefore selects a richer rendering
//! only where a body is actually reachable; on a lazy reference it is a no-op,
//! because expansion never re-resolves (§14.3). [`SemanticNodeData::Alias`] is a
//! transparent structural indirection (it carries a target, not a name) and is
//! always followed.

use super::demand::{DisplayFacet, DisplayNeeds};
use super::{
    DeclarationAnalysisValue, FunctionParam, IndexKey, LiteralValue, OptionalityMod, PrimitiveKind,
    ProgramAnalysisValue, ReadonlyMod, RelationOutcome, RelationPayload, SemanticNodeData,
    SemanticNodeId, SemanticQueryValue, SignatureRef, TypeParamDecl,
};
use crate::project_semantic_dispatch::walk::{
    MergedDeclDisplaySurface, MergedDeclMember, MergedDeclSurface, ShallowSurfaceMember,
};
use crate::semantic_query_memo::SemanticGraphStore;
use std::fmt;
use std::sync::Arc;

type SubstitutionPair = (SemanticNodeId, SemanticNodeId);

/// A read-only view of the remaining ordered substitutions. An argument is
/// interpreted under the suffix AFTER its replacement, never under the pair
/// that introduced it. No semantic nodes or query results are constructed.
#[derive(Clone, Default)]
struct DisplaySubstitution {
    pairs: Option<Arc<[SubstitutionPair]>>,
    start: usize,
}

impl DisplaySubstitution {
    fn remaining(&self) -> &[SubstitutionPair] {
        &self.pairs.as_deref().unwrap_or_default()[self.start..]
    }

    fn from_pairs(pairs: Vec<SubstitutionPair>) -> Self {
        Self {
            pairs: (!pairs.is_empty()).then(|| pairs.into()),
            start: 0,
        }
    }

    fn prepend(&self, pairs: &[SubstitutionPair]) -> Self {
        if pairs.is_empty() {
            return self.clone();
        }
        Self::from_pairs(pairs.iter().chain(self.remaining()).copied().collect())
    }

    fn resolve(
        &self,
        store: &SemanticGraphStore,
        mut node: SemanticNodeId,
    ) -> (SemanticNodeId, Self) {
        let mut suffix = self.clone();
        while let Some(index) = suffix.remaining().iter().position(|&(parameter, _)| {
            if parameter == node {
                return true;
            }
            matches!(
                (store.node_data(parameter).as_deref(), store.node_data(node).as_deref()),
                (Some(SemanticNodeData::Infer { binder: parameter, .. }),
                 Some(SemanticNodeData::Infer { binder: occurrence, .. }
                    | SemanticNodeData::InferRef { binder: occurrence, .. }))
                    if parameter == occurrence
            )
        }) {
            node = suffix.remaining()[index].1;
            suffix.start += index + 1;
        }
        (node, suffix)
    }

    fn without_parameter(&self, parameter: SemanticNodeId) -> Self {
        Self::from_pairs(
            self.remaining()
                .iter()
                .copied()
                .filter(|pair| pair.0 != parameter)
                .collect(),
        )
    }

    fn in_conditional_scope(&self, store: &SemanticGraphStore, extends: SemanticNodeId) -> Self {
        let mut accepted = Vec::new();
        for &(parameter, argument) in self.remaining() {
            if let Some(SemanticNodeData::Infer { binder, .. }) =
                store.node_data(parameter).as_deref()
            {
                let prefix = Self::from_pairs(accepted.clone());
                if pattern_declares_infer(store, extends, &prefix, binder) {
                    continue;
                }
            }
            accepted.push((parameter, argument));
        }
        Self::from_pairs(accepted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ImplicitInferConstraint {
    String,
    UnknownArray,
    PropertyKey,
    MappedValue {
        value: SemanticNodeId,
        parameter: SemanticNodeId,
        key_space: SemanticNodeId,
    },
}

/// A display-only request for the consumer's canonical identity authority.
/// Ordered substitutions distinguish the same binder at different use sites.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct InferConstraintRequest {
    pub actual: SemanticNodeId,
    pub inferred: Vec<ImplicitInferConstraint>,
    pub substitutions: Vec<SubstitutionPair>,
}

pub(crate) type InferConstraintDecisions = rustc_hash::FxHashMap<InferConstraintRequest, bool>;

/// The positions that infer a constraint without consulting a referenced
/// declaration (the checker's omitTypeReferences path). A nested conditional
/// declares its own inference scope.
fn inferred_constraints(
    store: &SemanticGraphStore,
    check: SemanticNodeId,
    pattern: SemanticNodeId,
) -> rustc_hash::FxHashMap<super::InferBinderId, Vec<ImplicitInferConstraint>> {
    let mut inferred = rustc_hash::FxHashMap::<_, Vec<_>>::default();
    let (check, _) = follow_alias_chain(store, check, &DisplaySubstitution::default());
    let (pattern, _) = follow_alias_chain(store, pattern, &DisplaySubstitution::default());
    if let (
        Some(SemanticNodeData::Mapped { mapper: check, .. }),
        Some(SemanticNodeData::Mapped {
            mapper: pattern, ..
        }),
    ) = (
        store.node_data(check).as_deref(),
        store.node_data(pattern).as_deref(),
    ) {
        if let Some(SemanticNodeData::Infer { binder, .. }) =
            store.node_data(pattern.value_expr).as_deref()
        {
            inferred.entry(binder.clone()).or_default().push(
                ImplicitInferConstraint::MappedValue {
                    value: check.value_expr,
                    parameter: check.parameter_node,
                    key_space: check.key_space,
                },
            );
        }
    }
    let mut stack = vec![pattern];
    let mut seen = rustc_hash::FxHashSet::default();
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        let Some(data) = store.node_data(node) else {
            continue;
        };
        if matches!(data.as_ref(), SemanticNodeData::Conditional { .. }) {
            continue;
        }
        let mut note = |child: SemanticNodeId, constraint| {
            let (child, _) = follow_alias_chain(store, child, &DisplaySubstitution::default());
            if let Some(SemanticNodeData::Infer { binder, .. }) = store.node_data(child).as_deref()
            {
                inferred.entry(binder.clone()).or_default().push(constraint);
            }
        };
        match data.as_ref() {
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                for expression in expressions.iter() {
                    note(*expression, ImplicitInferConstraint::String);
                }
            }
            SemanticNodeData::Tuple { elements, .. } => {
                for element in elements.iter().filter(|element| element.rest) {
                    note(element.value, ImplicitInferConstraint::UnknownArray);
                }
            }
            SemanticNodeData::Signature { params, .. } => {
                for parameter in params.iter().filter(|parameter| parameter.rest) {
                    note(parameter.ty, ImplicitInferConstraint::UnknownArray);
                }
            }
            SemanticNodeData::Mapped { mapper, .. } => {
                note(mapper.key_space, ImplicitInferConstraint::PropertyKey)
            }
            _ => {}
        }
        data.for_each_retained_child(|child| stack.push(child));
    }
    inferred
}

/// The same lexical boundary as semantic substitution: only a declaration in
/// this extends pattern shadows; a reference or a nested conditional does not.
fn pattern_declares_infer(
    store: &SemanticGraphStore,
    pattern: SemanticNodeId,
    substitution: &DisplaySubstitution,
    target: &super::InferBinderId,
) -> bool {
    let mut stack = vec![(pattern, substitution.clone(), MAX_DISPLAY_DEPTH)];
    let mut seen: Vec<(SemanticNodeId, DisplaySubstitution)> = Vec::new();
    while let Some((node, substitution, depth)) = stack.pop() {
        if depth == 0 {
            continue;
        }
        let (node, substitution) = substitution.resolve(store, node);
        if seen.iter().any(|(previous, frame)| {
            *previous == node && frame.remaining() == substitution.remaining()
        }) {
            continue;
        }
        seen.push((node, substitution.clone()));
        let Some(data) = store.node_data(node) else {
            continue;
        };
        let mut children = Vec::new();
        match data.as_ref() {
            SemanticNodeData::Infer { binder, .. } if binder == target => return true,
            SemanticNodeData::Conditional { .. } => continue,
            SemanticNodeData::Alias(node)
            | SemanticNodeData::KeyOf { base: node }
            | SemanticNodeData::Array { element: node, .. } => children.push(*node),
            SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_) => {
                children.extend(data.composite_members().expect("composite").iter().copied());
            }
            SemanticNodeData::Tuple { elements, .. } => {
                children.extend(elements.iter().map(|element| element.value))
            }
            SemanticNodeData::TemplateLiteral { expressions, .. }
            | SemanticNodeData::InstantiationRef {
                args: expressions, ..
            }
            | SemanticNodeData::IntrinsicApplication {
                args: expressions, ..
            } => children.extend(expressions.iter().copied()),
            SemanticNodeData::Object(surface) => {
                children.extend(surface.positive_members().iter().map(|member| member.value));
                children.extend(surface.call_signatures.iter().copied());
                children.extend(surface.construct_signatures.iter().copied());
                for signature in surface.index_signatures.iter() {
                    children.extend([signature.key_type, signature.value_type]);
                }
                children.extend(surface.keyspace);
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                predicate,
                ..
            } => {
                children.extend(params.iter().map(|parameter| parameter.ty));
                children.push(*return_type);
                for parameter in type_parameters.iter() {
                    children.extend(parameter.constraint);
                    children.extend(parameter.default);
                }
                children.extend(predicate.and_then(|predicate| predicate.ty));
            }
            SemanticNodeData::Mapped { source, mapper } => {
                children.extend([*source, mapper.key_space]);
                let scoped = substitution.without_parameter(mapper.parameter_node);
                stack.push((mapper.value_expr, scoped.clone(), depth - 1));
                if let Some(remap) = mapper.name_remap {
                    stack.push((remap, scoped, depth - 1));
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                children.push(*object);
                if let IndexKey::Computed(index) = index {
                    children.push(*index);
                }
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                children.extend(program.child_nodes())
            }
            // An unresolved carrier applies its type arguments at the
            // reference site, so `Foo<infer X>` DECLARES `X` in this pattern
            // exactly as `infer X` written bare does. Descending through the
            // single structural accessor keeps every carrier covered; the
            // non-carrier arms it also reaches return an empty slice and add
            // no children.
            other => children.extend(other.carrier_type_args().iter().copied()),
        }
        stack.extend(
            children
                .into_iter()
                .map(|child| (child, substitution.clone(), depth - 1)),
        );
    }
    false
}

#[derive(Default)]
pub(crate) struct DisplayContext {
    substitution: DisplaySubstitution,
    ancestors: Vec<(SemanticNodeId, DisplaySubstitution)>,
    /// Print a union without its `null` and `undefined` arms: under
    /// `strictNullChecks: false` the checker never builds them, so a
    /// message names neither. A union of only those arms prints whole.
    pub(crate) omit_nullish: bool,
    /// The checker's node-builder length count, kept while printing a type
    /// for a message ([`Self::for_checker`]) so its abbreviation is the
    /// checker's.
    length: Option<NodeBuilderLength>,
    /// Evaluated forms prepared by the consumer ([`Self::with_settled_types`]):
    /// closed mapped types and spread programs print as their resolved object
    /// surfaces, and anonymous merged declarations as their peer-merged form.
    settled_types: rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
    /// The types a message print met with no evaluated form, for the
    /// consumer to materialize ([`Self::unsettled_types`]).
    unsettled_types: Vec<SemanticNodeId>,
    /// Constraints inferred from the immediate syntactic positions of this
    /// conditional's infer declarations, excluding reference constraints.
    inferred_constraints: rustc_hash::FxHashMap<super::InferBinderId, Vec<ImplicitInferConstraint>>,
    infer_constraint_decisions: InferConstraintDecisions,
    unsettled_infer_constraints: Vec<InferConstraintRequest>,
    /// The symbol chains prepared by the consumer for a fully qualified
    /// print ([`Self::with_declaration_chains`]).
    declaration_chains: rustc_hash::FxHashMap<DeclarationName, DeclarationChain>,
    /// The declarations a fully qualified print named with no prepared
    /// chain, for the consumer to resolve ([`Self::unchained_declarations`]).
    unchained_declarations: Vec<DeclarationName>,
}

/// A declaration a fully qualified message print names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DeclarationName {
    pub(crate) canonical_id: verter_type_expr::OwnedStr,
    pub(crate) decl_name: verter_type_expr::OwnedStr,
}

/// How `typeToString` spells a declaration under `UseFullyQualifiedType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeclarationChain {
    /// No module contains it: a global, or a declaration its module does
    /// not export, prints its own name.
    Unqualified,
    /// `import("<specifier>").<name>`, `name` being the module export
    /// (and any namespace path below it).
    Module { specifier: String, name: String },
    /// The chain is not known, so no print naming it is the checker's.
    Unknown,
}

/// `typeToString`'s clip of a message's type text: a text of at least twice
/// the truncation length keeps its first that-many-minus-three bytes and
/// `...`. The checker clips bytes, so a character the clip splits prints
/// as U+FFFD.
pub(crate) fn clip_checker_text(printed: String) -> String {
    let clip = super::checker_policy::MAXIMUM_TRUNCATION_LENGTH * 2;
    if printed.len() < clip {
        return printed;
    }
    let kept = String::from_utf8_lossy(&printed.as_bytes()[..clip - "...".len()]);
    format!("{kept}...")
}

/// The running length the checker's node builder counts while it prints one
/// type (`approximateLength`), and whether it has started abbreviating
/// (`checkTruncationLength`'s latch). The checker counts in UTF-8
/// bytes, per node kind, as the node is built rather than as its text
/// prints; a list or member set past
/// [`MAXIMUM_TRUNCATION_LENGTH`](super::checker_policy::MAXIMUM_TRUNCATION_LENGTH)
/// elides its remaining entries.
#[derive(Debug, Default)]
struct NodeBuilderLength {
    approximate: usize,
    truncating: bool,
    /// An upper bound on what the node kinds whose count is not modelled
    /// add: while it could take the count past the truncation length, which
    /// entries the checker elides is not known.
    unmodeled: usize,
}

/// Printed in place of what the display depth cap or a missing node cut
/// from a checker message: a noncharacter, so its position in the printed
/// text tells whether the cut is inside what the message keeps.
const CHECKER_CUT: &str = "\u{FDD0}";

impl DisplayContext {
    /// A context printing as the checker prints a type in a message when
    /// `strictNullChecks` is `omit_nullish`'s opposite, abbreviating as its
    /// node builder does. Finish the print with [`Self::checker_text`].
    pub(crate) fn for_checker(strict_null_checks: bool) -> Self {
        Self {
            omit_nullish: !strict_null_checks,
            length: Some(NodeBuilderLength::default()),
            ..Self::default()
        }
    }

    /// The text a message prints for `printed`, a type printed under this
    /// context: `typeToString`'s clip of a text of at least twice the
    /// truncation length to its first that-many-minus-three bytes and
    /// `...`. `None` when the print cannot be the checker's: a node kind
    /// whose node-builder count is not modelled could have moved where the
    /// checker abbreviates, or the depth cap cut a part the message keeps.
    pub(crate) fn checker_text(&self, printed: String) -> Option<String> {
        use super::checker_policy::MAXIMUM_TRUNCATION_LENGTH;
        let length = self.length.as_ref()?;
        if length.unmodeled > 0
            && (length.truncating
                || length.approximate + length.unmodeled > MAXIMUM_TRUNCATION_LENGTH)
        {
            return None;
        }
        if let Some(cut) = printed.find(CHECKER_CUT) {
            // Every byte before the cut is the checker's; it keeps fewer.
            if cut < MAXIMUM_TRUNCATION_LENGTH * 2 {
                return None;
            }
        }
        Some(clip_checker_text(printed))
    }

    /// This context printing every part, without the node builder's
    /// abbreviation.
    pub(crate) fn unabbreviated(mut self) -> Self {
        self.length = None;
        self
    }

    /// Print the forms the consumer evaluated before display. Display itself
    /// remains a read-only projection and never dispatches a semantic query.
    pub(crate) fn with_settled_types(
        mut self,
        settled: rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
        decisions: InferConstraintDecisions,
    ) -> Self {
        self.settled_types = settled;
        self.infer_constraint_decisions = decisions;
        self
    }

    /// The types a checker print met that its evaluated forms did not
    /// name, in the order it met them.
    pub(crate) fn unsettled_types(&self) -> &[SemanticNodeId] {
        &self.unsettled_types
    }

    pub(crate) fn unsettled_infer_constraints(&self) -> &[InferConstraintRequest] {
        &self.unsettled_infer_constraints
    }

    /// The symbol chains this print spells qualified names with.
    pub(crate) fn with_declaration_chains(
        mut self,
        chains: rustc_hash::FxHashMap<DeclarationName, DeclarationChain>,
    ) -> Self {
        self.declaration_chains = chains;
        self
    }

    /// The declarations a fully qualified print named without a chain.
    pub(crate) fn unchained_declarations(&self) -> &[DeclarationName] {
        &self.unchained_declarations
    }

    /// The symbol chains this context prints by.
    pub(crate) fn take_declaration_chains(
        &mut self,
    ) -> rustc_hash::FxHashMap<DeclarationName, DeclarationChain> {
        std::mem::take(&mut self.declaration_chains)
    }

    /// The evaluated forms and identity decisions this context prints by.
    pub(crate) fn into_settled_types(
        self,
    ) -> (
        rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
        InferConstraintDecisions,
    ) {
        (self.settled_types, self.infer_constraint_decisions)
    }

    fn measuring(&self) -> bool {
        self.length.is_some()
    }

    /// Count `bytes` the node builder adds for what it builds next.
    fn add_length(&mut self, bytes: usize) {
        if let Some(length) = &mut self.length {
            length.approximate += bytes;
        }
    }

    /// Whether the node builder abbreviates from here
    /// (`checkTruncationLength`): once the count passes the truncation
    /// length it does for the rest of the print.
    fn check_truncation(&mut self) -> bool {
        let Some(length) = &mut self.length else {
            return false;
        };
        if !length.truncating {
            let limit = super::checker_policy::MAXIMUM_TRUNCATION_LENGTH;
            length.truncating = length.approximate > limit;
        }
        length.truncating
    }

    /// A symbol-keyed property's name and colon count its full dotted
    /// spelling, its leaf again and twenty bytes. Measured on TypeScript
    /// 7.1.0-beta, which counts it deterministically (independent of the
    /// checker's process history): `[s0]` 24, `[longname0]` 38, `[ns.q0]`
    /// 27 and `[deep.er.q0]` 32 bytes, against `k0:`'s 3.
    fn symbol_property_length(&mut self, spelling: &str, leaf: &str) {
        self.add_length(spelling.len() + leaf.len() + 20);
    }

    /// Record `printed`, the text of a node whose node-builder count is not
    /// modelled, by a bound on what the checker counts for it.
    fn unmodeled(&mut self, printed: &str) {
        if let Some(length) = &mut self.length {
            length.unmodeled += 2 * (printed.len() + 1);
        }
    }

    /// The text of a part the depth cap or a missing node cut.
    fn cut(&self) -> String {
        if self.measuring() {
            CHECKER_CUT.to_owned()
        } else {
            TRUNCATION_TOKEN.to_owned()
        }
    }

    /// The node builder's list of type nodes (`mapToTypeNodes`): each entry
    /// counts two bytes for its separator before it builds, and once the
    /// count passes the truncation length the rest of the list is elided —
    /// a bare list (a union's or an intersection's members) as
    /// `... N more ...` before its last entry, any other list (type
    /// arguments, tuple elements) as `...` alone when it begins elided.
    /// Every entry renders when no count is kept.
    fn type_node_list<T: Copy>(
        &mut self,
        entries: &[T],
        bare: bool,
        mut render: impl FnMut(&mut Self, T) -> String,
    ) -> Vec<String> {
        let Some(&last) = entries.last() else {
            return Vec::new();
        };
        if self.check_truncation() {
            if !bare {
                return vec!["...".to_owned()];
            }
            if entries.len() > 2 {
                let first = render(self, entries[0]);
                let elided = format!("... {} more ...", entries.len() - 2);
                return vec![first, elided, render(self, last)];
            }
        }
        let mut rendered = Vec::with_capacity(entries.len());
        for (index, &entry) in entries.iter().enumerate() {
            let shown = index + 1;
            if shown + 2 < entries.len() - 1 && self.check_truncation() {
                rendered.push(format!("... {} more ...", entries.len() - shown));
                rendered.push(render(self, last));
                break;
            }
            self.add_length(2);
            rendered.push(render(self, entry));
        }
        rendered
    }

    fn contains(&self, node: &SemanticNodeId) -> bool {
        self.ancestors.iter().any(|(ancestor, substitution)| {
            ancestor == node && substitution.remaining() == self.substitution.remaining()
        })
    }

    fn push(&mut self, node: SemanticNodeId) {
        self.ancestors.push((node, self.substitution.clone()));
    }

    fn pop(&mut self) {
        self.ancestors.pop();
    }

    fn scoped<T>(
        &mut self,
        substitution: DisplaySubstitution,
        render: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = std::mem::replace(&mut self.substitution, substitution);
        let result = render(self);
        self.substitution = previous;
        result
    }
}

/// A rendered display string projected from a [`SemanticQueryValue`]. A plain
/// newtype over [`String`]: it is the *output* of the projection, carries no
/// identity, and is safe to be fully `pub`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayString(pub String);

impl DisplayString {
    /// Borrow the rendered string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<DisplayString> for String {
    fn from(value: DisplayString) -> Self {
        value.0
    }
}

/// Display-side recursion cap (`.claude/skills/type-resolution/SKILL.md` §14,
/// termination rule 3). It bounds the structural walk independently of any
/// resolver-side budget, so even a pathological graph (deep nesting that escaped
/// a cycle back-reference) terminates with a `…` truncation token.
pub(crate) const MAX_DISPLAY_DEPTH: usize = 64;

/// Beyond this many members a union is truncated when
/// [`DisplayFacet::TruncateLargeUnions`] is set.
const UNION_TRUNCATION_THRESHOLD: usize = 8;

/// The truncation / unresolved token
/// (`.claude/skills/type-resolution/SKILL.md` §14, termination rule 3).
const TRUNCATION_TOKEN: &str = "…";

construction_ordinal_seam! {
    /// The single §14.1 projection rule: render an already-computed
    /// [`SemanticQueryValue`] under `needs`. NEVER stores, re-parses, or
    /// re-resolves — it only reads `store` node data + `needs`.
    fn display(
        store: &SemanticGraphStore,
        value: &SemanticQueryValue,
        needs: DisplayNeeds,
    ) -> DisplayString {
        match value {
            SemanticQueryValue::TypeNode(id) => {
                display_type_node(store, *id, needs, MAX_DISPLAY_DEPTH, &mut DisplayContext::default())
            }
            SemanticQueryValue::ObjectProjection(_) => unreachable!(
                "display: ObjectProjection has no producer until witness-gated projection consumers land"
            ),
            SemanticQueryValue::OverloadSet(sigs) => {
                let rendered: Vec<String> = sigs
                    .iter()
                    .map(|s| display_signature(store, s, needs).0)
                    .collect();
                DisplayString(rendered.join("; "))
            }
            SemanticQueryValue::Relation(payload) => display_relation(payload),
            SemanticQueryValue::BroadRuntime(classification) => DisplayString(
                classification
                    .kinds()
                    .iter()
                    .map(|kind| format!("{kind:?}"))
                    .collect::<Vec<_>>()
                    .join(" | "),
            ),
            SemanticQueryValue::MaterializationCycleGate(outcome) => {
                DisplayString(format!("{outcome:?}"))
            }
            SemanticQueryValue::DeclarationAnalysis(d) => display_declaration_analysis(store, d, needs),
            SemanticQueryValue::ProgramAnalysis(p) => display_program_analysis(store, p, needs),
            SemanticQueryValue::FlowReturn(result) => DisplayString(format!(
                "{} (can_fall_through: {})",
                display_type_node(store, result.return_type(), needs, MAX_DISPLAY_DEPTH, &mut DisplayContext::default()).0,
                result
                    .can_fall_through
                    .reaches_end(verter_session_query::flow::completion::CompletionDischarge::PublishedResult),
            )),
            SemanticQueryValue::ResolveCall(result) => {
                let return_type = match result.as_ref() {
                    crate::semantic_query::ResolvedCallResult::Selected { return_type, .. }
                    | crate::semantic_query::ResolvedCallResult::DynamicAny { return_type } => {
                        *return_type
                    }
                };
                DisplayString(
                    display_type_node(store, return_type, needs, MAX_DISPLAY_DEPTH, &mut DisplayContext::default()).0,
                )
            }
            SemanticQueryValue::SignatureSet(value) => DisplayString(format!("{:?}", value.set)),
            SemanticQueryValue::SignatureResult(value) => {
                DisplayString(format!("{:?}", value.result))
            }
            SemanticQueryValue::TruthinessDomain(domain) => {
                DisplayString(format!("{domain:?}"))
            }
            // §14.1: the reserved native-checker seam. No producer constructs it,
            // so reaching it here is a logic error. Matched explicitly — NOT via a
            // `_` wildcard — so any future live `SemanticQueryValue` arm forces a
            // compile error here instead of silently falling through.
            SemanticQueryValue::DiagnosticAnalysis(_) => unreachable!(
                "display: DiagnosticAnalysis is a non-live reserved seam — no producer constructs it"
            ),
        }
    }
}

/// [`display`] inside a construction lane this thread holds of `store`.
/// Without such a lane a value's bare ordinals authorize no read and there
/// is no rendering.
pub fn display_in_scope(
    store: &SemanticGraphStore,
    value: &SemanticQueryValue,
    needs: DisplayNeeds,
) -> Option<DisplayString> {
    store
        .holds_construction_lane()
        .then(|| display(store, value, needs))
}

/// Structural walk of one graph node. EXHAUSTIVE over every
/// [`SemanticNodeData`] variant — no `_` wildcard, so a new node variant forces
/// a compile error here. `depth` decrements per level; `visited` is the
/// ancestor stack used to break cyclic / recursive types.
pub(crate) fn display_type_node(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> DisplayString {
    // Termination #3: depth cap.
    let (id, substitution) = visited.substitution.resolve(store, id);
    visited.scoped(substitution, |visited| {
        display_resolved_type_node(store, id, needs, depth, visited)
    })
}

/// Where a checker-spelled union print places one constituent: the
/// intrinsic primitives in their creation order — `any`-class first, then
/// `string`, `number`, `bigint`, `symbol`, `boolean` — before the types a
/// program declares, with the literals a check interned after them, and
/// `null` then `undefined` last of all, where the printer moves them
/// (`formatUnionTypes`): `"q" | undefined`. Measured on TypeScript 7.0.2,
/// whose unions sort by type id (`sortAndDeduplicateTypes`) and intern
/// those classes in this order.
fn checker_union_constituent_rank(
    store: &SemanticGraphStore,
    arm: &crate::semantic_query::PrintedUnionArm,
) -> u8 {
    use crate::semantic_query::PrintedUnionArm;
    let PrintedUnionArm::Node(node) = arm else {
        return 8;
    };
    match store.node_data(*node).as_deref() {
        Some(SemanticNodeData::Primitive(
            PrimitiveKind::Any
            | PrimitiveKind::Unknown
            | PrimitiveKind::Void
            | PrimitiveKind::Never,
        )) => 0,
        Some(SemanticNodeData::Primitive(PrimitiveKind::String)) => 1,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Number)) => 2,
        Some(SemanticNodeData::Primitive(PrimitiveKind::BigInt)) => 3,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Symbol)) => 4,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean)) => 5,
        Some(
            SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::TemplateLiteral { .. },
        ) => 9,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Null)) => 10,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined)) => 11,
        _ => 8,
    }
}

/// Where the checker meets one constituent of a union when it relates
/// the members one by one: by type id, so `unknown`, `undefined` and
/// `null` come first, then `void`, `string`, `number`, `bigint`, `symbol`
/// and `boolean` in the order the checker interns those intrinsics, then the types a program declares and the literals a check
/// interned last. Measured on TypeScript 7.0.2. It differs from the print
/// order, which moves `null` and `undefined` last. A stable sort by this
/// rank keeps the stored order of constituents it leaves unranked.
pub(crate) fn checker_relation_order_rank(store: &SemanticGraphStore, node: SemanticNodeId) -> u8 {
    match store.node_data(node).as_deref() {
        Some(SemanticNodeData::Primitive(PrimitiveKind::Any)) => 0,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => 1,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined)) => 2,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Null)) => 3,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Void | PrimitiveKind::Never)) => 4,
        Some(SemanticNodeData::Primitive(PrimitiveKind::String)) => 5,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Number)) => 6,
        Some(SemanticNodeData::Primitive(PrimitiveKind::BigInt)) => 7,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Symbol)) => 8,
        Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean)) => 9,
        // A literal is met where its base primitive is: the checker orders
        // `1 | "a" | true` as `"a"`, `1`, `true`.
        Some(SemanticNodeData::Literal(value)) => match value {
            LiteralValue::String(_) => 5,
            LiteralValue::Number(_) => 6,
            LiteralValue::BigInt(_) => 7,
            _ => 9,
        },
        Some(SemanticNodeData::EnumLiteral(_) | SemanticNodeData::TemplateLiteral { .. }) => 12,
        _ => 11,
    }
}

fn display_resolved_type_node(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> DisplayString {
    if depth == 0 {
        return DisplayString(visited.cut());
    }
    let data = match store.node_data(id) {
        Some(d) => d,
        // Unresolved / un-interned id — render the truncation token rather than
        // panicking (display tolerates a torn graph).
        None => return DisplayString(visited.cut()),
    };
    // Cycle break: a node already on the ancestor stack renders as a
    // back-reference (its name when available, else `…`) instead of recursing.
    if visited.contains(&id) {
        return DisplayString(back_ref_token(&data, needs, visited));
    }
    visited.push(id);
    let child_depth = depth - 1;
    if let Some(settled) = visited.settled_types.get(&id).copied() {
        let out = display_type_node(store, settled, needs, child_depth, visited);
        visited.pop();
        return out;
    }
    if visited.measuring()
        && matches!(
            data.as_ref(),
            SemanticNodeData::ObjectSpreadProgram(_) | SemanticNodeData::MergedDecl { .. }
        )
        && !visited.unsettled_types.contains(&id)
    {
        visited.unsettled_types.push(id);
    }
    // A node kind whose node-builder count is not modelled bounds it by its
    // printed text instead.
    let mut unmodeled = false;
    let out = match data.as_ref() {
        // Transparent structural indirection — always followed (it carries a
        // target, not a name); raise.rs treats it identically.
        SemanticNodeData::Alias(target) => {
            display_type_node(store, *target, needs, child_depth, visited).0
        }
        SemanticNodeData::Object(surface) => {
            render_object_surface(store, surface, needs, child_depth, visited)
        }
        SemanticNodeData::ObjectSpreadProgram(program) => {
            use crate::semantic_query::{AuthoredPropertyKey, ObjectConstructionEffect};
            let render_key = |key: &AuthoredPropertyKey, visited: &mut DisplayContext| match key {
                AuthoredPropertyKey::String(name) => member_name_token(name),
                AuthoredPropertyKey::Number(number) => number.to_string(),
                AuthoredPropertyKey::UniqueSymbol(identity) => {
                    format!("[unique symbol {}]", identity.symbol)
                }
                AuthoredPropertyKey::Computed(node) => format!(
                    "[{}]",
                    display_type_node(store, *node, needs, child_depth, visited).0
                ),
            };
            unmodeled = true;
            let mut parts = Vec::with_capacity(program.effects.len());
            for effect in program.effects.iter() {
                let part = match effect {
                    ObjectConstructionEffect::DirectProperty(effect) => format!(
                        "{}{}: {}",
                        render_key(&effect.key, visited),
                        if effect.optional { "?" } else { "" },
                        display_type_node(store, effect.value, needs, child_depth, visited).0
                    ),
                    ObjectConstructionEffect::DirectMethod(effect) => format!(
                        "{}{}: {}",
                        render_key(&effect.key, visited),
                        if effect.optional { "?" } else { "" },
                        display_type_node(store, effect.signature, needs, child_depth, visited).0
                    ),
                    ObjectConstructionEffect::DirectGet(effect) => format!(
                        "get {}: {}",
                        render_key(&effect.key, visited),
                        display_type_node(store, effect.signature, needs, child_depth, visited).0
                    ),
                    ObjectConstructionEffect::DirectSet(effect) => format!(
                        "set {}: {}",
                        render_key(&effect.key, visited),
                        display_type_node(store, effect.signature, needs, child_depth, visited).0
                    ),
                    ObjectConstructionEffect::DirectIndex(effect) => format!(
                        "[key: {}]: {}",
                        display_type_node(store, effect.key_type, needs, child_depth, visited).0,
                        display_type_node(store, effect.value_type, needs, child_depth, visited).0
                    ),
                    ObjectConstructionEffect::DirectCall(node)
                    | ObjectConstructionEffect::DirectConstruct(node) => {
                        display_type_node(store, *node, needs, child_depth, visited).0
                    }
                    ObjectConstructionEffect::Spread(node) => format!(
                        "...{}",
                        display_type_node(store, *node, needs, child_depth, visited).0
                    ),
                };
                parts.push(part);
            }
            if parts.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {} }}", parts.join("; "))
            }
        }
        SemanticNodeData::Union(arms) => {
            // A union arm only parenthesises a LOOSER binder (Conditional /
            // Function). A same-kind nested union arm (`A | (B | C)`) and a
            // tighter intersection arm (`A | B & C`) both render bare.
            let printed = printed_union_arms_in_order(store, arms, needs, visited);
            if needs.contains(DisplayFacet::TruncateLargeUnions)
                && printed.len() > UNION_TRUNCATION_THRESHOLD
            {
                let rendered: Vec<String> = printed[..UNION_TRUNCATION_THRESHOLD]
                    .iter()
                    .map(|arm| match arm {
                        crate::semantic_query::PrintedUnionArm::Node(a) => {
                            render_operand(store, *a, needs, child_depth, visited, Prec::Union)
                        }
                        crate::semantic_query::PrintedUnionArm::Enum(decl) => {
                            decl.decl_name.to_string()
                        }
                    })
                    .collect();
                format!("{} | {TRUNCATION_TOKEN}", rendered.join(" | "))
            } else {
                let entries: Vec<Option<&crate::semantic_query::PrintedUnionArm>> =
                    printed.iter().map(Some).collect();
                render_union_entries(store, &entries, needs, child_depth, visited)
            }
        }
        SemanticNodeData::Intersection(arms) => {
            // An intersection arm parenthesises looser binders (Conditional /
            // Function → Loose, and Union), but NOT a same-kind nested
            // intersection (`A & (B & C)` renders `A & B & C`).
            let arms: Vec<SemanticNodeId> = arms.iter().copied().collect();
            visited
                .type_node_list(&arms, true, |visited, a| {
                    render_operand(store, a, needs, child_depth, visited, Prec::Intersection)
                })
                .join(" & ")
        }
        // A same-name merged declaration renders a peer-merged surface from a
        // transient projection. Display stays read-only: semantic consumers that
        // need a graph node materialize through the reducer before rendering.
        SemanticNodeData::MergedDecl { contributors } => {
            unmodeled = true;
            let surface =
                crate::project_semantic_dispatch::walk::reduce_merged_decl_display_surface(
                    store,
                    contributors,
                );
            render_merged_decl_surface(store, &surface, needs, child_depth, visited)
        }
        SemanticNodeData::Primitive(kind) => {
            visited.add_length(primitive_length(*kind));
            primitive_keyword(*kind).to_string()
        }
        SemanticNodeData::Literal(LiteralValue::String(s))
            if needs.contains(DisplayFacet::CheckerSpelling) =>
        {
            visited.add_length(s.len() + 2);
            double_quoted(s)
        }
        SemanticNodeData::Literal(value) => {
            visited.add_length(literal_length(value));
            literal_token(value)
        }
        // A member prints as a reference through its enum (`E.A`), which the
        // node builder counts as the enum's.
        SemanticNodeData::EnumLiteral(literal) => {
            let name = reference_name(
                needs,
                &literal.enum_decl.canonical_id,
                &literal.enum_decl.decl_name,
                visited,
            );
            if literal.is_sole() {
                name
            } else {
                format!("{name}.{}", literal.member)
            }
        }
        // Error carrier riding through display — a concise token, NOT a panic.
        // A checker message has no type for it: the part is cut, so a message
        // that would show it is not reported.
        SemanticNodeData::Opaque(_) if visited.measuring() => visited.cut(),
        SemanticNodeData::Opaque(_) => "<error>".to_string(),
        SemanticNodeData::Array { element, readonly } => {
            // The leading-`readonly` postfix-base parenthesisation is handled
            // uniformly inside `render_operand` for `Prec::Postfix` operands.
            let inner = render_operand(store, *element, needs, child_depth, visited, Prec::Postfix);
            let ro = *readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier);
            if ro {
                format!("readonly {inner}[]")
            } else {
                format!("{inner}[]")
            }
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            // The node builder counts an element's type alone: a label, a
            // `?` and a `...` add nothing. Under `strictNullChecks` the
            // checker prints an optional element's type with `undefined`
            // added, as it does an optional property's.
            let checker = needs.contains(DisplayFacet::CheckerSpelling);
            let elements: Vec<&crate::semantic_query::TupleElement> = elements.iter().collect();
            let parts = visited.type_node_list(&elements, false, |visited, el| {
                let mut s = String::new();
                if el.rest {
                    s.push_str("...");
                }
                let adds_undefined = el.optional
                    && checker
                    && !visited.omit_nullish
                    && !holds_undefined(store, el.value);
                if let Some(label) = &el.label {
                    s.push_str(label);
                    if el.optional {
                        s.push('?');
                    }
                    s.push_str(": ");
                    if adds_undefined {
                        s.push_str(&render_with_undefined(
                            store,
                            el.value,
                            needs,
                            child_depth,
                            visited,
                        ));
                    } else {
                        s.push_str(
                            &display_type_node(store, el.value, needs, child_depth, visited).0,
                        );
                    }
                } else if adds_undefined && settles_to_never(store, el.value, visited) {
                    // `never` with `undefined` added is `undefined`, no union.
                    s.push_str(&render_with_undefined(
                        store,
                        el.value,
                        needs,
                        child_depth,
                        visited,
                    ));
                    s.push('?');
                } else if adds_undefined {
                    let value = render_with_undefined(store, el.value, needs, child_depth, visited);
                    s.push_str(&format!("({value})?"));
                } else if el.optional && checker {
                    s.push_str(&render_operand(
                        store,
                        el.value,
                        needs,
                        child_depth,
                        visited,
                        Prec::Postfix,
                    ));
                    s.push('?');
                } else {
                    s.push_str(&display_type_node(store, el.value, needs, child_depth, visited).0);
                    if el.optional {
                        s.push('?');
                    }
                }
                s
            });
            let body = format!("[{}]", parts.join(", "));
            if *readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
                format!("readonly {body}")
            } else {
                body
            }
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let mut s = String::from("`");
            for (i, quasi) in quasis.iter().enumerate() {
                // Quasis are the template's cooked text
                // (`verter_type_expr_oxc::template_element_text`), escaped
                // back into template source as the checker prints them.
                verter_type_expr::push_template_quasi(&mut s, quasi);
                if let Some(expr) = expressions.get(i) {
                    s.push_str("${");
                    s.push_str(&display_type_node(store, *expr, needs, child_depth, visited).0);
                    s.push('}');
                }
            }
            s.push('`');
            visited.add_length(2);
            s
        }
        SemanticNodeData::KeyOf { base } => {
            visited.add_length(6);
            let inner = render_operand(store, *base, needs, child_depth, visited, Prec::Prefix);
            format!("keyof {inner}")
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            let obj = render_operand(store, *object, needs, child_depth, visited, Prec::Postfix);
            let key = match index {
                IndexKey::String(s) => {
                    visited.add_length(s.len() + 2);
                    single_quoted(s)
                }
                IndexKey::Number(n) => {
                    let n = n.to_string();
                    visited.add_length(n.len());
                    n
                }
                IndexKey::UniqueSymbol(identity) => {
                    if needs.contains(DisplayFacet::CheckerSpelling) {
                        visited.add_length(13);
                        "unique symbol".to_owned()
                    } else {
                        format!("unique symbol {}", identity.symbol)
                    }
                }
                IndexKey::Computed(node) => {
                    display_type_node(store, *node, needs, child_depth, visited).0
                }
            };
            visited.add_length(2);
            format!("{obj}[{key}]")
        }
        SemanticNodeData::Mapped { source: _, mapper } => {
            if visited.measuring() && !visited.unsettled_types.contains(&id) {
                visited.unsettled_types.push(id);
            }
            let param = match store.node_data(mapper.parameter_node).as_deref() {
                Some(SemanticNodeData::TypeParam { display_name, .. }) => display_name.to_string(),
                _ => {
                    display_type_node(
                        store,
                        mapper.parameter_node,
                        needs,
                        child_depth,
                        &mut DisplayContext::default(),
                    )
                    .0
                }
            };
            // The builder makes the key constraint before the parameter name.
            // A keyof-declared mapping constructs the keyof node directly;
            // only its operand contributes to approximateLength.
            let key_space = match store.node_data(mapper.key_space).as_deref() {
                Some(SemanticNodeData::KeyOf { base }) => format!(
                    "keyof {}",
                    render_operand(store, *base, needs, child_depth, visited, Prec::Prefix)
                ),
                _ => display_type_node(store, mapper.key_space, needs, child_depth, visited).0,
            };
            visited.add_length(param.len() + 1);
            let scoped = visited
                .substitution
                .without_parameter(mapper.parameter_node);
            let remap = match mapper.name_remap {
                Some(node) => visited.scoped(scoped.clone(), |visited| {
                    format!(
                        " as {}",
                        display_type_node(store, node, needs, child_depth, visited).0
                    )
                }),
                None => String::new(),
            };
            let value = visited.scoped(scoped, |visited| {
                display_type_node(store, mapper.value_expr, needs, child_depth, visited).0
            });
            visited.add_length(10);
            let readonly = match mapper.readonly {
                ReadonlyMod::Add => "readonly ",
                ReadonlyMod::Remove => "-readonly ",
                ReadonlyMod::Keep => "",
            };
            let optionality = match mapper.optionality {
                OptionalityMod::Add => "?",
                OptionalityMod::Remove => "-?",
                OptionalityMod::Keep => "",
            };
            if needs.contains(DisplayFacet::CheckerSpelling) {
                format!("{{ {readonly}[{param} in {key_space}{remap}]{optionality}: {value}; }}")
            } else {
                format!("{{ {readonly}[{param} in {key_space}{remap}]{optionality}: {value} }}")
            }
        }
        // A query is a type query node over the queried name's symbol chain
        // (`symbolToTypeNode` with a value meaning), its instantiation
        // arguments built before the name is counted.
        SemanticNodeData::TypeOf(_) => {
            let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
            let type_args = data.carrier_type_args();
            let rendered = type_argument_list(store, type_args, needs, child_depth, visited);
            visited.add_length(entity_name_length(
                std::iter::once(value_root.name.as_ref())
                    .chain(path.iter().map(|seg| seg.as_ref())),
            ));
            let mut s = format!("typeof {}", value_root.name);
            for seg in path.iter() {
                s.push('.');
                s.push_str(seg);
            }
            // Instantiation args render in a `<...>` segment, matching the
            // `ImportType` / `InstantiationRef` convention; empty stays bare.
            if !type_args.is_empty() {
                s.push('<');
                s.push_str(&rendered.join(", "));
                s.push('>');
            }
            s
        }
        SemanticNodeData::TypeOfNominal(_) if needs.contains(DisplayFacet::CheckerSpelling) => {
            // The checker prints a unique symbol's type as such, not as the
            // query that names it.
            visited.add_length(13);
            "unique symbol".to_owned()
        }
        SemanticNodeData::TypeOfNominal(_) => {
            // Renders the AUTHORED head (`typeof TOKEN` / `typeof Ns.TOKEN`)
            // — the local spelling every display consumer reads; the
            // declaring identity rides the payload and is never spliced into
            // rendered type text. No instantiation segment: the terminal is
            // never an instantiation site.
            let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
            visited.add_length(entity_name_length(
                std::iter::once(value_root.name.as_ref())
                    .chain(path.iter().map(|seg| seg.as_ref())),
            ));
            let mut s = format!("typeof {}", value_root.name);
            for seg in path.iter() {
                s.push('.');
                s.push_str(seg);
            }
            s
        }
        SemanticNodeData::TypeParam { display_name, .. } => {
            visited.add_length(reference_length(display_name));
            display_name.to_string()
        }
        SemanticNodeData::Infer {
            name,
            constraint,
            binder,
        } => {
            visited.add_length(name.len() + 6);
            let constraint = constraint
                .filter(|_| needs.contains(DisplayFacet::CheckerSpelling))
                .filter(|constraint| {
                    let Some(inferred) = visited
                        .inferred_constraints
                        .get(binder)
                        .filter(|inferred| !inferred.is_empty())
                    else {
                        return true;
                    };
                    let request = InferConstraintRequest {
                        actual: *constraint,
                        inferred: inferred.clone(),
                        substitutions: visited.substitution.remaining().to_vec(),
                    };
                    if let Some(omit) = visited.infer_constraint_decisions.get(&request) {
                        return !omit;
                    }
                    visited.unsettled_infer_constraints.push(request);
                    true
                })
                .map(|constraint| {
                    visited.add_length(9);
                    display_type_node(store, constraint, needs, child_depth, visited).0
                });
            visited.add_length(name.len() + 1);
            match constraint {
                Some(constraint) => format!("infer {name} extends {constraint}"),
                None => format!("infer {name}"),
            }
        }
        SemanticNodeData::InferRef { name, .. } => {
            visited.add_length(reference_length(name));
            name.to_string()
        }
        // The node builder abbreviates a conditional it begins past the
        // truncation length whole (`conditionalTypeToTypeNode`).
        SemanticNodeData::Conditional { .. } if visited.check_truncation() => {
            visited.add_length(3);
            "...".to_owned()
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive: _,
            pending,
        } => {
            // `check` / `extends` sit before `extends` / `?`, so any compound
            // operand (Function / Conditional → Loose, Union, Intersection)
            // must parenthesise: `Prec::Prefix` wraps everything looser than a
            // prefix operator. The TRUE branch must wrap a nested Loose binder
            // (a bare nested conditional there mis-reads), so it renders at
            // `Prec::Union` (wraps Conditional / Function only). The FALSE
            // branch is the trailing, right-associative position and stays bare
            // — `A extends B ? C : D extends E ? F : G` already nests correctly.
            let scoped = visited.substitution.in_conditional_scope(store, *extends);
            let true_substitution = scoped.prepend(
                pending
                    .as_ref()
                    .map_or(&[], |frame| frame.true_branch().pairs()),
            );
            let false_substitution = visited.substitution.prepend(
                pending
                    .as_ref()
                    .map_or(&[], |frame| frame.false_branch().pairs()),
            );
            let inferred = inferred_constraints(store, *check, *extends);
            let check = render_operand(store, *check, needs, child_depth, visited, Prec::Prefix);
            visited.add_length(15);
            let outer_infers = std::mem::replace(&mut visited.inferred_constraints, inferred);
            let extends = visited.scoped(scoped, |visited| {
                render_operand(store, *extends, needs, child_depth, visited, Prec::Prefix)
            });
            visited.inferred_constraints = outer_infers;
            let t = visited.scoped(true_substitution, |visited| {
                render_operand(
                    store,
                    *true_branch_ref,
                    needs,
                    child_depth,
                    visited,
                    Prec::Union,
                )
            });
            let f = visited.scoped(false_substitution, |visited| {
                display_type_node(store, *false_branch_ref, needs, child_depth, visited).0
            });
            format!("{check} extends {extends} ? {t} : {f}")
        }
        SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            type_parameters,
            predicate,
            is_abstract,
            ..
        } => {
            // A function or constructor type is the checker's object type with
            // one signature, so it elides as `...` when it begins past the
            // truncation length, as [`render_object_surface`] does for one
            // lowered as an object (TypeScript 7.1.0-beta:
            // `{ padding: "<155 x>"; fun: () => void }` prints `fun: ...`).
            if visited.check_truncation() {
                visited.add_length(3);
                "...".to_string()
            } else {
                let new_prefix = match kind {
                    crate::semantic_query::SignatureKind::Call => "",
                    crate::semantic_query::SignatureKind::Construct if *is_abstract => {
                        "abstract new "
                    }
                    crate::semantic_query::SignatureKind::Construct => "new ",
                };
                // The fewest a signature prints beside its parts
                // (`signatureToSignatureDeclarationHelper`).
                visited.add_length(3);
                let tps =
                    render_type_parameters(store, type_parameters, needs, child_depth, visited);
                let rendered_params = render_params(store, params, needs, child_depth, visited);
                let ret = render_return(
                    store,
                    *return_type,
                    *predicate,
                    params,
                    needs,
                    child_depth,
                    visited,
                );
                format!("{new_prefix}{tps}({rendered_params}) => {ret}")
            }
        }
        // Lazy named references: render the NAME (§14.3 — a lazy ref carries no
        // materialised body, and display must not re-resolve, so `ExpandAliases`
        // is a no-op here). `QualifyNames` qualifies with the declaration origin.
        SemanticNodeData::DeclRef { identity } => {
            reference_name(needs, &identity.canonical_id, &identity.decl_name, visited)
        }
        // A class expression renders by the name the checker prints for a
        // reference to it (`Mixin.(Anonymous class)`, `(Anonymous
        // class)<string>`) — like a lazy reference, its body is never
        // re-rendered here. An object literal's recursive anonymous type has
        // no name: it renders as its surface, whose own `this` renders as
        // `this`.
        SemanticNodeData::ClassExpressionInstance {
            identity, surface, ..
        } if identity.object_literal => display_type_node(store, *surface, needs, depth, visited).0,
        SemanticNodeData::ClassExpressionInstance {
            identity,
            type_arguments,
            ..
        } => {
            // A reference to the class: its own type arguments, then the
            // symbol chain of its printed name.
            let printed = identity.printed_name_in(store, type_arguments);
            let own = identity.own_type_arguments(type_arguments);
            let rendered = type_argument_list(store, own, needs, child_depth, visited);
            visited.add_length(entity_name_length(printed.split('.')));
            // The chain of a class expression's printed name is not modelled.
            let name = if checker_qualified(needs) {
                visited.cut()
            } else {
                qualified_name(needs, &identity.canonical_id, &printed)
            };
            if own.is_empty() {
                name
            } else {
                format!("{name}<{}>", rendered.join(", "))
            }
        }
        // A compiler-native operation renders by its OP name — there is no
        // declaration to qualify, because none was applied.
        //
        // The node builder builds a reference's type arguments before it
        // counts the referenced name (`typeReferenceToTypeNode` and the alias
        // path), so the arguments see the count from before the name.
        // `Awaited<T>` is the lib alias over its argument list; `NoInfer<T>`
        // is a substitution type, whose base type builds bare, outside any
        // list.
        SemanticNodeData::IntrinsicApplication { op, args } => {
            let rendered = match op {
                verter_type_expr::CompilerIntrinsicTypeOp::Awaited => {
                    type_argument_list(store, args, needs, child_depth, visited)
                }
                verter_type_expr::CompilerIntrinsicTypeOp::NoInfer => args
                    .iter()
                    .map(|arg| display_type_node(store, *arg, needs, child_depth, visited).0)
                    .collect(),
            };
            visited.add_length(reference_length(op.display_name()));
            format!("{}<{}>", op.display_name(), rendered.join(", "))
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            let rendered = type_argument_list(store, args, needs, child_depth, visited);
            let name = reference_name(needs, &base.canonical_id, &base.decl_name, visited);
            if args.is_empty() {
                name
            } else {
                format!("{name}<{}>", rendered.join(", "))
            }
        }
        // Unresolved bare-name carrier renders as its written name plus any
        // applied type arguments (`Foo<Arg>`), matching the `ImportType` /
        // `InstantiationRef` convention; an empty `type_args` stays bare.
        // Display never re-resolves a carrier — the args render structurally.
        SemanticNodeData::BareRef(_) => {
            let (name, _scope) = data.bare_ref_head().expect("BareRef carrier head");
            let type_args = data.carrier_type_args();
            let rendered = type_argument_list(store, type_args, needs, child_depth, visited);
            visited.add_length(reference_length(name));
            if type_args.is_empty() {
                name.to_string()
            } else {
                format!("{name}<{}>", rendered.join(", "))
            }
        }
        // Dynamic-import carrier renders the import expression + qualifier
        // path + any type arguments.
        //
        // The node builder counts an import type's specifier and its
        // `import("")`, and each qualifier name and its separator, after its
        // type arguments.
        SemanticNodeData::ImportType(_) => {
            let (specifier, qualifier, typeof_query) =
                data.import_type_head().expect("ImportType carrier head");
            let type_args = data.carrier_type_args();
            let rendered = type_argument_list(store, type_args, needs, child_depth, visited);
            visited.add_length(
                specifier.len()
                    + 10
                    + qualifier
                        .iter()
                        .map(|segment| segment.len() + 1)
                        .sum::<usize>(),
            );
            let mut out = if typeof_query {
                format!("typeof import(\"{specifier}\")")
            } else {
                format!("import(\"{specifier}\")")
            };
            for segment in qualifier.iter() {
                out.push('.');
                out.push_str(segment);
            }
            if !type_args.is_empty() {
                out.push('<');
                out.push_str(&rendered.join(", "));
                out.push('>');
            }
            out
        }
        // Raw-fallback carrier renders its preserved text verbatim.
        SemanticNodeData::RawFallback { value } => {
            unmodeled = true;
            value.raw().to_string()
        }
        // Synthetic-binding carrier renders the bound name.
        SemanticNodeData::SyntheticBinding { id, .. } => {
            unmodeled = true;
            id.binding_name.to_string()
        }
        // The sealed callable carrier has no observable return, so it has
        // no function-type rendering; it is never part of a displayed type.
        SemanticNodeData::DeferredCallable(_) => {
            unmodeled = true;
            "DeferredCallable".to_string()
        }
    };
    if unmodeled {
        visited.unmodeled(&out);
    }
    visited.pop();
    DisplayString(out)
}

/// A reference's type arguments through the node builder's list: elided
/// whole as `...` when the list begins past the truncation length.
fn type_argument_list(
    store: &SemanticGraphStore,
    args: &[SemanticNodeId],
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> Vec<String> {
    visited.type_node_list(args, false, |visited, arg| {
        display_type_node(store, arg, needs, depth, visited).0
    })
}

/// An object type's surface, its properties, then its call, construct and
/// index signatures. Counted as the checker's node builder builds it
/// (`createTypeNodesFromResolvedType`): the signatures first, then each
/// property, and once the count passes the truncation length the remaining
/// properties elide as `... N more ...` before the last, or the whole
/// surface as `...` when it begins past it (TypeScript 7.1.0-beta; the
/// checker no longer prints an elided type literal as `{ ...; }`).
fn render_object_surface(
    store: &SemanticGraphStore,
    surface: &crate::semantic_query::SurfaceView,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    let checker = needs.contains(DisplayFacet::CheckerSpelling);
    if surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && surface.index_signatures.is_empty()
    {
        visited.add_length(2);
        return "{}".to_string();
    }
    // An object type begun past the truncation length elides before it would
    // print as its sole signature's function type (TypeScript 7.1.0-beta).
    if visited.check_truncation() {
        visited.add_length(3);
        return "...".to_string();
    }
    if checker {
        if let Some((signature, prefix)) = sole_signature(store, surface) {
            return format!(
                "{prefix}{}",
                display_type_node(store, signature, needs, depth, visited).0
            );
        }
    }
    let mut signatures: Vec<String> = Vec::new();
    for sig in surface.call_signatures.iter() {
        signatures.push(render_signature_colon(
            store, *sig, "", needs, depth, visited,
        ));
    }
    for sig in surface.construct_signatures.iter() {
        signatures.push(render_signature_colon(
            store, *sig, "new ", needs, depth, visited,
        ));
    }
    for index in surface.index_signatures.iter() {
        let mut s = String::new();
        if index.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
            s.push_str("readonly ");
        }
        // The node builder builds the key type, then the value type, then
        // counts the parameter name and four more (TypeScript 7.1.0-beta:
        // `{ t: { [k: string]: "<141 x>" }; b: Wrapper<string> }` prints
        // whole, 142 abbreviates `Wrapper<...>`); `readonly` counts as on a
        // property. The checker prints a signature no declaration wrote with
        // the parameter `x`.
        let key = display_type_node(store, index.key_type, needs, depth, visited).0;
        let value = display_type_node(store, index.value_type, needs, depth, visited).0;
        let name = match (&index.key_name, checker) {
            (Some(name), true) => name.as_ref(),
            (None, true) => "x",
            (_, false) => "key",
        };
        visited.add_length(name.len() + 4);
        if index.readonly {
            visited.add_length(9);
        }
        s.push_str(&format!("[{name}: {key}]: {value}"));
        signatures.push(s);
    }
    let members: Vec<&crate::semantic_query::SurfaceMember> = surface.members.iter().collect();
    let mut parts: Vec<String> = Vec::new();
    let render_member = |visited: &mut DisplayContext,
                         member: &crate::semantic_query::SurfaceMember| {
        let mut s = String::new();
        if member.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
            s.push_str("readonly ");
        }
        // The node builder counts a property's symbol name and its colon
        // before its type (`addPropertyToElementList`).
        let name = match &member.key {
            crate::semantic_query::AuthoredPropertyKey::String(name) => {
                visited.add_length(name.len() + 1);
                if checker && !is_bare_member_name(name) {
                    double_quoted(name)
                } else {
                    member_name_token(name)
                }
            }
            crate::semantic_query::AuthoredPropertyKey::Number(number) => {
                let number = number.to_string();
                visited.add_length(number.len() + 1);
                number
            }
            crate::semantic_query::AuthoredPropertyKey::UniqueSymbol(identity) => {
                if checker {
                    let leaf = identity
                        .member_path
                        .last()
                        .map_or(identity.symbol.as_ref(), |name| name.as_str());
                    let mut name = identity.symbol.to_string();
                    for segment in identity.member_path.iter() {
                        name.push('.');
                        name.push_str(segment);
                    }
                    visited.symbol_property_length(&name, leaf);
                    format!("[{name}]")
                } else {
                    format!("[unique symbol {}]", identity.symbol)
                }
            }
            crate::semantic_query::AuthoredPropertyKey::Computed(node) => {
                let (mut key, _) = follow_alias_chain(store, *node, &visited.substitution);
                if checker {
                    if let Some(settled) = visited.settled_types.get(&key) {
                        key = follow_alias_chain(store, *settled, &visited.substitution).0;
                    } else if matches!(
                        store.node_data(key).as_deref(),
                        Some(SemanticNodeData::TypeOf(_))
                    ) && !visited.unsettled_types.contains(&key)
                    {
                        visited.unsettled_types.push(key);
                    }
                    // Only a key uses the enum member's literal base. The
                    // same node in a value position keeps its nominal name.
                    if let Some(SemanticNodeData::EnumLiteral(member)) =
                        store.node_data(key).as_deref()
                    {
                        key = member.base;
                    }
                }
                match store.node_data(key).as_deref() {
                    Some(SemanticNodeData::Literal(LiteralValue::String(name))) if checker => {
                        visited.add_length(name.len() + 1);
                        if is_bare_member_name(name) {
                            name.to_string()
                        } else {
                            double_quoted(name)
                        }
                    }
                    Some(SemanticNodeData::Literal(LiteralValue::Number(number))) if checker => {
                        let name = number.to_string();
                        visited.add_length(name.len() + 1);
                        name
                    }
                    Some(data @ SemanticNodeData::TypeOfNominal(_)) if checker => {
                        let (root, path) = data.typeof_head().expect("nominal query head");
                        let mut name = root.name.to_string();
                        for segment in path.iter() {
                            name.push('.');
                            name.push_str(segment);
                        }
                        visited.symbol_property_length(
                            &name,
                            path.last().map_or(root.name.as_ref(), |name| name.as_ref()),
                        );
                        format!("[{name}]")
                    }
                    _ => {
                        let name = format!(
                            "[{}]",
                            display_type_node(store, *node, needs, depth, visited).0
                        );
                        visited.unmodeled(&name);
                        name
                    }
                }
            }
        };
        s.push_str(&name);
        if member.optional {
            s.push('?');
        }
        if member.method_kind.is_some() && resolves_to_function(store, member.value, visited) {
            s.push_str(&render_signature_colon(
                store,
                member.value,
                "",
                needs,
                depth,
                visited,
            ));
        } else if member.optional
            && checker
            && !visited.omit_nullish
            && !holds_undefined(store, member.value)
        {
            // Under `strictNullChecks` the checker prints an optional
            // property's type with `undefined` added: one union, its
            // members then `undefined`.
            s.push_str(": ");
            s.push_str(&render_with_undefined(
                store,
                member.value,
                needs,
                depth,
                visited,
            ));
        } else {
            s.push_str(": ");
            s.push_str(&display_type_node(store, member.value, needs, depth, visited).0);
        }
        if member.readonly {
            visited.add_length(9);
        }
        s
    };
    let count = members.len();
    for (index, member) in members.iter().enumerate() {
        let shown = index + 1;
        if shown + 2 < count - 1 && visited.check_truncation() {
            parts.push(format!("... {} more ...", count - shown));
            parts.push(render_member(visited, members[count - 1]));
            break;
        }
        parts.push(render_member(visited, member));
    }
    visited.add_length(2);
    if checker {
        // The checker prints the signatures before the properties, in the
        // order it builds them.
        signatures.extend(parts);
        format!("{{ {}; }}", signatures.join("; "))
    } else {
        parts.extend(signatures);
        format!("{{ {} }}", parts.join("; "))
    }
}

/// The one signature of an object type the checker prints as a function or
/// constructor type (`createTypeNodeFromObjectType`) — no property and no
/// index signature, and one call signature and no construct signature, or
/// one construct signature and no call signature — with the `new ` its
/// print adds where the signature does not carry its own.
fn sole_signature(
    store: &SemanticGraphStore,
    surface: &crate::semantic_query::SurfaceView,
) -> Option<(SemanticNodeId, &'static str)> {
    if !surface.members.is_empty() || !surface.index_signatures.is_empty() {
        return None;
    }
    match (
        &surface.call_signatures[..],
        &surface.construct_signatures[..],
    ) {
        ([call], []) => Some((*call, "")),
        ([], [construct]) => {
            let (resolved, _) =
                follow_alias_chain(store, *construct, &DisplaySubstitution::default());
            let carries_new = matches!(
                store.node_data(resolved).as_deref(),
                Some(SemanticNodeData::Signature {
                    kind: crate::semantic_query::SignatureKind::Construct,
                    ..
                })
            );
            Some((*construct, if carries_new { "" } else { "new " }))
        }
        _ => None,
    }
}

/// What the node builder counts for an entity name it builds from a symbol
/// chain (`createAccessFromSymbolChain`): each name and its separator, the
/// first counted twice.
fn entity_name_length<'s>(mut segments: impl Iterator<Item = &'s str>) -> usize {
    segments.next().map_or(0, reference_length)
        + segments.map(|segment| segment.len() + 1).sum::<usize>()
}

/// `value` with `undefined` added, as the checker prints an optional
/// property's type under `strictNullChecks`: a union's members then
/// `undefined`, as one list, and `never` with it added `undefined` alone.
fn render_with_undefined(
    store: &SemanticGraphStore,
    value: SemanticNodeId,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    if settles_to_never(store, value, visited) {
        visited.add_length(primitive_length(PrimitiveKind::Undefined));
        return primitive_keyword(PrimitiveKind::Undefined).to_owned();
    }
    let (resolved, substitution) = follow_alias_chain(store, value, &visited.substitution);
    let arms = match store.node_data(resolved).as_deref() {
        Some(SemanticNodeData::Union(arms)) if !visited.contains(&resolved) => {
            Some(printed_union_arms_in_order(store, arms, needs, visited))
        }
        _ => None,
    };
    match arms {
        Some(arms) => visited.scoped(substitution, |visited| {
            let entries: Vec<Option<&crate::semantic_query::PrintedUnionArm>> =
                arms.iter().map(Some).chain(std::iter::once(None)).collect();
            render_union_entries(store, &entries, needs, depth, visited)
        }),
        None => {
            let arm = crate::semantic_query::PrintedUnionArm::Node(value);
            render_union_entries(store, &[Some(&arm), None], needs, depth, visited)
        }
    }
}

/// A union's printed members in the order a checker-spelled union prints
/// them, without `null` and `undefined` when they are omitted.
fn printed_union_arms_in_order(
    store: &SemanticGraphStore,
    arms: &[SemanticNodeId],
    needs: DisplayNeeds,
    visited: &DisplayContext,
) -> Vec<crate::semantic_query::PrintedUnionArm> {
    let mut printed = crate::semantic_query::printed_union_arms(store, arms);
    if visited.omit_nullish {
        let is_nullish = |arm: &crate::semantic_query::PrintedUnionArm| {
            matches!(
                arm,
                crate::semantic_query::PrintedUnionArm::Node(node)
                    if matches!(
                        store.node_data(*node).as_deref(),
                        Some(SemanticNodeData::Primitive(
                            PrimitiveKind::Null | PrimitiveKind::Undefined
                        ))
                    )
            )
        };
        if printed.iter().any(|arm| !is_nullish(arm)) {
            printed.retain(|arm| !is_nullish(arm));
        }
    }
    // A checker-spelled union prints its constituents in the checker's own
    // id order (`sortAndDeduplicateTypes`), not the stored one: the
    // intrinsics by their creation — `string`, `number`, `bigint`,
    // `boolean`, `symbol`, then `null` and `undefined` — before the types a
    // program declares, with the literals a check interned last. The sort
    // is stable, so constituents that timeline leaves unranked keep the
    // stored order.
    if needs.contains(DisplayFacet::CheckerSpelling) {
        printed.sort_by_key(|arm| checker_union_constituent_rank(store, arm));
    }
    printed
}

/// A union's entries joined by ` | `, through the node builder's list
/// (`None` is an `undefined` the checker added).
fn render_union_entries(
    store: &SemanticGraphStore,
    entries: &[Option<&crate::semantic_query::PrintedUnionArm>],
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    visited
        .type_node_list(entries, true, |visited, entry| match entry {
            Some(crate::semantic_query::PrintedUnionArm::Node(arm)) => {
                render_operand(store, *arm, needs, depth, visited, Prec::Union)
            }
            Some(crate::semantic_query::PrintedUnionArm::Enum(decl)) => {
                reference_name(needs, &decl.canonical_id, &decl.decl_name, visited)
            }
            None => {
                visited.add_length(9);
                "undefined".to_owned()
            }
        })
        .join(" | ")
}

/// What the node builder counts for a reference to the symbol `name`
/// (`createAccessFromSymbolChain`): the root of the chain is counted twice,
/// its name and a separator each time.
fn reference_length(name: &str) -> usize {
    2 * (name.len() + 1)
}

/// What the node builder counts for a primitive keyword. `unknown` counts
/// nothing.
fn primitive_length(kind: PrimitiveKind) -> usize {
    match kind {
        PrimitiveKind::Unknown => 0,
        PrimitiveKind::Any => 3,
        PrimitiveKind::Void | PrimitiveKind::Null => 4,
        PrimitiveKind::Never => 5,
        PrimitiveKind::String
        | PrimitiveKind::Number
        | PrimitiveKind::BigInt
        | PrimitiveKind::Symbol
        | PrimitiveKind::Object => 6,
        PrimitiveKind::Boolean => 7,
        PrimitiveKind::Undefined => 9,
    }
}

/// What the node builder counts for a literal type: a string's value and
/// its quotes, a number as written, a bigint's digits and its `n`.
fn literal_length(value: &LiteralValue) -> usize {
    match value {
        LiteralValue::String(value) => value.len() + 2,
        LiteralValue::Number(value) => value.to_string().len(),
        LiteralValue::BigInt(digits) => digits.len() + 1,
        LiteralValue::Boolean(true) => 4,
        LiteralValue::Boolean(false) => 5,
    }
}

fn render_merged_decl_surface(
    store: &SemanticGraphStore,
    surface: &MergedDeclDisplaySurface,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    let own = render_merged_decl_own_surface(store, &surface.own_surface, needs, depth, visited);
    if surface.heritage_arms.is_empty() {
        return own;
    }
    let mut rendered: Vec<String> = surface
        .heritage_arms
        .iter()
        .map(|arm| render_operand(store, *arm, needs, depth, visited, Prec::Intersection))
        .collect();
    rendered.push(own);
    rendered.join(" & ")
}

fn render_merged_decl_own_surface(
    store: &SemanticGraphStore,
    surface: &MergedDeclSurface,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for member in &surface.members {
        parts.push(render_merged_decl_member(
            store, member, needs, depth, visited,
        ));
    }
    for sig in surface.call_signatures.iter() {
        parts.push(render_signature_colon(
            store, *sig, "", needs, depth, visited,
        ));
    }
    for sig in surface.construct_signatures.iter() {
        parts.push(render_signature_colon(
            store, *sig, "new ", needs, depth, visited,
        ));
    }
    for index in surface.index_signatures.iter() {
        let mut s = String::new();
        if index.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
            s.push_str("readonly ");
        }
        s.push_str(&format!(
            "[key: {}]: {}",
            display_type_node(store, index.key_type, needs, depth, visited).0,
            display_type_node(store, index.value_type, needs, depth, visited).0
        ));
        parts.push(s);
    }
    if parts.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", parts.join("; "))
    }
}

/// Render one peer-merged member. This mirrors the `Object` arm of
/// [`display_type_node`] applied to the graph reducer's reduced surface: a
/// single-signature method renders as method shorthand (`name(params): ret`);
/// an accumulated overload group (`values.len() > 1`) renders as a PROPERTY
/// holding the intersection of its function types — exactly what the graph
/// reducer's interned `Intersection` overload value renders as (an `is_method`
/// member whose value is an `Intersection` falls through to property style to
/// stay valid TS). Display therefore never emits separate method signatures
/// where the reduced Object would not.
fn render_merged_decl_member(
    store: &SemanticGraphStore,
    merged: &MergedDeclMember,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    let member = &merged.member;
    let mut s = member_prefix(store, member, needs, depth, visited);
    match merged.values.as_slice() {
        [value] if member.method_kind.is_some() && resolves_to_function(store, *value, visited) => {
            s.push_str(&render_signature_colon(
                store, *value, "", needs, depth, visited,
            ));
        }
        [value] => {
            s.push_str(": ");
            s.push_str(&display_type_node(store, *value, needs, depth, visited).0);
        }
        [] => {
            s.push_str(": ");
            s.push_str(TRUNCATION_TOKEN);
        }
        values => {
            s.push_str(": ");
            let rendered: Vec<String> = values
                .iter()
                .map(|value| {
                    render_operand(store, *value, needs, depth, visited, Prec::Intersection)
                })
                .collect();
            s.push_str(&rendered.join(" & "));
        }
    }
    s
}

fn member_prefix(
    store: &SemanticGraphStore,
    member: &ShallowSurfaceMember,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    let mut s = String::new();
    if member.readonly && needs.contains(DisplayFacet::IncludeReadonlyModifier) {
        s.push_str("readonly ");
    }
    s.push_str(&match &member.key {
        crate::semantic_query::AuthoredPropertyKey::String(name) => member_name_token(name),
        crate::semantic_query::AuthoredPropertyKey::Number(number) => number.to_string(),
        crate::semantic_query::AuthoredPropertyKey::UniqueSymbol(identity) => {
            format!("[unique symbol {}]", identity.symbol)
        }
        crate::semantic_query::AuthoredPropertyKey::Computed(node) => format!(
            "[{}]",
            display_type_node(store, *node, needs, depth.saturating_sub(1), visited).0
        ),
    });
    if member.optional {
        s.push('?');
    }
    s
}

/// Render one overload signature (a graph node, typically a
/// [`SemanticNodeData::Signature`]).
fn display_signature(
    store: &SemanticGraphStore,
    sig: &SignatureRef,
    needs: DisplayNeeds,
) -> DisplayString {
    display_type_node(
        store,
        sig.node,
        needs,
        MAX_DISPLAY_DEPTH,
        &mut DisplayContext::default(),
    )
}

/// Render a relation outcome from the payload — never recomputed (§14.2).
/// Renders the `outcome` only; the payload-side proof table never appears on a
/// display surface. The public outcome is three-valued — `Assignable` /
/// `NotAssignable` / `BudgetExceeded` — and has NO `Unknown` form (a deferred /
/// undischarged relation routes through `ReturnOnly` and never surfaces here).
fn display_relation(payload: &RelationPayload) -> DisplayString {
    let token = match &payload.outcome {
        RelationOutcome::Assignable => "true",
        RelationOutcome::NotAssignable => "false",
        RelationOutcome::BudgetExceeded(_) => "budget-exceeded",
    };
    DisplayString(token.to_string())
}

/// Render the merged declaration / augmentation surface: each contributor node
/// rendered shallow, joined as a merge (`&`). Contributors render at
/// `Prec::Intersection` — identical to the `Intersection` arm — so a looser
/// contributor (Union / Conditional / Function) parenthesises rather than
/// mis-binding under the implicit `&`.
fn display_declaration_analysis(
    store: &SemanticGraphStore,
    value: &DeclarationAnalysisValue,
    needs: DisplayNeeds,
) -> DisplayString {
    let rendered: Vec<String> = value
        .contributors
        .iter()
        .map(|c| {
            render_operand(
                store,
                *c,
                needs,
                MAX_DISPLAY_DEPTH,
                &mut DisplayContext::default(),
                Prec::Intersection,
            )
        })
        .collect();
    DisplayString(rendered.join(" & "))
}

/// Render a narrowed / contextual program-analysis node.
fn display_program_analysis(
    store: &SemanticGraphStore,
    value: &ProgramAnalysisValue,
    needs: DisplayNeeds,
) -> DisplayString {
    display_type_node(
        store,
        value.node,
        needs,
        MAX_DISPLAY_DEPTH,
        &mut DisplayContext::default(),
    )
}

/// The back-reference token emitted when a node is already on the ancestor
/// stack: its name when the node carries one, else the truncation token —
/// in a checker message the `...` the node builder prints for a type it is
/// already printing.
fn back_ref_token(
    data: &SemanticNodeData,
    needs: DisplayNeeds,
    visited: &mut DisplayContext,
) -> String {
    if checker_qualified(needs) {
        match data {
            SemanticNodeData::DeclRef { identity } => {
                return reference_name(needs, &identity.canonical_id, &identity.decl_name, visited);
            }
            SemanticNodeData::InstantiationRef { base, .. } => {
                return reference_name(needs, &base.canonical_id, &base.decl_name, visited);
            }
            _ => {}
        }
    }
    let name = match data {
        SemanticNodeData::DeclRef { identity } => identity.decl_name.to_string(),
        SemanticNodeData::ClassExpressionInstance { identity, .. } => identity.name.to_string(),
        SemanticNodeData::InstantiationRef { base, .. } => base.decl_name.to_string(),
        SemanticNodeData::IntrinsicApplication { op, .. } => op.display_name().to_string(),
        SemanticNodeData::TypeParam { display_name, .. } => display_name.to_string(),
        _ if visited.measuring() => {
            visited.add_length(3);
            return "...".to_owned();
        }
        _ => return TRUNCATION_TOKEN.to_string(),
    };
    visited.add_length(reference_length(&name));
    name
}

/// Whether names print as the checker's `UseFullyQualifiedType` does.
fn checker_qualified(needs: DisplayNeeds) -> bool {
    needs.contains(DisplayFacet::QualifyNames) && needs.contains(DisplayFacet::CheckerSpelling)
}

/// A reference to a declaration: counted as the node builder counts it, and
/// spelled through its symbol chain in a fully qualified message print
/// (`symbolToTypeNode`). A chain not prepared yet is requested, and the
/// print repeated.
fn reference_name(
    needs: DisplayNeeds,
    canonical_id: &verter_type_expr::OwnedStr,
    decl_name: &verter_type_expr::OwnedStr,
    visited: &mut DisplayContext,
) -> String {
    if !checker_qualified(needs) {
        visited.add_length(reference_length(decl_name));
        return qualified_name(needs, canonical_id, decl_name);
    }
    let declaration = DeclarationName {
        canonical_id: canonical_id.clone(),
        decl_name: decl_name.clone(),
    };
    match visited.declaration_chains.get(&declaration) {
        Some(DeclarationChain::Unqualified) => {
            visited.add_length(reference_length(decl_name));
            decl_name.to_string()
        }
        Some(DeclarationChain::Module { specifier, name }) => {
            let text = format!("import(\"{specifier}\").{name}");
            let length = specifier.len() + 10 + entity_name_length(name.split('.'));
            visited.add_length(length);
            text
        }
        Some(DeclarationChain::Unknown) => visited.cut(),
        None => {
            visited.unchained_declarations.push(declaration);
            decl_name.to_string()
        }
    }
}

/// A declaration name, optionally qualified with its canonical origin when
/// [`DisplayFacet::QualifyNames`] is set. Never fabricates an origin: the bare
/// name is used when qualification is not requested.
fn qualified_name(needs: DisplayNeeds, canonical_id: &str, decl_name: &str) -> String {
    if needs.contains(DisplayFacet::QualifyNames) {
        format!("{canonical_id}:{decl_name}")
    } else {
        decl_name.to_string()
    }
}

/// Canonical primitive keyword spellings — match
/// `resolver_core::surface_projector`'s leaf conventions so the two renderers
/// agree (this walks `SemanticNodeData`, not `TypeExpr`, so it cannot call it).
fn primitive_keyword(kind: PrimitiveKind) -> &'static str {
    match kind {
        PrimitiveKind::String => "string",
        PrimitiveKind::Number => "number",
        PrimitiveKind::Boolean => "boolean",
        PrimitiveKind::Symbol => "symbol",
        PrimitiveKind::BigInt => "bigint",
        PrimitiveKind::Any => "any",
        PrimitiveKind::Unknown => "unknown",
        PrimitiveKind::Void => "void",
        PrimitiveKind::Never => "never",
        PrimitiveKind::Null => "null",
        PrimitiveKind::Undefined => "undefined",
        PrimitiveKind::Object => "object",
    }
}

/// Canonical literal spellings — string literals single-quoted (and escaped);
/// number / bool / bigint verbatim (matches `surface_projector`'s leaf
/// conventions).
fn literal_token(value: &LiteralValue) -> String {
    match value {
        LiteralValue::String(s) => single_quoted(s),
        LiteralValue::Number(n) => n.to_string(),
        LiteralValue::Boolean(b) => b.to_string(),
        // `LiteralValue::BigInt` stores base-10 digits WITHOUT the `n` suffix
        // (`verter_type_expr_oxc` = `b.value.to_string()`); the suffix is what
        // makes it a bigint-literal type rather than a number literal, so append
        // it. The stored string never carries `n`, so this never double-appends.
        LiteralValue::BigInt(s) => format!("{s}n"),
    }
}

/// Render an object member name. A name that is a valid bare TS member key (a
/// valid identifier, or an all-digit numeric key — both legal unquoted) is
/// emitted as-is; any other name (e.g. `"foo-bar"`, `"has space"`) is quoted as
/// a single-quoted escaped string so the surface stays valid TS.
fn member_name_token(name: &str) -> String {
    if is_bare_member_name(name) {
        name.to_string()
    } else {
        single_quoted(name)
    }
}

/// Structural test for a name that may appear unquoted as an object member key:
/// a valid TS identifier (`[A-Za-z_$][A-Za-z0-9_$]*`) or an all-ASCII-digit
/// numeric key (`123:` is legal unquoted).
fn is_bare_member_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if name.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Render `s` as a double-quoted string literal, as the checker spells a
/// string literal type in a message (`"a"`), escaping `\`, `"` and control
/// characters.
fn double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render `s` as a single-quoted TS string literal, escaping `\`, `'`, and
/// every control char (`< 0x20`) so the rendered token is valid (and round-trips
/// through a parser without splicing the surrounding type).
fn single_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Any remaining non-printing control char (NUL, backspace,
            // form-feed, vertical-tab, …) escapes generically as `\u{..}` so the
            // rendered token stays a valid TS string literal.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// Render a function's type-parameter clause `<T extends C = D, …>`. Empty when
/// the function declares no type parameters. Constraint and default are LIVE
/// [`TypeParamDecl`] fields and are rendered when present; a bare parameter is
/// just its name.
fn render_type_parameters(
    store: &SemanticGraphStore,
    type_parameters: &[TypeParamDecl],
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    if type_parameters.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = type_parameters
        .iter()
        .map(|tp| {
            let mut s = tp.name.to_string();
            if let Some(constraint) = tp.constraint {
                s.push_str(" extends ");
                s.push_str(&display_type_node(store, constraint, needs, depth, visited).0);
            }
            if let Some(default) = tp.default {
                s.push_str(" = ");
                s.push_str(&display_type_node(store, default, needs, depth, visited).0);
            }
            s
        })
        .collect();
    format!("<{}>", rendered.join(", "))
}

/// Render a parameter list body `name: T, …` (no surrounding parens). Shared by
/// the standalone-function arrow rendering and the object-position colon
/// rendering.
fn render_params(
    store: &SemanticGraphStore,
    params: &[FunctionParam],
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    let rendered: Vec<String> = params
        .iter()
        .map(|p| {
            let mut ps = String::new();
            if p.rest {
                ps.push_str("...");
            }
            let name = p.name.as_deref().unwrap_or("arg");
            ps.push_str(name);
            if p.optional {
                ps.push('?');
            }
            ps.push_str(": ");
            ps.push_str(&display_type_node(store, p.ty, needs, depth, visited).0);
            // Counted after its type (`symbolToParameterDeclaration`).
            visited.add_length(name.len() + 3);
            ps
        })
        .collect();
    rendered.join(", ")
}

/// Render a signature's return position the way TypeScript prints it: its
/// type predicate when it carries one (`x is T`, `asserts x`,
/// `asserts this is T`), never the `boolean` / `void` return that
/// predicate denotes; otherwise the return type.
fn render_return(
    store: &SemanticGraphStore,
    return_type: SemanticNodeId,
    predicate: Option<crate::semantic_query::SignaturePredicate>,
    params: &[FunctionParam],
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    let Some(predicate) = predicate else {
        return display_type_node(store, return_type, needs, depth, visited).0;
    };
    let subject = match predicate.subject {
        crate::semantic_query::PredicateSubject::This => "this",
        crate::semantic_query::PredicateSubject::Parameter(_) => predicate
            .subject_parameter(params)
            .and_then(|param| param.name.as_deref())
            .unwrap_or("arg"),
    };
    let mut rendered = String::new();
    if predicate.asserts {
        rendered.push_str("asserts ");
    }
    rendered.push_str(subject);
    // The node builder counts a predicate's type alone
    // (`typePredicateToTypePredicateNode`).
    if let Some(target) = predicate.ty {
        rendered.push_str(" is ");
        rendered.push_str(&display_type_node(store, target, needs, depth, visited).0);
    }
    rendered
}

/// Render a [`SemanticNodeData::Signature`] signature node in object / type-
/// literal position, where the return type is introduced by a COLON
/// (`(params): ret`) — distinct from the standalone arrow form. `prefix` is
/// `""` for a call signature, `"new "` for a construct signature, or already
/// holds the member name for a method shorthand.
fn render_signature_colon(
    store: &SemanticGraphStore,
    sig: SemanticNodeId,
    prefix: &str,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
) -> String {
    // Follow the transparent `Alias` chain to the underlying node before
    // matching: a member with `value = Alias(Function)` enters method shorthand
    // (via `resolves_to_function`, which also follows the chain), so the colon
    // form must reach the same `Function` — otherwise an `Alias` falls to the
    // defensive arm and renders the arrow form, an invalid `name(p) => r` hybrid.
    let (resolved, substitution) = follow_alias_chain(store, sig, &visited.substitution);
    visited.scoped(substitution, |visited| {
        match store.node_data(resolved).as_deref() {
            Some(SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                predicate,
                ..
            }) => {
                visited.add_length(3);
                let tps = render_type_parameters(store, type_parameters, needs, depth, visited);
                let rendered_params = render_params(store, params, needs, depth, visited);
                let ret = render_return(
                    store,
                    *return_type,
                    *predicate,
                    params,
                    needs,
                    depth,
                    visited,
                );
                format!("{prefix}{tps}({rendered_params}): {ret}")
            }
            // Defensive: a genuinely non-function signature node renders through its
            // own arm, prefixed verbatim.
            _ => format!(
                "{prefix}{}",
                display_type_node(store, resolved, needs, depth, visited).0
            ),
        }
    })
}

/// Structural precedence of a rendered type, used to decide parenthesisation by
/// NODE KIND rather than by scanning rendered text. Higher binds tighter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    /// `a extends b ? c : d` and `(…) => r` — the loosest binders.
    Loose = 1,
    /// `a | b`
    Union = 2,
    /// `a & b`
    Intersection = 3,
    /// prefix operators: `keyof a`, `infer a`
    Prefix = 4,
    /// postfix operators: `a[]`, `a[K]`
    Postfix = 5,
    /// atomic — names, primitives, literals, `{…}`, `[…]`, `typeof`, template.
    Atom = 6,
}

/// Precedence of a single node by kind. Exhaustive — a new
/// [`SemanticNodeData`] variant forces a decision here rather than defaulting.
fn prec_of(data: &SemanticNodeData) -> Prec {
    match data {
        // A conditional is a loose binder: nested in any tighter position (array
        // element, keyof base, indexed-access object, another conditional's
        // check / extends / true branch) it must parenthesise.
        SemanticNodeData::Conditional { .. } => Prec::Loose,
        // A constructor type renders `new (…) => R` — a loose binder like a
        // function type.
        SemanticNodeData::Signature { .. } => Prec::Loose,
        SemanticNodeData::Union(_) => Prec::Union,
        SemanticNodeData::Intersection(_) => Prec::Intersection,
        // Prefix operators (`keyof T`, `infer T`).
        SemanticNodeData::KeyOf { .. } | SemanticNodeData::Infer { .. } => Prec::Prefix,
        SemanticNodeData::InferRef { .. } => Prec::Atom,
        SemanticNodeData::Array { .. } | SemanticNodeData::IndexedAccess { .. } => Prec::Postfix,
        // `Alias` is transparent (rendered as its target); its precedence is
        // resolved through the chain by `node_precedence`. Reaching it here
        // means the chain bottomed out — treat as atomic.
        // A merged declaration renders its peer-merged `Object` surface (`{…}`),
        // which is atomic — see the `display_type_node` MergedDecl arm.
        SemanticNodeData::Alias(_)
        | SemanticNodeData::Object(_)
        | SemanticNodeData::ObjectSpreadProgram(_)
        | SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::EnumLiteral(_)
        | SemanticNodeData::Opaque(_)
        | SemanticNodeData::Tuple { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::TypeOf(_)
        | SemanticNodeData::TypeOfNominal(_)
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::MergedDecl { .. }
        | SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. }
        | SemanticNodeData::ClassExpressionInstance { .. }
        | SemanticNodeData::IntrinsicApplication { .. }
        // Unresolved bare-name / dynamic-import / raw-fallback /
        // synthetic-binding carriers all render as atomic references.
        | SemanticNodeData::BareRef(_)
        | SemanticNodeData::ImportType(_)
        | SemanticNodeData::RawFallback { .. }
        | SemanticNodeData::DeferredCallable(_)
        | SemanticNodeData::SyntheticBinding { .. } => Prec::Atom,
    }
}

/// Precedence of a node id, following the transparent `Alias` chain (bounded by
/// the display depth cap) to the node that actually renders. Under
/// [`DisplayFacet::CheckerSpelling`] an object type the checker prints as its
/// sole signature binds as loosely as a function type.
fn node_precedence(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    needs: DisplayNeeds,
    visited: &DisplayContext,
) -> Prec {
    let (mut node, _) = follow_alias_chain(store, id, &visited.substitution);
    for _ in 0..MAX_DISPLAY_DEPTH {
        let Some(settled) = visited.settled_types.get(&node) else {
            break;
        };
        node = follow_alias_chain(store, *settled, &visited.substitution).0;
    }
    match store.node_data(node).as_deref() {
        Some(SemanticNodeData::Object(surface))
            if needs.contains(DisplayFacet::CheckerSpelling)
                && sole_signature(store, surface).is_some() =>
        {
            Prec::Loose
        }
        data => data.map_or(Prec::Atom, prec_of),
    }
}
/// Follow the transparent `Alias` chain (bounded by the display depth cap) and
/// report whether the node ultimately renders as a [`SemanticNodeData::Signature`]
/// — the only shape that may use object method-shorthand. After intersection
/// merging a member's `is_method` flag can be ORed true over an `Intersection`
/// of overloads; such a member must render property-style (with a colon).
fn resolves_to_function(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    visited: &DisplayContext,
) -> bool {
    // CALL kind only: method shorthand (`name(p): r`) is a call-signature
    // form — a construct-signature member renders property-style
    // (`name: new (…) => R`).
    matches!(
        store
            .node_data(follow_alias_chain(store, id, &visited.substitution).0)
            .as_deref(),
        Some(SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Call,
            ..
        })
    )
}

/// Follow the transparent `Alias` chain (bounded by the display depth cap) and
/// return the id of the node that actually renders. A non-`Alias` node (or an
/// unresolved id) is returned as-is.
fn follow_alias_chain(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    substitution: &DisplaySubstitution,
) -> (SemanticNodeId, DisplaySubstitution) {
    let (mut current, mut substitution) = substitution.resolve(store, id);
    for _ in 0..MAX_DISPLAY_DEPTH {
        match store.node_data(current).as_deref() {
            Some(SemanticNodeData::Alias(target)) => {
                (current, substitution) = substitution.resolve(store, *target);
            }
            _ => return (current, substitution),
        }
    }
    (current, substitution)
}
/// True when `id` resolves (through the transparent `Alias` chain) to an
/// `Array`/`Tuple` node carrying `readonly: true` AND the readonly modifier will
/// actually be rendered (`IncludeReadonlyModifier` active). Such a node placed in
/// postfix position (`X[]`) must parenthesise so its leading `readonly` does not
/// re-read as the OUTER array's modifier: `(readonly string[])[]`. Decided from
/// the node kind + flag + facet — node inspection, NOT rendered-text sniffing.
fn element_renders_leading_readonly(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    visited: &DisplayContext,
    needs: DisplayNeeds,
) -> bool {
    if !needs.contains(DisplayFacet::IncludeReadonlyModifier) {
        return false;
    }
    matches!(
        store
            .node_data(follow_alias_chain(store, id, &visited.substitution).0)
            .as_deref(),
        Some(SemanticNodeData::Array { readonly: true, .. })
            | Some(SemanticNodeData::Tuple { readonly: true, .. })
    )
}

/// Render `id` and parenthesise it when its structural precedence binds looser
/// than `min_prec` requires in the enclosing position. Parenthesisation is
/// structural: the decision is taken from the node kind, so a `Conditional`
/// operand wraps and a string literal whose TEXT contains `" | "` does not.
/// Whether `node` already holds `undefined` (or absorbs it), so adding it
/// as an optional property's type changes nothing.
/// Whether `node` is `never` through its transparent alias chain.
fn settles_to_never(
    store: &SemanticGraphStore,
    node: SemanticNodeId,
    visited: &DisplayContext,
) -> bool {
    let (resolved, _) = follow_alias_chain(store, node, &visited.substitution);
    matches!(
        store.node_data(resolved).as_deref(),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
    )
}

fn holds_undefined(store: &SemanticGraphStore, node: SemanticNodeId) -> bool {
    let absorbs = |node: SemanticNodeId| {
        matches!(
            store.node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::Undefined | PrimitiveKind::Any | PrimitiveKind::Unknown
            ))
        )
    };
    match store.node_data(node).as_deref() {
        Some(SemanticNodeData::Union(arms)) => arms.iter().any(|&arm| absorbs(arm)),
        _ => absorbs(node),
    }
}

fn render_operand(
    store: &SemanticGraphStore,
    id: SemanticNodeId,
    needs: DisplayNeeds,
    depth: usize,
    visited: &mut DisplayContext,
    min_prec: Prec,
) -> String {
    let rendered = display_type_node(store, id, needs, depth, visited).0;
    // Postfix-base position (`X[]`, `X[K]`): a readonly array/tuple operand
    // renders a leading `readonly` that would otherwise re-read as the OUTER
    // postfix operator's modifier, so it must parenthesise even though its own
    // precedence (`Postfix`) is not looser than `min_prec`. Folded here so
    // EVERY postfix base (array element, indexed-access object, and any future
    // postfix base) is uniformly correct.
    if min_prec == Prec::Postfix && element_renders_leading_readonly(store, id, visited, needs) {
        return format!("({rendered})");
    }
    if node_precedence(store, id, needs, visited) < min_prec {
        format!("({rendered})")
    } else {
        rendered
    }
}

#[cfg(test)]
#[path = "pending_display_tests.rs"]
mod pending_display_tests;
