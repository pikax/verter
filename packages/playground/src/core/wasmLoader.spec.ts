import { describe, expect, it } from "vitest";
import { loadLocalWasm } from "./wasmLoader";

describe("loadLocalWasm", () => {
  it("initializes the bundled wasm runtime under vitest", async () => {
    const wasmModule = await loadLocalWasm();

    expect(wasmModule.VerterHost).toBeTypeOf("function");

    const host = new wasmModule.VerterHost({
      devMode: true,
      compileErrorPolicy: "devServeLastKnownGood",
      maxProfilesPerFile: 8,
    });

    expect(host).toBeTruthy();
  });
});
