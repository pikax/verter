/**
 * @ai-generated - Tests for built-in playground presets.
 */
import { describe, it, expect } from "vitest";
import { presets } from "./presets";

describe("presets", () => {
  it("has at least 6 presets", () => {
    expect(presets.length).toBeGreaterThanOrEqual(6);
  });

  it("every preset has required fields", () => {
    for (const preset of presets) {
      expect(preset.name).toBeTruthy();
      expect(preset.description).toBeTruthy();
      expect(preset.state).toBeTruthy();
      expect(preset.state.files).toBeTruthy();
      expect(preset.state.activeFile).toBeTruthy();
      expect(preset.state.outputMode).toBeTruthy();
      expect(preset.state.compilerOptions).toBeTruthy();
    }
  });

  it("every preset has an App.vue file", () => {
    for (const preset of presets) {
      expect(preset.state.files["App.vue"]).toBeTruthy();
    }
  });

  it("every preset has unique names", () => {
    const names = presets.map((p) => p.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it("no preset has empty file content", () => {
    for (const preset of presets) {
      for (const [filename, code] of Object.entries(preset.state.files)) {
        expect(code.length, `${preset.name}/${filename} is empty`).toBeGreaterThan(0);
      }
    }
  });

  it("activeFile exists in files for every preset", () => {
    for (const preset of presets) {
      expect(
        preset.state.files[preset.state.activeFile],
        `${preset.name}: activeFile '${preset.state.activeFile}' not in files`,
      ).toBeTruthy();
    }
  });
});
