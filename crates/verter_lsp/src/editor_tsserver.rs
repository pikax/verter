//! Neutral attestation facts for an editor-owned tsserver plugin session.

use std::path::Path;

use serde::Deserialize;
use verter_workspace::native_fs::NativeFs;

/// Wire version shared with `@verter/language-shared`.
pub const EDITOR_TSSERVER_ATTESTATION_VERSION: u32 = 1;

/// A validated receipt written from inside the editor-owned tsserver process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTsserverAttestation {
    pub pid: u32,
    pub projects: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawEditorTsserverAttestation {
    version: u32,
    nonce: String,
    pid: u64,
    projects: Vec<String>,
}

/// Read and validate a current-session, project-bound editor tsserver receipt.
pub fn read_editor_tsserver_attestation(
    receipt_path: &Path,
    expected_nonce: &str,
) -> Result<EditorTsserverAttestation, String> {
    if expected_nonce.len() != 32
        || !expected_nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("editor tsserver attestation nonce must be 32 lowercase hex digits".into());
    }

    // Editor receipts are native control-plane files, not document overlays,
    // but disk access still belongs to the workspace filesystem boundary.
    // Keeping this read on `NativeFs` gives all production filesystem access
    // one normalization and observability owner.
    let receipt_id = receipt_path.to_str().ok_or_else(|| {
        format!(
            "editor tsserver attestation path is not valid UTF-8: {}",
            receipt_path.display()
        )
    })?;
    let source = NativeFs::new().read_file(receipt_id).ok_or_else(|| {
        format!(
            "failed to read editor tsserver attestation {}",
            receipt_path.display()
        )
    })?;
    let bytes = source.as_bytes();
    if bytes.len() > 64 * 1024 {
        return Err("editor tsserver attestation exceeds the 64 KiB protocol limit".into());
    }
    let raw: RawEditorTsserverAttestation = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid editor tsserver attestation JSON: {error}"))?;
    if raw.version != EDITOR_TSSERVER_ATTESTATION_VERSION {
        return Err(format!(
            "unsupported editor tsserver attestation version {}",
            raw.version
        ));
    }
    if raw.nonce != expected_nonce {
        return Err("editor tsserver attestation belongs to another session".into());
    }
    let pid = u32::try_from(raw.pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| "editor tsserver attestation has an invalid process id".to_string())?;
    if raw.projects.is_empty() || raw.projects.iter().any(String::is_empty) {
        return Err("editor tsserver attestation is not bound to a project".into());
    }
    let mut projects = raw.projects;
    projects.sort();
    projects.dedup();
    Ok(EditorTsserverAttestation { pid, projects })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const NONCE: &str = "0123456789abcdef0123456789abcdef";

    fn write_receipt(value: serde_json::Value) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp receipt");
        file.write_all(&serde_json::to_vec(&value).expect("serialize receipt"))
            .expect("write receipt");
        file.flush().expect("flush receipt");
        file
    }

    #[test]
    fn accepts_only_current_session_project_bound_receipt() {
        let file = write_receipt(serde_json::json!({
            "version": 1,
            "nonce": NONCE,
            "pid": 4242,
            "projects": ["/ws/tsconfig.json", "/ws/tsconfig.json", "/ws/pkg/tsconfig.json"]
        }));

        assert_eq!(
            read_editor_tsserver_attestation(file.path(), NONCE).expect("valid receipt"),
            EditorTsserverAttestation {
                pid: 4242,
                projects: vec!["/ws/pkg/tsconfig.json".into(), "/ws/tsconfig.json".into()],
            }
        );
    }

    #[test]
    fn rejects_stale_nonce_wrong_version_and_unbound_project() {
        for value in [
            serde_json::json!({
                "version": 1,
                "nonce": "ffffffffffffffffffffffffffffffff",
                "pid": 4242,
                "projects": ["/ws/tsconfig.json"]
            }),
            serde_json::json!({
                "version": 2,
                "nonce": NONCE,
                "pid": 4242,
                "projects": ["/ws/tsconfig.json"]
            }),
            serde_json::json!({
                "version": 1,
                "nonce": NONCE,
                "pid": 4242,
                "projects": []
            }),
        ] {
            let file = write_receipt(value);
            assert!(read_editor_tsserver_attestation(file.path(), NONCE).is_err());
        }
    }

    #[test]
    fn rejects_invalid_challenge_pid_and_project_entries() {
        let valid = serde_json::json!({
            "version": 1,
            "nonce": NONCE,
            "pid": 4242,
            "projects": ["/ws/tsconfig.json"]
        });
        assert!(read_editor_tsserver_attestation(Path::new("missing"), "not-a-nonce").is_err());

        for (key, value) in [
            ("pid", serde_json::json!(0)),
            ("projects", serde_json::json!([""])),
        ] {
            let mut receipt = valid.clone();
            receipt[key] = value;
            let file = write_receipt(receipt);
            assert!(read_editor_tsserver_attestation(file.path(), NONCE).is_err());
        }
    }
}
