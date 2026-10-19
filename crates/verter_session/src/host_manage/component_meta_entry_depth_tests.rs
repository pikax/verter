//! A component whose own source nests past the connected query-depth bound.
//!
//! `defineProps<P30>()` over `type Pk = Partial<P(k-1)>` resolves each level
//! inside the previous one, so the production bound
//! (`MAX_CONNECTED_QUERY_DEPTH`) trips under the default operation
//! allowance. The public component-meta routes must deliver that refusal as
//! `CONNECTED_QUERY_DEPTH_LIMIT` and nothing the trip did not cause: a
//! lookup the trip cut short is not a missing dependency. An isolated
//! request then retains it as an isolated-root refusal and repeats it
//! without running the producer, while every ineligible request computes.
//!
//! `deep_partial_chain_source` and `isolated_depth_refusal_host` are the
//! reusable fixture and public invocation for this boundary.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_type_engine::request_context::{
    current_request_budget, RequestContext, RequestContextGuard,
};
use verter_type_engine::semantic_query::{FactStatus, PartialReasonSet};

use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

use super::COLD_BODY_PRE_RESOLVE_HOOK;

const OWNER: &str = "/w/deep-partial.vue";
const SIBLING: &str = "/w/shallow-sibling.vue";

/// `<script setup>` declaring `levels` nested `Partial` aliases over
/// `{ x: string }` and `defineProps` over the outermost. `prefix` is
/// prepended inside the script block, `extra` intersected into the props.
pub(crate) fn deep_partial_chain_source(levels: usize, prefix: &str, extra: &str) -> String {
    let mut script = String::from(prefix);
    script.push_str("type P0 = { x: string };\n");
    for level in 1..=levels {
        script.push_str(&format!("type P{level} = Partial<P{}>;\n", level - 1));
    }
    format!("<script setup lang=\"ts\">\n{script}defineProps<P{levels}{extra}>()\n</script>\n")
}

/// Far past the bound: each level nests one more resolution.
const DEEP: usize = 30;

fn upsert(host: &VerterHost, canonical: &str, source: &str, language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.into()),
            input_id: canonical.into(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// A default host (production depth and operation limits) holding the deep
/// owner and a shallow, complete sibling.
pub(crate) fn isolated_depth_refusal_host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        OWNER,
        &deep_partial_chain_source(DEEP, "", ""),
        FileLanguage::vue(),
    );
    upsert(
        &host,
        SIBLING,
        "<script setup lang=\"ts\">defineProps<{ y: number }>()</script>",
        FileLanguage::vue(),
    );
    host
}

fn reasons(status: FactStatus) -> PartialReasonSet {
    match status {
        FactStatus::Complete => PartialReasonSet::empty(),
        FactStatus::Approximate(reasons, _) | FactStatus::Unavailable(reasons) => reasons.get(),
    }
}

fn is_depth_refusal(status: FactStatus) -> bool {
    matches!(status, FactStatus::Unavailable(_))
        && reasons(status).contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT)
        && !reasons(status).contains(PartialReasonSet::MISSING_DEPENDENCY)
}

/// Counts cold-body entries on this thread while armed. Disarms on drop,
/// so a failed assertion never leaves the hook set for the next test.
struct ColdBodyProbe {
    entered: Rc<Cell<bool>>,
}

impl ColdBodyProbe {
    fn arm() -> Self {
        let entered = Rc::new(Cell::new(false));
        let flag = Rc::clone(&entered);
        COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.set(true)));
        });
        Self { entered }
    }

    fn entered(&self) -> bool {
        self.entered.get()
    }
}

impl Drop for ColdBodyProbe {
    fn drop(&mut self) {
        COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}

fn producer_misses(host: &VerterHost) -> u64 {
    host.provenance()
        .resolver_node_cache_misses
        .load(Ordering::Relaxed)
}

fn assert_sibling_exact(host: &VerterHost) {
    let sibling = host.get_component_meta(SIBLING).expect("sibling");
    assert!(sibling.is_complete(), "{:?}", sibling.status());
    let names: Vec<&str> = sibling.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["y"]);
}

/// The source computation itself crosses the depth bound: the delivered
/// status names `CONNECTED_QUERY_DEPTH_LIMIT`, not a generic or propagated
/// partial and not a missing dependency the trip invented. The identical
/// isolated repeat, on either output entry path, is the retained refusal
/// and runs no producer; the cold oracle on a fresh host computes the same
/// status.
///
/// Mutations: adding the cut-short lookup's `MISSING_DEPENDENCY` back in
/// `adopt_projection_result`, or dropping the isolated-root arm of
/// `classify_final_component_retention`, fails this test.
#[test]
fn a_real_depth_trip_is_delivered_as_depth_and_replayed_without_the_producer() {
    let host = isolated_depth_refusal_host();
    assert_sibling_exact(&host);

    let cold = ColdBodyProbe::arm();
    let first = host
        .get_component_meta(OWNER)
        .expect("the refusal recovers");
    assert!(cold.entered(), "the first demand computes");
    drop(cold);
    assert!(
        is_depth_refusal(first.status()),
        "the delivered status must be the depth refusal, got {:?}",
        first.status()
    );

    let misses = producer_misses(&host);
    let warm = ColdBodyProbe::arm();
    let again = host.get_component_meta(OWNER).expect("the refusal repeats");
    assert_eq!(again.status(), first.status());
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution(OWNER)
        .expect("the resolution path repeats the refusal");
    assert_eq!(resolution.fact_status(), first.status());
    assert!(
        resolution.synthesis_should_suppress,
        "a replayed refusal is not a cacheable value"
    );
    assert!(
        !warm.entered(),
        "an isolated repeat must not enter the cold body"
    );
    drop(warm);
    assert_eq!(
        producer_misses(&host),
        misses,
        "an isolated repeat runs no semantic producer"
    );
    assert_eq!(
        host.project_type_store().component_meta_results().len(),
        1,
        "only the complete sibling is a successful candidate"
    );
    assert_sibling_exact(&host);

    let oracle = isolated_depth_refusal_host();
    let cold = ColdBodyProbe::arm();
    let oracle_meta = oracle.get_component_meta(OWNER).expect("oracle");
    assert!(cold.entered(), "the cold oracle computes");
    assert_eq!(oracle_meta.status(), first.status());
}

/// The resolution entry path seals the same refusal when it is the first
/// caller, and the plain path repeats it.
#[test]
fn the_resolution_entry_path_seals_the_same_depth_refusal() {
    let host = isolated_depth_refusal_host();
    let cold = ColdBodyProbe::arm();
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution(OWNER)
        .expect("the refusal recovers");
    assert!(cold.entered());
    drop(cold);
    let status = resolution.fact_status();
    assert!(is_depth_refusal(status), "got {status:?}");
    assert!(resolution.synthesis_should_suppress);

    let misses = producer_misses(&host);
    let warm = ColdBodyProbe::arm();
    let (_analysis, again) = host
        .get_component_meta_with_resolution(OWNER)
        .expect("repeat");
    assert_eq!(again.fact_status(), status);
    let plain = host.get_component_meta(OWNER).expect("plain repeat");
    assert_eq!(plain.status(), status);
    assert!(!warm.entered(), "neither repeat enters the cold body");
    assert_eq!(producer_misses(&host), misses);
}

/// A relevant edit invalidates the retained refusal, and a shallow
/// equivalent heals to the exact answer.
#[test]
fn an_edit_invalidates_the_depth_refusal_and_a_shallow_equivalent_heals() {
    let host = isolated_depth_refusal_host();
    let first = host.get_component_meta(OWNER).expect("refusal");
    assert!(is_depth_refusal(first.status()), "{:?}", first.status());

    upsert(
        &host,
        OWNER,
        &deep_partial_chain_source(DEEP + 1, "", ""),
        FileLanguage::vue(),
    );
    let cold = ColdBodyProbe::arm();
    let edited = host.get_component_meta(OWNER).expect("edited");
    assert!(cold.entered(), "an edited owner recomputes");
    drop(cold);
    assert!(is_depth_refusal(edited.status()), "{:?}", edited.status());

    upsert(
        &host,
        OWNER,
        &deep_partial_chain_source(2, "", ""),
        FileLanguage::vue(),
    );
    let cold = ColdBodyProbe::arm();
    let healed = host.get_component_meta(OWNER).expect("healed");
    assert!(cold.entered(), "the shallow equivalent recomputes");
    drop(cold);
    assert!(healed.is_complete(), "{:?}", healed.status());
    let names: Vec<&str> = healed.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["x"]);
    assert_sibling_exact(&host);
}

fn request(id: u64, projection_budget: usize) -> RequestContextGuard {
    RequestContextGuard::install(RequestContext::with_kind_timing_and_projection_budget(
        id,
        Arc::from(OWNER),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        projection_budget,
    ))
}

/// A request that already spent before entering is not an isolated root:
/// it neither replays the isolated refusal nor seals one of its own, and a
/// different operation profile does not reuse it either. The isolated
/// refusal still repeats afterwards.
#[test]
fn non_isolated_and_other_profile_requests_compute_the_depth_refusal() {
    let host = isolated_depth_refusal_host();
    let sealed = host.get_component_meta(OWNER).expect("refusal");
    assert!(is_depth_refusal(sealed.status()), "{:?}", sealed.status());

    for round in 0..2 {
        let _spent = request(10 + round, 5_000_000);
        current_request_budget()
            .expect("request")
            .check_projection_op_count();
        let cold = ColdBodyProbe::arm();
        let meta = host.get_component_meta(OWNER).expect("non-isolated");
        assert!(
            cold.entered(),
            "a non-isolated request computes (round {round})"
        );
        assert!(is_depth_refusal(meta.status()), "{:?}", meta.status());
    }

    {
        let _profile = request(20, 4_000_000);
        let cold = ColdBodyProbe::arm();
        let meta = host.get_component_meta(OWNER).expect("other profile");
        assert!(cold.entered(), "a different operation profile computes");
        assert!(is_depth_refusal(meta.status()), "{:?}", meta.status());
    }

    let warm = ColdBodyProbe::arm();
    let replay = host.get_component_meta(OWNER).expect("isolated repeat");
    assert!(!warm.entered(), "the isolated refusal is still retained");
    assert_eq!(replay.status(), sealed.status());
}

/// A real missing dependency beside the depth trip is a competing cause:
/// both are delivered and the outcome is not retained. Supplying the input
/// heals the missing-dependency cause, after which the remaining depth
/// refusal is retained.
///
/// Mutation: admitting a set that also holds `MISSING_DEPENDENCY` in
/// `classify_final_component_retention` fails this test.
#[test]
fn a_missing_input_beside_the_depth_trip_is_kept_and_vetoes_retention() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        OWNER,
        &deep_partial_chain_source(DEEP, "import type { Extra } from './extra';\n", " & Extra"),
        FileLanguage::vue(),
    );

    for round in 0..2 {
        let cold = ColdBodyProbe::arm();
        let meta = host.get_component_meta(OWNER).expect("mixed");
        assert!(cold.entered(), "an ineligible outcome recomputes ({round})");
        drop(cold);
        let found = reasons(meta.status());
        assert!(
            found.contains(PartialReasonSet::MISSING_DEPENDENCY),
            "the missing import is kept as its own cause: {found:?}"
        );
        assert!(!meta.is_complete());
    }

    upsert(
        &host,
        "/w/extra.ts",
        "export type Extra = { z?: number };\n",
        FileLanguage::script_ts(),
    );
    let supplied = host.get_component_meta(OWNER).expect("supplied");
    assert!(
        is_depth_refusal(supplied.status()),
        "supplying the input leaves only the depth refusal: {:?}",
        supplied.status()
    );
    let warm = ColdBodyProbe::arm();
    let replay = host.get_component_meta(OWNER).expect("retained");
    assert!(!warm.entered(), "the remaining depth refusal is retained");
    assert_eq!(replay.status(), supplied.status());
}

/// A missing input with no depth trip is not retained and heals to the
/// exact answer when supplied.
#[test]
fn a_missing_input_alone_heals_when_supplied() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        OWNER,
        "<script setup lang=\"ts\">\nimport type { Extra } from './extra';\ndefineProps<Extra>()\n</script>\n",
        FileLanguage::vue(),
    );
    for round in 0..2 {
        let cold = ColdBodyProbe::arm();
        let meta = host.get_component_meta(OWNER).expect("missing");
        assert!(cold.entered(), "a missing input recomputes ({round})");
        drop(cold);
        assert!(
            reasons(meta.status()).contains(PartialReasonSet::MISSING_DEPENDENCY),
            "{:?}",
            meta.status()
        );
    }
    upsert(
        &host,
        "/w/extra.ts",
        "export type Extra = { z: number };\n",
        FileLanguage::script_ts(),
    );
    let healed = host.get_component_meta(OWNER).expect("healed");
    assert!(healed.is_complete(), "{:?}", healed.status());
    let names: Vec<&str> = healed.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["z"]);
}
