/**
 * Release-wiring guards for every Verter binary family's npm channel.
 *
 * Publishing a binary family has four moving parts that must agree: the build
 * matrix that produces one binary per platform, the artifact names those builds
 * upload under, the publish job that stages each artifact into its platform
 * package, and the publish set that decides which packages actually ship. A
 * mismatch in any one of them ships a launcher whose platform package is empty
 * — an install that resolves and then cannot start the binary.
 *
 * Every expectation is derived from a family's own platform matrix, or from
 * `scripts/lib/publish-set.mjs` executed for real — never from the workflow
 * text under test.
 */

import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve as resolvePath, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { computePublishSet } from "../../scripts/lib/publish-set.mjs";
import type { PlatformEntry } from "./index.js";
import { readJobBody, readMatrixRows } from "./test-support/release-workflow.ts";

const PACKAGE_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolvePath(PACKAGE_DIR, "..", "..");

const lspPlatforms = require("../verter-lsp/platforms.js") as {
  PLATFORM_MATRIX: readonly PlatformEntry[];
};
const mcpPlatforms = require("../verter-mcp/platforms.js") as {
  PLATFORM_MATRIX: readonly PlatformEntry[];
};

interface Family {
  /** Launcher package name. */
  readonly name: string;
  /** `packages/<dir>` holding the launcher. */
  readonly dir: string;
  /** The release job that builds this family's binaries. */
  readonly job: string;
  /** Artifact-name prefix the build job uploads under. */
  readonly artifactPrefix: string;
  readonly matrix: readonly PlatformEntry[];
}

const FAMILIES: readonly Family[] = [
  {
    name: "verter-lsp",
    dir: "verter-lsp",
    job: "build-lsp",
    artifactPrefix: "lsp-",
    matrix: lspPlatforms.PLATFORM_MATRIX,
  },
  {
    name: "verter-mcp",
    dir: "verter-mcp",
    job: "build-mcp",
    artifactPrefix: "mcp-",
    matrix: mcpPlatforms.PLATFORM_MATRIX,
  },
];

const publishNpmBody = readJobBody("publish-npm").join("\n");
const buildVsixBody = readJobBody("build-vsix").join("\n");
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

describe.each(FAMILIES.map((family) => [family.name, family] as const))(
  "release.yml wiring — %s",
  (_name, family) => {
    const rows = readMatrixRows(family.job);
    const jobBody = readJobBody(family.job).join("\n");

    it("parses as a non-vacuous matrix (the extractor still finds the block)", () => {
      expect(rows.length).toBeGreaterThan(0);
      for (const row of rows) {
        expect(Object.keys(row).length).toBeGreaterThan(1);
      }
      // Negative control for the extractor itself: an absent job yields nothing
      // rather than a coincidental match elsewhere in the file.
      expect(readMatrixRows("no-such-job")).toEqual([]);
    });

    it("builds exactly the rust targets the platform matrix declares", () => {
      expect(rows.map((row) => row.target).sort()).toEqual(
        family.matrix.map((row) => row.rustTarget).sort(),
      );
    });

    it("builds each target into its matching npm platform package", () => {
      for (const row of family.matrix) {
        const matrixRow = rows.find((r) => r.target === row.rustTarget);
        expect(matrixRow, `no ${family.job} row for ${row.rustTarget}`).toBeDefined();
        expect(matrixRow!["npm-pkg"]).toBe(row.npmSuffix);
        expect(matrixRow!.binary).toBe(row.binaryName);
      }
    });

    it("uploads each build under its npm platform-package name", () => {
      expect(jobBody).toContain(`name: ${family.artifactPrefix}\${{ matrix.npm-pkg }}`);
    });

    it("is staged into its platform packages before publishing", () => {
      expect(needsOf(publishNpmBody)).toContain(family.job);
      expect(publishNpmBody).toContain(`pattern: ${family.artifactPrefix}*`);
      expect(publishNpmBody).toContain(`packages/${family.dir}/npm/`);
    });

    it("is verified on the registry after publishing", () => {
      const critical = /CRITICAL_PACKAGES=\(([^)]*)\)/s.exec(publishNpmBody);
      expect(critical, "publish-npm has no CRITICAL_PACKAGES list").not.toBeNull();
      expect(critical![1]).toContain(`"${family.name}"`);
    });
  },
);

describe("release.yml publish-npm job", () => {
  it("publishes platform packages from the derived publish set, not a hand-listed family", () => {
    // The derived list is the single authority; a per-family hardcoded loop is
    // exactly the drift that leaves a new platform family unpublished.
    expect(publishNpmBody).toContain("scripts/publish-platform-dirs.mjs");
    for (const family of ["native", "verter-lsp", "verter-mcp", "verter-tsc"]) {
      expect(publishNpmBody).not.toContain(`for platform_dir in packages/${family}/npm/*/`);
    }
  });
});

describe("release.yml build-vsix job", () => {
  it("resolves the bundled binary from the platform-named LSP artifacts", () => {
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

    for (const buildJob of ["build-native", "build-lsp", "build-mcp", "build-wasm", "build-vsix"]) {
      expect(needs).toContain(buildJob);
    }

    // A failed npm or Marketplace publish must not withhold the release and its
    // binary assets — every asset comes from a build job.
    expect(needs).not.toContain("publish-npm");
    expect(needs).not.toContain("publish-vscode");
  });

  it.each(FAMILIES.map((family) => [family.name, family] as const))(
    "ships %s binaries as release assets, under platform-qualified names",
    (name, family) => {
      expect(githubReleaseBody).toContain(`pattern: ${family.artifactPrefix}*`);
      // A bare binary name would collide across the seven platforms.
      expect(githubReleaseBody).toContain(`"${name}-\${platform}"`);
    },
  );

  it("stages assets under explicit names with a per-family count", () => {
    // Every family's size is asserted, so a missing build leg is a failed
    // release rather than a quietly short asset list.
    for (const expectation of [
      "EXPECTED_NATIVE=7",
      "EXPECTED_LSP=7",
      "EXPECTED_MCP=7",
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

  it("publishes every launcher and the resolution they share", () => {
    for (const name of [...FAMILIES.map((f) => f.name), "@verter/binary-launcher"]) {
      expect(publishSet.npm).toContain(name);
      expect(publishSet.order).toContain(name);
    }
  });

  it("orders the shared launcher before the packages that depend on it", () => {
    const shared = publishSet.order.indexOf("@verter/binary-launcher");
    for (const family of FAMILIES) {
      expect(publishSet.order.indexOf(family.name)).toBeGreaterThan(shared);
    }
  });

  it("includes every platform package directory", () => {
    const dirs = publishSet.platform.map(posix);
    for (const family of FAMILIES) {
      for (const row of family.matrix) {
        expect(dirs).toContain(`packages/${family.dir}/npm/${row.npmSuffix}`);
      }
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
    for (const family of FAMILIES) {
      for (const row of family.matrix) {
        expect(emitted).toContain(`packages/${family.dir}/npm/${row.npmSuffix}`);
      }
    }
  });
});

describe("these guards run in CI", () => {
  const ciYaml = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");

  it.each([...FAMILIES.map((f) => `packages/${f.dir}`), "packages/binary-launcher"])(
    "ci.yml executes %s specs by explicit path",
    (path) => {
      // The workspace test job runs selected specs by path, so a guard that is
      // never referenced there is a guard that never runs.
      expect(ciYaml).toContain(path);
    },
  );
});
