/**
 * @ai-generated - Locks the supported root API surface to project-based metadata access.
 */

import { describe, expect, it } from "vitest";
import * as componentMeta from "./index.js";

describe("@verter/component-meta root exports", () => {
  it("does not expose the removed adapter-based metadata workflow", () => {
    expect(componentMeta.openMetaProject).toBeTypeOf("function");
    expect(componentMeta.shutdownMetaRuntime).toBeTypeOf("function");

    expect("createAdapter" in componentMeta).toBe(false);
    expect("createNapiAdapter" in componentMeta).toBe(false);
    expect("createWasmAdapter" in componentMeta).toBe(false);
    expect("wrapNapiHost" in componentMeta).toBe(false);
    expect("wrapWasmHost" in componentMeta).toBe(false);
  });
});
