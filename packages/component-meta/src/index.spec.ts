/**
 * @ai-generated - Locks the supported root API surface to project-based metadata access.
 */

import { describe, expect, it } from "vitest";
import * as componentMeta from "./index.js";

describe("@verter/component-meta root exports", () => {
  it("prefers the session-first root API", () => {
    expect(componentMeta.openComponentMetaSession).toBeTypeOf("function");
    expect(componentMeta.evictComponentMetaSession).toBeTypeOf("function");
    expect(componentMeta.shutdownMetaRuntime).toBeTypeOf("function");
    expect(componentMeta.ComponentMetaSession).toBeTypeOf("function");

    expect("openMetaProject" in componentMeta).toBe(false);
    expect("MetaProject" in componentMeta).toBe(false);
    expect("evictMetaProject" in componentMeta).toBe(false);
  });

  it("does not expose the removed adapter-based metadata workflow", () => {
    expect(componentMeta.openComponentMetaSession).toBeTypeOf("function");

    expect("createAdapter" in componentMeta).toBe(false);
    expect("createNapiAdapter" in componentMeta).toBe(false);
    expect("createWasmAdapter" in componentMeta).toBe(false);
    expect("wrapNapiHost" in componentMeta).toBe(false);
    expect("wrapWasmHost" in componentMeta).toBe(false);
  });
});
