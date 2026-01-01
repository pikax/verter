/**
 * @ai-generated - This test file was generated with AI assistance.
 * Covers barrel exports stability:
 * - `VueDocument` must be present at runtime even with circular imports.
 */

import { describe, expect, it } from "vitest";

import * as documents from "./index";
import { VueDocument } from "./index";

describe("documents barrel exports", () => {
  it("exports VueDocument at runtime", () => {
    expect("VueDocument" in documents).toBe(true);
    expect(VueDocument).toBeTruthy();
    expect(typeof VueDocument).toBe("function");
    expect(VueDocument.name).toBe("VueDocument");
  });
});
