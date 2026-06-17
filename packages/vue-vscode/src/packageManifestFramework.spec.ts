import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { CLIENT_ACTIVATION_LANGUAGE_IDS, CLIENT_FRAMEWORKS } from "@verter/language-shared";

const srcDir = fileURLToPath(new URL(".", import.meta.url));
const extensionDir = path.resolve(srcDir, "..");

interface ContributedLanguage {
  id: string;
  extensions?: string[];
}

interface ExtensionManifest {
  activationEvents?: string[];
  contributes?: {
    languages?: ContributedLanguage[];
  };
}

function readExtensionManifest(): ExtensionManifest {
  return JSON.parse(
    readFileSync(path.join(extensionDir, "package.json"), "utf8"),
  ) as ExtensionManifest;
}

describe("extension package manifest framework wiring (manifest-driven)", () => {
  const pkg = readExtensionManifest();
  const activationEvents = pkg.activationEvents ?? [];
  const contributedLanguages = pkg.contributes?.languages ?? [];

  it("declares an onLanguage activation event for every manifest activation language id", () => {
    for (const languageId of CLIENT_ACTIVATION_LANGUAGE_IDS) {
      expect(activationEvents).toContain(`onLanguage:${languageId}`);
    }
  });

  it("registers a contributes.languages entry for every framework client language id", () => {
    const registeredIds = new Set(contributedLanguages.map((l) => l.id));
    for (const fw of CLIENT_FRAMEWORKS) {
      for (const languageId of fw.clientLanguageIds) {
        expect(registeredIds.has(languageId)).toBe(true);
      }
    }
  });

  it("registers each framework's carrier extension on its contributed language", () => {
    for (const fw of CLIENT_FRAMEWORKS) {
      const lang = contributedLanguages.find((l) => fw.clientLanguageIds.includes(l.id));
      expect(lang, `no contributed language for framework ${fw.frameworkId}`).toBeDefined();
      for (const ext of fw.carrierExtensions) {
        expect(lang!.extensions ?? []).toContain(ext);
      }
    }
  });

  it("makes BOTH vue and svelte first-class — svelte is no longer opt-in-gated", () => {
    const frameworkIds = CLIENT_FRAMEWORKS.map((f) => f.frameworkId);
    expect(frameworkIds).toContain("vue");
    expect(frameworkIds).toContain("svelte");
    // Both have an activation event and a registered language.
    expect(activationEvents).toContain("onLanguage:vue");
    expect(activationEvents).toContain("onLanguage:svelte");
    const ids = contributedLanguages.map((l) => l.id);
    expect(ids).toContain("vue");
    expect(ids).toContain("svelte");
  });

  it("no longer carries the retired verter.frameworks opt-in config", () => {
    const raw = readFileSync(path.join(extensionDir, "package.json"), "utf8");
    expect(raw).not.toContain("verter.frameworks");
  });

  it("does not register a TextMate grammar for svelte (relies on the user's Svelte extension)", () => {
    const raw = readFileSync(path.join(extensionDir, "package.json"), "utf8");
    expect(raw).not.toContain("source.svelte");
  });
});
