//! Typed request/response DTOs for the tsgo `--api` high-level ops.
//!
//! These mirror `typescript/dist/api/proto.generated.d.ts` (the request and
//! response shapes) and the literal `apiRequest(method, params)` call sites in
//! `dist/api/sync/api.js`. Field names are emitted exactly as the JS
//! `JSON.stringify(params)` would emit them (camelCase), so the JSON bytes this
//! module produces are byte-compatible with the official client's. Cited source
//! lines accompany each type so a version-update agent can re-verify against
//! the same reference.
//!
//! Payload encoding: the high-level ops carry their params/results as a UTF-8
//! JSON document inside the frame `payload` bin field (sync/client.js:54-63).

use serde::{Deserialize, Serialize};

/// Wire method-name strings, one per op, exactly as passed to
/// `apiRequest(...)` / `apiRequestBinary(...)` in `sync/api.js`.
///
/// The method names are NOT namespaced; the JS class grouping is client-side
/// only. Each constant cites the `sync/api.js` line of its call site.
///
/// # Ops deliberately deferred (follow-up)
///
/// TODO(follow-up): the LSP-feature ops — go-to-definition, find-references,
/// rename, and completion/completion-resolve — are served by the tsgo LSP
/// server mode, not this channel. "Definition" is reachable today by composing
/// [`GET_SYMBOL_AT_POSITION`] with the returned [`SymbolResponse::declarations`]
/// node-handle paths.
///
/// TODO(follow-up): the BINARY ops ([`GET_SOURCE_FILE`], plus `typeToTypeNode`
/// and `signatureToSignatureDeclaration`) return the columnar binary-AST blob
/// (PROTOCOL_VERSION 9, `node/protocol.d.ts:1`), not JSON. The codec
/// hand-writes the frame envelope for them, but the columnar-AST decoder is
/// deferred — no current consumer needs the AST blob.
pub mod method {
    /// `"initialize"` — once-only startup handshake (sync/api.js:174). Params `null`.
    pub const INITIALIZE: &str = "initialize";
    /// `"parseConfigFile"` (sync/api.js:255).
    pub const PARSE_CONFIG_FILE: &str = "parseConfigFile";
    /// `"createSnapshot"` — mint a new, independent snapshot from
    /// [`super::SnapshotChanges`] (sync/api.js:433).
    pub const CREATE_SNAPSHOT: &str = "createSnapshot";
    /// `"updateSnapshot"` — derive a NEW snapshot from an active base snapshot
    /// (`{ snapshot, changes }`, sync/api.js:461).
    pub const UPDATE_SNAPSHOT: &str = "updateSnapshot";
    /// `"getCurrentLanguageServerSnapshot"` — the language server's current
    /// canonical snapshot, after atomically adopting API-driven changes. Only
    /// available on an LSP-attached `--api` session (sync/api.js:547).
    pub const GET_CURRENT_LANGUAGE_SERVER_SNAPSHOT: &str = "getCurrentLanguageServerSnapshot";
    /// `"release"` — release a snapshot handle (sync/api.js:1026).
    pub const RELEASE: &str = "release";
    /// `"getDefaultProjectForFile"` (sync/api.js:985).
    pub const GET_DEFAULT_PROJECT_FOR_FILE: &str = "getDefaultProjectForFile";
    /// `"getSemanticDiagnostics"` (sync/api.js:2341).
    pub const GET_SEMANTIC_DIAGNOSTICS: &str = "getSemanticDiagnostics";
    /// `"getSyntacticDiagnostics"` (sync/api.js:2285).
    pub const GET_SYNTACTIC_DIAGNOSTICS: &str = "getSyntacticDiagnostics";
    /// `"getSuggestionDiagnostics"` (sync/api.js:2369).
    pub const GET_SUGGESTION_DIAGNOSTICS: &str = "getSuggestionDiagnostics";
    /// `"getConfigFileParsingDiagnostics"` — the project's config-file parse /
    /// compiler-options diagnostics (e.g. an invalid `target`, a bad option
    /// value). These are program-wide diagnostics NOT covered by the per-file
    /// semantic/syntactic getters (sync/api.js:2459).
    pub const GET_CONFIG_FILE_PARSING_DIAGNOSTICS: &str = "getConfigFileParsingDiagnostics";
    /// `"getTypeAtPosition"` (sync/api.js:3120).
    pub const GET_TYPE_AT_POSITION: &str = "getTypeAtPosition";
    /// `"getSymbolAtPosition"` (sync/api.js:2864).
    pub const GET_SYMBOL_AT_POSITION: &str = "getSymbolAtPosition";
    /// `"typeToString"` (sync/api.js:3629).
    pub const TYPE_TO_STRING: &str = "typeToString";
    /// `"echo"` — diagnostic round-trip (sync/client.js:105). String payload.
    pub const ECHO: &str = "echo";
    /// `"getSourceFile"` — BINARY op (`apiRequestBinary`, sync/api.js:1966).
    /// The response payload is the columnar binary AST, not JSON; this crate's
    /// typed decode for it is a deliberate follow-up (see crate `proto` docs).
    pub const GET_SOURCE_FILE: &str = "getSourceFile";
}

// ── DocumentIdentifier (proto.generated.d.ts:209-211) ───────────────────────
/// A document identifier: a file-name string, or `{ uri }`. Mirrors
/// `DocumentIdentifier` (proto.generated.d.ts:209-211). On the wire it is almost
/// always a plain string; we accept both forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DocumentIdentifier {
    /// A plain file-name path string.
    FileName(String),
    /// A `{ uri }` object form.
    Uri {
        /// The document URI.
        uri: String,
    },
}

impl DocumentIdentifier {
    /// Construct a file-name identifier.
    pub fn file_name(path: impl Into<String>) -> Self {
        DocumentIdentifier::FileName(path.into())
    }
}

// ── FileNotifications (proto.generated.d.ts:1076-1081) ──────────────────────
/// Host file-system changes the engine must re-read while creating a snapshot:
/// either `invalidate_all` (discard every cached file) or per-document
/// `changed`/`created`/`deleted` lists. Mirrors `FileNotifications`
/// (proto.generated.d.ts:1076-1081). Absent fields are omitted from the JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileNotifications {
    /// Discard every cached file.
    #[serde(rename = "invalidateAll", skip_serializing_if = "Option::is_none")]
    pub invalidate_all: Option<bool>,
    /// Documents whose content changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<Vec<DocumentIdentifier>>,
    /// Documents newly created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<Vec<DocumentIdentifier>>,
    /// Documents deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Vec<DocumentIdentifier>>,
}

// ── Snapshot changes (proto.generated.d.ts:283-296, 1036-1069) ──────────────
/// The project/file changes a snapshot request applies: the body of
/// `createSnapshot`, the `changes` of `updateSnapshot`, and (without
/// `file_notifications`) the `changes` of `getCurrentLanguageServerSnapshot`.
/// Mirrors the subset of `CreateSnapshotParams` /
/// `SnapshotRequestChangesParams` Verter sends (proto.generated.d.ts:283-296,
/// 1036-1069).
///
/// Opens are REF-COUNTED and PERSIST across snapshots until explicitly closed
/// (proto.generated.d.ts:1037-1057); a field omitted (`None`) never rides the
/// wire, so a persisted open is not re-leased. Every field is
/// `skip_serializing_if = "Option::is_none"` to match the JS optional-omit
/// behavior.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotChanges {
    /// tsconfig.json files to open in the new snapshot. Ref-counted: the open
    /// persists until a matching close.
    #[serde(rename = "openProjects", skip_serializing_if = "Option::is_none")]
    pub open_projects: Option<Vec<DocumentIdentifier>>,
    /// tsconfig.json files to close (balances a prior open's refcount).
    #[serde(rename = "closeProjects", skip_serializing_if = "Option::is_none")]
    pub close_projects: Option<Vec<DocumentIdentifier>>,
    /// Files to open in the new snapshot. Ref-counted like `open_projects`.
    #[serde(rename = "openFiles", skip_serializing_if = "Option::is_none")]
    pub open_files: Option<Vec<DocumentIdentifier>>,
    /// Files to close (balances a prior open's refcount).
    #[serde(rename = "closeFiles", skip_serializing_if = "Option::is_none")]
    pub close_files: Option<Vec<DocumentIdentifier>>,
    /// Host file-system changes to re-read while creating the snapshot.
    #[serde(rename = "fileNotifications", skip_serializing_if = "Option::is_none")]
    pub file_notifications: Option<FileNotifications>,
    /// Projects whose programs must be brought up to date in the new snapshot.
    /// A snapshot derived from a base (`updateSnapshot`,
    /// `getCurrentLanguageServerSnapshot`) leaves a changed project `dirty`
    /// otherwise, and the engine answers a dirty project's checker requests
    /// from the STALE program (measured: an `updateSnapshot` notifying a changed
    /// file without it still reports the old file's empty diagnostics).
    #[serde(rename = "ensurePrograms", skip_serializing_if = "Option::is_none")]
    pub ensure_programs: Option<EnsurePrograms>,
}

/// Which projects' programs a snapshot request must bring up to date. Mirrors
/// `EnsurePrograms = true | readonly ProjectId[]` (proto.generated.d.ts:213).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsurePrograms {
    /// Every project in the snapshot (`true` on the wire).
    All,
    /// The listed project ids.
    Projects(Vec<String>),
}

impl Serialize for EnsurePrograms {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_bool(true),
            Self::Projects(ids) => ids.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for EnsurePrograms {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Flag(bool),
            Projects(Vec<String>),
        }
        match Wire::deserialize(deserializer)? {
            Wire::Flag(true) => Ok(Self::All),
            Wire::Flag(false) => Err(serde::de::Error::custom(
                "ensurePrograms is `true` or a project-id list, never `false`",
            )),
            Wire::Projects(ids) => Ok(Self::Projects(ids)),
        }
    }
}

impl SnapshotChanges {
    /// Ergonomic single-project open: `open_projects: [p]` with every other
    /// field left `None`. `p` is already the resolved file-name form (Verter
    /// threads normalized absolute tsconfig paths).
    #[must_use]
    pub fn single_project(p: impl Into<String>) -> Self {
        Self {
            open_projects: Some(vec![DocumentIdentifier::file_name(p)]),
            ..Default::default()
        }
    }
}

/// Params for `updateSnapshot`: the ACTIVE base snapshot the new one derives
/// from, plus the changes to apply. Mirrors `UpdateSnapshotParams`
/// (proto.generated.d.ts:311-314; sync/api.js:461-464). A released or
/// superseded-and-released base is refused by the engine (`snapshot N not
/// found`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSnapshotParams {
    /// The active base snapshot handle.
    pub snapshot: OpaqueHandle,
    /// The changes to apply on top of the base.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<SnapshotChanges>,
}

/// Params for `getCurrentLanguageServerSnapshot`. Mirrors
/// `GetCurrentLanguageServerSnapshotParams` (proto.generated.d.ts:315-318;
/// sync/api.js:547-550). Without a `base_snapshot` the response carries EVERY
/// project; with one, only the projects added or replaced relative to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageServerSnapshotParams {
    /// The response base (an active snapshot), if any.
    #[serde(rename = "baseSnapshot", skip_serializing_if = "Option::is_none")]
    pub base_snapshot: Option<OpaqueHandle>,
    /// API-driven changes to adopt into the language server's canonical state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<SnapshotChanges>,
}

// ── Response DTOs (proto.generated.d.ts) ─────────────────────────────────────
/// Response from `initialize`. Mirrors `InitializeResponse`
/// (proto.generated.d.ts:276-281).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResponse {
    /// How the host file system compares paths: `0` insensitive, `1` sensitive
    /// (`CaseSensitivity`, enums/caseSensitivity.enum.d.ts).
    #[serde(rename = "caseSensitivity")]
    pub case_sensitivity: u8,
    /// The server's current working directory.
    #[serde(rename = "currentDirectory")]
    pub current_directory: String,
}

impl InitializeResponse {
    /// Whether the host file system compares paths case-sensitively.
    #[must_use]
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitivity == 1
    }
}

/// A project's parsed configuration. Mirrors the load-bearing fields of
/// `ConfigFileResponse` (proto.generated.d.ts:372-381); the rest is ignored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParsedCommandLine {
    /// The program's root file names.
    #[serde(rename = "fileNames", default)]
    pub file_names: Vec<String>,
    /// The resolved compiler options.
    #[serde(default)]
    pub options: serde_json::Map<String, serde_json::Value>,
    /// The config parse errors.
    #[serde(default)]
    pub errors: Vec<Diagnostic>,
}

/// A project entry within a snapshot. Mirrors `ProjectResponse`
/// (proto.generated.d.ts:423-429).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectResponse {
    /// Project handle: the canonical (path-key) config path for a configured
    /// project.
    pub id: String,
    /// The tsconfig.json path backing this project (absent for an inferred or
    /// synthetic project).
    #[serde(
        rename = "configFileName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub config_file_name: Option<String>,
    /// The project's current directory.
    #[serde(rename = "currentDirectory", default)]
    pub current_directory: String,
    /// Whether the project's program is stale relative to the snapshot.
    #[serde(default)]
    pub dirty: bool,
    /// The project's parsed configuration (root files, options, errors).
    #[serde(rename = "parsedCommandLine")]
    pub parsed_command_line: ParsedCommandLine,
}

impl ProjectResponse {
    /// The root files of the project's program
    /// (`parsedCommandLine.fileNames`).
    #[must_use]
    pub fn root_files(&self) -> &[String] {
        &self.parsed_command_line.file_names
    }

    /// The resolved compiler options (`parsedCommandLine.options`).
    #[must_use]
    pub fn compiler_options(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.parsed_command_line.options
    }

    /// Whether this project's `configFileName` satisfies `predicate` (an
    /// inferred or synthetic project, which has none, never does).
    pub fn config_matches(&self, predicate: impl Fn(&str) -> bool) -> bool {
        self.config_file_name.as_deref().is_some_and(predicate)
    }
}

/// An opaque engine-issued numeric handle.
///
/// The tsgo `--api` engine issues the whole opaque-numeric-handle class — the
/// snapshot handle, the type id, the symbol id — as a bare JSON INTEGER
/// (`"snapshot":1`, `"id":14`; the shipped `proto.generated.d.ts` types these
/// as `number`). Symbol ids use the high bits (e.g. `281474976710661`), which
/// still fit `i64`. The codec carries every such handle through this one
/// shared [`i64`] newtype.
///
/// `#[serde(transparent)]` makes the wire shape exactly the inner integer in
/// both directions: a JSON integer decodes straight into [`OpaqueHandle`], and
/// embedding the handle in `serde_json::json!({ "snapshot": handle, .. })`
/// re-serializes it as the same bare integer — never coerced to a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpaqueHandle(pub i64);

/// Response from `createSnapshot`, `updateSnapshot` and
/// `getCurrentLanguageServerSnapshot`. Mirrors `CreateSnapshotResponse`
/// (proto.generated.d.ts:298-310).
///
/// `projects` carries EVERY project when the request named no response base
/// (`createSnapshot`, a base-less `getCurrentLanguageServerSnapshot`), and only
/// the projects added or replaced relative to the base otherwise
/// (`updateSnapshot`) — an unchanged project is carried over from the base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotResponse {
    /// Opaque handle for the new snapshot — a bare integer (`1`); see
    /// [`OpaqueHandle`].
    pub snapshot: OpaqueHandle,
    /// Projects in (or changed by) the snapshot.
    pub projects: Vec<ProjectResponse>,
    /// Source-file differences from the response base, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<serde_json::Value>,
    /// Results correlated with the request that produced the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<serde_json::Value>,
}

/// A symbol's identity and server-resolvable owner. Mirrors `SymbolReference`
/// (proto.generated.d.ts:1010-1012, 1204-1209).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolReference {
    /// Opaque symbol handle (a bare integer; see [`OpaqueHandle`]).
    pub id: OpaqueHandle,
    /// The owner kind: `0` file-owned, `1` snapshot-owned (`SymbolOwnerKind`,
    /// enums/symbolOwnerKind.enum.d.ts).
    pub kind: u8,
    /// The owning file's descriptor (file-owned symbols), kept raw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<serde_json::Value>,
    /// The owning snapshot (snapshot-owned symbols).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<OpaqueHandle>,
    /// The owning project (snapshot-owned symbols).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// A symbol reflection. Mirrors `SymbolResponse` (proto.generated.d.ts:256-265).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolResponse {
    /// The symbol's identity and owner.
    pub reference: SymbolReference,
    /// Symbol name.
    pub name: String,
    /// `SymbolFlags` bitfield.
    pub flags: u32,
    /// Internal check flags bitfield.
    #[serde(rename = "checkFlags")]
    pub check_flags: u32,
    /// Declaration node-handle strings (carry source paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declarations: Option<Vec<String>>,
    /// The value declaration node handle, when present.
    #[serde(
        rename = "valueDeclaration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub value_declaration: Option<String>,
}

/// A symbol reference embedded in another response, without its owning file's
/// full descriptor. Mirrors `CompactSymbolReference`
/// (proto.generated.d.ts:1019-1022).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactSymbolReference {
    /// Opaque symbol handle (a bare integer; see [`OpaqueHandle`]).
    pub id: OpaqueHandle,
    /// The owning source file's node id, or absent for a snapshot-owned symbol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// A type reflection. Mirrors `TypeResponse` (proto.generated.d.ts:467-518).
/// Only the load-bearing fields are modeled strongly; the many optional
/// structural fields are ignored on decode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeResponse {
    /// Opaque type handle (a bare integer; see [`OpaqueHandle`]).
    pub id: OpaqueHandle,
    /// `TypeFlags` bitfield.
    pub flags: u32,
    /// `ObjectFlags` bitfield, when present.
    #[serde(
        rename = "objectFlags",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub object_flags: Option<u32>,
    /// Literal value, when the type is a literal (`null` otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// The intrinsic name of an intrinsic type (`string`, `number`, …).
    #[serde(
        rename = "intrinsicName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub intrinsic_name: Option<String>,
    /// The symbol associated with a structured type, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<CompactSymbolReference>,
}

/// One diagnostic. Mirrors the load-bearing fields of `DiagnosticResponse`
/// (proto.generated.d.ts:909-940): `{ code, category, text, pos, end,
/// fileName?, ... }`; the rendering helpers (`startPosition`, `sourceLines`,
/// `messageChain`, …) are ignored on decode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The TS diagnostic code (e.g. 2345).
    pub code: u32,
    /// The diagnostic category (0=warning,1=error,2=suggestion,3=message per
    /// `DiagnosticCategory`).
    pub category: u32,
    /// The human-readable message text.
    pub text: String,
    /// Start offset within the file (`0` for a location-less diagnostic; see
    /// [`deserialize_offset`]).
    #[serde(deserialize_with = "deserialize_offset")]
    pub pos: u32,
    /// End offset within the file (`0` for a location-less diagnostic).
    #[serde(deserialize_with = "deserialize_offset")]
    pub end: u32,
    /// The file the diagnostic belongs to, when present.
    #[serde(rename = "fileName", default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// Decode a diagnostic offset. The engine reports `-1` for a diagnostic that
/// has no location at all — a project-level diagnostic with no `fileName`,
/// e.g. TS18003 "No inputs were found in config file" — and a non-negative
/// UTF-16 offset otherwise. A location-less diagnostic has no offset to map,
/// so `-1` decodes as `0`, the start a file-less diagnostic is placed at;
/// any other negative value is a wire divergence and is refused.
fn deserialize_offset<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let raw = i64::deserialize(deserializer)?;
    match raw {
        -1 => Ok(0),
        n => u32::try_from(n).map_err(|_| {
            serde::de::Error::custom(format!("diagnostic offset {n} is outside 0..=u32::MAX"))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The request param JSON must be byte-identical to JS `JSON.stringify`.
    // serde_json emits keys in struct-declaration order and omits None fields,
    // which matches the JS object-literal field order at each call site.

    #[test]
    fn create_snapshot_open_projects_json_matches_js_shape() {
        let p = SnapshotChanges::single_project("/repo/tsconfig.json");
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"openProjects":["/repo/tsconfig.json"]}"#
        );
    }

    #[test]
    fn snapshot_changes_file_notifications_json_matches_js_shape() {
        let p = SnapshotChanges {
            open_projects: Some(vec![DocumentIdentifier::file_name("/repo/tsconfig.json")]),
            file_notifications: Some(FileNotifications {
                changed: Some(vec![DocumentIdentifier::file_name("/repo/src/a.ts")]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"openProjects":["/repo/tsconfig.json"],"fileNotifications":{"changed":["/repo/src/a.ts"]}}"#
        );
    }

    #[test]
    fn ensure_programs_serializes_true_or_the_project_list() {
        let all = SnapshotChanges {
            ensure_programs: Some(EnsurePrograms::All),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&all).unwrap(),
            r#"{"ensurePrograms":true}"#
        );
        let some = SnapshotChanges {
            ensure_programs: Some(EnsurePrograms::Projects(vec!["/repo/tsconfig.json".into()])),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&some).unwrap(),
            r#"{"ensurePrograms":["/repo/tsconfig.json"]}"#
        );
        let back: SnapshotChanges = serde_json::from_str(r#"{"ensurePrograms":true}"#).unwrap();
        assert_eq!(back.ensure_programs, Some(EnsurePrograms::All));
        assert!(serde_json::from_str::<SnapshotChanges>(r#"{"ensurePrograms":false}"#).is_err());
    }

    #[test]
    fn file_notifications_invalidate_all_json() {
        let fc = FileNotifications {
            invalidate_all: Some(true),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&fc).unwrap(),
            r#"{"invalidateAll":true}"#
        );
    }

    // ── DISCRIMINATING: `updateSnapshot` names its ACTIVE base and nests the
    //    changes (sync/api.js:461-464). The pre-7.1 flat shape
    //    (`{"openProjects":[…]}` with no base) is refused by the engine with
    //    `snapshot 0 not found`, so the base must always ride. ───────────────
    #[test]
    fn update_snapshot_params_name_the_base_and_nest_the_changes() {
        let p = UpdateSnapshotParams {
            snapshot: OpaqueHandle(3),
            changes: Some(SnapshotChanges {
                file_notifications: Some(FileNotifications {
                    changed: Some(vec![DocumentIdentifier::file_name("/repo/src/a.ts")]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"snapshot":3,"changes":{"fileNotifications":{"changed":["/repo/src/a.ts"]}}}"#
        );
    }

    #[test]
    fn language_server_snapshot_params_json_matches_js_shape() {
        let p = LanguageServerSnapshotParams {
            base_snapshot: None,
            changes: Some(SnapshotChanges::single_project("/repo/tsconfig.json")),
        };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"changes":{"openProjects":["/repo/tsconfig.json"]}}"#
        );
        let empty = LanguageServerSnapshotParams::default();
        assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");
    }

    #[test]
    fn document_identifier_serializes_untagged() {
        assert_eq!(
            serde_json::to_string(&DocumentIdentifier::file_name("/x.ts")).unwrap(),
            r#""/x.ts""#,
            "file-name form is a bare JSON string"
        );
        assert_eq!(
            serde_json::to_string(&DocumentIdentifier::Uri {
                uri: "file:///x.ts".to_string()
            })
            .unwrap(),
            r#"{"uri":"file:///x.ts"}"#
        );
    }

    // ── response decode mirrors proto.generated.d.ts (camelCase wire keys) ──
    #[test]
    fn initialize_response_decodes_case_sensitivity() {
        let json = r#"{"caseSensitivity":1,"currentDirectory":"/repo"}"#;
        let r: InitializeResponse = serde_json::from_str(json).unwrap();
        assert!(r.case_sensitive());
        assert_eq!(r.current_directory, "/repo");
        let insensitive: InitializeResponse =
            serde_json::from_str(r#"{"caseSensitivity":0,"currentDirectory":"C:/repo"}"#).unwrap();
        assert!(!insensitive.case_sensitive());
    }

    // ── DISCRIMINATING: a project carries its root files and options under
    //    `parsedCommandLine` (proto.generated.d.ts:423-429, 372-381). ──────────
    #[test]
    fn snapshot_response_decodes_projects_with_parsed_command_line() {
        let json = r#"{
            "snapshot":1,
            "projects":[{
                "id":"/repo/tsconfig.json",
                "configFileName":"/repo/tsconfig.json",
                "currentDirectory":"/repo",
                "dirty":false,
                "parsedCommandLine":{
                    "fileNames":["/repo/src/a.ts"],
                    "options":{"strict":true},
                    "errors":[]
                }
            }],
            "operation":{}
        }"#;
        let r: SnapshotResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.snapshot, OpaqueHandle(1));
        assert_eq!(r.projects.len(), 1);
        let project = &r.projects[0];
        assert_eq!(
            project.config_file_name.as_deref(),
            Some("/repo/tsconfig.json")
        );
        assert!(project.config_matches(|c| c == "/repo/tsconfig.json"));
        assert_eq!(project.root_files(), ["/repo/src/a.ts".to_string()]);
        assert_eq!(
            project.compiler_options()["strict"],
            serde_json::json!(true)
        );
        assert!(
            r.changes.is_none(),
            "a base-less snapshot has no changes field"
        );
    }

    // ── DISCRIMINATING: an inferred project (no configFileName) never matches
    //    a config predicate. ─────────────────────────────────────────────────
    #[test]
    fn inferred_project_never_matches_a_config() {
        let json = r#"{"id":"/dev/null/inferred","currentDirectory":"/repo","dirty":false,
            "parsedCommandLine":{"fileNames":[],"options":{},"errors":[]}}"#;
        let p: ProjectResponse = serde_json::from_str(json).unwrap();
        assert!(!p.config_matches(|_| true));
    }

    // ── DISCRIMINATING: the snapshot handle decodes from a JSON INTEGER into
    //    `OpaqueHandle`, and RE-SERIALIZES faithfully as an integer; the
    //    path-bearing project id stays a STRING. ────────────────────────────────
    #[test]
    fn snapshot_handle_decodes_integer_and_reserializes_as_integer() {
        let int_json = r#"{
            "snapshot":1,
            "projects":[{
                "id":"c:/repo/tsconfig.json",
                "configFileName":"c:/repo/tsconfig.json",
                "currentDirectory":"c:/repo",
                "dirty":false,
                "parsedCommandLine":{"fileNames":["c:/repo/src/a.ts"],"options":{},"errors":[]}
            }]
        }"#;
        let resp: SnapshotResponse =
            serde_json::from_str(int_json).expect("integer snapshot handle must decode");
        assert_eq!(resp.snapshot, OpaqueHandle(1));

        let snap_wire = serde_json::to_value(resp.snapshot).unwrap();
        assert_eq!(
            snap_wire,
            serde_json::json!(1),
            "an integer handle must re-serialize as a JSON integer"
        );
        assert!(
            snap_wire.is_number() && !snap_wire.is_string(),
            "the re-serialized handle is a JSON integer, NOT a string: {snap_wire}"
        );
        assert_eq!(resp.projects[0].id, "c:/repo/tsconfig.json");

        let req = serde_json::json!({ "snapshot": resp.snapshot, "project": "p.x" });
        assert_eq!(req["snapshot"], serde_json::json!(1));
        assert!(req["snapshot"].is_number());
    }

    // ── DISCRIMINATING: a STRING snapshot handle must NOT decode — the codec is
    //    integer-handle only, with no int-or-string leniency. ───────────────────
    #[test]
    fn string_snapshot_handle_does_not_decode() {
        let str_json = r#"{"snapshot":"n0000000000000001","projects":[]}"#;
        assert!(
            serde_json::from_str::<SnapshotResponse>(str_json).is_err(),
            "a string snapshot handle must be rejected — the wire is integer-only"
        );
    }

    // ── DISCRIMINATING: `SymbolResponse` carries its identity in `reference`
    //    (a high-bit integer id plus the owner), while the path-bearing
    //    declaration handles stay STRINGS. Shape recorded from the live engine. ─
    #[test]
    fn symbol_response_decodes_reference_and_path_string_declarations() {
        let json = r#"{"reference":{"kind":0,"file":{"fileName":"C:/x/main.ts","path":"c:/x/main.ts","contentHash":"96e8","parseOptionsKey":"0","scriptKind":3,"nodeId":"106"},"id":281474976710661},"name":"u","flags":2097152,"checkFlags":0,"declarations":["7.278.c:/x/main.ts"]}"#;
        let s: SymbolResponse = serde_json::from_str(json).expect("symbol reference must decode");
        assert_eq!(s.reference.id, OpaqueHandle(281_474_976_710_661));
        assert_eq!(s.reference.kind, 0);
        assert!(s.reference.file.is_some());
        assert_eq!(s.name, "u");
        let id_wire = serde_json::to_value(s.reference.id).unwrap();
        assert!(
            id_wire.is_number() && !id_wire.is_string(),
            "the symbol id re-serializes as a JSON integer, NOT a string: {id_wire}"
        );
        assert_eq!(
            s.declarations.as_deref(),
            Some(["7.278.c:/x/main.ts".to_string()].as_slice())
        );
        assert_eq!(s.value_declaration, None);
    }

    // ── DISCRIMINATING: `TypeResponse` decodes the live shape — integer `id`,
    //    `value: null`, the intrinsic name — and a structured type's compact
    //    symbol reference. ────────────────────────────────────────────────────
    #[test]
    fn type_response_decodes_integer_id_and_compact_symbol() {
        let json = r#"{"id":14,"flags":32,"objectFlags":0,"isTupleType":false,"value":null,"isThisType":false,"intrinsicName":"string"}"#;
        let t: TypeResponse = serde_json::from_str(json).expect("live TypeResponse must decode");
        assert_eq!(t.id, OpaqueHandle(14));
        assert_eq!(t.flags, 32);
        assert_eq!(t.object_flags, Some(0));
        assert_eq!(t.intrinsic_name.as_deref(), Some("string"));
        assert_eq!(t.symbol, None);

        let structured = r#"{"id":86,"flags":524288,"objectFlags":16,"value":null,"symbol":{"id":7,"file":"106"}}"#;
        let t: TypeResponse = serde_json::from_str(structured).unwrap();
        let symbol = t.symbol.expect("structured types carry their symbol");
        assert_eq!(symbol.id, OpaqueHandle(7));
        assert_eq!(symbol.file.as_deref(), Some("106"));
        let id_wire = serde_json::to_value(t.id).unwrap();
        assert_eq!(id_wire, serde_json::json!(86));
    }

    // ── DISCRIMINATING: a location-less project diagnostic carries `-1`
    //    offsets and no `fileName` (recorded from the engine: TS18003); it
    //    decodes at offset 0, while any other negative offset is refused. ──────
    #[test]
    fn location_less_diagnostic_decodes_at_offset_zero() {
        let json = r#"{"pos":-1,"end":-1,"code":18003,"category":1,"text":"No inputs were found in config file"}"#;
        let d: Diagnostic = serde_json::from_str(json).unwrap();
        assert_eq!((d.pos, d.end), (0, 0));
        assert_eq!(d.file_name, None);
        let diverged = r#"{"pos":-2,"end":0,"code":1,"category":1,"text":"x"}"#;
        assert!(serde_json::from_str::<Diagnostic>(diverged).is_err());
    }

    #[test]
    fn diagnostic_decodes_load_bearing_fields() {
        let json = r#"{"fileName":"/repo/src/a.ts","pos":120,"end":123,"startPosition":{"line":1,"character":6},"sourceLines":[{"line":1,"text":"x"}],"code":2345,"category":1,"text":"Argument not assignable"}"#;
        let d: Diagnostic = serde_json::from_str(json).unwrap();
        assert_eq!(d.code, 2345);
        assert_eq!(d.category, 1);
        assert_eq!(d.pos, 120);
        assert_eq!(d.end, 123);
        assert_eq!(d.file_name.as_deref(), Some("/repo/src/a.ts"));
    }

    // ── DISCRIMINATING: a wrong wire key must NOT silently decode ───────────
    #[test]
    fn initialize_response_rejects_snake_case_keys() {
        let json = r#"{"case_sensitivity":1,"current_directory":"/repo"}"#;
        assert!(
            serde_json::from_str::<InitializeResponse>(json).is_err(),
            "snake_case keys must not decode — the wire is camelCase"
        );
    }

    #[test]
    fn method_name_strings_are_exact() {
        assert_eq!(method::CREATE_SNAPSHOT, "createSnapshot");
        assert_eq!(method::UPDATE_SNAPSHOT, "updateSnapshot");
        assert_eq!(
            method::GET_CURRENT_LANGUAGE_SERVER_SNAPSHOT,
            "getCurrentLanguageServerSnapshot"
        );
        assert_eq!(method::RELEASE, "release");
        assert_eq!(method::GET_SEMANTIC_DIAGNOSTICS, "getSemanticDiagnostics");
        assert_eq!(method::GET_TYPE_AT_POSITION, "getTypeAtPosition");
        assert_eq!(method::GET_SYMBOL_AT_POSITION, "getSymbolAtPosition");
        assert_eq!(method::TYPE_TO_STRING, "typeToString");
        assert_eq!(method::INITIALIZE, "initialize");
    }
}
