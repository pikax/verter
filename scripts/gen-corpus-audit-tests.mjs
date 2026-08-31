#!/usr/bin/env node
/**
 * Generator — corpus audit tests.
 *
 * Sweeps the vendored Vue fixture set under
 * `crates/verter_session/tests/cases/component_meta_audit_corpus/fixtures/`
 * (recursive; sorted lexicographically for deterministic cross-platform
 * output) and writes moderate table-test chunks under
 * `crates/verter_session/tests/cases/component_meta_audit_corpus/`, plus a
 * `corpus_audit_tests.rs` stitching module. The canonical gate uses nextest's
 * per-`#[test]` process isolation, so a file/test per fixture would defeat any
 * process-local immutable/runtime sharing. Each generated chunk is one test
 * process and still constructs a fresh host shell for every logical row.
 * An `overrides/<slug>.rs` file, when present, remains a standalone authored
 * test module — use overrides to pin assertions the table runner cannot express.
 *
 * Fixture provenance is documented in
 * `crates/verter_session/tests/cases/component_meta_audit_corpus/fixtures/README.md`.
 * Vendoring keeps the default `cargo test --workspace --tests` run
 * hermetic; tests that need the live `.integration-tests/repos/...`
 * clone are gated behind the `external-corpus` Cargo feature.
 *
 * ## Modes
 *
 * - default: write chunked table tests that resolve each component with
 *   `AuditedRequest::builder().files([(canonical, src)])` and assert
 *   the audit record is produced. Pinned snapshots live in overrides
 *   (so the generator itself never needs to regenerate them).
 * - `--dry-run --output-dir=<path>`: write the same generated tree to
 *   `<path>` instead of `crates/verter_session/tests/cases/`. Used by the
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

import { mkdirSync, readdirSync, writeFileSync, rmSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = resolve(__dirname, "..");

const DEFAULT_OUTPUT_DIR = resolve(REPO_ROOT, "crates/verter_session/tests/cases");
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
// Moderate enough to retain useful nextest parallelism/failure locality while
// amortising process-local worker pools over sixteen fresh host shells.
const CORPUS_CHUNK_SIZE = 16;

// ---------------------------------------------------------------------------
// Corpus configuration table
// ---------------------------------------------------------------------------
//
// One row per framework corpus the generator sweeps. ONE idempotent
// script sweeps every corpus, so every later framework vertical adds a
// ROW here instead of forking a parallel generator (no generator
// divergence). Each row declares:
//
//   - frameworkId: the framework adapter id (drives the request shape).
//   - fileExtension: the carrier file extension the sweep discovers.
//   - requestShape: how the generated test resolves each component
//     (`componentMeta` drives `AuditedRequest::...resolve_component_meta`).
//   - testSubdir / fixturesSubdir / entryStem: the output layout.
//
// At the compiler scaffold's landing the ONLY corpus is the vendored Vue
// fixture set, so the table has a single row that reproduces the
// historical single-corpus output byte-for-byte. The parity +
// idempotency tests pin that re-running the generator produces no diff.
const CORPUS_CONFIGS = [
  {
    frameworkId: "vue",
    fileExtension: ".vue",
    requestShape: "componentMeta",
    testSubdir: TEST_SUBDIR,
    fixturesSubdir: FIXTURES_SUBDIR,
    entryStem: ENTRY_STEM,
  },
];

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
          "`crates/verter_session/tests/cases/component_meta_audit_corpus/overrides/<slug>.rs` and " +
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

function discoverComponentFiles(root, fileExtension) {
  const found = [];
  const stack = [root];
  while (stack.length > 0) {
    const dir = stack.pop();
    const entries = readdirSync(dir, { withFileTypes: true });
    for (const e of entries) {
      // README.md / LICENSE.md / overrides/ etc. are not carrier
      // fixtures.
      if (e.name === OVERRIDES_SUBDIR) continue;
      const p = join(dir, e.name);
      if (e.isDirectory()) stack.push(p);
      else if (e.isFile() && e.name.endsWith(fileExtension)) found.push(p);
    }
  }
  return found.sort();
}

// ---------------------------------------------------------------------------
// Codegen — moderate table-test chunks
// ---------------------------------------------------------------------------

/** Turn a file path into a snake_case Rust identifier slug. */
function slugFor(absPath, componentsRoot, fileExtension) {
  const rel = relative(componentsRoot, absPath).replace(/\\/g, "/");
  const escExt = fileExtension.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const noExt = rel.replace(new RegExp(`${escExt}$`), "");
  return noExt
    .replace(/[\/-]/g, "_")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .toLowerCase();
}

/** Canonical id used inside the AuditedRequest harness. */
function canonicalForComponent(absPath, componentsRoot) {
  const name = relative(componentsRoot, absPath).replace(/\\/g, "/");
  return `/${name}`;
}

/** Render one chunk. Every `CorpusCase::new` stays on one line so the
 *  hand-written layout discriminator can count/compare logical rows without
 *  trusting this generator's own manifest. */
function renderChunkBody(chunkIndex, componentFiles, config) {
  const { componentsRoot, fileExtension, testSubdir } = config;
  const chunkSlug = `chunk_${String(chunkIndex).padStart(3, "0")}`;
  const lines = [
    "//! Generated by scripts/gen-corpus-audit-tests.mjs. Do not edit.",
    "//!",
    "//! One nextest process covering several hermetic logical corpus rows.",
    "//! Worker pools are shared; every row gets a fresh host and semantic state.",
    "",
    "use super::chunk_harness::{run_chunk, CorpusCase};",
    "",
    "#[test]",
    `fn corpus_audit_${chunkSlug}_produces_audit_records_or_documents_skips() {`,
    "    #[rustfmt::skip]",
    "    const CASES: &[CorpusCase] = &[",
  ];
  for (const absPath of componentFiles) {
    const slug = slugFor(absPath, componentsRoot, fileExtension);
    const canonical = canonicalForComponent(absPath, componentsRoot);
    const relToCrateTestDir = relative(resolve(DEFAULT_OUTPUT_DIR, testSubdir), absPath).replace(
      /\\/g,
      "/",
    );
    lines.push(
      `        CorpusCase::new("${slug}", "${canonical}", include_str!("${relToCrateTestDir}")),`,
    );
  }
  lines.push("    ];");
  lines.push("    run_chunk(CASES);");
  lines.push("}");
  lines.push("");
  return { chunkSlug, body: lines.join("\n") };
}

// ---------------------------------------------------------------------------
// mod.rs generator
// ---------------------------------------------------------------------------

function renderEntryPointRs(chunkSlugs, overrideSlugs, testSubdir) {
  // The committed entry point is `tests/cases/corpus_audit_tests.rs`,
  // compiled as a submodule of the consolidated `main` integration
  // binary. Every chunk is one nextest process. The hand-written harness owns
  // the fresh-host/shared-worker-pool boundary and is intentionally preserved
  // across regeneration.
  const lines = [
    "//! Generated by scripts/gen-corpus-audit-tests.mjs. Do not edit.",
    "//!",
    "//! Stitches moderate table-test chunks into the consolidated session",
    "//! integration binary. Nextest gives every `#[test]` its own process;",
    "//! sharing therefore occurs inside each chunk, never across tests.",
    "",
  ];
  for (const slug of chunkSlugs) {
    lines.push(`#[path = "${testSubdir}/${slug}.rs"]`);
    lines.push(`mod ${slug};`);
  }
  lines.push(`#[path = "${testSubdir}/chunk_harness.rs"]`);
  lines.push("mod chunk_harness;");
  for (const slug of overrideSlugs) {
    lines.push(`#[path = "${testSubdir}/${OVERRIDES_SUBDIR}/${slug}.rs"]`);
    lines.push(`mod override_${slug};`);
  }
  lines.push("");
  return lines.join("\n");
}

function renderCorpusReadme(count) {
  return `# Corpus audit tests

Auto-generated by \`scripts/gen-corpus-audit-tests.mjs\`.

At the time of generation there were **${count}** vendored
\`.vue\` fixtures under
\`crates/verter_session/tests/cases/component_meta_audit_corpus/fixtures/\`;
they are emitted as deterministic chunks of at most **${CORPUS_CHUNK_SIZE}**
logical rows. The canonical nextest gate launches one process per chunk; the
chunk harness shares execution pools while constructing a fresh workspace,
host, scheduler/driver, caches, audit store, and request state for every row.
Fixture provenance and license are documented in \`fixtures/README.md\`.

## Regenerating

\`\`\`bash
node scripts/gen-corpus-audit-tests.mjs
\`\`\`

## Overrides

Place \`overrides/<slug>.rs\` to pin component-specific assertions the table
runner cannot express. An override removes that fixture from generated chunks
and remains a standalone authored test module. At landing the override
directory is empty — the chunk harness covers the corpus-wide "audit record
produced + footprint attached" invariants.
`;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/** Sweep ONE corpus config: discover its fixtures, write per-component
 *  test files into its subdir, and write its entry point. Returns the
 *  number of fixtures discovered. */
function sweepCorpus(corpus, cliConfig) {
  const componentsRoot = resolve(DEFAULT_OUTPUT_DIR, corpus.testSubdir, corpus.fixturesSubdir);
  const config = {
    componentsRoot,
    fileExtension: corpus.fileExtension,
    requestShape: corpus.requestShape,
    testSubdir: corpus.testSubdir,
  };

  const componentFiles = discoverComponentFiles(componentsRoot, corpus.fileExtension);
  if (componentFiles.length === 0) {
    console.error(
      `No ${corpus.fileExtension} files found under ${componentsRoot}. ` +
        `Is the ${corpus.frameworkId} fixture set vendored?`,
    );
    process.exit(1);
  }

  const testDir = resolve(cliConfig.outputDir, corpus.testSubdir);
  const overridesDir = resolve(testDir, OVERRIDES_SUBDIR);
  // Overrides are authored INPUT, not generated output. Dry-run parity writes
  // into a temporary output tree, so discovering overrides there would silently
  // forget the committed override inventory and generate a different module
  // graph. Always read the authoritative checkout directory.
  const authoritativeOverridesDir = resolve(
    DEFAULT_OUTPUT_DIR,
    corpus.testSubdir,
    OVERRIDES_SUBDIR,
  );

  // Clean regeneration — remove any prior generated files, preserve
  // overrides/, fixtures/, README, the hand-written harness.rs,
  // chunk_harness.rs, and the hand-written mod.rs. harness.rs is the
  // cross-component regression capture harness; mod.rs preserves the
  // historical second Main.vue logical row beside the generated chunk row.
  // Neither is generator output and both must survive regeneration.
  if (!cliConfig.dryRun) {
    try {
      for (const entry of readdirSync(testDir)) {
        if (
          entry === OVERRIDES_SUBDIR ||
          entry === corpus.fixturesSubdir ||
          entry === "README.md" ||
          entry === "harness.rs" ||
          entry === "chunk_harness.rs" ||
          entry === "mod.rs"
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

  const overrideSlugs = new Set(
    (() => {
      try {
        return readdirSync(authoritativeOverridesDir)
          .filter((n) => n.endsWith(".rs"))
          .map((n) => n.replace(/\.rs$/, ""));
      } catch {
        return [];
      }
    })(),
  );

  const generatedComponents = componentFiles.filter(
    (component) => !overrideSlugs.has(slugFor(component, componentsRoot, corpus.fileExtension)),
  );
  const chunkSlugs = [];
  for (let start = 0, chunkIndex = 0; start < generatedComponents.length; chunkIndex += 1) {
    const chunk = generatedComponents.slice(start, start + CORPUS_CHUNK_SIZE);
    const rendered = renderChunkBody(chunkIndex, chunk, config);
    chunkSlugs.push(rendered.chunkSlug);
    writeFileSync(resolve(testDir, `${rendered.chunkSlug}.rs`), rendered.body);
    start += chunk.length;
  }

  // Cargo integration test target at `tests/<entryStem>.rs`. The stem
  // differs from the subdir's stem because cargo auto-discovers
  // `tests/<name>.rs` AND `tests/<name>/` as candidates for the same
  // target name, raising a duplicate-name error when both exist.
  writeFileSync(
    resolve(cliConfig.outputDir, `${corpus.entryStem}.rs`),
    renderEntryPointRs(chunkSlugs, Array.from(overrideSlugs).sort(), corpus.testSubdir),
  );
  writeFileSync(resolve(testDir, "README.md"), renderCorpusReadme(componentFiles.length));

  // overrides/ gitkeep so the directory exists in fresh checkouts.
  writeFileSync(resolve(overridesDir, ".gitkeep"), "");

  if (!cliConfig.dryRun) {
    console.log(
      `Generated ${componentFiles.length} ${corpus.frameworkId} logical rows in ` +
        `${chunkSlugs.length} chunks into ` +
        `${relative(REPO_ROOT, testDir)}/`,
    );
  }

  return componentFiles.length;
}

function main() {
  const cliConfig = parseArgs(process.argv.slice(2));
  // ONE idempotent script sweeps every configured corpus. A later
  // framework vertical adds a row to CORPUS_CONFIGS; no generator fork.
  for (const corpus of CORPUS_CONFIGS) {
    sweepCorpus(corpus, cliConfig);
  }
}

main();
