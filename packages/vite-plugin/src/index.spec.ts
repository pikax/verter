/**
 * @ai-generated - Sanity tests for the @verter/vite-plugin re-export from @verter/unplugin.
 */
import { describe, it, expect } from "vitest";
import defaultExport, { verter } from "./index";

describe("@verter/vite-plugin re-export", () => {
  it("default export is a function", () => {
    expect(typeof defaultExport).toBe("function");
  });

  it("named verter export is a function", () => {
    expect(typeof verter).toBe("function");
  });

  it("default and named exports are the same", () => {
    expect(defaultExport).toBe(verter);
  });

  it("calling the function returns a Vite plugin object", () => {
    const plugin = verter();
    expect(plugin).toBeDefined();

    // Unplugin wraps into an array for Vite
    const plugins = Array.isArray(plugin) ? plugin : [plugin];
    const mainPlugin = plugins[0];

    expect(mainPlugin).toHaveProperty("name");
    expect(typeof mainPlugin.name).toBe("string");
  });
});
