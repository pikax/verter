/**
 * @ai-generated - Locks the supported entry exports and session-first metadata access.
 */

import { describe, expect, it } from "vitest";
import { create, toBinary } from "@bufbuild/protobuf";
import { ComponentMetaPayloadSchema, createTestComponentMetaPayload } from "@verter/proto";
import {
  decodeComponentMetaPayload as decodeBrowserPayload,
  type NativeComponentMetaResult as BrowserNativeComponentMetaResult,
} from "./browser.js";
import {
  decodeComponentMetaPayload as decodeRootPayload,
  type NativeComponentMetaResult,
} from "./index.js";
import * as componentMeta from "./index.js";

describe("@verter/component-meta entry exports", () => {
  it.each([
    ["browser", decodeBrowserPayload],
    ["root", decodeRootPayload],
  ] as const)("decodes schema-15 component metadata through the %s entry", (_entry, decode) => {
    const payload = create(ComponentMetaPayloadSchema, createTestComponentMetaPayload());
    const result: NativeComponentMetaResult & BrowserNativeComponentMetaResult = decode(
      toBinary(ComponentMetaPayloadSchema, payload),
    );

    expect(result).toMatchObject({
      filePath: "/src/Tree.vue",
      props: [{ name: "root", required: true }],
      slots: [{ name: "default", bindings: [{ name: "root" }] }],
      facts: { props: { kind: "complete" } },
      componentPublicContract: { kind: "supported" },
    });
  });

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
