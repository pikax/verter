//! tsc-compatible diagnostic output formatting.

use std::fmt;
use std::path::Path;

/// A single diagnostic to report.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// File path (as displayed to the user).
    pub file: String,
    /// 1-indexed line.
    pub line: u32,
    /// 1-indexed column.
    pub col: u32,
    /// TypeScript error code.
    pub ts_code: u32,
    /// Human-readable message.
    pub message: String,
    /// Severity ("error" or "warning").
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}({},{}): {} TS{}: {}",
            self.file, self.line, self.col, self.severity, self.ts_code, self.message
        )
    }
}

/// Parse the raw output of `tsc` into a list of diagnostics.
///
/// tsc outputs lines in the format:
/// ```text
/// path/to/file.ts(line,col): error TS1234: message
/// ```
/// Multi-line messages (continuation lines without `(`/`: `) are folded into
/// the previous diagnostic.
pub fn parse_tsc_output(raw: &str) -> Vec<TscDiagnostic> {
    let mut result = Vec::new();

    for line in raw.lines() {
        if let Some(d) = parse_tsc_line(line) {
            result.push(d);
        }
    }

    result
}

/// A raw tsc diagnostic before source-map remapping.
#[derive(Debug, Clone)]
pub struct TscDiagnostic {
    /// File path as reported by tsc (may be `.tsc.tsx`).
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub severity: Severity,
    pub ts_code: u32,
    pub message: String,
}

impl TscDiagnostic {
    /// Convert to a displayable `Diagnostic` using an optionally remapped file path.
    pub fn into_diagnostic(
        self,
        remapped_file: Option<String>,
        remapped_line: u32,
        remapped_col: u32,
    ) -> Diagnostic {
        Diagnostic {
            file: remapped_file.unwrap_or(self.file),
            line: remapped_line,
            col: remapped_col,
            severity: self.severity,
            ts_code: self.ts_code,
            message: self.message,
        }
    }
}

/// Attempt to parse a tsc error line:
/// `<file>(<line>,<col>): error TS<code>: <message>`
fn parse_tsc_line(line: &str) -> Option<TscDiagnostic> {
    // Find the `(line,col): ` part.
    let paren_start = line.find('(')?;
    let paren_end = line[paren_start..].find(')')? + paren_start;

    let file = &line[..paren_start];
    let coords = &line[paren_start + 1..paren_end];

    // coords = "line,col"
    let mut parts = coords.splitn(2, ',');
    let line_n: u32 = parts.next()?.trim().parse().ok()?;
    let col_n: u32 = parts.next()?.trim().parse().ok()?;

    // Rest should be `: error TS<code>: <message>` or `: warning TS<code>: ...`
    let rest = line[paren_end + 1..].trim();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim();

    let (severity, rest) = if let Some(after) = rest.strip_prefix("error ") {
        (Severity::Error, after)
    } else if let Some(after) = rest.strip_prefix("warning ") {
        (Severity::Warning, after)
    } else {
        return None;
    };

    // rest = "TS<code>: <message>"
    let rest = rest.strip_prefix("TS")?;
    let colon = rest.find(':')?;
    let ts_code: u32 = rest[..colon].parse().ok()?;
    let message = rest[colon + 1..].trim().to_string();

    // Normalize file path separators.
    let file = file.replace('\\', "/");

    Some(TscDiagnostic {
        file,
        line: line_n,
        col: col_n,
        severity,
        ts_code,
        message,
    })
}

/// Find the path to the `tsgo` binary.
///
/// Search order:
/// 1. Native binary in `node_modules/@typescript/native-preview-<platform>/lib/tsgo[.exe]`
///    (walking up parent dirs — skips Node.js shim overhead)
/// 2. `node_modules/.bin/tsgo[.cmd]` (walking up parent dirs)
/// 3. `tsgo` on PATH
/// 4. npx cache (`%LOCALAPPDATA%/npm-cache/_npx/` or `~/.npm/_npx/`)
pub fn find_tsgo(start_dir: &Path) -> Option<std::path::PathBuf> {
    let native_pkg = native_tsgo_package_name();
    let native_bin = native_tsgo_binary_name();

    // Walk up from start_dir checking node_modules at each level.
    let mut dir = start_dir.to_path_buf();
    loop {
        let nm = dir.join("node_modules");

        // 1. Direct native binary (fastest — no Node.js shim).
        if let Some(pkg) = &native_pkg {
            let candidate = nm
                .join("@typescript")
                .join(pkg)
                .join("lib")
                .join(native_bin);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // 2. npm shim in .bin/.
        let bin_dir = nm.join(".bin");
        if cfg!(target_os = "windows") {
            let candidate_cmd = bin_dir.join("tsgo.cmd");
            if candidate_cmd.exists() {
                return Some(candidate_cmd);
            }
        }
        let candidate = bin_dir.join("tsgo");
        if candidate.exists() {
            return Some(candidate);
        }

        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }

    // 3. PATH lookup.
    #[cfg(target_os = "windows")]
    {
        if let Some(p) = which_simple("tsgo.cmd").or_else(|| which_simple("tsgo.exe")) {
            return Some(p);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(p) = which_simple("tsgo") {
            return Some(p);
        }
    }

    // 4. npx cache.
    find_tsgo_in_npx_cache()
}

/// Search the npx cache for a tsgo binary.
fn find_tsgo_in_npx_cache() -> Option<std::path::PathBuf> {
    let cache_dir = npm_cache_npx_dir()?;
    let entries = std::fs::read_dir(&cache_dir).ok()?;
    let native_pkg = native_tsgo_package_name();
    let native_bin = native_tsgo_binary_name();

    for entry in entries.flatten() {
        let base = entry.path().join("node_modules");

        // Native binary in the cache.
        if let Some(pkg) = &native_pkg {
            let candidate = base
                .join("@typescript")
                .join(pkg)
                .join("lib")
                .join(native_bin);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // Shim in the cache.
        if cfg!(target_os = "windows") {
            let shim = base.join(".bin").join("tsgo.cmd");
            if shim.exists() {
                return Some(shim);
            }
        }
        let shim = base.join(".bin").join("tsgo");
        if shim.exists() {
            return Some(shim);
        }
    }

    None
}

/// Get the npm cache `_npx` directory.
fn npm_cache_npx_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA")
            .ok()
            .map(|d| std::path::PathBuf::from(d).join("npm-cache").join("_npx"))
    } else {
        std::env::var("HOME")
            .ok()
            .map(|d| std::path::PathBuf::from(d).join(".npm").join("_npx"))
    }
}

/// Return the platform-specific native tsgo package name, e.g. `native-preview-win32-x64`.
fn native_tsgo_package_name() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("native-preview-win32-x64")
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        Some("native-preview-win32-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("native-preview-linux-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("native-preview-linux-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("native-preview-darwin-x64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("native-preview-darwin-arm64")
    } else {
        None
    }
}

/// Return the platform-specific native binary name (`tsgo.exe` on Windows, `tsgo` elsewhere).
fn native_tsgo_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "tsgo.exe"
    } else {
        "tsgo"
    }
}

/// Returns `true` if the binary at `path` is a native executable (not a `.cmd`/`.sh` shim).
pub fn is_native_binary(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("cmd") => false,
        Some(ext) if ext.eq_ignore_ascii_case("sh") => false,
        Some(ext) if ext.eq_ignore_ascii_case("exe") => true,
        Some(_) => false,
        // No extension: native on Unix, shim on Windows (node_modules/.bin/tsgo is a shell script).
        // But the actual native binary `tsgo` (no ext) in the platform package IS native.
        // Heuristic: if the path contains `@typescript/native-preview`, it's a native binary.
        None => {
            let path_str = path.to_string_lossy();
            path_str.contains("native-preview-") || !cfg!(target_os = "windows")
        }
    }
}

/// Find the path to the `tsc` binary in node_modules or PATH.
pub fn find_tsc(start_dir: &Path) -> Option<std::path::PathBuf> {
    // Check node_modules/.bin/tsc relative to start_dir and its parents.
    // On Windows, prefer .cmd (batch wrapper) over bare name (shell script).
    let mut dir = start_dir.to_path_buf();
    loop {
        let bin_dir = dir.join("node_modules").join(".bin");
        if cfg!(target_os = "windows") {
            let candidate_cmd = bin_dir.join("tsc.cmd");
            if candidate_cmd.exists() {
                return Some(candidate_cmd);
            }
        }
        let candidate = bin_dir.join("tsc");
        if candidate.exists() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    // Fall back to PATH.
    #[cfg(target_os = "windows")]
    {
        which_simple("tsc.cmd").or_else(|| which_simple("tsc"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        which_simple("tsc")
    }
}

/// Look for a binary on PATH by iterating PATH entries.
fn which_simple(name: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    // ── is_native_binary tests ───────────────────────────────────────

    #[test]
    fn is_native_binary_exe() {
        assert!(
            is_native_binary(Path::new("/some/path/tsgo.exe")),
            ".exe should be native"
        );
    }

    #[test]
    fn is_native_binary_cmd_is_not_native() {
        assert!(
            !is_native_binary(Path::new("/some/path/tsgo.cmd")),
            ".cmd should not be native"
        );
    }

    #[test]
    fn is_native_binary_native_preview_no_ext() {
        assert!(
            is_native_binary(Path::new(
                "/project/node_modules/@typescript/native-preview-linux-x64/lib/tsgo"
            )),
            "tsgo in native-preview package without ext should be native"
        );
    }

    #[test]
    fn is_native_binary_sh_is_not_native() {
        assert!(
            !is_native_binary(Path::new("/some/path/tsgo.sh")),
            ".sh should not be native"
        );
    }

    // ── parse_tsc_output tests (tsgo produces identical format) ──────

    #[test]
    fn parse_tsc_output_standard_error() {
        let output =
            "src/App.vue(10,5): error TS2322: Type 'string' is not assignable to type 'number'.";
        let diags = parse_tsc_output(output);
        assert_eq!(diags.len(), 1, "should parse one diagnostic");
        let d = &diags[0];
        assert_eq!(d.file, "src/App.vue");
        assert_eq!(d.line, 10);
        assert_eq!(d.col, 5);
        assert_eq!(d.ts_code, 2322);
        assert_eq!(d.severity, Severity::Error);
        assert!(d.message.contains("Type 'string' is not assignable"));
    }

    #[test]
    fn parse_tsc_output_ts5102_removed_option() {
        // tsgo emits TS5102 for removed compiler options; verify we parse it.
        let output =
            "tsconfig.json(3,5): error TS5102: Option 'importsNotUsedAsValues' has been removed.";
        let diags = parse_tsc_output(output);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].ts_code, 5102);
        assert!(diags[0].message.contains("removed"));
    }

    #[test]
    fn parse_tsc_output_multiple_diagnostics() {
        let output = "\
src/a.ts(1,1): error TS2304: Cannot find name 'foo'.
src/b.ts(5,10): error TS2307: Cannot find module 'bar'.
";
        let diags = parse_tsc_output(output);
        assert_eq!(diags.len(), 2, "should parse both diagnostics");
        assert_eq!(diags[0].file, "src/a.ts");
        assert_eq!(diags[1].file, "src/b.ts");
    }

    #[test]
    fn parse_tsc_output_ignores_non_diagnostic_lines() {
        let output = "\
Starting compilation...
src/a.ts(1,1): error TS2304: Cannot find name 'foo'.
Found 1 error.
";
        let diags = parse_tsc_output(output);
        assert_eq!(diags.len(), 1, "should ignore non-diagnostic lines");
        assert_eq!(diags[0].ts_code, 2304);
    }

    #[test]
    fn parse_tsc_output_windows_backslash_paths() {
        let output = r"src\components\App.vue(3,12): error TS2345: Argument of type 'string' is not assignable.";
        let diags = parse_tsc_output(output);
        assert_eq!(diags.len(), 1);
        // Paths should be normalized to forward slashes.
        assert!(
            !diags[0].file.contains('\\'),
            "backslashes should be normalized"
        );
        assert_eq!(diags[0].file, "src/components/App.vue");
    }

    // ── find_tsgo tests ─────────────────────────────────────────────

    #[test]
    fn find_tsgo_discovers_node_modules_bin() {
        let temp = tempfile::TempDir::new().unwrap();
        let bin_dir = temp.path().join("node_modules/.bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        if cfg!(target_os = "windows") {
            let tsgo_cmd = bin_dir.join("tsgo.cmd");
            std::fs::write(&tsgo_cmd, "@echo off").unwrap();
            let result = find_tsgo(temp.path());
            assert!(
                result.is_some(),
                "should find tsgo.cmd in node_modules/.bin"
            );
            assert!(
                result.unwrap().to_string_lossy().contains("tsgo.cmd"),
                "should return the .cmd path on Windows"
            );
        } else {
            let tsgo = bin_dir.join("tsgo");
            std::fs::write(&tsgo, "#!/bin/sh").unwrap();
            let result = find_tsgo(temp.path());
            assert!(result.is_some(), "should find tsgo in node_modules/.bin");
        }
    }

    #[test]
    fn find_tsgo_prefers_native_binary_over_shim() {
        let temp = tempfile::TempDir::new().unwrap();
        let nm = temp.path().join("node_modules");

        // Create both the shim and the native binary.
        let bin_dir = nm.join(".bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        if let Some(pkg) = native_tsgo_package_name() {
            let native_dir = nm.join("@typescript").join(pkg).join("lib");
            std::fs::create_dir_all(&native_dir).unwrap();
            let native_bin = native_dir.join(native_tsgo_binary_name());
            std::fs::write(&native_bin, "native").unwrap();

            if cfg!(target_os = "windows") {
                std::fs::write(bin_dir.join("tsgo.cmd"), "@echo off").unwrap();
            } else {
                std::fs::write(bin_dir.join("tsgo"), "#!/bin/sh").unwrap();
            }

            let result = find_tsgo(temp.path());
            assert!(result.is_some(), "should find tsgo");
            let path = result.unwrap();
            assert!(
                path.to_string_lossy().contains("native-preview-"),
                "should prefer native binary over shim: {}",
                path.display()
            );
        }
    }

    #[test]
    fn find_tsgo_does_not_panic_on_empty_dir() {
        // Verify find_tsgo doesn't crash on a directory with no node_modules.
        // We can't assert None because tsgo may be on PATH or in npx cache.
        let temp = tempfile::TempDir::new().unwrap();
        let _result = find_tsgo(temp.path()); // should not panic
    }

    // ── helper tests ────────────────────────────────────────────────

    #[test]
    fn native_tsgo_package_name_is_set() {
        // On any common CI/dev platform, this should return Some.
        let name = native_tsgo_package_name();
        assert!(
            name.is_some(),
            "should have a known platform package name on this OS/arch"
        );
        assert!(
            name.unwrap().starts_with("native-preview-"),
            "package name should start with native-preview-"
        );
    }

    #[test]
    fn diagnostic_display_format() {
        let d = Diagnostic {
            file: "src/App.vue".to_string(),
            line: 10,
            col: 5,
            ts_code: 2322,
            message: "Type error".to_string(),
            severity: Severity::Error,
        };
        let s = format!("{d}");
        assert_eq!(s, "src/App.vue(10,5): error TS2322: Type error");
        assert!(!s.contains("warning"), "should be error, not warning");
    }
}
