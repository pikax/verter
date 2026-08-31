//! Architecture guard for the generated component-meta audit corpus.
//!
//! The canonical gate is nextest, which starts one process per `#[test]`.
//! Consequently the corpus must stay tabled into moderate chunks: merely
//! compiling every former integration target into one binary does not share
//! process-local worker pools. This hand-written guard is deliberately outside
//! the generator so a broken generator cannot rewrite its own expected shape.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_workspace::WorkspaceRead;

const CHUNK_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedRow {
    canonical: String,
    include_path: String,
}

fn collect_vue_files(root: &Path, output: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).expect("read corpus fixture directory") {
        let entry = entry.expect("read corpus fixture entry");
        let path = entry.path();
        if path.is_dir() {
            collect_vue_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "vue") {
            output.push(path);
        }
    }
}

fn generated_row(line: &str) -> Option<(String, GeneratedRow)> {
    let rest = line.trim().strip_prefix("CorpusCase::new(\"")?;
    let (slug, rest) = rest.split_once("\", \"")?;
    let (canonical, rest) = rest.split_once("\", include_str!(\"")?;
    let include_path = rest.strip_suffix("\")),")?;
    Some((
        slug.to_string(),
        GeneratedRow {
            canonical: canonical.to_string(),
            include_path: include_path.to_string(),
        },
    ))
}

fn slug_for(relative: &str) -> String {
    let no_ext = relative.trim_end_matches(".vue").replace('\\', "/");
    let with_unders = no_ext.replace(['/', '-'], "_");
    let mut output = String::with_capacity(with_unders.len() + 8);
    let mut previous = None;
    for character in with_unders.chars() {
        if character.is_ascii_uppercase() {
            if previous
                .is_some_and(|prior: char| prior.is_ascii_lowercase() || prior.is_ascii_digit())
            {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
        previous = Some(character);
    }
    output
}

fn override_slugs(corpus_root: &Path) -> BTreeSet<String> {
    let overrides_root = corpus_root.join("overrides");
    std::fs::read_dir(&overrides_root)
        .expect("read corpus overrides directory")
        .filter_map(|entry| {
            let path = entry.expect("read corpus override entry").path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("rs")).then(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .expect("override file stem must be UTF-8")
                    .to_string()
            })
        })
        .collect()
}

fn is_generated_chunk_path(path: &Path) -> bool {
    let Some(suffix) = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix("chunk_"))
    else {
        return false;
    };
    suffix.len() == 3 && suffix.chars().all(|character| character.is_ascii_digit())
}

#[test]
fn generated_component_meta_corpus_is_chunked_complete_and_nonduplicating() {
    let cases_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let corpus_root = cases_root.join("component_meta_audit_corpus");
    let fixtures_root = corpus_root.join("fixtures");

    let mut fixture_files = Vec::new();
    collect_vue_files(&fixtures_root, &mut fixture_files);
    let expected_all = fixture_files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&fixtures_root)
                .expect("fixture must remain below fixtures root")
                .to_string_lossy()
                .replace('\\', "/");
            (
                slug_for(&relative),
                GeneratedRow {
                    canonical: format!("/{relative}"),
                    include_path: format!("fixtures/{relative}"),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert!(
        !expected_all.is_empty(),
        "vendored corpus must not be empty"
    );

    let overrides = override_slugs(&corpus_root);
    let orphan_overrides = overrides
        .difference(&expected_all.keys().cloned().collect())
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        orphan_overrides.is_empty(),
        "every authored override must replace a vendored fixture slug: {orphan_overrides:?}"
    );
    let expected = expected_all
        .iter()
        .filter(|(slug, _)| !overrides.contains(*slug))
        .map(|(slug, row)| (slug.clone(), row.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut chunk_paths = std::fs::read_dir(&corpus_root)
        .expect("read generated corpus directory")
        .map(|entry| entry.expect("read generated corpus entry").path())
        .filter(|path| is_generated_chunk_path(path))
        .collect::<Vec<_>>();
    chunk_paths.sort();

    let expected_chunk_count = expected.len().div_ceil(CHUNK_SIZE);
    assert_eq!(
        chunk_paths.len(),
        expected_chunk_count,
        "generator must table the corpus into ceil(fixtures/{CHUNK_SIZE}) moderate chunks"
    );

    let mut occurrences = BTreeMap::<String, Vec<GeneratedRow>>::new();
    for path in &chunk_paths {
        let source = std::fs::read_to_string(path).expect("read generated chunk");
        assert_eq!(
            source.matches("#[test]").count(),
            1,
            "each generated chunk must contribute exactly one nextest process: {}",
            path.display()
        );
        let rows = source
            .lines()
            .filter(|line| line.contains("CorpusCase::new("))
            .map(|line| {
                generated_row(line).unwrap_or_else(|| {
                    panic!(
                        "malformed generated corpus row in {}: {line}",
                        path.display()
                    )
                })
            })
            .collect::<Vec<_>>();
        assert!(
            !rows.is_empty() && rows.len() <= CHUNK_SIZE,
            "chunk size must be within 1..={CHUNK_SIZE}: {}",
            path.display()
        );
        for (slug, row) in rows {
            occurrences.entry(slug).or_default().push(row);
        }
    }

    assert!(
        occurrences.values().all(|rows| rows.len() == 1),
        "each fixture slug must occur exactly once in the generated chunk lane: {occurrences:?}"
    );
    let actual = occurrences
        .iter()
        .map(|(slug, rows)| (slug.clone(), rows[0].clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        actual, expected,
        "every non-overridden fixture must occur exactly once with matching slug, canonical, and include path"
    );

    let entry = std::fs::read_to_string(cases_root.join("corpus_audit_tests.rs"))
        .expect("read generated corpus entry point");
    let entry_chunks = entry
        .lines()
        .filter_map(|line| line.trim().strip_prefix("mod ")?.strip_suffix(';'))
        .filter(|module| {
            module.strip_prefix("chunk_").is_some_and(|suffix| {
                suffix.len() == 3 && suffix.chars().all(|character| character.is_ascii_digit())
            })
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    let expected_chunk_modules = chunk_paths
        .iter()
        .map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .expect("chunk file stem must be UTF-8")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entry_chunks, expected_chunk_modules,
        "entry point must include every generated chunk exactly once and in order"
    );
    let entry_overrides = entry
        .lines()
        .filter_map(|line| line.trim().strip_prefix("mod override_")?.strip_suffix(';'))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        entry_overrides, overrides,
        "entry point must include every authored override exactly once"
    );

    let duplicate = std::fs::read_to_string(corpus_root.join("mod.rs"))
        .expect("read intentional Main.vue duplicate shim");
    assert_eq!(
        duplicate.matches("#[test]").count(),
        1,
        "the historical Main.vue duplicate remains one separate logical row"
    );
    assert!(
        duplicate.contains("include_str!(\"fixtures/Main.vue\")"),
        "the intentional duplicate must continue to exercise Main.vue"
    );
    assert!(
        actual.contains_key("main") || overrides.contains("main"),
        "Main.vue must occur once in the generated-or-override lane and once in the manual duplicate"
    );
}

#[test]
fn shared_test_worker_pools_reuse_execution_substrate_not_host_state() {
    let config = HostConfig::default();
    let pools = verter_session::TestHostWorkerPools::new(
        &config,
        verter_scheduler::scheduler::SchedulerConfig::default(),
    );
    let pool_ids = pools.pool_ids();

    let workspace_a = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    let workspace_b = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    let host_a = VerterHost::new_with_test_worker_pools(
        config.clone(),
        workspace_a.clone(),
        Arc::clone(&pools),
    );
    let host_a_id = host_a.test_instance_id();
    assert_eq!(host_a.test_worker_pool_ids(), pool_ids);

    let canonical = "/only-in-a.vue";
    let source = "<script setup lang=\"ts\">const onlyInA = 1</script>";
    let _ = host_a
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: host_a.language_classifier().classify(canonical),
            aliases: Vec::new(),
        })
        .expect("host A upsert must succeed");
    assert_eq!(workspace_a.read_file(canonical).as_deref(), Some(source));
    let overlapping = std::panic::catch_unwind(std::panic::AssertUnwindSafe({
        let config = config.clone();
        let workspace_b = workspace_b.clone();
        let pools = Arc::clone(&pools);
        move || VerterHost::new_with_test_worker_pools(config, workspace_b, pools)
    }));
    assert!(
        overlapping.is_err(),
        "a second live scheduler shell must be rejected before it can share the bounded I/O transport"
    );
    drop(host_a);
    assert_eq!(
        pools.receipt().active_scheduler_shells,
        0,
        "dropping host A must release the exclusive scheduler-shell lease"
    );

    let host_b =
        VerterHost::new_with_test_worker_pools(config, workspace_b.clone(), Arc::clone(&pools));
    assert_ne!(
        host_a_id,
        host_b.test_instance_id(),
        "shared execution pools must still create a fresh host identity"
    );
    assert_eq!(host_b.test_worker_pool_ids(), pool_ids);
    assert!(
        workspace_b.read_file(canonical).is_none(),
        "a shared worker substrate must not share workspace content"
    );
    assert!(
        host_b
            .get_component_meta_with_resolution(canonical)
            .is_none(),
        "a shared worker substrate must not share scheduler or semantic cache state"
    );
    drop(host_b);

    let receipt = pools.receipt();
    assert_eq!(receipt.host_shells_created, 2);
    assert_eq!(receipt.scheduler_shells_created, 2);
    assert_eq!(receipt.pool_ids, pool_ids);
    assert_eq!(receipt.active_scheduler_shells, 0);
}
