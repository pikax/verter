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
  configuration?: string;
}

interface ContributedGrammar {
  language?: string;
  scopeName: string;
  path: string;
  embeddedLanguages?: Record<string, string>;
}

interface ExtensionManifest {
  activationEvents?: string[];
  contributes?: {
    languages?: ContributedLanguage[];
    grammars?: ContributedGrammar[];
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

  it("registers a TextMate grammar for svelte (source.svelte) with an on-disk grammar file", () => {
    const grammars = pkg.contributes?.grammars ?? [];
    const svelteGrammar = grammars.find((g) => g.language === "svelte");
    expect(svelteGrammar, "svelte must contribute a TextMate grammar").toBeDefined();
    expect(svelteGrammar!.scopeName).toBe("source.svelte");
    const grammarPath = path.join(extensionDir, svelteGrammar!.path);
    const grammarJson = JSON.parse(readFileSync(grammarPath, "utf8")) as {
      scopeName?: string;
      patterns?: unknown[];
    };
    expect(grammarJson.scopeName).toBe("source.svelte");
    expect(Array.isArray(grammarJson.patterns)).toBe(true);
    expect(grammarJson.patterns!.length).toBeGreaterThan(0);
  });

  it("maps the svelte grammar's embedded languages for TS/JS scripts and CSS/SCSS/LESS styles", () => {
    const grammars = pkg.contributes?.grammars ?? [];
    const svelteGrammar = grammars.find((g) => g.language === "svelte");
    expect(svelteGrammar).toBeDefined();
    const embedded = svelteGrammar!.embeddedLanguages ?? {};
    expect(embedded["source.ts"]).toBe("typescript");
    expect(embedded["source.js"]).toBe("javascript");
    expect(embedded["source.css"]).toBe("css");
    expect(embedded["source.css.scss"]).toBe("scss");
    expect(embedded["source.css.less"]).toBe("less");
  });

  it("wires a language configuration (comments/brackets/auto-closing) onto the svelte language", () => {
    const svelteLang = contributedLanguages.find((l) => l.id === "svelte");
    expect(svelteLang?.configuration, "svelte must declare a language configuration").toBeDefined();
    const configPath = path.join(extensionDir, svelteLang!.configuration!);
    const config = JSON.parse(readFileSync(configPath, "utf8")) as {
      comments?: { blockComment?: string[] };
      brackets?: unknown[];
      autoClosingPairs?: unknown[];
    };
    expect(config.comments?.blockComment).toEqual(["<!--", "-->"]);
    expect(config.brackets!.length).toBeGreaterThan(0);
    expect(config.autoClosingPairs!.length).toBeGreaterThan(0);
  });
});
