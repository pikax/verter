//! Measure configuration plus interleaved injection/upsert of a generated
//! TypeScript project. Corpus generation, host construction, answer checking
//! and teardown are outside the timer. Timings are samples, never gates.
//!
//! cargo run --release -p verter_session --example project_open_profile

use std::sync::Arc;
use std::time::Instant;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace};

fn main() {
    const FILES: usize = 8_441;
    const DIRECTORIES: usize = 104;
    let mut files: Vec<(String, Arc<str>)> = Vec::with_capacity(FILES);
    files.push((
        "/open/lib.d.ts".into(),
        Arc::from("interface SharedRoot { value: number }\n"),
    ));
    for n in 0..FILES - 1 {
        let mut source = format!(
            "export interface Item{n} extends SharedRoot {{ id: number; label: string }}\n\
             export const item{n}: Item{n} = {{ value: {n}, id: {n}, label: 'item' }};\n"
        );
        // Keep the generated shape near 3.3 MB without adding semantic work.
        source.push_str("// ");
        source.extend(std::iter::repeat_n(
            'x',
            390usize.saturating_sub(source.len()),
        ));
        source.push('\n');
        files.push((
            format!("/open/dir{}/file{n}.ts", n % DIRECTORIES),
            Arc::from(source),
        ));
    }
    let bytes: usize = files.iter().map(|(_, source)| source.len()).sum();
    let config: Arc<str> =
        Arc::from(r#"{"compilerOptions":{"noLib":true,"strict":true},"include":["**/*.ts"]}"#);
    let mut samples = Vec::new();
    for _ in 0..3 {
        let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        let host = VerterHost::new(HostConfig::default(), workspace.clone());
        let start = Instant::now();
        workspace.inject_file("/open/tsconfig.json".into(), config.clone());
        let mut project = verter_workspace::ide_project_config(
            "/open".into(),
            "/open".into(),
            Some("/open/tsconfig.json".into()),
        );
        project.compiler_options =
            verter_workspace::load_compiler_options(workspace.as_ref(), "/open/tsconfig.json");
        host.configure_projects(vec![project]);
        for (canonical, source) in &files {
            workspace.inject_file(canonical.clone(), source.clone());
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical.clone(),
                    source: source.clone(),
                    file_language: FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("open project member");
        }
        samples.push(start.elapsed().as_secs_f64());
        // Demand the shared root only after opening; setup never evaluates types.
        let (answer, _) = host
            .resolve_named_symbol_with_audit(&files[1].0, "SharedRoot", None)
            .into_parts();
        assert!(answer.expect("shared root request").is_some());
    }
    println!(
        "{}",
        serde_json::json!({
            "files": FILES,
            "directories": DIRECTORIES,
            "bytes": bytes,
            "noLib": true,
            "sharedRootResolved": true,
            "setupSeconds": samples,
        })
    );
}
