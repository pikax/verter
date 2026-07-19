//! Generator smoke + hermetic freshness guards for the vendored Vue RC
//! conformance goldens.
//!
//! These guards run in the DEFAULT canonical suite and are FULLY HERMETIC:
//! they read only committed files (corpus SFCs, vendored goldens, metadata,
//! manifest, the JS pin authority, and `pnpm-lock.yaml`). They NEVER shell
//! `node`, NEVER load the live Vue compiler, and NEVER require `pnpm install`.
//!
//! The LIVE regenerate-and-compare drift check lives behind the
//! `vue-oracle-live` feature (`generator_check_mode_reproduces_committed_goldens`
//! below) and in the JS `node packages/vue-conformance-oracle/gen-vue-goldens.mjs
//! --check` command; both fail loudly when Node or the pinned RC toolchain is
//! unavailable.
//!
//! Smoke coverage (Slice 1 — the structural comparator is a later slice):
//! - the generator has produced a golden + metadata for at least one VDOM and
//!   one Vapor case, and every committed golden is non-empty;
//! - every committed golden parses as valid ESM JavaScript;
//! - every metadata file records the pinned Vue RC version (single authority:
//!   `packages/vue-conformance-oracle/vue-golden-lib.mjs`);
//! - manifest ↔ corpus bijection and recorded SHA-256 hashes match the
//!   committed bytes;
//! - the lockfile resolves the exact pinned toolchain.

use std::collections::BTreeSet;
use std::path::PathBuf;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_vue_conformance::{
    case_sfc_paths, corpus_file, corpus_root, read_text_normalized, sha256_hex, workspace_root,
    Backend, Disposition, GoldenMeta, Manifest,
};

/// The single JS pin authority every Vue golden consumer reads.
fn oracle_lib_path() -> PathBuf {
    workspace_root()
        .join("packages")
        .join("vue-conformance-oracle")
        .join("vue-golden-lib.mjs")
}

fn oracle_lib_src() -> String {
    read_text_normalized(&oracle_lib_path()).expect("read vue-golden-lib.mjs")
}

/// Extract an `export const <NAME> = "…";` (string) or `= <digits>;` (number)
/// constant from the JS pin authority — the guards never re-declare versions.
fn js_lib_constant<'a>(lib_src: &'a str, name: &str) -> &'a str {
    for line in lib_src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix(&format!("export const {name}")) {
            let after_eq = rest.split('=').nth(1).expect("pin assignment has `=`");
            return after_eq
                .trim()
                .trim_end_matches(';')
                .trim()
                .trim_matches('"');
        }
    }
    panic!("{name} constant not found in vue-golden-lib.mjs");
}

const ORACLE_PACKAGES: [&str; 4] = [
    "vue",
    "@vue/compiler-dom",
    "@vue/compiler-sfc",
    "@vue/compiler-vapor",
];

fn load_manifest() -> Manifest {
    Manifest::load(&corpus_root()).expect("load corpus/manifest.json")
}

fn load_cell_meta(case_id: &str, backend: Backend, meta_rel: &str) -> GoldenMeta {
    let path = corpus_file(&corpus_root(), meta_rel);
    let meta = GoldenMeta::load(&path)
        .unwrap_or_else(|e| panic!("load meta for {case_id} [{backend:?}]: {e}"));
    assert_eq!(
        meta.case_id, case_id,
        "meta caseId must equal the manifest case id"
    );
    assert_eq!(
        meta.backend, backend,
        "meta backend must equal the manifest cell backend"
    );
    meta
}

/// The generator has produced a golden + metadata for at least one VDOM and
/// one Vapor case, and every committed compiled golden is non-empty.
#[test]
fn committed_corpus_covers_both_backends_with_non_empty_goldens() {
    let corpus = corpus_root();
    let manifest = load_manifest();
    assert!(!manifest.cases.is_empty(), "manifest must list seed cases");

    let mut vdom_compiled = 0usize;
    let mut vapor_compiled = 0usize;
    for case in &manifest.cases {
        for (backend, cell) in &case.backends {
            if cell.disposition != Disposition::Compiled {
                continue;
            }
            let golden_rel = cell
                .golden
                .as_ref()
                .expect("compiled cell must record a golden path");
            let code = read_text_normalized(&corpus_file(&corpus, golden_rel))
                .expect("read committed golden");
            assert!(
                !code.trim().is_empty() && code.contains("export"),
                "golden {golden_rel} must be a non-empty JS module"
            );
            match backend {
                Backend::Vdom => vdom_compiled += 1,
                Backend::Vapor => vapor_compiled += 1,
            }
        }
    }
    assert!(
        vdom_compiled >= 1,
        "at least one VDOM case must carry a compiled golden"
    );
    assert!(
        vapor_compiled >= 1,
        "at least one Vapor case must carry a compiled golden"
    );
}

/// Every committed compiled golden parses as valid ESM JavaScript.
#[test]
fn compiled_goldens_parse_as_valid_javascript() {
    let corpus = corpus_root();
    let manifest = load_manifest();
    let mut parsed = 0usize;
    for case in &manifest.cases {
        for cell in case.backends.values() {
            let Some(golden_rel) = &cell.golden else {
                continue;
            };
            let path = corpus_file(&corpus, golden_rel);
            let code = read_text_normalized(&path).expect("read golden");
            let allocator = Allocator::default();
            let ret = Parser::new(&allocator, &code, SourceType::mjs()).parse();
            assert!(
                !ret.panicked && ret.errors.is_empty(),
                "golden {golden_rel} must parse as valid ESM JavaScript: {:?}",
                ret.errors
            );
            parsed += 1;
        }
    }
    assert!(parsed >= 2, "expected at least one golden per backend");
}

/// Every metadata file records the pinned Vue RC version; the single version
/// authority is `VUE_ORACLE_VERSION` in `vue-golden-lib.mjs`.
#[test]
fn metadata_records_pinned_rc_versions() {
    let lib_src = oracle_lib_src();
    let vue_pin = js_lib_constant(&lib_src, "VUE_ORACLE_VERSION");
    let esbuild_pin = js_lib_constant(&lib_src, "ESBUILD_VERSION");
    let generator_version = js_lib_constant(&lib_src, "GENERATOR_VERSION");
    let meta_schema = js_lib_constant(&lib_src, "META_SCHEMA_VERSION");
    let manifest_schema = js_lib_constant(&lib_src, "MANIFEST_SCHEMA_VERSION");

    let manifest = load_manifest();
    assert_eq!(manifest.vue_version, vue_pin);
    assert_eq!(manifest.schema.to_string(), manifest_schema);
    assert_eq!(manifest.generator.version.to_string(), generator_version);
    for pkg in ORACLE_PACKAGES {
        assert_eq!(
            manifest.packages.get(pkg).map(String::as_str),
            Some(vue_pin),
            "manifest must pin {pkg} at the RC version"
        );
    }
    assert_eq!(
        manifest.packages.get("esbuild").map(String::as_str),
        Some(esbuild_pin)
    );

    for case in &manifest.cases {
        for (backend, cell) in &case.backends {
            let meta = load_cell_meta(&case.id, *backend, &cell.meta);
            assert_eq!(meta.schema.to_string(), meta_schema);
            assert_eq!(meta.generator.version.to_string(), generator_version);
            for pkg in ORACLE_PACKAGES {
                assert_eq!(
                    meta.versions.get(pkg).map(String::as_str),
                    Some(vue_pin),
                    "meta for {} [{backend:?}] must pin {pkg}",
                    case.id
                );
            }
            assert_eq!(
                meta.versions.get("esbuild").map(String::as_str),
                Some(esbuild_pin)
            );
            // The vendored golden dir is version-scoped: `goldens/<pin>/…`.
            if let Some(golden) = &cell.golden {
                assert!(
                    golden.starts_with(&format!("goldens/{vue_pin}/")),
                    "golden path {golden} must live under goldens/{vue_pin}/"
                );
            }
        }
    }
}

/// Manifest ↔ corpus bijection and recorded SHA-256 hashes match committed bytes.
#[test]
fn manifest_bijection_and_artifact_hashes() {
    let corpus = corpus_root();
    let manifest = load_manifest();

    // Bijection: every committed `cases/**.vue` is in the manifest, and vice versa.
    let on_disk = case_sfc_paths(&corpus).expect("walk corpus cases");
    let in_manifest: BTreeSet<String> = manifest.cases.iter().map(|c| c.sfc.clone()).collect();
    assert_eq!(
        on_disk, in_manifest,
        "manifest ↔ cases/ bijection broken (missing or stale entries)"
    );

    // Case ids are unique and each declares exactly the two backends.
    let ids: BTreeSet<&str> = manifest.cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), manifest.cases.len(), "duplicate case ids");
    for case in &manifest.cases {
        assert_eq!(
            case.backends.len(),
            Backend::ALL.len(),
            "case {} must declare vdom + vapor cells",
            case.id
        );

        let source = read_text_normalized(&corpus_file(&corpus, &case.sfc)).expect("read SFC");
        for (backend, cell) in &case.backends {
            let meta = load_cell_meta(&case.id, *backend, &cell.meta);

            // Manifest cell and metadata agree on the disposition and paths.
            assert_eq!(meta.disposition, cell.disposition);
            assert_eq!(meta.source.path, case.sfc);
            assert_eq!(
                meta.source.sha256,
                sha256_hex(&source),
                "source SHA-256 drift for {}",
                case.id
            );
            assert_eq!(meta.options.sha256.len(), 64);
            assert!(meta.options.sha256.chars().all(|c| c.is_ascii_hexdigit()));

            match cell.disposition {
                Disposition::Compiled => {
                    let golden_rel = cell.golden.as_ref().expect("compiled ⇒ golden path");
                    let map_rel = cell.map.as_ref().expect("compiled ⇒ map path");
                    let code_ref = meta
                        .artifacts
                        .code
                        .as_ref()
                        .expect("compiled ⇒ code artifact");
                    let map_ref = meta
                        .artifacts
                        .map
                        .as_ref()
                        .expect("compiled ⇒ map artifact");
                    assert_eq!(&code_ref.path, golden_rel);
                    assert_eq!(&map_ref.path, map_rel);

                    let code = read_text_normalized(&corpus_file(&corpus, golden_rel))
                        .expect("read golden");
                    let map =
                        read_text_normalized(&corpus_file(&corpus, map_rel)).expect("read map");
                    assert_eq!(
                        code_ref.sha256,
                        sha256_hex(&code),
                        "code hash drift: {golden_rel}"
                    );
                    assert_eq!(
                        code_ref.bytes.expect("compiled ⇒ byte length"),
                        code.len() as u64,
                        "code byte-length drift: {golden_rel}"
                    );
                    assert_eq!(
                        map_ref.sha256,
                        sha256_hex(&map),
                        "map hash drift: {map_rel}"
                    );
                }
                Disposition::Rejected => {
                    assert!(cell.golden.is_none() && cell.map.is_none());
                    assert!(meta.artifacts.code.is_none() && meta.artifacts.map.is_none());
                    assert!(
                        !meta.diagnostics.is_empty(),
                        "rejected cell {} [{backend:?}] must record diagnostics",
                        case.id
                    );
                }
            }
        }
    }
}

/// The lockfile resolves the exact pinned toolchain (silent drift impossible).
#[test]
fn lockfile_matches_oracle_pins() {
    let lib_src = oracle_lib_src();
    let vue_pin = js_lib_constant(&lib_src, "VUE_ORACLE_VERSION");
    let esbuild_pin = js_lib_constant(&lib_src, "ESBUILD_VERSION");
    let lock_src = read_text_normalized(&workspace_root().join("pnpm-lock.yaml"))
        .expect("read pnpm-lock.yaml");

    assert!(
        lock_src.contains("packages/vue-conformance-oracle:"),
        "lockfile must contain the oracle package importer"
    );
    for pkg in [
        "@vue/compiler-dom",
        "@vue/compiler-sfc",
        "@vue/compiler-vapor",
    ] {
        assert!(
            lock_src.contains(&format!("'{pkg}@{vue_pin}':")),
            "lockfile must resolve {pkg}@{vue_pin}"
        );
    }
    // Bare `vue@<pin>:` resolved package key (no peer suffix, no path entry).
    let vue_resolved = lock_src.lines().any(|line| {
        line.trim()
            .strip_prefix("vue@")
            .and_then(|rest| rest.strip_suffix(':'))
            .is_some_and(|v| v == vue_pin)
    });
    assert!(vue_resolved, "lockfile must resolve vue@{vue_pin}");
    assert!(
        lock_src.contains(&format!("esbuild@{esbuild_pin}:")),
        "lockfile must resolve esbuild@{esbuild_pin}"
    );
}

/// LIVE drift guard (opt-in): re-runs the JS generator in `--check` mode,
/// which regenerates every artifact in-memory and byte-compares against the
/// committed tree. Fails loudly when Node or the pinned toolchain is absent.
#[cfg(feature = "vue-oracle-live")]
#[test]
fn generator_check_mode_reproduces_committed_goldens() {
    let root = workspace_root();
    let script = root
        .join("packages")
        .join("vue-conformance-oracle")
        .join("gen-vue-goldens.mjs");
    let output = std::process::Command::new("node")
        .arg(script)
        .arg("--check")
        .current_dir(&root)
        .output()
        .expect(
            "vue-oracle-live requires `node` on PATH and the pinned RC toolchain \
             installed (`pnpm install`); both are mandatory for the live guard",
        );
    assert!(
        output.status.success(),
        "generator --check failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
