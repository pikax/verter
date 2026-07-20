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

/// Returns `true` if the binary at `path` is a native executable (not a `.cmd`/`.sh` shim).
pub fn is_native_binary(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("cmd") => false,
        Some(ext) if ext.eq_ignore_ascii_case("sh") => false,
        Some(ext) if ext.eq_ignore_ascii_case("exe") => true,
        Some(_) => false,
        // No extension: native on Unix (the platform package's `tsc`; a `.bin`
        // shim there carries a shebang and spawns fine), not directly
        // executable on Windows.
        None => !cfg!(target_os = "windows"),
    }
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
    fn is_native_binary_platform_package_no_ext() {
        let path = Path::new("/project/node_modules/@typescript/typescript-linux-x64/lib/tsc");
        if cfg!(target_os = "windows") {
            assert!(
                !is_native_binary(path),
                "an extensionless name is not directly executable on Windows"
            );
        } else {
            assert!(
                is_native_binary(path),
                "the platform package's extensionless tsc is native on Unix"
            );
        }
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
