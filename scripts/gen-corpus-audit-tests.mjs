#!/usr/bin/env node
/**
 * Generator — corpus audit tests.
 *
 * Sweeps the vendored Vue fixture set under
 * `crates/verter_session/tests/component_meta_audit_corpus/fixtures/`
 * (recursive; sorted lexicographically for deterministic cross-platform
 * output) and writes one test file per component under
 * `crates/verter_session/tests/component_meta_audit_corpus/`, plus a
 * `corpus_audit_tests.rs` stitching module. An `overrides/<slug>.rs`
 * file, when present, is preferred over the generated stub — use
 * overrides to pin component-specific assertions that the generator
 * can't produce automatically.
 *
 * Fixture provenance is documented in
 * `crates/verter_session/tests/component_meta_audit_corpus/fixtures/README.md`.
 * Vendoring keeps the default `cargo test --workspace --tests` run
 * hermetic; tests that need the live `.integration-tests/repos/...`
 * clone are gated behind the `external-corpus` Cargo feature.
 *
 * ## Modes
 *
 * - default: write test skeletons that resolve each component with
 *   `AuditedRequest::builder().files([(canonical, src)])` and assert
 *   the audit record is produced. Pinned snapshots live in overrides
 *   (so the generator itself never needs to regenerate them).
 * - `--dry-run --output-dir=<path>`: write the same generated tree to
 *   `<path>` instead of `crates/verter_session/tests/`. Used by the
 *   `corpus_generator_output_matches_committed_files` parity test.
 *
 * `--update-snapshots` is NOT offered. Per-component pinned snapshots
 * would require running each generated test in turn and capturing
 * the audit bundle — a meaningfully large operation beyond this
 * generator's scope. Reviewer guidance (plan §3 Commit 13 test list
 * item): if a reviewer needs a batch snapshot refresh, author the
 * override manually under `overrides/<slug>.rs` and commit. The
 * generator stays side-effect-free.
 *
 * ## Usage
 *
 *     node scripts/gen-corpus-audit-tests.mjs
 *     node scripts/gen-corpus-audit-tests.mjs --dry-run --output-dir=/tmp/corpus
 *
 * Plan §3 Commit 12 / F10.
 */

import { mkdirSync, readdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = resolve(__dirname, "..");

const DEFAULT_OUTPUT_DIR = resolve(REPO_ROOT, "crates/verter_session/tests");
const TEST_SUBDIR = "component_meta_audit_corpus";
const FIXTURES_SUBDIR = "fixtures";

// Vendored Vue fixtures live under
// `tests/component_meta_audit_corpus/fixtures/` so the generator
// (and the tests it produces) operate hermetically from a fresh
// checkout. See the directory's README.md for provenance.
const COMPONENTS_ROOT = resolve(DEFAULT_OUTPUT_DIR, TEST_SUBDIR, FIXTURES_SUBDIR);
// Entry-point file name — must differ from the subdir's stem to
// avoid cargo's test-target name-collision (it picks up both
// `tests/foo.rs` and `tests/foo/` as potential targets named "foo").
const ENTRY_STEM = "corpus_audit_tests";
const OVERRIDES_SUBDIR = "overrides";

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const config = {
    dryRun: false,
    outputDir: DEFAULT_OUTPUT_DIR,
  };
  for (const arg of argv) {
    if (arg === "--dry-run") config.dryRun = true;
    else if (arg.startsWith("--output-dir=")) {
      config.outputDir = resolve(arg.slice("--output-dir=".length));
    } else if (arg === "--update-snapshots") {
      console.error(
        "error: --update-snapshots is not implemented. Author per-component overrides under " +
          "`crates/verter_session/tests/component_meta_audit_corpus/overrides/<slug>.rs` and " +
          "commit them. See the generator docblock for guidance.",
      );
      process.exit(2);
    }
  }
  return config;
}

// ---------------------------------------------------------------------------
// Discovery — recursive, sorted
// ---------------------------------------------------------------------------

function discoverVueFiles(root) {
  const found = [];
  const stack = [root];
  while (stack.length > 0) {
    const dir = stack.pop();
    const entries = readdirSync(dir, { withFileTypes: true });
    for (const e of entries) {
      // README.md / LICENSE.md / overrides/ etc. are not Vue
      // fixtures.
      if (e.name === OVERRIDES_SUBDIR) continue;
      const p = join(dir, e.name);
      if (e.isDirectory()) stack.push(p);
      else if (e.isFile() && e.name.endsWith(".vue")) found.push(p);
    }
  }
  return found.sort();
}

// ---------------------------------------------------------------------------
// Codegen — per-component test file
// ---------------------------------------------------------------------------

/** Turn a file path into a snake_case Rust identifier slug. */
function slugFor(absPath) {
  const rel = relative(COMPONENTS_ROOT, absPath).replace(/\\/g, "/");
  const noExt = rel.replace(/\.vue$/, "");
  return noExt
    .replace(/[\/-]/g, "_")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .toLowerCase();
}

/** Canonical id used inside the AuditedRequest harness. */
function canonicalForComponent(absPath) {
  const name = relative(COMPONENTS_ROOT, absPath).replace(/\\/g, "/");
  return `/${name}`;
}

/** Render the per-component test file body. */
function renderTestBody(absPath) {
  const slug = slugFor(absPath);
  const canonical = canonicalForComponent(absPath);
  const relToCrateTestDir = relative(resolve(DEFAULT_OUTPUT_DIR, TEST_SUBDIR), absPath).replace(
    /\\/g,
    "/",
  );

  // Template matches rustfmt output (include_str! multi-line form for
  // long paths) so regeneration + cargo fmt produces a stable result.
  return `//! Generated by scripts/gen-corpus-audit-tests.mjs. Do not edit.
//!
//! Corpus coverage for the vendored Vue fixture at \`${canonical}\`.
//! The assertion posture is minimal but discriminating: the audit
//! record must be produced and the footprint attached when
//! resolution succeeds; the ONLY tolerated error variant is
//! \`ResolutionFailed\` (hermetic setup missing transitive deps).
//! Any other \`AuditedRequestError\` variant is a genuine
//! audit-wiring regression and panics the test.

use verter_session::audited_request::{AuditedRequest, AuditedRequestError};

#[test]
fn corpus_audit_${slug}_produces_audit_record_or_documents_skip() {
    let src = include_str!("${relToCrateTestDir}");
    let result = AuditedRequest::builder()
        .files([("${canonical}", src)])
        .resolve_component_meta("${canonical}");

    match result {
        Ok((_, _, record)) => {
            assert_eq!(
                record.canonical_id, "${canonical}",
                "audit record must identify the requested canonical",
            );
            // Hermetic \`AuditedRequest\` always enables footprint
            // capture — the miner MUST attach a footprint on
            // resolution success. Discriminating: fails if capture
            // wiring regresses, or if a future refactor accidentally
            // drops the miner call for this code path. Would NOT fail
            // for benign partial analysis (missing deps in the hermetic
            // setup) because the footprint attaches regardless of
            // analysis depth.
            assert!(
                record.footprint.is_some(),
                "hermetic AuditedRequest must attach Some(footprint) on resolution success",
            );
        }
        Err(AuditedRequestError::ResolutionFailed) => {
            // Benign: hermetic fixture lacks transitive deps, so
            // \`get_component_meta_with_resolution\` returned
            // \`None\`. This is the ONLY error variant we treat as
            // skip — every other variant is a genuine regression
            // (nested-audit guard, multi-request counter, audit
            // record missing from store, config validation).
            eprintln!(
                "corpus_audit_${slug}: hermetic resolution returned None (missing deps) — documenting skip",
            );
        }
        Err(other) => panic!(
            "corpus_audit_${slug}: unexpected audit error — this indicates an audit-wiring regression, not a hermetic-dep gap: {other:?}",
        ),
    }
}
`;
}

// ---------------------------------------------------------------------------
// mod.rs generator
// ---------------------------------------------------------------------------

function renderEntryPointRs(testSlugs) {
  // The cargo integration test target is `tests/corpus_audit_tests.rs`.
  // Each `mod` line pulls in one per-component test file via
  // `#[path = ...]` — matching the sibling `component_meta_audit.rs`
  // pattern.
  const lines = [
    "//! Generated by scripts/gen-corpus-audit-tests.mjs. Do not edit.",
    "//!",
    "//! Stitches every per-component corpus test file into one cargo",
    "//! integration test target so `cargo test -p verter_session",
    "//! --test corpus_audit_tests` discovers every generated slug.",
    "",
  ];
  for (const slug of testSlugs) {
    lines.push(`#[path = "component_meta_audit_corpus/${slug}.rs"]`);
    lines.push(`mod ${slug};`);
  }
  lines.push("");
  return lines.join("\n");
}

function renderCorpusReadme(count) {
  return `# Corpus audit tests

Auto-generated by \`scripts/gen-corpus-audit-tests.mjs\`.

At the time of generation there were **${count}** vendored
\`.vue\` fixtures under
\`crates/verter_session/tests/component_meta_audit_corpus/fixtures/\`;
each produces one test file here. Fixture provenance and license
are documented in \`fixtures/README.md\`.

## Regenerating

\`\`\`bash
node scripts/gen-corpus-audit-tests.mjs
\`\`\`

## Overrides

Place \`overrides/<slug>.rs\` to pin component-specific assertions
the generator cannot produce automatically. Overrides replace the
generated stub entirely for the matching slug. At landing the
override directory is empty — the generated stubs cover the
corpus-wide "audit record produced + footprint attached"
invariants; author an override when a component needs sharper
assertions that the generator cannot derive automatically.
`;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const config = parseArgs(process.argv.slice(2));

  const vueFiles = discoverVueFiles(COMPONENTS_ROOT);
  if (vueFiles.length === 0) {
    console.error(
      `No .vue files found under ${COMPONENTS_ROOT}. Is the integration-tests submodule present?`,
    );
    process.exit(1);
  }

  const testDir = resolve(config.outputDir, TEST_SUBDIR);
  const overridesDir = resolve(testDir, OVERRIDES_SUBDIR);

  // Clean regeneration — remove any prior generated files, preserve
  // overrides/, fixtures/, README, and the hand-written harness.rs.
  // harness.rs is the cross-component regression capture harness that
  // lives alongside the generated per-component tests; it is not
  // generator output and must survive regeneration.
  if (!config.dryRun) {
    try {
      for (const entry of readdirSync(testDir)) {
        if (
          entry === OVERRIDES_SUBDIR ||
          entry === FIXTURES_SUBDIR ||
          entry === "README.md" ||
          entry === "harness.rs"
        ) {
          continue;
        }
        rmSync(resolve(testDir, entry), { force: true });
      }
    } catch {
      // First run — dir may not exist yet.
    }
  }

  mkdirSync(testDir, { recursive: true });
  mkdirSync(overridesDir, { recursive: true });

  const slugs = [];
  const overrideSlugs = new Set(
    (() => {
      try {
        return readdirSync(overridesDir)
          .filter((n) => n.endsWith(".rs"))
          .map((n) => n.replace(/\.rs$/, ""));
      } catch {
        return [];
      }
    })(),
  );

  for (const vue of vueFiles) {
    const slug = slugFor(vue);
    slugs.push(slug);
    if (overrideSlugs.has(slug)) continue; // override wins
    const body = renderTestBody(vue);
    writeFileSync(resolve(testDir, `${slug}.rs`), body);
  }

  // Add overrides to the module graph even when their file is not
  // generated from the .vue scan — they're authored pins.
  const allSlugs = Array.from(new Set([...slugs, ...overrideSlugs])).sort();

  // Cargo integration test target at `tests/corpus_audit_tests.rs`.
  // The stem differs from the subdir's `component_meta_audit_corpus`
  // because cargo auto-discovers `tests/<name>.rs` AND `tests/<name>/`
  // as candidates for the same target name, raising a duplicate-name
  // error when both exist.
  writeFileSync(resolve(config.outputDir, `${ENTRY_STEM}.rs`), renderEntryPointRs(allSlugs));
  writeFileSync(resolve(testDir, "README.md"), renderCorpusReadme(vueFiles.length));

  // overrides/ gitkeep so the directory exists in fresh checkouts.
  writeFileSync(resolve(overridesDir, ".gitkeep"), "");

  if (!config.dryRun) {
    console.log(`Generated ${allSlugs.length} corpus tests into ${relative(REPO_ROOT, testDir)}/`);
  }
}

main();
