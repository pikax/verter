/**
 * @ai-generated - Guards the editor-neutral real-provider process contract.
 */
import { describe, expect, it } from "vitest";

import { editorNeutralServerEnvironment } from "../src/editor-neutral/rawLspDriver.js";

describe("raw editor-neutral LSP driver", () => {
  it("starts Verter in provider-only E2E mode", () => {
    expect(editorNeutralServerEnvironment("C:/tools/tsgo.exe")).toMatchObject({
      VERTER_TSGO_BIN: "C:/tools/tsgo.exe",
      VERTER_LOG: "info",
      VERTER_E2E_TEST: "1",
      VERTER_E2E_PROVIDER_ONLY_COMPLETIONS: "1",
    });
  });
});
