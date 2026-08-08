//! The ONE standalone typeinfo host construction.
//!
//! Every consumer of the "standalone" host class builds it here: the
//! `typeinfo_tests` helper `make_host_with_footprint`, and the oracle
//! harness's source-side walk / reducer preflight. A second construction
//! would let the generator preflight a DIFFERENT program than the row's test
//! resolves, and a snapshot minted that way would pin the wrong answer.
//!
//! The host carries one configured project over the fixture root plus the
//! callable ambient corpus registered against it. The configured project is
//! what gives the ambient registry a stable key to attach to, and it is what
//! a callable's apparent-type lookup resolves the fixture canonicals to.
//! Its root is the fixture directory, so workspace fixtures rooted elsewhere
//! keep their existing project-less classification.

use std::sync::Arc;

use verter_workspace::{
    AmbientLibSpec, CanonicalPath, ConfiguredMembership, IdeProjectCompilerOptions, MemoryOptions,
    MemoryWorkspace, ProjectGraph, ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

use crate::types::HostConfig;
use crate::VerterHost;

/// The canonical the callable ambient corpus registers under. Deliberately
/// outside the fixture project root: an ambient lib is served through the
/// ambient registry, and a real file at the same canonical shadows it.
pub(crate) const AMBIENT_CALLABLE_LIB_ID: &str = "lib.callable.d.ts";

/// The vendored callable half of the TypeScript standard library.
pub(crate) const AMBIENT_CALLABLE_LIB: &str =
    include_str!("../typeinfo_tests/fixtures/ambient_callable_lib.d.ts");

/// The project root every standalone typeinfo host is configured at — the
/// directory the fixture canonicals live in.
pub(crate) const FIXTURE_PROJECT_ROOT: &str = "/fixtures";

/// Build the standalone typeinfo host: audit + footprint capture on, one
/// configured project over [`FIXTURE_PROJECT_ROOT`], and the callable ambient
/// corpus registered against that project.
pub(crate) fn standalone_footprint_host() -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    // The project's supported-extension set comes from the language registry,
    // so a newly registered carrier joins the fixture project without editing
    // a list here.
    let mut extensions: Vec<String> = vec![".ts".into(), ".tsx".into(), ".d.ts".into()];
    extensions.extend(
        verter_language::LanguageRegistry::global()
            .carrier_extensions()
            .into_iter()
            .map(|extension| format!(".{extension}")),
    );
    workspace.set_project_graph(ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: FIXTURE_PROJECT_ROOT.to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some(format!("{FIXTURE_PROJECT_ROOT}/tsconfig.json")),
        root_files: vec![],
        extensions,
        workspace_root: FIXTURE_PROJECT_ROOT.to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new(
            FIXTURE_PROJECT_ROOT,
        )),
    }]));
    workspace
        .register_ambient_lib(AmbientLibSpec {
            project_id: None,
            canonical_id: Arc::from(AMBIENT_CALLABLE_LIB_ID),
            source: Arc::from(AMBIENT_CALLABLE_LIB),
        })
        .expect("the callable ambient corpus MUST register against the fixture project");
    let access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        access,
    ))
}
