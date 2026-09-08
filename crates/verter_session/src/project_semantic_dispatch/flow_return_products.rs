//! Typed accessors over the shared, selected flow product execution.
//!
//! Snapshots retain one canonical product store and share the same execution
//! capability. This adapter performs no lattice operations and owns no second
//! semantic state map.

use std::cell::RefCell;
use std::rc::Rc;
use verter_semantic::analysis::flow::flow_graph::FlowNodeId;
use verter_semantic::analysis::flow::FlowBindingRef;

use super::flow_products::{
    DeclaredTypeProduct, DefiniteAssignmentProduct, FlowProductExecution, FlowProductFailure,
    FlowProductStore, FlowProductTransfer, FlowProductValue, FlowSemanticAlgebra, NarrowingProduct,
    ReachingTypeProduct, ReachingValueProduct, WideningMembership,
};
use super::flow_solve::FlowDomain;
use crate::semantic_query::SemanticNodeId;

/// Declaration lifetime metadata; it never participates in product identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FlowBindingLayer {
    Lexical,
    Function,
}

#[cfg(test)]
mod tests {
    thread_local! {
        pub(super) static WRITE_KEY_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    use super::super::flow_products::{FlowProductBudget, FlowProductInputs, GraphSemanticAlgebra};
    use super::super::flow_solve::{build_flow_demand_plan, FlowDemandRequest, FlowResourcePolicy};
    use super::*;
    use crate::semantic_query::{
        CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
        FlowReturnKey, FlowReturnPolicy, PrimitiveKind, ResolvedDeclSlotIdentity,
        ReturnProjectionDemand, SemanticNodeData, SemanticQueryKey,
    };
    use std::sync::Arc;
    use verter_semantic::analysis::flow::flow_graph::FlowNodeKind;
    use verter_semantic::analysis::flow::hashing::compute_flow_slice_hash;
    use verter_semantic::analysis::flow::peeker::{ReturnPathPeeker, SliceDemand};

    fn fixture(
        source: &str,
        nested: bool,
    ) -> (
        FlowFrameProducts,
        crate::cache_runtime::flow_slice_node::BoundFlowGraph,
        super::super::flow_solve::FlowDemandPlan,
    ) {
        fixture_with_resources(source, nested, FlowResourcePolicy::default())
    }

    fn fixture_with_resources(
        source: &str,
        nested: bool,
        resources: FlowResourcePolicy,
    ) -> (
        FlowFrameProducts,
        crate::cache_runtime::flow_slice_node::BoundFlowGraph,
        super::super::flow_solve::FlowDemandPlan,
    ) {
        let (state, _) =
            crate::resolver_core::ShallowFileState::service_backed_with_provenance_for_test(
                "/ws/product-continuations.ts",
                source,
            );
        let memo = state.decl_bodies();
        let index = memo.function_program_index();
        let entry = index
            .matches_named("products")
            .find(|candidate| candidate.entry().lexical_parent.is_some() == nested)
            .unwrap()
            .entry();

        let bound = memo.flow_bound_graph_for_tests(entry);
        let bundle = bound.bundle();
        let request = FlowDemandRequest {
            query: SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
                function: FlowFunctionSlotIdentity {
                    declaration_slot: ResolvedDeclSlotIdentity::value_slot(
                        Arc::clone(&bound.key().canonical_id),
                        entry.key.declaration.owner,
                        Arc::clone(&entry.key.declaration.name),
                        0,
                        [0; 16],
                        [0; 16],
                    ),
                    function_part: entry.key.part.clone(),
                    overload_ordinal: entry.key.overload_ordinal,
                },
                normalized_type_args: Arc::from([]),
                context: FlowReturnContext {
                    parse_env_hash: bound.key().parse_env_hash,
                    resolve_env_hash: [0; 16],
                    type_env_hash: [0; 16],
                    lib_env_hash: [0; 16],
                    project_identity: [0; 16],
                    type_substitution: CanonicalTypeSubstitution::empty(),
                    policy: FlowReturnPolicy {},
                },
                demand: ReturnProjectionDemand::whole_return(),
                input: FlowInputContext::empty(),
                result_contract: super::super::flow_solve::flow_return_result_contract_id(),
            })),
            input_basis: verter_identity::identity::InputBasisId::from_canonical(
                &super::super::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance::new(
                    1, 1, 1, 0,
                ),
            ),
            resources,
            additional_requirements: Arc::from([]),
        };
        let selection = ReturnPathPeeker::new(&bundle.graph)
            .plan(
                &SliceDemand::for_return_projection(&bundle.skeleton, &[]),
                &request.resources.slice_budget,
            )
            .unwrap();
        let retained = crate::cache_runtime::flow_slice_node::PlannedFlowSlice::for_test(
            compute_flow_slice_hash(&selection, &bundle.graph, &bundle.skeleton),
            selection,
        );
        let plan = build_flow_demand_plan(request, &bound, &retained).unwrap();
        let execution = FlowProductExecution::new(
            &FlowProductInputs::for_bound_graph(&bound),
            &plan,
            FlowProductBudget::for_demand_plan(&plan),
        )
        .unwrap();
        let products = FlowFrameProducts::new(execution, &bound).unwrap();
        (products, bound, plan)
    }

    #[test]
    fn continuation_rewind_and_join_preserve_only_actual_reaching_definitions() {
        let (mut products, bound, plan) = fixture(
            "function products(flag: boolean) { let x = 0; if (flag) { x = 1; x = 2; } else { x = 3; } return x; }",
            false,
        );
        let bundle = bound.bundle();
        let binding = bundle
            .graph
            .nodes()
            .find_map(|node| {
                let FlowNodeKind::Binding(binding) = bundle.graph.node_kind(node) else {
                    return None;
                };
                (bundle.bindings.identity(binding)?.name.as_ref() == "x").then_some(binding)
            })
            .unwrap();
        let subject = FlowBindingRef::Local(binding);
        let initializer = bundle
            .graph
            .expr_site_node(bundle.skeleton.binding(binding).initializer.unwrap());
        let writes: Vec<_> = bundle
            .skeleton
            .writes
            .iter()
            .map(|write| {
                bundle
                    .graph
                    .expr_site_node(write.value.expect("assignment RHS"))
            })
            .collect();
        assert_eq!(writes.len(), 3);
        let graph = crate::semantic_query_memo::SemanticGraphStore::new();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        products.bind_at(
            &subject,
            DefiniteAssignmentProduct::assigned(),
            ReachingTypeProduct::of(number),
            Some(initializer),
        );
        let entry = products.clone();
        let entry_writes = products.observe_writes();
        products.bind_at(
            &subject,
            DefiniteAssignmentProduct::assigned(),
            ReachingTypeProduct::of(number),
            Some(writes[0]),
        );
        products.bind_at(
            &subject,
            DefiniteAssignmentProduct::assigned(),
            ReachingTypeProduct::of(number),
            Some(writes[1]),
        );
        let consequent = products.clone();
        products.restore_reaching_from(&subject, &entry, None, false);
        let definitions = |products: &FlowFrameProducts| {
            let Some(FlowProductValue::ReachingValue(value)) =
                products.get(FlowDomain::ReachingValue, &subject)
            else {
                panic!("an established binding retains definition provenance");
            };
            value.definitions().to_vec()
        };
        assert_eq!(
            definitions(&products),
            vec![initializer],
            "arm rewind restores definition provenance with its type"
        );
        products.bind_at(
            &subject,
            DefiniteAssignmentProduct::assigned(),
            ReachingTypeProduct::of(number),
            Some(writes[2]),
        );
        let joined = FlowFrameProducts::join(
            &[&consequent, &products],
            &entry_writes,
            &GraphSemanticAlgebra(&graph),
        );
        let mut expected = vec![writes[1], writes[2]];
        expected.sort_unstable_by_key(|node| node.index());
        assert_eq!(definitions(&joined), expected, "the stale overwritten arm and entry definition are absent from the actual predecessor join");
        assert!(joined.finish(Some(&plan)).unwrap().is_some());
    }
    #[test]
    fn unchanged_write_receipts_skip_unrelated_history_and_follow_actual_continuations() {
        for count in [32, 512] {
            let declarations = (0..count)
                .map(|i| format!("let x{i}=0;"))
                .collect::<String>();
            let members = (0..count)
                .map(|i| format!("x{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let source = format!("function products(){{{declarations}return {{{members}}};}}");
            let (mut products, bound, _) = fixture_with_resources(
                &source,
                false,
                FlowResourcePolicy {
                    max_obligations: 30_000,
                    ..Default::default()
                },
            );
            let graph = crate::semantic_query_memo::SemanticGraphStore::new();
            let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            let bindings: Vec<_> = bound
                .bundle()
                .graph
                .nodes()
                .filter_map(|node| match bound.bundle().graph.node_kind(node) {
                    FlowNodeKind::Binding(binding) => Some(binding),
                    _ => None,
                })
                .collect();
            assert_eq!(bindings.len(), count);
            for binding in &bindings {
                products.bind_at(
                    &FlowBindingRef::Local(*binding),
                    DefiniteAssignmentProduct::assigned(),
                    ReachingTypeProduct::of(number),
                    Some(
                        bound.bundle().graph.expr_site_node(
                            bound
                                .bundle()
                                .skeleton
                                .binding(*binding)
                                .initializer
                                .unwrap(),
                        ),
                    ),
                );
                assert!(
                    products.execution.borrow().failure.is_none(),
                    "input binding {binding:?}: {:?}",
                    products.execution.borrow().failure
                );
            }
            let before = products.clone();
            let observation = products.observe_writes();
            assert!(
                before.writes.ptr_eq(&products.writes),
                "snapshots share receipt storage"
            );
            assert!(before
                .writes_by_sequence
                .ptr_eq(&products.writes_by_sequence));
            let binding = bindings[count / 2];
            let subject = FlowBindingRef::Local(binding);
            let definition = bound.bundle().graph.expr_site_node(
                bound
                    .bundle()
                    .skeleton
                    .binding(binding)
                    .initializer
                    .unwrap(),
            );
            products.bind_at(
                &subject,
                DefiniteAssignmentProduct::assigned(),
                ReachingTypeProduct::of(number),
                Some(definition),
            );
            assert!(
                products.execution.borrow().failure.is_none(),
                "repeated write: {:?}",
                products.execution.borrow().failure
            );
            let write_subject = products.write_subject(&subject).unwrap();
            assert!(
                products.writes.get(&write_subject).unwrap().sequence
                    > before.writes.get(&write_subject).unwrap().sequence,
                "a successful unchanged write advances its receipt"
            );
            for domain in [
                FlowDomain::ReachingValue,
                FlowDomain::ReachingType,
                FlowDomain::DefiniteAssignment,
            ] {
                assert_eq!(
                    products.get(domain, &subject),
                    before.get(domain, &subject),
                    "the repeated site succeeds without changing products"
                );
            }
            WRITE_KEY_COMPARISONS.with(|work| work.set(0));
            assert_eq!(
                products.writes_since(&observation),
                std::slice::from_ref(&subject)
            );
            let comparisons = WRITE_KEY_COMPARISONS.with(|work| work.get());
            assert!(
                comparisons <= 128,
                "one write after {count} prior bindings compared {comparisons} keys"
            );
            assert_eq!(products.writes.len(), count);
            assert_eq!(
                products.writes_by_sequence.len(),
                count,
                "replacement removes the earlier sequence entry"
            );
            let surviving =
                FlowFrameProducts::join(&[&before], &observation, &GraphSemanticAlgebra(&graph));
            assert!(
                surviving.writes_since(&observation).is_empty(),
                "a terminated arm contributes no receipt"
            );
            WRITE_KEY_COMPARISONS.with(|work| work.set(0));
            let continuing = FlowFrameProducts::join(
                &[&before, &products],
                &observation,
                &GraphSemanticAlgebra(&graph),
            );
            let merge_comparisons = WRITE_KEY_COMPARISONS.with(|work| work.get());
            assert!(merge_comparisons <= 256, "one changed receipt after {count} prior bindings compared {merge_comparisons} keys during merge");
            assert_eq!(
                continuing.writes_since(&observation),
                std::slice::from_ref(&subject),
                "an untouched predecessor cannot hide an executed write"
            );
            let after_other_arm = before.observe_writes();
            assert!(products.writes_since(&after_other_arm).is_empty(), "entry observations use the shared execution clock, including work on other snapshots");
            for _ in 0..16 {
                products.bind_at(
                    &subject,
                    DefiniteAssignmentProduct::assigned(),
                    ReachingTypeProduct::of(number),
                    Some(definition),
                );
            }
            assert_eq!(
                products.writes_since(&after_other_arm),
                std::slice::from_ref(&subject)
            );
            assert_eq!(products.writes.len(), count);
            assert_eq!(products.writes_by_sequence.len(), count);
            products.restore_reaching_from(&subject, &before, None, true);
            assert!(
                products.writes_since(&observation).is_empty(),
                "rewind restores matching control receipts"
            );
        }
    }

    #[test]
    fn clause_write_observations_reject_foreign_executions() {
        let source = "function products(x:number){return x;}";
        let (products, _, _) = fixture(source, false);
        let (foreign, _, _) = fixture(source, false);
        let observation = foreign.observe_writes();
        assert!(products.writes_since(&observation).is_empty());
        assert_eq!(
            products.execution.borrow().failure,
            Some(FlowProductFailure::ScopeMismatch)
        );
        let (other, _, _) = fixture(source, false);
        let graph = crate::semantic_query_memo::SemanticGraphStore::new();
        let joined =
            FlowFrameProducts::join(&[&other], &observation, &GraphSemanticAlgebra(&graph));
        assert_eq!(
            joined.execution.borrow().failure,
            Some(FlowProductFailure::ScopeMismatch)
        );
    }

    #[test]
    fn clause_write_replay_moves_every_runtime_domain_and_kills_old_guards() {
        let (mut entering, bound, _) =
            fixture("function products(x:string|number){x=1;return x;}", false);
        let binding = bound
            .bundle()
            .graph
            .nodes()
            .find_map(|node| match bound.bundle().graph.node_kind(node) {
                FlowNodeKind::Binding(binding)
                    if bound.bundle().skeleton.binding(binding).kind
                        == verter_semantic::analysis::flow::SkeletonBindingKind::Param =>
                {
                    Some(binding)
                }
                _ => None,
            })
            .unwrap();
        let subject = FlowBindingRef::Local(binding);
        let graph = crate::semantic_query_memo::SemanticGraphStore::new();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        entering.bind(
            &subject,
            DefiniteAssignmentProduct::default(),
            ReachingTypeProduct::of(string),
        );
        entering.set_declared_type(&subject, Some(string));
        entering.set_narrowing(
            &subject,
            NarrowingProduct::new([super::super::flow_products::FlowNarrowingFact {
                binding: entering.identity(&subject).unwrap(),
                path: Arc::from([]),
                narrowed_to: string,
            }]),
        );
        let observation = entering.observe_writes();
        let mut end = entering.clone();
        let definition = bound
            .bundle()
            .graph
            .expr_site_node(bound.bundle().skeleton.writes[0].value.unwrap());
        end.bind_at(
            &subject,
            DefiniteAssignmentProduct::assigned(),
            ReachingTypeProduct::of(number),
            Some(definition),
        );
        assert_eq!(
            end.writes_since(&observation),
            std::slice::from_ref(&subject)
        );
        entering.apply_executed_write_from(&subject, &end);
        for domain in [
            FlowDomain::ReachingValue,
            FlowDomain::ReachingType,
            FlowDomain::DefiniteAssignment,
        ] {
            assert_eq!(entering.get(domain, &subject), end.get(domain, &subject));
        }
        assert!(entering.narrowing(&subject).is_none());
        assert_eq!(
            entering.declared_type(&subject),
            Some(string),
            "source authority is independent of continuation writes"
        );
    }

    #[test]
    fn declared_capture_input_records_its_hub_and_preserves_unassigned_state() {
        let (mut products, bound, plan) = fixture(
            "function products() { const read = () => x; let x: 'a' | 'b' = 'a'; return read; }",
            true,
        );
        let (hub, identity) = bound
            .bundle()
            .graph
            .nodes()
            .find_map(|node| {
                let FlowNodeKind::CapturedBinding(binding) = bound.bundle().graph.node_kind(node)
                else {
                    return None;
                };
                Some((node, bound.bundle().graph.captured_binding(binding).clone()))
            })
            .expect("real selected captured hub");
        let graph = crate::semantic_query_memo::SemanticGraphStore::new();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let unassigned = DefiniteAssignmentProduct::default();
        let subject = FlowBindingRef::Captured(identity);
        products.import_capture_input(
            &subject,
            unassigned,
            Some(ReachingTypeProduct::of(string)),
            Some(string),
        );
        assert_eq!(
            products.assignment(&subject),
            unassigned,
            "importing declared input does not execute the later initializer"
        );
        let Some(FlowProductValue::ReachingValue(definitions)) =
            products.get(FlowDomain::ReachingValue, &subject)
        else {
            panic!("declared capture input needs exact definition provenance");
        };
        assert_eq!(
            definitions.definitions(),
            &[hub],
            "only the child's actual input hub defines the imported value"
        );
        let key = products.key(FlowDomain::ReachingValue, &subject).unwrap();
        let evidence = products.finish(Some(&plan)).unwrap().unwrap();
        assert!(evidence.executed(&key));
    }
}

#[derive(Clone, PartialEq, Eq)]
struct FlowWriteSubject(
    verter_semantic::analysis::function_program::FunctionProgramKey,
    u32,
);

impl PartialOrd for FlowWriteSubject {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FlowWriteSubject {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(test)]
        tests::WRITE_KEY_COMPARISONS.with(|count| count.set(count.get() + 1));
        (&self.0, self.1).cmp(&(&other.0, other.1))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FlowWriteSequence(u64);
impl PartialOrd for FlowWriteSequence {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FlowWriteSequence {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(test)]
        tests::WRITE_KEY_COMPARISONS.with(|count| count.set(count.get() + 1));
        self.0.cmp(&other.0)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct FlowWriteReceipt {
    sequence: u64,
    subject: FlowBindingRef,
}

pub(super) struct FlowFrameExecution {
    pub execution: FlowProductExecution,
    pub content: super::flow_products::FlowProductContent,
    pub failure: Option<FlowProductFailure>,
    write_sequence: u64,
}

pub(super) type SharedFlowExecution = Rc<RefCell<FlowFrameExecution>>;

/// A scoped observation of successful transfers at a control entry.
#[derive(Clone)]
pub(super) struct FlowWriteObservation {
    execution: SharedFlowExecution,
    sequence: u64,
}

/// A snapshot of the sole product store, with typed binding accessors.
#[derive(Clone)]
pub(super) struct FlowFrameProducts {
    pub state: FlowProductStore,
    pub execution: SharedFlowExecution,
    // Successful write receipts are control metadata, not semantic values.
    // Persistent snapshots retain exactly the writes on their own paths.
    writes: imbl::OrdMap<FlowWriteSubject, FlowWriteReceipt>,
    writes_by_sequence: imbl::OrdMap<FlowWriteSequence, FlowBindingRef>,
}

impl FlowFrameProducts {
    pub fn captured_input_bytes(
        declared: Option<SemanticNodeId>,
        reaching: Option<&ReachingTypeProduct>,
        assignment: DefiniteAssignmentProduct,
    ) -> Vec<u8> {
        use verter_identity::encoding::CanonicalEncoder;
        let mut e = CanonicalEncoder::new("verter.session.flow.captured_input.v1");
        let declared = declared.map(|node| node.0.to_le_bytes());
        e.field_option(1, declared.as_ref().map(|bytes| bytes.as_slice()));
        if let Some(reaching) = reaching {
            e.field_bool(2, true);
            for node in reaching.contributors() {
                e.field_u64(3, node.0);
            }
            let united = reaching.united().map(|node| node.0.to_le_bytes());
            e.field_option(4, united.as_ref().map(|bytes| bytes.as_slice()));
            match reaching.widening() {
                None => {
                    e.field_enum_discriminant(5, 0);
                }
                Some(WideningMembership::All) => {
                    e.field_enum_discriminant(5, 1);
                }
                Some(WideningMembership::Partial(values)) => {
                    e.field_enum_discriminant(5, 2);
                    for node in values.iter() {
                        e.field_u64(6, node.0);
                    }
                }
            }
        } else {
            e.field_bool(2, false);
        }
        e.field_bool(7, assignment.single_path());
        e.field_bool(8, assignment.failed_initializer());
        for (tag, state) in [
            super::flow_products::DefiniteAssignment::Unassigned,
            super::flow_products::DefiniteAssignment::Assigned,
            super::flow_products::DefiniteAssignment::MaybeAssigned,
        ]
        .into_iter()
        .enumerate()
        {
            if assignment == assignment.with_state(state) {
                e.field_u32(9, tag as u32);
            }
        }
        e.finish()
    }

    pub fn new(
        execution: FlowProductExecution,
        bound: &crate::cache_runtime::flow_slice_node::BoundFlowGraph,
    ) -> Result<Self, FlowProductFailure> {
        let content = execution.attach_content(bound)?;
        let state = execution.empty_state();
        Ok(Self {
            state,
            writes: imbl::OrdMap::new(),
            writes_by_sequence: imbl::OrdMap::new(),
            execution: Rc::new(RefCell::new(FlowFrameExecution {
                execution,
                content,
                failure: None,
                write_sequence: 0,
            })),
        })
    }

    pub fn contains_subject(&self, subject: &FlowBindingRef) -> bool {
        self.execution
            .borrow()
            .content
            .key_for_binding(FlowDomain::ReachingType, subject)
            .is_ok()
    }

    pub fn identity(
        &self,
        subject: &FlowBindingRef,
    ) -> Option<verter_semantic::analysis::function_program::FlowBindingIdentity> {
        self.execution
            .borrow()
            .content
            .key_for_binding(FlowDomain::ReachingType, subject)
            .ok()?
            .runtime_binding()
            .cloned()
    }

    pub fn key(
        &self,
        domain: FlowDomain,
        subject: &FlowBindingRef,
    ) -> Option<super::flow_products::FlowProductKey> {
        self.execution
            .borrow()
            .content
            .key_for_binding(domain, subject)
            .ok()
    }

    pub fn finish(
        &self,
        plan: Option<&super::flow_solve::FlowDemandPlan>,
    ) -> Result<Option<super::flow_products::FlowProductEvidence>, FlowProductFailure> {
        let mut frame = self.execution.borrow_mut();
        if let Some(failure) = &frame.failure {
            return Err(failure.clone());
        }
        match plan {
            Some(plan) => frame.execution.finish_with_plan(plan).map(Some),
            None => frame.execution.finish().map(|_| None),
        }
    }

    fn get(&self, domain: FlowDomain, subject: &FlowBindingRef) -> Option<&FlowProductValue> {
        let key = self
            .execution
            .borrow()
            .content
            .key_for_binding(domain, subject)
            .ok()?;
        self.state.get(&key)
    }

    fn set(
        &mut self,
        domain: FlowDomain,
        subject: &FlowBindingRef,
        value: Option<FlowProductValue>,
    ) {
        self.set_values(subject, [(domain, value)]);
    }

    fn set_values(
        &mut self,
        subject: &FlowBindingRef,
        values: impl IntoIterator<Item = (FlowDomain, Option<FlowProductValue>)>,
    ) -> bool {
        let mut frame = self.execution.borrow_mut();
        if frame.failure.is_some() {
            return false;
        }
        let result: Result<bool, FlowProductFailure> = (|| {
            let transfers = values.into_iter().map(|(domain, value)| {
                let key = frame.content.key_for_binding(domain, subject)?;
                Ok(FlowProductTransfer { key, value })
            }).collect::<Result<smallvec::SmallVec<[FlowProductTransfer; 4]>, FlowProductFailure>>()?;
            let Some(first) = transfers.first() else {
                return Ok(false);
            };
            let site = first.key.site();
            frame
                .execution
                .apply_transfers(&mut self.state, &site, &transfers)
        })();
        if let Err(failure) = result {
            frame.failure = Some(failure);
            return false;
        }
        true
    }

    /// Import the exact prepared child input. Its selected captured hub is the
    /// definition site; the enclosing initializer and assignment state are not
    /// fabricated by annotation hydration.
    pub fn import_capture_input(
        &mut self,
        subject: &FlowBindingRef,
        assignment: DefiniteAssignmentProduct,
        reaching: Option<ReachingTypeProduct>,
        declared: Option<SemanticNodeId>,
    ) {
        self.set_declared_type(subject, declared);
        if let Some(reaching) = reaching {
            self.bind(subject, assignment, reaching);
        } else {
            self.set_assignment(subject, assignment);
        }
    }

    /// Establish the assignment and reaching value and invalidate the replaced
    /// value's guard facts as one executed, transactional binding operation.
    pub fn bind(
        &mut self,
        subject: &FlowBindingRef,
        assignment: DefiniteAssignmentProduct,
        reaching: ReachingTypeProduct,
    ) {
        let definition = self
            .key(FlowDomain::ReachingValue, subject)
            .map(|key| key.node());
        self.bind_at(subject, assignment, reaching, definition);
    }

    pub fn bind_at(
        &mut self,
        subject: &FlowBindingRef,
        assignment: DefiniteAssignmentProduct,
        reaching: ReachingTypeProduct,
        definition: Option<FlowNodeId>,
    ) {
        let Some((write_subject, sequence)) = self.prepare_write(subject) else {
            return;
        };
        let reaching_value = {
            let mut frame = self.execution.borrow_mut();
            let site = definition
                .ok_or(super::flow_products::FlowProductKeyError::UnselectedNode)
                .and_then(|definition| frame.content.site(definition));
            match site {
                Ok(site) => ReachingValueProduct::at(&site),
                Err(failure) => {
                    frame.failure = Some(failure.into());
                    return;
                }
            }
        };
        if self.set_values(
            subject,
            [
                (
                    FlowDomain::DefiniteAssignment,
                    Some(FlowProductValue::DefiniteAssignment(assignment)),
                ),
                (
                    FlowDomain::ReachingType,
                    Some(FlowProductValue::ReachingType(reaching)),
                ),
                (FlowDomain::Narrowing, None),
                (
                    FlowDomain::ReachingValue,
                    Some(FlowProductValue::ReachingValue(reaching_value)),
                ),
            ],
        ) {
            self.execution.borrow_mut().write_sequence = sequence;
            self.install_write(
                write_subject,
                FlowWriteReceipt {
                    sequence,
                    subject: subject.clone(),
                },
            );
        }
    }

    fn write_subject(&self, subject: &FlowBindingRef) -> Option<FlowWriteSubject> {
        let identity = self.identity(subject)?;
        Some(FlowWriteSubject(
            identity.defining_function,
            identity.binding_slot,
        ))
    }

    fn prepare_write(&self, subject: &FlowBindingRef) -> Option<(FlowWriteSubject, u64)> {
        let mut frame = self.execution.borrow_mut();
        let identity = match frame
            .content
            .key_for_binding(FlowDomain::ReachingValue, subject)
        {
            Ok(key) => key.runtime_binding().cloned(),
            Err(failure) => {
                frame.failure = Some(failure.into());
                return None;
            }
        };
        let Some(identity) = identity else {
            frame.failure =
                Some(super::flow_products::FlowProductKeyError::UnmodeledBinding.into());
            return None;
        };
        let key = FlowWriteSubject(identity.defining_function, identity.binding_slot);
        let Some(sequence) = frame.write_sequence.checked_add(1) else {
            frame.failure = Some(FlowProductFailure::Gap(
                crate::semantic_query::FlowGap::UnmodeledExpression,
            ));
            return None;
        };
        Some((key, sequence))
    }

    fn install_write(&mut self, subject: FlowWriteSubject, receipt: FlowWriteReceipt) {
        if let Some(previous) = self.writes.insert(subject, receipt.clone()) {
            self.writes_by_sequence
                .remove(&FlowWriteSequence(previous.sequence));
        }
        self.writes_by_sequence
            .insert(FlowWriteSequence(receipt.sequence), receipt.subject);
    }

    fn remove_write(&mut self, subject: &FlowWriteSubject) {
        if let Some(previous) = self.writes.remove(subject) {
            self.writes_by_sequence
                .remove(&FlowWriteSequence(previous.sequence));
        }
    }

    pub fn observe_writes(&self) -> FlowWriteObservation {
        FlowWriteObservation {
            execution: Rc::clone(&self.execution),
            sequence: self.execution.borrow().write_sequence,
        }
    }

    fn accepts_observation(&self, observation: &FlowWriteObservation) -> bool {
        if Rc::ptr_eq(&self.execution, &observation.execution) {
            return true;
        }
        self.execution.borrow_mut().failure = Some(FlowProductFailure::ScopeMismatch);
        false
    }

    pub fn writes_since(&self, observation: &FlowWriteObservation) -> Vec<FlowBindingRef> {
        if !self.accepts_observation(observation) {
            return Vec::new();
        }
        // Seek past the shared execution's explicit control-entry sequence.
        // Only the latest receipt per selected subject is retained.
        self.writes_by_sequence
            .range((
                std::ops::Bound::Excluded(FlowWriteSequence(observation.sequence)),
                std::ops::Bound::Unbounded,
            ))
            .map(|(_, subject)| subject.clone())
            .collect()
    }

    pub fn join(
        incoming: &[&Self],
        observation: &FlowWriteObservation,
        algebra: &dyn FlowSemanticAlgebra,
    ) -> Self {
        let first = incoming.first().expect("a continuation has a predecessor");
        let mut joined = (*first).clone();
        if !first.accepts_observation(observation) || incoming.len() == 1 {
            return joined;
        }
        let mut frame = first.execution.borrow_mut();
        if frame.failure.is_some() {
            return joined;
        }
        let states: smallvec::SmallVec<[&FlowProductStore; 4]> =
            incoming.iter().map(|products| &products.state).collect();
        match frame.execution.join_products(&states, algebra) {
            Ok(state) => {
                joined.state = state;
                for predecessor in &incoming[1..] {
                    for (sequence, binding) in predecessor.writes_by_sequence.range((
                        std::ops::Bound::Excluded(FlowWriteSequence(observation.sequence)),
                        std::ops::Bound::Unbounded,
                    )) {
                        let identity = frame
                            .content
                            .key_for_binding(FlowDomain::ReachingValue, binding)
                            .ok()
                            .and_then(|key| key.runtime_binding().cloned())
                            .expect("successful receipt retains its selected runtime identity");
                        let subject =
                            FlowWriteSubject(identity.defining_function, identity.binding_slot);
                        if joined
                            .writes
                            .get(&subject)
                            .is_none_or(|earlier| sequence.0 > earlier.sequence)
                        {
                            joined.install_write(
                                subject,
                                FlowWriteReceipt {
                                    sequence: sequence.0,
                                    subject: binding.clone(),
                                },
                            );
                        }
                    }
                }
            }
            Err(failure) => frame.failure = Some(failure),
        }
        joined
    }

    pub fn remove(&mut self, domain: FlowDomain, subject: &FlowBindingRef) {
        if self.set_values(subject, [(domain, None)]) && domain == FlowDomain::ReachingValue {
            if let Some(identity) = self.write_subject(subject) {
                self.remove_write(&identity);
            }
        }
    }

    /// Rewind a continuation's runtime products as one replacement. Authored
    /// declaration authority lives independently of these snapshots.
    pub fn restore_reaching_from(
        &mut self,
        subject: &FlowBindingRef,
        source: &Self,
        assignment: Option<DefiniteAssignmentProduct>,
        restore_narrowing: bool,
    ) {
        let mut values = smallvec::SmallVec::<[(FlowDomain, Option<FlowProductValue>); 4]>::new();
        for domain in [
            FlowDomain::ReachingValue,
            FlowDomain::ReachingType,
            FlowDomain::DefiniteAssignment,
        ] {
            let value = if domain == FlowDomain::DefiniteAssignment {
                assignment
                    .map(FlowProductValue::DefiniteAssignment)
                    .or_else(|| source.get(domain, subject).cloned())
            } else {
                source.get(domain, subject).cloned()
            };
            values.push((domain, value));
        }
        if restore_narrowing {
            values.push((
                FlowDomain::Narrowing,
                source.get(FlowDomain::Narrowing, subject).cloned(),
            ));
        }
        if self.set_values(subject, values) {
            if let Some(identity) = self.write_subject(subject) {
                if let Some(sequence) = source.writes.get(&identity) {
                    self.install_write(identity, sequence.clone());
                } else {
                    self.remove_write(&identity);
                }
            }
        }
    }

    /// Replay a successfully executed clause write onto its normal continuation.
    /// All runtime domains move together, and the old value's guards are killed.
    pub fn apply_executed_write_from(&mut self, subject: &FlowBindingRef, source: &Self) {
        let values: smallvec::SmallVec<[(FlowDomain, Option<FlowProductValue>); 4]> = [
            FlowDomain::ReachingValue,
            FlowDomain::ReachingType,
            FlowDomain::DefiniteAssignment,
        ]
        .into_iter()
        .map(|domain| (domain, source.get(domain, subject).cloned()))
        .chain(std::iter::once((FlowDomain::Narrowing, None)))
        .collect();
        if self.set_values(subject, values) {
            if let Some(identity) = self.write_subject(subject) {
                if let Some(sequence) = source.writes.get(&identity) {
                    self.install_write(identity, sequence.clone());
                }
            }
        }
    }

    pub fn reaching_type(&self, subject: &FlowBindingRef) -> Option<&ReachingTypeProduct> {
        match self.get(FlowDomain::ReachingType, subject)? {
            FlowProductValue::ReachingType(value) => Some(value),
            _ => None,
        }
    }

    pub fn reaching(&self, subject: &FlowBindingRef) -> Option<SemanticNodeId> {
        self.reaching_type(subject)?.united()
    }

    pub fn set_reaching_type(&mut self, subject: &FlowBindingRef, value: ReachingTypeProduct) {
        self.set(
            FlowDomain::ReachingType,
            subject,
            Some(FlowProductValue::ReachingType(value)),
        );
    }

    pub fn widening(&self, subject: &FlowBindingRef) -> Option<&WideningMembership> {
        self.reaching_type(subject)?.widening()
    }

    pub fn declared_type(&self, subject: &FlowBindingRef) -> Option<SemanticNodeId> {
        match self.get(FlowDomain::DeclaredType, subject)? {
            FlowProductValue::DeclaredType(value) => value.declared(),
            _ => None,
        }
    }

    /// Runtime reads use the kernel's installed source-order authority index.
    /// The exact-only accessor above remains the source hydration boundary.
    pub fn runtime_declared_type(&self, subject: &FlowBindingRef) -> Option<SemanticNodeId> {
        let key = self.key(FlowDomain::DeclaredType, subject)?;
        self.state.declared_type(&key)
    }

    pub fn set_declared_type(&mut self, subject: &FlowBindingRef, value: Option<SemanticNodeId>) {
        let Some(value) = value else {
            return;
        };
        self.set(
            FlowDomain::DeclaredType,
            subject,
            Some(FlowProductValue::DeclaredType(DeclaredTypeProduct::of(
                value,
            ))),
        );
    }

    pub fn assignment(&self, subject: &FlowBindingRef) -> DefiniteAssignmentProduct {
        match self.get(FlowDomain::DefiniteAssignment, subject) {
            Some(FlowProductValue::DefiniteAssignment(value)) => *value,
            _ => DefiniteAssignmentProduct::default(),
        }
    }

    pub fn set_assignment(&mut self, subject: &FlowBindingRef, value: DefiniteAssignmentProduct) {
        self.set(
            FlowDomain::DefiniteAssignment,
            subject,
            Some(FlowProductValue::DefiniteAssignment(value)),
        );
    }

    pub fn narrowing(&self, subject: &FlowBindingRef) -> Option<&NarrowingProduct> {
        match self.get(FlowDomain::Narrowing, subject)? {
            FlowProductValue::Narrowing(value) => Some(value),
            _ => None,
        }
    }

    pub fn set_narrowing(&mut self, subject: &FlowBindingRef, value: NarrowingProduct) {
        self.set(
            FlowDomain::Narrowing,
            subject,
            (!value.facts().is_empty()).then_some(FlowProductValue::Narrowing(value)),
        );
    }

    pub fn subjects_in(&self, domain: FlowDomain) -> Vec<FlowBindingRef> {
        self.state
            .ordered_entries()
            .filter_map(|(key, _)| {
                if key.domain() != domain {
                    return None;
                }
                key.binding_ref().cloned()
            })
            .collect()
    }

    pub fn subjects(&self) -> Vec<FlowBindingRef> {
        let mut seen = rustc_hash::FxHashSet::default();
        self.state
            .ordered_entries()
            .filter_map(|(key, _)| {
                let subject = key.binding_ref()?.clone();
                seen.insert(subject.clone()).then_some(subject)
            })
            .collect()
    }
}
