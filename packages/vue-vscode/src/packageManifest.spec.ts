import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const srcDir = fileURLToPath(new URL(".", import.meta.url));
const extensionDir = path.resolve(srcDir, "..");
const playgroundDir = path.resolve(extensionDir, "..", "playground");

interface LanguageDefaults {
  "editor.formatOnType"?: boolean;
}

interface ConfigurationDefaults {
  "emmet.includeLanguages"?: Record<string, string>;
  "[vue]"?: LanguageDefaults;
  "[svelte]"?: LanguageDefaults;
}

interface ExtensionManifest {
  icon?: string;
  contributes?: {
    configurationDefaults?: ConfigurationDefaults;
  };
}

function readManifest(): ExtensionManifest {
  return JSON.parse(
    readFileSync(path.join(extensionDir, "package.json"), "utf8"),
  ) as ExtensionManifest;
}

describe("extension package manifest", () => {
  it("uses the playground logo as the extension icon", () => {
    const packageJson = readManifest();

    expect(packageJson.icon).toBe("logo.png");

    const extensionLogoPath = path.join(extensionDir, packageJson.icon!);
    expect(existsSync(extensionLogoPath)).toBe(true);
  });

  // VS Code ONLY honors `contributes.configurationDefaults`; a top-level
  // `configurationDefaults` key is silently ignored. Read from `contributes`
  // ONLY (no `?? packageJson.configurationDefaults` fallback) so this fails
  // against a top-level placement and passes once the block is moved inside
  // `contributes`.
  it("enables editor.formatOnType for [vue] and [svelte] under contributes", () => {
    const packageJson = readManifest();
    const defaults = packageJson.contributes?.configurationDefaults;

    expect(
      defaults,
      "configurationDefaults must live inside `contributes` (VS Code ignores the top-level key)",
    ).to.not.equal(undefined);

    expect(
      defaults!["[vue]"]?.["editor.formatOnType"],
      "[vue] must default editor.formatOnType to true so the on-type tag auto-close handler fires",
    ).toBe(true);
    expect(
      defaults!["[svelte]"]?.["editor.formatOnType"],
      "[svelte] must default editor.formatOnType to true for tag auto-close parity",
    ).toBe(true);
  });

  // The long-dead `emmet.includeLanguages` mapping must survive the move into
  // `contributes` so emmet expansion keeps working inside `.vue` files.
  it("keeps the emmet vue->html include-languages mapping under contributes", () => {
    const packageJson = readManifest();
    const defaults = packageJson.contributes?.configurationDefaults;

    expect(defaults?.["emmet.includeLanguages"]?.["vue"]).toBe("html");
  });
});
