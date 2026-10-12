//! Foreign crates cannot implement the sealed observation contract, mint
//! private kernel payloads, or enter the gateway through an alternate API.

use std::fs;
use verter_test_support::compile_fixture;

#[test]
fn type_info_privacy_rails() {
    let control = compile_fixture(
        "gateway_control.rs",
        include_str!("../ui/gateway_control.rs"),
    );
    assert!(
        control.status.success(),
        "the sanctioned gateway must compile against the packaged metadata:\n{}",
        String::from_utf8_lossy(&control.stderr)
    );
    // nextest remaps the crate's working directory for archive consumers.
    let mut fixtures: Vec<_> = fs::read_dir("tests/ui/fixtures")
        .expect("discover the privacy fixture inventory")
        .map(|entry| entry.expect("read a privacy fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    fixtures.sort();
    assert!(!fixtures.is_empty(), "privacy fixtures must be nonempty");
    for path in fixtures {
        let name = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read_to_string(&path).expect("read the privacy fixture");
        let expected = fs::read_to_string(path.with_extension("stderr"))
            .expect("each privacy fixture has a diagnostic snapshot");
        let output = compile_fixture(name, &source);
        assert!(!output.status.success(), "{name} must fail to compile");
        let diagnostics: Vec<serde_json::Value> = String::from_utf8(output.stderr)
            .expect("rustc emits UTF-8 JSON diagnostics")
            .lines()
            .map(|line| serde_json::from_str(line).expect("a rustc diagnostic is JSON"))
            .collect();
        let rendered = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["code"].is_object())
            .map(render_diagnostic)
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(
            normalize_diagnostics(&rendered),
            normalize_diagnostics(&expected),
            "{name}: preserve the complete privacy diagnostic snapshot"
        );
    }
}

// rustc's imported spans retain file/line/column identities in metadata, but
// their source excerpts require the builder's original source paths. Restore
// an unavailable excerpt from the packaged source at exactly that emitted
// span, rather than discarding any part of the diagnostic snapshot.
fn render_diagnostic(diagnostic: &serde_json::Value) -> String {
    fn spans<'a>(diagnostic: &'a serde_json::Value, output: &mut Vec<&'a serde_json::Value>) {
        output.extend(diagnostic["spans"].as_array().unwrap());
        for child in diagnostic["children"].as_array().unwrap() {
            spans(child, output);
        }
    }
    let mut diagnostic_spans = Vec::new();
    spans(diagnostic, &mut diagnostic_spans);
    let rendered = diagnostic["rendered"].as_str().unwrap();
    let lines: Vec<_> = rendered.lines().collect();
    let mut result = String::new();
    for (index, line) in lines.iter().enumerate() {
        result.push_str(line);
        result.push('\n');
        let Some(location) = line.trim().strip_prefix("--> ") else {
            continue;
        };
        if lines
            .get(index + 1)
            .is_some_and(|line| line.trim().starts_with('|'))
        {
            continue;
        }
        let (path_line, column) = location.rsplit_once(':').unwrap();
        let (path, line_number) = path_line.rsplit_once(':').unwrap();
        let line_number: u64 = line_number.parse().unwrap();
        let column: u64 = column.parse().unwrap();
        let span = diagnostic_spans
            .iter()
            .find(|span| {
                span["file_name"].as_str().unwrap().replace('\\', "/") == path.replace('\\', "/")
                    && span["line_start"].as_u64() == Some(line_number)
                    && (column == 0 || span["column_start"].as_u64() == Some(column))
            })
            .unwrap_or_else(|| {
                panic!("rendered location {location} has no emitted span: {diagnostic_spans:?}")
            });
        assert_eq!(
            span["line_start"], span["line_end"],
            "handle multiline imported excerpts explicitly"
        );
        let source_path = normalize_diagnostics(&format!("--> {location}"));
        let source = verter_test_support::fixture_diagnostic_source(
            source_path.strip_prefix("--> ").unwrap(),
        );
        let source_line = source.lines().nth(line_number as usize - 1).unwrap();
        let column = span["column_start"].as_u64().unwrap();
        let width = span["column_end"].as_u64().unwrap() - column;
        result.push_str(&format!(
            "  |\n{line_number} | {source_line}\n  | {}{}",
            " ".repeat(column as usize - 1),
            "^".repeat(width as usize)
        ));
        if let Some(label) = span["label"].as_str() {
            result.push(' ');
            result.push_str(label);
        }
        result.push('\n');
    }
    result
}

// Like trybuild, keep input-file spans while hiding dependency source line
// numbers and machine-specific paths. Compare every diagnostic, note, help,
// source excerpt and underline against the existing snapshots.
fn normalize_diagnostics(rendered: &str) -> String {
    let mut dependency_excerpt = false;
    let mut lines = Vec::new();
    for line in rendered.replace('\\', "/").lines() {
        let mut line = line.trim().to_owned();
        if let Some(path) = line.strip_prefix("--> ") {
            dependency_excerpt = !path.starts_with("tests/ui/fixtures/");
            if dependency_excerpt {
                let mut path = path.to_owned();
                while path
                    .rsplit_once(':')
                    .is_some_and(|(_, number)| number.chars().all(|ch| ch.is_ascii_digit()))
                {
                    path.truncate(path.rfind(':').unwrap());
                }
                if let Some((_, relative)) = path.split_once("crates/verter_semantic/") {
                    path = relative.to_owned();
                }
                if let Some((_, registry)) = path.split_once("/registry/src/") {
                    let (_, package_path) = registry.split_once('/').unwrap();
                    let (package, relative) = package_path.split_once('/').unwrap();
                    let (name, _) = package.rsplit_once('-').unwrap();
                    path = format!("$CARGO/{name}-$VERSION/{relative}");
                }
                line = format!("--> {path}");
            }
        } else if dependency_excerpt {
            if let Some((number, excerpt)) = line.split_once('|') {
                if number.trim().chars().all(|ch| ch.is_ascii_digit()) {
                    line = format!("|{excerpt}");
                }
            } else {
                dependency_excerpt = false;
            }
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_owned()
}
