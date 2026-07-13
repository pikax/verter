//! Discriminating tests for the emit-plan builder and the CSS-manifest
//! fixture corpus writer/checker.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::generate::{
    build_plan, check_corpus, coverage_index_json, coverage_summary_md, default_corpus_root,
    plan_json, write_corpus, Drift, DriftKind,
};
use crate::manifest::{manifest, SCHEMA_VERSION};
use crate::model::{
    CompileTarget, CssSource, ElementRegion, ManifestCompileOptions, Quoting, SelectorKind,
    SelectorValueRepresentation, StructuralKind, Target, TemplateValueRepresentation,
};

/// The pinned selected-case count of the committed manifest. A change here is
/// a manifest change and must be deliberate (regenerate the corpus + goldens).
/// v2 grew the selection: the former slot-region refusal rows joined the
/// SUPPORTED coverage universe, widening the t-way obligations.
const PINNED_CASES: usize = 609;

/// The pinned full candidate product (6·7·4·3·3·6·2·5·3).
const PINNED_FULL_PRODUCT: u64 = 272_160;

fn parse(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("output parses as JSON")
}

/// Recursively snapshot every file under `root` as `/`-joined relative path →
/// bytes.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("under root")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(rel, std::fs::read(&path).expect("read file"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn temp_root(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("css-matrix-{tag}-"))
        .tempdir()
        .expect("create temp corpus root")
}

// ---------------------------------------------------------------------------
// Plan construction
// ---------------------------------------------------------------------------

#[test]
fn plan_is_deterministic_and_slug_ordered() {
    let first = build_plan();
    let second = build_plan();
    assert_eq!(plan_json(&first), plan_json(&second));
    assert_eq!(coverage_index_json(&first), coverage_index_json(&second));
    assert_eq!(coverage_summary_md(&first), coverage_summary_md(&second));

    // Strictly ascending slug order (implies uniqueness).
    for pair in first.entries.windows(2) {
        assert!(
            pair[0].slug < pair[1].slug,
            "entries must be strictly slug-ordered: {} !< {}",
            pair[0].slug,
            pair[1].slug
        );
    }
}

#[test]
fn plan_covers_every_manifest_case_with_full_backend_expansion() {
    let plan = build_plan();
    let manifest = manifest();
    assert_eq!(plan.entries.len(), manifest.cases().len());
    assert_eq!(plan.entries.len(), PINNED_CASES);
    assert_eq!(plan.schema_version, SCHEMA_VERSION);
    assert_eq!(plan.manifest_hash, manifest.manifest_hash());
    assert_eq!(plan.full_product, PINNED_FULL_PRODUCT);

    for entry in &plan.entries {
        let case = manifest
            .case_for_slug(&entry.slug)
            .unwrap_or_else(|| panic!("plan slug {} missing from manifest", entry.slug));
        assert!(!entry.source.is_empty(), "{}: empty source", entry.slug);
        assert_eq!(
            entry.source,
            case.render_source(),
            "{}: source must be the model rendering",
            entry.slug
        );
        assert!(
            entry.source.contains("<style>") && entry.source.contains("</style>"),
            "{}: fixture must carry its style block",
            entry.slug
        );
        assert_eq!(entry.compile_options, case.compile_options);
        assert_eq!(entry.disposition, case.disposition);
        assert_eq!(entry.expected_outcome, case.expected_outcome);
        assert_eq!(
            entry.backends,
            [CompileTarget::Client, CompileTarget::Server]
        );
    }
}

#[test]
fn every_generative_level_appears_in_the_plan() {
    let manifest = manifest();
    let plan = build_plan();
    let levels: Vec<_> = plan
        .entries
        .iter()
        .map(|entry| {
            manifest
                .case_for_slug(&entry.slug)
                .expect("plan slug resolves")
                .levels
        })
        .collect();

    for &kind in SelectorKind::ALL {
        assert!(
            levels.iter().any(|l| l.selector_kind == kind),
            "selector kind {kind:?} never selected"
        );
    }
    for &value in TemplateValueRepresentation::ALL {
        assert!(
            levels.iter().any(|l| l.template_value == value),
            "template representation {value:?} never selected"
        );
    }
    for &value in SelectorValueRepresentation::ALL {
        assert!(
            levels.iter().any(|l| l.selector_value == value),
            "selector representation {value:?} never selected"
        );
    }
    for &target in Target::ALL {
        assert!(
            levels.iter().any(|l| l.target == target),
            "target {target:?} never selected"
        );
    }
    for &quoting in Quoting::ALL {
        assert!(
            levels.iter().any(|l| l.quoting == quoting),
            "quoting {quoting:?} never selected"
        );
    }
    for &region in ElementRegion::ALL {
        assert!(
            levels.iter().any(|l| l.region == region),
            "region {region:?} never selected"
        );
    }
    for &source in CssSource::ALL {
        assert!(
            levels.iter().any(|l| l.css_source == source),
            "css source {source:?} never selected"
        );
    }
    for &structural in StructuralKind::ALL {
        assert!(
            levels.iter().any(|l| l.structural == structural),
            "structural kind {structural:?} never selected"
        );
    }
}

// ---------------------------------------------------------------------------
// emit-plan JSON wire shape
// ---------------------------------------------------------------------------

#[test]
fn plan_json_carries_manifest_identity_and_wire_shape() {
    let plan = build_plan();
    let text = plan_json(&plan);
    assert!(text.ends_with('\n'), "plan JSON must end with a newline");

    let value = parse(&text);
    assert_eq!(value["schemaVersion"], u64::from(SCHEMA_VERSION));
    assert_eq!(value["manifestHash"], manifest().manifest_hash());

    let cases = value["cases"].as_array().expect("cases array");
    assert_eq!(cases.len(), PINNED_CASES);

    // Stable key order on the wire (Value parsing re-sorts keys, so the
    // order is asserted on the serialized text itself).
    let key_positions: Vec<usize> = [
        "\"slug\":",
        "\"source\":",
        "\"compileOptions\":",
        "\"disposition\":",
        "\"expectedOutcome\":",
        "\"backends\":",
    ]
    .iter()
    .map(|needle| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle} on wire"))
    })
    .collect();
    assert!(
        key_positions.windows(2).all(|pair| pair[0] < pair[1]),
        "per-case keys must serialize in the declared stable order"
    );

    let mut dispositions = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    for case in cases {
        let object = case.as_object().expect("case object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "backends",
                "compileOptions",
                "disposition",
                "expectedOutcome",
                "slug",
                "source"
            ],
            "closed per-case key set"
        );
        assert!(!object["slug"].as_str().expect("slug string").is_empty());
        assert!(!object["source"].as_str().expect("source string").is_empty());
        assert!(object["compileOptions"].is_object());
        assert_eq!(
            object["backends"],
            serde_json::json!(["client", "server"]),
            "every case expands to both backends"
        );
        dispositions.insert(object["disposition"].as_str().expect("str").to_string());
        outcomes.insert(object["expectedOutcome"].as_str().expect("str").to_string());
    }

    let known_dispositions: BTreeSet<String> = [
        "supported",
        "oracle-rejected:css-nesting-selector-invalid-placement",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        dispositions, known_dispositions,
        "both inhabited partitions must surface in the plan, and nothing else"
    );

    let known_outcomes: BTreeSet<String> = ["match", "no-match", "maybe"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert_eq!(
        outcomes, known_outcomes,
        "all three declared outcomes must surface in the plan, and nothing else"
    );
}

#[test]
fn compile_options_wire_form_matches_typed_authority() {
    for custom_element in [false, true] {
        for filename_undefined in [false, true] {
            let options = ManifestCompileOptions {
                custom_element,
                filename_undefined,
            };
            let wire = crate::generate::compile_options_wire(&options);
            assert_eq!(
                wire.get(),
                options.to_json(),
                "wire compile options must be byte-identical to the typed rendering"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// coverage-index.json
// ---------------------------------------------------------------------------

#[test]
fn coverage_index_proof_is_non_vacuous() {
    let plan = build_plan();
    let text = coverage_index_json(&plan);
    assert!(text.ends_with('\n'));
    let value = parse(&text);

    assert_eq!(value["schemaVersion"], u64::from(SCHEMA_VERSION));
    assert_eq!(value["manifestHash"], manifest().manifest_hash());

    let cases = value["cases"].as_array().expect("cases array");
    assert_eq!(cases.len(), PINNED_CASES);
    for case in cases {
        let object = case.as_object().expect("case object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["backends", "disposition", "expectedOutcome", "slug"],
            "index cases are the light projection (no source)"
        );
    }

    let proof = &value["proof"];
    assert_eq!(proof["selectedCases"], PINNED_CASES as u64);
    assert_eq!(proof["fullProduct"], PINNED_FULL_PRODUCT);
    assert_eq!(
        proof["compression"],
        format!("{PINNED_CASES}/{PINNED_FULL_PRODUCT}")
    );

    // Per-partition counts equal the manifest's full-space inventories.
    let manifest = manifest();
    let partitions = &proof["partitions"];
    assert_eq!(partitions["supported"], manifest.supported_row_count());
    for (kind, count) in manifest.refused_inventory() {
        assert_eq!(partitions["refused"][kind.id()], *count);
    }
    for (kind, count) in manifest.oracle_rejected_inventory() {
        assert_eq!(partitions["oracleRejected"][kind.id()], *count);
    }
    for (kind, count) in manifest.invalid_inventory() {
        assert_eq!(partitions["invalid"][kind.id()], *count);
    }

    // The four partitions tile the full candidate space exactly.
    let sum_kinds = |name: &str| -> u64 {
        partitions[name]
            .as_object()
            .unwrap_or_else(|| panic!("partitions.{name} object"))
            .values()
            .map(|v| v.as_u64().expect("count"))
            .sum()
    };
    let total = partitions["supported"].as_u64().expect("count")
        + sum_kinds("refused")
        + sum_kinds("oracleRejected")
        + sum_kinds("invalid");
    assert_eq!(total, PINNED_FULL_PRODUCT, "partitions must tile the space");

    // Strengthened groups are present, named, and carry their strengths.
    let groups = proof["groups"].as_array().expect("groups array");
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["strength"], 5);
    assert_eq!(
        groups[0]["factors"],
        serde_json::json!([
            "template-value",
            "target",
            "quoting",
            "element-region",
            "match-outcome"
        ])
    );
    assert_eq!(groups[1]["strength"], 4);
    assert_eq!(
        groups[1]["factors"],
        serde_json::json!([
            "selector-kind",
            "selector-value",
            "structural-kind",
            "match-outcome"
        ])
    );

    // The independently verified covering proof rides along verbatim.
    let covering = proof["coveringProof"].as_str().expect("proof text");
    assert!(covering.contains("covering-array proof"));
    assert!(covering.contains("refusal-partitions:"));
}

#[test]
fn coverage_summary_projects_axes_partitions_and_groups() {
    let plan = build_plan();
    let text = coverage_summary_md(&plan);
    assert!(text.ends_with('\n'));

    assert!(text.contains(manifest().manifest_hash()));
    // The combined representation-axes projection (template × selector).
    for &value in TemplateValueRepresentation::ALL {
        assert!(
            text.contains(&format!("`{}`", value.id())),
            "summary must project template representation {value:?}"
        );
    }
    for &value in SelectorValueRepresentation::ALL {
        assert!(
            text.contains(&format!("`{}`", value.id())),
            "summary must project selector representation {value:?}"
        );
    }
    // Per-partition tallies and the strengthened-group inventory.
    assert!(text.contains("supported"));
    assert!(
        !text.contains("legacy-slot-scope-unprovable"),
        "the retired refusal id must not resurface in the summary"
    );
    assert!(text.contains("css-nesting-selector-invalid-placement"));
    assert!(text.contains("strength 5"));
    assert!(text.contains("strength 4"));
    assert!(text.contains(&PINNED_CASES.to_string()));
    assert!(text.contains(&PINNED_FULL_PRODUCT.to_string()));
    // The summary documents the crate-owned corpus layout.
    assert!(text.contains("fixtures: `fixtures/<slug>.svelte`"));
    assert!(
        !text.contains("css_matrix"),
        "summary must not reference the retired shared-oracle-corpus layout"
    );
}

// ---------------------------------------------------------------------------
// Corpus write / check
// ---------------------------------------------------------------------------

#[test]
fn write_then_check_roundtrip_is_clean_and_idempotent() {
    let root = temp_root("roundtrip");
    let report = write_corpus(root.path()).expect("write corpus");
    assert_eq!(report.fixtures_written, PINNED_CASES);
    assert_eq!(check_corpus(root.path()), Ok(()));

    let first = snapshot(root.path());
    write_corpus(root.path()).expect("rewrite corpus");
    let second = snapshot(root.path());
    assert_eq!(first, second, "write must be byte-idempotent");

    // Exactly the expected file set: 518 flat fixtures + index + summary.
    assert_eq!(first.len(), PINNED_CASES + 2);
    assert!(first.contains_key("coverage-index.json"));
    assert!(first.contains_key("coverage-summary.md"));
    let plan = build_plan();
    for entry in &plan.entries {
        assert!(
            first.contains_key(&format!("fixtures/{}.svelte", entry.slug)),
            "fixture for {} must sit flat under fixtures/",
            entry.slug
        );
    }
}

#[test]
fn check_reports_exact_drift_kinds() {
    // Content drift.
    let root = temp_root("drift");
    write_corpus(root.path()).expect("write corpus");
    let victim_rel = {
        let plan = build_plan();
        format!("fixtures/{}.svelte", plan.entries[0].slug)
    };
    let victim = root
        .path()
        .join(victim_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let mut bytes = std::fs::read(&victim).expect("read victim");
    bytes.extend_from_slice(b"<!-- mutated -->\n");
    std::fs::write(&victim, bytes).expect("mutate victim");
    assert_eq!(
        check_corpus(root.path()),
        Err(vec![Drift {
            kind: DriftKind::Drifted,
            path: victim_rel.clone(),
        }]),
        "a mutated fixture must be the one exact DRIFTED entry"
    );

    // Missing fixture.
    write_corpus(root.path()).expect("restore");
    std::fs::remove_file(&victim).expect("delete victim");
    assert_eq!(
        check_corpus(root.path()),
        Err(vec![Drift {
            kind: DriftKind::Missing,
            path: victim_rel.clone(),
        }])
    );

    // Stale orphan under fixtures/.
    write_corpus(root.path()).expect("restore");
    let orphan = root.path().join("fixtures").join("zzz-orphan.svelte");
    std::fs::write(&orphan, "<div>orphan</div>\n").expect("write orphan");
    assert_eq!(
        check_corpus(root.path()),
        Err(vec![Drift {
            kind: DriftKind::Stale,
            path: "fixtures/zzz-orphan.svelte".to_string(),
        }])
    );

    // Corrupted review index.
    write_corpus(root.path()).expect("restore");
    std::fs::write(root.path().join("coverage-index.json"), "{}\n").expect("corrupt index");
    assert_eq!(
        check_corpus(root.path()),
        Err(vec![Drift {
            kind: DriftKind::Drifted,
            path: "coverage-index.json".to_string(),
        }])
    );

    // Missing summary.
    write_corpus(root.path()).expect("restore");
    std::fs::remove_file(root.path().join("coverage-summary.md")).expect("delete summary");
    assert_eq!(
        check_corpus(root.path()),
        Err(vec![Drift {
            kind: DriftKind::Missing,
            path: "coverage-summary.md".to_string(),
        }])
    );
}

#[test]
fn write_touches_only_the_owned_corpus_surface() {
    let root = temp_root("isolation");
    // A corpus sibling outside `fixtures/` (the goldens home the Node façade
    // will populate) that must survive byte-identically.
    let golden_keep = root.path().join("goldens").join("keep.client.json");
    std::fs::create_dir_all(golden_keep.parent().expect("parent")).expect("mkdir");
    std::fs::write(&golden_keep, "{\"slug\":\"keep\"}\n").expect("seed sibling");
    // Stale prior fixtures — flat, and under the retired nested layout — that
    // the clean rewrite must remove.
    let stale = root.path().join("fixtures").join("old-retired-case.svelte");
    let stale_nested = root
        .path()
        .join("fixtures")
        .join("css_matrix")
        .join("old-layout-case.svelte");
    for (path, contents) in [
        (&stale, "<div>old</div>\n"),
        (&stale_nested, "<div>older</div>\n"),
    ] {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, contents).expect("seed stale");
    }

    write_corpus(root.path()).expect("write corpus");

    assert_eq!(
        std::fs::read_to_string(&golden_keep).expect("golden sibling survives"),
        "{\"slug\":\"keep\"}\n"
    );
    assert!(!stale.exists(), "clean rewrite must remove stale fixtures");
    assert!(
        !stale_nested.exists(),
        "clean rewrite must remove the retired nested layout"
    );

    // Nothing outside `fixtures/` + the two root review artifacts was
    // written; the golden sibling is the only survivor beyond them.
    let plan = build_plan();
    let expected: BTreeSet<String> = plan
        .entries
        .iter()
        .map(|entry| format!("fixtures/{}.svelte", entry.slug))
        .chain([
            "coverage-index.json".to_string(),
            "coverage-summary.md".to_string(),
            "goldens/keep.client.json".to_string(),
        ])
        .collect();
    let on_disk: BTreeSet<String> = snapshot(root.path()).into_keys().collect();
    assert_eq!(on_disk, expected, "write escaped its owned surface");
}

#[test]
fn fixture_paths_are_portable_and_unique() {
    let plan = build_plan();
    let mut seen = BTreeSet::new();
    for entry in &plan.entries {
        assert!(
            !entry.slug.is_empty()
                && entry
                    .slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "slug {} is not path-safe",
            entry.slug
        );
        assert!(
            seen.insert(entry.slug.clone()),
            "duplicate slug {}",
            entry.slug
        );

        // The full repo-relative path stays within the portable-path budget
        // enforced by `tracked_paths_are_portable`.
        let repo_relative = format!(
            "crates/verter_svelte_conformance/corpus/fixtures/{}.svelte",
            entry.slug
        );
        assert!(
            repo_relative.len() <= 200,
            "{repo_relative} exceeds the 200-byte tracked-path budget"
        );
    }
}

// ---------------------------------------------------------------------------
// Committed-corpus drift guard
// ---------------------------------------------------------------------------

#[test]
fn corpus_matches_committed() {
    let root = default_corpus_root();
    assert!(
        root.is_dir(),
        "conformance corpus root missing: {}",
        root.display()
    );
    if let Err(drifts) = check_corpus(&root) {
        let rendered: Vec<String> = drifts.iter().map(Drift::render).collect();
        panic!(
            "committed conformance corpus drifted from the manifest ({} findings; \
             regenerate with `cargo run -p verter_svelte_conformance -- write`):\n{}",
            rendered.len(),
            rendered.join("\n")
        );
    }
}

/// `default_corpus_root` must resolve to the conformance crate's own
/// committed corpus (guards the path wiring against crate moves and against
/// regressing to the shared verter_compiler oracle corpus).
#[test]
fn default_corpus_root_points_at_the_conformance_corpus() {
    let root = default_corpus_root();
    assert_eq!(
        root,
        Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus"),
        "corpus root must be the crate-owned corpus/ dir"
    );
    assert!(
        !root
            .components()
            .any(|c| c.as_os_str() == "verter_compiler"),
        "corpus root must not point into the shared oracle corpus: {}",
        root.display()
    );
    assert!(
        root.join("fixtures").is_dir(),
        "corpus root must contain the fixtures subtree: {}",
        root.display()
    );
}
