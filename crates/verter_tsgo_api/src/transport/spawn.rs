//! Portable `--api` spawn-argument construction.
//!
//! Mirrors the JS sync client's spawn (`dist/api/sync/client.js:12-29`): the
//! base args are `["--api", "--cwd", <cwd>]`, followed by
//! `--callbacks=<comma-joined enabled callbacks>` when the host services FS
//! callbacks. On Windows the pipe layer additionally appends `--pipe <path>`
//! (see [`crate::transport`]); that flag is added there, not here, because the
//! pipe path is only known once the pipe is created.
//!
//! tsgo binary DISCOVERY lives in [`crate::toolchain::discovery`] (the 4-tier
//! resolver) — this module only builds spawn arguments.

/// The host-callback names enabled on the wire, in the exact order the JS client
/// emits them (`fs.js:3`). The Verter overlay snapshot services all five, so the
/// enabled set is the full list.
pub const ENABLED_CALLBACKS: &[&str] = &[
    "readFile",
    "fileExists",
    "directoryExists",
    "getAccessibleEntries",
    "realpath",
];

/// Build the base `--api` (sync MessagePack mode) spawn arguments.
///
/// Mirrors `sync/client.js:12-29`. `cwd` is the project working directory the
/// engine resolves relative paths against. When `with_callbacks` is true the
/// `--callbacks=…` flag is appended (the host will service FS callbacks).
///
/// The Windows `--pipe <path>` flag is NOT added here — the pipe layer appends
/// it once the named pipe is created.
pub fn build_sync_api_args(cwd: &str, with_callbacks: bool) -> Vec<String> {
    let mut args = vec!["--api".to_string(), "--cwd".to_string(), cwd.to_string()];
    if with_callbacks {
        args.push(format!("--callbacks={}", ENABLED_CALLBACKS.join(",")));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_api_args_without_callbacks() {
        let args = build_sync_api_args("/repo", false);
        assert_eq!(args, vec!["--api", "--cwd", "/repo"]);
    }

    #[test]
    fn sync_api_args_with_callbacks_lists_all_five_in_order() {
        let args = build_sync_api_args("/repo", true);
        assert_eq!(
            args,
            vec![
                "--api",
                "--cwd",
                "/repo",
                "--callbacks=readFile,fileExists,directoryExists,getAccessibleEntries,realpath",
            ]
        );
    }

    #[test]
    fn callback_order_matches_fs_js() {
        // The wire order is fixed by fs.js:3; guard it against accidental edits.
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
}
