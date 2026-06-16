/**
 * @ai-generated - Tests for built-in playground presets.
 */
import { describe, it, expect } from "vitest";
import { presets } from "./presets";
import { FRAMEWORKS, frameworkById } from "./frameworks";

describe("presets", () => {
  it("has at least 6 presets", () => {
    expect(presets.length).toBeGreaterThanOrEqual(6);
  });

  it("every preset has required fields", () => {
    for (const preset of presets) {
      expect(preset.name).toBeTruthy();
      expect(preset.description).toBeTruthy();
      expect(preset.language).toBeTruthy();
      expect(preset.mainFile).toBeTruthy();
      expect(preset.state).toBeTruthy();
      expect(preset.state.files).toBeTruthy();
      expect(preset.state.activeFile).toBeTruthy();
      expect(preset.state.outputMode).toBeTruthy();
      expect(preset.state.compilerOptions).toBeTruthy();
    }
  });

  it("every preset's language is a registered framework (manifest authority)", () => {
    for (const preset of presets) {
      expect(
        frameworkById(preset.language),
        `${preset.name}: language '${preset.language}' not in the manifest`,
      ).toBeTruthy();
    }
  });

  it("every preset's mainFile exists in files and matches the language carrier extension", () => {
    for (const preset of presets) {
      expect(
        preset.state.files[preset.mainFile],
        `${preset.name}: mainFile '${preset.mainFile}' not in files`,
      ).toBeTruthy();
      const framework = frameworkById(preset.language)!;
      const matchesCarrier = framework.carrierExtensions.some((ext) =>
        preset.mainFile.endsWith(ext),
      );
      expect(matchesCarrier, `${preset.name}: mainFile does not match a carrier ext`).toBe(true);
    }
  });

  it("the serialized state language matches the preset language", () => {
    for (const preset of presets) {
      expect(preset.state.language).toBe(preset.language);
    }
  });

  it("covers every registered framework with at least one preset", () => {
    const covered = new Set(presets.map((p) => p.language));
    for (const framework of FRAMEWORKS) {
      expect(
        covered.has(framework.frameworkId),
        `no preset for framework '${framework.frameworkId}'`,
      ).toBe(true);
    }
  });

  it("ships multiple Svelte presets exercising landed runes/snippets/props/modules", () => {
    const svelte = presets.filter((p) => p.language === "svelte");
    expect(svelte.length).toBeGreaterThanOrEqual(5);
    // A .svelte.ts adapter-module preset exists.
    const hasAdapterModule = svelte.some((p) =>
      Object.keys(p.state.files).some((f) => f.endsWith(".svelte.ts")),
    );
    expect(hasAdapterModule).toBe(true);
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
