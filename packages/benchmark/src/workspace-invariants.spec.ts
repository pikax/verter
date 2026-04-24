/**
 * Workspace-level invariants enforced by plan §3 Commit 8.
 *
 * Three plan-required tests that never landed in the original Commit
 * 8 squash — added here by the 2026-04-24 review-fix pass (see
 * `.claude/feedback/feedback-2026-04-24-audit-footprint-review.md`
 * finding F3). Colocated with the benchmark specs because they
 * already have a vitest runner and exercise repo-wide invariants;
 * the benchmark package is the de-facto home for CI-harness
 * cross-package tests.
 *
 * ## Tests
 *
 * - `root_pnpm_test_runs_every_workspace_package_test_script` — the
 *   root `pnpm test` script fans out to every workspace package.
 *   Enforced by reading `package.json` and asserting the script uses
 *   `pnpm -r` with no excludes that would omit known packages.
 * - `every_workspace_package_has_test_script_or_is_documented_as_no_tests`
 *   — every `packages/*` directory's `package.json` has a `test`
 *   script OR a documented no-op (echo-based) per the plan's
 *   "escape hatch" clause.
 * - `audit_generated_ts_file_compiles_via_packages_types_tsc_noemit`
 *   — shells out `tsc --noEmit` for `@verter/types` so a tsc-level
 *   drift in `audit.generated.ts` (e.g. a malformed generic
 *   following a ts-rs upgrade) surfaces here — the package's
 *   default `vitest --run` script uses esbuild/swc and would
 *   tolerate the drift.
 */

import { execSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = resolve(__dirname, "../../..");

function readJson(path: string): Record<string, unknown> {
  return JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown>;
}

function packagesDirs(): string[] {
  const pkgsDir = resolve(REPO_ROOT, "packages");
  return readdirSync(pkgsDir)
    .filter((n) => existsSync(resolve(pkgsDir, n, "package.json")))
    .map((n) => resolve(pkgsDir, n))
    .sort();
}

describe("workspace invariants — plan §3 Commit 8", () => {
  it("root_pnpm_test_runs_every_workspace_package_test_script", () => {
    // The root package.json `test` script must fan out to every
    // workspace package. Plan §0.2 gate: "from Commit 8 onward,
    // `pnpm test` covers every workspace package with a `test`
    // script". Discriminating: a refactor that narrows the root
    // script (e.g. via `--filter !@verter/wasm`) breaks this test.
    const root = readJson(resolve(REPO_ROOT, "package.json"));
    const scripts = (root.scripts ?? {}) as Record<string, string>;
    const testScript = scripts.test ?? "";

    expect(testScript, "root `test` script must use `pnpm -r` to fan out to every package").toMatch(
      /pnpm\s+-r/,
    );
    // Must not carry an exclude-filter that would skip packages
    // silently. The only allowed syntax is `pnpm -r` possibly with
    // `--parallel` / `--stream`.
    expect(
      testScript,
      "root `test` script must not use `--filter !` excludes (they silently skip packages)",
    ).not.toMatch(/--filter\s+!/);
  });

  it("every_workspace_package_has_test_script_or_is_documented_as_no_tests", () => {
    // Every `packages/*/package.json` must declare a `test` script.
    // Plan §3 Commit 8 allows a no-op (echo-based) script as a
    // documented escape hatch for packages with pre-existing test
    // gaps, but the script itself MUST be present so `pnpm -r run
    // test` exits 0 for that package rather than failing on a
    // missing script. Discriminating: a `packages/foo/package.json`
    // with no `test` script at all surfaces here as a clear failure
    // naming the missing package.
    const missing: string[] = [];
    for (const pkg of packagesDirs()) {
      const pj = readJson(resolve(pkg, "package.json"));
      const scripts = (pj.scripts ?? {}) as Record<string, string>;
      if (!scripts.test) {
        missing.push(pkg.replace(`${REPO_ROOT}/`, "").replace(`${REPO_ROOT}\\`, ""));
      }
    }
    expect(
      missing,
      "every packages/* must declare a `test` script (echo no-op is permitted for documented escape hatches)",
    ).toEqual([]);
  });

  it("audit_generated_ts_file_compiles_via_packages_types_tsc_noemit", () => {
    // @verter/types owns `audit.generated.ts` (the ts-rs output from
    // `verter_session`). Its default `vitest --run` uses esbuild and
    // will NOT surface a tsc-level type error (malformed generic,
    // missing import, ambient-dep drift). Shell out to a narrowly
    // scoped tsc --noEmit against the generated file in isolation
    // (strict + isolatedModules — the file is self-contained with
    // no imports, so a targeted invocation is both faster and more
    // discriminating than running against the package's full
    // tsconfig, which would surface unrelated errors).
    //
    // Implementation note: copy `audit.generated.ts` to a tempdir
    // that contains NO tsconfig.json, so tsc's TS6-beta
    // `TS5112: tsconfig.json is present but will not be loaded if
    // files are specified on commandline` doesn't fire. The tempdir
    // shares no ambient config with `packages/types`, giving the
    // strictest possible isolated compilation check.
    //
    // Discriminating: regress `audit.generated.ts` by hand-editing
    // e.g. `Vec<string>` → `Vec<string` (missing close) and this
    // test fails with the exact tsc diagnostic pointing at the bad
    // line. A ts-rs upgrade that emits a TypeScript surface tsc
    // rejects likewise fails here.
    const typesDir = resolve(REPO_ROOT, "packages/types");
    const generated = resolve(typesDir, "audit.generated.ts");
    expect(
      existsSync(generated),
      "packages/types/audit.generated.ts must exist — regenerate via `cargo test -p verter_session --test ts_bindings`",
    ).toBe(true);

    // Copy the generated file into an isolated tempdir so tsc's
    // cwd-based tsconfig.json lookup doesn't trip TS5112 on the
    // `packages/types/tsconfig.json`. Resolve the tsc binary path
    // from `packages/types/node_modules` and invoke it with the
    // tempdir as cwd — no ambient tsconfig, no TS5112.
    const workDir = mkdtempSync(resolve(tmpdir(), "verter-audit-tsc-"));
    const copied = resolve(workDir, "audit.generated.ts");
    copyFileSync(generated, copied);

    // Locate tsc: the binary is installed in typesDir/node_modules/.bin/tsc
    // (or parent `node_modules/.bin/tsc` if hoisted by pnpm).
    const tscCandidates = [
      resolve(typesDir, "node_modules/.bin/tsc.cmd"),
      resolve(typesDir, "node_modules/.bin/tsc"),
      resolve(REPO_ROOT, "node_modules/.bin/tsc.cmd"),
      resolve(REPO_ROOT, "node_modules/.bin/tsc"),
    ];
    const tscBin = tscCandidates.find((p) => existsSync(p));
    expect(
      tscBin,
      `tsc not found in any of: ${tscCandidates.join(", ")}. Run \`pnpm install\`.`,
    ).toBeTruthy();

    let combinedOutput = "";
    let exitCode = 0;
    try {
      combinedOutput = execSync(
        `${JSON.stringify(tscBin)} --noEmit --strict --target es2020 --module esnext --moduleResolution bundler --isolatedModules --skipLibCheck ${JSON.stringify(copied)}`,
        {
          cwd: workDir,
          encoding: "utf8",
          stdio: ["ignore", "pipe", "pipe"],
        },
      );
    } catch (err) {
      const e = err as { status?: number; stdout?: string; stderr?: string };
      exitCode = e.status ?? 1;
      combinedOutput = `${e.stdout ?? ""}${e.stderr ?? ""}`;
    }
    expect(
      exitCode,
      `tsc --noEmit on packages/types/audit.generated.ts must exit 0 (file must compile cleanly). Output:\n${combinedOutput}`,
    ).toBe(0);
  }, 60_000);
});
