//! A failed relation reported as the checker reports it: elaborated into
//! the parts of the authored expression that produced its source
//! (`elaborateError`), and explained below its message by the relation step
//! that failed, one chain line per step (`reportRelationError`).
//!
//! The explanation is computed when a diagnostic is bound to its authored
//! occurrence, from the relation's two types: the relation itself records
//! no history. Each step asks the shared relation which child of the pair
//! fails, in the order the checker relates them. A step whose failing
//! child this does not model — a best match in a target union, index
//! signatures, generic signatures, optional or rest parameters — explains
//! nothing, and a diagnostic it would explain is not reported rather than
//! reported with a message the checker's is not.

use std::sync::Arc;

use crate::semantic_query::{
    AuthoredPropertyKey, CheckerDiagnostic, CheckerDiagnosticChainLine, CheckerDiagnosticCode,
    CheckerDiagnosticOccurrence, CheckerDiagnosticOperation, PrimitiveKind, SemanticNodeData,
    SemanticNodeId, SignatureKind, SurfaceView,
};
use verter_type_expr::{IndexedElaboratedProperty, IndexedElaborationForm, IndexedReportSite};

use super::super::dispatch_txn::RelationStep;
use super::super::ProjectSemanticDispatch;

/// How many relation steps one diagnostic's chain explains before it gives
/// up on the explanation (and with it, the diagnostic). This is an
/// elaboration budget of this surface, not a checker value: the checker
/// bounds its explanation only through its relation depth limits, so a
/// chain past the budget fails closed — no diagnostic is reported rather
/// than a truncated chain the checker would not print.
const CHAIN_STEP_LIMIT: u32 = 64;

/// The relation step a failed relation's pair fails at: the child pair it
/// fails through, as the checker relates them.
enum FailedStep {
    /// The pair fails as a whole; nothing below explains it.
    Leaf,
    /// The source lacks properties the target requires: the message names
    /// them, in place of the pair's own line (`reportUnmatchedProperty`).
    Missing {
        code: CheckerDiagnosticCode,
        arguments: Vec<String>,
    },
    /// A property's types (`Types of property 'a' are incompatible.`).
    Property {
        name: String,
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
    /// A union source's member, an intersection target's member, or an
    /// array's element type, reported with its own line.
    Member {
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
    /// One call signature's return; `arguments` is whether either
    /// signature takes any.
    Return {
        source: SemanticNodeId,
        target: SemanticNodeId,
        arguments: bool,
    },
    /// One parameter, related target to source.
    Parameter {
        source_name: String,
        target_name: String,
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
    /// The signatures' type predicates.
    Predicate {
        source_text: String,
        target_text: String,
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
    /// One tuple position.
    Position {
        index: usize,
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
}

/// One hop of a run of property and return failures the checker prints as
/// one path (`The types of 'a.b' are incompatible between these types.`).
enum PathHop {
    Property(String),
    Return { arguments: bool },
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The occurrences the checker reports for `source` failing `target`
    /// at the authored expression `site`, under `head`
    /// (`checkTypeRelatedToAndOptionallyElaborate`): the failure elaborated
    /// into the expression's parts where its form allows it, else an object
    /// literal's first unknown property, else one diagnostic at the
    /// expression with the relation's message chain. `None` when the
    /// failure runs through a form or a step not modelled.
    pub(in crate::project_semantic_dispatch) fn checker_relation_failure(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        head: CheckerDiagnostic,
        site: &IndexedReportSite,
    ) -> Option<Vec<CheckerDiagnosticOccurrence>> {
        if let Some(elaborated) = self.elaborate_failure(site.form.as_ref(), source, target)? {
            return Some(elaborated);
        }
        if let Some(IndexedElaborationForm::Object(properties)) = &site.form {
            if let Some(excess) = self.excess_property(properties, target)? {
                return Some(vec![excess]);
            }
        }
        // A message that names the whole expression names the checker's
        // REPORTED type for it: a fresh object or array literal's widened
        // literal tree, not the literal one it checked.
        let source = self.widened_fresh_literal(site.form.as_ref(), source);
        Some(vec![self.report_relation_failure(
            source,
            target,
            (site.point, site.end),
            head,
        )?])
    }

    /// The type the checker REPORTS a fresh object or array literal
    /// expression by (`getWidenedLiteralLikeTypeForContextualType` under a
    /// contextual type that keeps no literal): its literal tree widened —
    /// every literal leaf to its base type, an authored array literal to
    /// the array of its widened elements' union. `source` unchanged for
    /// every other form, and for every part of the tree the authored form
    /// does not carry (a value a fresh literal merely references keeps its
    /// own type).
    fn widened_fresh_literal(
        &self,
        form: Option<&IndexedElaborationForm>,
        source: SemanticNodeId,
    ) -> SemanticNodeId {
        match form {
            Some(IndexedElaborationForm::Object(_)) | Some(IndexedElaborationForm::Array(_)) => {
                self.widen_fresh_literal_value(form, source)
            }
            _ => source,
        }
    }

    /// One value of a fresh literal tree, widened through the authored form
    /// that produced it: a literal leaf to its base type, an object literal
    /// member-by-member, an array literal to the array of its widened
    /// elements' union.
    fn widen_fresh_literal_value(
        &self,
        form: Option<&IndexedElaborationForm>,
        source: SemanticNodeId,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let Some(data) = graph.node_data(source) else {
            return source;
        };
        match (data.as_ref(), form) {
            (SemanticNodeData::Literal(_) | SemanticNodeData::EnumLiteral(_), _) => {
                self.widened_literal(source)
            }
            (
                SemanticNodeData::Object(surface),
                Some(IndexedElaborationForm::Object(properties)),
            ) => {
                let surface = surface.clone();
                let property_form = |member: &crate::semantic_query::SurfaceMember| {
                    let AuthoredPropertyKey::String(name) = &member.key else {
                        return None;
                    };
                    properties
                        .iter()
                        .find(|property| property.name.as_ref() == &**name)
                        .and_then(|property| property.value.as_ref())
                        .and_then(|value| value.form.as_ref())
                };
                let mut members: Vec<crate::semantic_query::SurfaceMember> = surface
                    .positive_members()
                    .iter()
                    .map(crate::semantic_query::SurfaceMember::clone)
                    .collect();
                let mut changed = false;
                for member in &mut members {
                    let value = match property_form(member) {
                        Some(form) => self.widen_fresh_literal_value(Some(form), member.value),
                        None => match self.graph().node_data(member.value).as_deref() {
                            Some(
                                SemanticNodeData::Literal(_) | SemanticNodeData::EnumLiteral(_),
                            ) => self.widened_literal(member.value),
                            _ => member.value,
                        },
                    };
                    if value != member.value {
                        member.value = value;
                        changed = true;
                    }
                }
                if !changed {
                    return source;
                }
                graph.intern_node(SemanticNodeData::Object(
                    surface.with_positive_members(Arc::from(members.into_boxed_slice())),
                ))
            }
            (
                SemanticNodeData::Tuple { elements, .. },
                Some(IndexedElaborationForm::Array(sites)),
            ) if elements.len() == sites.len() => {
                let widened: Vec<SemanticNodeId> = elements
                    .iter()
                    .zip(sites.iter())
                    .map(|(element, site)| {
                        self.widen_fresh_literal_value(site.form.as_ref(), element.value)
                    })
                    .collect();
                let element = self.intern_normalized_union_or_intersection(&widened, true);
                graph.intern_node(SemanticNodeData::Array {
                    element,
                    readonly: false,
                })
            }
            _ => source,
        }
    }

    /// `elaborateError`: the failure of `source` against `target` reported
    /// into the parts of an expression of `form`. `Some(None)` when the
    /// form does not elaborate it.
    fn elaborate_failure(
        &self,
        form: Option<&IndexedElaborationForm>,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<Option<Vec<CheckerDiagnosticOccurrence>>> {
        let Some(form) = form else {
            return Some(None);
        };
        // A target that is or holds a generic conditional is not elaborated
        // (`isOrHasGenericConditional`).
        if matches!(
            self.settled_data(target).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        ) {
            return Some(None);
        }
        match form {
            IndexedElaborationForm::Unmodelled => None,
            IndexedElaborationForm::Arrow(returned) => {
                self.elaborate_arrow_return(returned, source, target)
            }
            IndexedElaborationForm::Object(properties) => {
                self.elaborate_object_literal(properties, source, target)
            }
            IndexedElaborationForm::Array(elements) => {
                self.elaborate_array_literal(elements, source, target)
            }
        }
    }

    /// `elaborateArrowFunction`: the source's one call signature's return
    /// against the union of the target's call signatures' returns, reported
    /// at the returned expression.
    fn elaborate_arrow_return(
        &self,
        returned: &IndexedReportSite,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<Option<Vec<CheckerDiagnosticOccurrence>>> {
        let source_signatures = self.call_signature_nodes(source)?;
        let [source_signature] = source_signatures.as_slice() else {
            return Some(None);
        };
        let target_signatures = self.call_signature_nodes(target)?;
        if target_signatures.is_empty() {
            return Some(None);
        }
        let source_return = self.signature_return(*source_signature)?;
        let target_returns = target_signatures
            .iter()
            .map(|&signature| self.signature_return(signature))
            .collect::<Option<Vec<_>>>()?;
        let target_return = match target_returns.as_slice() {
            [only] => *only,
            returns => self.intern_normalized_union_or_intersection(returns, true),
        };
        if self.relation_holds(source_return, target_return)? {
            return Some(None);
        }
        if let Some(elaborated) =
            self.elaborate_failure(returned.form.as_ref(), source_return, target_return)?
        {
            return Some(Some(elaborated));
        }
        Some(Some(vec![self.report_relation_failure(
            source_return,
            target_return,
            (returned.point, returned.end),
            type_not_assignable(),
        )?]))
    }

    /// `elaborateObjectLiteral`: each authored property whose type fails
    /// the target's property of its name, reported into its value or at
    /// its name.
    fn elaborate_object_literal(
        &self,
        properties: &[IndexedElaboratedProperty],
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<Option<Vec<CheckerDiagnosticOccurrence>>> {
        if self.settled_is_primitive(target) {
            return Some(None);
        }
        let source_view = self.object_view(source)?;
        let target_view = self.object_view(target)?;
        if !target_view.index_signatures().is_empty() {
            return None;
        }
        let mut reported = Vec::new();
        for property in properties {
            let Some(target_member) = member_named(&target_view, &property.name) else {
                continue;
            };
            let Some(source_member) = member_named(&source_view, &property.name) else {
                continue;
            };
            let (source_type, target_type) = (source_member.value, target_member.value);
            if self.relation_holds(source_type, target_type)? {
                continue;
            }
            // An optional target property also admits `undefined`, which
            // its declared type (the one the checker prints) does not hold.
            if target_member.optional {
                let undefined = self
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                if self.relation_holds(source_type, undefined)? {
                    continue;
                }
            }
            if let Some(value) = &property.value {
                if let Some(elaborated) =
                    self.elaborate_failure(value.form.as_ref(), source_type, target_type)?
                {
                    reported.extend(elaborated);
                    continue;
                }
            }
            reported.push(self.report_relation_failure(
                source_type,
                target_type,
                (property.point, property.end),
                type_not_assignable(),
            )?);
        }
        Some((!reported.is_empty()).then_some(reported))
    }

    /// `elaborateArrayLiteral`: each authored element whose type fails the
    /// target's element at its position, reported into the element or at
    /// it.
    fn elaborate_array_literal(
        &self,
        elements: &[IndexedReportSite],
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<Option<Vec<CheckerDiagnosticOccurrence>>> {
        if self.settled_is_primitive(target) {
            return Some(None);
        }
        let source = self.structural_data(source)?;
        let source_elements: Vec<SemanticNodeId> = match source.as_ref() {
            SemanticNodeData::Tuple {
                elements: types, ..
            } if types.len() == elements.len() => {
                types.iter().map(|element| element.value).collect()
            }
            // One element's type is the array's element type.
            SemanticNodeData::Array { element, .. } if elements.len() == 1 => vec![*element],
            _ => return None,
        };
        let target = self.structural_data(target)?;
        let mut reported = Vec::new();
        for (index, (element, source_type)) in elements.iter().zip(source_elements).enumerate() {
            let target_type = match target.as_ref() {
                SemanticNodeData::Array { element, .. } => *element,
                SemanticNodeData::Tuple { elements, .. } => match elements.get(index) {
                    Some(element) if !element.rest && !element.optional => element.value,
                    Some(_) => return None,
                    None => continue,
                },
                _ => return None,
            };
            if self.relation_holds(source_type, target_type)? {
                continue;
            }
            if let Some(elaborated) =
                self.elaborate_failure(element.form.as_ref(), source_type, target_type)?
            {
                reported.extend(elaborated);
                continue;
            }
            reported.push(self.report_relation_failure(
                source_type,
                target_type,
                (element.point, element.end),
                type_not_assignable(),
            )?);
        }
        Some((!reported.is_empty()).then_some(reported))
    }

    /// The excess-property check of a fresh object literal
    /// (`hasExcessProperties`): its first authored property the target
    /// names in none of its object types, as TS2353 at the property's name
    /// — TS2561 with the target's property whose name it likely misspells.
    /// `Some(None)` when the target takes every property.
    fn excess_property(
        &self,
        properties: &[IndexedElaboratedProperty],
        target: SemanticNodeId,
    ) -> Option<Option<CheckerDiagnosticOccurrence>> {
        if self.settled_is_primitive(target) {
            return Some(None);
        }
        let views = match self.settled_data(target)?.as_ref() {
            SemanticNodeData::Union(members) => members
                .iter()
                .map(|&member| self.object_view(member))
                .collect::<Option<Vec<_>>>()?,
            _ => vec![self.object_view(target)?],
        };
        if views.iter().any(|view| !view.index_signatures().is_empty()) {
            return None;
        }
        // An empty object type takes any property.
        if views.iter().any(|view| {
            view.positive_members().is_empty()
                && view.call_signatures().is_empty()
                && view.construct_signatures().is_empty()
        }) {
            return Some(None);
        }
        let excess = properties.iter().find(|property| {
            !views
                .iter()
                .any(|view| member_named(view, &property.name).is_some())
        });
        let Some(excess) = excess else {
            return Some(None);
        };
        let name = super::context::checker_property_name(&AuthoredPropertyKey::String(
            excess.name.clone().into(),
        ))?;
        // The suggestion is among the properties every object type has.
        let suggestion = super::context::is_identifier_name(&excess.name)
            .then(|| {
                spelling_suggestion(
                    &excess.name,
                    views[0].positive_members().iter().filter_map(|member| {
                        let AuthoredPropertyKey::String(candidate) = &member.key else {
                            return None;
                        };
                        views[1..]
                            .iter()
                            .all(|view| member_named(view, candidate).is_some())
                            .then_some(&**candidate)
                    }),
                )
            })
            .flatten();
        let printed = self.print_diagnostic_type(target)?;
        let (code, arguments) = match suggestion {
            Some(suggestion) => (
                CheckerDiagnosticCode::ExcessPropertySuggestion,
                vec![name, printed, suggestion.to_owned()],
            ),
            None => (CheckerDiagnosticCode::ExcessProperty, vec![name, printed]),
        };
        Some(Some(occurrence(
            (excess.point, excess.end),
            code,
            arguments,
            Vec::new(),
        )))
    }

    /// `checkTypeRelatedTo` reporting at `point`: `head` naming the two
    /// types with the failure's message chain below it — or, where the
    /// pair itself lacks properties its target requires, that message in
    /// its place.
    fn report_relation_failure(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        span: (u32, u32),
        head: CheckerDiagnostic,
    ) -> Option<CheckerDiagnosticOccurrence> {
        if let FailedStep::Missing { code, arguments } = self.failed_step(source, target)? {
            return Some(occurrence(span, code, arguments, Vec::new()));
        }
        let mut chain = Vec::new();
        let mut steps = CHAIN_STEP_LIMIT;
        self.explain_failure(source, target, 1, false, &mut chain, &mut steps)?;
        let (source, target) = self.print_relation_operands(source, target)?;
        Some(CheckerDiagnosticOccurrence::new(
            span.0,
            span.1,
            head,
            Box::new([source.into_boxed_str(), target.into_boxed_str()]),
            chain.into_boxed_slice(),
        ))
    }

    /// Whether the checker's weak-type rule (`isRelatedTo`) decides this
    /// failure: a target whose properties are all optional, with no
    /// signatures, and a source with apparent members of its own (a
    /// primitive, an enum member, an array or a tuple) report TS2559 where
    /// they share no property with it — as the diagnostic itself, or as the
    /// chain line of a union member or property the failure descends into.
    /// Those apparent members are not modelled, so the diagnostic is not
    /// reported rather than reported with a head or chain line the checker
    /// does not print.
    fn weak_target_decides(&self, source: SemanticNodeId, target: SemanticNodeId) -> Option<bool> {
        let Some(target_view) = self.object_view(target) else {
            return Some(false);
        };
        let members = target_view.positive_members();
        if members.is_empty()
            || !members.iter().all(|member| member.optional)
            || !target_view.call_signatures().is_empty()
            || !target_view.construct_signatures().is_empty()
            || !target_view.index_signatures().is_empty()
        {
            return Some(false);
        }
        Some(matches!(
            self.settled_data(source)?.as_ref(),
            SemanticNodeData::Primitive(
                PrimitiveKind::String
                    | PrimitiveKind::Number
                    | PrimitiveKind::Boolean
                    | PrimitiveKind::BigInt
                    | PrimitiveKind::Symbol,
            ) | SemanticNodeData::Literal(_)
                | SemanticNodeData::EnumLiteral(_)
                | SemanticNodeData::TemplateLiteral { .. }
                | SemanticNodeData::TypeOfNominal(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
        ))
    }

    /// Push the chain lines explaining why `source` fails `target`, from
    /// `depth`: the pair's own line when `own_line` (a pair below the
    /// diagnostic's own message), then its failed step's.
    fn explain_failure(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        depth: u16,
        own_line: bool,
        chain: &mut Vec<CheckerDiagnosticChainLine>,
        steps: &mut u32,
    ) -> Option<()> {
        *steps = steps.checked_sub(1)?;
        if self.weak_target_decides(source, target)? {
            return None;
        }
        let step = self.failed_step(source, target)?;
        let push = |chain: &mut Vec<CheckerDiagnosticChainLine>, depth, text: String| {
            chain.push(CheckerDiagnosticChainLine::new(
                depth,
                text.into_boxed_str(),
            ));
        };
        if let FailedStep::Missing { code, arguments } = &step {
            push(chain, depth, code.format(arguments));
            return Some(());
        }
        let depth = if own_line {
            let (source, target) = self.print_relation_operands(source, target)?;
            push(
                chain,
                depth,
                CheckerDiagnosticCode::TypeNotAssignable.format(&[source, target]),
            );
            depth.checked_add(1)?
        } else {
            depth
        };
        let below = depth.checked_add(1)?;
        match step {
            FailedStep::Leaf | FailedStep::Missing { .. } => Some(()),
            FailedStep::Member { source, target } | FailedStep::Return { source, target, .. } => {
                self.explain_failure(source, target, depth, true, chain, steps)
            }
            FailedStep::Parameter {
                source_name,
                target_name,
                source,
                target,
            } => {
                push(
                    chain,
                    depth,
                    format!(
                        "Types of parameters '{source_name}' and '{target_name}' are incompatible."
                    ),
                );
                self.explain_failure(source, target, below, true, chain, steps)
            }
            FailedStep::Predicate {
                source_text,
                target_text,
                source,
                target,
            } => {
                push(
                    chain,
                    depth,
                    format!("Type predicate '{source_text}' is not assignable to '{target_text}'."),
                );
                self.explain_failure(source, target, below, true, chain, steps)
            }
            FailedStep::Position {
                index,
                source,
                target,
            } => {
                push(
                    chain,
                    depth,
                    format!(
                        "Type at position {index} in source is not compatible with type at position {index} in target."
                    ),
                );
                self.explain_failure(source, target, below, true, chain, steps)
            }
            FailedStep::Property {
                name,
                source,
                target,
            } => {
                // A run of property and return failures is one path
                // (`reportIncompatibleStack`); the pairs inside it print no
                // line of their own.
                let mut path = vec![PathHop::Property(name)];
                let (mut source, mut target) = (source, target);
                loop {
                    *steps = steps.checked_sub(1)?;
                    match self.failed_step(source, target)? {
                        FailedStep::Property {
                            name,
                            source: inner_source,
                            target: inner_target,
                        } => {
                            path.push(PathHop::Property(name));
                            (source, target) = (inner_source, inner_target);
                        }
                        FailedStep::Return {
                            source: inner_source,
                            target: inner_target,
                            arguments,
                        } => {
                            path.push(PathHop::Return { arguments });
                            (source, target) = (inner_source, inner_target);
                        }
                        _ => break,
                    }
                }
                push(chain, depth, incompatible_path_message(&path));
                self.explain_failure(source, target, below, true, chain, steps)
            }
        }
    }

    /// The relation step `source` fails `target` at. `None` when the pair
    /// relates, when the relation is undecided, or when the failing step is
    /// one this does not model.
    fn failed_step(&self, source: SemanticNodeId, target: SemanticNodeId) -> Option<FailedStep> {
        if self.relation_holds(source, target)? {
            return None;
        }
        let (source, target) = (
            self.without_unbuilt_nullish(source)?,
            self.without_unbuilt_nullish(target)?,
        );
        let source_data = self.settled_data(source)?;
        let target_data = self.settled_data(target)?;
        if let SemanticNodeData::Union(members) = source_data.as_ref() {
            // An enum's union of its members is a primitive the checker does
            // not elaborate.
            if members.iter().all(|&member| {
                matches!(
                    self.graph().node_data(member).as_deref(),
                    Some(SemanticNodeData::EnumLiteral(_))
                )
            }) {
                return Some(FailedStep::Leaf);
            }
            for member in self.in_relation_order(members.iter().copied()) {
                if !self.relation_holds(member, target)? {
                    return Some(FailedStep::Member {
                        source: member,
                        target,
                    });
                }
            }
            return None;
        }
        if let SemanticNodeData::Union(_) = target_data.as_ref() {
            // A non-primitive source is elaborated against the target's
            // best-matching member (`getBestMatchingType`).
            return is_atomic(&source_data).then_some(FailedStep::Leaf);
        }
        if let SemanticNodeData::Intersection(members) = target_data.as_ref() {
            for member in self.in_relation_order(members.iter().copied()) {
                if !self.relation_holds(source, member)? {
                    return Some(FailedStep::Member {
                        source,
                        target: member,
                    });
                }
            }
            return None;
        }
        if matches!(source_data.as_ref(), SemanticNodeData::Intersection(_)) {
            return None;
        }
        if is_atomic(&source_data) || is_atomic(&target_data) {
            return Some(FailedStep::Leaf);
        }
        let source_data = self.structural_data(source)?;
        let target_data = self.structural_data(target)?;
        match (source_data.as_ref(), target_data.as_ref()) {
            (
                SemanticNodeData::Array {
                    element: source_element,
                    readonly: source_readonly,
                },
                SemanticNodeData::Array {
                    element: target_element,
                    readonly: target_readonly,
                },
            ) => (!(*source_readonly && !*target_readonly)
                && !self.relation_holds(*source_element, *target_element)?)
            .then_some(FailedStep::Member {
                source: *source_element,
                target: *target_element,
            }),
            (
                SemanticNodeData::Tuple {
                    elements: source_elements,
                    readonly: source_readonly,
                },
                SemanticNodeData::Tuple {
                    elements: target_elements,
                    readonly: target_readonly,
                },
            ) => {
                if (*source_readonly && !*target_readonly)
                    || source_elements.len() != target_elements.len()
                    || source_elements
                        .iter()
                        .chain(target_elements.iter())
                        .any(|element| element.rest || element.optional)
                {
                    return None;
                }
                for (index, (source, target)) in source_elements
                    .iter()
                    .zip(target_elements.iter())
                    .enumerate()
                {
                    if !self.relation_holds(source.value, target.value)? {
                        return Some(FailedStep::Position {
                            index,
                            source: source.value,
                            target: target.value,
                        });
                    }
                }
                None
            }
            (SemanticNodeData::Array { .. } | SemanticNodeData::Tuple { .. }, _)
            | (_, SemanticNodeData::Array { .. } | SemanticNodeData::Tuple { .. }) => None,
            _ => self.structured_failed_step(source, target),
        }
    }

    /// `structuredTypeRelatedTo` over two object types: properties, then
    /// call signatures.
    fn structured_failed_step(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<FailedStep> {
        let source_view = self.object_view(source);
        let target_view = self.object_view(target);
        if let Some(target_view) = &target_view {
            if target_view
                .positive_members()
                .iter()
                .any(|member| !member.optional)
            {
                // A source without an object surface lacks them all, which
                // this does not list.
                source_view.as_ref()?;
                let missing = self.missing_required_properties(source, target)?;
                if !missing.is_empty() {
                    let (source_printed, target_printed) = (
                        self.print_diagnostic_type(source)?,
                        self.print_diagnostic_type(target)?,
                    );
                    let (code, arguments) = match missing.as_slice() {
                        [name] => (
                            CheckerDiagnosticCode::PropertyMissing,
                            vec![name.clone(), source_printed, target_printed],
                        ),
                        listed if listed.len() <= 5 => (
                            CheckerDiagnosticCode::PropertiesMissing,
                            vec![source_printed, target_printed, listed.join(", ")],
                        ),
                        many => (
                            CheckerDiagnosticCode::ManyPropertiesMissing,
                            vec![
                                source_printed,
                                target_printed,
                                many[..4].join(", "),
                                (many.len() - 4).to_string(),
                            ],
                        ),
                    };
                    return Some(FailedStep::Missing { code, arguments });
                }
            }
            for target_member in target_view.positive_members() {
                let Some(source_member) = source_view.as_ref().and_then(|view| {
                    view.positive_members().iter().find(|member| {
                        super::context::same_property_key(&member.key, &target_member.key)
                    })
                }) else {
                    continue;
                };
                if self.relation_holds(source_member.value, target_member.value)? {
                    continue;
                }
                // An optional source property relates with its `undefined`,
                // which this surface does not separate.
                if source_member.optional {
                    return None;
                }
                return Some(FailedStep::Property {
                    name: super::context::checker_property_name(&target_member.key)?,
                    source: source_member.value,
                    target: target_member.value,
                });
            }
            if !target_view.construct_signatures().is_empty()
                || !target_view.index_signatures().is_empty()
            {
                return None;
            }
        }
        let target_signatures = self.call_signature_nodes(target)?;
        let source_signatures = self.call_signature_nodes(source)?;
        match (source_signatures.as_slice(), target_signatures.as_slice()) {
            ([source], [target]) => self.signature_failed_step(*source, *target),
            _ => None,
        }
    }

    /// `compareSignaturesRelated` over two non-generic signatures with
    /// plain parameters: the first parameter whose types relate in neither
    /// direction, then the type predicates or the returns.
    fn signature_failed_step(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<FailedStep> {
        let source_data = self.graph().node_data(source)?;
        let target_data = self.graph().node_data(target)?;
        let (
            SemanticNodeData::Signature {
                kind: SignatureKind::Call,
                params: source_params,
                return_type: source_return,
                type_parameters: source_type_parameters,
                predicate: source_predicate,
                ..
            },
            SemanticNodeData::Signature {
                kind: SignatureKind::Call,
                params: target_params,
                return_type: target_return,
                type_parameters: target_type_parameters,
                predicate: target_predicate,
                ..
            },
        ) = (source_data.as_ref(), target_data.as_ref())
        else {
            return None;
        };
        if !source_type_parameters.is_empty() || !target_type_parameters.is_empty() {
            return None;
        }
        let (source_this, source_ordinary) =
            crate::semantic_query::split_this_receiver(source_params.as_arc());
        let (target_this, target_ordinary) =
            crate::semantic_query::split_this_receiver(target_params.as_arc());
        if source_this.is_some()
            || target_this.is_some()
            || source_ordinary
                .iter()
                .chain(target_ordinary.iter())
                .any(|param| param.rest || param.optional)
            || source_ordinary.len() > target_ordinary.len()
        {
            return None;
        }
        for (source_param, target_param) in source_ordinary.iter().zip(target_ordinary.iter()) {
            if self.relation_holds(target_param.ty, source_param.ty)? {
                continue;
            }
            // A callback parameter relates its signatures in their own mode,
            // and a parameter relating one way only relates by the
            // function's variance.
            if self.relation_holds(source_param.ty, target_param.ty)?
                || self
                    .call_signature_nodes(source_param.ty)
                    .is_none_or(|signatures| !signatures.is_empty())
            {
                return None;
            }
            return Some(FailedStep::Parameter {
                source_name: source_param.name.as_deref()?.to_owned(),
                target_name: target_param.name.as_deref()?.to_owned(),
                source: target_param.ty,
                target: source_param.ty,
            });
        }
        match (source_predicate, target_predicate) {
            (Some(source_predicate), Some(target_predicate)) => {
                let (Some(source_type), Some(target_type)) =
                    (source_predicate.ty, target_predicate.ty)
                else {
                    return None;
                };
                if source_predicate.asserts != target_predicate.asserts
                    || source_predicate.subject != target_predicate.subject
                    || self.relation_holds(source_type, target_type)?
                {
                    return None;
                }
                Some(FailedStep::Predicate {
                    source_text: self.print_predicate(*source_predicate, source_params.as_arc())?,
                    target_text: self.print_predicate(*target_predicate, target_params.as_arc())?,
                    source: source_type,
                    target: target_type,
                })
            }
            (None, Some(_)) => None,
            (_, None) => {
                if matches!(
                    self.settled_data(*target_return).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Void))
                ) || self.relation_holds(*source_return, *target_return)?
                {
                    return None;
                }
                Some(FailedStep::Return {
                    source: *source_return,
                    target: *target_return,
                    arguments: !(source_ordinary.is_empty() && target_ordinary.is_empty()),
                })
            }
        }
    }

    /// A type predicate as a message prints it (`typePredicateToString`).
    fn print_predicate(
        &self,
        predicate: crate::semantic_query::SignaturePredicate,
        params: &[crate::semantic_query::FunctionParam],
    ) -> Option<String> {
        let subject = match predicate.subject {
            crate::semantic_query::PredicateSubject::This => "this",
            crate::semantic_query::PredicateSubject::Parameter(_) => {
                predicate.subject_parameter(params)?.name.as_deref()?
            }
        };
        let mut printed = String::new();
        if predicate.asserts {
            printed.push_str("asserts ");
        }
        printed.push_str(subject);
        if let Some(ty) = predicate.ty {
            printed.push_str(" is ");
            printed.push_str(&self.print_diagnostic_type(ty)?);
        }
        Some(printed)
    }

    /// `node` as the checker builds it under `strictNullChecks: false`: a
    /// union without its `null` and `undefined` arms, the one arm left
    /// standing for itself. Every other node, and every node under strict
    /// null checks, is its own.
    fn without_unbuilt_nullish(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        if self.relation_strict_config().strict_null_checks {
            return Some(node);
        }
        let data = self.settled_data(node)?;
        let SemanticNodeData::Union(members) = data.as_ref() else {
            return Some(node);
        };
        let kept = self.in_relation_order(members.iter().copied());
        if kept.len() == members.iter().count() {
            return Some(node);
        }
        Some(match kept.as_slice() {
            [only] => *only,
            _ => self.intern_normalized_union_or_intersection(&kept, true),
        })
    }

    /// The constituents of a union or intersection in the order the checker
    /// relates them — by type id (see
    /// [`checker_relation_order_rank`](crate::semantic_query::display::checker_relation_order_rank))
    /// — with `null` and `undefined` left out under `strictNullChecks:
    /// false`, where the checker never builds them.
    fn in_relation_order(
        &self,
        members: impl Iterator<Item = SemanticNodeId>,
    ) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let mut ordered: Vec<SemanticNodeId> = members.collect();
        if !self.relation_strict_config().strict_null_checks {
            let nullish = |node: &SemanticNodeId| {
                matches!(
                    graph.node_data(*node).as_deref(),
                    Some(SemanticNodeData::Primitive(
                        PrimitiveKind::Null | PrimitiveKind::Undefined
                    ))
                )
            };
            if ordered.iter().any(|node| !nullish(node)) {
                ordered.retain(|node| !nullish(node));
            }
        }
        ordered.sort_by_key(|&node| {
            crate::semantic_query::display::checker_relation_order_rank(graph, node)
        });
        ordered
    }

    /// Whether `source` relates to `target` (`isTypeAssignableTo`); `None`
    /// when the relation is undecided.
    fn relation_holds(&self, source: SemanticNodeId, target: SemanticNodeId) -> Option<bool> {
        match self.execute_relate_pair(source, target) {
            RelationStep::Assignable { .. } => Some(true),
            RelationStep::NotAssignable => Some(false),
            RelationStep::Unknown | RelationStep::BudgetExceeded(_) | RelationStep::Assumed(_) => {
                None
            }
        }
    }

    /// `node`'s data through its aliases and declaration carriers.
    fn settled_data(&self, node: SemanticNodeId) -> Option<crate::semantic_query_memo::NodeData> {
        self.graph().node_data(self.settle_through_carriers(node)?)
    }

    /// `node`'s structural data, its references resolved.
    fn structural_data(
        &self,
        node: SemanticNodeId,
    ) -> Option<crate::semantic_query_memo::NodeData> {
        let transit =
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            );
        match self.normalize_node_for_structural_fact_demand(node, transit) {
            super::super::evaluate::StructuralFactDemandOutcome::Complete(node) => self
                .graph()
                .node_data(self.regularized_empty_object_program(node)),
            _ => None,
        }
    }

    /// `node`'s object surface, when it is one.
    fn object_view(&self, node: SemanticNodeId) -> Option<SurfaceView> {
        match self.structural_data(node)?.as_ref() {
            SemanticNodeData::Object(view) => Some(view.clone()),
            _ => None,
        }
    }

    /// Whether `node` settles to a primitive (`TypeFlags.Primitive`, or
    /// `never`), into which the checker elaborates no literal.
    fn settled_is_primitive(&self, node: SemanticNodeId) -> bool {
        self.settled_data(node).is_some_and(|data| is_atomic(&data))
    }

    /// `node`'s call signatures. `None` when they are not all read.
    fn call_signature_nodes(&self, node: SemanticNodeId) -> Option<Vec<SemanticNodeId>> {
        match self.shared_signature_nodes(node, SignatureKind::Call) {
            super::super::signature_discovery::SharedSignatureNodes::Nodes(nodes) => Some(nodes),
            super::super::signature_discovery::SharedSignatureNodes::Incomplete(_) => None,
        }
    }

    /// A signature node's return type.
    fn signature_return(&self, signature: SemanticNodeId) -> Option<SemanticNodeId> {
        match self.graph().node_data(signature)?.as_ref() {
            SemanticNodeData::Signature { return_type, .. } => Some(*return_type),
            _ => None,
        }
    }
}

/// The checker's TS2322 head for a relation it reports with no message of
/// its own.
const fn type_not_assignable() -> CheckerDiagnostic {
    CheckerDiagnostic {
        code: CheckerDiagnosticCode::TypeNotAssignable,
        operation: CheckerDiagnosticOperation::CallResolution,
    }
}

/// One call-resolution occurrence with no chain beyond `chain`.
fn occurrence(
    span: (u32, u32),
    code: CheckerDiagnosticCode,
    arguments: Vec<String>,
    chain: Vec<CheckerDiagnosticChainLine>,
) -> CheckerDiagnosticOccurrence {
    CheckerDiagnosticOccurrence::new(
        span.0,
        span.1,
        CheckerDiagnostic {
            code,
            operation: CheckerDiagnosticOperation::CallResolution,
        },
        arguments.into_iter().map(String::into_boxed_str).collect(),
        chain.into_boxed_slice(),
    )
}

/// Whether a type is one the checker relates as a whole, with no structure
/// to elaborate: a primitive, a literal, an enum member or a template
/// literal type.
fn is_atomic(data: &SemanticNodeData) -> bool {
    matches!(
        data,
        SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::TemplateLiteral { .. }
    )
}

/// The member of `view` named `name` by a string key.
fn member_named<'v>(
    view: &'v SurfaceView,
    name: &str,
) -> Option<&'v crate::semantic_query::SurfaceMember> {
    view.positive_members().iter().find(|member| {
        member.key.as_known().is_some_and(|known| {
            known.element_access_collides(&verter_type_expr::PropertyKey::from(name))
        })
    })
}

/// The message of a run of incompatible properties and returns
/// (`reportIncompatibleStack`): one property's own message, or the path of
/// the run.
fn incompatible_path_message(path: &[PathHop]) -> String {
    if let [PathHop::Property(name)] = path {
        return format!("Types of property '{name}' are incompatible.");
    }
    let mut printed = String::new();
    for hop in path {
        match hop {
            PathHop::Property(name) => {
                if printed.starts_with("new ") {
                    printed = format!("({printed})");
                }
                if printed.is_empty() {
                    printed.push_str(name);
                } else if super::context::is_identifier_name(name) {
                    printed.push('.');
                    printed.push_str(name);
                } else if name.starts_with('[') && name.ends_with(']') {
                    printed.push_str(name);
                } else {
                    printed.push('[');
                    printed.push_str(name);
                    printed.push(']');
                }
            }
            PathHop::Return { arguments } => {
                printed.push_str(if *arguments { "(...)" } else { "()" });
            }
        }
    }
    if printed.ends_with(')') {
        format!("The types returned by '{printed}' are incompatible between these types.")
    } else {
        format!("The types of '{printed}' are incompatible between these types.")
    }
}

/// The candidate whose name `name` most likely misspells
/// (`getSpellingSuggestion`), if any is close enough.
///
/// The 34% length allowance, the 40% distance threshold and the 0.1 / 2.0
/// edit costs are the checker's own (`getSpellingSuggestion` and
/// `levenshteinWithMax` in TypeScript's `src/compiler/core.ts`, unchanged
/// through the pinned 7.0.2 port); they select TS2353 versus TS2561 and the
/// suggestion text, so they are copied, never tuned.
fn spelling_suggestion<'c>(
    name: &str,
    candidates: impl Iterator<Item = &'c str>,
) -> Option<&'c str> {
    let length = name.chars().count();
    let maximum_length_difference = 2.max(length * 34 / 100);
    let mut best_distance = (length * 4 / 10) as f64 + 1.0;
    let mut best = None;
    for candidate in candidates {
        let candidate_length = candidate.chars().count();
        if candidate_length.abs_diff(length) > maximum_length_difference || candidate == name {
            continue;
        }
        if candidate_length < 3 && candidate.to_lowercase() != name.to_lowercase() {
            continue;
        }
        if let Some(distance) = levenshtein_with_max(name, candidate, best_distance - 0.1) {
            best_distance = distance;
            best = Some(candidate);
        }
    }
    best
}

/// The checker's case-cheap edit distance between `a` and `b`, `None` past
/// `max` (`levenshteinWithMax`).
fn levenshtein_with_max(a: &str, b: &str, max: f64) -> Option<f64> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let big = max + 0.01;
    let mut previous: Vec<f64> = (0..=b.len()).map(|i| i as f64).collect();
    let mut current = vec![0.0; b.len() + 1];
    for i in 1..=a.len() {
        let first = if (i as f64) > max {
            ((i as f64) - max).ceil() as usize
        } else {
            1
        };
        let last = if (b.len() as f64) > max + i as f64 {
            (max + i as f64).floor() as usize
        } else {
            b.len()
        };
        current[0] = i as f64;
        let mut column_min = i as f64;
        for slot in current.iter_mut().take(first).skip(1) {
            *slot = big;
        }
        for j in first..=last {
            let substitution = if a[i - 1].to_lowercase().eq(b[j - 1].to_lowercase()) {
                previous[j - 1] + 0.1
            } else {
                previous[j - 1] + 2.0
            };
            let distance = if a[i - 1] == b[j - 1] {
                previous[j - 1]
            } else {
                (previous[j] + 1.0)
                    .min(current[j - 1] + 1.0)
                    .min(substitution)
            };
            current[j] = distance;
            column_min = column_min.min(distance);
        }
        for slot in current.iter_mut().skip(last + 1) {
            *slot = big;
        }
        if column_min > max {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let result = previous[b.len()];
    (result <= max).then_some(result)
}
