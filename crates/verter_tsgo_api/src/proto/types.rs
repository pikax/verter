//! Typed request/response DTOs for the tsgo `--api` high-level ops.
//!
//! These mirror `typescript/dist/api/proto.d.ts` (the response
//! shapes) and the literal `apiRequest(method, params)` call sites in
//! `dist/api/sync/api.js` (the request param shapes). Field names are emitted
//! exactly as the JS `JSON.stringify(params)` would emit them (camelCase), so
//! the JSON bytes this module produces are byte-compatible with the official
//! client's. Cited source lines accompany each type so a version-update agent
//! can re-verify against the same reference.
//!
//! Payload encoding: the high-level ops carry their params/results as a UTF-8
//! JSON document inside the frame `payload` bin field (sync/client.js:48-55).

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
/// rename, and completion/completion-resolve — are NOT part of the tsgo `--api`
/// compiler-reflection surface (they belong to the separate tsgo LSP server
/// mode, not this channel). "Definition" is reachable today by composing
/// [`GET_SYMBOL_AT_POSITION`] with the returned [`SymbolResponse::declarations`]
/// node-handle paths; the dedicated LSP-feature ops are added when this crate
/// also drives the tsgo LSP transport.
///
/// TODO(follow-up): the BINARY ops ([`GET_SOURCE_FILE`], plus `typeToTypeNode`
/// and `signatureToSignatureDeclaration`) return the columnar binary-AST blob
/// (PROTOCOL_VERSION 5), not JSON. The codec hand-writes the frame envelope for
/// them, but the columnar-AST decoder is deferred — no current owned-backend
/// consumer needs the AST blob, and decoding it faithfully is a separate, large
/// surface. The frame round-trips; only the binary payload decode is deferred.
pub mod method {
    /// `"initialize"` — once-only startup handshake (sync/api.js:51). Params `null`.
    pub const INITIALIZE: &str = "initialize";
    /// `"parseConfigFile"` (sync/api.js:60).
    pub const PARSE_CONFIG_FILE: &str = "parseConfigFile";
    /// `"updateSnapshot"` (sync/api.js:68).
    pub const UPDATE_SNAPSHOT: &str = "updateSnapshot";
    /// `"release"` — release a snapshot handle (sync/api.js:155).
    pub const RELEASE: &str = "release";
    /// `"getDefaultProjectForFile"` (sync/api.js:138).
    pub const GET_DEFAULT_PROJECT_FOR_FILE: &str = "getDefaultProjectForFile";
    /// `"getSemanticDiagnostics"` (sync/api.js:241).
    pub const GET_SEMANTIC_DIAGNOSTICS: &str = "getSemanticDiagnostics";
    /// `"getSyntacticDiagnostics"` (sync/api.js:229).
    pub const GET_SYNTACTIC_DIAGNOSTICS: &str = "getSyntacticDiagnostics";
    /// `"getSuggestionDiagnostics"` (sync/api.js:253).
    pub const GET_SUGGESTION_DIAGNOSTICS: &str = "getSuggestionDiagnostics";
    /// `"getTypeAtPosition"` (sync/api.js:387).
    pub const GET_TYPE_AT_POSITION: &str = "getTypeAtPosition";
    /// `"getSymbolAtPosition"` (sync/api.js:312).
    pub const GET_SYMBOL_AT_POSITION: &str = "getSymbolAtPosition";
    /// `"typeToString"` (sync/api.js:563).
    pub const TYPE_TO_STRING: &str = "typeToString";
    /// `"echo"` — diagnostic round-trip (sync/client.js:62). String payload.
    pub const ECHO: &str = "echo";
    /// `"getSourceFile"` — BINARY op (`apiRequestBinary`, sync/api.js:209).
    /// The response payload is the columnar binary AST, not JSON; this crate's
    /// typed decode for it is a deliberate follow-up (see crate `proto` docs).
    pub const GET_SOURCE_FILE: &str = "getSourceFile";
}

// ── DocumentIdentifier (proto.d.ts:11-13) ───────────────────────────────────
/// A document identifier: a file-name string, or `{ uri }`. Mirrors
/// `DocumentIdentifier` (proto.d.ts:11-13). The JS client resolves the value
/// through `resolveFileName` before sending, so on the wire it is almost always
/// a plain string; we accept both forms.
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

// ── FileChanges (proto.d.ts:50-57) ──────────────────────────────────────────
/// A per-snapshot file-change summary. Mirrors `FileChangeSummary`
/// (proto.d.ts:50-54). Absent fields are omitted from the JSON (matching the
/// JS optional-field behavior).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChangeSummary {
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

/// File changes between snapshots: a summary, or `{ invalidateAll: true }`.
/// Mirrors `FileChanges` (proto.d.ts:55-57).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileChanges {
    /// A per-file summary.
    Summary(FileChangeSummary),
    /// Invalidate every file.
    InvalidateAll {
        /// Always `true`.
        #[serde(rename = "invalidateAll")]
        invalidate_all: bool,
    },
}

// ── updateSnapshot params (proto.d.ts:46-63; sync/api.js:62-68) ──────────────
/// Params for `updateSnapshot`. Mirrors `UpdateSnapshotParams`
/// (proto.d.ts:61-63). `openProject` is run through `resolveFileName` on the JS
/// side before sending (sync/api.js:65-67).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSnapshotParams {
    /// Path to a tsconfig.json to open in the new snapshot.
    #[serde(rename = "openProject", skip_serializing_if = "Option::is_none")]
    pub open_project: Option<String>,
    /// File changes relative to the previous snapshot.
    #[serde(rename = "fileChanges", skip_serializing_if = "Option::is_none")]
    pub file_changes: Option<FileChanges>,
}

// ── Response DTOs (proto.d.ts) ───────────────────────────────────────────────
/// Response from `initialize`. Mirrors `InitializeResponse` (proto.d.ts:36-41).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResponse {
    /// Whether the host file system is case-sensitive.
    #[serde(rename = "useCaseSensitiveFileNames")]
    pub use_case_sensitive_file_names: bool,
    /// The server's current working directory.
    #[serde(rename = "currentDirectory")]
    pub current_directory: String,
}

/// A project entry within a snapshot. Mirrors `ProjectResponse`
/// (proto.d.ts:93-98).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectResponse {
    /// Opaque project handle (e.g. `p.<config>`).
    pub id: String,
    /// The tsconfig.json path backing this project.
    #[serde(rename = "configFileName")]
    pub config_file_name: String,
    /// Resolved compiler options.
    #[serde(rename = "compilerOptions")]
    pub compiler_options: serde_json::Map<String, serde_json::Value>,
    /// Root files of the project's program.
    #[serde(rename = "rootFiles")]
    pub root_files: Vec<String>,
}

/// An opaque engine-issued numeric handle.
///
/// The tsgo `--api` engine issues the whole opaque-numeric-handle class — the
/// `updateSnapshot` snapshot handle, the `getTypeAtPosition` type id, the
/// owning-symbol id, the `getSymbolAtPosition` symbol id — as a bare JSON
/// INTEGER (`"snapshot":1`, `"id":86`, `"symbol":1`; the shipped `proto.d.ts`
/// types these as `number`). The codec carries every such handle through this
/// one shared [`i64`] newtype.
///
/// `#[serde(transparent)]` makes the wire shape exactly the inner integer in
/// both directions: a JSON integer decodes straight into [`OpaqueHandle`], and
/// embedding the handle in `serde_json::json!({ "snapshot": handle, .. })`
/// re-serializes it as the same bare integer — never coerced to a string. A
/// newtype (rather than a bare `i64`) keeps the typed protocol surface the
/// parity oracle checks and makes this the single handle type the re-send sites
/// reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpaqueHandle(pub i64);

/// Response from `updateSnapshot`. Mirrors `UpdateSnapshotResponse`
/// (proto.d.ts:85-92). `changes` is absent for the first snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSnapshotResponse {
    /// Opaque handle for the new snapshot — a bare integer (`1`); see
    /// [`OpaqueHandle`].
    pub snapshot: OpaqueHandle,
    /// Projects in the snapshot.
    pub projects: Vec<ProjectResponse>,
    /// Changes from the previous snapshot, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<serde_json::Value>,
}

/// A symbol reflection. Mirrors `SymbolResponse` (proto.d.ts:103-110).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolResponse {
    /// Opaque symbol handle (a bare integer; see [`OpaqueHandle`]).
    pub id: OpaqueHandle,
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

/// A type reflection. Mirrors `TypeResponse` (proto.d.ts:111-131). Only the
/// always-present fields are modeled strongly; the many optional discriminant
/// fields are retained as raw JSON so no information is lost while keeping the
/// hand-written surface small. (The wire's optional fields are themselves a
/// stable closed set per proto.d.ts.)
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
    /// Literal value, when the type is a literal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Owning symbol handle, when present (a bare integer; see [`OpaqueHandle`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<OpaqueHandle>,
}

/// One diagnostic. The shipped `proto.d.ts` does not export a `Diagnostic`
/// interface (it is produced by the binary path), but the JSON shape observed
/// on the wire for `get*Diagnostics` is stable: `{ code, category, text, pos,
/// end, fileName?, ... }`. We model the load-bearing fields strongly and keep
/// the rest accessible via [`serde_json::Value`] in `related`/`message_chain`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The TS diagnostic code (e.g. 2345).
    pub code: u32,
    /// The diagnostic category (0=warning,1=error,2=suggestion,3=message per
    /// `DiagnosticCategory`).
    pub category: u32,
    /// The human-readable message text.
    pub text: String,
    /// Start offset within the file.
    pub pos: u32,
    /// End offset within the file.
    pub end: u32,
    /// The file the diagnostic belongs to, when present.
    #[serde(rename = "fileName", default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // The request param JSON must be byte-identical to JS `JSON.stringify`.
    // serde_json emits keys in struct-declaration order and omits None fields,
    // which matches the JS object-literal field order at each call site.

    #[test]
    fn update_snapshot_open_project_json_matches_js_shape() {
        let p = UpdateSnapshotParams {
            open_project: Some("/repo/tsconfig.json".to_string()),
            file_changes: None,
        };
        // JS sends `{ openProject: "/repo/tsconfig.json" }` (fileChanges omitted).
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"openProject":"/repo/tsconfig.json"}"#
        );
    }

    #[test]
    fn update_snapshot_file_changes_json_matches_js_shape() {
        let p = UpdateSnapshotParams {
            open_project: Some("/repo/tsconfig.json".to_string()),
            file_changes: Some(FileChanges::Summary(FileChangeSummary {
                changed: Some(vec![DocumentIdentifier::file_name("/repo/src/a.ts")]),
                ..Default::default()
            })),
        };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"openProject":"/repo/tsconfig.json","fileChanges":{"changed":["/repo/src/a.ts"]}}"#
        );
    }

    #[test]
    fn file_changes_invalidate_all_json() {
        let fc = FileChanges::InvalidateAll {
            invalidate_all: true,
        };
        assert_eq!(
            serde_json::to_string(&fc).unwrap(),
            r#"{"invalidateAll":true}"#
        );
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

    // ── response decode mirrors proto.d.ts (camelCase wire keys) ────────────
    #[test]
    fn initialize_response_decodes_camelcase() {
        let json = r#"{"useCaseSensitiveFileNames":true,"currentDirectory":"/repo"}"#;
        let r: InitializeResponse = serde_json::from_str(json).unwrap();
        assert!(r.use_case_sensitive_file_names);
        assert_eq!(r.current_directory, "/repo");
    }

    #[test]
    fn update_snapshot_response_decodes_with_projects() {
        let json = r#"{
            "snapshot":1,
            "projects":[{
                "id":"p./repo/tsconfig.json",
                "configFileName":"/repo/tsconfig.json",
                "compilerOptions":{"strict":true},
                "rootFiles":["/repo/src/a.ts"]
            }]
        }"#;
        let r: UpdateSnapshotResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.snapshot, OpaqueHandle(1));
        assert_eq!(r.projects.len(), 1);
        assert_eq!(r.projects[0].config_file_name, "/repo/tsconfig.json");
        assert_eq!(r.projects[0].root_files, vec!["/repo/src/a.ts"]);
        assert!(r.changes.is_none(), "first snapshot has no changes field");
    }

    // ── DISCRIMINATING: the snapshot handle decodes from a JSON INTEGER (the rc
    //    wire) into `OpaqueHandle`, and RE-SERIALIZES faithfully as an integer.
    //    A `String`-typed field cannot decode the integer form, and the path-
    //    bearing project id stays a STRING. ─────────────────────────────────────
    #[test]
    fn snapshot_handle_decodes_integer_and_reserializes_as_integer() {
        // rc wire: "snapshot":1 (a bare integer), with a path-string project id.
        let int_json = r#"{
            "snapshot":1,
            "projects":[{
                "id":"c:/repo/tsconfig.json",
                "configFileName":"c:/repo/tsconfig.json",
                "compilerOptions":{},
                "rootFiles":["c:/repo/src/a.ts"]
            }]
        }"#;
        let resp: UpdateSnapshotResponse =
            serde_json::from_str(int_json).expect("integer snapshot handle must decode (rc wire)");
        assert_eq!(resp.snapshot, OpaqueHandle(1));

        // Faithful re-send: the handle re-serializes as a JSON integer — and is
        // NOT a string.
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

        // The path-bearing project id stays the path STRING (not folded into a handle).
        assert_eq!(resp.projects[0].id, "c:/repo/tsconfig.json");

        // Embedded in a downstream request param the SAME way the client sends it:
        // the integer wire shape rides into `{ "snapshot": <handle>, ... }`.
        let req = serde_json::json!({ "snapshot": resp.snapshot, "project": "p.x" });
        assert_eq!(
            req["snapshot"],
            serde_json::json!(1),
            "re-sending the handle preserves the integer wire shape"
        );
        assert!(
            req["snapshot"].is_number(),
            "the re-sent handle is a JSON integer, never a string"
        );
    }

    // ── DISCRIMINATING: a STRING snapshot handle (the retired native-preview
    //    wire) must NO LONGER decode — the codec is rc-integer-only, with no
    //    int-or-string leniency. ───────────────────────────────────────────────
    #[test]
    fn string_snapshot_handle_no_longer_decodes() {
        let str_json = r#"{"snapshot":"n0000000000000001","projects":[]}"#;
        assert!(
            serde_json::from_str::<UpdateSnapshotResponse>(str_json).is_err(),
            "a string snapshot handle must be rejected — the rc wire is integer-only"
        );
    }

    // ── DISCRIMINATING: `SymbolResponse.id` decodes the rc INTEGER handle, while
    //    the path-bearing declaration handles stay STRINGS. This turns
    //    `SymbolResponse` from "decoded by nothing" into "decoded by a proof". A
    //    `String`-typed `id` rejects the integer, so this fails pre-change. ──────
    #[test]
    fn symbol_response_decodes_integer_id_and_path_string_declarations() {
        // rc wire shape: integer `id`, path-string declarations.
        let json = r#"{"id":3,"name":"origin","flags":2,"checkFlags":0,"declarations":["19.261.c:/x/index.ts"],"valueDeclaration":"19.261.c:/x/index.ts"}"#;
        let s: SymbolResponse =
            serde_json::from_str(json).expect("integer symbol id must decode (rc wire)");
        assert_eq!(s.id, OpaqueHandle(3));
        assert_eq!(s.name, "origin");
        // The id re-serializes as a JSON integer, never a string.
        let id_wire = serde_json::to_value(s.id).unwrap();
        assert!(
            id_wire.is_number() && !id_wire.is_string(),
            "SymbolResponse.id re-serializes as a JSON integer, NOT a string: {id_wire}"
        );
        // Path-bearing declaration handles stay STRINGS (not folded into handles).
        assert_eq!(
            s.declarations.as_deref(),
            Some(["19.261.c:/x/index.ts".to_string()].as_slice())
        );
        assert_eq!(s.value_declaration.as_deref(), Some("19.261.c:/x/index.ts"));
    }

    // ── DISCRIMINATING: `TypeResponse` decodes the rc shape — integer `id` AND
    //    integer `symbol` — and re-serializes both as integers. With a `String`
    //    `id` / `Option<String>` `symbol`, the WHOLE decode fails pre-change. ────
    #[test]
    fn type_response_decodes_integer_id_and_symbol() {
        let json = r#"{"id":86,"flags":1048576,"objectFlags":2,"value":null,"symbol":1}"#;
        let t: TypeResponse =
            serde_json::from_str(json).expect("rc TypeResponse (integer id+symbol) must decode");
        assert_eq!(t.id, OpaqueHandle(86));
        assert_eq!(t.symbol, Some(OpaqueHandle(1)));
        assert_eq!(t.flags, 1048576);
        assert_eq!(t.object_flags, Some(2));

        // Both handles re-serialize as JSON integers (never strings).
        let id_wire = serde_json::to_value(t.id).unwrap();
        assert_eq!(id_wire, serde_json::json!(86));
        assert!(
            id_wire.is_number() && !id_wire.is_string(),
            "TypeResponse.id re-serializes as a JSON integer: {id_wire}"
        );
        let sym_wire = serde_json::to_value(t.symbol.unwrap()).unwrap();
        assert_eq!(sym_wire, serde_json::json!(1));
        assert!(
            sym_wire.is_number() && !sym_wire.is_string(),
            "TypeResponse.symbol re-serializes as a JSON integer: {sym_wire}"
        );
    }

    #[test]
    fn diagnostic_decodes_load_bearing_fields() {
        let json = r#"{"code":2345,"category":1,"text":"Argument not assignable","pos":120,"end":123,"fileName":"/repo/src/a.ts"}"#;
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
        // The wire is camelCase; a snake_case payload must fail (proves we did
        // not accidentally accept the Rust field names on the wire).
        let json = r#"{"use_case_sensitive_file_names":true,"current_directory":"/repo"}"#;
        assert!(
            serde_json::from_str::<InitializeResponse>(json).is_err(),
            "snake_case keys must not decode — the wire is camelCase"
        );
    }

    #[test]
    fn method_name_strings_are_exact() {
        // Guard the literal wire strings against accidental edits.
        assert_eq!(method::UPDATE_SNAPSHOT, "updateSnapshot");
        assert_eq!(method::GET_SEMANTIC_DIAGNOSTICS, "getSemanticDiagnostics");
        assert_eq!(method::GET_TYPE_AT_POSITION, "getTypeAtPosition");
        assert_eq!(method::GET_SYMBOL_AT_POSITION, "getSymbolAtPosition");
        assert_eq!(method::TYPE_TO_STRING, "typeToString");
        assert_eq!(method::INITIALIZE, "initialize");
    }
}
