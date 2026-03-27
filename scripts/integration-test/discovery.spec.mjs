import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import {
  analyzeRepo,
  buildDiscoveryInventory,
  renderDiscoveryMarkdown,
  VERTER_EXTENSION_ID,
} from "./discovery.mjs";

const tempDirs = [];

function createRepo(structure) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "verter-discovery-"));
  tempDirs.push(repoRoot);
  fs.mkdirSync(path.join(repoRoot, ".git"));

  for (const [relativePath, value] of Object.entries(structure)) {
    const fullPath = path.join(repoRoot, relativePath);
    fs.mkdirSync(path.dirname(fullPath), { recursive: true });
    fs.writeFileSync(fullPath, value);
  }

  return repoRoot;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("analyzeRepo", () => {
  it("classifies a Vite repo with vue-tsc and Volar settings as full_stack", () => {
    const repoRoot = createRepo({
      "package.json": JSON.stringify(
        {
          name: "full-stack-app",
          packageManager: "pnpm@10.0.0",
          scripts: {
            build: "vite build",
            test: "vitest run",
            typecheck: "vue-tsc --noEmit",
          },
          devDependencies: {
            vue: "^3.5.0",
            "vue-tsc": "^3.1.0",
            "@vitejs/plugin-vue": "^5.0.0",
          },
        },
        null,
        2,
      ),
      "pnpm-lock.yaml": "lockfileVersion: 9.0\n",
      "tsconfig.json": JSON.stringify(
        {
          compilerOptions: {
            plugins: [{ name: "@vue/typescript-plugin" }],
          },
        },
        null,
        2,
      ),
      "vite.config.ts":
        "import vue from '@vitejs/plugin-vue'\nexport default { plugins: [vue()] }\n",
      ".vscode/extensions.json": JSON.stringify(
        {
          recommendations: ["Vue.volar"],
        },
        null,
        2,
      ),
      "src/App.vue": "<template><div /></template>\n",
    });

    const repo = analyzeRepo(repoRoot, { discoveryRoot: path.dirname(repoRoot) });
    expect(repo.replacementRecipe).toBe("full_stack");
    expect(repo.executionTier).toBe("tier2");
    expect(repo.chosenTsconfig).toBe("tsconfig.json");
    expect(repo.replacementSteps).toContain("editor");
    expect(repo.replacementSteps).toContain("typescript-plugin");
    expect(repo.replacementSteps).toContain("verter-tsc");
    expect(repo.replacementSteps).toContain("build-plugin");
  });

  it("classifies a repo with only Volar recommendations as editor_only", () => {
    const repoRoot = createRepo({
      ".vscode/extensions.json": JSON.stringify(
        {
          recommendations: ["Vue.volar", VERTER_EXTENSION_ID],
        },
        null,
        2,
      ),
    });

    const repo = analyzeRepo(repoRoot, { discoveryRoot: path.dirname(repoRoot) });
    expect(repo.replacementRecipe).toBe("editor_only");
    expect(repo.replacementSteps).toEqual(["editor"]);
  });

  it("falls back to manual_review when typecheck surface has no deterministic root tsconfig", () => {
    const repoRoot = createRepo({
      "package.json": JSON.stringify(
        {
          name: "nested-tsconfig",
          scripts: {
            test: "vitest run",
          },
          devDependencies: {
            vue: "^3.5.0",
            "vue-tsc": "^3.1.0",
          },
        },
        null,
        2,
      ),
      "pnpm-lock.yaml": "lockfileVersion: 9.0\n",
      "packages/app/tsconfig.json": JSON.stringify(
        {
          compilerOptions: {},
        },
        null,
        2,
      ),
      "packages/app/src/App.vue": "<template><div /></template>\n",
    });

    const repo = analyzeRepo(repoRoot, { discoveryRoot: path.dirname(repoRoot) });
    expect(repo.replacementRecipe).toBe("manual_review");
    expect(repo.reasons).toContain("missing root tsconfig");
  });

  it("marks ambiguous monorepos as manual_review instead of guessing a bundler", () => {
    const repoRoot = createRepo({
      "package.json": JSON.stringify(
        {
          name: "mixed-monorepo",
          packageManager: "pnpm@10.0.0",
          scripts: {
            build: "pnpm -r build",
          },
        },
        null,
        2,
      ),
      "pnpm-lock.yaml": "lockfileVersion: 9.0\n",
      "apps/web/vite.config.ts":
        "import vue from '@vitejs/plugin-vue'\nexport default { plugins: [vue()] }\n",
      "apps/docs/nuxt.config.ts": "export default defineNuxtConfig({})\n",
    });

    const repo = analyzeRepo(repoRoot, { discoveryRoot: path.dirname(repoRoot) });
    expect(repo.replacementRecipe).toBe("manual_review");
    expect(repo.surfaces.buildSurfaceKinds).toEqual(expect.arrayContaining(["vite", "nuxt"]));
    expect(repo.reasons.some((reason) => reason.startsWith("ambiguous build surface:"))).toBe(true);
  });

  it("marks the verter workspace as manual_review instead of a consumer project", () => {
    const repoRoot = createRepo({
      "package.json": JSON.stringify(
        {
          name: "verter",
          packageManager: "pnpm@10.0.0",
          scripts: {
            build: "pnpm run build",
            test: "pnpm test",
          },
        },
        null,
        2,
      ),
      "pnpm-lock.yaml": "lockfileVersion: 9.0\n",
      "tsconfig.json": JSON.stringify({ compilerOptions: {} }, null, 2),
      ".vscode/extensions.json": JSON.stringify(
        {
          recommendations: ["Vue.volar"],
        },
        null,
        2,
      ),
      "packages/app/vite.config.ts":
        "import vue from '@vitejs/plugin-vue'\nexport default { plugins: [vue()] }\n",
      "packages/app/src/App.vue": "<template><div /></template>\n",
    });

    const repo = analyzeRepo(repoRoot, { discoveryRoot: path.dirname(repoRoot) });
    expect(repo.replacementRecipe).toBe("manual_review");
    expect(repo.surfaces.isToolchainRepo).toBe(true);
    expect(repo.reasons).toContain("repo is verter toolchain");
  });
});

describe("buildDiscoveryInventory", () => {
  it("emits markdown grouped by recipe", () => {
    const repoRoot = createRepo({
      "package.json": JSON.stringify(
        {
          name: "inventory-app",
          scripts: {
            build: "vite build",
          },
          devDependencies: {
            vue: "^3.5.0",
            "@vitejs/plugin-vue": "^5.0.0",
          },
        },
        null,
        2,
      ),
      "package-lock.json": "{}",
      "vite.config.ts": "import vue from '@vitejs/plugin-vue'\n",
    });

    const inventory = buildDiscoveryInventory({
      roots: [repoRoot],
      matrixProjects: [
        {
          name: "coreui",
          repo: "coreui/coreui",
          packageManager: "npm",
          bundler: "vite",
          buildCmd: "npm run build",
          testCmd: "",
        },
      ],
    });

    expect(inventory.localProjects).toHaveLength(1);
    expect(inventory.tier1Projects).toHaveLength(1);

    const markdown = renderDiscoveryMarkdown(inventory);
    expect(markdown).toContain("## build_only");
    expect(markdown).toContain(inventory.localProjects[0].relativeRoot);
    expect(markdown).toContain("coreui");
  });
});
