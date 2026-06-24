//! The analysis-input config schema.
//!
//! Mirrors the TS loader's `verter.analysis-projects.v1` shape one-to-one (the two
//! sides stay in lockstep — the JSON authority is shared). The real filesystem
//! paths (`root`, `tsconfig`, `ambientDts`, `vueTscBin`, `checkerBin`) are the
//! corpus's private bytes: they live in PRIVATE fields, are NOT re-serialized, and
//! the hand-written [`Debug`]/[`Display`] print only the opaque id plus
//! `<redacted>`. Paths leave this type only through the narrow I/O accessors
//! ([`AnalysisProject::root`] et al.), never a blanket getter that would hand a
//! path straight to an emitter.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::id::ProjectId;

/// The schema discriminant every valid config carries.
pub const ANALYSIS_PROJECTS_SCHEMA: &str = "verter.analysis-projects.v1";

/// Which campaign workstream(s) a project participates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Workstream {
    /// DX / IDE comparison (workstream A).
    Ide,
    /// Type-check comparison (workstream B).
    Tsc,
    /// Build / compile-parity comparison (workstream C).
    Build,
}

/// The project shape: a Vite app, a Nuxt app, or a component library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    /// A plain Vite + Vue application.
    Vite,
    /// A Nuxt application (auto-import ambient `.d.ts` likely present).
    Nuxt,
    /// A component library.
    Lib,
}

/// The wire shape of one project, used ONLY transiently during deserialization.
/// It is never exposed; [`AnalysisProject`] is built from it so the public type can
/// hold the paths privately with no derived `Serialize`/`Debug`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisProjectWire {
    id: ProjectId,
    root: PathBuf,
    #[serde(default)]
    tsconfig: Option<PathBuf>,
    kind: ProjectKind,
    #[serde(default)]
    ambient_dts: Vec<PathBuf>,
    #[serde(default)]
    vue_tsc_bin: Option<PathBuf>,
    workstreams: Vec<Workstream>,
}

/// One analysis-input project.
///
/// The opaque [`ProjectId`], [`ProjectKind`], and [`Workstream`] set are public and
/// safe to emit. The filesystem paths are PRIVATE — reachable only through the
/// narrow accessors below, which exist for the I/O layer (open the tsconfig, hand
/// the ambient `.d.ts` to the checker), never to feed an emitter.
#[derive(Clone)]
pub struct AnalysisProject {
    id: ProjectId,
    kind: ProjectKind,
    workstreams: Vec<Workstream>,
    // --- private path bytes: not Serialize, not derived Debug ---
    root: PathBuf,
    tsconfig: Option<PathBuf>,
    ambient_dts: Vec<PathBuf>,
    vue_tsc_bin: Option<PathBuf>,
}

impl AnalysisProject {
    /// The opaque id — the ONLY identity safe to emit.
    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    /// The project kind.
    pub fn kind(&self) -> ProjectKind {
        self.kind
    }

    /// The workstreams this project participates in.
    pub fn workstreams(&self) -> &[Workstream] {
        &self.workstreams
    }

    /// The project root — a PRIVATE path, for the I/O layer only. Never pass the
    /// result to an emitter; redact through [`crate::Redactor`] first.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project's tsconfig path, if declared — PRIVATE, I/O only.
    pub fn tsconfig(&self) -> Option<&Path> {
        self.tsconfig.as_deref()
    }

    /// The generated ambient `.d.ts` paths — PRIVATE, fed to the checker only.
    pub fn ambient_dts(&self) -> &[PathBuf] {
        &self.ambient_dts
    }

    /// The project-pinned `vue-tsc` binary, if declared — PRIVATE, I/O only.
    pub fn vue_tsc_bin(&self) -> Option<&Path> {
        self.vue_tsc_bin.as_deref()
    }
}

impl From<AnalysisProjectWire> for AnalysisProject {
    fn from(w: AnalysisProjectWire) -> Self {
        AnalysisProject {
            id: w.id,
            kind: w.kind,
            workstreams: w.workstreams,
            root: w.root,
            tsconfig: w.tsconfig,
            ambient_dts: w.ambient_dts,
            vue_tsc_bin: w.vue_tsc_bin,
        }
    }
}

/// A redacted view: the opaque id, the kind, the workstreams, and a `<redacted>`
/// marker for every path — never a path byte. Used by both `Debug` and `Display`.
impl fmt::Debug for AnalysisProject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisProject")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("workstreams", &self.workstreams)
            .field("root", &"<redacted>")
            .field("tsconfig", &self.tsconfig.as_ref().map(|_| "<redacted>"))
            .field(
                "ambientDts",
                &format_args!("[{} <redacted>]", self.ambient_dts.len()),
            )
            .field("vueTscBin", &self.vue_tsc_bin.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl fmt::Display for AnalysisProject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?}, paths <redacted>)", self.id, self.kind)
    }
}

/// The wire shape of the whole config.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisProjectsWire {
    schema: String,
    #[serde(default)]
    checker_bin: Option<PathBuf>,
    projects: Vec<AnalysisProjectWire>,
}

/// The whole analysis-input config: the schema discriminant, an optional pinned
/// checker binary (PRIVATE path), and the projects.
#[derive(Clone)]
pub struct AnalysisProjects {
    schema: String,
    checker_bin: Option<PathBuf>,
    projects: Vec<AnalysisProject>,
}

impl AnalysisProjects {
    /// The schema discriminant string as read from the config.
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Whether the config's schema matches the expected discriminant.
    pub fn schema_matches(&self) -> bool {
        self.schema == ANALYSIS_PROJECTS_SCHEMA
    }

    /// The pinned checker binary path, if declared — PRIVATE, I/O only.
    pub fn checker_bin(&self) -> Option<&Path> {
        self.checker_bin.as_deref()
    }

    /// The projects.
    pub fn projects(&self) -> &[AnalysisProject] {
        &self.projects
    }

    /// Build directly from validated parts. Used by the parser.
    fn from_wire(wire: AnalysisProjectsWire) -> Self {
        AnalysisProjects {
            schema: wire.schema,
            checker_bin: wire.checker_bin,
            projects: wire.projects.into_iter().map(AnalysisProject::from).collect(),
        }
    }
}

/// The opaque-id → real-root pairs, for building a [`crate::Redactor`]. Returns the
/// PRIVATE roots, so this is consumed only by the redactor constructor, never an
/// emitter.
impl AnalysisProjects {
    pub(crate) fn id_root_pairs(&self) -> Vec<(ProjectId, PathBuf)> {
        self.projects
            .iter()
            .map(|p| (p.id.clone(), p.root.clone()))
            .collect()
    }
}

/// Same redaction discipline at the collection level.
impl fmt::Debug for AnalysisProjects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisProjects")
            .field("schema", &self.schema)
            .field("checkerBin", &self.checker_bin.as_ref().map(|_| "<redacted>"))
            .field("projects", &self.projects)
            .finish()
    }
}

/// Parse a config from an explicit JSON string. ALWAYS available (no feature, no
/// I/O): only env/default-FILE loading is gated. Validates opaque ids at the
/// deserialization gate.
pub fn parse_config(json: &str) -> Result<AnalysisProjects, crate::error::AnalysisInputError> {
    let wire: AnalysisProjectsWire = serde_json::from_str(json).map_err(|e| {
        // serde_json's error never embeds a filesystem path (it reports a JSON
        // position), but route through the path-free Parse reason regardless.
        crate::error::AnalysisInputError::Parse {
            reason: e.to_string(),
        }
    })?;
    Ok(AnalysisProjects::from_wire(wire))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
        "schema": "verter.analysis-projects.v1",
        "checkerBin": "/path/to/tsgo",
        "projects": [
            {
                "id": "p0001",
                "root": "/path/to/project",
                "tsconfig": "/path/to/project/tsconfig.json",
                "kind": "vite",
                "ambientDts": [],
                "vueTscBin": null,
                "workstreams": ["ide", "tsc", "build"]
            }
        ]
    }"#;

    #[test]
    fn parses_a_good_config_with_opaque_ids() {
        let cfg = parse_config(GOOD).expect("good config parses");
        assert!(cfg.schema_matches());
        assert_eq!(cfg.projects().len(), 1);
        assert_eq!(cfg.projects()[0].id().as_str(), "p0001");
        assert_eq!(cfg.projects()[0].kind(), ProjectKind::Vite);
        assert_eq!(cfg.projects()[0].workstreams(), &[
            Workstream::Ide,
            Workstream::Tsc,
            Workstream::Build
        ]);
    }

    #[test]
    fn rejects_a_config_with_a_descriptive_id() {
        let bad = GOOD.replace("\"p0001\"", "\"nexus-ui\"");
        let err = parse_config(&bad).expect_err("descriptive id rejected");
        // The parse-gate routes the id rejection through a path-free reason.
        assert!(!format!("{err}").contains("/path/to"));
    }

    #[test]
    fn debug_redacts_every_path() {
        let cfg = parse_config(GOOD).unwrap();
        let project_dbg = format!("{:?}", cfg.projects()[0]);
        let cfg_dbg = format!("{cfg:?}");
        for shown in [&project_dbg, &cfg_dbg] {
            assert!(shown.contains("<redacted>"), "expected redaction marker in {shown}");
            assert!(!shown.contains("/path/to"), "Debug leaked a path: {shown}");
            assert!(!shown.contains("tsgo"), "Debug leaked the checker bin: {shown}");
        }
        // The opaque id survives — it is the safe identity.
        assert!(project_dbg.contains("p0001"));
    }

    #[test]
    fn display_redacts_paths_keeps_opaque_id() {
        let cfg = parse_config(GOOD).unwrap();
        let shown = format!("{}", cfg.projects()[0]);
        assert!(shown.contains("p0001"));
        assert!(shown.contains("<redacted>"));
        assert!(!shown.contains("/path/to"));
    }
}
