/**
 * @ai-generated - Guards project bootstrap against reintroducing the removed JS HTML intrinsics shim.
 */

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

describe("openComponentMetaSession bootstrap source", () => {
  it("does not import or invoke the removed project-html-intrinsics shim", () => {
    const source = readFileSync(resolve(here, "project.ts"), "utf8");
    const engineSource = readFileSync(resolve(here, "runtime", "project-engine.ts"), "utf8");

    expect(source).not.toContain("project-html-intrinsics");
    expect(source).not.toContain("configureProjectHtmlIntrinsics");
    expect(engineSource).not.toContain("setHtmlIntrinsicsCatalog");
  });
});
