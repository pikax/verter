//! Portable `--api` spawn-argument construction.
//!
//! Mirrors the JS sync client's spawn (`dist/api/options.js:13-24` +
//! `dist/api/sync/client.js:22-26`): the base args are
//! `["--api", "--cwd", <cwd>, "--useCaseSensitiveFileNames=<bool>"]`, followed
//! by `--callbacks=<comma-joined enabled callbacks>` when the host services FS
//! callbacks. On Windows the pipe layer additionally appends `--pipe <path>`
//! (see [`crate::transport`]); that flag is added there, not here, because the
//! pipe path is only known once the pipe is created.
//!
//! tsgo binary DISCOVERY lives in [`crate::toolchain::discovery`] (the 4-tier
//! resolver) — this module only builds spawn arguments.

use std::path::Path;

/// The host-callback names enabled on the wire, in the engine's callback-table
/// order (`fsCallbacks.js:2-19`). The Verter overlay snapshot services these
/// five; the table's other entries (`stat`, `writeFile`, `removeFile`) are left
/// off `--callbacks=`, which the engine reads as "serve from the host
/// filesystem" (the `useOS` default, `fsCallbacks.js:21-40`).
pub const ENABLED_CALLBACKS: &[&str] = &[
    "readFile",
    "fileExists",
    "directoryExists",
    "getAccessibleEntries",
    "realpath",
];

/// Build the base `--api` (sync MessagePack mode) spawn arguments.
///
/// Mirrors `options.js:13-24` + `sync/client.js:22-26`. `cwd` is the project
/// working directory the engine resolves relative paths against.
/// `case_sensitive` is the host file system's path comparison (see
/// [`host_file_system_is_case_sensitive`]). When `with_callbacks` is true the
/// `--callbacks=…` flag is appended (the host will service FS callbacks).
///
/// The Windows `--pipe <path>` flag is NOT added here — the pipe layer appends
/// it once the named pipe is created.
pub fn build_sync_api_args(cwd: &str, case_sensitive: bool, with_callbacks: bool) -> Vec<String> {
    let mut args = vec![
        "--api".to_string(),
        "--cwd".to_string(),
        cwd.to_string(),
        format!("--useCaseSensitiveFileNames={case_sensitive}"),
    ];
    if with_callbacks {
        args.push(format!("--callbacks={}", ENABLED_CALLBACKS.join(",")));
    }
    args
}

/// Whether the host file system compares paths case-sensitively, inferred the
/// way the JS client does (`options.js:26-32`): never on Windows; elsewhere,
/// true iff the case-flipped spelling of `probe` (an existing file) does NOT
/// resolve to an existing file.
pub fn host_file_system_is_case_sensitive(probe: &Path) -> bool {
    if cfg!(windows) {
        return false;
    }
    let spelled = probe.to_string_lossy();
    let flipped: String = spelled
        .chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else if c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    flipped == spelled || !Path::new(&flipped).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_api_args_without_callbacks() {
        let args = build_sync_api_args("/repo", true, false);
        assert_eq!(
            args,
            vec![
                "--api",
                "--cwd",
                "/repo",
                "--useCaseSensitiveFileNames=true"
            ]
        );
    }

    #[test]
    fn sync_api_args_with_callbacks_lists_all_five_in_order() {
        let args = build_sync_api_args("/repo", false, true);
        assert_eq!(
            args,
            vec![
                "--api",
                "--cwd",
                "/repo",
                "--useCaseSensitiveFileNames=false",
                "--callbacks=readFile,fileExists,directoryExists,getAccessibleEntries,realpath",
            ]
        );
    }

    #[test]
    fn callback_order_matches_the_engine_callback_table() {
        // The order is fixed by fsCallbacks.js:2-19; guard it against accidental edits.
        assert_eq!(
            ENABLED_CALLBACKS,
            &[
                "readFile",
                "fileExists",
                "directoryExists",
                "getAccessibleEntries",
                "realpath"
            ]
        );
    }

    #[test]
    fn windows_is_case_insensitive_and_a_missing_flip_is_sensitive() {
        let exe = std::env::current_exe().expect("current exe");
        let sensitive = host_file_system_is_case_sensitive(&exe);
        if cfg!(windows) {
            assert!(
                !sensitive,
                "Windows file systems compare case-insensitively"
            );
        } else {
            let flipped_exists = {
                let s = exe.to_string_lossy();
                let f: String = s
                    .chars()
                    .map(|c| {
                        if c.is_ascii_uppercase() {
                            c.to_ascii_lowercase()
                        } else {
                            c.to_ascii_uppercase()
                        }
                    })
                    .collect();
                f != s && Path::new(&f).exists()
            };
            assert_eq!(sensitive, !flipped_exists);
        }
    }
}
