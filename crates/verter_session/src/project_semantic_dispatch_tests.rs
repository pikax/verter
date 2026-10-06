//! Session-composed suites for the project-semantic dispatcher.
//!
//! Every suite here drives the dispatcher through a live session host,
//! session-owned fixtures or the parser front-end, so it lives with the
//! session rather than beside the dispatcher. Shared harness suites
//! (`checker_probe_lane_tests`, `differential_harness_tests`,
//! `call_resolve_tests`, ...) stay siblings of the suites that use them.

use std::sync::Arc;

use verter_session_query::type_solver::host::ResolvedRootIdentity;

#[allow(unused_imports)]
use crate::project_semantic_dispatch::*;
use crate::semantic_query::{
    DepVersion, IndexKey, OriginMeta, PrimitiveKind, QueryError, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryValue,
};
use crate::semantic_query_memo::SemanticGraphStore;

mod abstract_construct_tests;
mod ambient_module_value_tests;
mod apparent_type_tests;
mod base_signature_tests;
mod broad_runtime_tests;
mod call_resolve_tests;
mod callee_signature_effect_tests;
mod captured_declared_type_tests;
mod carrier_head_resolution_tests;
mod carrier_materialize_tests;
mod carrier_reduction_tests;
mod carrier_type_param_descent_tests;
mod checker_probe_lane_tests;
mod class_member_return_tests;
mod class_owner_tests;
mod class_prototype_property_tests;
mod class_self_reference_tests;
mod class_value_heritage_tests;
mod closedness_evaluator_tests;
mod closure_narrowing_tests;
mod conditional_decision_tests;
mod conditional_indexed_check_tests;
mod conditional_operand_tests;
mod conditional_tail_tests;
mod connected_demand_tests;
mod const_literal_widening_tests;
mod constrained_infer_tests;
mod continuation_depth_tests;
mod cost_receipt_tests;
mod cycle_gate_tests;
mod deep_input_tests;
mod differential_call_tests;
mod differential_class_tests;
mod differential_depth_tests;
mod differential_fishing_tests;
mod differential_flow_tests;
mod differential_global_library_tests;
mod differential_harness_tests;
mod differential_inference_tests;
mod differential_literal_tests;
mod differential_module_tests;
mod differential_narrowing_tests;
mod differential_relation_tests;
mod differential_type_operator_tests;
mod dispatch_txn_flow_demand_tests;
mod enum_literal_tests;
mod equality_value_narrowing_tests;
mod flow_narrowing_parity_tests;
mod flow_return_accessor_tests;
mod flow_return_class_tests;
mod flow_return_construct_tests;
pub(crate) mod flow_return_coverage_tests;
pub(crate) mod flow_return_frame_seal_tests;
mod flow_return_global_tests;
pub(crate) mod flow_return_lexical_tests;
pub(crate) mod flow_return_loop_completion_tests;
mod flow_return_null_policy_tests;
pub(crate) mod flow_return_positional_tests;
mod flow_return_products_tests;
pub(crate) mod flow_return_root_gate_tests;
mod flow_return_tagged_template_tests;
pub(crate) mod flow_return_tests;
mod flow_return_type_argument_default_tests;
mod freshness_tests;
mod generic_source_inference_tests;
mod helper_depth_tests;
mod heritage_signature_tests;
mod homomorphic_mapped_tests;
mod index_signature_access_tests;
mod indexed_access_name_tests;
mod indexed_access_relation_tests;
mod infer_inventory_tests;
mod inference_census_tests;
mod inference_fixation_tests;
mod intersection_complexity_tests;
mod intersection_distribution_tests;
mod keyof_application_tests;
mod lib_global_tests;
mod local_declaration_tests;
mod mapped_key_domain_carrier_tests;
mod member_accessibility_relation_tests;
mod merged_declaration_signature_tests;
mod module_object_tests;
mod module_value_surface_tests;
mod named_arm_reduction_tests;
mod namespace_member_value_tests;
mod namespace_value_tests;
mod narrowing_ledger_tests;
mod never_callee_tests;
mod node_domain_tests;
mod object_literal_accessor_tests;
mod object_literal_key_tests;
mod object_spread_projection_eval_tests;
mod projected_terminal_surface_tests;
mod projection_stack_safety_tests;
mod raise_tests;
mod raised_shape_tests;
mod reactive_wrapper_tests;
mod reference_inference_tests;
mod relation_depth_tests;
mod relation_operand_tests;
pub(crate) mod relation_reverse_ownership_tests;
mod relation_variance_tests;
mod relation_work_tests;
mod return_equation_tests;
mod semantic_operand_binder_tests;
/// Unified dead-operand deep-work matrix (cross-operator table over
/// the forcing boundary) plus the one-authority convergence proof for
/// the builtin utility route.
mod semantic_operand_dead_work_tests;
mod semantic_operand_tests;
mod semantic_source_tests;
mod signature_discovery_tests;
mod signature_epoch_tests;
mod signature_predicate_inference_tests;
mod signature_predicate_tests;
mod signature_relation_tests;
mod string_mapping_template_tests;
mod symbol_identity_tests;
mod template_complexity_tests;
mod template_pattern_relation_tests;
mod tests;
mod this_receiver_tests;
mod truthiness_domain_tests;
mod tuple_length_and_apparent_member_tests;
mod type_syntax_depth_tests;
mod undecided_call_tests;
mod undecided_conditional_tests;
mod unique_symbol_key_tests;
mod unique_symbol_widening_tests;
mod unread_marker_relation_tests;
mod wide_union_relation_tests;
