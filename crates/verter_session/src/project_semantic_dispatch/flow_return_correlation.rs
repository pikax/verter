//! Narrowings a destructured binding carries beyond itself: the checker's
//! destructured discriminated unions (`getNarrowedTypeOfSymbol`, a test
//! of one element narrows the pattern's parent union and retypes its
//! siblings) and its destructured discriminant aliases
//! (`getCandidateDiscriminantPropertyAccess`, a test of an element of
//! `const { kind } = o` narrows `o`).

use std::sync::Arc;

use super::{FlowEvaluator, FlowProductSubject, GuardNarrowing, LiteralComparison};
use crate::flow_slice_content::{
    SliceGuard, SliceNarrowRoot, SliceNarrowSubject, SlicePattern, SlicePatternKey,
};
use crate::semantic_query::{FlowGap, FlowReturnDegradation, SemanticNodeData, SemanticNodeId};

/// One correlated pattern: the pseudo-reference the checker narrows (the
/// pattern itself) and the elements that read their members of it.
#[derive(Clone)]
pub(super) struct CorrelatedGroup {
    /// The pseudo-reference, rooted at one of the pattern's own elements
    /// under a U+0000 segment no property name spells. `None` when the
    /// parent's union is not decidable here (an unresolved parent, a type
    /// parameter): a test of an element then takes the typed gap.
    pseudo: Option<SliceNarrowSubject>,
    /// The parent type at the declaration.
    parent: Option<SemanticNodeId>,
    /// Each correlated element: its reference and the member it reads.
    elements: Vec<(SliceNarrowSubject, Arc<str>)>,
}

/// What a test of one destructured element also narrows.
#[derive(Clone)]
pub(super) enum GuardAlias {
    /// The element's member of its correlated group's pseudo-reference.
    Correlated { group: usize, property: Arc<str> },
    /// The member of the destructured source reference the element reads.
    Source(SliceNarrowSubject),
}

/// The pseudo-reference segment.
const PSEUDO_SEGMENT: &str = "\u{0}pattern";

impl FlowEvaluator<'_, '_> {
    /// Register the narrowings a destructured declaration's elements carry
    /// (see the module doc): each object or array pattern of two or more
    /// elements whose parent is a union correlates its elements that have
    /// neither a default nor a rest spread, and the top-level elements of
    /// a destructured narrowable `source` alias its members.
    pub(super) fn register_destructured_aliases(
        &mut self,
        pattern: &SlicePattern,
        parent: Option<SemanticNodeId>,
        correlated: bool,
        source: Option<&SliceNarrowSubject>,
    ) {
        if correlated {
            self.register_correlated_patterns(pattern, parent);
        }
        let Some(source) = source else {
            return;
        };
        for (binding, property) in self.pattern_plain_elements(pattern) {
            let mut path = source.path.to_vec();
            path.push(property);
            let member = SliceNarrowSubject {
                root: source.root.clone(),
                path: Arc::from(path.into_boxed_slice()),
            };
            self.add_guard_alias(binding, GuardAlias::Source(member));
        }
    }

    fn register_correlated_patterns(
        &mut self,
        pattern: &SlicePattern,
        parent: Option<SemanticNodeId>,
    ) {
        let count = match pattern {
            SlicePattern::Object { properties, rest } => {
                properties.len() + usize::from(rest.is_some())
            }
            SlicePattern::Array { elements, rest } => elements.len() + usize::from(rest.is_some()),
            SlicePattern::Binding { .. } | SlicePattern::Target { .. } => return,
        };
        // Nested patterns correlate against their own parent member.
        match pattern {
            SlicePattern::Object { properties, .. } => {
                for (key, element) in properties.iter() {
                    if matches!(
                        element.pattern,
                        SlicePattern::Object { .. } | SlicePattern::Array { .. }
                    ) {
                        let member = match (parent, key) {
                            (Some(parent), SlicePatternKey::Named(name)) => {
                                self.pattern_property(parent, name)
                            }
                            _ => None,
                        };
                        self.register_correlated_patterns(&element.pattern, member);
                    }
                }
            }
            SlicePattern::Array { elements, .. } => {
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    if matches!(
                        element.pattern,
                        SlicePattern::Object { .. } | SlicePattern::Array { .. }
                    ) {
                        let member = parent.and_then(|parent| self.pattern_position(parent, index));
                        self.register_correlated_patterns(&element.pattern, member);
                    }
                }
            }
            _ => {}
        }
        if count < 2 {
            return;
        }
        let plain = self.pattern_plain_elements(pattern);
        if plain.is_empty() {
            return;
        }
        let decidable = match parent {
            Some(parent) => match self.parent_union_verdict(parent) {
                ParentUnion::Union => true,
                ParentUnion::NotUnion => return,
                ParentUnion::Undecided => false,
            },
            None => false,
        };
        let elements: Vec<(SliceNarrowSubject, Arc<str>)> = plain
            .iter()
            .map(|(binding, property)| (self.element_subject(binding), Arc::clone(property)))
            .collect();
        let host = elements
            .iter()
            .find(|(subject, _)| self.resolved_narrow_subject(&subject.root).is_some())
            .map(|(subject, _)| subject.root.clone());
        let pseudo = match (decidable, host) {
            (true, Some(root)) => Some(SliceNarrowSubject {
                root,
                path: Arc::from(vec![Arc::from(PSEUDO_SEGMENT)].into_boxed_slice()),
            }),
            (true, None) => return,
            (false, _) => None,
        };
        if let (Some(pseudo), Some(parent)) = (pseudo.as_ref(), parent) {
            self.push_narrowing(pseudo, parent);
        }
        let group = CorrelatedGroup {
            pseudo,
            parent,
            elements,
        };
        let first = plain.first().map(|(binding, _)| binding.clone());
        let index = match first.and_then(|first| self.correlated_group_of(&first)) {
            Some(index) => {
                self.correlated_groups[index] = group;
                index
            }
            None => {
                self.correlated_groups.push(group);
                self.correlated_groups.len() - 1
            }
        };
        for (binding, property) in plain {
            self.add_guard_alias(
                binding,
                GuardAlias::Correlated {
                    group: index,
                    property,
                },
            );
        }
    }

    /// The elements of one pattern level that are plain bindings with no
    /// default: their binding and the member each reads.
    fn pattern_plain_elements(
        &self,
        pattern: &SlicePattern,
    ) -> Vec<(FlowProductSubject, Arc<str>)> {
        let mut out = Vec::new();
        match pattern {
            SlicePattern::Object { properties, .. } => {
                for (key, element) in properties.iter() {
                    if let (
                        SlicePattern::Binding { binding, .. },
                        None,
                        SlicePatternKey::Named(name),
                    ) = (&element.pattern, &element.default, key)
                    {
                        out.push((FlowProductSubject::Local(*binding), Arc::clone(name)));
                    }
                }
            }
            SlicePattern::Array { elements, .. } => {
                for (index, element) in elements.iter().enumerate() {
                    if let Some(element) = element {
                        if let (SlicePattern::Binding { binding, .. }, None) =
                            (&element.pattern, &element.default)
                        {
                            out.push((
                                FlowProductSubject::Local(*binding),
                                Arc::from(index.to_string().as_str()),
                            ));
                        }
                    }
                }
            }
            SlicePattern::Binding { .. } | SlicePattern::Target { .. } => {}
        }
        out
    }

    fn element_subject(&self, binding: &FlowProductSubject) -> SliceNarrowSubject {
        let name = self
            .products
            .identity(binding)
            .map(|identity| identity.name)
            .unwrap_or_else(|| Arc::from(""));
        let binding = match binding {
            FlowProductSubject::Local(binding) => {
                verter_semantic::analysis::flow::FlowBindingRef::Local(*binding)
            }
            FlowProductSubject::Captured(captured) => {
                verter_semantic::analysis::flow::FlowBindingRef::Captured(captured.clone())
            }
        };
        SliceNarrowSubject {
            root: SliceNarrowRoot::Local { name, binding },
            path: Arc::from(Vec::new().into_boxed_slice()),
        }
    }

    fn correlated_group_of(&self, binding: &FlowProductSubject) -> Option<usize> {
        let key = self.canonical_runtime_subject(binding);
        self.guard_aliases
            .get(&key)?
            .iter()
            .find_map(|alias| match alias {
                GuardAlias::Correlated { group, .. } => Some(*group),
                GuardAlias::Source(_) => None,
            })
    }

    fn add_guard_alias(&mut self, binding: FlowProductSubject, alias: GuardAlias) {
        let key = self.canonical_runtime_subject(&binding);
        let aliases = self.guard_aliases.entry(key).or_default();
        let same = |existing: &GuardAlias| match (existing, &alias) {
            (GuardAlias::Correlated { group: a, .. }, GuardAlias::Correlated { group: b, .. }) => {
                a == b
            }
            (GuardAlias::Source(a), GuardAlias::Source(b)) => a == b,
            _ => false,
        };
        aliases.retain(|existing| !same(existing));
        aliases.push(alias);
    }

    /// The aliases a test of `subject` also narrows — a bare element
    /// reference only.
    fn guard_aliases_of(&self, subject: &SliceNarrowSubject) -> Option<Vec<GuardAlias>> {
        if !subject.path.is_empty() {
            return None;
        }
        let key = self.canonical_runtime_subject(&self.narrow_subject(&subject.root));
        self.guard_aliases.get(&key).cloned()
    }

    /// Whether a switch discriminant or a guard this module does not carry
    /// tests a destructured element that aliases a narrowing: the typed
    /// guard-narrowing gap, never a silently unnarrowed sibling.
    pub(super) fn degrade_unaliased_test(&mut self, subject: &SliceNarrowSubject) {
        if self
            .guard_aliases_of(subject)
            .is_some_and(|aliases| !aliases.is_empty())
        {
            self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing));
        }
    }

    /// [`Self::degrade_unaliased_test`] for a test the checker carries only
    /// through a DISCRIMINANT element: an element whose member
    /// discriminates none of the references it aliases narrows only itself.
    fn degrade_discriminant_test(&mut self, subject: &SliceNarrowSubject) {
        let Some(aliases) = self.guard_aliases_of(subject) else {
            return;
        };
        for alias in aliases {
            let (parent, property) = match alias {
                GuardAlias::Correlated { group, property } => {
                    match self
                        .correlated_groups
                        .get(group)
                        .and_then(|group| group.parent)
                    {
                        Some(parent) => (parent, property),
                        None => {
                            self.record_degradation(FlowReturnDegradation::FlowGap(
                                FlowGap::GuardNarrowing,
                            ));
                            return;
                        }
                    }
                }
                GuardAlias::Source(member) => {
                    let parent_subject = SliceNarrowSubject {
                        root: member.root.clone(),
                        path: Arc::from(
                            member.path[..member.path.len() - 1]
                                .to_vec()
                                .into_boxed_slice(),
                        ),
                    };
                    let Some(parent) = self.reference_node(&parent_subject, true) else {
                        continue;
                    };
                    (parent, Arc::clone(&member.path[member.path.len() - 1]))
                }
            };
            if self.is_discriminant_property(parent, &property) != Some(false) {
                self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing));
                return;
            }
        }
    }

    /// Carry one leaf test of a destructured element onto every reference
    /// it aliases, on the same edge: the correlated pattern's parent (and
    /// through it every sibling element) and the destructured source's
    /// member. A discriminant is a member whose type differs across the
    /// parent's arms and holds a unit literal in one — the only test the
    /// checker carries.
    pub(super) fn apply_guard_aliases(&mut self, guard: &SliceGuard, positive: bool) {
        let subject = match guard {
            SliceGuard::Typeof { subject, .. }
            | SliceGuard::Truthy { subject, .. }
            | SliceGuard::EqLiteral { subject, .. } => subject,
            SliceGuard::Instanceof { subject, .. }
            | SliceGuard::In { subject, .. }
            | SliceGuard::TypePredicate { subject, .. } => {
                self.degrade_discriminant_test(subject);
                return;
            }
            // A value equality or a resolved call's predicate over an
            // element is a test this carry does not model.
            SliceGuard::EqValue { left, right, .. } => {
                for subject in [&left.subject, &right.subject].into_iter().flatten() {
                    self.degrade_discriminant_test(subject);
                }
                return;
            }
            SliceGuard::CallPredicate {
                arguments,
                receiver,
                ..
            } => {
                for subject in arguments.iter().chain(std::iter::once(receiver)).flatten() {
                    self.degrade_discriminant_test(subject);
                }
                return;
            }
            SliceGuard::EqReference { subject, value, .. } => {
                self.degrade_discriminant_test(subject);
                if let crate::flow_slice_content::SliceEqOther::Reference(reference) = value {
                    self.degrade_discriminant_test(reference);
                }
                return;
            }
            SliceGuard::CalleePredicate { arguments, .. } => {
                for subject in arguments.iter().flatten() {
                    self.degrade_discriminant_test(subject);
                }
                return;
            }
            // Each part is its own test under the same polarity.
            SliceGuard::Both(parts) => {
                for part in parts.iter() {
                    self.apply_guard_aliases(part, positive);
                }
                return;
            }
            SliceGuard::None | SliceGuard::And(_) | SliceGuard::Or(_) => return,
        };
        let Some(aliases) = self.guard_aliases_of(subject) else {
            return;
        };
        for alias in aliases {
            match alias {
                GuardAlias::Correlated { group, property } => {
                    let Some(group) = self.correlated_groups.get(group).cloned() else {
                        continue;
                    };
                    let (Some(pseudo), Some(parent)) = (group.pseudo.clone(), group.parent) else {
                        self.record_degradation(FlowReturnDegradation::FlowGap(
                            FlowGap::GuardNarrowing,
                        ));
                        continue;
                    };
                    match self.is_discriminant_property(parent, &property) {
                        Some(true) => {}
                        Some(false) => continue,
                        None => {
                            self.record_degradation(FlowReturnDegradation::FlowGap(
                                FlowGap::GuardNarrowing,
                            ));
                            continue;
                        }
                    }
                    let before = self.narrowed_read(&pseudo);
                    let mut path = pseudo.path.to_vec();
                    path.push(Arc::clone(&property));
                    let member = SliceNarrowSubject {
                        root: pseudo.root.clone(),
                        path: Arc::from(path.into_boxed_slice()),
                    };
                    self.narrow_alias_leaf(guard, &member, positive);
                    let after = self.narrowed_read(&pseudo);
                    if after == before {
                        continue;
                    }
                    let Some(after) = after else { continue };
                    self.retype_correlated_elements(&group, after);
                }
                GuardAlias::Source(member) => {
                    let parent_subject = SliceNarrowSubject {
                        root: member.root.clone(),
                        path: Arc::from(
                            member.path[..member.path.len() - 1]
                                .to_vec()
                                .into_boxed_slice(),
                        ),
                    };
                    let Some(parent) = self.reference_node(&parent_subject, true) else {
                        continue;
                    };
                    let property = &member.path[member.path.len() - 1];
                    match self.is_discriminant_property(parent, property) {
                        Some(true) => self.narrow_alias_leaf(guard, &member, positive),
                        Some(false) => {}
                        None => self.record_degradation(FlowReturnDegradation::FlowGap(
                            FlowGap::GuardNarrowing,
                        )),
                    }
                }
            }
        }
    }

    /// Apply one leaf test to `subject` in place of the guard's own.
    fn narrow_alias_leaf(
        &mut self,
        guard: &SliceGuard,
        subject: &SliceNarrowSubject,
        positive: bool,
    ) {
        let fact = match guard {
            SliceGuard::Typeof { kind, negated, .. } => {
                self.narrow_typeof(subject, *kind, *negated == positive)
            }
            SliceGuard::Truthy { negated, .. } => self.narrow_truthy(subject, *negated == positive),
            SliceGuard::EqLiteral {
                literal,
                negated,
                loose,
                ..
            } => self.narrow_eq_literal(
                subject,
                literal,
                *negated == positive,
                if *loose {
                    LiteralComparison::Loose
                } else {
                    LiteralComparison::Strict
                },
            ),
            _ => return,
        };
        if let GuardNarrowing::Narrowed(subject, node) = fact {
            self.push_narrowing(&subject, node);
        }
    }

    /// Every correlated element reads its member of the narrowed parent,
    /// under the narrowing it already carries.
    fn retype_correlated_elements(&mut self, group: &CorrelatedGroup, parent: SemanticNodeId) {
        for (element, property) in &group.elements {
            if self.resolved_narrow_subject(&element.root).is_none() {
                continue;
            }
            let Some(member) = self.pattern_property(parent, property) else {
                self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing));
                continue;
            };
            let Some(current) = self.subject_current_node(element) else {
                continue;
            };
            if let (Some(narrowed), _) = self.narrow_node_to_candidate(current, member) {
                self.push_narrowing(element, narrowed);
            }
        }
    }

    /// Whether `property` is a discriminant of `parent`: a union whose
    /// arms all carry the member, not all at one type, one of them holding
    /// a unit literal. `None` when a member is not read here.
    pub(super) fn is_discriminant_property(
        &mut self,
        parent: SemanticNodeId,
        property: &Arc<str>,
    ) -> Option<bool> {
        let view = self.dispatch.resolved_reduction_view(parent);
        let Some(arms) = self.dispatch.union_arms_of(view) else {
            return Some(false);
        };
        let mut members = Vec::with_capacity(arms.len());
        for arm in arms.iter() {
            members.push(self.pattern_property(*arm, property)?);
        }
        let uniform = members.windows(2).all(|pair| pair[0] == pair[1]);
        if uniform {
            return Some(false);
        }
        let unit = members.iter().any(|member| {
            self.enumerated_union_arms_or_self(*member)
                .iter()
                .any(|leaf| {
                    matches!(
                        self.dispatch.graph().node_data(*leaf).as_deref(),
                        Some(
                            SemanticNodeData::Literal(_)
                                | SemanticNodeData::Primitive(
                                    crate::semantic_query::PrimitiveKind::Null
                                        | crate::semantic_query::PrimitiveKind::Undefined
                                )
                        )
                    )
                })
        });
        Some(unit)
    }

    /// Whether a destructuring parent is a union the checker correlates
    /// over (through a type parameter's constraint).
    fn parent_union_verdict(&mut self, parent: SemanticNodeId) -> ParentUnion {
        let view = self.dispatch.resolved_reduction_view(parent);
        if self
            .dispatch
            .union_arms_of(view)
            .is_some_and(|arms| arms.len() >= 2)
        {
            return ParentUnion::Union;
        }
        match self.dispatch.graph().node_data(view).as_deref() {
            Some(SemanticNodeData::TypeParam { .. }) => ParentUnion::Undecided,
            Some(SemanticNodeData::Opaque(_)) => ParentUnion::Undecided,
            _ => ParentUnion::NotUnion,
        }
    }
}

enum ParentUnion {
    Union,
    NotUnion,
    Undecided,
}
