/**
 * Release-wiring guards for the `verter-lsp` npm channel.
 *
 * Publishing a binary package family has four moving parts that must agree:
 * the build matrix that produces one binary per platform, the artifact names
 * those builds upload under, the publish job that stages each artifact into its
 * platform package, and the publish set that decides which packages actually
 * ship. A mismatch in any one of them ships a launcher whose platform package
 * is empty — an install that resolves and then cannot start the server.
 *
 * Every expectation is derived from `platforms.js` (for the build matrix) or
 * from `scripts/lib/publish-set.mjs` executed for real (for the publish set),
 * never from the workflow text under test.
 */

import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve as resolvePath, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { computePublishSet } from "../../scripts/lib/publish-set.mjs";
import { PLATFORM_MATRIX } from "./platforms.js";
import { readJobBody, readMatrixRows } from "./test-helpers/release-workflow.ts";

const PACKAGE_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolvePath(PACKAGE_DIR, "..", "..");

const buildLspRows = readMatrixRows("build-lsp");
const buildLspBody = readJobBody("build-lsp").join("\n");
const buildVsixBody = readJobBody("build-vsix").join("\n");
const publishNpmBody = readJobBody("publish-npm").join("\n");
const publishVscodeBody = readJobBody("publish-vscode").join("\n");
const githubReleaseBody = readJobBody("github-release").join("\n");

/** A job's `needs:` list, as authored job names. */
function needsOf(jobBody: string): string[] {
  const match = /needs:\s*\[([^\]]*)\]/s.exec(jobBody);
  if (!match) return [];
  return match[1]
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean);
}

/** Normalise a repo-relative path to forward slashes for comparison. */
function posix(path: string): string {
  return path.split(sep).join("/");
}

describe("release.yml build-lsp matrix", () => {
  it("parses as a non-vacuous matrix (the extractor still finds the block)", () => {
    expect(buildLspRows.length).toBeGreaterThan(0);
    // Every row must carry more than the leading `- target:` key, otherwise
    // the row parser silently dropped the continuation lines.
    for (const row of buildLspRows) {
      expect(Object.keys(row).length).toBeGreaterThan(1);
    }
    // Negative control for the extractor itself: an absent job yields nothing
    // rather than a coincidental match elsewhere in the file.
    expect(readMatrixRows("no-such-job")).toEqual([]);
  });

  it("builds exactly the rust targets the platform matrix declares", () => {
    const expected = PLATFORM_MATRIX.map((row) => row.rustTarget).sort();
    const actual = buildLspRows.map((row) => row.target).sort();
    expect(actual).toEqual(expected);
  });

  it.each(PLATFORM_MATRIX.map((row) => [row.rustTarget, row] as const))(
    "%s builds into its matching npm platform package",
    (rustTarget, row) => {
      const matrixRow = buildLspRows.find((r) => r.target === rustTarget);
      expect(matrixRow, `no build-lsp row for ${rustTarget}`).toBeDefined();
      expect(matrixRow!["npm-pkg"]).toBe(row.npmSuffix);
      expect(matrixRow!.binary).toBe(row.binaryName);
    },
  );

  it("uploads each build under its npm platform-package name", () => {
    expect(buildLspBody).toContain("name: lsp-${{ matrix.npm-pkg }}");
  });
});

describe("release.yml publish-npm job", () => {
  it("waits for the LSP binaries before publishing", () => {
    expect(needsOf(publishNpmBody), "publish-npm has no needs: [...] list").toContain("build-lsp");
  });

  it("downloads the LSP artifacts and stages them into the platform packages", () => {
    expect(publishNpmBody).toContain("pattern: lsp-*");
    expect(publishNpmBody).toContain("packages/verter-lsp/npm/");
  });

  it("publishes platform packages from the derived publish set, not a hand-listed family", () => {
    // The derived list is the single authority; a per-family hardcoded loop is
    // exactly the drift that leaves a new platform family unpublished.
    expect(publishNpmBody).toContain("scripts/publish-platform-dirs.mjs");
    expect(publishNpmBody).not.toContain("for platform_dir in packages/native/npm/*/");
    expect(publishNpmBody).not.toContain("for platform_dir in packages/verter-tsc/npm/*/");
  });

  it("verifies the launcher reached the registry", () => {
    const critical = /CRITICAL_PACKAGES=\(([^)]*)\)/s.exec(publishNpmBody);
    expect(critical, "publish-npm has no CRITICAL_PACKAGES list").not.toBeNull();
    expect(critical![1]).toContain('"verter-lsp"');
  });
});

describe("release.yml build-vsix job", () => {
  it("resolves the bundled binary from the platform-named LSP artifacts", () => {
    // The VSIX channel consumes the same build-lsp artifacts. It maps its own
    // vsce target names onto the npm platform-package names, so the rename
    // cannot silently break extension packaging.
    for (const vsceTarget of [
      "linux-x64",
      "linux-arm64",
      "darwin-x64",
      "darwin-arm64",
      "win32-x64",
    ]) {
      expect(buildVsixBody).toContain(`[${vsceTarget}]=`);
    }
    expect(buildVsixBody).toContain("LSP_PKG");
    expect(buildVsixBody).toContain("/tmp/lsp-artifacts/lsp-${lsp_pkg}");
  });

  it("packages without publishing, and produces the artifact the release consumes", () => {
    // Packaging is a BUILD: the VSIX must exist before either publish job runs,
    // so the GitHub Release can be assembled from build output alone.
    expect(buildVsixBody).toContain("node package.mjs");
    expect(buildVsixBody).toContain("name: vsix");
    expect(buildVsixBody).not.toContain("vsce publish");
    expect(buildVsixBody).not.toContain("VSCE_PAT");
    expect(needsOf(buildVsixBody)).not.toContain("publish-npm");
  });
});

describe("release.yml publish-vscode job", () => {
  it("publishes the prebuilt VSIX rather than packaging its own", () => {
    expect(needsOf(publishVscodeBody)).toContain("build-vsix");
    expect(publishVscodeBody).toContain("name: vsix");
    expect(publishVscodeBody).toContain("vsce publish");
    expect(publishVscodeBody).not.toContain("node package.mjs");
  });

  it("still publishes after npm, so the extension never lands before its packages", () => {
    expect(needsOf(publishVscodeBody)).toContain("publish-npm");
  });
});

describe("release.yml github-release job", () => {
  it("is gated on the builds, not on publishing", () => {
    const needs = needsOf(githubReleaseBody);
    expect(needs, "github-release has no needs: [...] list").not.toEqual([]);

    for (const buildJob of ["build-native", "build-wasm", "build-mcp-server", "build-vsix"]) {
      expect(needs).toContain(buildJob);
    }

    // A failed npm or Marketplace publish must not withhold the release and its
    // binary assets — every asset comes from a build job.
    expect(needs).not.toContain("publish-npm");
    expect(needs).not.toContain("publish-vscode");
  });

  it("ships the server binaries as release assets, under platform-qualified names", () => {
    expect(needsOf(githubReleaseBody)).toContain("build-lsp");
    expect(githubReleaseBody).toContain("pattern: lsp-*");
    // A bare `verter-lsp` asset name would collide across the seven platforms.
    expect(githubReleaseBody).toContain('"verter-lsp-${platform}"');
  });

  it("stages assets under explicit names with a per-family count", () => {
    // Every family's size is asserted, so a missing build leg is a failed
    // release rather than a quietly short asset list.
    for (const expectation of [
      "EXPECTED_NATIVE=7",
      "EXPECTED_LSP=7",
      "EXPECTED_MCP=5",
      "EXPECTED_VSIX=5",
    ]) {
      expect(githubReleaseBody).toContain(expectation);
    }
  });

  it("neither sweeps unnamed files nor silently caps the asset list", () => {
    // A blanket `-name '*.js'` swept the NAPI loader into the release as an
    // opaque `index.js`; `head -50` dropped anything past the 50th with no
    // error at all.
    expect(githubReleaseBody).not.toContain("-name '*.js'");
    expect(githubReleaseBody).not.toContain("head -50");
    // The loader artifact is still downloaded (`native-*`) but must not ship.
    expect(githubReleaseBody).toContain("-name '*.node'");
  });

  it("reports the attached assets in the workflow run summary", () => {
    expect(githubReleaseBody).toContain("GITHUB_STEP_SUMMARY");
  });

  it("is still gated on the test job, transitively", () => {
    // Dropping the publish jobs from `needs` removed the path that used to
    // carry the test gate. The release must not become the one job that ships
    // past a red suite, so prove `test` is still upstream of it.
    const seen = new Set<string>();
    const queue = needsOf(githubReleaseBody);
    while (queue.length > 0) {
      const job = queue.shift()!;
      if (seen.has(job)) continue;
      seen.add(job);
      queue.push(...needsOf(readJobBody(job).join("\n")));
    }
    expect([...seen]).toContain("test");
  });
});

describe("derived publish set", () => {
  const publishSet = computePublishSet();

  it("includes the launcher as a published npm package", () => {
    expect(publishSet.npm).toContain("verter-lsp");
    expect(publishSet.order).toContain("verter-lsp");
  });

  it("includes every platform package directory", () => {
    const dirs = publishSet.platform.map(posix);
    for (const row of PLATFORM_MATRIX) {
      expect(dirs).toContain(`packages/verter-lsp/npm/${row.npmSuffix}`);
    }
  });

  it("the script CI publishes from emits exactly the derived platform dirs", () => {
    // Executed for real, so the list CI loops over is proven — not inferred
    // from the script's source text.
    const stdout = execFileSync(process.execPath, [join("scripts", "publish-platform-dirs.mjs")], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
    const emitted = stdout.trim().split(/\r?\n/).filter(Boolean).map(posix);

    expect(emitted.sort()).toEqual(publishSet.platform.map(posix).sort());
    for (const row of PLATFORM_MATRIX) {
      expect(emitted).toContain(`packages/verter-lsp/npm/${row.npmSuffix}`);
    }
  });
});

describe("these guards run in CI", () => {
  it("ci.yml executes this package's specs by explicit path", () => {
    // The workspace test job runs selected specs by path, so a guard that is
    // never referenced there is a guard that never runs.
    const ciYaml = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
    expect(ciYaml).toContain("packages/verter-lsp");
  });
});
