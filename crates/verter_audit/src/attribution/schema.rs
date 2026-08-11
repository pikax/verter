//! The closed work-site schema.
//!
//! Every counted quantity in the substrate is named by a [`WorkSite`]
//! variant. The enum is CLOSED and declared in exactly one place (the
//! [`declare_work_sites!`] invocation below), so a site cannot be minted
//! ad hoc at a call site the way a string-keyed counter can, and the
//! full inventory is enumerable at compile time via [`WorkSite::ALL`].
//!
//! The schema itself is compiled unconditionally — it carries no
//! storage and no reader. Only the counter table and its accessors are
//! behind the `attribution` feature, which is what keeps a production
//! build unable to resolve a path from a counter back into a decision
//! (see the module docs on [`super`]).

/// The category of work a [`WorkSite`] accounts for.
///
/// One variant per measurement category the baseline is required to
/// explain. A site declares exactly one domain, so a report can roll
/// per-site rows up to a domain total without a second mapping table.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum WorkDomain {
    /// Path/specifier/type normalisation.
    Normalization,
    /// Content, semantic, environment and profile hashing.
    Hashing,
    /// Source parsing, including repeat parses of the same file version.
    Parsing,
    /// Shallow indexing, declaration lowering and prepared-declaration
    /// construction.
    Preparation,
    /// Demand planning and batch/task submission shaping.
    Planning,
    /// Published-surface projection and structural materialisation.
    Projection,
    /// Query-time type resolution dispatch.
    Resolution,
    /// Assignability / relation decisions.
    Relation,
    /// Instantiation, substitution and conditional reduction.
    Inference,
    /// Flow-graph construction and slice evaluation.
    Flow,
    /// Style-block parsing, analysis and rewriting.
    Css,
    /// Emitted-code construction.
    Rendering,
    /// Source-map and position-mapping construction.
    Mapping,
    /// Derivation-edge and fact-observation recording.
    Provenance,
    /// Wire/DTO encoding.
    Serialization,
    /// Native/WASM boundary crossings.
    Ffi,
    /// Duplication of already-materialised data.
    Copying,
    /// Heap allocation attributed to an enclosing scope.
    Allocation,
    /// Bump-arena live and reserved bytes.
    Arena,
    /// Scheduler task execution and in-flight joins.
    Task,
    /// Scheduler queue occupancy.
    Queue,
    /// Cache admission decisions.
    Admission,
    /// Cache eviction decisions.
    Eviction,
    /// Bytes held live by a store after a request completes.
    Retention,
    /// Artifact pin acquire/release.
    Pinning,
    /// Order-independent digests over produced values.
    Digest,
}

impl WorkDomain {
    /// Stable lowercase identifier, used as the report column key.
    pub const fn id(self) -> &'static str {
        match self {
            WorkDomain::Normalization => "normalization",
            WorkDomain::Hashing => "hashing",
            WorkDomain::Parsing => "parsing",
            WorkDomain::Preparation => "preparation",
            WorkDomain::Planning => "planning",
            WorkDomain::Projection => "projection",
            WorkDomain::Resolution => "resolution",
            WorkDomain::Relation => "relation",
            WorkDomain::Inference => "inference",
            WorkDomain::Flow => "flow",
            WorkDomain::Css => "css",
            WorkDomain::Rendering => "rendering",
            WorkDomain::Mapping => "mapping",
            WorkDomain::Provenance => "provenance",
            WorkDomain::Serialization => "serialization",
            WorkDomain::Ffi => "ffi",
            WorkDomain::Copying => "copying",
            WorkDomain::Allocation => "allocation",
            WorkDomain::Arena => "arena",
            WorkDomain::Task => "task",
            WorkDomain::Queue => "queue",
            WorkDomain::Admission => "admission",
            WorkDomain::Eviction => "eviction",
            WorkDomain::Retention => "retention",
            WorkDomain::Pinning => "pinning",
            WorkDomain::Digest => "digest",
        }
    }
}

/// What a site's `amount` column means.
///
/// The unit is part of the schema so a report never has to guess
/// whether a number is a count, a size or a duration.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum WorkUnit {
    /// `amount` is unused; only `calls` is meaningful.
    Calls,
    /// `amount` accumulates a logical item count (members, edges, tasks).
    Items,
    /// `amount` accumulates bytes.
    Bytes,
    /// `amount` accumulates wall-clock nanoseconds (inclusive of nested
    /// scopes, matching the enclosing-scope convention).
    ///
    /// NOT ADDITIVE, and NOT a share of wall clock. Every interval is
    /// INCLUSIVE, so a site that re-enters itself — recursion, a cold
    /// build that re-dispatches — records the full interval once per
    /// open frame and the column double-counts by recursion depth. The
    /// same applies to the `nanos` column that scope guards fill. A
    /// recursive site's total can exceed the run's entire wall clock
    /// (it does on the captured baseline), and summing intervals across
    /// sites double-counts every nested region. Read the column as
    /// "summed inclusive intervals", never as "time spent here".
    Nanoseconds,
    /// `amount` holds a running MAXIMUM rather than a sum — a gauge.
    Gauge,
    /// The site contributes to the order-independent `digest` column;
    /// `amount` is unused.
    Digest,
}

impl WorkUnit {
    /// Stable lowercase identifier, used as the report column key.
    pub const fn id(self) -> &'static str {
        match self {
            WorkUnit::Calls => "calls",
            WorkUnit::Items => "items",
            WorkUnit::Bytes => "bytes",
            WorkUnit::Nanoseconds => "ns",
            WorkUnit::Gauge => "gauge",
            WorkUnit::Digest => "digest",
        }
    }

    /// Whether the site's `amount` column is a running maximum instead
    /// of a running sum. Reset/merge logic branches on this.
    pub const fn is_gauge(self) -> bool {
        matches!(self, WorkUnit::Gauge)
    }
}

macro_rules! declare_work_sites {
    ($( $variant:ident => $id:literal, $domain:ident, $unit:ident ; )*) => {
        /// The closed inventory of counted work sites.
        ///
        /// Variants are ordered by domain for readability only; the
        /// discriminant is an implementation detail and is not stable
        /// across revisions of this list. [`WorkSite::id`] is the stable
        /// external name.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        #[repr(u16)]
        pub enum WorkSite {
            $(
                #[doc = $id]
                $variant,
            )*
        }

        impl WorkSite {
            /// Every declared site, in declaration order.
            pub const ALL: &'static [WorkSite] = &[ $( WorkSite::$variant, )* ];

            /// Number of declared sites — the counter table's length.
            pub const COUNT: usize = WorkSite::ALL.len();

            /// The site's stable dotted identifier
            /// (`<crate>.<chokepoint>`).
            pub const fn id(self) -> &'static str {
                match self { $( WorkSite::$variant => $id, )* }
            }

            /// The measurement category this site rolls up into.
            pub const fn domain(self) -> WorkDomain {
                match self { $( WorkSite::$variant => WorkDomain::$domain, )* }
            }

            /// What this site's `amount` column means.
            pub const fn unit(self) -> WorkUnit {
                match self { $( WorkSite::$variant => WorkUnit::$unit, )* }
            }

            /// Dense table index.
            pub const fn index(self) -> usize {
                self as usize
            }
        }
    };
}

declare_work_sites! {
    // ── normalization ────────────────────────────────────────────────
    NormalizeCanonicalId    => "workspace.normalize_canonical_id",   Normalization, Bytes;
    CollapsePath            => "workspace.collapse_path",            Normalization, Bytes;
    NormalizeRelativeSpec   => "workspace.normalize_relative_specifier", Normalization, Calls;
    NormalizeUnion          => "session.normalize_union",            Normalization, Calls;
    NormalizeIntersection   => "session.normalize_intersection",     Normalization, Calls;

    // ── hashing ──────────────────────────────────────────────────────
    ContentHash             => "session.hash_16",                    Hashing, Bytes;
    SemanticHash            => "session.semantic_hash",              Hashing, Calls;
    CompileProfileHash      => "session.compile_profile_hash",       Hashing, Calls;
    ParseStableHash         => "session.parse_stable_hash",          Hashing, Calls;
    EnvHash                 => "workspace.env_hash",                 Hashing, Bytes;

    // ── parsing ──────────────────────────────────────────────────────
    CarrierParse            => "compiler.carrier_parse",             Parsing, Bytes;
    ScriptParse             => "session.oxc_script_parse",           Parsing, Bytes;
    EvalProgramParse        => "session.oxc_eval_program_parse",     Parsing, Bytes;
    RetainedSnapshotReuse   => "session.retained_snapshot_reuse",    Parsing, Calls;
    CompilerExpressionParse => "compiler.oxc_expression_parse",      Parsing, Bytes;

    // ── preparation ──────────────────────────────────────────────────
    IndexedReadyBuild       => "session.indexed_ready_build",        Preparation, Calls;
    ShallowStateBuild       => "session.shallow_file_state_build",   Preparation, Calls;
    EvalEnvBuild            => "session.eval_env_build",             Preparation, Calls;
    PreparedDeclBuild       => "session.prepared_decl_build",        Preparation, Calls;
    DeclBodyLower           => "session.decl_body_lower",            Preparation, Calls;

    // ── planning ─────────────────────────────────────────────────────
    FrameworkPlanSurfaces   => "session.framework_plan_surfaces",    Planning, Items;
    SchedulerSubmitRequest  => "scheduler.submit_request",           Planning, Calls;
    SchedulerSubmitBatch    => "scheduler.submit_batch",             Planning, Items;

    // ── projection ───────────────────────────────────────────────────
    PublishFieldTypes       => "session.publish_field_types",        Projection, Items;
    MacroMemberWalk         => "session.macro_member_walk",          Projection, Items;
    MaterializeStructure    => "session.materialize_structure",      Projection, Calls;
    GraphExportEncode       => "session.graph_export_encode",        Projection, Items;

    // ── resolution ───────────────────────────────────────────────────
    SemanticDispatch        => "session.semantic_dispatch",          Resolution, Calls;
    SemanticColdBuild       => "session.semantic_cold_build",        Resolution, Calls;
    SemanticWarmHit         => "session.semantic_warm_hit",          Resolution, Calls;
    ResolveDecl             => "session.resolve_decl",               Resolution, Calls;
    ImportRouteResolve      => "session.import_route_resolve",       Resolution, Calls;
    FrontierResolve         => "session.external_frontier_resolve",  Resolution, Calls;

    // ── relation ─────────────────────────────────────────────────────
    RelationDecide          => "session.relation_decide",            Relation, Calls;

    // ── inference ────────────────────────────────────────────────────
    Instantiate             => "session.instantiate",                Inference, Calls;
    Substitute              => "session.substitute",                 Inference, Items;
    ConditionalReduce       => "session.conditional_reduce",         Inference, Calls;

    // ── flow ─────────────────────────────────────────────────────────
    FlowGraphBuild          => "session.flow_graph_build",           Flow, Calls;
    FlowSliceCompute        => "session.flow_slice_compute",         Flow, Calls;

    // ── css ──────────────────────────────────────────────────────────
    CssParse                => "compiler.css_parse",                 Css, Bytes;
    CssTransform            => "compiler.css_transform",             Css, Calls;
    StyleAnalysis           => "compiler.style_analysis",            Css, Calls;

    // ── rendering ────────────────────────────────────────────────────
    CodeTransformRender     => "compiler.code_transform_render",     Rendering, Bytes;
    TemplateCodegenIde      => "compiler.template_codegen_ide",      Rendering, Calls;
    TemplateCodegenRuntime  => "compiler.template_codegen_runtime",  Rendering, Calls;

    // ── mapping ──────────────────────────────────────────────────────
    SourceMapBuild          => "compiler.source_map_build",          Mapping, Items;

    // ── provenance ───────────────────────────────────────────────────
    FactObserve             => "session.fact_observe",               Provenance, Items;
    ReadSetSignatureBuild   => "session.read_set_signature_build",   Provenance, Items;
    OriginEdgeRecord        => "session.origin_edge_record",         Provenance, Items;

    // ── serialization ────────────────────────────────────────────────
    TypeInfoGraphEncode     => "ffi.typeinfo_graph_encode",          Serialization, Bytes;
    AuditRecordEncode       => "napi.audit_record_encode",           Serialization, Bytes;

    // ── ffi ──────────────────────────────────────────────────────────
    NapiBoundaryCall        => "napi.boundary_call",                 Ffi, Calls;
    WasmBoundaryCall        => "wasm.boundary_call",                 Ffi, Calls;

    // ── copying ──────────────────────────────────────────────────────
    SourceTextCopy          => "session.source_text_copy",           Copying, Bytes;
    AnalysisSnapshotCopy    => "session.analysis_snapshot_copy",     Copying, Calls;
    CacheCandidateCopy      => "session.cache_candidate_copy",       Copying, Calls;

    // ── allocation ───────────────────────────────────────────────────
    UnattributedAllocation  => "runtime.unattributed_allocation",    Allocation, Bytes;

    // ── arena ────────────────────────────────────────────────────────
    ParseArenaUsed          => "session.parse_arena_used",           Arena, Bytes;
    ParseArenaCapacity      => "session.parse_arena_capacity",       Arena, Gauge;

    // ── task ─────────────────────────────────────────────────────────
    TaskExecute             => "scheduler.task_execute",             Task, Calls;
    TaskDedupJoin           => "scheduler.task_dedup_join",          Task, Calls;
    TaskWait                => "scheduler.task_wait",                Task, Nanoseconds;

    // ── queue ────────────────────────────────────────────────────────
    QueueDepth              => "scheduler.queue_depth",              Queue, Gauge;

    // ── admission ────────────────────────────────────────────────────
    CacheAdmitCacheable     => "session.cache_admit_cacheable",      Admission, Calls;
    CacheAdmitReturnOnly    => "session.cache_admit_return_only",    Admission, Calls;

    // ── eviction ─────────────────────────────────────────────────────
    FamilyCandidateEvict    => "session.family_candidate_evict",     Eviction, Calls;

    // ── retention ────────────────────────────────────────────────────
    StoreRetainedBytes      => "session.store_retained_bytes",       Retention, Gauge;

    // ── pinning ──────────────────────────────────────────────────────
    ArtifactPinAcquire      => "session.artifact_pin_acquire",       Pinning, Calls;
    ArtifactPinRelease      => "session.artifact_pin_release",       Pinning, Calls;

    // ── digest ───────────────────────────────────────────────────────
    ComponentMetaDigest     => "session.component_meta_digest",      Digest, Digest;
    CompiledOutputDigest    => "compiler.compiled_output_digest",    Digest, Digest;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_index_is_the_dense_declaration_ordinal() {
        for (ordinal, site) in WorkSite::ALL.iter().enumerate() {
            assert_eq!(
                site.index(),
                ordinal,
                "{} must index its own slot in the counter table",
                site.id()
            );
        }
        assert_eq!(WorkSite::COUNT, WorkSite::ALL.len());
    }

    #[test]
    fn site_ids_are_unique() {
        let mut ids: Vec<&'static str> = WorkSite::ALL.iter().map(|site| site.id()).collect();
        let declared = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            declared,
            "two work sites share a stable id — a report row would be ambiguous"
        );
    }

    #[test]
    fn site_ids_are_dotted_crate_qualified_names() {
        for site in WorkSite::ALL {
            let id = site.id();
            let (owner, chokepoint) = id
                .split_once('.')
                .unwrap_or_else(|| panic!("{id} is not `<owner>.<chokepoint>`"));
            assert!(!owner.is_empty(), "{id} has an empty owner segment");
            assert!(
                !chokepoint.is_empty(),
                "{id} has an empty chokepoint segment"
            );
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'),
                "{id} must be lowercase snake/dotted so report keys stay stable"
            );
        }
    }

    #[test]
    fn every_declared_domain_has_at_least_one_site() {
        // The domain list is the charter's measurement categories. A
        // domain with no site is a category the baseline cannot explain,
        // so it is a schema defect, not a gap to note later.
        const DOMAINS: &[WorkDomain] = &[
            WorkDomain::Normalization,
            WorkDomain::Hashing,
            WorkDomain::Parsing,
            WorkDomain::Preparation,
            WorkDomain::Planning,
            WorkDomain::Projection,
            WorkDomain::Resolution,
            WorkDomain::Relation,
            WorkDomain::Inference,
            WorkDomain::Flow,
            WorkDomain::Css,
            WorkDomain::Rendering,
            WorkDomain::Mapping,
            WorkDomain::Provenance,
            WorkDomain::Serialization,
            WorkDomain::Ffi,
            WorkDomain::Copying,
            WorkDomain::Allocation,
            WorkDomain::Arena,
            WorkDomain::Task,
            WorkDomain::Queue,
            WorkDomain::Admission,
            WorkDomain::Eviction,
            WorkDomain::Retention,
            WorkDomain::Pinning,
            WorkDomain::Digest,
        ];
        for domain in DOMAINS {
            assert!(
                WorkSite::ALL.iter().any(|site| site.domain() == *domain),
                "domain `{}` has no declared work site",
                domain.id()
            );
        }
    }

    #[test]
    fn domain_ids_are_unique() {
        let mut ids: Vec<&'static str> = WorkSite::ALL
            .iter()
            .map(|site| site.domain().id())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let mut domains: Vec<WorkDomain> = WorkSite::ALL.iter().map(|site| site.domain()).collect();
        domains.sort_unstable();
        domains.dedup();
        assert_eq!(
            ids.len(),
            domains.len(),
            "two domains share a stable id — a roll-up row would be ambiguous"
        );
    }

    #[test]
    fn the_gauge_inventory_is_exactly_the_high_water_mark_sites() {
        // A gauge site keeps a running MAXIMUM; every other site SUMS.
        // Getting that wrong silently corrupts a column rather than
        // failing, so the inventory is pinned here: declaring a new
        // gauge, or demoting one of these to a summing site, must be a
        // deliberate edit to this list.
        const EXPECTED_GAUGES: &[WorkSite] = &[
            WorkSite::ParseArenaCapacity,
            WorkSite::QueueDepth,
            WorkSite::StoreRetainedBytes,
        ];

        let declared: Vec<WorkSite> = WorkSite::ALL
            .iter()
            .copied()
            .filter(|site| site.unit().is_gauge())
            .collect();

        assert_eq!(
            declared,
            EXPECTED_GAUGES,
            "the set of high-water-mark sites changed: declared {:?}, expected {:?}",
            declared.iter().map(|s| s.id()).collect::<Vec<_>>(),
            EXPECTED_GAUGES.iter().map(|s| s.id()).collect::<Vec<_>>(),
        );
        for site in EXPECTED_GAUGES {
            assert_eq!(
                site.unit(),
                WorkUnit::Gauge,
                "{} lost its gauge unit",
                site.id()
            );
        }
    }
}
