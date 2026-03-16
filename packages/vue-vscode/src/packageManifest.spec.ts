import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const srcDir = fileURLToPath(new URL(".", import.meta.url));
const extensionDir = path.resolve(srcDir, "..");
const playgroundDir = path.resolve(extensionDir, "..", "playground");

describe("extension package manifest", () => {
  it("uses the playground logo as the extension icon", () => {
    const packageJson = JSON.parse(
      readFileSync(path.join(extensionDir, "package.json"), "utf8"),
    ) as { icon?: string };

    expect(packageJson.icon).toBe("logo.svg");

    const extensionLogoPath = path.join(extensionDir, packageJson.icon!);
    const playgroundLogoPath = path.join(playgroundDir, "public", "logo.svg");

    expect(existsSync(extensionLogoPath)).toBe(true);
    expect(readFileSync(extensionLogoPath, "utf8")).toBe(readFileSync(playgroundLogoPath, "utf8"));
  });
});
