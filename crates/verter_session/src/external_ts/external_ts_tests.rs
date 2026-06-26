//! Contract unit tests for the project-bound external-TS contract.
//!
//! These exercise the four project-resolution states through a REAL
//! `WorkspaceSnapshot` (built from an in-memory workspace via the production
//! `build_workspace_snapshot` path, so the TS-correct extension model and the
//! carrier-path conflict pass are exercised end-to-end), the
//! `provider_op_requires_resolved_project` witness chain, the carrier registry,
//! and the R21 `EnvDims` shape.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};

use super::*;

/// The directory containing a tsconfig path (its project root).
fn tsconfig_dir(tsconfig: &str) -> String {
    tsconfig
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_else(|| tsconfig.to_string())
}

/// Deterministic non-zero R21 env dims for a project (a stand-in for the host's
/// per-project env-hash reader). Non-zero so tests never rely on a forbidden
/// default/zero env identity; distinct per axis so the R21 shape is observable.
fn test_env_dims(_tsconfig_uri: &str) -> EnvDims {
    EnvDims {
        parse_env_hash: [1u8; 16],
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        project_identity: crate::file_artifact_store::ProjectIdentity([4u8; 16]),
    }
}

const WORKSPACE_ROOT: &str = "d:/ws";

/// The carrier extensions the live registry registers (`.vue`, `.svelte`, …),
/// WITHOUT a leading dot. Tests are adapter-parameterized over these — never a
/// hardcoded `"vue"`/`"svelte"`.
fn carrier_exts() -> Vec<String> {
    verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .map(|e| (*e).to_string())
        .collect()
}

/// Build a `MemoryWorkspace` with the given files (each `(path, content)`),
/// injected under the workspace root.
fn workspace_with(files: &[(&str, &str)]) -> MemoryWorkspace {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![WORKSPACE_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    for (path, content) in files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws
}

/// Build a real `WorkspaceSnapshot` from the in-memory workspace WITHOUT a disk
/// walk: parse each named tsconfig through the production parse +
/// supported-extension-expansion chain (`load_project_membership` +
/// `membership_to_spec`), build a configured `OwnershipProject` per tsconfig,
/// and assemble via `build_workspace_snapshot_simple`.
///
/// `build_workspace_snapshot` discovers tsconfigs with `walkdir` over the REAL
/// filesystem, which an in-memory workspace has no presence on; this helper
/// drives the same membership parse/expansion path hermetically.
fn snapshot_from_tsconfigs(ws: &MemoryWorkspace, tsconfigs: &[&str]) -> WorkspaceSnapshot {
    let mut projects = Vec::new();
    for (i, tsconfig) in tsconfigs.iter().enumerate() {
        let root = CanonicalPath::new(&tsconfig_dir(tsconfig));
        let raw_membership = load_project_membership(ws, tsconfig);
        let compiler_options = load_compiler_options(ws, tsconfig);
        let supported = supported_extensions_for(&compiler_options);
        let spec = membership_to_spec(&root, &raw_membership, &supported);
        let references = load_project_references(ws, tsconfig)
            .into_iter()
            .map(|r| CanonicalPath::new(&r))
            .collect();
        projects.push(OwnershipProject {
            id: ProjectId(i as u32),
            root: root.clone(),
            workspace_root: CanonicalPath::new(WORKSPACE_ROOT),
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(tsconfig),
                membership: ConfiguredMembership {
                    spec,
                    // Empty materialized set ⇒ `ConfiguredMembership::contains`
                    // uses the static spec (bridge mode), so the
                    // supported-extension-expanded include globs ARE the
                    // membership decision under test.
                    materialized_files: FxHashSet::default(),
                },
                compiler_options,
                references,
                workspace_aliases: Vec::new(),
            },
        });
    }
    build_workspace_snapshot_simple(projects, SnapshotGeneration(1))
}

// ── ProjectBinding / NoProject (the §2.6 extension rule, end-to-end) ──

#[test]
fn bare_star_include_resolves_carrier_source_to_project_binding() {
    let exts = carrier_exts();
    for ext in &exts {
        let ws = workspace_with(&[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
            (&format!("d:/ws/src/Foo.{ext}"), "<template></template>"),
        ]);
        let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
        let resolver = WorkspaceProjectResolver::new(
            &snap,
            &ws,
            "7.0.1",
            &(test_env_dims as fn(&str) -> EnvDims),
        );

        let res = resolver.resolve(&format!("d:/ws/src/Foo.{ext}"));
        match res {
            ProjectResolution::ProjectBinding(binding) => {
                assert_eq!(
                    binding.tsconfig_uri(),
                    "d:/ws/tsconfig.json",
                    "binding must carry the owning tsconfig (.{ext})"
                );
                assert_eq!(binding.workspace_root(), "d:/ws");
            }
            other => {
                panic!("bare-star include must own `Foo.{ext}` ⇒ ProjectBinding, got {other:?}")
            }
        }
    }
}

#[test]
fn extension_specific_include_does_not_own_carrier_source() {
    let exts = carrier_exts();
    for ext in &exts {
        let ws = workspace_with(&[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*.ts"] }"#),
            (&format!("d:/ws/src/Foo.{ext}"), "<template></template>"),
        ]);
        let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
        let resolver = WorkspaceProjectResolver::new(
            &snap,
            &ws,
            "7.0.1",
            &(test_env_dims as fn(&str) -> EnvDims),
        );

        assert_eq!(
            resolver.resolve(&format!("d:/ws/src/Foo.{ext}")),
            ProjectResolution::NoProject,
            "an extension-specific `*.ts` include must NOT own `Foo.{ext}` ⇒ NoProject"
        );
    }
}

#[test]
fn binding_carries_reference_graph_data() {
    // A project with `references` exposes them on the binding (reference-graph
    // awareness; the data is present even though live cross-program publish is
    // a later concern).
    let ws = workspace_with(&[
        (
            "d:/ws/tsconfig.json",
            r#"{ "include": ["src/**/*"], "references": [{ "path": "./packages/shared" }] }"#,
        ),
        (
            "d:/ws/packages/shared/tsconfig.json",
            r#"{ "include": ["src/**/*"] }"#,
        ),
        ("d:/ws/src/Foo.vue", "<template></template>"),
        ("d:/ws/packages/shared/src/index.ts", "export const x = 1;"),
    ]);
    let snap = snapshot_from_tsconfigs(
        &ws,
        &["d:/ws/tsconfig.json", "d:/ws/packages/shared/tsconfig.json"],
    );
    let resolver =
        WorkspaceProjectResolver::new(&snap, &ws, "7.0.1", &(test_env_dims as fn(&str) -> EnvDims));

    match resolver.resolve("d:/ws/src/Foo.vue") {
        ProjectResolution::ProjectBinding(binding) => {
            assert!(
                binding
                    .references()
                    .iter()
                    .any(|r| r.contains("packages/shared")),
                "the binding must carry the resolved project reference, got {:?}",
                binding.references()
            );
        }
        other => panic!("expected ProjectBinding, got {other:?}"),
    }
}

// ── Carrier-path conflict pass ⇒ Ambiguous (§2.2 / §2.6 step 4) ──

#[test]
fn real_file_at_carrier_path_downgrades_to_ambiguous() {
    // A real user file occupying the EXACT `{name}.{carrier}.tsx` carrier path ⇒
    // Verter never overlay-shadows it; the source is `Ambiguous`. Asserted for
    // every registered carrier (`.vue` AND `.svelte`).
    let exts = carrier_exts();
    for ext in &exts {
        let ws = workspace_with(&[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
            (&format!("d:/ws/src/Foo.{ext}"), "<template></template>"),
            // A REAL user file at the exact carrier-companion path.
            (
                &format!("d:/ws/src/Foo.{ext}.tsx"),
                "export const realUserFile = 1;",
            ),
        ]);
        let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
        let resolver = WorkspaceProjectResolver::new(
            &snap,
            &ws,
            "7.0.1",
            &(test_env_dims as fn(&str) -> EnvDims),
        );

        assert_eq!(
            resolver.resolve(&format!("d:/ws/src/Foo.{ext}")),
            ProjectResolution::Ambiguous(AmbiguityCause::CarrierPathOccupiedByRealFile),
            "a real file at `Foo.{ext}.tsx` must downgrade `Foo.{ext}` to Ambiguous \
             (Verter never overlay-shadows a real user file)"
        );
    }
}

#[test]
fn same_stem_svelte_rune_downgrades_to_ambiguous() {
    // A `Foo.svelte` component beside a real same-stem `Foo.svelte.ts` rune ⇒
    // DETECTED ambiguity (the rune the engine probes first shadows the bare
    // import). Fail closed, never a silently-wrong edge.
    if !carrier_exts().iter().any(|e| e == "svelte") {
        // The svelte adapter is a built-in; if it were ever removed this test
        // would no longer be meaningful. Assert its presence so the case is not
        // silently vacuous.
        panic!("the svelte carrier extension must be registered for this case");
    }
    let ws = workspace_with(&[
        ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
        ("d:/ws/src/Foo.svelte", "<script></script>"),
        // A REAL same-stem rune module beside the component.
        ("d:/ws/src/Foo.svelte.ts", "export const rune = 1;"),
    ]);
    let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
    let resolver =
        WorkspaceProjectResolver::new(&snap, &ws, "7.0.1", &(test_env_dims as fn(&str) -> EnvDims));

    assert_eq!(
        resolver.resolve("d:/ws/src/Foo.svelte"),
        ProjectResolution::Ambiguous(AmbiguityCause::SameStemRuneModule),
        "a same-stem `Foo.svelte.ts` rune beside `Foo.svelte` must fail closed (Ambiguous)"
    );
}

#[test]
fn carrier_with_no_conflict_resolves_cleanly() {
    // The NORMAL case: a carrier with no occupying file and no same-stem rune
    // resolves to a clean ProjectBinding (discriminates the conflict pass from a
    // blanket downgrade).
    let ws = workspace_with(&[
        ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
        ("d:/ws/src/Clean.svelte", "<script></script>"),
        // A DIFFERENT-stem rune module nearby must NOT trip the same-stem check.
        ("d:/ws/src/state.svelte.ts", "export const s = 1;"),
    ]);
    let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
    let resolver =
        WorkspaceProjectResolver::new(&snap, &ws, "7.0.1", &(test_env_dims as fn(&str) -> EnvDims));

    assert!(
        matches!(
            resolver.resolve("d:/ws/src/Clean.svelte"),
            ProjectResolution::ProjectBinding(_)
        ),
        "a carrier with no path conflict and only a DIFFERENT-stem rune nearby \
         must resolve cleanly"
    );
}

#[test]
fn real_file_at_jsx_carrier_path_downgrades_to_ambiguous() {
    // FIX 2: the carrier-companion path must be DERIVED from the
    // `VirtualFileNaming` descriptor, not hardcoded to `.tsx`. Vue's `ide`
    // policy is `JsxConditional { jsx: ".jsx", non_jsx: ".tsx" }`, so a real
    // user file at the JSX carrier path `Foo.vue.jsx` (the companion for a
    // `<script lang="jsx">` Vue SFC) is just as much a shadow hazard as
    // `Foo.vue.tsx`. The conflict pass must check ALL descriptor-valid IDE
    // carrier paths and downgrade to `Ambiguous`.
    //
    // DISCRIMINATING: against the hardcoded `.tsx`-only probe, `Foo.vue.jsx`
    // passes through to a ProjectBinding (no conflict detected) — the red.
    let ws = workspace_with(&[
        ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
        ("d:/ws/src/Foo.vue", "<template></template>"),
        // A REAL user file at the JSX carrier-companion path.
        ("d:/ws/src/Foo.vue.jsx", "export const realUserFile = 1;"),
    ]);
    let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
    let resolver =
        WorkspaceProjectResolver::new(&snap, &ws, "7.0.1", &(test_env_dims as fn(&str) -> EnvDims));

    assert_eq!(
        resolver.resolve("d:/ws/src/Foo.vue"),
        ProjectResolution::Ambiguous(AmbiguityCause::CarrierPathOccupiedByRealFile),
        "a real file at the descriptor-valid JSX carrier path `Foo.vue.jsx` must \
         downgrade `Foo.vue` to Ambiguous (the carrier path is descriptor-derived, \
         not hardcoded `.tsx`)"
    );
}

#[test]
fn tsx_carrier_path_still_detected_after_descriptor_derivation() {
    // FIX 2 regression guard: deriving from the descriptor must KEEP the `.tsx`
    // (non-JSX) carrier path detected for Vue, and the `.tsx` Svelte carrier.
    for ext in ["vue", "svelte"] {
        let source = format!("d:/ws/src/Foo.{ext}");
        let carrier = format!("d:/ws/src/Foo.{ext}.tsx");
        let ws = workspace_with(&[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
            (source.as_str(), "// carrier"),
            (carrier.as_str(), "export const realUserFile = 1;"),
        ]);
        let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
        let resolver = WorkspaceProjectResolver::new(
            &snap,
            &ws,
            "7.0.1",
            &(test_env_dims as fn(&str) -> EnvDims),
        );
        assert_eq!(
            resolver.resolve(&source),
            ProjectResolution::Ambiguous(AmbiguityCause::CarrierPathOccupiedByRealFile),
            "the `.tsx` carrier path for `.{ext}` must still be detected after \
             descriptor derivation"
        );
    }
}

#[test]
fn non_canonical_source_uri_still_detects_carrier_conflict() {
    // FIX 5: the conflict pass must normalize `source_uri` at its entry, like the
    // rest of `verter_workspace::resolver`. A non-canonical caller URI (uppercase
    // drive / backslashes) must NOT be able to bypass the disk-occupancy probe on
    // a case-insensitive FS. The real carrier file is stored at the CANONICAL
    // path; the probe is driven from a non-canonical `source_uri`.
    //
    // DISCRIMINATING: without normalization the conflict candidate is built from
    // the raw `D:/ws/...` / backslash URI, whose exact-key `file_exists` probe
    // misses the canonical-keyed store ⇒ conflict undetected ⇒ ProjectBinding
    // (the safety gate silently bypassed — the red).
    for raw_source in ["D:/ws/src/Foo.vue", r"d:\ws\src\Foo.vue"] {
        let ws = workspace_with(&[
            ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
            ("d:/ws/src/Foo.vue", "<template></template>"),
            // The real user file at the CANONICAL carrier path.
            ("d:/ws/src/Foo.vue.tsx", "export const realUserFile = 1;"),
        ]);
        let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
        let resolver = WorkspaceProjectResolver::new(
            &snap,
            &ws,
            "7.0.1",
            &(test_env_dims as fn(&str) -> EnvDims),
        );
        assert_eq!(
            resolver.resolve(raw_source),
            ProjectResolution::Ambiguous(AmbiguityCause::CarrierPathOccupiedByRealFile),
            "a non-canonical source_uri `{raw_source}` must still detect the carrier \
             conflict (the fail-closed gate cannot be bypassed by a non-canonical URI)"
        );
    }
}

// ── SyntheticScratch ──

#[test]
fn synthetic_scratch_carries_labelled_binding() {
    let res = ProjectResolution::synthetic_scratch("untitled:Buffer-1");
    match res {
        ProjectResolution::SyntheticScratch(scratch) => {
            assert_eq!(scratch.label(), "untitled:Buffer-1");
        }
        other => panic!("expected SyntheticScratch, got {other:?}"),
    }
}

// ── provider_op_requires_resolved_project: the witness chain ──

#[test]
fn project_binding_mints_ensure_project_request() {
    // A `ProjectBinding` is the SOLE source of an `EnsureProject` — the head of
    // the type-state chain that leads to the `BoundProject` witness.
    let ws = workspace_with(&[
        ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
        ("d:/ws/src/Foo.vue", "<template></template>"),
    ]);
    let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
    let resolver =
        WorkspaceProjectResolver::new(&snap, &ws, "7.0.1", &(test_env_dims as fn(&str) -> EnvDims));

    let binding = match resolver.resolve("d:/ws/src/Foo.vue") {
        ProjectResolution::ProjectBinding(b) => b,
        other => panic!("expected ProjectBinding, got {other:?}"),
    };

    let request = binding.ensure_project_request();
    assert_eq!(request.tsconfig_uri(), "d:/ws/tsconfig.json");
    assert_eq!(request.workspace_root(), "d:/ws");
    assert_eq!(request.ts_version(), "7.0.1");
    // The request carries the orthogonal env dims, not a bundled hash.
    let dims = request.env_dims();
    assert_eq!(dims.project_identity, request.project_identity());
}

#[test]
fn no_project_and_ambiguous_carry_no_binding() {
    // The fail-closed states hold NO `ProjectBinding`, so there is no way to mint
    // an `EnsureProject` (hence no production op) from them. This is the runtime
    // shadow of the compile-time guarantee; the static guard
    // `provider_op_requires_resolved_project` is the source-level backstop.
    let no_project = ProjectResolution::NoProject;
    let ambiguous = ProjectResolution::Ambiguous(AmbiguityCause::MultipleOwners);
    for state in [no_project, ambiguous] {
        assert!(
            !matches!(state, ProjectResolution::ProjectBinding(_)),
            "fail-closed states must not carry a ProjectBinding"
        );
    }
}

#[test]
fn bound_project_witness_round_trips_through_seal() {
    // The witness can only be built with a seal (minted inside the contract,
    // after a binding produced an EnsureProject). Build one to prove the
    // production ops can hang off it once a binding exists.
    let dims = EnvDims {
        parse_env_hash: [1u8; 16],
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        project_identity: crate::file_artifact_store::ProjectIdentity([4u8; 16]),
    };
    let witness = BoundProject::sealed(
        BoundProjectSeal::new(),
        Arc::<str>::from("d:/ws/tsconfig.json"),
        EngineCapabilities::default(),
        dims,
    );
    assert_eq!(witness.project(), "d:/ws/tsconfig.json");
    assert_eq!(witness.env_dims().parse_env_hash, [1u8; 16]);
    assert_eq!(witness.env_dims().resolve_env_hash, [2u8; 16]);
}

#[test]
fn bound_project_mints_from_ensure_project_request() {
    // The foreign-backend mint path: a real `EngineBackend` in another crate
    // obtains its `BoundProject` from `BoundProject::from_ensured(&EnsureProject,
    // caps)`. Because an `EnsureProject` is mintable ONLY from a resolved
    // `ProjectBinding`, this preserves `provider_op_requires_resolved_project`
    // without exposing the raw seal. The project URI + env dims are READ FROM the
    // request — the backend cannot substitute a different project.
    let ws = workspace_with(&[
        ("d:/ws/tsconfig.json", r#"{ "include": ["src/**/*"] }"#),
        ("d:/ws/src/Foo.vue", "<template></template>"),
    ]);
    let snap = snapshot_from_tsconfigs(&ws, &["d:/ws/tsconfig.json"]);
    let resolver =
        WorkspaceProjectResolver::new(&snap, &ws, "7.0.1", &(test_env_dims as fn(&str) -> EnvDims));
    let binding = match resolver.resolve("d:/ws/src/Foo.vue") {
        ProjectResolution::ProjectBinding(b) => b,
        other => panic!("expected ProjectBinding, got {other:?}"),
    };
    let request = binding.ensure_project_request();

    let witness = BoundProject::from_ensured(&request, EngineCapabilities::default());
    // The witness is bound to the SAME project + env dims the request carries.
    assert_eq!(witness.project(), request.tsconfig_uri());
    assert_eq!(witness.env_dims(), request.env_dims());
}

#[test]
fn scratch_witness_is_distinct_from_bound_project() {
    // The scratch witness is a DISTINCT type usable only for non-cross-file
    // features; it cannot be passed where a production op expects a BoundProject.
    let scratch = ScratchProject::sealed(ScratchProjectSeal::new(), Arc::<str>::from("untitled:1"));
    assert_eq!(scratch.label(), "untitled:1");
}

// ── Query carries the expected carrier identity (§2.1) ──

#[test]
fn query_carries_carrier_identity_and_it_is_part_of_identity() {
    // §2.1: "every query carries the expected carrier identity (content-hash +
    // source-map id) and fails closed on mismatch." Without `content_hash` /
    // `map_hash` on the `Query`, a backend cannot fail closed on stale carrier
    // content / source-map identity. The fields REUSE `SnapshotFile`'s `Hash16`
    // newtype — no parallel hash type. They are part of the query IDENTITY: two
    // queries differing ONLY in `content_hash` (or ONLY in `map_hash`) are not
    // equal.
    let base = Query {
        project: Arc::<str>::from("d:/ws/tsconfig.json"),
        provider_uri: Arc::<str>::from("d:/ws/src/Foo.vue.tsx"),
        carrier_offset: 12,
        feature: QueryFeature::Hover,
        content_hash: [5u8; 16],
        map_hash: [6u8; 16],
        required_version: 3,
    };

    // The carrier identity is readable off the query.
    assert_eq!(base.content_hash, [5u8; 16]);
    assert_eq!(base.map_hash, [6u8; 16]);

    // content_hash participates in identity.
    let other_content = Query {
        content_hash: [9u8; 16],
        ..base.clone()
    };
    assert_ne!(
        base, other_content,
        "a query differing only in content_hash must be a DISTINCT query identity"
    );

    // map_hash participates in identity.
    let other_map = Query {
        map_hash: [9u8; 16],
        ..base.clone()
    };
    assert_ne!(
        base, other_map,
        "a query differing only in map_hash must be a DISTINCT query identity"
    );
}

// ── EnvDims: R21 shape ──

#[test]
fn env_dims_carries_four_orthogonal_axes() {
    // R21: env dims are the orthogonal axes a value depends on — NEVER one
    // bundled hash. Distinct values per axis must be independently observable.
    let dims = EnvDims {
        parse_env_hash: [10u8; 16],
        resolve_env_hash: [20u8; 16],
        lib_env_hash: [30u8; 16],
        project_identity: crate::file_artifact_store::ProjectIdentity([40u8; 16]),
    };
    assert_ne!(dims.parse_env_hash, dims.resolve_env_hash);
    assert_ne!(dims.resolve_env_hash, dims.lib_env_hash);
    assert_ne!(dims.lib_env_hash, dims.project_identity.0);
}

// ── CarrierRegistry ──

#[test]
fn in_memory_carrier_registry_round_trips_artifact() {
    let mut registry = InMemoryCarrierRegistry::new();
    let artifact = CarrierArtifact {
        provider_uri: Arc::<str>::from("d:/ws/src/Foo.vue.tsx"),
        role: CarrierRole::CarrierIde,
        content: Arc::<str>::from("export default {} as any;"),
        content_hash: [7u8; 16],
        map_hash: [8u8; 16],
        version: 3,
    };
    registry.insert("d:/ws/src/Foo.vue", artifact.clone());

    assert_eq!(registry.carrier_for("d:/ws/src/Foo.vue"), Some(artifact));
    assert_eq!(
        registry.carrier_for("d:/ws/src/Missing.vue"),
        None,
        "an unknown source has no carrier"
    );
}

#[test]
fn carrier_role_has_provisional_batch_variant() {
    // CarrierBatch is present as a provisional variant alongside the four
    // existing roles (its keep-or-merge decision is downstream).
    let roles = [
        CarrierRole::CarrierIde,
        CarrierRole::CarrierApi,
        CarrierRole::CarrierBatch,
        CarrierRole::Shadow,
        CarrierRole::Real,
    ];
    // All five are distinct.
    for (i, a) in roles.iter().enumerate() {
        for (j, b) in roles.iter().enumerate() {
            assert_eq!(i == j, a == b, "role identity must be 1:1");
        }
    }
}
