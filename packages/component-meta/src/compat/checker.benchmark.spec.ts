import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { normalizeComponentMetaArtifact } from "../../../benchmark/src/component-meta-artifact.ts";
import {
  parseMetaUiBenchArgs,
  prepareMetaUiProject,
} from "../../../benchmark/src/meta-ui-bench.ts";
import { compareNormalizedArtifacts } from "../../../benchmark/src/meta-ui-core.ts";
import { createCheckerByJson } from "./checker.js";

const repoRoot = resolve(import.meta.dirname, "../../../..");
const uiRoot = resolve(repoRoot, ".integration-tests", "repos", "nuxt-ui");
const expectedRoot = resolve(
  repoRoot,
  "packages",
  "benchmark",
  "benchmark-results",
  "meta-ui",
  ".expected-vue-component-meta",
);

function readExpectedArtifact(relativePath: string) {
  return JSON.parse(readFileSync(resolve(expectedRoot, `${relativePath}.json`), "utf8"));
}

function formatParityFailure(
  componentPath: string,
  comparison: ReturnType<typeof compareNormalizedArtifacts>,
) {
  const lines = [`Benchmark parity mismatch for ${componentPath}`];

  for (const [collectionName, collection] of Object.entries(comparison.collections)) {
    if (collection.exact) {
      continue;
    }
    if (collection.missing.length > 0) {
      lines.push(`${collectionName}.missing=${collection.missing.join(",")}`);
    }
    if (collection.extra.length > 0) {
      lines.push(`${collectionName}.extra=${collection.extra.join(",")}`);
    }
    for (const mismatch of collection.fieldMismatches.slice(0, 20)) {
      lines.push(
        `${collectionName}.${mismatch.name}.${mismatch.field}\nexpected=${mismatch.expected}\nactual=${mismatch.actual}`,
      );
    }
  }

  return lines.join("\n");
}

function tryResolveTypesDeclaration(fullPath: string): string {
  if (!fullPath.includes("node_modules") || !fullPath.endsWith(".vue")) {
    return fullPath;
  }

  const patterns = [
    fullPath.replace(".vue", ".d.vue.ts"),
    fullPath.replace(".vue", ".vue.d.ts"),
    fullPath.replace(".vue", ".d.ts"),
  ];

  for (const candidate of patterns) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }

  return fullPath;
}

describe("nuxt-ui benchmark parity", () => {
  let checker: Awaited<ReturnType<typeof createCheckerByJson>>;
  const prepared = prepareMetaUiProject(parseMetaUiBenchArgs(["--expected=none"]));

  beforeAll(async () => {
    checker = await createCheckerByJson(
      uiRoot,
      {
        extends: `${prepared.uiRoot}/tsconfig.json`,
        skipLibCheck: true,
        include: prepared.componentSnapshots.map((component) =>
          tryResolveTypesDeclaration(component.absolutePath),
        ),
        exclude: [],
        compilerOptions: {
          ...(prepared.compilerOptions.baseUrl
            ? { baseUrl: prepared.compilerOptions.baseUrl }
            : {}),
          ...(prepared.compilerOptions.paths ? { paths: prepared.compilerOptions.paths } : {}),
        },
      },
      {
        forceUseTs: true,
        schema: { literalBooleanSchema: true },
        runtimeMode: "dedicated",
      },
    );

    for (const component of prepared.componentSnapshots) {
      checker.updateFile(component.absolutePath, component.transformedSource);
    }
  });

  afterAll(() => {
    checker.close();
  });

  for (const relativePath of [
    "src/runtime/components/Alert.vue",
    "src/runtime/components/App.vue",
    "src/runtime/components/Tooltip.vue",
    "src/runtime/components/Button.vue",
  ]) {
    it(`matches the pinned normalized artifact for ${relativePath}`, async () => {
      const meta = await checker.getComponentMeta(resolve(uiRoot, relativePath));
      const actualArtifact = normalizeComponentMetaArtifact(relativePath, meta);
      const expectedArtifact = readExpectedArtifact(relativePath);
      const comparison = compareNormalizedArtifacts(actualArtifact, expectedArtifact);

      expect(comparison.exact, formatParityFailure(relativePath, comparison)).toBe(true);
    });
  }
});
