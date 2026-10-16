//! Allocation work per completed `Paths<Api>` demand.
//!
//! Every instantiation, conditional and template step of a recursive
//! mapped-index evaluation runs through the shared query frame: a fact
//! tracer, its finalised signature, the completed carrier and the memo's
//! admission with its reverse-index registration. This case measures what
//! one cold demand allocates on its own thread across several layout sizes,
//! so a frame, signature or admission step that copies per fact or per
//! registration shows as growth per entity.

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

/// Allocations one cold `Paths<Api>` demand makes on this thread, and the
/// number of dotted paths it published.
fn cold_paths_allocations(count: usize, groups: usize) -> (u64, usize) {
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
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(
            "/paths/scenario.ts",
            "Probe",
            &[],
            Some(verter_type_engine::semantic_query::ProjectionMode::Expanded),
        )
        .into_parts();
    let allocations = alloc_count();
    let node = outcome.expect("the demand resolves").expect("Probe exists");
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("Probe projects");
    let answer: TypeExpr = serde_json::from_slice(&bytes).expect("valid TypeExpr JSON");
    let TypeExpr::Union(paths) = &answer else {
        panic!("{count} entities must publish a path union, got {answer:?}");
    };
    (allocations, paths.len())
}

#[test]
fn recursive_paths_allocations_per_entity_stay_flat() {
    let sizes = [(4, 2), (8, 4), (16, 4)];
    let measured = sizes.map(|(count, groups)| {
        let (allocations, paths) = cold_paths_allocations(count, groups);
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
