//! Allocation and fact work per completed `Paths<Api>` demand.
//!
//! Every instantiation, conditional and template step of a recursive
//! mapped-index evaluation runs through the shared query frame: a fact
//! tracer, its finalised signature, the completed carrier and the memo's
//! admission with its reverse-index registration. This case measures what
//! one cold demand allocates, and the fact work its tracers do, on its own
//! thread across several layout sizes, so a frame, signature or admission
//! step that copies per fact, per registration or per enclosing scope shows
//! as growth per entity.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{alloc_count, reset_alloc_counter};

const UTILS: &str = "export interface Meta<T> { tag: T; created: number }\n\
export type Paths<T, D extends unknown[] = []> = D['length'] extends 3 ? never\n\
: T extends object ? { [K in keyof T & string]: K | `${K}.${Paths<T[K], [...D, 0]>}` }[keyof T & string] : never;";

/// `Paths<Api>` over `count` entities spread across `groups` mutually
/// importing modules.
fn paths_files(count: usize, groups: usize) -> Vec<(String, String)> {
    let mut files = vec![("/paths/utils.ts".to_string(), UTILS.to_string())];
    for group in 0..groups {
        let mut source = "import type { Meta } from './utils';\n".to_string();
        for other in (0..groups).filter(|other| *other != group) {
            let imports = (other..count)
                .step_by(groups)
                .map(|i| format!("E{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            source.push_str(&format!(
                "import type {{ {imports} }} from './entities-{other}';\n"
            ));
        }
        for i in (group..count).step_by(groups) {
            source.push_str(&format!(
                "export interface E{i} {{ id: {i}; name: string; next: E{}; owner: E{}; meta: Meta<{i}> }}\n",
                (i + 1) % count,
                (i * 7 + 3) % count
            ));
        }
        files.push((format!("/paths/entities-{group}.ts"), source));
    }
    let mut api = String::new();
    for group in 0..groups {
        let imports = (group..count)
            .step_by(groups)
            .map(|i| format!("E{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        api.push_str(&format!(
            "import type {{ {imports} }} from './entities-{group}';\n"
        ));
    }
    api.push_str("export interface Api {\n");
    for i in 0..count {
        api.push_str(&format!("e{i}: E{i};\n"));
    }
    api.push_str("}\n");
    files.push(("/paths/api.ts".to_string(), api));
    files.push((
        "/paths/scenario.ts".to_string(),
        "import type { Api } from './api'; import type { Paths } from './utils'; export type Probe = Paths<Api>;"
            .to_string(),
    ));
    files
}

/// What one cold `Paths<Api>` demand costs on this thread.
struct ColdPathsDemand {
    /// Allocations it made.
    allocations: u64,
    /// The fact-propagation work its tracers did: every fact or run they
    /// compared, merged or absorbed while observations propagated through
    /// the enclosing scopes and their signatures were sealed.
    fact_work: usize,
    /// The dotted paths it published.
    paths: usize,
}

fn cold_paths_demand(count: usize, groups: usize) -> ColdPathsDemand {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let files = paths_files(count, groups);
    for (path, source) in &files {
        workspace.inject_file(path.clone(), Arc::from(source.as_str()));
    }
    let workspace: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            ..HostConfig::default()
        },
        workspace,
    ));
    for (path, source) in &files {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(path.clone()),
            input_id: path.clone(),
            source: Arc::from(source.as_str()),
            file_language: FileLanguage::script_ts(),
            aliases: vec![],
        });
    }
    reset_alloc_counter();
    let fact_work_before = verter_session_query::facts::fact_read_set::signature_work_for_tests();
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(
            "/paths/scenario.ts",
            "Probe",
            &[],
            Some(verter_type_engine::semantic_query::ProjectionMode::Expanded),
        )
        .into_parts();
    let allocations = alloc_count();
    let fact_work =
        verter_session_query::facts::fact_read_set::signature_work_for_tests() - fact_work_before;
    let node = outcome.expect("the demand resolves").expect("Probe exists");
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("Probe projects");
    let answer: TypeExpr = serde_json::from_slice(&bytes).expect("valid TypeExpr JSON");
    let TypeExpr::Union(paths) = &answer else {
        panic!("{count} entities must publish a path union, got {answer:?}");
    };
    ColdPathsDemand {
        allocations,
        fact_work,
        paths: paths.len(),
    }
}

#[test]
fn recursive_paths_allocations_per_entity_stay_flat() {
    let sizes = [(4, 2), (8, 4), (16, 4)];
    let measured = sizes.map(|(count, groups)| {
        let ColdPathsDemand {
            allocations, paths, ..
        } = cold_paths_demand(count, groups);
        // Five members, two entity hops of five and the two meta fields.
        assert_eq!(
            paths,
            count * 18,
            "{count} entities publish every dotted path"
        );
        let per_entity = allocations / count as u64;
        eprintln!(
            "RECURSIVE_PATHS entities={count} allocations={allocations} per_entity={per_entity}"
        );
        per_entity
    });
    eprintln!("RECURSIVE_PATHS per_entity={measured:?}");
    // Each fact a frame merges, each registration the memo admits and each
    // drain it runs costs a bounded number of allocations; a copy per fact
    // per merge round, per enclosing tracer or per registration lookup
    // lifts every layout above this line.
    const PER_ENTITY_CEILING: u64 = 28_500;
    for (per_entity, (count, _)) in measured.iter().zip(sizes) {
        assert!(
            *per_entity <= PER_ENTITY_CEILING,
            "{count} entities allocated {per_entity} times per entity (ceiling {PER_ENTITY_CEILING}): {measured:?}"
        );
    }
    let (least, most) = (
        measured.iter().min().copied().unwrap_or_default(),
        measured.iter().max().copied().unwrap_or_default(),
    );
    assert!(
        most * 4 <= least * 5,
        "allocations per entity must stay flat as the layout grows: {measured:?}"
    );
}

/// An observation reaches the scopes that must see it once, and a completed
/// scope hands its sealed signature to the scope that consumes it: the
/// fact work of one cold demand is a bounded multiple of its entities, not
/// of its entities times the depth of the scopes they nest under.
#[test]
fn recursive_paths_fact_work_per_entity_stays_flat() {
    let sizes = [(4, 2), (8, 4), (16, 4)];
    let measured = sizes.map(|(count, groups)| {
        let demand = cold_paths_demand(count, groups);
        assert_eq!(
            demand.paths,
            count * 18,
            "{count} entities publish every dotted path"
        );
        let per_entity = demand.fact_work / count;
        eprintln!(
            "RECURSIVE_PATHS entities={count} fact_work={} per_entity={per_entity}",
            demand.fact_work
        );
        per_entity
    });
    eprintln!("RECURSIVE_PATHS fact_work_per_entity={measured:?}");
    // A scope that also re-absorbed every observation of every scope nested
    // under it, or re-merged the same runs once per enclosing level, lifts
    // every layout far above this line.
    const PER_ENTITY_CEILING: usize = 12_000;
    for (per_entity, (count, _)) in measured.iter().zip(sizes) {
        assert!(
            *per_entity <= PER_ENTITY_CEILING,
            "{count} entities did {per_entity} fact work per entity (ceiling {PER_ENTITY_CEILING}): {measured:?}"
        );
    }
    let (least, most) = (
        measured.iter().min().copied().unwrap_or_default(),
        measured.iter().max().copied().unwrap_or_default(),
    );
    assert!(
        most * 4 <= least * 5,
        "fact work per entity must stay flat as the layout grows: {measured:?}"
    );
}
