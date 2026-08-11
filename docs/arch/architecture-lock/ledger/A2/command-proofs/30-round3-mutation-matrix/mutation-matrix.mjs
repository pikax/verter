// Round-3 comparator mutation matrix for block A2.
// For each mutation: prove the plant applies (find unique+present, replace absent),
// apply, run the u6_flow suite, record which tests FAIL, restore from baseline,
// verify byte-identical restore. A mutation that leaves the suite green is SURVIVED.
import { readFileSync, writeFileSync, appendFileSync } from "node:fs";
import { execSync } from "node:child_process";

const ROOT = "<REPO>-wt-a2";
const EXPECT = `${ROOT}/crates/verter_session/src/u6_flow_expect_tests.rs`;
const CORPUS = `${ROOT}/crates/verter_session/src/u6_flow_shape_corpus_tests.rs`;
const ROWS = `${ROOT}/crates/verter_session/src/u6_flow_shape_corpus_rows_tests.rs`;
const OUT = process.argv[2] || "mutation-results.json";
const LOG = OUT.replace(/\.json$/, ".log");

const baseline = {
  [EXPECT]: readFileSync(EXPECT, "utf8"),
  [CORPUS]: readFileSync(CORPUS, "utf8"),
  [ROWS]: readFileSync(ROWS, "utf8"),
};

const R = String.raw;

const mutations = [
  // ── lit_matches ──
  { id: "L1_lit_str_eq", file: EXPECT, edits: [[R`(Lit::Str(e), LiteralValue::String(g)) => *e == g.as_str(),`, R`(Lit::Str(e), LiteralValue::String(g)) => { let _ = (e, g); true },`]], expect: ["literal_expectation_rejects_a_different_value"] },
  { id: "L2_lit_num_eq", file: EXPECT, edits: [[R`(Lit::Num(e), LiteralValue::Number(g)) => e.to_bits() == g.to_bits(),`, R`(Lit::Num(e), LiteralValue::Number(g)) => { let _ = (e, g); true },`]], expect: ["literal_expectation_rejects_a_different_value"] },
  // ── node_matches ──
  { id: "P1_primitive_eq", file: EXPECT, edits: [[R`(ExpectedNode::Primitive(kind), SemanticNodeData::Primitive(got)) => kind == got,`, R`(ExpectedNode::Primitive(kind), SemanticNodeData::Primitive(got)) => { let _ = (kind, got); true },`]], expect: ["object_expectation_rejects_wrong_missing_extra_and_duplicate_members", "signature_params_and_arity_reject_wrong_shapes"] },
  { id: "U1_union_len", file: EXPECT, edits: [[R`    if measured.len() != expected.len() {
        return false;
    }`, R`    if false && measured.len() != expected.len() {
        return false;
    }`]], expect: ["union_set_equality_rejects_subset_and_superset"] },
  { id: "U2_union_assign", file: EXPECT, edits: [[R`            if !used[slot] && node_matches(dispatch, *candidate, &expected[index], depth) {`, R`            if !used[slot] && { let _ = (candidate, &expected[index]); true } {`]], expect: ["union_set_equality_rejects_subset_and_superset"] },
  { id: "I1_intersection_len", file: EXPECT, edits: [[R`        (ExpectedNode::Intersection(exp), SemanticNodeData::Intersection(members)) => {
            members.len() == exp.len()
                && members`, R`        (ExpectedNode::Intersection(exp), SemanticNodeData::Intersection(members)) => {
            (members.len() == exp.len() || true)
                && members`]], expect: ["intersection_expectation_rejects_a_wrong_arm"] },
  { id: "I2_intersection_order", file: EXPECT, edits: [[R`        (ExpectedNode::Intersection(exp), SemanticNodeData::Intersection(members)) => {
            members.len() == exp.len()
                && members
                    .iter()
                    .zip(exp.iter())
                    .all(|(member, arm)| node_matches(dispatch, *member, arm, depth + 1))
        }`, R`        (ExpectedNode::Intersection(exp), SemanticNodeData::Intersection(members)) => {
            members.len() == exp.len()
                && exp.iter().all(|arm| {
                    members.iter().any(|member| node_matches(dispatch, *member, arm, depth + 1))
                })
        }`]], expect: ["intersection_expectation_rejects_a_wrong_arm"] },
  { id: "S1_signature_arity", file: EXPECT, edits: [[R`            got_params.len() == params.len()
                && got_params
                    .iter()
                    .zip(params.iter())
                    .all(|(param, exp)| node_matches(dispatch, param.ty, exp, depth + 1))`, R`            (got_params.len() == params.len() || true)
                && got_params
                    .iter()
                    .zip(params.iter())
                    .all(|(param, exp)| node_matches(dispatch, param.ty, exp, depth + 1))`]], expect: ["signature_params_and_arity_reject_wrong_shapes"] },
  { id: "S2_signature_params", file: EXPECT, edits: [[R`.all(|(param, exp)| node_matches(dispatch, param.ty, exp, depth + 1))`, R`.all(|(param, exp)| { let _ = (param, exp); true })`]], expect: ["signature_params_and_arity_reject_wrong_shapes"] },
  { id: "S3_signature_ret", file: EXPECT, edits: [[R`                && node_matches(dispatch, *return_type, ret, depth + 1)
        }
        (ExpectedNode::Object(exp), SemanticNodeData::Object(surface)) => {`, R`                && { let _ = (return_type, ret); true }
        }
        (ExpectedNode::Object(exp), SemanticNodeData::Object(surface)) => {`]], expect: ["signature_expectation_rejects_a_different_return"] },
  { id: "O1_object_len", file: EXPECT, edits: [[R`            let members = surface.positive_members();
            if members.len() != exp.len() {
                return false;
            }`, R`            let members = surface.positive_members();
            if false && members.len() != exp.len() {
                return false;
            }`]], expect: ["object_expectation_rejects_wrong_missing_extra_and_duplicate_members"] },
  { id: "O2_object_key", file: EXPECT, edits: [[R`                        || member.key.as_string() != Some(*name)`, R`                        || false && member.key.as_string() != Some(*name)`]], expect: ["object_expectation_rejects_wrong_missing_extra_and_duplicate_members"] },
  { id: "O3_object_value", file: EXPECT, edits: [[R`                        || !node_matches(dispatch, member.value, value, depth + 1)`, R`                        || false && !node_matches(dispatch, member.value, value, depth + 1)`]], expect: ["object_expectation_rejects_wrong_missing_extra_and_duplicate_members"] },
  { id: "O4_object_injective", file: EXPECT, edits: [[R`                    if used[slot]
                        || member.key.as_string() != Some(*name)`, R`                    if (false && used[slot])
                        || member.key.as_string() != Some(*name)`]], expect: ["object_expectation_rejects_wrong_missing_extra_and_duplicate_members"] },
  { id: "T1_type_param_name", file: EXPECT, edits: [[R`        (ExpectedNode::TypeParam { name }, SemanticNodeData::TypeParam { display_name, .. }) => {
            &**display_name == *name
        }`, R`        (ExpectedNode::TypeParam { name }, SemanticNodeData::TypeParam { display_name, .. }) => {
            let _ = (name, display_name);
            true
        }`]], expect: ["type_param_expectation_rejects_a_wrong_name"] },
  { id: "R1_decl_ref_name", file: EXPECT, edits: [[R`        (ExpectedNode::DeclRef { name }, SemanticNodeData::DeclRef { identity }) => {
            &*identity.decl_name == *name
        }`, R`        (ExpectedNode::DeclRef { name }, SemanticNodeData::DeclRef { identity }) => {
            let _ = (name, identity);
            true
        }`]], expect: ["intersection_expectation_rejects_a_wrong_arm"] },
  { id: "BR1_bare_ref_name", file: EXPECT, edits: [[R`            data.bare_ref_head().is_some_and(|(got, _)| &**got == *name)`, R`            data.bare_ref_head().is_some_and(|(got, _)| { let _ = (got, name); true })`]], expect: ["bare_ref_expectation_rejects_a_wrong_name"] },
  { id: "OP1_opaque_broaden", file: EXPECT, edits: [[R`        (
            ExpectedNode::OpaqueUnmodeledPosition,
            SemanticNodeData::Opaque(QueryError::UnmodeledPosition),
        ) => true,`, R`        (
            ExpectedNode::OpaqueUnmodeledPosition,
            SemanticNodeData::Opaque(_),
        ) => true,`]], expect: ["opaque_unmodeled_position_marker_is_discriminating"] },
  // ── check_boundary first-call + pin clauses ──
  { id: "B1_first_from_cache", file: EXPECT, edits: [[R`    if measured.first_from_cache {
        failures.push(
            "call 1 reported from_cache=true — the first call on a fresh host must be COLD"
                .to_owned(),
        );
    }`, R`    if false && measured.first_from_cache {
        failures.push(
            "call 1 reported from_cache=true — the first call on a fresh host must be COLD"
                .to_owned(),
        );
    }`]], expect: ["boundary_first_call_cold_clauses_fail_against_a_warm_first_trace"] },
  { id: "B2_first_cold_computes", file: EXPECT, edits: [[R`    if measured.first_cold_computes == 0 {
        failures.push(
            "call 1 reported cold_computes == 0 — a cold evaluation must count at least one"
                .to_owned(),
        );
    }`, R`    if false && measured.first_cold_computes == 0 {
        failures.push(
            "call 1 reported cold_computes == 0 — a cold evaluation must count at least one"
                .to_owned(),
        );
    }`]], expect: ["boundary_first_call_cold_clauses_fail_against_a_warm_first_trace"] },
  { id: "B3_degradation_pin", file: EXPECT, edits: [[R`    match measured.degradation {
        Some(got) if got == degradation => {}`, R`    match measured.degradation {
        Some(got) if { let _ = got; true } => {}`]], expect: ["cache_replay_assertion_fails_in_both_directions"] },
  { id: "B4_json_pin", file: EXPECT, edits: [[R`    match &measured.json {
        Some(got) if got == json => {}`, R`    match &measured.json {
        Some(got) if { let _ = got; true } => {}`]], expect: ["cache_replay_assertion_fails_in_both_directions"] },
  { id: "B12_no_value_carrier", file: EXPECT, edits: [[R`    if let Some(error) = &measured.error {
        failures.push(format!(
            "the public boundary returned NO VALUE ({error}) where the pin expects a result"
        ));
        return failures;
    }`, R`    if let (false, Some(error)) = (true, &measured.error) {
        failures.push(format!(
            "the public boundary returned NO VALUE ({error}) where the pin expects a result"
        ));
        return failures;
    }`]], expect: ["boundary_replay_drift_and_no_value_clauses_fail"] },
  // ── replay_clauses ──
  { id: "RC1_replay_class_drift", file: EXPECT, edits: [[R`    if let Some(err) = &measured.second_error {
        failures.push(format!(
            "call 2 returned NO VALUE ({err}) where call 1 produced a result — the replay \
             changed the result CLASS"
        ));
        return failures;
    }`, R`    if let (false, Some(err)) = (true, &measured.second_error) {
        failures.push(format!(
            "call 2 returned NO VALUE ({err}) where call 1 produced a result — the replay \
             changed the result CLASS"
        ));
        return failures;
    }`]], expect: ["boundary_replay_class_and_degradation_drift_fail"] },
  { id: "RC2_replay_degr_drift", file: EXPECT, edits: [[R`    if measured.second_degradation != measured.degradation {`, R`    if false && measured.second_degradation != measured.degradation {`]], expect: ["boundary_replay_class_and_degradation_drift_fail"] },
  { id: "RC3_warm_from_cache", file: EXPECT, edits: [[R`        if !measured.second_from_cache {
            failures.push(
                "call 2 reported from_cache=false — a clean result must REPLAY WARM".to_owned(),
            );
        }`, R`        if false && !measured.second_from_cache {
            failures.push(
                "call 2 reported from_cache=false — a clean result must REPLAY WARM".to_owned(),
            );
        }`]], expect: ["boundary_warm_pair_clauses_fail_individually"] },
  { id: "RC4_warm_zero_cold", file: EXPECT, edits: [[R`        if measured.second_cold_computes != 0 {
            failures.push(format!(
                "call 2 ran {} cold evaluations — a warm family replay runs ZERO",
                measured.second_cold_computes
            ));
        }`, R`        if false && measured.second_cold_computes != 0 {
            failures.push(format!(
                "call 2 ran {} cold evaluations — a warm family replay runs ZERO",
                measured.second_cold_computes
            ));
        }`]], expect: ["boundary_warm_pair_clauses_fail_individually", "cache_replay_assertion_fails_in_both_directions"] },
  { id: "RC5_cold_no_poison", file: EXPECT, edits: [[R`        if measured.second_from_cache {
            failures.push(
                "call 2 reported from_cache=true — this result is ReturnOnly and must NOT be \
                 admitted warm; a warm replay here is the no-poison violation the pin exists \
                 to catch"
                    .to_owned(),
            );
        }`, R`        if false && measured.second_from_cache {
            failures.push(
                "call 2 reported from_cache=true — this result is ReturnOnly and must NOT be \
                 admitted warm; a warm replay here is the no-poison violation the pin exists \
                 to catch"
                    .to_owned(),
            );
        }`]], expect: ["cache_replay_assertion_fails_in_both_directions", "cell_value_clauses_each_reject_a_wrong_pin"] },
  { id: "RC6_cold_recompute", file: EXPECT, edits: [[R`        if measured.second_cold_computes == 0 {
            failures.push(
                "call 2 ran ZERO cold evaluations — a result that must not be admitted warm \
                 has to COLD-COMPUTE again on the second call; from_cache=false with zero \
                 cold work is a replay that did nothing"
                    .to_owned(),
            );
        }`, R`        if false && measured.second_cold_computes == 0 {
            failures.push(
                "call 2 ran ZERO cold evaluations — a result that must not be admitted warm \
                 has to COLD-COMPUTE again on the second call; from_cache=false with zero \
                 cold work is a replay that did nothing"
                    .to_owned(),
            );
        }`]], expect: ["boundary_cold_replay_requires_actual_cold_work"] },
  { id: "RC7_json_drift", file: EXPECT, edits: [[R`    match (&measured.json, &measured.second_json) {
        (Some(first), Some(second)) if first == second => {}`, R`    match (&measured.json, &measured.second_json) {
        (Some(first), Some(second)) if { let _ = (first, second); true } => {}`]], expect: ["boundary_replay_drift_and_no_value_clauses_fail"] },
  // ── check_boundary_refusal ──
  { id: "F1_refusal_value_first", file: EXPECT, edits: [[R`    if measured.error.is_none() {
        failures.push(format!(
            "call 1 produced a VALUE (projection {:?}, degradation {:?}) where the pin \
             expects a typed no-value refusal",`, R`    if false && measured.error.is_none() {
        failures.push(format!(
            "call 1 produced a VALUE (projection {:?}, degradation {:?}) where the pin \
             expects a typed no-value refusal",`]], expect: ["refusal_comparator_clauses_fail_individually"] },
  { id: "F2_refusal_first_warm", file: EXPECT, edits: [[R`    if measured.first_from_cache {
        failures.push(
            "call 1 reported from_cache=true — a refusal on a fresh host must be COLD".to_owned(),
        );
    }`, R`    if false && measured.first_from_cache {
        failures.push(
            "call 1 reported from_cache=true — a refusal on a fresh host must be COLD".to_owned(),
        );
    }`]], expect: ["refusal_comparator_clauses_fail_individually"] },
  { id: "F3_refusal_first_cold", file: EXPECT, edits: [[R`    if measured.first_cold_computes == 0 {
        failures.push(
            "call 1 reported cold_computes == 0 — a genuine refusal is computed, not conjured"
                .to_owned(),
        );
    }`, R`    if false && measured.first_cold_computes == 0 {
        failures.push(
            "call 1 reported cold_computes == 0 — a genuine refusal is computed, not conjured"
                .to_owned(),
        );
    }`]], expect: ["refusal_comparator_clauses_fail_individually"] },
  { id: "F4_refusal_second_class", file: EXPECT, edits: [[R`    if measured.second_error.is_none() {`, R`    if false && measured.second_error.is_none() {`]], expect: ["refusal_comparator_clauses_fail_individually"] },
  { id: "F5_refusal_non_admission", file: EXPECT, edits: [[R`    if measured.second_from_cache {
        failures.push(
            "call 2 reported from_cache=true — a refusal must NEVER be admitted warm; a \
             cached refusal is the typed-non-admission violation this pin exists to catch"
                .to_owned(),
        );
    }`, R`    if false && measured.second_from_cache {
        failures.push(
            "call 2 reported from_cache=true — a refusal must NEVER be admitted warm; a \
             cached refusal is the typed-non-admission violation this pin exists to catch"
                .to_owned(),
        );
    }`]], expect: ["refusal_comparator_clauses_fail_individually", "cell_no_value_arm_rejects_a_cached_refusal"] },
  { id: "F6_refusal_recompute", file: EXPECT, edits: [[R`    if measured.second_cold_computes == 0 {
        failures.push(
            "call 2 ran ZERO cold evaluations — a non-admitted refusal must genuinely \
             RECOMPUTE on every demand"
                .to_owned(),
        );
    }`, R`    if false && measured.second_cold_computes == 0 {
        failures.push(
            "call 2 ran ZERO cold evaluations — a non-admitted refusal must genuinely \
             RECOMPUTE on every demand"
                .to_owned(),
        );
    }`]], expect: ["refusal_comparator_clauses_fail_individually"] },
  // ── check_cell_outcome_measured ──
  { id: "C1_cell_novalue_class", file: EXPECT, edits: [[R`            CellOutcome::NoValue => {
                if !no_value {`, R`            CellOutcome::NoValue => {
                if false && !no_value {`]], expect: ["cell_outcome_class_clauses_reject_the_opposite_class"] },
  { id: "C2_cell_value_class", file: EXPECT, edits: [[R`                if no_value {
                    describe(
                        failures,
                        format!("EXPECTED a value rendering {want}; MEASURED a no-value refusal"),
                    );
                    return;
                }`, R`                if false && no_value {
                    describe(
                        failures,
                        format!("EXPECTED a value rendering {want}; MEASURED a no-value refusal"),
                    );
                    return;
                }`]], expect: ["cell_outcome_class_clauses_reject_the_opposite_class"] },
  { id: "C3_cell_rendered", file: EXPECT, edits: [[R`                if rendered != Some(*want) {`, R`                if false && rendered != Some(*want) {`]], expect: ["cell_value_clauses_each_reject_a_wrong_pin"] },
  { id: "C4_cell_degradation", file: EXPECT, edits: [[R`                if measured.boundary.degradation != Some(*want_degr) {`, R`                if false && measured.boundary.degradation != Some(*want_degr) {`]], expect: ["cell_value_clauses_each_reject_a_wrong_pin"] },
  { id: "C5_cell_replay_delegation", file: EXPECT, edits: [[R`                for failure in replay_clauses(*warm_replay, &measured.boundary) {`, R`                for failure in replay_clauses(*warm_replay, &measured.boundary).into_iter().take(0) {`]], expect: ["cell_value_clauses_each_reject_a_wrong_pin"] },
  { id: "C6_cell_refusal_delegation", file: EXPECT, edits: [[R`                for failure in check_boundary_refusal(&measured.boundary) {`, R`                for failure in check_boundary_refusal(&measured.boundary).into_iter().take(0) {`]], expect: ["cell_no_value_arm_rejects_a_cached_refusal"] },
  // ── checker_syntax::matches_node ──
  { id: "K1_checker_string_eq", file: EXPECT, edits: [[R`            (CheckerType::StringLit(e), SemanticNodeData::Literal(LiteralValue::String(g))) => {
                e == g.as_str()
            }`, R`            (CheckerType::StringLit(e), SemanticNodeData::Literal(LiteralValue::String(g))) => {
                let _ = (e, g);
                true
            }`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K2_checker_union_assign", file: EXPECT, edits: [[R`                        if !used[slot]
                            && matches_node(dispatch, *candidate, &expected[index], depth)`, R`                        if !used[slot]
                            && { let _ = (candidate, &expected[index]); true }`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K3_checker_intersection_order", file: EXPECT, edits: [[R`            (CheckerType::Intersection(exp), SemanticNodeData::Intersection(members)) => {
                members.len() == exp.len()
                    && members
                        .iter()
                        .zip(exp.iter())
                        .all(|(member, arm)| matches_node(dispatch, *member, arm, depth + 1))
            }`, R`            (CheckerType::Intersection(exp), SemanticNodeData::Intersection(members)) => {
                members.len() == exp.len()
                    && exp.iter().all(|arm| {
                        members
                            .iter()
                            .any(|member| matches_node(dispatch, *member, arm, depth + 1))
                    })
            }`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K4_checker_ref_name", file: EXPECT, edits: [[R`            (CheckerType::Ref(name), SemanticNodeData::DeclRef { identity }) => {
                &*identity.decl_name == name.as_str()
            }`, R`            (CheckerType::Ref(name), SemanticNodeData::DeclRef { identity }) => {
                let _ = (name, identity);
                true
            }`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K5_checker_primitive_eq", file: EXPECT, edits: [[R`            (CheckerType::Primitive(e), SemanticNodeData::Primitive(g)) => e == g,`, R`            (CheckerType::Primitive(e), SemanticNodeData::Primitive(g)) => { let _ = (e, g); true },`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K6_checker_object_key", file: EXPECT, edits: [[R`                            || member.key.as_string() != Some(name.as_str())`, R`                            || false && member.key.as_string() != Some(name.as_str())`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K7_checker_fn_arity", file: EXPECT, edits: [[R`                got_params.len() == params.len()
                    && got_params
                        .iter()
                        .zip(params.iter())
                        .all(|(param, exp)| matches_node(dispatch, param.ty, exp, depth + 1))`, R`                (got_params.len() == params.len() || true)
                    && got_params
                        .iter()
                        .zip(params.iter())
                        .all(|(param, exp)| matches_node(dispatch, param.ty, exp, depth + 1))`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K8_checker_fn_params", file: EXPECT, edits: [[R`.all(|(param, exp)| matches_node(dispatch, param.ty, exp, depth + 1))`, R`.all(|(param, exp)| { let _ = (param, exp); true })`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K9_checker_fn_ret", file: EXPECT, edits: [[R`                    && matches_node(dispatch, *return_type, ret, depth + 1)`, R`                    && { let _ = (return_type, ret); true }`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  { id: "K10_checker_number_eq", file: EXPECT, edits: [[R`            (CheckerType::NumberLit(e), SemanticNodeData::Literal(LiteralValue::Number(g))) => {
                e.to_bits() == g.to_bits()
            }`, R`            (CheckerType::NumberLit(e), SemanticNodeData::Literal(LiteralValue::Number(g))) => {
                let _ = (e, g);
                true
            }`]], expect: ["checker_column_mutations_are_rejected_semantically"] },
  // ── corpus-level hatch demonstrations ──
  { id: "X85MOVE_bogus_incomparable", file: CORPUS, edits: [
      [R`        const RENDER_COMPARABLE: &[&str] = &[
            "X85_nested_closure_write_updates_captured_binding",
            "X87_read_only_let_capture_keeps_reaching_literal",
        ];`, R`        const RENDER_COMPARABLE: &[&str] = &["X87_read_only_let_capture_keeps_reaching_literal"];`],
      [R`        const RENDER_INCOMPARABLE: &[(&str, &str)] = &[
            (
                "X88_nested_label_inherits_enclosing_suffix_return",`, R`        const RENDER_INCOMPARABLE: &[(&str, &str)] = &[
            (
                "X85_nested_closure_write_updates_captured_binding",
                "bogus reason — reclassified to dodge the byte comparison",
            ),
            (
                "X88_nested_label_inherits_enclosing_suffix_return",`],
    ], expect: ["checker_column_cross_validates_against_live_rendering"] },
  { id: "N26CHK_wrong_intersection_arm", file: ROWS, edits: [[R`checker: "{ v: string | (A & B); }"`, R`checker: "{ v: string | (A & C); }"`]], expect: ["deep_pinned_rows_semantic_equality_follows_their_verdict"] },
];

function log(line) {
  appendFileSync(LOG, line + "\n");
  console.log(line);
}

const results = [];
let aborted = false;
for (const m of mutations) {
  const original = baseline[m.file];
  let mutated = original;
  let plantOk = true;
  const plantNotes = [];
  for (const [find, replace] of m.edits) {
    const occurrences = mutated.split(find).length - 1;
    const preexisting = original.split(replace).length - 1;
    if (occurrences !== 1) { plantOk = false; plantNotes.push(`find occurs ${occurrences}x (need 1)`); }
    if (preexisting !== 0) { plantOk = false; plantNotes.push(`replacement already present ${preexisting}x`); }
    if (plantOk) mutated = mutated.replace(find, replace);
  }
  if (plantOk) {
    for (const [, replace] of m.edits) {
      const applied = mutated.split(replace).length - 1;
      if (applied !== 1) { plantOk = false; plantNotes.push(`post-apply replacement occurs ${applied}x (need 1)`); }
    }
  }
  if (!plantOk) {
    log(`PLANT-FAILED ${m.id}: ${plantNotes.join("; ")}`);
    results.push({ id: m.id, verdict: "PLANT-FAILED", notes: plantNotes });
    aborted = true;
    break; // a failed plant means the matrix is unsound — stop and fix
  }
  writeFileSync(m.file, mutated);
  let out = "";
  try {
    out = execSync(
      'cargo nextest run -p verter_session -E "test(u6_flow)" --no-fail-fast --status-level fail --failure-output never 2>&1',
      { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }
    );
  } catch (err) {
    out = (err.stdout || "") + (err.stderr || "");
  }
  writeFileSync(m.file, original);
  if (readFileSync(m.file, "utf8") !== original) {
    log(`RESTORE-FAILED ${m.id}`);
    results.push({ id: m.id, verdict: "RESTORE-FAILED" });
    aborted = true;
    break;
  }
  const failing = [...out.matchAll(/FAIL \[[^\]]*\]\s+(?:\([^)]*\)\s+)?verter_session\s+(\S+)/g)].map((x) => x[1]);
  const compileError = /error\[|error: could not compile/.test(out);
  const summary = (out.match(/Summary \[[^\]]*\].*$/m) || [""])[0].trim();
  // The run must PROVE it ran: a Summary line with a non-zero executed
  // count. A broken runner invocation must never read as SURVIVED.
  const ranCount = Number((summary.match(/(\d+) tests? run/) || [0, 0])[1]);
  const caughtBy = m.expect.filter((e) => failing.some((f) => f.includes(e)));
  const missed = m.expect.filter((e) => !failing.some((f) => f.includes(e)));
  let verdict;
  if (compileError) verdict = "COMPILE-ERROR";
  else if (!summary || ranCount < 40) verdict = "RUN-ERROR";
  else if (failing.length === 0) verdict = "SURVIVED";
  else if (missed.length === 0) verdict = "CAUGHT-BY-NAMED-CONTROL";
  else verdict = "CAUGHT-BY-OTHER";
  log(`${verdict} ${m.id} | expected [${m.expect.join(", ")}] | failing [${[...new Set(failing)].join(", ")}] | ${summary}`);
  results.push({ id: m.id, verdict, expected: m.expect, failing: [...new Set(failing)], missed, summary });
  if (verdict === "COMPILE-ERROR" || verdict === "RUN-ERROR") { aborted = true; break; }
}

writeFileSync(OUT, JSON.stringify({ aborted, results }, null, 2));
log(`DONE aborted=${aborted} total=${results.length}`);
