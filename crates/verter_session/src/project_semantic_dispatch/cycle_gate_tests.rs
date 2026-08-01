//! Discriminating tests for the sealed
//! `SemanticQueryKey::ClassifyMaterializationCycleGate` family — the
//! materialization cycle gate.
//!
//! Rows (each with its mutation recipe in the assertion text):
//!
//! - Golden verdict table: every parity row must surface
//!   `Decided(Stop | Continue)` — a verdict flip or a demoted
//!   `LegacyFallback` fails the row.
//! - Hop-cap polarity: plain path at the 64-dequeue cap →
//!   `LegacyFallback { Continue, [HopLimit] }`, NOT partial; complex
//!   path → `LegacyFallback { Stop, [HopLimit] }`, NOT partial.
//! - Missing-body parity: an `Opaque(Miss)` body is an ordinary empty
//!   body → `Decided(Continue)` (exact BFS parity).
//! - Admission: `Decided` warm-admits (second read does not recompute);
//!   `LegacyFallback` never admits (every read recomputes).
//! - Key axes: P / R / T / L / J are all family identity — a key
//!   differing in any single axis computes independently.
//! - Invalidation: bare generation bump and visited-helper content edit
//!   both force a recompute.
//! - Cache proofs: cold AND warm reads return the family dep signature
//!   (consumer fence parity); every visited non-builtin declaration is
//!   an observed self-root; per-canonical reverse-index invalidation
//!   evicts; an untracked visited self-root rejects the warm entry; a
//!   content-only revision admits a new candidate under the same key.
//! - Cross-domain isolation: invoking the classifier around `Relate`
//!   leaves relation frames untouched.

use std::sync::Arc as StdArc;

use crate::meta::MetaProject;
use crate::project_semantic_dispatch::cycle_gate::{
    cycle_gate_compute_counter_for_test, reset_cycle_gate_compute_counter_for_test,
};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    CacheRead, DeclIdentity, MaterializationCycleGateFallbackReason,
    MaterializationCycleGateOutcome, MaterializationCycleGateVerdict, SemanticQueryKey,
};
use crate::types::HostConfig;
use crate::VerterHost;

fn make_project() -> StdArc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn decl_identity(host: &VerterHost, canonical: &str, name: &str) -> DeclIdentity {
    let whole_hash = host
        .shallow_file_state(canonical)
        .map(|s| s.whole_hash)
        .unwrap_or([0u8; 16]);
    DeclIdentity {
        canonical_id: StdArc::from(canonical),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash,
        decl_name: StdArc::from(name),
    }
}

fn gate_read(
    host: &VerterHost,
    canonical: &str,
    name: &str,
) -> CacheRead<MaterializationCycleGateOutcome> {
    let dispatch = ProjectSemanticDispatch::new(host);
    let id = decl_identity(host, canonical, name);
    dispatch.classify_materialization_cycle_gate(&id)
}

/// Assert the golden-table shape: a complete `Decided(expected)` with
/// clean admission rails. Mutation recipe: a producer that demotes any
/// complete walk to `LegacyFallback`, flips a verdict, or suppresses a
/// `Decided` outcome fails here.
fn assert_decided(
    read: &CacheRead<MaterializationCycleGateOutcome>,
    expected: MaterializationCycleGateVerdict,
    row: &str,
) {
    assert_eq!(
        read.value,
        MaterializationCycleGateOutcome::Decided(expected),
        "{row}: the complete walk must surface Decided({expected:?})"
    );
    assert!(
        !read.cache_suppress,
        "{row}: a Decided outcome must not suppress family admission"
    );
    assert!(
        !read.result_is_partial,
        "{row}: a Decided outcome must not be partial"
    );
}

#[test]
fn cycle_gate_decided_continue_on_productive_tree_recursion() {
    let project = make_project();
    project
        .upsert_base("/cg_tree.ts", "export type Tree = { children: Tree[] }")
        .unwrap();
    let read = gate_read(project.host(), "/cg_tree.ts", "Tree");
    assert_decided(&read, MaterializationCycleGateVerdict::Continue, "Tree");
}

#[test]
fn cycle_gate_decided_stop_on_jsonvalue_complex_union() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_json.ts",
            "export type JSONValue = string | { [k: string]: JSONValue } | JSONValue[]",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_json.ts", "JSONValue");
    assert_decided(&read, MaterializationCycleGateVerdict::Stop, "JSONValue");
}

#[test]
fn cycle_gate_decided_stop_on_generic_mutual_recursion() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_mutual.ts",
            "export type GetItemKeys<T> = DotPathKeys<T>\n\
             export type DotPathKeys<T> = T extends object ? GetItemKeys<T> : never\n",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_mutual.ts", "GetItemKeys");
    assert_decided(
        &read,
        MaterializationCycleGateVerdict::Stop,
        "generic mutual",
    );
}

#[test]
fn cycle_gate_decided_stop_on_keyof_intermediary() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_keyof.ts",
            "export type A = { kids: B }\nexport type B = keyof A\n",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_keyof.ts", "A");
    assert_decided(
        &read,
        MaterializationCycleGateVerdict::Stop,
        "keyof intermediary",
    );
}

#[test]
fn cycle_gate_decided_stop_on_three_decl_cycle_containing_root() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_tri.ts",
            "export type TriA = keyof TriB\nexport type TriB = keyof TriC\nexport type TriC = keyof TriA\n",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_tri.ts", "TriA");
    assert_decided(
        &read,
        MaterializationCycleGateVerdict::Stop,
        "three-decl cycle",
    );
}

#[test]
fn cycle_gate_decided_stop_on_nuxt_dotpathkeys_shape() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_dpk.ts",
            "export type DotPathKeys<T> = T extends object\n  \
             ? { [K in keyof T & string]: K | `${K}.${DotPathKeys<NonNullable<T[K]>>}` }[keyof T & string]\n  \
             : never\n",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_dpk.ts", "DotPathKeys");
    assert_decided(&read, MaterializationCycleGateVerdict::Stop, "DotPathKeys");
}

#[test]
fn cycle_gate_decided_continue_when_root_outside_intermediate_scc() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_scc.ts",
            "export type SccRoot = { a: SccA }\nexport type SccA = keyof SccB\nexport type SccB = keyof SccA\n",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_scc.ts", "SccRoot");
    assert_decided(
        &read,
        MaterializationCycleGateVerdict::Continue,
        "root outside intermediate SCC",
    );
}

#[test]
fn cycle_gate_decided_continue_on_plain_first_shared_node() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_first_plain.ts",
            "export type FirstS = { back: FirstRoot }\n\
             export type FirstP = { s: FirstS }\n\
             export type FirstC = keyof FirstS\n\
             export type FirstRoot = { c: FirstC, p: FirstP }\n",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_first_plain.ts", "FirstRoot");
    assert_decided(
        &read,
        MaterializationCycleGateVerdict::Continue,
        "plain-first shared node",
    );
}

#[test]
fn cycle_gate_decided_stop_on_complex_first_shared_node() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_first_complex.ts",
            "export type First2S = { back: First2Root }\n\
             export type First2P = { s: First2S }\n\
             export type First2C = keyof First2S\n\
             export type First2Root = { p: First2P, c: First2C }\n",
        )
        .unwrap();
    let read = gate_read(project.host(), "/cg_first_complex.ts", "First2Root");
    assert_decided(
        &read,
        MaterializationCycleGateVerdict::Stop,
        "complex-first shared node",
    );
}

/// Hop-cap polarity, plain path: the 64-dequeue bound exhausts with an
/// all-plain carried signal, so the verdict is Continue and the ONLY
/// reason is `HopLimit`. `HopLimit` is ReturnOnly (cache-suppressed)
/// but NEVER partial. Mutation recipe: marking the hop-limit fallback
/// partial, or dropping the carried-signal polarity, fails here.
#[test]
fn cycle_gate_hop_cap_plain_is_continue_return_only_not_partial() {
    let mut fixture = String::new();
    for i in 0..200 {
        fixture.push_str(&format!(
            "export type A_{i} = {{ x: A_{} }}\n",
            (i + 1) % 200
        ));
    }
    let project = make_project();
    project.upsert_base("/cg_plain_chain.ts", &fixture).unwrap();
    let read = gate_read(project.host(), "/cg_plain_chain.ts", "A_0");
    let reasons = read
        .value
        .fallback_reasons()
        .expect("hop-cap walk must demote to LegacyFallback");
    assert_eq!(
        read.value.verdict(),
        MaterializationCycleGateVerdict::Continue,
        "plain path at the hop cap returns the carried (plain) signal"
    );
    assert!(
        reasons.contains(MaterializationCycleGateFallbackReason::HopLimit),
        "the hop-limit exhaustion must be recorded as HopLimit"
    );
    assert!(
        read.cache_suppress,
        "a LegacyFallback always suppresses family admission"
    );
    assert!(
        !read.result_is_partial,
        "HopLimit is ReturnOnly, NOT partial — a bounded complete observation"
    );
}

/// Hop-cap polarity, complex path: every hop is `keyof` (complex), so
/// the carried signal at the cap is complex → Stop, still NOT partial.
#[test]
fn cycle_gate_hop_cap_complex_is_stop_return_only_not_partial() {
    let mut fixture = String::new();
    for i in 0..100 {
        fixture.push_str(&format!("export type K_{i} = keyof K_{}\n", i + 1));
    }
    fixture.push_str("export type K_100 = { x: string }\n");
    let project = make_project();
    project.upsert_base("/cg_keyof_chain.ts", &fixture).unwrap();
    let read = gate_read(project.host(), "/cg_keyof_chain.ts", "K_0");
    let reasons = read
        .value
        .fallback_reasons()
        .expect("hop-cap walk must demote to LegacyFallback");
    assert_eq!(
        read.value.verdict(),
        MaterializationCycleGateVerdict::Stop,
        "complex path at the hop cap returns the carried (complex) signal"
    );
    assert!(
        reasons.contains(MaterializationCycleGateFallbackReason::HopLimit),
        "the hop-limit exhaustion must be recorded as HopLimit"
    );
    assert!(read.cache_suppress, "a LegacyFallback always suppresses");
    assert!(
        !read.result_is_partial,
        "HopLimit is ReturnOnly, NOT partial"
    );
}

/// Missing-body parity: a root whose per-hop `Instantiate` read
/// surfaces a missing body (`Value(Opaque(Miss))`, the
/// missing-prepared-decl arm) is an ordinary EMPTY body — the walk
/// continues past it and decides: verdict Continue, fully `Decided`.
/// Type parameters, builtins, and external decls traverse the same
/// empty-body shape; demoting them would false-suppress consumers.
/// Mutation recipe: recording a fallback reason for the
/// `Opaque(Miss)`-body shape fails the Decided assertion; hard-stopping
/// the walk (verdict Stop) fails the verdict assertion.
#[test]
fn cycle_gate_missing_body_is_decided_continue() {
    let project = make_project();
    project
        .upsert_base("/cg_present.ts", "export type Present = { x: number }\n")
        .unwrap();
    let read = gate_read(project.host(), "/cg_present.ts", "NotHere");
    assert_eq!(
        read.value,
        MaterializationCycleGateOutcome::Decided(MaterializationCycleGateVerdict::Continue),
        "a missing body is an ordinary empty body — the walk continues and decides Continue"
    );
    // The missing-decl hop read is itself non-cacheable, so the generic
    // finalizer refuses the OUTER entry (`cache_suppress`, NOT partial):
    // the verdict flows, admission follows the generic rails.
    assert!(
        read.cache_suppress && !read.result_is_partial,
        "the non-cacheable missing-decl hop must refuse outer admission \
         without going partial (cache_suppress={}, result_is_partial={})",
        read.cache_suppress,
        read.result_is_partial
    );
}

/// `Decided` admits: the second read of the same root is served warm —
/// the producer does not re-run. Mutation recipe: publishing every
/// build (no warm serve) or refusing `Decided` admission fails the
/// zero-recompute assertion.
#[test]
fn cycle_gate_decided_outcome_warm_admits_without_recompute() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_warm.ts",
            "export type JSONValue = string | { [k: string]: JSONValue } | JSONValue[]",
        )
        .unwrap();
    let host = project.host();
    let first = gate_read(host, "/cg_warm.ts", "JSONValue");
    assert_decided(&first, MaterializationCycleGateVerdict::Stop, "warm prime");

    reset_cycle_gate_compute_counter_for_test();
    let second = gate_read(host, "/cg_warm.ts", "JSONValue");
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        0,
        "a warm family read must not re-run the producer"
    );
    assert_eq!(
        second.value, first.value,
        "warm read serves the same outcome"
    );
}

/// `LegacyFallback` never admits: every read recomputes. Mutation
/// recipe: admitting a hop-cap fallback into the family memo fails the
/// recompute assertion (the second read would warm-serve).
#[test]
fn cycle_gate_legacy_fallback_never_admits() {
    let mut fixture = String::new();
    for i in 0..100 {
        fixture.push_str(&format!("export type K_{i} = keyof K_{}\n", i + 1));
    }
    fixture.push_str("export type K_100 = { x: string }\n");
    let project = make_project();
    project.upsert_base("/cg_noadmit.ts", &fixture).unwrap();
    let host = project.host();
    let first = gate_read(host, "/cg_noadmit.ts", "K_0");
    assert!(
        !first.value.is_decided(),
        "fixture: the hop-cap read is a LegacyFallback"
    );

    reset_cycle_gate_compute_counter_for_test();
    let _ = gate_read(host, "/cg_noadmit.ts", "K_0");
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        1,
        "a LegacyFallback must never warm-serve — the producer re-runs"
    );
}

/// Key axes: P / R / T / L / J are ALL family identity. Two keys
/// differing in any single axis are distinct and compute independently;
/// an identical re-key warm-hits. Mutation recipe: dropping any axis
/// from the key fails the distinctness half; failing the live-env
/// derivation in `type_slot_for` (a zeroed / mismatched env tail)
/// fails the warm-isolation half.
#[test]
fn classify_materialization_cycle_gate_keys_do_not_warm_hit_across_env_axes() {
    let z = [0u8; 16];
    let canonical: StdArc<str> = StdArc::from("/cg_axes.ts");
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let name: StdArc<str> = StdArc::from("Probe");
    let root =
        |project: u32, t: crate::semantic_query::HashValue, l: crate::semantic_query::HashValue| {
            crate::semantic_query::ResolvedDeclSlotIdentity::type_slot(
                StdArc::clone(&canonical),
                owner,
                StdArc::clone(&name),
                project,
                t,
                l,
            )
        };
    let key = |root: crate::semantic_query::ResolvedDeclSlotIdentity,
               p: crate::semantic_query::HashValue,
               r: crate::semantic_query::HashValue| {
        crate::semantic_query::MaterializationCycleGateKey {
            root,
            parse_env_hash: p,
            resolve_env_hash: r,
        }
    };
    let base = key(root(1, z, z), z, z);
    for (axis, variant) in [
        ("P", key(root(1, z, z), [0xAA; 16], z)),
        ("R", key(root(1, z, z), z, [0xBB; 16])),
        ("T", key(root(1, [0xCC; 16], z), z, z)),
        ("L", key(root(1, z, [0xDD; 16]), z, z)),
        ("J", key(root(2, z, z), z, z)),
    ] {
        assert_ne!(
            variant, base,
            "a key differing only in the {axis} env axis must be distinct"
        );
    }

    // Warm isolation: the SAME root identity under a different env axis
    // is an independent family entry — both compute cold, and an exact
    // re-key warm-hits.
    let project = make_project();
    project
        .upsert_base("/cg_axes.ts", "export type Probe = { x: number }\n")
        .unwrap();
    let host = project.host();
    let dispatch = ProjectSemanticDispatch::new(host);
    let id = decl_identity(host, "/cg_axes.ts", "Probe");
    let live_key = dispatch.materialization_cycle_gate_key_for(&id);
    let SemanticQueryKey::ClassifyMaterializationCycleGate(live) = live_key.clone() else {
        panic!("key builder must produce the gate variant");
    };
    let shifted = SemanticQueryKey::ClassifyMaterializationCycleGate(
        crate::semantic_query::MaterializationCycleGateKey {
            parse_env_hash: [0xAA; 16],
            ..live
        },
    );
    assert_eq!(
        crate::semantic_query_memo::family_variant_label_for_tests(&live_key),
        "ClassifyMaterializationCycleGate",
        "the gate key maps to its dedicated family"
    );

    reset_cycle_gate_compute_counter_for_test();
    let _ = dispatch.classify_materialization_cycle_gate_read(live_key.clone());
    let _ = dispatch.classify_materialization_cycle_gate_read(shifted);
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        2,
        "keys differing only in P must compute independently"
    );
    let _ = dispatch.classify_materialization_cycle_gate_read(live_key);
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        2,
        "an exact re-key must warm-hit, not recompute"
    );
}

/// Live-generation gate: a bare project-generation bump (no content
/// edit, NO eager eviction) rejects the warm candidate — the producer
/// re-runs. The rejection rides two rails: the carrier's
/// `FactVersionRef::ProjectGeneration` (from the walk's dep signature)
/// AND the family's membership in `family_requires_live_generation_gate`
/// (the `validated_at_generation` parity rail). Mutation recipe: an
/// entry admitted with neither rail warm-serves stale and fails the
/// recompute assertion.
#[test]
fn cycle_gate_warm_candidate_rejected_on_bare_generation_bump() {
    let project = make_project();
    project
        .upsert_base("/cg_gen.ts", "export type Probe = { x: number }\n")
        .unwrap();
    let host = project.host();
    let first = gate_read(host, "/cg_gen.ts", "Probe");
    assert_decided(
        &first,
        MaterializationCycleGateVerdict::Continue,
        "gen prime",
    );

    // Bare generation bump WITHOUT the eager evict: only the family's
    // live-generation gate can reject the warm candidate.
    host.project_type_store().bump_project_generation();

    reset_cycle_gate_compute_counter_for_test();
    let _ = gate_read(host, "/cg_gen.ts", "Probe");
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        1,
        "a bare generation bump must reject the warm candidate — the producer re-runs"
    );
}

/// Visited-root capture: the walk records EVERY visited non-builtin
/// declaration (root + helpers) as an observed self-root, so a content
/// edit to a visited helper rejects the warm entry. Mutation recipe:
/// recording only the root (or no observed self-roots) lets the edited
/// helper's entry warm-serve and fails the recompute assertion.
#[test]
fn cycle_gate_visited_helper_edit_forces_recompute() {
    let project = make_project();
    project
        .upsert_base(
            "/cg_helper.ts",
            "export type Helper<T> = { wrapped: T; next: Helper<T> };\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/cg_root.ts",
            "import type { Helper } from './cg_helper';\nexport type Probe = Helper<number>;\n",
        )
        .unwrap();
    let host = project.host();
    assert!(host.ensure_indexed_ready("/cg_helper.ts").is_some());
    assert!(host.ensure_indexed_ready("/cg_root.ts").is_some());

    let first = gate_read(host, "/cg_root.ts", "Probe");
    assert_decided(
        &first,
        MaterializationCycleGateVerdict::Stop,
        "Helper<number> root",
    );

    // Content edit to the VISITED helper only; the root is untouched.
    project
        .upsert_base(
            "/cg_helper.ts",
            "export type Helper<T> = { wrapped: T; sibling: string; next: Helper<T> };\n",
        )
        .unwrap();
    assert!(host.ensure_indexed_ready("/cg_helper.ts").is_some());

    reset_cycle_gate_compute_counter_for_test();
    let _ = gate_read(host, "/cg_root.ts", "Probe");
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        1,
        "a content edit to a visited helper must reject the warm entry — \
         every visited declaration is an observed self-root"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Cache / invalidation proofs
// ──────────────────────────────────────────────────────────────────────────

/// The helper fixture: a generic self-referencing helper (complex
/// signal — the ref carries type args) reached from a plain root, so
/// the walk visits BOTH canonicals and decides Stop.
fn helper_pair_fixture(project: &StdArc<MetaProject>, root: &str, helper: &str) {
    project
        .upsert_base(
            helper,
            "export type Helper<T> = { wrapped: T; next: Helper<T> };\n",
        )
        .unwrap();
    project
        .upsert_base(
            root,
            &format!(
                "import type {{ Helper }} from './{}';\nexport type Probe = Helper<number>;\n",
                helper.trim_start_matches('/').trim_end_matches(".ts")
            ),
        )
        .unwrap();
}

/// Whether the dep signature carries a `WholeHash` entry for
/// `canonical`.
fn signature_covers(signature: &crate::semantic_query::DepSignature, canonical: &str) -> bool {
    signature.iter().any(|(c, version)| {
        c.as_ref() == canonical
            && matches!(version, crate::semantic_query::DepVersion::WholeHash(_))
    })
}

/// Cold AND warm reads return the family dep signature — the consumer
/// fence-merge parity (both read paths must hand the caller the root
/// and visited-helper `WholeHash` entries). Mutation recipe: a warm
/// path that drops the signature (serving the bare outcome) fails the
/// warm half; a producer that drops the per-hop fence merge fails the
/// cold half.
#[test]
fn cycle_gate_cold_and_warm_reads_return_family_dep_signature() {
    let project = make_project();
    helper_pair_fixture(&project, "/cg_sig_root.ts", "/cg_sig_helper.ts");
    let host = project.host();
    assert!(host.ensure_indexed_ready("/cg_sig_helper.ts").is_some());
    assert!(host.ensure_indexed_ready("/cg_sig_root.ts").is_some());

    let cold = gate_read(host, "/cg_sig_root.ts", "Probe");
    assert_decided(&cold, MaterializationCycleGateVerdict::Stop, "sig prime");
    assert!(
        signature_covers(&cold.dep_signature, "/cg_sig_root.ts"),
        "cold read: dep signature must cover the root canonical"
    );
    assert!(
        signature_covers(&cold.dep_signature, "/cg_sig_helper.ts"),
        "cold read: dep signature must cover the visited helper canonical"
    );

    reset_cycle_gate_compute_counter_for_test();
    let warm = gate_read(host, "/cg_sig_root.ts", "Probe");
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        0,
        "fixture: the second read is warm"
    );
    assert!(
        signature_covers(&warm.dep_signature, "/cg_sig_root.ts")
            && signature_covers(&warm.dep_signature, "/cg_sig_helper.ts"),
        "warm read: dep signature must cover root and visited helper — \
         the consumer merges it into its local fence on BOTH read paths"
    );
}

/// Observed-self-root completeness: the producer records EVERY visited
/// non-builtin declaration (root included) as `(canonical, observed
/// whole_hash)`. Mutation recipe: dropping the root's (or the visited
/// child's) `record_self_root` call fails the exact-set assertion.
#[test]
fn cycle_gate_observed_self_roots_cover_root_and_every_visited_decl() {
    let project = make_project();
    helper_pair_fixture(&project, "/cg_roots_root.ts", "/cg_roots_helper.ts");
    let host = project.host();
    let root_hash = host
        .ensure_indexed_ready("/cg_roots_root.ts")
        .expect("root indexed")
        .whole_hash;
    let helper_hash = host
        .ensure_indexed_ready("/cg_roots_helper.ts")
        .expect("helper indexed")
        .whole_hash;

    let dispatch = ProjectSemanticDispatch::new(host);
    let id = decl_identity(host, "/cg_roots_root.ts", "Probe");
    let SemanticQueryKey::ClassifyMaterializationCycleGate(key) =
        dispatch.materialization_cycle_gate_key_for(&id)
    else {
        panic!("key builder must produce the gate variant");
    };
    let output = dispatch.build_classify_materialization_cycle_gate(&key);

    assert!(
        output
            .observed_self_roots
            .contains(&(StdArc::from("/cg_roots_root.ts"), root_hash)),
        "the root declaration is an observed self-root with its observed hash"
    );
    assert!(
        output
            .observed_self_roots
            .contains(&(StdArc::from("/cg_roots_helper.ts"), helper_hash)),
        "every visited declaration is an observed self-root with its observed hash"
    );
    assert!(
        !output.cache_suppress && !output.result_is_partial,
        "a complete walk carries clean admission rails"
    );
}

/// Per-canonical invalidation through the semantic-family reverse
/// index: draining the VISITED helper's canonical evicts the gate
/// entry (the carrier's facts register every visited canonical).
/// Mutation recipe: a family whose publish path skips reverse-index
/// registration warm-serves stale and fails the recompute assertion.
#[test]
fn cycle_gate_warm_entry_evicted_by_per_canonical_reverse_index_drain() {
    let project = make_project();
    helper_pair_fixture(&project, "/cg_drain_root.ts", "/cg_drain_helper.ts");
    let host = project.host();
    assert!(host.ensure_indexed_ready("/cg_drain_helper.ts").is_some());
    assert!(host.ensure_indexed_ready("/cg_drain_root.ts").is_some());

    let cold = gate_read(host, "/cg_drain_root.ts", "Probe");
    assert_decided(&cold, MaterializationCycleGateVerdict::Stop, "drain prime");

    let drained = host
        .project_type_store()
        .semantic_graph()
        .invalidate_canonical("/cg_drain_helper.ts");
    assert!(
        drained >= 1,
        "the helper canonical's reverse-index shard must hold the gate entry \
         (drained {drained})"
    );

    reset_cycle_gate_compute_counter_for_test();
    let _ = gate_read(host, "/cg_drain_root.ts", "Probe");
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        1,
        "a per-canonical reverse-index drain of a visited helper must evict the \
         gate entry — the producer re-runs"
    );
}

/// Untracked visited self-root: removing the visited helper file makes
/// its recorded self-root untracked, and the warm entry must NOT serve
/// (the strict self-root rail rejects untracked self-roots; the
/// removal-side invalidation may also drain eagerly — either way the
/// stale verdict cannot survive). Mutation recipe: a producer that
/// records no observed self-root for the helper (the lazy
/// untracked-accept fence arm would then admit the entry) fails the
/// recompute assertion.
#[test]
fn cycle_gate_untracked_visited_self_root_rejects_warm_entry() {
    let project = make_project();
    helper_pair_fixture(&project, "/cg_untracked_root.ts", "/cg_untracked_helper.ts");
    let host = project.host();
    assert!(host
        .ensure_indexed_ready("/cg_untracked_helper.ts")
        .is_some());
    assert!(host.ensure_indexed_ready("/cg_untracked_root.ts").is_some());

    let cold = gate_read(host, "/cg_untracked_root.ts", "Probe");
    assert_decided(
        &cold,
        MaterializationCycleGateVerdict::Stop,
        "untracked prime",
    );

    host.remove("/cg_untracked_helper.ts");

    reset_cycle_gate_compute_counter_for_test();
    let _ = gate_read(host, "/cg_untracked_root.ts", "Probe");
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        1,
        "an untracked visited self-root must reject the warm entry — \
         the producer re-runs (same verdict, ReturnOnly)"
    );
}

/// Content-only revision: the key is content-free (R6), so a content
/// edit keeps the SAME key but must admit a NEW validated candidate —
/// no stale reuse, and the post-edit read warm-serves its own fresh
/// candidate. Mutation recipe: keying content into the family identity
/// breaks the same-key assertion; failing self-version rooting
/// warm-serves the stale verdict and breaks the recompute assertion.
#[test]
fn cycle_gate_content_only_revision_admits_new_candidate_under_same_key() {
    let project = make_project();
    helper_pair_fixture(&project, "/cg_rev_root.ts", "/cg_rev_helper.ts");
    let host = project.host();
    assert!(host.ensure_indexed_ready("/cg_rev_helper.ts").is_some());
    assert!(host.ensure_indexed_ready("/cg_rev_root.ts").is_some());

    let dispatch = ProjectSemanticDispatch::new(host);
    let id = decl_identity(host, "/cg_rev_root.ts", "Probe");
    let key_before = dispatch.materialization_cycle_gate_key_for(&id);

    let cold = dispatch.classify_materialization_cycle_gate(&id);
    assert_decided(
        &cold,
        MaterializationCycleGateVerdict::Stop,
        "revision prime",
    );

    // Content-only revision of the ROOT file (verdict-neutral: an added
    // unused declaration). The content-free key is unchanged.
    project
        .upsert_base(
            "/cg_rev_root.ts",
            "import type { Helper } from './cg_rev_helper';\n\
             export type Probe = Helper<number>;\n\
             export type Unused = { extra: string };\n",
        )
        .unwrap();
    assert!(host.ensure_indexed_ready("/cg_rev_root.ts").is_some());
    let key_after = dispatch.materialization_cycle_gate_key_for(&id);
    assert_eq!(
        key_before, key_after,
        "a content-only revision must keep the SAME content-free key"
    );

    reset_cycle_gate_compute_counter_for_test();
    let revised = dispatch.classify_materialization_cycle_gate(&id);
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        1,
        "the stale candidate must be rejected — the producer re-runs"
    );
    assert_decided(
        &revised,
        MaterializationCycleGateVerdict::Stop,
        "revised read",
    );

    reset_cycle_gate_compute_counter_for_test();
    let warm = dispatch.classify_materialization_cycle_gate(&id);
    assert_eq!(
        cycle_gate_compute_counter_for_test(),
        0,
        "the NEW candidate must validate and warm-serve (no stale reuse, no thrash)"
    );
    assert_eq!(warm.value, revised.value);
}

/// Cross-domain isolation: the classifier contributes NO checker frame
/// and no lowlink edge — a relation computed BEFORE and AFTER a gate
/// walk is bit-identical, and a NotAssignable judgement stays
/// NotAssignable. Mutation recipe: a producer that pushes relation
/// frames (or otherwise poisons the checker reentry stack) flips or
/// corrupts a recursive relation outcome here.
#[test]
fn cycle_gate_classifier_invoked_around_relate_leaves_relation_frames_untouched() {
    use crate::project_semantic_dispatch::relation_txn::RelationStep;
    use crate::semantic_query::{
        InstantiateKey, ProjectionMode, ProjectionReductionContext, QueryResult,
    };

    let project = make_project();
    project
        .upsert_base(
            "/cg_relate.ts",
            "export type RecA = { b: RecB | null; tag: number }\n\
             export type RecB = { a: RecA | null }\n\
             export type PlainA = { x: number; inner: { y: string } }\n\
             export type PlainB = { x: number; inner: { y: string } }\n\
             export type PlainC = { x: string; inner: { y: string } }\n\
             export type JSONValue = string | { [k: string]: JSONValue } | JSONValue[]\n",
        )
        .unwrap();
    let host = project.host();
    assert!(host.ensure_indexed_ready("/cg_relate.ts").is_some());
    let dispatch = ProjectSemanticDispatch::new(host);

    let instantiate = |name: &str| {
        let id = decl_identity(host, "/cg_relate.ts", name);
        let key = SemanticQueryKey::Instantiate(InstantiateKey::new(
            dispatch.type_slot_for(
                StdArc::clone(&id.canonical_id),
                id.owner,
                StdArc::clone(&id.decl_name),
            ),
            StdArc::from(Vec::new().into_boxed_slice()),
            dispatch.instantiate_context_for(
                "/cg_relate.ts",
                ProjectionReductionContext::published(ProjectionMode::Expanded),
            ),
        ));
        match dispatch.execute_read(key).value {
            QueryResult::Value(node) => node,
            other => panic!("Instantiate({name}) must produce a value, got {other:?}"),
        }
    };

    let rec_a = instantiate("RecA");
    let plain_a = instantiate("PlainA");
    let plain_b = instantiate("PlainB");
    let plain_c = instantiate("PlainC");

    let relate = |source, target| dispatch.execute_relate(dispatch.relate_key_for(source, target));

    let assignable_before = relate(plain_a, plain_b);
    let not_assignable_before = relate(plain_a, plain_c);
    let recursive_self_before = relate(rec_a, rec_a);
    assert!(
        matches!(assignable_before, RelationStep::Assignable { .. }),
        "fixture: structurally identical plain types are assignable, got {assignable_before:?}"
    );
    assert!(
        matches!(not_assignable_before, RelationStep::NotAssignable),
        "fixture: mismatched member is not assignable, got {not_assignable_before:?}"
    );

    // Invoke the classifier BETWEEN the two relation batches.
    let gate = gate_read(host, "/cg_relate.ts", "JSONValue");
    assert_decided(
        &gate,
        MaterializationCycleGateVerdict::Stop,
        "gate between relates",
    );

    let assignable_after = relate(plain_a, plain_b);
    let not_assignable_after = relate(plain_a, plain_c);
    let recursive_self_after = relate(rec_a, rec_a);
    assert!(
        matches!(assignable_after, RelationStep::Assignable { .. }),
        "the gate walk must not touch relation frames — the assignable \
         judgement is identical before and after"
    );
    assert!(
        matches!(not_assignable_after, RelationStep::NotAssignable),
        "the gate walk must not touch relation frames — the not-assignable \
         judgement is identical before and after"
    );
    assert_eq!(
        std::mem::discriminant(&recursive_self_before),
        std::mem::discriminant(&recursive_self_after),
        "the gate walk must not touch relation frames — the recursive \
         self-relation verdict is stable before and after \
         ({recursive_self_before:?} vs {recursive_self_after:?})"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Carrier-arg descent for the producer's scanners. A `BareRef` / `TypeOf` /
// `ImportType` carrier applies its `type_args` at the reference site; those
// args can carry a `DeclRef` / `InstantiationRef` (a real cross-decl edge)
// or an `Opaque(RecursiveRef)` (a cycle back-edge). The scanners MUST
// descend `SemanticNodeData::carrier_type_args` — a missed edge would
// under-collect the cycle graph and let a genuine cycle escape the gate.
// ──────────────────────────────────────────────────────────────────────────
mod carrier_descent_tests {
    use std::sync::Arc;

    use crate::semantic_query::{
        DeclIdentity, NodeScopeId, QueryError, ScopeId, SemanticNodeData, SemanticNodeId,
        ValueRootKey,
    };
    use crate::semantic_query_memo::SemanticGraphStore;

    use crate::project_semantic_dispatch::cycle_gate::{
        cycle_gate_body_contains_recursive_ref_for_test as cycle_gate_body_contains_recursive_ref,
        cycle_gate_collect_ref_identities_for_test as cycle_gate_collect_ref_identities,
    };

    fn decl_identity(canonical: &str, name: &str) -> DeclIdentity {
        DeclIdentity::from_scope(
            &NodeScopeId::File {
                canonical_id: Arc::from(canonical),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: [7u8; 16],
                local_scope: None,
            },
            Arc::from(name),
        )
    }

    /// Build the three carriers, each wrapping `arg` as its single `type_args`
    /// entry, so a single descent assertion covers all three carrier kinds.
    fn carriers_wrapping(graph: &SemanticGraphStore, arg: SemanticNodeId) -> Vec<SemanticNodeId> {
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![arg].into_boxed_slice());
        vec![
            graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from("Foo"),
                NodeScopeId::Global,
                Arc::clone(&args),
            )),
            graph.intern_node(SemanticNodeData::new_typeof(
                ValueRootKey {
                    scope: ScopeId {
                        canonical_id: Arc::from("/v.ts"),
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        local_scope: None,
                        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                            verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        ),
                    },
                    name: Arc::from("factory"),
                },
                Arc::from(Vec::new().into_boxed_slice()),
                Arc::clone(&args),
            )),
            graph.intern_node(SemanticNodeData::new_import_type(
                Arc::from("./m"),
                Arc::from(vec![Arc::<str>::from("G")].into_boxed_slice()),
                Arc::clone(&args),
                false,
            )),
        ]
    }

    // D1 — collect descends carrier args. NEGATIVE: with the `_ => {}`
    // arm the carrier is a leaf and the identity is missed.
    #[test]
    fn collect_ref_identities_descends_carrier_args() {
        let graph = SemanticGraphStore::new();
        let inner_id = decl_identity("/dep.ts", "Inner");
        let decl_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: inner_id.clone(),
        });

        for carrier in carriers_wrapping(&graph, decl_ref) {
            let mut out: Vec<(DeclIdentity, bool)> = Vec::new();
            let mut reasons = Vec::new();
            cycle_gate_collect_ref_identities(&graph, carrier, &mut out, &mut reasons);
            assert!(
                out.iter().any(|(id, _)| *id == inner_id),
                "a DeclRef inside a carrier's type_args must be collected; got {out:?}"
            );
            assert!(reasons.is_empty(), "a complete scan records no reasons");
        }

        // InstantiationRef arg variant — the base identity is collected with
        // `has_type_args = true`.
        let inst_base = decl_identity("/dep.ts", "Box");
        let inst_ref = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: inst_base.clone(),
            args: Arc::from(Vec::new().into_boxed_slice()),
        });
        for carrier in carriers_wrapping(&graph, inst_ref) {
            let mut out: Vec<(DeclIdentity, bool)> = Vec::new();
            let mut reasons = Vec::new();
            cycle_gate_collect_ref_identities(&graph, carrier, &mut out, &mut reasons);
            assert!(
                out.iter().any(|(id, _)| *id == inst_base),
                "an InstantiationRef inside a carrier's type_args must be collected; got {out:?}"
            );
        }
    }

    // D2 — recursive-ref detection descends carrier args. NEGATIVE: with
    // the `_ => {}` arm the carrier is a leaf and the predicate returns
    // `false`.
    #[test]
    fn body_contains_recursive_ref_descends_carrier_args() {
        let graph = SemanticGraphStore::new();
        let target: Arc<str> = Arc::from("SelfRef");
        let rec = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::clone(&target),
        }));

        for carrier in carriers_wrapping(&graph, rec) {
            let mut reasons = Vec::new();
            assert!(
                cycle_gate_body_contains_recursive_ref(&graph, carrier, &target, 0, &mut reasons),
                "a RecursiveRef back-edge inside a carrier's type_args must be found for `{target}`"
            );
        }

        // NEGATIVE control: a carrier whose args contain a RecursiveRef to a
        // DIFFERENT name does NOT match the target (proving the descent reads
        // the actual name, not a blanket true).
        let other = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::from("OtherName"),
        }));
        for carrier in carriers_wrapping(&graph, other) {
            let mut reasons = Vec::new();
            assert!(
                !cycle_gate_body_contains_recursive_ref(&graph, carrier, &target, 0, &mut reasons),
                "a carrier whose args reference a DIFFERENT name must NOT match the target"
            );
        }
    }
}
